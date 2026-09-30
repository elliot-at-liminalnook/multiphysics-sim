//! Saved robot-preset recordings. The run thread snapshots the shared
//! recording (`EmbeddedEnvironment::episode_recording()` for a preset with a
//! task, `EmbeddedSession::recording()` without one: the same objects the
//! browser worker's `recording` request returns and its Download button
//! saves) and a writer thread serialises it, compact, into `<stem>.json`,
//! unchanged, so the file can be given straight to `prepare_replay` (both
//! types are `deny_unknown_fields`). The viewer's metadata (preset, time,
//! seed, note, viewer version, replayability) goes into a sidecar
//! `<stem>.meta.json`. Files are created, never overwritten.
use serde_json::{Value, json};
use sim_runtime::embedded::EmbeddedRecording;
use sim_runtime::environment::EnvironmentRecording;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// Where saves go by default, relative to the repository root.
pub const DIR: &str = "runs/robot-presets";
pub const LOCATION_RULE: &str = "default runs/robot-presets/<preset-id>/<UTC yyyymmddThhmmss.mmmZ>.json under the repository root, which is the directory the preset's declared paths were resolved against (the launch directory holding web/viewer/presets.json; runs/ is git-ignored). An explicit path is resolved against the same root, `..` removed; it must end in .json (not .meta.json) and must not lie under examples/, cad/ or web/ (checked again with its parent directory canonicalised before writing).";
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
}
impl Snapshot {
    pub fn kind(&self) -> &str {
        match self {
            Snapshot::Environment(r) => &r.kind,
            Snapshot::Session(r) => &r.kind,
        }
    }
    pub fn version(&self) -> u32 {
        match self {
            Snapshot::Environment(r) => r.version,
            Snapshot::Session(r) => r.version,
        }
    }
    fn runtime(&self) -> &EmbeddedRecording {
        match self {
            Snapshot::Environment(r) => &r.runtime,
            Snapshot::Session(r) => r,
        }
    }
    pub fn completed_steps(&self) -> usize {
        self.runtime().completed_steps
    }
    /// The recorded error or failure, if any.
    pub fn failure(&self) -> Option<&str> {
        match self {
            Snapshot::Environment(r) => r.error.as_deref().or(r.runtime.failure.as_deref()),
            Snapshot::Session(r) => r.failure.as_deref(),
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
        }
    }
    /// The shared recording JSON, compact.
    pub fn to_json(&self) -> Result<String, String> {
        match self {
            Snapshot::Environment(r) => serde_json::to_string(r),
            Snapshot::Session(r) => serde_json::to_string(r),
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
            return Err(format!("refused: {} is under {dir}/ ({}); recordings are run outputs and are never written under examples/, cad/ or web/ (default {DIR}/<preset-id>/)", path.display(), root.join(dir).display()));
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
    let path = match explicit {
        None => root.join(DIR).join(preset_id).join(format!("{}.json", stamp(unix_ms))),
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
pub fn meta(snapshot: &Snapshot, run: &crate::robot_preset::PresetRun, recording_file: &Path, note: Option<&str>, unix_ms: u128, generation: u64, chunks: u64, final_frame: Option<Value>) -> Value {
    let p = &run.preset;
    let replayable = snapshot.replayable();
    let identity = snapshot.runtime().runtime_identity.as_ref();
    json!({"schema": META_SCHEMA, "schema_version": META_VERSION,
        "recording_file": recording_file.file_name().map(|n| n.to_string_lossy()), "recording_kind": snapshot.kind(), "recording_version": snapshot.version(),
        "recording_rule": FILE_RULE,
        "completed_steps": snapshot.completed_steps(), "sim_time_s": snapshot.completed_steps() as f64 * run.config.step_s, "requested_steps": run.config.steps,
        "replayable": replayable.is_ok(), "not_replayable_reason": replayable.err(), "failure": snapshot.failure(), "replayable_rule": REPLAYABLE_RULE,
        "runtime_identity": identity, "runtime_identity_note": "copied from the recording's runtime_identity (library sources/features); binary and host attestations are not included",
        "preset": {"id": p.id, "label": p.label, "mode": p.mode, "scene": p.scene, "config": p.config, "task": p.task, "presets_file": crate::robot_preset::PRESETS, "runs_as": run.kind()},
        "root": run.root, "seed": run.seed, "seed_rule": crate::robot_preset::SEED_RULE, "note": note,
        "saved_utc": iso(unix_ms), "viewer": {"crate": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION")},
        "run": {"generation": generation, "chunks": chunks},
        "final_frame": final_frame, "final_frame_note": "measured at save from the session's interactive_frame(): link frames at their com, model frame; a reference for comparison, not part of the recording or of any replay verdict"})
}
