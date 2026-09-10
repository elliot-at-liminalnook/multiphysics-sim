//! Composite EI using shared planar motion and independent response GPs.
//! Response blocks are end-of-prefix relative x/y/heading and body vx/vy/yaw rate.
use serde::{Deserialize, Serialize};
use sim_domain_control::planar_prediction::PlanarResponseWindow;
use sim_solve::{
    bayesian::{Problem, FixedParameter, conditioned_design},
    composite::{self, Config, Observation, Response},
};
use std::io::Write;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateRegion {
    /// Normalized bounds relative to the declared full parameter domain.
    bounds: Vec<[f64; 2]>,
    count: usize,
    seed: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    fixed: Vec<FixedParameter>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    problem: Problem,
    responses: Vec<Response>,
    observations: Vec<Observation>,
    #[serde(default)]
    regions: Vec<CandidateRegion>,
    /// Explicit candidates support held-out prediction audits with no new simulation.
    #[serde(default)]
    candidates: Vec<Vec<f64>>,
    /// Empty retains the original disjoint six-response block layout.
    #[serde(default)]
    windows: Vec<PlanarResponseWindow>,
    config: Config,
    prefix_s: f64,
    horizon_s: f64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: suggest_composite_planar request.json new-result.json".into());
    }
    let r: Request = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    if !r.prefix_s.is_finite()
        || !r.horizon_s.is_finite()
        || r.prefix_s <= 0.
        || r.horizon_s <= r.prefix_s
        || r.responses.is_empty()
        || (r.windows.is_empty() && r.responses.len() % 6 != 0)
        || (r.regions.is_empty() && r.candidates.is_empty())
        || r.regions.len() > 16
        || r.windows.len() > 64
    {
        return Err(
            "positive prefix preceding horizon, six-response blocks and candidate regions required"
                .into(),
        );
    }
    let windows = if r.windows.is_empty() {
        (0..r.responses.len() / 6)
            .map(|i| PlanarResponseWindow {
                pose: [6 * i, 6 * i + 1, 6 * i + 2],
                twist: [6 * i + 3, 6 * i + 4, 6 * i + 5],
            })
            .collect::<Vec<_>>()
    } else {
        r.windows.clone()
    };
    let units = ["m", "m", "rad", "m/s", "m/s", "rad/s"];
    if windows.iter().any(|w| {
        w.pose
            .iter()
            .chain(&w.twist)
            .zip(units)
            .any(|(index, unit)| r.responses.get(*index).is_none_or(|s| s.unit != unit))
    }) || r.problem.objective_unit != "m/s"
    {
        return Err("planar response blocks and objective require physical SI units".into());
    }
    let compose = |v: &[f64]| -> Result<f64, String> {
        let mut speed: f64 = 0.;
        for window in &windows {
            let pose = window.predict(v, r.horizon_s - r.prefix_s)?;
            let s = pose[0].hypot(pose[1]) / r.horizon_s;
            if !s.is_finite() {
                return Err("nonfinite planar composition".into());
            }
            speed = speed.max(s);
        }
        Ok(-speed)
    };
    let mut candidates = r.candidates.clone();
    for region in &r.regions {
        if region.bounds.len() != r.problem.parameters.len()
            || region
                .bounds
                .iter()
                .any(|[a, b]| !a.is_finite() || !b.is_finite() || *a < 0. || *b > 1. || a >= b)
        {
            return Err("dimension-matched ordered normalized region bounds required".into());
        }
        let mut local = r.problem.clone();
        for (p, [a, b]) in local.parameters.iter_mut().zip(&region.bounds) {
            let [lo, hi] = p.bounds;
            p.bounds = [(1. - a) * lo + a * hi, (1. - b) * lo + b * hi];
        }
        candidates.extend(conditioned_design(&local, region.count, region.seed, &region.fixed)?);
    }
    let observed_objectives = r
        .observations
        .iter()
        .map(|o| match &o.outcome {
            composite::Outcome::Complete { responses } => compose(responses).map(Some),
            composite::Outcome::Failed { .. } => Ok(None),
        })
        .collect::<Result<Vec<_>, String>>()?;
    let result = composite::rank_candidates(
        &r.problem,
        &r.responses,
        &r.observations,
        &candidates,
        &r.config,
        compose,
    )?;
    let report = serde_json::json!({"version":1,"request":r,"result":result,"observed_objectives":observed_objectives,
        "scope":"Composite acquisition over empirical planar response GPs, using the existing shared SE2 trajectory integration and maximum of fitted-window forecasts. Independent outputs omit response covariance. Forecasts do not model contact, actuator dynamics or future falls. Finite candidate ranking is an adaptation of composite EI, not a reproduction of its gradient optimizer or a proven sample-efficiency gain."});
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?
        .write_all(&serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
