//! Bounded environment loads, independent of robot/controller implementation.
//! Forces and moments use world axes; moments are about the selected base COM.
//! Pulse boundaries must coincide with nominal physics steps. Loads are held
//! throughout each step, including every rejected trial and internal subdivision.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldLoadPulse {
    pub name: String,
    pub start_s: f64,
    pub duration_s: f64,
    pub force_world_n: [f64; 3],
    pub moment_world_nm: [f64; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldLoadSchedule {
    pub version: u32,
    pub base_link: String,
    /// Identifies the experimental assumption or physical measurement.
    pub provenance: String,
    pub maximum_force_n: f64,
    pub maximum_moment_nm: f64,
    pub pulses: Vec<WorldLoadPulse>,
}

pub struct BoundWorldLoads {
    offset: usize,
    pulses: Vec<(usize, usize, [f64; 6])>,
}

fn step_index(time: f64, step: f64, horizon: usize) -> Result<usize, String> {
    let n = time / step;
    if !time.is_finite()
        || time < 0.0
        || !n.is_finite()
        || n > horizon as f64
        || (n - n.round()).abs() > 1e-8
    {
        return Err(
            "world load boundaries must align with physics steps inside the horizon".into(),
        );
    }
    Ok(n.round() as usize)
}

impl WorldLoadSchedule {
    /// `floating_bases` is in generalized-coordinate order. Grounded bodies
    /// must be excluded, so their names cannot accidentally select a free base.
    pub fn bind(
        &self,
        floating_bases: &[String],
        step_s: f64,
        steps: usize,
    ) -> Result<BoundWorldLoads, String> {
        if self.version != 1
            || self.provenance.trim().is_empty()
            || !step_s.is_finite()
            || step_s <= 0.0
            || steps == 0
            || [self.maximum_force_n, self.maximum_moment_nm]
                .iter()
                .any(|v| !v.is_finite() || *v < 0.0)
            || self.pulses.len() > 10_000
        {
            return Err("invalid world load version, provenance, bounds or horizon".into());
        }
        let base = floating_bases
            .iter()
            .position(|n| n == &self.base_link)
            .ok_or("world load requires a named floating base")?;
        let mut names = BTreeSet::new();
        let mut pulses = Vec::new();
        let mut boundaries = BTreeSet::new();
        for p in &self.pulses {
            if p.name.trim().is_empty()
                || !names.insert(&p.name)
                || !p.duration_s.is_finite()
                || p.duration_s <= 0.0
                || p.force_world_n
                    .iter()
                    .chain(&p.moment_world_nm)
                    .any(|v| !v.is_finite())
            {
                return Err("invalid world load pulse name, duration or wrench".into());
            }
            let start = step_index(p.start_s, step_s, steps)?;
            let end = step_index(p.start_s + p.duration_s, step_s, steps)?;
            if end <= start {
                return Err("world load pulse must span at least one physics step".into());
            }
            pulses.push((
                start,
                end,
                [
                    p.force_world_n[0],
                    p.force_world_n[1],
                    p.force_world_n[2],
                    p.moment_world_nm[0],
                    p.moment_world_nm[1],
                    p.moment_world_nm[2],
                ],
            ));
            boundaries.insert(start);
            boundaries.insert(end);
        }
        let result = BoundWorldLoads {
            offset: 6 * base,
            pulses,
        };
        // Check the combined load on every constant interval, including overlaps.
        for step in boundaries {
            let w = result.wrench(step);
            let force = w[0].hypot(w[1]).hypot(w[2]);
            let moment = w[3].hypot(w[4]).hypot(w[5]);
            if !force.is_finite()
                || !moment.is_finite()
                || force > self.maximum_force_n
                || moment > self.maximum_moment_nm
            {
                return Err("combined world load exceeds declared force/moment bounds".into());
            }
        }
        Ok(result)
    }
}

impl BoundWorldLoads {
    pub fn wrench(&self, step: usize) -> [f64; 6] {
        let mut value = [0.0; 6];
        for (start, end, load) in &self.pulses {
            if *start <= step && step < *end {
                for i in 0..6 {
                    value[i] += load[i];
                }
            }
        }
        value
    }
    pub fn add_to(&self, step: usize, loads: &mut [f64]) -> Result<(), String> {
        let target = loads
            .get_mut(self.offset..self.offset + 6)
            .ok_or("world load base dimension mismatch")?;
        for (v, w) in target.iter_mut().zip(self.wrench(step)) {
            *v += w;
        }
        if target.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite combined applied load".into());
        }
        Ok(())
    }
}
