//! Published frames: link poses, overlays and motor targets, and how each run kind maps into them.
use bevy::math::{DMat3, DQuat};
use serde::Deserialize as _;
use serde::Serialize;
use serde_json::{Value, json};
use sim_runtime::embedded_capture::{CaptureFrame, CapturePose};
use std::sync::Arc;
use super::short;

/// One published state of the robot, stamped with its generation.
#[derive(Clone, Debug)]
pub struct Frame {
    pub generation: u64,
    pub time: f64,
    /// Chunks of `CHUNK_S` advanced since the build.
    pub steps: u64,
    /// Nominal solver steps completed (presets; the session's own count).
    pub completed_steps: Option<u64>,
    /// Per link of the loaded model (by index): position of the link frame (at
    /// its com) and orientation, model frame; None when the frame has no pose
    /// of that name.
    pub poses: Vec<Option<([f64; 3], DQuat)>>,
    /// Presets: per loaded link (by index), the published world-frame linear
    /// velocity of the link frame (m/s) and angular velocity (rad/s) from the
    /// session frame's `velocity_m_s` / `angular_velocity_rad_s`; None when the
    /// pose has none. Empty for `--robot FILE` (PhysicalRobot::poses has none).
    pub velocities: Vec<Option<([f64; 3], [f64; 3])>>,
    /// Link names in the simulation's frame that match no loaded link.
    pub unmatched: Vec<String>,
    pub joint_names: Vec<String>,
    pub joint_angles: Vec<f64>,
    /// Snapshot of the robot's servo targets when the frame was taken.
    pub targets: Vec<f64>,
    /// Presets: the held action (the session's input values, in `inputs()`
    /// order) when the frame was taken; empty for `--robot FILE`.
    pub inputs: Vec<f64>,
    /// `--robot FILE`: contacts, joint frames and deflections copied from the
    /// PhysicalRobot on the run thread (each only while its overlay is on).
    pub overlays: Overlays,
    /// Live preset frames: the session frame's named motor targets (what the
    /// browser's hardware sync streams, viewer.js:428); None for `--robot
    /// FILE` and recorded frames.
    pub motor_targets: Option<MotorTargets>,
}

/// A live preset frame's motor targets by name: the session frame's
/// `servo_targets_rad` with the session's `coordinate_names`
/// (`EmbeddedSession::coordinate_names`, `EmbeddedEnvironment::metadata`),
/// copied as published; nothing is computed here.
#[derive(Clone, Debug, PartialEq)]
pub struct MotorTargets {
    /// The session's coordinate names (`joint.+X | Hip servo output`, …), shared by every frame of a build.
    pub coordinates: Arc<Vec<String>>,
    /// `servo_targets_rad`, in the frame's order (its length may differ from `coordinates`; consumers check).
    pub targets_rad: Vec<f64>,
    /// The session frame's `done` (the episode ended: the page's `frame.done`); false when absent.
    pub done: bool,
}
impl Frame {
    /// (target, measured angle) of a joint by its file name, if the robot has a target for it.
    pub fn servo(&self, joint: &str) -> Option<(f64, f64)> {
        let i = self.joint_names.iter().position(|n| short(n) == joint)?;
        Some((*self.targets.get(i)?, *self.joint_angles.get(i)?))
    }
}

/// Force line length per newton of contact force (m/N).
pub const FORCE_SCALE_M_PER_N: f64 = 0.005;
/// Deflection lines are drawn this many times their displacement.
pub const DEFLECTION_MAGNIFICATION: f64 = 50.0;
/// Half-length of each drawn joint axis (m).
pub const JOINT_AXIS_HALF_M: f64 = 0.02;
/// Contacts listed with their values in `robot_state.overlays` (all are counted).
pub const OVERLAY_SAMPLE: usize = 4;
pub const OVERLAY_COST_RULE: &str = "each overlay's data is computed on the run thread only while that overlay is on (PhysicalRobot::contacts, joint_frames and deflections each evaluate the articulation once per published frame, i.e. per 0.02 s chunk); a hidden overlay's list is null in the frame, never stale data";

/// Which run-thread overlays are on (`robot_overlay`, `system_ui` overlay:*, keys C/J/F, the inspector buttons).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct OverlayFlags {
    pub contacts: bool,
    pub joints: bool,
    pub deflections: bool,
}
impl Default for OverlayFlags {
    /// All three are drawn by default (each toggleable).
    fn default() -> Self {
        Self { contacts: true, joints: true, deflections: true }
    }
}
/// One `PhysicalRobot::contacts()` entry: model frame, SI (m, N).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OverlayContact {
    pub link: String,
    /// The other link's name, or "ground".
    pub other: String,
    pub point: [f64; 3],
    pub force: [f64; 3],
    pub penetration: f64,
}
/// One `PhysicalRobot::joint_frames()` entry: the joint point and its axes, model frame.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OverlayJoint {
    pub name: String,
    pub point: [f64; 3],
    pub axes: Vec<[f64; 3]>,
}
/// One `PhysicalRobot::deflections()` entry: a flexible link's boundary point and its displacement (m), model frame.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OverlayDeflection {
    pub link: usize,
    pub point: [f64; 3],
    pub displacement: [f64; 3],
}
/// A frame's overlay data; each list is None when its overlay was off (not computed) or for a preset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overlays {
    /// The flags the run thread built this frame with.
    pub flags: Option<OverlayFlags>,
    pub contacts: Option<Vec<OverlayContact>>,
    pub joints: Option<Vec<OverlayJoint>>,
    pub deflections: Option<Vec<OverlayDeflection>>,
}
fn a3(v: &sim_domain_robot::math::V) -> [f64; 3] {
    [v.x, v.y, v.z]
}
/// Copies the robot's own contacts, joint frames and deflections for the overlays that are on.
pub fn overlays(robot: &sim_runtime::physical::PhysicalRobot, flags: OverlayFlags) -> Overlays {
    let name = |i: usize| robot.art.links.get(i).map_or_else(|| format!("link {i}"), |l| l.name.clone());
    let contacts = flags.contacts.then(|| {
        robot.contacts().iter().map(|c| OverlayContact { link: name(c.link), other: c.other.map_or_else(|| "ground".to_string(), name), point: a3(&c.point), force: a3(&c.force), penetration: c.penetration }).collect()
    });
    let joints = flags.joints.then(|| robot.joint_frames().iter().map(|(n, p, axes)| OverlayJoint { name: n.clone(), point: a3(p), axes: axes.iter().map(a3).collect() }).collect());
    let deflections = flags.deflections.then(|| robot.deflections().iter().map(|(l, p, u)| OverlayDeflection { link: *l, point: a3(p), displacement: a3(u) }).collect());
    Overlays { flags: Some(flags), contacts, joints, deflections }
}

/// The UI keeps a frame only if it belongs to the current generation.
pub fn accept(current_generation: u64, frame: &Frame) -> bool {
    frame.generation >= current_generation
}

pub(super) fn frame(robot: &sim_runtime::physical::PhysicalRobot, generation: u64, steps: u64, flags: OverlayFlags) -> Frame {
    let poses = robot
        .poses()
        .iter()
        .map(|(r, p)| {
            let cols: [f64; 9] = std::array::from_fn(|i| r.as_slice()[i]);
            Some(([p.x, p.y, p.z], DQuat::from_mat3(&DMat3::from_cols_array(&cols)).normalize()))
        })
        .collect();
    let targets = robot.targets.lock().unwrap_or_else(|p| p.into_inner()).clone();
    Frame { generation, time: robot.time(), steps, completed_steps: None, poses, velocities: Vec::new(), unmatched: Vec::new(), joint_names: robot.joint_names.clone(), joint_angles: robot.joint_angles(), targets, inputs: Vec::new(), overlays: overlays(robot, flags), motor_targets: None }
}

/// Link poses by loaded-link index, their published velocities, and the pose
/// names that match no loaded link (kept, never dropped silently). One mapping
/// for live preset frames and recorded captures: `poses[]` of `{name,
/// position_m, rotation}` (rotation row-major, the same link frames
/// `PhysicalRobot::poses` gives), the shape `sim_runtime::embedded_capture` reads.
/// A row-major 3×3 rotation (as frames and mirror poses publish it) as a unit quaternion.
pub fn rotation_quat(m: &[[f64; 3]; 3]) -> DQuat {
    let cols = DMat3::from_cols([m[0][0], m[1][0], m[2][0]].into(), [m[0][1], m[1][1], m[2][1]].into(), [m[0][2], m[1][2], m[2][2]].into());
    DQuat::from_mat3(&cols).normalize()
}
type Mapped = (Vec<Option<([f64; 3], DQuat)>>, Vec<Option<([f64; 3], [f64; 3])>>, Vec<String>);
pub fn map_poses(poses: &[CapturePose], links: &[String]) -> Mapped {
    let mut out = vec![None; links.len()];
    let mut velocities = vec![None; links.len()];
    let mut unmatched = Vec::new();
    for pose in poses {
        match links.iter().position(|l| *l == pose.name) {
            Some(i) => {
                out[i] = Some((pose.position_m, rotation_quat(&pose.rotation)));
                // Published velocities, when the pose has both (never differentiated from positions here).
                velocities[i] = pose.velocity_m_s.zip(pose.angular_velocity_rad_s);
            }
            None => unmatched.push(pose.name.clone()),
        }
    }
    (out, velocities, unmatched)
}

/// A frame from a preset session's `interactive_frame()`, its poses parsed as
/// the shared [`CapturePose`] and mapped by [`map_poses`].
pub(super) fn preset_frame(v: &Value, links: &[String], generation: u64, steps: u64, step_s: f64, inputs: &[f64], names: &Arc<Vec<String>>) -> Result<Frame, String> {
    let completed = v.get("completed_steps").and_then(Value::as_u64).ok_or("session frame has no completed_steps")?;
    let raw = v.get("poses").and_then(Value::as_array).ok_or("session frame has no poses")?;
    let parsed = raw.iter().enumerate().map(|(k, pose)| CapturePose::deserialize(pose).map_err(|e| format!("session frame poses[{k}] ({}): {e}", pose.get("name").and_then(Value::as_str).unwrap_or("no name")))).collect::<Result<Vec<_>, _>>()?;
    let (poses, velocities, unmatched) = map_poses(&parsed, links);
    Ok(Frame { generation, time: completed as f64 * step_s, steps, completed_steps: Some(completed), poses, velocities, unmatched, joint_names: Vec::new(), joint_angles: Vec::new(), targets: Vec::new(), inputs: inputs.to_vec(), overlays: Overlays::default(), motor_targets: motor_targets(v, names) })
}

/// The frame's `servo_targets_rad` (every entry a number) with the session's
/// coordinate names and the frame's `done`; None when the frame publishes no
/// such array.
pub(super) fn motor_targets(v: &Value, names: &Arc<Vec<String>>) -> Option<MotorTargets> {
    let targets_rad = v.get("servo_targets_rad")?.as_array()?.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>()?;
    let done = v.get("done").and_then(Value::as_bool).unwrap_or(false);
    Some(MotorTargets { coordinates: names.clone(), targets_rad, done })
}

/// An environment's coordinate names (`metadata()["coordinate_names"]`, the
/// session's `coordinate_names()`; the environment keeps its session private).
pub(super) fn environment_names(env: &sim_runtime::environment::EmbeddedEnvironment) -> Arc<Vec<String>> {
    Arc::new(env.metadata()["coordinate_names"].as_array().map(|a| a.iter().filter_map(|n| n.as_str().map(str::to_string)).collect()).unwrap_or_default())
}

/// A recorded capture frame mapped to the loaded links by [`map_poses`]:
/// `time` is the frame's recorded time_s and `steps` its index in the capture.
pub fn recorded_frame(f: &CaptureFrame, links: &[String], generation: u64, index: u64) -> Frame {
    let (poses, velocities, unmatched) = map_poses(&f.poses, links);
    Frame { generation, time: f.time_s, steps: index, completed_steps: None, poses, velocities, unmatched, joint_names: Vec::new(), joint_angles: Vec::new(), targets: Vec::new(), inputs: Vec::new(), overlays: Overlays::default(), motor_targets: None }
}

/// A frame's time, step count and link poses as JSON (the sidecar's final frame).
pub(super) fn frame_json(f: &Frame, links: &[String]) -> Value {
    let poses: Vec<Value> = f.poses.iter().enumerate().map(|(i, pose)| match pose {
        Some((p, q)) => json!({"link": links.get(i), "position": p, "quat_xyzw": [q.x, q.y, q.z, q.w]}),
        None => json!({"link": links.get(i), "position": null, "quat_xyzw": null}),
    }).collect();
    json!({"time": f.time, "completed_steps": f.completed_steps, "chunks": f.steps, "generation": f.generation, "inputs": f.inputs, "poses": poses})
}
