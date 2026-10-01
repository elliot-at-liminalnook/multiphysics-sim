//! Robot mode for a planar (v2) `.simrobot.json` (`--robot FILE`): RoboCAD's
//! planar summary (bodies with mass, centre, planar inertia and section
//! outline in mm in the CAD working plane, revolute joints with pivots).
//! `PhysicalModel::parse` refuses these files, so robot mode reads them as
//! sim-phenomena's `CadModel` from the same bytes (version rule:
//! `sim_domain_robot::model::simrobot_version`) and runs them through the one
//! shared planar build, `cad_robot::build_planar` (`CadRobot`): no physics
//! of its own, and never the v3 physical model.
//!
//! One run thread (`jobs::RunThread`, "robot-run (planar v2)") builds the
//! `CadRobot` from the loaded model and advances it on simulation time with
//! the planar viewer's rules ([`PACING`]); the UI thread sends commands and
//! draws the generation-stamped frames it publishes ([`PlanarFrame`]) as
//! gizmos: outlines, centres of mass, chain-tip contact points, a ground grid.
use crate::jobs::Stamped;
use serde::Serialize;
use serde_json::{Value, json};
use sim_phenomena::scenarios::cad_robot::{CadModel, PLANAR_BANDWIDTH_HZ, PLANAR_DAMPING_RATIO, build_planar};
use std::path::Path;
use std::time::Duration;

mod thread;
mod view;
#[cfg(test)]
mod tests;

pub use thread::{PlanarCommand, PlanarRun};
pub use view::{bounds, display, draw, inspector_text, run_line};

/// `robot_state.format.name` for a planar file.
pub const FORMAT_NAME: &str = "planar v2 summary";
/// What a planar run is, stated from `CadRobot::build` (cad_robot.rs); shown
/// verbatim in `robot_state.format.fidelity` and at the top of the inspector.
pub const FIDELITY: &str = "planar v2 summary: a 2-D multibody in the CAD working plane built by sim-phenomena's CadRobot (cad_robot::build_planar). Root: the body the CAD marks ground (held nearly fixed: a planar body of mass and inertia 1e6 without gravity), else the heaviest body that is no kept joint's child, a free planar rigid body under gravity (9.81 m/s² along −y of the working plane; the file's plane block is not used) on two compliant point contacts (friction 0.8, stiffness 2e5, damping 2e3) at the leftmost and rightmost of its six lowest outline points (±20 mm about its centre of mass at the model's floor when it has fewer than two). Joints: only revolute and continuous joints are kept; a fixed or other joint is dropped (each named in the warnings), so its child is not attached through it: that body and everything beyond it are neither simulated nor drawn and their mass is not added to the parent, unless the child is then picked as the root by the heaviest-body rule, in which case the intended base and its chains are the ones left out. Links: serial chains from the root's children, continued while a link has exactly one child; at a branch the chain ends and every child starts its own chain whose base is attached to the root body at that pivot's offset (rerooted, not to its parent link, whose gains then leave the branch out). Control: every joint held at its target (the CAD pose until moved) by a PD law on the seam sampled every 2 ms with ideal encoder and tachometer feedback, kp from the outboard inertia (plus rotor inertia) for a 6 Hz bandwidth and damping ratio 1, the file's joint damping added to kd, the target clamped to the CAD limits, the command clamped to the motor's stall-torque / no-load-speed line when the joint has a motor, then soft end stops (10·kp past a limit) added, then a ±500 N·m clamp, through a 100 Hz servo element capped at 50 N·m. A compliant point contact (friction 0.8) at every chain tip. Backward Euler with a 1 ms step. Uncalibrated; no collision between links; not the v3 physical model (no 3-D inertia, collision geometry, joint friction or backlash, motor electrical models, sensor models or stress results).";
/// The header's short form of [`FIDELITY`] (the full text is in Source and `robot_state.format`).
pub const HEADER_LABEL: &str = "planar v2 summary · uncalibrated · not the v3 physical model";
/// The run thread's name.
pub const THREAD: &str = "robot-run (planar v2)";
/// Simulation time advanced per grid step (s).
pub const GRID_S: f64 = 0.02;
/// Real time counted per tick at most (s), before the speed scale.
pub const MAX_REAL_DT_S: f64 = 0.05;
/// The run thread's tick (about 60 Hz).
pub const TICK: Duration = Duration::from_micros(16_667);
/// Arrow-key target steps (rad): plain and with Shift.
pub const KEY_STEP_RAD: f64 = 0.01;
pub const KEY_STEP_SHIFT_RAD: f64 = 0.05;
pub const PACING: &str = "planar v2 pacing (the planar viewer's rule, kept unchanged): every ~60 Hz tick adds min(real time since the last tick, 0.05 s) × speed_scale to an accumulator; when it holds at least one 0.02 s grid step the robot advances exactly one grid step and the accumulator is capped at one grid step, so a slow model never stalls the window and at most one grid step runs per tick (at most ~1.2 × real time at 60 Hz, whatever the scale). Step advances one grid step while paused. achieved_rate = sim seconds / wall seconds since Run or the last speed change; compute_s_per_sim_s = wall seconds spent inside CadRobot::advance per simulated second.";
pub const JOG_RULE: &str = "planar v2 joint targets: CadRobot::set_target on the run thread; a delta is added to the joint's current target on the thread (the arrow keys: ↑/↓ ±0.01 rad, Shift ±0.05 rad; the jog buttons ±0.05 rad), target sets it. Targets are not refused at the CAD limits: the PD law clamps them to the limits (with soft end stops), as the planar build does. Reset rebuilds from the loaded model, so targets return to the CAD pose. Nothing is written.";

// Every refusal of an action without a v2 meaning, by name: "planar v2 file: <what> is not available (<why>)".
pub const MOTION: &str = "planar v2 file: motion requests (WASD keys, motion:*, robot_input) are not available (they drive a robot preset's Rust controller; a planar v2 file has no PhysicalModel and no preset)";
pub const SAVE_RECORDING: &str = "planar v2 file: save recording is not available (it saves a robot preset's session recording; a planar v2 run has none)";
pub const REPLAY: &str = "planar v2 file: replay and the recordings list are not available (they re-execute a robot preset's saved session recordings; a planar v2 run has none)";
pub const GAIT: &str = "planar v2 file: gait preview is not available (it poses a robot preset's scene with the shared KinematicMirror; no PhysicalModel — it needs a v3 export in a preset)";
pub const RECORDED: &str = "planar v2 file: the recorded timeline is not available (it plays a recorded preset's capture)";
pub const JOINT_FRAMES: &str = "planar v2 file: the joint-frame overlay is not available (it reads PhysicalRobot::joint_frames; it needs a v3 export)";
pub const DEFLECTIONS: &str = "planar v2 file: the deflection overlay is not available (it reads PhysicalRobot::deflections of flexible links; it needs a v3 export)";
pub const GRAPHS: &str = "planar v2 file: graphs are not available (robot_graphs charts a PhysicalRobot run's or a preset session's frames; no planar series is defined; it needs a v3 export)";
/// Refusal of the stress overlay (keys S and H) for a planar file.
pub const STRESS: &str = "planar v2 file: the stress overlay is not available (stress needs a v3 physical export: `sim-cad run` writes a .simresult.json only for v3 files)";
/// Why the hardware mirror cannot pose a planar file (for `robot::hardware::mirror::scene_of`).
pub const NO_MIRROR: &str = "planar v2 file: the leg mirror is not available (it poses a robot preset's scene; a planar v2 summary has no scene or PhysicalModel)";
/// Live motor sync (hardware panel) streams a live controller's named motor targets; a planar v2 run has none.
pub const LIVE_SYNC: &str = "planar v2 file: live motor sync is not available (it streams a live walking controller's named motor targets; the planar v2 build holds joint targets with its own PD and names no motors)";

/// `robot_state.unavailable` for a planar file.
pub const UNAVAILABLE: [(&str, &str); 11] = [
    ("motion", MOTION),
    ("save_recording", SAVE_RECORDING),
    ("replay", REPLAY),
    ("gait_preview", GAIT),
    ("recorded", RECORDED),
    ("overlay_joints", JOINT_FRAMES),
    ("overlay_deflections", DEFLECTIONS),
    ("overlay_stress", STRESS),
    ("graphs", GRAPHS),
    ("leg_mirror", NO_MIRROR),
    ("live_sync", LIVE_SYNC),
];

/// A planar file as read on the worker thread (built there once only to
/// refuse a file that cannot build; the run thread builds the robot it runs).
pub struct PlanarLoaded {
    pub model: CadModel,
    /// The file's `version` as stored (None when absent: read as 2).
    pub declared_version: Option<u64>,
    /// The version the shared rule reads (`simrobot_version`).
    pub version: u32,
    /// The file's `plane` block as stored (null when absent).
    pub plane: Value,
    /// The file's `unit` as stored.
    pub unit: Option<String>,
}

/// Reads a planar file from the bytes the worker read (`raw` is the same bytes
/// as JSON). Refused, naming the path: a unit other than mm (the planar build
/// reads millimetres), no bodies, a revolute/continuous joint naming an
/// unknown body, or anything else the shared build refuses or panics on: the
/// build is tried once here, on the reload worker (off the UI thread), so a
/// file that cannot build keeps the last good model and its run instead of
/// replacing them with a failed one (`robot_source::RULE`). The run thread
/// builds its own robot from the same model.
pub fn load_bytes(path: &Path, bytes: &[u8], raw: &Value) -> Result<PlanarLoaded, String> {
    let name = path.display();
    let model: CadModel = serde_json::from_slice(bytes).map_err(|e| format!("{name}: planar v2 file: {e}"))?;
    let unit = raw.get("unit").and_then(Value::as_str).map(str::to_string);
    if let Some(u) = unit.as_deref().filter(|u| *u != "mm") {
        return Err(format!("{name}: planar v2 file declares unit `{u}`; the planar build reads millimetres (unit `mm`) only"));
    }
    if model.bodies.is_empty() {
        return Err(format!("{name}: planar v2 file: the model has no bodies"));
    }
    if let Some(j) = model.joints.iter().filter(|j| matches!(j.kind.as_str(), "revolute" | "continuous")).find(|j| !model.bodies.iter().any(|b| b.name == j.child)) {
        return Err(format!("{name}: planar v2 file: joint {} names an unknown body {}", j.name, j.child));
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build_planar(model.clone()))) {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => return Err(format!("{name}: planar v2 file does not build: {e}")),
        Err(_) => return Err(format!("{name}: planar v2 file does not build: the planar build panicked (a duplicate joint or body name is the usual cause)")),
    }
    Ok(PlanarLoaded {
        declared_version: raw.get("version").and_then(Value::as_u64),
        version: sim_domain_robot::model::simrobot_version(raw),
        plane: raw.get("plane").cloned().unwrap_or(Value::Null),
        unit,
        model,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanarPhase {
    /// The run thread is building the robot (at open and after Reset).
    #[default]
    Building,
    Running,
    Paused,
    /// The build or an advance failed (`error`, verbatim); Reset rebuilds.
    Failed,
}
impl PlanarPhase {
    pub fn name(self) -> &'static str {
        match self {
            PlanarPhase::Building => "building",
            PlanarPhase::Running => "running",
            PlanarPhase::Paused => "paused",
            PlanarPhase::Failed => "failed",
        }
    }
}


/// One body's pose: world (m, in the plane, ground at y = 0) and angle (rad).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PlanarPose {
    pub body: usize,
    pub com: [f64; 2],
    pub angle: f64,
}

/// What the run thread publishes: status and the built robot's state, stamped
/// with the generation of the command it follows.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PlanarFrame {
    pub generation: u64,
    /// Publication counter of this thread (the UI copies only a new one).
    pub seq: u64,
    pub phase: PlanarPhase,
    /// A robot is built (false while building and after a failed build).
    pub built: bool,
    pub time: f64,
    /// Grid steps advanced in this generation.
    pub steps: u64,
    /// Built joint order (chain order), as `CadRobot::joint_names`.
    pub joint_names: Vec<String>,
    pub joint_angles: Vec<f64>,
    pub targets: Vec<f64>,
    /// `CadRobot::outlines`: per body index, a closed polyline (m).
    pub outlines: Vec<(usize, Vec<[f64; 2]>)>,
    /// `CadRobot::poses`: root first, then per link.
    pub poses: Vec<PlanarPose>,
    /// Chain tips (the tip contact points), read through the runtime (m).
    pub tips: Vec<[f64; 2]>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    pub root: Option<String>,
    pub root_fixed: bool,
    pub bodies: usize,
    pub joints: usize,
    pub speed_scale: f64,
    pub sim_s: f64,
    pub compute_s: f64,
    pub achieved_rate: Option<f64>,
}
impl Stamped for PlanarFrame {
    fn generation(&self) -> u64 {
        self.generation
    }
}

/// Robot mode's state for a planar file: the loaded summary, its run, the
/// arrow keys' joint and the contacts overlay.
pub struct PlanarView {
    pub loaded: PlanarLoaded,
    pub run: PlanarRun,
    /// The joint ←/→ select and ↑/↓ move (index in the built joint order).
    pub selected_joint: usize,
    /// The joint selected before a reload, by name: resolved against the new
    /// build's joint order on its first built frame (`selected_joint` then
    /// points at it, else stays clamped to the joint count).
    pub pending_joint: Option<String>,
    /// Red chain-tip dots (key C, overlay:contacts, robot_overlay contacts).
    pub contacts: bool,
    /// Frame the camera on the next built frame: Some(true) also moves the focus (an open), Some(false) only sets the extent (a reload keeps the camera).
    pub frame_camera: Option<bool>,
}
impl PlanarView {
    /// Spawns the run thread for a loaded file (`generation`, speed and contacts carried over by the caller).
    pub fn new(loaded: PlanarLoaded, generation: u64, speed: f64, contacts: bool, move_camera: bool) -> Self {
        let run = PlanarRun::spawn(loaded.model.clone(), generation, speed);
        Self { loaded, run, selected_joint: 0, pending_joint: None, contacts, frame_camera: Some(move_camera) }
    }
    /// The built joint names (empty until the first built frame).
    pub fn joint_names(&self) -> &[String] {
        self.run.frame().filter(|f| f.built).map(|f| f.joint_names.as_slice()).unwrap_or(&[])
    }
    pub fn selected_joint_name(&self) -> Option<&str> {
        self.joint_names().get(self.selected_joint).map(String::as_str)
    }
    /// Once built, point `selected_joint` at the joint selected before a
    /// reload (by name), or clamp it to the new joint count.
    pub fn resolve_pending_joint(&mut self) {
        let names = self.joint_names();
        if names.is_empty() {
            return;
        }
        let index = self.pending_joint.as_ref().and_then(|n| names.iter().position(|j| j == n)).unwrap_or(self.selected_joint.min(names.len() - 1));
        self.selected_joint = index;
        self.pending_joint = None;
    }
    /// `robot_state.format` for a planar file.
    pub fn format_json(&self) -> Value {
        json!({"version": self.loaded.version, "declared_version": self.loaded.declared_version, "name": FORMAT_NAME, "fidelity": FIDELITY,
            "build": {"function": "sim_phenomena::scenarios::cad_robot::build_planar", "bandwidth_hz": PLANAR_BANDWIDTH_HZ, "damping_ratio": PLANAR_DAMPING_RATIO},
            "warnings": self.run.frame().map(|f| f.warnings.clone()).unwrap_or_default()})
    }
    /// `robot_state.planar`: the summary as read, the latest frame and the rules.
    pub fn json(&self, selected: Option<usize>) -> Value {
        let m = &self.loaded.model;
        let bodies: Vec<Value> = m.bodies.iter().enumerate().map(|(i, b)| json!({"index": i, "id": b.id, "name": b.name, "material": b.material, "mass_kg": b.mass_kg, "com_mm": b.com,
            "inertia_zz_kg_m2": b.inertia_zz, "ground": b.ground, "outline_loops": b.outline.len(), "bbox_mm": b.bbox})).collect();
        let simulated = self.joint_names();
        let joints: Vec<Value> = m.joints.iter().map(|j| json!({"name": j.name, "type": j.kind, "parent": j.parent, "child": j.child, "pivot_mm": j.pivot2, "limits_rad": j.limits,
            "damping": j.damping, "axis_sign": j.axis_sign, "simulated": simulated.contains(&j.name),
            "motor": j.motor.as_ref().map(|x| json!({"name": x.name, "spec": x.spec, "stall_torque_n_m": x.stall_torque, "no_load_speed_rad_s": x.no_load_speed, "rotor_inertia_kg_m2": x.rotor_inertia}))})).collect();
        let f = self.run.frame().filter(|f| f.built);
        let frame = f.map(|f| json!({"generation": f.generation, "time": f.time, "steps": f.steps, "joint_names": f.joint_names, "joint_angles_rad": f.joint_angles, "targets_rad": f.targets,
            "poses": f.poses, "tips_m": f.tips, "outline_count": f.outlines.len(), "root": f.root, "root_fixed": f.root_fixed}));
        let selected_body = selected.and_then(|i| bodies.get(i).cloned());
        json!({"unit": self.loaded.unit, "plane": self.loaded.plane, "source": m.source, "body_count": m.bodies.len(), "joint_count": m.joints.len(), "simulated_joint_count": simulated.len(),
            "bodies": bodies, "joints": joints, "selected_body": selected_body, "selected_joint": self.selected_joint_name(), "frame": frame,
            "frame_axes": "x right, y up in the CAD working plane (m), ground at y = 0 (the model's lowest point); drawn in the display frame as (x, y, 0)", "jog_rule": JOG_RULE})
    }
    /// `robot_state.overlays` for a planar file.
    pub fn overlays_json(&self) -> Value {
        json!({"contacts": {"on": self.contacts, "available": true, "count": self.run.frame().map(|f| f.tips.len()), "what": "chain-tip contact points (CadRobot chains[].tip), red dots"},
            "joints": {"available": false, "reason": JOINT_FRAMES},
            "deflections": {"available": false, "reason": DEFLECTIONS},
            "stress": {"available": false, "reason": STRESS}})
    }
}
