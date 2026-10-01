//! `CadInvoke`: what RoboCAD's command does when its menu entry, button or
//! key fires (its handler in ui/app.py, or `set_tool`): run at once, open
//! the parameter form, or start the operation's interaction (a pick, place,
//! sketch, extrude, plane or robot click tool). Split from `ops` to keep it
//! under the size guard.
use super::form::open_form_with;
use super::*;
use crate::cad::document::CadTool;

/// `CadInvoke` of catalogue entry `id` (a RoboCAD command that is not in
/// the catalogue goes to the command surfaces).
pub(super) fn invoke(id: &str, call: &mut Call, cx: &mut Cx) -> Outcome {
    let Some(entry) = entry(id) else { return crate::cad::surfaces::invoke_command(id, call, cx) };
    match entry.flow {
        Flow::Immediate | Flow::AtCursorSnap => run(entry, &Map::new(), None, None, call, cx),
        // Viewer state: no RoboCAD call, nothing to refuse.
        Flow::View(act) => Outcome::Done(Ok(crate::cad::sketch::plane::view_act(&mut *cx.doc, &mut *cx.plane, act))),
        Flow::Sketch(_) | Flow::Extrude { .. } | Flow::PlanePick(_) => {
            end_tool(call, cx);
            let selection = cx.shared.items();
            let mode_before = cx.doc.select_mode;
            let (doc, env) = cx.split(&selection);
            clear_interactions(doc, Some(entry.id));
            // A sketch or extrude tool's Tab fields (RoboCAD's numeric bar);
            // a plane tool has none (RoboCAD's `PlaneTool` only picks).
            let answer = if entry.params.is_empty() || matches!(entry.flow, Flow::PlanePick(_)) {
                doc.ops.form = None;
                json!({"active": entry.id})
            } else {
                open_form_with(doc, entry, Some(&env))
            };
            // Each interaction's own start, after its form opened (it may set
            // drafts: the polygon's remembered sides, the text field's focus):
            // its state, the selection mode a plane tool sets, an extrude's source.
            let started = match entry.flow {
                Flow::Sketch(shape) => crate::cad::sketch::interact::begin(doc, shape),
                Flow::Extrude { revolve } => crate::cad::sketch::extrude::begin(doc, &env, revolve),
                Flow::PlanePick(mode) => crate::cad::sketch::plane::begin(doc, mode),
                _ => Ok(()),
            };
            let outcome = match started {
                Err(e) => {
                    form_cancel(doc);
                    // The reason, not form_cancel's "Cancelled …", stays on the status line.
                    doc.show(Err(e.clone()));
                    Outcome::Done(Err(e))
                }
                Ok(()) => {
                    doc.show(Ok(entry.hint.to_string()));
                    Outcome::Done(Ok(answer))
                }
            };
            // A plane tool sets the selection mode (`sketch::plane::begin`): pushed with the items.
            if cx.doc.select_mode != mode_before {
                crate::cad::selection::publish(cx.doc, cx.shared.view());
            }
            outcome
        }
        Flow::Form => {
            // RoboCAD's handlers check the selection before their dialog opens.
            let selection = cx.shared.items();
            if let Err(e) = resolve::resolve(entry, cx.doc, &cx.env(&selection), None) {
                return Outcome::Done(Err(e));
            }
            // RoboCAD's checks before a robot dialog opens ("add a motor and
            // a joint first"), and that the description it is filled from is
            // the shown revision's (the form's `began`, which its OK checks).
            if let Some(why) = robot_form::precheck(entry, cx.doc, &selection) {
                return Outcome::Done(Err(why));
            }
            // The dialog replaces an active pick or place tool's form, so
            // that tool ends as its Cancel ends it (`form_cancel`, less the
            // status line): a tool left active without its form would keep
            // taking clicks with no form to run them.
            let (doc, env) = cx.split(&selection);
            clear_interactions(doc, None);
            Outcome::Done(Ok(open_form_with(doc, entry, Some(&env))))
        }
        Flow::PickThenForm(mode) => {
            end_tool(call, cx);
            let doc = &mut *cx.doc;
            // As RoboCAD's EdgeTool/ShellTool.activate (ui/tools.py:946, :999):
            // the mode is set directly and the selection is kept.
            if doc.select_mode != mode {
                doc.select_mode = mode;
                crate::cad::selection::publish(doc, cx.shared.view());
            }
            clear_interactions(doc, Some(entry.id));
            let answer = open_form_with(doc, entry, None);
            doc.show(Ok(entry.hint.to_string()));
            Outcome::Done(Ok(answer))
        }
        Flow::Place(_) => {
            end_tool(call, cx);
            let doc = &mut *cx.doc;
            clear_interactions(doc, Some(entry.id));
            let answer = open_form_with(doc, entry, None);
            doc.show(Ok(entry.hint.to_string()));
            Outcome::Done(Ok(answer))
        }
        Flow::RobotPick(tool) => {
            // RoboCAD's `set_tool(MotorTool(…))` / `set_tool(JointTool(…))`:
            // the tool sets its selection mode (face, or body for the joint
            // tool's first two clicks) and the picks start over.
            end_tool(call, cx);
            let selection = cx.shared.items();
            let mode_before = cx.doc.select_mode;
            let (doc, env) = cx.split(&selection);
            clear_interactions(doc, Some(entry.id));
            doc.select_mode = tool.mode();
            // The motor tool's form holds the Add motor dialog's values beside
            // the view while it picks; the joint tool has no fields.
            let answer = if entry.params.is_empty() {
                doc.ops.form = None;
                json!({"active": entry.id})
            } else {
                open_form_with(doc, entry, Some(&env))
            };
            doc.show(Ok(tool.status(entry).to_string()));
            if cx.doc.select_mode != mode_before {
                crate::cad::selection::publish(cx.doc, cx.shared.view());
            }
            Outcome::Done(Ok(answer))
        }
    }
}

/// End every catalogue interaction (`active`: the one starting now, if any):
/// a placement, a sketch shape, an extrude drag, plane picks and the robot
/// click tools' picks.
fn clear_interactions(doc: &mut CadDocument, active: Option<&'static str>) {
    doc.ops.active = active;
    doc.ops.place = None;
    doc.ops.sketch = None;
    doc.ops.extrude = None;
    doc.ops.plane_picks.clear();
    doc.robot.tools.reset_picks();
}

/// RoboCAD's `set_tool` replaces the active tool: a transform tool's live
/// work ends and Select becomes the tool before a pick or place operation starts.
fn end_tool(call: &mut Call, cx: &mut Cx) {
    if cx.doc.tool != CadTool::Select {
        let _ = crate::cad::transform::handle(&CadAction::CadTool { tool: CadTool::Select }, call, cx);
    }
}
