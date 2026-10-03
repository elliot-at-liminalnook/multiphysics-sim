//! Drive geometry derived from the robot's physical model, so a wheel
//! mixer's track width, wheelbase, wheel radius and joint signs come from
//! CAD (AGENTS.md: CAD owns the robot's physical definition) with a
//! provenance naming the joints, links and method — never a default.
//!
//! Frames (cad/PHYSICAL_MODEL.md "Frames"): a joint's `origin` and `axis`
//! are in the world frame at export pose; a link frame has its origin at
//! the link's `com` with axes parallel to the world's, and its collision
//! vertices are in that frame. So a wheel joint's anchor in the chassis
//! frame is `origin − chassis.com`, and the wheel centre in the wheel's own
//! frame is `origin − wheel.com`. The chassis frame's axes are the body
//! axes: forward +x, left +y, up +z (gravity must point along −z).
//!
//! Wheel joint sign: a positive joint rotation about axis `a` moves the
//! contact point backwards relative to the wheel centre by `(a × ẑ)·x̂ = a_y`,
//! so the body rolls forward when `a_y > 0`: sign = sign(a_y), for an axis
//! along ±y (perpendicular to forward and up) only.
//!
//! Mecanum: the mixer assumes the X roller layout, and the model does not
//! describe rollers, so a mecanum geometry is never derived from the model:
//! [`derive`] refuses it and a mecanum profile declares its geometry with
//! its source (that declaration is then the layout's provenance).
//!
//! [`resolve`] also checks the profile against the model where both are
//! needed: the deadman timeout must be longer than the model's control
//! period, and each axis's max speed must be one the wheel motors can reach
//! (free-running wheel speed from the motors' gearbox `max_output_speed`).
use crate::model::{Joint, Link, Motor, PhysicalModel};
use sim_domain_control::drive::geometry::{DriveGeometry, Provenance, Valued, WheelJoint};
use sim_domain_control::drive::profile::{DriveProfile, KinematicsSpec};

/// How far a wheel axis may stray from ±y (unit-vector components).
pub const AXIS_TOLERANCE: f64 = 1e-6;
/// How far wheel radii, track widths, wheelbases and axle lines may differ
/// between wheels that the mixer treats as equal (m).
pub const LENGTH_TOLERANCE_M: f64 = 1e-6;

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// One wheel joint read from the model.
struct Wheel<'a> {
    role: &'static str,
    joint: &'a Joint,
    parent: &'a str,
    child: &'a Link,
    /// Joint origin in the parent (chassis) frame.
    anchor: V3,
    axis: V3,
    radius: f64,
    sign: f64,
}

fn wheel<'a>(model: &'a PhysicalModel, role: &'static str, name: &str) -> Result<Wheel<'a>, String> {
    let joint = model.joint(name).ok_or_else(|| {
        let names: Vec<&str> = model.joints.iter().map(|j| j.name.as_str()).collect();
        format!("drive geometry: the model has no joint `{name}` (kinematics.{role}); its joints are: {}", names.join(", "))
    })?;
    // A revolute joint has travel limits (or would silently be treated as
    // limitless): a wheel must turn without them, so only continuous joints.
    if joint.kind != "continuous" {
        return Err(format!(
            "drive geometry: joint `{name}` (kinematics.{role}) is a {} joint{}; a wheel joint must be continuous (it turns without travel limits)",
            joint.kind,
            joint.limits.map(|[lo, hi]| format!(" with limits [{lo}, {hi}] rad")).unwrap_or_default()
        ));
    }
    let parent = joint.parent.as_deref().ok_or_else(|| format!("drive geometry: joint `{name}` (kinematics.{role}) has no parent link"))?;
    let parent_link = model.link(parent).ok_or_else(|| format!("drive geometry: joint `{name}` names parent link `{parent}`, which the model does not have"))?;
    let child = model.link(&joint.child).ok_or_else(|| format!("drive geometry: joint `{name}` names child link `{}`, which the model does not have", joint.child))?;
    if joint.origin.iter().chain(&joint.axis).chain(&parent_link.com).chain(&child.com).any(|v| !v.is_finite()) {
        return Err(format!("drive geometry: joint `{name}` or its links `{parent}`, `{}` have a non-finite origin, axis or com", joint.child));
    }
    let length = norm(joint.axis);
    if length == 0.0 {
        return Err(format!("drive geometry: joint `{name}` has a zero axis"));
    }
    let axis = joint.axis.map(|c| c / length);
    if axis[0].abs() > AXIS_TOLERANCE || axis[2].abs() > AXIS_TOLERANCE {
        return Err(format!(
            "drive geometry: joint `{name}` axis {:?} is not along the body's lateral axis (±y): a wheel axle must be perpendicular to forward (+x) and up (+z)",
            joint.axis
        ));
    }
    let sign = if axis[1] > 0.0 { 1.0 } else { -1.0 };
    let vertices = &child.collision.vertices;
    if vertices.is_empty() {
        return Err(format!(
            "drive geometry: link `{}` (the wheel of joint `{name}`) has no collision vertices; the wheel radius is derived from them",
            joint.child
        ));
    }
    let centre = sub(joint.origin, child.com);
    let mut radius = 0.0_f64;
    for v in vertices {
        let d = sub(*v, centre);
        let along = dot(d, axis);
        let r = norm(sub(d, axis.map(|c| c * along)));
        if !r.is_finite() {
            return Err(format!("drive geometry: link `{}` has a non-finite collision vertex", joint.child));
        }
        radius = radius.max(r);
    }
    if radius <= 0.0 {
        return Err(format!("drive geometry: link `{}` has no collision vertex off the axis of joint `{name}`", joint.child));
    }
    Ok(Wheel { role, joint, parent, child, anchor: sub(joint.origin, parent_link.com), axis, radius, sign })
}

fn same_parent(wheels: &[Wheel]) -> Result<(), String> {
    let first = &wheels[0];
    for w in &wheels[1..] {
        if w.parent != first.parent {
            return Err(format!(
                "drive geometry: wheel joints `{}` and `{}` hang from different links (`{}`, `{}`); every wheel joint's parent must be the same body",
                first.joint.name, w.joint.name, first.parent, w.parent
            ));
        }
    }
    Ok(())
}

fn equal_radii(wheels: &[Wheel]) -> Result<Valued, String> {
    let (min, max) = wheels.iter().fold((f64::INFINITY, 0.0_f64), |(lo, hi), w| (lo.min(w.radius), hi.max(w.radius)));
    if max - min > LENGTH_TOLERANCE_M {
        let list: Vec<String> = wheels.iter().map(|w| format!("`{}` (joint `{}`) {} m", w.child.name, w.joint.name, w.radius)).collect();
        return Err(format!("drive geometry: the wheel radii differ by more than {LENGTH_TOLERANCE_M} m: {}", list.join(", ")));
    }
    let mean = wheels.iter().map(|w| w.radius).sum::<f64>() / wheels.len() as f64;
    let each: Vec<String> = wheels.iter().map(|w| format!("`{}` about joint `{}` {} m", w.child.name, w.joint.name, w.radius)).collect();
    Ok(Valued {
        value: mean,
        provenance: Provenance::Derived {
            from: format!(
                "mean wheel radius: the largest distance of each wheel link's collision vertices (simrobot links.collision.vertices, link frame) from its joint axis line (joints.origin, joints.axis): {}",
                each.join(", ")
            ),
        },
    })
}

fn wheel_joint(w: &Wheel) -> WheelJoint {
    WheelJoint {
        joint: w.joint.name.clone(),
        sign: w.sign,
        provenance: Provenance::Derived {
            from: format!(
                "joint `{}` axis {:?}: a positive rotation rolls the body {} (sign of the axis's +y component; body forward +x)",
                w.joint.name,
                w.axis,
                if w.sign > 0.0 { "forward" } else { "backward" }
            ),
        },
    }
}

/// Derive the drive geometry of a differential `kinematics` from the
/// model: track width from the wheel joints' anchors (lateral distance in
/// the chassis frame), wheel radius from the wheel links' collision
/// vertices about the joint axes, signs from the joint axes. Every value is
/// `Derived` with the joints and links named; missing or inconsistent data
/// is refused by name. A mecanum drive is refused: the model does not
/// describe its roller layout (see the module notes).
pub fn derive(model: &PhysicalModel, kinematics: &KinematicsSpec) -> Result<DriveGeometry, String> {
    let (left, right) = match kinematics {
        KinematicsSpec::Differential { left, right } => (left, right),
        KinematicsSpec::Mecanum { .. } => {
            return Err(
                "drive geometry: geometry.source is `model`, but the model does not describe mecanum roller layout; declare the geometry with its source (\"source\": \"declared\")"
                    .into(),
            );
        }
    };
    let g = model.gravity;
    if !(g[2] < 0.0 && g[0].abs() <= AXIS_TOLERANCE * g[2].abs() && g[1].abs() <= AXIS_TOLERANCE * g[2].abs()) {
        return Err(format!("drive geometry: the model's gravity {g:?} does not point along −z; the body axes (forward +x, left +y, up +z) assume it does"));
    }
    let wheels = kinematics
        .roles()
        .iter()
        .zip(kinematics.joints())
        .map(|(role, joint)| wheel(model, *role, joint))
        .collect::<Result<Vec<_>, String>>()?;
    same_parent(&wheels)?;
    let wheel_radius_m = equal_radii(&wheels)?;
    let parent = wheels[0].parent;
    let (l, r) = (&wheels[0], &wheels[1]);
    let dx = (l.anchor[0] - r.anchor[0]).abs();
    if dx > LENGTH_TOLERANCE_M {
        return Err(format!(
            "drive geometry: wheel joints `{}` and `{}` are {dx} m apart along forward (x); the mixer needs them on one axle line",
            l.joint.name, r.joint.name
        ));
    }
    let track = l.anchor[1] - r.anchor[1];
    if !(track > 0.0) {
        return Err(format!(
            "drive geometry: `{}` (kinematics.{}) is not to the left of `{}` (kinematics.{}): y {} m vs {} m in the `{parent}` frame (left is +y)",
            l.joint.name, l.role, r.joint.name, r.role, l.anchor[1], r.anchor[1]
        ));
    }
    Ok(DriveGeometry {
        track_width_m: Valued {
            value: track,
            provenance: Provenance::Derived {
                from: format!("|y({left}) − y({right})|: lateral (y) distance between the joint origins of `{left}` and `{right}` in the `{parent}` frame (simrobot joints.origin; body forward +x, left +y)"),
            },
        },
        wheelbase_m: None,
        wheel_radius_m,
        wheels: wheels.iter().map(wheel_joint).collect(),
    })
}

/// The geometry a profile asks for: derived from `model` for
/// `{"source": "model"}`, else the profile's declared values with their
/// declared sources (whose wheel joints must exist in the model). Then the
/// profile is checked against the model: [`check_deadman`] and
/// [`check_speeds`].
pub fn resolve(model: &PhysicalModel, profile: &DriveProfile) -> Result<DriveGeometry, String> {
    let geometry = match profile.declared_geometry()? {
        None => derive(model, &profile.kinematics)?,
        Some(geometry) => {
            for wheel in &geometry.wheels {
                if model.joint(&wheel.joint).is_none() {
                    return Err(format!("drive geometry: the model has no joint `{}` named by the drive profile's kinematics", wheel.joint));
                }
            }
            geometry
        }
    };
    check_deadman(model, profile)?;
    check_speeds(model, profile, &geometry)?;
    Ok(geometry)
}

/// The deadman timeout must be longer than one control period
/// (`control.period_s`): otherwise a request is lost before the controller
/// samples it again, and the robot never moves.
pub fn check_deadman(model: &PhysicalModel, profile: &DriveProfile) -> Result<(), String> {
    let period = model.control.period_s;
    if !(period.is_finite() && period > 0.0) {
        return Err(format!("drive profile deadman.timeout_s cannot be checked: the model's control.period_s is {period}, not a positive period"));
    }
    let timeout = profile.deadman.timeout_s;
    if !(timeout > period) {
        return Err(format!(
            "drive profile deadman.timeout_s {timeout} s is not longer than the model's control period (control.period_s {period} s): every request would be lost before the next control step"
        ));
    }
    Ok(())
}

/// The fastest one wheel joint can turn (rad/s) and how that was found: its
/// motor's gearbox `max_output_speed` divided by the motor's extra joint
/// `gear_ratio` (cad/PHYSICAL_MODEL.md "Motor"; the motor unit's total
/// ratio is gearbox.ratio × gear_ratio, crates/sim-domain-robot/src/motor.rs),
/// or, for a joint without a motor that is the driven joint of a
/// transmission, the driver joint's motor speed divided by |ratio| (driver
/// angle = ratio × driven angle).
fn joint_speed_limit(model: &PhysicalModel, joint: &str) -> Result<(f64, String), String> {
    match motors_on(model, joint).as_slice() {
        [motor] => return motor_speed(motor, joint),
        [] => {}
        many => {
            let names: Vec<&str> = many.iter().map(|m| m.name.as_str()).collect();
            return Err(format!("wheel joint `{joint}` is driven by {} motors ({}); the drive check needs one", many.len(), names.join(", ")));
        }
    }
    let through: Vec<_> = model.transmissions.iter().filter(|t| t.driven_joint == joint).collect();
    if let [t] = through.as_slice() {
        if let [motor] = motors_on(model, t.driver_joint.as_str()).as_slice() {
            if !(t.ratio.is_finite() && t.ratio != 0.0) {
                return Err(format!("transmission `{}` driving wheel joint `{joint}` has ratio {}; it must be finite and nonzero", t.name, t.ratio));
            }
            let (speed, how) = motor_speed(motor, t.driver_joint.as_str())?;
            return Ok((speed / t.ratio.abs(), format!("{how}, through transmission `{}` / |ratio {}|", t.name, t.ratio)));
        }
    }
    Err(format!(
        "wheel joint `{joint}` has no motor (no motors[].joint names it, nor drives it through a transmission); the drive profile's max speeds cannot be checked against a free-running wheel speed"
    ))
}

fn motors_on<'a>(model: &'a PhysicalModel, joint: &str) -> Vec<&'a Motor> {
    model.motors.iter().filter(|m| m.joint.as_deref() == Some(joint)).collect()
}

/// One motor's joint speed limit (rad/s) and its derivation.
fn motor_speed(motor: &Motor, on: &str) -> Result<(f64, String), String> {
    let (out, ratio) = (motor.gearbox.max_output_speed, motor.gear_ratio);
    if !(out.is_finite() && out > 0.0) {
        return Err(format!(
            "motor `{}` on joint `{on}` declares no finite gearbox.max_output_speed ({out}); the drive profile's max speeds are checked against it",
            motor.name
        ));
    }
    if !(ratio.is_finite() && ratio > 0.0) {
        return Err(format!("motor `{}` on joint `{on}` has gear_ratio {ratio}; it must be positive", motor.name));
    }
    Ok((out / ratio, format!("motor `{}` on joint `{on}`: gearbox.max_output_speed {out} rad/s / gear_ratio {ratio}", motor.name)))
}

/// Each supported axis's max speed must be one the wheel motors can reach.
/// The free-running wheel speed is `v_free` = the slowest wheel joint's speed
/// limit (`joint_speed_limit`) × wheel radius. Differential: forward ≤
/// v_free, yaw ≤ 2·v_free / track. Mecanum: forward and lateral ≤ v_free,
/// yaw ≤ v_free / ((track + wheelbase) / 2). Each axis alone; a combined
/// request can still ask more of the outer wheels.
pub fn check_speeds(model: &PhysicalModel, profile: &DriveProfile, geometry: &DriveGeometry) -> Result<(), String> {
    let mut slowest: Option<(f64, String)> = None;
    for wheel in &geometry.wheels {
        let (speed, how) = joint_speed_limit(model, &wheel.joint).map_err(|e| format!("drive profile max speeds: {e}"))?;
        if slowest.as_ref().is_none_or(|(s, _)| speed < *s) {
            slowest = Some((speed, how));
        }
    }
    let Some((joint_speed, how)) = slowest else {
        return Err("drive profile max speeds: the geometry lists no wheels".into());
    };
    let radius = geometry.wheel_radius_m.value;
    let radius_kind = match geometry.wheel_radius_m.provenance {
        Provenance::Derived { .. } => "derived",
        Provenance::Declared { .. } => "declared",
        Provenance::Estimated { .. } => "estimated",
        Provenance::Measured { .. } => "measured",
    };
    let v_free = joint_speed * radius;
    let free = format!("free-running wheel speed v_free = {how} = {joint_speed} rad/s (the slowest wheel) × wheel radius {radius} m ({radius_kind}) = {v_free} m/s");
    let track = geometry.track_width_m.value;
    let (lever, lever_text) = match (&profile.kinematics, &geometry.wheelbase_m) {
        (KinematicsSpec::Mecanum { .. }, Some(base)) => (0.5 * (track + base.value), format!("(track {track} m + wheelbase {} m) / 2", base.value)),
        (KinematicsSpec::Mecanum { .. }, None) => return Err("drive profile max speeds: a mecanum geometry needs wheelbase_m".into()),
        (KinematicsSpec::Differential { .. }, _) => (0.5 * track, format!("track {track} m / 2")),
    };
    let ceilings = [("forward", v_free, "m/s", "v_free".to_string()), ("lateral", v_free, "m/s", "v_free".to_string()), ("yaw", v_free / lever, "rad/s", format!("v_free / ({lever_text})"))];
    for (axis, ceiling, unit, rule) in ceilings {
        let Some(spec) = profile.axes.get(axis) else { continue };
        let asked = spec.max_speed.value;
        if asked > ceiling {
            return Err(format!(
                "drive profile axes.{axis}.max_speed {asked} {unit} is more than the motors can reach: {rule} = {ceiling} {unit}, with {free}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn robot() -> PhysicalModel {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/wheeled-robot/baseline/robot.simrobot.json");
        PhysicalModel::load(path).unwrap()
    }
    fn differential(left: &str, right: &str) -> KinematicsSpec {
        KinematicsSpec::Differential { left: left.into(), right: right.into() }
    }

    #[test]
    fn the_wheeled_robot_derives_its_drive_geometry() {
        let g = derive(&robot(), &differential("left axle", "right axle")).unwrap();
        // Joint origins y = ±0.06 m (left +y), axes [0, 1, 0], wheel collision radius 0.03 m.
        assert!((g.track_width_m.value - 0.12).abs() < 1e-12, "{:?}", g.track_width_m);
        assert!((g.wheel_radius_m.value - 0.03).abs() < 1e-5, "{:?}", g.wheel_radius_m);
        assert_eq!(g.wheels.iter().map(|w| (w.joint.as_str(), w.sign)).collect::<Vec<_>>(), [("left axle", 1.0), ("right axle", 1.0)]);
        assert!(g.wheelbase_m.is_none());
        let Provenance::Derived { from } = &g.track_width_m.provenance else { panic!("derived") };
        assert!(from.contains("left axle") && from.contains("right axle") && from.contains("chassis"), "{from}");
        let Provenance::Derived { from } = &g.wheel_radius_m.provenance else { panic!("derived") };
        assert!(from.contains("left wheel") && from.contains("right wheel"), "{from}");
        // The geometry builds the mixer: 0.03 m/s straight is 1 rad/s on both wheels.
        let mixer = g.differential().unwrap();
        let rates = mixer.mix(sim_domain_control::drive::BodyTwist::new(0.03, 0.0, 0.0)).unwrap();
        assert!((rates[0] - 0.03 / g.wheel_radius_m.value).abs() < 1e-12 && rates[0] == rates[1]);
    }

    #[test]
    fn missing_or_inconsistent_wheels_are_refused_by_name() {
        let model = robot();
        let e = derive(&model, &differential("left axle", "front axle")).unwrap_err();
        assert!(e.contains("no joint `front axle`") && e.contains("kinematics.right"), "{e}");
        // The passive wheel is smaller (0.02 m): both radii are named.
        let e = derive(&model, &differential("left axle", "passive axle")).unwrap_err();
        assert!(e.contains("radii") && e.contains("left wheel") && e.contains("passive wheel"), "{e}");
        // Swapped sides would invert yaw.
        let e = derive(&model, &differential("right axle", "left axle")).unwrap_err();
        assert!(e.contains("not to the left"), "{e}");

        let mut tilted = model.clone();
        tilted.joints.iter_mut().find(|j| j.name == "left axle").unwrap().axis = [1.0, 0.0, 0.0];
        let e = derive(&tilted, &differential("left axle", "right axle")).unwrap_err();
        assert!(e.contains("left axle") && e.contains("lateral axis"), "{e}");

        let mut bare = model.clone();
        bare.links.iter_mut().find(|l| l.name == "right wheel").unwrap().collision.vertices.clear();
        let e = derive(&bare, &differential("left axle", "right axle")).unwrap_err();
        assert!(e.contains("right wheel") && e.contains("no collision vertices"), "{e}");

        let mut fixed = model.clone();
        fixed.joints.iter_mut().find(|j| j.name == "right axle").unwrap().kind = "fixed".into();
        let e = derive(&fixed, &differential("left axle", "right axle")).unwrap_err();
        assert!(e.contains("fixed joint"), "{e}");

        // A revolute joint has travel limits: refused, with its limits named.
        let mut limited = model.clone();
        let j = limited.joints.iter_mut().find(|j| j.name == "left axle").unwrap();
        j.kind = "revolute".into();
        j.limits = Some([-1.0, 1.0]);
        let e = derive(&limited, &differential("left axle", "right axle")).unwrap_err();
        assert!(e.contains("left axle") && e.contains("revolute joint with limits [-1, 1]") && e.contains("continuous"), "{e}");

        // The model does not describe mecanum rollers.
        let mecanum = KinematicsSpec::Mecanum {
            front_left: "left axle".into(),
            front_right: "right axle".into(),
            rear_left: "passive axle".into(),
            rear_right: "rear".into(),
        };
        let e = derive(&model, &mecanum).unwrap_err();
        assert!(e.contains("does not describe mecanum roller layout") && e.contains("declare the geometry with its source"), "{e}");
    }

    fn example_profile() -> DriveProfile {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/wheeled-robot/baseline/robot.drive.json");
        DriveProfile::load(&path).unwrap_or_else(|e| panic!("{e}")).0
    }

    #[test]
    fn the_example_profile_resolves_within_the_motors_and_period() {
        // v_free = 14.6608 rad/s (N20 gearbox max_output_speed, gear_ratio 1)
        // × 0.0300007 m ≈ 0.4398 m/s; yaw ceiling 2·v_free/0.12 ≈ 7.33 rad/s.
        // The profile asks 0.26 m/s and 4.3 rad/s, deadman 0.5 s > 0.02 s.
        let g = resolve(&robot(), &example_profile()).unwrap();
        assert!((g.track_width_m.value - 0.12).abs() < 1e-12);
    }

    #[test]
    fn profiles_beyond_the_model_are_refused_by_field() {
        let model = robot();
        let mut p = example_profile();
        p.deadman.timeout_s = 0.02;
        let e = resolve(&model, &p).unwrap_err();
        assert!(e.contains("deadman.timeout_s 0.02") && e.contains("control.period_s 0.02"), "{e}");

        let mut p = example_profile();
        p.axes.get_mut("forward").unwrap().max_speed.value = 0.5;
        let e = resolve(&model, &p).unwrap_err();
        assert!(e.contains("axes.forward.max_speed 0.5 m/s") && e.contains("max_output_speed") && e.contains("v_free"), "{e}");

        let mut p = example_profile();
        p.axes.get_mut("yaw").unwrap().max_speed.value = 8.0;
        let e = resolve(&model, &p).unwrap_err();
        assert!(e.contains("axes.yaw.max_speed 8 rad/s") && e.contains("v_free / (track"), "{e}");

        // A wheel joint no motor drives cannot be checked.
        let mut motorless = model.clone();
        motorless.motors.retain(|m| m.joint.as_deref() != Some("right axle"));
        let e = resolve(&motorless, &example_profile()).unwrap_err();
        assert!(e.contains("wheel joint `right axle` has no motor"), "{e}");

        // An unbounded (omitted) gearbox speed is not filled in.
        let mut unbounded = model.clone();
        unbounded.motors.iter_mut().for_each(|m| m.gearbox.max_output_speed = f64::INFINITY);
        let e = resolve(&unbounded, &example_profile()).unwrap_err();
        assert!(e.contains("gearbox.max_output_speed"), "{e}");
    }

    #[test]
    fn a_reversed_axle_flips_its_sign() {
        let mut model = robot();
        model.joints.iter_mut().find(|j| j.name == "right axle").unwrap().axis = [0.0, -1.0, 0.0];
        let g = derive(&model, &differential("left axle", "right axle")).unwrap();
        assert_eq!([g.wheels[0].sign, g.wheels[1].sign], [1.0, -1.0]);
    }

    #[test]
    fn declared_geometry_keeps_its_sources() {
        let text = r#"{"schema":"sim.drive/1",
            "kinematics":{"type":"differential","left":"left axle","right":"right axle"},
            "geometry":{"source":"declared","track_width_m":{"value":0.13,"source":"caliper, 2026-10-01"},
                        "wheel_radius_m":{"value":0.031,"source":"tyre datasheet"},"signs":[1,-1]},
            "axes":{"forward":{"max_speed":{"value":0.3,"unit":"m/s"},"max_accel":{"value":0.6,"unit":"m/s^2"},"stop_decel":{"value":1.2,"unit":"m/s^2"},"provenance":{"kind":"estimated","source":"test"}}},
            "actions":[],
            "deadman":{"timeout_s":0.5,"on_loss":"immediate","provenance":{"kind":"estimated","source":"test"}}}"#;
        let profile = DriveProfile::from_json(text, Path::new("test.drive.json")).unwrap();
        let g = resolve(&robot(), &profile).unwrap();
        assert_eq!(g.track_width_m.value, 0.13);
        assert_eq!(g.track_width_m.provenance, Provenance::Declared { source: "caliper, 2026-10-01".into() });
        assert_eq!([g.wheels[0].sign, g.wheels[1].sign], [1.0, -1.0]);
        let resolved = profile.resolve(Path::new("test.drive.json"), "abc", g).unwrap();
        assert_eq!(resolved.kinematics, "differential");
        assert_eq!(resolved.limits.supported, [true, false, false]);

        let mut value: serde_json::Value = serde_json::from_str(text).unwrap();
        value["geometry"] = serde_json::json!({"source": "model"});
        let model_text = value.to_string();
        let profile = DriveProfile::from_json(&model_text, Path::new("test.drive.json")).unwrap();
        assert!(matches!(resolve(&robot(), &profile).unwrap().track_width_m.provenance, Provenance::Derived { .. }));
    }
}
