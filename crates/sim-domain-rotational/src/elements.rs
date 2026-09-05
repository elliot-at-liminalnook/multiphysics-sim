//! One-dimensional rotational elements as compiled behaviors.
//!
//! Sign convention (shared by every domain): a through variable is positive
//! *into* the behavior, and every node sums its ports' throughs to zero.

use sim_core::{
    Behavior, Input, LocalJacobian, Output, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, Provision, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or, signal_in, signal_out,
};
use std::collections::BTreeMap;

pub const INERTIA: &str = "rotational.inertia";
pub const SPRING: &str = "rotational.spring";
pub const DAMPER: &str = "rotational.damper";
pub const GROUND: &str = "rotational.ground";
pub const TORQUE_SOURCE: &str = "rotational.torque_source";
pub const ANGLE_SENSOR: &str = "rotational.angle_sensor";
pub const SPEED_SENSOR: &str = "rotational.speed_sensor";
pub const BACKLASH_MESH: &str = "rotational.backlash_mesh";
pub const IDEAL_GEAR: &str = "rotational.ideal_gear";
pub const AVERAGE_SPEED_SENSOR: &str = "rotational.average_speed_sensor";
pub const SPEED_TRIP: &str = "rotational.speed_trip";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// Rigid inertia with viscous drag; owns its speed, which *is* the shaft's
/// speed lane, and reads its angle from the node.
pub struct Inertia {
    pub inertia: f64,
    pub damping: f64,
    pub initial_speed: f64,
}
impl Behavior for Inertia {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("speed", QuantityKind::AngularVelocity, self.initial_speed)]
    }
    fn provides(&self) -> Vec<Provision> {
        vec![Provision { port: 0, lane: 1, state: 0 }]
    }
    fn residual(&self, ctx: &mut Context) {
        let omega = ctx.state(0);
        ctx.set_state_residual(0, omega - ctx.across_derivative(0, 0));
        let torque_in = self.inertia * ctx.state_rate(0) + self.damping * omega;
        ctx.add_through(0, torque_in);
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.inertia * view.state(0).powi(2)
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        out.state_state(0, 0, 1.0);
        out.set(Output::State(0), Input::AcrossDerivative(0, 0), -1.0);
        out.through(0, Input::StateRate(0), self.inertia);
        out.through(0, Input::State(0), self.damping);
        true
    }
}
fn inertia(p: &Params) -> Made {
    Ok(Box::new(Inertia { inertia: param(p, "inertia")?, damping: param_or(p, "damping", 0.0), initial_speed: param_or(p, "initial.speed", 0.0) }))
}

pub struct Spring {
    pub stiffness: f64,
}
impl Behavior for Spring {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let torque = self.stiffness * (ctx.across(0) - ctx.across(1));
        ctx.add_through(0, torque);
        ctx.add_through(1, -torque);
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.stiffness * (view.across(0) - view.across(1)).powi(2)
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        for (port, sign) in [(0, 1.0), (1, -1.0)] {
            out.through(port, Input::Across(0, 0), sign * self.stiffness);
            out.through(port, Input::Across(1, 0), -sign * self.stiffness);
        }
        true
    }
}
fn spring(p: &Params) -> Made {
    Ok(Box::new(Spring { stiffness: param(p, "stiffness")? }))
}

pub struct Damper {
    pub damping: f64,
}
impl Behavior for Damper {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let torque = self.damping * (ctx.across_rate(0) - ctx.across_rate(1));
        ctx.add_through(0, torque);
        ctx.add_through(1, -torque);
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        for (port, sign) in [(0, 1.0), (1, -1.0)] {
            out.through(port, Input::AcrossRate(0, 0), sign * self.damping);
            out.through(port, Input::AcrossRate(1, 0), -sign * self.damping);
        }
        true
    }
}
fn damper(p: &Params) -> Made {
    Ok(Box::new(Damper { damping: param(p, "damping")? }))
}

/// Holds its node at a fixed angle; the reaction torque is its one state.
pub struct Ground {
    pub angle: f64,
}
impl Behavior for Ground {
    fn pinned(&self) -> Vec<(usize, usize, f64)> {
        vec![(0, 0, 0.0)]
    }
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("reaction", QuantityKind::Torque, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.across(0) - self.angle);
        ctx.add_through(0, ctx.state(0));
    }
}
fn ground(p: &Params) -> Made {
    Ok(Box::new(Ground { angle: param_or(p, "angle", 0.0) }))
}

/// Applies the commanded torque to its node.
pub struct TorqueSource;
impl Behavior for TorqueSource {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let command = ctx.signal_in(0);
        ctx.add_through(0, -command);
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        out.through(0, Input::Signal(0), -1.0);
        true
    }
}
fn torque_source(_: &Params) -> Made {
    Ok(Box::new(TorqueSource))
}

pub struct AngleSensor;
impl Behavior for AngleSensor {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let angle = ctx.across(0);
        ctx.set_signal(0, angle);
    }
}
fn angle_sensor(_: &Params) -> Made {
    Ok(Box::new(AngleSensor))
}

/// Reads the shaft's exact speed lane.
pub struct SpeedSensor;
impl Behavior for SpeedSensor {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let speed = ctx.across_rate(0);
        ctx.set_signal(0, speed);
    }
}
fn speed_sensor(_: &Params) -> Made {
    Ok(Box::new(SpeedSensor))
}

/// Reads the step-average angle rate — what a sensor saw before the speed
/// lane existed. Kept as the falsifier for plate 14.
pub struct AverageSpeedSensor;
impl Behavior for AverageSpeedSensor {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let speed = ctx.across_derivative(0, 0);
        ctx.set_signal(0, speed);
    }
}
fn average_speed_sensor(_: &Params) -> Made {
    Ok(Box::new(AverageSpeedSensor))
}

/// Compliant mesh with a dead zone of half-width `gap`.
pub struct BacklashMesh {
    pub stiffness: f64,
    pub gap: f64,
}
impl Behavior for BacklashMesh {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let twist = ctx.across(0) - ctx.across(1);
        let torque = self.stiffness * (twist.abs() - self.gap).max(0.0) * twist.signum();
        ctx.add_through(0, torque);
        ctx.add_through(1, -torque);
    }
    fn energy(&self, view: &View) -> f64 {
        let twist = view.across(0) - view.across(1);
        0.5 * self.stiffness * (twist.abs() - self.gap).max(0.0).powi(2)
    }
}
fn backlash_mesh(p: &Params) -> Made {
    Ok(Box::new(BacklashMesh { stiffness: param(p, "stiffness")?, gap: param_or(p, "gap", 0.0) }))
}

/// Ideal gear: `angle_in = ratio · angle_out`, enforced by a multiplier that
/// is the input-side torque. The constraint is imposed at velocity level
/// with a position correction (Baumgarte), which keeps the DAE at index 2
/// for the implicit midpoint rule.
pub struct IdealGear {
    pub ratio: f64,
    pub correction: f64,
}
impl Behavior for IdealGear {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("constraint_torque", QuantityKind::Torque, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let drift = ctx.across(0) - self.ratio * ctx.across(1);
        let slip = ctx.across_rate(0) - self.ratio * ctx.across_rate(1);
        ctx.set_state_residual(0, slip + self.correction * drift);
        let lambda = ctx.state(0);
        ctx.add_through(0, lambda);
        ctx.add_through(1, -self.ratio * lambda);
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        out.set(Output::State(0), Input::AcrossRate(0, 0), 1.0);
        out.set(Output::State(0), Input::AcrossRate(1, 0), -self.ratio);
        out.set(Output::State(0), Input::Across(0, 0), self.correction);
        out.set(Output::State(0), Input::Across(1, 0), -self.correction * self.ratio);
        out.through(0, Input::State(0), 1.0);
        out.through(1, Input::State(0), -self.ratio);
        true
    }
}
fn ideal_gear(p: &Params) -> Made {
    Ok(Box::new(IdealGear { ratio: param(p, "ratio")?, correction: param_or(p, "correction", 10.0) }))
}

/// Latches the instant the shaft speed falls through `threshold` — a guard
/// on the exact speed lane, which is what the lane exists for.
pub struct SpeedTrip {
    pub threshold: f64,
}
impl Behavior for SpeedTrip {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("trip_time", QuantityKind::Time, -1.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.state_rate(0));
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        out.push(if view.state(0) >= 0.0 { 1.0 } else { view.across_rate(0) - self.threshold });
    }
    fn jump(&mut self, _index: usize, view: &View, states: &mut [f64]) {
        states[0] = view.time;
    }
}
fn speed_trip(p: &Params) -> Made {
    Ok(Box::new(SpeedTrip { threshold: param(p, "threshold")? }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use ConnectorKind::Rotational as R;
    for descriptor in [
        BehaviorDescriptor::new(INERTIA, "Rotational inertia", vec![acausal("shaft", R)], inertia).with_parameters(vec![P::required("inertia", "kg·m²").positive(), P::optional("damping", "N·m·s/rad", 0.0), P::optional("initial.speed", "rad/s", 0.0)]),
        BehaviorDescriptor::new(SPRING, "Torsional spring", vec![acausal("a", R), acausal("b", R)], spring).with_parameters(vec![P::required("stiffness", "N·m/rad")]),
        BehaviorDescriptor::new(DAMPER, "Torsional damper", vec![acausal("a", R), acausal("b", R)], damper).with_parameters(vec![P::required("damping", "N·m·s/rad")]),
        BehaviorDescriptor::new(GROUND, "Fixed angle", vec![acausal("flange", R)], ground).with_parameters(vec![P::optional("angle", "rad", 0.0)]),
        BehaviorDescriptor::new(TORQUE_SOURCE, "Commanded torque", vec![acausal("shaft", R), signal_in("torque", QuantityKind::Torque)], torque_source).with_parameters(vec![]),
        BehaviorDescriptor::new(ANGLE_SENSOR, "Angle sensor", vec![acausal("shaft", R), signal_out("angle", QuantityKind::Angle)], angle_sensor).with_parameters(vec![]),
        BehaviorDescriptor::new(SPEED_SENSOR, "Speed sensor", vec![acausal("shaft", R), signal_out("speed", QuantityKind::AngularVelocity)], speed_sensor).with_parameters(vec![]),
        BehaviorDescriptor::new(AVERAGE_SPEED_SENSOR, "Step-average speed sensor", vec![acausal("shaft", R), signal_out("speed", QuantityKind::AngularVelocity)], average_speed_sensor).with_parameters(vec![]),
        BehaviorDescriptor::new(SPEED_TRIP, "Speed trip latch", vec![acausal("shaft", R)], speed_trip).with_parameters(vec![P::required("threshold", "rad/s")]),
        BehaviorDescriptor::new(BACKLASH_MESH, "Compliant mesh with backlash", vec![acausal("a", R), acausal("b", R)], backlash_mesh).with_parameters(vec![P::required("stiffness", "N·m/rad"), P::optional("gap", "rad", 0.0).nonnegative()]),
        BehaviorDescriptor::new(IDEAL_GEAR, "Ideal gear", vec![acausal("input", R), acausal("output", R)], ideal_gear).with_parameters(vec![P::required("ratio", "1"), P::optional("correction", "1/s", 10.0)]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}
