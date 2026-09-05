//! One-dimensional translational elements as compiled behaviors.

use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, Provision, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or, signal_in, signal_out,
};
use std::collections::BTreeMap;

pub const MASS: &str = "translational.mass";
pub const SPRING: &str = "translational.spring";
pub const DAMPER: &str = "translational.damper";
pub const GROUND: &str = "translational.ground";
pub const DOUBLE_WELL: &str = "translational.double_well";
pub const LANGEVIN: &str = "translational.langevin";
pub const FORCE_SOURCE: &str = "translational.force_source";
pub const POSITION_SENSOR: &str = "translational.position_sensor";
pub const BELT_FRICTION: &str = "translational.belt_friction";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

pub struct Mass {
    pub mass: f64,
    pub damping: f64,
    pub initial_velocity: f64,
}
impl Behavior for Mass {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("velocity", QuantityKind::LinearVelocity, self.initial_velocity)]
    }
    fn provides(&self) -> Vec<Provision> {
        vec![Provision { port: 0, lane: 1, state: 0 }]
    }
    fn residual(&self, ctx: &mut Context) {
        let v = ctx.state(0);
        ctx.set_state_residual(0, v - ctx.across_derivative(0, 0));
        let force_in = self.mass * ctx.state_rate(0) + self.damping * v;
        ctx.add_through(0, force_in);
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.mass * view.state(0).powi(2)
    }
}
fn mass(p: &Params) -> Made {
    Ok(Box::new(Mass { mass: param(p, "mass")?, damping: param_or(p, "damping", 0.0), initial_velocity: param_or(p, "initial.velocity", 0.0) }))
}

pub struct Spring {
    pub stiffness: f64,
    pub rest: f64,
}
impl Behavior for Spring {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let force = self.stiffness * (ctx.across(0) - ctx.across(1) - self.rest);
        ctx.add_through(0, force);
        ctx.add_through(1, -force);
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.stiffness * (view.across(0) - view.across(1) - self.rest).powi(2)
    }
}
fn spring(p: &Params) -> Made {
    Ok(Box::new(Spring { stiffness: param(p, "stiffness")?, rest: param_or(p, "rest", 0.0) }))
}

pub struct Damper {
    pub damping: f64,
}
impl Behavior for Damper {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let force = self.damping * (ctx.across_rate(0) - ctx.across_rate(1));
        ctx.add_through(0, force);
        ctx.add_through(1, -force);
    }
}
fn damper(p: &Params) -> Made {
    Ok(Box::new(Damper { damping: param(p, "damping")? }))
}

pub struct Ground {
    pub position: f64,
}
impl Behavior for Ground {
    fn pinned(&self) -> Vec<(usize, usize, f64)> {
        vec![(0, 0, 0.0)]
    }
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("reaction", QuantityKind::Force, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.across(0) - self.position);
        ctx.add_through(0, ctx.state(0));
    }
}
fn ground(p: &Params) -> Made {
    Ok(Box::new(Ground { position: param_or(p, "position", 0.0) }))
}

pub struct ForceSource;
impl Behavior for ForceSource {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let command = ctx.signal_in(0);
        ctx.add_through(0, -command);
    }
}
fn force_source(_: &Params) -> Made {
    Ok(Box::new(ForceSource))
}

pub struct PositionSensor;
impl Behavior for PositionSensor {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let x = ctx.across(0);
        ctx.set_signal(0, x);
    }
}
fn position_sensor(_: &Params) -> Made {
    Ok(Box::new(PositionSensor))
}

/// Stribeck friction against a belt moving at `belt_speed`.
pub struct BeltFriction {
    pub normal_force: f64,
    pub static_friction: f64,
    pub kinetic_friction: f64,
    pub stribeck_velocity: f64,
    pub regularisation: f64,
    pub belt_speed: f64,
}
impl BeltFriction {
    /// Friction force on the attached body as a function of slip `belt − body`.
    pub fn force(&self, slip: f64) -> f64 {
        let magnitude = self.kinetic_friction + (self.static_friction - self.kinetic_friction) * (-slip.abs() / self.stribeck_velocity).exp();
        self.normal_force * magnitude * (slip / self.regularisation).tanh()
    }
    /// Slope of the friction curve at positive slip.
    pub fn slope(&self, slip: f64) -> f64 {
        -self.normal_force * (self.static_friction - self.kinetic_friction) / self.stribeck_velocity * (-slip / self.stribeck_velocity).exp()
    }
}
impl Behavior for BeltFriction {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let force = self.force(self.belt_speed - ctx.across_rate(0));
        ctx.add_through(0, -force);
    }
}
fn belt_friction(p: &Params) -> Made {
    Ok(Box::new(BeltFriction {
        normal_force: param(p, "normal_force")?,
        static_friction: param(p, "static_friction")?,
        kinetic_friction: param(p, "kinetic_friction")?,
        stribeck_velocity: param(p, "stribeck_velocity")?,
        regularisation: param_or(p, "regularisation", 1.0e-4),
        belt_speed: param(p, "belt_speed")?,
    }))
}

/// A bistable spring: potential `−a·x²/2 + b·x⁴/4`, wells at `±√(a/b)`,
/// barrier `a²/(4b)`.
pub struct DoubleWell {
    pub a: f64,
    pub b: f64,
}
impl Behavior for DoubleWell {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let x = ctx.across(0);
        ctx.add_through(0, -self.a * x + self.b * x * x * x);
    }
    fn energy(&self, view: &View) -> f64 {
        let x = view.across(0);
        -0.5 * self.a * x * x + 0.25 * self.b * x.powi(4)
    }
}
fn double_well(p: &Params) -> Made {
    Ok(Box::new(DoubleWell { a: param(p, "a")?, b: param(p, "b")? }))
}

/// A Langevin bath on a node: viscous `damping`, a white force of
/// `intensity` (`2γkT` for a bath at kT), and an optional periodic drive
/// `amplitude·cos(2π·frequency·t)`.
pub struct Langevin {
    pub damping: f64,
    pub intensity: f64,
    pub drive_amplitude: f64,
    pub drive_frequency: f64,
}
impl Behavior for Langevin {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let drive = self.drive_amplitude * (2.0 * std::f64::consts::PI * self.drive_frequency * ctx.time).cos();
        ctx.add_through(0, self.damping * ctx.across_rate(0) - drive);
        ctx.add_noise(0, self.intensity);
    }
}
fn langevin(p: &Params) -> Made {
    Ok(Box::new(Langevin { damping: param(p, "damping")?, intensity: param_or(p, "intensity", 0.0), drive_amplitude: param_or(p, "drive_amplitude", 0.0), drive_frequency: param_or(p, "drive_frequency", 0.0) }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    registry.register(BehaviorDescriptor::new(DOUBLE_WELL, "Bistable spring", vec![acausal("axis", ConnectorKind::Translational)], double_well).with_parameters(vec![P::required("a", "N/m"), P::required("b", "N/m³")]))?;
    registry.register(BehaviorDescriptor::new(LANGEVIN, "Langevin bath with drive", vec![acausal("axis", ConnectorKind::Translational)], langevin).with_parameters(vec![P::required("damping", "N·s/m"), P::optional("intensity", "N²·s", 0.0).nonnegative(), P::optional("drive_amplitude", "N", 0.0), P::optional("drive_frequency", "Hz", 0.0)]))?;
    use ConnectorKind::Translational as T;
    for descriptor in [
        BehaviorDescriptor::new(MASS, "Point mass", vec![acausal("axis", T)], mass).with_parameters(vec![P::required("mass", "kg").positive(), P::optional("damping", "N·s/m", 0.0), P::optional("initial.velocity", "m/s", 0.0)]),
        BehaviorDescriptor::new(SPRING, "Linear spring", vec![acausal("a", T), acausal("b", T)], spring).with_parameters(vec![P::required("stiffness", "N/m"), P::optional("rest", "m", 0.0)]),
        BehaviorDescriptor::new(DAMPER, "Linear damper", vec![acausal("a", T), acausal("b", T)], damper).with_parameters(vec![P::required("damping", "N·s/m")]),
        BehaviorDescriptor::new(GROUND, "Fixed position", vec![acausal("axis", T)], ground).with_parameters(vec![P::optional("position", "m", 0.0)]),
        BehaviorDescriptor::new(FORCE_SOURCE, "Commanded force", vec![acausal("axis", T), signal_in("force", QuantityKind::Force)], force_source).with_parameters(vec![]),
        BehaviorDescriptor::new(POSITION_SENSOR, "Position sensor", vec![acausal("axis", T), signal_out("position", QuantityKind::Length)], position_sensor).with_parameters(vec![]),
        BehaviorDescriptor::new(BELT_FRICTION, "Belt with Stribeck friction", vec![acausal("axis", T)], belt_friction).with_parameters(vec![P::required("normal_force", "N"), P::required("static_friction", "1"), P::required("kinetic_friction", "1"), P::required("stribeck_velocity", "m/s").positive(), P::optional("regularisation", "m/s", 1.0e-4).positive(), P::required("belt_speed", "m/s")]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}
