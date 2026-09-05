//! Planar (x, y) point-mass mechanics on `Planar` connectors: masses, rods,
//! bending springs, pins, sliders, anchored springs and constant forces.

use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or,
};
use std::collections::BTreeMap;

pub const POINT_MASS: &str = "planar.point_mass";
pub const ROD: &str = "planar.rod";
pub const BEND: &str = "planar.bend";
pub const PIN: &str = "planar.pin";
pub const SLIDER_MASS: &str = "planar.slider_mass";
pub const FORCE: &str = "planar.force";
pub const ANCHORED_SPRING: &str = "planar.anchored_spring";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

pub struct PointMass {
    pub mass: f64,
    pub gravity: f64,
    pub damping: f64,
    pub initial_velocity: [f64; 2],
}
impl Behavior for PointMass {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![
            StateDeclaration::new("vx", QuantityKind::LinearVelocity, self.initial_velocity[0]),
            StateDeclaration::new("vy", QuantityKind::LinearVelocity, self.initial_velocity[1]),
        ]
    }
    fn residual(&self, ctx: &mut Context) {
        for lane in 0..2 {
            ctx.set_state_residual(lane, ctx.state(lane) - ctx.across_rate_lane(0, lane));
        }
        let fx = self.mass * ctx.state_rate(0) + self.damping * ctx.state(0);
        let fy = self.mass * ctx.state_rate(1) + self.damping * ctx.state(1) + self.mass * self.gravity;
        ctx.add_through_lane(0, 0, fx);
        ctx.add_through_lane(0, 1, fy);
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.mass * (view.state(0).powi(2) + view.state(1).powi(2)) + self.mass * self.gravity * view.across_lane(0, 1)
    }
}
fn point_mass(p: &Params) -> Made {
    Ok(Box::new(PointMass { mass: param(p, "mass")?, gravity: param_or(p, "gravity", 0.0), damping: param_or(p, "damping", 0.0), initial_velocity: [param_or(p, "initial.vx", 0.0), param_or(p, "initial.vy", 0.0)] }))
}

/// Axial spring between two planar nodes.
pub struct Rod {
    pub stiffness: f64,
    pub rest_length: f64,
}
impl Rod {
    fn geometry(a: &[f64], b: &[f64]) -> ([f64; 2], f64) {
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1.0e-12);
        ([d[0] / len, d[1] / len], len)
    }
}
impl Behavior for Rod {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let (u, len) = Self::geometry(ctx.across_bundle(0), ctx.across_bundle(1));
        let tension = self.stiffness * (len - self.rest_length);
        // Tension pulls a toward b: force on a = +t·u, so through into rod at a = −t·u.
        ctx.add_through_lane(0, 0, -tension * u[0]);
        ctx.add_through_lane(0, 1, -tension * u[1]);
        ctx.add_through_lane(1, 0, tension * u[0]);
        ctx.add_through_lane(1, 1, tension * u[1]);
    }
    fn energy(&self, view: &View) -> f64 {
        let (_, len) = Self::geometry(view.across_bundle(0), view.across_bundle(1));
        0.5 * self.stiffness * (len - self.rest_length).powi(2)
    }
}
fn rod(p: &Params) -> Made {
    Ok(Box::new(Rod { stiffness: param(p, "stiffness")?, rest_length: param(p, "rest_length")? }))
}

/// Bending spring at node b between segments a→b and b→c: `V = k(1 − û₁·û₂)`.
pub struct Bend {
    pub stiffness: f64,
}
impl Behavior for Bend {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let (a, b, c) = (ctx.across_bundle(0).to_vec(), ctx.across_bundle(1).to_vec(), ctx.across_bundle(2).to_vec());
        let (u0, l0) = Rod::geometry(&a, &b);
        let (u1, l1) = Rod::geometry(&b, &c);
        let dot = u0[0] * u1[0] + u0[1] * u1[1];
        let g0 = [(u1[0] - dot * u0[0]) / l0, (u1[1] - dot * u0[1]) / l0];
        let g1 = [(u0[0] - dot * u1[0]) / l1, (u0[1] - dot * u1[1]) / l1];
        let k = self.stiffness;
        // Forces (−∂V/∂x) on a, b, c; through into the element is their negative.
        let fa = [-k * g0[0], -k * g0[1]];
        let fb = [k * (g0[0] - g1[0]), k * (g0[1] - g1[1])];
        let fc = [k * g1[0], k * g1[1]];
        for (port, f) in [(0, fa), (1, fb), (2, fc)] {
            ctx.add_through_lane(port, 0, -f[0]);
            ctx.add_through_lane(port, 1, -f[1]);
        }
    }
    fn energy(&self, view: &View) -> f64 {
        let (u0, _) = Rod::geometry(view.across_bundle(0), view.across_bundle(1));
        let (u1, _) = Rod::geometry(view.across_bundle(1), view.across_bundle(2));
        self.stiffness * (1.0 - (u0[0] * u1[0] + u0[1] * u1[1]))
    }
}
fn bend(p: &Params) -> Made {
    Ok(Box::new(Bend { stiffness: param(p, "stiffness")? }))
}

/// Pins a node at (x, y); the two reactions are its states.
pub const REVOLUTE: &str = "joint.revolute";
pub const PRISMATIC: &str = "joint.prismatic";
pub const FIXED: &str = "joint.fixed";

/// World position and velocity of a body-fixed point of a planar frame bundle.
fn anchor(b: &[f64], px: f64, py: f64) -> ([f64; 2], [f64; 2], [f64; 2]) {
    let (c, s) = (b[2].cos(), b[2].sin());
    let r = [c * px - s * py, s * px + c * py];
    let position = [b[0] + r[0], b[1] + r[1]];
    let velocity = [b[3] - b[5] * r[1], b[4] + b[5] * r[0]];
    (position, velocity, r)
}

/// A revolute joint between two planar frames: the anchor `(ax, ay)` in
/// frame `a` and `(bx, by)` in frame `b` coincide. Two multipliers carry
/// the pin force; each body feels it at its anchor. Position-level
/// constraints with Baumgarte stabilisation, as `contact.point_plane`.
pub struct Revolute {
    pub ax: f64,
    pub ay: f64,
    pub bx: f64,
    pub by: f64,
    pub stabilisation: f64,
}
impl Behavior for Revolute {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("fx", QuantityKind::Force, 0.0), StateDeclaration::new("fy", QuantityKind::Force, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let (pa, va, ra) = anchor(ctx.across_bundle(0), self.ax, self.ay);
        let (pb, vb, rb) = anchor(ctx.across_bundle(1), self.bx, self.by);
        let (fx, fy) = (ctx.state(0), ctx.state(1));
        // Velocity-level constraint with position feedback.
        for k in 0..2 {
            ctx.set_state_residual(k, (va[k] - vb[k]) + self.stabilisation * (pa[k] - pb[k]));
        }
        // The pin pushes `a` with (fx, fy) and `b` with the opposite.
        ctx.add_through_lane(0, 0, -fx);
        ctx.add_through_lane(0, 1, -fy);
        ctx.add_through_lane(0, 2, -(ra[0] * fy - ra[1] * fx));
        ctx.add_through_lane(1, 0, fx);
        ctx.add_through_lane(1, 1, fy);
        ctx.add_through_lane(1, 2, rb[0] * fy - rb[1] * fx);
    }
}
fn revolute(p: &Params) -> Made {
    Ok(Box::new(Revolute { ax: param_or(p, "ax", 0.0), ay: param_or(p, "ay", 0.0), bx: param_or(p, "bx", 0.0), by: param_or(p, "by", 0.0), stabilisation: param_or(p, "stabilisation", 20.0) }))
}

/// A fixed joint: the anchors coincide and the frames keep their relative
/// angle `offset`. Three multipliers.
pub struct Fixed {
    pub ax: f64,
    pub ay: f64,
    pub bx: f64,
    pub by: f64,
    pub offset: f64,
    pub stabilisation: f64,
}
impl Behavior for Fixed {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("fx", QuantityKind::Force, 0.0), StateDeclaration::new("fy", QuantityKind::Force, 0.0), StateDeclaration::new("torque", QuantityKind::Torque, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let a = ctx.across_bundle(0).to_vec();
        let b = ctx.across_bundle(1).to_vec();
        let (pa, va, ra) = anchor(&a, self.ax, self.ay);
        let (pb, vb, rb) = anchor(&b, self.bx, self.by);
        let (fx, fy, torque) = (ctx.state(0), ctx.state(1), ctx.state(2));
        for k in 0..2 {
            ctx.set_state_residual(k, (va[k] - vb[k]) + self.stabilisation * (pa[k] - pb[k]));
        }
        ctx.set_state_residual(2, (a[5] - b[5]) + self.stabilisation * (a[2] - b[2] - self.offset));
        ctx.add_through_lane(0, 0, -fx);
        ctx.add_through_lane(0, 1, -fy);
        ctx.add_through_lane(0, 2, -(ra[0] * fy - ra[1] * fx) - torque);
        ctx.add_through_lane(1, 0, fx);
        ctx.add_through_lane(1, 1, fy);
        ctx.add_through_lane(1, 2, rb[0] * fy - rb[1] * fx + torque);
    }
}
fn fixed(p: &Params) -> Made {
    Ok(Box::new(Fixed { ax: param_or(p, "ax", 0.0), ay: param_or(p, "ay", 0.0), bx: param_or(p, "bx", 0.0), by: param_or(p, "by", 0.0), offset: param_or(p, "offset", 0.0), stabilisation: param_or(p, "stabilisation", 20.0) }))
}

/// A prismatic joint: frame `b` slides along the unit axis `(ux, uy)` of
/// frame `a` through `a`'s anchor, keeping the relative angle `offset`.
/// Two multipliers: the lateral force and the torque.
pub struct Prismatic {
    pub ax: f64,
    pub ay: f64,
    pub bx: f64,
    pub by: f64,
    pub ux: f64,
    pub uy: f64,
    pub offset: f64,
    pub stabilisation: f64,
}
impl Behavior for Prismatic {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("lateral", QuantityKind::Force, 0.0), StateDeclaration::new("torque", QuantityKind::Torque, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let a = ctx.across_bundle(0).to_vec();
        let b = ctx.across_bundle(1).to_vec();
        let (pa, va, ra) = anchor(&a, self.ax, self.ay);
        let (pb, vb, rb) = anchor(&b, self.bx, self.by);
        // The axis and its normal in the world.
        let (c, s) = (a[2].cos(), a[2].sin());
        let axis = [c * self.ux - s * self.uy, s * self.ux + c * self.uy];
        let normal = [-axis[1], axis[0]];
        let d = [pb[0] - pa[0], pb[1] - pa[1]];
        let dv = [vb[0] - va[0], vb[1] - va[1]];
        // The normal itself rotates with `a`: d/dt(n·d) = n·dv + (ω_a × n)·d.
        let n_dot_d = normal[0] * d[0] + normal[1] * d[1];
        let rate = normal[0] * dv[0] + normal[1] * dv[1] + a[5] * (-axis[0] * d[0] - axis[1] * d[1]);
        let (lateral, torque) = (ctx.state(0), ctx.state(1));
        ctx.set_state_residual(0, rate + self.stabilisation * n_dot_d);
        ctx.set_state_residual(1, (a[5] - b[5]) + self.stabilisation * (a[2] - b[2] - self.offset));
        let f = [lateral * normal[0], lateral * normal[1]];
        ctx.add_through_lane(0, 0, -f[0]);
        ctx.add_through_lane(0, 1, -f[1]);
        ctx.add_through_lane(0, 2, -(ra[0] * f[1] - ra[1] * f[0]) - torque);
        ctx.add_through_lane(1, 0, f[0]);
        ctx.add_through_lane(1, 1, f[1]);
        ctx.add_through_lane(1, 2, rb[0] * f[1] - rb[1] * f[0] + torque);
    }
}
fn prismatic(p: &Params) -> Made {
    let axis_norm = param_or(p, "ux", 1.).hypot(param_or(p, "uy", 0.));
    if (axis_norm - 1.).abs() > 1e-9 {
        return Err(sim_core::EquationError::InvalidParameter("ux/uy".into(), "prismatic axis must have unit length".into()));
    }
    Ok(Box::new(Prismatic { ax: param_or(p, "ax", 0.0), ay: param_or(p, "ay", 0.0), bx: param_or(p, "bx", 0.0), by: param_or(p, "by", 0.0), ux: param_or(p, "ux", 1.0), uy: param_or(p, "uy", 0.0), offset: param_or(p, "offset", 0.0), stabilisation: param_or(p, "stabilisation", 20.0) }))
}

pub struct Pin {
    pub x: f64,
    pub y: f64,
}
impl Behavior for Pin {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("rx", QuantityKind::Force, 0.0), StateDeclaration::new("ry", QuantityKind::Force, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.across_lane(0, 0) - self.x);
        ctx.set_state_residual(1, ctx.across_lane(0, 1) - self.y);
        ctx.add_through_lane(0, 0, ctx.state(0));
        ctx.add_through_lane(0, 1, ctx.state(1));
    }
}
fn pin(p: &Params) -> Made {
    Ok(Box::new(Pin { x: param_or(p, "x", 0.0), y: param_or(p, "y", 0.0) }))
}

/// A point mass free along x with its y held at `y` (the reaction is a state).
pub struct SliderMass {
    pub mass: f64,
    pub damping: f64,
    pub y: f64,
}
impl Behavior for SliderMass {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("vx", QuantityKind::LinearVelocity, 0.0), StateDeclaration::new("ry", QuantityKind::Force, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.state(0) - ctx.across_rate_lane(0, 0));
        ctx.set_state_residual(1, ctx.across_lane(0, 1) - self.y);
        ctx.add_through_lane(0, 0, self.mass * ctx.state_rate(0) + self.damping * ctx.state(0));
        ctx.add_through_lane(0, 1, ctx.state(1));
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.mass * view.state(0).powi(2)
    }
}
fn slider_mass(p: &Params) -> Made {
    Ok(Box::new(SliderMass { mass: param(p, "mass")?, damping: param_or(p, "damping", 0.0), y: param_or(p, "y", 0.0) }))
}

pub struct Force {
    pub fx: f64,
    pub fy: f64,
}
impl Behavior for Force {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.add_through_lane(0, 0, -self.fx);
        ctx.add_through_lane(0, 1, -self.fy);
    }
}
fn force(p: &Params) -> Made {
    Ok(Box::new(Force { fx: param_or(p, "fx", 0.0), fy: param_or(p, "fy", 0.0) }))
}

/// Spring from a planar node to a fixed anchor.
pub struct AnchoredSpring {
    pub stiffness: f64,
    pub rest_length: f64,
    pub anchor: [f64; 2],
}
impl Behavior for AnchoredSpring {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let (u, len) = Rod::geometry(&self.anchor, ctx.across_bundle(0));
        let tension = self.stiffness * (len - self.rest_length);
        ctx.add_through_lane(0, 0, tension * u[0]);
        ctx.add_through_lane(0, 1, tension * u[1]);
    }
    fn energy(&self, view: &View) -> f64 {
        let (_, len) = Rod::geometry(&self.anchor, view.across_bundle(0));
        0.5 * self.stiffness * (len - self.rest_length).powi(2)
    }
}
fn anchored_spring(p: &Params) -> Made {
    Ok(Box::new(AnchoredSpring { stiffness: param(p, "stiffness")?, rest_length: param(p, "rest_length")?, anchor: [param_or(p, "anchor_x", 0.0), param_or(p, "anchor_y", 0.0)] }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use ConnectorKind::Planar as C;
    use sim_core::ParameterDeclaration as P;
    let joint = || vec![P::optional("ax", "m", 0.), P::optional("ay", "m", 0.),
        P::optional("bx", "m", 0.), P::optional("by", "m", 0.), P::optional("stabilisation", "1/s", 20.).nonnegative()];
    let mut fixed_parameters = joint(); fixed_parameters.push(P::optional("offset", "rad", 0.));
    let mut prismatic_parameters = fixed_parameters.clone();
    prismatic_parameters.extend([P::optional("ux", "1", 1.), P::optional("uy", "1", 0.)]);
    for descriptor in [
        BehaviorDescriptor::new(REVOLUTE, "Revolute joint", vec![acausal("a", ConnectorKind::PlanarFrame), acausal("b", ConnectorKind::PlanarFrame)], revolute).with_parameters(joint()),
        BehaviorDescriptor::new(FIXED, "Fixed joint", vec![acausal("a", ConnectorKind::PlanarFrame), acausal("b", ConnectorKind::PlanarFrame)], fixed).with_parameters(fixed_parameters),
        BehaviorDescriptor::new(PRISMATIC, "Prismatic joint", vec![acausal("a", ConnectorKind::PlanarFrame), acausal("b", ConnectorKind::PlanarFrame)], prismatic).with_parameters(prismatic_parameters),
        BehaviorDescriptor::new(POINT_MASS, "Planar point mass", vec![acausal("node", C)], point_mass).with_parameters(vec![
            P::required("mass", "kg").positive(), P::optional("gravity", "m/s²", 0.), P::optional("damping", "N·s/m", 0.).nonnegative(),
            P::optional("initial.vx", "m/s", 0.), P::optional("initial.vy", "m/s", 0.)]),
        BehaviorDescriptor::new(ROD, "Axial rod spring", vec![acausal("a", C), acausal("b", C)], rod).with_parameters(vec![
            P::required("stiffness", "N/m").nonnegative(), P::required("rest_length", "m").nonnegative()]),
        BehaviorDescriptor::new(BEND, "Bending spring", vec![acausal("a", C), acausal("b", C), acausal("c", C)], bend).with_parameters(vec![
            P::required("stiffness", "J").nonnegative()]),
        BehaviorDescriptor::new(PIN, "Pinned node", vec![acausal("node", C)], pin).with_parameters(vec![P::optional("x", "m", 0.), P::optional("y", "m", 0.)]),
        BehaviorDescriptor::new(SLIDER_MASS, "Mass on a horizontal slider", vec![acausal("node", C)], slider_mass).with_parameters(vec![
            P::required("mass", "kg").positive(), P::optional("damping", "N·s/m", 0.).nonnegative(), P::optional("y", "m", 0.)]),
        BehaviorDescriptor::new(FORCE, "Constant planar force", vec![acausal("node", C)], force).with_parameters(vec![P::optional("fx", "N", 0.), P::optional("fy", "N", 0.)]),
        BehaviorDescriptor::new(ANCHORED_SPRING, "Spring to a fixed anchor", vec![acausal("node", C)], anchored_spring).with_parameters(vec![
            P::required("stiffness", "N/m").nonnegative(), P::required("rest_length", "m").nonnegative(),
            P::optional("anchor_x", "m", 0.), P::optional("anchor_y", "m", 0.)]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}
