//! Sliding helical contact: a thread driving a follower, the mechanism
//! shared by worm gears and lead screws.
//!
//! Geometry (explicit, from the parameters): the driving thread has pitch
//! radius `r₁` and lead angle `λ`; the follower moves `arm` metres of
//! pitch-line travel per unit of its coordinate (a wheel's pitch radius, or
//! 1 for a nut that translates). Kinematics are exact:
//!
//! ```text
//! r₁·ω₁·tan λ = arm·ω₂        ⇒   ω₁ = N·ω₂,   N = arm / (r₁·tan λ)
//! ```
//!
//! Forces follow the classical worm/screw analysis (Shigley §15-7 for worms,
//! §8-2 for power screws). `W` is the tooth normal force, `φₙ` the normal
//! pressure (thread) angle, `μ` the sliding friction coefficient and
//! `s = tanh(ω₁/ε)` a regularised sign of the sliding direction:
//!
//! ```text
//! τ₁ =  r₁ · (W·cos φₙ·sin λ + μ·|W|·s·cos λ)       torque into the thread
//! F₂ = −arm· (W·cos φₙ·cos λ − μ·|W|·s·sin λ)       generalized force into the follower
//! loss = τ₁ω₁ + F₂ω₂ = μ·|W|·|v_s|·|s| ≥ 0,   v_s = r₁ω₁ / cos λ
//! ```
//!
//! Directional efficiency and self-locking are therefore *consequences* of
//! the contact model, not separate rules: when `μ > cos φₙ·tan λ` no follower
//! load can turn the thread, and the model holds it (to within the creep the
//! regularisation `ε` allows).
use sim_core::{Behavior, Context, DerivedValue, Input, LocalJacobian, Output, QuantityKind, StateDeclaration, View};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HelicalContact {
    /// Pitch radius of the driving thread, m.
    pub thread_radius: f64,
    /// Lead angle λ, rad.
    pub lead_angle: f64,
    /// Normal pressure (thread flank) angle φₙ, rad.
    pub pressure_angle: f64,
    /// Sliding friction coefficient μ.
    pub friction: f64,
    /// Follower pitch-line travel per unit of its coordinate: the wheel pitch
    /// radius (m/rad) for a worm gear, 1 for a translating nut.
    pub arm: f64,
}

impl HelicalContact {
    /// Thread coordinate per follower coordinate (rad/rad or rad/m).
    pub fn ratio(&self) -> f64 {
        self.arm / (self.thread_radius * self.lead_angle.tan())
    }
    /// Efficiency when the thread drives the follower.
    pub fn forward_efficiency(&self) -> f64 {
        let (c, t) = (self.pressure_angle.cos(), self.lead_angle.tan());
        (c - self.friction * t) / (c + self.friction / t)
    }
    /// Efficiency when the follower drives the thread; ≤ 0 means self-locking.
    pub fn backdrive_efficiency(&self) -> f64 {
        let (c, t) = (self.pressure_angle.cos(), self.lead_angle.tan());
        (c - self.friction / t) / (c + self.friction * t)
    }
    pub fn self_locking(&self) -> bool {
        self.friction >= self.pressure_angle.cos() * self.lead_angle.tan()
    }
    /// (τ₁, F₂) for normal force `w` and friction sign `s` ∈ [−1, 1].
    pub fn loads(&self, w: f64, s: f64) -> (f64, f64) {
        let (sl, cl, cp) = (self.lead_angle.sin(), self.lead_angle.cos(), self.pressure_angle.cos());
        let a = w.abs();
        (self.thread_radius * (w * cp * sl + self.friction * a * s * cl), -self.arm * (w * cp * cl - self.friction * a * s * sl))
    }

    pub fn derived(&self, follower: &str, follower_unit: &str) -> Vec<DerivedValue> {
        let backdrive = self.backdrive_efficiency();
        vec![
            DerivedValue::new("lead angle", self.lead_angle.to_degrees(), "deg", "λ = atan(lead / (π·d₁))"),
            DerivedValue::new("ratio", self.ratio(), &format!("rad per {follower_unit}"), &format!("N = {follower} travel ÷ (r₁·tan λ)")),
            DerivedValue::new("forward efficiency", 100. * self.forward_efficiency(), "%", "η = (cos φₙ − μ tan λ) / (cos φₙ + μ cot λ)"),
            DerivedValue::new("backdrive efficiency", 100. * backdrive.max(0.), "%", "η' = (cos φₙ − μ cot λ) / (cos φₙ + μ tan λ), 0 when locked"),
            DerivedValue::new("self-locking", if self.self_locking() { 1. } else { 0. }, "yes=1", "μ ≥ cos φₙ · tan λ"),
            DerivedValue::new("locking margin", self.friction / (self.pressure_angle.cos() * self.lead_angle.tan()), "1", "μ / (cos φₙ tan λ); > 1 locks"),
        ]
    }
}

/// The behavior: port 0 is the thread (rotational), port 1 the follower
/// (rotational wheel or translational nut). One state, the normal force `W`,
/// enforces the kinematic constraint at velocity level with a Baumgarte
/// position correction, like `rotational.ideal_gear`.
pub struct HelicalDrive {
    pub contact: HelicalContact,
    /// Regularisation speed ε of the friction sign, rad/s at the thread.
    pub smoothing_speed: f64,
    pub correction: f64,
}

impl Behavior for HelicalDrive {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("normal_force", QuantityKind::Force, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let n = self.contact.ratio();
        let drift = ctx.across(0) - n * ctx.across(1);
        let omega = ctx.across_rate(0);
        ctx.set_state_residual(0, omega - n * ctx.across_rate(1) + self.correction * drift);
        let (t1, f2) = self.contact.loads(ctx.state(0), (omega / self.smoothing_speed).tanh());
        ctx.add_through(0, t1);
        ctx.add_through(1, f2);
    }
    fn jacobian(&self, view: &View, out: &mut LocalJacobian) -> bool {
        let n = self.contact.ratio();
        out.set(Output::State(0), Input::AcrossRate(0, 0), 1.0);
        out.set(Output::State(0), Input::AcrossRate(1, 0), -n);
        out.set(Output::State(0), Input::Across(0, 0), self.correction);
        out.set(Output::State(0), Input::Across(1, 0), -self.correction * n);
        let HelicalContact { thread_radius: r1, lead_angle, pressure_angle, friction: mu, arm } = self.contact;
        let (sl, cl, cp) = (lead_angle.sin(), lead_angle.cos(), pressure_angle.cos());
        let w = view.state(0);
        let s = (view.across_rate(0) / self.smoothing_speed).tanh();
        let ds = (1. - s * s) / self.smoothing_speed;
        let sw = if w > 0. { 1. } else if w < 0. { -1. } else { 0. };
        out.through(0, Input::State(0), r1 * (cp * sl + mu * sw * s * cl));
        out.through(0, Input::AcrossRate(0, 0), r1 * mu * w.abs() * cl * ds);
        out.through(1, Input::State(0), -arm * (cp * cl - mu * sw * s * sl));
        out.through(1, Input::AcrossRate(0, 0), arm * mu * w.abs() * sl * ds);
        true
    }
}

type Params = BTreeMap<String, f64>;

fn get(p: &Params, name: &str, default: f64) -> f64 {
    p.get(name).copied().unwrap_or(default)
}

/// Worm-gear geometry from standard gear parameters (module, starts, teeth,
/// worm pitch diameter). Missing values fall back to the registry defaults.
pub fn worm_contact(p: &Params) -> HelicalContact {
    let module = get(p, "module", WORM_DEFAULTS.module);
    let starts = get(p, "worm_starts", WORM_DEFAULTS.starts);
    let teeth = get(p, "wheel_teeth", WORM_DEFAULTS.teeth);
    let d1 = get(p, "worm_pitch_diameter", WORM_DEFAULTS.worm_pitch_diameter);
    HelicalContact {
        thread_radius: d1 / 2.,
        // Lead = z₁·π·m; tan λ = lead / (π·d₁) = z₁·m / d₁.
        lead_angle: (starts * module / d1).atan(),
        pressure_angle: get(p, "pressure_angle", WORM_DEFAULTS.pressure_angle),
        friction: get(p, "friction", WORM_DEFAULTS.friction),
        // Wheel pitch radius r₂ = z₂·m / 2, so N = z₂ / z₁ exactly.
        arm: teeth * module / 2.,
    }
}

/// Power-screw geometry from lead and pitch diameter.
pub fn screw_contact(p: &Params) -> HelicalContact {
    let lead = get(p, "lead", SCREW_DEFAULTS.lead);
    let d = get(p, "pitch_diameter", SCREW_DEFAULTS.pitch_diameter);
    HelicalContact {
        thread_radius: d / 2.,
        lead_angle: (lead / (std::f64::consts::PI * d)).atan(),
        pressure_angle: get(p, "pressure_angle", SCREW_DEFAULTS.pressure_angle),
        friction: get(p, "friction", SCREW_DEFAULTS.friction),
        arm: 1.,
    }
}

pub struct WormDefaults {
    pub module: f64,
    pub starts: f64,
    pub teeth: f64,
    pub worm_pitch_diameter: f64,
    pub pressure_angle: f64,
    pub friction: f64,
}
/// A small 0.5-module steel worm on a bronze wheel, 30:1.
pub const WORM_DEFAULTS: WormDefaults = WormDefaults { module: 0.5e-3, starts: 1., teeth: 30., worm_pitch_diameter: 8e-3, pressure_angle: 20f64 * std::f64::consts::PI / 180., friction: 0.06 };

pub struct ScrewDefaults {
    pub lead: f64,
    pub pitch_diameter: f64,
    pub pressure_angle: f64,
    pub friction: f64,
}
/// A Tr8×2 trapezoidal screw with a bronze nut (30° thread: 15° flank).
pub const SCREW_DEFAULTS: ScrewDefaults = ScrewDefaults { lead: 2e-3, pitch_diameter: 7e-3, pressure_angle: 15f64 * std::f64::consts::PI / 180., friction: 0.15 };

#[cfg(test)]
mod tests {
    use super::*;

    fn worm(mu: f64) -> HelicalContact {
        let mut p = Params::new();
        p.insert("friction".into(), mu);
        worm_contact(&p)
    }

    #[test]
    fn ratio_is_teeth_over_starts() {
        assert!((worm(0.05).ratio() - 30.).abs() < 1e-12);
        let mut p = Params::new();
        p.insert("worm_starts".into(), 2.);
        p.insert("wheel_teeth".into(), 40.);
        assert!((worm_contact(&p).ratio() - 20.).abs() < 1e-12);
    }

    #[test]
    fn loads_reproduce_textbook_efficiencies_and_dissipate() {
        for mu in [0.0, 0.02, 0.06, 0.12] {
            let c = worm(mu);
            let n = c.ratio();
            let w1 = 100.0; // worm driving forward
            let (t1, f2) = c.loads(10.0, 1.0);
            let w2 = w1 / n;
            let eta = -f2 * w2 / (t1 * w1);
            assert!((eta - c.forward_efficiency()).abs() < 1e-12, "μ={mu}: {eta} vs {}", c.forward_efficiency());
            let loss = t1 * w1 + f2 * w2;
            let v_s = c.thread_radius * w1 / c.lead_angle.cos();
            assert!((loss - mu * 10.0 * v_s).abs() < 1e-9 * (1. + loss.abs()));
            // Backdriving: follower pushes (power in at port 1), thread moves backwards
            // relative to the load direction, so W·ω₁ < 0.
            if !c.self_locking() {
                let (t1, f2) = c.loads(10.0, -1.0);
                let eta_b = (-t1 * -w1) / (f2 * -w2);
                assert!((eta_b - c.backdrive_efficiency()).abs() < 1e-12, "μ={mu}: {eta_b}");
            }
        }
    }

    #[test]
    fn self_locking_threshold() {
        let c = worm(0.0);
        let threshold = c.pressure_angle.cos() * c.lead_angle.tan();
        assert!(!worm(threshold * 0.99).self_locking());
        assert!(worm(threshold * 1.01).self_locking());
        assert!(worm(threshold * 1.01).backdrive_efficiency() < 0.);
    }
}
