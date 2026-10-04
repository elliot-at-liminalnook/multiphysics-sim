//! A preset's session inputs set directly: the browser's input sliders
//! (`viewer.js` makeInputs: one range per typed input channel, the motion
//! heartbeat hidden, the `residual.*` motor corrections grouped with a clear
//! button), the inspector's Inputs block and REST `robot_inputs`. Values are
//! validated here against the built session's typed channels (bounds named,
//! never clamped) and merged into the held action on the run thread, which
//! validates them again through the session's own setter.
use serde_json::{Value, json};
use std::collections::BTreeMap;
use super::protocol::Command;
use super::{Phase, RunController};

/// The prefix of the motor-correction inputs the browser groups ("Motor corrections").
pub const RESIDUAL_PREFIX: &str = "residual.";
pub const INPUTS_RULE: &str = "each value sets one named session input of the held action (the session's own InputChannel bounds, refused outside them, never clamped); the motion heartbeat is the controller's packet sequence and is not set by hand; a motion channel set here also becomes the motion request (as the browser's sliders and WASD share one value). Applied on the run thread through the session's validating setter (EmbeddedSession::set_inputs, the environment's held action, Session's held action), republished at once while paused; held until changed, Reset or a replay.";

impl RunController {
    /// The (index, value) pairs `values` names, or why they are refused now:
    /// a recorded preset, a `--robot FILE`, a replay holding the run, a failed
    /// or ended run, no built session, an unknown or heartbeat channel, or a
    /// value outside the channel's bounds.
    pub fn check_inputs(&self, values: &BTreeMap<String, f64>) -> Result<Vec<(usize, f64)>, String> {
        self.recorded_refusal("an input change")?;
        let Some(p) = &self.preset else {
            return Err("session inputs are for robot presets (their declared controller's typed inputs); a `--robot FILE` has servo-target jog or, with a controller binding, drive requests".into());
        };
        let id = &p.preset.id;
        if values.is_empty() {
            return Err("robot_inputs needs at least one input value by name".into());
        }
        if let Some(why) = self.replay_block() {
            return Err(format!("preset `{id}`: input change refused: {why}"));
        }
        match self.status.phase {
            Phase::Failed => return Err(format!("preset `{id}`: the run failed; Reset rebuilds the session before inputs change")),
            Phase::Ended => return Err(format!("preset `{id}`: the run ended; Reset rebuilds at t = 0 before inputs change")),
            _ => {}
        }
        let drive = self.drive.as_ref().ok_or_else(|| format!("preset `{id}`: no built session yet; opening, Run or Step builds {} and lists its inputs", p.kind()))?;
        let heartbeat = drive.heartbeat.as_ref().map(|h| h.index);
        values
            .iter()
            .map(|(name, x)| {
                let (i, c) = drive.inputs.iter().enumerate().find(|(_, c)| c.name == *name).ok_or_else(|| {
                    let names: Vec<&str> = drive.inputs.iter().enumerate().filter(|(i, _)| Some(*i) != heartbeat).map(|(_, c)| c.name.as_str()).collect();
                    format!("preset `{id}` has no input `{name}`; its inputs: {}", names.join(", "))
                })?;
                if Some(i) == heartbeat {
                    return Err(format!("input `{name}` is the motion heartbeat (the controller's packet sequence); it advances once per action packet and is not set by hand"));
                }
                if !(x.is_finite() && *x >= c.lower && *x <= c.upper) {
                    return Err(format!("input `{name}` = {x} is outside its bounds [{}, {}] (refused, not clamped)", c.lower, c.upper));
                }
                Ok((i, *x))
            })
            .collect()
    }
    /// The one input handler behind the Inputs block's sliders and clear
    /// button, `system_ui` and REST `robot_inputs` (INPUTS_RULE).
    pub fn set_inputs_named(&mut self, values: &BTreeMap<String, f64>) -> Result<(), String> {
        let pairs = match self.check_inputs(values) {
            Ok(p) => p,
            Err(e) => {
                self.motion_refusal = Some(e.clone());
                return Err(e);
            }
        };
        // A motion channel set here is the motion request too.
        if let Some(m) = self.drive.as_ref().and_then(|d| d.motion.as_ref()) {
            let held = |i: usize| self.frame.as_ref().and_then(|f| f.inputs.get(i).copied()).unwrap_or(0.0);
            let mut requested = self.requested.unwrap_or_else(|| std::array::from_fn(|k| held(m.channels[k].index)));
            let mut touched = false;
            for (i, x) in &pairs {
                if let Some(k) = m.channels.iter().position(|c| c.index == *i) {
                    requested[k] = *x;
                    touched = true;
                }
            }
            if touched {
                self.requested = Some(requested);
                self.keys.clear();
                self.keys_physical = false;
            }
        }
        self.thread.send(Command::Inputs { values: pairs }).map_err(|_| "the run thread has stopped".to_string())?;
        self.motion_refusal = None;
        Ok(())
    }
    /// The whole held action by index (a tested recipe's step-0 inputs), applied
    /// on the run thread after the build (validated there by the session's setter).
    pub fn apply_action(&mut self, values: Vec<f64>) {
        let _ = self.thread.send(Command::Inputs { values: values.into_iter().enumerate().collect() });
    }
    /// Build the session now if none is built (sent when a preset opens).
    pub fn prepare(&self) {
        let _ = self.thread.send(Command::Prepare);
    }
    /// `robot_state.inputs`: every typed input of the built session (name,
    /// kind, unit, bounds, initial, held now), which one is the heartbeat,
    /// the residual group, availability and the rule; null without a preset.
    pub fn inputs_json(&self) -> Value {
        let Some(_) = &self.preset else { return Value::Null };
        let Some(drive) = &self.drive else {
            return json!({"available": false, "reason": "no built session yet (opening, Run or Step builds it)", "rule": INPUTS_RULE});
        };
        let heartbeat = drive.heartbeat.as_ref().map(|h| h.index);
        let held = |i: usize| self.frame.as_ref().and_then(|f| f.inputs.get(i).copied());
        let channels: Vec<Value> = drive.inputs.iter().enumerate().map(|(i, c)| {
            json!({"index": i, "name": c.name, "kind": c.kind, "lower": c.lower, "upper": c.upper, "initial": c.initial, "held": held(i),
                "heartbeat": Some(i) == heartbeat, "residual": c.name.starts_with(RESIDUAL_PREFIX)})
        }).collect();
        let probe: BTreeMap<String, f64> = drive.inputs.iter().enumerate().filter(|(i, _)| Some(*i) != heartbeat).map(|(i, c)| (c.name.clone(), held(i).unwrap_or(c.initial))).take(1).collect();
        let available = if probe.is_empty() { Err("the session has no settable inputs".to_string()) } else { self.check_inputs(&probe).map(|_| ()) };
        json!({"available": available.is_ok(), "unavailable_reason": available.err(), "channels": channels, "residual_prefix": RESIDUAL_PREFIX,
            "last_refusal": self.motion_refusal, "last_apply_error": self.motion_error, "rule": INPUTS_RULE})
    }
}
