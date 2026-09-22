//! Portable FPGA experiment review, shared by native UI and headless clients.
use super::{calibration as cal, calibration_data::CalibrationData, fpga, power};
use crate::{experiment_study::ModelSettings, physics_context::RuntimeIdentity};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[serde(rename = "recorded_pwm_replay")]
    Replay,
    #[serde(rename = "own_feedback")]
    ClosedLoop,
    #[serde(rename = "recorded_pwm_measured_voltage")]
    ReplayMeasuredVoltage,
    #[serde(rename = "own_feedback_measured_voltage")]
    ClosedLoopMeasuredVoltage,
}
impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Replay => "Recorded PWM replay",
            Self::ClosedLoop => "Own-feedback closed loop",
            Self::ReplayMeasuredVoltage => "Recorded PWM + measured voltage",
            Self::ClosedLoopMeasuredVoltage => "Own feedback + measured voltage",
        }
    }
    pub fn closed_loop(self) -> bool {
        matches!(self, Self::ClosedLoop | Self::ClosedLoopMeasuredVoltage)
    }
    pub fn measured_voltage(self) -> bool {
        matches!(self, Self::ReplayMeasuredVoltage | Self::ClosedLoopMeasuredVoltage)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prediction {
    pub mode: Mode,
    pub id: u8,
    pub model: ModelSettings,
    pub recording_blake3: String,
    pub controller_ir_blake3: String,
    pub runtime: RuntimeIdentity,
    pub assumptions: String,
    pub voltage_v: f64,
    pub temperature_c: f64,
    pub rms_prediction_counts: f64,
    pub rms_prediction_degrees: f64,
    pub peak_prediction_counts: f64,
    pub prediction_limit_counts: f64,
    pub prediction_pass: bool,
    pub samples_time_encoder_duty_angle: Vec<[f64; 4]>,
    pub electrical: Vec<power::Sample>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_input: Option<super::fpga_voltage::Conditioning>,
}
impl Prediction {
    pub fn validate(&self, r: &fpga::Recording) -> Result<(), String> {
        r.validate()?;
        self.model.validate()?;
        self.runtime.validate()?;
        if self.recording_blake3 != r.fingerprint()
            || !r.plan.ids.contains(&self.id)
            || self.samples_time_encoder_duty_angle.len() != r.frames.len()
            || self.controller_ir_blake3.len() != 64
            || self.assumptions.is_empty()
            || !self.voltage_v.is_finite()
            || !self.temperature_c.is_finite()
            || self.prediction_limit_counts != 3.
        {
            return Err("Prediction identity, coverage or frozen limit changed".into());
        }
        let mut sum = 0.;
        let mut peak = 0f64;
        for (sample, frame) in self.samples_time_encoder_duty_angle.iter().zip(&r.frames) {
            let o = frame.observations.iter().find(|o| o.id == self.id).unwrap();
            if sample.iter().any(|x| !x.is_finite())
                || (sample[0] - (o.request_s + o.completion_s) * 0.5).abs() > 1e-7
                || !(0. ..=4095.).contains(&sample[1])
                || sample[1].fract() != 0.
                || sample[2].abs() > r.plan.gains.limit as f64
            {
                return Err("Invalid simulated sample or captured observation timing".into());
            }
            let e = sample[1] - o.telemetry.position_raw as f64;
            sum += e * e;
            peak = peak.max(e.abs());
        }
        let rms = (sum / r.frames.len() as f64).sqrt();
        for (actual, expected) in [
            (self.rms_prediction_counts, rms),
            (self.rms_prediction_degrees, rms * 360. / 4096.),
            (self.peak_prediction_counts, peak),
        ] {
            if !actual.is_finite() || (actual - expected).abs() > 1e-8 {
                return Err("Saved prediction score differs from samples".into());
            }
        }
        if self.prediction_pass != (rms <= self.prediction_limit_counts) {
            return Err("Saved prediction pass differs from frozen limit".into());
        }
        for sample in &self.electrical {
            sample.validate()?;
        }
        if self.mode.measured_voltage() != self.voltage_input.is_some() {
            return Err("Prediction voltage mode and conditioning evidence disagree".into());
        }
        if let Some(input) = &self.voltage_input { input.validate(r, self)?; }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Review {
    pub recording_hash: String,
    pub models: std::collections::BTreeMap<u8, ModelSettings>,
    pub mode: Mode,
    pub predictions: Vec<Prediction>,
    pub failures: Vec<(u8, String)>,
    pub cancelled: bool,
}
impl Review {
    pub fn validate(&self, r: &fpga::Recording) -> Result<(), String> {
        r.validate()?;
        for model in self.models.values() {
            model.validate()?;
        }
        if self.recording_hash != r.fingerprint() {
            return Err("Review refers to a different recording".into());
        }
        let mut ids = Vec::new();
        for p in &self.predictions {
            p.validate(r)?;
            if p.mode != self.mode || self.models.get(&p.id) != Some(&p.model) {
                return Err("Prediction differs from frozen review model or mode".into());
            }
            ids.push(p.id);
        }
        for (id, error) in &self.failures {
            if error.is_empty() {
                return Err("Missing per-motor failure explanation".into());
            }
            ids.push(*id);
        }
        ids.sort();
        if ids != r.plan.ids || self.models.keys().copied().collect::<Vec<_>>() != r.plan.ids {
            return Err("Review must account for every motor exactly once".into());
        }
        Ok(())
    }
}
pub fn review(
    r: &fpga::Recording,
    model: &ModelSettings,
    mode: Mode,
    cancel: &AtomicBool,
    progress: impl FnMut(usize, usize),
) -> Result<Review, String> {
    review_family(
        r,
        &cal::Family {
            shared: model.clone(),
            device_deltas: Default::default(),
        },
        mode,
        cancel,
        progress,
    )
}
pub fn review_family(
    r: &fpga::Recording,
    family: &cal::Family,
    mode: Mode,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Review, String> {
    r.validate()?;
    let models: std::collections::BTreeMap<_, _> = r
        .plan
        .ids
        .iter()
        .map(|id| Ok((*id, family.model(*id)?)))
        .collect::<Result<_, String>>()?;
    let mut result = Review {
        recording_hash: r.fingerprint(),
        models,
        mode,
        predictions: vec![],
        failures: vec![],
        cancelled: false,
    };
    for (index, &id) in r.plan.ids.iter().enumerate() {
        let p = if mode.measured_voltage() {
            super::fpga_voltage::predict(r, id, &result.models[&id], mode, cancel,
                |done, _| progress(index * 1000 + done, r.plan.ids.len() * 1000))
        } else { fpga::predict_cancellable(
            r,
            id,
            &result.models[&id],
            mode.closed_loop(),
            cancel,
            |done, _| progress(index * 1000 + done, r.plan.ids.len() * 1000),
        )
        .and_then(|v| serde_json::from_value::<Prediction>(v).map_err(|e| e.to_string())) }
        .and_then(|p| {
            p.validate(r)?;
            Ok(p)
        });
        match p {
            Ok(p) => {
                result.models.insert(id, p.model.clone());
                result.predictions.push(p);
            },
            Err(e) => result.failures.push((id, e)),
        }
    }
    result.cancelled = cancel.load(Ordering::Relaxed);
    progress(r.plan.ids.len() * 1000, r.plan.ids.len() * 1000);
    result.validate(r)?;
    Ok(result)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FitAttempt {
    pub dataset: fpga::Dataset,
    pub attempt: cal::FitAttempt,
}
pub fn fit(
    recordings: Vec<fpga::Recording>,
    model: cal::Family,
    coordinates: Vec<cal::Coordinate>,
    cancel: &AtomicBool,
    progress: impl FnMut(usize, usize),
) -> Result<FitAttempt, String> {
    fit_using_voltage(recordings, model, coordinates, false, cancel, progress)
}
pub fn fit_measured_voltage(
    recordings: Vec<fpga::Recording>,
    model: cal::Family,
    coordinates: Vec<cal::Coordinate>,
    cancel: &AtomicBool,
    progress: impl FnMut(usize, usize),
) -> Result<FitAttempt, String> {
    fit_using_voltage(recordings, model, coordinates, true, cancel, progress)
}
fn fit_using_voltage(
    recordings: Vec<fpga::Recording>,
    model: cal::Family,
    coordinates: Vec<cal::Coordinate>,
    measured_voltage: bool,
    cancel: &AtomicBool,
    progress: impl FnMut(usize, usize),
) -> Result<FitAttempt, String> {
    let dataset = fpga::Dataset { recordings, measured_voltage };
    let cases = dataset.cases()?;
    let request = cal::FitRequest {
        model,
        coordinates,
        training_ids: cases
            .iter()
            .filter(|c| c.split == "train")
            .map(|c| c.id.clone())
            .collect(),
        validation_ids: cases
            .iter()
            .filter(|c| c.split != "train")
            .map(|c| c.id.clone())
            .collect(),
        maximum_evaluations: 40,
        validation_influenced: true,
    };
    let attempt = cal::attempt(&dataset, &request, cancel, progress);
    attempt.validate(&dataset)?;
    Ok(FitAttempt { dataset, attempt })
}
