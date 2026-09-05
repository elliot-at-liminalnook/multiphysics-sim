//! Rigid bodies, contacts and a few classic mechanisms as compiled behaviors.
//!
//! A rigid body owns a `Frame` connector: its thirteen states — position,
//! unit quaternion (w, x, y, z), world velocity, body-frame angular
//! velocity — *are* the frame's across bundle, and attachments push world
//! wrenches back into its six twist rows. The world is z-up.

use nalgebra::{Matrix3, UnitQuaternion, Vector3};
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, Provision, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or, signal_in,
};
use std::collections::BTreeMap;

pub const RIGID_BODY: &str = "multibody.rigid_body";
pub const SPHERE_CONTACT: &str = "multibody.sphere_contact";
pub const COMPASS_WALKER: &str = "multibody.compass_walker";
pub const DRIVEN_PENDULUM: &str = "multibody.driven_pendulum";
pub const PENDULUM_ON_CART: &str = "multibody.pendulum_on_cart";
pub const PITCH_PLUNGE_SECTION: &str = "multibody.pitch_plunge_section";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

fn v3(s: &[f64], at: usize) -> Vector3<f64> {
    Vector3::new(s[at], s[at + 1], s[at + 2])
}
fn quat(s: &[f64], at: usize) -> UnitQuaternion<f64> {
    UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(s[at], s[at + 1], s[at + 2], s[at + 3]))
}
fn raw_quat(s: &[f64], at: usize) -> nalgebra::Quaternion<f64> {
    nalgebra::Quaternion::new(s[at], s[at + 1], s[at + 2], s[at + 3])
}

/// Free rigid body with diagonal body-frame inertia and uniform gravity.
pub struct RigidBody {
    pub mass: f64,
    pub inertia: Vector3<f64>,
    pub gravity: f64,
    pub initial: [f64; 13],
}
impl RigidBody {
    /// World-frame angular momentum from a frame state bundle.
    pub fn angular_momentum_world(&self, s: &[f64]) -> Vector3<f64> {
        let q = quat(s, 3);
        q * (self.inertia.component_mul(&v3(s, 10)))
    }
}
impl Behavior for RigidBody {
    fn owned_frame(&self) -> Option<usize> { Some(0) }
    fn states(&self) -> Vec<StateDeclaration> {
        let names = ["x", "y", "z", "qw", "qx", "qy", "qz", "vx", "vy", "vz", "wx", "wy", "wz"];
        let kinds = [
            QuantityKind::Length, QuantityKind::Length, QuantityKind::Length,
            QuantityKind::Dimensionless, QuantityKind::Dimensionless, QuantityKind::Dimensionless, QuantityKind::Dimensionless,
            QuantityKind::LinearVelocity, QuantityKind::LinearVelocity, QuantityKind::LinearVelocity,
            QuantityKind::AngularVelocity, QuantityKind::AngularVelocity, QuantityKind::AngularVelocity,
        ];
        names.iter().zip(kinds).zip(self.initial).map(|((n, k), v)| StateDeclaration::new(*n, k, v)).collect()
    }
    fn residual(&self, ctx: &mut Context) {
        let s = ctx.states().to_vec();
        let d = ctx.state_rates().to_vec();
        let v = v3(&s, 7);
        let w = v3(&s, 10);
        for k in 0..3 {
            ctx.set_state_residual(k, d[k] - v[k]);
        }
        // q̇ = ½ q ⊗ (0, ω_body)
        let q = raw_quat(&s, 3);
        let omega_q = nalgebra::Quaternion::new(0.0, w.x, w.y, w.z);
        let qdot = q * omega_q * 0.5;
        ctx.set_state_residual(3, d[3] - qdot.w);
        ctx.set_state_residual(4, d[4] - qdot.i);
        ctx.set_state_residual(5, d[5] - qdot.j);
        ctx.set_state_residual(6, d[6] - qdot.k);
        // Linear: m·v̇ − m·g (gravity along −z) — attachments add their through.
        let vdot = v3(&d, 7);
        let inertial = self.mass * vdot + Vector3::new(0.0, 0.0, self.mass * self.gravity);
        for k in 0..3 {
            ctx.set_state_residual(7 + k, inertial[k]);
        }
        // Angular, expressed in the world frame so attachments' world torques add.
        let wdot = v3(&d, 10);
        let euler = self.inertia.component_mul(&wdot) + w.cross(&self.inertia.component_mul(&w));
        let world = quat(&s, 3) * euler;
        for k in 0..3 {
            ctx.set_state_residual(10 + k, world[k]);
        }
    }
    fn energy(&self, view: &View) -> f64 {
        let s = view.states;
        let v = v3(s, 7);
        let w = v3(s, 10);
        0.5 * self.mass * v.dot(&v) + 0.5 * w.dot(&self.inertia.component_mul(&w)) + self.mass * self.gravity * s[2]
    }
}
fn rigid_body(p: &Params) -> Made {
    let names = ["x", "y", "z", "qw", "qx", "qy", "qz", "vx", "vy", "vz", "wx", "wy", "wz"];
    let mut initial = [0.0; 13];
    initial[3] = 1.0;
    for (k, n) in names.iter().enumerate() {
        initial[k] = param_or(p, &format!("initial.{n}"), param_or(p, &format!("initial.frame.{n}"), initial[k]));
    }
    // The compiler validates the complete quaternion after gathering initial
    // frame values from every connected attachment, including partial tuples.
    Ok(Box::new(RigidBody {
        mass: param(p, "mass")?,
        inertia: Vector3::new(param(p, "ixx")?, param(p, "iyy")?, param(p, "izz")?),
        gravity: param_or(p, "gravity", 0.0),
        initial,
    }))
}

/// Sphere of `radius` whose centre sits `offset` along the body's z axis
/// from the centre of mass, pressed on the plane z = 0 with a penalty
/// normal and regularised Coulomb friction.
pub struct SphereContact {
    pub radius: f64,
    pub offset: f64,
    pub stiffness: f64,
    pub damping: f64,
    pub friction: f64,
    pub regularisation: f64,
}
impl SphereContact {
    /// Contact force and torque on the body (world frame), and stored energy.
    pub fn wrench(&self, s: &[f64]) -> (Vector3<f64>, Vector3<f64>, f64) {
        let r = v3(s, 0);
        let q = quat(s, 3);
        let v = v3(s, 7);
        let w_world = q * v3(s, 10);
        let axis = q * Vector3::z();
        let centre = r + self.offset * axis;
        let point = centre - self.radius * Vector3::z();
        let penetration = -point.z;
        if penetration <= 0.0 {
            return (Vector3::zeros(), Vector3::zeros(), 0.0);
        }
        let arm = point - r;
        let velocity = v + w_world.cross(&arm);
        let normal = (self.stiffness * penetration - self.damping * velocity.z).max(0.0);
        let slip = Vector3::new(velocity.x, velocity.y, 0.0);
        let speed = slip.norm();
        let friction = if speed > 0.0 { -self.friction * normal * (speed / self.regularisation).tanh() / speed * slip } else { Vector3::zeros() };
        let force = Vector3::new(0.0, 0.0, normal) + friction;
        (force, arm.cross(&force), 0.5 * self.stiffness * penetration * penetration)
    }
}
impl Behavior for SphereContact {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let (force, torque, _) = self.wrench(ctx.across_bundle(0));
        for k in 0..3 {
            ctx.add_through_lane(0, k, -force[k]);
            ctx.add_through_lane(0, 3 + k, -torque[k]);
        }
    }
    fn energy(&self, view: &View) -> f64 {
        self.wrench(view.across_bundle(0)).2
    }
}
fn sphere_contact(p: &Params) -> Made {
    Ok(Box::new(SphereContact {
        radius: param(p, "radius")?,
        offset: param_or(p, "offset", 0.0),
        stiffness: param(p, "stiffness")?,
        damping: param_or(p, "damping", 0.0),
        friction: param_or(p, "friction", 0.0),
        regularisation: param_or(p, "regularisation", 5.0e-3),
    }))
}

/// Garcia, Chatterjee, Ruina & Coleman's simplest walking model
/// in physical time t = τ·time_scale (time_scale = sqrt(length/gravity)).
/// Stance angle θ is from the slope normal, φ is the inter-leg angle;
/// heel strike occurs when the swing foot reaches the ground past vertical.
pub struct CompassWalker {
    pub slope: f64,
    pub time_scale: f64,
    pub elastic: bool,
    pub initial: [f64; 4],
}
impl Behavior for CompassWalker {
    fn states(&self) -> Vec<StateDeclaration> {
        let names = ["theta", "phi", "theta_dot", "phi_dot"];
        let kinds = [QuantityKind::Angle, QuantityKind::Angle, QuantityKind::AngularVelocity, QuantityKind::AngularVelocity];
        names.iter().zip(kinds).zip(self.initial).map(|((n, kind), v)| StateDeclaration::new(*n, kind, v)).collect()
    }
    fn residual(&self, ctx: &mut Context) {
        let (theta, phi, theta_d, phi_d) = (ctx.state(0), ctx.state(1), ctx.state(2), ctx.state(3));
        let g = self.slope;
        let inverse_time_squared = 1. / self.time_scale.powi(2);
        ctx.set_state_residual(0, ctx.state_rate(0) - theta_d);
        ctx.set_state_residual(1, ctx.state_rate(1) - phi_d);
        ctx.set_state_residual(2, ctx.state_rate(2) - inverse_time_squared * (theta - g).sin());
        ctx.set_state_residual(3, ctx.state_rate(3) - (inverse_time_squared * ((theta - g).sin() - (theta - g).cos() * phi.sin()) + theta_d * theta_d * phi.sin()));
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        let (theta, phi) = (view.state(0), view.state(1));
        let height = theta.cos() - (theta - phi).cos();
        out.push(if theta < -0.05 { height } else { 1.0 });
    }
    fn jump(&mut self, _index: usize, _view: &View, x: &mut [f64]) {
        let (theta, theta_d) = (x[0], x[2]);
        let c = (2.0 * theta).cos();
        if self.elastic {
            x[3] = (1.0 - c) * theta_d;
        } else {
            x[2] = c * theta_d;
            x[3] = c * (1.0 - c) * theta_d;
        }
        x[0] = -theta;
        x[1] = -2.0 * theta;
    }
}
fn compass_walker(p: &Params) -> Made {
    let time_scale = param_or(p, "time_scale", 1.);
    Ok(Box::new(CompassWalker {
        slope: param(p, "slope")?,
        time_scale,
        elastic: param_or(p, "elastic", 0.0) > 0.5,
        initial: [param_or(p, "initial.theta", 0.2), param_or(p, "initial.phi", 0.4), param_or(p, "initial.theta_dot", -0.2 / time_scale), param_or(p, "initial.phi_dot", -0.015 / time_scale)],
    }))
}

/// Pendulum whose pivot accelerates vertically (signal input, up positive);
/// the angle is measured from the *upward* vertical.
pub struct DrivenPendulum {
    pub length: f64,
    pub gravity: f64,
    pub initial_angle: f64,
}
impl Behavior for DrivenPendulum {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![
            StateDeclaration::new("angle", QuantityKind::Angle, self.initial_angle),
            StateDeclaration::new("rate", QuantityKind::AngularVelocity, 0.0),
        ]
    }
    fn residual(&self, ctx: &mut Context) {
        let pivot = ctx.signal_in(0);
        ctx.set_state_residual(0, ctx.state_rate(0) - ctx.state(1));
        ctx.set_state_residual(1, ctx.state_rate(1) - (self.gravity + pivot) / self.length * ctx.state(0).sin());
    }
}
fn driven_pendulum(p: &Params) -> Made {
    Ok(Box::new(DrivenPendulum { length: param(p, "length")?, gravity: param_or(p, "gravity", 9.81), initial_angle: param_or(p, "initial.angle", 0.0) }))
}

/// Pendulum hanging from a translational node, with an escapement that
/// kicks it at every zero crossing.
pub struct PendulumOnCart {
    pub mass: f64,
    pub length: f64,
    pub damping: f64,
    pub gravity: f64,
    pub kick: f64,
    pub initial_angle: f64,
    pub initial_rate: f64,
}
impl Behavior for PendulumOnCart {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![
            StateDeclaration::new("angle", QuantityKind::Angle, self.initial_angle),
            StateDeclaration::new("rate", QuantityKind::AngularVelocity, self.initial_rate),
            StateDeclaration::new("cart_velocity", QuantityKind::LinearVelocity, 0.0),
        ]
    }
    fn residual(&self, ctx: &mut Context) {
        let (m, l, g) = (self.mass, self.length, self.gravity);
        let (theta, omega, u) = (ctx.state(0), ctx.state(1), ctx.state(2));
        let (omega_dot, u_dot) = (ctx.state_rate(1), ctx.state_rate(2));
        ctx.set_state_residual(0, ctx.state_rate(0) - omega);
        ctx.set_state_residual(2, u - ctx.across_rate(0));
        ctx.set_state_residual(1, m * l * l * omega_dot + m * l * u_dot * theta.cos() + m * g * l * theta.sin() + self.damping * omega);
        ctx.add_through(0, m * u_dot + m * l * (omega_dot * theta.cos() - omega * omega * theta.sin()));
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        out.push(view.state(0));
        out.push(-view.state(0));
    }
    fn jump(&mut self, _index: usize, _view: &View, x: &mut [f64]) {
        x[1] += self.kick * x[1].signum();
    }
    fn energy(&self, view: &View) -> f64 {
        let (m, l, g) = (self.mass, self.length, self.gravity);
        let (theta, omega, u) = (view.state(0), view.state(1), view.state(2));
        0.5 * m * (u * u + 2.0 * u * l * omega * theta.cos() + l * l * omega * omega) - m * g * l * theta.cos()
    }
}
fn pendulum_on_cart(p: &Params) -> Made {
    Ok(Box::new(PendulumOnCart {
        mass: param(p, "mass")?,
        length: param(p, "length")?,
        damping: param_or(p, "damping", 0.0),
        gravity: param_or(p, "gravity", 9.81),
        kick: param_or(p, "escapement_kick", 0.0),
        initial_angle: param_or(p, "initial.angle", 0.0),
        initial_rate: param_or(p, "initial.rate", 0.0),
    }))
}

/// Two-degree-of-freedom typical section: plunge mass and pitch inertia
/// coupled by static unbalance, each sprung and damped to ground.
pub struct PitchPlungeSection {
    pub mass: f64,
    pub unbalance: f64,
    pub pitch_inertia: f64,
    pub plunge_stiffness: f64,
    pub plunge_damping: f64,
    pub pitch_stiffness: f64,
    pub pitch_damping: f64,
    /// Lock pitch at zero: the pitch state becomes the holding moment.
    pub pitch_locked: bool,
}
impl Behavior for PitchPlungeSection {
    fn provides(&self) -> Vec<Provision> {
        let mut p = vec![Provision { port: 0, lane: 1, state: 0 }];
        if !self.pitch_locked {
            p.push(Provision { port: 1, lane: 1, state: 1 });
        }
        p
    }
    fn states(&self) -> Vec<StateDeclaration> {
        vec![
            StateDeclaration::new("plunge_rate", QuantityKind::LinearVelocity, 0.0),
            StateDeclaration::new(if self.pitch_locked { "holding_moment" } else { "pitch_rate" }, if self.pitch_locked { QuantityKind::Torque } else { QuantityKind::AngularVelocity }, 0.0),
        ]
    }
    fn residual(&self, ctx: &mut Context) {
        let hd = ctx.state(0);
        let hdd = ctx.state_rate(0);
        ctx.set_state_residual(0, hd - ctx.across_derivative(0, 0));
        if self.pitch_locked {
            ctx.set_state_residual(1, ctx.across(1));
            let plunge = self.mass * hdd + self.plunge_damping * hd + self.plunge_stiffness * ctx.across(0);
            ctx.add_through(0, plunge);
            ctx.add_through(1, ctx.state(1));
            return;
        }
        let ad = ctx.state(1);
        let add = ctx.state_rate(1);
        ctx.set_state_residual(1, ad - ctx.across_derivative(1, 0));
        let plunge = self.mass * hdd + self.unbalance * add + self.plunge_damping * hd + self.plunge_stiffness * ctx.across(0);
        let pitch = self.unbalance * hdd + self.pitch_inertia * add + self.pitch_damping * ad + self.pitch_stiffness * ctx.across(1);
        ctx.add_through(0, plunge);
        ctx.add_through(1, pitch);
    }
    fn energy(&self, view: &View) -> f64 {
        let (hd, ad) = (view.state(0), if self.pitch_locked { 0. } else { view.state(1) });
        0.5 * self.mass * hd * hd + self.unbalance * hd * ad + 0.5 * self.pitch_inertia * ad * ad
            + 0.5 * self.plunge_stiffness * view.across(0).powi(2) + 0.5 * self.pitch_stiffness * view.across(1).powi(2)
    }
}
fn pitch_plunge_section(p: &Params) -> Made {
    let mass = param(p, "mass")?;
    let pitch_inertia = param(p, "pitch_inertia")?;
    let unbalance = param_or(p, "unbalance", 0.);
    if unbalance.abs() / mass.sqrt() >= pitch_inertia.sqrt() {
        return Err(sim_core::EquationError::InvalidParameter("unbalance".into(), "mass * pitch_inertia must exceed unbalance² for positive kinetic energy".into()));
    }
    Ok(Box::new(PitchPlungeSection {
        mass,
        unbalance,
        pitch_inertia,
        plunge_stiffness: param(p, "plunge_stiffness")?,
        plunge_damping: param_or(p, "plunge_damping", 0.0),
        pitch_stiffness: param(p, "pitch_stiffness")?,
        pitch_damping: param_or(p, "pitch_damping", 0.0),
        pitch_locked: param_or(p, "pitch_locked", 0.0) > 0.5,
    }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use ConnectorKind::{Frame as F, Rotational as R, Translational as T};
    use sim_core::ParameterDeclaration as P;
    let mut body = vec![P::required("mass", "kg").positive(), P::required("ixx", "kg·m²").positive(),
        P::required("iyy", "kg·m²").positive(), P::required("izz", "kg·m²").positive(), P::optional("gravity", "m/s²", 0.)];
    for (names, unit) in [(&["x", "y", "z"][..], "m"), (&["qw", "qx", "qy", "qz"][..], "1"),
        (&["vx", "vy", "vz"][..], "m/s"), (&["wx", "wy", "wz"][..], "rad/s")] {
        body.extend(names.iter().map(|name| P::optional(format!("initial.{name}"), unit, if *name == "qw" { 1. } else { 0. })));
    }
    let mut walker = vec![P::required("slope", "rad"), P::optional("elastic", "1", 0.).integer(0., 1.),
        P::optional("time_scale", "s", 1.).positive(), P::optional("initial.theta", "rad", 0.2), P::optional("initial.phi", "rad", 0.4)];
    for (name, default) in [("initial.theta_dot", "-0.2 / time_scale"), ("initial.phi_dot", "-0.015 / time_scale")] {
        let mut declaration = P::alternative(name, "rad/s");
        declaration.default_label = Some(default.into());
        walker.push(declaration);
    }
    for descriptor in [
        BehaviorDescriptor::new(RIGID_BODY, "Free rigid body", vec![acausal("frame", F)], rigid_body).with_parameters(body),
        BehaviorDescriptor::new(SPHERE_CONTACT, "Sphere on a plane with friction", vec![acausal("frame", F)], sphere_contact).with_parameters(vec![
            P::required("radius", "m").positive(), P::optional("offset", "m", 0.), P::required("stiffness", "N/m").positive(),
            P::optional("damping", "N·s/m", 0.).nonnegative(), P::optional("friction", "1", 0.).nonnegative(), P::optional("regularisation", "m/s", 5e-3).positive()]),
        BehaviorDescriptor::new(COMPASS_WALKER, "Simplest walking model with explicit time scale", Vec::new(), compass_walker).with_parameters(walker),
        BehaviorDescriptor::new(DRIVEN_PENDULUM, "Pendulum on a vertically driven pivot", vec![signal_in("pivot_acceleration", QuantityKind::LinearAcceleration)], driven_pendulum).with_parameters(vec![
            P::required("length", "m").positive(), P::optional("gravity", "m/s²", 9.81), P::optional("initial.angle", "rad", 0.)]),
        BehaviorDescriptor::new(PENDULUM_ON_CART, "Escapement pendulum on a cart", vec![acausal("cart", T)], pendulum_on_cart).with_parameters(vec![
            P::required("mass", "kg").positive(), P::required("length", "m").positive(), P::optional("damping", "N·m·s/rad", 0.).nonnegative(),
            P::optional("gravity", "m/s²", 9.81), P::optional("escapement_kick", "rad/s", 0.).nonnegative(),
            P::optional("initial.angle", "rad", 0.), P::optional("initial.rate", "rad/s", 0.)]),
        BehaviorDescriptor::new(PITCH_PLUNGE_SECTION, "Pitch–plunge typical section", vec![acausal("plunge", T), acausal("pitch", R)], pitch_plunge_section).with_parameters(vec![
            P::required("mass", "kg").positive(), P::optional("unbalance", "kg·m", 0.), P::required("pitch_inertia", "kg·m²").positive(),
            P::required("plunge_stiffness", "N/m").nonnegative(), P::optional("plunge_damping", "N·s/m", 0.).nonnegative(),
            P::required("pitch_stiffness", "N·m/rad").nonnegative(), P::optional("pitch_damping", "N·m·s/rad", 0.).nonnegative(), P::optional("pitch_locked", "1", 0.).integer(0., 1.)]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}

/// Unused helper kept for attachments that need the body's inertia tensor in the world frame.
#[allow(dead_code)]
fn world_inertia(body: &RigidBody, s: &[f64]) -> Matrix3<f64> {
    let r = quat(s, 3).to_rotation_matrix();
    r.matrix() * Matrix3::from_diagonal(&body.inertia) * r.matrix().transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locking_pitch_reports_torque_without_storing_reaction_energy() {
        let parameters = Params::from_iter([
            ("mass".into(), 2.), ("pitch_inertia".into(), 1.), ("unbalance".into(), 0.5),
            ("plunge_stiffness".into(), 10.), ("pitch_stiffness".into(), 20.), ("pitch_locked".into(), 1.),
        ]);
        let section = pitch_plunge_section(&parameters).unwrap();
        assert_eq!(section.states()[1].kind, QuantityKind::Torque);
        assert_eq!(section.states()[1].name, "holding_moment");
        for reaction in [-1000., 0., 1000.] {
            let view = View { time: 0., states: &[3., reaction], offsets: &[0, 2, 4], rate_map: &[],
                across: &[0.2, 3., 0., 0.], across_rates: &[0.; 4], signals_in: &[] };
            assert!((section.energy(&view) - 9.2).abs() < 1e-12);
        }
    }
}
