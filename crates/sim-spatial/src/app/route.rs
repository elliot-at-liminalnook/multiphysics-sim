//! The one REST dispatch in front of every mode's handler (one server for
//! the window, `rest::bind`): `viewer_mode` and `system_ui` `mode:*` go to
//! the mode switch; a command of another mode is refused naming the active
//! mode; everything else goes, unchanged, to the active mode's own handler
//! (`rest::poll` for inspect/build/lessons, `robot::poll_rest`,
//! `place_view::poll_rest`), which calls [`route`] first and [`annotate`]
//! last.
use super::switch::{Documents, ModeSwitch, Switcher};
use super::ViewerMode;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};
use sim_api::Outcome;
use std::path::PathBuf;

/// What every mode's REST poll needs for the dispatch.
#[derive(SystemParam)]
pub(crate) struct Modes<'w> {
    pub mode: Res<'w, State<ViewerMode>>,
    pub switch: ResMut<'w, Switcher>,
    pub documents: Res<'w, Documents>,
}
impl Modes<'_> {
    pub(crate) fn active(&self) -> ViewerMode {
        *self.mode.get()
    }
    pub(crate) fn parts(&mut self) -> (&mut Switcher, &Documents) {
        (&mut *self.switch, &*self.documents)
    }
}

/// Commands every mode answers: `viewer_mode` and `screenshot`.
pub fn capabilities() -> Vec<Value> {
    use sim_api::capability as c;
    vec![
        c("viewer_mode", json!({"mode":"robot","preset":"robot-measured-400hz"}), "Switch this window's mode, or with no mode (args {}) report it: active, modes, pending (a document still loading), entering, message (the switcher's last line) and documents (what each mode reopens: inspect description/spatial, lessons dir and lesson, robot path or preset, place; library, models, presets). mode inspect | build | lessons | robot | place; optional path (inspect: a *.description.json with its *.spatial.json beside it; build: a *.system.json; lessons: a folder of <slug>/lesson.md; robot: a *.simrobot.json; place: a sim-place build directory) or preset (robot: an id listed by robot_presets). Without one, a mode reopens what it showed before in this window or at launch; inspect falls back to the example assembly. The same handler as the mode switcher (bottom right of the window) and system_ui mode:<mode> (every mode). Refused, naming the reason, with the current mode kept: no document for the target mode; leaving build or lessons (or replacing the builder with a new lesson's) while a text or discussion draft, placement drag, study, replay, Codex answer or open is in progress (the builder's system_open blockers) or a lesson draft or contact sheet is, and a new lesson replacing a build-mode builder with a live run; leaving robot mode while a recording is being written or a replay runs; another switch in progress; a path of the wrong kind; a document that fails to load (named). Documents load off the UI thread; poll the job. Build and lessons share the builder: switching between them keeps it and the lesson (the lesson screen is drawn over the builder). The builder stays in the window across every switch: leaving build/lessons pauses a live run and parks its scene; build with a path while it has another file open is refused (use system_open in build mode). Leaving lessons closes the lesson (its recordings and narration stop; lessons reopens it); leaving robot mode stops its run, gait and playback threads; leaving inspect parks its scene and selection link. Commands of another mode are refused naming the active mode. Result: mode, previous, document, load_seconds (unchanged=true when the mode was already active)."),
        c("screenshot", json!({"path":"/tmp/view.png"}), "Save the window exactly as drawn (UI, overlays, lesson pages) to a PNG after the next frame; refused, naming the cause, while the window is not visible"),
    ]
}

/// `system_ui` in modes without controls of their own.
pub fn mode_ui_capability() -> Value {
    sim_api::capability("system_ui", json!({"action":{"operation":"controls"}}), "Inspect and place mode: the mode switcher's controls mode:inspect | mode:build | mode:lessons | mode:robot | mode:place (controls; activate {id, ui_revision}, the same handler as viewer_mode and the switcher's buttons; ui_revision is not checked for mode:* controls). In build, lessons and robot mode system_ui is the mode's own (its controls list ends with these mode:* controls).")
}

/// The command's modes (from the tagged capability list), or None for a
/// command no capability names (its handler answers it).
pub(crate) fn command_modes(name: &str) -> Option<&'static [ViewerMode]> {
    static TABLE: std::sync::OnceLock<std::collections::BTreeMap<String, Vec<ViewerMode>>> = std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| {
            let mut table: std::collections::BTreeMap<String, Vec<ViewerMode>> = Default::default();
            for c in crate::rest::capabilities() {
                let modes = table.entry(c["command"].as_str().unwrap_or_default().to_string()).or_default();
                for m in c["modes"].as_array().into_iter().flatten().filter_map(|m| m.as_str()).filter_map(|m| ViewerMode::parse(m).ok()) {
                    if !modes.contains(&m) {
                        modes.push(m);
                    }
                }
            }
            table
        })
        .get(name)
        .map(Vec::as_slice)
}

fn mode_list(modes: &[ViewerMode]) -> String {
    let names: Vec<&str> = modes.iter().map(|m| m.name()).collect();
    match names.as_slice() {
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
        [] => String::new(),
    }
}

/// The one REST dispatch, in front of every mode's handler: answers
/// `viewer_mode` and `system_ui` `mode:*` itself (and `system_ui` in modes
/// without controls), and refuses a command of another mode, naming it,
/// the modes it needs and the active mode. None: the active mode's own
/// handler takes the command, unchanged. `modes` is None for the headless
/// server, which has no window to switch.
pub(crate) fn route(mode: ViewerMode, modes: Option<(&mut Switcher, &Documents)>, command: &sim_api::Command, continuation: &mut Value, cancelled: bool) -> Option<Outcome> {
    let name = command.command.as_str();
    let headless = || Outcome::Done(Err(format!("{name}: this headless server has no window to switch (inspect mode only)")));
    if name == "viewer_mode" {
        return Some(match modes {
            Some((switch, documents)) => viewer_mode(switch, documents, mode, &command.args, continuation, cancelled),
            None => headless(),
        });
    }
    if name == "system_ui" {
        let action = &command.args["action"];
        let target = action["id"].as_str().and_then(|id| id.strip_prefix("mode:"));
        if action["operation"] == "activate" {
            if let Some(target) = target {
                return Some(match modes {
                    Some((switch, _)) => rest_switch(switch, ViewerMode::parse(target).map(|mode| ModeSwitch { mode, document: None }), continuation, cancelled),
                    None => headless(),
                });
            }
        }
        if matches!(mode, ViewerMode::Inspect | ViewerMode::Place) {
            return Some(Outcome::Done(match action["operation"].as_str() {
                Some("controls") => Ok(json!({"ui_revision": 0, "ready": true, "controls": mode_controls(mode), "state": {"viewer_mode": mode}})),
                _ => Err(format!("system_ui in {} mode has only the mode switcher's controls: operation controls, or activate with a mode:<mode> id", mode.name())),
            }));
        }
        return None;
    }
    match command_modes(name) {
        Some(needs) if !needs.contains(&mode) => Some(Outcome::Done(Err(format!(
            "`{name}` is a {} mode command; the active mode is {}. Switch first: viewer_mode {{\"mode\":\"{}\"}}.",
            mode_list(needs),
            mode.name(),
            needs[0].name()
        )))),
        _ => None,
    }
}

/// What every mode adds to its handler's answer: the active mode on
/// `state`, and the mode switcher's controls at the end of `system_ui`
/// controls.
pub(crate) fn annotate(mode: ViewerMode, command: &sim_api::Command, mut outcome: Outcome) -> Outcome {
    if let Outcome::Done(Ok(value)) = &mut outcome {
        match command.command.as_str() {
            "state" if value.is_object() => value["viewer_mode"] = json!(mode.name()),
            "system_ui" if command.args["action"]["operation"] == "controls" => {
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
fn mode_controls(active: ViewerMode) -> Vec<Value> {
    ViewerMode::ALL
        .into_iter()
        .map(|m| {
            json!({"id": format!("mode:{}", m.name()), "label": format!("Mode: {}", m.label()), "enabled": m != active,
                "disabled_reason": (m == active).then(|| format!("{} mode is active", m.label())), "action": {"viewer_mode": {"mode": m}}})
        })
        .collect()
}

fn viewer_mode(switch: &mut Switcher, documents: &Documents, mode: ViewerMode, args: &Value, continuation: &mut Value, cancelled: bool) -> Outcome {
    let status = continuation.is_null() && (args.is_null() || args.as_object().is_some_and(|a| a.is_empty()));
    if status {
        return Outcome::Done(Ok(switch.json(mode, documents)));
    }
    rest_switch(switch, parse_request(args), continuation, cancelled)
}

fn parse_request(args: &Value) -> Result<ModeSwitch, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Args {
        mode: String,
        #[serde(default)]
        path: Option<PathBuf>,
        #[serde(default)]
        preset: Option<String>,
    }
    let args: Args = serde_json::from_value(args.clone()).map_err(|e| format!("viewer_mode: {e}"))?;
    let mode = ViewerMode::parse(&args.mode)?;
    let document = match (args.path, args.preset) {
        (Some(_), Some(_)) => return Err("viewer_mode takes path or preset, not both".into()),
        (Some(p), None) => Some(super::switch::Document::Path(p)),
        (None, Some(id)) => Some(super::switch::Document::Preset(id)),
        (None, None) => None,
    };
    Ok(ModeSwitch { mode, document })
}

/// REST `viewer_mode` and `system_ui` `mode:*`: submit on the first call,
/// answer once [`handle`] has decided (a load or the transition may take
/// frames; the job stays pending meanwhile and is never dropped).
fn rest_switch(switch: &mut Switcher, request: Result<ModeSwitch, String>, continuation: &mut Value, cancelled: bool) -> Outcome {
    let Some(seq) = continuation.get("switch").and_then(Value::as_u64) else {
        return match request {
            Ok(request) => {
                *continuation = json!({"switch": switch.submit(request)});
                Outcome::Pending
            }
            Err(e) => Outcome::Done(Err(e)),
        };
    };
    if let Some(result) = switch.outcome(seq) {
        return Outcome::Done(result);
    }
    if cancelled && switch.cancel(seq) {
        return Outcome::Done(Err("cancelled".into()));
    }
    if switch.waiting(seq) { Outcome::Pending } else { Outcome::Done(Err("the mode switch was superseded".into())) }
}

