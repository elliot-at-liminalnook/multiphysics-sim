//! One-dimensional rotational elements as compiled behaviors.
//!
//! Sign convention (shared by every domain): a through variable is positive
//! *into* the behavior, and every node sums its ports' throughs to zero.

use sim_core::{
    Behavior, Input, LocalJacobian, Output, BehaviorDescriptor, BehaviorRegistry, Context, Provision, QuantityKind, RegistryError,
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
pub const WORM_GEAR: &str = "rotational.worm_gear";
pub const LOAD_TORQUE: &str = "rotational.load_torque";
pub const LOSSY_GEAR: &str = "rotational.lossy_gear";
pub const COULOMB_FRICTION: &str = "rotational.coulomb_friction";

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
        // The value the residual holds the node at, as for every other pin.
        vec![(0, 0, self.angle)]
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

/// A constant torque acting on the shaft (positive along its angle), such as
/// a hanging weight on a drum.
pub struct LoadTorque {
    pub torque: f64,
}
impl Behavior for LoadTorque {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.add_through(0, -self.torque);
    }
    fn jacobian(&self, _view: &View, _out: &mut LocalJacobian) -> bool {
        true
    }
}
fn load_torque(p: &Params) -> Made {
    Ok(Box::new(LoadTorque { torque: param(p, "torque")? }))
}

/// Gear with a constant mesh efficiency in both directions. The transmitted
/// torque λ is the constraint multiplier; friction adds μ·|λ|·s at each side
/// with s = tanh(ω_in/ε), so the loss 2·μ·|λ|·|ω_in|·|s| is never negative and
/// the efficiency is (1 − μ)/(1 + μ) whichever side drives.
pub struct LossyGear {
    pub ratio: f64,
    pub mu: f64,
    pub smoothing_speed: f64,
    pub correction: f64,
}
impl LossyGear {
    pub fn from_efficiency(ratio: f64, efficiency: f64, smoothing_speed: f64, correction: f64) -> Self {
        Self { ratio, mu: (1. - efficiency) / (1. + efficiency), smoothing_speed, correction }
    }
}
impl Behavior for LossyGear {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("transmitted_torque", QuantityKind::Torque, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let n = self.ratio;
        let drift = ctx.across(0) - n * ctx.across(1);
        let w = ctx.across_rate(0);
        ctx.set_state_residual(0, w - n * ctx.across_rate(1) + self.correction * drift);
        let l = ctx.state(0);
        let f = self.mu * l.abs() * (w / self.smoothing_speed).tanh();
        ctx.add_through(0, l + f);
        ctx.add_through(1, -n * (l - f));
    }
    fn jacobian(&self, view: &View, out: &mut LocalJacobian) -> bool {
        let n = self.ratio;
        out.set(Output::State(0), Input::AcrossRate(0, 0), 1.0);
        out.set(Output::State(0), Input::AcrossRate(1, 0), -n);
        out.set(Output::State(0), Input::Across(0, 0), self.correction);
        out.set(Output::State(0), Input::Across(1, 0), -self.correction * n);
        let l = view.state(0);
        let s = (view.across_rate(0) / self.smoothing_speed).tanh();
        let ds = (1. - s * s) / self.smoothing_speed;
        let sl = if l > 0. { 1. } else if l < 0. { -1. } else { 0. };
        out.through(0, Input::State(0), 1. + self.mu * sl * s);
        out.through(0, Input::AcrossRate(0, 0), self.mu * l.abs() * ds);
        out.through(1, Input::State(0), -n * (1. - self.mu * sl * s));
        out.through(1, Input::AcrossRate(0, 0), n * self.mu * l.abs() * ds);
        true
    }
}
fn lossy_gear(p: &Params) -> Made {
    Ok(Box::new(LossyGear::from_efficiency(param(p, "ratio")?, param_or(p, "efficiency", 0.95), param_or(p, "smoothing_speed", 0.01), param_or(p, "correction", 10.0))))
}

/// Dry friction against a fixed frame: a torque of constant size opposing
/// the shaft's speed, with the sign smoothed below `smoothing_speed`.
pub struct CoulombFriction {
    pub torque: f64,
    pub smoothing_speed: f64,
}
impl Behavior for CoulombFriction {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let w = ctx.across_rate(0);
        ctx.add_through(0, self.torque * (w / self.smoothing_speed).tanh());
    }
    fn jacobian(&self, view: &View, out: &mut LocalJacobian) -> bool {
        let s = (view.across_rate(0) / self.smoothing_speed).tanh();
        out.through(0, Input::AcrossRate(0, 0), self.torque * (1. - s * s) / self.smoothing_speed);
        true
    }
}
fn coulomb_friction(p: &Params) -> Made {
    Ok(Box::new(CoulombFriction { torque: param(p, "torque")?, smoothing_speed: param_or(p, "smoothing_speed", 0.01) }))
}

fn worm_gear(p: &Params) -> Made {
    Ok(Box::new(crate::helical::HelicalDrive {
        contact: crate::helical::worm_contact(p),
        smoothing_speed: param_or(p, "smoothing_speed", 0.01),
        correction: param_or(p, "correction", 10.0),
    }))
}

use sim_core::ComponentNotes as Notes;

/// Companions a shaft-carrying drivetrain part typically attaches to.
const SHAFT_PAIRS: &[&str] = &[INERTIA, WORM_GEAR, IDEAL_GEAR, LOSSY_GEAR, LOAD_TORQUE, "bridge.brushed_motor", DAMPER, SPRING, BACKLASH_MESH, SPEED_SENSOR, ANGLE_SENSOR, GROUND, TORQUE_SOURCE, "bridge.lead_screw"];

static INERTIA_NOTES: Notes = Notes { category: "Mechanical",
    explanation: "A rigid rotating body: a rotor, flywheel, gear blank or load. It stores kinetic energy ½Jω² and resists changes in speed. The optional viscous drag models bearing and windage losses proportional to speed.",
    equations: &["J·dω/dt = Σ torques − b·ω", "dθ/dt = ω"],
    tradeoffs: "More inertia smooths speed ripple and stores energy but slows acceleration; reflected through a gear of ratio N it appears N² times larger at the input, which is why high-ratio gearboxes make motors see tiny load inertia.",
    limits: "Rigid (no shaft twist) and single-axis; no gyroscopic or unbalance effects.",
    parameters: &[("inertia", "Moment of inertia about the shaft axis, J. A solid disk is ½·m·r²."), ("damping", "Viscous drag b, torque per unit speed (bearings, windage)."), ("initial.speed", "Speed at t = 0.")],
    typical: &[("inertia", 1e-5)],
    pairs_with: SHAFT_PAIRS,
    ..Notes::new("A spinning mass: stores energy and resists speed changes.")
};
static SPRING_NOTES: Notes = Notes { category: "Mechanical",
    explanation: "A torsionally elastic shaft or coupling between two rotational nodes. The torque is proportional to the twist between its ends.",
    equations: &["τ = k·(θ_a − θ_b)", "energy = ½·k·(θ_a − θ_b)²"],
    tradeoffs: "Compliance filters shocks and protects gears, but adds a resonance ω = √(k/J) that limits how fast a position loop can be tuned.",
    limits: "Linear and massless; no hysteresis.",
    parameters: &[("stiffness", "Torsional stiffness k. For a solid round shaft k = G·π·d⁴ / (32·L).")],
    typical: &[("stiffness", 30.0)],
    pairs_with: SHAFT_PAIRS,
    ..Notes::new("A twistable shaft or coupling: torque proportional to twist.")
};
static DAMPER_NOTES: Notes = Notes { category: "Mechanical",
    explanation: "Viscous torque between two nodes, proportional to their relative speed: oil films, eddy-current brakes, lumped friction.",
    equations: &["τ = b·(ω_a − ω_b)"],
    tradeoffs: "Damping settles oscillation but converts power to heat continuously while moving.",
    limits: "Linear; no stiction or Coulomb friction.",
    parameters: &[("damping", "Torque per unit relative speed, b.")],
    typical: &[("damping", 1e-4)],
    pairs_with: SHAFT_PAIRS,
    ..Notes::new("Speed-proportional drag between two shafts.")
};
static GROUND_NOTES: Notes = Notes { category: "Mechanical",
    explanation: "An immovable frame. Motor cases, gearbox housings and brakes react their torque into ground.",
    equations: &["θ = θ₀ (held); reaction torque is solved for"],
    pairs_with: &["bridge.brushed_motor", SPRING, DAMPER],
    ..Notes::new("The fixed frame: holds a rotational node still.")
};
static TORQUE_SOURCE_NOTES: Notes = Notes { category: "Mechanical",
    explanation: "Applies whatever torque its signal input commands. Use it for external loads (a hanging weight on a drum is m·g·r) or for idealised actuators.",
    equations: &["τ applied = u(t)"],
    limits: "Ideal: no power limit and no reaction on any case.",
    pairs_with: &[INERTIA, "control.constant", "control.sine"],
    active: true,
    ..Notes::new("An ideal commanded torque, e.g. a load or disturbance.")
};
static LOAD_TORQUE_NOTES: Notes = Notes { category: "Mechanical",
    explanation: "A constant external torque on a shaft. A mass m hanging from a drum of radius r pulls with τ = −m·g·r (negative: it resists lifting when positive rotation winds the rope up).",
    equations: &["τ applied = τ₀"],
    limits: "Constant: it keeps pulling even at a hard stop, and it adds no inertia (add the load's inertia m·r² separately).",
    parameters: &[("torque", "Torque applied along the shaft's positive direction.")],
    typical: &[("torque", -0.1)],
    pairs_with: &[INERTIA, WORM_GEAR, IDEAL_GEAR],
    active: true,
    ..Notes::new("A constant load torque, e.g. a weight on a winch drum.")
};
static ANGLE_SENSOR_NOTES: Notes = Notes { category: "Sensing", explanation: "Reads the shaft angle exactly (an ideal encoder).", pairs_with: SHAFT_PAIRS, ..Notes::new("Ideal angle readout.") };
static SPEED_SENSOR_NOTES: Notes = Notes { category: "Sensing", explanation: "Reads the shaft speed exactly (an ideal tachometer).", pairs_with: SHAFT_PAIRS, ..Notes::new("Ideal speed readout.") };
static BACKLASH_NOTES: Notes = Notes { category: "Transmissions",
    explanation: "A stiff tooth contact with a dead zone: inside the gap no torque passes; once the teeth touch they behave as a stiff spring.",
    equations: &["τ = k·(|Δθ| − g)·sign(Δθ) when |Δθ| > g, else 0"],
    tradeoffs: "Every real gear train has some backlash; it makes position control limit-cycle and reversals clunk. Anti-backlash gears and preloaded drives remove it at the cost of friction.",
    limits: "No impact damping; the contact is conservative.",
    parameters: &[("stiffness", "Mesh stiffness once in contact."), ("gap", "Half the free play, measured at this element's shaft.")],
    typical: &[("stiffness", 50.0), ("gap", 0.005)],
    pairs_with: SHAFT_PAIRS,
    ..Notes::new("A gear mesh with free play.")
};
static IDEAL_GEAR_NOTES: Notes = Notes { category: "Transmissions",
    explanation: "A lossless gear pair (spur, helical, planetary as a lump). Angles are locked in proportion and power passes through unchanged, so torque scales up exactly as speed scales down.",
    equations: &["θ_in = N·θ_out", "τ_out = N·τ_in (efficiency 100 %)"],
    tradeoffs: "Spur and planetary stages run at 95–98 % efficiency per stage and are easily backdriven, which is good for compliant or torque-controlled joints and bad when a load must be held with the power off. Compare the worm gear.",
    limits: "No friction, backlash or tooth compliance; add a backlash mesh or damper to model them.",
    parameters: &[("ratio", "Input turns per output turn, N."), ("correction", "Constraint drift correction rate (numerical).")],
    typical: &[("ratio", 10.0)],
    pairs_with: SHAFT_PAIRS,
    ..Notes::new("A lossless gear pair: trades speed for torque.")
};
static COULOMB_NOTES: Notes = Notes { category: "Mechanical",
    explanation: "Dry (Coulomb) friction between a shaft and its frame: a torque of fixed size that always opposes motion, independent of speed. Gear trains, seals and brushes all add some; it is why a motor needs a minimum drive before it turns at all.",
    equations: &["τ = −T_c · tanh(ω / ε)"],
    tradeoffs: "A constant friction torque sets a dead band: small commands produce no motion, which limits fine positioning. Viscous friction (a damper) grows with speed instead.",
    limits: "No stiction peak (breakaway above running friction) and no Stribeck curve; below ε the sign is smoothed, so a held shaft creeps slowly instead of sticking.",
    parameters: &[("torque", "Friction torque T_c while sliding."), ("smoothing_speed", "Speed ε below which the sign is smoothed.")],
    typical: &[("torque", 0.01)],
    realtime: &[("smoothing_speed", 0.05)],
    pairs_with: &[INERTIA, LOSSY_GEAR, WORM_GEAR, "bridge.brushed_motor"],
    ..Notes::new("Dry friction: a constant torque opposing rotation.")
};

static LOSSY_GEAR_NOTES: Notes = Notes { category: "Transmissions",
    explanation: "A gear stage (spur, helical or planetary, lumped) that loses a fixed fraction of the power it passes, whichever side drives. Rolling tooth contact keeps mesh losses small (1–3 % per spur stage), so it back-drives freely.",
    equations: &["θ_in = N·θ_out", "τ_in = λ + μ|λ|s,  τ_out = N(λ − μ|λ|s),  s = tanh(ω_in/ε)", "η = (1 − μ)/(1 + μ) in both directions"],
    tradeoffs: "Efficient and back-drivable: good for torque control and energy recovery, bad for holding a load unpowered (add a brake). Planetary stages pack high ratios coaxially; each stage costs about 3 % efficiency and adds backlash.",
    limits: "Efficiency independent of load and speed; no backlash, no no-load drag (add a damper for churning).",
    parameters: &[("ratio", "Input turns per output turn N."), ("efficiency", "Power out ÷ power in, the same in both directions."), ("smoothing_speed", "Speed ε below which the friction sign is smoothed."), ("correction", "Constraint drift correction rate (numerical).")],
    typical: &[("ratio", 10.0)],
    realtime: &[("smoothing_speed", 0.05)],
    pairs_with: SHAFT_PAIRS,
    derived: Some(|p| {
        let eta = p.get("efficiency").copied().unwrap_or(0.95);
        vec![sim_core::DerivedValue::new("forward efficiency", 100. * eta, "%", "η"), sim_core::DerivedValue::new("backdrive efficiency", 100. * eta, "%", "η (symmetric)"), sim_core::DerivedValue::new("self-locking", 0., "yes=1", "never: η > 0 both ways")]
    }),
    ..Notes::new("A gear stage with a fixed efficiency: efficient and back-drivable.")
};

static WORM_NOTES: Notes = Notes { category: "Transmissions",
    explanation: "A screw-shaped worm meshes with a toothed wheel at right angles. One worm turn advances the wheel by its number of starts, so a single-start worm gives a large ratio (z₂ : z₁) in one compact stage. The teeth slide along each other rather than rolling, so friction dominates the behaviour: efficiency depends on the lead angle and on which side drives, and a shallow lead makes the drive self-locking: the load cannot turn the worm. The model derives lead angle, ratio and both efficiencies from the gear geometry and computes tooth forces from the classical worm-gear force analysis.",
    equations: &[
        "tan λ = z₁·m / d₁   (lead angle)",
        "N = z₂ / z₁,   ω_worm = N·ω_wheel",
        "τ_worm  = r₁·(W cos φₙ sin λ + μ|W| s cos λ)",
        "τ_wheel = r₂·(W cos φₙ cos λ − μ|W| s sin λ),   s = tanh(ω_worm/ε)",
        "η_forward = (cos φₙ − μ tan λ) / (cos φₙ + μ cot λ)",
        "η_back = (cos φₙ − μ cot λ) / (cos φₙ + μ tan λ);  self-locking when μ ≥ cos φₙ tan λ",
    ],
    tradeoffs: "Large ratios in one stage, quiet, and (single start) self-locking, so a lift or joint holds its load with the motor unpowered. The price is efficiency: a self-locking worm is below 50 % forward efficient, so the motor draws more than twice the power the load receives and the rest heats the mesh. More starts raise the lead angle and efficiency but give up self-locking and ratio. Compare the ideal (spur) gear: efficient and backdrivable.",
    limits: "Constant friction coefficient (real μ falls with sliding speed, ~0.1 near stall to ~0.02 at several m/s); no backlash, tooth compliance, bearing or churning loss; the worm and wheel are massless here, so add inertias. Self-locking holds to within a creep speed set by ε. Friction values are estimates until measured.",
    parameters: &[
        ("module", "Axial module of the worm = transverse module of the wheel, m. Tooth size."),
        ("worm_starts", "Number of thread starts z₁ on the worm (1 = single start)."),
        ("wheel_teeth", "Number of teeth z₂ on the wheel."),
        ("worm_pitch_diameter", "Worm pitch diameter d₁. Smaller diameter → steeper lead."),
        ("pressure_angle", "Normal pressure angle φₙ (20° is standard)."),
        ("friction", "Sliding friction coefficient μ between worm and wheel (steel on bronze, lubricated: 0.02–0.1)."),
        ("smoothing_speed", "Speed ε below which friction is smoothed; sets the creep of a self-locked drive."),
        ("correction", "Constraint drift correction rate (numerical)."),
    ],
    realtime: &[("smoothing_speed", 0.05)],
    pairs_with: &["bridge.brushed_motor", INERTIA, TORQUE_SOURCE, DAMPER, SPEED_SENSOR, ANGLE_SENSOR, BACKLASH_MESH, IDEAL_GEAR],
    derived: Some(|p| crate::helical::worm_contact(p).derived("wheel", "rad")),
    ..Notes::new("A worm driving a wheel: high ratio in one stage, sliding friction, self-locking.")
};

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use sim_core::connectors::Rotational as R;
    for descriptor in [
        BehaviorDescriptor::new(INERTIA, "Rotational inertia", vec![acausal("shaft", R)], inertia).with_parameters(vec![P::required("inertia", "kg·m²").positive(), P::optional("damping", "N·m·s/rad", 0.0), P::optional("initial.speed", "rad/s", 0.0)]).with_notes(&INERTIA_NOTES),
        BehaviorDescriptor::new(SPRING, "Torsional spring", vec![acausal("a", R), acausal("b", R)], spring).with_parameters(vec![P::required("stiffness", "N·m/rad")]).with_notes(&SPRING_NOTES),
        BehaviorDescriptor::new(DAMPER, "Torsional damper", vec![acausal("a", R), acausal("b", R)], damper).with_parameters(vec![P::required("damping", "N·m·s/rad")]).with_notes(&DAMPER_NOTES),
        BehaviorDescriptor::new(GROUND, "Fixed angle", vec![acausal("flange", R)], ground).with_parameters(vec![P::optional("angle", "rad", 0.0)]).with_notes(&GROUND_NOTES),
        BehaviorDescriptor::new(TORQUE_SOURCE, "Commanded torque", vec![acausal("shaft", R), signal_in("torque", QuantityKind::Torque)], torque_source).with_parameters(vec![]).with_notes(&TORQUE_SOURCE_NOTES),
        BehaviorDescriptor::new(ANGLE_SENSOR, "Angle sensor", vec![acausal("shaft", R), signal_out("angle", QuantityKind::Angle)], angle_sensor).with_parameters(vec![]).with_notes(&ANGLE_SENSOR_NOTES),
        BehaviorDescriptor::new(SPEED_SENSOR, "Speed sensor", vec![acausal("shaft", R), signal_out("speed", QuantityKind::AngularVelocity)], speed_sensor).with_parameters(vec![]).with_notes(&SPEED_SENSOR_NOTES),
        BehaviorDescriptor::new(AVERAGE_SPEED_SENSOR, "Step-average speed sensor", vec![acausal("shaft", R), signal_out("speed", QuantityKind::AngularVelocity)], average_speed_sensor).with_parameters(vec![]),
        BehaviorDescriptor::new(SPEED_TRIP, "Speed trip latch", vec![acausal("shaft", R)], speed_trip).with_parameters(vec![P::required("threshold", "rad/s")]),
        BehaviorDescriptor::new(BACKLASH_MESH, "Compliant mesh with backlash", vec![acausal("a", R), acausal("b", R)], backlash_mesh).with_parameters(vec![P::required("stiffness", "N·m/rad"), P::optional("gap", "rad", 0.0).nonnegative()]).with_notes(&BACKLASH_NOTES),
        BehaviorDescriptor::new(IDEAL_GEAR, "Ideal gear", vec![acausal("input", R), acausal("output", R)], ideal_gear).with_parameters(vec![P::required("ratio", "1"), P::optional("correction", "1/s", 10.0)]).with_notes(&IDEAL_GEAR_NOTES),
    ] {
        registry.register(descriptor)?;
    }
    registry.register(
        BehaviorDescriptor::new(COULOMB_FRICTION, "Coulomb friction", vec![acausal("shaft", R)], coulomb_friction)
            .with_parameters(vec![P::required("torque", "N·m").nonnegative(), P::optional("smoothing_speed", "rad/s", 0.01).positive()])
            .with_notes(&COULOMB_NOTES),
    )?;
    registry.register(
        BehaviorDescriptor::new(LOSSY_GEAR, "Gear stage with losses", vec![acausal("input", R), acausal("output", R)], lossy_gear)
            .with_parameters(vec![P::required("ratio", "1"), P::optional("efficiency", "1", 0.95).positive().at_most(1.), P::optional("smoothing_speed", "rad/s", 0.01).positive(), P::optional("correction", "1/s", 10.0)])
            .with_notes(&LOSSY_GEAR_NOTES),
    )?;
    registry.register(BehaviorDescriptor::new(LOAD_TORQUE, "Load torque", vec![acausal("shaft", R)], load_torque).with_parameters(vec![P::required("torque", "N·m")]).with_notes(&LOAD_TORQUE_NOTES))?;
    let d = crate::helical::WORM_DEFAULTS;
    registry.register(
        BehaviorDescriptor::new(WORM_GEAR, "Worm gear", vec![acausal("worm", R), acausal("wheel", R)], worm_gear)
            .with_parameters(vec![
                P::optional("module", "m", d.module).positive(),
                P::optional("worm_starts", "1", d.starts).integer(1., 8.),
                P::optional("wheel_teeth", "1", d.teeth).integer(8., 400.),
                P::optional("worm_pitch_diameter", "m", d.worm_pitch_diameter).positive(),
                P::optional("pressure_angle", "rad", d.pressure_angle).positive().at_most(0.8),
                P::optional("friction", "1", d.friction).nonnegative().at_most(1.),
                P::optional("smoothing_speed", "rad/s", 0.01).positive(),
                P::optional("correction", "1/s", 10.0),
            ])
            .with_notes(&WORM_NOTES),
    )?;
    Ok(())
}
