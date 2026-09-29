//! Joints across a seam and what they can carry. The formulas are the ones
//! the library parts and lessons use (`part.dowel_pin`, `part.threaded_joint`,
//! `part.dovetail`), fed from the registry.

use crate::mesh::{V3, cross, dot, norm, scale, sub, unit};
use crate::registry::{Material, Registry};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Joint {
    /// A steel dowel pin across the seam: locates and carries shear only.
    Dowel { at: V3, diameter_mm: f64, depth_minus_mm: f64, depth_plus_mm: f64 },
    /// A screw from the plus piece into a heat-set insert in the minus piece.
    InsertScrew { at: V3, size: String, screw_length_mm: f64, clamp_mm: f64 },
    /// A dovetail rail on the minus piece sliding into a groove in the plus piece.
    Dovetail { at: V3, along: V3, rail_length_mm: f64, neck_mm: f64, depth_mm: f64 },
}

impl Joint {
    pub fn at(&self) -> V3 {
        match self {
            Joint::Dowel { at, .. } | Joint::InsertScrew { at, .. } | Joint::Dovetail { at, .. } => *at,
        }
    }
    pub fn kind(&self) -> &'static str {
        match self {
            Joint::Dowel { .. } => "dowel",
            Joint::InsertScrew { .. } => "insert_screw",
            Joint::Dovetail { .. } => "dovetail",
        }
    }
}

/// What one joint can carry (N): pulling the seam apart, and sliding it.
#[derive(Clone, Debug, Serialize)]
pub struct Capacity {
    pub tension_n: f64,
    pub shear_n: f64,
    /// Which limit sets each.
    pub tension_limit: String,
    pub shear_limit: String,
}

pub fn capacity(joint: &Joint, registry: &Registry, material: &Material, strength_factor: f64) -> Result<Capacity, String> {
    let bearing = material.bearing.value * strength_factor;
    Ok(match joint {
        Joint::Dowel { diameter_mm, depth_minus_mm, depth_plus_mm, .. } => {
            // Bearing of the pin on the shorter hole's wall (projected area d·L).
            let l = depth_minus_mm.min(*depth_plus_mm);
            Capacity { tension_n: 0., shear_n: bearing * diameter_mm * l * 1e-6, tension_limit: "a slip-fit pin does not hold the seam closed".into(), shear_limit: format!("pin bearing on {l:.1} mm of printed hole") }
        }
        Joint::InsertScrew { size, clamp_mm, .. } => {
            let insert = registry.joints.get("heat_set_insert").and_then(|v| v.get(size)).ok_or_else(|| format!("joints.heat_set_insert.{size} is not in the registry"))?;
            let get = |k: &str| insert.get(k).and_then(|v| v.as_f64()).ok_or_else(|| format!("joints.heat_set_insert.{size}.{k} is missing"));
            let (knurl, length) = (get("knurl_mm")?, get("length_mm")?);
            let eta = registry.joint_number("heat_set_insert.grip_share")?;
            // Pull-out: shear of the plastic cylinder around the knurls, τ·π·D·L·η.
            let pullout = material.interlayer_shear.value * strength_factor * std::f64::consts::PI * knurl * length * eta * 1e-6;
            let screws = registry.joints.get("screw").ok_or("joints.screw is missing")?;
            let proof = screws.get("proof_load_n").and_then(|v| v.get(size)).and_then(|v| v.as_f64()).ok_or_else(|| format!("joints.screw.proof_load_n.{size} is missing"))?;
            let head = screws.get("head_mm").and_then(|v| v.get(size)).and_then(|v| v.as_f64()).unwrap_or(knurl);
            let hole = screws.get("clearance_mm").and_then(|v| v.get(size)).and_then(|v| v.as_f64()).unwrap_or(knurl * 0.6);
            // The head pressing on printed plastic.
            let head_bearing = bearing * std::f64::consts::PI / 4. * (head * head - hole * hole) * 1e-6;
            let (tension, limit) = [(pullout, "insert pull-out (τ·π·D·L·η)"), (proof, "screw proof load"), (head_bearing, "screw head bearing on plastic")].into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).unwrap();
            let d = size.trim_start_matches('M').parse::<f64>().unwrap_or(3.);
            Capacity { tension_n: tension, shear_n: bearing * d * clamp_mm.min(length) * 1e-6, tension_limit: limit.into(), shear_limit: "screw bearing on the clamped plastic".into() }
        }
        Joint::Dovetail { rail_length_mm, neck_mm, depth_mm, .. } => {
            // The tail's neck in tension; across the layers is the weak case.
            let s = material.tensile_in_layer.value.min(material.tensile_across_layers.value) * strength_factor;
            let neck = s * neck_mm * rail_length_mm * 1e-6;
            let angle = registry.joint_number("dovetail.angle_deg")?.to_radians();
            // Pulling out spreads the socket walls: each flank carries F/(2 sin α); walls in bending.
            let wall = material.in_layer_shear.value * strength_factor * depth_mm * rail_length_mm * 1e-6 * 2. * angle.sin();
            let (mut tension, mut limit) = if neck < wall { (neck, "tail neck in tension".to_string()) } else { (wall, "socket walls spreading (flank wedge)".to_string()) };
            // A measured tab (coupon test) replaces the model, scaled by neck area × thickness.
            if let Some(q) = material.measured_extra.get("dovetail_pull_n") {
                let geo = |k: &str| q.evidence.as_ref().and_then(|e| e.get("geometry")).and_then(|g| g.get(k)).and_then(|v| v.as_f64());
                if let (Some(n0), Some(t0)) = (geo("neck_mm"), geo("thickness_mm")) {
                    tension = q.value * strength_factor * (neck_mm * rail_length_mm) / (n0 * t0);
                    limit = format!("measured tab capacity {:.0} N (coupons), scaled by neck area", q.value);
                }
            }
            Capacity { tension_n: tension, shear_n: bearing * depth_mm * rail_length_mm * 1e-6, tension_limit: limit, shear_limit: "flank bearing across the rail (sliding along it is free)".into() }
        }
    })
}

/// Load through a seam (on the plus piece from the minus piece), part frame.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct SeamLoad {
    /// Pulling the pieces apart (N, > 0 is tension).
    pub tension_n: f64,
    /// Sliding force in the seam plane (vector, N).
    pub shear: V3,
    /// Bending moment in the seam plane (vector, N·m) about the section centroid.
    pub bending: V3,
    /// Twist about the seam normal (N·m).
    pub torsion_nm: f64,
    pub centroid: V3,
    pub normal: V3,
}

#[derive(Clone, Debug, Serialize)]
pub struct JointCheck {
    pub kind: String,
    pub at: V3,
    pub tension_n: f64,
    pub shear_n: f64,
    pub capacity: Capacity,
    /// Load ÷ capacity (1 = at capacity).
    pub utilisation: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SeamCheck {
    pub name: String,
    pub load: SeamLoad,
    pub joints: Vec<JointCheck>,
    /// Smallest capacity ÷ load over the joints (∞ with no load).
    pub safety_factor: f64,
    pub notes: Vec<String>,
}

/// Share a seam's load among its joints: tension (and bending, as a bolt
/// group) goes to joints that hold tension; shear and twist to all joints by
/// shear capacity.
pub fn check_seam(name: &str, load: &SeamLoad, joints: &[Joint], registry: &Registry, material: &Material, strength_factor: f64) -> Result<SeamCheck, String> {
    let caps: Vec<Capacity> = joints.iter().map(|j| capacity(j, registry, material, strength_factor)).collect::<Result<_, _>>()?;
    let n = unit(load.normal);
    let c = load.centroid;
    let mut notes = Vec::new();
    let tension_holders: Vec<usize> = (0..joints.len()).filter(|i| caps[*i].tension_n > 0.).collect();
    let mut unresisted = false;
    // Bolt group: tension per holder = N·w/Σw + M×r terms (w = capacity weight).
    let mut tension = vec![0.; joints.len()];
    if !tension_holders.is_empty() {
        let w: f64 = tension_holders.iter().map(|i| caps[*i].tension_n).sum();
        let r: Vec<V3> = tension_holders.iter().map(|i| {
            let d = sub(joints[*i].at(), c);
            scale(sub(d, scale(n, dot(d, n))), 1e-3)
        }).collect();
        // The moment on the plus piece is M = Σ Tᵢ (n × rᵢ): with Tᵢ = sᵢ·x for
        // sᵢ = n × rᵢ, x solves A x = M, A = Σ sᵢ sᵢᵀ (in-plane, pseudo-inverse).
        let e1 = unit(if n[0].abs() < 0.9 { cross(n, [1., 0., 0.]) } else { cross(n, [0., 1., 0.]) });
        let e2 = cross(n, e1);
        let s: Vec<[f64; 2]> = r.iter().map(|ri| {
            let si = cross(n, *ri);
            [dot(si, e1), dot(si, e2)]
        }).collect();
        let (mut a11, mut a12, mut a22) = (0., 0., 0.);
        for si in &s {
            a11 += si[0] * si[0];
            a12 += si[0] * si[1];
            a22 += si[1] * si[1];
        }
        let m = [dot(load.bending, e1), dot(load.bending, e2)];
        let det = a11 * a22 - a12 * a12;
        let x = if det.abs() > 1e-12 * (a11 + a22).powi(2).max(1e-30) {
            [(a22 * m[0] - a12 * m[1]) / det, (a11 * m[1] - a12 * m[0]) / det]
        } else if a11 + a22 > 1e-15 {
            // Joints in a line: only the moment component they can resist (eigen-direction of A).
            let t = a11 + a22;
            let dir = if a11 >= a22 { [a11, a12] } else { [a12, a22] };
            let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt().max(1e-30);
            let d = [dir[0] / len, dir[1] / len];
            let along = (m[0] * d[0] + m[1] * d[1]) / t;
            let across = (m[0] * -d[1] + m[1] * d[0]).abs();
            if across > 0.05 * (m[0] * m[0] + m[1] * m[1]).sqrt() && across > 1e-6 {
                notes.push(format!("the tension joints lie in a line: {across:.3} N·m of bending about that line has nothing to resist it (the seam would open)"));
                unresisted = true;
            }
            [along * d[0], along * d[1]]
        } else {
            [0., 0.]
        };
        for (k, i) in tension_holders.iter().enumerate() {
            let from_bending = s[k][0] * x[0] + s[k][1] * x[1];
            tension[*i] = load.tension_n * caps[*i].tension_n / w + from_bending;
        }
    }
    // Pulling apart or bending with nothing to hold the seam closed: it opens.
    let opens = tension_holders.is_empty() && (load.tension_n > 1e-6 || norm(load.bending) > 1e-9);
    if opens {
        notes.push(format!("no joint here holds the seam closed, but it carries {:.1} N of tension and {:.3} N·m of bending: add a screw or a dovetail", load.tension_n.max(0.), norm(load.bending)));
    }
    let shear_total: f64 = norm(load.shear);
    let ws: f64 = caps.iter().map(|c| c.shear_n).sum::<f64>().max(1e-12);
    let r_all: Vec<f64> = joints.iter().map(|j| {
        let d = sub(j.at(), c);
        norm(sub(d, scale(n, dot(d, n)))) * 1e-3
    }).collect();
    let polar: f64 = r_all.iter().map(|r| r * r).sum::<f64>().max(1e-12);
    let mut checks = Vec::new();
    let mut sf = f64::INFINITY;
    for (i, j) in joints.iter().enumerate() {
        let shear = shear_total * caps[i].shear_n / ws + load.torsion_nm.abs() * r_all[i] / polar;
        let t = tension[i].max(0.);
        let ut = if caps[i].tension_n > 0. { t / caps[i].tension_n } else { 0. };
        let us = if caps[i].shear_n > 0. { shear / caps[i].shear_n } else if shear > 0. { f64::INFINITY } else { 0. };
        let u = (ut * ut + us * us).sqrt();
        if u > 0. {
            sf = sf.min(1. / u);
        }
        checks.push(JointCheck { kind: j.kind().into(), at: j.at(), tension_n: t, shear_n: shear, capacity: caps[i].clone(), utilisation: u });
    }
    if joints.is_empty() {
        notes.push("the seam has no joints".into());
        sf = 0.;
    }
    if opens || unresisted {
        sf = 0.;
    }
    Ok(SeamCheck { name: name.into(), load: *load, joints: checks, safety_factor: sf, notes })
}
