//! Coverage is evidence about specific conditions, never a global accuracy percentage.
use crate::experiment_comparison::hx_archive::{Archive, ENCODER_QUANTUM_RAD};
use crate::experiment_study::Evaluation;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoverageRow {
    pub trial_id: String,
    pub device: u8,
    pub role: String,
    pub duty: f64,
    pub duration_s: f64,
    pub displacement_range_rad: [f64; 2],
    pub voltage_range_v: [f64; 2],
    pub temperature_range_c: [f64; 2],
    pub largest_observation_window_s: f64,
    pub largest_sample_gap_s: f64,
    pub onset_window_s: Option<[f64; 2]>,
    pub model_rmse_rad: Option<f64>,
    pub model_peak_error_rad: Option<f64>,
    pub passes: Option<bool>,
    pub kind: String,
    pub release: Option<super::transients::ReleaseResponse>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Repeatability {
    pub device: u8,
    pub duty: f64,
    pub duration_s: f64,
    pub trials: Vec<String>,
    pub endpoint_range_rad: [f64; 2],
    pub interpretation: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Coverage {
    pub rows: Vec<CoverageRow>,
    pub repeatability: Vec<Repeatability>,
    pub measurement_resolution_rad: f64,
    pub limitations: Vec<String>,
}
fn range(values: impl Iterator<Item = f64>) -> [f64; 2] {
    values.fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], v| {
        [lo.min(v), hi.max(v)]
    })
}
pub fn coverage(archive: &Archive, evaluation: Option<&Evaluation>) -> Coverage {
    let mut rows = vec![];
    let mut repetitions = BTreeMap::<(u8, u64, u64, String), Vec<_>>::new();
    for t in &archive.trials {
        let samples = &t.measured.samples;
        let prediction = evaluation
            .and_then(|e| e.results.iter().find(|r| r.trial_id == t.id))
            .and_then(|r| r.candidate.as_ref());
        let onset = samples
            .windows(2)
            .find(|w| {
                w[0].value.abs() <= 3. * ENCODER_QUANTUM_RAD
                    && w[1].value.abs() > 3. * ENCODER_QUANTUM_RAD
            })
            .map(|w| [w[0].request_s, w[1].completion_s]);
        rows.push(CoverageRow {
            kind: t.kind.clone(),
            release: t.release.as_ref().and_then(|_| {
                let origin = (t.on_host_window_s[0] + t.on_host_window_s[1]) * 0.5;
                super::transients::release(
                    &t.measured,
                    [
                        t.off_host_window_s[0] - origin,
                        t.off_host_window_s[1] - origin,
                    ],
                    ENCODER_QUANTUM_RAD,
                    0.15,
                )
                .ok()
            }),
            trial_id: t.id.clone(),
            device: t.device,
            role: t.split.clone(),
            duty: t.drive,
            duration_s: t.duration_s,
            displacement_range_rad: range(samples.iter().map(|s| s.value)),
            voltage_range_v: t.voltage_range_v,
            temperature_range_c: t.temperature_range_c,
            largest_observation_window_s: samples
                .iter()
                .map(|s| s.completion_s - s.request_s)
                .fold(0., f64::max),
            largest_sample_gap_s: samples
                .windows(2)
                .map(|w| w[1].time_s - w[0].time_s)
                .fold(0., f64::max),
            onset_window_s: onset,
            model_rmse_rad: prediction.map(|p| p.metrics.rmse),
            model_peak_error_rad: prediction.map(|p| p.metrics.maximum_abs_error),
            passes: prediction.map(|p| p.metrics.passes),
        });
        repetitions
            .entry((
                t.device,
                t.drive.to_bits(),
                t.duration_s.to_bits(),
                t.kind.clone(),
            ))
            .or_default()
            .push(t);
    }
    let repeatability = repetitions.into_values().filter(|ts| ts.len() > 1).map(|ts| Repeatability {
        device: ts[0].device, duty: ts[0].drive, duration_s: ts[0].duration_s,
        trials: ts.iter().map(|t| t.id.clone()).collect(),
        endpoint_range_rad: range(ts.iter().filter_map(|t| t.measured.samples.last().map(|s| s.value))),
        interpretation: "Observed endpoint spread for matching device, duty, duration and mode; supply, temperature, starting angle and end-sample time may differ. Not isolated measurement noise or a confidence interval.".into(),
    }).collect();
    Coverage { rows, repeatability, measurement_resolution_rad: ENCODER_QUANTUM_RAD, limitations: vec![
        "Coverage describes recorded trials, not the interior of a fitted operating envelope. Untested combinations are extrapolations.".into(),
        "Encoder resolution is not calibrated angular accuracy. Transaction windows are host timing; unknown sensor age remains additional uncertainty.".into(),
        "Onset brackets cross three encoder counts and include sampling/transaction timing. Unresolved onset is not zero delay.".into(),
        "Only unloaded PWM response is covered here. Loaded torque, joint backlash/compliance, warm/cold dependence and assembled-robot accuracy remain unvalidated.".into(),
        "Zero-duty electrical braking is an explicit model assumption. Torque-off trials use their separately declared passive driver hypothesis; Hiwonder electronics are not calibrated by that choice.".into(),
    ] }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ControllerCoverage {
    pub recording_hash: String,
    pub device: u8,
    pub name: String,
    pub completed: bool,
    pub fixture: String,
    pub initial_encoder_rad: f64,
    pub displacement_range_rad: Option<[f64; 2]>,
    pub sampled_velocity_range_rad_s: Option<[f64; 2]>,
    pub duty_range: Option<[f64; 2]>,
    pub voltage_range_v: Option<[f64; 2]>,
    pub temperature_range_c: Option<[f64; 2]>,
    pub maximum_tick_gap_s: Option<f64>,
    pub prediction_results: Vec<(super::recording::Purpose, f64, bool)>,
    pub draft_extrapolation_reasons: Vec<String>,
    pub uncertainty: String,
}
/// Report each observed combination separately. Marginal ranges are never a validated envelope.
pub fn controller_coverage(
    w: &super::workspace::Workspace,
    model: &crate::experiment_study::ModelSettings,
) -> Vec<ControllerCoverage> {
    let draft = &w.experiment;
    w.recordings.iter().map(|r|{
        let hash=r.fingerprint();let e=&r.experiment;let frames=&r.frames;
        let known_range=|values:Vec<f64>| if values.is_empty(){None}else{Some(range(values.into_iter()))};
        let mut reasons=vec![];
        for (diff,reason) in [
            (e.device!=draft.device,"Different motor identity"),
            (e.component_id!=draft.component_id,"Different CAD component association"),
            (e.fixture!=draft.fixture,"Different fixture declaration"),
            (e.controller!=draft.controller,"Different controller or parameters"),
            (e.trajectory!=draft.trajectory || e.duration_s!=draft.duration_s,"Untested trajectory or duration"),
            (e.timing!=draft.timing,"Different observation/command timing assumptions"),
            ((e.initial_encoder_rad-draft.initial_encoder_rad).abs()>e.timing.encoder_quantum_rad,"Different starting encoder angle"),
        ] {if diff {reasons.push(reason.into());}}
        let voltage=known_range(frames.iter().map(|f|f.voltage_v).collect());
        let temperature=known_range(frames.iter().map(|f|f.temperature_c).collect());
        if !voltage.is_some_and(|[lo,hi]|lo<=draft.voltage_v&&draft.voltage_v<=hi){reasons.push("Supply outside this recording's observed range".into());}
        if !temperature.is_some_and(|[lo,hi]|lo<=draft.temperature_c&&draft.temperature_c<=hi){reasons.push("Temperature outside this recording's observed range".into());}
        let predictions=w.predictions.iter().filter(|p|p.recording_hash==hash&&&p.model==model).map(|p|(p.purpose,p.model_error.rmse,p.model_error.passes)).collect::<Vec<_>>();
        if predictions.is_empty(){reasons.push("Current model revision has no predictions for this recording".into());}
        if !r.completed{reasons.push("Incomplete measurement remains unscored".into());}
        ControllerCoverage {recording_hash:hash,device:e.device,name:e.name.clone(),completed:r.completed,fixture:e.fixture.clone(),initial_encoder_rad:e.initial_encoder_rad,
            displacement_range_rad:known_range(frames.iter().map(|f|f.control.estimated_position_rad).collect()),
            sampled_velocity_range_rad_s:known_range(frames.windows(2).filter_map(|f|{let dt=f[1].control.observation.observed_s-f[0].control.observation.observed_s;(dt>0.).then(||(f[1].control.estimated_position_rad-f[0].control.estimated_position_rad)/dt)}).collect()),
            duty_range:known_range(frames.iter().map(|f|f.drive_counts as f64/1000.).collect()),voltage_range_v:voltage,temperature_range_c:temperature,
            maximum_tick_gap_s:frames.windows(2).map(|f|f[1].control.time_s-f[0].control.time_s).reduce(f64::max),prediction_results:predictions,draft_extrapolation_reasons:reasons,
            uncertainty:format!("Encoder resolution {:.6} rad; angular accuracy and device sample age uncalibrated. Velocity range is a difference of quantized samples, not resolved instantaneous speed. Repeatability is between trials; model scenarios are hypotheses, not confidence bounds. Matching marginal conditions does not validate their unmeasured combinations. Load and assembly coupling require separate setup evidence.",e.timing.encoder_quantum_rad),
        }
    }).collect()
}
