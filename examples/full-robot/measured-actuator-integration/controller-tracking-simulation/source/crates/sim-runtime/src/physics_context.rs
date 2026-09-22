//! Compact, source-bound physics profiles for fixed-model trajectory forecasts.
//! Episode state, commands and reward are separate from physical parameters.
use crate::{
    embedded::{Config, EmbeddedRecording},
    session::Scene,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeIdentity {
    pub version: u32,
    pub library_source_blake3: String,
    pub features: Vec<String>,
}
impl RuntimeIdentity {
    pub fn current() -> Self {
        Self {
            version: 1,
            library_source_blake3: env!("SIM_RUNTIME_SOURCE_BLAKE3").into(),
            features: env!("SIM_RUNTIME_IDENTITY_FEATURES")
                .split(',')
                .filter(|x| !x.is_empty())
                .map(str::to_owned)
                .collect(),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || !digest(&self.library_source_blake3)
            || self.features.iter().any(|f| f.is_empty())
            || self.features.windows(2).any(|w| w[0] >= w[1])
        {
            return Err("invalid runtime source identity".into());
        }
        Ok(())
    }
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsContext {
    pub version: u32,
    pub runtime: RuntimeIdentity,
    /// BLAKE3 of versioned canonical typed JSON sections. Full definitions remain
    /// in the episode recording; these hashes are not an authenticated signature.
    pub sections: BTreeMap<String, String>,
}
impl PhysicsContext {
    pub fn from_runtime(scene: &Scene, config: &Config) -> Result<Self, String> {
        Self::from_definition(scene, config, RuntimeIdentity::current())
    }
    pub fn from_recording(record: &EmbeddedRecording) -> Result<Self, String> {
        if record.version != 3 || record.kind != "embedded_session" {
            return Err("physics binding requires an embedded v3 recording".into());
        }
        let runtime=record.runtime_identity.clone().ok_or("bound forecast requires recorded runtime source identity; replay legacy data to record it explicitly")?;
        Self::from_definition(&record.scene, &record.config, runtime)
    }
    fn from_definition(
        scene: &Scene,
        config: &Config,
        runtime: RuntimeIdentity,
    ) -> Result<Self, String> {
        runtime.validate()?;
        let mut sections = BTreeMap::new();
        let robot = scene.robot.to_json_value_checked()?;
        for (key, value) in robot.as_object().ok_or("invalid physical model")? {
            sections.insert(format!("/scene/robot/{}", escape(key)), fingerprint(value));
        }
        sections.insert(
            "/scene/version".into(),
            fingerprint(&serde_json::json!(scene.version)),
        );
        sections.insert(
            "/scene/options".into(),
            fingerprint(&serde_json::to_value(&scene.options).map_err(|e| e.to_string())?),
        );
        sections.insert(
            "/scene/period_s".into(),
            fingerprint(&serde_json::json!(scene.period_s)),
        );
        let mut config = serde_json::to_value(config).map_err(|e| e.to_string())?;
        let config = config
            .as_object_mut()
            .ok_or("invalid physics configuration")?;
        // Explicit exclusions are states, commanded references or diagnostics.
        // Any new configuration field is included by default.
        for field in [
            "policy",
            "motion_gate",
            "steps",
            "report_every",
            "profile_solver",
            "trace_trials",
            "audit_contact_steps",
            "initial_coordinates",
            "initial_base_translation_m",
            "initial_base_rotation_vector_rad",
        ] {
            config.remove(field);
        }
        if let Some(motors) = config.get_mut("motors").and_then(Value::as_object_mut) {
            motors.remove("target_trajectory");
            motors.remove("target_coordinates");
            if let Some(servos) = motors.get_mut("servos").and_then(Value::as_array_mut) {
                for servo in servos {
                    servo
                        .as_object_mut()
                        .ok_or("invalid servo boundary")?
                        .remove("target_rad");
                }
            }
        }
        for (key, value) in config {
            sections.insert(format!("/config/{}", escape(key)), fingerprint(value));
        }
        let context = Self {
            version: 1,
            runtime,
            sections,
        };
        context.validate()?;
        Ok(context)
    }
    pub fn validate(&self) -> Result<(), String> {
        self.runtime.validate()?;
        if self.version != 1
            || self.sections.is_empty()
            || self
                .sections
                .iter()
                .any(|(k, v)| !k.starts_with('/') || !digest(v))
        {
            return Err("invalid forecast physics context".into());
        }
        for required in [
            "/scene/version",
            "/scene/options",
            "/scene/period_s",
            "/scene/robot/version",
            "/scene/robot/gravity",
            "/scene/robot/links",
            "/config/step_s",
        ] {
            if !self.sections.contains_key(required) {
                return Err(format!("missing physics context section {required}"));
            }
        }
        Ok(())
    }
    pub fn matches(&self, actual: &Self) -> Result<(), String> {
        self.validate()?;
        actual.validate()?;
        if self.runtime != actual.runtime {
            return Err("forecast runtime source/features mismatch".into());
        }
        for key in self
            .sections
            .keys()
            .chain(actual.sections.keys())
            .collect::<BTreeSet<_>>()
        {
            if self.sections.get(key) != actual.sections.get(key) {
                return Err(format!("forecast physics context mismatch at {key}"));
            }
        }
        Ok(())
    }
}
fn escape(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

// Framed type-tagged encoding: sorted object keys, ordered arrays, exact integer
// values, IEEE-754 bits for other finite numbers. Normalize signed zero and
// integral floats within the exact JSON/JavaScript integer range. The framing
// avoids ambiguous concatenations; no locale, map iteration or host hash seed.
pub(crate) fn fingerprint(value: &Value) -> String {
    fn bytes(h: &mut blake3::Hasher, tag: &[u8], b: &[u8]) {
        h.update(tag);
        h.update(&(b.len() as u64).to_le_bytes());
        h.update(b);
    }
    fn feed(h: &mut blake3::Hasher, v: &Value) {
        match v {
            Value::Null => {
                h.update(b"n");
            }
            Value::Bool(b) => {
                h.update(if *b { b"t" } else { b"f" });
            }
            Value::String(s) => bytes(h, b"s", s.as_bytes()),
            Value::Number(n) => {
                if let Some(u) = n.as_u64() {
                    bytes(h, b"u", &u.to_le_bytes());
                } else if let Some(i) = n.as_i64() {
                    bytes(h, b"i", &i.to_le_bytes());
                } else {
                    let f = n.as_f64().unwrap();
                    if f.fract() == 0. && f.abs() <= 9007199254740991. {
                        if f >= 0. {
                            bytes(h, b"u", &(f as u64).to_le_bytes());
                        } else {
                            bytes(h, b"i", &(f as i64).to_le_bytes());
                        }
                    } else {
                        bytes(h, b"d", &f.to_bits().to_le_bytes());
                    }
                }
            }
            Value::Array(a) => {
                h.update(b"a");
                h.update(&(a.len() as u64).to_le_bytes());
                for v in a {
                    feed(h, v);
                }
            }
            Value::Object(o) => {
                h.update(b"o");
                h.update(&(o.len() as u64).to_le_bytes());
                for k in o.keys().collect::<BTreeSet<_>>() {
                    bytes(h, b"k", k.as_bytes());
                    feed(h, &o[k]);
                }
            }
        }
    }
    let mut h = blake3::Hasher::new();
    h.update(b"sim-runtime-physics-json-v1\0");
    feed(&mut h, value);
    h.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn canonical_encoding_preserves_units_order_values_and_json_transport_equivalence() {
        assert_eq!(fingerprint(&json!(0)), fingerprint(&json!(-0.)));
        assert_eq!(fingerprint(&json!(1)), fingerprint(&json!(1.)));
        assert_eq!(fingerprint(&json!(-1)), fingerprint(&json!(-1.)));
        assert_eq!(
            fingerprint(&json!({"a":1,"b":2})),
            fingerprint(&json!({"b":2,"a":1}))
        );
        for (a, b) in [
            (json!([1, 2]), json!([2, 1])),
            (json!("1"), json!(1)),
            (json!({}), json!([])),
            (json!(9007199254740992u64), json!(9007199254740993u64)),
            (json!("rad"), json!("m")),
            (json!(0.1), json!(0.10000000000000002)),
        ] {
            assert_ne!(fingerprint(&a), fingerprint(&b));
        }
    }
}
