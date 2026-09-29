//! Readable maneuver files: a timed list of body commands (forward, lateral,
//! turn) played through a steered gait, with the bounds and rates that
//! shape its transitions. Written for people and language models; converts
//! exactly to [`SteeringConfig`] and command samples. No robot, kinematics
//! or physics lives here.
use crate::contact_phase::steered::SteeringConfig;
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManeuverScript {
    pub version: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    pub limits: Limits,
    /// Where in the gait cycle to begin, as a fraction (0 to <1).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub start_phase: f64,
    /// Spacing of the planned body path.
    #[serde(default = "plan_step", skip_serializing_if = "is_plan_step")]
    pub plan_step_s: f64,
    /// Each command holds from `at_s` until the next; the first is at 0.
    pub commands: Vec<Command>,
    pub duration_s: f64,
}

/// Command bounds and how fast the body may change speed and turn rate.
/// Forward is the gait's walking direction; lateral is to its left.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub forward_m_s: f64,
    pub lateral_m_s: f64,
    pub yaw_deg_s: f64,
    pub forward_m_s2: f64,
    pub lateral_m_s2: f64,
    pub yaw_deg_s2: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub at_s: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub forward_m_s: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub lateral_m_s: f64,
    /// Counterclockwise seen from above.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub yaw_deg_s: f64,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.
}
fn plan_step() -> f64 {
    0.01
}
fn is_plan_step(v: &f64) -> bool {
    *v == plan_step()
}

impl ManeuverScript {
    /// Check the file; errors name the path.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != VERSION {
            return Err(format!("maneuver file version {} is not supported (expected {VERSION})", self.version));
        }
        let l = &self.limits;
        for (name, v) in [
            ("forward_m_s", l.forward_m_s),
            ("lateral_m_s", l.lateral_m_s),
            ("yaw_deg_s", l.yaw_deg_s),
            ("forward_m_s2", l.forward_m_s2),
            ("lateral_m_s2", l.lateral_m_s2),
            ("yaw_deg_s2", l.yaw_deg_s2),
        ] {
            if !v.is_finite() || v < 0. || (name.ends_with('2') && v == 0.) {
                return Err(format!("limits.{name} must be finite and {}", if name.ends_with('2') { "positive" } else { "not negative" }));
            }
        }
        if !(0. ..1.).contains(&self.start_phase) {
            return Err("start_phase must be in [0, 1)".into());
        }
        if !self.plan_step_s.is_finite() || self.plan_step_s <= 0. {
            return Err("plan_step_s must be positive".into());
        }
        if self.commands.first().is_none_or(|c| c.at_s != 0.) {
            return Err("commands must start with one at_s: 0".into());
        }
        for (i, c) in self.commands.iter().enumerate() {
            if i > 0 && !(c.at_s > self.commands[i - 1].at_s) {
                return Err(format!("commands[{i}].at_s must be later than commands[{}].at_s", i - 1));
            }
            for (name, v, bound) in [
                ("forward_m_s", c.forward_m_s, l.forward_m_s),
                ("lateral_m_s", c.lateral_m_s, l.lateral_m_s),
                ("yaw_deg_s", c.yaw_deg_s, l.yaw_deg_s),
            ] {
                if !v.is_finite() || v.abs() > bound {
                    return Err(format!("commands[{i}].{name} = {v} exceeds limits.{name} = {bound}"));
                }
            }
        }
        if !(self.duration_s > self.commands.last().map_or(0., |c| c.at_s)) || !self.duration_s.is_finite() {
            return Err("duration_s must be finite and after the last command".into());
        }
        Ok(())
    }

    /// Steering for a gait whose walking direction is `forward_axis_rad` in
    /// its own frame.
    pub fn steering(&self, forward_axis_rad: f64) -> Result<SteeringConfig, String> {
        self.validate()?;
        let l = &self.limits;
        Ok(SteeringConfig {
            maximum_twist: [l.forward_m_s, l.lateral_m_s, l.yaw_deg_s.to_radians()],
            maximum_twist_rate: [l.forward_m_s2, l.lateral_m_s2, l.yaw_deg_s2.to_radians()],
            plan_step_s: self.plan_step_s,
            forward_axis_rad,
        })
    }

    /// The command held at `time_s`: forward, lateral (m/s), yaw rate (rad/s).
    pub fn command_at(&self, time_s: f64) -> [f64; 3] {
        let i = self.commands.partition_point(|c| c.at_s <= time_s).saturating_sub(1);
        let c = &self.commands[i];
        [c.forward_m_s, c.lateral_m_s, c.yaw_deg_s.to_radians()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tour() -> ManeuverScript {
        serde_json::from_value(serde_json::json!({
            "version": 1, "name": "tour",
            "limits": {"forward_m_s": 0.25, "lateral_m_s": 0.2, "yaw_deg_s": 20, "forward_m_s2": 0.15, "lateral_m_s2": 0.15, "yaw_deg_s2": 20},
            "commands": [{"at_s": 0, "forward_m_s": 0.19}, {"at_s": 6, "forward_m_s": 0.1, "yaw_deg_s": 10}, {"at_s": 12}],
            "duration_s": 16
        }))
        .unwrap()
    }
    #[test]
    fn commands_hold_until_the_next_and_convert_units() {
        let m = tour();
        let s = m.steering(0.5).unwrap();
        assert_eq!(s.forward_axis_rad, 0.5);
        assert!((s.maximum_twist[2] - 20f64.to_radians()).abs() < 1e-15 && s.plan_step_s == 0.01);
        assert_eq!(m.command_at(5.99), [0.19, 0., 0.]);
        assert_eq!(m.command_at(6.), [0.1, 0., 10f64.to_radians()]);
        assert_eq!(m.command_at(100.), [0.; 3]);
    }
    #[test]
    fn mistakes_name_the_path() {
        let m = tour();
        let err = |f: fn(&mut ManeuverScript)| {
            let mut s = m.clone();
            f(&mut s);
            s.validate().unwrap_err()
        };
        assert!(err(|s| s.commands[1].yaw_deg_s = 30.).contains("commands[1].yaw_deg_s"));
        assert!(err(|s| s.commands[2].at_s = 5.).contains("commands[2].at_s"));
        assert!(err(|s| s.commands[0].at_s = 1.).contains("at_s: 0"));
        assert!(err(|s| s.limits.yaw_deg_s2 = 0.).contains("limits.yaw_deg_s2"));
        assert!(err(|s| s.duration_s = 12.).contains("duration_s"));
    }
}
