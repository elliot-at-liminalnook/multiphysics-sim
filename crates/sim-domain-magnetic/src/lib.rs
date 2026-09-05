//! Magnetic domain: lumped magnetic circuits on the power-conjugate pair
//! `(mmf | flux_rate)` — the gyrator–capacitor picture, where a reluctance
//! stores energy `½·R·Φ²` and a coil is a gyrator to the electrical domain —
//! plus the field of a ring base for a dipole that flies above it.

use nalgebra::{UnitQuaternion, Vector3};
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or,
};
use std::collections::BTreeMap;

pub const GROUND: &str = "magnetic.ground";
pub const RELUCTANCE: &str = "magnetic.reluctance";
pub const SATURABLE_CORE: &str = "magnetic.saturable_core";
pub const PERMANENT_MAGNET: &str = "magnetic.permanent_magnet";
pub const COIL: &str = "bridge.coil";
pub const AIR_GAP: &str = "bridge.air_gap";
pub const EDDY_SHEET: &str = "bridge.eddy_sheet";
pub const MAGNETIC_TOP: &str = "bridge.magnetic_top";

pub const MU0: f64 = 4.0e-7 * std::f64::consts::PI;

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// mmf reference.
pub struct Ground;
impl Behavior for Ground {
    fn pinned(&self) -> Vec<(usize, usize, f64)> {
        vec![(0, 0, 0.0)]
    }
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("flux_rate", QuantityKind::Voltage, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.across(0));
        ctx.add_through(0, ctx.state(0));
    }
}

/// Linear reluctance `R = l/(μ₀μᵣA)` carrying flux Φ from `a` to `b`:
/// `mmf_a − mmf_b = R·Φ`, stored energy `½RΦ²`.
pub struct Reluctance {
    pub reluctance: f64,
    pub initial_flux: f64,
}
impl Behavior for Reluctance {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("flux", QuantityKind::MagneticFlux, self.initial_flux)]
    }
    fn residual(&self, ctx: &mut Context) {
        let flux = ctx.state(0);
        ctx.set_state_residual(0, ctx.across(0) - ctx.across(1) - self.reluctance * flux);
        let rate = ctx.state_rate(0);
        ctx.add_through(0, rate);
        ctx.add_through(1, -rate);
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.reluctance * view.state(0).powi(2)
    }
}

/// A core of `length` and `area` with a saturating B–H curve
/// `B = Bₛ·(2/π)·atan(π μ₀μᵣ H / (2Bₛ))`: linear at low field, flat at
/// saturation.
pub struct SaturableCore {
    pub length: f64,
    pub area: f64,
    pub saturation: f64,
    pub relative_permeability: f64,
}
impl SaturableCore {
    fn field_of(&self, flux_density: f64) -> f64 {
        let bs = self.saturation;
        let ratio = (flux_density / bs).clamp(-0.999_999, 0.999_999);
        (2.0 * bs / (std::f64::consts::PI * MU0 * self.relative_permeability)) * (std::f64::consts::FRAC_PI_2 * ratio).tan()
    }
}
impl Behavior for SaturableCore {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("flux", QuantityKind::MagneticFlux, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let flux = ctx.state(0);
        let h = self.field_of(flux / self.area);
        ctx.set_state_residual(0, ctx.across(0) - ctx.across(1) - h * self.length);
        let rate = ctx.state_rate(0);
        ctx.add_through(0, rate);
        ctx.add_through(1, -rate);
    }
    fn energy(&self, view: &View) -> f64 {
        // ∫H dB for the atan law: −(2Bₛ/π)²/(μ₀μᵣ)·ln cos(πB/2Bₛ), per volume.
        let bs = self.saturation;
        let ratio = (view.state(0) / self.area / bs).clamp(-0.999_999, 0.999_999);
        let per_volume = -(2.0 * bs / std::f64::consts::PI).powi(2) / (MU0 * self.relative_permeability) * (std::f64::consts::FRAC_PI_2 * ratio).cos().ln();
        per_volume * self.area * self.length
    }
}

/// Permanent magnet: coercive mmf behind an internal reluctance, driving
/// flux out of `b`: `mmf_a − mmf_b = R_i·Φ − F_c`.
pub struct PermanentMagnet {
    pub coercive_mmf: f64,
    pub reluctance: f64,
}
impl Behavior for PermanentMagnet {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("flux", QuantityKind::MagneticFlux, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let flux = ctx.state(0);
        ctx.set_state_residual(0, ctx.across(0) - ctx.across(1) - (self.reluctance * flux - self.coercive_mmf));
        let rate = ctx.state_rate(0);
        ctx.add_through(0, rate);
        ctx.add_through(1, -rate);
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.reluctance * view.state(0).powi(2)
    }
}

/// N-turn coil: a gyrator between the winding (`p`, `n`) and the magnetic
/// path (`a`, `b`): `mmf_a − mmf_b = N·i`, `v_p − v_n = N·dΦ/dt`.
pub struct Coil {
    pub turns: f64,
}
impl Behavior for Coil {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        // Gyrator: electrical power in is magnetic power out, so the flux
        // rate leaves the coil at `a`.
        let current = (ctx.across(2) - ctx.across(3)) / self.turns;
        let flux_rate = (ctx.across(0) - ctx.across(1)) / self.turns;
        ctx.add_through(0, current);
        ctx.add_through(1, -current);
        ctx.add_through(2, -flux_rate);
        ctx.add_through(3, flux_rate);
    }
}

/// An air gap of `area` whose length is `gap + x`, `x` the position of the
/// translational `armature` port: reluctance `(gap + x)/(μ₀A)` and the
/// pull `−∂(½RΦ²)/∂x = −Φ²/(2μ₀A)` on the armature.
pub struct AirGap {
    pub area: f64,
    pub gap: f64,
}
impl AirGap {
    pub fn pull(&self, flux: f64) -> f64 {
        -flux * flux / (2.0 * MU0 * self.area)
    }
}
impl Behavior for AirGap {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("flux", QuantityKind::MagneticFlux, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let flux = ctx.state(0);
        let length = (self.gap + ctx.across(2)).max(1.0e-9);
        let reluctance = length / (MU0 * self.area);
        ctx.set_state_residual(0, ctx.across(0) - ctx.across(1) - reluctance * flux);
        let rate = ctx.state_rate(0);
        ctx.add_through(0, rate);
        ctx.add_through(1, -rate);
        // Force on the armature node: through into the element is minus it.
        ctx.add_through(2, -self.pull(flux));
    }
    fn energy(&self, view: &View) -> f64 {
        let length = (self.gap + view.across(2)).max(1.0e-9);
        0.5 * length / (MU0 * self.area) * view.state(0).powi(2)
    }
}

/// Eddy-current sheet: a magnetic "resistor" `mmf_a − mmf_b = k·dΦ/dt`
/// whose loss `k·(dΦ/dt)²` leaves through the thermal port.
pub struct EddySheet {
    pub loss: f64,
}
impl Behavior for EddySheet {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let drop = ctx.across(0) - ctx.across(1);
        let flux_rate = drop / self.loss;
        ctx.add_through(0, flux_rate);
        ctx.add_through(1, -flux_rate);
        ctx.add_through(2, -drop * flux_rate);
    }
}

/// Complete elliptic integrals K(k), E(k) by the arithmetic–geometric mean.
pub fn elliptic_ke(k: f64) -> (f64, f64) {
    let k = k.clamp(0.0, 1.0 - 1.0e-15);
    let (mut a, mut b) = (1.0, (1.0 - k * k).sqrt());
    let mut c = k;
    let mut sum = 0.5 * c * c;
    let mut power = 0.5;
    for _ in 0..40 {
        let an = 0.5 * (a + b);
        let bn = (a * b).sqrt();
        c = 0.5 * (a - b);
        a = an;
        b = bn;
        power *= 2.0;
        sum += power * c * c;
        if c.abs() < 1.0e-16 {
            break;
        }
    }
    let kk = std::f64::consts::FRAC_PI_2 / a;
    (kk, kk * (1.0 - sum))
}

/// Field of a circular loop of radius `a` in the plane `z = z0`, scaled so
/// the field at its centre is `b0` (`μ₀I = 2·a·b0`). Exact, via K and E.
#[derive(Clone, Copy, Debug)]
pub struct LoopField {
    pub radius: f64,
    pub centre_field: f64,
    pub z0: f64,
}
impl LoopField {
    pub fn field(&self, r: Vector3<f64>) -> Vector3<f64> {
        let a = self.radius;
        let rho = (r.x * r.x + r.y * r.y).sqrt();
        let z = r.z - self.z0;
        let mu0_i = 2.0 * a * self.centre_field;
        let denom_plus = ((a + rho).powi(2) + z * z).sqrt();
        let denom_minus = (a - rho).powi(2) + z * z;
        let k = (4.0 * a * rho).sqrt() / denom_plus;
        let (kk, ee) = elliptic_ke(k);
        let common = mu0_i / (2.0 * std::f64::consts::PI) / denom_plus;
        let bz = common * (kk + (a * a - rho * rho - z * z) / denom_minus * ee);
        let brho = if rho < 1.0e-12 { 0.0 } else { common * z / rho * (-kk + (a * a + rho * rho + z * z) / denom_minus * ee) };
        let (cx, cy) = if rho < 1.0e-12 { (0.0, 0.0) } else { (r.x / rho, r.y / rho) };
        Vector3::new(brho * cx, brho * cy, bz)
    }
    /// On-axis field magnitude and its first two z-derivatives.
    pub fn on_axis(&self, z: f64) -> (f64, f64, f64) {
        let h = 1.0e-5 * self.radius;
        let b = |z: f64| self.field(Vector3::new(0.0, 0.0, z)).z;
        (b(z), (b(z + h) - b(z - h)) / (2.0 * h), (b(z + h) - 2.0 * b(z) + b(z - h)) / (h * h))
    }
}

/// A rigid body (a `Frame` port) carrying a dipole `moment` along its body
/// z axis in a `LoopField`: force `∇(μ·B)`, torque `μ×B`, energy `−μ·B`.
/// Spin it and it is a Levitron.
pub struct MagneticTop {
    pub moment: f64,
    pub field: LoopField,
}
impl MagneticTop {
    pub fn dipole(&self, s: &[f64]) -> Vector3<f64> {
        let q = UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(s[3], s[4], s[5], s[6]));
        q * Vector3::new(0.0, 0.0, self.moment)
    }
    pub fn wrench(&self, s: &[f64]) -> (Vector3<f64>, Vector3<f64>, f64) {
        let r = Vector3::new(s[0], s[1], s[2]);
        let m = self.dipole(s);
        let b = self.field.field(r);
        let h = 1.0e-6 * self.field.radius;
        let mut force = Vector3::zeros();
        for k in 0..3 {
            let mut e = Vector3::zeros();
            e[k] = h;
            force[k] = (m.dot(&self.field.field(r + e)) - m.dot(&self.field.field(r - e))) / (2.0 * h);
        }
        (force, m.cross(&b), -m.dot(&b))
    }
}
impl Behavior for MagneticTop {
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

fn ground(_: &Params) -> Made {
    Ok(Box::new(Ground))
}
fn reluctance(p: &Params) -> Made {
    let reluctance = match p.get("reluctance") {
        Some(r) => *r,
        None => param(p, "length")? / (MU0 * param_or(p, "relative_permeability", 1.0) * param(p, "area")?),
    };
    Ok(Box::new(Reluctance { reluctance, initial_flux: param_or(p, "initial.flux", 0.0) }))
}
fn saturable_core(p: &Params) -> Made {
    Ok(Box::new(SaturableCore { length: param(p, "length")?, area: param(p, "area")?, saturation: param(p, "saturation")?, relative_permeability: param_or(p, "relative_permeability", 1000.0) }))
}
fn permanent_magnet(p: &Params) -> Made {
    Ok(Box::new(PermanentMagnet { coercive_mmf: param(p, "coercive_mmf")?, reluctance: param(p, "reluctance")? }))
}
fn coil(p: &Params) -> Made {
    Ok(Box::new(Coil { turns: param(p, "turns")? }))
}
fn air_gap(p: &Params) -> Made {
    Ok(Box::new(AirGap { area: param(p, "area")?, gap: param(p, "gap")? }))
}
fn eddy_sheet(p: &Params) -> Made {
    Ok(Box::new(EddySheet { loss: param(p, "loss")? }))
}
fn magnetic_top(p: &Params) -> Made {
    Ok(Box::new(MagneticTop {
        moment: param(p, "moment")?,
        field: LoopField { radius: param(p, "ring_radius")?, centre_field: param(p, "ring_field")?, z0: param_or(p, "ring_z", 0.0) },
    }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use ConnectorKind::{Electrical as E, Frame as F, Magnetic as M, Thermal as H, Translational as T};
    for descriptor in [
        BehaviorDescriptor::new(GROUND, "mmf reference", vec![acausal("node", M)], ground).with_parameters(vec![]),
        BehaviorDescriptor::new(RELUCTANCE, "Linear reluctance", vec![acausal("a", M), acausal("b", M)], reluctance).with_parameters(vec![P::alternative("reluctance", "1/H").positive(), P::alternative("length", "m").positive(), P::alternative("area", "m²").positive(), P::optional("relative_permeability", "1", 1.0).positive(), P::optional("initial.flux", "Wb", 0.0)]),
        BehaviorDescriptor::new(SATURABLE_CORE, "Saturable core", vec![acausal("a", M), acausal("b", M)], saturable_core).with_parameters(vec![P::required("length", "m").positive(), P::required("area", "m²").positive(), P::required("saturation", "T").positive(), P::optional("relative_permeability", "1", 1000.0).positive()]),
        BehaviorDescriptor::new(PERMANENT_MAGNET, "Permanent magnet", vec![acausal("a", M), acausal("b", M)], permanent_magnet).with_parameters(vec![P::required("coercive_mmf", "A"), P::required("reluctance", "1/H").positive()]),
        BehaviorDescriptor::new(COIL, "Coil (electrical ↔ magnetic gyrator)", vec![acausal("p", E), acausal("n", E), acausal("a", M), acausal("b", M)], coil).with_parameters(vec![P::required("turns", "1")]),
        BehaviorDescriptor::new(AIR_GAP, "Air gap with armature", vec![acausal("a", M), acausal("b", M), acausal("armature", T)], air_gap).with_parameters(vec![P::required("area", "m²").positive(), P::required("gap", "m").positive()]),
        BehaviorDescriptor::new(EDDY_SHEET, "Eddy-current sheet", vec![acausal("a", M), acausal("b", M), acausal("heat", H)], eddy_sheet).with_parameters(vec![P::required("loss", "S").nonnegative()]),
        BehaviorDescriptor::new(MAGNETIC_TOP, "Dipole body in a ring field", vec![acausal("frame", F)], magnetic_top).with_parameters(vec![P::required("moment", "A·m²"), P::required("ring_radius", "m").positive(), P::required("ring_field", "T"), P::optional("ring_z", "m", 0.0)]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::ModelWorld;
    use sim_domain_electrical::elements as el;

    fn registry() -> BehaviorRegistry {
        let mut r = BehaviorRegistry::default();
        el::register(&mut r).unwrap();
        sim_domain_translational::elements::register(&mut r).unwrap();
        sim_domain_thermal::register(&mut r).unwrap();
        register(&mut r).unwrap();
        r
    }

    /// A coil on a reluctance is an inductor `L = N²/R`: with a series
    /// resistor on a voltage step the current rises as `V/R_e·(1 − e^{−t/τ})`.
    #[test]
    fn coil_on_reluctance_is_an_inductor() {
        let registry = registry();
        let (turns, reluctance, resistance, volts) = (200.0, 4.0e6, 10.0, 5.0);
        let mut m = ModelWorld::default();
        let source = m.part(&registry, "source", el::VOLTAGE_SOURCE, [("voltage", volts)]).unwrap();
        let resistor = m.part(&registry, "resistor", el::RESISTOR, [("resistance", resistance)]).unwrap();
        let coil = m.part(&registry, "coil", COIL, [("turns", turns)]).unwrap();
        let core = m.part(&registry, "core", RELUCTANCE, [("reluctance", reluctance)]).unwrap();
        let ground = m.part(&registry, "ground", el::GROUND, []).unwrap();
        let mground = m.part(&registry, "mground", GROUND, []).unwrap();
        m.connect([source.port("p"), resistor.port("p")]);
        m.connect([resistor.port("n"), coil.port("p")]);
        m.connect([coil.port("n"), source.port("n"), ground.port("pin")]);
        m.connect([coil.port("a"), core.port("a")]);
        m.connect([core.port("b"), coil.port("b"), mground.port("node")]);
        let mut runtime = sim_compile::Runtime::new(m, &registry, sim_dynamics::Integrator::ImplicitMidpoint(Default::default())).unwrap();
        let flux = runtime.state_id(core.behavior, "flux");
        let inductance = turns * turns / reluctance;
        let tau = inductance / resistance;
        runtime.advance(tau, tau / 400.0).unwrap();
        let current = runtime.get(flux) * reluctance / turns;
        let expected = volts / resistance * (1.0 - (-1.0_f64).exp());
        assert!((current - expected).abs() < 1.0e-3 * expected, "current {current} vs {expected}");
    }

    /// The pull on an armature equals `Φ²/(2μ₀A)` for the flux the circuit settles at.
    #[test]
    fn air_gap_pull_matches_energy_gradient() {
        let gap = AirGap { area: 1.0e-4, gap: 1.0e-3 };
        let flux = 2.0e-5;
        let energy = |x: f64| 0.5 * (gap.gap + x) / (MU0 * gap.area) * flux * flux;
        let h = 1.0e-7;
        let numerical = -(energy(h) - energy(-h)) / (2.0 * h);
        assert!((gap.pull(flux) - numerical).abs() < 1.0e-6 * numerical.abs());
    }

    /// Loop field on the axis is the textbook `μ₀I a²/(2(a²+z²)^{3/2})`.
    #[test]
    fn loop_field_on_axis() {
        let field = LoopField { radius: 0.05, centre_field: 0.02, z0: 0.0 };
        for z in [0.0, 0.02, 0.05, 0.1] {
            let b = field.field(Vector3::new(0.0, 0.0, z)).z;
            let expected = 0.02 * 0.05_f64.powi(3) / (0.05_f64.powi(2) + z * z).powf(1.5);
            assert!((b - expected).abs() < 1.0e-9, "z={z}: {b} vs {expected}");
        }
        // Off axis the field is divergence-free: ∇·B = 0.
        let r = Vector3::new(0.02, 0.01, 0.03);
        let h = 1.0e-6;
        let mut div = 0.0;
        for k in 0..3 {
            let mut e = Vector3::zeros();
            e[k] = h;
            div += (field.field(r + e)[k] - field.field(r - e)[k]) / (2.0 * h);
        }
        assert!(div.abs() < 1.0e-6 * field.field(r).norm() / 0.05, "divergence {div}");
    }
}
