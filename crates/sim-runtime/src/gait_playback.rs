//! Shared gait playback: one implementation for every consumer.
//!
//! A gait is a compiled contact reference (`compiled.json` from the gait
//! search): a periodic joint trajectory over the robot's independent motor
//! coordinates. Viewers (through the WASM worker) and the hardware host both
//! sample it here on a shared playback clock, and map joint angles to a
//! physical leg's encoder counts with the same [`LegBinding`], so the
//! simulated robot and the real leg follow identical curves.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sim_domain_control::{reference_governor::{Config as GovernorConfig, State as GovernorState}, trajectory::{Trajectory, TrajectoryConfig}};

type R<T> = Result<T, String>;
const RAD: f64 = std::f64::consts::TAU / 4096.;

#[derive(Clone, Debug, Serialize)]
pub struct GaitInfo {
    pub name: String,
    /// CAD joint names ("+X | Foot servo output"), in trajectory order.
    pub joints: Vec<String>,
    pub period_s: f64,
    pub nominal_speed_m_s: f64,
    /// Largest reference speed per joint (rad/s).
    pub maximum_rates_rad_s: Vec<f64>,
    /// The reference governor the simulation ran this gait through (its
    /// commanded motion is the governed reference, not the raw curve).
    pub governor: Option<GovernorConfig>,
}
#[derive(Clone, Debug)]
pub struct Gait {
    pub info: GaitInfo,
    trajectory: Trajectory,
}
impl Gait {
    /// From a compiled contact reference (`compiled.json`).
    pub fn from_compiled(compiled: &Value, name: &str) -> R<Self> {
        let config: TrajectoryConfig = serde_json::from_value(compiled["trajectory"].clone()).map_err(|e| format!("gait trajectory: {e}"))?;
        let joints: Vec<String> = compiled["recipe"]["independent_coordinates"].as_array().ok_or("gait has no independent coordinates")?
            .iter().map(|c| c.as_str().unwrap_or_default().trim_start_matches("joint.").to_string()).collect();
        let period_s = config.keyframes.last().map(|k| k.time_s).ok_or("gait has no keyframes")?;
        let trajectory = Trajectory::new(config)?;
        if trajectory.dimension() != joints.len() || !(period_s > 0.) {
            return Err("gait coordinates and trajectory disagree".into());
        }
        let maximum_rates_rad_s = trajectory.maximum_absolute_rates()?;
        // Hosts attach the trial's governor as `playback_governor` (from its spec).
        let governor = match compiled.get("playback_governor") {
            Some(Value::Null) | None => None,
            Some(g) => {
                let g: GovernorConfig = serde_json::from_value(g.clone()).map_err(|e| format!("playback governor: {e}"))?;
                g.validate()?;
                Some(g)
            }
        };
        Ok(Self { info: GaitInfo { name: name.into(), joints, period_s, nominal_speed_m_s: compiled["nominal_speed_m_s"].as_f64().unwrap_or(0.), maximum_rates_rad_s, governor }, trajectory })
    }
    /// Joint angles (rad) at gait time `t` (periodic).
    pub fn sample(&self, t: f64) -> R<Vec<f64>> {
        Ok(self.trajectory.sample(t.rem_euclid(self.info.period_s))?.values)
    }
    pub fn rates(&self, t: f64) -> R<Vec<f64>> {
        Ok(self.trajectory.sample(t.rem_euclid(self.info.period_s))?.rates)
    }
    pub fn index(&self, joint: &str) -> Option<usize> {
        self.info.joints.iter().position(|j| j == joint)
    }
}

/// The gait as commanded: each joint's sampled reference passed through the
/// gait's reference governor (the same law the simulation runs), optionally
/// tightened per joint (e.g. to a physical motor's limits) and clamped to a
/// per-joint range. Stateful: step it with the playback clock.
#[derive(Clone, Debug)]
pub struct GovernedGait {
    pub gait: Gait,
    configs: Vec<Option<GovernorConfig>>,
    ranges: Vec<Option<(f64, f64)>>,
    states: Vec<Option<GovernorState>>,
}
impl GovernedGait {
    pub fn new(gait: Gait) -> Self {
        let n = gait.info.joints.len();
        let configs = vec![gait.info.governor.clone(); n];
        Self { gait, configs, ranges: vec![None; n], states: vec![None; n] }
    }
    /// Tighten joint `i`'s governor to at most `speed` (rad/s) and
    /// `acceleration` (rad/s²); a gait without a governor gets one.
    pub fn limit(&mut self, i: usize, speed: f64, acceleration: f64) -> R<()> {
        let base = self.configs[i].clone().unwrap_or(GovernorConfig { period_s: 0.02, maximum_speed_rad_s: f64::INFINITY, maximum_acceleration_rad_s2: f64::INFINITY, response_rate_per_s: 10. });
        let c = GovernorConfig { maximum_speed_rad_s: base.maximum_speed_rad_s.min(speed), maximum_acceleration_rad_s2: base.maximum_acceleration_rad_s2.min(acceleration), ..base };
        c.validate()?;
        self.configs[i] = Some(c);
        Ok(())
    }
    pub fn config(&self, i: usize) -> Option<&GovernorConfig> {
        self.configs[i].as_ref()
    }
    /// Keep joint `i`'s desired reference inside [lo, hi] (rad).
    pub fn clamp(&mut self, i: usize, lo: f64, hi: f64) {
        self.ranges[i] = Some((lo.min(hi), lo.max(hi)));
    }
    /// Start joint `i` from a measured angle (e.g. the real leg's pose).
    pub fn start_from(&mut self, i: usize, angle_rad: f64) {
        self.states[i] = Some(GovernorState { angle_rad, velocity_rad_s: 0. });
    }
    /// Desired (raw, clamped) reference at gait time `t`.
    pub fn desired(&self, t: f64) -> R<Vec<f64>> {
        Ok(self.gait.sample(t)?.into_iter().zip(&self.ranges).map(|(q, r)| r.map_or(q, |(lo, hi)| q.clamp(lo, hi))).collect())
    }
    /// Advance `dt` s of wall time toward the desired reference at gait time
    /// `t`; returns (angle, velocity) per joint. Joints without a governor
    /// follow the desired reference directly (velocity from the curve × `rate_scale`).
    pub fn step(&mut self, t: f64, dt: f64, rate_scale: f64) -> R<Vec<(f64, f64)>> {
        let desired = self.desired(t)?;
        let rates = self.gait.rates(t)?;
        let mut out = Vec::with_capacity(desired.len());
        for i in 0..desired.len() {
            let Some(c) = &self.configs[i] else {
                out.push((desired[i], rates[i] * rate_scale));
                continue;
            };
            let mut state = self.states[i].unwrap_or(GovernorState { angle_rad: desired[i], velocity_rad_s: 0. });
            let n = (dt / c.period_s).ceil().max(1.) as usize;
            let sub = GovernorConfig { period_s: dt.max(1e-6) / n as f64, ..c.clone() };
            for _ in 0..n {
                state = sub.update(state, desired[i])?;
            }
            self.states[i] = Some(state);
            out.push((state.angle_rad, state.velocity_rad_s));
        }
        Ok(out)
    }
}

/// One physical motor bound to a CAD joint: `reference_counts` is the encoder
/// reading when the real joint is at the CAD home angle `home_rad`, and
/// `polarity` (±1) is the encoder direction relative to the CAD joint.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LegBinding {
    pub id: u8,
    pub joint: String,
    pub polarity: f64,
    pub reference_counts: f64,
    pub home_rad: f64,
}
impl LegBinding {
    pub fn validate(&self) -> R<()> {
        if (self.polarity.abs() - 1.).abs() > 1e-9 || !self.reference_counts.is_finite() || !self.home_rad.is_finite() || self.joint.is_empty() {
            return Err(format!("motor {}: binding needs a joint, polarity ±1 and finite alignment", self.id));
        }
        Ok(())
    }
    pub fn counts(&self, joint_rad: f64) -> f64 {
        self.reference_counts + self.polarity * (joint_rad - self.home_rad) / RAD
    }
    pub fn joint_rad(&self, counts: f64) -> f64 {
        self.home_rad + self.polarity * (counts - self.reference_counts) * RAD
    }
    pub fn counts_per_rad(&self) -> f64 {
        1. / RAD
    }
}

/// Playback clock: gait time advances at `speed_scale` × wall time.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Clock {
    pub gait_time_s: f64,
    pub speed_scale: f64,
    pub playing: bool,
}
impl Clock {
    pub fn advance(&mut self, dt: f64) {
        if self.playing {
            self.gait_time_s += dt * self.speed_scale.clamp(0., 1.);
        }
    }
}

/// Encoder-count targets for the bound motors at gait time `t`, with the
/// reference speed each needs (counts/s) at `speed_scale`.
pub fn leg_targets(gait: &Gait, bindings: &[LegBinding], t: f64, speed_scale: f64) -> R<Vec<(u8, f64, f64)>> {
    let values = gait.sample(t)?;
    let rates = gait.rates(t)?;
    bindings.iter().map(|b| {
        b.validate()?;
        let i = gait.index(&b.joint).ok_or(format!("gait has no joint {}", b.joint))?;
        Ok((b.id, b.counts(values[i]), b.polarity * rates[i] * speed_scale / RAD))
    }).collect()
}
/// Largest encoder acceleration any bound motor needs at full speed
/// (counts/s²), from 400 samples per period; it scales with speed_scale².
pub fn peak_counts_per_s2(gait: &Gait, bindings: &[LegBinding]) -> R<f64> {
    let idx: Vec<usize> = bindings.iter().map(|b| gait.index(&b.joint).ok_or(format!("gait has no joint {}", b.joint))).collect::<R<_>>()?;
    let mut peak = 0f64;
    for k in 0..400 {
        let s = gait.trajectory.sample(gait.info.period_s * k as f64 / 400.)?;
        for i in &idx {
            peak = peak.max(s.accelerations[*i].abs() / RAD);
        }
    }
    Ok(peak)
}
/// Largest encoder speed any bound motor needs at `speed_scale` (counts/s).
pub fn peak_counts_per_s(gait: &Gait, bindings: &[LegBinding], speed_scale: f64) -> R<f64> {
    bindings.iter().map(|b| {
        let i = gait.index(&b.joint).ok_or(format!("gait has no joint {}", b.joint))?;
        Ok(gait.info.maximum_rates_rad_s[i] * speed_scale / RAD)
    }).try_fold(0f64, |m, v: R<f64>| Ok(m.max(v?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binding_round_trips_and_targets_follow_the_gait() {
        let b = LegBinding { id: 1, joint: "+X | Foot servo output".into(), polarity: -1., reference_counts: 3100., home_rad: -0.4 };
        for q in [-1., -0.4, 0.3] {
            assert!((b.joint_rad(b.counts(q)) - q).abs() < 1e-12);
        }
        assert_eq!(b.counts(-0.4), 3100.);
        let compiled: Value = serde_json::from_slice(&std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19/comparison/2301-CmaEs-000/compiled.json")).unwrap()).unwrap();
        let gait = Gait::from_compiled(&compiled, "baseline").unwrap();
        assert_eq!(gait.info.joints.len(), 12);
        let i = gait.index("+X | Foot servo output").unwrap();
        let t = 0.3;
        let targets = leg_targets(&gait, &[b.clone()], t, 0.5).unwrap();
        assert!((targets[0].1 - b.counts(gait.sample(t).unwrap()[i])).abs() < 1e-9);
        // Periodic.
        let p = gait.info.period_s;
        assert!((gait.sample(t).unwrap()[i] - gait.sample(t + 2. * p).unwrap()[i]).abs() < 1e-9);
        assert!(peak_counts_per_s(&gait, &[b], 1.).unwrap() > 0.);
    }
    #[test]
    fn governed_gait_respects_limits_and_converges() {
        let mut compiled: Value = serde_json::from_slice(&std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19/comparison/2301-CmaEs-000/compiled.json")).unwrap()).unwrap();
        compiled["playback_governor"] = serde_json::json!({"period_s": 0.02, "maximum_speed_rad_s": 1.4, "maximum_acceleration_rad_s2": 7.0, "response_rate_per_s": 10.});
        let gait = Gait::from_compiled(&compiled, "governed").unwrap();
        let mut g = GovernedGait::new(gait);
        let i = g.gait.index("+X | Hip servo output").unwrap();
        g.limit(i, 0.5, 2.0).unwrap();
        g.start_from(i, 0.3);
        let (mut t, mut last) = (0., None::<(f64, f64)>);
        for _ in 0..600 {
            let s = g.step(t, 0.03, 1.).unwrap();
            let (q, v) = s[i];
            assert!(v.abs() <= 0.5 + 1e-9, "speed limited");
            if let Some((_, v0)) = last {
                assert!(((v - v0) / 0.03).abs() <= 2.0 + 1e-6, "acceleration limited");
            }
            last = Some((q, v));
            t += 0.03;
        }
        // Unlimited joints follow the gait's own governor.
        let k = g.gait.index("+X | Foot servo output").unwrap();
        assert_eq!(g.config(k).unwrap().maximum_speed_rad_s, 1.4);
    }
}
