//! Local sensitivity, bounded physical-model fitting, and per-device deviations.
//! Tuning and held-out whole trials are always separate. Convergence is not validation.
use super::{
    calibration_data::{CalibrationData, Case},
    control,
};
use crate::{
    experiment_comparison::{Comparison, Trace, compare},
    experiment_study::ModelSettings,
    physics_context::RuntimeIdentity,
};
use nalgebra::{DMatrix, DVector};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicBool, Ordering},
};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Family {
    pub shared: ModelSettings,
    pub device_deltas: BTreeMap<u8, BTreeMap<String, f64>>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coordinate {
    pub path: String,
    pub device: Option<u8>,
    pub lower: f64,
    pub upper: f64,
}
fn registry_default(kind: &str, name: &str) -> Option<f64> {
    crate::registry()
        .get(&kind.into())
        .ok()?
        .parameters
        .as_ref()?
        .iter()
        .find(|p| p.name == name)?
        .default
}
fn get(s: &ModelSettings, path: &str) -> Result<f64, String> {
    let (group, name) = path
        .split_once('.')
        .ok_or("Parameter path must name a group")?;
    match group {
        "motor" => s
            .motor
            .get(if name == "constant" {
                "torque_constant"
            } else {
                name
            })
            .copied()
            .or_else(|| {
                registry_default(
                    "robot.motor_unit",
                    if name == "constant" {
                        "torque_constant"
                    } else {
                        name
                    },
                )
            })
            .ok_or("Unknown motor parameter".into()),
        "bridge" => s
            .bridge
            .get(name)
            .copied()
            .or_else(|| registry_default("robot.h_bridge", name))
            .ok_or("Unknown driver parameter".into()),
        "condition" => match name {
            "load_inertia" => Ok(s.conditions.load_inertia),
            "load_torque" => Ok(s.conditions.load_torque),
            "delay" => Ok(s.conditions.command_delay_s),
            _ => Err("Unknown condition parameter".into()),
        },
        _ => Err("Unknown parameter group".into()),
    }
}
fn set(s: &mut ModelSettings, path: &str, value: f64) -> Result<(), String> {
    get(s, path)?;
    let (group, name) = path.split_once('.').unwrap();
    match group {
        "motor" if name == "constant" => {
            s.motor.insert("torque_constant".into(), value);
            s.motor.insert("back_emf_constant".into(), value);
        }
        "motor" => {
            if matches!(name, "torque_constant" | "back_emf_constant") {
                return Err("Use motor.constant to preserve reciprocal SI motor constants".into());
            }
            s.motor.insert(name.into(), value);
        }
        "bridge" => {
            s.bridge.insert(name.into(), value);
        }
        "condition" => match name {
            "load_inertia" => s.conditions.load_inertia = value,
            "load_torque" => s.conditions.load_torque = value,
            "delay" => s.conditions.command_delay_s = value,
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
    Ok(())
}
impl Family {
    pub fn model(&self, device: u8) -> Result<ModelSettings, String> {
        let mut s = self.shared.clone();
        if let Some(d) = self.device_deltas.get(&device) {
            for (path, delta) in d {
                let value = get(&s, path)? + delta;
                set(&mut s, path, value)?;
            }
        }
        s.validate()?;
        Ok(s)
    }
    fn values(&self, coords: &[Coordinate]) -> Result<Vec<f64>, String> {
        coords
            .iter()
            .map(|c| match c.device {
                None => get(&self.shared, &c.path),
                Some(d) => {
                    get(&self.shared, &c.path)?;
                    Ok(self
                        .device_deltas
                        .get(&d)
                        .and_then(|m| m.get(&c.path))
                        .copied()
                        .unwrap_or(0.))
                }
            })
            .collect()
    }
    fn with_values(&self, coords: &[Coordinate], values: &[f64]) -> Result<Self, String> {
        let mut next = self.clone();
        for (c, &v) in coords.iter().zip(values) {
            if let Some(d) = c.device {
                next.device_deltas
                    .entry(d)
                    .or_default()
                    .insert(c.path.clone(), v);
            } else {
                set(&mut next.shared, &c.path, v)?;
            }
        }
        next.shared.validate()?;
        for &d in next.device_deltas.keys() {
            next.model(d)?;
        }
        Ok(next)
    }
}
fn validate_coordinates(f: &Family, coords: &[Coordinate]) -> Result<Vec<f64>, String> {
    if coords.is_empty() || coords.len() > 16 {
        return Err("Select 1–16 bounded parameters".into());
    }
    let mut seen = BTreeSet::new();
    let values = f.values(coords)?;
    for (c, &v) in coords.iter().zip(&values) {
        if !seen.insert((c.device, c.path.as_str()))
            || !c.lower.is_finite()
            || !c.upper.is_finite()
            || c.lower >= c.upper
            || v < c.lower
            || v > c.upper
        {
            return Err("Parameters need distinct coordinates and finite bounds containing the initial value".into());
        }
        let mut check = f.shared.clone();
        set(&mut check, &c.path, get(&f.shared, &c.path)?)?;
    }
    Ok(values)
}
fn selected(a: &impl CalibrationData, ids: &[String], training: bool) -> Result<Vec<Case>, String> {
    let cases = a.cases()?;
    if cases.iter().map(|c| &c.id).collect::<BTreeSet<_>>().len() != cases.len()
        || cases
            .iter()
            .any(|c| c.id.is_empty() || !c.resolution.is_finite() || c.resolution <= 0.)
    {
        return Err("Calibration dataset requires unique trial IDs and finite positive measurement resolution".into());
    }
    let mut seen = BTreeSet::new();
    let selected = ids
        .iter()
        .map(|id| {
            if !seen.insert(id) {
                return Err("Duplicate trial selection".into());
            }
            let t = cases.iter().find(|t| &t.id == id).ok_or("Unknown trial")?;
            if training && t.split != "train" {
                return Err("Held-out trials cannot enter the fitting objective".into());
            }
            Ok(t.clone())
        })
        .collect::<Result<Vec<_>, String>>()?;
    if selected.is_empty() {
        return Err("Select at least one whole trial".into());
    }
    Ok(selected)
}
fn residuals(
    a: &impl CalibrationData,
    f: &Family,
    trials: &[Case],
    cancel: &AtomicBool,
) -> Result<Vec<f64>, String> {
    let mut r = vec![];
    for t in trials {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let p = a.predict(&t.id, &f.model(t.device)?, cancel)?;
        compare(&t.measured, &p, &t.limits)?;
        if !t.resolution.is_finite() || t.resolution <= 0. {
            return Err("Dataset resolution must be finite and positive".into());
        }
        let scale = t.resolution * (t.measured.samples.len() as f64).sqrt();
        r.extend(
            p.samples
                .iter()
                .zip(&t.measured.samples)
                .map(|(p, m)| (p.value - m.value) / scale),
        );
    }
    Ok(r)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sensitivity {
    pub source_model: Family,
    pub trial_ids: Vec<String>,
    pub coordinates: Vec<Coordinate>,
    pub column_norms: Vec<f64>,
    pub similar_parameters: Vec<(usize, usize, f64)>,
    pub singular_values: Vec<f64>,
    pub rank: usize,
    pub warnings: Vec<String>,
    pub runtime: RuntimeIdentity,
}
pub fn sensitivity(
    a: &impl CalibrationData,
    f: &Family,
    ids: &[String],
    coords: &[Coordinate],
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Sensitivity, String> {
    let values = validate_coordinates(f, coords)?;
    let trials = selected(a, ids, false)?;
    let reference = residuals(a, f, &trials, cancel)?;
    let mut columns = vec![];
    let mut warnings=vec!["Local finite-difference response sensitivity, not a parameter confidence interval or proof of unique identification.".into()];
    for (i, c) in coords.iter().enumerate() {
        let width = c.upper - c.lower;
        let h = width * 0.001;
        let mut left = values.clone();
        let mut right = values.clone();
        left[i] = (values[i] - h).max(c.lower);
        right[i] = (values[i] + h).min(c.upper);
        let l = residuals(a, &f.with_values(coords, &left)?, &trials, cancel)?;
        let r = residuals(a, &f.with_values(coords, &right)?, &trials, cancel)?;
        if l.len() != reference.len() || r.len() != reference.len() {
            return Err("Sensitivity sample identity changed".into());
        }
        columns.push(DVector::from_iterator(
            l.len(),
            l.iter()
                .zip(r)
                .map(|(l, r)| (r - l) / (right[i] - left[i]) * width),
        ));
        progress(i + 1, coords.len());
    }
    let norms = columns.iter().map(|c| c.norm()).collect::<Vec<_>>();
    let mut similar = vec![];
    for i in 0..columns.len() {
        if norms[i] < 1e-6 {
            warnings.push(format!("{} has negligible response over these trials; try a different excitation or known load",coords[i].path));
        }
        for j in 0..i {
            let denominator = norms[i] * norms[j];
            if denominator > 1e-16 {
                let cosine = (columns[i].dot(&columns[j]) / denominator).clamp(-1., 1.);
                if cosine.abs() > 0.98 {
                    similar.push((j, i, cosine));
                }
            }
        }
    }
    let matrix = DMatrix::from_columns(&columns);
    let sv = matrix.svd(false, false).singular_values.as_slice().to_vec();
    let largest = sv.iter().copied().fold(0., f64::max);
    let rank = sv
        .iter()
        .filter(|&&s| s > largest * 1e-6 && s > 1e-10)
        .count();
    if rank < coords.len() || !similar.is_empty() {
        warnings.push("These parameters have overlapping effects. Use reversals, longer trajectories, known loads, or independently measured electrical quantities to distinguish them.".into());
    }
    Ok(Sensitivity {
        source_model: f.clone(),
        trial_ids: ids.to_vec(),
        coordinates: coords.to_vec(),
        column_norms: norms,
        similar_parameters: similar,
        singular_values: sv,
        rank,
        warnings,
        runtime: RuntimeIdentity::current(),
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrialScore {
    pub id: String,
    pub device: u8,
    pub split: String,
    pub baseline: Option<Comparison>,
    pub candidate: Option<Comparison>,
    pub failure: Option<String>,
    #[serde(default)]
    pub baseline_prediction: Option<Trace>,
    #[serde(default)]
    pub candidate_prediction: Option<Trace>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fit {
    pub baseline: Family,
    pub candidate: Family,
    pub coordinates: Vec<Coordinate>,
    pub training_ids: Vec<String>,
    pub validation_ids: Vec<String>,
    pub optimizer: serde_json::Value,
    pub scores: Vec<TrialScore>,
    pub validation_influenced: bool,
    pub runtime: RuntimeIdentity,
    pub status: String,
}
impl Fit {
    pub fn validate(&self, archive: &impl CalibrationData) -> Result<(), String> {
        self.runtime.validate()?;
        validate_coordinates(&self.baseline, &self.coordinates)?;
        let training = selected(archive, &self.training_ids, true)?;
        let held = selected(archive, &self.validation_ids, false)?;
        if held.iter().any(|t| t.split == "train")
            || self
                .training_ids
                .iter()
                .any(|id| self.validation_ids.contains(id))
        {
            return Err("Saved fit has an invalid validation split".into());
        }
        let values: Vec<f64> =
            serde_json::from_value(self.optimizer["values"].clone()).map_err(|e| e.to_string())?;
        if values.len() != self.coordinates.len()
            || values
                .iter()
                .zip(&self.coordinates)
                .any(|(v, c)| !v.is_finite() || *v < c.lower || *v > c.upper)
        {
            return Err("Saved fitted values violate declared bounds".into());
        }
        if self.baseline.with_values(&self.coordinates, &values)? != self.candidate {
            return Err(
                "Saved candidate does not match the optimizer's captured parameter values".into(),
            );
        }
        if self.scores.len() != training.len() + held.len() {
            return Err("Saved fit does not account for every selected trial".into());
        }
        let mut seen = BTreeSet::new();
        for s in &self.scores {
            let trial = training
                .iter()
                .chain(&held)
                .find(|t| t.id == s.id)
                .ok_or("Saved score references an unselected trial")?;
            if !seen.insert(&s.id) || trial.device != s.device || trial.split != s.split {
                return Err("Saved fit changes trial identity or split".into());
            }
            for (metrics, trace) in [
                (&s.baseline, &s.baseline_prediction),
                (&s.candidate, &s.candidate_prediction),
            ] {
                match (metrics, trace) {
                    (Some(metrics), Some(trace)) => {
                        if compare(&trial.measured, trace, &trial.limits)? != *metrics {
                            return Err(
                                "Saved fit metrics do not match its prediction trace".into()
                            );
                        }
                    }
                    (None, None) | (Some(_), None) => {}
                    _ => return Err("Saved fit prediction has no comparison metrics".into()),
                }
            }
        }
        Ok(())
    }
    /// Legacy summaries remain readable but must not be shown as trace-verified evidence.
    pub fn has_verified_traces(&self) -> bool {
        self.scores.iter().all(|s| {
            (s.baseline.is_none() || s.baseline_prediction.is_some())
                && (s.candidate.is_none() || s.candidate_prediction.is_some())
        })
    }
}
pub fn fit(
    a: &impl CalibrationData,
    f: &Family,
    training: &[String],
    validation: &[String],
    coords: &[Coordinate],
    max_evaluations: usize,
    validation_influenced: bool,
    cancel: &AtomicBool,
    progress: impl FnMut(usize, usize),
) -> Result<Fit, String> {
    fit_audited(
        a,
        f,
        training,
        validation,
        coords,
        max_evaluations,
        validation_influenced,
        cancel,
        progress,
        |_, _| {},
    )
}
fn fit_audited(
    a: &impl CalibrationData,
    f: &Family,
    training: &[String],
    validation: &[String],
    coords: &[Coordinate],
    max_evaluations: usize,
    validation_influenced: bool,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
    mut audit: impl FnMut(&[f64], &Result<Vec<f64>, String>),
) -> Result<Fit, String> {
    let initial = validate_coordinates(f, coords)?;
    let trials = selected(a, training, true)?;
    let held = selected(a, validation, false)?;
    if held.iter().any(|t| t.split == "train") || training.iter().any(|id| validation.contains(id))
    {
        return Err("Validation must use distinct original held-out trials".into());
    }
    if !(3..=1000).contains(&max_evaluations) {
        return Err("Fitting budget must be 3–1000 model evaluations".into());
    }
    let config = sim_solve::least_squares::LeastSquaresConfig {
        maximum_iterations: 20,
        maximum_evaluations: max_evaluations,
        difference_step: 0.001,
        initial_damping: 0.01,
        gradient_tolerance: 1e-6,
    };
    let bounds = coords
        .iter()
        .map(|c| sim_solve::least_squares::VariableBound {
            lower: c.lower,
            upper: c.upper,
        })
        .collect::<Vec<_>>();
    let mut n = 0;
    let result =
        sim_solve::least_squares::bounded_least_squares(&initial, &bounds, &config, |values| {
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled".into());
            }
            n += 1;
            progress(n, max_evaluations);
            let result = f
                .with_values(coords, values)
                .and_then(|f| residuals(a, &f, &trials, cancel));
            audit(values, &result);
            result
        })?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled; no fitted candidate adopted".into());
    }
    let candidate = f.with_values(coords, &result.values)?;
    let mut scores = vec![];
    for t in trials.iter().chain(&held) {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled during held-out evaluation".into());
        }
        let score = |family: &Family| {
            a.predict(&t.id, &family.model(t.device)?, cancel)
                .and_then(|p| compare(&t.measured, &p, &t.limits).map(|metrics| (metrics, p)))
        };
        let baseline = score(f);
        let fitted = score(&candidate);
        let failure = baseline.as_ref().err().or(fitted.as_ref().err()).cloned();
        scores.push(TrialScore {
            id: t.id.clone(),
            device: t.device,
            split: t.split.clone(),
            baseline: baseline.as_ref().ok().map(|(score, _)| score.clone()),
            candidate: fitted.as_ref().ok().map(|(score, _)| score.clone()),
            baseline_prediction: baseline.ok().map(|(_, trace)| trace),
            candidate_prediction: fitted.ok().map(|(_, trace)| trace),
            failure,
        });
    }
    Ok(Fit {
        baseline: f.clone(),
        candidate,
        coordinates: coords.to_vec(),
        training_ids: training.to_vec(),
        validation_ids: validation.to_vec(),
        optimizer: serde_json::to_value(&result).map_err(|e| e.to_string())?,
        scores,
        validation_influenced,
        runtime: RuntimeIdentity::current(),
        status:
            "Candidate only; convergence does not imply identified constants or accepted validation"
                .into(),
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FitRequest {
    pub model: Family,
    pub training_ids: Vec<String>,
    pub validation_ids: Vec<String>,
    pub coordinates: Vec<Coordinate>,
    pub maximum_evaluations: usize,
    pub validation_influenced: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelEvaluation {
    pub values: Vec<f64>,
    pub residual_sum_squares: Option<f64>,
    pub failure: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FitAttempt {
    pub request: FitRequest,
    pub archive_hash: String,
    pub runtime: RuntimeIdentity,
    pub evaluations: Vec<ModelEvaluation>,
    pub outcome: Option<Fit>,
    pub failure: Option<String>,
    pub cancelled: bool,
}
/// Every attempt retains its frozen inputs and every objective evaluation, including failures.
/// Neither a failed fit nor a cancelled fit replaces a model revision.
pub fn attempt(
    a: &impl CalibrationData,
    request: &FitRequest,
    cancel: &AtomicBool,
    progress: impl FnMut(usize, usize),
) -> FitAttempt {
    let mut evaluations = vec![];
    let result = fit_audited(
        a,
        &request.model,
        &request.training_ids,
        &request.validation_ids,
        &request.coordinates,
        request.maximum_evaluations,
        request.validation_influenced,
        cancel,
        progress,
        |values, result| {
            evaluations.push(ModelEvaluation {
                values: values.to_vec(),
                residual_sum_squares: result.as_ref().ok().map(|r| r.iter().map(|v| v * v).sum()),
                failure: result.as_ref().err().cloned(),
            });
        },
    );
    FitAttempt {
        request: request.clone(),
        archive_hash: a.fingerprint(),
        runtime: RuntimeIdentity::current(),
        evaluations,
        failure: result.as_ref().err().cloned(),
        outcome: result.ok(),
        cancelled: cancel.load(Ordering::Relaxed),
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Variant {
    pub label: String,
    pub model: ModelSettings,
    pub timing: control::Timing,
    pub evidence: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Robustness {
    #[serde(default)]
    pub variants: Vec<Variant>,
    #[serde(default)]
    pub cancelled: bool,
    pub experiment: control::Experiment,
    pub runs: Vec<(String, String, control::Run)>,
    pub prediction_envelope: Vec<[f64; 3]>,
    pub failures: Vec<String>,
    pub interpretation: String,
}
fn scenario_envelope(runs: &[(String, String, control::Run)]) -> Vec<[f64; 3]> {
    let mut envelope = vec![];
    if let Some((_, _, first)) = runs.first() {
        for p in &first.truth {
            let values = runs
                .iter()
                .filter_map(|(_, _, r)| {
                    r.truth
                        .iter()
                        .find(|v| (v[0] - p[0]).abs() < 1e-10)
                        .map(|v| v[1])
                })
                .collect::<Vec<_>>();
            if values.len() == runs.len() {
                envelope.push([
                    p[0],
                    values.iter().copied().fold(f64::INFINITY, f64::min),
                    values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                ]);
            }
        }
    }
    envelope
}
impl Robustness {
    pub fn validate(&self) -> Result<(), String> {
        self.experiment.validate()?;
        if self.interpretation.trim().is_empty() || self.variants.len() > 32 {
            return Err("Invalid robustness provenance".into());
        }
        let mut labels = BTreeSet::new();
        for (label, evidence, run) in &self.runs {
            if label.trim().is_empty() || evidence.trim().is_empty() || !labels.insert(label) {
                return Err("Scenario results require unique labels and provenance".into());
            }
            run.validate()?;
            let mut expected = self.experiment.clone();
            expected.timing = run.experiment.timing.clone();
            if expected != run.experiment {
                return Err("Robustness changed controller or trajectory between scenarios".into());
            }
            if !self.variants.is_empty() {
                let v = self
                    .variants
                    .iter()
                    .find(|v| &v.label == label)
                    .ok_or("Scenario result has no captured input")?;
                if v.model != run.model
                    || v.timing != run.experiment.timing
                    || &v.evidence != evidence
                {
                    return Err("Scenario result differs from its captured inputs".into());
                }
            }
        }
        if self
            .variants
            .iter()
            .map(|v| &v.label)
            .collect::<BTreeSet<_>>()
            .len()
            != self.variants.len()
        {
            return Err("Duplicate scenario inputs".into());
        }
        // Empty input snapshots are legacy reports: retain their runs, but never add missing evidence.
        if !self.variants.is_empty()
            && self.failures.is_empty()
            && !self.cancelled
            && self.runs.len() != self.variants.len()
        {
            return Err("Robustness silently omitted a scenario".into());
        }
        let complete = !self.cancelled
            && self.failures.is_empty()
            && self
                .runs
                .iter()
                .all(|(_, _, r)| r.failure.is_none() && !r.cancelled);
        let expected = if complete {
            scenario_envelope(&self.runs)
        } else {
            vec![]
        };
        if expected != self.prediction_envelope {
            return Err("Scenario envelope does not match complete retained runs".into());
        }
        Ok(())
    }
}
pub fn robustness(
    e: &control::Experiment,
    variants: &[Variant],
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Robustness, String> {
    if variants.is_empty()
        || variants.len() > 32
        || variants
            .iter()
            .any(|v| v.label.is_empty() || v.evidence.is_empty())
    {
        return Err(
            "Provide 1–32 named variants with evidence or explicit hypothesis provenance".into(),
        );
    }
    if variants
        .iter()
        .map(|v| &v.label)
        .collect::<BTreeSet<_>>()
        .len()
        != variants.len()
    {
        return Err("Scenario names must be unique".into());
    }
    let mut runs = vec![];
    let mut failures = vec![];
    for (i, v) in variants.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            failures.push(format!("{}: cancelled before scenario", v.label));
            continue;
        }
        let mut spec = e.clone();
        spec.timing = v.timing.clone();
        match control::simulate(&spec, &v.model, cancel, |_, _| {}) {
            Ok(r) => runs.push((v.label.clone(), v.evidence.clone(), r)),
            Err(err) => failures.push(format!("{}: {err}", v.label)),
        };
        progress(i + 1, variants.len());
    }
    // Only complete scenarios contribute, at explicit common times without resampling.
    let envelope = if failures.is_empty()
        && runs.len() == variants.len()
        && runs
            .iter()
            .all(|(_, _, r)| r.failure.is_none() && !r.cancelled)
    {
        scenario_envelope(&runs)
    } else {
        vec![]
    };
    Ok(Robustness{variants:variants.to_vec(),cancelled:cancel.load(Ordering::Relaxed),experiment:e.clone(),runs,prediction_envelope:envelope,failures,interpretation:"Scenario envelope, not a calibrated confidence band. Controller tracking scores are separate from measured model-prediction error. Different schedules contribute only at shared output times.".into()})
}

impl FitAttempt {
    pub fn validate(&self, data: &impl CalibrationData) -> Result<(), String> {
        self.runtime.validate()?;
        if self.archive_hash != data.fingerprint() {
            return Err("Fit attempt refers to a different measurement archive".into());
        }
        if let Some(fit) = &self.outcome {
            fit.validate(data)?;
            let r = &self.request;
            if fit.baseline != r.model
                || fit.coordinates != r.coordinates
                || fit.training_ids != r.training_ids
                || fit.validation_ids != r.validation_ids
                || fit.validation_influenced != r.validation_influenced
            {
                return Err("Fit outcome differs from its frozen request".into());
            }
        }
        for evaluation in &self.evaluations {
            if evaluation.values.len() != self.request.coordinates.len()
                || evaluation.values.iter().any(|v| !v.is_finite())
                || evaluation
                    .residual_sum_squares
                    .is_some_and(|v| !v.is_finite() || v < 0.)
                || evaluation.failure.is_some() == evaluation.residual_sum_squares.is_some()
            {
                return Err("Invalid retained objective evaluation".into());
            }
        }
        if (self.cancelled || self.failure.is_some()) && self.outcome.is_some() {
            return Err("Failed/cancelled attempt cannot contain an adopted fitted result".into());
        }
        Ok(())
    }
}
