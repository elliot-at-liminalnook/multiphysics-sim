//! The one REST dispatch in front of every mode's action type (one server for
//! the window, `rest::bind`; one poll, `actions::serve`): `viewer_mode` and
//! `system_ui` `mode:*` go to the mode switch's actions in every mode; a
//! command of another mode is refused naming the active mode; everything
//! else goes, unchanged, to the action type the registry names for the
//! active mode (`actions::feature_for`). `system_ui` `picker:*` (the
//! document picker's entries, path field and Close, `app::picker`) goes to
//! the mode switch's actions too. [`annotate`] adds what every mode adds to
//! its answer, including the open picker's controls. The headless server (`window` false) has no window
//! to switch and no camera to move: `viewer_mode`, `mode:*` and every
//! command of the shared camera are refused there, and its `system_ui`
//! controls do not list `camera:*` ([`annotate_for`]).
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

/// Why the headless server refuses the shared camera's commands.
const NO_CAMERA: &str = "the headless server has no camera to move; it renders through inspect's camera/fit commands";

/// `system_ui` activating a `mode:*` control (the switcher's) or a
/// `picker:*` control (the document picker's): the window's, in every mode.
fn mode_control(command: &sim_api::Command) -> bool {
    let action = &command.args["action"];
    command.command == "system_ui" && action["operation"] == "activate" && action["id"].as_str().is_some_and(|id| id.starts_with("mode:") || id.starts_with("picker:"))
}

/// The one REST dispatch: the action type that takes `command` in `mode`,
/// or the refusal of a command of another mode, naming it, the modes it
/// needs and the active mode. `window` is false for the headless server,
/// which has no window to switch and no camera to move (`camera_*` and
/// `system_ui` `camera:*` are refused there, naming inspect's `camera` and
/// `fit`).
pub(crate) fn route(mode: ViewerMode, window: bool, command: &sim_api::Command) -> Result<&'static Feature, String> {
    let name = command.command.as_str();
    if name == "system_ui" && command.args["action"]["operation"] == "activate"
        && command.args["action"]["id"].as_str().is_some_and(|id| id.starts_with("study:"))
    {
        return if window { Ok(actions::named("measured_study")) }
            else { Err("measured studies require the native window".into()) };
    }
    if name == "viewer_mode" || mode_control(command) {
        return if window { Ok(actions::named("window")) } else { Err(format!("{name}: this headless server has no window to switch (inspect mode only)")) };
    }
    // `system_ui` `camera:*`: the shared camera's controls, in every orbit mode.
    if crate::camera::is_camera_control(command) && crate::camera::ORBIT_MODES.contains(&mode) {
        return if window { Ok(actions::named("camera")) } else { Err(format!("{name}: {NO_CAMERA}")) };
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
        _ => {
            let feature = actions::feature_for(mode, name).unwrap_or_else(|| actions::fallback(mode, name));
            // Any command the shared camera registers (`camera_*`).
            if !window && feature.name == "camera" {
                return Err(format!("`{name}`: {NO_CAMERA}"));
            }
            Ok(feature)
        }
    }
}

/// What every mode adds to its handler's answer in the window
/// ([`annotate_with`] with a window): `picker` is the open document
/// picker's controls (`app::picker::Picker::controls`; empty when closed).
pub(crate) fn annotate(mode: ViewerMode, picker: &[Value], command: &sim_api::Command, outcome: Outcome) -> Outcome {
    annotate_with(mode, true, picker, command, outcome)
}

/// [`annotate_with`] with no picker (the headless server has none).
pub(crate) fn annotate_for(mode: ViewerMode, window: bool, command: &sim_api::Command, outcome: Outcome) -> Outcome {
    annotate_with(mode, window, &[], command, outcome)
}

/// What every mode adds to its handler's answer: the active mode on
/// `state`, and the mode switcher's controls at the end of `system_ui`
/// controls (in build, lessons and robot mode, after the mode's own),
/// after the shared camera's in every orbit mode when there is a window
/// (`window` false is the headless server, which refuses them), then the
/// open document picker's (`picker`, in every mode).
pub(crate) fn annotate_with(mode: ViewerMode, window: bool, picker: &[Value], command: &sim_api::Command, mut outcome: Outcome) -> Outcome {
    if let Outcome::Done(Ok(value)) = &mut outcome {
        match command.command.as_str() {
            "state" if value.is_object() => value["viewer_mode"] = json!(mode.name()),
            "system_ui" if command.args["action"]["operation"] == "controls" => {
                if let Some(controls) = value.get_mut("controls").and_then(Value::as_array_mut) {
                    // The shared camera's controls in every orbit mode, then the switcher's
                    // (which the switcher-only modes' own list already ends with).
                    if window && crate::camera::ORBIT_MODES.contains(&mode) {
                        controls.extend(crate::camera::controls());
                    }
                    if !actions::SWITCHER_ONLY.contains(&mode) {
                        controls.extend(mode_controls(mode));
                    }
                    controls.extend(picker.iter().cloned());
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
