//! CAD-owned pack and radial power distribution, distinct from signal buses.
use crate::PhysicalModel;
use crate::actuator_profile::{Evidence, Parameter, digest, parameters};
use crate::articulated::embedding::{EmbeddedPowerBank, EmbeddedPowerConfig, PowerBranchConfig};
use serde::{Deserialize, Serialize};
use sim_core::BehaviorRegistry;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerBranch {
    pub id: String,
    pub parent: Option<String>,
    /// Resistance of this segment's complete supply/return path, not ancestors.
    pub resistance: Parameter,
    /// Stable CAD motor body IDs fed at this node; never bus addresses.
    pub motors: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatingLimits {
    pub minimum_pack_voltage: Parameter,
    pub maximum_pack_voltage: Parameter,
    pub minimum_soc: Parameter,
    pub maximum_soc: Parameter,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerDistribution {
    pub version: u32,
    pub description: String,
    pub limitations: Vec<String>,
    pub evidence: BTreeMap<String, Evidence>,
    pub battery: BTreeMap<String, Parameter>,
    pub branches: Vec<PowerBranch>,
    /// Declared admissible model envelope. Exceeding it stops an experiment;
    /// this does not invent a physical BMS, cutoff switch or protection circuit.
    pub operating_limits: OperatingLimits,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolvedPower {
    pub config: EmbeddedPowerConfig,
    pub limits: crate::articulated::embedding::PowerOperatingLimits,
    pub motor_ids: Vec<String>,
}

impl PhysicalModel {
    pub fn resolve_power_profile(
        &self,
        registry: &BehaviorRegistry,
        residual_scales: [f64; 4],
    ) -> Result<Option<ResolvedPower>, String> {
        let Some(p) = self
            .actuator_profiles
            .as_ref()
            .and_then(|p| p.power.as_ref())
        else {
            return Ok(None);
        };
        if self.battery.is_some() {
            return Err(
                "CAD power profile and legacy battery cannot both define the supply".into(),
            );
        }
        if p.version != 1
            || p.description.trim().is_empty()
            || p.limitations.is_empty()
            || p.limitations.iter().any(|s| s.trim().is_empty())
            || p.evidence.is_empty()
        {
            return Err(
                "Power distribution requires version 1, description, limitations and evidence"
                    .into(),
            );
        }
        for (id, e) in &p.evidence {
            if id.trim().is_empty()
                || e.path.trim().is_empty()
                || e.scope.trim().is_empty()
                || !digest(&e.sha256)
            {
                return Err("Power evidence requires identity, path, SHA-256 and scope".into());
            }
        }
        let battery = parameters(crate::BATTERY, &p.battery, &p.evidence, registry)?;
        let l = &p.operating_limits;
        for (parameter, unit) in [
            (&l.minimum_pack_voltage, "V"),
            (&l.maximum_pack_voltage, "V"),
            (&l.minimum_soc, "1"),
            (&l.maximum_soc, "1"),
        ] {
            parameter.validate(unit, &p.evidence)?;
        }
        let limits = crate::articulated::embedding::PowerOperatingLimits {
            minimum_pack_voltage_v: l.minimum_pack_voltage.value,
            maximum_pack_voltage_v: l.maximum_pack_voltage.value,
            minimum_soc: l.minimum_soc.value,
            maximum_soc: l.maximum_soc.value,
        };
        limits.validate()?;
        if battery["initial_soc"] < limits.minimum_soc
            || battery["initial_soc"] > limits.maximum_soc
        {
            return Err("Initial battery SOC lies outside the declared operating envelope".into());
        }
        let mut ids = BTreeSet::new();
        let mut coordinates = BTreeMap::new();
        let mut names = Vec::new();
        for m in &self.motors {
            if m.id.trim().is_empty() || !ids.insert(m.id.clone()) {
                return Err("Power distribution requires unique nonempty CAD motor IDs".into());
            }
            let name = m.joint.as_ref().ok_or("Powered motor requires a joint")?;
            let joints: Vec<_> = self.joints.iter().filter(|j| &j.name == name).collect();
            if joints.len() != 1 || !["revolute", "continuous"].contains(&joints[0].kind.as_str()) {
                return Err("Powered motor requires one existing revolute joint".into());
            }
            let dof = format!("joint.{name}");
            names.push(dof.clone());
            coordinates.insert(&m.id, dof);
        }
        let branches = p
            .branches
            .iter()
            .map(|b| {
                b.resistance.validate("Ω", &p.evidence)?;
                Ok(PowerBranchConfig {
                    id: b.id.clone(),
                    parent: b.parent.clone(),
                    resistance_ohm: b.resistance.value,
                    motors: b
                        .motors
                        .iter()
                        .map(|id| {
                            coordinates
                                .get(id)
                                .cloned()
                                .ok_or_else(|| format!("Unknown CAD power motor {id}"))
                        })
                        .collect::<Result<_, _>>()?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let config = EmbeddedPowerConfig {
            battery,
            branches,
            residual_scales,
        };
        EmbeddedPowerBank::for_motor_names(&names, &config)?;
        Ok(Some(ResolvedPower {
            config,
            limits,
            motor_ids: self.motors.iter().map(|m| m.id.clone()).collect(),
        }))
    }
}
