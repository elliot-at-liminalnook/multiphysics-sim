//! Radiative transfer as a network: across the radiosity (W/m²) of a
//! surface's band, through the radiant power (W). A surface is an
//! Oppenheim resistance `(1−ε)/(εA)` between its band-limited blackbody
//! emissive power and its radiosity node; a view factor is a conductance
//! `A·F` between two radiosity nodes; a sky pins a node at what a band of
//! the atmosphere radiates. One physical surface in several bands is
//! several surface elements sharing one thermal node.

use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, QuantityKind, RegistryError,
    StateDeclaration, acausal, param, param_or,
};
use std::collections::BTreeMap;

pub const SURFACE: &str = "radiation.surface";
pub const VIEW: &str = "radiation.view";
pub const SKY: &str = "radiation.sky";

pub const STEFAN_BOLTZMANN: f64 = 5.670_374_419e-8;
/// Second radiation constant, µm·K.
const C2: f64 = 14_387.77;

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// Fraction of blackbody emission below wavelength `lambda` (µm) at `t` (K).
pub fn planck_fraction(lambda: f64, t: f64) -> f64 {
    if lambda <= 0.0 || t <= 0.0 {
        return 0.0;
    }
    let zeta = C2 / (lambda * t);
    let mut sum = 0.0;
    for n in 1..=200 {
        let n = n as f64;
        sum += (-n * zeta).exp() / n * (zeta.powi(3) + 3.0 * zeta * zeta / n + 6.0 * zeta / (n * n) + 6.0 / (n * n * n));
    }
    (15.0 / std::f64::consts::PI.powi(4) * sum).clamp(0.0, 1.0)
}

/// A wavelength band in µm; `hi = ∞` (any value ≥ 1e6) means "and beyond".
#[derive(Clone, Copy, Debug)]
pub struct Band {
    pub lo: f64,
    pub hi: f64,
}
impl Band {
    pub const ALL: Band = Band { lo: 0.0, hi: 1.0e9 };
    pub fn fraction(&self, t: f64) -> f64 {
        let upper = if self.hi >= 1.0e6 { 1.0 } else { planck_fraction(self.hi, t) };
        (upper - planck_fraction(self.lo, t)).max(0.0)
    }
    /// Blackbody emissive power in this band, W/m².
    pub fn emissive_power(&self, t: f64) -> f64 {
        self.fraction(t) * STEFAN_BOLTZMANN * t.powi(4)
    }
}

/// An opaque surface of `area` and `emissivity` in `band`: radiates
/// `εA(E_b(T) − J)/(1−ε)` into its radiosity node and draws that power
/// from its thermal node. Emission at T carries entropy `q/T` away.
pub struct Surface {
    pub area: f64,
    pub emissivity: f64,
    pub band: Band,
}
impl Behavior for Surface {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let t = ctx.across(1);
        let eps = self.emissivity.clamp(1.0e-6, 0.999);
        let radiated = eps * self.area / (1.0 - eps) * (self.band.emissive_power(t) - ctx.across(0));
        ctx.add_through(0, -radiated);
        ctx.add_through(1, radiated);
        ctx.store_entropy(radiated / t);
    }
}

/// Exchange between two radiosity nodes through view factor `F` from a
/// surface of `area`: `A·F·(J_a − J_b)`.
pub struct View {
    pub area: f64,
    pub factor: f64,
}
impl Behavior for View {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let flow = self.area * self.factor * (ctx.across(0) - ctx.across(1));
        ctx.add_through(0, flow);
        ctx.add_through(1, -flow);
    }
}

/// The sky in one band: a node held at the emissive power of a blackbody
/// at the band's effective temperature (the atmosphere's, or space's
/// through the atmospheric window).
pub struct Sky {
    pub temperature: f64,
    pub band: Band,
}
impl Behavior for Sky {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("radiant_flux", QuantityKind::Power, 0.0)]
    }
    fn pinned(&self) -> Vec<(usize, usize, f64)> {
        vec![(0, 0, self.band.emissive_power(self.temperature))]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.across(0) - self.band.emissive_power(self.temperature));
        ctx.add_through(0, ctx.state(0));
    }
}

fn band_of(p: &Params) -> Band {
    Band { lo: param_or(p, "band_lo", 0.0), hi: param_or(p, "band_hi", 1.0e9) }
}
fn surface(p: &Params) -> Made {
    Ok(Box::new(Surface { area: param(p, "area")?, emissivity: param(p, "emissivity")?, band: band_of(p) }))
}
fn view(p: &Params) -> Made {
    Ok(Box::new(View { area: param(p, "area")?, factor: param_or(p, "factor", 1.0) }))
}
fn sky(p: &Params) -> Made {
    Ok(Box::new(Sky { temperature: param(p, "temperature")?, band: band_of(p) }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use ConnectorKind::{Radiative as Rd, Thermal as H};
    for descriptor in [
        BehaviorDescriptor::new(SURFACE, "Band-limited opaque surface", vec![acausal("face", Rd), acausal("heat", H)], surface).with_parameters(vec![P::required("area", "m²").positive(), P::required("emissivity", "1").nonnegative().at_most(1.0), P::optional("band_lo", "µm", 0.0).nonnegative(), P::optional("band_hi", "µm", 1.0e9).positive()]),
        BehaviorDescriptor::new(VIEW, "View factor between two surfaces", vec![acausal("a", Rd), acausal("b", Rd)], view).with_parameters(vec![P::required("area", "m²").positive(), P::optional("factor", "1", 1.0).nonnegative().at_most(1.0)]),
        BehaviorDescriptor::new(SKY, "Sky in one band", vec![acausal("node", Rd)], sky).with_parameters(vec![P::required("temperature", "K").nonnegative(), P::optional("band_lo", "µm", 0.0).nonnegative(), P::optional("band_hi", "µm", 1.0e9).positive()]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn planck_fractions() {
        // Half of a 300 K blackbody's power lies below λT ≈ 4107 µm·K.
        assert!((planck_fraction(4107.0 / 300.0, 300.0) - 0.5).abs() < 2.0e-3);
        assert!((planck_fraction(1.0e6, 300.0) - 1.0).abs() < 1.0e-6);
        let window = Band { lo: 8.0, hi: 13.0 };
        let f = window.fraction(300.0);
        assert!((f - 0.32).abs() < 0.03, "window fraction {f}");
    }
}
