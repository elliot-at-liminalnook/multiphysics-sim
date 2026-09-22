//! Sample-window response metrics. No fitted time shifts or interpolated crossing times.
use super::control::Experiment;
use crate::experiment_comparison::{Limits, Trace, compare};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimingEstimate {
    pub interval_s: Option<[f64; 2]>,
    pub unresolved: Option<String>,
}
impl TimingEstimate {
    fn missing(reason: &str) -> Self {
        Self {
            interval_s: None,
            unresolved: Some(reason.into()),
        }
    }
    fn interval(value: [f64; 2]) -> Self {
        Self {
            interval_s: Some(value),
            unresolved: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub transition_start_s: f64,
    pub hold_start_s: f64,
    pub observed_until_s: f64,
    pub from_rad: f64,
    pub target_rad: f64,
    pub onset_delay: TimingEstimate,
    pub rise_10_to_90: TimingEstimate,
    pub settling_after_hold: TimingEstimate,
    pub sampled_overshoot_rad: f64,
    pub final_sample_error_rad: f64,
    pub maximum_sample_gap_s: f64,
    pub interpretation: String,
}
fn difference(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [(a[0] - b[1]).max(0.), (a[1] - b[0]).max(0.)]
}
pub fn response(
    trace: &Trace,
    start: [f64; 2],
    hold_start: f64,
    end: f64,
    from: f64,
    target: f64,
    quantum: f64,
    tolerance: f64,
    dwell: f64,
) -> Result<Response, String> {
    compare(
        trace,
        trace,
        &Limits {
            rmse: 0.,
            final_abs_error: 0.,
        },
    )?;
    if trace.unit != "rad"
        || [
            start[0], start[1], hold_start, end, from, target, quantum, tolerance, dwell,
        ]
        .iter()
        .any(|v| !v.is_finite())
        || start[0] > start[1]
        || hold_start < start[1]
        || end < hold_start
        || quantum <= 0.
        || tolerance < 0.
        || dwell <= 0.
        || from == target
    {
        return Err("Transient response requires a finite angular transition, ordered times, resolution, tolerance and dwell".into());
    }
    let samples = trace
        .samples
        .iter()
        .filter(|s| s.time_s >= start[0] && s.time_s <= end)
        .collect::<Vec<_>>();
    let last = samples.last().ok_or("No samples within transition")?;
    let delta = target - from;
    let sign = delta.signum();
    let baseline = trace
        .samples
        .iter()
        .rev()
        .find(|s| s.time_s <= start[0])
        .or(samples.first().copied())
        .unwrap();
    let baseline_established = (baseline.value - from).abs() <= tolerance.max(3. * quantum);
    let crossing = |fraction: f64| {
        samples
            .windows(2)
            .find(|w| {
                (w[0].value - from) / delta < fraction && (w[1].value - from) / delta >= fraction
            })
            .map(|w| [w[0].request_s.max(start[0]), w[1].completion_s])
    };
    let resolve = baseline_established && delta.abs() >= 10. * quantum;
    let unresolved = if !baseline_established {
        "Previous target was not established; rise/delay would conflate prior tracking error"
    } else {
        "Transition is too small relative to encoder resolution"
    };
    let onset = if resolve {
        crossing(3. * quantum / delta.abs())
            .map(|w| TimingEstimate::interval(difference(w, start)))
            .unwrap_or_else(|| TimingEstimate::missing("No resolved three-count onset crossing"))
    } else {
        TimingEstimate::missing(unresolved)
    };
    let rise = if resolve {
        crossing(0.1)
            .zip(crossing(0.9))
            .map(|(a, b)| TimingEstimate::interval(difference(b, a)))
            .unwrap_or_else(|| {
                TimingEstimate::missing("10% and 90% crossings are not both bracketed by samples")
            })
    } else {
        TimingEstimate::missing(unresolved)
    };
    let mut settling =
        TimingEstimate::missing("No sustained in-tolerance tail of the required duration");
    for (i, s) in samples.iter().enumerate() {
        if s.time_s < hold_start || last.time_s - s.time_s < dwell {
            continue;
        }
        if samples[i..]
            .iter()
            .all(|p| (p.value - target).abs() <= tolerance)
        {
            let gap = samples[i..]
                .windows(2)
                .map(|w| w[1].time_s - w[0].time_s)
                .fold(0., f64::max);
            if samples.len() - i < 3 || gap > dwell {
                settling = TimingEstimate::missing(
                    "Sampling gaps are too large to resolve the requested settling dwell",
                );
                continue;
            }
            let left = if i == 0 {
                hold_start
            } else {
                samples[i - 1].request_s.max(hold_start)
            };
            settling = TimingEstimate::interval(difference(
                [left, s.completion_s],
                [hold_start, hold_start],
            ));
            break;
        }
    }
    Ok(Response{transition_start_s:start[0],hold_start_s:hold_start,observed_until_s:last.time_s,from_rad:from,target_rad:target,onset_delay:onset,rise_10_to_90:rise,settling_after_hold:settling,sampled_overshoot_rad:samples.iter().map(|s|sign*(s.value-target)).fold(0.,f64::max),final_sample_error_rad:last.value-target,maximum_sample_gap_s:samples.windows(2).map(|w|w[1].time_s-w[0].time_s).fold(0.,f64::max),interpretation:"Intervals bracket observed crossings using sample transaction windows; unknown device sample age and angular calibration error remain additional uncertainty. Rise includes any programmed ramp. Overshoot is the largest sampled excess, not a guaranteed continuous peak. Settling describes only the observed hold tail; motion between samples or after the recording is unobserved.".into()})
}
pub fn trajectory(experiment: &Experiment, trace: &Trace) -> Vec<Result<Response, String>> {
    let mut results = vec![];
    for (i, w) in experiment.trajectory.windows(2).enumerate() {
        if w[0].position_rad == w[1].position_rad {
            continue;
        }
        let end = experiment
            .trajectory
            .iter()
            .enumerate()
            .skip(i + 2)
            .find(|(_, k)| k.position_rad != w[1].position_rad)
            .map(|(j, _)| experiment.trajectory[j - 1].time_s)
            .unwrap_or(experiment.duration_s);
        results.push(response(
            trace,
            [w[0].time_s, w[0].time_s],
            w[1].time_s,
            end,
            w[0].position_rad,
            w[1].position_rad,
            experiment.timing.encoder_quantum_rad,
            experiment.limits.settled_rad,
            (3. * experiment.timing.period_s).max(0.15),
        ));
    }
    results
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ReleaseResponse {
    pub command_window_s: [f64; 2],
    pub reference_sample_window_s: Option<[f64; 2]>,
    pub terminal_offset_from_reference_rad: Option<f64>,
    pub stationary_tail_start_delay: TimingEstimate,
    pub interpretation: String,
}
/// Bracket an observed encoder plateau, not an unobserved instantaneous mechanical stop.
pub fn release(
    trace: &crate::experiment_comparison::Trace,
    command: [f64; 2],
    quantum: f64,
    dwell_s: f64,
) -> Result<ReleaseResponse, String> {
    crate::experiment_comparison::compare(
        trace,
        trace,
        &crate::experiment_comparison::Limits {
            rmse: 0.,
            final_abs_error: 0.,
        },
    )?;
    if trace.unit != "rad"
        || command.iter().any(|t| !t.is_finite())
        || command[0] > command[1]
        || !quantum.is_finite()
        || quantum <= 0.
        || !dwell_s.is_finite()
        || dwell_s <= 0.
    {
        return Err("Invalid release timing, angular resolution or dwell".into());
    }
    let before = trace
        .samples
        .iter()
        .filter(|s| s.completion_s <= command[0])
        .collect::<Vec<_>>();
    let post = trace
        .samples
        .iter()
        .filter(|s| s.request_s >= command[1])
        .collect::<Vec<_>>();
    let reference = before.last().copied();
    let last = post.last().copied();
    let mut estimate = TimingEstimate {
        interval_s: None,
        unresolved: Some("Insufficient pre/post-release motion or stationary dwell".into()),
    };
    let prior = before
        .iter()
        .filter(|s| s.time_s >= command[0] - 0.1)
        .map(|s| s.value)
        .collect::<Vec<_>>();
    let motion = prior.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - prior.iter().copied().fold(f64::INFINITY, f64::min);
    if let Some(last) = last {
        let start = post
            .iter()
            .rposition(|s| (s.value - last.value).abs() > quantum * 1.001)
            .map_or(0, |i| i + 1);
        let tail = &post[start..];
        if motion >= 3. * quantum
            && tail.len() >= 3
            && last.request_s - tail[0].completion_s >= dwell_s
            && tail
                .windows(2)
                .all(|w| w[1].completion_s - w[0].request_s <= dwell_s * 0.5)
        {
            estimate = TimingEstimate {
                interval_s: Some([
                    (tail[0].request_s - command[1]).max(0.),
                    (tail[0].completion_s - command[0]).max(0.),
                ]),
                unresolved: None,
            };
        }
    }
    Ok(ReleaseResponse{command_window_s:command,reference_sample_window_s:reference.map(|s|[s.request_s,s.completion_s]),terminal_offset_from_reference_rad:reference.zip(last).map(|(a,b)|b.value-a.value),stationary_tail_start_delay:estimate,interpretation:"First observed suffix within one encoder count of the terminal sample, with required dwell and bounded gaps. This is a sampled plateau, not proof of zero velocity between samples. Terminal offset starts at the last sample before the release transaction, not an interpolated command-time angle. Device sample age and calibrated angular accuracy remain unknown.".into()})
}
