//! Fit dimensionless uncertainty scales without changing predictive means.
use serde::{Deserialize, Serialize};
use sim_solve::{
    composite::Response,
    uncertainty::{PredictionObservation, fit_standard_deviation_scales},
};
use std::{collections::HashSet, io::Write};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    responses: Vec<Response>,
    observations: Vec<PredictionObservation>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: calibrate_response_uncertainty request.json new-result.json".into());
    }
    let request: Request = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let mut names = HashSet::new();
    if request.responses.is_empty()
        || request
            .responses
            .iter()
            .any(|r| r.name.trim().is_empty() || r.unit.trim().is_empty() || !names.insert(&r.name))
        || request
            .observations
            .iter()
            .any(|o| o.mean.len() != request.responses.len())
    {
        return Err(
            "named responses with units and matching prediction dimensions required".into(),
        );
    }
    let result = fit_standard_deviation_scales(&request.observations)?;
    let report = serde_json::json!({"version":1,"request":request,"result":result,
        "scope":"Per-output Gaussian NLL standard-deviation scaling from supplied excluded predictions. Means stay fixed; no coverage guarantee, response covariance or robot-specific calibration is inferred from a fit. Caller must preserve evidence and assess new outcomes independently."});
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?
        .write_all(&serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
