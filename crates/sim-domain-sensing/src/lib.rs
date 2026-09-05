//! Sensors and actuators: the boundary a controller sees, made as real as
//! the plant — bandwidth, latency, sample-and-hold, quantisation, noise and
//! faults on the way in; dead bands, limits and lags on the way out.
//!
//! Every sensor computes only its raw value from its ports and hands it to
//! a [`Chain`], which owns the pipeline's states and events. Common sensor
//! parameters: `bandwidth` (Hz), `latency` (s) with `stages`, `period` (s),
//! `quantum`, `noise` with `seed`, and `fault.{mode,time,duration,samples}`.

mod chain;

use chain::Chain;
use sim_core::{
    Behavior, Input, LocalJacobian, Output, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or, signal_in, signal_out,
};
use std::collections::BTreeMap;
use std::f64::consts::TAU;

pub const ENCODER: &str = "sensor.encoder";
pub const TACHOMETER: &str = "sensor.tachometer";
pub const IMU: &str = "sensor.imu";
pub const CURRENT_SENSOR: &str = "sensor.current";
pub const VOLTAGE_SENSOR: &str = "sensor.voltage";
pub const FORCE_SENSOR: &str = "sensor.force";
pub const PWM_DRIVER: &str = "actuator.pwm_driver";
pub const SERVO: &str = "actuator.servo";
pub const QUANTISER: &str = "actuator.quantiser";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// A one-port sensor reading a lane of its node through one chain.
macro_rules! lane_sensor {
    ($(#[$doc:meta])* $name:ident, $kind:expr, |$ctx:ident| $raw:expr, $partials:expr) => {
        $(#[$doc])*
        pub struct $name {
            chain: Chain,
        }
        impl Behavior for $name {
            fn states(&self) -> Vec<StateDeclaration> {
                self.chain.states("", $kind)
            }
            fn residual(&self, $ctx: &mut Context) {
                let raw = $raw;
                let value = self.chain.residual($ctx, raw);
                $ctx.set_signal(0, value);
            }
            fn guards(&self, view: &View, out: &mut Vec<f64>) {
                self.chain.guards(view, out)
            }
            fn jump(&mut self, index: usize, view: &View, states: &mut [f64]) {
                self.chain.jump(index, view, states)
            }
            fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
                let published = self.chain.jacobian(out, &$partials);
                for (input, v) in published {
                    out.set(Output::Signal(0), input, v);
                }
                true
            }
        }
    };
}

lane_sensor!(
    /// Shaft angle, optionally in `counts` per turn (which sets the quantum).
    Encoder, QuantityKind::Angle, |ctx| ctx.across(0), [(Input::Across(0, 0), 1.0)]
);
fn encoder(p: &Params) -> Made {
    let mut chain = Chain::new(p)?;
    let counts = param_or(p, "counts", 0.0);
    if counts > 0.0 {
        if param_or(p, "period", 0.) <= 0. {
            return Err(sim_core::EquationError::InvalidParameter("counts".into(), "encoder quantisation needs `period > 0`".into()));
        }
        chain.quantum = TAU / counts;
    }
    Ok(Box::new(Encoder { chain }))
}

lane_sensor!(
    /// Shaft speed, read from the exact speed lane.
    Tachometer, QuantityKind::AngularVelocity, |ctx| ctx.across_rate(0), [(Input::AcrossRate(0, 0), 1.0)]
);
fn tachometer(p: &Params) -> Made {
    Ok(Box::new(Tachometer { chain: Chain::new(p)? }))
}

lane_sensor!(
    /// Ideal voltmeter: reads `v_p − v_n`, draws no current.
    VoltageSensor, QuantityKind::Voltage, |ctx| ctx.across(0) - ctx.across(1), [(Input::Across(0, 0), 1.0), (Input::Across(1, 0), -1.0)]
);
fn voltage_sensor(p: &Params) -> Made {
    Ok(Box::new(VoltageSensor { chain: Chain::new(p)? }))
}

/// A series element that measures its own through variable: an ammeter or
/// a load cell. Its `flow` is a multiplier state enforcing equal across
/// values at its two ports, positive into `a`/`p`.
pub struct ThroughSensor {
    chain: Chain,
    kind: QuantityKind,
}
impl Behavior for ThroughSensor {
    fn states(&self) -> Vec<StateDeclaration> {
        let mut states = vec![StateDeclaration::new("flow", self.kind, 0.0)];
        states.extend(self.chain.states("", self.kind));
        states
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.across(0) - ctx.across(1));
        let flow = ctx.state(0);
        ctx.add_through(0, flow);
        ctx.add_through(1, -flow);
        let value = self.chain.residual(ctx, flow);
        ctx.set_signal(0, value);
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        out.set(Output::State(0), Input::Across(0, 0), 1.0);
        out.set(Output::State(0), Input::Across(1, 0), -1.0);
        out.through(0, Input::State(0), 1.0);
        out.through(1, Input::State(0), -1.0);
        let published = self.chain.jacobian(out, &[(Input::State(0), 1.0)]);
        for (input, v) in published {
            out.set(Output::Signal(0), input, v);
        }
        true
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        self.chain.guards(view, out)
    }
    fn jump(&mut self, index: usize, view: &View, states: &mut [f64]) {
        self.chain.jump(index, view, states)
    }
}
fn current_sensor(p: &Params) -> Made {
    Ok(Box::new(ThroughSensor { chain: Chain::new(p)?.at(1), kind: QuantityKind::Current }))
}
fn force_sensor(p: &Params) -> Made {
    Ok(Box::new(ThroughSensor { chain: Chain::new(p)?.at(1), kind: QuantityKind::Force }))
}

/// Planar inertial unit on a rigid body: specific force `a − g` rotated into
/// the body frame (so a supported body reads `+gravity` up its own y-axis
/// and a free-falling one reads zero), plus the body's rate `ω`. Each
/// channel carries a `bias` and runs its own copy of the shared chain.
pub struct Imu {
    gravity: f64,
    bias: [f64; 3],
    chains: [Chain; 3],
}
impl Behavior for Imu {
    fn states(&self) -> Vec<StateDeclaration> {
        let kinds = [QuantityKind::LinearAcceleration, QuantityKind::LinearAcceleration, QuantityKind::AngularVelocity];
        ["ax", "ay", "gyro"].iter().zip(&self.chains).zip(kinds).flat_map(|((name, chain), kind)| chain.states(name, kind)).collect()
    }
    fn residual(&self, ctx: &mut Context) {
        let (theta, omega) = (ctx.across_lane(0, 2), ctx.across_lane(0, 5));
        let (fx, fy) = (ctx.across_rate_lane(0, 3), ctx.across_rate_lane(0, 4) + self.gravity);
        let (c, s) = (theta.cos(), theta.sin());
        let raw = [c * fx + s * fy + self.bias[0], c * fy - s * fx + self.bias[1], omega + self.bias[2]];
        for (k, chain) in self.chains.iter().enumerate() {
            let value = chain.residual(ctx, raw[k]);
            ctx.set_signal(k, value);
        }
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        self.chains.iter().for_each(|chain| chain.guards(view, out));
    }
    fn jump(&mut self, index: usize, view: &View, states: &mut [f64]) {
        let per = self.chains[0].guard_count();
        self.chains[index / per].jump(index % per, view, states);
    }
}
fn imu(p: &Params) -> Made {
    let mut base = 0;
    let mut channel = |name: &str, stream| -> Result<Chain, sim_core::EquationError> {
        let mut params = p.clone();
        for setting in ["noise", "quantum"] {
            params.insert(setting.into(), param_or(p, &format!("{setting}.{name}"), 0.));
        }
        let chain = Chain::new(&params)?.stream(stream).at(base);
        base += chain.len();
        Ok(chain)
    };
    Ok(Box::new(Imu {
        gravity: param_or(p, "gravity", 9.81),
        bias: [param_or(p, "bias.ax", 0.0), param_or(p, "bias.ay", 0.0), param_or(p, "bias.gyro", 0.0)],
        chains: [channel("ax", 0)?, channel("ay", 1)?, channel("gyro", 2)?],
    }))
}

/// H-bridge as a controlled voltage source: `v = supply · clamp(duty, ±1)`,
/// zero inside the dead band, behind an on-resistance; its current is a
/// multiplier state, positive into `p`.
pub struct PwmDriver {
    supply: f64,
    resistance: f64,
    dead_band: f64,
}
impl Behavior for PwmDriver {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("current", QuantityKind::Current, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let duty = ctx.signal_in(0);
        let voltage = if duty.abs() < self.dead_band { 0.0 } else { self.supply * duty.clamp(-1.0, 1.0) };
        let i = ctx.state(0);
        ctx.set_state_residual(0, ctx.across(0) - ctx.across(1) - voltage - self.resistance * i);
        ctx.add_through(0, i);
        ctx.add_through(1, -i);
    }
}
fn pwm_driver(p: &Params) -> Made {
    Ok(Box::new(PwmDriver { supply: param(p, "supply")?, resistance: param_or(p, "resistance", 0.0), dead_band: param_or(p, "dead_band", 0.0) }))
}

/// Torque-controlled servo: the shaft torque follows the clamped command
/// through a first-order lag of bandwidth `bandwidth`; the reported current
/// is `torque / torque_constant`, and `current_limit` bounds both.
pub struct Servo {
    bandwidth: f64,
    torque_limit: f64,
    torque_constant: f64,
    current_limit: f64,
}
impl Behavior for Servo {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("torque", QuantityKind::Torque, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let limit = self.torque_limit.min(self.current_limit * self.torque_constant);
        let command = ctx.signal_in(0).clamp(-limit, limit);
        let torque = ctx.state(0);
        ctx.set_state_residual(0, ctx.state_rate(0) - TAU * self.bandwidth * (command - torque));
        ctx.add_through(0, -torque);
        ctx.set_signal(0, (torque / self.torque_constant).clamp(-self.current_limit, self.current_limit));
    }
}
fn servo(p: &Params) -> Made {
    Ok(Box::new(Servo {
        bandwidth: param(p, "bandwidth")?,
        torque_limit: param_or(p, "torque_limit", f64::INFINITY),
        torque_constant: param_or(p, "torque_constant", 1.0),
        current_limit: param_or(p, "current_limit", f64::INFINITY),
    }))
}

/// `output = clamp(round(input / step) · step, ±limit)`: a DAC or a command
/// word of finite resolution.
pub struct Quantiser {
    step: f64,
    limit: f64,
}
impl Behavior for Quantiser {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let value = (ctx.signal_in(0) / self.step).round() * self.step;
        ctx.set_signal(0, value.clamp(-self.limit, self.limit));
    }
}
fn quantiser(p: &Params) -> Made {
    Ok(Box::new(Quantiser { step: param(p, "step")?, limit: param_or(p, "limit", f64::INFINITY) }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use ConnectorKind::{Electrical as E, PlanarFrame as F, Rotational as R, Translational as T};
    use QuantityKind as Q;
    let mut encoder_parameters = chain::parameters(Some("rad"));
    encoder_parameters.push(P::optional("counts", "1", 0.).integer(0., 9_007_199_254_740_991.));
    let mut imu_parameters = chain::parameters(None);
    imu_parameters.push(P::optional("gravity", "m/s²", 9.81));
    for (channel, unit) in [("ax", "m/s²"), ("ay", "m/s²"), ("gyro", "rad/s")] {
        imu_parameters.extend([
            P::optional(format!("bias.{channel}"), unit, 0.),
            P::optional(format!("noise.{channel}"), unit, 0.).nonnegative(),
            P::optional(format!("quantum.{channel}"), unit, 0.).nonnegative(),
        ]);
    }
    for descriptor in [
        BehaviorDescriptor::new(ENCODER, "Shaft encoder", vec![acausal("shaft", R), signal_out("angle", Q::Angle)], encoder).with_parameters(encoder_parameters),
        BehaviorDescriptor::new(TACHOMETER, "Tachometer", vec![acausal("shaft", R), signal_out("speed", Q::AngularVelocity)], tachometer).with_parameters(chain::parameters(Some("rad/s"))),
        BehaviorDescriptor::new(IMU, "Planar inertial unit", vec![acausal("frame", F), signal_out("ax", Q::LinearAcceleration), signal_out("ay", Q::LinearAcceleration), signal_out("gyro", Q::AngularVelocity)], imu).with_parameters(imu_parameters),
        BehaviorDescriptor::new(CURRENT_SENSOR, "Current sensor", vec![acausal("p", E), acausal("n", E), signal_out("current", Q::Current)], current_sensor).with_parameters(chain::parameters(Some("A"))),
        BehaviorDescriptor::new(VOLTAGE_SENSOR, "Voltage sensor", vec![acausal("p", E), acausal("n", E), signal_out("voltage", Q::Voltage)], voltage_sensor).with_parameters(chain::parameters(Some("V"))),
        BehaviorDescriptor::new(FORCE_SENSOR, "Load cell", vec![acausal("a", T), acausal("b", T), signal_out("force", Q::Force)], force_sensor).with_parameters(chain::parameters(Some("N"))),
        BehaviorDescriptor::new(PWM_DRIVER, "PWM voltage driver", vec![acausal("p", E), acausal("n", E), signal_in("duty", Q::Dimensionless)], pwm_driver).with_parameters(vec![
            P::required("supply", "V").nonnegative(), P::optional("resistance", "Ω", 0.).nonnegative(), P::optional("dead_band", "1", 0.).nonnegative().at_most(1.)]),
        BehaviorDescriptor::new(SERVO, "Torque servo", vec![acausal("shaft", R), signal_in("command", Q::Torque), signal_out("current", Q::Current)], servo).with_parameters(vec![
            P::required("bandwidth", "Hz").positive(), P::optional("torque_limit", "N·m", f64::INFINITY).nonnegative(),
            P::optional("torque_constant", "N·m/A", 1.).positive(), P::optional("current_limit", "A", f64::INFINITY).nonnegative()]),
        BehaviorDescriptor::new(QUANTISER, "Quantiser", vec![signal_in("input", Q::Dimensionless), signal_out("output", Q::Dimensionless)], quantiser).with_parameters(vec![
            P::required("step", "1").positive(), P::optional("limit", "1", f64::INFINITY).nonnegative()]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}
