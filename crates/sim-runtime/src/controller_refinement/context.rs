//! Immutable experiment setup snapshots. Missing physical facts stay explicit.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Measured,
    Derived,
    Estimated,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Property {
    pub name: String,
    pub value: Option<f64>,
    pub unit: String,
    pub coordinate_frame: String,
    pub origin: Origin,
    pub source: String,
    /// Absolute lower/upper bounds in the declared unit; None means unestablished.
    pub uncertainty_bounds: Option<[f64; 2]>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub role: String,
    pub location: String,
    pub blake3: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub hardware_id: u8,
    pub cad_component_id: String,
    pub joint_id: Option<String>,
    pub source: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptureContext {
    pub recording_hash: String,
    pub fixture: String,
    pub attached_output_hardware: String,
    pub transmission: String,
    pub properties: Vec<Property>,
    pub artifacts: Vec<Artifact>,
    pub bindings: Vec<Binding>,
    pub limitations: Vec<String>,
}
impl CaptureContext {
    pub fn unknown(recording: &super::recording::Recording) -> Self {
        Self { recording_hash:recording.fingerprint(), fixture:recording.experiment.fixture.clone(),
            attached_output_hardware:"Not documented".into(), transmission:"Internal servo transmission; detailed properties unmeasured".into(),
            properties:[("attached_mass","kg"),("attached_inertia","kg*m^2"),("external_torque","N*m"),("initial_joint_angle","rad"),("encoder_accuracy","rad")].into_iter().map(|(name,unit)|Property {
                name:name.into(),value:None,unit:unit.into(),coordinate_frame:"Not established".into(),origin:Origin::Unknown,source:"No independent measurement supplied".into(),uncertainty_bounds:None,
            }).collect(),artifacts:vec![],bindings:vec![],limitations:vec!["Unloaded does not mean zero attached or rotor inertia. Internal encoder angle does not establish the joint coordinate frame.".into()] }
    }
    pub fn validate(&self) -> Result<(), String> {
        fn hash(s: &str) -> bool {
            s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit())
        }
        if !hash(&self.recording_hash)
            || self.fixture.trim().is_empty()
            || self.attached_output_hardware.trim().is_empty()
            || self.transmission.trim().is_empty()
        {
            return Err(
                "Context requires recording identity and explicit fixture descriptions".into(),
            );
        }
        let mut names = std::collections::BTreeSet::new();
        for p in &self.properties {
            if p.name.trim().is_empty()
                || !names.insert(&p.name)
                || p.unit.trim().is_empty()
                || p.coordinate_frame.trim().is_empty()
                || p.source.trim().is_empty()
                || (p.origin == Origin::Unknown) != p.value.is_none()
                || p.value.is_some_and(|v| !v.is_finite())
            {
                return Err("Setup properties require unique names, units, frames, sources and honest known/unknown values".into());
            }
            if let Some([lo, hi]) = p.uncertainty_bounds {
                if !lo.is_finite()
                    || !hi.is_finite()
                    || lo > hi
                    || !p.value.is_some_and(|v| lo <= v && v <= hi)
                {
                    return Err("Uncertainty bounds must contain the declared value".into());
                }
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        for b in &self.bindings {
            if !(1..=253).contains(&b.hardware_id)
                || !ids.insert(b.hardware_id)
                || b.cad_component_id.trim().is_empty()
                || b.source.trim().is_empty()
            {
                return Err(
                    "Hardware bindings require unique IDs and explicit stable CAD identity/source"
                        .into(),
                );
            }
        }
        for a in &self.artifacts {
            if a.role.trim().is_empty() || a.location.trim().is_empty() || !hash(&a.blake3) {
                return Err(
                    "Setup artifact requires a role, durable location and content hash".into(),
                );
            }
        }
        Ok(())
    }
}
