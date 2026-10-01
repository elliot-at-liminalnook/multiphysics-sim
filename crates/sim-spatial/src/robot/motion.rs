//! Typed motion requests for a running robot preset: which three session input
//! channels are the planar motion commands, which (if any) is the packet
//! heartbeat, and what W/A/S/D request. A UI mapping only, ported from
//! `web/viewer/motion-commands.mjs`: the channels and their bounds come from
//! the running session (`inputs()` and its `policy_contract`) or the preset's
//! declaration in presets.json, and the preset's Rust controller turns the
//! requests into motion. Nothing here chooses a controller, gain or joint
//! target, and nothing is clamped: an out-of-bounds request is refused naming
//! the channel and its bounds.
use serde::Serialize;
use serde_json::{Value, json};
use sim_core::QuantityKind;
use sim_runtime::session::InputChannel;

pub const LABEL: &str = "motion request through the preset's Rust controller (not joint control)";
/// Motion keys in the order of the declared `motion_key_vectors`.
pub const KEYS: [char; 4] = ['w', 'a', 's', 'd'];
pub const STOP_KEY: &str = "X";
pub const KEY_SEMANTICS: &str = "Physical W/A/S/D follow press/release as in the browser: the held keys' request is sent on each press and release, and releasing the last key requests zero (losing keyboard focus releases every key). X is Stop (every motion channel to 0). A motion button (W · Forward, A · Left, S · Back, D · Right) latches that one key's request until Stop, another key or a channel request; a physical key press replaces a latched key. Robot mode's camera takes the mouse and the shared numpad camera keys, so W/A/S/D/X take no camera action; they are read only while a preset with a motion config is loaded and the gait path field does not have the keyboard.";
pub const CLAMP_RULE: &str = "never clamped: a requested value, a key's vector or the sum of held keys' vectors outside a channel's bounds is refused naming the channel and bounds (the browser clamps combined keys; the native viewer refuses instead)";
pub const HEARTBEAT_RULE: &str = "incremented by 1 before every action packet (each chunk the run thread advances: one EmbeddedEnvironment::step or one EmbeddedSession chunk), as the browser's nextMotionAction; the preset's Rust controller detects a lost link from an unchanged sequence. At its upper bound the next packet fails the run (reset the session) and motion requests are refused.";

/// One typed session input channel used for motion, with its index in the action.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct MotionChannel {
    pub name: String,
    pub index: usize,
    pub kind: Value,
    pub unit: String,
    pub lower: f64,
    pub upper: f64,
}
impl MotionChannel {
    fn new(index: usize, c: &InputChannel) -> Self {
        Self { name: c.name.clone(), index, kind: serde_json::to_value(&c.kind).unwrap_or(Value::Null), unit: c.kind.unit().to_string(), lower: c.lower, upper: c.upper }
    }
    pub fn bounds(&self) -> String {
        format!("[{}, {}] {}", self.lower, self.upper, self.unit)
    }
    /// A finite value within the channel's own bounds; never clamped.
    pub fn check(&self, value: f64, what: &str) -> Result<(), String> {
        if !value.is_finite() {
            return Err(format!("motion channel `{}`: {what} {value} is not finite (bounds {})", self.name, self.bounds()));
        }
        if value < self.lower || value > self.upper {
            return Err(format!("motion channel `{}`: {what} {value} is outside its bounds {}; not clamped", self.name, self.bounds()));
        }
        Ok(())
    }
}

/// Where the three motion channels come from.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub enum Source {
    /// The running session's own `policy_contract.step_reference.config.command_channels`.
    #[serde(rename = "policy_contract.step_reference.config")]
    PolicyContract,
    /// The preset's `motion_commands` in presets.json (three unique typed velocity channels containing zero).
    #[serde(rename = "presets.json motion_commands")]
    Preset,
}

/// A preset's motion config, resolved against its running session.
#[derive(Clone, Debug, Serialize)]
pub struct Motion {
    pub source: Source,
    /// Body-frame forward, lateral (m/s) and yaw rate (rad/s).
    pub channels: [MotionChannel; 3],
    /// The declared W/A/S/D vectors, in `KEYS` order; None: W/S request the
    /// forward channel's upper/lower bound and A/D the yaw channel's.
    pub key_vectors: Option<[[f64; 3]; 4]>,
    /// The session's step sequence settings (policy contract), or the browser's default.
    pub sequence: Value,
}

/// The packet heartbeat channel the preset declares (`motion_heartbeat`), as
/// the browser's motionHeartbeatIndex: a unique bounded integer Dimensionless
/// channel from 0, with an integer initial value below its upper bound, that
/// is not a motion command.
pub fn heartbeat(entry: &Value, inputs: &[InputChannel]) -> Result<Option<MotionChannel>, String> {
    let Some(name) = entry.get("motion_heartbeat") else { return Ok(None) };
    let fail = |why: &str| Err(format!("presets.json motion_heartbeat {name}: {why}; a heartbeat must be a unique bounded integer Dimensionless sequence channel (lower 0, integer upper ≥ 1, integer initial below upper) that is not a motion command"));
    let Some(name) = name.as_str() else { return fail("not a channel name") };
    let matches: Vec<usize> = inputs.iter().enumerate().filter(|(_, c)| c.name == name).map(|(i, _)| i).collect();
    let [i] = matches[..] else { return fail(&format!("{} session input channels have this name", matches.len())) };
    let c = &inputs[i];
    let safe = |x: f64| x.fract() == 0.0 && x.abs() <= 9007199254740991.0;
    let commands = entry.get("motion_commands").and_then(Value::as_array).is_some_and(|a| a.iter().any(|n| n.as_str() == Some(name)));
    if c.kind != QuantityKind::Dimensionless || c.lower != 0.0 || !safe(c.upper) || c.upper < 1.0 || !safe(c.initial) || c.initial < 0.0 || c.initial >= c.upper || commands {
        return fail(&format!("channel is {:?} [{}, {}] initial {}{}", c.kind, c.lower, c.upper, c.initial, if commands { ", and listed in motion_commands" } else { "" }));
    }
    Ok(Some(MotionChannel::new(i, c)))
}

/// The motion config, as the browser's motionCommandConfig: the session's own
/// `policy_contract.step_reference.config` first, else the preset's declared
/// `motion_commands`; None when neither exists. A declared config that does
/// not match the session's typed inputs is an error naming the declaration.
pub fn config(entry: &Value, policy_contract: &Value, inputs: &[InputChannel]) -> Result<Option<Motion>, String> {
    let unique = |name: &str| -> Result<usize, usize> {
        let m: Vec<usize> = inputs.iter().enumerate().filter(|(_, c)| c.name == name).map(|(i, _)| i).collect();
        if let [i] = m[..] { Ok(i) } else { Err(m.len()) }
    };
    let (source, names, sequence) = if let Some(c) = policy_contract.get("step_reference").and_then(|s| s.get("config")) {
        let names: Option<Vec<String>> = c.get("command_channels").and_then(Value::as_array).map(|a| a.iter().filter_map(|n| n.as_str().map(str::to_string)).collect());
        let names = names.filter(|n| n.len() == 3).ok_or("the session's policy_contract.step_reference.config has no three command_channels")?;
        (Source::PolicyContract, names, c.get("sequence").cloned().unwrap_or(Value::Null))
    } else if let Some(n) = entry.get("motion_commands") {
        let kinds = [QuantityKind::LinearVelocity, QuantityKind::LinearVelocity, QuantityKind::AngularVelocity];
        let names: Option<Vec<String>> = n.as_array().map(|a| a.iter().filter_map(|n| n.as_str().map(str::to_string)).collect());
        let bad = |why: String| format!("presets.json motion_commands {n}: {why}; motion controls require three unique typed velocity inputs (LinearVelocity, LinearVelocity, AngularVelocity) whose bounds contain zero");
        let names = names.filter(|v| v.len() == 3 && n.as_array().is_some_and(|a| a.len() == 3)).ok_or_else(|| bad("not three channel names".into()))?;
        if names.iter().collect::<std::collections::BTreeSet<_>>().len() != 3 {
            return Err(bad("names repeat".into()));
        }
        for (name, kind) in names.iter().zip(&kinds) {
            let i = unique(name).map_err(|n| bad(format!("{n} session input channels are named `{name}`")))?;
            let c = &inputs[i];
            if c.kind != *kind || c.lower > 0.0 || c.upper < 0.0 {
                return Err(bad(format!("`{name}` is {:?} [{}, {}]", c.kind, c.lower, c.upper)));
            }
        }
        (Source::Preset, names, json!({"update_command_before_lift": false}))
    } else {
        return Ok(None);
    };
    let mut channels = Vec::with_capacity(3);
    for name in &names {
        let i = unique(name).map_err(|n| format!("motion command channel `{name}` ({source:?}): {n} session input channels have this name"))?;
        channels.push(MotionChannel::new(i, &inputs[i]));
    }
    let channels: [MotionChannel; 3] = channels.try_into().expect("three channels");
    let key_vectors = match entry.get("motion_key_vectors") {
        None => None,
        Some(v) => {
            let bad = |why: String| format!("presets.json motion_key_vectors: {why}; W/A/S/D vectors require four finite, typed, in-range motion requests");
            let map = v.as_object().ok_or_else(|| bad("not an object".into()))?;
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            if keys != ["a", "d", "s", "w"] {
                return Err(bad(format!("keys {keys:?} are not exactly w, a, s, d")));
            }
            let mut out = [[0.0; 3]; 4];
            for (k, key) in KEYS.iter().enumerate() {
                let v = map[&key.to_string()].as_array().filter(|a| a.len() == 3).ok_or_else(|| bad(format!("`{key}` is not three numbers")))?;
                for (i, c) in channels.iter().enumerate() {
                    let x = v[i].as_f64().ok_or_else(|| bad(format!("`{key}`[{i}] is not a number")))?;
                    c.check(x, &format!("key `{key}` vector component")).map_err(bad)?;
                    out[k][i] = x;
                }
            }
            Some(out)
        }
    };
    Ok(Some(Motion { source, channels, key_vectors, sequence }))
}

impl Motion {
    /// The request of the held `keys`, as the browser's driveMotionValues but
    /// refused (naming the channel, its bounds and the keys) rather than
    /// clamped when the summed vectors leave a channel's bounds.
    pub fn keys(&self, keys: &[char]) -> Result<[f64; 3], String> {
        if let Some(k) = keys.iter().find(|k| !KEYS.contains(k)) {
            return Err(format!("unknown motion key `{k}`; keys: w, a, s, d (stop is its own request)"));
        }
        let has = |k: char| keys.contains(&k);
        let values = match &self.key_vectors {
            None => {
                let dir = [has('w') as i8 - has('s') as i8, 0, has('a') as i8 - has('d') as i8];
                std::array::from_fn(|i| match dir[i] {
                    1 => self.channels[i].upper,
                    -1 => self.channels[i].lower,
                    _ => 0.0,
                })
            }
            Some(v) => std::array::from_fn(|i| KEYS.iter().enumerate().filter(|(_, k)| has(**k)).map(|(k, _)| v[k][i]).sum()),
        };
        let held: String = KEYS.iter().filter(|k| has(**k)).map(|k| k.to_string()).collect::<Vec<_>>().join("+");
        for (c, x) in self.channels.iter().zip(values) {
            c.check(x, &format!("keys {} request", if held.is_empty() { "(none)".into() } else { held.clone() }))?;
        }
        Ok(values)
    }
    /// Explicit requests by channel name, over the currently requested values.
    /// Unknown and non-motion channels are refused by name.
    pub fn values(&self, current: [f64; 3], requests: &[(String, f64)], inputs: &[InputChannel]) -> Result<[f64; 3], String> {
        let mut out = current;
        let names = || self.channels.iter().map(|c| format!("`{}` {}", c.name, c.bounds())).collect::<Vec<_>>().join(", ");
        for (name, value) in requests {
            let Some(i) = self.channels.iter().position(|c| &c.name == name) else {
                return Err(match inputs.iter().find(|c| &c.name == name) {
                    Some(c) => format!("channel `{name}` [{}, {}] is a session input but not a motion command channel; motion channels: {}", c.lower, c.upper, names()),
                    None => format!("unknown channel `{name}`: not a session input; motion channels: {}", names()),
                });
            };
            self.channels[i].check(*value, "requested value")?;
            out[i] = *value;
        }
        Ok(out)
    }
    /// Stop: every motion channel to zero (each channel's bounds are checked; never clamped).
    pub fn stop(&self) -> Result<[f64; 3], String> {
        for c in &self.channels {
            c.check(0.0, "stop request")?;
        }
        Ok([0.0; 3])
    }
    pub fn json(&self) -> Value {
        let vectors = self.key_vectors.map(|v| json!({"w": v[0], "a": v[1], "s": v[2], "d": v[3]}));
        json!({"source": self.source, "channels": self.channels, "key_vectors": vectors,
            "key_rule": if self.key_vectors.is_some() { "the held keys' declared vectors (presets.json motion_key_vectors) summed per channel" } else { "no motion_key_vectors: W/S request the forward channel's upper/lower bound, A/D the yaw channel's upper/lower bound, lateral 0 (as the browser)" },
            "sequence": self.sequence})
    }
}

/// Advances the heartbeat of one action packet in `action`, as the browser's
/// nextMotionAction; refused when the sequence is exhausted or invalid.
pub fn next_packet(heartbeat: Option<&MotionChannel>, action: &mut [f64]) -> Result<(), String> {
    let Some(h) = heartbeat else { return Ok(()) };
    let x = action.get(h.index).copied().unwrap_or(f64::NAN);
    if !(x.fract() == 0.0 && x >= 0.0 && x < h.upper) {
        return Err(format!("motion packet sequence `{}` exhausted or invalid at {x} (bounds [{}, {}]); reset the session", h.name, h.lower, h.upper));
    }
    action[h.index] = x + 1.0;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn channel(name: &str, kind: QuantityKind, lower: f64, upper: f64) -> InputChannel {
        InputChannel { name: name.into(), kind, lower, upper, initial: 0.0 }
    }

    /// The robot-measured-400hz declaration (presets.json) against the scene
    /// file's real channel bounds; parsing the scene needs no build.
    #[test]
    fn declared_key_vectors_sum_per_channel_and_refuse_out_of_bounds_sums() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let preset = crate::robot::preset::select(&root.join(crate::robot::preset::PRESETS), &root, "robot-measured-400hz").unwrap();
        let text = std::fs::read_to_string(root.join(preset.scene.as_ref().unwrap())).unwrap();
        let scene: sim_runtime::session::Scene = serde_json::from_str(&text).unwrap();
        let inputs = &scene.controller.as_ref().expect("controller").inputs;
        let m = config(&preset.entry, &Value::Null, inputs).unwrap().expect("declared motion_commands");
        assert_eq!(m.source, Source::Preset);
        assert_eq!(m.channels.each_ref().map(|c| c.name.as_str()), ["command.forward_speed", "command.lateral_speed", "command.yaw_rate"]);
        assert_eq!(m.keys(&['w']).unwrap(), [0.1, 0.0, 0.0]);
        let wa = m.keys(&['w', 'a']).unwrap();
        assert!((wa[0] - 0.15).abs() < 1e-12 && wa[1] == 0.0 && (wa[2] - 0.15).abs() < 1e-12, "{wa:?}");
        assert_eq!(m.keys(&[]).unwrap(), [0.0; 3]);
        let hb = heartbeat(&preset.entry, inputs).unwrap().expect("declared heartbeat");
        assert_eq!(hb.name, "command.packet_sequence");
        // A tighter forward bound (in memory) makes W+A's summed 0.15 leave it: refused, not clamped.
        let mut tight = inputs.clone();
        tight.iter_mut().find(|c| c.name == "command.forward_speed").unwrap().upper = 0.12;
        let m = config(&preset.entry, &Value::Null, &tight).unwrap().unwrap();
        let e = m.keys(&['w', 'a']).unwrap_err();
        assert!(e.contains("`command.forward_speed`") && e.contains("[-0.8, 0.12]") && e.contains("w+a") && e.contains("not clamped"), "{e}");
        // Heartbeat: one increment per packet, refused when exhausted.
        let mut action = vec![0.0; inputs.len()];
        next_packet(Some(&hb), &mut action).unwrap();
        assert_eq!(action[hb.index], 1.0);
        action[hb.index] = hb.upper;
        assert!(next_packet(Some(&hb), &mut action).unwrap_err().contains("exhausted"));
    }

    #[test]
    fn without_key_vectors_keys_request_the_channel_bounds() {
        let inputs = vec![
            channel("gain", QuantityKind::Dimensionless, 0.0, 1.0),
            channel("vx", QuantityKind::LinearVelocity, -0.2, 0.3),
            channel("vy", QuantityKind::LinearVelocity, 0.0, 0.0),
            channel("wz", QuantityKind::AngularVelocity, -1.0, 0.5),
        ];
        let entry = json!({"motion_commands": ["vx", "vy", "wz"]});
        let m = config(&entry, &Value::Null, &inputs).unwrap().unwrap();
        assert_eq!(m.keys(&['w', 'd']).unwrap(), [0.3, 0.0, -1.0]);
        assert_eq!(m.keys(&['w', 's', 'a']).unwrap(), [0.0, 0.0, 0.5]);
        let e = m.values([0.0; 3], &[("gain".into(), 0.5)], &inputs).unwrap_err();
        assert!(e.contains("`gain`") && e.contains("not a motion command channel"), "{e}");
        let e = m.values([0.0; 3], &[("vz".into(), 0.1)], &inputs).unwrap_err();
        assert!(e.contains("unknown channel `vz`"), "{e}");
        assert!(m.values([0.0; 3], &[("vx".into(), f64::INFINITY)], &inputs).unwrap_err().contains("not finite"));
        // A declared yaw channel of the wrong kind is refused naming the declaration.
        let bad = json!({"motion_commands": ["vx", "vy", "gain"]});
        assert!(config(&bad, &Value::Null, &inputs).unwrap_err().contains("`gain`"));
        assert!(config(&json!({}), &Value::Null, &inputs).unwrap().is_none());
    }
}
