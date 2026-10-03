//! Replay of a saved preset or drive-session recording: its state, preparation through the shared runtime and verdict.
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use crate::robot::recording;
use super::controlled::{ControlledRun, differences};
use sim_runtime::drive_host::DriveHost;
use super::frames::environment_names;
use super::protocol::{Drive, Source};
use super::sim::Sim;

#[derive(Clone, Copy, Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayPhase {
    Idle,
    Replaying,
    /// Stopped between chunks by Cancel: never a verdict.
    Cancelled,
    /// The shared runtime's replay checks passed (recording::VERDICT_RULE).
    Done,
    /// Refused before replacing the run, or the runtime reported an error.
    Failed,
}

/// `robot_state.replay`: one replay request and what the run thread made of it.
#[derive(Clone, Debug, Serialize)]
pub struct ReplayState {
    /// Request number (0 before any replay).
    pub seq: u64,
    /// Generation of the replayed run (a refused replay republishes the current run under it).
    pub generation: u64,
    pub path: Option<std::path::PathBuf>,
    pub phase: ReplayPhase,
    /// Progress in `unit`: returned actions (environment) or nominal steps (session).
    pub completed: u64,
    pub total: Option<u64>,
    pub unit: Option<&'static str>,
    /// The replayed runtime's own completed_steps, and the recording's.
    pub completed_steps: Option<u64>,
    pub recorded_completed_steps: Option<u64>,
    pub recorded_failure: Option<String>,
    pub kind: Option<String>,
    pub verdict: Option<String>,
    pub error: Option<String>,
    /// Whether the current run is this replay's simulation (false when refused before replacing it).
    pub replaced: bool,
    pub cancel_requested: bool,
    /// recording::MEASURED_RULE (null without a sidecar final_frame).
    pub measured: Option<Value>,
    /// The sidecar's preset id and saved time, when it exists.
    pub sidecar: Option<Value>,
    /// Wall time from the prepared replay to its end (s).
    pub wall_s: Option<f64>,
}
impl ReplayState {
    pub(super) fn new(seq: u64, generation: u64, path: Option<std::path::PathBuf>, phase: ReplayPhase) -> Self {
        Self { seq, generation, path, phase, completed: 0, total: None, unit: None, completed_steps: None, recorded_completed_steps: None, recorded_failure: None,
            kind: None, verdict: None, error: None, replaced: false, cancel_requested: false, measured: None, sidecar: None, wall_s: None }
    }
    pub(super) fn file(&self) -> String {
        self.path.as_ref().and_then(|p| p.file_name()).map_or("the recording".into(), |n| n.to_string_lossy().into_owned())
    }
    pub(super) fn progress(&self) -> String {
        format!("{}/{} {}", self.completed, self.total.map_or("?".into(), |t| t.to_string()), self.unit.unwrap_or("units"))
    }
}

/// What a prepared replay still has to advance: the actions
/// `EmbeddedEnvironment::prepare_replay` returned, or the steps
/// `EmbeddedSession::prepare_replay` returned.
pub(super) enum ReplayWork {
    Actions(VecDeque<Vec<f64>>),
    Steps { remaining: usize },
    /// A drive session's recorded actions, one per seam period, stepped through `Session::step`.
    Drive(VecDeque<Vec<f64>>),
}
impl ReplayWork {
    pub(super) fn done(&self) -> bool {
        match self {
            ReplayWork::Actions(q) | ReplayWork::Drive(q) => q.is_empty(),
            ReplayWork::Steps { remaining } => *remaining == 0,
        }
    }
}
/// A replay the run thread is advancing.
pub(super) struct ActiveReplay {
    pub(super) state: ReplayState,
    pub(super) work: ReplayWork,
    pub(super) final_frame: Option<Value>,
    pub(super) started: Instant,
}

/// Reads `path` and prepares it through the shared runtime against the loaded
/// preset (recording::REPLAY_RULE): the replacement simulation, its work
/// and the state so far. Refusals name the reason; runtime refusals are verbatim.
pub(super) fn prepare_replay(source: &Source, current: Option<&Sim>, path: &std::path::Path, mut state: ReplayState) -> Result<(Sim, ActiveReplay), (String, ReplayState)> {
    use sim_runtime::embedded::{CaptureMode, EmbeddedRecording, EmbeddedSession};
    use sim_runtime::environment::{EmbeddedEnvironment, EnvironmentRecording};
    use sim_runtime::physics_context::fingerprint;
    if let Source::Controlled(run) = source {
        return prepare_drive_replay(run, path, state);
    }
    let Source::Preset(run) = source else {
        return Err(("replay is for robot presets; `--robot FILE` runs PhysicalRobot, which has no recording or replay".into(), state));
    };
    let id = &run.preset.id;
    macro_rules! tryr {
        ($e:expr) => {
            match $e {
                Ok(x) => x,
                Err(e) => return Err((e, state)),
            }
        };
    }
    let text = tryr!(std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display())));
    let value: Value = tryr!(serde_json::from_str(&text).map_err(|e| format!("{}: not JSON: {e}", path.display())));
    let kind = value.get("kind").and_then(Value::as_str).unwrap_or("(no kind)").to_string();
    let expected = if run.task.is_some() { "sampled_environment_recording" } else { "embedded_session" };
    state.kind = Some(kind.clone());
    if kind != expected {
        return Err((format!("refused: {} is a `{kind}` recording, but preset `{id}` runs {} and replays `{expected}` recordings (the kind its Save and the browser's Download write)", path.display(), run.kind()), state));
    }
    let sidecar: Option<Value> = std::fs::read_to_string(recording::meta_path(path)).ok().and_then(|t| serde_json::from_str(&t).ok());
    let final_frame = sidecar.as_ref().map(|m| m["final_frame"].clone()).filter(|f| !f.is_null());
    state.sidecar = sidecar.as_ref().map(|m| json!({"path": recording::meta_path(path), "preset_id": m["preset"]["id"], "saved_utc": m["saved_utc"], "note": m["note"], "has_final_frame": final_frame.is_some()}));
    match &run.task {
        Some(task) => {
            let record: EnvironmentRecording = tryr!(serde_json::from_value(value).map_err(|e| format!("{}: not a shared EnvironmentRecording: {e}", path.display())));
            state.recorded_completed_steps = Some(record.runtime.completed_steps as u64);
            state.recorded_failure = record.error.clone().or(record.runtime.failure.clone());
            // prepare_replay compares the record with this (loaded) environment; build one if none is built yet.
            let built;
            let env = match current {
                Some(Sim::Environment { env, .. }) => env,
                _ => {
                    built = tryr!(EmbeddedEnvironment::new(run.scene.clone(), run.config.clone(), task.clone(), run.seed).map_err(|e| format!("building preset `{id}` to check the recording against: {e}")));
                    &built
                }
            };
            let (next, actions) = tryr!(env.prepare_replay(record).map_err(|e| format!("EmbeddedEnvironment::prepare_replay refused it: {e}")));
            let held = next.inputs().iter().map(|c| c.initial).collect();
            let drive = Arc::new(tryr!(Drive::resolve(run, &next.metadata()["policy_contract"], next.inputs())));
            state.total = Some(actions.len() as u64);
            state.unit = Some("actions");
            let names = environment_names(&next);
            Ok((Sim::Environment { env: next, held, run: run.clone(), drive, names }, ActiveReplay { state, work: ReplayWork::Actions(actions.into()), final_frame, started: Instant::now() }))
        }
        None => {
            let record: EmbeddedRecording = tryr!(serde_json::from_value(value).map_err(|e| format!("{}: not a shared EmbeddedRecording: {e}", path.display())));
            state.recorded_completed_steps = Some(record.completed_steps as u64);
            state.recorded_failure = record.failure.clone();
            // recording::IDENTITY_RULE: EmbeddedSession::prepare_replay does not compare with the loaded preset.
            for (what, a, b) in [("scene", json!(record.scene), json!(run.scene)), ("config (controller recipe)", json!(record.config), json!(run.config))] {
                if fingerprint(&a) != fingerprint(&b) {
                    return Err((format!("refused by the viewer identity check (as sim-web's): the recording's {what} differs from preset `{id}`'s; replay must match the loaded scene and controller recipe; load another preset to change them"), state));
                }
            }
            let (session, steps) = tryr!(EmbeddedSession::prepare_replay(record, CaptureMode::Latest).map_err(|e| format!("EmbeddedSession::prepare_replay refused it: {e}")));
            let drive = Arc::new(tryr!(Drive::resolve(run, &session.policy_metadata(), session.inputs())));
            state.total = Some(steps as u64);
            state.unit = Some("nominal steps");
            let names = Arc::new(session.coordinate_names().to_vec());
            Ok((Sim::Session { session, run: run.clone(), drive, names }, ActiveReplay { state, work: ReplayWork::Steps { remaining: steps }, final_frame, started: Instant::now() }))
        }
    }
}

/// Reads `path` as a drive session's `sim_runtime::session::Recording`,
/// refuses it by name when its controller or robot differs from the loaded
/// binding's (`controlled::differences`), else rebuilds the drive host
/// (`DriveHost::new`: `Session::new(recorded scene, recorded seed)`, which
/// starts the controller again, with the loaded binding's limits, whose
/// identity the recording matched) with the recorded actions to step
/// (recording::DRIVE_REPLAY_RULE).
fn prepare_drive_replay(run: &Arc<ControlledRun>, path: &std::path::Path, mut state: ReplayState) -> Result<(Sim, ActiveReplay), (String, ReplayState)> {
    use sim_runtime::session::Recording;
    macro_rules! tryr {
        ($e:expr) => {
            match $e {
                Ok(x) => x,
                Err(e) => return Err((e, state)),
            }
        };
    }
    state.kind = Some(recording::DRIVE_KIND.into());
    let text = tryr!(std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display())));
    let record: Recording = tryr!(serde_json::from_str(&text).map_err(|e| format!("{}: not a drive session recording (sim_runtime::session::Recording): {e}", path.display())));
    if record.version != 1 {
        return Err((format!("refused: {} is a drive session recording version {}; this build replays version 1 (Session::replay)", path.display(), record.version), state));
    }
    let sidecar: Option<Value> = std::fs::read_to_string(recording::meta_path(path)).ok().and_then(|t| serde_json::from_str(&t).ok());
    let final_frame = sidecar.as_ref().map(|m| m["final_frame"].clone()).filter(|f| !f.is_null());
    state.sidecar = sidecar.as_ref().map(|m| json!({"path": recording::meta_path(path), "model": m["model"], "identity": m["identity"], "saved_utc": m["saved_utc"], "note": m["note"], "has_final_frame": final_frame.is_some()}));
    state.recorded_failure = sidecar.as_ref().and_then(|m| m["failure"].as_str()).map(str::to_string);
    state.recorded_completed_steps = Some(record.actions.len() as u64);
    let differ = differences(run, &record.scene);
    if !differ.is_empty() {
        return Err((format!("refused by the drive identity check: {} was recorded with a different controller or robot than {} and its binding {}: {}; a replay must run the loaded robot's controller (restore the recorded script/profile or open the recorded model to replay it)",
            path.display(), run.model_path.display(), run.controlled.binding_path.display(), differ.join("; ")), state));
    }
    let seed = record.seed;
    let host = tryr!(DriveHost::new(record.scene, seed, &run.controlled).map_err(|e| format!("the recorded drive session could not be rebuilt (Session::new with the recorded scene and seed {seed}, then its command inputs): {e}")));
    state.total = Some(record.actions.len() as u64);
    state.unit = Some("seam periods");
    Ok((Sim::Controlled { host, run: run.clone() }, ActiveReplay { state, work: ReplayWork::Drive(record.actions.into()), final_frame, started: Instant::now() }))
}

/// The verdict once the work is exhausted (`error` None) or the runtime
/// returned an error (recording::VERDICT_RULE): only what it establishes.
/// A drive replay's recorded requests stop here (`Sim::end_drive_replay`).
pub(super) fn finish_replay(sim: &mut Sim, r: &mut ActiveReplay, error: Option<String>) {
    sim.end_drive_replay();
    let sim = &*sim;
    let s = &mut r.state;
    s.completed_steps = sim.completed_steps();
    s.wall_s = Some(r.started.elapsed().as_secs_f64());
    let (done, recorded) = (s.completed_steps, s.recorded_completed_steps);
    let steps_match = done.is_some() && done == recorded;
    let counts = format!("completed_steps {} (recorded {})", done.map_or("?".into(), |n| n.to_string()), recorded.map_or("?".into(), |n| n.to_string()));
    let (phase, verdict, err) = match (sim, error) {
        (Sim::Controlled { .. }, None) if steps_match => (ReplayPhase::Done, format!("passed the drive replay checks: the recording's controller (script, its sha256, the simloop library's sha256, args, profile and its sha256, the resolved drive) and robot matched the loaded binding's, Session::new rebuilt the recorded scene with the recorded seed and every recorded action stepped through Session::step without error; {counts}; states are not compared (see measured)"), None),
        (Sim::Controlled { .. }, None) => (ReplayPhase::Failed, format!("failed: {counts} differ"), Some(format!("replayed {counts}"))),
        (Sim::Controlled { .. }, Some(e)) => (ReplayPhase::Failed, format!("failed: Session::step returned an error during the replay{}", s.recorded_failure.as_ref().map_or(String::new(), |f| format!(" (the saved run had failed: {f})"))), Some(e)),
        (Sim::Environment { env, .. }, None) => match env.error() {
            Some(e) => (ReplayPhase::Failed, "failed: the environment reported an error after the replay".to_string(), Some(e.to_string())),
            None if steps_match => (ReplayPhase::Done, format!("passed the shared runtime's replay checks: EmbeddedEnvironment::prepare_replay accepted the recording against the loaded task, scene and config, and all {} returned actions stepped through EmbeddedEnvironment::step without error; {counts}; states are not compared by the runtime", s.completed), None),
            None => (ReplayPhase::Failed, format!("failed: {counts} differ"), Some(format!("replayed {counts}"))),
        },
        (Sim::Environment { .. }, Some(e)) => (ReplayPhase::Failed, "failed: EmbeddedEnvironment::step returned an error during the replay".into(), Some(e)),
        (_, None) if steps_match && s.recorded_failure.is_none() => (ReplayPhase::Done, format!("passed the shared runtime's replay checks: EmbeddedSession::prepare_replay rebuilt the recording (after the viewer identity check) and every returned step advanced with replay_expected satisfied (recorded inputs re-applied at their steps, no failure); {counts}; states are not compared by the runtime"), None),
        (_, None) => (ReplayPhase::Failed, format!("failed: {counts}{}", s.recorded_failure.as_ref().map_or(String::new(), |f| format!("; the recorded failure `{f}` did not occur"))), Some(format!("replayed {counts}"))),
        (_, Some(e)) if s.recorded_failure.as_deref() == Some(e.as_str()) && steps_match => (ReplayPhase::Done, format!("passed the shared runtime's replay checks: the recorded failure reproduced with the same message at the same step (replay_expected); {counts}; the run is failed as recorded"), None),
        (_, Some(e)) => (ReplayPhase::Failed, "failed: the shared runtime returned an error during the replay".into(), Some(e)),
    };
    s.phase = phase;
    s.verdict = Some(verdict);
    s.error = err;
}
