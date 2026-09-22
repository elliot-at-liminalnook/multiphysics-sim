//! Import supervised single-pulse sweeps with a predeclared whole-repetition split.
use crate::{
    acquisition::servo_bus::{Telemetry, reply},
    experiment_comparison::{
        Limits, Observation, Trace, compare,
        hx_archive::{Archive, ENCODER_QUANTUM_RAD, Trial},
    },
    experiment_study::{self, Study},
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
fn num(v: &Value) -> Result<f64, String> {
    v.as_f64()
        .filter(|v| v.is_finite())
        .ok_or("Missing finite numeric evidence".into())
}
pub fn load(
    directory: &Path,
    split_path: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(usize, usize),
) -> Result<Study, String> {
    load_inner(directory, split_path, None, cancel, progress)
}
/// Import the explicit single-pulse/release experiment; other waveforms remain rejected.
/// Passive driver parameters are declared hypotheses, not inferred Hiwonder electronics.
pub fn load_release(
    directory: &Path,
    split_path: &Path,
    coast: crate::actuator_bench::DriverRelease,
    cancel: &AtomicBool,
    progress: impl FnMut(usize, usize),
) -> Result<Study, String> {
    if !matches!(
        coast,
        crate::actuator_bench::DriverRelease::TorqueOff { .. }
    ) {
        return Err("Release import needs explicit passive driver parameters".into());
    }
    load_inner(directory, split_path, Some(coast), cancel, progress)
}
fn load_inner(
    directory: &Path,
    split_path: &Path,
    coast: Option<crate::actuator_bench::DriverRelease>,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Study, String> {
    let mut hashes = BTreeMap::new();
    let mut read = |path: &Path| {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        hashes.insert(
            path.display().to_string(),
            blake3::hash(&bytes).to_hex().to_string(),
        );
        Ok::<_, String>(bytes)
    };
    let run: Value =
        serde_json::from_slice(&read(&directory.join("run.json"))?).map_err(|e| e.to_string())?;
    let checkpoint: Value = serde_json::from_slice(&read(&directory.join("checkpoint.json"))?)
        .map_err(|e| e.to_string())?;
    let split: Value = serde_json::from_slice(&read(split_path)?).map_err(|e| e.to_string())?;
    let transactions = String::from_utf8(read(&directory.join("transactions.jsonl"))?)
        .map_err(|e| e.to_string())?
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let csv = String::from_utf8(read(&directory.join("sweep.csv"))?).map_err(|e| e.to_string())?;
    if run["completed"] != true
        || run["mode"] != "fpga_supervised_pwm_sweep"
        || (split["frozen_before_acquisition"] != true && coast.is_none())
        || run["result"]["fpga_stop"]["Ok"]["latched"] != true
    {
        return Err("Require a completed supervised sweep, latched final stop and a declared pre-acquisition split".into());
    }
    if !run["result"]["recovery"].as_object().is_some_and(|m| {
        !m.is_empty()
            && m.values().all(|v| {
                v["final_state"]["Ok"]["torque_enable"] == serde_json::json!([0])
                    && v["final_state"]["Ok"]["telemetry"]["speed_raw"] == 0
            })
    }) {
        return Err(
            "All acquisition motors require verified torque-off and stationary final feedback"
                .into(),
        );
    }
    let train: Vec<u64> =
        serde_json::from_value(split["training_repetitions"].clone()).map_err(|e| e.to_string())?;
    let held: Vec<u64> = serde_json::from_value(split["validation_repetitions"].clone())
        .map_err(|e| e.to_string())?;
    if train.is_empty() || held.is_empty() || train.iter().any(|i| held.contains(i)) {
        return Err("Training and held-out repetition sets must be nonempty and disjoint".into());
    }
    let limits: Limits =
        serde_json::from_value(split["limits_rad"].clone()).map_err(|e| e.to_string())?;
    let mut lines = csv.lines();
    let header = lines
        .next()
        .ok_or("Missing sweep header")?
        .split(',')
        .collect::<Vec<_>>();
    let mut groups = BTreeMap::<(usize, u8), Vec<BTreeMap<&str, f64>>>::new();
    for line in lines {
        let values = line.split(',').collect::<Vec<_>>();
        if values.len() != header.len() {
            return Err("Partial sweep row".into());
        }
        let mut row = BTreeMap::new();
        for (key, value) in header.iter().zip(values) {
            if *key == "phase" {
                continue;
            }
            let number = if *key == "torque_enabled" {
                match value {
                    "true" => 1.,
                    "false" => 0.,
                    _ => return Err("Invalid captured torque-enable flag".into()),
                }
            } else {
                value.parse::<f64>().map_err(|e| e.to_string())?
            };
            if !number.is_finite() {
                return Err("Nonfinite sweep sample".into());
            }
            row.insert(*key, number);
        }
        let field = |k| {
            row.get(k)
                .copied()
                .ok_or_else(|| format!("Missing sweep column {k}"))
        };
        if field("status")? != 0. {
            return Err("Faulted sweep sample cannot be scored".into());
        }
        groups
            .entry((field("trial")? as usize, field("id")? as u8))
            .or_default()
            .push(row);
    }
    let outcomes = checkpoint["outcomes_this_run"]
        .as_array()
        .ok_or("Missing completed outcomes")?;
    let (baseline, _) = experiment_study::baseline()?;
    let mut trials = vec![];
    let name = directory
        .file_name()
        .ok_or("Missing sweep name")?
        .to_string_lossy()
        .to_string();
    for ((stage, id), rows) in groups {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let outcome = outcomes
            .iter()
            .find(|o| o["trial"]["index"] == stage)
            .ok_or("Sample has no completed outcome")?;
        let events = outcome["segment_events"]
            .as_array()
            .ok_or("Missing command events")?;
        let release = if let Some(coast) = &coast {
            if events.len() != 3
                || events[0]["phase"] != "excitation"
                || events[0]["command"]["torque_enabled"] != true
                || events[1]["phase"] != "excitation"
                || events[1]["command"]["drive"] != 0
                || !events[1]["command"]["torque_enabled"].is_boolean()
                || events[2]["phase"] != "rest"
                || events[2]["command"]["drive"] != 0
                || events[2]["command"]["torque_enabled"] != events[1]["command"]["torque_enabled"]
                || events[0]["command_sent"] != true
                || events[1]["command_sent"] != true
            {
                return Err("Release importer requires one driven pulse followed by an explicit zero-PWM or torque-off release and matching rest".into());
            }
            Some(if events[1]["command"]["torque_enabled"] == true {
                crate::actuator_bench::DriverRelease::ElectricalBrake
            } else {
                coast.clone()
            })
        } else {
            None
        };
        if outcome["stationary_verified"] != true
            || (coast.is_none()
                && (events.len() != 2
                    || events[0]["phase"] != "pulse"
                    || events[1]["command"]["drive"] != 0
                    || events[1]["command"]["torque_enabled"] != true))
        {
            return Err("Importer supports single-pulse/zero-PWM braking only; arbitrary waveforms need their own adapter".into());
        }
        let on = [num(&events[0]["request_s"])?, num(&events[0]["receipt_s"])?];
        let off = [num(&events[1]["request_s"])?, num(&events[1]["receipt_s"])?];
        let origin = (on[0] + on[1]) * 0.5;
        if on[0] > on[1] || off[0] > off[1] || off[0] <= on[1] {
            return Err("Invalid command windows".into());
        }
        let before = transactions
            .iter()
            .rev()
            .find(|t| {
                t["id"] == id
                    && t["instruction"] == 2
                    && t["tx"][5] == 56
                    && t["tx"][6] == 15
                    && t["completion_host_ns"]
                        .as_f64()
                        .is_some_and(|t| t * 1e-9 <= on[0])
            })
            .ok_or("Missing measured pre-pulse stationary position")?;
        let bytes: Vec<u8> =
            serde_json::from_value(before["rx"].clone()).map_err(|e| e.to_string())?;
        let response = reply(&bytes, id, 15).map_err(str::to_string)?;
        let start = Telemetry::decode(&response.parameters).map_err(str::to_string)?;
        if response.error != 0 || start.status != 0 || start.speed_raw != 0 {
            return Err("Pre-pulse feedback is not healthy/stationary".into());
        }
        let mut samples = vec![];
        for row in &rows {
            let field = |k| {
                row.get(k)
                    .copied()
                    .ok_or_else(|| format!("Missing sweep column {k}"))
            };
            let request = field("request_s")? - origin;
            let completion = field("completion_s")? - origin;
            if coast.is_some() {
                let index = field("segment_index")?;
                if index < 0. || index.fract() != 0. {
                    return Err("Invalid release segment index".into());
                }
                let event = events
                    .get(index as usize)
                    .ok_or("Sample refers to a missing release segment")?;
                if field("drive_counts")? != num(&event["command"]["drive"])?
                    || field("torque_enabled")?
                        != if event["command"]["torque_enabled"] == true {
                            1.
                        } else {
                            0.
                        }
                {
                    return Err(
                        "Sample PWM/torque state disagrees with captured release command".into(),
                    );
                }
            }
            let position = if row.contains_key("position_unwrapped_counts") {
                field("position_unwrapped_counts")?
            } else {
                field("position_raw")?
            };
            samples.push(Observation {
                time_s: (request + completion) * 0.5,
                value: (position - start.position_raw as f64) * ENCODER_QUANTUM_RAD,
                request_s: request,
                completion_s: completion,
            });
        }
        let range = |key| -> Result<[f64; 2], String> {
            let v = rows
                .iter()
                .map(|r| r.get(key).copied().ok_or("Missing condition".to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            Ok([
                v.iter().copied().fold(f64::INFINITY, f64::min),
                v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            ])
        };
        let repetition = outcome["trial"]["repetition"]
            .as_u64()
            .ok_or("Missing repetition")?;
        let role = if train.contains(&repetition) {
            "train"
        } else if held.contains(&repetition) {
            "held_out_repetition"
        } else {
            return Err("Repetition has no predeclared role".into());
        };
        let measured = Trace {
            quantity: sim_core::QuantityKind::Angle.definition_id(),
            unit: "rad".into(),
            samples,
        };
        let mut trial = Trial {
            id: format!("{name}/{id}/{stage}"),
            run: name.clone(),
            device: id,
            stage,
            kind: match &release {
                Some(crate::actuator_bench::DriverRelease::TorqueOff { .. }) => {
                    "supervised_torque_off_release"
                }
                Some(_) => "supervised_zero_pwm_release",
                None => "supervised_individual_pwm_zero_brake",
            }
            .into(),
            drive: num(&outcome["trial"]["drive"])? / 1000.,
            duration_s: (off[0] + off[1]) * 0.5 - origin,
            split: role.into(),
            voltage_range_v: range("voltage_v")?,
            temperature_range_c: range("temperature_c")?,
            on_host_window_s: on,
            off_host_window_s: off,
            predicted: measured.clone(),
            comparison: compare(&measured, &measured, &limits)?,
            measured,
            limits: limits.clone(),
            release,
        };
        trial.predicted = experiment_study::simulate(&trial, &baseline, cancel)?;
        trial.comparison = compare(&trial.measured, &trial.predicted, &limits)?;
        trials.push(trial);
        progress(trials.len(), outcomes.len());
    }
    if trials.len() != outcomes.len() {
        return Err(
            "Pulse importer requires one motor per trial and exact outcome/sample coverage".into(),
        );
    }
    let archive = Archive {
        label: name,
        trials,
        interpretation: format!(
            "Retained reference is the uncalibrated shared physical baseline, not an empirical fit. All acquired samples retained. {}",
            run["plan"]["bench_condition"]
        ),
        split_policy: serde_json::to_string(&split).map_err(|e| e.to_string())?,
        observation_blake3: hashes[&directory.join("sweep.csv").display().to_string()].clone(),
        model_blake3: baseline.fingerprint(),
        verified_inputs: 0,
        input_blake3: hashes,
        integrity_issues: vec![],
    };
    let mut study = Study::new(archive)?;
    study.notes="Raw source hashes captured at import; no external manifest authentication claimed. Whole-trial roles and their declaration provenance are retained in the split policy. Command timings are host transaction midpoint assumptions.".into();
    if coast.is_some() {
        study.validation_influenced = split["frozen_before_acquisition"] != true;
        study.notes.push_str("Explicit release comparison. Zero PWM is modeled as electrical braking; torque-off uses declared passive leakage/diode hypotheses. Device electronics and current calibration remain unverified. Modified or inspected split declarations require fresh confirmation.");
    }
    Ok(study)
}
