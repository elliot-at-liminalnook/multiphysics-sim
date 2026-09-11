//! Deterministic, resumable signed-drive experiment schedule. This is an
//! acquisition policy; it contains no motor equations or guessed properties.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SweepSchedule {
    pub levels: Vec<u16>,
    pub repetitions: u16,
    pub include_concurrent: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trial {
    pub index: usize,
    pub ids: Vec<u8>,
    pub drive: i16,
    pub repetition: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<DriveSegment>,
}
/// Piecewise constant command. Torque-off is terminal within a trial: an
/// experiment cannot re-enable drive while coasting without a new preflight.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveSegment {
    pub drive: i16,
    pub duration_ms: u64,
    pub torque_enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaveformTemplate {
    pub name: String,
    pub segments: Vec<DriveSegment>,
}
impl WaveformTemplate {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.name.trim().is_empty()
            || self.name.len() > 100
            || self.segments.is_empty()
            || self.segments.len() > 128
        {
            return Err("named nonempty bounded waveform required");
        }
        let mut disabled = false;
        for segment in &self.segments {
            if segment.drive.unsigned_abs() > 1000
                || !(20..=300).contains(&segment.duration_ms)
                || (!segment.torque_enabled && segment.drive != 0)
                || (disabled && segment.torque_enabled)
            {
                return Err("invalid waveform command/duration or re-enable after torque-off");
            }
            disabled |= !segment.torque_enabled;
        }
        if self.segments.iter().map(|s| s.duration_ms).sum::<u64>() > 3000
            || self.segments.iter().all(|s| s.drive == 0)
        {
            return Err("waveform must excite the actuator and last at most 3 seconds");
        }
        Ok(())
    }
}
impl SweepSchedule {
    pub fn waveform_trials(
        &self,
        ids: &[u8],
        templates: &[WaveformTemplate],
    ) -> Result<Vec<Trial>, &'static str> {
        // Reuse the ordinary schedule's ID/group/repetition validation. Levels
        // are empty in waveform plans so there are never two competing inputs.
        if !self.levels.is_empty() || templates.is_empty() || templates.len() > 100 {
            return Err("waveform plans require templates and an empty levels list");
        }
        let base = SweepSchedule {
            levels: vec![25],
            repetitions: self.repetitions,
            include_concurrent: self.include_concurrent,
        }
        .trials(ids)?;
        for (i, t) in templates.iter().enumerate() {
            t.validate()?;
            if templates[..i].iter().any(|other| other.name == t.name) {
                return Err("duplicate waveform name");
            }
        }
        let mut out = Vec::new();
        // Base has +/- pairs. Emit templates once per group/repetition.
        for anchor in base.iter().step_by(2) {
            for t in templates {
                out.push(Trial {
                    index: out.len(),
                    ids: anchor.ids.clone(),
                    drive: t.segments.iter().find(|s| s.drive != 0).unwrap().drive,
                    repetition: anchor.repetition,
                    label: Some(t.name.clone()),
                    segments: t.segments.clone(),
                });
            }
        }
        Ok(out)
    }
    pub fn trials(&self, ids: &[u8]) -> Result<Vec<Trial>, &'static str> {
        if ids.is_empty()
            || ids.iter().any(|id| *id > 253)
            || (0..ids.len()).any(|i| ids[..i].contains(&ids[i]))
            || self.levels.is_empty()
            || self.levels[0] == 0
            || self.levels[0] > 25
            || self.levels.iter().any(|v| *v > 1000)
            || self
                .levels
                .windows(2)
                .any(|w| w[1] <= w[0] || w[1] - w[0] > 100)
            || !(1..=20).contains(&self.repetitions)
        {
            return Err(
                "invalid progressive sweep: unique IDs, start <=25, increments <=100, drive <=1000, repeats 1..20",
            );
        }
        let mut groups: Vec<Vec<u8>> = ids.iter().map(|id| vec![*id]).collect();
        if self.include_concurrent && ids.len() > 1 {
            groups.push(ids.to_vec());
        }
        let mut trials = Vec::new();
        for group in groups {
            for &level in &self.levels {
                for repetition in 0..self.repetitions {
                    for sign in [1, -1] {
                        trials.push(Trial {
                            index: trials.len(),
                            ids: group.clone(),
                            drive: level as i16 * sign,
                            repetition,
                            label: None,
                            segments: Vec::new(),
                        });
                    }
                }
            }
        }
        Ok(trials)
    }
}
/// A checkpoint certifies only a contiguous prefix of completed trials.
/// Failed/interrupted trials are rerun after a new preflight; never skipped.
pub fn resume_index(trials: &[Trial], completed: &[Trial]) -> Result<usize, &'static str> {
    if completed.len() > trials.len() || trials[..completed.len()] != *completed {
        return Err("checkpoint does not match the exact schedule prefix");
    }
    Ok(completed.len())
}
/// Admission check using measured velocity and an explicitly supplied latency
/// margin. It is a conservative host guard, not a certified stopping-distance
/// model: acceleration, coasting, and a failed bus require separate bounds.
pub fn travel_guard(
    position: f64,
    home: f64,
    speed: f64,
    limit: f64,
    latency_s: f64,
) -> Result<(), &'static str> {
    if [position, home, speed, limit, latency_s]
        .iter()
        .any(|v| !v.is_finite())
        || limit <= 0.
        || latency_s < 0.
    {
        return Err("invalid travel guard inputs");
    }
    if (position - home).abs() + speed.abs() * latency_s >= limit {
        return Err("travel margin exhausted");
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_servos_then_concurrent_both_directions_and_exact_resume() {
        let plan = SweepSchedule {
            levels: vec![25, 50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 1000],
            repetitions: 3,
            include_concurrent: true,
        };
        let t = plan.trials(&(4..=12).collect::<Vec<_>>()).unwrap();
        assert_eq!(t.len(), 720);
        assert!(t[..648].iter().all(|t| t.ids.len() == 1));
        assert!(t[648..].iter().all(|t| t.ids.len() == 9));
        for pair in t.chunks(2) {
            assert_eq!(pair[0].drive, -pair[1].drive);
            assert_eq!(pair[0].ids, pair[1].ids);
        }
        assert_eq!(resume_index(&t, &t[..73]).unwrap(), 73);
        assert!(resume_index(&t, &t[1..74]).is_err());
        assert!(plan.trials(&[4, 4]).is_err());
    }
    #[test]
    fn invalid_progression_and_travel_projection_fail_closed() {
        for levels in [vec![26], vec![25, 200], vec![25, 25], vec![0], vec![]] {
            assert!(
                SweepSchedule {
                    levels,
                    repetitions: 1,
                    include_concurrent: false
                }
                .trials(&[4])
                .is_err()
            );
        }
        assert!(travel_guard(10., 0., 100., 50., 0.3).is_ok());
        assert!(travel_guard(25., 0., 100., 50., 0.3).is_err());
        assert!(travel_guard(f64::NAN, 0., 0., 50., 0.3).is_err());
    }
}

#[cfg(test)]
mod waveform_tests {
    use super::*;
    #[test]
    fn preserves_reversal_order_terminal_coast_and_resume_identity() {
        let s = SweepSchedule {
            levels: vec![],
            repetitions: 2,
            include_concurrent: true,
        };
        let wave = WaveformTemplate {
            name: "reversal".into(),
            segments: vec![
                DriveSegment {
                    drive: 100,
                    duration_ms: 50,
                    torque_enabled: true,
                },
                DriveSegment {
                    drive: -100,
                    duration_ms: 50,
                    torque_enabled: true,
                },
                DriveSegment {
                    drive: 0,
                    duration_ms: 150,
                    torque_enabled: false,
                },
            ],
        };
        let trials = s.waveform_trials(&[4, 5], &[wave.clone()]).unwrap();
        assert_eq!(trials.len(), 6);
        assert_eq!(trials[4].ids, vec![4, 5]);
        assert_eq!(trials[4].segments, wave.segments);
        let saved = serde_json::to_vec(&trials[..3]).unwrap();
        let prefix: Vec<Trial> = serde_json::from_slice(&saved).unwrap();
        assert_eq!(resume_index(&trials, &prefix).unwrap(), 3);
        let mut altered = prefix;
        altered[0].segments[1].drive = -50;
        assert!(resume_index(&trials, &altered).is_err());
        let mut bad = wave.clone();
        bad.segments.push(DriveSegment {
            drive: 100,
            duration_ms: 50,
            torque_enabled: true,
        });
        assert!(bad.validate().is_err());
        bad = wave;
        bad.segments[2].drive = 1;
        assert!(bad.validate().is_err());
    }
    #[test]
    fn old_pulse_checkpoints_decode_without_waveform_fields() {
        let t: Trial =
            serde_json::from_str(r#"{"index":0,"ids":[4],"drive":25,"repetition":0}"#).unwrap();
        assert!(t.segments.is_empty() && t.label.is_none());
        assert_eq!(
            serde_json::to_value(t).unwrap(),
            serde_json::json!({"index":0,"ids":[4],"drive":25,"repetition":0})
        );
    }
}
