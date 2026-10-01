//! Reading a `.simresult.json` (the results document written by
//! `sim_runtime::physical::PhysicalRobot::results`, contract:
//! `cad/PHYSICAL_MODEL.md`) for display: the per-link stress hotspot
//! colouring and the provenance check. Pure functions over the parsed JSON;
//! the viewers (sim-spatial robot mode) share them.
use serde_json::Value;

/// The colour scale the stress colouring uses, for labels.
pub const SCALE: &str = "blue = 0.1 % of yield, red = yield (log scale over 3 decades)";

/// Turbo colour map for stress (0 → blue, 1 → red).
pub fn colormap(t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    let r = (1.7 * t - 0.3).clamp(0.0, 1.0) * 0.95 + 0.05;
    let g = (1.0 - (2.0 * t - 1.0).abs()).clamp(0.0, 1.0) * 0.85 + 0.1;
    let b = (1.0 - 1.7 * t).clamp(0.0, 1.0) * 0.95 + 0.05;
    [r, g, b, 1.0]
}

/// One link's stress hotspot cells (link frame, the frame of
/// `Collision::display_triangles`) and their peak stress.
#[derive(Clone, Debug, PartialEq)]
pub struct Hotspot {
    pub cells: Vec<[f64; 3]>,
    pub stress_pa: Vec<f64>,
}
impl Hotspot {
    /// `results["links"][link]["hotspot"]`; `None` without cells (the link keeps its normal look).
    pub fn from_results(results: &Value, link: &str) -> Option<Self> {
        Self::from_block(&results["links"][link]["hotspot"])
    }
    /// One hotspot block `{"cells": [[x, y, z], …], "stress_pa": […]}` (the
    /// link block's `hotspot`, also as RoboCAD hangs it on a node's
    /// `results`); `None` without cells. A cell and its stress are kept or
    /// dropped together (a non-number in either, e.g. a NaN written as
    /// null, drops the pair), so every later cell keeps its own stress.
    pub fn from_block(hotspot: &Value) -> Option<Self> {
        let cells = hotspot["cells"].as_array()?;
        let stress = hotspot["stress_pa"].as_array()?;
        let (cells, stress_pa): (Vec<[f64; 3]>, Vec<f64>) = cells.iter().zip(stress).filter_map(|(c, v)| Some(([c[0].as_f64()?, c[1].as_f64()?, c[2].as_f64()?], v.as_f64()?))).unzip();
        if cells.is_empty() { None } else { Some(Self { cells, stress_pa }) }
    }
    /// The largest recorded cell stress, at least 1 Pa: the yield a link
    /// without a known material yield is scaled by (RoboCAD's fallback,
    /// `max(stress, 1)`).
    pub fn peak_or_one(&self) -> f64 {
        self.stress_pa.iter().copied().filter(|v| v.is_finite()).fold(1.0, f64::max)
    }
}

/// The colour at `p` (link frame): the nearest hotspot cell's peak stress,
/// normalised by `yield_strength`, through [`colormap`] ([`SCALE`]).
pub fn stress_colour(hotspot: &Hotspot, yield_strength: f64, p: [f64; 3]) -> [f32; 4] {
    let mut best = (f64::INFINITY, 0.0);
    for (c, v) in hotspot.cells.iter().zip(&hotspot.stress_pa) {
        let d = (c[0] - p[0]).powi(2) + (c[1] - p[1]).powi(2) + (c[2] - p[2]).powi(2);
        if d < best.0 {
            best = (d, *v);
        }
    }
    // Scale so that yield is red; the scale is logarithmic over 3 decades.
    let ratio = (best.1 / yield_strength.max(1.0)).max(1e-6);
    colormap(((ratio.log10() + 3.0) / 3.0) as f32)
}

/// The one stress colouring rule for a whole link: each position (link
/// frame, metres; the frame of the hotspot cells) through [`stress_colour`].
/// Robot mode (`robot::stress`, positions of `Collision::display_triangles`)
/// and CAD mode's overlay (RoboCAD's mesh vertices, mm, moved into the link
/// frame by the caller) both colour through this.
pub fn link_colours(hotspot: &Hotspot, yield_strength: f64, positions_link_frame_m: impl IntoIterator<Item = [f64; 3]>) -> Vec<[f32; 4]> {
    positions_link_frame_m.into_iter().map(|p| stress_colour(hotspot, yield_strength, p)).collect()
}

/// The model hash the results were computed from: `provenance.physical_hash`
/// (provenance is null when the model's `source.physical_hash` was absent).
pub fn recorded_physical_hash(results: &Value) -> Option<&str> {
    results["provenance"]["physical_hash"].as_str()
}

/// Whether results match a model: `current` when the recorded
/// `provenance.physical_hash` equals the model's `source.physical_hash`,
/// `stale` when both exist and differ, `no recorded hash` when either is absent.
pub fn staleness(recorded: Option<&str>, model: Option<&str>) -> &'static str {
    match (recorded, model) {
        (Some(r), Some(m)) if r == m => "current",
        (Some(_), Some(_)) => "stale",
        _ => "no recorded hash",
    }
}

/// Per link, `peak_stress_pa` as recorded (null when absent or not a number).
pub fn peaks(results: &Value) -> Vec<(String, Option<f64>)> {
    results["links"].as_object().map(|l| l.iter().map(|(k, v)| (k.clone(), v["peak_stress_pa"].as_f64())).collect()).unwrap_or_default()
}
