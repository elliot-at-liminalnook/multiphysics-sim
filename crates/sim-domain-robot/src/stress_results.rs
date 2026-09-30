//! Reading a `.simresult.json` (the results document written by
//! `sim_runtime::physical::PhysicalRobot::results`, contract:
//! `cad/PHYSICAL_MODEL.md`) for display: the per-link stress hotspot
//! colouring and the provenance check. Pure functions over the parsed JSON;
//! the viewers (sim-app's cad scene, sim-spatial robot mode) share them.
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
        let l = &results["links"][link];
        let cells: Vec<[f64; 3]> = l["hotspot"]["cells"].as_array()?.iter().filter_map(|c| Some([c[0].as_f64()?, c[1].as_f64()?, c[2].as_f64()?])).collect();
        let stress_pa: Vec<f64> = l["hotspot"]["stress_pa"].as_array()?.iter().filter_map(|v| v.as_f64()).collect();
        if cells.is_empty() { None } else { Some(Self { cells, stress_pa }) }
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
