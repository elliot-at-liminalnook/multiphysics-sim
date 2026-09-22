//! Observable shaft-speed response, shared by offline reviews and the viewer.
//! These are finite-difference estimates, never instantaneous motor limits.
use super::{fpga, fpga_review, transients::TimingEstimate};
use crate::experiment_comparison::{Limits, Observation, Trace, compare};
use serde::{Deserialize, Serialize};

const SCOPE: &str = "Secant speed from encoder positions; acceleration compares separated secants. Both measured and simulated encoders use the same estimator and timing windows. Bounds cover declared encoder quantization and transaction/sample-age windows, not encoder calibration error or unobserved motion. Crossings and settling describe the sampled estimates; they are not guaranteed instantaneous mechanical times or maximum motor capability. No time shift or automatic model promotion.";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Estimator {
    pub encoder_quantum_rad: f64,
    pub lag_samples: usize,
    pub maximum_sample_gap_s: f64,
    /// Explicit continuity gate; wrapped encoders must be unwrapped at acquisition.
    pub maximum_sample_step_rad: f64,
    /// None leaves an additional, unknown timing uncertainty. Zero asserts no age.
    pub maximum_sample_age_s: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Estimate {
    pub time_s: f64,
    pub support_s: [f64; 2],
    pub value: f64,
    pub bounds: [f64; 2],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Motion {
    pub speed_rad_s: Vec<Estimate>,
    pub acceleration_rad_s2: Vec<Estimate>,
    pub maximum_sample_gap_s: f64,
    pub maximum_estimator_window_s: f64,
    pub sample_age_bounded: bool,
    pub interpretation: String,
}

fn finite_positive(x: f64) -> bool {
    x.is_finite() && x > 0.
}
fn quotient(n: [f64; 2], d: [f64; 2]) -> Result<[f64; 2], String> {
    if !finite_positive(d[0]) || !finite_positive(d[1]) || d[0] > d[1] {
        return Err("Timing uncertainty overlaps successive samples; increase estimator spacing or improve timestamps".into());
    }
    let v = [n[0] / d[0], n[0] / d[1], n[1] / d[0], n[1] / d[1]];
    if v.iter().any(|x| !x.is_finite()) {
        return Err("Nonfinite derivative bounds".into());
    }
    Ok([
        v.into_iter().fold(f64::INFINITY, f64::min),
        v.into_iter().fold(f64::NEG_INFINITY, f64::max),
    ])
}
pub fn estimate(trace: &Trace, e: &Estimator) -> Result<Motion, String> {
    compare(
        trace,
        trace,
        &Limits {
            rmse: 0.,
            final_abs_error: 0.,
        },
    )?;
    if trace.unit != "rad"
        || trace.quantity != sim_core::QuantityKind::Angle.definition_id()
        || !finite_positive(e.encoder_quantum_rad)
        || !finite_positive(e.maximum_sample_gap_s)
        || !finite_positive(e.maximum_sample_step_rad)
        || e.lag_samples == 0
        || e.lag_samples >= trace.samples.len()
        || e.maximum_sample_age_s
            .is_some_and(|a| !a.is_finite() || a < 0.)
    {
        return Err("Require an angular trace, positive estimator limits, valid lag and explicit sample-age assumption".into());
    }
    let mut gap = 0_f64;
    for w in trace.samples.windows(2) {
        let dt = w[1].time_s - w[0].time_s;
        gap = gap.max(dt);
        if dt > e.maximum_sample_gap_s
            || (w[1].value - w[0].value).abs() > e.maximum_sample_step_rad
        {
            return Err("Sampling gap or angular continuity limit exceeded; do not bridge missing samples or encoder wrap".into());
        }
    }
    let age = e.maximum_sample_age_s.unwrap_or(0.);
    let mut speed = Vec::new();
    for w in trace.samples.windows(e.lag_samples + 1) {
        let a = &w[0];
        let b = &w[e.lag_samples];
        let d = b.value - a.value;
        let bounds = quotient(
            [d - e.encoder_quantum_rad, d + e.encoder_quantum_rad],
            [
                b.request_s - age - a.completion_s,
                b.completion_s - a.request_s + age,
            ],
        )?;
        let value = d / (b.time_s - a.time_s);
        let support_s = [a.request_s - age, b.completion_s];
        if !value.is_finite() || support_s.iter().any(|x| !x.is_finite()) {
            return Err("Nonfinite speed estimate".into());
        }
        speed.push(Estimate {
            time_s: a.time_s + (b.time_s - a.time_s) * 0.5,
            support_s,
            value,
            bounds,
        });
    }
    // Separate the secants: adjacent ones share an encoder sample and their
    // temporal supports overlap. Do not present a falsely precise derivative.
    let mut acceleration = Vec::new();
    for (a, b) in speed.iter().zip(speed.iter().skip(e.lag_samples + 1)) {
        let dt = [
            b.support_s[0] - a.support_s[1],
            b.support_s[1] - a.support_s[0],
        ];
        let bounds = quotient([b.bounds[0] - a.bounds[1], b.bounds[1] - a.bounds[0]], dt)?;
        let value = (b.value - a.value) / (b.time_s - a.time_s);
        if !value.is_finite() {
            return Err("Nonfinite acceleration estimate".into());
        }
        acceleration.push(Estimate {
            time_s: a.time_s + (b.time_s - a.time_s) * 0.5,
            support_s: [a.support_s[0], b.support_s[1]],
            value,
            bounds,
        });
    }
    Ok(Motion {
        maximum_estimator_window_s: speed
            .iter()
            .map(|s| s.support_s[1] - s.support_s[0])
            .fold(0., f64::max),
        speed_rad_s: speed,
        acceleration_rad_s2: acceleration,
        maximum_sample_gap_s: gap,
        sample_age_bounded: e.maximum_sample_age_s.is_some(),
        interpretation: SCOPE.into(),
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub command_window_s: [f64; 2],
    pub observed_until_s: f64,
    pub from_rad_s: f64,
    pub target_rad_s: f64,
    pub tolerance_rad_s: f64,
    /// Required for both the pre-command baseline and settled post-command tail.
    pub dwell_s: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub baseline_established: bool,
    pub onset_delay: TimingEstimate,
    pub rise_10_to_90: TimingEstimate,
    pub command_to_settled_speed: TimingEstimate,
    pub command_to_zero_crossing: TimingEstimate,
    pub zero_to_settled_opposite_speed: TimingEstimate,
    /// Relative to the last encoder observation wholly before the command.
    pub sampled_forward_travel_before_reversal_rad: Option<f64>,
    pub travel_reference_window_s: Option<[f64; 2]>,
    pub sampled_peak_abs_speed_rad_s: f64,
    pub sampled_overshoot_rad_s: f64,
    pub sampled_peak_abs_acceleration_rad_s2: Option<f64>,
    pub interpretation: String,
}
fn missing(reason: &str) -> TimingEstimate {
    TimingEstimate {
        interval_s: None,
        unresolved: Some(reason.into()),
    }
}
fn interval(v: [f64; 2]) -> TimingEstimate {
    TimingEstimate {
        interval_s: Some(v),
        unresolved: None,
    }
}
fn elapsed(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [(a[0] - b[1]).max(0.), (a[1] - b[0]).max(0.)]
}
fn in_band(s: &Estimate, target: f64, tol: f64) -> bool {
    s.bounds[0] >= target - tol && s.bounds[1] <= target + tol
}
fn dwell(samples: &[&Estimate], duration: f64, gap: f64) -> bool {
    samples.len() >= 3
        && samples.last().unwrap().support_s[0] - samples[0].support_s[1] >= duration
        && samples.windows(2).all(|w| w[1].time_s - w[0].time_s <= gap)
}
pub fn response(
    trace: &Trace,
    e: &Estimator,
    t: &Transition,
) -> Result<(Motion, Response), String> {
    let motion = estimate(trace, e)?;
    let [start, command_end] = t.command_window_s;
    if [
        start,
        command_end,
        t.observed_until_s,
        t.from_rad_s,
        t.target_rad_s,
        t.tolerance_rad_s,
        t.dwell_s,
    ]
    .iter()
    .any(|v| !v.is_finite())
        || start > command_end
        || t.observed_until_s <= command_end
        || !finite_positive(t.tolerance_rad_s)
        || !finite_positive(t.dwell_s)
        || (t.target_rad_s - t.from_rad_s).abs() <= 2. * t.tolerance_rad_s
    {
        return Err("Require ordered command/observation times, distinct speeds, positive tolerance and dwell".into());
    }
    let samples: Vec<_> = motion
        .speed_rad_s
        .iter()
        .filter(|s| s.support_s[1] <= t.observed_until_s)
        .collect();
    let before: Vec<_> = samples
        .iter()
        .copied()
        .filter(|s| s.support_s[1] <= start)
        .collect();
    let tail_start = before
        .iter()
        .rposition(|s| !in_band(s, t.from_rad_s, t.tolerance_rad_s))
        .map_or(0, |i| i + 1);
    let baseline = dwell(&before[tail_start..], t.dwell_s, e.maximum_sample_gap_s)
        && before
            .last()
            .is_some_and(|s| start - s.support_s[1] <= e.maximum_sample_gap_s);
    let post: Vec<_> = samples
        .iter()
        .copied()
        .filter(|s| s.support_s[0] >= command_end)
        .collect();
    if post.is_empty() {
        return Err(
            "No speed estimates wholly after the command and inside the observation window".into(),
        );
    }
    let delta = t.target_rad_s - t.from_rad_s;
    // A crossing needs definitely-below and definitely-above estimates. Keep
    // intervening uncertainty instead of interpolating it to an exact time.
    let crossing = |fraction: f64| -> Option<[f64; 2]> {
        let mut below = None;
        for s in &samples {
            let v = [
                (s.bounds[0] - t.from_rad_s) / delta,
                (s.bounds[1] - t.from_rad_s) / delta,
            ];
            let lo = v[0].min(v[1]);
            let hi = v[0].max(v[1]);
            if hi < fraction {
                below = Some(s.support_s[0]);
            }
            if lo >= fraction && s.support_s[1] >= command_end {
                return below.map(|a| [a.max(start), s.support_s[1]]);
            }
        }
        None
    };
    let start_tail = post
        .iter()
        .rposition(|s| !in_band(s, t.target_rad_s, t.tolerance_rad_s))
        .map_or(0, |i| i + 1);
    let settled_tail = &post[start_tail..];
    let settled = if baseline
        && dwell(settled_tail, t.dwell_s, e.maximum_sample_gap_s)
        && t.observed_until_s - post.last().unwrap().support_s[1] <= e.maximum_sample_gap_s
    {
        Some([
            if start_tail == 0 {
                command_end
            } else {
                post[start_tail - 1].support_s[0].max(command_end)
            },
            settled_tail[0].support_s[1],
        ])
    } else {
        None
    };
    let unresolved = if !baseline {
        "Previous steady speed is not established for the required dwell"
    } else {
        "Crossing is not bracketed by resolved speed estimates"
    };
    let onset = if baseline {
        crossing(t.tolerance_rad_s / delta.abs())
    } else {
        None
    };
    let rise = if baseline {
        crossing(0.1).zip(crossing(0.9)).map(|(a, b)| elapsed(b, a))
    } else {
        None
    };
    let reversal = t.from_rad_s * t.target_rad_s < 0.;
    let zero = if reversal && baseline {
        crossing(-t.from_rad_s / delta)
    } else {
        None
    };
    let reference = trace.samples.iter().rev().find(|s| s.completion_s <= start);
    let travel = zero.zip(reference).map(|(w, a)| {
        trace
            .samples
            .iter()
            .filter(|s| s.time_s >= a.time_s && s.request_s <= w[1])
            .map(|s| t.from_rad_s.signum() * (s.value - a.value))
            .fold(0., f64::max)
    });
    let peak_accel = motion
        .acceleration_rad_s2
        .iter()
        .filter(|s| s.support_s[0] >= command_end && s.support_s[1] <= t.observed_until_s)
        .map(|s| s.value.abs())
        .reduce(f64::max);
    let result=Response{baseline_established:baseline,
        onset_delay:onset.map(|v|interval(elapsed(v,t.command_window_s))).unwrap_or_else(||missing(unresolved)),
        rise_10_to_90:rise.map(interval).unwrap_or_else(||missing(unresolved)),
        command_to_settled_speed:settled.map(|v|interval(elapsed(v,t.command_window_s))).unwrap_or_else(||missing("Initial speed or sustained final plateau is unresolved; check tolerance, quantization, dwell and record length")),
        command_to_zero_crossing:zero.map(|v|interval(elapsed(v,t.command_window_s))).unwrap_or_else(||missing(if reversal{unresolved}else{"Not an opposite-direction transition"})),
        zero_to_settled_opposite_speed:zero.zip(settled).map(|(a,b)|interval(elapsed(b,a))).unwrap_or_else(||missing("Both a resolved zero crossing and opposite-speed plateau are required")),
        sampled_forward_travel_before_reversal_rad:travel,travel_reference_window_s:reference.map(|s|[s.request_s,s.completion_s]),
        sampled_peak_abs_speed_rad_s:post.iter().map(|s|s.value.abs()).fold(0.,f64::max),
        sampled_overshoot_rad_s:post.iter().map(|s|delta.signum()*(s.value-t.target_rad_s)).fold(0.,f64::max),
        sampled_peak_abs_acceleration_rad_s2:peak_accel,
        interpretation:format!("{SCOPE} Onset is departure from the initial-speed tolerance band. Travel is the largest sampled forward excursion from the last pre-command encoder sample through the zero-crossing bracket; sub-sample extrema and exact command-time angle are unknown. Unknown sample age adds unbounded uncertainty when sample_age_bounded=false.")};
    Ok((motion, result))
}

/// Convert captured/quantized encoder outputs, including identical host windows.
pub fn fpga_trace(
    r: &fpga::Recording,
    id: u8,
    p: Option<&fpga_review::Prediction>,
) -> Result<Trace, String> {
    r.validate()?;
    if !r.plan.ids.contains(&id) {
        return Err("Motor is not in this recording".into());
    }
    if let Some(p) = p {
        p.validate(r)?;
        if p.id != id {
            return Err("Prediction belongs to a different motor".into());
        }
    }
    Ok(Trace {
        quantity: sim_core::QuantityKind::Angle.definition_id(),
        unit: "rad".into(),
        samples: r
            .frames
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let o = f.observations.iter().find(|o| o.id == id).unwrap();
                let count = p.map_or(o.telemetry.position_raw as f64, |p| {
                    p.samples_time_encoder_duty_angle[i][1]
                });
                Observation {
                    time_s: (o.request_s + o.completion_s) * 0.5,
                    value: count * std::f64::consts::TAU / 4096.,
                    request_s: o.request_s,
                    completion_s: o.completion_s,
                }
            })
            .collect(),
    })
}

/// Whole-group motion diagnostics without inventing steady-speed transitions.
/// All axes retain their real sample windows, including serial polling offsets.
pub fn group_motion(r: &fpga::Recording, e: &Estimator) -> Result<serde_json::Value, String> {
    r.validate()?;
    if (e.encoder_quantum_rad - std::f64::consts::TAU / 4096.).abs() > 1e-12 {
        return Err("Use the recorded HX encoder resolution".into());
    }
    let mut axes = std::collections::BTreeMap::new();
    for &id in &r.plan.ids {
        axes.insert(id, estimate(&fpga_trace(r, id, None)?, e)?);
    }
    Ok(
        serde_json::json!({"recording_blake3":r.fingerprint(),"estimator":e,
        "controller_ids":r.plan.ids,
        "planned_moving_ids":r.plan.ids.iter().filter(|id|r.plan.targets.iter().any(|row|row[(**id-4) as usize]!=0)).collect::<Vec<_>>(),
        "scores":r.scores(),"axes":axes,
        "interpretation":"Measured group response with serial per-axis windows. Peak secant speed is not a rated or continuous maximum. Group correlations alone do not identify a shared supply model; current remains uncalibrated raw telemetry."}),
    )
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trial {
    pub motor_id: u8,
    /// The actual captured command transaction, not a planned host timestamp.
    pub command_tick: usize,
    pub last_observation_tick: usize,
    pub from_rad_s: f64,
    pub target_rad_s: f64,
    pub tolerance_rad_s: f64,
    pub dwell_s: f64,
    pub travel_limited: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub estimator: Estimator,
    /// Fixture/load provenance; the motor's own inertia is not zero when unloaded.
    pub setup_evidence: String,
    pub trials: Vec<Trial>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conditions {
    pub motor_id: u8,
    pub simultaneously_controlled_ids: Vec<u8>,
    pub voltage_range_v: [f64; 2],
    pub temperature_range_c: [f64; 2],
    pub current_range_raw: [f64; 2],
    pub written_pwm_range_per_mille: [f64; 2],
    pub duty_limit_per_mille: u16,
    pub data_role: String,
    pub timing_evidence: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comparison {
    pub measured: Response,
    pub predicted: Option<Response>,
    /// Signed predicted-minus-measured interval. None means unscored.
    pub settling_error_s: Option<[f64; 2]>,
    pub rise_error_s: Option<[f64; 2]>,
    pub zero_crossing_error_s: Option<[f64; 2]>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrialReview {
    pub conditions: Conditions,
    pub transition: Transition,
    pub comparison: Comparison,
    pub measured_motion: Motion,
    pub predicted_motion: Option<Motion>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Review {
    pub version: u32,
    pub recording_blake3: String,
    pub request_blake3: String,
    pub prediction_review_blake3: Option<String>,
    pub request: Request,
    pub trials: Vec<TrialReview>,
    pub interpretation: String,
}
fn censor(r: &mut Response) {
    let m = missing(
        "Trial was limited by available travel; no steady-speed capability or complete reversal timing is established",
    );
    r.onset_delay = m.clone();
    r.rise_10_to_90 = m.clone();
    r.command_to_settled_speed = m.clone();
    r.command_to_zero_crossing = m.clone();
    r.zero_to_settled_opposite_speed = m;
}
fn error(p: Option<&TimingEstimate>, m: &TimingEstimate) -> Option<[f64; 2]> {
    p.and_then(|p| p.interval_s)
        .zip(m.interval_s)
        .map(|(p, m)| [p[0] - m[1], p[1] - m[0]])
}
pub fn review(
    r: &fpga::Recording,
    request: &Request,
    prediction: Option<&fpga_review::Review>,
) -> Result<Review, String> {
    r.validate()?;
    if request.setup_evidence.trim().is_empty()
        || request.trials.is_empty()
        || request.trials.len() > 128
        || (request.estimator.encoder_quantum_rad - std::f64::consts::TAU / 4096.).abs() > 1e-12
    {
        return Err("Require setup evidence, 1–128 declared transitions and the captured HX encoder resolution".into());
    }
    if let Some(p) = prediction {
        p.validate(r)?;
    }
    let mut trials = Vec::new();
    for trial in &request.trials {
        let first = r
            .frames
            .get(trial.command_tick)
            .ok_or("Command tick outside recording")?;
        let last = r
            .frames
            .get(trial.last_observation_tick)
            .ok_or("End tick outside recording")?;
        if trial.last_observation_tick <= trial.command_tick {
            return Err("End tick must follow command tick".into());
        }
        let trace = fpga_trace(r, trial.motor_id, None)?;
        let transition = Transition {
            command_window_s: [first.command_request_s, first.command_receipt_s],
            observed_until_s: last
                .observations
                .iter()
                .find(|o| o.id == trial.motor_id)
                .unwrap()
                .completion_s,
            from_rad_s: trial.from_rad_s,
            target_rad_s: trial.target_rad_s,
            tolerance_rad_s: trial.tolerance_rad_s,
            dwell_s: trial.dwell_s,
        };
        let (motion, mut measured) = response(&trace, &request.estimator, &transition)?;
        let mut predicted = None;
        let mut predicted_motion = None;
        if let Some(p) = prediction {
            let axis = p
                .predictions
                .iter()
                .find(|p| p.id == trial.motor_id)
                .ok_or("Requested motor has no prediction; do not silently omit it")?;
            let (m, res) = response(
                &fpga_trace(r, trial.motor_id, Some(axis))?,
                &request.estimator,
                &transition,
            )?;
            predicted = Some(res);
            predicted_motion = Some(m);
        }
        if trial.travel_limited {
            censor(&mut measured);
            if let Some(p) = &mut predicted {
                censor(p);
            }
        }
        let range = |values: Vec<f64>| {
            [
                values.iter().copied().fold(f64::INFINITY, f64::min),
                values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            ]
        };
        // Include the baseline as well as the response; do not hide pre-command conditions.
        let observations: Vec<_> = r.frames[..=trial.last_observation_tick]
            .iter()
            .map(|f| {
                f.observations
                    .iter()
                    .find(|o| o.id == trial.motor_id)
                    .unwrap()
            })
            .collect();
        let conditions=Conditions{motor_id:trial.motor_id,simultaneously_controlled_ids:r.plan.ids.clone(),
            voltage_range_v:range(observations.iter().map(|o|o.telemetry.voltage_v).collect()),
            temperature_range_c:range(observations.iter().map(|o|o.telemetry.temperature_c as f64).collect()),
            current_range_raw:range(observations.iter().map(|o|o.telemetry.current_raw as f64).collect()),
            written_pwm_range_per_mille:range(r.frames[..=trial.last_observation_tick].iter().map(|f|f.pwm_readback[(trial.motor_id-4) as usize] as f64).collect()),
            duty_limit_per_mille:r.plan.gains.limit,data_role:r.plan.role.clone(),
            timing_evidence:"Captured host request/receipt windows, not exact device-clock application or internal sample time. Current is raw register counts; amps, power and battery validation are unavailable.".into()};
        let comparison = Comparison {
            settling_error_s: error(
                predicted.as_ref().map(|p| &p.command_to_settled_speed),
                &measured.command_to_settled_speed,
            ),
            rise_error_s: error(
                predicted.as_ref().map(|p| &p.rise_10_to_90),
                &measured.rise_10_to_90,
            ),
            zero_crossing_error_s: error(
                predicted.as_ref().map(|p| &p.command_to_zero_crossing),
                &measured.command_to_zero_crossing,
            ),
            measured,
            predicted,
        };
        trials.push(TrialReview {
            conditions,
            transition,
            comparison,
            measured_motion: motion,
            predicted_motion,
        });
    }
    Ok(Review {
        version: 1,
        recording_blake3: r.fingerprint(),
        request_blake3: blake3::hash(&serde_json::to_vec(request).map_err(|e| e.to_string())?)
            .to_hex()
            .to_string(),
        prediction_review_blake3: prediction
            .map(|p| serde_json::to_vec(p).map(|b| blake3::hash(&b).to_hex().to_string()))
            .transpose()
            .map_err(|e| e.to_string())?,
        request: request.clone(),
        trials,
        interpretation: format!(
            "{SCOPE} Targets and tolerances are declared before review; a desired speed is not evidence it was reached. Signed timing residuals are comparisons, not acceptance gates. Sampling-limited, travel-limited and unknown-age results cannot establish physical maximum capability."
        ),
    })
}
