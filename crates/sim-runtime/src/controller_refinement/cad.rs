//! Reviewable physical-property proposals; preserve the original CAD export and unknown fields.
use crate::experiment_study::ModelSettings;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Property {
    pub parameter: String,
    pub pointer: String,
    pub unit: String,
    pub previous: Option<f64>,
    pub proposed: f64,
    pub provenance: String,
    pub uncertainty: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proposal {
    pub version: u32,
    pub source_model_hash: String,
    pub hardware_id: u8,
    pub component_id: String,
    pub evidence: Vec<String>,
    pub tested_scope: String,
    pub changes: Vec<Property>,
    pub unmapped: Vec<String>,
}
fn hash(value: &Value) -> String {
    blake3::hash(&serde_json::to_vec(value).unwrap())
        .to_hex()
        .to_string()
}
fn motor_path(parameter: &str) -> Option<(&'static str, &'static str)> {
    Some(match parameter {
        "resistance" => ("electrical/resistance", "ohm"),
        "inductance" => ("electrical/inductance", "H"),
        "torque_constant" => ("electrical/torque_constant", "N*m/A"),
        "back_emf_constant" => ("electrical/back_emf_constant", "V*s/rad"),
        "rotor_inertia" => ("electrical/rotor_inertia", "kg*m^2"),
        "no_load_current" => ("electrical/no_load_current", "A"),
        "loss_speed_scale" => ("electrical/loss_speed_scale", "rad/s"),
        _ => return None,
    })
}
pub fn propose(
    source: &Value,
    component: &str,
    device: u8,
    baseline: &ModelSettings,
    candidate: &ModelSettings,
    evidence: Vec<String>,
    scope: &str,
) -> Result<Proposal, String> {
    baseline.validate()?;
    candidate.validate()?;
    if component.trim().is_empty()
        || !(1..=253).contains(&device)
        || evidence.is_empty()
        || evidence.iter().any(|e| e.trim().is_empty())
        || scope.trim().is_empty()
    {
        return Err(
            "Proposal requires explicit hardware/CAD identity, source evidence and tested scope"
                .into(),
        );
    }
    let motors = source["motors"]
        .as_array()
        .ok_or("Expected a physical CAD export with motors")?;
    let matches = motors
        .iter()
        .enumerate()
        .filter(|(_, m)| m["id"] == component)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(
            "CAD motor ID must match exactly one component; names are not inferred as IDs".into(),
        );
    }
    let index = matches[0].0;
    let mut proposal = Proposal {
        version: 1,
        source_model_hash: hash(source),
        hardware_id: device,
        component_id: component.into(),
        evidence,
        tested_scope: scope.into(),
        changes: vec![],
        unmapped: vec![],
    };
    for (name, &value) in &candidate.motor {
        let previous_model = baseline.motor.get(name).copied().or_else(|| {
            crate::registry()
                .get(&sim_domain_robot::motor::MOTOR_UNIT.into())
                .ok()?
                .parameters
                .as_ref()?
                .iter()
                .find(|p| &p.name == name)?
                .default
        });
        if previous_model == Some(value) {
            continue;
        }
        if let Some((path, unit)) = motor_path(name) {
            let pointer = format!("/motors/{index}/{path}");
            let previous = source.pointer(&pointer).and_then(Value::as_f64);
            if previous.is_none() {
                if name != "loss_speed_scale" || source.pointer(&pointer).is_some() {
                    return Err(format!(
                        "CAD is missing explicit numeric {path}; a proposal cannot silently supply a default"
                    ));
                }
                let (parent, _) = pointer.rsplit_once('/').unwrap();
                if !source.pointer(parent).is_some_and(Value::is_object) {
                    return Err("CAD is missing the actuator electrical/model definition".into());
                }
                proposal.version = 2;
            }
            proposal.changes.push(Property{parameter:format!("motor.{name}"),pointer,unit:unit.into(),previous,proposed:value,provenance:"Estimated from model refinement; acceptance does not imply unique physical identification".into(),uncertainty:"Not identified; bounds on fitting are not confidence intervals".into()});
        } else {
            proposal.unmapped.push(format!(
                "motor.{name}: needs an explicit CAD property/derivation mapping"
            ));
        }
    }
    for (name, value) in &candidate.bridge {
        if baseline.bridge.get(name) != Some(value) {
            proposal.unmapped.push(format!(
                "bridge.{name}: needs an explicit driver property mapping"
            ));
        }
    }
    if candidate.power != baseline.power {
        proposal.unmapped.push("Electrical source/battery scenario changed; requires an explicit CAD battery, wiring and electronics-load mapping, not an actuator-property update".into());
    }
    if candidate.conditions != baseline.conditions {
        proposal.unmapped.push("Bench condition overrides belong to the fixture/experiment, not automatically to the robot".into());
    }
    if proposal.changes.is_empty() && proposal.unmapped.is_empty() {
        return Err("No changed physical properties to propose".into());
    }
    Ok(proposal)
}
impl Proposal {
    /// Explicit acceptance returns a new CAD-owned artifact for the common Rust robot runtime.
    /// The caller chooses where to save; source files and other robot properties are untouched.
    pub fn accept(&self, source: &Value, decision: &str) -> Result<Value, String> {
        if ![1, 2].contains(&self.version) || self.source_model_hash != hash(source) {
            return Err(
                "CAD source changed since proposal; regenerate and review the new diff".into(),
            );
        }
        if decision.trim().is_empty() || self.changes.is_empty() || !self.unmapped.is_empty() {
            return Err("Acceptance needs a decision and mapped properties only; resolve unmapped changes first".into());
        }
        let mut out = source.clone();
        let motor_index = source["motors"]
            .as_array()
            .ok_or("Missing CAD motors")?
            .iter()
            .position(|m| m["id"] == self.component_id)
            .ok_or("Missing CAD motor ID")?;
        for change in &self.changes {
            let parameter = change
                .parameter
                .strip_prefix("motor.")
                .ok_or("Unsupported proposed parameter group")?;
            let (path, unit) = motor_path(parameter).ok_or("Unsupported proposed CAD parameter")?;
            if change.pointer != format!("/motors/{motor_index}/{path}") || change.unit != unit {
                return Err("Proposal field mapping or units changed; regenerate proposal".into());
            }
            if !change.proposed.is_finite()
                || change.uncertainty.trim().is_empty()
                || change.provenance.trim().is_empty()
            {
                return Err(
                    "Property needs finite value, provenance and explicit uncertainty".into(),
                );
            }
            if let Some(previous) = change.previous {
                let value = out
                    .pointer_mut(&change.pointer)
                    .ok_or("CAD property no longer exists")?;
                if value.as_f64() != Some(previous) {
                    return Err("CAD property baseline changed".into());
                }
                *value = Value::from(change.proposed);
            } else {
                if self.version != 2
                    || parameter != "loss_speed_scale"
                    || out.pointer(&change.pointer).is_some()
                {
                    return Err("Only an explicitly reviewed new loss-regularization field may be introduced".into());
                }
                let (parent, key) = change
                    .pointer
                    .rsplit_once('/')
                    .ok_or("Invalid CAD field path")?;
                out.pointer_mut(parent)
                    .and_then(Value::as_object_mut)
                    .ok_or("Missing CAD parameter object")?
                    .insert(key.into(), Value::from(change.proposed));
            }
        }
        let model = sim_domain_robot::model::PhysicalModel::parse(
            &serde_json::to_string(&out).map_err(|e| e.to_string())?,
        )?;
        let motor = model
            .motors
            .iter()
            .find(|m| m.id == self.component_id)
            .ok_or("Missing proposed motor")?;
        if (motor.electrical.torque_constant - motor.electrical.back_emf_constant).abs() > 1e-12 {
            return Err(
                "Accepted SI motor constants must remain reciprocal; review Kt and Ke together"
                    .into(),
            );
        }
        let parameters =
            sim_domain_robot::motor::cad_motor_unit_parameters(motor, 0., 298.15, true, false)
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect();
        crate::registry()
            .get(&sim_domain_robot::motor::MOTOR_UNIT.into())
            .map_err(|e| e.to_string())?
            .validate_parameters(&parameters)
            .map_err(|e| e.to_string())?;
        for change in &self.changes {
            if parameters.get(change.parameter.strip_prefix("motor.").unwrap())
                != Some(&change.proposed)
            {
                return Err("Shared CAD mapping would clamp or alter the proposed value; reconcile the physical definition first".into());
            }
        }
        let old_source = out.get("source").cloned().unwrap_or(Value::Null);
        if !out["source"].is_object() {
            out["source"] = serde_json::json!({"previous_source":old_source});
        }
        let provenance = serde_json::json!({"proposal":self,"acceptance_decision":decision,"interpretation":"Accepted estimated properties for stated scope; loaded joint, leg and quadruped still require separate validation"});
        let source_object = out["source"]
            .as_object_mut()
            .ok_or("Invalid source metadata")?;
        let history = source_object
            .entry("controller_model_refinements")
            .or_insert_with(|| Value::Array(vec![]));
        history
            .as_array_mut()
            .ok_or("Existing refinement metadata has an incompatible shape")?
            .push(provenance);
        Ok(out)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Acceptance {
    pub proposal: Proposal,
    pub decision: String,
    pub artifact_hash: String,
    pub new_file: String,
}
pub fn save_accepted_new(
    proposal: &Proposal,
    source_path: &std::path::Path,
    destination: &std::path::Path,
    decision: &str,
) -> Result<Acceptance, String> {
    let source: Value =
        serde_json::from_slice(&std::fs::read(source_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let artifact = proposal.accept(&source, decision)?;
    crate::experiment_study::write_new(
        destination,
        &serde_json::to_vec_pretty(&artifact).map_err(|e| e.to_string())?,
    )?;
    Ok(Acceptance {
        proposal: proposal.clone(),
        decision: decision.into(),
        artifact_hash: hash(&artifact),
        new_file: destination.display().to_string(),
    })
}
