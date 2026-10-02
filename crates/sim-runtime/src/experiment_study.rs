//! Retained measurements → shared physical runtime → immutable candidate evidence.
//! No hardware IO and no CAD mutation. Hosts schedule work off the UI thread.
pub mod commands;
pub mod input_content;
pub mod terminal;
mod publication;
pub mod portable;
#[cfg(test)]
mod portable_fixtures;
mod report;
#[cfg(test)]
mod publication_fixtures;
#[cfg(test)]
mod input_content_fixtures;
pub mod refinement;
#[cfg(test)]
mod compatibility;

use crate::experiment_comparison::{
    Comparison, Limits, Trace, compare,
    hx_archive::{Archive, Trial},
};
use crate::physics_context::RuntimeIdentity;
use serde::{Deserialize, Serialize};
use sim_domain_robot::motor::*;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power: Option<crate::controller_refinement::power::Setup>,
    pub motor: BTreeMap<String, f64>,
    pub bridge: BTreeMap<String, f64>,
    pub conditions: Conditions,
    pub step_s: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conditions {
    /// None uses each trial's reported range midpoint, not an exact measured waveform.
    pub voltage_v: Option<f64>,
    pub temperature_c: Option<f64>,
    pub load_inertia: f64,
    pub load_torque: f64,
    pub command_delay_s: f64,
}
impl ModelSettings {
    pub fn fingerprint(&self) -> String {
        blake3::hash(&serde_json::to_vec(self).unwrap())
            .to_hex()
            .to_string()
    }
    pub fn validate(&self) -> Result<(), String> {
        if let Some(power) = &self.power {
            power.validate().map_err(|e|format!("power: {e}"))?;
            if self.conditions.voltage_v.is_some() {
                return Err(
                    "conditions.voltage_v: an explicit power source cannot also have a fixed voltage override".into(),
                );
            }
        }
        let registry = crate::registry();
        for (name, params) in [(MOTOR_UNIT, &self.motor), (H_BRIDGE, &self.bridge)] {
            registry
                .get(&name.into())
                .map_err(|e| e.to_string())?
                .validate_parameters(params)
                .map_err(|e| format!("{name}: {e}"))?;
        }
        let c = &self.conditions;
        for (path, valid) in [
            ("step_s", self.step_s.is_finite() && (0.00001..=0.002).contains(&self.step_s)),
            ("conditions.load_inertia", c.load_inertia.is_finite() && c.load_inertia > 0.),
            ("conditions.load_torque", c.load_torque.is_finite()),
            ("conditions.command_delay_s", c.command_delay_s.is_finite() && (0. ..=0.5).contains(&c.command_delay_s)),
            ("conditions.voltage_v", c.voltage_v.is_none_or(|v|v.is_finite() && v>0.)),
            ("conditions.temperature_c", c.temperature_c.is_none_or(|v|v.is_finite() && v> -273.15)),
        ] { if !valid { return Err(format!("{path}: invalid value (step 10 µs–2 ms, delay 0–0.5 s, positive inertia/voltage, temperature above absolute zero)")); } }
        Ok(())
    }
}
/// Baseline is the existing endpoint-derived characterization hypothesis, not a fitted motor calibration.
pub fn baseline() -> Result<(ModelSettings, serde_json::Value), String> {
    let source: serde_json::Value =
        serde_json::from_str(include_str!("../../../examples/actuators/hx30hm/plan.json"))
            .map_err(|e| e.to_string())?;
    let motor: sim_domain_robot::model::Motor =
        serde_json::from_value(source["motor"].clone()).map_err(|e| e.to_string())?;
    let mut parameters: BTreeMap<String, f64> =
        cad_motor_unit_parameters(&motor, 0., 298.15, true, false)
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect();
    let overrides: BTreeMap<String, f64> =
        serde_json::from_value(source["cases"][0]["motor_overrides"].clone())
            .map_err(|e| e.to_string())?;
    parameters.extend(overrides);
    let settings = ModelSettings {
        power: None,
        motor: parameters,
        bridge: cad_h_bridge_parameters(&motor),
        conditions: Conditions {
            voltage_v: None,
            temperature_c: None,
            load_inertia: 0.00002,
            load_torque: 0.,
            command_delay_s: 0.,
        },
        step_s: 0.00025,
    };
    settings.validate()?;
    Ok((settings, source))
}

pub const ASSUMPTIONS: &str = "Exploratory physical simulation, not matched controller validation. Reconstructed single PWM pulse at command midpoint; legacy pulses assume zero-duty electrical braking; trials with explicit release hypotheses retain their separate disabled-driver behavior. Starts at rest with zero current and relative angle zero. Servo firmware and sensor sample age are not modeled. Legacy fixtures omit electronics consumption; explicit electrical scenarios retain their source and auxiliary-load hypotheses. Supply uses the declared electrical source when present, otherwise the reported range midpoint; temperature uses the reported range midpoint unless overridden; temperature is clamped, not a thermal prediction. Load inertia is an explicit unmeasured fixture assumption, load torque a signed opposing-positive torque. Baseline parameters are endpoint-derived hypotheses from the retained characterization plan, not identified physical constants. No implicit time alignment. No physical properties are promoted into CAD.";

/// Intended execution identity is captured by hosts before spawning a job,
/// including jobs cancelled before runtime execution begins.
pub fn execution_identity() -> serde_json::Value {
    serde_json::json!({"runtime":RuntimeIdentity::current(),
        "integrator":sim_dynamics::Integrator::BackwardEuler(crate::newton()),"seed":0u64})
}

fn validate_trial_inputs(trial: &Trial) -> Result<(),String> {
    if !trial.drive.is_finite() || trial.drive.abs()>1. {return Err(format!("archive.trials.{}.drive: expected finite duty in [-1,1]",trial.id));}
    if !trial.duration_s.is_finite() || trial.duration_s<=0. || trial.duration_s>60. {return Err(format!("archive.trials.{}.duration_s: expected positive duration <=60 s",trial.id));}
    for (name, range, lower) in [("voltage_range_v",trial.voltage_range_v,0.),("temperature_range_c",trial.temperature_range_c,-273.15)] {
        if range.iter().any(|v|!v.is_finite() || *v<=lower) || range[0]>range[1] {return Err(format!("archive.trials.{}.{name}: invalid range",trial.id));}
    }
    for (name,range) in [("on_host_window_s",trial.on_host_window_s),("off_host_window_s",trial.off_host_window_s)] {if range.iter().any(|v|!v.is_finite()) || range[0]>range[1] {return Err(format!("archive.trials.{}.{name}: invalid timing range",trial.id));}}
    if trial.measured.samples.is_empty() || trial.measured.samples.iter().any(|s| !s.time_s.is_finite() || s.time_s<0. || s.time_s>60. || !s.value.is_finite()) {return Err(format!("archive.trials.{}.measured.samples: finite nonnegative recording <=60 s required",trial.id));}
    Ok(())
}

pub fn simulate(
    trial: &Trial,
    settings: &ModelSettings,
    cancel: &AtomicBool,
) -> Result<Trace, String> {
    settings.validate()?;
    validate_trial_inputs(trial)?;
    if !trial.drive.is_finite()
        || trial.drive.abs() > 1.
        || !trial.duration_s.is_finite()
        || trial.duration_s <= 0.
        || trial.measured.samples.last().is_none_or(|s| s.time_s > 60.)
    {
        return Err(
            "Pulse replay requires bounded duty and a recording of at most 60 seconds".into(),
        );
    }
    let result = (|| -> Result<Trace, Box<dyn std::error::Error>> {
        let c = &settings.conditions;
        let mut bench = crate::actuator_bench::prepare(
            settings,
            c.voltage_v
                .unwrap_or((trial.voltage_range_v[0] + trial.voltage_range_v[1]) * 0.5),
            c.temperature_c
                .unwrap_or((trial.temperature_range_c[0] + trial.temperature_range_c[1]) * 0.5),
            if let Some(release) = &trial.release {
                crate::actuator_bench::Drive::ReleasedPulse {
                    duty: trial.drive,
                    start_s: c.command_delay_s,
                    duration_s: trial.duration_s,
                    release: release.clone(),
                }
            } else {
                crate::actuator_bench::Drive::Pulse {
                    duty: trial.drive,
                    start_s: c.command_delay_s,
                    duration_s: trial.duration_s,
                }
            },
        )?;
        let output = bench.angle;
        let mut prediction = trial.measured.clone();
        let mut time = 0.;
        for sample in &mut prediction.samples {
            if sample.time_s < 0. {
                return Err("Pretrigger samples need an explicit initial-state experiment".into());
            }
            while time < sample.time_s {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Cancelled".into());
                }
                let duration = (sample.time_s - time).min(if settings.power.is_some() {
                    settings.step_s
                } else {
                    0.002
                });
                bench.runtime.advance(duration, settings.step_s)?;
                time += duration;
                if let Some(e) = bench.electrical_sample(time) {
                    e.validate()?;
                    if e.state_of_charge.is_some_and(|v| !(0. ..=1.).contains(&v)) {
                        return Err("Battery state of charge outside [0,1]; pulse prediction cannot continue".into());
                    }
                }
            }
            sample.value = bench.runtime.get(output);
            sample.request_s = sample.time_s;
            sample.completion_s = sample.time_s;
        }
        Ok(prediction)
    })();
    result.map_err(|e| e.to_string())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prediction {
    pub trace: Trace,
    pub metrics: Comparison,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrialResult {
    pub trial_id: String,
    pub baseline: Option<Prediction>,
    pub candidate: Option<Prediction>,
    pub errors: Vec<String>,
}
/// Comparison outcomes require a complete error-free baseline/candidate pair.
/// A surviving individual prediction retains inspectable metrics, not a pair score.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrialOutcome { Pass, Fail, Unscored }
impl TrialOutcome {
    pub fn label(self)->&'static str {match self {Self::Pass=>"PASS",Self::Fail=>"FAIL",Self::Unscored=>"UNSCORED"}}
    pub fn is_scored(self)->bool {self!=Self::Unscored}
}
impl TrialResult {
    pub fn pair(&self)->Option<(&Prediction,&Prediction)> {
        if !self.errors.is_empty() {return None;}
        self.baseline.as_ref().zip(self.candidate.as_ref())
    }
    pub fn outcome(&self)->TrialOutcome {
        match self.pair() {Some((_,c)) if c.metrics.passes=>TrialOutcome::Pass,Some(_)=>TrialOutcome::Fail,None=>TrialOutcome::Unscored}
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evaluation {
    /// Captured archive/document evidence; absent in historical reviews.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub capture: BTreeMap<String, serde_json::Value>,
    pub id: String,
    pub runtime: RuntimeIdentity,
    pub integrator: sim_dynamics::Integrator,
    pub seed: u64,
    pub assumptions: String,
    pub baseline: ModelSettings,
    pub candidate: ModelSettings,
    pub limits: Option<Limits>,
    pub results: Vec<TrialResult>,
    pub cancelled: bool,
    pub validation_influenced: bool,
    pub decision: String,
    pub notes: String,
}
impl Evaluation {
    pub fn stale(
        &self,
        baseline: &ModelSettings,
        candidate: &ModelSettings,
        limits: &Option<Limits>,
    ) -> bool {
        self.baseline != *baseline || self.candidate != *candidate || self.limits != *limits
    }
}
/// Whole-trial evaluations retain failed and cancelled work; no successful trace is overwritten.
pub fn evaluate(
    archive: &Archive,
    ids: &[String],
    baseline: &ModelSettings,
    candidate: &ModelSettings,
    limits: Option<Limits>,
    validation_influenced: bool,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Evaluation, String> {
    baseline.validate()?;
    candidate.validate()?;
    commands::validate_limits(&limits)?;
    if ids.is_empty() { return Err("evaluation.trial_ids: select at least one trial".into()); }
    let mut seen=std::collections::BTreeSet::new();
    for id in ids {
        if !seen.insert(id) || !archive.trials.iter().any(|t|&t.id==id) { return Err(format!("evaluation.trial_ids.{id}: duplicate or unknown trial")); }
    }
    let mut results = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let trial = archive
            .trials
            .iter()
            .find(|t| &t.id == id)
            .ok_or("Unknown trial")?;
        let limits = limits.as_ref().unwrap_or(&trial.limits);
        let predict = |settings| {
            simulate(trial, settings, cancel).and_then(|trace| {
                compare(&trial.measured, &trace, limits)
                    .map(|metrics| Prediction { trace, metrics })
            })
        };
        let mut errors = Vec::new();
        let b = match predict(baseline) {
            Ok(v) => Some(v),
            Err(e) => {
                errors.push(format!("Baseline: {e}"));
                None
            }
        };
        let c = if baseline == candidate {
            b.clone()
        } else {
            match predict(candidate) {
                Ok(v) => Some(v),
                Err(e) => {
                    errors.push(format!("Candidate: {e}"));
                    None
                }
            }
        };
        results.push(TrialResult {
            trial_id: id.clone(),
            baseline: b,
            candidate: c,
            errors,
        });
        progress(index + 1, ids.len());
    }
    // Explicit unscored entries for work not reached after cancellation.
    for id in &ids[results.len()..] {
        results.push(TrialResult {
            trial_id: id.clone(),
            baseline: None,
            candidate: None,
            errors: vec!["Cancelled before trial".into()],
        });
    }
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos()
        .to_string();
    Ok(Evaluation {
        capture: BTreeMap::from([
            ("observation_blake3".into(), serde_json::json!(archive.observation_blake3)),
            ("model_blake3".into(), serde_json::json!(archive.model_blake3)),
            ("trial_ids".into(), serde_json::json!(ids)),
            ("split_policy".into(), serde_json::json!(archive.split_policy)),
            ("trial_splits".into(), serde_json::json!(archive.trials.iter().filter(|t|ids.contains(&t.id)).map(|t|(&t.id,&t.split)).collect::<BTreeMap<_,_>>())),
        ]),
        id,
        runtime: RuntimeIdentity::current(),
        integrator: sim_dynamics::Integrator::BackwardEuler(crate::newton()),
        seed: 0,
        assumptions: ASSUMPTIONS.into(),
        baseline: baseline.clone(),
        candidate: candidate.clone(),
        limits,
        results,
        cancelled: cancel.load(Ordering::Relaxed),
        validation_influenced,
        decision: "Investigating".into(),
        notes: String::new(),
    })
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ReviewView {
    pub trial_id: Option<String>,
    pub evaluation: Option<usize>,
    pub component_id: Option<String>,
    pub device: u8,
    pub direction: i8,
    pub min_drive: f64,
    pub max_drive: f64,
    pub role: String,
    pub outcome: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Study {
    #[serde(default)]
    pub input_contents: input_content::Store,
    pub version: u32,
    pub archive: Archive,
    pub baseline: ModelSettings,
    pub draft: ModelSettings,
    pub baseline_source: serde_json::Value,
    pub evaluations: Vec<Evaluation>,
    pub view: ReviewView,
    pub limits: Option<Limits>,
    pub validation_seen: bool,
    pub validation_influenced: bool,
    pub notes: String,
    #[serde(default)]
    pub refinement: crate::controller_refinement::workspace::Workspace,
    #[serde(default)]
    pub refinement_evidence: refinement::Evidence,
    #[serde(flatten, default)]
    pub retained_fields: BTreeMap<String, serde_json::Value>,
}
impl Study {
    pub fn new(archive: Archive) -> Result<Self, String> {
        let (baseline, source) = baseline()?;
        Ok(Self {
            version: 1,
            input_contents: Default::default(),
            archive,
            draft: baseline.clone(),
            baseline,
            baseline_source: source,
            evaluations: vec![],
            view: ReviewView {
                max_drive: 1.,
                role: "All".into(),
                outcome: "All".into(),
                ..Default::default()
            },
            limits: None,
            validation_seen: false,
            validation_influenced: false,
            notes: String::new(),
            refinement: Default::default(),
            refinement_evidence: Default::default(),
            retained_fields: Default::default(),
        })
    }
    pub fn candidate_edited(&mut self) {
        // Inspection before reservation still influences a later held-out declaration.
        for influenced in self.refinement_evidence.recording_exposure.values_mut() {
            *influenced = true;
        }
        if self.validation_seen {
            self.validation_influenced = true;
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("Unsupported comparison file version".into());
        }
        self.input_contents.validate()?;
        for (i, receipt) in self.refinement_evidence.receipts.iter().enumerate() {
            if let Some(envelope)=receipt.inputs.get("terminal_result").filter(|v| !v.is_null()) {
                self.input_contents.validate_envelope(envelope,&format!("refinement_evidence.receipts.{i}.inputs.terminal_result"))?;
            }
            if let Some(envelope)=receipt.inputs.get("electrical") {
                self.input_contents.validate_envelope(envelope,&format!("refinement_evidence.receipts.{i}.inputs.electrical"))?;
            }
            if let Some(envelope) = receipt.inputs.get("additional_input") {
                self.input_contents.validate_envelope(envelope, &format!("refinement_evidence.receipts.{i}.inputs.additional_input"))?;
            }
        }
        for (key, value) in &self.retained_fields {
            if key == "native_offline_job_receipts" || key.starts_with("native_offline_job_receipts_retained_") {
                if let Some(rows) = value.as_array() {
                    for (i, row) in rows.iter().enumerate() {
                        if let Some(envelope)=row.get("terminal_result").filter(|v| !v.is_null()) {
                            self.input_contents.validate_envelope(envelope,&format!("{key}.{i}.terminal_result"))?;
                        }
                        if let Some(envelope) = row.get("launch").and_then(|launch| launch.get("additional_input")) {
                            self.input_contents.validate_envelope(envelope, &format!("{key}.{i}.launch.additional_input"))?;
                        }
                    }
                }
            }
        }
        self.baseline.validate()?;
        self.draft.validate()?;
        self.refinement.validate()?;
        self.refinement.validate_archive(&self.archive)?;
        refinement::validate_evidence(self)?;
        commands::validate_limits(&self.limits)?;
        commands::validate_view(self)?;
        let mut ids = std::collections::BTreeSet::new();
        for t in &self.archive.trials {
            // Archival comparison evidence may be valid even when pulse replay
            // cannot represent its duration or pretrigger initial conditions.
            // compare below validates retained samples/limits independently.
            if let Some(release) = &t.release {
                release.validate()?;
            }
            if !ids.insert(&t.id) {
                return Err("Duplicate trial identity".into());
            }
            if compare(&t.measured, &t.predicted, &t.limits)? != t.comparison {
                return Err("Saved empirical metrics do not match measurements".into());
            }
        }
        if ids.is_empty() {
            return Err("Empty measurement archive".into());
        }
        for e in &self.evaluations {
            commands::validate_limits(&e.limits)?;
            e.runtime.validate()?;
            crate::system_session::SessionConfig {
                interval: e.baseline.step_s,
                integrator: e.integrator,
                seed: e.seed,
                grid_snapping: false,
            }
            .validate()?;
            if e.assumptions.is_empty() {
                return Err("Missing experiment assumptions".into());
            }
            e.baseline.validate()?;
            e.candidate.validate()?;
            let mut seen = std::collections::BTreeSet::new();
            for r in &e.results {
                if !seen.insert(&r.trial_id) {
                    return Err("Duplicate result identity".into());
                }
                let trial = self
                    .archive
                    .trials
                    .iter()
                    .find(|t| t.id == r.trial_id)
                    .ok_or("Result references missing trial")?;
                for prediction in [&r.baseline, &r.candidate].into_iter().flatten() {
                    let actual = compare(
                        &trial.measured,
                        &prediction.trace,
                        e.limits.as_ref().unwrap_or(&trial.limits),
                    )?;
                    if actual != prediction.metrics {
                        return Err("Saved metrics do not match saved traces".into());
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Default, Debug)]
pub struct ScoreSummary {
    pub total: usize,
    pub passes: usize,
    pub failures: usize,
    pub unscored: usize,
    pub improved: usize,
    pub regressed: usize,
    pub new_failures: usize,
}
impl Evaluation {
    pub fn summary(&self, ids: &[String]) -> ScoreSummary {
        let mut s = ScoreSummary {
            total: ids.len(),
            ..Default::default()
        };
        for id in ids {
            let Some(r) = self.results.iter().find(|r| &r.trial_id == id) else {
                s.unscored += 1;
                continue;
            };
            if !r.outcome().is_scored() {
                s.unscored += 1;
                continue;
            }
            if let Some((b,c)) = r.pair() {
                match r.outcome() {TrialOutcome::Pass=>s.passes+=1,TrialOutcome::Fail=>s.failures+=1,TrialOutcome::Unscored=>{}}

                {
                    if c.metrics.rmse < b.metrics.rmse {
                        s.improved += 1;
                    }
                    if c.metrics.rmse > b.metrics.rmse {
                        s.regressed += 1;
                    }
                    if b.metrics.passes && !c.metrics.passes {
                        s.new_failures += 1;
                    }
                }
            } else {
                s.unscored += 1;
            }
        }
        s
    }
}
/// Human-readable changes retained separately for model and experimental assumptions.
pub fn changes(baseline: &ModelSettings, candidate: &ModelSettings) -> Vec<String> {
    let mut changes = vec![];
    if baseline.power != candidate.power {
        changes.push("Electrical source/battery scenario changed (retained source parameters, auxiliary load, evidence and limits)".into());
    }
    for (group, b, c) in [
        ("Motor", &baseline.motor, &candidate.motor),
        ("Driver", &baseline.bridge, &candidate.bridge),
    ] {
        for (name, value) in c {
            if b.get(name) != Some(value) {
                changes.push(format!("{group} {name}: {:?} → {value}", b.get(name)));
            }
        }
    }
    for (name, b, c) in [
        (
            "Supply override V",
            baseline.conditions.voltage_v,
            candidate.conditions.voltage_v,
        ),
        (
            "Temperature override °C",
            baseline.conditions.temperature_c,
            candidate.conditions.temperature_c,
        ),
        (
            "Load inertia kg·m²",
            Some(baseline.conditions.load_inertia),
            Some(candidate.conditions.load_inertia),
        ),
        (
            "Signed load torque N·m",
            Some(baseline.conditions.load_torque),
            Some(candidate.conditions.load_torque),
        ),
        (
            "Command delay s",
            Some(baseline.conditions.command_delay_s),
            Some(candidate.conditions.command_delay_s),
        ),
        ("Step s", Some(baseline.step_s), Some(candidate.step_s)),
    ] {
        if b != c {
            changes.push(format!(
                "Condition / timing {name}: {} → {}",
                b.map(|v| v.to_string()).unwrap_or("trial midpoint".into()),
                c.map(|v| v.to_string()).unwrap_or("trial midpoint".into())
            ));
        }
    }
    changes
}
