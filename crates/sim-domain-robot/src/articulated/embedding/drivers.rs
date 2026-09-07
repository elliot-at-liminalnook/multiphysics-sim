//! Exact algebraic connection of the registered H-bridge to motor current.
//! KCL fixes bridge current to minus motor current. Evaluating the original
//! voltage residual at zero output voltage yields the required motor voltage.
use super::{EmbeddedMotorBank, MotorBoundary};
use sim_core::{Behavior, BehaviorRegistry, Context};
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedDriverConfig {
    pub dof: String,
    /// Parameters of robot.h_bridge with the registry's declared SI units.
    pub parameters: BTreeMap<String, f64>,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverBoundary {
    pub supply_voltage_v: f64,
    /// Averaged bridge duty, in [-1,1]. Hold or schedule commands externally.
    pub duty: f64,
    pub winding_temperature_k: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct EmbeddedDriverReading {
    pub motor_voltage_v: f64,
    pub motor_current_a: f64,
    pub supply_current_a: f64,
    /// Supply power minus motor electrical input power. No driver thermal
    /// network is integrated, and no loss model is added to the shared law.
    pub power_difference_w: f64,
}

pub struct EmbeddedDriverBank {
    equations: Vec<Box<dyn Behavior>>,
    layout: Vec<(usize, usize, usize)>,
    names: Vec<String>,
    state_count: usize,
}
impl EmbeddedDriverBank {
    pub fn new(
        motors: &EmbeddedMotorBank,
        configs: &[EmbeddedDriverConfig],
    ) -> Result<Self, String> {
        let names = motors.dof_names();
        if configs.len() != names.len() || configs.iter().zip(&names).any(|(c, n)| &c.dof != n) {
            return Err("one driver per motor in exact named motor order is required".into());
        }
        let mut registry = BehaviorRegistry::default();
        crate::motor::register(&mut registry).map_err(|e| e.to_string())?;
        let descriptor = registry
            .get(&crate::H_BRIDGE.into())
            .map_err(|e| e.to_string())?;
        let mut equations = Vec::new();
        for config in configs {
            descriptor
                .validate_parameters(&config.parameters)
                .map_err(|e| e.to_string())?;
            let behavior =
                descriptor.equations.ok_or("missing H-bridge equations")?(&config.parameters)
                    .map_err(|e| e.to_string())?;
            if behavior.states().len() != 1 {
                return Err("H-bridge algebraic state layout changed".into());
            }
            equations.push(behavior);
        }
        Ok(Self {
            equations,
            layout: motors.state_layout(),
            names,
            state_count: motors.initial_states().len(),
        })
    }
    /// Protect a configured current layout from accidental use with another bank.
    pub fn validate_binding(&self, motors: &EmbeddedMotorBank) -> Result<(), String> {
        if self.names != motors.dof_names() || self.layout != motors.state_layout() {
            Err("driver/motor binding mismatch".into())
        } else {
            Ok(())
        }
    }
    /// Pure instantaneous driver evaluation. Feedback uses the current trial
    /// current, never the previous accepted current. This preserves the original
    /// soft foldback, including its regeneration/clamping limitations; it does
    /// not impose an ideal hard current cap or model switching/driver heating.
    pub fn evaluate(
        &self,
        time_s: f64,
        motor_states: &[f64],
        inputs: &[DriverBoundary],
    ) -> Result<(Vec<MotorBoundary>, Vec<EmbeddedDriverReading>), String> {
        if !time_s.is_finite()
            || time_s < 0.0
            || motor_states.len() != self.state_count
            || motor_states.iter().any(|v| !v.is_finite())
            || inputs.len() != self.equations.len()
            || inputs.iter().any(|b| {
                !b.supply_voltage_v.is_finite()
                    || b.supply_voltage_v < 0.0
                    || !b.duty.is_finite()
                    || b.duty.abs() > 1.0
                    || !b.winding_temperature_k.is_finite()
                    || b.winding_temperature_k <= 0.0
            })
        {
            return Err("invalid driver boundary or motor state".into());
        }
        let mut boundaries = Vec::with_capacity(inputs.len());
        let mut readings = Vec::with_capacity(inputs.len());
        for ((equations, layout), input) in self.equations.iter().zip(&self.layout).zip(inputs) {
            let current = motor_states[layout.0];
            let mut residual = [0.0];
            let mut through = [0.0; 4];
            equations.residual(&mut Context::new(
                time_s,
                &[-current],
                &[0.0],
                &[0, 1, 2, 3, 4, 4],
                &[None; 5],
                &[input.supply_voltage_v, 0.0, 0.0, 0.0],
                &[0.0; 4],
                &[input.duty],
                &mut residual,
                &mut through,
                &mut [],
            ));
            let voltage = -residual[0];
            let power_difference = input.supply_voltage_v * through[0] - voltage * current;
            if residual.iter().chain(&through).any(|v| !v.is_finite())
                || !power_difference.is_finite()
            {
                return Err("nonfinite registered driver result".into());
            }
            boundaries.push(MotorBoundary {
                voltage_v: voltage,
                winding_temperature_k: input.winding_temperature_k,
            });
            readings.push(EmbeddedDriverReading {
                motor_voltage_v: voltage,
                motor_current_a: current,
                supply_current_a: through[0],
                power_difference_w: power_difference,
            });
        }
        Ok((boundaries, readings))
    }
}
