//! Radial DC supply adapter for the registered battery, resistor and H-bridge
//! laws. Topology is configuration; every voltage and SOC is a coupled unknown.
//! No battery curve or winding law is duplicated here.
use super::{
    DriverBoundary, EmbeddedDriverBank, EmbeddedDriverReading, EmbeddedMotorBank, MotorBoundary,
};
use sim_core::{Behavior, BehaviorRegistry, Context};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerBranchConfig {
    pub id: String,
    /// None connects to the pack; otherwise an exact branch ID. Resistance
    /// covers the complete outgoing/return path of this segment only.
    pub parent: Option<String>,
    pub resistance_ohm: f64,
    /// Exact motor DOFs fed at this node, excluding descendants.
    pub motors: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedPowerConfig {
    /// All five registered robot.battery parameters must be explicit.
    pub battery: BTreeMap<String, f64>,
    pub branches: Vec<PowerBranchConfig>,
    /// Positive equation divisors: voltage V, branch current A, SOC rate 1/s,
    /// and terminal energy rate W. These are numerical settings, not physics.
    pub residual_scales: [f64; 4],
}

struct Branch {
    id: String,
    parent: Option<usize>,
    downstream_motors: Vec<usize>,
    resistor: Option<Box<dyn Behavior>>,
}

pub struct EmbeddedPowerBank {
    limits: Option<PowerOperatingLimits>,
    battery: Box<dyn Behavior>,
    branches: Vec<Branch>,
    motor_branch: Vec<usize>,
    names: Vec<String>,
    scales: [f64; 4],
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct PowerOperatingLimits {
    pub minimum_pack_voltage_v: f64,
    pub maximum_pack_voltage_v: f64,
    pub minimum_soc: f64,
    pub maximum_soc: f64,
}
impl PowerOperatingLimits {
    pub fn validate_sample(&self, voltage_v: f64, soc: f64) -> Result<(), String> {
        if !voltage_v.is_finite()
            || !soc.is_finite()
            || soc < self.minimum_soc
            || soc > self.maximum_soc
            || voltage_v < self.minimum_pack_voltage_v
            || voltage_v > self.maximum_pack_voltage_v
        {
            return Err(format!(
                "CAD power operating envelope exceeded: pack {voltage_v} V, SOC {soc}"
            ));
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.minimum_pack_voltage_v,
            self.maximum_pack_voltage_v,
            self.minimum_soc,
            self.maximum_soc,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || self.minimum_pack_voltage_v < 0.0
            || self.minimum_pack_voltage_v >= self.maximum_pack_voltage_v
            || self.minimum_soc < 0.0
            || self.maximum_soc > 1.0
            || self.minimum_soc >= self.maximum_soc
        {
            return Err("Invalid power operating envelope".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PowerBranchReading {
    pub id: String,
    pub voltage_v: f64,
    pub current_a: f64,
    pub wiring_loss_w: f64,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EmbeddedPowerReading {
    pub voltage_v: f64,
    /// Positive means battery discharge; negative means modeled regeneration.
    pub current_a: f64,
    pub power_w: f64,
    pub state_of_charge: f64,
    /// Signed terminal energy integrated by the same time integrator as motors.
    pub terminal_energy_j: f64,
    pub branches: Vec<PowerBranchReading>,
}

impl EmbeddedPowerBank {
    pub fn with_limits(mut self, limits: PowerOperatingLimits) -> Result<Self, String> {
        limits.validate()?;
        self.limits = Some(limits);
        Ok(self)
    }
    pub fn validate_endpoint(&self, states: &[f64]) -> Result<(), String> {
        self.validate_states(states)?;
        if let Some(l) = &self.limits {
            l.validate_sample(states[1], states[0])?;
        }
        Ok(())
    }
    pub fn new(motors: &EmbeddedMotorBank, config: &EmbeddedPowerConfig) -> Result<Self, String> {
        Self::for_motor_names(&motors.dof_names(), config)
    }

    /// Validate a CAD declaration without compiling geometry or mechanics.
    pub fn for_motor_names(names: &[String], config: &EmbeddedPowerConfig) -> Result<Self, String> {
        let mut registry = BehaviorRegistry::default();
        crate::motor::register(&mut registry).map_err(|e| e.to_string())?;
        sim_domain_electrical::elements::register(&mut registry).map_err(|e| e.to_string())?;
        let descriptor = registry
            .get(&crate::BATTERY.into())
            .map_err(|e| e.to_string())?;
        descriptor
            .validate_parameters(&config.battery)
            .map_err(|e| e.to_string())?;
        for key in [
            "cells",
            "nominal_voltage",
            "internal_resistance",
            "capacity_ah",
            "initial_soc",
        ] {
            if !config.battery.get(key).is_some_and(|v| v.is_finite()) {
                return Err(format!(
                    "power model requires explicit finite battery.{key}"
                ));
            }
        }
        // The registered battery protects division with this floor. Reject
        // values it would silently change in an explicitly authored network.
        if config.battery["capacity_ah"] < 1e-6 {
            return Err("battery capacity is below the registered model's 1e-6 A·h domain".into());
        }
        if config
            .residual_scales
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        {
            return Err("power residual scales must be positive and finite".into());
        }
        let battery = descriptor.equations.ok_or("missing battery equations")?(&config.battery)
            .map_err(|e| e.to_string())?;
        if battery
            .states()
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            != ["current", "soc"]
        {
            return Err("registered battery state layout changed".into());
        }
        let names = names.to_vec();
        if names.iter().any(|n| n.trim().is_empty())
            || names.iter().collect::<BTreeSet<_>>().len() != names.len()
        {
            return Err("power requires unique nonempty motor coordinates".into());
        }
        let ids: BTreeMap<_, _> = config
            .branches
            .iter()
            .enumerate()
            .map(|(i, b)| (b.id.as_str(), i))
            .collect();
        if ids.len() != config.branches.len() || ids.contains_key("") {
            return Err("power branch IDs must be nonempty and unique".into());
        }
        let mut branches = Vec::new();
        let mut motor_branch = vec![usize::MAX; names.len()];
        let resistor = registry
            .get(&sim_domain_electrical::elements::RESISTOR.into())
            .map_err(|e| e.to_string())?;
        for (i, b) in config.branches.iter().enumerate() {
            if !b.resistance_ohm.is_finite() || b.resistance_ohm < 0.0 {
                return Err("branch resistance must be finite and nonnegative".into());
            }
            let parent = b
                .parent
                .as_ref()
                .map(|p| {
                    ids.get(p.as_str())
                        .copied()
                        .ok_or_else(|| format!("unknown power branch parent {p}"))
                })
                .transpose()?;
            for name in &b.motors {
                let motor = names
                    .iter()
                    .position(|n| n == name)
                    .ok_or_else(|| format!("unknown power branch motor {name}"))?;
                if motor_branch[motor] != usize::MAX {
                    return Err(format!("duplicate power feed for {name}"));
                }
                motor_branch[motor] = i;
            }
            // Zero is an explicit ideal connection, never a 1/0 resistor.
            let equations = if b.resistance_ohm == 0.0 {
                None
            } else {
                let parameters = BTreeMap::from([("resistance".into(), b.resistance_ohm)]);
                resistor
                    .validate_parameters(&parameters)
                    .map_err(|e| e.to_string())?;
                Some(
                    resistor.equations.ok_or("missing resistor equations")?(&parameters)
                        .map_err(|e| e.to_string())?,
                )
            };
            branches.push(Branch {
                id: b.id.clone(),
                parent,
                downstream_motors: vec![],
                resistor: equations,
            });
        }
        if motor_branch.contains(&usize::MAX) {
            return Err("every motor requires an explicit power feed".into());
        }
        // Check even unloaded branches: an unused cycle is still invalid CAD.
        for start in 0..branches.len() {
            let mut seen = BTreeSet::new();
            let mut next = Some(start);
            while let Some(i) = next {
                if !seen.insert(i) {
                    return Err("power branches contain a cycle".into());
                }
                next = branches[i].parent;
            }
        }
        for (motor, branch) in motor_branch.iter().enumerate() {
            let mut next = Some(*branch);
            while let Some(i) = next {
                branches[i].downstream_motors.push(motor);
                next = branches[i].parent;
            }
        }
        Ok(Self {
            limits: None,
            battery,
            branches,
            motor_branch,
            names,
            scales: config.residual_scales,
        })
    }

    pub(super) fn validate_binding(&self, names: &[String]) -> Result<(), String> {
        if self.names == names {
            Ok(())
        } else {
            Err("power/motor binding mismatch".into())
        }
    }

    /// SOC, pack terminal voltage, branch voltages in configuration order,
    /// accumulated terminal energy. No current or voltage history is hidden.
    pub fn initial_states(&self) -> Vec<f64> {
        let soc = self.battery.states()[1].initial;
        let mut residual = [0.0; 2];
        self.battery.residual(&mut Context::new(
            0.0,
            &[0.0, soc],
            &[0.0; 2],
            &[0, 1, 2, 2],
            &[None; 3],
            &[0.0; 2],
            &[0.0; 2],
            &[],
            &mut residual,
            &mut [0.0; 2],
            &mut [0.0],
        ));
        let mut states = vec![soc, -residual[0]];
        states.extend(vec![-residual[0]; self.branches.len()]);
        states.push(0.0);
        states
    }

    pub(super) fn validate_states(&self, states: &[f64]) -> Result<(), String> {
        if states.len() != 3 + self.branches.len() || states.iter().any(|v| !v.is_finite()) {
            Err("invalid continuous power state".into())
        } else {
            Ok(())
        }
    }

    /// Resolve instantaneous voltages after a command jump, keeping SOC and
    /// accumulated energy fixed. The shared nonlinear solver evaluates the
    /// same registered circuit residuals; no second power law is introduced.
    pub fn reconcile(
        &self,
        time: f64,
        drivers: &EmbeddedDriverBank,
        motors: &[f64],
        inputs: &[DriverBoundary],
        states: &mut [f64],
    ) -> Result<(), String> {
        self.validate_states(states)?;
        let end = states.len() - 1;
        let mut voltages = states[1..end].to_vec();
        let error = std::cell::RefCell::new(None);
        sim_solve::solve_newton(&mut voltages, sim_solve::NewtonConfig::default(), |x, r| {
            let mut trial = states.to_vec();
            trial[1..end].copy_from_slice(x);
            match self.evaluate(
                time,
                drivers,
                motors,
                inputs,
                &trial,
                &vec![0.0; trial.len()],
            ) {
                Ok(result) => r.copy_from_slice(&result.2[1..end]),
                Err(e) => {
                    *error.borrow_mut() = Some(e);
                    r.fill(f64::NAN);
                }
            }
        })
        .map_err(|e| {
            format!(
                "power algebraic consistency: {}",
                error.into_inner().unwrap_or_else(|| e.to_string())
            )
        })?;
        states[1..end].copy_from_slice(&voltages);
        Ok(())
    }

    /// Pure network evaluation at trial states and rates. Input supply voltages
    /// are replaced by the explicit power network. Each H-bridge draws current
    /// from its own branch; all ancestor segments carry descendant loads.
    pub fn evaluate(
        &self,
        time: f64,
        drivers: &EmbeddedDriverBank,
        motors: &[f64],
        inputs: &[DriverBoundary],
        states: &[f64],
        rates: &[f64],
    ) -> Result<
        (
            Vec<MotorBoundary>,
            Vec<EmbeddedDriverReading>,
            Vec<f64>,
            EmbeddedPowerReading,
        ),
        String,
    > {
        self.validate_states(states)?;
        self.validate_binding(&drivers.names)?;
        if rates.len() != states.len()
            || rates.iter().any(|v| !v.is_finite())
            || inputs.len() != self.names.len()
        {
            return Err("invalid continuous power rates or driver inputs".into());
        }
        let inputs: Vec<_> = inputs
            .iter()
            .enumerate()
            .map(|(i, input)| DriverBoundary {
                supply_voltage_v: states[2 + self.motor_branch[i]],
                ..*input
            })
            .collect();
        let (boundaries, driver_readings) = drivers.evaluate(time, motors, &inputs)?;
        let current: f64 = driver_readings.iter().map(|d| d.supply_current_a).sum();
        let mut battery_residual = [0.0; 2];
        self.battery.residual(&mut Context::new(
            time,
            &[-current, states[0]],
            &[0.0, rates[0]],
            &[0, 1, 2, 2],
            &[None; 3],
            &[states[1], 0.0],
            &[rates[1], 0.0],
            &[],
            &mut battery_residual,
            &mut [0.0; 2],
            &mut [0.0],
        ));
        let mut residuals = vec![
            battery_residual[1] / self.scales[2],
            battery_residual[0] / self.scales[0],
        ];
        let mut branches = Vec::new();
        for (i, b) in self.branches.iter().enumerate() {
            let parent_voltage = states[b.parent.map_or(1, |p| p + 2)];
            let voltage = states[i + 2];
            let branch_current: f64 = b
                .downstream_motors
                .iter()
                .map(|m| driver_readings[*m].supply_current_a)
                .sum();
            let residual = if let Some(resistor) = &b.resistor {
                let mut through = [0.0; 2];
                resistor.residual(&mut Context::new(
                    time,
                    &[],
                    &[],
                    &[0, 1, 2],
                    &[None; 2],
                    &[parent_voltage, voltage],
                    &[0.0; 2],
                    &[],
                    &mut [],
                    &mut through,
                    &mut [],
                ));
                (through[0] - branch_current) / self.scales[1]
            } else {
                (parent_voltage - voltage) / self.scales[0]
            };
            residuals.push(residual);
            branches.push(PowerBranchReading {
                id: b.id.clone(),
                voltage_v: voltage,
                current_a: branch_current,
                wiring_loss_w: (parent_voltage - voltage) * branch_current,
            });
        }
        let energy = states.len() - 1;
        let power = states[1] * current;
        residuals.push((rates[energy] - power) / self.scales[3]);
        if residuals.iter().any(|v| !v.is_finite()) || !power.is_finite() {
            return Err("nonfinite registered power equations".into());
        }
        Ok((
            boundaries,
            driver_readings,
            residuals,
            EmbeddedPowerReading {
                voltage_v: states[1],
                current_a: current,
                power_w: power,
                state_of_charge: states[0],
                terminal_energy_j: states[energy],
                branches,
            },
        ))
    }
}
