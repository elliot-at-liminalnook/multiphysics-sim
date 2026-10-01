//! Servo-target jog for `--robot FILE`: which joints hold a target, and target checks.
use serde::Serialize;
use sim_domain_robot::PhysicalModel;

/// One +/− jog press: rad on a revolute/continuous joint, m on a prismatic one.
pub const JOG_STEP_RAD: f64 = 0.05;
pub const JOG_STEP_M: f64 = 0.005;
pub const JOG_LABEL: &str = "servo target (PD hold from the export), not walking-controller teleop";
pub const JOG_SEMANTICS: &str = "a jog sets PhysicalRobot::set_target on the run thread (index resolved by the robot's joint_names with the joint./slide. prefix stripped, as physical.rs does). While paused it sets the target, which takes effect on the next run or step; before the first build it is queued and applied right after the build; Reset rebuilds from the file, so jogged targets return to the file's control targets. Targets outside the file's limits are refused, never clamped; a joint without limits in the file accepts any finite target (no limit in file). Nothing is written to the file.";

/// A joint the run thread holds a servo target for, as read from the file.
#[derive(Clone, Debug, Serialize)]
pub struct Servo {
    pub joint: String,
    pub unit: &'static str,
    /// The file's `limits`, or None ("no limit in file").
    pub limit: Option<[f64; 2]>,
    /// The file's control target: what a build starts from (physical.rs uses 0 when absent).
    pub file_target: f64,
}
impl Servo {
    pub fn step(&self) -> f64 {
        if self.unit == "m" { JOG_STEP_M } else { JOG_STEP_RAD }
    }
    pub fn limit_text(&self) -> String {
        self.limit.map_or("no limit in file".into(), |[lo, hi]| format!("limits [{lo}, {hi}] {} (file)", self.unit))
    }
}

/// Whether the file gives `joint` a servo target under `BuildOptions::default()`,
/// read from the file alone (works with no build and after a failed one). It
/// mirrors `PhysicalRobot::build`: only a joint driven by a motor whose
/// firmware is not `none` gets a position servo and a target, and a
/// `trajectory` with keyframes overwrites every target each control step.
pub fn servo(model: &PhysicalModel, joint: &str) -> Result<Servo, String> {
    let Some(j) = model.joint(joint) else {
        let known: Vec<&str> = model.joints.iter().filter(|j| servo(model, &j.name).is_ok()).map(|j| j.name.as_str()).collect();
        return Err(format!("unknown joint `{joint}`; joints with a servo target in this file: {}", if known.is_empty() { "none".into() } else { known.join(", ") }));
    };
    let control = &model.control;
    match control.mode.as_str() {
        "hold" => {}
        "trajectory" if control.trajectory.is_empty() => {}
        "trajectory" => return Err(format!("joint `{joint}`: the file's control mode is `trajectory` ({} keyframes), which sets every servo target each control step, so a jog would be overwritten; jogging needs mode `hold`", control.trajectory.len())),
        mode => return Err(format!("joint `{joint}`: the file's control mode `{mode}` is not supported by PhysicalRobot (hold | trajectory)")),
    }
    if matches!(j.kind.as_str(), "fixed") || j.is_loop() {
        return Err(format!("joint `{joint}` has no servo target: it is a `{}` joint with no driven degree of freedom", j.kind));
    }
    let Some(motor) = model.motors.iter().find(|m| m.joint.as_deref() == Some(joint)) else {
        return Err(format!("joint `{joint}` has no servo target: no motor in the file drives it (passive joint)"));
    };
    if motor.firmware.kind == "none" {
        return Err(format!("joint `{joint}` has no servo target: its motor `{}` has firmware `none` (powered directly), so the build makes no position servo for it", motor.name));
    }
    let unit = if j.kind == "prismatic" { "m" } else { "rad" };
    Ok(Servo { joint: joint.into(), unit, limit: j.limits, file_target: control.targets.get(joint).copied().unwrap_or(0.0) })
}

/// A requested target for `servo`: finite and within the file's limits; never clamped.
pub fn check_target(servo: &Servo, target: f64) -> Result<(), String> {
    let joint = &servo.joint;
    if !target.is_finite() {
        return Err(format!("joint `{joint}`: target {target} is not finite"));
    }
    match servo.limit {
        Some([lo, hi]) if target < lo || target > hi => Err(format!("joint `{joint}`: target {target} {} is outside the file's limits [{lo}, {hi}] {}; not clamped", servo.unit, servo.unit)),
        _ => Ok(()),
    }
}

/// Short joint name of a robot port (`joint.hip` → `hip`), as physical.rs strips it.
pub(crate) fn short(port: &str) -> &str {
    port.trim_start_matches("joint.").trim_start_matches("slide.")
}
