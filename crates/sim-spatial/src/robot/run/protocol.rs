//! The run thread's protocol: phases, what it builds, its commands and what it publishes.
use serde::Serialize;
use serde_json::Value;
use sim_domain_robot::PhysicalModel;
use sim_runtime::session::InputChannel;
use std::sync::Arc;
use crate::robot::motion::{self, Motion, MotionChannel};
use crate::robot::preset::PresetRun;
use crate::robot::recording::Saved;
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

/// What the run thread builds: the loaded file, or a preset's parsed inputs.
#[derive(Clone)]
pub enum Source {
    Robot(PhysicalModel),
    Preset(Arc<PresetRun>),
}
impl Source {
    pub(super) fn chunk_s(&self) -> f64 {
        match self {
            Source::Robot(_) => CHUNK_S,
            Source::Preset(p) => p.chunk_s(),
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
}

/// A built preset's typed input channels (the session's `inputs()`), its
/// motion config (`motion::config`) and declared packet heartbeat.
#[derive(Debug)]
pub struct Drive {
    pub inputs: Vec<InputChannel>,
    pub motion: Option<Motion>,
    pub heartbeat: Option<MotionChannel>,
}
impl Drive {
    /// As the browser does on load: an invalid declaration fails the build, naming it.
    pub(super) fn resolve(run: &PresetRun, policy_contract: &Value, inputs: &[InputChannel]) -> Result<Self, String> {
        let entry = &run.preset.entry;
        Ok(Self { inputs: inputs.to_vec(), motion: motion::config(entry, policy_contract, inputs)?, heartbeat: motion::heartbeat(entry, inputs)? })
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
