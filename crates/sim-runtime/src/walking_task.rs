//! Task scoring over executed poses and held planning references. No actuation
//! or dynamics lives here. The same sampled lift checker audits offline runs.
use crate::{
    contact_audit::sampled_floor_clearances,
    lift::{LiftReport, LiftRequirements, LiftSample, evaluate_lift},
    session::LinkPose,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sim_domain_robot::Articulated;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WalkingTaskConfig {
    pub version: u32,
    pub minimum_clearance_m: f64,
    pub maximum_swing_force_n: f64,
    pub minimum_support_force_n: f64,
    pub qualifying_duration_s: f64,
    pub body_position_scale_m: f64,
    pub body_position_weight_per_s: f64,
    /// Maximum normalized squared-error cost, limiting the incentive to end a
    /// bad episode early merely to avoid an unbounded future tracking penalty.
    pub body_position_cost_cap: f64,
    pub qualified_step_reward: f64,
    pub failed_step_penalty: f64,
    /// Optional task-only heading score. Does not expose world heading to the
    /// motor policy or change its observation contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<HeadingTaskConfig>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadingTaskConfig {
    pub scale_rad: f64,
    pub weight_per_s: f64,
    pub cost_cap: f64,
    /// Add to the planner yaw to obtain the target world-Z bearing of the
    /// reference body's +X axis. Explicitly declare even a zero frame offset.
    pub reference_offset_rad: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct HeadingObservation {
    pub target_rad: f64,
    pub actual_rad: f64,
    /// Actual minus target, wrapped to [-pi, pi].
    pub error_rad: f64,
    pub reward: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StepOutcome {
    pub step: u64,
    pub foot: usize,
    pub complete: bool,
    pub passed: bool,
    pub lift: Option<LiftReport>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct WalkingObservation {
    pub reference_sample: Option<u64>,
    /// Error against the planning reference held during this control interval.
    pub body_error_world_m: [f64; 3],
    pub body_reward: f64,
    pub step_reward: f64,
    pub qualified_steps: usize,
    pub failed_steps: usize,
    pub outcome: Option<StepOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<HeadingObservation>,
}
struct ActiveSwing {
    step: u64,
    foot: usize,
    samples: Vec<LiftSample>,
}
pub struct WalkingMonitor {
    config: WalkingTaskConfig,
    period_s: f64,
    body: String,
    feet: Vec<String>,
    active: Option<ActiveSwing>,
    qualified: usize,
    failed: usize,
}
impl WalkingMonitor {
    pub fn new(
        config: WalkingTaskConfig,
        period_s: f64,
        body: String,
        feet: Vec<String>,
        art: &Articulated,
    ) -> Result<Self, String> {
        let positive = [
            period_s,
            config.minimum_clearance_m,
            config.minimum_support_force_n,
            config.qualifying_duration_s,
            config.body_position_scale_m,
            config.body_position_cost_cap,
        ];
        let nonnegative = [
            config.maximum_swing_force_n,
            config.body_position_weight_per_s,
            config.qualified_step_reward,
            config.failed_step_penalty,
        ];
        if config.version != 1
            || positive.iter().any(|v| !v.is_finite() || *v <= 0.)
            || nonnegative.iter().any(|v| !v.is_finite() || *v < 0.)
            || feet.len() < 2
            || feet.iter().collect::<BTreeSet<_>>().len() != feet.len()
            || std::iter::once(&body)
                .chain(&feet)
                .any(|n| !art.links.iter().any(|l| &l.name == n))
        {
            return Err("walking task requires v1, finite physical scales, nonnegative weights and distinct named feet".into());
        }
        if config.heading.as_ref().is_some_and(|h| {
            !h.scale_rad.is_finite()
                || h.scale_rad <= 0.
                || !h.weight_per_s.is_finite()
                || h.weight_per_s < 0.
                || !h.cost_cap.is_finite()
                || h.cost_cap <= 0.
                || !h.reference_offset_rad.is_finite()
        }) {
            return Err("heading task requires finite positive angle scale/cost cap, nonnegative weight and explicit reference offset".into());
        }
        Ok(Self {
            config,
            period_s,
            body,
            feet,
            active: None,
            qualified: 0,
            failed: 0,
        })
    }
    fn finish(&mut self, complete: bool) -> Result<Option<StepOutcome>, String> {
        let Some(active) = self.active.take() else {
            return Ok(None);
        };
        let first = active.samples.first().unwrap().time_s;
        let last = active.samples.last().unwrap().time_s;
        let lift = if last - first >= self.config.qualifying_duration_s {
            Some(evaluate_lift(
                &active.samples,
                &LiftRequirements {
                    start_s: first,
                    end_s: last,
                    maximum_sample_gap_s: self.period_s * (1. + 1e-8),
                    qualifying_duration_s: self.config.qualifying_duration_s,
                    swing_link: self.feet[active.foot].clone(),
                    minimum_clearance_m: self.config.minimum_clearance_m,
                    maximum_swing_force_n: self.config.maximum_swing_force_n,
                    minimum_support_forces_n: self
                        .feet
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i != active.foot)
                        .map(|(_, n)| (n.clone(), self.config.minimum_support_force_n))
                        .collect(),
                },
            )?)
        } else {
            None
        };
        let passed = complete && lift.as_ref().is_some_and(|r| r.passed);
        if passed {
            self.qualified += 1;
        } else {
            self.failed += 1;
        }
        Ok(Some(StepOutcome {
            step: active.step,
            foot: active.foot,
            complete,
            passed,
            lift,
        }))
    }
    /// Exactly one call at each task endpoint. Before the first controller
    /// sample, reset has no held reference and earns no reward.
    pub fn observe(
        &mut self,
        art: &Articulated,
        frame: &Value,
        elapsed_s: f64,
        horizon: bool,
    ) -> Result<WalkingObservation, String> {
        if elapsed_s == 0. {
            return Ok(WalkingObservation::default());
        }
        if (elapsed_s - self.period_s).abs() > 1e-10 {
            return Err("walking task requires its declared sampling period".into());
        }
        let reference = &frame["policy"]["step_reference"]["reference"];
        let sample = reference["sample"]
            .as_u64()
            .ok_or("missing held step reference")?;
        let target: [f64; 3] =
            serde_json::from_value(reference["body_world_m"].clone()).map_err(|e| e.to_string())?;
        let poses: Vec<LinkPose> =
            serde_json::from_value(frame["poses"].clone()).map_err(|e| e.to_string())?;
        let body = poses
            .iter()
            .find(|p| p.name == self.body)
            .ok_or("missing walking body pose")?;
        let mut error = [0.; 3];
        for i in 0..3 {
            error[i] = body.position_m[i] - target[i];
        }
        let body_cost = error
            .iter()
            .map(|v| (v / self.config.body_position_scale_m).powi(2))
            .sum::<f64>();
        if !body_cost.is_finite() {
            return Err("nonfinite walking body reward".into());
        }
        let body_reward = -body_cost.min(self.config.body_position_cost_cap)
            * self.config.body_position_weight_per_s
            * elapsed_s;
        if !body_reward.is_finite() {
            return Err("nonfinite walking body reward".into());
        }
        let heading = self
            .config
            .heading
            .as_ref()
            .map(|h| {
                let yaw = reference["yaw_rad"]
                    .as_f64()
                    .ok_or("missing held yaw reference")?;
                let x = body.rotation[0][0];
                let y = body.rotation[1][0];
                let target_rad = yaw + h.reference_offset_rad;
                if !target_rad.is_finite() || !x.is_finite() || !y.is_finite() || x.hypot(y) < 1e-12
                {
                    return Err("nonfinite or undefined world-Z body heading".to_string());
                }
                let actual_rad = y.atan2(x);
                let delta = actual_rad - target_rad;
                let error_rad = delta.sin().atan2(delta.cos());
                if !error_rad.is_finite() {
                    return Err("nonfinite heading error".to_string());
                }
                let reward =
                    -(error_rad / h.scale_rad).powi(2).min(h.cost_cap) * h.weight_per_s * elapsed_s;
                if !reward.is_finite() {
                    return Err("nonfinite heading reward".to_string());
                }
                Ok(HeadingObservation {
                    target_rad,
                    actual_rad,
                    error_rad,
                    reward,
                })
            })
            .transpose()?;
        let phase = reference["phase"].as_str().ok_or("missing step phase")?;
        let step = reference["step"].as_u64().ok_or("missing step id")?;
        let swinging = matches!(phase, "raise" | "lower");
        let mut outcome = None;
        if self
            .active
            .as_ref()
            .is_some_and(|a| !swinging || a.step != step)
        {
            let complete = self.active.as_ref().is_some_and(|a| a.step == step)
                && matches!(phase, "return" | "settle");
            outcome = self.finish(complete)?;
        }
        if swinging {
            let foot = reference["foot"]
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .filter(|i| *i < self.feet.len())
                .ok_or("invalid swing foot")?;
            let active = self.active.get_or_insert_with(|| ActiveSwing {
                step,
                foot,
                samples: vec![],
            });
            if active.foot != foot || active.samples.len() >= 1_000_000 {
                return Err("changed swing identity or unbounded swing history".into());
            }
            let clearance = sampled_floor_clearances(art, &poses, &[self.feet[foot].clone()])?[0]
                .minimum_clearance_m;
            let mut forces: BTreeMap<String, f64> =
                self.feet.iter().map(|n| (n.clone(), 0.)).collect();
            for c in frame["contacts"]
                .as_array()
                .ok_or("missing current contacts")?
            {
                if !c["other"].is_null() {
                    continue;
                }
                let link = c["link"]
                    .as_u64()
                    .and_then(|i| art.links.get(i as usize))
                    .ok_or("invalid floor contact link")?;
                if let Some(force) = forces.get_mut(&link.name) {
                    *force += c["force_n"][2].as_f64().ok_or("invalid floor force")?;
                }
            }
            active.samples.push(LiftSample {
                time_s: frame["time_s"].as_f64().ok_or("missing endpoint time")?,
                swing_clearance_m: clearance,
                floor_forces_n: forces,
            });
        }
        if horizon && self.active.is_some() {
            if outcome.is_some() {
                return Err("multiple swing outcomes at one endpoint".into());
            }
            outcome = self.finish(false)?;
        }
        let step_reward = outcome.as_ref().map_or(0., |o| {
            if o.passed {
                self.config.qualified_step_reward
            } else {
                -self.config.failed_step_penalty
            }
        });
        Ok(WalkingObservation {
            reference_sample: Some(sample),
            body_error_world_m: error,
            body_reward,
            step_reward,
            qualified_steps: self.qualified,
            failed_steps: self.failed,
            outcome,
            heading,
        })
    }
}
