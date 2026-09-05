//! Planar rigid bodies and unilateral contact with Coulomb friction, at the
//! velocity level and without penalties.
//!
//! A contact is an element attached to a body's `PlanarFrame`. Its normal
//! and tangential forces are algebraic states related to the contact point's
//! velocity by complementarity: the Fischer–Burmeister function
//! `φ(a, b) = a + b − √(a² + b²)` vanishes exactly when `a ≥ 0`, `b ≥ 0`
//! and `a·b = 0`, and a projection equation keeps the friction inside the
//! Coulomb cone. Touchdown is a guard on the gap; separation follows from
//! the complementarity itself. Because the constraint is imposed on
//! velocities, an impact is inelastic and appears as a one-step impulse —
//! Moreau's time-stepping — which is also why Painlevé's "no consistent
//! sliding solution" resolves into an impact without collision instead of
//! a failure.

use sim_core::{
    Behavior, BehaviorDescriptor, Branch, Input, LocalJacobian, Output, BehaviorRegistry, ConnectorKind, Context, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or,
};
use std::collections::BTreeMap;

pub const PLANAR_RIGID_BODY: &str = "planar.rigid_body";
pub const POINT_PLANE: &str = "contact.point_plane";
pub const POINT_PLANE_COMPLIANT: &str = "contact.point_plane_compliant";
pub const POINT_TERRAIN_COMPLIANT: &str = "contact.point_terrain_compliant";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// Planar rigid body owning a `PlanarFrame`: states x, y, θ, vx, vy, ω.
pub struct PlanarRigidBody {
    pub mass: f64,
    pub inertia: f64,
    pub gravity: f64,
    /// Tilt of the world (rad): gravity gains a component `−g·sin(slope)`
    /// along +x — a hill, with the plane `y = 0` as the road.
    pub slope: f64,
    pub initial: [f64; 6],
}
impl Behavior for PlanarRigidBody {
    fn owned_frame(&self) -> Option<usize> { Some(0) }
    fn states(&self) -> Vec<StateDeclaration> {
        let names = ["x", "y", "theta", "vx", "vy", "omega"];
        let kinds = [QuantityKind::Length, QuantityKind::Length, QuantityKind::Angle, QuantityKind::LinearVelocity, QuantityKind::LinearVelocity, QuantityKind::AngularVelocity];
        names.iter().zip(kinds).zip(self.initial).map(|((n, k), v)| StateDeclaration::new(*n, k, v)).collect()
    }
    fn residual(&self, ctx: &mut Context) {
        for k in 0..3 {
            ctx.set_state_residual(k, ctx.state_rate(k) - ctx.state(3 + k));
        }
        ctx.set_state_residual(3, self.mass * ctx.state_rate(3) + self.mass * self.gravity * self.slope.sin());
        ctx.set_state_residual(4, self.mass * ctx.state_rate(4) + self.mass * self.gravity * self.slope.cos());
        ctx.set_state_residual(5, self.inertia * ctx.state_rate(5));
    }
    fn energy(&self, view: &View) -> f64 {
        let (vx, vy, w) = (view.state(3), view.state(4), view.state(5));
        0.5 * self.mass * (vx * vx + vy * vy) + 0.5 * self.inertia * w * w + self.mass * self.gravity * (view.state(1) * self.slope.cos() + view.state(0) * self.slope.sin())
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        for k in 0..3 {
            out.state_rate(k, k, 1.0);
            out.state_state(k, 3 + k, -1.0);
        }
        out.state_rate(3, 3, self.mass);
        out.state_rate(4, 4, self.mass);
        out.state_rate(5, 5, self.inertia);
        true
    }
}
fn planar_rigid_body(p: &Params) -> Made {
    let names = ["x", "y", "theta", "vx", "vy", "omega"];
    let mut initial = [0.0; 6];
    for (k, n) in names.iter().enumerate() {
        initial[k] = param_or(p, &format!("initial.{n}"), 0.0);
    }
    Ok(Box::new(PlanarRigidBody { mass: param(p, "mass")?, inertia: param(p, "inertia")?, gravity: param_or(p, "gravity", 9.81), slope: param_or(p, "slope", 0.0), initial }))
}

/// Kinematics of a body-fixed point against the plane y = 0.
fn point(bundle: &[f64], px: f64, py: f64) -> (f64, f64, f64, f64, f64) {
    let (x, y, theta, vx, vy, w) = (bundle[0], bundle[1], bundle[2], bundle[3], bundle[4], bundle[5]);
    let (c, s) = (theta.cos(), theta.sin());
    let (ox, oy) = (c * px - s * py, s * px + c * py);
    let gap = y + oy;
    let vt = vx - w * oy;
    let vn = vy + w * ox;
    let _ = x;
    (gap, vt, vn, ox, oy)
}

/// Smoothed so its derivative is defined at the origin (a contact at rest);
/// the relaxation `a·b ≈ ε²/2` is far below any force or speed here.
fn fischer_burmeister(a: f64, b: f64) -> f64 {
    const EPSILON: f64 = 1.0e-4;
    a + b - (a * a + b * b + EPSILON * EPSILON).sqrt()
}

/// Unilateral point contact with Coulomb friction, velocity-level.
pub struct PointPlane {
    pub px: f64,
    pub py: f64,
    pub friction: f64,
    /// Gap drift correction rate in the closed constraint `v_n + α·gap ≥ 0`.
    pub stabilisation: f64,
    /// Scale relating the friction projection's argument to velocity.
    pub friction_scale: f64,
}
impl Behavior for PointPlane {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![
            StateDeclaration::new("normal_force", QuantityKind::Force, 0.0),
            StateDeclaration::new("tangential_force", QuantityKind::Force, 0.0),
            StateDeclaration::new("touching", QuantityKind::Dimensionless, 0.0),
        ]
    }
    fn residual(&self, ctx: &mut Context) {
        let (gap, vt, vn, ox, oy) = point(ctx.across_bundle(0), self.px, self.py);
        let (n, t) = (ctx.state(0), ctx.state(1));
        ctx.set_state_residual(2, ctx.state_rate(2));
        if ctx.state(2) > 0.5 {
            // Closed: 0 ≤ N ⊥ (v_n + α·gap) ≥ 0, friction in the cone.
            ctx.set_state_residual(0, fischer_burmeister(n, vn + self.stabilisation * gap));
            let bound = self.friction * n.max(0.0);
            ctx.set_state_residual(1, t - (t - self.friction_scale * vt).clamp(-bound, bound));
        } else {
            ctx.set_state_residual(0, n);
            ctx.set_state_residual(1, t);
        }
        // Force on the body (t, n) at the point; torque about the body origin.
        ctx.add_through_lane(0, 0, -t);
        ctx.add_through_lane(0, 1, -n);
        ctx.add_through_lane(0, 2, -(ox * n - oy * t));
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        let (gap, _, vn, _, _) = point(view.across_bundle(0), self.px, self.py);
        if view.state(2) > 0.5 {
            // Separate once the contact carries no force and moves away.
            out.push(if view.state(0) <= 1.0e-9 && vn > 0.0 { -1.0 } else { 1.0 });
        } else {
            out.push(gap);
        }
    }
    fn jump(&mut self, _index: usize, _view: &View, states: &mut [f64]) {
        states[2] = if states[2] > 0.5 { 0.0 } else { 1.0 };
        states[0] = 0.0;
        states[1] = 0.0;
    }
    /// When the sliding branch has no solution (Painlevé's paradox) the
    /// step's solution is an impact without collision: an impulsive
    /// normal force that stops the tangential slip within the step.
    /// Propose the stick branch — a large normal force with sub-critical
    /// friction opposing the slip — and let Newton finish it.
    fn branches(&self, view: &View, out: &mut Vec<Branch>) {
        if view.state(2) < 0.5 {
            return;
        }
        let bundle = view.across_bundle(0);
        let (_, vt, _, _, _) = point(bundle, self.px, self.py);
        if vt.abs() < 1.0e-9 {
            return;
        }
        let normal = 1.0e2 * (view.state(0).abs() + 1.0);
        out.push(Branch {
            states: vec![normal, -0.5 * self.friction * normal * vt.signum(), 1.0],
            // Stick: the frame's translation absorbs the slip.
            across: vec![(0, 3, bundle[3] - vt)],
        });
    }
}
fn point_plane(p: &Params) -> Made {
    Ok(Box::new(PointPlane {
        px: param_or(p, "px", 0.0),
        py: param_or(p, "py", 0.0),
        friction: param_or(p, "friction", 0.0),
        stabilisation: param_or(p, "stabilisation", 20.0),
        friction_scale: param_or(p, "friction_scale", 1.0e2),
    }))
}

/// The falsifier: the same point on a penalty spring with regularised
/// friction. It stiffens; it never jams.
pub struct PointPlaneCompliant {
    pub px: f64,
    pub py: f64,
    pub stiffness: f64,
    pub damping: f64,
    pub friction: f64,
    pub regularisation: f64,
}
impl Behavior for PointPlaneCompliant {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let (gap, vt, vn, ox, oy) = point(ctx.across_bundle(0), self.px, self.py);
        if gap >= 0.0 {
            return;
        }
        let n = (-self.stiffness * gap - self.damping * vn).max(0.0);
        let t = -self.friction * n * (vt / self.regularisation).tanh();
        ctx.add_through_lane(0, 0, -t);
        ctx.add_through_lane(0, 1, -n);
        ctx.add_through_lane(0, 2, -(ox * n - oy * t));
    }
    fn energy(&self, view: &View) -> f64 {
        let (gap, _, _, _, _) = point(view.across_bundle(0), self.px, self.py);
        if gap < 0.0 { 0.5 * self.stiffness * gap * gap } else { 0.0 }
    }
    fn jacobian(&self, view: &View, out: &mut LocalJacobian) -> bool {
        let b = view.across_bundle(0);
        let (gap, vt, vn, ox, oy) = point(b, self.px, self.py);
        if gap >= 0.0 {
            return true;
        }
        let raw = -self.stiffness * gap - self.damping * vn;
        if raw <= 0.0 {
            return true;
        }
        // n = k·(−gap) − c·vn with gap = y + oy(θ), vn = vy + ω·ox(θ);
        // t = −μ·n·tanh(vt/ε) with vt = vx − ω·oy(θ).
        let (c_, s_) = (b[2].cos(), b[2].sin());
        let (dox, doy) = (-s_ * self.px - c_ * self.py, c_ * self.px - s_ * self.py); // d/dθ of (ox, oy)
        let w = b[5];
        let dn = [
            (1, -self.stiffness),                       // ∂n/∂y
            (2, -self.stiffness * doy - self.damping * w * dox), // ∂n/∂θ
            (4, -self.damping),                         // ∂n/∂vy
            (5, -self.damping * ox),                    // ∂n/∂ω
        ];
        let th = (vt / self.regularisation).tanh();
        let sech2 = 1.0 - th * th;
        let dvt = [(3, 1.0), (5, -oy), (2, -w * doy)];
        // ∂t/∂lane = −μ·(∂n·tanh + n·sech²/ε·∂vt)
        for (lane, d) in dn {
            let dt = -self.friction * d * th;
            out.set(Output::Through(0, 0), Input::Across(0, lane), -dt);
            out.set(Output::Through(0, 1), Input::Across(0, lane), -d);
            out.set(Output::Through(0, 2), Input::Across(0, lane), -(ox * d - oy * dt));
        }
        for (lane, d) in dvt {
            let dt = -self.friction * raw * sech2 / self.regularisation * d;
            out.set(Output::Through(0, 0), Input::Across(0, lane), -dt);
            out.set(Output::Through(0, 2), Input::Across(0, lane), oy * dt);
        }
        // The lever arm's own dependence on θ.
        let t = -self.friction * raw * th;
        out.set(Output::Through(0, 2), Input::Across(0, 2), -(dox * raw - doy * t));
        true
    }
}
fn point_plane_compliant(p: &Params) -> Made {
    Ok(Box::new(PointPlaneCompliant {
        px: param_or(p, "px", 0.0),
        py: param_or(p, "py", 0.0),
        stiffness: param(p, "stiffness")?,
        damping: param_or(p, "damping", 0.0),
        friction: param_or(p, "friction", 0.0),
        regularisation: param_or(p, "regularisation", 1.0e-3),
    }))
}

/// A point on a body's frame against a terrain of horizontal patches —
/// stepping stones, stairs, a plank — each `patch<i>.{x0, x1, y}`; between
/// patches there is nothing to stand on. The same penalty law as
/// [`PointPlaneCompliant`] on the patch under the point, faded in over
/// `edge` metres at a patch's ends so a foot on the edge does not switch
/// its force on and off between Newton iterations.
pub struct PointTerrainCompliant {
    pub px: f64,
    pub py: f64,
    pub stiffness: f64,
    pub damping: f64,
    pub friction: f64,
    pub regularisation: f64,
    pub edge: f64,
    /// `(x0, x1, y)` per patch.
    pub patches: Vec<(f64, f64, f64)>,
}
impl PointTerrainCompliant {
    /// `(weight, height)` of the patch under `x`: the fade-in weight and
    /// its top; `None` off every patch.
    pub fn under(&self, x: f64) -> Option<(f64, f64)> {
        let mut best: Option<(f64, f64)> = None;
        for (x0, x1, y) in &self.patches {
            if x < *x0 || x > *x1 {
                continue;
            }
            let w = ((x - x0) / self.edge).min((x1 - x) / self.edge).min(1.0).max(0.0);
            if best.is_none_or(|(bw, _)| w > bw) {
                best = Some((w, *y));
            }
        }
        best
    }
    fn forces(&self, bundle: &[f64]) -> Option<(f64, f64, f64, f64)> {
        let (gap0, vt, vn, ox, oy) = point(bundle, self.px, self.py);
        let x = bundle[0] + ox;
        let (w, y) = self.under(x)?;
        let gap = gap0 - y;
        if gap >= 0.0 || w <= 0.0 {
            return None;
        }
        let n = w * (-self.stiffness * gap - self.damping * vn).max(0.0);
        let t = -self.friction * n * (vt / self.regularisation).tanh();
        Some((n, t, ox, oy))
    }
}
impl Behavior for PointTerrainCompliant {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        if let Some((n, t, ox, oy)) = self.forces(ctx.across_bundle(0)) {
            ctx.add_through_lane(0, 0, -t);
            ctx.add_through_lane(0, 1, -n);
            ctx.add_through_lane(0, 2, -(ox * n - oy * t));
        }
    }
    fn energy(&self, view: &View) -> f64 {
        let b = view.across_bundle(0);
        let (gap0, _, _, ox, _) = point(b, self.px, self.py);
        match self.under(b[0] + ox) {
            Some((w, y)) if gap0 - y < 0.0 => 0.5 * w * self.stiffness * (gap0 - y) * (gap0 - y),
            _ => 0.0,
        }
    }
}
fn point_terrain_compliant(p: &Params) -> Made {
    let count = param_or(p, "patches", 0.0).max(0.0) as usize;
    for name in p.keys().filter(|name| name.starts_with("patch") && name.as_str() != "patches") {
        let index = name.strip_prefix("patch").and_then(|s| s.split_once('.')).and_then(|(s, _)| s.parse::<usize>().ok());
        if index.is_none_or(|k| k >= count || !name.starts_with(&format!("patch{k}."))) {
            return Err(sim_core::EquationError::InvalidParameter(name.clone(), "patch index must be an integer in 0..patches".into()));
        }
    }
    let patches = (0..count)
        .map(|k| Ok((param(p, &format!("patch{k}.x0"))?, param(p, &format!("patch{k}.x1"))?, param_or(p, &format!("patch{k}.y"), 0.0))))
        .collect::<Result<Vec<_>, sim_core::EquationError>>()?;
    for (index, (x0, x1, _)) in patches.iter().enumerate() {
        if x0 >= x1 {
            return Err(sim_core::EquationError::InvalidParameter(format!("patch{index}.x1"), "patch end must exceed its start (x0)".into()));
        }
    }
    Ok(Box::new(PointTerrainCompliant {
        px: param_or(p, "px", 0.0),
        py: param_or(p, "py", 0.0),
        stiffness: param(p, "stiffness")?,
        damping: param_or(p, "damping", 0.0),
        friction: param_or(p, "friction", 0.0),
        regularisation: param_or(p, "regularisation", 1.0e-3),
        edge: param_or(p, "edge", 0.01),
        patches,
    }))
}

/// A wheel on a body's planar frame: hub at `(px, py)`, radius `r`, its own
/// spin as a state, an `axle` rotational port a motor drives (across the
/// wheel angle, the spin providing the speed lane), and a compliant
/// contact with the plane `y = 0` at the rim's lowest point. Traction is
/// regularised Coulomb friction on the slip `v_hub + ω·r`, applied to the
/// body at the contact point and to the wheel's spin. A car is four of
/// these on a body.
pub struct Wheel {
    pub px: f64,
    pub py: f64,
    pub radius: f64,
    pub inertia: f64,
    pub stiffness: f64,
    pub damping: f64,
    pub friction: f64,
    pub regularisation: f64,
    pub initial_spin: f64,
}
impl Wheel {
    /// `(gap, slip, normal force, tangential force, hub offset)` at the contact.
    fn contact(&self, bundle: &[f64], spin: f64) -> (f64, f64, f64, f64, [f64; 2]) {
        let (_, vt, vn, ox, oy) = point(bundle, self.px, self.py);
        let hub_height = bundle[1] + oy;
        let gap = hub_height - self.radius;
        let slip = vt + spin * self.radius;
        let (n, t) = if gap < 0.0 {
            let n = (-self.stiffness * gap - self.damping * vn).max(0.0);
            (n, -self.friction * n * (slip / self.regularisation).tanh())
        } else {
            (0.0, 0.0)
        };
        (gap, slip, n, t, [ox, oy - self.radius])
    }
}
impl Behavior for Wheel {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("spin", QuantityKind::AngularVelocity, self.initial_spin)]
    }
    fn provides(&self) -> Vec<sim_core::Provision> {
        vec![sim_core::Provision { port: 1, lane: 1, state: 0 }]
    }
    fn residual(&self, ctx: &mut Context) {
        let spin = ctx.state(0);
        let (_, _, n, t, offset) = self.contact(ctx.across_bundle(0), spin);
        // The contact force on the body at the rim's lowest point.
        ctx.add_through_lane(0, 0, -t);
        ctx.add_through_lane(0, 1, -n);
        ctx.add_through_lane(0, 2, -(offset[0] * n - offset[1] * t));
        // The axle: the wheel's angle is the node, the spin its rate; the
        // spin absorbs the axle torque less the traction's moment.
        ctx.set_state_residual(0, spin - ctx.across_derivative(1, 0));
        ctx.add_through(1, self.inertia * ctx.state_rate(0) - self.radius * t);
    }
    fn energy(&self, view: &View) -> f64 {
        let spin = view.state(0);
        let (gap, _, _, _, _) = self.contact(view.across_bundle(0), spin);
        0.5 * self.inertia * spin * spin + if gap < 0.0 { 0.5 * self.stiffness * gap * gap } else { 0.0 }
    }
}
fn wheel(p: &Params) -> Made {
    Ok(Box::new(Wheel {
        px: param_or(p, "px", 0.0),
        py: param_or(p, "py", 0.0),
        radius: param(p, "radius")?,
        inertia: param(p, "inertia")?,
        stiffness: param_or(p, "stiffness", 1.0e5),
        damping: param_or(p, "damping", 500.0),
        friction: param_or(p, "friction", 1.0),
        regularisation: param_or(p, "regularisation", 1.0e-2),
        initial_spin: param_or(p, "initial.spin", 0.0),
    }))
}

pub const WHEEL: &str = "contact.wheel";
pub const DRAG: &str = "planar.drag";

/// Quadratic drag on a planar frame: `F = −½·ρ·C_d·A·|v|·v` at the frame's
/// origin (plus linear `damping` on the spin). What a car's air is.
pub struct Drag {
    pub coefficient: f64,
    pub spin_damping: f64,
}
impl Behavior for Drag {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let b = ctx.across_bundle(0);
        let (vx, vy, w) = (b[3], b[4], b[5]);
        let speed = (vx * vx + vy * vy).sqrt();
        ctx.add_through_lane(0, 0, self.coefficient * speed * vx);
        ctx.add_through_lane(0, 1, self.coefficient * speed * vy);
        ctx.add_through_lane(0, 2, self.spin_damping * w);
    }
    fn jacobian(&self, view: &View, out: &mut LocalJacobian) -> bool {
        let b = view.across_bundle(0);
        let (vx, vy) = (b[3], b[4]);
        let speed = (vx * vx + vy * vy).sqrt().max(1.0e-12);
        let c = self.coefficient;
        out.set(Output::Through(0, 0), Input::Across(0, 3), c * (speed + vx * vx / speed));
        out.set(Output::Through(0, 0), Input::Across(0, 4), c * vx * vy / speed);
        out.set(Output::Through(0, 1), Input::Across(0, 3), c * vx * vy / speed);
        out.set(Output::Through(0, 1), Input::Across(0, 4), c * (speed + vy * vy / speed));
        out.set(Output::Through(0, 2), Input::Across(0, 5), self.spin_damping);
        true
    }
}
fn drag(p: &Params) -> Made {
    if p.contains_key("coefficient") && ["density", "cd", "area"].iter().any(|name| p.contains_key(*name)) {
        return Err(sim_core::EquationError::InvalidParameter("coefficient".into(), "supply coefficient or density/cd/area, not both".into()));
    }
    // `coefficient` = ½·ρ·C_d·A (kg/m); or give `density`, `cd` and `area`.
    let coefficient = match p.get("coefficient") {
        Some(c) => *c,
        None => 0.5 * param_or(p, "density", 1.225) * param(p, "cd")? * param(p, "area")?,
    };
    Ok(Box::new(Drag { coefficient, spin_damping: param_or(p, "spin_damping", 0.0) }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use ConnectorKind::PlanarFrame as F;
    use sim_core::ParameterDeclaration as P;
    let point_parameters = || vec![P::optional("px", "m", 0.), P::optional("py", "m", 0.)];
    let mut unilateral = point_parameters();
    unilateral.extend([P::optional("friction", "1", 0.).nonnegative(),
        P::optional("stabilisation", "1/s", 20.).nonnegative(), P::optional("friction_scale", "N·s/m", 1e2).positive()]);
    let mut penalty = point_parameters();
    penalty.extend([P::required("stiffness", "N/m").positive(), P::optional("damping", "N·s/m", 0.).nonnegative(),
        P::optional("friction", "1", 0.).nonnegative(), P::optional("regularisation", "m/s", 1e-3).positive()]);
    let mut terrain = penalty.clone();
    let mut edge = P::optional("edge", "m", 0.01); edge.minimum = Some(1e-6);
    terrain.extend([P::optional("patches", "1", 0.).integer(0., 4096.), edge,
        P::alternative("patch*.x0", "m"), P::alternative("patch*.x1", "m"), P::optional("patch*.y", "m", 0.)]);
    let mut wheel_parameters = point_parameters();
    wheel_parameters.extend([P::required("radius", "m").positive(), P::required("inertia", "kg·m²").positive(),
        P::optional("stiffness", "N/m", 1e5).positive(), P::optional("damping", "N·s/m", 500.).nonnegative(),
        P::optional("friction", "1", 1.).nonnegative(), P::optional("regularisation", "m/s", 1e-2).positive(),
        P::optional("initial.spin", "rad/s", 0.)]);
    let mut rigid_body = vec![P::required("mass", "kg").positive(), P::required("inertia", "kg·m²").positive(),
        P::optional("gravity", "m/s²", 9.81), P::optional("slope", "rad", 0.)];
    for (name, unit) in [("x", "m"), ("y", "m"), ("theta", "rad"), ("vx", "m/s"), ("vy", "m/s"), ("omega", "rad/s")] {
        rigid_body.push(P::optional(format!("initial.{name}"), unit, 0.));
    }
    for descriptor in [
        BehaviorDescriptor::new(WHEEL, "Wheel with rolling contact on the plane", vec![acausal("frame", F), acausal("axle", ConnectorKind::Rotational)], wheel).with_parameters(wheel_parameters),
        BehaviorDescriptor::new(DRAG, "Quadratic drag on a frame", vec![acausal("frame", F)], drag).with_parameters(vec![
            P::alternative("coefficient", "kg/m").nonnegative(), P::optional("density", "kg/m³", 1.225).nonnegative(),
            P::alternative("cd", "1").nonnegative(), P::alternative("area", "m²").nonnegative(), P::optional("spin_damping", "N·m·s/rad", 0.).nonnegative()]),
        BehaviorDescriptor::new(PLANAR_RIGID_BODY, "Planar rigid body", vec![acausal("frame", F)], planar_rigid_body).with_parameters(rigid_body),
        BehaviorDescriptor::new(POINT_PLANE, "Unilateral point contact with Coulomb friction", vec![acausal("frame", F)], point_plane).with_parameters(unilateral),
        BehaviorDescriptor::new(POINT_PLANE_COMPLIANT, "Penalty point contact", vec![acausal("frame", F)], point_plane_compliant).with_parameters(penalty),
        BehaviorDescriptor::new(POINT_TERRAIN_COMPLIANT, "Penalty point contact on horizontal patches (stones, stairs)", vec![acausal("frame", F)], point_terrain_compliant).with_parameters(terrain),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}
