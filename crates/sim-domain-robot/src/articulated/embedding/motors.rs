//! Adapter for the existing registered motor component. The motor's electrical,
//! gearbox, loss and heat equations remain in `MotorUnit::residual`.
use super::{CoupledForces, Generalized};
use crate::{Articulated, MOTOR_UNIT, articulated::DofKind};
use sim_core::{Behavior, BehaviorRegistry, Context, View};
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedMotorConfig {
    /// Exact compiled revolute DOF name; no fuzzy matching or inferred binding.
    pub dof: String,
    /// Shared registry parameters with the registry's declared SI units.
    pub parameters: BTreeMap<String, f64>,
    /// Positive equation divisors: winding volts, output torque N m, angle
    /// evolution rad/s. Choose deliberately; no tolerances inferred from CAD.
    pub residual_scales: [f64; 3],
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotorBoundary {
    pub voltage_v: f64,
    pub winding_temperature_k: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct EmbeddedMotorReading {
    pub current_a: f64,
    pub shaft_torque_nm: f64,
    pub gear_speed_rad_s: f64,
    /// Heat delivered to the winding boundary, not an integrated temperature.
    pub heating_w: f64,
}

struct BoundMotor {
    name: String,
    dof: usize,
    equations: Box<dyn Behavior>,
    scales: [f64; 3],
    state_start: usize,
    state_count: usize,
    guard_start: usize,
    guard_count: usize,
}

/// Voltage/temperature boundary adapter for a set of registered motor units.
/// Drivers, batteries, firmware, thermal networks and event schedules are not
/// silently supplied. Their states can later use the same coupling interface.
pub struct EmbeddedMotorBank {
    motors: Vec<BoundMotor>,
    joint_count: usize,
    base_columns: usize,
    state_count: usize,
    pub(super) audit_contact_steps: bool,
}

impl EmbeddedMotorBank {
    pub fn new(art: &Articulated, configs: &[EmbeddedMotorConfig]) -> Result<Self, String> {
        Self::build(art, configs, false)
    }

    /// Event-capable bank. Use `advance_with_events` or supply a scheduler that
    /// calls `event_data`/`jump`; raw continuous stepping cannot skip its events.
    pub fn new_with_events(
        art: &Articulated,
        configs: &[EmbeddedMotorConfig],
    ) -> Result<Self, String> {
        Self::build(art, configs, true)
    }

    fn build(
        art: &Articulated,
        configs: &[EmbeddedMotorConfig],
        allow_events: bool,
    ) -> Result<Self, String> {
        let mut registry = BehaviorRegistry::default();
        crate::motor::register(&mut registry).map_err(|e| e.to_string())?;
        let descriptor = registry
            .get(&MOTOR_UNIT.into())
            .map_err(|e| e.to_string())?;
        let dofs: Vec<_> = art.dofs().map(|(_, d)| d).collect();
        let mut motors: Vec<BoundMotor> = Vec::new();
        let mut state_count = 0;
        let mut guard_count = 0;
        for config in configs {
            descriptor
                .validate_parameters(&config.parameters)
                .map_err(|e| e.to_string())?;
            if config
                .residual_scales
                .iter()
                .any(|v| !v.is_finite() || *v <= 0.0)
            {
                return Err("motor residual scales must be positive and finite".into());
            }
            if !allow_events
                && config
                    .parameters
                    .get("backlash.events")
                    .copied()
                    .unwrap_or(0.0)
                    > 0.5
            {
                return Err("embedded motor events need an external event scheduler; use the recorded algebraic backlash law".into());
            }
            let found: Vec<_> = dofs
                .iter()
                .enumerate()
                .filter(|(_, d)| d.name == config.dof)
                .collect();
            if found.len() != 1 || !matches!(found[0].1.kind, DofKind::Revolute) {
                return Err(format!(
                    "motor requires one exact revolute DOF: {}",
                    config.dof
                ));
            }
            let dof = found[0].0;
            if motors.iter().any(|m| m.dof == dof) {
                return Err("duplicate motor DOF binding".into());
            }
            let equations =
                descriptor.equations.ok_or("missing motor equations")?(&config.parameters)
                    .map_err(|e| e.to_string())?;
            let n = equations.states().len();
            if n != 3 && !(allow_events && n == 4) {
                return Err("unsupported motor state layout".into());
            }
            motors.push(BoundMotor {
                name: config.dof.clone(),
                dof,
                equations,
                scales: config.residual_scales,
                state_start: state_count,
                state_count: n,
                guard_start: guard_count,
                guard_count: if n == 4 { 3 } else { 0 },
            });
            state_count += n;
            guard_count += if n == 4 { 3 } else { 0 };
        }
        Ok(Self {
            motors,
            audit_contact_steps: false,
            joint_count: dofs.len(),
            state_count,
            base_columns: 6 * art.bases.iter().filter(|b| !b.grounded).count(),
        })
    }

    /// Enable accepted-step contact diagnostics. This changes only returned
    /// diagnostics; event-search trials and failed intervals never commit a log.
    pub fn set_contact_step_audit(&mut self, enabled: bool) {
        self.audit_contact_steps = enabled;
    }

    /// Per motor: winding current, rotor speed, gearbox output angle, and
    /// (event-mode motors only) held backlash mode. Inspect `state_layout`.
    pub fn initial_states(&self) -> Vec<f64> {
        self.motors
            .iter()
            .flat_map(|m| m.equations.states().into_iter().map(|s| s.initial))
            .collect()
    }

    pub fn guard_count(&self) -> usize {
        self.motors.iter().map(|m| m.guard_count).sum()
    }

    pub fn dof_names(&self) -> Vec<String> {
        self.motors.iter().map(|m| m.name.clone()).collect()
    }

    /// `(state start, state count, compiled joint index)` in configured order.
    pub fn state_layout(&self) -> Vec<(usize, usize, usize)> {
        self.motors
            .iter()
            .map(|m| (m.state_start, m.state_count, m.dof))
            .collect()
    }

    fn validate_event_point(
        &self,
        time: f64,
        g: &Generalized,
        state: &[f64],
        boundaries: &[MotorBoundary],
    ) -> Result<(), String> {
        if !time.is_finite()
            || time < 0.0
            || state.len() != self.state_count
            || g.q.len() != self.joint_count
            || g.qd.len() != self.joint_count
            || boundaries.len() != self.motors.len()
            || state
                .iter()
                .chain(&g.q)
                .chain(&g.qd)
                .any(|v| !v.is_finite())
            || boundaries.iter().any(|b| {
                !b.voltage_v.is_finite()
                    || !b.winding_temperature_k.is_finite()
                    || b.winding_temperature_k <= 0.0
            })
        {
            return Err("invalid motor event point".into());
        }
        Ok(())
    }

    /// Original component guards and known deadlines, in stable bank order.
    pub fn event_data(
        &self,
        time: f64,
        g: &Generalized,
        state: &[f64],
        boundaries: &[MotorBoundary],
    ) -> Result<(Vec<f64>, Vec<(usize, f64)>), String> {
        self.validate_event_point(time, g, state, boundaries)?;
        let mut guards = Vec::new();
        let mut scheduled = Vec::new();
        for (i, m) in self.motors.iter().enumerate() {
            let s = m.state_start;
            let n = m.state_count;
            let b = boundaries[i];
            let across = [
                b.voltage_v,
                0.0,
                g.q[m.dof],
                g.qd[m.dof],
                b.winding_temperature_k,
            ];
            let rates = [0.0, 0.0, g.qd[m.dof], 0.0, 0.0];
            let view = View {
                time,
                states: &state[s..s + n],
                offsets: &[0, 1, 2, 4, 5],
                rate_map: &[None, None, Some(3), None, None],
                across: &across,
                across_rates: &rates,
                signals_in: &[],
            };
            let mut local = Vec::new();
            m.equations.guards(&view, &mut local);
            if local.len() != m.guard_count || local.iter().any(|v| !v.is_finite()) {
                return Err("invalid motor guard layout".into());
            }
            guards.extend(local);
            let mut clocks = Vec::new();
            m.equations.scheduled_events(&view, &mut clocks);
            for (guard, t) in clocks {
                if guard >= m.guard_count || !t.is_finite() {
                    return Err("invalid motor event schedule".into());
                }
                scheduled.push((m.guard_start + guard, t));
            }
        }
        Ok((guards, scheduled))
    }

    /// Apply the registered motor jump to its owned state only. It does not
    /// impose a position or velocity impulse. The scheduler owns event timing.
    pub fn jump(
        &mut self,
        guard: usize,
        time: f64,
        g: &Generalized,
        state: &mut [f64],
        boundaries: &[MotorBoundary],
    ) -> Result<(), String> {
        self.validate_event_point(time, g, state, boundaries)?;
        let (i, m) = self
            .motors
            .iter_mut()
            .enumerate()
            .find(|(_, m)| guard >= m.guard_start && guard < m.guard_start + m.guard_count)
            .ok_or("unknown motor event")?;
        let s = m.state_start;
        let n = m.state_count;
        let b = boundaries[i];
        let previous = state[s..s + n].to_vec();
        let across = [
            b.voltage_v,
            0.0,
            g.q[m.dof],
            g.qd[m.dof],
            b.winding_temperature_k,
        ];
        let rates = [0.0, 0.0, g.qd[m.dof], 0.0, 0.0];
        let view = View {
            time,
            states: &previous,
            offsets: &[0, 1, 2, 4, 5],
            rate_map: &[None, None, Some(3), None, None],
            across: &across,
            across_rates: &rates,
            signals_in: &[],
        };
        m.equations
            .jump(guard - m.guard_start, &view, &mut state[s..s + n]);
        if state[s..s + 3] != previous[..3] || state[s..s + n].iter().any(|v| !v.is_finite()) {
            state[s..s + n].copy_from_slice(&previous);
            return Err("motor event changed continuous state or produced nonfinite mode".into());
        }
        Ok(())
    }

    /// Pure backward-Euler residual assembly from the shared component laws.
    /// Thermal boundaries must be explicit even for a short isothermal test.
    pub fn evaluate(
        &self,
        end_time_s: f64,
        step_s: f64,
        mechanics: &Generalized,
        old: &[f64],
        trial: &[f64],
        boundaries: &[MotorBoundary],
    ) -> Result<(CoupledForces, Vec<EmbeddedMotorReading>), String> {
        if !step_s.is_finite()
            || step_s <= 0.0
            || old.len() != trial.len()
            || old.iter().any(|v| !v.is_finite())
        {
            return Err("invalid embedded motor evaluation".into());
        }
        let rates: Vec<_> = trial
            .iter()
            .zip(old)
            .map(|(new, old)| (new - old) / step_s)
            .collect();
        self.evaluate_with_rates(end_time_s, mechanics, trial, &rates, boundaries)
    }

    /// Evaluate original registered laws with explicit physical state rates.
    /// The time integrator owns the relation between states and rates. This
    /// avoids recovering small increments by subtracting rounded large states.
    pub fn evaluate_with_rates(
        &self,
        end_time_s: f64,
        mechanics: &Generalized,
        trial: &[f64],
        rates: &[f64],
        boundaries: &[MotorBoundary],
    ) -> Result<(CoupledForces, Vec<EmbeddedMotorReading>), String> {
        if !end_time_s.is_finite()
            || end_time_s < 0.0
            || trial.len() != self.state_count
            || rates.len() != trial.len()
            || boundaries.len() != self.motors.len()
            || mechanics.q.len() != self.joint_count
            || mechanics.qd.len() != self.joint_count
            || rates
                .iter()
                .chain(trial)
                .chain(&mechanics.q)
                .chain(&mechanics.qd)
                .any(|v| !v.is_finite())
            || boundaries.iter().any(|b| {
                !b.voltage_v.is_finite()
                    || !b.winding_temperature_k.is_finite()
                    || b.winding_temperature_k <= 0.0
            })
        {
            return Err("invalid embedded motor evaluation".into());
        }
        let mut result = CoupledForces {
            generalized_loads: vec![0.0; self.base_columns + self.joint_count],
            auxiliary_residuals: vec![0.0; trial.len()],
        };
        let mut readings = Vec::with_capacity(self.motors.len());
        for (i, motor) in self.motors.iter().enumerate() {
            let s = motor.state_start;
            let n = motor.state_count;
            let boundary = boundaries[i];
            // Registry ports: p, n, shaft(angle/rate), winding. MotorUnit uses
            // the shaft derivative explicitly, so supply its physical velocity.
            let across = [
                boundary.voltage_v,
                0.0,
                mechanics.q[motor.dof],
                mechanics.qd[motor.dof],
                boundary.winding_temperature_k,
            ];
            let across_rates = [0.0, 0.0, mechanics.qd[motor.dof], 0.0, 0.0];
            let mut residuals = vec![0.0; n];
            let mut through = [0.0; 5];
            let mut signals = [0.0; 3];
            motor.equations.residual(&mut Context::new(
                end_time_s,
                &trial[s..s + n],
                &rates[s..s + n],
                &[0, 1, 2, 4, 5],
                &[None, None, Some(3), None, None],
                &across,
                &across_rates,
                &[],
                &mut residuals,
                &mut through,
                &mut signals,
            ));
            if residuals
                .iter()
                .chain(&through)
                .chain(&signals)
                .any(|v| !v.is_finite())
            {
                return Err("nonfinite shared motor equations".into());
            }
            result.generalized_loads[self.base_columns + motor.dof] -= through[2];
            for k in 0..n {
                // Held mode equation has the declared one-per-second scale.
                result.auxiliary_residuals[s + k] =
                    residuals[k] / if k < 3 { motor.scales[k] } else { 1.0 };
            }
            if result.auxiliary_residuals[s..s + n]
                .iter()
                .any(|v| !v.is_finite())
            {
                return Err("nonfinite scaled motor residual".into());
            }
            readings.push(EmbeddedMotorReading {
                current_a: signals[0],
                shaft_torque_nm: signals[1],
                gear_speed_rad_s: signals[2],
                heating_w: -through[4],
            });
        }
        Ok((result, readings))
    }
}
