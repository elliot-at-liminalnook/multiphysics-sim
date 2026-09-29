//! Scripted presentation timelines. A Rhai script (or a YAML cue list) is
//! evaluated once into a deterministic, sorted list of [`Cue`]s keyed to
//! simulation time. Only `set` changes physics, and it does so as a recorded
//! run input applied through the shared runtime; every other cue (captions,
//! highlights, camera, plots, pauses) is presentation and never touches the
//! model. Lessons use this; phenomena exhibits and builder demos can too.
//!
//! Script API (times in simulated seconds):
//! ```text
//! at(0.0);                      // move the cursor to t = 0
//! caption("Motor on");
//! highlight("gearbox/worm");    // or highlight(["motor", "gearbox"]); clear_highlight()
//! camera("side");               // preset; camera("iso", "gearbox"); camera(#{preset:"top", zoom:1.5})
//! plot("drum.shaft.speed");     // or plot([...])
//! wait(1.2);                    // advance the cursor
//! set("supply.voltage", 0.0);   // physics: instance path "." parameter
//! pause();                      // stop playback here until the reader resumes
//! speed(0.25);                  // playback speed from here on
//! zoom("gearbox", 2.5, 2.0);      // glide to frame a part, 2.5× closer, over 2 s
//! orbit(0.2);                     // circle the subject slowly (rad/s; 0 stops)
//! spotlight("motor");             // dim everything else; spotlight([]) clears
//! pin("gearbox/worm", "tooth pushes here"); unpin();
//! inset("gearbox/mesh", 3.0);     // picture-in-picture following a part; inset_off()
//! xray(true); explode(true);
//! ```
use rhai::{Array, Dynamic, Engine, EvalAltResult, Map, Position};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

pub const MAX_CUES: usize = 10_000;

/// Where the camera looks: a preset direction, optionally framing one
/// instance path, with an optional zoom (1 = fit).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraSpec {
    #[serde(default)]
    pub preset: Option<Preset>,
    /// Instance path to frame (its parts and everything inside it).
    #[serde(default)]
    pub focus: Option<String>,
    #[serde(default)]
    pub zoom: Option<f32>,
    /// Explicit orbit angles in radians (override the preset).
    #[serde(default)]
    pub yaw: Option<f32>,
    #[serde(default)]
    pub pitch: Option<f32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    Iso,
    Front,
    Side,
    Top,
}
impl Preset {
    pub fn parse(s: &str) -> Result<Self, String> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| format!("unknown camera preset `{s}` (iso, front, side or top)"))
    }
    /// Orbit (yaw, pitch) in radians, Y up.
    pub fn angles(self) -> (f32, f32) {
        match self {
            Preset::Iso => (0.35, 0.60),
            Preset::Front => (0.0, 0.08),
            Preset::Side => (std::f32::consts::FRAC_PI_2, 0.08),
            Preset::Top => (0.0, 1.45),
        }
    }
}
impl CameraSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.zoom.is_some_and(|z| !z.is_finite() || z <= 0.0 || z > 100.0) {
            return Err("camera zoom must be in (0, 100]".into());
        }
        if self.yaw.is_some_and(|v| !v.is_finite()) || self.pitch.is_some_and(|p| !p.is_finite() || p.abs() > 1.5) {
            return Err("camera yaw must be finite and |pitch| ≤ 1.5 rad".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Physics input: `parameter` is `instance/path.parameter`.
    Set { parameter: String, value: f64 },
    Caption { text: String },
    /// Empty clears the highlight.
    Highlight { paths: Vec<String> },
    Camera { camera: CameraSpec },
    Plot { observables: Vec<String> },
    Pause,
    Speed { factor: f64 },
    /// Camera and emphasis directives (display only).
    View { view: View },
}

/// Camera and emphasis directives shared by scene scripts, YAML cues and
/// narration. Display only: none of them changes the physics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "view", rename_all = "snake_case", deny_unknown_fields)]
pub enum View {
    /// Glide the camera to frame `focus` (an instance path; none = all) at
    /// `zoom` (1 = fit) over `seconds`, keeping the current direction.
    Zoom { focus: Option<String>, zoom: f64, seconds: f64 },
    /// Circle the subject at `rate` rad/s; 0 stops. The learner's drag stops it.
    Orbit { rate: f64 },
    /// Dim everything outside these instance paths; empty clears.
    Spotlight { paths: Vec<String> },
    /// A 3D arrow and label anchored to a part.
    Pin { path: String, label: String },
    /// Remove every pin.
    Unpin,
    /// A picture-in-picture view following a part (`zoom` × closer); none closes.
    Inset { path: Option<String>, zoom: f64 },
    /// Everything but the spotlight (or selection) drawn see-through.
    Xray { on: bool },
    /// Parts moved apart along their layout, eased.
    Explode { on: bool },
}
impl View {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            View::Zoom { zoom, seconds, .. } if !(zoom.is_finite() && *zoom > 0. && *zoom <= 100. && seconds.is_finite() && *seconds >= 0. && *seconds <= 60.) => Err("zoom needs a factor in (0, 100] and 0–60 seconds".into()),
            View::Orbit { rate } if !(rate.is_finite() && rate.abs() <= 3.) => Err("orbit rate must be within ±3 rad/s".into()),
            View::Inset { zoom, .. } if !(zoom.is_finite() && *zoom > 0. && *zoom <= 100.) => Err("inset zoom must be in (0, 100]".into()),
            _ => Ok(()),
        }
    }
}

/// The display state that view directives build up over time.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ViewState {
    /// The latest zoom glide and when it was cued (acted on once).
    pub zoom: Option<(f64, View)>,
    pub orbit: f64,
    pub spotlight: Vec<String>,
    pub pins: Vec<(String, String)>,
    pub inset: Option<(String, f64)>,
    pub xray: bool,
    pub explode: bool,
}
impl ViewState {
    /// Fold one directive, cued at `at_s`, into the state.
    pub fn apply(&mut self, at_s: f64, view: &View) {
        match view {
            View::Zoom { .. } => self.zoom = Some((at_s, view.clone())),
            View::Orbit { rate } => self.orbit = *rate,
            View::Spotlight { paths } => self.spotlight = paths.clone(),
            View::Pin { path, label } => {
                self.pins.retain(|(p, _)| p != path);
                self.pins.push((path.clone(), label.clone()));
            }
            View::Unpin => self.pins.clear(),
            View::Inset { path, zoom } => self.inset = path.clone().map(|p| (p, *zoom)),
            View::Xray { on } => self.xray = *on,
            View::Explode { on } => self.explode = *on,
        }
    }
    pub fn is_default(&self) -> bool {
        *self == ViewState::default()
    }
}
impl Action {
    pub fn is_physics(&self) -> bool {
        matches!(self, Action::Set { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cue {
    pub at_s: f64,
    pub action: Action,
}

/// Cues sorted by time (authoring order kept for equal times).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub cues: Vec<Cue>,
}
impl Timeline {
    pub fn new(mut cues: Vec<Cue>) -> Result<Self, String> {
        if cues.len() > MAX_CUES {
            return Err(format!("more than {MAX_CUES} cues"));
        }
        for c in &cues {
            if !c.at_s.is_finite() || c.at_s < 0.0 {
                return Err(format!("cue time {} must be finite and ≥ 0", c.at_s));
            }
            match &c.action {
                Action::Set { parameter, value } => {
                    split_parameter(parameter)?;
                    if !value.is_finite() {
                        return Err(format!("set {parameter}: value must be finite"));
                    }
                }
                Action::Speed { factor } if !(factor.is_finite() && *factor > 0.0 && *factor <= 100.0) => {
                    return Err("speed factor must be in (0, 100]".into());
                }
                Action::Camera { camera } => camera.validate()?,
                Action::View { view } => view.validate()?,
                Action::Caption { text } if text.len() > 4000 => return Err("caption longer than 4000 bytes".into()),
                _ => {}
            }
        }
        cues.sort_by(|a, b| a.at_s.total_cmp(&b.at_s));
        Ok(Self { cues })
    }
    pub fn merged(mut self, other: Timeline) -> Result<Self, String> {
        self.cues.extend(other.cues);
        Timeline::new(self.cues)
    }
    /// Physics inputs only, in time order.
    pub fn physics(&self) -> impl Iterator<Item = (f64, &str, f64)> {
        self.cues.iter().filter_map(|c| match &c.action {
            Action::Set { parameter, value } => Some((c.at_s, parameter.as_str(), *value)),
            _ => None,
        })
    }
    /// Presentation state at time `t`: the latest caption, highlight, camera,
    /// plots and speed at or before `t`.
    pub fn state_at(&self, t: f64) -> PresentationState {
        let mut s = PresentationState { speed: 1.0, ..Default::default() };
        for c in self.cues.iter().take_while(|c| c.at_s <= t + 1e-12) {
            match &c.action {
                Action::Caption { text } => s.caption = Some(text.clone()),
                Action::Highlight { paths } => s.highlight = paths.clone(),
                Action::Camera { camera } => s.camera = Some((c.at_s, camera.clone())),
                Action::Plot { observables } => s.plots = Some(observables.clone()),
                Action::Speed { factor } => s.speed = *factor,
                Action::View { view } => s.view.apply(c.at_s, view),
                Action::Set { .. } | Action::Pause => {}
            }
        }
        s
    }
    /// First pause strictly after `from` and at or before `to`.
    pub fn pause_between(&self, from: f64, to: f64) -> Option<f64> {
        self.cues.iter().find(|c| matches!(c.action, Action::Pause) && c.at_s > from + 1e-12 && c.at_s <= to + 1e-12).map(|c| c.at_s)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresentationState {
    pub caption: Option<String>,
    pub highlight: Vec<String>,
    /// When the camera cue fired, and where to look.
    pub camera: Option<(f64, CameraSpec)>,
    pub plots: Option<Vec<String>>,
    pub speed: f64,
    pub view: ViewState,
}

/// `instance/path.parameter` → (at, name, parameter): `at` is the parent path
/// of the instance ("" at the top), `name` the instance, `parameter` the rest
/// after the first dot following the last `/` (parameters such as
/// `initial.shaft.speed` keep their dots).
pub fn split_parameter(key: &str) -> Result<(String, String, String), String> {
    let slash = key.rfind('/').map(|i| i + 1).unwrap_or(0);
    let dot = key[slash..].find('.').map(|i| i + slash).ok_or_else(|| format!("`{key}` must be instance/path.parameter"))?;
    let (path, parameter) = (&key[..dot], &key[dot + 1..]);
    if path.is_empty() || parameter.is_empty() || path.split('/').any(|p| p.is_empty()) {
        return Err(format!("`{key}` must be instance/path.parameter"));
    }
    let (at, name) = match path.rsplit_once('/') {
        Some((at, name)) => (at.to_string(), name.to_string()),
        None => (String::new(), path.to_string()),
    };
    Ok((at, name, parameter.to_string()))
}

struct Cursor {
    t: f64,
    cues: Vec<Cue>,
}

fn located(name: &str, e: &EvalAltResult) -> String {
    let p: Position = e.position();
    match p.line() {
        Some(line) => format!("{name}:{line}: {}", e.to_string().split(" (line ").next().unwrap_or_default()),
        None => format!("{name}: {e}"),
    }
}

fn strings(value: Dynamic) -> Result<Vec<String>, Box<EvalAltResult>> {
    if value.is_string() {
        return Ok(vec![value.into_string().map_err(|e| format!("expected text, found {e}"))?]);
    }
    let array = value.try_cast::<Array>().ok_or("expected a string or an array of strings")?;
    array.into_iter().map(|v| v.into_string().map_err(|e| format!("expected text, found {e}").into())).collect()
}

/// Evaluate a presentation script into a timeline. `name` is used in errors
/// (`name:line: message`). Scripts are pure: no file or clock access, a
/// bounded operation count, and the same source always gives the same cues.
pub fn evaluate(name: &str, source: &str) -> Result<Timeline, String> {
    let cursor = Arc::new(Mutex::new(Cursor { t: 0.0, cues: Vec::new() }));
    let mut engine = Engine::new();
    engine.set_max_operations(1_000_000);
    engine.set_max_call_levels(32);
    engine.set_max_expr_depths(64, 64);
    engine.on_print(|_| {});
    let push = {
        let cursor = cursor.clone();
        move |action: Action| -> Result<(), Box<EvalAltResult>> {
            let mut c = cursor.lock().unwrap();
            if c.cues.len() >= MAX_CUES {
                return Err(format!("more than {MAX_CUES} cues").into());
            }
            let at_s = c.t;
            c.cues.push(Cue { at_s, action });
            Ok(())
        }
    };
    let time = |v: Dynamic| -> Result<f64, Box<EvalAltResult>> {
        let t = if v.is_int() { v.as_int().unwrap() as f64 } else { v.as_float().map_err(|_| "time must be a number of seconds")? };
        if !t.is_finite() || t < 0.0 {
            return Err(format!("time {t} must be finite and ≥ 0").into());
        }
        Ok(t)
    };
    {
        let cursor = cursor.clone();
        engine.register_fn("at", move |t: Dynamic| -> Result<(), Box<EvalAltResult>> {
            cursor.lock().unwrap().t = time(t)?;
            Ok(())
        });
    }
    {
        let cursor = cursor.clone();
        engine.register_fn("wait", move |dt: Dynamic| -> Result<(), Box<EvalAltResult>> {
            let dt = time(dt)?;
            cursor.lock().unwrap().t += dt;
            Ok(())
        });
    }
    {
        let cursor = cursor.clone();
        engine.register_fn("time", move || cursor.lock().unwrap().t);
    }
    {
        let push = push.clone();
        engine.register_fn("set", move |parameter: &str, value: Dynamic| -> Result<(), Box<EvalAltResult>> {
            let value = if value.is_int() { value.as_int().unwrap() as f64 } else { value.as_float().map_err(|_| "set value must be a number")? };
            split_parameter(parameter)?;
            push(Action::Set { parameter: parameter.into(), value })
        });
    }
    {
        let push = push.clone();
        engine.register_fn("caption", move |text: &str| push(Action::Caption { text: text.into() }));
    }
    {
        let push = push.clone();
        engine.register_fn("highlight", move |paths: Dynamic| push(Action::Highlight { paths: strings(paths)? }));
    }
    {
        let push = push.clone();
        engine.register_fn("clear_highlight", move || push(Action::Highlight { paths: vec![] }));
    }
    {
        let push = push.clone();
        engine.register_fn("camera", move |preset: &str| push(Action::Camera { camera: CameraSpec { preset: Some(Preset::parse(preset)?), ..Default::default() } }));
    }
    {
        let push = push.clone();
        engine.register_fn("camera", move |preset: &str, focus: &str| push(Action::Camera { camera: CameraSpec { preset: Some(Preset::parse(preset)?), focus: Some(focus.into()), ..Default::default() } }));
    }
    {
        let push = push.clone();
        engine.register_fn("camera", move |spec: Map| -> Result<(), Box<EvalAltResult>> {
            let value: serde_json::Value = rhai::serde::from_dynamic(&Dynamic::from(spec))?;
            let camera: CameraSpec = serde_json::from_value(value).map_err(|e| format!("camera: {e}"))?;
            camera.validate()?;
            push(Action::Camera { camera })
        });
    }
    {
        let push = push.clone();
        engine.register_fn("plot", move |ids: Dynamic| push(Action::Plot { observables: strings(ids)? }));
    }
    {
        let push = push.clone();
        engine.register_fn("pause", move || push(Action::Pause));
    }
    {
        let push = push.clone();
        engine.register_fn("speed", move |factor: Dynamic| -> Result<(), Box<EvalAltResult>> {
            let factor = if factor.is_int() { factor.as_int().unwrap() as f64 } else { factor.as_float().map_err(|_| "speed must be a number")? };
            push(Action::Speed { factor })
        });
    }
    let num = |v: Dynamic, what: &str| -> Result<f64, Box<EvalAltResult>> {
        if v.is_int() { Ok(v.as_int().unwrap() as f64) } else { v.as_float().map_err(|_| format!("{what} must be a number").into()) }
    };
    let view = {
        let push = push.clone();
        move |v: View| -> Result<(), Box<EvalAltResult>> {
            v.validate()?;
            push(Action::View { view: v })
        }
    };
    {
        let view = view.clone();
        engine.register_fn("zoom", move |focus: &str, zoom: Dynamic, seconds: Dynamic| view(View::Zoom { focus: (!focus.is_empty()).then(|| focus.to_string()), zoom: num(zoom, "zoom")?, seconds: num(seconds, "seconds")? }));
    }
    {
        let view = view.clone();
        engine.register_fn("orbit", move |rate: Dynamic| view(View::Orbit { rate: num(rate, "orbit rate")? }));
    }
    {
        let view = view.clone();
        engine.register_fn("spotlight", move |paths: Dynamic| view(View::Spotlight { paths: strings(paths)? }));
    }
    {
        let view = view.clone();
        engine.register_fn("pin", move |path: &str, label: &str| view(View::Pin { path: path.into(), label: label.into() }));
    }
    {
        let view = view.clone();
        engine.register_fn("unpin", move || view(View::Unpin));
    }
    {
        let view = view.clone();
        engine.register_fn("inset", move |path: &str, zoom: Dynamic| view(View::Inset { path: Some(path.into()), zoom: num(zoom, "inset zoom")? }));
    }
    {
        let view = view.clone();
        engine.register_fn("inset_off", move || view(View::Inset { path: None, zoom: 1. }));
    }
    {
        let view = view.clone();
        engine.register_fn("xray", move |on: bool| view(View::Xray { on }));
    }
    {
        let view = view.clone();
        engine.register_fn("explode", move |on: bool| view(View::Explode { on }));
    }
    engine.run(source).map_err(|e| located(name, &e))?;
    let cues = std::mem::take(&mut cursor.lock().unwrap().cues);
    Timeline::new(cues).map_err(|e| format!("{name}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn script_builds_a_sorted_deterministic_timeline() {
        let src = r#"
            caption("Motor on");
            camera("iso", "gearbox");
            wait(1.2);
            set("supply.voltage", 0);
            highlight(["gearbox/worm", "drum"]);
            at(0.5);
            plot("drum.shaft.speed");
            at(2.0);
            pause();
        "#;
        let a = evaluate("power-off.rhai", src).unwrap();
        assert_eq!(a, evaluate("power-off.rhai", src).unwrap());
        let times: Vec<f64> = a.cues.iter().map(|c| c.at_s).collect();
        assert_eq!(times, vec![0.0, 0.0, 0.5, 1.2, 1.2, 2.0]);
        assert_eq!(a.physics().collect::<Vec<_>>(), vec![(1.2, "supply.voltage", 0.0)]);
        let s = a.state_at(1.5);
        assert_eq!(s.caption.as_deref(), Some("Motor on"));
        assert_eq!(s.highlight, vec!["gearbox/worm".to_string(), "drum".into()]);
        assert_eq!(a.pause_between(1.9, 2.1), Some(2.0));
    }
    #[test]
    fn errors_name_the_script_and_line() {
        let e = evaluate("bad.rhai", "caption(\"ok\");\nset(\"nodot\", 1.0);").unwrap_err();
        assert!(e.starts_with("bad.rhai:2:"), "{e}");
        let e = evaluate("bad.rhai", "camera(\"sideways\");").unwrap_err();
        assert!(e.contains("unknown camera preset"), "{e}");
        assert_eq!(split_parameter("gearbox/mesh.initial.shaft.speed").unwrap(), ("gearbox".into(), "mesh".into(), "initial.shaft.speed".into()));
        assert_eq!(split_parameter("supply.voltage").unwrap(), (String::new(), "supply".into(), "voltage".into()));
    }

    #[test]
    fn view_directives_build_a_view_state() {
        let src = r#"
            zoom("gearbox", 2.5, 1.5);
            orbit(0.2);
            spotlight(["motor", "gearbox"]);
            pin("gearbox/worm", "tooth pushes here");
            inset("gearbox/mesh", 3);
            wait(1.0);
            unpin();
            spotlight([]);
            inset_off();
            xray(true);
            explode(true);
            orbit(0);
        "#;
        let t = evaluate("view.rhai", src).unwrap();
        let early = t.state_at(0.5).view;
        assert_eq!(early.orbit, 0.2);
        assert_eq!(early.spotlight, ["motor", "gearbox"]);
        assert_eq!(early.pins, [("gearbox/worm".to_string(), "tooth pushes here".to_string())]);
        assert_eq!(early.inset, Some(("gearbox/mesh".to_string(), 3.0)));
        assert!(matches!(early.zoom, Some((at, View::Zoom { zoom, .. })) if at == 0.0 && zoom == 2.5));
        let late = t.state_at(1.5).view;
        assert!(late.pins.is_empty() && late.spotlight.is_empty() && late.inset.is_none());
        assert!(late.xray && late.explode && late.orbit == 0.0);
        assert!(evaluate("bad.rhai", "zoom(\"x\", -1, 1);").is_err());
        assert!(evaluate("bad.rhai", "orbit(9);").is_err());
    }
}
