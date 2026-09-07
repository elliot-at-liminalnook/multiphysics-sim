//! Sampled lift-phase evidence. A lift requires simultaneous clearance,
//! unloading and support; unrelated peaks do not establish a successful lift.
//! This does not certify landing, continuous clearance, balance or walking.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiftRequirements {
    pub start_s: f64,
    pub end_s: f64,
    pub maximum_sample_gap_s: f64,
    pub qualifying_duration_s: f64,
    pub swing_link: String,
    pub minimum_clearance_m: f64,
    pub maximum_swing_force_n: f64,
    pub minimum_support_forces_n: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiftSample {
    pub time_s: f64,
    pub swing_clearance_m: f64,
    /// World-Z floor resultants, excluding internal contact forces.
    pub floor_forces_n: BTreeMap<String, f64>,
}

#[derive(Debug, Serialize)]
pub struct LiftReport {
    pub passed: bool,
    pub samples: usize,
    pub longest_qualifying_span_s: f64,
    pub qualifying_start_s: Option<f64>,
    pub peak_clearance_m: f64,
    pub minimum_swing_force_n: f64,
    pub failed_clearance_samples: usize,
    pub failed_unloading_samples: usize,
    pub failed_support_samples: usize,
    pub scope: &'static str,
}

pub fn evaluate_lift(samples: &[LiftSample], r: &LiftRequirements) -> Result<LiftReport, String> {
    let finite = [
        r.start_s,
        r.end_s,
        r.maximum_sample_gap_s,
        r.qualifying_duration_s,
        r.minimum_clearance_m,
        r.maximum_swing_force_n,
    ];
    if finite.iter().any(|v| !v.is_finite())
        || r.start_s < 0.0
        || r.end_s <= r.start_s
        || r.maximum_sample_gap_s <= 0.0
        || r.qualifying_duration_s <= 0.0
        || r.qualifying_duration_s > r.end_s - r.start_s
        || r.minimum_clearance_m <= 0.0
        || r.maximum_swing_force_n < 0.0
        || r.swing_link.trim().is_empty()
        || r.minimum_support_forces_n.is_empty()
        || r.minimum_support_forces_n.contains_key(&r.swing_link)
        || r.minimum_support_forces_n
            .iter()
            .any(|(n, f)| n.trim().is_empty() || !f.is_finite() || *f <= 0.0)
    {
        return Err("finite ordered phase, positive clearance/dwell/support and distinct named feet required".into());
    }
    let names: BTreeSet<_> = r
        .minimum_support_forces_n
        .keys()
        .chain(std::iter::once(&r.swing_link))
        .collect();
    if samples.iter().any(|s| {
        !s.time_s.is_finite()
            || s.time_s < 0.0
            || !s.swing_clearance_m.is_finite()
            || names
                .iter()
                .any(|n| !s.floor_forces_n.get(*n).is_some_and(|f| f.is_finite()))
    }) || samples.windows(2).any(|w| w[1].time_s <= w[0].time_s)
    {
        return Err("ordered finite samples with every required foot force required".into());
    }
    let time_tolerance =
        (32.0 * f64::EPSILON * (r.start_s.abs() + r.end_s.abs() + r.maximum_sample_gap_s))
            .min(r.maximum_sample_gap_s * 1e-6);
    let phase: Vec<_> = samples
        .iter()
        .filter(|s| s.time_s >= r.start_s - time_tolerance && s.time_s <= r.end_s + time_tolerance)
        .collect();
    if phase.len() < 2
        || (phase[0].time_s - r.start_s).abs() > time_tolerance
        || (phase.last().unwrap().time_s - r.end_s).abs() > time_tolerance
        || phase
            .windows(2)
            .any(|w| w[1].time_s - w[0].time_s > r.maximum_sample_gap_s + time_tolerance)
    {
        return Err("complete phase endpoints and bounded sample gaps required".into());
    }
    let mut report = LiftReport {
        passed: false,
        samples: phase.len(),
        longest_qualifying_span_s: 0.0,
        qualifying_start_s: None,
        peak_clearance_m: f64::NEG_INFINITY,
        minimum_swing_force_n: f64::INFINITY,
        failed_clearance_samples: 0,
        failed_unloading_samples: 0,
        failed_support_samples: 0,
        scope: "Consecutive reporting samples simultaneously meet caller-declared geometric clearance, swing unloading and support-force requirements. No interpolation or between-sample guarantee; not a landing, balance, collision, hardware-transfer or walking certificate.",
    };
    let mut start = None;
    for s in phase {
        let force = s.floor_forces_n[&r.swing_link];
        let clearance = s.swing_clearance_m >= r.minimum_clearance_m;
        // Absolute resultant also rejects an unphysical large downward load.
        let unloaded = force.abs() <= r.maximum_swing_force_n;
        let supported = r
            .minimum_support_forces_n
            .iter()
            .all(|(n, min)| s.floor_forces_n[n] >= *min);
        report.peak_clearance_m = report.peak_clearance_m.max(s.swing_clearance_m);
        report.minimum_swing_force_n = report.minimum_swing_force_n.min(force);
        report.failed_clearance_samples += usize::from(!clearance);
        report.failed_unloading_samples += usize::from(!unloaded);
        report.failed_support_samples += usize::from(!supported);
        if clearance && unloaded && supported {
            let begin = *start.get_or_insert(s.time_s);
            let span = s.time_s - begin;
            if span > report.longest_qualifying_span_s {
                report.longest_qualifying_span_s = span;
                report.qualifying_start_s = Some(begin);
            }
        } else {
            start = None;
        }
    }
    report.passed = report.longest_qualifying_span_s
        + time_tolerance.min(r.qualifying_duration_s * 1e-6)
        >= r.qualifying_duration_s;
    Ok(report)
}
