//! Saved robot-preset recordings. The run thread snapshots the shared
//! recording (`EmbeddedEnvironment::episode_recording()` for a preset with a
//! task, `EmbeddedSession::recording()` without one: the same objects the
//! browser worker's `recording` request returns and its Download button
//! saves) and a writer thread serialises it, compact, into `<stem>.json`,
//! unchanged, so the file can be given straight to `prepare_replay` (both
//! types are `deny_unknown_fields`). The viewer's metadata (preset, time,
//! seed, note, viewer version, replayability) goes into a sidecar
//! `<stem>.meta.json`. Files are created, never overwritten.
//!
//! A `--robot FILE` drive session (`run::ControlledRun`) saves the shared
//! `sim_runtime::session::Session::recording()` (scene with the external
//! program's identity, seed and one `[forward, lateral, yaw, heartbeat]`
//! action per seam period) the same way, under `runs/robot-drive/<model
//! stem>/<stamp>.recording.json`, with a sidecar naming the controller.
use serde_json::{Value, json};
use sim_runtime::embedded::EmbeddedRecording;
use sim_runtime::environment::EnvironmentRecording;
use sim_runtime::session::Recording;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// Where saves go by default, relative to the workspace root (`crate::workspace`), joined to it at every use.
pub const DIR: &str = "runs/robot-presets";
pub const LOCATION_RULE: &str = "default runs/robot-presets/<preset-id>/<UTC yyyymmddThhmmss.mmmZ>.json under the workspace root, which is the directory the preset's declared paths were resolved against (--workspace, $SIM_WORKSPACE, else the nearest ancestor of the opened file or launch directory holding a Cargo.toml [workspace] and library/, whatever the launch directory; runs/ is git-ignored). An explicit path is resolved against the same root, `..` removed; it must end in .json (not .meta.json) and must not lie under examples/, cad/ or web/ (checked again with its parent directory canonicalised before writing).";
pub const FILE_RULE: &str = "<stem>.json is the shared recording exactly as serde_json::to_string writes it (what sim-web's recording() returns to the browser worker, whose Download re-stringifies the same object), so prepare_replay reads it unchanged; <stem>.meta.json is the viewer's sidecar. Both are created with create_new: an existing file is never overwritten, and if the sidecar cannot be created the just-created recording is removed and the error names both.";
pub const REPLAYABLE_RULE: &str = "as the shared runtime decides: an environment episode whose error (or runtime failure) is set is a diagnostic record that EmbeddedEnvironment::prepare_replay refuses (\"failed attempts remain diagnostic records\"), so it is saved labelled replayable=false with that reason; a version-3 session recording with a failure is replayable by EmbeddedSession::prepare_replay, which re-executes to the recorded failure and expects it again (replay_expected). A horizon or terminated episode is not a failure.";
/// The sidecar's own schema (the recording keeps the runtime's).
pub const META_SCHEMA: &str = "sim-spatial.robot-preset-recording-meta";
pub const META_VERSION: u32 = 1;
/// Directories under the root that saves never write into.
pub const PROTECTED: [&str; 3] = ["examples", "cad", "web"];

/// The shared recording, as the run thread took it.
pub enum Snapshot {
    Environment(EnvironmentRecording),
    Session(EmbeddedRecording),
    /// A drive session's `Session::recording()`; `failure` is the session's
    /// latched error (the recording itself has no failure field).
    Drive { recording: Recording, failure: Option<String> },
}
impl Snapshot {
    pub fn kind(&self) -> &str {
        match self {
            Snapshot::Environment(r) => &r.kind,
            Snapshot::Session(r) => &r.kind,
            Snapshot::Drive { .. } => DRIVE_KIND,
        }
    }
    pub fn version(&self) -> u32 {
        match self {
            Snapshot::Environment(r) => r.version,
            Snapshot::Session(r) => r.version,
            Snapshot::Drive { recording, .. } => recording.version,
        }
    }
    fn runtime(&self) -> Option<&EmbeddedRecording> {
        match self {
            Snapshot::Environment(r) => Some(&r.runtime),
            Snapshot::Session(r) => Some(r),
            Snapshot::Drive { .. } => None,
        }
    }
    /// Nominal steps (presets) or seam periods (one action each, a drive session).
    pub fn completed_steps(&self) -> usize {
        match self {
            Snapshot::Drive { recording, .. } => recording.actions.len(),
            _ => self.runtime().map_or(0, |r| r.completed_steps),
        }
    }
    /// The recorded error or failure, if any.
    pub fn failure(&self) -> Option<&str> {
        match self {
            Snapshot::Environment(r) => r.error.as_deref().or(r.runtime.failure.as_deref()),
            Snapshot::Session(r) => r.failure.as_deref(),
            Snapshot::Drive { failure, .. } => failure.as_deref(),
        }
    }
    /// `Err(reason)` when the runtime's prepare_replay refuses this kind of record (REPLAYABLE_RULE).
    pub fn replayable(&self) -> Result<(), String> {
        match self {
            Snapshot::Environment(_) => match self.failure() {
                Some(e) => Err(format!("diagnostic record: the episode failed ({e}); EmbeddedEnvironment::prepare_replay refuses it: only valid completed-transition environment prefixes can be replayed; failed attempts remain diagnostic records")),
                None => Ok(()),
            },
            Snapshot::Session(r) if r.version != 3 && r.failure.is_some() => Err(format!("recording version {} with a failure: EmbeddedSession::prepare_replay replays failures only from version 3", r.version)),
            Snapshot::Session(_) => Ok(()),
            Snapshot::Drive { failure: Some(e), .. } => Err(format!("the drive session had failed ({e}): its last recorded period ended in that error, so a replay re-executes to it and reports it as an error (Session has no expected-failure mode)")),
            Snapshot::Drive { .. } => Ok(()),
        }
    }
    /// The shared recording JSON, compact.
    pub fn to_json(&self) -> Result<String, String> {
        match self {
            Snapshot::Environment(r) => serde_json::to_string(r),
            Snapshot::Session(r) => serde_json::to_string(r),
            Snapshot::Drive { recording, .. } => serde_json::to_string(recording),
        }
        .map_err(|e| format!("serialising the {} recording: {e}", self.kind()))
    }
}

/// A saved pair, as robot_state.recording.last_saved reports it.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Saved {
    pub path: PathBuf,
    pub meta_path: PathBuf,
    pub kind: String,
    pub version: u32,
    pub completed_steps: usize,
    pub replayable: bool,
    pub not_replayable_reason: Option<String>,
    pub failure: Option<String>,
    pub saved_utc: String,
    pub bytes: usize,
}

/// `unix_ms` as a file stamp: `yyyymmddThhmmss.mmmZ` (UTC).
pub fn stamp(unix_ms: u128) -> String {
    let digits: String = crate::builder::gait_lab::utc((unix_ms / 1000) as u64).chars().filter(char::is_ascii_digit).collect();
    format!("{}T{}.{:03}Z", &digits[..8], &digits[8..], unix_ms % 1000)
}
pub fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()
}
/// `unix_ms` as ISO 8601 UTC with milliseconds.
pub fn iso(unix_ms: u128) -> String {
    let s = crate::builder::gait_lab::utc((unix_ms / 1000) as u64);
    format!("{}T{}.{:03}Z", &s[..10], &s[11..19], unix_ms % 1000)
}

/// `path` made absolute against `root` with `.`/`..` removed (no file-system access).
fn lexical(root: &Path, path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in root.join(path).components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}
fn protected(root: &Path, path: &Path) -> Result<(), String> {
    let root = lexical(root, Path::new(""));
    for dir in PROTECTED {
        if path.starts_with(root.join(dir)) {
            return Err(format!("refused: {} is under {dir}/ ({}); recordings are run outputs and are never written under examples/, cad/ or web/ (default {DIR}/<preset-id>/, or {DRIVE_DIR}/<model stem>/ for a drive session)", path.display(), root.join(dir).display()));
        }
    }
    Ok(())
}
/// The sidecar path for a recording path (`x.json` → `x.meta.json`).
pub fn meta_path(path: &Path) -> PathBuf {
    path.with_extension("meta.json")
}

/// Where a save goes (LOCATION_RULE), refused naming the reason. No file-system access: safe on the UI thread.
pub fn target(root: &Path, preset_id: &str, explicit: Option<&str>, unix_ms: u128) -> Result<PathBuf, String> {
    target_in(root, &Path::new(DIR).join(preset_id), ".json", explicit, unix_ms)
}
/// A drive session's save target (DRIVE_LOCATION_RULE): `<root>/runs/robot-drive/<stem>/<stamp>.recording.json`, or an explicit path under the same rules as a preset's.
pub fn drive_target(root: &Path, model_stem: &str, explicit: Option<&str>, unix_ms: u128) -> Result<PathBuf, String> {
    target_in(root, &Path::new(DRIVE_DIR).join(model_stem), DRIVE_SUFFIX, explicit, unix_ms)
}
fn target_in(root: &Path, dir: &Path, suffix: &str, explicit: Option<&str>, unix_ms: u128) -> Result<PathBuf, String> {
    let path = match explicit {
        None => root.join(dir).join(format!("{}{suffix}", stamp(unix_ms))),
        Some(p) if p.trim().is_empty() => return Err("refused: an explicit recording path is empty".into()),
        Some(p) => lexical(root, Path::new(p)),
    };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !name.ends_with(".json") || name.ends_with(".meta.json") {
        return Err(format!("refused: {} must end in .json (not .meta.json, which names the sidecar)", path.display()));
    }
    protected(root, &path)?;
    Ok(path)
}

/// Writes the pair (writer thread). Re-checks the protected directories with
/// the parent canonicalised; never overwrites either file.
pub fn write(root: &Path, path: &Path, snapshot: &Snapshot, meta: Value) -> Result<Saved, String> {
    let saved_utc = meta["saved_utc"].as_str().unwrap_or_default().to_string();
    let body = snapshot.to_json()?;
    let parent = path.parent().ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    // Symlinks: canonicalise the nearest existing ancestor and check again before creating anything.
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let existing = parent.ancestors().find(|a| a.exists()).ok_or_else(|| format!("{} has no existing ancestor", path.display()))?;
    let rest = path.strip_prefix(existing).unwrap_or(path);
    let canonical = existing.canonicalize().map_err(|e| format!("{}: {e}", existing.display()))?.join(rest);
    protected(&canonical_root, &canonical)?;
    std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    let meta_path = meta_path(path);
    for p in [path, meta_path.as_path()] {
        if p.exists() {
            return Err(format!("refused: {} already exists; recordings are never overwritten", p.display()));
        }
    }
    let create = |p: &Path, text: &str| -> Result<(), String> {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(p).map_err(|e| format!("{}: {e} (never overwritten)", p.display()))?;
        f.write_all(text.as_bytes()).and_then(|_| f.sync_all()).map_err(|e| format!("writing {}: {e}", p.display()))
    };
    create(path, &body)?;
    let replayable = snapshot.replayable();
    let mut meta = meta;
    meta["recording_bytes"] = json!(body.len());
    let meta_text = serde_json::to_string_pretty(&meta).map_err(|e| format!("serialising the sidecar: {e}"))?;
    if let Err(e) = create(&meta_path, &meta_text) {
        // Only the file this save just created is removed; nothing else is touched.
        let removed = std::fs::remove_file(path).map_or_else(|r| format!("could not remove it ({r})"), |_| "removed it".into());
        return Err(format!("sidecar {} not written: {e}; the recording {} this save had just created: {removed}", meta_path.display(), path.display()));
    }
    Ok(Saved { path: path.to_path_buf(), meta_path, kind: snapshot.kind().into(), version: snapshot.version(), completed_steps: snapshot.completed_steps(),
        replayable: replayable.is_ok(), not_replayable_reason: replayable.err(), failure: snapshot.failure().map(str::to_string), saved_utc, bytes: body.len() })
}

/// The sidecar (`<stem>.meta.json`): what the recording itself does not say.
#[allow(clippy::too_many_arguments)]
pub fn meta(snapshot: &Snapshot, run: &crate::robot::preset::PresetRun, recording_file: &Path, note: Option<&str>, unix_ms: u128, generation: u64, chunks: u64, final_frame: Option<Value>) -> Value {
    let p = &run.preset;
    let replayable = snapshot.replayable();
    let identity = snapshot.runtime().and_then(|r| r.runtime_identity.as_ref());
    json!({"schema": META_SCHEMA, "schema_version": META_VERSION,
        "recording_file": recording_file.file_name().map(|n| n.to_string_lossy()), "recording_kind": snapshot.kind(), "recording_version": snapshot.version(),
        "recording_rule": FILE_RULE,
        "completed_steps": snapshot.completed_steps(), "sim_time_s": snapshot.completed_steps() as f64 * run.config.step_s, "requested_steps": run.config.steps,
        "replayable": replayable.is_ok(), "not_replayable_reason": replayable.err(), "failure": snapshot.failure(), "replayable_rule": REPLAYABLE_RULE,
        "runtime_identity": identity, "runtime_identity_note": "copied from the recording's runtime_identity (library sources/features); binary and host attestations are not included",
        "preset": {"id": p.id, "label": p.label, "mode": p.mode, "scene": p.scene, "config": p.config, "task": p.task, "presets_file": crate::robot::preset::PRESETS, "runs_as": run.kind()},
        "root": run.root, "seed": run.seed, "seed_rule": crate::robot::preset::SEED_RULE, "note": note,
        "saved_utc": iso(unix_ms), "viewer": {"crate": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION")},
        "run": {"generation": generation, "chunks": chunks},
        "final_frame": final_frame, "final_frame_note": "measured at save from the session's interactive_frame(): link frames at their com, model frame; a reference for comparison, not part of the recording or of any replay verdict"})
}

pub const REPLAY_RULE: &str = "replay re-executes a saved recording through the shared runtime on the run thread: the file is parsed as the shared type of the loaded preset's kind (a kind mismatch is refused naming both), prepared with EmbeddedEnvironment::prepare_replay (task) or EmbeddedSession::prepare_replay(record, CaptureMode::Latest) (no task), then advanced in bounded chunks (one returned action per chunk for an environment, the preset's chunk of nominal steps for a session), paced at most to real time like a run, publishing a new generation's frames and completed/total after every chunk; Cancel is checked between chunks. A replay needs a paused (or idle, ended or failed) run and replaces it; a refused replay leaves the current run as it was. During a replay Run, Pause, Step, motion requests, Save and a second replay are refused naming the replay; Cancel and Reset always work. After done the replayed simulation is the current paused run (Run, Step, Save and motion work again, as in the browser); after cancelled it is a partial replay, so Run, Step, motion and Save are refused until Reset (a fresh run) or another replay; after failed the run is failed as the runtime latched it (Reset).";
pub const VERDICT_RULE: &str = "the verdict is what the shared runtime establishes, nothing more. Environment (EmbeddedEnvironment::prepare_replay): it verifies the record's version/kind, that it holds no error or runtime failure, that completed_steps is within the horizon and a whole number of action intervals, that task, scene and config fingerprints equal the loaded environment's (else \"replay must match loaded robot, controller and task\", shown verbatim), and that the recorded input schedule is valid for the loaded input channels; it then rebuilds from the recording's seed and the host steps every returned action through EmbeddedEnvironment::step, whose errors are reported verbatim. done = every returned action stepped without error, error() is None afterwards and the replayed transition's completed_steps equals the recording's. It does NOT compare the replayed states, observations, rewards or termination with the original episode (they are not in the record). Session (EmbeddedSession::prepare_replay): it verifies version, kind, step count and input-event order, rebuilds from the recording's own scene/config/seed and sets replay_expected, so advance re-applies each recorded input event at its step and errors \"replay failure mismatch …\" if a failure occurs that was not recorded (or at another step or with another message) and \"recorded failure did not reproduce\" if a recorded one does not. done = the returned steps were all advanced with no runtime error (or, for a recorded failure, the runtime returned exactly the recorded failure at the recorded step) and completed_steps equals the recording's. It does NOT compare states; and it does not compare the recording with the loaded preset, so the viewer does (IDENTITY_RULE). failed = the runtime's error verbatim.";
pub const IDENTITY_RULE: &str = "viewer identity check (session presets only, the same check sim-web's EmbeddedSimulation.prepare_replay makes): the recording's scene and config must equal the loaded preset's, compared by sim_runtime::physics_context::fingerprint of their JSON values; EmbeddedSession::prepare_replay itself rebuilds from the recording's own scene and does not compare it with the loaded one, so without this check a recording of another preset would replay a different robot in this window. Environment presets need no viewer check: prepare_replay compares task, scene and config itself.";
pub const MEASURED_RULE: &str = "measured difference, not a pass criterion: when the sidecar holds final_frame (link poses at save), the replayed final frame's link positions are compared by link name (|Δp| in m, per link, max and the first link); never thresholded and never part of the verdict.";

/// A saved recording found for a preset, with a summary of its sidecar.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Listed {
    pub file: String,
    pub path: PathBuf,
    pub bytes: u64,
    /// kind, completed_steps, replayable, saved_utc, note (null without a readable sidecar).
    pub meta: Option<Value>,
    pub meta_error: Option<String>,
}

/// The recordings in `<root>/runs/robot-presets/<preset-id>/` (`*.json` but not `*.meta.json`), by file name (oldest first). Blocking: call off the UI thread.
pub fn list(root: &Path, preset_id: &str) -> Result<Vec<Listed>, String> {
    list_in(&root.join(DIR).join(preset_id))
}
/// A drive session's recordings: `<root>/runs/robot-drive/<model stem>/`, as [`list`]. Blocking: call off the UI thread.
pub fn list_drive(root: &Path, model_stem: &str) -> Result<Vec<Listed>, String> {
    list_in(&root.join(DRIVE_DIR).join(model_stem))
}
fn list_in(dir: &Path) -> Result<Vec<Listed>, String> {
    let dir = dir.to_path_buf();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let file = entry.file_name().to_string_lossy().into_owned();
        if !file.ends_with(".json") || file.ends_with(".meta.json") || !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        let (meta, meta_error) = match std::fs::read_to_string(meta_path(&path)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, Some("no sidecar".into())),
            Err(e) => (None, Some(format!("{}: {e}", meta_path(&path).display()))),
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(m) => (Some(json!({"kind": m["recording_kind"], "completed_steps": m["completed_steps"], "replayable": m["replayable"], "saved_utc": m["saved_utc"], "note": m["note"], "preset_id": m["preset"]["id"], "model_stem": m["model"]["stem"]})), None),
                Err(e) => (None, Some(format!("{}: {e}", meta_path(&path).display()))),
            },
        };
        out.push(Listed { file, bytes: entry.metadata().map(|m| m.len()).unwrap_or(0), path, meta, meta_error });
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(out)
}

/// The recording a replay request names: `file` in the preset's directory (a bare file name) or an explicit `path` (relative to the root or absolute; reading is allowed anywhere). No file-system access.
pub fn replay_source(root: &Path, preset_id: &str, file: Option<&str>, path: Option<&str>) -> Result<PathBuf, String> {
    replay_source_in(root, &Path::new(DIR).join(preset_id), file, path)
}
/// A drive session's recording to replay: `file` in `runs/robot-drive/<model stem>/`, or an explicit `path` (as [`replay_source`]).
pub fn drive_replay_source(root: &Path, model_stem: &str, file: Option<&str>, path: Option<&str>) -> Result<PathBuf, String> {
    replay_source_in(root, &Path::new(DRIVE_DIR).join(model_stem), file, path)
}
fn replay_source_in(root: &Path, dir: &Path, file: Option<&str>, path: Option<&str>) -> Result<PathBuf, String> {
    let p = match (file, path) {
        (Some(f), None) => {
            if f.is_empty() || f.contains(['/', '\\']) || f == "." || f == ".." {
                return Err(format!("replay file `{f}` must be a bare file name in {}/ (use path for another location)", dir.display()));
            }
            root.join(dir).join(f)
        }
        (None, Some(p)) if !p.trim().is_empty() => lexical(root, Path::new(p)),
        _ => return Err("replay needs exactly one of file (a saved recording of this preset or drive session) or path (any readable recording .json)".into()),
    };
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !name.ends_with(".json") || name.ends_with(".meta.json") {
        return Err(format!("{} is not a recording: it must end in .json (a .meta.json is the viewer's sidecar)", p.display()));
    }
    Ok(p)
}

/// MEASURED_RULE: per-link position differences between two final frames (`frame_json` shape), by link name.
pub fn measured(recorded: &Value, replayed: &Value) -> Option<Value> {
    let positions = |f: &Value| -> Vec<(String, [f64; 3])> {
        f["poses"].as_array().into_iter().flatten().filter_map(|p| {
            let a = p["position"].as_array()?;
            Some((p["link"].as_str()?.to_string(), [a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?]))
        }).collect()
    };
    let old = positions(recorded);
    let new = positions(replayed);
    let per: Vec<(String, f64)> = new.iter().filter_map(|(l, p)| old.iter().find(|(o, _)| o == l).map(|(_, q)| (l.clone(), ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()))).collect();
    let first = per.first()?.clone();
    let max = per.iter().cloned().fold(first.clone(), |m, x| if x.1 > m.1 { x } else { m });
    Some(json!({"label": "measured difference, not a pass criterion", "rule": MEASURED_RULE, "links_compared": per.len(),
        "max_position_diff_m": max.1, "max_link": max.0, "first_link": first.0, "first_link_position_diff_m": first.1,
        "recorded_completed_steps": recorded["completed_steps"], "replayed_completed_steps": replayed["completed_steps"],
        "per_link_m": per.iter().map(|(l, d)| json!({"link": l, "diff_m": d})).collect::<Vec<_>>()}))
}

/// Drive sessions (`run::ControlledRun`): where saves go, relative to the workspace root.
pub const DRIVE_DIR: &str = "runs/robot-drive";
/// A drive recording's file suffix (the sidecar is `<stamp>.recording.meta.json`).
pub const DRIVE_SUFFIX: &str = ".recording.json";
/// The viewer's kind name for a drive session recording (the shared `Recording` has no kind field).
pub const DRIVE_KIND: &str = "drive_session";
pub const DRIVE_META_SCHEMA: &str = "sim-spatial.robot-drive-recording-meta";
pub const DRIVE_LOCATION_RULE: &str = "default runs/robot-drive/<model stem>/<UTC yyyymmddThhmmss.mmmZ>.recording.json under the workspace root (<model stem>: the opened file name without .simrobot.json); an explicit path follows the preset rules (resolved against the root, `..` removed, must end in .json and not .meta.json, never under examples/, cad/ or web/). Written on a writer thread; never overwritten.";
pub const DRIVE_FILE_RULE: &str = "<stem>.recording.json is sim_runtime::session::Session::recording() exactly as serde_json::to_string writes it: version 1, the scene (robot, BuildOptions, period_s, duration_s and controller.external: the Python script, its sha256, args with the host's --drive-json resolved drive, clients_root, profile_sha256, library_sha256: the simloop library's hash, sim_runtime::session::library_sha256), the seed and one action per seam period [command.forward m/s, command.lateral m/s, command.yaw rad/s, command.heartbeat] (the limited twist and request sequence the run thread sent); <stem>.recording.meta.json is the viewer's sidecar (model path, binding, profile path and sha256, identity, seed, steps, label). Both created with create_new.";
pub const DRIVE_REPLAYABLE_RULE: &str = "a drive recording replays when its controller and robot match the loaded binding's (DRIVE_IDENTITY_RULE); a session that had failed is saved labelled replayable=false: its last action ends in the failure, which the replay reports as an error";
pub const DRIVE_REPLAY_RULE: &str = "replay of a drive session: the file is read on the run thread as sim_runtime::session::Recording (version 1), checked against the loaded binding (DRIVE_IDENTITY_RULE; any difference refuses it by name and the current run is unchanged), rebuilt with Session::new(recorded scene, recorded seed), which starts the recorded controller program again, and every recorded action is stepped through Session::step, one per chunk (one seam period), paced like a run; Cancel is checked between chunks. Live drive requests, Run, Pause, Step and Save are refused during it; Cancel and Reset always work.";
pub const DRIVE_VERDICT_RULE: &str = "done = identity matched, Session::new accepted the recorded scene and seed, and every recorded action stepped through Session::step without error (completed seam periods equal the recording's action count); states are not compared (measured compares the final link positions with the sidecar's final_frame when present, never as a pass criterion). failed = the first error verbatim.";
pub const DRIVE_IDENTITY_RULE: &str = "drive identity check: the recording's scene.controller.external and the loaded run's (both built by controller_binding::scene) must agree on language, script path, script sha256, simloop library sha256, args (without the host's --drive-json), drive profile path and sha256 and the resolved drive JSON, and the recorded robot (sim_runtime::physics_context::fingerprint of its JSON) and seam period must equal the loaded model's; each difference is named with both values. Session::new also re-verifies the script's and the library's sha256 on disk.";

/// A drive session's sidecar (`<stem>.recording.meta.json`): what the recording does not say.
#[allow(clippy::too_many_arguments)]
pub fn drive_meta(snapshot: &Snapshot, run: &crate::robot::run::ControlledRun, recording_file: &Path, note: Option<&str>, unix_ms: u128, generation: u64, chunks: u64, final_frame: Option<Value>) -> Value {
    let c = &run.controlled;
    let replayable = snapshot.replayable();
    let steps = snapshot.completed_steps();
    let mut meta = json!({"schema": DRIVE_META_SCHEMA, "schema_version": META_VERSION,
        "recording_file": recording_file.file_name().map(|n| n.to_string_lossy()), "recording_kind": snapshot.kind(), "recording_version": snapshot.version(),
        "recording_rule": DRIVE_FILE_RULE, "label": crate::robot::run::CONTROLLER_LABEL, "fidelity": crate::robot::run::DRIVE_FIDELITY,
        "completed_steps": steps, "period_s": run.scene.period_s, "sim_time_s": steps as f64 * run.scene.period_s, "duration_s": run.scene.duration_s,
        "replayable": replayable.is_ok(), "not_replayable_reason": replayable.err(), "failure": snapshot.failure(), "replayable_rule": DRIVE_REPLAYABLE_RULE});
    meta["model"] = json!({"path": run.model_path, "stem": run.stem()});
    meta["binding"] = json!({"path": c.binding_path});
    meta["profile"] = json!({"path": c.profile_path, "sha256": c.resolved.profile_sha256});
    meta["identity"] = serde_json::to_value(&c.identity).unwrap_or(Value::Null);
    meta["seed"] = json!(run.seed);
    meta["seed_rule"] = json!(crate::robot::run::SEED_RULE);
    meta["actions"] = json!({"channels": sim_runtime::controller_binding::COMMAND_CHANNELS, "units": ["m/s", "m/s", "rad/s", "requests"], "per": "seam period"});
    meta["root"] = json!(run.root.as_ref().ok());
    meta["note"] = json!(note);
    meta["saved_utc"] = json!(iso(unix_ms));
    meta["viewer"] = json!({"crate": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION")});
    meta["run"] = json!({"generation": generation, "chunks": chunks});
    meta["final_frame"] = json!(final_frame);
    meta["final_frame_note"] = json!("measured at save from the drive session's PhysicalRobot frame: link frames at their com, model frame; a reference for comparison, not part of the recording or of any replay verdict");
    meta
}
