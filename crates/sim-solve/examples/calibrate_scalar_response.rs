//! Fit an empirical input/output response and propose a target-setting input.
use serde::Deserialize;
use sim_solve::affine_response::AffineResponse;
use std::io::Write;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    samples: Vec<[f64; 2]>,
    input_unit: String,
    output_unit: String,
    target: f64,
    available_input_range: [f64; 2],
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: calibrate_scalar_response request.json new-report.json".into());
    }
    let request: Request = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let [lo, hi] = request.available_input_range;
    if request.input_unit.trim().is_empty()
        || request.output_unit.trim().is_empty()
        || !lo.is_finite()
        || !hi.is_finite()
        || lo >= hi
    {
        return Err("explicit units and ordered finite input domain required".into());
    }
    let model = AffineResponse::fit(&request.samples)?;
    let proposed_input = model.input_for(request.target)?;
    let report = serde_json::json!({"version":1,"input_unit":request.input_unit,"output_unit":request.output_unit,
        "target":request.target,"model":model,"proposed_input":proposed_input,
        "within_available_domain":lo<=proposed_input&&proposed_input<=hi,
        "within_observed_domain":model.observed_input_range[0]<=proposed_input&&proposed_input<=model.observed_input_range[1],
        "scope":"Empirical affine response calibrated from supplied observations. Proposed input requires physical validation; no nonlinear-dynamics accuracy, uncertainty calibration or speed gain is established."});
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?
        .write_all(&serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
