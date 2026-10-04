//! Robot-mode run thread: one worker owns the simulation and advances it in
//! fixed sim-time chunks, paced at most to real time. For a physical (v3+)
//! `--robot FILE` (a planar v2 file runs on `robot_planar`'s thread instead) it is
//! the shared `sim_runtime::physical::PhysicalRobot` (built from a clone of the
//! loaded model with `sim_runtime::registry()` and `BuildOptions::default()`);
//! for a preset (`robot_preset`) it is the shared
//! `EmbeddedEnvironment` (with a task) or `EmbeddedSession` built from the
//! preset's files unchanged. A `--robot FILE` with a controller binding
//! beside it (`controlled`) runs as the shared `sim_runtime::drive_host::DriveHost`
//! (the shared `Session` driven by its external controller). The UI thread only sends commands and
//! applies the published frames; it never builds, advances or locks the simulation.

mod controlled;
mod controller;
mod frames;
mod history;
mod inputs;
mod jog;
mod pacing;
mod preset_ops;
mod protocol;
mod replay;
mod sim;
mod worker;

pub use controlled::{CONTROLLER_LABEL, ControlledRun, DEADMAN_RULE, DRIVE_FIDELITY, DRIVE_RULE, DRIVE_SEED, NOT_RUNNING, SEED_RULE};
pub use controller::RunController;
pub use frames::{DEFLECTION_MAGNIFICATION, FORCE_SCALE_M_PER_N, Frame, JOINT_AXIS_HALF_M, MotorTargets, OVERLAY_COST_RULE, OVERLAY_SAMPLE, OverlayContact, OverlayDeflection, OverlayFlags, OverlayJoint, Overlays, accept, map_poses, overlays, recorded_frame, rotation_quat};
pub(crate) use jog::short;
pub use jog::{JOG_LABEL, JOG_SEMANTICS, JOG_STEP_M, JOG_STEP_RAD, Servo, check_target, servo};
pub use pacing::{CHUNK_S, COMPUTE_LIMITED_FRACTION, COMPUTE_LIMITED_RULE, PACING, Pace, SPEED_SCALES, SpeedRequest, pace, speed_target};
pub use protocol::{Drive, MotionRequest, Phase, RunAction, Source};
pub use replay::{ReplayPhase, ReplayState};
pub use inputs::{INPUTS_RULE, RESIDUAL_PREFIX};
pub use history::{HISTORY_RULE, HISTORY_S};

#[cfg(test)]
mod tests;
