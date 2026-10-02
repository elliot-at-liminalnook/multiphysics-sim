//! Shared rendered-control catalogue and system_ui activation.
use super::actions::{CadAction, Cx, handle};
use super::snapshot::{Parts, state_json};
use crate::app::actions::Call;
use serde_json::{Map, Value, json};
use sim_api::Outcome;
/// CAD mode's `system_ui` controls: the panel's own list (`panel::controls`),
/// so a control's label, enabled state and action are the button's.
fn controls(cx: &Cx) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let mut out: Vec<_> = super::panel::controls(cx.doc, &cx.shared.items())
        .into_iter()
        .map(|c| (c.id, c.label, c.action, c.ready))
        .collect();
    // cad-views-export: cad:display:*, cad:section:*, cad:view:*, cad:file:*.
    out.extend(super::display::controls(cx));
    out.extend(super::views::controls(cx));
    out.extend(super::files::controls(cx));
    // cad-physical-inspect: cad:robot:*, cad:materials:*, cad:inspect:*, cad:results:*.
    out.extend(super::robot::controls(cx));
    out.extend(super::materials::controls(cx));
    out.extend(super::inspector::physical_controls(cx));
    out.extend(super::results::controls(cx));
    // cad-print: cad:print:*.
    out.extend(super::print::controls(cx));
    // cad-organize: cad:tree:*, cad:threads:*, cad:references:*.
    out.extend(super::tree::controls(cx));
    out.extend(super::threads::controls(cx));
    out.extend(super::references::controls(cx));
    out.extend(super::components::controls(cx));
    out.extend(super::composition::controls_of(cx.doc, cx.composition));
    out.extend(super::experiments::controls(cx));
    out.extend(super::experiment_review::controls(cx));
    out.extend(super::motion::controls(cx));
    out
}

/// `system_ui`: the controls, or one activated through this handler (the
/// same action a click writes). Ids are stable names, so `ui_revision` is
/// reported but not required.
pub(super) fn system_ui(call: &mut Call, cx: &mut Cx, args: &Map<String, Value>) -> Outcome {
    let action = args.get("action").cloned().unwrap_or(Value::Null);
    match action["operation"].as_str() {
        Some("controls") => {
            let items: Vec<Value> = controls(cx)
                .into_iter()
                .map(|(id, label, action, ready)| json!({"id": id, "label": label, "enabled": ready.is_ok(), "disabled_reason": ready.err(), "action": super::rest_form::rest_form(&action)}))
                .collect();
            Outcome::Done(Ok(json!({"ui_revision": cx.doc.revision, "ready": true, "controls": items, "state": state_json(cx.doc, &cx.shared.items(), cx.meshes.as_deref(), Some(&*cx.plane), Parts::of(cx.display.as_deref(), cx.views.as_deref(), cx.files.as_deref()).authoring(cx.components, cx.composition).experiments(cx.experiments, cx.review, cx.motion))})))
        }
        Some("activate") => {
            let Some(id) = action["id"].as_str() else { return Outcome::Done(Err("system_ui activate needs an id; request controls".into())) };
            let found = controls(cx).into_iter().find(|(i, ..)| i == id);
            match found {
                None => Outcome::Done(Err(format!("unknown control {id}; request controls"))),
                Some((id, _, _, Err(why))) => Outcome::Done(Err(format!("{id} is disabled: {why}"))),
                Some((_, _, action, Ok(()))) => handle(&action, call, cx),
            }
        }
        _ => Outcome::Done(Err("system_ui in CAD mode: operation controls, or activate with a control id (cad:* or mode:*)".into())),
    }
}
