//! `multibody.chain`: a planar serial chain of rigid links in minimal
//! coordinates — the joint-elimination pass as a library element.
//!
//! Where `joint.revolute` keeps every body's six states and pins them with
//! multipliers, a chain owns only its joint rates: the links' poses are
//! functions of the base frame and the joint angles, and the equations of
//! motion come from a recursive Newton–Euler pass with the joint
//! accelerations as the unknowns. The chain hangs from a `base` planar
//! frame (another body's, or a fixed one) at anchor `(ax, ay)`, exposes each
//! joint as a rotational port `joint.<name>` — its across lane is the joint
//! angle, its through lane the torque the joint absorbs, so a motor plugs
//! in like it would on a shaft — and owns a `tip` planar frame at the end
//! of the last link, where a contact or a load attaches.
//!
//! Parameters: `link<i>.mass`, `link<i>.length`, `link<i>.com` (distance of
//! the centre of mass along the link, default half the length),
//! `link<i>.inertia` (about the centre of mass, default `mL²/12`),
//! `gravity`, `ax`, `ay`; `joint.<name> = i` names joint `i` (zero-based);
//! `initial.joint.<name>.angle` and `initial.joint.<name>.speed` start it.
//! Port order: `base`, `tip`, then the joints sorted by name; the tip must
//! come first in its connection (it owns that frame).

use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, Provision, QuantityKind, RegistryError, StateDeclaration, View,
    acausal, param, param_or,
};
use std::collections::BTreeMap;

pub const CHAIN: &str = "multibody.chain";

type Params = BTreeMap<String, f64>;

#[derive(Debug, Clone, Copy)]
pub struct Link {
    pub mass: f64,
    pub length: f64,
    pub com: f64,
    pub inertia: f64,
}

pub struct Chain {
    pub links: Vec<Link>,
    pub gravity: f64,
    pub anchor: [f64; 2],
    /// Joint names in chain order.
    pub joints: Vec<String>,
    /// Slot port index of each joint in chain order.
    port_of_joint: Vec<usize>,
    initial_speed: Vec<f64>,
    initial_angle: Vec<f64>,
}

const LOAD: std::ops::Range<usize> = 6..9;
const RATES: usize = 9;

fn rot(angle: f64, v: [f64; 2]) -> [f64; 2] {
    let (s, c) = angle.sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1]]
}
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
/// `ω × r` in the plane.
fn spin(omega: f64, r: [f64; 2]) -> [f64; 2] {
    [-omega * r[1], omega * r[0]]
}
fn add(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] + b[0], a[1] + b[1]]
}
fn scale(k: f64, a: [f64; 2]) -> [f64; 2] {
    [k * a[0], k * a[1]]
}

/// Kinematics of one link: absolute angle and rates, joint point and its
/// motion, centre of mass and its motion.
struct Pose {
    phi: f64,
    omega: f64,
    alpha: f64,
    joint: [f64; 2],
    joint_velocity: [f64; 2],
    com_offset: [f64; 2],
    com_acceleration: [f64; 2],
    com_velocity: [f64; 2],
    next: [f64; 2],
}

impl Chain {
    /// Forward pass from the base bundle `(x, y, φ, vx, vy, ω)` with base
    /// acceleration `(ax, ay, α)`, joint angles, rates and accelerations.
    fn forward(&self, base: &[f64], base_acceleration: [f64; 3], theta: &[f64], theta_dot: &[f64], theta_ddot: &[f64]) -> Vec<Pose> {
        let r = rot(base[2], self.anchor);
        let (mut phi, mut omega, mut alpha) = (base[2], base[5], base_acceleration[2]);
        let mut joint = add([base[0], base[1]], r);
        let mut joint_velocity = add([base[3], base[4]], spin(base[5], r));
        let mut joint_acceleration = add(add([base_acceleration[0], base_acceleration[1]], spin(base_acceleration[2], r)), scale(-base[5] * base[5], r));
        let mut poses = Vec::with_capacity(self.links.len());
        for (k, link) in self.links.iter().enumerate() {
            phi += theta[k];
            omega += theta_dot[k];
            alpha += theta_ddot[k];
            let com_offset = rot(phi, [link.com, 0.0]);
            let com_velocity = add(joint_velocity, spin(omega, com_offset));
            let com_acceleration = add(add(joint_acceleration, spin(alpha, com_offset)), scale(-omega * omega, com_offset));
            let along = rot(phi, [link.length, 0.0]);
            let next = add(joint, along);
            poses.push(Pose { phi, omega, alpha, joint, joint_velocity, com_offset, com_acceleration, com_velocity, next });
            joint_velocity = add(joint_velocity, spin(omega, along));
            joint_acceleration = add(add(joint_acceleration, spin(alpha, along)), scale(-omega * omega, along));
            joint = next;
        }
        poses
    }

    /// Backward pass: the force and torque each joint transmits to its
    /// link, given the load `(fx, fy, m)` the tip carries. Returns
    /// `(f_k, n_k)` for k = 0..N.
    fn backward(&self, poses: &[Pose], tip_load: [f64; 3]) -> Vec<([f64; 2], f64)> {
        let g = [0.0, -self.gravity];
        let mut f_next = scale(-1.0, [tip_load[0], tip_load[1]]);
        let mut n_next = -tip_load[2];
        let mut out = vec![([0.0; 2], 0.0); self.links.len()];
        for (k, (link, pose)) in self.links.iter().zip(poses).enumerate().rev() {
            let inertial = scale(link.mass, add(pose.com_acceleration, scale(-1.0, g)));
            let along = add(pose.next, scale(-1.0, pose.joint));
            let f = add(inertial, f_next);
            let n = link.inertia * pose.alpha + cross(pose.com_offset, inertial) + n_next + cross(along, f_next);
            out[k] = (f, n);
            f_next = f;
            n_next = n;
        }
        out
    }

    fn joint_angles(&self, across: impl Fn(usize) -> f64) -> Vec<f64> {
        self.port_of_joint.iter().map(|p| across(*p)).collect()
    }
}

impl Behavior for Chain {
    fn owned_frame(&self) -> Option<usize> { Some(1) }
    fn states(&self) -> Vec<StateDeclaration> {
        use QuantityKind::*;
        // The first six alias the owned tip frame; then the tip load the
        // attachments apply; then the joint rates.
        let (x, y, phi) = {
            // A starting guess for the tip from the initial angles with the
            // base at the origin; the consistent initialisation finishes it.
            let base = [0.0; 6];
            let zeros = vec![0.0; self.links.len()];
            let poses = self.forward(&base, [0.0; 3], &self.initial_angle, &zeros, &zeros);
            poses.last().map(|p| (p.next[0], p.next[1], p.phi)).unwrap_or((0.0, 0.0, 0.0))
        };
        let mut out = vec![
            StateDeclaration::new("tip.x", Length, x),
            StateDeclaration::new("tip.y", Length, y),
            StateDeclaration::new("tip.theta", Angle, phi),
            StateDeclaration::new("tip.vx", LinearVelocity, 0.0),
            StateDeclaration::new("tip.vy", LinearVelocity, 0.0),
            StateDeclaration::new("tip.omega", AngularVelocity, 0.0),
            StateDeclaration::new("tip.fx", Force, 0.0),
            StateDeclaration::new("tip.fy", Force, 0.0),
            StateDeclaration::new("tip.torque", Torque, 0.0),
        ];
        out.extend(self.joints.iter().zip(&self.initial_speed).map(|(n, v)| StateDeclaration::new(format!("{n}.speed"), AngularVelocity, *v)));
        out
    }
    fn provides(&self) -> Vec<Provision> {
        self.port_of_joint.iter().enumerate().map(|(k, port)| Provision { port: *port, lane: 1, state: RATES + k }).collect()
    }
    fn residual(&self, ctx: &mut Context) {
        let n = self.links.len();
        let base = ctx.across_bundle(0).to_vec();
        let base_acceleration = [ctx.across_derivative(0, 3), ctx.across_derivative(0, 4), ctx.across_derivative(0, 5)];
        let theta = self.joint_angles(|p| ctx.across(p));
        let theta_dot: Vec<f64> = (0..n).map(|k| ctx.state(RATES + k)).collect();
        let theta_ddot: Vec<f64> = (0..n).map(|k| ctx.state_rate(RATES + k)).collect();
        let poses = self.forward(&base, base_acceleration, &theta, &theta_dot, &theta_ddot);
        let load = [ctx.state(LOAD.start), ctx.state(LOAD.start + 1), ctx.state(LOAD.start + 2)];
        let wrenches = self.backward(&poses, load);
        let last = poses.last().expect("a chain has at least one link");
        // Tip pose and twist follow the joints.
        let tip_velocity = add(last.joint_velocity, spin(last.omega, add(last.next, scale(-1.0, last.joint))));
        ctx.set_state_residual(0, ctx.state(0) - last.next[0]);
        ctx.set_state_residual(1, ctx.state(1) - last.next[1]);
        ctx.set_state_residual(2, ctx.state(2) - last.phi);
        // Rows 3..6 receive the attachments' through; the load states make
        // them read `load + Σ through = 0`, so the load is the force on the tip.
        for k in 0..3 {
            ctx.set_state_residual(3 + k, ctx.state(LOAD.start + k));
        }
        ctx.set_state_residual(6, ctx.state(3) - tip_velocity[0]);
        ctx.set_state_residual(7, ctx.state(4) - tip_velocity[1]);
        ctx.set_state_residual(8, ctx.state(5) - last.omega);
        // Each joint rate is the rate of its node's angle; the joint absorbs
        // the torque the recursion demands.
        for (k, port) in self.port_of_joint.iter().enumerate() {
            ctx.set_state_residual(RATES + k, ctx.state(RATES + k) - ctx.across_derivative(*port, 0));
            ctx.add_through(*port, wrenches[k].1);
        }
        // The base supplies the first joint's force and torque; the chain's
        // through on the base port is what the base gives up.
        let (f, torque) = wrenches[0];
        let r = rot(base[2], self.anchor);
        ctx.add_through_lane(0, 0, f[0]);
        ctx.add_through_lane(0, 1, f[1]);
        ctx.add_through_lane(0, 2, torque + cross(r, f));
    }
    fn energy(&self, view: &View) -> f64 {
        let n = self.links.len();
        let base = view.across_bundle(0).to_vec();
        let theta = self.joint_angles(|p| view.across(p));
        let theta_dot: Vec<f64> = (0..n).map(|k| view.state(RATES + k)).collect();
        let zeros = vec![0.0; n];
        let poses = self.forward(&base, [0.0; 3], &theta, &theta_dot, &zeros);
        self.links
            .iter()
            .zip(&poses)
            .map(|(link, p)| {
                let v = p.com_velocity;
                let height = p.joint[1] + p.com_offset[1];
                0.5 * link.mass * (v[0] * v[0] + v[1] * v[1]) + 0.5 * link.inertia * p.omega * p.omega + link.mass * self.gravity * height
            })
            .sum()
    }
}

fn chain(p: &Params) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    // Joints: `joint.<name> = index`, ports sorted by name after base and tip.
    let mut named: Vec<(String, usize)> = p.iter().filter_map(|(k, v)| k.strip_prefix("joint.").map(|n| (n.to_owned(), *v as usize))).collect();
    named.sort_by(|a, b| a.0.cmp(&b.0));
    let mut joints: Vec<(usize, String, usize)> = named.iter().enumerate().map(|(rank, (name, index))| (*index, name.clone(), 2 + rank)).collect();
    joints.sort_by_key(|j| j.0);
    if joints.iter().enumerate().any(|(k, j)| j.0 != k) {
        return Err(sim_core::EquationError::InvalidParameter("joint.*".into(), "joint indices must be 0, 1, 2, … in chain order".into()));
    }
    for name in p.keys().filter(|name| name.starts_with("link")) {
        let index = name.strip_prefix("link").and_then(|rest| rest.split_once('.')).map(|(index, _)| index).unwrap_or("");
        if !index.parse::<usize>().ok().is_some_and(|k| k < joints.len() && index == k.to_string()) {
            return Err(sim_core::EquationError::InvalidParameter(name.clone(), "link index must be canonical and identify an existing joint".into()));
        }
    }
    let mut links = Vec::new();
    for k in 0..joints.len() {
        let mass = param(p, &format!("link{k}.mass"))?;
        let length = param(p, &format!("link{k}.length"))?;
        links.push(Link { mass, length, com: param_or(p, &format!("link{k}.com"), 0.5 * length), inertia: param_or(p, &format!("link{k}.inertia"), mass * length * length / 12.0) });
    }
    if links.is_empty() {
        return Err(sim_core::EquationError::MissingParameter("joint.*".into()));
    }
    let initial_angle = joints.iter().map(|(_, n, _)| param_or(p, &format!("initial.joint.{n}.angle"), 0.0)).collect();
    let initial_speed = joints.iter().map(|(_, n, _)| param_or(p, &format!("initial.joint.{n}.speed"), 0.0)).collect();
    Ok(Box::new(Chain {
        links,
        gravity: param_or(p, "gravity", 9.81),
        anchor: [param_or(p, "ax", 0.0), param_or(p, "ay", 0.0)],
        joints: joints.iter().map(|j| j.1.clone()).collect(),
        port_of_joint: joints.iter().map(|j| j.2).collect(),
        initial_speed,
        initial_angle,
    }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    let mut com = P::alternative("link*.com", "m");
    com.default_label = Some("link length / 2".into());
    let mut inertia = P::alternative("link*.inertia", "kg·m²").nonnegative();
    inertia.default_label = Some("link mass * length² / 12".into());
    registry.register(BehaviorDescriptor::new(
        CHAIN,
        "Planar serial chain in minimal coordinates",
        vec![acausal("base", ConnectorKind::PlanarFrame), acausal("tip", ConnectorKind::PlanarFrame), acausal("joint.*", ConnectorKind::Rotational)],
        chain,
    ).with_parameters(vec![
        P::required("joint.*", "index").integer(0., 1023.),
        P::required("link*.mass", "kg").positive(), P::required("link*.length", "m").positive(), com, inertia,
        P::optional("gravity", "m/s²", 9.81), P::optional("ax", "m", 0.), P::optional("ay", "m", 0.),
        P::optional("initial.joint.*.angle", "rad", 0.), P::optional("initial.joint.*.speed", "rad/s", 0.),
    ]))
}
