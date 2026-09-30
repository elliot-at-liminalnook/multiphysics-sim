//! Robot presets declared in `web/viewer/presets.json` (the browser's list):
//! a scene, a controller config and an optional task, run natively by the
//! shared `sim_runtime` `EmbeddedEnvironment` (with a task) or
//! `EmbeddedSession` (without). The files are parsed exactly as declared into
//! the sim_runtime types; the viewer chooses no controller, gain, option or
//! parameter. Only the seed is the viewer's, and it is recorded.
use serde_json::{Value, json};
use sim_runtime::embedded::Config;
use sim_runtime::environment::Task;
use sim_runtime::session::Scene;
use std::path::{Path, PathBuf};

/// Where the browser's preset list lives, relative to the launch directory
/// (the repository root); the declared paths resolve against the same root.
pub const PRESETS: &str = "web/viewer/presets.json";
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
    /// Declared input paths (scene, config, task), in that order.
    pub fn paths(&self) -> Vec<(&'static str, &str)> {
        [("scene", &self.scene), ("config", &self.config), ("task", &self.task)].into_iter().filter_map(|(k, p)| p.as_deref().map(|p| (k, p))).collect()
    }
    /// Declared inputs missing under `root`.
    pub fn missing(&self, root: &Path) -> Vec<String> {
        self.paths().into_iter().filter(|(_, p)| !root.join(p).is_file()).map(|(_, p)| p.to_string()).collect()
    }
    /// A discovery row: id, label, mode, paths, whether the inputs exist, and whether it can run here.
    pub fn discovery(&self, root: &Path) -> Value {
        let missing = self.missing(root);
        let under_runs = self.paths().iter().any(|(_, p)| p.starts_with("runs/"));
        let openable = match (self.mode.as_str(), self.config.is_some(), missing.is_empty()) {
            ("embedded", true, true) => Ok(()),
            ("embedded", false, _) => Err("declares no controller config".to_string()),
            ("embedded", true, false) => Err(format!("missing inputs: {}", missing.join(", "))),
            (mode, ..) => Err(format!("mode `{mode}` is not embedded; only embedded presets run natively")),
        };
        json!({"id": self.id, "label": self.label, "mode": self.mode, "scene": self.scene, "config": self.config, "task": self.task,
            "inputs_exist": missing.is_empty(), "missing": missing, "under_ignored_runs": under_runs,
            "runs_as": if self.task.is_some() { "EmbeddedEnvironment" } else { "EmbeddedSession" },
            "openable": openable.is_ok(), "not_openable_reason": openable.err(), "openable_rule": "embedded, declares a scene and config, and every declared input exists; the build is not attempted here and can still fail (robot_state.run.error)"})
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
            Ok(Preset { mode: s("mode").unwrap_or_default(), label: s("label").unwrap_or_else(|| id.clone()), scene: s("scene"), config: s("config"), task: s("task"), entry: e.clone(), id })
        })
        .collect()
}

/// The preset `id`, if it can run natively: it exists, its mode is
/// `embedded`, it declares a scene and a config, and its inputs exist under `root`.
pub fn select(path: &Path, root: &Path, id: &str) -> Result<Preset, String> {
    let presets = list(path)?;
    let Some(p) = presets.iter().find(|p| p.id == id) else {
        return Err(format!("unknown robot preset `{id}` in {}; embedded presets: {}", path.display(), presets.iter().filter(|p| p.mode == "embedded").map(|p| p.id.as_str()).collect::<Vec<_>>().join(", ")));
    };
    if p.mode != "embedded" {
        return Err(format!("robot preset `{id}` has mode `{}`; only mode `embedded` (a scene + controller config run by EmbeddedSession/EmbeddedEnvironment) runs natively", p.mode));
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
        fn read<T: serde::de::DeserializeOwned>(root: &Path, rel: &str) -> Result<(T, Value), String> {
            let path = root.join(rel);
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let raw: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            let parsed = T::deserialize(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok((parsed, raw))
        }
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
