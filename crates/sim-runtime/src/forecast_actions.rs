//! Typed held controller commands for trajectory prediction. Commands remain
//! inputs to the existing controller; their units do not imply a physical actuator.
use crate::{embedded::EmbeddedRecording, session::InputChannel};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Executable command interpretation, separate from CAD mechanics. A model
/// conditioned on controller inputs must not silently reuse a different policy,
/// reference trajectory, initial reference or controller clock.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerContext {
    pub program: crate::session::ControllerProgram,
    pub policy: crate::embedded_policy::PolicyConfig,
    pub period_s: f64,
    pub motion_gate: Option<crate::embedded::MotionGate>,
    pub target_trajectory: Option<sim_domain_control::trajectory::TrajectoryConfig>,
    pub initial_targets_rad: Vec<f64>,
}
impl std::fmt::Debug for ControllerContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControllerContext")
            .field("period_s", &self.period_s)
            .finish_non_exhaustive()
    }
}
impl ControllerContext {
    pub fn from_runtime(
        scene: &crate::session::Scene,
        config: &crate::embedded::Config,
    ) -> Result<Self, String> {
        let motors = config
            .motors
            .as_ref()
            .ok_or("missing controller actuator configuration")?;
        Ok(Self {
            program: scene
                .controller
                .clone()
                .ok_or("missing controller program")?,
            policy: config
                .policy
                .clone()
                .ok_or("missing sampled controller policy")?,
            period_s: scene.period_s,
            motion_gate: config.motion_gate.clone(),
            target_trajectory: motors.target_trajectory.clone(),
            initial_targets_rad: motors
                .servos
                .as_ref()
                .ok_or("missing controller initial targets")?
                .iter()
                .map(|s| s.target_rad)
                .collect(),
        })
    }
    pub fn validate(&self, channels: &[InputChannel]) -> Result<(), String> {
        bind(channels, &self.program.inputs)?;
        if !self.period_s.is_finite()
            || self.period_s <= 0.
            || self.initial_targets_rad.is_empty()
            || self.initial_targets_rad.iter().any(|v| !v.is_finite())
        {
            return Err("invalid forecast controller context".into());
        }
        Ok(())
    }
    pub fn matches(&self, actual: &Self) -> Result<(), String> {
        // Input order is handled by named binding; all other interpretation
        // parameters must match exactly. Existing typed definitions are reused.
        bind(&self.program.inputs, &actual.program.inputs)?;
        let mut expected = serde_json::to_value(self).map_err(|e| e.to_string())?;
        let mut actual = serde_json::to_value(actual).map_err(|e| e.to_string())?;
        expected["program"]["inputs"] = serde_json::json!([]);
        actual["program"]["inputs"] = serde_json::json!([]);
        if expected != actual {
            return Err("forecast controller program/policy/reference context mismatch".into());
        }
        Ok(())
    }
}

/// The recipe may reorder inputs, but must describe the complete runtime action
/// contract. This avoids conditioning on only some independently changing inputs.
pub fn bind(recipe: &[InputChannel], runtime: &[InputChannel]) -> Result<Vec<usize>, String> {
    validate(recipe)?;
    validate(runtime)?;
    if recipe.len() != runtime.len() {
        return Err("forecast requires the complete controller input contract".into());
    }
    recipe
        .iter()
        .map(|c| {
            let i = runtime
                .iter()
                .position(|r| r.name == c.name)
                .ok_or("unknown forecast controller input")?;
            let r = &runtime[i];
            if c.kind != r.kind
                || c.lower != r.lower
                || c.upper != r.upper
                || c.initial != r.initial
            {
                return Err(format!(
                    "forecast controller input units/bounds/initial mismatch: {}",
                    c.name
                ));
            }
            Ok(i)
        })
        .collect()
}
pub fn validate(channels: &[InputChannel]) -> Result<(), String> {
    let mut names = std::collections::BTreeSet::new();
    if channels.is_empty() {
        return Err("empty forecast controller input contract".into());
    }
    for c in channels {
        if c.name.trim().is_empty()
            || !names.insert(&c.name)
            || ![c.lower, c.upper, c.initial].iter().all(|x| x.is_finite())
            || c.lower > c.upper
            || c.initial < c.lower
            || c.initial > c.upper
        {
            return Err("invalid forecast controller input declaration".into());
        }
    }
    Ok(())
}
pub fn validate_values(channels: &[InputChannel], values: &[f64]) -> Result<(), String> {
    if values.len() != channels.len()
        || values
            .iter()
            .zip(channels)
            .any(|(v, c)| !v.is_finite() || *v < c.lower || *v > c.upper)
    {
        return Err("forecast controller action shape/value outside declared input bounds".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ControllerActionSequence {
    /// All values use these channels' SI quantities, in this exact order.
    pub channels: Vec<InputChannel>,
    pub previous: Vec<f64>,
    pub future: Vec<Vec<f64>>,
}

/// Reconstruct the same sparse held-input schedule used by runtime replay.
/// Refuse changes hidden inside a forecast interval: a single command vector
/// cannot describe such an interval. Optional endpoint values are cross-checked.
pub fn from_recording(
    record: &EmbeddedRecording,
    frames: &[Value],
    channels: &[InputChannel],
) -> Result<Vec<Vec<f64>>, String> {
    let runtime = &record
        .scene
        .controller
        .as_ref()
        .ok_or("missing recorded controller")?
        .inputs;
    let indices = bind(channels, runtime)?;
    let step = record.config.step_s;
    if record.version != 3
        || record.kind != "embedded_session"
        || record.failure.is_some()
        || !step.is_finite()
        || step <= 0.
        || frames.len() < 2
        || record.completed_steps > record.config.steps
    {
        return Err("invalid controller forecast recording".into());
    }
    for (i, e) in record.input_events.iter().enumerate() {
        validate_values(runtime, &e.values)?;
        if e.at_step >= record.completed_steps
            || (i > 0 && record.input_events[i - 1].at_step >= e.at_step)
        {
            return Err("invalid recorded controller action schedule".into());
        }
    }
    let index = |frame: &Value| -> Result<usize, String> {
        let t = frame["time_s"]
            .as_f64()
            .ok_or("missing forecast frame time")?;
        let n = t / step;
        if !n.is_finite()
            || n < 0.
            || n > record.completed_steps as f64 + 1e-7
            || (n - n.round()).abs() > 1e-7
        {
            return Err("forecast frame is not on the recorded physics clock".into());
        }
        Ok(n.round() as usize)
    };
    let mut held = runtime.iter().map(|c| c.initial).collect::<Vec<_>>();
    let mut events = record.input_events.iter().peekable();
    let mut actions = Vec::new();
    for pair in frames.windows(2) {
        let start = index(&pair[0])?;
        let end = index(&pair[1])?;
        if start >= end {
            return Err("non-increasing forecast frame clock".into());
        }
        while events.peek().is_some_and(|e| e.at_step <= start) {
            held = events.next().unwrap().values.clone();
        }
        if events.peek().is_some_and(|e| e.at_step < end) {
            return Err("controller input changes inside a forecast interval; use a finer observation period".into());
        }
        if let Some(raw) = pair[1].get("policy_inputs") {
            let observed: Vec<f64> =
                serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
            if observed != held {
                return Err(
                    "endpoint controller inputs disagree with recorded action schedule".into(),
                );
            }
        }
        actions.push(indices.iter().map(|i| held[*i]).collect());
    }
    Ok(actions)
}
