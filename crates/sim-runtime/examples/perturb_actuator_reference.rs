//! Precompute independent command interventions using the shared seeded Gaussian
//! sampler. Runtime physics still executes the resulting authored trajectory.
use serde::Deserialize;
use sim_core::QuantityKind;
use sim_domain_control::{
    ppo::{GaussianExploration, GaussianSampler},
    trajectory::Trajectory,
};
use sim_runtime::predictive_control::ForecastActionReference;
use std::{fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Experiment {
    version: u32,
    reference: ForecastActionReference,
    /// Existing intersected software/CAD bounds in the declared actuator order.
    bounds_rad: Vec<[f64; 2]>,
    standard_deviation_fraction: f64,
    seed: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: perturb_actuator_reference experiment.json fresh-directory".into());
    }
    let bytes = fs::read(&args[0])?;
    let experiment: Experiment = serde_json::from_slice(&bytes)?;
    let width = experiment.reference.actuators.len();
    if experiment.version != 1
        || experiment.reference.expected_cad_sha256.is_empty()
        || experiment
            .reference
            .actuators
            .iter()
            .any(|a| a.kind != QuantityKind::Angle || a.name.is_empty())
        || experiment
            .reference
            .actuators
            .iter()
            .map(|a| &a.name)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != width
        || experiment.bounds_rad.len() != width
        || experiment.bounds_rad.iter().any(|[lo, hi]| {
            !lo.is_finite() || !hi.is_finite() || lo >= hi || !(hi - lo).is_finite()
        })
        || Trajectory::new(experiment.reference.trajectory.clone())?.dimension() != width
        || experiment.reference.trajectory.keyframes.iter().any(|k| {
            k.values
                .iter()
                .zip(&experiment.bounds_rad)
                .any(|(v, b)| *v < b[0] || *v > b[1])
        })
    {
        return Err("invalid typed reference or explicit actuator bounds".into());
    }
    let mut sampler = GaussianSampler::new(
        GaussianExploration {
            standard_deviation: vec![experiment.standard_deviation_fraction; width],
        },
        width,
        experiment.seed,
    )?;
    let mut reference = experiment.reference;
    let mut clipped = 0usize;
    let mut maximum_change_rad = vec![0f64; width];
    for knot in &mut reference.trajectory.keyframes {
        let (noise, _) = sampler.sample(&vec![0.; width])?;
        for j in 0..width {
            let [lo, hi] = experiment.bounds_rad[j];
            let original = knot.values[j];
            let requested = original + noise[j] * (hi - lo);
            if !requested.is_finite() {
                return Err("nonfinite perturbed command".into());
            }
            let applied = requested.clamp(lo, hi);
            clipped += usize::from(applied != requested);
            maximum_change_rad[j] = maximum_change_rad[j].max((applied - original).abs());
            knot.values[j] = applied;
        }
    }
    Trajectory::new(reference.trajectory.clone())?;
    let root = Path::new(&args[1]);
    fs::create_dir(root)?;
    fs::write(root.join("experiment.json"), bytes)?;
    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join("reference.json"))?;
    serde_json::to_writer(file, &reference)?;
    let report = serde_json::json!({"version":1,"seed":experiment.seed,
        "standard_deviation_fraction":experiment.standard_deviation_fraction,
        "knots":reference.trajectory.keyframes.len(),"actuators":width,
        "clipped_commands":clipped,"maximum_change_rad":maximum_change_rad,
        "scope":"All perturbations are drawn before simulation, independently of future physical states. Noise fractions scale existing actuator command spans; saturation uses those same explicit bounds. The resulting scheduled targets still execute through ordinary Rust servo/contact physics. No policy likelihood or learning-speed claim is made."});
    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join("report.json"))?;
    serde_json::to_writer_pretty(file, &report)?;
    Ok(())
}
