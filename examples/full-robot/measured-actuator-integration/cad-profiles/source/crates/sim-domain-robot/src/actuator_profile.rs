//! CAD-owned physical families and explicit unit bindings. Registry declarations
//! own parameter names, units and bounds. A profile is not calibration approval.
use crate::{
    model::PhysicalModel,
    motor::{H_BRIDGE, MOTOR_UNIT},
};
use serde::{Deserialize, Serialize};
use sim_core::BehaviorRegistry;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub path: String,
    pub sha256: String,
    pub scope: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    Measured,
    Derived,
    Estimated,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub value: f64,
    pub unit: String,
    pub provenance: Provenance,
    /// Standard uncertainty in the same units; null means unknown, not zero.
    pub uncertainty: Option<f64>,
    pub evidence: String,
}
impl Parameter {
    fn validate(&self, unit: &str, evidence: &BTreeMap<String, Evidence>) -> Result<(), String> {
        if !self.value.is_finite()
            || self.unit != unit
            || !evidence.contains_key(&self.evidence)
            || self.uncertainty.is_some_and(|v| !v.is_finite() || v < 0.)
        {
            return Err(format!(
                "Parameter needs finite value, unit {unit}, evidence and nonnegative uncertainty"
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedPd {
    pub gains: sim_domain_control::fixed_pd::Gains,
    pub period: Parameter,
    pub latency: Parameter,
    pub encoder_quantum: Parameter,
    pub evidence: String,
    pub implementation_blake3: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    pub version: u32,
    pub description: String,
    /// Positive gearbox output rotation relative to its housing. The CAD joint
    /// axis and external transmissions carry the assembly coordinate mapping.
    pub shaft_coordinate: String,
    /// All profiles remain provisional until a separately verified scoped
    /// acceptance report is attached by the calibration workflow.
    pub limitations: Vec<String>,
    pub evidence: BTreeMap<String, Evidence>,
    pub motor: BTreeMap<String, Parameter>,
    pub driver: BTreeMap<String, Parameter>,
    pub controller: FixedPd,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub family: String,
    pub version: u32,
    pub physical_unit: Option<String>,
    /// Additive offsets, keyed motor.NAME or driver.NAME. They use the family
    /// evidence namespace and the exact registry units; bounds apply after addition.
    pub deviations: BTreeMap<String, Parameter>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profiles {
    pub version: u32,
    pub families: BTreeMap<String, Family>,
    /// Stable CAD motor IDs; never names, bus order or inferred joint positions.
    pub bindings: BTreeMap<String, Binding>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Resolved {
    pub family: String,
    pub version: u32,
    pub physical_unit: Option<String>,
    pub motor: BTreeMap<String, f64>,
    pub driver: BTreeMap<String, f64>,
    pub controller: FixedPd,
}
fn digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
fn physical(name: &str) -> bool {
    !["jacobian.", "dynamics.", "initial."]
        .iter()
        .any(|p| name.starts_with(p))
        && name != "backlash.events"
}
fn parameters(
    kind: &str,
    values: &BTreeMap<String, Parameter>,
    evidence: &BTreeMap<String, Evidence>,
    registry: &BehaviorRegistry,
) -> Result<BTreeMap<String, f64>, String> {
    let descriptor = registry.get(&kind.into()).map_err(|e| e.to_string())?;
    let declarations = descriptor
        .parameters
        .as_ref()
        .ok_or("Missing registry parameters")?;
    let expected: BTreeSet<_> = declarations
        .iter()
        .filter(|p| physical(&p.name))
        .map(|p| &p.name)
        .collect();
    if values.keys().collect::<BTreeSet<_>>() != expected {
        return Err(format!(
            "{kind}: explicitly declare every physical registry parameter; integration options are not motor properties"
        ));
    }
    for d in declarations.iter().filter(|p| physical(&p.name)) {
        values[&d.name].validate(&d.unit, evidence)?;
    }
    let result = values.iter().map(|(k, p)| (k.clone(), p.value)).collect();
    descriptor
        .validate_parameters(&result)
        .map_err(|e| e.to_string())?;
    Ok(result)
}
impl Profiles {
    pub fn resolve(
        &self,
        model: &PhysicalModel,
        registry: &BehaviorRegistry,
    ) -> Result<BTreeMap<String, Resolved>, String> {
        if self.version != 1 || self.families.is_empty() || self.bindings.is_empty() {
            return Err("Actuator profiles require version 1, families and bindings".into());
        }
        let mut bases = BTreeMap::new();
        for (name, f) in &self.families {
            if f.shaft_coordinate != "motor_output_relative_to_housing" {
                return Err("Declare the motor output coordinate; external joint/transmission frames stay in CAD".into());
            }
            if name.trim().is_empty()
                || f.version == 0
                || f.description.trim().is_empty()
                || f.limitations.is_empty()
                || f.limitations.iter().any(|s| s.trim().is_empty())
                || f.evidence.is_empty()
            {
                return Err(
                    "Family requires identity, version, description, limitations and evidence"
                        .into(),
                );
            }
            for (id, e) in &f.evidence {
                if id.trim().is_empty()
                    || e.path.trim().is_empty()
                    || !digest(&e.sha256)
                    || e.scope.trim().is_empty()
                {
                    return Err("Evidence requires identity, path, SHA-256 and tested scope".into());
                }
            }
            f.controller.gains.validate()?;
            f.controller.period.validate("s", &f.evidence)?;
            f.controller.latency.validate("s", &f.evidence)?;
            f.controller.encoder_quantum.validate("rad", &f.evidence)?;
            if f.controller.period.value <= 0.
                || f.controller.latency.value < 0.
                || f.controller.encoder_quantum.value <= 0.
                || !digest(&f.controller.implementation_blake3)
                || !f.evidence.contains_key(&f.controller.evidence)
            {
                return Err(
                    "Invalid fixed-PD timing, quantization or implementation identity".into(),
                );
            }
            bases.insert(
                name,
                (
                    parameters(MOTOR_UNIT, &f.motor, &f.evidence, registry)?,
                    parameters(H_BRIDGE, &f.driver, &f.evidence, registry)?,
                ),
            );
        }
        let mut units = BTreeSet::new();
        let mut result = BTreeMap::new();
        for (id, b) in &self.bindings {
            let motors: Vec<_> = model.motors.iter().filter(|m| &m.id == id).collect();
            if id.trim().is_empty() || motors.len() != 1 || motors[0].joint.is_none() {
                return Err(format!(
                    "Profile binding {id} requires exactly one CAD motor with a joint"
                ));
            }
            if !motors[0].gear_ratio.is_finite() || motors[0].gear_ratio <= 0. {
                return Err("CAD extra gearbox ratio must be finite and positive".into());
            }
            let joint = motors[0].joint.as_ref().unwrap();
            if model.joints.iter().filter(|j| &j.name == joint).count() != 1 {
                return Err("Profile motor must reference exactly one existing CAD joint".into());
            }
            if model
                .identification
                .contains_key(motors[0].joint.as_ref().unwrap())
            {
                return Err(
                    "A profile binding cannot also carry legacy identification overrides".into(),
                );
            }
            let f = self
                .families
                .get(&b.family)
                .ok_or("Unknown actuator family")?;
            if f.version != b.version {
                return Err("Actuator family version mismatch".into());
            }
            if let Some(unit) = &b.physical_unit {
                if unit.trim().is_empty() || !units.insert(unit) {
                    return Err("Physical unit must be nonempty and assigned only once".into());
                }
            } else if !b.deviations.is_empty() {
                return Err("Per-unit deviations require an explicit physical identity".into());
            }
            let (mut motor, mut driver) = bases[&b.family].clone();
            for (key, p) in &b.deviations {
                let (group, name) = key
                    .split_once('.')
                    .ok_or("Deviation requires motor.NAME or driver.NAME")?;
                let (target, declared) = match group {
                    "motor" => (&mut motor, &f.motor),
                    "driver" => (&mut driver, &f.driver),
                    _ => return Err("Unknown deviation group".into()),
                };
                let base = declared.get(name).ok_or("Unknown deviation parameter")?;
                p.validate(&base.unit, &f.evidence)?;
                *target.get_mut(name).unwrap() += p.value;
            }
            for (kind, values) in [(MOTOR_UNIT, &motor), (H_BRIDGE, &driver)] {
                registry
                    .get(&kind.into())
                    .map_err(|e| e.to_string())?
                    .validate_parameters(values)
                    .map_err(|e| e.to_string())?;
            }
            result.insert(
                id.clone(),
                Resolved {
                    family: b.family.clone(),
                    version: b.version,
                    physical_unit: b.physical_unit.clone(),
                    motor,
                    driver,
                    controller: f.controller.clone(),
                },
            );
        }
        Ok(result)
    }
}
impl PhysicalModel {
    pub fn resolve_actuator_profiles(&mut self, registry: &BehaviorRegistry) -> Result<(), String> {
        let resolved = self
            .actuator_profiles
            .as_ref()
            .map(|p| p.resolve(self, registry))
            .transpose()?
            .unwrap_or_default();
        // Commit only after the entire declaration validates. Re-resolution is
        // idempotent: per-unit deltas always start from the immutable family.
        for m in &mut self.motors {
            m.resolved_actuator = resolved.get(&m.id).cloned();
        }
        Ok(())
    }
}
