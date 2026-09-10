//! Shared derivative diagnostics on every future CAD trajectory coordinate.
use serde::Deserialize;
use serde_json::json;
use sim_runtime::{
    contact_implicit::{ContactImplicitConfig, ContactImplicitPlanner},
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use sim_solve::{derivative_audit::central_difference_audit, least_squares::VariableBound};
#[derive(Deserialize)]
struct Recipe {
    config: ContactImplicitConfig,
    bounds: Vec<Vec<VariableBound>>,
}
#[derive(Deserialize)]
struct Path {
    positions: Vec<Vec<f64>>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 4 {
        return Err("usage: audit_contact_derivatives scene markers recipe positions".into());
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&a[0])?)?;
    let markers: CaptureConfig = serde_json::from_slice(&std::fs::read(&a[1])?)?;
    let recipe: Recipe = serde_json::from_slice(&std::fs::read(&a[2])?)?;
    let path: Path = serde_json::from_slice(&std::fs::read(&a[3])?)?;
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let periodic = recipe.config.periodic_horizontal_translation;
    let planner = ContactImplicitPlanner::new(&session.robot.art, &seed, &markers, recipe.config)?;
    let n = path.positions[0].len();
    let parameters = planner.parameterization(&path.positions, &recipe.bounds)?;
    let x = &parameters.values;
    let bounds = &parameters.bounds;
    let mut cached = planner.evaluator();
    let mut audits = vec![];
    for j in 0..x.len() {
        let width = bounds[j].upper - bounds[j].lower;
        if width <= 0. {
            continue;
        }
        let distance = ((x[j] - bounds[j].lower).min(bounds[j].upper - x[j]) / width).min(1e-4);
        if distance < 1e-9 {
            audits.push(json!({"parameter_index":j,"knot":usize::from(!periodic)+j/n,"coordinate":j%n,"skipped":"too close to bound for symmetric audit"}));
            continue;
        }
        let mut direction = vec![0.; x.len()];
        direction[j] = width;
        let steps = (0..6)
            .map(|i| distance * 10_f64.powi(-i))
            .collect::<Vec<_>>();
        let samples = central_difference_audit(&x, &direction, &steps, |x| {
            Ok(cached.evaluate(&parameters.decode(x)?)?.residuals)
        })?;
        audits.push(json!({"parameter_index":j,"kind":if periodic&&j>=(path.positions.len()-1)*n {"cycle_displacement"}else{"pose"},"knot":usize::from(!periodic)+j/n,"coordinate":j%n,"physical_coordinate_scale":width,"samples":samples}));
    }
    let mut descent_audits = vec![];
    for derivative_index in [3, 5] {
        let mut gradient = vec![0.; x.len()];
        for entry in &audits {
            if let Some(slope) = entry["samples"][derivative_index]["residual_cost_slope"].as_f64()
            {
                let j = entry["parameter_index"].as_u64().unwrap() as usize;
                gradient[j] = slope;
            }
        }
        let maximum = gradient.iter().map(|v| v.abs()).fold(0., f64::max);
        if maximum == 0. {
            continue;
        }
        let direction = gradient
            .iter()
            .enumerate()
            .map(|(j, g)| -g / maximum * (bounds[j].upper - bounds[j].lower))
            .collect::<Vec<_>>();
        let steps = (0..8).map(|i| 1e-5 * 10_f64.powi(-i)).collect::<Vec<_>>();
        let samples = central_difference_audit(&x, &direction, &steps, |x| {
            Ok(cached.evaluate(&parameters.decode(x)?)?.residuals)
        })?;
        descent_audits.push(json!({"gradient_probe_index":derivative_index,
            "maximum_gradient":maximum,"predicted_directional_cost_slope":
            -gradient.iter().map(|g|g*g).sum::<f64>()/maximum,"samples":samples}));
    }
    println!(
        "{}",
        json!({"audits":audits,"descent_audits":descent_audits,"scope":"Central finite differences at six decreasing steps for every free coordinate. All physics through the shared planner; no derivative accuracy or gait feasibility certificate."})
    );
    Ok(())
}
