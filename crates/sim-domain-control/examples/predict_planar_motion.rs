//! Fit past planar motion and report future endpoints. Observed future poses
//! are used only for scoring, never fitting. Does not certify physical survival.
use serde::Deserialize;
use serde_json::json;
use sim_domain_control::planar_prediction::{
    PlanarPrediction, PlanarTrendPrediction, TimedPlanarPose,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    time_s: f64,
    observed_position_m: Option<[f64; 2]>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    samples: Vec<TimedPlanarPose>,
    origin: TimedPlanarPose,
    queries: Vec<Query>,
    #[serde(default)]
    heading_trend: bool,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: predict_planar_motion request.json new-report.json".into());
    }
    let request: Request = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let trend = if request.heading_trend {
        Some(PlanarTrendPrediction::fit(&request.samples)?)
    } else {
        None
    };
    let model = match &trend {
        Some(t) => t.motion.clone(),
        None => PlanarPrediction::fit(&request.samples)?,
    };
    let anchor = *request.samples.last().unwrap();
    let first = request.samples[0];
    if !request.origin.time_s.is_finite()
        || request.origin.time_s > first.time_s
        || request.origin.pose.iter().any(|x| !x.is_finite())
    {
        return Err("finite origin preceding fit required".into());
    }
    let mut forecasts = vec![];
    for query in request.queries {
        let predicted = match &trend {
            Some(t) => t.predict(query.time_s)?,
            None => model.predict(anchor, query.time_s)?,
        };
        let total_time = query.time_s - request.origin.time_s;
        if total_time <= 0.
            || !total_time.is_finite()
            || query
                .observed_position_m
                .is_some_and(|p| p.iter().any(|x| !x.is_finite()))
        {
            return Err("positive elapsed query time and finite observation required".into());
        }
        let displacement = [
            predicted[0] - request.origin.pose[0],
            predicted[1] - request.origin.pose[1],
        ];
        let observed_speed = query.observed_position_m.map(|p| {
            (p[0] - request.origin.pose[0]).hypot(p[1] - request.origin.pose[1]) / total_time
        });
        let endpoint_error = query
            .observed_position_m
            .map(|p| (p[0] - predicted[0]).hypot(p[1] - predicted[1]));
        // Matched baseline: same fit interval, world-frame endpoint velocity.
        let linear: [f64; 2] = std::array::from_fn(|i| {
            anchor.pose[i]
                + (anchor.pose[i] - first.pose[i]) / model.duration_s
                    * (query.time_s - anchor.time_s)
        });
        forecasts.push(json!({"time_s":query.time_s,"predicted_pose":predicted,
            "predicted_net_speed_m_s":displacement[0].hypot(displacement[1])/total_time,
            "observed_net_speed_m_s":observed_speed,"endpoint_error_m":endpoint_error,
            "linear_baseline_position_m":linear,
            "linear_baseline_endpoint_error_m":query.observed_position_m.map(|p|
                (p[0]-linear[0]).hypot(p[1]-linear[1]))}));
    }
    let report = json!({"version":1,"fit_start_s":first.time_s,"fit_end_s":anchor.time_s,
        "model":model,"heading_trend":trend,"forecasts":forecasts,
        "scope":"Empirical constant body-frame twist extrapolation using past planar poses only. Future observations are scoring labels. No contact/actuator dynamics, fall prediction, speed bound or demonstrated search improvement."});
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?
        .write_all(serde_json::to_string_pretty(&report)?.as_bytes())?;
    Ok(())
}
