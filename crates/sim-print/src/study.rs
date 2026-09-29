//! A print study (`sim.print-study/1`): parts as meshes, where they are held,
//! the loads on them (numbers, or values read from a simulation run), cut
//! planes whose transmitted loads we want, and seams with their joints.

use crate::mesh::{V3, dot, norm, sub};
use crate::voxel::Settings;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA: &str = "sim.print-study/1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Study {
    pub schema: String,
    /// Registry file (relative to the study file); default: the repository's.
    #[serde(default)]
    pub registry: Option<String>,
    pub printer: String,
    pub material: String,
    /// The simulation the loads are read from.
    #[serde(default)]
    pub simulation: Option<Simulation>,
    /// Voxel edge (mm); default: sized for `voxels`.
    #[serde(default)]
    pub voxel_mm: Option<f64>,
    /// Target voxel count when `voxel_mm` is not given.
    #[serde(default = "default_voxels")]
    pub voxels: usize,
    /// Required safety factor on every piece (strength ÷ stress).
    #[serde(default = "default_safety")]
    pub safety_target: f64,
    pub parts: Vec<PartStudy>,
    #[serde(default)]
    pub plan: Option<PlanSpace>,
    /// Anything CAD wants to carry along (source document hash, …).
    #[serde(default)]
    pub provenance: BTreeMap<String, serde_json::Value>,
}
fn default_voxels() -> usize {
    40_000
}
fn default_safety() -> f64 {
    2.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Simulation {
    /// A `sim.system/1` file (relative to the study file).
    pub system: String,
    pub seconds: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartStudy {
    pub name: String,
    /// Binary or ASCII STL in millimetres, in the part's (CAD) frame.
    pub mesh: String,
    /// Build direction in the part frame (the way the layers stack).
    #[serde(default = "up")]
    pub build_direction: V3,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub fixtures: Vec<Fixture>,
    #[serde(default)]
    pub loads: Vec<Load>,
    /// Acceleration of the part (m/s², part frame) applied to its own mass, e.g. gravity [0, 0, −9.81].
    #[serde(default)]
    pub acceleration: Option<V3>,
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default)]
    pub seams: Vec<Seam>,
    /// Build directions the planner may try (default: the six axis directions).
    #[serde(default)]
    pub directions: Option<Vec<V3>>,
}
fn up() -> V3 {
    [0., 0., 1.]
}

/// Where on the part's surface something acts (part frame, mm).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Region {
    Sphere { center: V3, radius: f64 },
    Box { min: V3, max: V3 },
    /// A solid cylinder from `base` along `axis` for `length`.
    Cylinder { base: V3, axis: V3, radius: f64, length: f64 },
    /// Sample points of CAD faces: surface within `radius` of any point.
    Points { points: Vec<V3>, radius: f64 },
    /// The half-space below a height (e.g. everything on the bed or a floor).
    Below { axis: V3, height: f64 },
    /// A thin slab around a plane (a cut face): |(p − point)·normal| ≤ thickness/2.
    Slab { point: V3, normal: V3, thickness: f64 },
}

impl Region {
    pub fn contains(&self, p: V3) -> bool {
        match self {
            Region::Sphere { center, radius } => norm(sub(p, *center)) <= *radius,
            Region::Box { min, max } => (0..3).all(|k| p[k] >= min[k] && p[k] <= max[k]),
            Region::Cylinder { base, axis, radius, length } => {
                let a = crate::mesh::unit(*axis);
                let d = sub(p, *base);
                let t = dot(d, a);
                let radial = norm(sub(d, crate::mesh::scale(a, t)));
                t >= 0. && t <= *length && radial <= *radius
            }
            Region::Points { points, radius } => points.iter().any(|q| norm(sub(p, *q)) <= *radius),
            Region::Below { axis, height } => dot(p, crate::mesh::unit(*axis)) <= *height,
            Region::Slab { point, normal, thickness } => dot(sub(p, *point), crate::mesh::unit(*normal)).abs() <= thickness / 2.,
        }
    }
    /// The region grown by `d` mm on every side (voxel faces lie up to about
    /// half a voxel off the true surface, so matching allows for it).
    pub fn inflated(&self, d: f64) -> Region {
        match self {
            Region::Sphere { center, radius } => Region::Sphere { center: *center, radius: radius + d },
            Region::Box { min, max } => Region::Box { min: min.map(|x| x - d), max: max.map(|x| x + d) },
            Region::Cylinder { base, axis, radius, length } => {
                let a = crate::mesh::unit(*axis);
                Region::Cylinder { base: crate::mesh::sub(*base, crate::mesh::scale(a, d)), axis: *axis, radius: radius + d, length: length + 2. * d }
            }
            Region::Points { points, radius } => Region::Points { points: points.clone(), radius: radius + d },
            Region::Below { axis, height } => Region::Below { axis: *axis, height: height + d },
            Region::Slab { point, normal, thickness } => Region::Slab { point: *point, normal: *normal, thickness: thickness + 2. * d },
        }
    }
    /// The same region after rotating the part by `r`.
    pub fn rotated(&self, r: &crate::mesh::Rot) -> Region {
        match self {
            Region::Sphere { center, radius } => Region::Sphere { center: r.apply(*center), radius: *radius },
            Region::Box { min, max } => {
                // An axis-aligned box stays one only for axis rotations; keep its rotated corners' hull.
                let corners: Vec<V3> = (0..8).map(|i| r.apply([if i & 1 == 0 { min[0] } else { max[0] }, if i & 2 == 0 { min[1] } else { max[1] }, if i & 4 == 0 { min[2] } else { max[2] }])).collect();
                let lo = [0, 1, 2].map(|k| corners.iter().map(|c| c[k]).fold(f64::INFINITY, f64::min));
                let hi = [0, 1, 2].map(|k| corners.iter().map(|c| c[k]).fold(f64::NEG_INFINITY, f64::max));
                Region::Box { min: lo, max: hi }
            }
            Region::Cylinder { base, axis, radius, length } => Region::Cylinder { base: r.apply(*base), axis: r.apply(*axis), radius: *radius, length: *length },
            Region::Points { points, radius } => Region::Points { points: points.iter().map(|p| r.apply(*p)).collect(), radius: *radius },
            Region::Below { axis, height } => Region::Below { axis: r.apply(*axis), height: *height },
            Region::Slab { point, normal, thickness } => Region::Slab { point: r.apply(*point), normal: r.apply(*normal), thickness: *thickness },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub name: String,
    pub region: Region,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Load {
    pub name: String,
    pub region: Region,
    /// Direction of the force in the part frame (normalised).
    pub direction: V3,
    pub magnitude: Magnitude,
    /// A moment (N·m, part frame) spread over the region as a linear traction
    /// about `about` (mm; default the region's centroid): how a neighbouring
    /// piece's bending reaches this one through a seam.
    #[serde(default)]
    pub moment: Option<V3>,
    #[serde(default)]
    pub about: Option<V3>,
}

/// A force in newtons: a number, or read from the simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Magnitude {
    Newtons(f64),
    Observed(Observed),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observed {
    /// Observable key (`belt.hub_load`, as `sim-system run` prints it).
    pub observe: String,
    /// `max_abs` (default), `max`, `min`, `final`, `rms`.
    #[serde(default = "max_abs")]
    pub reduce: String,
    #[serde(default)]
    pub window: Option<[f64; 2]>,
    /// Multiplies the reduced value (e.g. a lever ratio or a dynamic factor).
    #[serde(default = "one")]
    pub scale: f64,
    /// Added after scaling (e.g. a static preload not in the model), N.
    #[serde(default)]
    pub add: f64,
    /// Why this mapping from the simulation to the part is right.
    #[serde(default)]
    pub why: String,
}
fn max_abs() -> String {
    "max_abs".into()
}
fn one() -> f64 {
    1.
}

impl Observed {
    /// Reduce a sampled series.
    pub fn reduce_series(&self, times: &[f64], values: &[f64]) -> Result<f64, String> {
        let w = self.window.unwrap_or([f64::NEG_INFINITY, f64::INFINITY]);
        let v: Vec<f64> = times.iter().zip(values).filter(|(t, v)| **t >= w[0] && **t <= w[1] && v.is_finite()).map(|(_, v)| *v).collect();
        if v.is_empty() {
            return Err(format!("`{}` has no finite samples in the window", self.observe));
        }
        let r = match self.reduce.as_str() {
            "max_abs" => v.iter().fold(0f64, |a, b| a.max(b.abs())),
            "max" => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            "min" => v.iter().copied().fold(f64::INFINITY, f64::min),
            "final" => *v.last().unwrap(),
            "rms" => (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt(),
            other => return Err(format!("unknown reduce `{other}` (max_abs, max, min, final, rms)")),
        };
        Ok(r * self.scale + self.add)
    }
}

/// A plane through the part whose transmitted load is reported.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub name: String,
    pub point: V3,
    /// Points from the first piece ("minus") into the second ("plus").
    pub normal: V3,
}

/// A cut plane with the joints that hold its two pieces together.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seam {
    pub name: String,
    pub point: V3,
    pub normal: V3,
    pub joints: Vec<crate::joints::Joint>,
}

/// What the planner may vary.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanSpace {
    #[serde(default = "walls")]
    pub walls: Vec<u32>,
    #[serde(default = "infill")]
    pub infill: Vec<f64>,
    #[serde(default)]
    pub layer_heights: Option<Vec<f64>>,
    #[serde(default = "pattern")]
    pub pattern: String,
    /// Voxel target for the search (the pick is re-checked at the study's resolution).
    #[serde(default = "search_voxels")]
    pub search_voxels: usize,
}
fn walls() -> Vec<u32> {
    vec![2, 3, 4, 6]
}
fn infill() -> Vec<f64> {
    vec![0.15, 0.25, 0.4, 0.6, 1.0]
}
fn pattern() -> String {
    "gyroid".into()
}
fn search_voxels() -> usize {
    12_000
}
impl Default for PlanSpace {
    fn default() -> Self {
        PlanSpace { walls: walls(), infill: infill(), layer_heights: None, pattern: pattern(), search_voxels: search_voxels() }
    }
}

impl Study {
    pub fn parse(text: &str) -> Result<Study, String> {
        let s: Study = serde_json::from_str(text).map_err(|e| format!("study: {e}"))?;
        if s.schema != SCHEMA {
            return Err(format!("study: schema must be `{SCHEMA}`"));
        }
        for (i, p) in s.parts.iter().enumerate() {
            let at = format!("parts[{i}] ({})", p.name);
            if norm(p.build_direction) < 1e-9 {
                return Err(format!("{at}.build_direction is zero"));
            }
            for (k, l) in p.loads.iter().enumerate() {
                if norm(l.direction) < 1e-9 {
                    return Err(format!("{at}.loads[{k}] ({}).direction is zero", l.name));
                }
                if let Magnitude::Observed(_) = &l.magnitude {
                    if s.simulation.is_none() {
                        return Err(format!("{at}.loads[{k}] ({}) reads the simulation, but the study has no `simulation`", l.name));
                    }
                }
            }
            if p.fixtures.is_empty() {
                return Err(format!("{at} has no fixtures: say where the part is held"));
            }
            let s2 = &p.settings;
            if !(0.0..=1.0).contains(&s2.infill) || s2.walls == 0 || !(s2.layer_height > 0.) {
                return Err(format!("{at}.settings: walls ≥ 1, infill in [0, 1], layer_height > 0"));
            }
        }
        Ok(s)
    }
    /// Every observation the loads need.
    pub fn observations(&self) -> Vec<Observed> {
        let mut out: Vec<Observed> = Vec::new();
        for p in &self.parts {
            for l in &p.loads {
                if let Magnitude::Observed(o) = &l.magnitude {
                    if !out.contains(o) {
                        out.push(o.clone());
                    }
                }
            }
        }
        out
    }
}
