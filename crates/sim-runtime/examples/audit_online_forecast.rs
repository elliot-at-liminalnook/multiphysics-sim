//! Score recorded online forecasts only when their proposed action sequence was
//! actually executed. Reuses runtime physical labels and coordinate transforms.
use serde_json::{Value, json};
use sim_runtime::{
    embedded::Config,
    motion_forecast::samples_from_capture,
    predictive_policy::{ForecastBundle, ForecastObservation},
};
use std::fs;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if !(2..=3).contains(&args.len()) {
        return Err(
            "usage: audit_online_forecast capture.json new-report.json [replacement-bundle.json]"
                .into(),
        );
    }
    let capture: Value = serde_json::from_slice(&fs::read(&args[0])?)?;
    let config: Config = serde_json::from_value(capture["recording"]["config"].clone())?;
    let original = config
        .policy
        .as_ref()
        .and_then(|p| p.trajectory_forecast.as_ref())
        .ok_or("capture has no forecast bundle")?;
    let replacement: Option<ForecastBundle> = args
        .get(2)
        .map(|p| -> Result<_, Box<dyn std::error::Error>> {
            Ok(serde_json::from_slice(&fs::read(p)?)?)
        })
        .transpose()?;
    let bundle = replacement.as_ref().unwrap_or(original);
    bundle.validate()?;
    if bundle.heads.len() != original.heads.len()
        || bundle.heads.iter().zip(&original.heads).any(|(a, b)| {
            serde_json::to_value(&a.recipe).ok() != serde_json::to_value(&b.recipe).ok()
        })
    {
        return Err("replacement bundle must preserve the recorded forecast recipes".into());
    }
    let frames = capture["frames"].as_array().ok_or("missing frames")?;
    let end = frames.last().ok_or("empty frames")?["time_s"]
        .as_f64()
        .ok_or("missing final time")?;
    let mut heads = vec![];
    for (head_index, head) in bundle.heads.iter().enumerate() {
        let samples = samples_from_capture(&capture, &head.recipe, 0., end)?;
        let h = head.recipe.horizons_steps[0];
        let actions = head.recipe.actuator_targets.len();
        let action_offset = head.recipe.future_action_offset();
        let mut matched = 0;
        let mut mismatched = 0;
        let mut sums = vec![[0.; 2]; head.network.outputs.len()];
        for s in &samples {
            let anchor = (s.time_s / head.recipe.period_s).round() as usize;
            let policy = &frames
                .get(anchor + 1)
                .ok_or("missing online forecast frame")?["policy"];
            let f: ForecastObservation =
                serde_json::from_value(policy["trajectory_forecast"].clone())?;
            if !f.history_valid {
                continue;
            }
            if (f.time_s - s.time_s).abs() > 1e-8
                || f.predictions.len() != bundle.heads.len()
                || f.priors.len() != bundle.heads.len()
                || f.predictions[head_index].len() != s.targets.len()
                || f.priors[head_index].len() != s.targets.len()
                || f.proposed_targets_rad.len() < h
                || f.proposed_targets_rad[..h]
                    .iter()
                    .any(|a| a.len() != actions)
            {
                return Err("online forecast shape/clock mismatch".into());
            }
            let proposed = f.proposed_targets_rad[..h]
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            if proposed != s.inputs[action_offset..] {
                mismatched += 1;
                continue;
            }
            matched += 1;
            let prediction = if replacement.is_some() {
                head.predict(&s.inputs, &s.prior)?
            } else {
                f.predictions[head_index].clone()
            };
            for i in 0..s.targets.len() {
                sums[i][0] += (prediction[i] - s.targets[i]).powi(2);
                sums[i][1] += (f.priors[head_index][i] - s.targets[i]).powi(2);
            }
        }
        let rows = if matched == 0 {
            json!(null)
        } else {
            json!(head.network.outputs.iter().enumerate().map(|(i,o)|json!({
            "name":o.target,"unit":o.kind.unit(),"prediction_rmse":(sums[i][0]/matched as f64).sqrt(),"reference_rmse":(sums[i][1]/matched as f64).sqrt()
        })).collect::<Vec<_>>())
        };
        heads.push(json!({"horizon_s":h as f64*head.recipe.period_s,"available_windows":samples.len(),"matched_action_windows":matched,"different_action_windows":mismatched,"per_output":rows}));
    }
    let report = json!({"heads":heads,"prediction_source":if replacement.is_some(){"replacement_model"}else{"recorded_online"},"scope":"Prediction errors against committed future Rust motion, scored only where every recorded proposed actuator target exactly matches the applied sequence through that horizon. Optional replacement models use these same matched windows. Action mismatches are diagnostic exclusions, not gait acceptance conditions. Nominal replay is not a counterfactual validation on unseen commands."});
    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&args[1])?;
    serde_json::to_writer_pretty(file, &report)?;
    Ok(())
}
