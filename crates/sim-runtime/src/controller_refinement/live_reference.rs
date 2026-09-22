//! Named live controller references with explicit bench mapping and strict freshness.
use super::{fpga, trajectory_binding::Binding};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    pub sequence: u64,
    pub time_s: f64,
    pub targets_rad: BTreeMap<String, f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub bindings: Vec<Binding>,
    pub amplitude: f64,
    pub source: String,
    pub initial: Sample,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Received {
    pub session: String,
    pub received_unix_s: f64,
    pub sample: Sample,
}
impl Request {
    pub fn validate(&self, scope: &[u8]) -> Result<(), String> {
        let mut ids: Vec<_> = self.bindings.iter().map(|b| b.motor_id).collect();
        ids.sort_unstable();
        fpga::validate_physical_scope(scope, &ids)?;
        if self.bindings.len() != 3
            || self.source.is_empty()
            || !self.amplitude.is_finite()
            || !(0.01..=0.1).contains(&self.amplitude)
            || self.bindings.iter().enumerate().any(|(i, b)| {
                ![-1, 1].contains(&b.polarity)
                    || self.bindings[..i]
                        .iter()
                        .any(|a| a.coordinate == b.coordinate)
            })
        {
            return Err("Live sync requires three unique motor/coordinate bindings, explicit polarity, and 1–10% bench amplitude".into());
        }
        self.validate_sample(&self.initial)
    }
    pub fn validate_sample(&self, s: &Sample) -> Result<(), String> {
        if !s.time_s.is_finite()
            || s.time_s < 0.
            || s.targets_rad.values().any(|v| !v.is_finite())
            || self
                .bindings
                .iter()
                .any(|b| !s.targets_rad.contains_key(&b.coordinate))
        {
            return Err("Missing or nonfinite live controller targets".into());
        }
        Ok(())
    }
    pub fn encode(&self, s: &Sample, previous: &[i16; 9]) -> Result<[i16; 9], String> {
        self.validate_sample(s)?;
        let mut row = [0i16; 9];
        for b in &self.bindings {
            let delta = (s.targets_rad[&b.coordinate] - self.initial.targets_rad[&b.coordinate])
                * self.amplitude
                * f64::from(b.polarity)
                * 4096.
                / std::f64::consts::TAU;
            if !delta.is_finite() || delta.abs() > 80. {
                return Err(format!(
                    "{} exceeds live bench excursion (80 counts); reduce motion scale",
                    b.coordinate
                ));
            }
            let i = usize::from(b.motor_id - 4);
            row[i] = delta.round() as i16;
            if (i32::from(row[i]) - i32::from(previous[i])).abs() > 32 {
                return Err(format!(
                    "{} exceeds live target-change limit (32 counts); reduce motion scale",
                    b.coordinate
                ));
            }
        }
        Ok(row)
    }
}
/// Duplicates may be held for one host control tick but may not renew freshness.
#[derive(Clone, Debug)]
pub struct Cursor {
    sequence: u64,
    time_s: f64,
}
impl Cursor {
    pub fn new(s: &Sample) -> Self {
        Self {
            sequence: s.sequence,
            time_s: s.time_s,
        }
    }
    pub fn advance(&mut self, s: &Sample) -> Result<bool, String> {
        if !s.time_s.is_finite() || s.sequence < self.sequence || s.time_s < self.time_s {
            return Err("Live reference sequence/time regressed".into());
        }
        if s.sequence == self.sequence {
            if s.time_s != self.time_s {
                return Err("Duplicate live sequence changed timestamp".into());
            }
            return Ok(false);
        }
        if s.time_s <= self.time_s {
            return Err(
                "Simulation did not advance; stale targets cannot refresh the live session".into(),
            );
        }
        self.sequence = s.sequence;
        self.time_s = s.time_s;
        Ok(true)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Request {
        Request {
            bindings: (0..3)
                .map(|i| Binding {
                    coordinate: format!("axis{i}"),
                    motor_id: 10 + i,
                    polarity: 1,
                })
                .collect(),
            amplitude: 0.03,
            source: "test".into(),
            initial: Sample {
                sequence: 0,
                time_s: 0.,
                targets_rad: (0..3).map(|i| (format!("axis{i}"), 0.)).collect(),
            },
        }
    }
    #[test]
    fn units_mapping_bounds_and_cursor() {
        let r = request();
        r.validate(&[10, 11, 12]).unwrap();
        let mut s = r.initial.clone();
        s.sequence = 1;
        s.time_s = 0.02;
        s.targets_rad.insert("axis1".into(), 0.5);
        let row = r.encode(&s, &[0; 9]).unwrap();
        assert_eq!(row[7], 10);
        assert_eq!(row[6], 0);
        let mut c = Cursor::new(&r.initial);
        assert!(c.advance(&s).unwrap());
        assert!(!c.advance(&s).unwrap());
        s.sequence += 1;
        assert!(c.advance(&s).is_err());
        s.targets_rad.insert("axis1".into(), 10.);
        assert!(r.encode(&s, &row).is_err());
    }
    #[test]
    fn reject_missing_duplicate_and_outside_scope() {
        let mut r = request();
        assert!(r.validate(&[10, 11]).is_err());
        r.bindings[1].motor_id = 10;
        assert!(r.validate(&[10, 11, 12]).is_err());
        let r = request();
        let mut s = r.initial.clone();
        s.targets_rad.remove("axis0");
        assert!(r.encode(&s, &[0; 9]).is_err());
    }
}
