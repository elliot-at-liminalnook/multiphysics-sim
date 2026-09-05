//! Two-phase fluid on the `FluidPh` connector: across `(pressure,
//! enthalpy)`, through `(mass_flow, enthalpy_flow)`. Volumes carry mass and
//! energy and *provide* both across lanes from their (p, h) states; pipes
//! and valves move mass and carry enthalpy upwind; a compact water equation
//! of state decides phase, density and temperature from (p, h).

use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, Provision, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or, signal_out,
};
use std::collections::BTreeMap;

pub const VOLUME_PH: &str = "fluid.volume_ph";
pub const PIPE_PH: &str = "fluid.pipe_ph";
pub const VALVE_PH: &str = "fluid.valve_ph";
pub const PUMP: &str = "fluid.pump";
pub const RESERVOIR_PH: &str = "fluid.reservoir_ph";
pub const WALL_HEAT: &str = "bridge.wall_heat";
pub const TANK_PH: &str = "fluid.tank_ph";
/// The equation of state behind every element here (not an element itself).
pub const EOS_WATER: &str = "fluid.eos_water";

pub const G: f64 = 9.81;
pub const ATMOSPHERE: f64 = 101_325.0;

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Liquid,
    TwoPhase,
    Vapour,
}

#[derive(Clone, Copy, Debug)]
pub struct FluidState {
    pub temperature: f64,
    pub density: f64,
    /// Vapour mass fraction, clamped to [0, 1].
    pub quality: f64,
    pub phase: Phase,
}

/// A compact water equation of state from (p, h): Clausius–Clapeyron
/// saturation curve, linear liquid enthalpy, latent heat falling with
/// temperature, ideal-gas steam, slightly compressible liquid. Good to a
/// few percent from 0 to ~200 °C, which is what a hot spring needs.
#[derive(Clone, Copy, Debug)]
pub struct Water;
impl Water {
    pub const CP_LIQUID: f64 = 4186.0;
    pub const CP_VAPOUR: f64 = 2000.0;
    pub const R_VAPOUR: f64 = 461.5;
    pub const BULK_MODULUS: f64 = 2.2e9;
    /// Enthalpy transition band as a fraction of the latent heat: the
    /// phase edges are smoothed over about 1 % of it (a subcooled-boiling
    /// band of a few kelvin), which is what lets a step walk onto them.
    const BAND: f64 = 1.0e-2;

    pub fn saturation_temperature(p: f64) -> f64 {
        let latent = 2.26e6;
        1.0 / (1.0 / 373.15 - Self::R_VAPOUR / latent * (p.max(1.0) / ATMOSPHERE).ln())
    }
    pub fn latent_heat(t: f64) -> f64 {
        2.501e6 - 2.4e3 * (t - 273.15)
    }
    pub fn liquid_enthalpy(t: f64) -> f64 {
        Self::CP_LIQUID * (t - 273.15)
    }
    pub fn liquid_density(t: f64, p: f64) -> f64 {
        (1000.0 - 0.4 * (t - 273.15)) * (1.0 + (p - ATMOSPHERE) / Self::BULK_MODULUS)
    }
    pub fn vapour_density(t: f64, p: f64) -> f64 {
        p / (Self::R_VAPOUR * t)
    }
    /// Smooth clamp of the quality into [0, 1] over a band of width `BAND`.
    fn soft_quality(x: f64) -> f64 {
        let e = Self::BAND;
        let softplus = |v: f64| if v > 40.0 * e { v } else { e * (1.0 + (v / e).exp()).ln() };
        softplus(x) - softplus(x - 1.0)
    }
    pub fn state(p: f64, h: f64) -> FluidState {
        let ts = Self::saturation_temperature(p);
        let hf = Self::liquid_enthalpy(ts);
        let hg = hf + Self::latent_heat(ts);
        let x = (h - hf) / (hg - hf);
        let quality = Self::soft_quality(x);
        let (temperature, phase) = if x < 0.0 {
            (ts + (h - hf) / Self::CP_LIQUID, Phase::Liquid)
        } else if x > 1.0 {
            (ts + (h - hg) / Self::CP_VAPOUR, Phase::Vapour)
        } else {
            (ts, Phase::TwoPhase)
        };
        let vf = 1.0 / Self::liquid_density(temperature.min(ts), p);
        let vg = 1.0 / Self::vapour_density(temperature.max(ts), p);
        let specific_volume = vf + quality * (vg - vf);
        FluidState { temperature, density: 1.0 / specific_volume, quality: x.clamp(0.0, 1.0), phase }
    }
    /// ∂ρ/∂p and ∂ρ/∂h by central differences on relative steps.
    pub fn density_derivatives(p: f64, h: f64) -> (f64, f64) {
        let dp = 1.0e-5 * p.abs().max(1.0e3);
        let dh = 1.0e-5 * h.abs().max(1.0e3);
        let rho_p = (Self::state(p + dp, h).density - Self::state(p - dp, h).density) / (2.0 * dp);
        let rho_h = (Self::state(p, h + dh).density - Self::state(p, h - dh).density) / (2.0 * dh);
        (rho_p, rho_h)
    }
}

/// A well-mixed volume that carries mass `M` and internal energy `U` —
/// exact integrals of the flows through its node — and *provides* the
/// node's `(pressure, enthalpy)` lanes as algebraic states pinned to them
/// by the equation of state: `ρ(p,h)·V = M`, `V·(ρh − p) = U`. The node's
/// mass and energy balances land on the `p` and `h` rows (the provider
/// leaves them free); its storage enters them as through, `Ṁ` and `U̇`.
/// Keeping the conserved quantities as the states is what lets a step
/// walk through a flash, where ρ(h) drops twentyfold in a few kJ/kg.
pub struct VolumePh {
    pub volume: f64,
    pub initial_pressure: f64,
    pub initial_enthalpy: f64,
}
impl VolumePh {
    fn mass_and_energy(&self, p: f64, h: f64) -> (f64, f64) {
        let rho = Water::state(p, h).density;
        (rho * self.volume, self.volume * (rho * h - p))
    }
}
impl Behavior for VolumePh {
    fn states(&self) -> Vec<StateDeclaration> {
        let (mass, energy) = self.mass_and_energy(self.initial_pressure, self.initial_enthalpy);
        vec![
            StateDeclaration::new("pressure", QuantityKind::Pressure, self.initial_pressure),
            StateDeclaration::new("enthalpy", QuantityKind::SpecificEnthalpy, self.initial_enthalpy),
            StateDeclaration::new("mass", QuantityKind::Mass, mass),
            StateDeclaration::new("energy", QuantityKind::Energy, energy),
        ]
    }
    fn provides(&self) -> Vec<Provision> {
        vec![Provision { port: 0, lane: 0, state: 0 }, Provision { port: 0, lane: 1, state: 1 }]
    }
    fn residual(&self, ctx: &mut Context) {
        let (p, h) = (ctx.state(0), ctx.state(1));
        let (mass, energy) = self.mass_and_energy(p, h);
        // The equation of state pins (p, h) to the stored mass and energy.
        ctx.set_state_residual(2, (mass - ctx.state(2)) / self.volume);
        ctx.set_state_residual(3, (energy - ctx.state(3)) / self.volume);
        // Storage enters the node balances as through.
        ctx.add_through_lane(0, 0, ctx.state_rate(2));
        ctx.add_through_lane(0, 1, ctx.state_rate(3));
    }
    fn energy(&self, view: &View) -> f64 {
        view.state(3)
    }
}

/// An open tank with a free surface: liquid of mass `M` over base `area`,
/// its port at the bottom at `p = p_atm + g·M/area`. Vapour that forms in
/// it separates out through the surface within `separation_time`, and
/// water above the rim (`height`) spills over at `spill_conductance`
/// kg/s per metre. Its `spill` signal is what a geyser throws.
pub struct TankPh {
    pub area: f64,
    pub height: f64,
    pub initial_level: f64,
    pub initial_enthalpy: f64,
    pub separation_time: f64,
    pub spill_conductance: f64,
    pub ambient: f64,
}
impl TankPh {
    fn initial_mass(&self) -> f64 {
        Water::state(self.ambient, self.initial_enthalpy).density * self.area * self.initial_level
    }
    pub fn level(&self, mass: f64, pressure: f64, enthalpy: f64) -> f64 {
        mass / (Water::state(pressure, enthalpy).density * self.area)
    }
    fn spill(&self, level: f64) -> f64 {
        let over = level - self.height;
        let smooth = 0.005;
        self.spill_conductance * smooth * (1.0 + (over / smooth).exp()).ln()
    }
}
impl Behavior for TankPh {
    fn states(&self) -> Vec<StateDeclaration> {
        let mass = self.initial_mass();
        let p = self.ambient + G * mass / self.area;
        let rho = Water::state(p, self.initial_enthalpy).density;
        vec![
            StateDeclaration::new("pressure", QuantityKind::Pressure, p),
            StateDeclaration::new("enthalpy", QuantityKind::SpecificEnthalpy, self.initial_enthalpy),
            StateDeclaration::new("mass", QuantityKind::Mass, mass),
            StateDeclaration::new("energy", QuantityKind::Energy, mass * (self.initial_enthalpy - p / rho)),
        ]
    }
    fn provides(&self) -> Vec<Provision> {
        vec![Provision { port: 0, lane: 0, state: 0 }, Provision { port: 0, lane: 1, state: 1 }]
    }
    fn residual(&self, ctx: &mut Context) {
        let (p, h, mass, energy) = (ctx.state(0), ctx.state(1), ctx.state(2), ctx.state(3));
        let state = Water::state(p, h);
        // The port sits under the tank's own head; enthalpy follows the stored energy.
        ctx.set_state_residual(2, (p - (self.ambient + G * mass / self.area)) / ATMOSPHERE);
        ctx.set_state_residual(3, (mass * (h - p / state.density) - energy) / (1.0 + energy.abs()));
        let level = mass / (state.density * self.area);
        let spill = self.spill(level);
        let ts = Water::saturation_temperature(p);
        let vapour_out = state.quality * mass / self.separation_time;
        let vapour_enthalpy = Water::liquid_enthalpy(ts) + Water::latent_heat(ts);
        // Storage plus what leaves through the surface and over the rim.
        ctx.add_through_lane(0, 0, ctx.state_rate(2) + spill + vapour_out);
        ctx.add_through_lane(0, 1, ctx.state_rate(3) + spill * h + vapour_out * vapour_enthalpy);
        ctx.set_signal(0, spill);
    }
    fn energy(&self, view: &View) -> f64 {
        view.state(3)
    }
}

/// Flow below which upwinding blends the two ends instead of switching:
/// a smooth switch keeps Newton's Jacobian honest through a reversal.
const BLEND_FLOW: f64 = 1.0e-3;

/// Fraction of the `a`-side property carried by a flow from `a` to `b`.
fn upwind_weight(mass_flow: f64) -> f64 {
    0.5 * (1.0 + (mass_flow / BLEND_FLOW).tanh())
}

/// Upwind enthalpy for a flow from `a` to `b`.
fn upwind(ctx: &Context, mass_flow: f64) -> f64 {
    let w = upwind_weight(mass_flow);
    w * ctx.across_lane(0, 1) + (1.0 - w) * ctx.across_lane(1, 1)
}

/// A pipe of `length`, `diameter`, Darcy friction `friction`, rising
/// `rise` from `a` to `b`, with the fluid's inertance and the weight of
/// the fluid it holds (mean of its end densities — a column that flashes
/// gets lighter).
pub struct PipePh {
    pub length: f64,
    pub diameter: f64,
    pub friction: f64,
    pub rise: f64,
    pub initial_flow: f64,
}
impl PipePh {
    fn area(&self) -> f64 {
        std::f64::consts::FRAC_PI_4 * self.diameter * self.diameter
    }
}
impl Behavior for PipePh {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("mass_flow", QuantityKind::MassFlow, self.initial_flow)]
    }
    fn residual(&self, ctx: &mut Context) {
        let m = ctx.state(0);
        let a = self.area();
        let sa = Water::state(ctx.across_lane(0, 0), ctx.across_lane(0, 1));
        let sb = Water::state(ctx.across_lane(1, 0), ctx.across_lane(1, 1));
        let rho_mean = 0.5 * (sa.density + sb.density);
        let w = upwind_weight(m);
        let rho_up = w * sa.density + (1.0 - w) * sb.density;
        let weight = rho_mean * G * self.rise;
        let friction = self.friction * self.length / self.diameter * m * m.abs() / (2.0 * rho_up * a * a);
        let inertance = self.length / a;
        ctx.set_state_residual(0, inertance * ctx.state_rate(0) - (ctx.across_lane(0, 0) - ctx.across_lane(1, 0) - weight - friction));
        let h = upwind(ctx, m);
        ctx.add_through_lane(0, 0, m);
        ctx.add_through_lane(1, 0, -m);
        ctx.add_through_lane(0, 1, m * h);
        ctx.add_through_lane(1, 1, -m * h);
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.length / self.area() * view.state(0).powi(2) / 1000.0
    }
}

/// An orifice: `ṁ = C·√(ρ_up·|Δp|)·sign(Δp)`, smoothed at zero.
pub struct ValvePh {
    pub conductance: f64,
}
impl Behavior for ValvePh {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let dp = ctx.across_lane(0, 0) - ctx.across_lane(1, 0);
        let w = 0.5 * (1.0 + (dp / 10.0).tanh());
        let rho_up = w * Water::state(ctx.across_lane(0, 0), ctx.across_lane(0, 1)).density + (1.0 - w) * Water::state(ctx.across_lane(1, 0), ctx.across_lane(1, 1)).density;
        let m = self.conductance * (rho_up * dp.abs()).sqrt() * dp / (dp.abs() + 1.0);
        let h = upwind(ctx, m);
        ctx.add_through_lane(0, 0, m);
        ctx.add_through_lane(1, 0, -m);
        ctx.add_through_lane(0, 1, m * h);
        ctx.add_through_lane(1, 1, -m * h);
    }
}

/// A pump moving a set `flow` from `a` to `b`, carrying enthalpy with it.
pub struct Pump {
    pub flow: f64,
}
impl Behavior for Pump {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let m = self.flow;
        let h = upwind(ctx, m);
        ctx.add_through_lane(0, 0, m);
        ctx.add_through_lane(1, 0, -m);
        ctx.add_through_lane(0, 1, m * h);
        ctx.add_through_lane(1, 1, -m * h);
    }
}

/// A boundary at fixed pressure and enthalpy (a pool, an aquifer).
pub struct ReservoirPh {
    pub pressure: f64,
    pub enthalpy: f64,
}
impl Behavior for ReservoirPh {
    fn pinned(&self) -> Vec<(usize, usize, f64)> {
        vec![(0, 0, self.pressure), (0, 1, self.enthalpy)]
    }
    fn states(&self) -> Vec<StateDeclaration> {
        vec![
            StateDeclaration::new("mass_flow", QuantityKind::MassFlow, 0.0),
            StateDeclaration::new("enthalpy_flow", QuantityKind::Power, 0.0),
        ]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.across_lane(0, 0) - self.pressure);
        ctx.set_state_residual(1, ctx.across_lane(0, 1) - self.enthalpy);
        ctx.add_through_lane(0, 0, ctx.state(0));
        ctx.add_through_lane(0, 1, ctx.state(1));
    }
}

/// Heat from a wall (thermal port) into the fluid (fluid port):
/// `Q = G·(T_wall − T_fluid)`, entering the fluid as enthalpy.
pub struct WallHeat {
    pub conductance: f64,
}
impl Behavior for WallHeat {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let fluid = Water::state(ctx.across_lane(0, 0), ctx.across_lane(0, 1));
        let wall = ctx.across(1);
        let heat = self.conductance * (wall - fluid.temperature);
        ctx.add_through_lane(0, 1, -heat);
        ctx.add_through(1, heat);
        // The heat leaves at the fluid's temperature.
        ctx.store_entropy(heat / fluid.temperature);
    }
}

fn volume_ph(p: &Params) -> Made {
    Ok(Box::new(VolumePh { volume: param(p, "volume")?, initial_pressure: param_or(p, "initial.pressure", ATMOSPHERE), initial_enthalpy: param_or(p, "initial.enthalpy", Water::liquid_enthalpy(293.15)) }))
}
fn pipe_ph(p: &Params) -> Made {
    Ok(Box::new(PipePh { length: param(p, "length")?, diameter: param(p, "diameter")?, friction: param_or(p, "friction", 0.02), rise: param_or(p, "rise", 0.0), initial_flow: param_or(p, "initial.flow", 0.0) }))
}
fn valve_ph(p: &Params) -> Made {
    Ok(Box::new(ValvePh { conductance: param(p, "conductance")? }))
}
fn pump(p: &Params) -> Made {
    Ok(Box::new(Pump { flow: param(p, "flow")? }))
}
fn reservoir_ph(p: &Params) -> Made {
    Ok(Box::new(ReservoirPh { pressure: param_or(p, "pressure", ATMOSPHERE), enthalpy: param_or(p, "enthalpy", Water::liquid_enthalpy(293.15)) }))
}
fn tank_ph(p: &Params) -> Made {
    Ok(Box::new(TankPh {
        area: param(p, "area")?,
        height: param(p, "height")?,
        initial_level: param_or(p, "initial.level", 0.0),
        initial_enthalpy: param_or(p, "initial.enthalpy", Water::liquid_enthalpy(293.15)),
        separation_time: param_or(p, "separation_time", 1.0),
        spill_conductance: param_or(p, "spill_conductance", 100.0),
        ambient: param_or(p, "ambient", ATMOSPHERE),
    }))
}
fn wall_heat(p: &Params) -> Made {
    Ok(Box::new(WallHeat { conductance: param(p, "conductance")? }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use ConnectorKind::{FluidPh as F, Thermal as H};
    for descriptor in [
        BehaviorDescriptor::new(VOLUME_PH, "Two-phase volume", vec![acausal("node", F)], volume_ph).with_parameters(vec![P::required("volume", "m³").positive(), P::optional("initial.pressure", "Pa", ATMOSPHERE).positive(), P::optional("initial.enthalpy", "J/kg", Water::liquid_enthalpy(293.15))]),
        BehaviorDescriptor::new(PIPE_PH, "Two-phase pipe", vec![acausal("a", F), acausal("b", F)], pipe_ph).with_parameters(vec![P::required("length", "m").positive(), P::required("diameter", "m").positive(), P::optional("friction", "1", 0.02).nonnegative(), P::optional("rise", "m", 0.0), P::optional("initial.flow", "kg/s", 0.0)]),
        BehaviorDescriptor::new(VALVE_PH, "Two-phase orifice", vec![acausal("a", F), acausal("b", F)], valve_ph).with_parameters(vec![P::required("conductance", "m²").nonnegative()]),
        BehaviorDescriptor::new(PUMP, "Fixed-flow pump", vec![acausal("a", F), acausal("b", F)], pump).with_parameters(vec![P::required("flow", "kg/s")]),
        BehaviorDescriptor::new(RESERVOIR_PH, "Fixed (p, h) boundary", vec![acausal("node", F)], reservoir_ph).with_parameters(vec![P::optional("pressure", "Pa", ATMOSPHERE).positive(), P::optional("enthalpy", "J/kg", Water::liquid_enthalpy(293.15))]),
        BehaviorDescriptor::new(WALL_HEAT, "Wall heat into a fluid", vec![acausal("fluid", F), acausal("wall", H)], wall_heat).with_parameters(vec![P::required("conductance", "W/K").nonnegative()]),
        BehaviorDescriptor::new(TANK_PH, "Open tank with a free surface", vec![acausal("bottom", F), signal_out("spill", QuantityKind::MassFlow)], tank_ph).with_parameters(vec![P::required("area", "m²").positive(), P::required("height", "m").positive(), P::optional("initial.level", "m", 0.0).nonnegative(), P::optional("initial.enthalpy", "J/kg", Water::liquid_enthalpy(293.15)), P::optional("separation_time", "s", 1.0).positive(), P::optional("spill_conductance", "kg/(s·m)", 100.0).nonnegative(), P::optional("ambient", "Pa", ATMOSPHERE).positive()]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturation_curve_and_densities() {
        assert!((Water::saturation_temperature(ATMOSPHERE) - 373.15).abs() < 1.0e-9);
        let t3 = Water::saturation_temperature(3.0 * ATMOSPHERE);
        assert!((t3 - 406.7).abs() < 2.0, "T_sat(3 atm) = {t3}");
        let liquid = Water::state(ATMOSPHERE, Water::liquid_enthalpy(293.15));
        assert_eq!(liquid.phase, Phase::Liquid);
        assert!((liquid.density - 992.0).abs() < 2.0 && (liquid.temperature - 293.15).abs() < 1.0e-6);
        let steam = Water::state(ATMOSPHERE, Water::liquid_enthalpy(373.15) + Water::latent_heat(373.15) + 1.0e5);
        assert_eq!(steam.phase, Phase::Vapour);
        assert!((steam.density - 0.52).abs() < 0.05, "steam density {}", steam.density);
        let mixed = Water::state(ATMOSPHERE, Water::liquid_enthalpy(373.15) + 0.5 * Water::latent_heat(373.15));
        assert_eq!(mixed.phase, Phase::TwoPhase);
        assert!((mixed.quality - 0.5).abs() < 1.0e-6 && mixed.density < 2.0);
    }
}
