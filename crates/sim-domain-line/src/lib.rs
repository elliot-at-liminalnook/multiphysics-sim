//! Distributed 1-D fields as single elements, discretised at compile time
//! into `cells` internal nodes, with ports at *positions* along them: a
//! `tap.<name> = position` parameter adds a port there. Anything can be
//! attached mid-line.

use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or,
};
use std::collections::BTreeMap;

pub const STRING: &str = "line.string";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// A taut string of `length`, `tension` and `mass` per length with viscous
/// `damping` per length, fixed at both ends, carrying `cells` internal
/// nodes of transverse displacement. Ports `left` and `right` are the ends
/// (translational: displacement | force); `tap.<name>` ports sit at
/// fractional positions and hold their node to the string there through a
/// reaction force, which the string feels spread over the two cells around
/// the tap.
pub struct TautString {
    pub length: f64,
    pub tension: f64,
    pub mass_per_length: f64,
    pub damping_per_length: f64,
    pub cells: usize,
    /// Tap positions in [0, 1], in port order (sorted by name).
    pub taps: Vec<f64>,
    pub initial_shape: f64,
}
impl TautString {
    fn dx(&self) -> f64 {
        self.length / (self.cells as f64 + 1.0)
    }
    /// The two cells a position falls between and the weight of the upper one.
    fn locate(&self, s: f64) -> (usize, f64) {
        let x = s.clamp(0.0, 1.0) * (self.cells as f64 + 1.0);
        let i = x.floor().min(self.cells as f64) as usize;
        (i, x - i as f64)
    }
    /// Displacement at a fractional position, given the node displacements
    /// with the fixed ends included.
    pub fn displacement_at(&self, s: f64, y: &[f64]) -> f64 {
        let (i, w) = self.locate(s);
        (1.0 - w) * y[i] + w * y[i + 1]
    }
    pub fn natural_frequency(&self, mode: usize) -> f64 {
        mode as f64 / (2.0 * self.length) * (self.tension / self.mass_per_length).sqrt()
    }
}
impl Behavior for TautString {
    fn states(&self) -> Vec<StateDeclaration> {
        let mut states = Vec::new();
        for i in 0..self.cells {
            let x = (i as f64 + 1.0) / (self.cells as f64 + 1.0);
            states.push(StateDeclaration::new(format!("y{i}"), QuantityKind::Length, self.initial_shape * (std::f64::consts::PI * x).sin()));
        }
        for i in 0..self.cells {
            states.push(StateDeclaration::new(format!("v{i}"), QuantityKind::LinearVelocity, 0.0));
        }
        for k in 0..self.taps.len() {
            states.push(StateDeclaration::new(format!("reaction{k}"), QuantityKind::Force, 0.0));
        }
        states
    }
    fn residual(&self, ctx: &mut Context) {
        let n = self.cells;
        let dx = self.dx();
        // Displacements with the ends' nodes included.
        let mut y = Vec::with_capacity(n + 2);
        y.push(ctx.across(0));
        y.extend((0..n).map(|i| ctx.state(i)));
        y.push(ctx.across(1));
        let mut force = vec![0.0; n + 2];
        for (k, s) in self.taps.iter().enumerate() {
            let reaction = ctx.state(2 * n + k);
            let (i, w) = self.locate(*s);
            force[i] += (1.0 - w) * reaction;
            force[i + 1] += w * reaction;
            // The tap's node is held to the string; `reaction` is the force
            // the node puts into the string (through into this element).
            ctx.set_state_residual(2 * n + k, ctx.across(2 + k) - self.displacement_at(*s, &y));
            ctx.add_through(2 + k, reaction);
        }
        for i in 0..n {
            let v = ctx.state(n + i);
            ctx.set_state_residual(i, ctx.state_rate(i) - v);
            let curvature = (y[i + 2] - 2.0 * y[i + 1] + y[i]) / dx;
            let inertial = self.mass_per_length * dx * ctx.state_rate(n + i);
            ctx.set_state_residual(n + i, inertial - (self.tension * curvature - self.damping_per_length * dx * v + force[i + 1]));
        }
        // Ends: the string pulls on whatever holds them.
        ctx.add_through(0, -(self.tension * (y[1] - y[0]) / dx + force[0]));
        ctx.add_through(1, -(self.tension * (y[n] - y[n + 1]) / dx + force[n + 1]));
    }
    fn energy(&self, view: &View) -> f64 {
        let n = self.cells;
        let dx = self.dx();
        let mut y = vec![view.across(0)];
        y.extend((0..n).map(|i| view.state(i)));
        y.push(view.across(1));
        let kinetic: f64 = (0..n).map(|i| 0.5 * self.mass_per_length * dx * view.state(n + i).powi(2)).sum();
        let potential: f64 = y.windows(2).map(|w| 0.5 * self.tension * (w[1] - w[0]).powi(2) / dx).sum();
        kinetic + potential
    }
}

fn taut_string(p: &Params) -> Made {
    let taps: Vec<f64> = p.iter().filter(|(k, _)| k.starts_with("tap.")).map(|(_, v)| *v).collect();
    Ok(Box::new(TautString {
        length: param(p, "length")?,
        tension: param(p, "tension")?,
        mass_per_length: param(p, "mass_per_length")?,
        damping_per_length: param_or(p, "damping_per_length", 0.0),
        cells: param_or(p, "cells", 16.0) as usize,
        taps,
        initial_shape: param_or(p, "initial.shape", 0.0),
    }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use ConnectorKind::Translational as T;
    registry.register(BehaviorDescriptor::new(STRING, "Taut string with taps", vec![acausal("left", T), acausal("right", T), acausal("tap.*", T)], taut_string).with_parameters(vec![
        P::required("length", "m").positive(), P::required("tension", "N").positive(),
        P::required("mass_per_length", "kg/m").positive(), P::optional("damping_per_length", "N·s/m²", 0.0).nonnegative(),
        P::optional("cells", "1", 16.0).integer(1.0, 4096.0),
        P::alternative("tap.*", "1").nonnegative().at_most(1.0), P::optional("initial.shape", "m", 0.0),
    ]))
}
