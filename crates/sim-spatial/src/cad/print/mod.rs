//! CAD mode's Print menu (cad-print; native-viewer.md "CAD print"), in
//! process: the checks and jobs are `sim_cad::printing` and
//! `sim_cad::print` (RoboCAD's printing.py and print_*.py, ported), the
//! stress check and planner `sim_runtime::print_tools` (`sim-print`'s).
//!
//! - [`checks`]: "Wall thickness check…" (Ctrl+W) and "Validate for
//!   printing" (Ctrl+Shift+V): reads on a job, RoboCAD's status text; the
//!   thin points are drawn by [`thin_overlay`], display only, cached by
//!   (node, revision, threshold).
//! - Overhang shading ("Toggle overhang shading") is a display toggle
//!   (`display::DisplaySetting::Overhangs`): the shown mesh's triangles past
//!   RoboCAD's 45° (`display::section::overhangs`), display only.
//! - [`edits`]: "Fastener hole…" (Ctrl+H; its face clicks in
//!   [`fastener_tool`], faces only through `CadMeshes::face_at`) and
//!   "Clearance offset…" (Ctrl+Shift+C), with RoboCAD's remembered values;
//!   each commit is one edit through `actions::edit_at`.
//! - [`studies`]: split for printing, check strength, plan, whole or split,
//!   assembly guide and test coupons: catalogue forms whose printers and
//!   filaments come from the print registry (read on a job once per
//!   generation) and whose study and split groups come from the shown
//!   archive. Each start is one job.
//! - [`jobs_tracker`]: [`PrintJobTracker`], the jobs in flight: "kind:
//!   message (n %)" on the status line, failures named, a job's staged edit
//!   published as one undo step, and leaving CAD mode refused while one
//!   runs (`CadDocument::switch_blockers`).
//! - [`jobs_panel`]: the Print jobs section of the right dock: the last
//!   eight jobs, Cancel after RoboCAD's "Cancel the running jobs?".
//! - [`overlay`]: the print study's results (each node's "print" results
//!   block) coloured through the shared stress rule (`results::overlay`,
//!   `sim_domain_robot::stress_results`), with the staleness label.
//!
//! Every intent is a [`CadAction`]: the catalogue's (`CadInvoke`,
//! `CadRun`, forms) for the tools and studies, [`PrintArgs`] (`cad_print`)
//! for the jobs panel, cancel, the fastener tool's picks and clearing the
//! check marks.
mod checks;
mod edits;
mod fastener_tool;
pub(in crate::cad) mod jobs_panel;
pub(crate) mod jobs_tracker;
pub(crate) mod overlay;
mod studies;
mod thin_overlay;
#[cfg(test)]
mod checks_tests;
#[cfg(test)]
mod edits_tests;
#[cfg(test)]
mod studies_tests;
#[cfg(test)]
mod tracker_tests;

pub(crate) use jobs_tracker::PrintJobTracker;

use super::actions::{CadAction, Cx};
use super::document::CadDocument;
use super::ops::{Built, Env, OpEntry, Resolved};
use crate::app::actions::{Call, Spec};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use crate::cad::types::SelectionItem;

/// cad-print's state on the document (reset with it, so per connection
/// generation and document).
#[derive(Default)]
pub struct PrintState {
    /// Document-local wall check and validation reads and thin points.
    pub(crate) checks: checks::ChecksState,
    /// The fastener tool's document-local clicks and picks.
    pub(crate) edits: edits::EditsState,
    /// The printing registry (per generation).
    pub(crate) studies: studies::StudiesState,
    /// The print jobs this document ran.
    pub(crate) jobs: PrintJobTracker,
}
impl PrintState {
    /// The fastener tool's picks start over (a tool started, cancelled or ended).
    pub(crate) fn reset_picks(&mut self) {
        self.edits.reset_picks();
    }
}

/// Which Print menu entry a catalogue run is (`ops::Shape::Print`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrintCall {
    /// `print.wall_check`: `sim_cad::printing::wall_thickness` per node on a job.
    WallCheck,
    /// `print.validate`: `sim_cad::printing::validate` per visible body on a job.
    Validate,
    /// `tool.fastener`: one `fastener_hole` per face click.
    Fastener,
    /// `tool.clearance`: one `clearance` per node with selected faces.
    Clearance,
    /// `print.split`: a split job (`sim_cad::print::jobs::split`).
    Split,
    /// `print.strength`: an analyze job with the document's study.
    Strength,
    /// `print.plan`: a plan job with the document's study.
    Plan,
    /// `print.strength_split`: a whole-or-split job.
    StrengthSplit,
    /// `print.assembly`: an assembly guide job.
    Assembly,
    /// `print.coupons`: a test coupon job.
    Coupons,
}

/// What a print run sends (`ops::Built::Print`), built from the catalogue
/// entry, the resolved selection and the values by [`build`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Plan {
    Check(checks::CheckPlan),
    Edit(edits::EditPlan),
    Study(studies::StudyPlan),
}

/// `ops::args::build` for `Shape::Print`.
pub(in crate::cad) fn build_plan(call: PrintCall, entry: &OpEntry, r: &Resolved, values: &Map<String, Value>, doc: &CadDocument, env: &Env) -> Result<Built, String> {
    let plan = match call {
        PrintCall::WallCheck | PrintCall::Validate => Plan::Check(checks::build(call, entry, r, values, doc, env)?),
        PrintCall::Fastener | PrintCall::Clearance => Plan::Edit(edits::build(call, entry, r, values, doc, env)?),
        PrintCall::Split | PrintCall::Strength | PrintCall::Plan | PrintCall::StrengthSplit | PrintCall::Assembly | PrintCall::Coupons => Plan::Study(studies::build(call, entry, r, values, doc, env)?),
    };
    Ok(Built::Print(plan))
}

/// `ops::start` for `Built::Print`: the check's job, the edit or the
/// study's start (each refused by name with nothing sent when it cannot go).
pub(in crate::cad) fn send(doc: &mut CadDocument, call: &mut Call, plan: Plan, settings: &mut crate::app::settings::SettingsOwner) -> Outcome {
    match plan {
        Plan::Check(p) => checks::send(doc, call, p, settings),
        Plan::Edit(p) => edits::send(doc, call, p, settings),
        Plan::Study(p) => studies::send(doc, call, p),
    }
}

/// A `FieldKind::Pick` list of cad-print (`ops::robot_form::picks`'s
/// fallback): "printers" (the registry's, "id (x × y × z mm)"),
/// "printer_ids" and "filaments" (ids as RoboCAD's coupon dialog lists
/// them); empty while the registry is not read, or for another source.
pub(in crate::cad) fn picks(source: &str, doc: &CadDocument) -> Vec<(String, String)> {
    studies::picks(source, doc)
}

/// A newly opened print form's presets (`ops::robot_form::seed`'s
/// fallback): the remembered values.
pub(in crate::cad) fn seed(entry: &OpEntry, doc: &CadDocument, env: &Env, texts: &mut [String]) {
    match entry.id {
        "tool.fastener" | "tool.clearance" => edits::seed(entry, doc, env, texts),
        "print.wall_check" => checks::seed(entry, env, texts),
        _ => studies::seed(entry, doc, env, texts),
    }
}

/// RoboCAD's refusal before a print dialog opens, or why its lists are
/// not known yet (`ops::invoke`'s form flow; `selection`: the shared
/// selection's CAD items).
pub(in crate::cad) fn precheck(entry: &OpEntry, doc: &CadDocument, selection: &[SelectionItem]) -> Option<String> {
    studies::precheck(entry, doc, selection)
}

/// What `cad_print` does.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PrintOp {
    /// cad-print's state (as `cad_state.print` shows it).
    #[default]
    State,
    /// The Print jobs section shown (`open: true`), hidden (`false`) or toggled.
    Jobs,
    /// Cancel the running print jobs (or `job`): without `confirm` the
    /// confirmation "Cancel the running jobs?" opens; `confirm: true`
    /// cancels each running job; `false` closes it.
    Cancel,
    /// One job (`job`) as listed, with its result once done.
    Job,
    /// The fastener tool's pick: `item` (a face) at `picked_at`.
    Pick,
    /// Clear the wall check's points (RoboCAD's `temp_shapes`).
    Clear,
}

/// `cad_print`'s arguments.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct PrintArgs {
    #[serde(default)]
    pub op: PrintOp,
    /// Shown or hidden (jobs; absent toggles).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
    /// One job's id (cancel; absent: every running job).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    /// The confirmation's answer (cancel; absent asks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<bool>,
    /// The picked face (pick).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<SelectionItem>,
    /// The shown revision the pick was made at (pick; required from REST).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picked_at: Option<u64>,
}
impl PrintArgs {
    pub(crate) fn of(op: PrintOp) -> CadAction {
        CadAction::CadPrint(PrintArgs { op, ..PrintArgs::default() })
    }
}

/// The action a RoboCAD command id stands for when it is not a catalogue
/// operation (`surfaces::registry`'s `Do::Print`): overhang shading (a
/// display toggle) and the print jobs panel (shown, as RoboCAD's dialog opens).
pub(crate) fn command_action(id: &str) -> Option<CadAction> {
    use super::display::{DisplayArgs, DisplaySetting};
    match id {
        "print.overhangs" => Some(CadAction::CadDisplay(DisplayArgs { toggle: Some(DisplaySetting::Overhangs), ..DisplayArgs::default() })),
        "print.jobs" => Some(CadAction::CadPrint(PrintArgs { op: PrintOp::Jobs, open: Some(true), ..PrintArgs::default() })),
        _ => None,
    }
}

/// `CadPrint`, from any entry point.
pub(in crate::cad) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    let CadAction::CadPrint(args) = action else { return Outcome::Done(Err("not a print action".into())) };
    let wrong = |what: &str, op: &str| Outcome::Done(Err(format!("{what} belongs to op {op}")));
    if args.open.is_some() && args.op != PrintOp::Jobs {
        return wrong("open", "jobs");
    }
    if args.job.is_some() && !matches!(args.op, PrintOp::Cancel | PrintOp::Job) {
        return wrong("job", "cancel or job");
    }
    if args.confirm.is_some() && args.op != PrintOp::Cancel {
        return wrong("confirm", "cancel");
    }
    if (args.item.is_some() || args.picked_at.is_some()) && args.op != PrintOp::Pick {
        return wrong("item and picked_at", "pick");
    }
    match args.op {
        PrintOp::State => Outcome::Done(Ok(state_json(cx.doc, Some(&cx.settings.cad)))),
        PrintOp::Jobs => Outcome::Done(jobs_panel::show(cx.doc, args.open)),
        PrintOp::Cancel => jobs_tracker::cancel(cx.doc, call, args.job.as_deref(), args.confirm),
        PrintOp::Job => Outcome::Done(args.job.as_deref().ok_or_else(|| "op job needs job (a print job id)".to_string()).and_then(|id| jobs_tracker::job_json(cx.doc, id))),
        PrintOp::Pick => fastener_tool::pick(args, call, cx),
        PrintOp::Clear => Outcome::Done(Ok(checks::clear(cx.doc, Some(&cx.settings.cad)))),
    }
}

/// `cad_state.print`.
pub(in crate::cad) fn state_json(doc: &CadDocument, defaults: Option<&crate::app::settings::CadDefaults>) -> Value {
    json!({
        "checks": checks::state_json(doc, defaults),
        "edits": edits::state_json(doc, defaults),
        "studies": studies::state_json(doc),
        "jobs": jobs_tracker::state_json(doc),
        "panel": jobs_panel::state_json(doc),
        "overlay": overlay::state_json(doc),
    })
}

/// cad-print's `system_ui` controls (`cad:print:*`): (id, label, action, ready).
pub(in crate::cad) fn controls(cx: &Cx) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let mut out = jobs_panel::controls(cx.doc);
    out.extend(checks::controls(cx.doc));
    out
}

/// cad-print's REST command (`cad_print`; the tools and studies are catalogue entries).
pub(in crate::cad) fn specs() -> Vec<Spec> {
    vec![crate::app::actions::spec(
        "cad_print",
        super::actions::CAD,
        json!({"op": "jobs", "open": true}),
        "CAD mode: the print jobs and the print tools' state (cad_state.print). op: state, jobs (open true | false, absent toggles the Print jobs section: the last eight print jobs with kind, state, progress and message), job (one job by id, with its result once done), cancel (without confirm the confirmation \"Cancel the running jobs?\" opens; confirm true cancels each running job, or only job; confirm false closes it), pick (the fastener tool's face pick: item [node, \"face\", index] at picked_at, the revision it was read at), clear (the wall check's points). The tools and studies are catalogue operations: cad_invoke or cad_run tool.fastener, tool.clearance, print.wall_check, print.validate, print.split, print.strength, print.plan, print.strength_split, print.assembly, print.coupons, ops.print_split. A study's start answers the started job ({id, kind, state}); read it with op job until its state is done, failed or cancelled. Jobs run in this process on the shown snapshot; a job that changes the document (split, strength, plan, an assembly's exploded view) publishes one undo step when it ends, refused by name if the document moved meanwhile. Leaving CAD mode is refused while one runs. system_ui lists cad:print:*.",
    )]
}

/// CadCorePlugin's windowless print systems: the checks' and studies'
/// reads, and the job poller (JobResults, after `sync::receive`).
pub(crate) struct CoreParts;
impl Plugin for CoreParts {
    fn build(&self, app: &mut App) {
        checks::build_core(app);
        studies::build_core(app);
        jobs_tracker::build_core(app);
    }
}

/// CadPlugin: the thin points, the fastener tool's clicks and the jobs section.
pub(in crate::cad) fn build(app: &mut App) {
    thin_overlay::build(app);
    fastener_tool::build(app);
    jobs_panel::build(app);
}
