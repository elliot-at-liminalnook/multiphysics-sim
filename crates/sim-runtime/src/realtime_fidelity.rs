//! Realtime profile versus detailed model: error and speed.
//!
//! Both variants run through the shared session. The error of each observed
//! quantity is the largest difference between the realtime run (linearly
//! interpolated onto the detailed run's times) and the detailed run,
//! relative to the detailed run's range of that quantity. Speed is
//! simulated seconds per wall second on the measuring build.
use crate::system_builder::{self, Series};
use sim_core::BehaviorRegistry;
use sim_system::{FidelityMeasurement, SystemDocument};
use std::collections::BTreeMap;

pub struct Report {
    pub measurement: FidelityMeasurement,
    pub detailed: Vec<Series>,
    pub realtime: Vec<Series>,
}

fn interpolate(s: &Series, t: f64) -> f64 {
    match s.times.iter().position(|x| *x >= t) {
        Some(0) => s.values[0],
        Some(i) => {
            let (t0, t1, v0, v1) = (s.times[i - 1], s.times[i], s.values[i - 1], s.values[i]);
            v0 + (v1 - v0) * (t - t0) / (t1 - t0).max(1e-300)
        }
        None => *s.values.last().unwrap_or(&0.),
    }
}

/// Error of `candidate` against `reference`, relative to the reference range.
pub fn relative_error(reference: &Series, candidate: &Series) -> f64 {
    let (lo, hi) = reference.values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    let range = (hi - lo).max(1e-9 * hi.abs().max(lo.abs())).max(1e-12);
    reference.times.iter().zip(&reference.values).map(|(t, v)| (interpolate(candidate, *t) - v).abs()).fold(0., f64::max) / range
}

/// Content hash of the system with any published measurement removed, so a
/// measurement records exactly the model it was taken on.
pub fn measured_hash(document: &SystemDocument) -> String {
    let mut d = document.clone();
    if let Some(r) = &mut d.realtime {
        r.measured = None;
    }
    d.content_hash()
}

pub fn host() -> String {
    format!("{} {} ({} build)", std::env::consts::ARCH, std::env::consts::OS, if cfg!(debug_assertions) { "debug" } else { "release" })
}

/// Measure the document's realtime profile against its detailed model.
pub fn measure(document: &SystemDocument, registry: &BehaviorRegistry) -> Result<Report, String> {
    let profile = document.realtime.clone().ok_or("the system has no realtime profile")?;
    let realtime_doc = sim_system::profile::realtime(document, registry).map_err(|e| e.to_string())?;
    let run = |doc: &SystemDocument| -> Result<(Vec<Series>, f64), String> {
        let started = std::time::Instant::now();
        let series = system_builder::simulate(doc, registry, profile.duration, system_builder::config_for(doc), &profile.observe)?;
        Ok((series, profile.duration / started.elapsed().as_secs_f64().max(1e-9)))
    };
    let (detailed, detailed_speed) = run(document)?;
    let (realtime, realtime_speed) = run(&realtime_doc)?;
    let mut errors = BTreeMap::new();
    for key in &profile.observe {
        let d = detailed.iter().find(|s| &s.label == key).ok_or_else(|| format!("detailed run lacks {key}"))?;
        let r = realtime.iter().find(|s| &s.label == key).ok_or_else(|| format!("realtime run lacks {key}"))?;
        errors.insert(key.clone(), relative_error(d, r));
    }
    Ok(Report { measurement: FidelityMeasurement { content_hash: measured_hash(document), errors, detailed_speed, realtime_speed, host: host() }, detailed, realtime })
}

/// Every measured error within the published bound.
pub fn within_bound(document: &SystemDocument, measurement: &FidelityMeasurement) -> Result<(), String> {
    let profile = document.realtime.as_ref().ok_or("no realtime profile")?;
    let bound = |k: &str| profile.bounds.get(k).copied().unwrap_or(profile.bound);
    let over: Vec<String> = measurement.errors.iter().filter(|(k, e)| **e > bound(k)).map(|(k, e)| format!("{k}: {:.3} % > {:.3} %", 100. * e, 100. * bound(k))).collect();
    if over.is_empty() { Ok(()) } else { Err(over.join("; ")) }
}
