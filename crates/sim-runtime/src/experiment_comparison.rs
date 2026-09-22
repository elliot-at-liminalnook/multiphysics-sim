//! Offline experiment comparison. Acquisition, predictions and decisions remain
//! distinct; this module never opens hardware or changes a model/CAD parameter.
use serde::{Deserialize, Serialize};
use sim_core::definitions::DefinitionId;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub time_s: f64,
    pub value: f64,
    pub request_s: f64,
    pub completion_s: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Trace {
    pub quantity: DefinitionId,
    pub unit: String,
    pub samples: Vec<Observation>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub rmse: f64,
    pub final_abs_error: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    /// Prediction minus measurement, in the declared canonical unit.
    pub residuals: Vec<f64>,
    pub rmse: f64,
    pub maximum_abs_error: f64,
    pub final_error: f64,
    pub passes: bool,
}
/// Predictions must explicitly sample the measurement time coordinates. No
/// hidden resampling, fitted time shift, extrapolation or deleted outlier rows.
pub fn compare(measured: &Trace, predicted: &Trace, limits: &Limits) -> Result<Comparison, String> {
    if measured.quantity != predicted.quantity || measured.unit != predicted.unit {
        return Err("measurement/prediction quantity or canonical unit mismatch".into());
    }
    if measured.samples.is_empty() || measured.samples.len() != predicted.samples.len() {
        return Err("comparison requires matching nonempty sample sets".into());
    }
    for trace in [measured, predicted] {
        if trace.samples.iter().any(|s| {
            !s.time_s.is_finite()
                || !s.value.is_finite()
                || !s.request_s.is_finite()
                || !s.completion_s.is_finite()
                || s.request_s > s.time_s
                || s.time_s > s.completion_s
        }) || trace.samples.windows(2).any(|w| w[0].time_s >= w[1].time_s)
        {
            return Err("invalid values, timing windows or sample order".into());
        }
    }
    if !limits.rmse.is_finite()
        || limits.rmse < 0.
        || !limits.final_abs_error.is_finite()
        || limits.final_abs_error < 0.
    {
        return Err("explicit finite nonnegative acceptance limits required".into());
    }
    let mut residuals = Vec::with_capacity(measured.samples.len());
    for (m, p) in measured.samples.iter().zip(&predicted.samples) {
        if m.time_s != p.time_s {
            return Err("prediction does not use the declared measurement times".into());
        }
        let error = p.value - m.value;
        if !error.is_finite() {
            return Err("nonfinite residual".into());
        }
        residuals.push(error);
    }
    let maximum_abs_error = residuals.iter().map(|e| e.abs()).fold(0., f64::max);
    let rmse = if maximum_abs_error == 0. {
        0.
    } else {
        maximum_abs_error
            * (residuals
                .iter()
                .map(|e| (e / maximum_abs_error).powi(2))
                .sum::<f64>()
                / residuals.len() as f64)
                .sqrt()
    };
    let final_error = *residuals.last().unwrap();
    Ok(Comparison {
        residuals,
        rmse,
        maximum_abs_error,
        final_error,
        passes: rmse <= limits.rmse && final_error.abs() <= limits.final_abs_error,
    })
}

/// Historical HX extraction adapter. Generic comparison and UI traces use SI;
/// the source's encoder-count acceptance criteria are converted explicitly.
pub mod hx_archive {
    use super::*;
    use sim_solve::pulse_response::{PulseObservation, PulseResponse};
    use std::{collections::BTreeMap, path::Path};
    pub const ENCODER_QUANTUM_RAD: f64 = std::f64::consts::TAU / 4096.;
    #[derive(Clone, Debug, Deserialize)]
    struct ExtractedTrial {
        run: String,
        id: u8,
        stage: usize,
        kind: String,
        observation: PulseObservation,
        windows: Vec<[f64; 2]>,
        voltage_range_v: [f64; 2],
        temperature_range_c: [f64; 2],
        on_host_window_s: [f64; 2],
        off_host_window_s: [f64; 2],
    }
    #[derive(Deserialize)]
    struct Evaluation {
        run: String,
        stage: usize,
        split: String,
    }
    #[derive(Deserialize)]
    struct Model {
        id: u8,
        model: PulseResponse,
        evaluations: Vec<Evaluation>,
    }
    #[derive(Deserialize)]
    struct Results {
        schema_version: u32,
        input_blake3: BTreeMap<String, String>,
        models: Vec<Model>,
        interpretation: String,
        split: String,
    }
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Trial {
        pub id: String,
        pub run: String,
        pub device: u8,
        pub stage: usize,
        pub kind: String,
        pub drive: f64,
        pub duration_s: f64,
        pub split: String,
        pub voltage_range_v: [f64; 2],
        pub temperature_range_c: [f64; 2],
        pub on_host_window_s: [f64; 2],
        pub off_host_window_s: [f64; 2],
        pub measured: Trace,
        pub predicted: Trace,
        pub limits: Limits,
        pub comparison: Comparison,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub release: Option<crate::actuator_bench::DriverRelease>,
    }
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Archive {
        pub label: String,
        pub trials: Vec<Trial>,
        pub interpretation: String,
        pub split_policy: String,
        pub observation_blake3: String,
        pub model_blake3: String,
        pub verified_inputs: usize,
        pub input_blake3: BTreeMap<String, String>,
        pub integrity_issues: Vec<String>,
    }
    pub fn load(directory: &Path, repository: &Path) -> Result<Archive, String> {
        let read = |name| std::fs::read(directory.join(name)).map_err(|e| format!("{name}: {e}"));
        let observations = read("observations.json")?;
        let models = read("results.json")?;
        let rows: Vec<ExtractedTrial> =
            serde_json::from_slice(&observations).map_err(|e| e.to_string())?;
        let results: Results = serde_json::from_slice(&models).map_err(|e| e.to_string())?;
        if results.schema_version != 1 {
            return Err("unsupported identification archive version".into());
        }
        let mut matched = BTreeMap::new();
        for m in &results.models {
            for e in &m.evaluations {
                if !matches!(
                    e.split.as_str(),
                    "train"
                        | "held_out_drive"
                        | "held_out_duration_direction"
                        | "held_out_direction"
                ) {
                    return Err(format!("unknown data split {}", e.split));
                }
                if matched
                    .insert(
                        (m.id, e.run.as_str(), e.stage),
                        (&m.model, e.split.as_str()),
                    )
                    .is_some()
                {
                    return Err("duplicate model evaluation identity".into());
                }
            }
        }
        let mut trials = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for row in rows {
            let key = (row.id, row.run.clone(), row.stage);
            let (model, split) = matched
                .get(&(key.0, key.1.as_str(), key.2))
                .ok_or("measurement trial has no corresponding model evaluation")?;
            if !seen.insert((row.id, row.run.clone(), row.stage)) {
                return Err("duplicate measurement trial identity".into());
            }
            if row.windows.len() != row.observation.samples.len() {
                return Err("measurement windows do not match samples".into());
            }
            for window in [
                row.on_host_window_s,
                row.off_host_window_s,
                row.voltage_range_v,
                row.temperature_range_c,
            ] {
                if window.iter().any(|v| !v.is_finite()) || window[0] > window[1] {
                    return Err("invalid condition/command window".into());
                }
            }
            let measured = Trace {
                quantity: sim_core::QuantityKind::Angle.definition_id(),
                unit: "rad".into(),
                samples: row
                    .observation
                    .samples
                    .iter()
                    .zip(&row.windows)
                    .map(|(s, w)| Observation {
                        time_s: s[0],
                        value: s[1],
                        request_s: w[0],
                        completion_s: w[1],
                    })
                    .collect(),
            };
            let mut predicted = measured.clone();
            for s in &mut predicted.samples {
                s.value =
                    model.predict(row.observation.input, row.observation.duration_s, s.time_s)?[0];
                s.request_s = s.time_s;
                s.completion_s = s.time_s;
            }
            let limits = Limits {
                rmse: 3. * ENCODER_QUANTUM_RAD,
                final_abs_error: 5. * ENCODER_QUANTUM_RAD,
            };
            let comparison = compare(&measured, &predicted, &limits)?;
            trials.push(Trial {
                release: None,
                id: format!("{}/{}/{}", row.run, row.id, row.stage),
                run: row.run,
                device: row.id,
                stage: row.stage,
                kind: row.kind,
                drive: row.observation.input,
                duration_s: row.observation.duration_s,
                split: (*split).into(),
                voltage_range_v: row.voltage_range_v,
                temperature_range_c: row.temperature_range_c,
                on_host_window_s: row.on_host_window_s,
                off_host_window_s: row.off_host_window_s,
                measured,
                predicted,
                limits,
                comparison,
            });
        }
        if seen.len() != matched.len() {
            return Err("model evaluations and measured trials are not one-to-one".into());
        }
        trials.sort_by(|a, b| (a.device, &a.run, a.stage).cmp(&(b.device, &b.run, b.stage)));
        let mut verified_inputs = 0;
        let mut integrity_issues = Vec::new();
        for (path, expected) in &results.input_blake3 {
            if Path::new(&path)
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                integrity_issues.push(format!("Unsupported input reference: {path}"));
                continue;
            }
            match std::fs::read(repository.join(&path)) {
                Ok(bytes) if blake3::hash(&bytes).to_hex().as_str() == expected.as_str() => {
                    verified_inputs += 1
                }
                Ok(_) => integrity_issues.push(format!("Hash mismatch: {path}")),
                Err(_) => integrity_issues.push(format!("Missing source: {path}")),
            }
        }
        Ok(Archive {
            label: directory
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            trials,
            interpretation: results.interpretation,
            split_policy: results.split,
            observation_blake3: blake3::hash(&observations).to_hex().to_string(),
            model_blake3: blake3::hash(&models).to_hex().to_string(),
            verified_inputs,
            input_blake3: results.input_blake3,
            integrity_issues,
        })
    }
}
