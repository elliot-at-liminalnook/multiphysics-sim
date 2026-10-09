//! The run thread's protocol: phases, what it builds, its commands and what it publishes.
use serde::Serialize;
use serde_json::Value;
use sim_domain_robot::PhysicalModel;
use sim_runtime::session::InputChannel;
use std::sync::Arc;
use crate::robot::motion::{self, Motion, MotionChannel};
use crate::robot::preset::PresetRun;
use crate::robot::recording::Saved;
use sim_domain_control::drive::kinematics::BodyTwist;
use super::controlled::ControlledRun;
use sim_runtime::drive_host::DriveStatus;
use super::{CHUNK_S, Frame, OverlayFlags, ReplayState};

#[derive(Clone, Copy, Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Nothing built yet; the static assembly pose is shown.
    Idle,
    Building,
    Running,
    Paused,
    /// A build or advance error stopped the run; Reset rebuilds.
    Failed,
    /// A preset reached its horizon, or its episode terminated/truncated
    /// (`run.end` says which and why); Reset rebuilds.
    Ended,
}

/// What the run thread builds: the loaded file, a preset's parsed inputs, or
/// a file with its controller binding.
#[derive(Clone)]
pub enum Source {
    Robot(PhysicalModel),
    Preset(Arc<PresetRun>),
    /// A `--robot FILE` whose controller binding loaded: a `Session` driven by its external controller.
    Controlled(Arc<ControlledRun>),
    /// A `--robot FILE` whose controller binding exists but did not load:
    /// every build fails with `error` ("controller binding <path>: …"); it
    /// never falls back to the file's hold run.
    Unbound { model: PhysicalModel, error: String },
    /// A `--robot FILE` that belongs to a robot project: the project's
    /// system runs (`sim_runtime::teleop::compose_robot`), its controllers
    /// as they are, the robot measured inside it.
    Composed(Arc<ComposedRun>),
}

/// A robot project's system as Robot mode runs it (read by the loader, off
/// the UI thread): the files, the document, the jog plan (which joints a
/// person may move, and why the others cannot be) and the drive link a
/// drive input block in the system is bound to.
pub struct ComposedRun {
    pub project: std::path::PathBuf,
    pub system: std::path::PathBuf,
    pub base: std::path::PathBuf,
    pub document: sim_system::SystemDocument,
    pub robot_instance: String,
    pub jog: sim_runtime::teleop::JogPlan,
    pub drive: sim_runtime::teleop::DriveLink,
}
impl ComposedRun {
    /// `robot_state.composed`.
    pub fn json(&self) -> Value {
        serde_json::json!({
            "project": self.project, "system": self.system, "robot": self.robot_instance,
            "jog": self.jog,
            "drive": {"bound": self.drive.bound(), "status": self.drive.status().map(|s| s.json())},
            "rule": "Robot mode runs the project's system: its controller blocks run as they are; a joint moves through its free servo target or its controller's free setpoint input, and is refused (jog.refused) when it has neither. A drive input block in the system follows robot_drive and the bound keys.",
        })
    }
}
impl Source {
    /// Sim time per chunk: the seam period for a drive session (one action per period).
    pub(super) fn chunk_s(&self) -> f64 {
        match self {
            Source::Robot(_) | Source::Unbound { .. } | Source::Composed(_) => CHUNK_S,
            Source::Preset(p) => p.chunk_s(),
            Source::Controlled(r) => r.scene.period_s,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Status {
    pub(super) phase: Phase,
    pub(super) generation: u64,
    pub(super) rtf: Option<f64>,
    pub(super) error: Option<String>,
    /// Why a preset run ended (phase `ended`).
    pub(super) end: Option<Value>,
}

/// What the worker publishes: its status and its latest frame.
pub(super) struct Published {
    pub(super) status: Status,
    pub(super) frame: Option<Frame>,
    /// Why the last jog could not be applied on the run thread (cleared by the next applied jog or a reset).
    pub(super) jog_error: Option<String>,
    /// A preset's typed inputs and motion config, resolved at build (None before a build and after Reset).
    pub(super) drive: Option<Arc<Drive>>,
    /// Why the last motion request could not be applied on the run thread.
    pub(super) motion_error: Option<String>,
    /// The last finished save (its request number, and the pair written or why not).
    pub(super) save: Option<(u64, Result<Saved, String>)>,
    /// The latest replay state (stamped with its request number and generation).
    pub(super) replay: Option<ReplayState>,
    /// A drive session's latest drive status (None before a controlled build and after Reset).
    pub(super) twist: Option<DriveStatus>,
    /// Why the last drive request could not be applied on the run thread.
    pub(super) twist_error: Option<String>,
}

/// A built preset's typed input channels (the session's `inputs()`), its
/// motion config (`motion::config`) and declared packet heartbeat.
#[derive(Debug)]
pub struct Drive {
    pub inputs: Vec<InputChannel>,
    pub motion: Option<Motion>,
    pub heartbeat: Option<MotionChannel>,
    /// The session's metadata as the browser worker returns it at load
    /// (`coordinate_names`, `joint_indices`, `policy_contract`, and an
    /// environment's `environment_contract`): what its readouts read.
    pub metadata: Value,
}
impl Drive {
    /// As the browser does on load: an invalid declaration fails the build, naming it.
    pub(super) fn resolve(run: &PresetRun, metadata: Value, inputs: &[InputChannel]) -> Result<Self, String> {
        let entry = &run.preset.entry;
        let (motion, heartbeat) = (motion::config(entry, &metadata["policy_contract"], inputs)?, motion::heartbeat(entry, inputs)?);
        Ok(Self { inputs: inputs.to_vec(), motion, heartbeat, metadata })
    }
    /// An embedded session's metadata, as sim-web's `EmbeddedSimulation.metadata()`.
    pub(super) fn session_metadata(s: &sim_runtime::embedded::EmbeddedSession) -> Value {
        let c = s.config();
        serde_json::json!({"coordinate_names": s.coordinate_names(), "joint_indices": s.joint_indices(), "step_s": c.step_s, "steps": c.steps, "report_every": c.report_every, "policy_contract": s.policy_metadata()})
    }
}

/// A motion request: the one handler behind physical W/A/S/D/X, the
/// `system_ui` motion:* controls and inspector buttons, and REST `robot_input`.
#[derive(Clone, Debug, Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MotionRequest {
    /// One key latched (system_ui / REST / inspector button) until Stop, another key or a channel request.
    Key(char),
    /// The physically held keys (press/release); empty requests zero.
    HeldKeys(Vec<char>),
    /// Every motion channel to zero.
    Stop,
    /// Explicit values by motion channel name, over the current request.
    Channels(std::collections::BTreeMap<String, f64>),
}

pub(super) enum Command {
    Start,
    Pause,
    Step,
    Reset { generation: u64 },
    Jog { joint: String, target: f64 },
    /// Validated motion values for the three motion channels (in `Motion::channels` order).
    Motion { values: [f64; 3] },
    /// Validated values for named session inputs (index, value), merged into
    /// the held action (the browser's input sliders, REST `robot_inputs`).
    Inputs { values: Vec<(usize, f64)> },
    /// Build the session now if none is built (a preset's open, as the
    /// browser worker builds at load), publishing its t = 0 frame paused.
    Prepare,
    /// A drive session's fresh twist request (checked against the profile on
    /// the UI thread): heartbeat + 1 at the current sim time; `halt` zeroes
    /// the request and the commanded twist at once.
    Twist { request: BodyTwist, halt: bool },
    /// Snapshot the shared recording and write it (on a writer thread) to `target`.
    SaveRecording { seq: u64, target: std::path::PathBuf, note: Option<String>, unix_ms: u128 },
    /// Read `path`, prepare it through the shared prepare_replay and advance it in chunks under `generation`.
    Replay { generation: u64, seq: u64, path: std::path::PathBuf },
    /// Stop the replay between chunks.
    CancelReplay,
    /// Which overlay data `--robot FILE` frames carry from now on (republished at once while paused).
    Overlays(OverlayFlags),
    /// The run speed scale (validated against SPEED_SCALES); re-anchors pacing.
    Speed(f64),
    /// Tests only: set the whole held action through `Sim::set_action` (the motion handler's setter), for presets without a motion config.
    #[cfg(test)]
    SetInputs(Vec<f64>),
}

/// Run actions shared by the buttons, `system_ui` and REST `robot_run`.
#[derive(Clone, Copy, Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunAction {
    Start,
    Pause,
    Step,
    Reset,
}
impl RunAction {
    pub const ALL: [RunAction; 4] = [RunAction::Start, RunAction::Pause, RunAction::Step, RunAction::Reset];
    pub fn name(self) -> &'static str {
        match self {
            RunAction::Start => "start",
            RunAction::Pause => "pause",
            RunAction::Step => "step",
            RunAction::Reset => "reset",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            RunAction::Start => "Run",
            RunAction::Pause => "Pause",
            RunAction::Step => "Step",
            RunAction::Reset => "Reset",
        }
    }
    pub fn parse(name: &str) -> Result<Self, String> {
        Self::ALL.into_iter().find(|a| a.name() == name).ok_or_else(|| {
            let valid: Vec<&str> = Self::ALL.iter().map(|a| a.name()).collect();
            format!("unknown robot_run action `{name}`; valid actions: {}", valid.join(", "))
        })
    }
}
