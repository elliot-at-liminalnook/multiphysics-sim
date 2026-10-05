//! The sampled angular-feedback/duty controller block and a reusable bounded position PID.
//! No actuator physics; hardware and simulated hosts call the same control law.
use serde::{Deserialize, Serialize};
use sim_core::{
    BlockInterface, BlockPort, BlockTiming, Clock, ImplementationRef, Instance, ModelWorld,
    QuantityKind as Q, RegistryError,
};

/// The host implementation name the feedback block declares.
pub const HOST: &str = "angular_pwm_feedback";

/// The feedback controller's block interface: `angle` in (plus the
/// electrical readings `supply_voltage`, `supply_current`, `winding_current`
/// when `electrical`), `duty` out (starts at 0, within [-1, 1]). The host
/// controller answers at the sample instant (feedthrough).
pub fn interface(electrical: bool) -> BlockInterface {
    let mut inputs = vec![BlockPort::new("angle", Q::Angle)];
    if electrical {
        inputs.extend([
            BlockPort::new("supply_voltage", Q::Voltage),
            BlockPort::new("supply_current", Q::Current),
            BlockPort::new("winding_current", Q::Current),
        ]);
    }
    BlockInterface {
        inputs,
        outputs: vec![BlockPort::new("duty", Q::Dimensionless).start(0.0).range(Some(-1.0), Some(1.0))],
        feedthrough: true,
    }
}

/// Add a feedback controller block named `name`, ticking on `clock` (a
/// period, or the captured acquisition times of a recorded experiment).
/// Bind its implementation with `Runtime::bind_coupler`.
pub fn add(world: &mut ModelWorld, name: &str, electrical: bool, clock: Clock) -> Result<Instance, RegistryError> {
    world.add_block(name, interface(electrical), BlockTiming::with_clock(clock), ImplementationRef::Host { name: HOST.into() })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pid {
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
    pub integral_limit: f64,
    pub duty_limit: f64,
}
impl Default for Pid {
    fn default() -> Self {
        Self {
            kp: 0.8,
            ki: 0.,
            kd: 0.03,
            integral_limit: 0.2,
            duty_limit: 0.2,
        }
    }
}
impl Pid {
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.kp,
            self.ki,
            self.kd,
            self.integral_limit,
            self.duty_limit,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.)
            || self.duty_limit == 0.
            || self.duty_limit > 1.
        {
            Err("PID values must be finite/nonnegative and duty limit in (0,1]".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PidState {
    pub integral: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Action {
    pub duty: f64,
    pub unsaturated: f64,
    pub saturated: bool,
}
impl PidState {
    pub fn step(
        &mut self,
        c: &Pid,
        target: f64,
        angle: f64,
        velocity: f64,
        dt: f64,
    ) -> Result<Action, String> {
        c.validate()?;
        if [target, angle, velocity, dt].iter().any(|v| !v.is_finite()) || dt <= 0. {
            return Err("Invalid feedback state or elapsed time".into());
        }
        let error = target - angle;
        let candidate = (self.integral + error * dt).clamp(-c.integral_limit, c.integral_limit);
        let raw = c.kp * error + c.ki * candidate - c.kd * velocity;
        // Conditional integration permits unwinding while preventing further saturation.
        if raw.abs() <= c.duty_limit || error * raw < 0. {
            self.integral = candidate;
        }
        let raw = c.kp * error + c.ki * self.integral - c.kd * velocity;
        if !raw.is_finite() {
            return Err("Nonfinite PID output".into());
        }
        Ok(Action {
            duty: raw.clamp(-c.duty_limit, c.duty_limit),
            unsaturated: raw,
            saturated: raw.abs() > c.duty_limit,
        })
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EncoderEstimate {
    pub last_time: Option<f64>,
    pub last_angle: f64,
    pub position: f64,
    pub velocity: f64,
}
impl EncoderEstimate {
    /// Unwrap a cyclic encoder; repeated observations retain the previous estimate.
    pub fn observe(
        &mut self,
        time: f64,
        angle: f64,
        wrap: f64,
        tau: f64,
    ) -> Result<(f64, f64), String> {
        if [time, angle, wrap, tau].iter().any(|v| !v.is_finite()) || wrap <= 0. || tau < 0. {
            return Err("Invalid encoder observation".into());
        }
        if let Some(previous) = self.last_time {
            if time < previous {
                return Err("Encoder observations arrived out of time order".into());
            }
            if time == previous {
                return Ok((self.position, self.velocity));
            }
            let delta = (angle - self.last_angle + wrap * 0.5).rem_euclid(wrap) - wrap * 0.5;
            let dt = time - previous;
            self.position += delta;
            let alpha = if tau == 0. { 1. } else { dt / (tau + dt) };
            self.velocity += alpha * (delta / dt - self.velocity);
        } else {
            self.position = angle;
        }
        self.last_angle = angle;
        self.last_time = Some(time);
        Ok((self.position, self.velocity))
    }
}
