//! Robot parts in process: the motor library and a motor's solid, the
//! cutter for its mounting holes, fastener cutters, joint inference from
//! coaxial cylinders, and the robot summary with its topology checks.
//! Reference: RoboCAD's robotics.py (`MotorSpec`, `MOTOR_LIBRARY`,
//! `motor_body`, `motor_mount_holes_tool`, `infer_joints`, `robot_summary`,
//! `validate_robot`) and printing.py (`METRIC`, `fastener_tool`).
//!
//! The library's numbers are RoboCAD's catalogue values (datasheet or
//! envelope estimates as its notes say), not the accepted actuator registry,
//! which remains the source of measured actuator models.
use crate::kernel::{self, Shape};
use crate::sketch::{Plane, Sketch};
use crate::ArchiveDocument;
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const JOINT_TYPES: [&str; 7] = ["revolute", "continuous", "prismatic", "fixed", "ball", "loop_revolute", "loop_spherical"];

/// An actuator of the library (`MotorSpec`): sizes mm, mass g, torque N·m
/// and speed rad/s at the output shaft (a linear actuator: N and m/s).
#[derive(Clone, Debug, Serialize)]
pub struct MotorSpec {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub shape: String,
    pub size: [f64; 3],
    pub shaft_diameter: f64,
    pub shaft_length: f64,
    pub mass_g: f64,
    pub stall_torque: f64,
    pub no_load_speed: f64,
    pub gear_ratio: f64,
    pub voltage: f64,
    pub rotor_inertia: f64,
    pub mount_holes: Vec<[f64; 3]>,
    pub flange: Option<[f64; 2]>,
    pub stroke: f64,
    pub color: [f64; 3],
    pub notes: String,
}

#[allow(clippy::too_many_arguments)]
fn spec(id: &str, name: &str, kind: &str, shape: &str, size: [f64; 3], shaft_diameter: f64, shaft_length: f64, mass_g: f64, stall_torque: f64, no_load_speed: f64, gear_ratio: f64, voltage: f64, rotor_inertia: f64, mount_holes: &[[f64; 3]], flange: Option<[f64; 2]>, stroke: f64, color: [f64; 3], notes: &str) -> MotorSpec {
    MotorSpec { id: id.into(), name: name.into(), kind: kind.into(), shape: shape.into(), size, shaft_diameter, shaft_length, mass_g, stall_torque, no_load_speed, gear_ratio, voltage, rotor_inertia, mount_holes: mount_holes.to_vec(), flange, stroke, color, notes: notes.into() }
}
fn nema(id: &str, name: &str, side: f64, length: f64, shaft: f64, mass: f64, torque: f64, holes: f64, hole_d: f64, flange: [f64; 2]) -> MotorSpec {
    let h = holes / 2.;
    spec(id, name, "stepper", "box", [side, side, length], shaft, 22., mass, torque, std::f64::consts::TAU * 5., 1., 12., if side > 40. { 5.4e-6 } else { 2.0e-6 }, &[[h, h, hole_d], [-h, h, hole_d], [h, -h, hole_d], [-h, -h, hole_d]], Some(flange), 0., [0.2, 0.21, 0.24], "1.8° bipolar stepper, ~5 rev/s usable")
}

/// RoboCAD's `MOTOR_LIBRARY`, in its order.
pub fn library() -> Vec<MotorSpec> {
    use std::f64::consts::TAU;
    let rad60 = 60f64.to_radians();
    let grey = [0.25, 0.27, 0.31];
    vec![
        spec("hx30hm", "Hiwonder HX-30HM serial bus servo", "servo", "box", [45.2, 24.7, 35.0], 6.0, 4.0, 52.0, 30.0 * 0.0980665, rad60 / 0.19, 1.0, 11.1, 0.0, &[], None, 0.0, grey, "Manufacturer: https://www.hiwonder.com/products/hx-30hm . 30 kgf cm at 11.1 V, 52 g, 0.19 s/60 deg. Shaft dimensions are envelope estimates; retain imported CAD. Internal dynamics are provisional equivalent output-shaft parameters; bus IDs and calibration are not assigned."),
        nema("nema14", "NEMA 14 stepper", 35.2, 34.0, 5.0, 200.0, 0.14, 26.0, 3.0, [22.0, 2.0]),
        nema("nema17", "NEMA 17 stepper", 42.3, 40.0, 5.0, 280.0, 0.40, 31.0, 3.0, [22.0, 2.0]),
        nema("nema17_pancake", "NEMA 17 pancake stepper", 42.3, 24.0, 5.0, 170.0, 0.16, 31.0, 3.0, [22.0, 2.0]),
        nema("nema23", "NEMA 23 stepper", 56.4, 56.0, 6.35, 700.0, 1.26, 47.1, 4.5, [38.1, 1.6]),
        spec("sg90", "SG90 micro servo", "servo", "box", [22.8, 12.2, 22.5], 4.8, 3.5, 9.0, 0.18, rad60 / 0.1, 1.0, 5.0, 0.0, &[[0.0, 13.9, 2.0], [0.0, -13.9, 2.0]], Some([5.9, 2.0]), 0.0, [0.3, 0.55, 0.85], "180° hobby servo; spline output; tabs at ±13.9 mm on the long axis"),
        spec("mg90s", "MG90S metal-gear micro servo", "servo", "box", [22.8, 12.2, 22.5], 4.8, 3.5, 13.4, 0.22, rad60 / 0.08, 1.0, 5.0, 0.0, &[[0.0, 13.9, 2.0], [0.0, -13.9, 2.0]], Some([5.9, 2.0]), 0.0, [0.3, 0.55, 0.85], "180° hobby servo, metal gears"),
        spec("mg996r", "MG996R servo", "servo", "box", [40.7, 19.7, 42.9], 5.9, 4.0, 55.0, 1.0, rad60 / 0.17, 1.0, 6.0, 0.0, &[[0.0, 24.5, 4.2], [0.0, -24.5, 4.2], [10.0, 24.5, 4.2], [10.0, -24.5, 4.2]], Some([7.0, 2.5]), 0.0, [0.2, 0.2, 0.22], "standard-size 180° servo, 25T spline"),
        spec("ds3218", "DS3218 20 kg servo", "servo", "box", [40.0, 20.0, 40.5], 5.9, 4.0, 60.0, 2.0, rad60 / 0.16, 1.0, 6.8, 0.0, &[[0.0, 24.5, 4.2], [0.0, -24.5, 4.2], [10.0, 24.5, 4.2], [10.0, -24.5, 4.2]], Some([7.0, 2.5]), 0.0, [0.15, 0.15, 0.17], "standard-size 270° high-torque servo"),
        spec("n20_100", "N20 gearmotor 100:1", "dc_gearmotor", "box", [12.0, 10.0, 24.0], 3.0, 9.0, 10.0, 0.20, TAU * 140. / 60., 100.0, 6.0, 1.0e-8, &[[4.5, 0.0, 1.6], [-4.5, 0.0, 1.6]], Some([4.0, 1.0]), 0.0, [0.75, 0.72, 0.6], "micro metal gearmotor, D shaft; 140 rpm"),
        spec("n20_298", "N20 gearmotor 298:1", "dc_gearmotor", "box", [12.0, 10.0, 24.0], 3.0, 9.0, 10.0, 0.45, TAU * 45. / 60., 298.0, 6.0, 1.0e-8, &[[4.5, 0.0, 1.6], [-4.5, 0.0, 1.6]], Some([4.0, 1.0]), 0.0, [0.75, 0.72, 0.6], "micro metal gearmotor; 45 rpm"),
        spec("ga25_150", "25GA-370 gearmotor 150:1", "dc_gearmotor", "cylinder", [25.0, 0.0, 60.0], 4.0, 12.0, 90.0, 0.9, TAU * 100. / 60., 150.0, 12.0, 5.0e-8, &[[8.5, 0.0, 2.5], [-8.5, 0.0, 2.5], [0.0, 8.5, 2.5], [0.0, -8.5, 2.5]], Some([7.0, 2.0]), 0.0, [0.6, 0.62, 0.65], "25 mm gearmotor with encoder; 100 rpm"),
        spec("gb37_100", "37GB-520 gearmotor 100:1", "dc_gearmotor", "cylinder", [37.0, 0.0, 70.0], 6.0, 15.0, 200.0, 2.5, TAU * 110. / 60., 100.0, 12.0, 1.2e-7, &[[15.5, 0.0, 3.0], [-15.5, 0.0, 3.0], [0.0, 15.5, 3.0], [0.0, -15.5, 3.0], [11.0, 11.0, 3.0], [-11.0, -11.0, 3.0]], Some([12.0, 2.0]), 0.0, [0.6, 0.62, 0.65], "37 mm gearmotor; 110 rpm"),
        spec("gm2804", "GM2804 gimbal BLDC", "bldc", "cylinder", [35.0, 0.0, 26.0], 4.0, 6.0, 40.0, 0.12, TAU * 6.0, 1.0, 12.0, 4.0e-6, &[[9.5, 9.5, 2.0], [-9.5, 9.5, 2.0], [9.5, -9.5, 2.0], [-9.5, -9.5, 2.0]], Some([8.0, 1.0]), 0.0, [0.3, 0.3, 0.32], "direct-drive gimbal motor, FOC"),
        spec("d5065", "D5065 270KV BLDC", "bldc", "cylinder", [65.0, 0.0, 50.0], 8.0, 20.0, 420.0, 1.6, TAU * 40.0, 1.0, 24.0, 6.4e-5, &[[12.5, 12.5, 4.0], [-12.5, 12.5, 4.0], [12.5, -12.5, 4.0], [-12.5, -12.5, 4.0]], Some([20.0, 2.0]), 0.0, [0.35, 0.36, 0.4], "outrunner for robot actuators (ODrive class)"),
        spec("cycloid_8108", "8108 BLDC + 9:1 planetary", "bldc", "cylinder", [88.0, 0.0, 60.0], 12.0, 12.0, 700.0, 12.0, TAU * 3.0, 9.0, 24.0, 9.0e-5, &[[35.0, 0.0, 4.0], [-35.0, 0.0, 4.0], [0.0, 35.0, 4.0], [0.0, -35.0, 4.0]], Some([30.0, 3.0]), 0.0, [0.28, 0.3, 0.34], "quasi-direct-drive leg actuator"),
        spec("linear_l12", "L12 micro linear actuator 50 mm", "linear", "box", [15.0, 12.0, 85.0], 4.0, 50.0, 40.0, 40.0, 0.012, 100.0, 6.0, 0.0, &[[0.0, 0.0, 4.0]], None, 50.0, [0.25, 0.28, 0.3], "force 40 N (stall_torque field = N), speed 12 mm/s (no_load_speed = m/s)"),
    ]
}

/// The library as `GET /motors` answers it: id → spec.
pub fn library_json() -> Value {
    let map: Map<String, Value> = library().into_iter().map(|m| (m.id.clone(), serde_json::to_value(m).expect("serializes"))).collect();
    Value::Object(map)
}

pub fn motor(id: &str) -> Result<MotorSpec, String> {
    library().into_iter().find(|m| m.id == id).ok_or_else(|| format!("unknown motor {id}; see the library"))
}

fn frame(mount: [f64; 3], shaft: [f64; 3], rotation_deg: f64) -> Plane {
    let base = crate::ops::plane_from_normal(mount, shaft);
    if rotation_deg == 0. {
        return base;
    }
    let a = rotation_deg.to_radians();
    let y = base.y_axis();
    let x = [0, 1, 2].map(|i| base.x_axis[i] * a.cos() + y[i] * a.sin());
    Plane { origin: mount, normal: base.normal, x_axis: x }
}
fn along(p: [f64; 3], n: [f64; 3], k: f64) -> [f64; 3] {
    [p[0] + n[0] * k, p[1] + n[1] * k, p[2] + n[2] * k]
}

/// `motor_body`: the housing behind the mount face, the pilot flange, the
/// shaft (a linear actuator's rod) and the mounting holes, as one solid;
/// and the motor's robot metadata (world frame).
pub fn motor_body(spec: &MotorSpec, mount: [f64; 3], shaft_dir: [f64; 3], rotation_deg: f64, c: &dyn Fn() -> bool) -> Result<(Vec<u8>, Map<String, Value>), String> {
    let f = frame(mount, shaft_dir, rotation_deg);
    let n = f.normal;
    let behind = n.map(|v| -v);
    let [w, d, l] = spec.size;
    let mut body = if spec.shape == "box" {
        let mut sk = Sketch::new(f, "motor");
        sk.call(&json!(["rectangle_center", [[0.0, 0.0], [w, d]]]))?;
        let face = sk.profile(&[], c)?;
        kernel::op1(kernel::Op::Extrude, &[&face], &[&behind[..], &[l, 0., 0.]].concat(), &[], c)?.brep
    } else {
        kernel::build(&Shape::Cylinder { base: mount, axis: behind, radius: w / 2., height: l }, c)?
    };
    let fuse = |a: Vec<u8>, b: Vec<u8>| kernel::build(&Shape::Fuse(vec![&a, &b]), c);
    if let Some([fd, fh]) = spec.flange {
        body = fuse(body, kernel::build(&Shape::Cylinder { base: mount, axis: n, radius: fd / 2., height: fh }, c)?)?;
    }
    let shaft_len = spec.shaft_length + spec.flange.map_or(0., |f| f[1]);
    let rod = if spec.kind == "linear" { spec.stroke + 10. } else { shaft_len };
    body = fuse(body, kernel::build(&Shape::Cylinder { base: mount, axis: n, radius: spec.shaft_diameter / 2., height: rod }, c)?)?;
    for [hx, hy, hd] in &spec.mount_holes {
        if (hx.abs() < w / 2. + 1e-6 && hy.abs() < d / 2. + 1e-6) || spec.shape == "cylinder" {
            let p = f.to_world([*hx, *hy]);
            let hole = kernel::build(&Shape::Cylinder { base: along(p, n, 1.), axis: behind, radius: hd / 2., height: 8. }, c)?;
            if let Ok(cut) = kernel::build(&Shape::Cut(vec![&body, &hole]), c) {
                body = cut;
            }
        }
    }
    let meta = json!({"kind": "motor", "spec": spec.id, "mount_point": mount, "shaft_axis": n, "shaft_tip": along(mount, n, shaft_len), "rotation_deg": rotation_deg, "mounted_on": null, "drives": null});
    Ok((body, meta.as_object().cloned().expect("object")))
}

/// `motor_mount_holes_tool`: the shaft/flange pass-through and mounting-hole
/// pattern (0.3 mm clearance, 20 mm deep) to cut from the bracket.
pub fn mount_holes_tool(spec: &MotorSpec, mount: [f64; 3], shaft_dir: [f64; 3], rotation_deg: f64, c: &dyn Fn() -> bool) -> Result<Vec<u8>, String> {
    let (depth, clearance) = (20., 0.3);
    let f = frame(mount, shaft_dir, rotation_deg);
    let n = f.normal;
    let fd = spec.flange.map_or(spec.shaft_diameter, |f| f[0]) + 2. * clearance;
    let mut tool = kernel::build(&Shape::Cylinder { base: along(mount, n, -1.), axis: n, radius: fd / 2., height: depth + 1. }, c)?;
    for [hx, hy, hd] in &spec.mount_holes {
        let p = f.to_world([*hx, *hy]);
        let hole = kernel::build(&Shape::Cylinder { base: along(p, n, -1.), axis: n, radius: (hd + clearance) / 2., height: depth + 1. }, c)?;
        tool = kernel::build(&Shape::Fuse(vec![&tool, &hole]), c)?;
    }
    Ok(tool)
}

/// printing.py's `METRIC`: clearance, tap, counterbore (d, depth),
/// countersink d, insert (d, depth), head.
fn metric(size: &str) -> Option<(f64, f64, [f64; 2], f64, [f64; 2])> {
    Some(match size {
        "M2" => (2.4, 1.6, [4.4, 2.2], 4.0, [3.2, 4.0]),
        "M2.5" => (2.9, 2.05, [5.4, 2.7], 5.0, [3.8, 5.0]),
        "M3" => (3.4, 2.5, [6.5, 3.2], 6.3, [4.0, 5.7]),
        "M4" => (4.5, 3.3, [8.0, 4.2], 8.4, [5.6, 8.0]),
        "M5" => (5.5, 4.2, [10.0, 5.2], 10.4, [6.4, 9.5]),
        "M6" => (6.6, 5.0, [11.0, 6.2], 12.6, [8.0, 12.7]),
        "M8" => (9.0, 6.8, [15.0, 8.2], 16.5, [9.6, 12.7]),
        _ => return None,
    })
}

/// `fastener_tool`: the cutter for a fastener hole entering at `point`
/// along `-normal`; `spec` is `{size, kind, extra_clearance, depth}`.
pub fn fastener_tool(point: [f64; 3], normal: [f64; 3], spec: &Value, depth: f64, c: &dyn Fn() -> bool) -> Result<Vec<u8>, String> {
    let size = spec["size"].as_str().unwrap_or("M3");
    let kind = spec["kind"].as_str().unwrap_or("clearance");
    let extra = spec["extra_clearance"].as_f64().unwrap_or(0.);
    let depth = spec["depth"].as_f64().unwrap_or(depth);
    let (clear, tap, cbore, csk, insert) = metric(size).ok_or_else(|| format!("unknown fastener size {size} (M2–M8)"))?;
    let d = match kind {
        "clearance" | "counterbore" | "countersink" => clear,
        "tap" => tap,
        "insert" => insert[0],
        other => return Err(format!("fastener kind must be clearance, tap, counterbore, countersink or insert, not {other}")),
    } + extra;
    let n = crate::ops::plane_from_normal([0.; 3], normal).normal;
    let start = along(point, n, 0.5);
    let axis = n.map(|v| -v);
    let cyl = |r: f64, h: f64| kernel::build(&Shape::Cylinder { base: start, axis, radius: r, height: h }, c);
    let fuse = |a: Vec<u8>, b: Vec<u8>| kernel::build(&Shape::Fuse(vec![&a, &b]), c);
    Ok(match kind {
        "counterbore" => fuse(cyl(d / 2., depth + 0.5)?, cyl((cbore[0] + extra) / 2., cbore[1] + 0.5)?)?,
        "countersink" => {
            let top = csk + extra;
            let cone = kernel::build(&Shape::Cone { base: start, axis, radius1: top / 2. + 0.5, radius2: d / 2., height: (top - d) / 2. + 0.5 }, c)?;
            fuse(cyl(d / 2., depth + 0.5)?, cone)?
        }
        "insert" => fuse(cyl((insert[0] + extra) / 2., insert[1] + 0.5)?, cyl(tap / 2. + 0.1, depth.min(insert[1] + 4.) + 0.5)?)?,
        _ => cyl(d / 2., depth + 0.5)?,
    })
}

/// `infer_joints`: revolute joints where a shaft sits in a bore of another
/// body (coaxial cylinders, radii within 0.6 mm, overlapping ≥ 1 mm); the
/// body with the hole is the parent. Joints already joining a pair are skipped.
pub fn infer_joints(cx: &crate::ops::Ctx) -> Result<Vec<Value>, String> {
    struct Cyl {
        node: String,
        hole: bool,
        radius: f64,
        point: [f64; 3],
        dir: [f64; 3],
        lo: f64,
        hi: f64,
    }
    let bodies: Vec<String> = cx.edit.manifest["nodes"].as_array().into_iter().flatten().filter(|n| n["kind"] == "body").filter_map(|n| n["id"].as_str().map(str::to_string)).collect();
    let mut cyls = Vec::new();
    for id in &bodies {
        let Ok(b) = cx.body(id) else { continue };
        let topo = kernel::topology(&b, cx.cancelled)?;
        for f in topo.faces.iter().filter(|f| f.kind == "cylinder") {
            let r = kernel::face_ref(&b, f.index as i32)?;
            let (Some(p), Some(d)) = (r.axis_point, r.axis_dir) else { continue };
            // The face's extent along its axis: its edges' ends projected.
            let ts: Vec<f64> = topo.edges.iter().flat_map(|e| [e.start, e.end]).filter(|q| {
                let v = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
                let t = v[0] * d[0] + v[1] * d[1] + v[2] * d[2];
                let perp = [v[0] - d[0] * t, v[1] - d[1] * t, v[2] - d[2] * t];
                ((perp[0] * perp[0] + perp[1] * perp[1] + perp[2] * perp[2]).sqrt() - r.radius).abs() < 1e-3
            }).map(|q| (q[0] - p[0]) * d[0] + (q[1] - p[1]) * d[1] + (q[2] - p[2]) * d[2]).collect();
            if ts.is_empty() {
                continue;
            }
            cyls.push(Cyl { node: id.clone(), hole: r.hole, radius: r.radius, point: p, dir: d, lo: ts.iter().copied().fold(f64::INFINITY, f64::min), hi: ts.iter().copied().fold(f64::NEG_INFINITY, f64::max) });
        }
    }
    let existing: HashSet<(String, String)> = cx.edit.manifest["nodes"].as_array().into_iter().flatten().filter(|n| n["kind"] == "joint").map(|n| (n["joint"]["parent"].as_str().unwrap_or("").to_string(), n["joint"]["child"].as_str().unwrap_or("").to_string())).collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (i, a) in cyls.iter().enumerate() {
        for b in &cyls[i + 1..] {
            if a.node == b.node || a.hole == b.hole || (a.radius - b.radius).abs() > 0.6 {
                continue;
            }
            let dd = a.dir[0] * b.dir[0] + a.dir[1] * b.dir[1] + a.dir[2] * b.dir[2];
            if (dd.abs() - 1.).abs() > 1e-3 {
                continue;
            }
            let off = [b.point[0] - a.point[0], b.point[1] - a.point[1], b.point[2] - a.point[2]];
            let t = off[0] * a.dir[0] + off[1] * a.dir[1] + off[2] * a.dir[2];
            let perp = [off[0] - a.dir[0] * t, off[1] - a.dir[1] * t, off[2] - a.dir[2] * t];
            if (perp[0] * perp[0] + perp[1] * perp[1] + perp[2] * perp[2]).sqrt() > 0.6 {
                continue;
            }
            let s = if dd > 0. { 1. } else { -1. };
            let (b0, b1) = (t + s * b.lo, t + s * b.hi);
            let (lo, hi) = (a.lo.max(b0.min(b1)), a.hi.min(b0.max(b1)));
            if hi - lo < 1. {
                continue;
            }
            let (parent, child) = if a.hole { (&a.node, &b.node) } else { (&b.node, &a.node) };
            let key = (parent.clone(), child.clone());
            if !seen.insert(key.clone()) || existing.contains(&key) {
                continue;
            }
            let m = 0.5 * (lo + hi);
            out.push(json!({"type": "revolute", "parent": parent, "child": child, "pivot": [a.point[0] + a.dir[0] * m, a.point[1] + a.dir[1] * m, a.point[2] + a.dir[2] * m], "axis": a.dir, "lower": null, "upper": null, "motor": null, "gear_ratio": 1.0, "damping": 0.0, "friction": 0.0, "home": 0.0, "stroke": 0.0}));
        }
    }
    Ok(out)
}

fn dof(t: &str) -> i64 {
    match t {
        "revolute" | "continuous" | "prismatic" => 1,
        "ball" => 3,
        "loop_revolute" => -5,
        "loop_spherical" => -3,
        _ => 0,
    }
}

/// `robot_summary` with `validate_robot`'s topology checks (exact geometry
/// checks are not run: `validation_scope` says so).
pub fn summary(doc: &ArchiveDocument) -> Value {
    let nodes: Vec<&Value> = doc.manifest["nodes"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    let by_id: HashMap<&str, &Value> = nodes.iter().filter_map(|n| n["id"].as_str().map(|id| (id, *n))).collect();
    let name = |id: &Value| id.as_str().and_then(|i| by_id.get(i)).and_then(|n| n["name"].as_str()).map(str::to_string);
    let lib: BTreeMap<String, MotorSpec> = library().into_iter().map(|m| (m.id.clone(), m)).collect();
    let joints: Vec<&Value> = nodes.iter().copied().filter(|n| n["kind"] == "joint" && n["joint"].is_object()).collect();
    let motors: Vec<&Value> = nodes.iter().copied().filter(|n| n["robot"]["kind"] == "motor").collect();
    let bodies: HashSet<&str> = nodes.iter().filter(|n| matches!(n["kind"].as_str(), Some("body"))).filter_map(|n| n["id"].as_str()).collect();
    let has_loops = joints.iter().any(|j| j["joint"]["type"].as_str().is_some_and(|t| t.starts_with("loop_")));
    let mut issues: Vec<Value> = Vec::new();
    let mut parents: HashMap<String, String> = HashMap::new();
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    for n in &joints {
        let j = &n["joint"];
        let nm = n["name"].as_str().unwrap_or("joint");
        let id = n["id"].clone();
        let mut issue = |sev: &str, msg: String| issues.push(json!({"severity": sev, "message": msg, "node": id}));
        let ty = j["type"].as_str().unwrap_or("");
        if !JOINT_TYPES.contains(&ty) {
            issue("error", format!("{nm}: unknown joint type {ty}"));
        }
        let child = j["child"].as_str().unwrap_or("");
        if !bodies.contains(child) {
            issue("error", format!("{nm}: child body is missing"));
            continue;
        }
        let parent = j["parent"].as_str();
        if parent.is_some_and(|p| !bodies.contains(p)) {
            issue("error", format!("{nm}: parent body is missing"));
            continue;
        }
        if parent == Some(child) {
            issue("error", format!("{nm}: a body cannot be jointed to itself"));
        }
        if ty.starts_with("loop_") {
            if parent.is_none() {
                issue("error", format!("{nm}: a loop joint needs two bodies"));
            }
            continue;
        }
        if parents.contains_key(child) {
            issue("error", format!("{nm}: {} already has a parent joint (the mechanism must be a tree; use a fixed joint or remove one)", name(&json!(child)).unwrap_or_default()));
        }
        parents.insert(child.into(), parent.unwrap_or("world").into());
        children.entry(parent.unwrap_or("world").into()).or_default().push(child.into());
        let axis = crate::ops::v3(&j["axis"]).unwrap_or([0.; 3]);
        if axis.iter().map(|v| v * v).sum::<f64>() < 1e-18 {
            issue("error", format!("{nm}: zero axis"));
        }
        if matches!(ty, "revolute" | "prismatic") && let (Some(lo), Some(hi)) = (j["lower"].as_f64(), j["upper"].as_f64()) && lo >= hi {
            issue("error", format!("{nm}: lower limit is not below the upper limit"));
        }
        match j["motor"].as_str() {
            Some(m) => match by_id.get(m).filter(|n| n["robot"]["kind"] == "motor") {
                None => issue("error", format!("{nm}: its motor is missing")),
                Some(mn) => {
                    let s = crate::ops::v3(&mn["robot"]["shaft_axis"]).unwrap_or([0., 0., 1.]);
                    let l = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
                    let d = (s[0] * axis[0] + s[1] * axis[1] + s[2] * axis[2]) / (l(s) * l(axis));
                    if (d.abs() - 1.).abs() > 1e-2 && ty != "prismatic" {
                        issue("warning", format!("{nm}: the motor shaft is not aligned with the joint axis"));
                    }
                }
            },
            None if matches!(ty, "revolute" | "continuous" | "prismatic") => issue("info", format!("{nm}: no motor assigned (passive joint)")),
            None => {}
        }
    }
    fn reaches(children: &HashMap<String, Vec<String>>, a: &str, target: &str, depth: usize) -> bool {
        depth > 100 || children.get(a).into_iter().flatten().any(|c| c == target || reaches(children, c, target, depth + 1))
    }
    for n in &joints {
        let j = &n["joint"];
        if let (Some(p), Some(c)) = (j["parent"].as_str(), j["child"].as_str())
            && !j["type"].as_str().unwrap_or("").starts_with("loop_")
            && reaches(&children, c, p, 0)
        {
            issues.push(json!({"severity": "error", "message": format!("{}: closes a loop in the tree; make it a loop_revolute/loop_spherical joint", n["name"].as_str().unwrap_or("joint")), "node": n["id"]}));
        }
    }
    json!({
        "joints": joints.iter().map(|n| {
            let mut j = n["joint"].clone();
            j["id"] = n["id"].clone();
            j["name"] = n["name"].clone();
            j["parent_name"] = json!(name(&n["joint"]["parent"]));
            j["child_name"] = json!(name(&n["joint"]["child"]));
            j["motor_name"] = json!(name(&n["joint"]["motor"]));
            j
        }).collect::<Vec<_>>(),
        "motors": motors.iter().map(|n| {
            let mut m = n["robot"].clone();
            m["id"] = n["id"].clone();
            m["name"] = n["name"].clone();
            let spec = n["robot"]["spec"].as_str().unwrap_or("");
            m["spec_name"] = json!(lib.get(spec).map_or(spec.to_string(), |s| s.name.clone()));
            m
        }).collect::<Vec<_>>(),
        "links": bodies.len(),
        "dof": if has_loops { Value::Null } else { json!(joints.iter().map(|n| dof(n["joint"]["type"].as_str().unwrap_or(""))).sum::<i64>()) },
        "has_closed_loops": has_loops,
        "ground": nodes.iter().filter(|n| n["kind"] == "body" && (n["name"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("ground")) || n["robot"]["ground"] == true)).map(|n| n["id"].clone()).collect::<Vec<_>>(),
        "issues": issues,
        "validation_scope": "topology only; exact geometry checks are explicit",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_matches_robocads_and_motors_build() {
        let lib = library();
        assert_eq!(lib.len(), 17);
        let n17 = motor("nema17").unwrap();
        assert_eq!(n17.mount_holes.len(), 4);
        assert!((n17.mount_holes[0][0] - 15.5).abs() < 1e-12);
        let (body, meta) = motor_body(&motor("sg90").unwrap(), [0.; 3], [0., 0., 1.], 0., &|| false).unwrap();
        assert_eq!(meta["kind"], "motor");
        assert!(!body.is_empty());
        let tool = fastener_tool([0.; 3], [0., 0., 1.], &json!({"size": "M3", "kind": "counterbore"}), 10., &|| false).unwrap();
        assert!(!tool.is_empty());
        assert!(motor("nope").is_err());
    }
}
