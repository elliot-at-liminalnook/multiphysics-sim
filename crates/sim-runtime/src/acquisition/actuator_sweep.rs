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
}
impl SweepSchedule {
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
