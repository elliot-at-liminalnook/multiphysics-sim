//! Robot presets declared in `web/viewer/presets.json` (the browser's list):
//! a scene, a controller config and an optional task, run natively by the
//! shared `sim_runtime` `EmbeddedEnvironment` (with a task) or
//! `EmbeddedSession` (without). The files are parsed exactly as declared into
//! the sim_runtime types; the viewer chooses no controller, gain, option or
//! parameter. Only the seed is the viewer's, and it is recorded.
//!
//! A `recorded` preset (a scene and a `capture`, the `*-execution.json` run
//! report `sim_runtime::embedded` writes) is played back, never simulated:
//! the scene's robot is drawn and the capture's frames pose its links by name
//! ([`RecordedRun`]). No physics is built or stepped for it.
use serde_json::{Value, json};
use sim_runtime::embedded::Config;
use sim_runtime::embedded_capture::RecordedCapture;
use sim_runtime::environment::Task;
use sim_runtime::session::Scene;
use std::path::{Path, PathBuf};

/// Where the browser's preset list lives, relative to the workspace root
/// (`crate::workspace`); the declared paths resolve against the same root.
pub const PRESETS: &str = "web/viewer/presets.json";
/// [`PRESETS`] under the workspace root (`crate::workspace`), or the error
/// naming what was searched.
pub fn default_file() -> Result<std::path::PathBuf, String> {
    crate::workspace::path(PRESETS)
}
/// The browser worker's default seed (`web/worker.js`: `data.seed ?? 0`); no preset declares one.
pub const SEED: u64 = 0;
pub const SEED_RULE: &str = "seed 0: the browser worker's default (web/worker.js `data.seed ?? 0`); no preset declares a seed. Reset rebuilds with the same seed.";
/// Session work chunk: the browser worker's `clamp(report_every, 1, 40)` nominal steps (web/worker.js).
pub const SESSION_CHUNK_MAX: usize = 40;

/// One declared preset, with its text fields verbatim.
#[derive(Clone, Debug)]
pub struct Preset {
    pub id: String,
    pub mode: String,
    pub label: String,
    pub scene: Option<String>,
    pub config: Option<String>,
    pub task: Option<String>,
    /// A recorded preset's capture (`*-execution.json`).
    pub capture: Option<String>,
    /// The whole entry as declared (readiness, evidence, description and motion_* verbatim).
    pub entry: Value,
}
impl Preset {
    fn text(&self, key: &str) -> Option<&str> {
        self.entry.get(key).and_then(|v| v.as_str())
    }
    pub fn readiness(&self) -> Option<&str> {
        self.text("readiness")
    }
    pub fn evidence(&self) -> Option<&str> {
        self.text("evidence")
    }
    /// Declared input paths (scene, config, task, capture), in that order.
    pub fn paths(&self) -> Vec<(&'static str, &str)> {
        [("scene", &self.scene), ("config", &self.config), ("task", &self.task), ("capture", &self.capture)].into_iter().filter_map(|(k, p)| p.as_deref().map(|p| (k, p))).collect()
    }
    /// Declared inputs missing under `root`.
    pub fn missing(&self, root: &Path) -> Vec<String> {
        self.paths().into_iter().filter(|(_, p)| !root.join(p).is_file()).map(|(_, p)| p.to_string()).collect()
    }
    pub fn is_recorded(&self) -> bool {
        self.mode == RECORDED
    }
    /// Why a recorded preset cannot be opened: every undeclared scene/capture
    /// and every missing declared file, named (`Ok` when it can be played back).
    fn recorded_openable(&self, root: &Path) -> Result<(), String> {
        let undeclared: Vec<&str> = [("scene", &self.scene), ("capture", &self.capture)].into_iter().filter(|(_, p)| p.is_none()).map(|(k, _)| k).collect();
        let missing = self.missing(root);
        let mut why = Vec::new();
        if !undeclared.is_empty() {
            why.push(format!("recorded preset declares no {}", undeclared.join(" and no ")));
        }
        if !missing.is_empty() {
            why.push(format!("missing inputs: {}", missing.join(", ")));
        }
        if why.is_empty() { Ok(()) } else { Err(why.join("; ")) }
    }
    /// A discovery row: id, label, mode, paths, whether the inputs exist, and whether it can run here.
    pub fn discovery(&self, root: &Path) -> Value {
        let missing = self.missing(root);
        let under_runs = self.paths().iter().any(|(_, p)| p.starts_with("runs/"));
        let openable = match (self.mode.as_str(), self.config.is_some(), missing.is_empty()) {
            ("embedded", true, true) => Ok(()),
            ("embedded", false, _) => Err("declares no controller config".to_string()),
            ("embedded", true, false) => Err(format!("missing inputs: {}", missing.join(", "))),
            (RECORDED, ..) => self.recorded_openable(root),
            (mode, ..) => Err(format!("mode `{mode}` is neither embedded nor recorded; only embedded presets run and recorded presets play back natively")),
        };
        let runs_as = match (self.is_recorded(), self.task.is_some()) {
            (true, _) => RECORDED_RUNS_AS,
            (false, true) => "EmbeddedEnvironment",
            (false, false) => "EmbeddedSession",
        };
        json!({"id": self.id, "label": self.label, "mode": self.mode, "scene": self.scene, "config": self.config, "task": self.task, "capture": self.capture,
            "inputs_exist": missing.is_empty(), "missing": missing, "under_ignored_runs": under_runs,
            "runs_as": runs_as,
            "openable": openable.is_ok(), "not_openable_reason": openable.err(), "openable_rule": OPENABLE_RULE})
    }
}

/// The preset list, read from `path`. Errors name the path.
pub fn list(path: &Path) -> Result<Vec<Preset>, String> {
    let name = path.display();
    let text = std::fs::read_to_string(path).map_err(|e| format!("{name}: {e}"))?;
    let raw: Value = serde_json::from_str(&text).map_err(|e| format!("{name}: {e}"))?;
    let entries = raw.get("presets").and_then(|p| p.as_array()).ok_or_else(|| format!("{name}: no `presets` array"))?;
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let s = |k: &str| e.get(k).and_then(|v| v.as_str()).map(str::to_string);
            let id = s("id").ok_or_else(|| format!("{name}: presets[{i}] has no id"))?;
            Ok(Preset { mode: s("mode").unwrap_or_default(), label: s("label").unwrap_or_else(|| id.clone()), scene: s("scene"), config: s("config"), task: s("task"), capture: s("capture"), entry: e.clone(), id })
        })
        .collect()
}

/// The preset `id`, if it can open natively: it exists, and either its mode
/// is `embedded`, it declares a scene and a config and its inputs exist under
/// `root`, or its mode is `recorded` with a scene and a capture that exist.
pub fn select(path: &Path, root: &Path, id: &str) -> Result<Preset, String> {
    let presets = list(path)?;
    let Some(p) = presets.iter().find(|p| p.id == id) else {
        let ids = |mode: &str| presets.iter().filter(|p| p.mode == mode).map(|p| p.id.as_str()).collect::<Vec<_>>().join(", ");
        return Err(format!("unknown robot preset `{id}` in {}; embedded presets: {}; recorded presets: {}", path.display(), ids("embedded"), ids(RECORDED)));
    };
    if p.is_recorded() {
        return p.recorded_openable(root).map(|()| p.clone()).map_err(|why| format!("robot preset `{id}` (recorded) cannot be opened under {}: {why}", root.display()));
    }
    if p.mode != "embedded" {
        return Err(format!("robot preset `{id}` has mode `{}`; only mode `embedded` (a scene + controller config run by EmbeddedSession/EmbeddedEnvironment) runs natively, and mode `recorded` (a scene + capture) plays back", p.mode));
    }
    if p.scene.is_none() || p.config.is_none() {
        return Err(format!("robot preset `{id}` declares no {}", if p.scene.is_none() { "scene" } else { "config" }));
    }
    let missing = p.missing(root);
    if !missing.is_empty() {
        return Err(format!("robot preset `{id}`: declared inputs not found under {}: {}", root.display(), missing.join(", ")));
    }
    Ok(p.clone())
}

/// The mode of a preset played back from a capture.
pub const RECORDED: &str = "recorded";
pub const RECORDED_RUNS_AS: &str = "recorded playback: the capture's frames pose the scene's links by name; no physics is built or stepped";
pub const OPENABLE_RULE: &str = "embedded: declares a scene and config and every declared input exists (the build is not attempted here and can still fail, robot_state.run.error); recorded: declares a scene and a capture and both files exist (they are parsed on a worker when opened and can still fail, robot_state.error)";
/// The label of a recorded preset's view (robot_state, the header).
pub const RECORDED_LABEL: &str = "recorded physics (played back, not simulated here)";

/// A recorded preset opened on the loader thread: the scene parsed as
/// declared, the capture read by the shared `sim_runtime::embedded_capture`
/// reader, and every frame mapped to the scene's links by name (the same
/// mapping as live preset frames, `robot_run::map_poses`).
pub struct RecordedRun {
    pub preset: Preset,
    /// Absolute scene path (the CAD link resolves against it).
    pub scene_path: PathBuf,
    pub scene: Scene,
    pub capture_path: PathBuf,
    pub capture: RecordedCapture,
    /// One mapped frame per capture frame, same order (generation 0).
    pub frames: Vec<crate::robot::run::Frame>,
    /// Capture pose names matching no scene link, over all frames (sorted, unique).
    pub unmatched: Vec<String>,
    pub root: PathBuf,
    /// Measured on the loader thread: scene read + parse, capture read + parse + sort, name mapping (s).
    pub scene_load_s: f64,
    pub capture_load_s: f64,
    pub map_s: f64,
}
impl RecordedRun {
    /// Reads the scene and capture and maps the frames. Run on a worker
    /// thread: captures are tens of megabytes. Errors name the file.
    pub fn load(preset: Preset, root: &Path) -> Result<(Self, Value), String> {
        let id = preset.id.clone();
        let scene_rel = preset.scene.clone().ok_or_else(|| format!("recorded preset `{id}` declares no scene"))?;
        let capture_rel = preset.capture.clone().ok_or_else(|| format!("recorded preset `{id}` declares no capture"))?;
        let t = std::time::Instant::now();
        let (scene, mut raw_scene): (Scene, Value) = read(root, &scene_rel)?;
        let scene_load_s = t.elapsed().as_secs_f64();
        let t = std::time::Instant::now();
        let capture_path = root.join(&capture_rel);
        let capture = RecordedCapture::read(&capture_path)?;
        let capture_load_s = t.elapsed().as_secs_f64();
        let t = std::time::Instant::now();
        let links: Vec<String> = scene.robot.links.iter().map(|l| l.name.clone()).collect();
        let frames: Vec<crate::robot::run::Frame> = capture.frames.iter().enumerate().map(|(i, f)| crate::robot::run::recorded_frame(f, &links, 0, i as u64)).collect();
        let mut unmatched: Vec<String> = frames.iter().flat_map(|f| f.unmatched.iter().cloned()).collect();
        unmatched.sort();
        unmatched.dedup();
        let map_s = t.elapsed().as_secs_f64();
        let robot = raw_scene.get_mut("robot").map(Value::take).unwrap_or(Value::Null);
        Ok((Self { scene_path: root.join(&scene_rel), preset, scene, capture_path, capture, frames, unmatched, root: root.to_path_buf(), scene_load_s, capture_load_s, map_s }, robot))
    }
    /// The refusal of a live-only action on this recorded preset, naming it.
    pub fn refusal(&self, what: &str) -> String {
        format!("{what} is refused: preset `{}` is a recorded preset ({RECORDED_LABEL}); nothing is simulated, so there is no run, jog, motion request, recording, replay or gait preview", self.preset.id)
    }
    /// `robot_state.preset` for a recorded preset: identity, paths, the verbatim
    /// text, the capture's metadata as written (absent fields null) and load timings.
    pub fn state_json(&self, links: usize) -> Value {
        let p = &self.preset;
        let c = &self.capture;
        json!({"id": p.id, "label": p.label, "mode": p.mode, "scene": p.scene, "capture": p.capture, "loaded": true,
            "description": p.entry.get("description"), "readiness": p.readiness(), "evidence": p.evidence(),
            "runs_as": RECORDED_RUNS_AS, "recorded_label": RECORDED_LABEL,
            "capture_path": self.capture_path, "frame_count": c.frames.len(), "first_time_s": c.frames.first().map(|f| f.time_s), "duration_s": c.duration_s(),
            "meta": c.meta, "recorded_rate": c.meta.recorded_rate(), "recorded_rate_rule": "simulated_s / stepping_wall_s from the file; null when either is absent",
            "unmatched_capture_links": self.unmatched, "scene_links": links,
            "time_rule": sim_runtime::embedded_capture::TIME_RULE, "lookup_rule": sim_runtime::embedded_capture::LOOKUP_RULE,
            "load_seconds": {"scene": self.scene_load_s, "capture": self.capture_load_s, "map": self.map_s, "thread": "robot preset loader (not the UI thread)"},
            "presets_file": PRESETS})
    }
}

/// Reads and parses a declared file as `T`, returning the raw JSON too. Errors name the file.
fn read<T: serde::de::DeserializeOwned>(root: &Path, rel: &str) -> Result<(T, Value), String> {
    let path = root.join(rel);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let raw: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let parsed = T::deserialize(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((parsed, raw))
}

/// A preset's inputs parsed exactly as declared, plus the recorded seed.
pub struct PresetRun {
    pub preset: Preset,
    /// Absolute scene path (the CAD link resolves against it).
    pub scene_path: PathBuf,
    pub scene: Scene,
    pub config: Config,
    pub task: Option<Task>,
    pub seed: u64,
    /// The root the declared paths were resolved against (saved recordings go under it).
    pub root: PathBuf,
}
impl PresetRun {
    /// Reads and parses the declared files (the scene's `robot` is also
    /// returned raw for the T9 loader's file notes). Run on a worker thread:
    /// full-robot scenes are megabytes. Errors name the file.
    pub fn load(preset: Preset, root: &Path) -> Result<(Self, Value), String> {
        let scene_rel = preset.scene.clone().ok_or_else(|| format!("robot preset `{}` declares no scene", preset.id))?;
        let config_rel = preset.config.clone().ok_or_else(|| format!("robot preset `{}` declares no config", preset.id))?;
        let (scene, mut raw_scene): (Scene, Value) = read(root, &scene_rel)?;
        let (config, _): (Config, Value) = read(root, &config_rel)?;
        let task = match &preset.task {
            Some(t) => Some(read::<Task>(root, t)?.0),
            None => None,
        };
        let robot = raw_scene.get_mut("robot").map(Value::take).unwrap_or(Value::Null);
        Ok((Self { scene_path: root.join(&scene_rel), preset, scene, config, task, seed: SEED, root: root.to_path_buf() }, robot))
    }
    /// Nominal steps per chunk: one whole action interval (task period ÷
    /// step) for an environment, as `EmbeddedEnvironment::step` advances;
    /// otherwise the browser worker's `clamp(report_every, 1, 40)`.
    pub fn chunk_steps(&self) -> usize {
        match &self.task {
            Some(t) => (t.period_s / self.config.step_s).round().max(1.0) as usize,
            None => self.config.report_every.clamp(1, SESSION_CHUNK_MAX),
        }
    }
    pub fn chunk_s(&self) -> f64 {
        self.chunk_steps() as f64 * self.config.step_s
    }
    pub fn kind(&self) -> &'static str {
        if self.task.is_some() { "EmbeddedEnvironment" } else { "EmbeddedSession" }
    }
    /// `robot_state.preset`: identity, declared paths, readiness/evidence verbatim, seed and step settings.
    pub fn state_json(&self, completed_steps: Option<u64>) -> Value {
        let p = &self.preset;
        json!({"id": p.id, "label": p.label, "mode": p.mode, "scene": p.scene, "config": p.config, "task": p.task,
            "readiness": p.readiness(), "evidence": p.evidence(), "description": p.entry.get("description"),
            "runs_as": self.kind(), "seed": self.seed, "seed_rule": SEED_RULE,
            "step_s": self.config.step_s, "requested_steps": self.config.steps, "completed_steps": completed_steps,
            "horizon_s": self.config.step_s * self.config.steps as f64,
            "chunk": {"nominal_steps": self.chunk_steps(), "seconds": self.chunk_s(),
                "rule": if self.task.is_some() { "one whole action interval (task period_s / step_s nominal steps) per EmbeddedEnvironment::step" } else { "clamp(report_every, 1, 40) nominal steps per EmbeddedSession::advance, as the browser worker" }},
            "action_rule": "the held action is the session's own input values: InputChannel.initial at build/reset (as EmbeddedEnvironment::prepare_replay holds them); the viewer does not choose values",
            "presets_file": PRESETS})
    }
}
