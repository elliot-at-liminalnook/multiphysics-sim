//! Retained measurements → shared physical runtime → immutable candidate evidence.
//! No hardware IO and no CAD mutation. Hosts schedule work off the UI thread.
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
            power.validate()?;
            if self.conditions.voltage_v.is_some() {
                return Err(
                    "An explicit power source cannot also have a fixed voltage override".into(),
                );
            }
        }
        let registry = crate::registry();
        for (name, params) in [(MOTOR_UNIT, &self.motor), (H_BRIDGE, &self.bridge)] {
            registry
                .get(&name.into())
                .map_err(|e| e.to_string())?
                .validate_parameters(params)
                .map_err(|e| e.to_string())?;
        }
        let c = &self.conditions;
        if !self.step_s.is_finite()
            || !(0.00001..=0.002).contains(&self.step_s)
            || !c.load_inertia.is_finite()
            || c.load_inertia <= 0.
            || !c.load_torque.is_finite()
            || !c.command_delay_s.is_finite()
            || !(0. ..=0.5).contains(&c.command_delay_s)
            || c.voltage_v.is_some_and(|v| !v.is_finite() || v <= 0.)
            || c.temperature_c
                .is_some_and(|v| !v.is_finite() || v <= -273.15)
        {
            return Err("Invalid conditions or step size (supported step 10 µs–2 ms)".into());
        }
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

pub fn simulate(
    trial: &Trial,
    settings: &ModelSettings,
    cancel: &AtomicBool,
) -> Result<Trace, String> {
    settings.validate()?;
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evaluation {
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
    if limits.as_ref().is_some_and(|l| {
        !l.rmse.is_finite()
            || l.rmse < 0.
            || !l.final_abs_error.is_finite()
            || l.final_abs_error < 0.
    }) {
        return Err("Evaluation limits must be finite and nonnegative".into());
    }
    if ids.is_empty() {
        return Err("Select at least one trial".into());
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
}
impl Study {
    pub fn new(archive: Archive) -> Result<Self, String> {
        let (baseline, source) = baseline()?;
        Ok(Self {
            version: 1,
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
        })
    }
    pub fn candidate_edited(&mut self) {
        if self.validation_seen {
            self.validation_influenced = true;
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("Unsupported comparison file version".into());
        }
        self.baseline.validate()?;
        self.draft.validate()?;
        self.refinement.validate()?;
        self.refinement.validate_archive(&self.archive)?;
        if self.limits.as_ref().is_some_and(|l| {
            !l.rmse.is_finite()
                || l.rmse < 0.
                || !l.final_abs_error.is_finite()
                || l.final_abs_error < 0.
        }) {
            return Err("Invalid draft evaluation limits".into());
        }
        if self
            .view
            .evaluation
            .is_some_and(|i| i >= self.evaluations.len())
            || self
                .view
                .trial_id
                .as_ref()
                .is_some_and(|id| !self.archive.trials.iter().any(|t| &t.id == id))
            || !self.view.min_drive.is_finite()
            || !self.view.max_drive.is_finite()
        {
            return Err("Invalid saved review selection or filters".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for t in &self.archive.trials {
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
            e.runtime.validate()?;
            crate::system_session::SessionConfig {
                interval: e.baseline.step_s,
                integrator: e.integrator,
                seed: e.seed,
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
    /// Each saved review is a new immutable file; existing evidence is never overwritten.
    pub fn save_new(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        write_new(
            path,
            &serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let study: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        study.validate()?;
        Ok(study)
    }
}
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let temp = parent.join(format!(".experiment-{}-{nonce}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::hard_link(&temp, path).map_err(|e| format!("Save needs a new filename: {e}"))
    })();
    let _ = std::fs::remove_file(temp);
    result
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
            if let Some(c) = &r.candidate {
                if c.metrics.passes {
                    s.passes += 1;
                } else {
                    s.failures += 1;
                }
                if let Some(b) = &r.baseline {
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
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
impl Study {
    /// Standalone shareable report with all run settings, metrics and embedded SVG plots.
    pub fn export_html_new(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let mut html = String::from(
            "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Experiment comparison</title><style>body{font:15px system-ui;max-width:1100px;margin:40px auto;color:#223}table{border-collapse:collapse}td,th{padding:6px;border-bottom:1px solid #ddd;text-align:left}pre{white-space:pre-wrap}svg{width:100%;background:#f5f7fa}section{break-inside:avoid;margin:30px 0}.muted{color:#567}</style><h1>Measured response / model refinement</h1>",
        );
        html += &format!(
            "<h2>{}</h2><p>{}</p><p>{}</p><p>{}</p><p>Observation hash: {}<br>Empirical model hash: {}</p>",
            escape(&self.archive.label),
            escape(ASSUMPTIONS),
            escape(&self.archive.split_policy),
            escape(&self.notes),
            escape(&self.archive.observation_blake3),
            escape(&self.archive.model_blake3)
        );
        for issue in &self.archive.integrity_issues {
            html += &format!("<p>Source integrity issue: {}</p>", escape(issue));
        }
        html += &format!(
            "<h2>Controller design and refinement</h2><p>{} controller runs; {} fits; {} sensitivity reports; {} robustness evaluations.</p><details><summary>Captured refinement settings and evidence</summary><pre>{}</pre></details>",
            self.refinement.controller_runs.len(),
            self.refinement.fits.len()
                + self
                    .refinement
                    .recording_fits
                    .iter()
                    .filter(|f| f.attempt.outcome.is_some())
                    .count()
                + self
                    .refinement
                    .combined_fits
                    .iter()
                    .filter(|f| f.attempt.outcome.is_some())
                    .count(),
            self.refinement.sensitivities.len(),
            self.refinement.robustness.len(),
            escape(&serde_json::to_string_pretty(&self.refinement).map_err(|e| e.to_string())?)
        );
        for (i, run) in self.refinement.controller_runs.iter().enumerate() {
            if let Some(trace) = &run.electrical {
                html += &format!(
                    "<section><h3>Electrical simulation {}: {}</h3><p>Simulation only. Electrical limits: {:?}; motion tracking: {:?}. Failure: {}</p><p>Voltage {:.4}–{:.4} V; peak discharge {:.4} A; peak winding {:.4} A; peak draw {:.4} W. Drawn {:.6} Wh; returned {:.6} Wh.</p><p>{}</p>",
                    i + 1,
                    escape(&run.experiment.name),
                    trace.summary.passes,
                    run.score.as_ref().map(|s| s.passes),
                    escape(run.failure.as_deref().unwrap_or("none")),
                    trace.summary.minimum_voltage_v,
                    trace.summary.maximum_voltage_v,
                    trace.summary.peak_discharge_current_a,
                    trace.summary.peak_winding_current_a,
                    trace.summary.peak_draw_power_w,
                    trace.summary.drawn_energy_j / 3600.,
                    trace.summary.returned_energy_j / 3600.,
                    escape(&trace.interpretation)
                );
                for (label, get) in [
                    (
                        "Supply voltage (V)",
                        (|s: &crate::controller_refinement::power::Sample| s.supply_voltage_v)
                            as fn(&crate::controller_refinement::power::Sample) -> f64,
                    ),
                    ("Supply current (A)", |s| s.supply_current_a),
                    ("Winding current (A)", |s| s.winding_current_a),
                    ("Supply power (W)", |s| s.supply_power_w),
                ] {
                    html += &format!("<h4>{label}</h4>");
                    html += &svg_plot(&[(
                        "#2782b8",
                        trace.samples.iter().map(|s| (s.time_s, get(s))).collect(),
                    )]);
                }
                for v in &trace.summary.violations {
                    html += &format!("<p>{}</p>", escape(v));
                }
                html += "</section>";
            }
        }
        for e in &self.refinement.electrical_comparisons {
            html += &format!(
                "<section><h3>Measured electrical comparison</h3><p>{}</p>",
                escape(&e.method)
            );
            for c in &e.channels {
                html += &format!(
                    "<h4>{} ({})</h4><p>RMS {:.6}; peak error {:.6}; acceptance {:?}. Blue measured, orange predicted.</p>",
                    escape(&c.name),
                    escape(&c.measured.unit),
                    c.rmse,
                    c.maximum_abs_error,
                    c.passes
                );
                html += &svg_plot(&[
                    (
                        "#2782b8",
                        c.measured
                            .samples
                            .iter()
                            .map(|s| (s.time_s, s.value))
                            .collect(),
                    ),
                    (
                        "#c86e1e",
                        c.predicted
                            .samples
                            .iter()
                            .map(|s| (s.time_s, s.value))
                            .collect(),
                    ),
                ]);
            }
            if let Some(e) = &e.supply_energy {
                html += &format!(
                    "<p>Energy from {:.4} to {:.4} s: drawn measured {:.6} / predicted {:.6} J; returned measured {:.6} / predicted {:.6} J.</p>",
                    e.start_s,
                    e.end_s,
                    e.measured_drawn_j,
                    e.predicted_drawn_j,
                    e.measured_returned_j,
                    e.predicted_returned_j
                );
            }
            html += "</section>";
        }
        for (kind, i, attempt) in self
            .refinement
            .recording_fits
            .iter()
            .enumerate()
            .map(|(i, r)| ("Captured-command", i, &r.attempt))
            .chain(
                self.refinement
                    .combined_fits
                    .iter()
                    .enumerate()
                    .map(|(i, r)| ("Combined evidence", i, &r.attempt)),
            )
        {
            html += &format!(
                "<h3>{kind} fit {}</h3><p>{} objective evaluations; full input history frozen; calibration uses recorded PWM, not independent controller feedback.</p>",
                i + 1,
                attempt.evaluations.len()
            );
            if let Some(error) = &attempt.failure {
                html += &format!("<p>Retained failure: {}</p>", escape(error));
            }
            if let Some(fit) = &attempt.outcome {
                html += &format!(
                    "<p>{} Validation influenced selection: {}.</p><table><tr><th>Trial / motor / role</th><th>Baseline RMS (rad)</th><th>Candidate RMS (rad)</th><th>Candidate outcome</th></tr>",
                    escape(&fit.status),
                    fit.validation_influenced
                );
                for score in &fit.scores {
                    html += &format!(
                        "<tr><td>{} / ID {} / {}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                        escape(&score.id),
                        score.device,
                        escape(&score.split),
                        score
                            .baseline
                            .as_ref()
                            .map(|s| format!("{:.6}", s.rmse))
                            .unwrap_or("Unscored".into()),
                        score
                            .candidate
                            .as_ref()
                            .map(|s| format!("{:.6}", s.rmse))
                            .unwrap_or("Unscored".into()),
                        score
                            .candidate
                            .as_ref()
                            .map(|s| if s.passes { "PASS" } else { "FAIL" })
                            .unwrap_or("UNSCORED")
                    );
                }
                html += "</table>";
            }
        }
        for run in &self.refinement.fpga_design_runs {
            html += &format!("<section><h2>FPGA controller design: {}</h2><p>SIMULATION ONLY. Tracking on a captured observation schedule; not model prediction accuracy against new hardware measurements. Cancelled: {}.</p>",escape(&run.experiment.plan.name),run.cancelled);
            for (id,error) in &run.failures {html += &format!("<p>ID {} unscored: {}</p>",id,escape(error));}
            for axis in &run.axes {html += &format!("<p>ID {}: simulated tracking RMS {:.4}°, peak {:.4}°, saturation {:.1}% — {}</p>",axis.id,axis.tracking.rms_counts*360./4096.,axis.tracking.peak_counts*360./4096.,axis.tracking.saturated_fraction*100.,if axis.tracking.passes {"TRACKING PASS"}else{"TRACKING FAIL"});}
            html += "</section>";
        }
        for r in &self.refinement.fpga_recordings {
            html += &format!("<section><h2>FPGA experiment: {}</h2><p>Frozen role: {}; {} motors; {} frames; requested period {:.0} ms. Completed: {}; stop verified: {}.</p>", escape(&r.plan.name),escape(&r.plan.role),r.plan.ids.len(),r.frames.len(),r.plan.period_s*1000.,r.completed,r.stop_verified);
            if let Err(error)=r.validate() {html += &format!("<p>Unscored acquisition: {}</p>",escape(&error));}
            else {
                let scores=r.scores();
                html += "<h3>Measured controller tracking</h3><table><tr><th>Motor</th><th>RMS degrees</th><th>Peak degrees</th><th>Tracking gate</th></tr>";
                for id in &r.plan.ids {
                    let score=&scores["motors"][id.to_string()];
                    html += &format!("<tr><td>{}</td><td>{:.4}</td><td>{:.4}</td><td>{}</td></tr>",id,score["rms_degrees"].as_f64().unwrap(),score["peak_degrees"].as_f64().unwrap(),if score["tracking_pass"]==true {"PASS"}else{"FAIL"});
                }
                html += "</table><p>Tracking compares internal encoder position against the previous applied target. Unloaded bench only. Raw current is not calibrated amps; measured power and shared battery behavior remain unvalidated.</p>";
            }
            for review in self.refinement.fpga_reviews.iter().filter(|p|p.recording_hash==r.fingerprint()) {
                html += &format!("<h3>{}</h3><p>Cancelled: {}. Model prediction scores are separate from controller tracking scores.</p>",review.mode.label(),review.cancelled);
                for (id,error) in &review.failures {html += &format!("<p>ID {}: unscored — {}</p>",id,escape(error));}
                for p in &review.predictions {
                    html += &format!("<h4>ID {}: model RMS {:.4}°, peak {:.4}° — {}</h4><p>Frozen RMS limit {:.4}°. {}</p>",p.id,p.rms_prediction_degrees,p.peak_prediction_counts*360./4096.,if p.prediction_pass {"PASS"}else{"FAIL"},p.prediction_limit_counts*360./4096.,escape(&p.assumptions));
                    let home=r.home[(p.id-4) as usize] as f64;
                    let measured=r.frames.iter().map(|f| {let o=f.observations.iter().find(|o|o.id==p.id).unwrap();((o.request_s+o.completion_s)*0.5,(o.telemetry.position_raw as f64-home)*360./4096.)}).collect();
                    let predicted=p.samples_time_encoder_duty_angle.iter().map(|v|(v[0],(v[1]-home)*360./4096.)).collect();
                    html += &svg_plot(&[("Measured position (degrees)",measured),("Predicted position (degrees)",predicted)]);
                }
            }
            html += "</section>";
        }
        for r in &self.refinement.recordings {
            let hash = r.fingerprint();
            html += &format!(
                "<section><h2>ID {}: {}</h2><p>{} frames; completed: {}; stop verified: {}.</p><p>{}</p><p>{}</p>",
                r.experiment.device,
                escape(&r.experiment.name),
                r.frames.len(),
                r.completed,
                r.stop_verified,
                escape(&r.experiment.fixture),
                escape(&r.timing_evidence)
            );
            if let Some(failure) = &r.failure {
                html += &format!("<p>Retained failure: {}</p>", escape(failure));
            }
            for p in self
                .refinement
                .predictions
                .iter()
                .filter(|p| p.recording_hash == hash)
            {
                let purpose = match p.purpose {
                    crate::controller_refinement::recording::Purpose::RecordedCommandReplay => {
                        "Recorded PWM response"
                    }
                    crate::controller_refinement::recording::Purpose::ClosedLoopPrediction => {
                        "Independent closed-loop prediction"
                    }
                };
                html += &format!(
                    "<h3>{}: {}</h3><p>Model RMS {:.6} rad; peak {:.6} rad; final absolute error {:.6} rad. Frozen RMS / final limits: {:.6} / {:.6} rad.</p><p>Model revision: {}</p>",
                    purpose,
                    if p.model_error.passes { "PASS" } else { "FAIL" },
                    p.model_error.rmse,
                    p.model_error.maximum_abs_error,
                    p.model_error.final_error.abs(),
                    p.limits.rmse,
                    p.limits.final_abs_error,
                    escape(&p.model.fingerprint())
                );
                html += "<p>Position (rad) over time (s): blue measured; green predicted.</p>";
                html += &svg_plot(&[
                    (
                        "#167da5",
                        p.measured
                            .samples
                            .iter()
                            .map(|s| (s.time_s, s.value))
                            .collect(),
                    ),
                    (
                        "#178252",
                        p.predicted
                            .samples
                            .iter()
                            .map(|s| (s.time_s, s.value))
                            .collect(),
                    ),
                ]);
                for (label, score) in [
                    ("Measured controller", &p.measured_tracking),
                    ("Simulated controller", &p.simulated_tracking),
                ] {
                    if let Some(score) = score {
                        html += &format!(
                            "<p>{label}: tracking RMS {:.6} rad; peak {:.6} rad; final-tail error {:.6} rad; saturated {:.1}%. Tracking limits {}. This score is separate from model-prediction error.</p>",
                            score.rms_rad,
                            score.peak_rad,
                            score.settled_error_rad,
                            score.saturation_fraction * 100.,
                            if score.passes { "pass" } else { "fail" }
                        );
                    }
                }
                html += &format!(
                    "<details><summary>Transient timing and assumptions</summary><p>{}</p>",
                    escape(&p.assumptions)
                );
                for (label, trace) in [("Measured", &p.measured), ("Predicted", &p.predicted)] {
                    for response in
                        crate::controller_refinement::transients::trajectory(&r.experiment, trace)
                    {
                        match response {
                            Ok(response) => {
                                html += &format!(
                                    "<p>{label}: {:.3} → {:.3} rad; sampled overshoot {:.6} rad.</p><ul>",
                                    response.from_rad,
                                    response.target_rad,
                                    response.sampled_overshoot_rad
                                );
                                for (name, estimate) in [
                                    ("Onset", response.onset_delay),
                                    ("10–90% rise", response.rise_10_to_90),
                                    ("Settling after hold", response.settling_after_hold),
                                ] {
                                    let value = estimate
                                        .interval_s
                                        .map(|[lo, hi]| format!("{lo:.5}–{hi:.5} s"))
                                        .unwrap_or_else(|| {
                                            format!(
                                                "Unresolved: {}",
                                                estimate.unresolved.unwrap_or_default()
                                            )
                                        });
                                    html += &format!("<li>{name}: {}</li>", escape(&value));
                                }
                                html += "</ul>";
                            }
                            Err(error) => html += &format!("<p>{label}: {}</p>", escape(&error)),
                        }
                    }
                }
                html += "</details>";
            }
            html += "</section>";
        }
        if self.evaluations.is_empty() {
            html += "<p>No manual pulse-model evaluations recorded.</p>";
        }
        html += "<details><summary>Original retained reference metrics and plots (all trials)</summary>";
        for t in &self.archive.trials {
            html += &format!(
                "<section><h3>{}</h3><p>{} · RMSE {:.6} rad · max |error| {:.6} rad · final |error| {:.6} rad · {}. Original limits {:.6} / {:.6} rad.</p>",
                escape(&t.id),
                escape(&t.split),
                t.comparison.rmse,
                t.comparison.maximum_abs_error,
                t.comparison.final_error.abs(),
                if t.comparison.passes { "PASS" } else { "FAIL" },
                t.limits.rmse,
                t.limits.final_abs_error
            );
            if let Some(release) = &t.release {
                let origin = (t.on_host_window_s[0] + t.on_host_window_s[1]) * 0.5;
                let report = crate::controller_refinement::transients::release(
                    &t.measured,
                    [
                        t.off_host_window_s[0] - origin,
                        t.off_host_window_s[1] - origin,
                    ],
                    crate::experiment_comparison::hx_archive::ENCODER_QUANTUM_RAD,
                    0.15,
                )?;
                html += &format!(
                    "<p>Captured condition: {}. Declared driver hypothesis: {}.</p><details><summary>Release timing at encoder resolution</summary><pre>{}</pre></details>",
                    escape(&t.kind),
                    escape(&serde_json::to_string(release).map_err(|e| e.to_string())?),
                    escape(&serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?)
                );
            }
            let series = vec![
                (
                    "#167da5",
                    t.measured
                        .samples
                        .iter()
                        .map(|s| (s.time_s, s.value))
                        .collect(),
                ),
                (
                    "#8a6b31",
                    t.predicted
                        .samples
                        .iter()
                        .map(|s| (s.time_s, s.value))
                        .collect(),
                ),
            ];
            html += "<p>Displacement (rad), time (s): blue measured; brown retained reference.</p>";
            html += &svg_plot(&series);
            html += "</section>";
        }
        html += "</details>";
        for e in &self.evaluations {
            html+=&format!("<h2>Evaluation {} · {}</h2><p>{}</p><p>Cancelled: {}. Validation influenced selection: {}. Runtime: {}</p><details><summary>Captured settings and changes</summary><pre>{}</pre></details>",escape(&e.id),escape(&e.decision),escape(&e.notes),e.cancelled,e.validation_influenced,escape(&e.runtime.library_source_blake3),escape(&serde_json::to_string_pretty(&serde_json::json!({"baseline":e.baseline,"candidate":e.candidate,"evaluation_limits_override":e.limits})).unwrap()));
            html += &format!("<p>{}</p>", escape(&e.assumptions));
            html += "<table><tr><th>Trial / split</th><th>Baseline RMSE (rad)</th><th>Candidate RMSE (rad)</th><th>Max |error| (rad)</th><th>Final |error| (rad)</th><th>Outcome</th></tr>";
            for r in &e.results {
                let t = self
                    .archive
                    .trials
                    .iter()
                    .find(|t| t.id == r.trial_id)
                    .unwrap();
                let value = |p: &Option<Prediction>, which: u8| {
                    p.as_ref()
                        .map(|p| {
                            format!(
                                "{:.6}",
                                match which {
                                    0 => p.metrics.rmse,
                                    1 => p.metrics.maximum_abs_error,
                                    _ => p.metrics.final_error.abs(),
                                }
                            )
                        })
                        .unwrap_or("Unscored".into())
                };
                html += &format!(
                    "<tr><td>{}<br>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    escape(&r.trial_id),
                    escape(&t.split),
                    value(&r.baseline, 0),
                    value(&r.candidate, 0),
                    value(&r.candidate, 1),
                    value(&r.candidate, 2),
                    r.candidate
                        .as_ref()
                        .map(|p| if p.metrics.passes { "PASS" } else { "FAIL" })
                        .unwrap_or("UNSCORED")
                );
            }
            html += "</table>";
            for r in &e.results {
                let t = self
                    .archive
                    .trials
                    .iter()
                    .find(|t| t.id == r.trial_id)
                    .unwrap();
                html += &format!(
                    "<section><h3>{}</h3><p>{} · {:.1}% duty · {:.4} s pulse · {:?} V · {:?} °C</p><p>Limits: RMSE {:.6} rad; final |error| {:.6} rad. {:?}</p>",
                    escape(&t.id),
                    escape(&t.split),
                    t.drive * 100.,
                    t.duration_s,
                    t.voltage_range_v,
                    t.temperature_range_c,
                    e.limits.as_ref().unwrap_or(&t.limits).rmse,
                    e.limits.as_ref().unwrap_or(&t.limits).final_abs_error,
                    r.errors.iter().map(|s| escape(s)).collect::<Vec<_>>()
                );
                for residual in [false, true] {
                    let mut series = vec![(
                        "#167da5",
                        t.measured
                            .samples
                            .iter()
                            .map(|s| (s.time_s, if residual { 0. } else { s.value }))
                            .collect::<Vec<_>>(),
                    )];
                    for (color, p) in [
                        ("#8a6b31", Some(&t.predicted)),
                        ("#785cc5", r.baseline.as_ref().map(|p| &p.trace)),
                        ("#178252", r.candidate.as_ref().map(|p| &p.trace)),
                    ] {
                        if let Some(p) = p {
                            series.push((
                                color,
                                p.samples
                                    .iter()
                                    .zip(&t.measured.samples)
                                    .map(|(s, m)| {
                                        (
                                            s.time_s,
                                            if residual { s.value - m.value } else { s.value },
                                        )
                                    })
                                    .collect(),
                            ));
                        }
                    }
                    html += &format!(
                        "<p>{} (rad) · x: time since command midpoint (s). Blue: measured; brown: empirical; purple: baseline; green: candidate.</p>",
                        if residual { "Residual" } else { "Displacement" }
                    );
                    html += &svg_plot(&series);
                }
                html += "</section>";
            }
        }
        html += "<details><summary>Baseline source and measurement interpretation</summary><pre>";
        html += &escape(&serde_json::to_string_pretty(&self.baseline_source).unwrap());
        html += "</pre><p>";
        html += &escape(&self.archive.interpretation);
        html += "</p></details></html>";
        write_new(path, html.as_bytes())
    }
}
fn svg_plot(series: &[(&str, Vec<(f64, f64)>)]) -> String {
    let max_t = series
        .iter()
        .flat_map(|(_, s)| s)
        .map(|(t, _)| *t)
        .fold(0., f64::max)
        .max(0.001);
    let low = series
        .iter()
        .flat_map(|(_, s)| s)
        .map(|(_, v)| *v)
        .fold(0., f64::min);
    let high = series
        .iter()
        .flat_map(|(_, s)| s)
        .map(|(_, v)| *v)
        .fold(0., f64::max);
    let span = (high - low).max(0.001);
    let mut svg = format!(
        "<svg viewBox=\"0 0 1000 220\" role=\"img\" aria-label=\"Response comparison\"><text x=\"5\" y=\"15\">{high:.5}</text><text x=\"5\" y=\"190\">{low:.5}</text><text x=\"70\" y=\"214\">0 s</text><text x=\"900\" y=\"214\">{max_t:.3} s</text>"
    );
    for (color, s) in series {
        let points = s
            .iter()
            .map(|(t, v)| {
                format!(
                    "{:.2},{:.2}",
                    70. + t / max_t * 900.,
                    190. - (v - low) / span * 170.
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        svg += &format!(
            "<polyline points=\"{points}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2\"/>"
        );
    }
    svg += "</svg>";
    svg
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
