//! The one REST dispatch in front of every mode's action type (one server for
//! the window, `rest::bind`; one poll, `actions::serve`): `viewer_mode` and
//! `system_ui` `mode:*` go to the mode switch's actions in every mode; a
//! command of another mode is refused naming the active mode; everything
//! else goes, unchanged, to the action type the registry names for the
//! active mode (`actions::feature_for`). [`annotate`] adds what every mode
//! adds to its answer.
use super::actions::{self, Feature};
use super::ViewerMode;
use serde_json::{Value, json};
use sim_api::Outcome;

fn mode_list(modes: &[ViewerMode]) -> String {
    let names: Vec<&str> = modes.iter().map(|m| m.name()).collect();
    match names.as_slice() {
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
        [] => String::new(),
    }
}

/// `system_ui` activating a `mode:*` control: the switcher's, in every mode.
fn mode_control(command: &sim_api::Command) -> bool {
    let action = &command.args["action"];
    command.command == "system_ui" && action["operation"] == "activate" && action["id"].as_str().is_some_and(|id| id.starts_with("mode:"))
}

/// The one REST dispatch: the action type that takes `command` in `mode`,
/// or the refusal of a command of another mode, naming it, the modes it
/// needs and the active mode. `window` is false for the headless server,
/// which has no window to switch.
pub(crate) fn route(mode: ViewerMode, window: bool, command: &sim_api::Command) -> Result<&'static Feature, String> {
    let name = command.command.as_str();
    if name == "viewer_mode" || mode_control(command) {
        return if window { Ok(actions::named("window")) } else { Err(format!("{name}: this headless server has no window to switch (inspect mode only)")) };
    }
    match actions::command_modes(name) {
        Some(needs) if !needs.contains(&mode) => Err(format!(
            "`{name}` is a {} mode command; the active mode is {}. Switch first: viewer_mode {{\"mode\":\"{}\"}}.",
            mode_list(needs),
            mode.name(),
            needs[0].name()
        )),
        // Inspect has no builder or lesson: an unknown `system*`/`lesson_*` name
        // is answered as before (headless too).
        None if mode == ViewerMode::Inspect && name.starts_with("lesson_") => Err("no lessons are open in this window: switch with viewer_mode {\"mode\":\"lessons\",\"path\":\"DIR\"}".into()),
        None if mode == ViewerMode::Inspect && name.starts_with("system") => Err("start the viewer with --system FILE to edit systems".into()),
        _ => Ok(actions::feature_for(mode, name).unwrap_or_else(|| actions::fallback(mode, name))),
    }
}

/// What every mode adds to its handler's answer: the active mode on
/// `state`, and the mode switcher's controls at the end of `system_ui`
/// controls (in build, lessons and robot mode, after the mode's own).
pub(crate) fn annotate(mode: ViewerMode, command: &sim_api::Command, mut outcome: Outcome) -> Outcome {
    if let Outcome::Done(Ok(value)) = &mut outcome {
        match command.command.as_str() {
            "state" if value.is_object() => value["viewer_mode"] = json!(mode.name()),
            "system_ui" if command.args["action"]["operation"] == "controls" && !actions::SWITCHER_ONLY.contains(&mode) => {
                if let Some(controls) = value.get_mut("controls").and_then(Value::as_array_mut) {
                    controls.extend(mode_controls(mode));
                }
            }
            _ => {}
        }
    }
    outcome
}

/// The switcher's controls as `system_ui` lists them.
pub(crate) fn mode_controls(active: ViewerMode) -> Vec<Value> {
    ViewerMode::ALL
        .into_iter()
        .map(|m| {
            json!({"id": format!("mode:{}", m.name()), "label": format!("Mode: {}", m.label()), "enabled": m != active,
                "disabled_reason": (m == active).then(|| format!("{} mode is active", m.label())), "action": {"viewer_mode": {"mode": m}}})
        })
        .collect()
}
