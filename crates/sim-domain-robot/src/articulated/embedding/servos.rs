//! Registered sampled servo firmware, held between scheduled ticks. No PID,
//! delay queue, quantization or saturation formula is duplicated here.
use super::{
    DriverBoundary, EmbeddedDriverBank, EmbeddedMotorBank, Generalized, MotorBoundary,
    SampledMotorControl,
};
use sim_core::{Behavior, BehaviorRegistry, Context, View};
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedServoConfig {
    pub dof: String,
    pub parameters: BTreeMap<String, f64>,
}
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServoBoundary {
    pub target_rad: f64,
    pub supply_voltage_v: f64,
    pub winding_temperature_k: f64,
}
struct BoundServo {
    equations: Box<dyn Behavior>,
    start: usize,
    count: usize,
    joint: usize,
}
pub struct EmbeddedServoBank {
    servos: Vec<BoundServo>,
    names: Vec<String>,
    motor_layout: Vec<(usize, usize, usize)>,
    state_count: usize,
}
impl EmbeddedServoBank {
    pub fn new(
        motors: &EmbeddedMotorBank,
        configs: &[EmbeddedServoConfig],
    ) -> Result<Self, String> {
        let names = motors.dof_names();
        let layout = motors.state_layout();
        if configs.len() != names.len() || configs.iter().zip(&names).any(|(c, n)| &c.dof != n) {
            return Err("one servo per motor in exact named order is required".into());
        }
        let mut registry = BehaviorRegistry::default();
        crate::motor::register(&mut registry).map_err(|e| e.to_string())?;
        let descriptor = registry
            .get(&crate::SERVO_FIRMWARE.into())
            .map_err(|e| e.to_string())?;
        let mut servos = Vec::new();
        let mut state_count = 0;
        for (config, motor) in configs.iter().zip(&layout) {
            descriptor
                .validate_parameters(&config.parameters)
                .map_err(|e| e.to_string())?;
            let delay = config.parameters.get("latency").copied().unwrap_or(0.0)
                * config
                    .parameters
                    .get("rate")
                    .copied()
                    .unwrap_or(50.0)
                    .max(1.0);
            if !delay.is_finite() || delay.round() > 65536.0 {
                return Err("servo delay queue exceeds 65536-sample adapter capacity".into());
            }
            let equations =
                descriptor.equations.ok_or("missing servo firmware")?(&config.parameters)
                    .map_err(|e| e.to_string())?;
            let states = equations.states();
            if states.len() < 5
                || states[0].name != "command"
                || states.last().unwrap().name != "next_sample"
            {
                return Err("registered servo state layout changed".into());
            }
            servos.push(BoundServo {
                equations,
                start: state_count,
                count: states.len(),
                joint: motor.2,
            });
            state_count += states.len();
        }
        Ok(Self {
            servos,
            names,
            motor_layout: layout,
            state_count,
        })
    }
    pub fn initial_states(&self) -> Vec<f64> {
        self.servos
            .iter()
            .flat_map(|s| s.equations.states().into_iter().map(|v| v.initial))
            .collect()
    }
    pub fn state_layout(&self) -> Vec<(usize, usize, usize)> {
        self.servos
            .iter()
            .map(|s| (s.start, s.count, s.joint))
            .collect()
    }
    fn validate(
        &self,
        time: f64,
        g: &Generalized,
        state: &[f64],
        inputs: &[ServoBoundary],
    ) -> Result<(), String> {
        if !time.is_finite()
            || time < 0.0
            || state.len() != self.state_count
            || state.iter().any(|v| !v.is_finite())
            || inputs.len() != self.servos.len()
            || self.servos.iter().any(|s| {
                s.joint >= g.q.len()
                    || s.joint >= g.qd.len()
                    || !g.q[s.joint].is_finite()
                    || !g.qd[s.joint].is_finite()
            })
            || inputs.iter().any(|i| {
                !i.target_rad.is_finite()
                    || !i.supply_voltage_v.is_finite()
                    || i.supply_voltage_v < 0.0
                    || !i.winding_temperature_k.is_finite()
                    || i.winding_temperature_k <= 0.0
            })
        {
            return Err("invalid servo state, measurement or boundary".into());
        }
        Ok(())
    }
    pub fn commands(
        &self,
        time: f64,
        g: &Generalized,
        state: &[f64],
        inputs: &[ServoBoundary],
    ) -> Result<Vec<f64>, String> {
        self.validate(time, g, state, inputs)?;
        self.servos
            .iter()
            .zip(inputs)
            .map(|(s, input)| {
                let mut residual = vec![0.0; s.count];
                let mut output = [0.0];
                s.equations.residual(&mut Context::new(
                    time,
                    &state[s.start..s.start + s.count],
                    &vec![0.0; s.count],
                    &[0; 5],
                    &[None; 4],
                    &[],
                    &[],
                    &[input.target_rad, g.q[s.joint], g.qd[s.joint]],
                    &mut residual,
                    &mut [],
                    &mut output,
                ));
                if !output[0].is_finite()
                    || output[0].abs() > 1.0
                    || residual.iter().any(|v| !v.is_finite() || *v != 0.0)
                {
                    return Err("servo is not a finite held duty controller".into());
                }
                Ok(output[0])
            })
            .collect()
    }
    pub fn event_data(
        &self,
        time: f64,
        g: &Generalized,
        state: &[f64],
        inputs: &[ServoBoundary],
    ) -> Result<(Vec<f64>, Vec<(usize, f64)>), String> {
        self.validate(time, g, state, inputs)?;
        let mut guards = Vec::new();
        let mut scheduled = Vec::new();
        for (index, (s, input)) in self.servos.iter().zip(inputs).enumerate() {
            let signals = [input.target_rad, g.q[s.joint], g.qd[s.joint]];
            let view = View {
                time,
                states: &state[s.start..s.start + s.count],
                offsets: &[0; 5],
                rate_map: &[None; 4],
                across: &[],
                across_rates: &[],
                signals_in: &signals,
            };
            let mut local = Vec::new();
            let mut deadlines = Vec::new();
            s.equations.guards(&view, &mut local);
            s.equations.scheduled_events(&view, &mut deadlines);
            if local.len() != 1
                || !local[0].is_finite()
                || deadlines.len() != 1
                || deadlines[0].0 != 0
                || !deadlines[0].1.is_finite()
            {
                return Err("servo clock layout changed or is nonfinite".into());
            }
            guards.push(local[0]);
            scheduled.push((index, deadlines[0].1));
        }
        Ok((guards, scheduled))
    }
    pub fn jump(
        &mut self,
        index: usize,
        time: f64,
        g: &Generalized,
        state: &mut [f64],
        inputs: &[ServoBoundary],
    ) -> Result<(), String> {
        self.validate(time, g, state, inputs)?;
        let s = self.servos.get_mut(index).ok_or("unknown servo clock")?;
        let input = inputs[index];
        let deadline = state[s.start + s.count - 1];
        if (time - deadline).abs() > 128.0 * f64::EPSILON * time.abs().max(deadline.abs()) {
            return Err("servo tick is not at its declared deadline".into());
        }
        let old = state[s.start..s.start + s.count].to_vec();
        let signals = [input.target_rad, g.q[s.joint], g.qd[s.joint]];
        let view = View {
            time,
            states: &old,
            offsets: &[0; 5],
            rate_map: &[None; 4],
            across: &[],
            across_rates: &[],
            signals_in: &signals,
        };
        s.equations
            .jump(0, &view, &mut state[s.start..s.start + s.count]);
        let updated = &state[s.start..s.start + s.count];
        if updated.iter().any(|v| !v.is_finite())
            || updated[s.count - 1] <= time
            || updated[0].abs() > 1.0
        {
            state[s.start..s.start + s.count].copy_from_slice(&old);
            return Err("invalid servo tick output or clock".into());
        }
        Ok(())
    }
    pub fn connect<'a>(
        &'a mut self,
        motors: &EmbeddedMotorBank,
        drivers: &'a EmbeddedDriverBank,
        inputs: &'a [ServoBoundary],
    ) -> Result<EmbeddedServoControl<'a>, String> {
        drivers.validate_binding(motors)?;
        if self.names != motors.dof_names()
            || self.motor_layout != motors.state_layout()
            || inputs.len() != self.servos.len()
        {
            return Err("servo/motor binding mismatch".into());
        }
        Ok(EmbeddedServoControl {
            servos: self,
            drivers,
            inputs,
            target_law: None,
        })
    }
    /// Supply a pure time-to-target law in named motor order (radians). The
    /// registered firmware consumes targets only at its existing sample ticks;
    /// voltage and temperature boundaries remain those supplied to connect.
    /// Trial calls must have no side effects. No PID or scheduling is duplicated.
    pub fn connect_target_law<'a>(
        &'a mut self,
        motors: &EmbeddedMotorBank,
        drivers: &'a EmbeddedDriverBank,
        inputs: &'a [ServoBoundary],
        law: &'a dyn Fn(f64) -> Result<Vec<f64>, String>,
    ) -> Result<EmbeddedServoControl<'a>, String> {
        let mut control = self.connect(motors, drivers, inputs)?;
        control.target_law = Some(law);
        Ok(control)
    }
}
pub struct EmbeddedServoControl<'a> {
    servos: &'a mut EmbeddedServoBank,
    drivers: &'a EmbeddedDriverBank,
    inputs: &'a [ServoBoundary],
    target_law: Option<&'a dyn Fn(f64) -> Result<Vec<f64>, String>>,
}
impl EmbeddedServoControl<'_> {
    fn inputs_at(&self, t: f64) -> Result<std::borrow::Cow<'_, [ServoBoundary]>, String> {
        let Some(law) = self.target_law else {
            return Ok(std::borrow::Cow::Borrowed(self.inputs));
        };
        let targets = law(t)?;
        if targets.len() != self.inputs.len() || targets.iter().any(|v| !v.is_finite()) {
            return Err("invalid servo target-law output".into());
        }
        let inputs = self
            .inputs
            .iter()
            .zip(targets)
            .map(|(input, target_rad)| ServoBoundary {
                target_rad,
                ..*input
            })
            .collect();
        Ok(std::borrow::Cow::Owned(inputs))
    }
}
impl SampledMotorControl for EmbeddedServoControl<'_> {
    type State = Vec<f64>;
    fn independent_motor_boundaries(&self) -> bool {
        // Firmware commands use held states and mechanics, not motor states.
        // Each registered bridge reads only its own motor current. Supply and
        // temperature are explicit imposed inputs; no shared supply solve here.
        true
    }
    fn boundaries(
        &self,
        t: f64,
        g: &Generalized,
        motors: &[f64],
        held: &Self::State,
    ) -> Result<Vec<MotorBoundary>, String> {
        let boundaries = self.inputs_at(t)?;
        let commands = self.servos.commands(t, g, held, &boundaries)?;
        let inputs: Vec<_> = boundaries
            .iter()
            .zip(commands)
            .map(|(i, duty)| DriverBoundary {
                supply_voltage_v: i.supply_voltage_v,
                duty,
                winding_temperature_k: i.winding_temperature_k,
            })
            .collect();
        self.drivers.evaluate(t, motors, &inputs).map(|r| r.0)
    }
    fn guards(&self, t: f64, g: &Generalized, held: &Self::State) -> Result<Vec<f64>, String> {
        self.servos
            .event_data(t, g, held, &self.inputs_at(t)?)
            .map(|r| r.0)
    }
    fn scheduled(
        &self,
        t: f64,
        g: &Generalized,
        held: &Self::State,
    ) -> Result<Vec<(usize, f64)>, String> {
        self.servos
            .event_data(t, g, held, &self.inputs_at(t)?)
            .map(|r| r.1)
    }
    fn permits_jacobian_reuse_after_sample(&self, guard: usize) -> bool {
        // Registered firmware has one scheduled sampling guard per servo.
        // It updates only held commands, delay queue and firmware state.
        guard < self.servos.servos.len()
    }
    fn jump(
        &mut self,
        i: usize,
        t: f64,
        g: &Generalized,
        held: &mut Self::State,
    ) -> Result<(), String> {
        let inputs = self.inputs_at(t)?.into_owned();
        self.servos.jump(i, t, g, held, &inputs)
    }
}
