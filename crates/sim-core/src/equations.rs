//! How a behavior contributes equations to the island it is compiled into.
//!
//! Every acausal connector is a bundle of *across* variables (shared by all
//! ports on a node) and *through* variables (summed to zero over a node).
//! A behavior owns continuous states and, at each residual evaluation, sees
//! its states and rates, the across values and rates of its ports, and its
//! signal inputs; it writes one residual per state, adds its through
//! contributions to each port, and sets its signal outputs. The compiler
//! turns that into one [`System`](../../sim_dynamics/trait.System.html) per
//! island with a balance row per node through variable.
//!
//! Frame connectors are *owned*: a body publishes its pose and twist as the
//! across bundle, attachments read them and push wrenches back, and no node
//! unknowns exist for the frame at all.

use crate::{ConnectorKind, QuantityKind};
use std::collections::BTreeMap;

/// One across/through pair of a connector bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lane {
    pub across: &'static str,
    pub through: &'static str,
    pub across_kind: QuantityKind,
    pub through_kind: QuantityKind,
    /// This lane is the exact time derivative of another lane of the same
    /// connector. The compiler either aliases it to a providing element's
    /// state (an inertia's speed) or adds the identity row itself.
    pub derivative_of: Option<usize>,
}

const fn lane(across: &'static str, through: &'static str, ak: QuantityKind, tk: QuantityKind) -> Lane {
    Lane { across, through, across_kind: ak, through_kind: tk, derivative_of: None }
}

const fn rate_lane(across: &'static str, ak: QuantityKind, of: usize) -> Lane {
    Lane { across, through: "-", across_kind: ak, through_kind: QuantityKind::Dimensionless, derivative_of: Some(of) }
}

static ELECTRICAL: [Lane; 1] = [lane("voltage", "current", QuantityKind::Voltage, QuantityKind::Current)];
static ROTATIONAL: [Lane; 2] = [
    lane("angle", "torque", QuantityKind::Angle, QuantityKind::Torque),
    rate_lane("speed", QuantityKind::AngularVelocity, 0),
];
static TRANSLATIONAL: [Lane; 2] = [
    lane("position", "force", QuantityKind::Length, QuantityKind::Force),
    rate_lane("velocity", QuantityKind::LinearVelocity, 0),
];
static THERMAL: [Lane; 1] = [lane("temperature", "heat_flow", QuantityKind::Temperature, QuantityKind::HeatFlow)];
static HYDRAULIC: [Lane; 1] = [lane("pressure", "volume_flow", QuantityKind::Pressure, QuantityKind::VolumeFlow)];
static NORMALIZED_ACOUSTIC: [Lane; 1] = [lane("pressure", "volume_flow", QuantityKind::Dimensionless, QuantityKind::Dimensionless)];
static PLANAR: [Lane; 2] = [
    lane("x", "fx", QuantityKind::Length, QuantityKind::Force),
    lane("y", "fy", QuantityKind::Length, QuantityKind::Force),
];
static FLUID_PH: [Lane; 2] = [
    lane("pressure", "mass_flow", QuantityKind::Pressure, QuantityKind::MassFlow),
    lane("enthalpy", "enthalpy_flow", QuantityKind::SpecificEnthalpy, QuantityKind::Power),
];
static CHEMICAL: [Lane; 1] = [lane("chemical_potential", "molar_flow", QuantityKind::ChemicalPotential, QuantityKind::MolarFlow)];
static RADIATIVE: [Lane; 1] = [lane("radiosity", "radiant_flux", QuantityKind::Radiosity, QuantityKind::Power)];
static GRANULAR: [Lane; 1] = [lane("stress", "particle_flow", QuantityKind::Pressure, QuantityKind::MassFlow)];
static MAGNETIC: [Lane; 1] = [lane("mmf", "flux_rate", QuantityKind::Current, QuantityKind::Voltage)];
static PLANAR_FRAME: [Lane; 6] = [
    lane("x", "fx", QuantityKind::Length, QuantityKind::Force),
    lane("y", "fy", QuantityKind::Length, QuantityKind::Force),
    lane("theta", "torque", QuantityKind::Angle, QuantityKind::Torque),
    rate_lane("vx", QuantityKind::LinearVelocity, 0),
    rate_lane("vy", QuantityKind::LinearVelocity, 1),
    rate_lane("omega", QuantityKind::AngularVelocity, 2),
];
/// Pose (position, unit quaternion w x y z), then twist (v, ω); the wrench
/// occupies the first six through lanes.
static FRAME: [Lane; 13] = [
    lane("x", "fx", QuantityKind::Length, QuantityKind::Force),
    lane("y", "fy", QuantityKind::Length, QuantityKind::Force),
    lane("z", "fz", QuantityKind::Length, QuantityKind::Force),
    lane("qw", "tx", QuantityKind::Dimensionless, QuantityKind::Torque),
    lane("qx", "ty", QuantityKind::Dimensionless, QuantityKind::Torque),
    lane("qy", "tz", QuantityKind::Dimensionless, QuantityKind::Torque),
    lane("qz", "-", QuantityKind::Dimensionless, QuantityKind::Dimensionless),
    lane("vx", "-", QuantityKind::LinearVelocity, QuantityKind::Dimensionless),
    lane("vy", "-", QuantityKind::LinearVelocity, QuantityKind::Dimensionless),
    lane("vz", "-", QuantityKind::LinearVelocity, QuantityKind::Dimensionless),
    lane("wx", "-", QuantityKind::AngularVelocity, QuantityKind::Dimensionless),
    lane("wy", "-", QuantityKind::AngularVelocity, QuantityKind::Dimensionless),
    lane("wz", "-", QuantityKind::AngularVelocity, QuantityKind::Dimensionless),
];

impl ConnectorKind {
    /// The lanes of this connector. Scalar connectors have one; planar
    /// frames two; rigid-body frames carry the full pose and twist across
    /// and a wrench through.
    pub fn lanes(self) -> Vec<Lane> {
        let plain: &'static [Lane] = match self {
            Self::Electrical => &ELECTRICAL,
            Self::Rotational => &ROTATIONAL,
            Self::Translational => &TRANSLATIONAL,
            Self::Thermal => &THERMAL,
            Self::Hydraulic | Self::Acoustic => &HYDRAULIC,
            Self::NormalizedAcoustic => &NORMALIZED_ACOUSTIC,
            Self::Magnetic => &MAGNETIC,
            Self::FluidPh => &FLUID_PH,
            Self::Chemical => &CHEMICAL,
            Self::Radiative => &RADIATIVE,
            Self::Granular => &GRANULAR,
            Self::Planar => &PLANAR,
            Self::PlanarFrame => &PLANAR_FRAME,
            Self::Frame => &FRAME,
            Self::Composite(members) => {
                // Member after member, derivative links shifted with them.
                let mut lanes = Vec::new();
                for member in members {
                    let offset = lanes.len();
                    lanes.extend(member.lanes().into_iter().map(|lane| Lane { derivative_of: lane.derivative_of.map(|b| b + offset), ..lane }));
                }
                return lanes;
            }
        };
        plain.to_vec()
    }

    /// Flat lane offset of member `member` inside this connector's bundle;
    /// a behavior addresses `(port, member, lane)` as `member_offset + lane`.
    pub fn member_offset(self, member: usize) -> usize {
        self.members().iter().take(member).map(|m| m.across_width()).sum()
    }

    /// Owned connectors have exactly one owner port per node, whose states
    /// *are* the across bundle; the node carries no unknowns.
    pub const fn is_owned(self) -> bool {
        matches!(self, Self::Frame | Self::PlanarFrame)
    }

    /// For owned connectors: the owner's state offset at which attachments'
    /// through contributions land (its twist rows), `across − through` lanes.
    pub fn owned_wrench_offset(self) -> usize {
        self.across_width() - self.through_width()
    }

    /// Number of through variables that get a balance row.
    pub fn through_width(self) -> usize {
        self.lanes().iter().filter(|l| l.through != "-").count()
    }

    pub fn across_width(self) -> usize {
        self.lanes().len()
    }
}

#[derive(Debug, Clone)]
pub struct StateDeclaration {
    pub name: String,
    pub kind: QuantityKind,
    pub initial: f64,
}

impl StateDeclaration {
    pub fn new(name: impl Into<String>, kind: QuantityKind, initial: f64) -> Self {
        Self { name: name.into(), kind, initial }
    }
}

/// What a behavior reads and writes during one residual evaluation. The
/// compiler backs this with slices into the island's unknown vector.
pub struct Context<'a> {
    pub time: f64,
    /// Set while the compiler probes which rates a behavior reads: index 0..n
    /// are this behavior's states, then each port's lanes in order.
    pub(crate) rate_reads: Option<&'a std::cell::RefCell<Vec<bool>>>,
    pub(crate) states: &'a [f64],
    pub(crate) state_rates: &'a [f64],
    /// Port lanes laid out flat: port `p` occupies `offsets[p]..offsets[p+1]`.
    pub(crate) offsets: &'a [usize],
    /// Flat lane → flat index of the lane carrying its exact rate, if any.
    pub(crate) rate_map: &'a [Option<usize>],
    pub(crate) across: &'a [f64],
    pub(crate) across_rates: &'a [f64],
    pub(crate) signals_in: &'a [f64],
    pub(crate) state_residuals: &'a mut [f64],
    /// Through contributions, same flat layout as `across`.
    pub(crate) through: &'a mut [f64],
    /// Standard-normal draws for this step, one per `add_noise` call in order.
    pub(crate) noise: &'a [f64],
    pub(crate) noise_cursor: usize,
    /// The integrator's step, which sets the noise's per-step scale.
    pub(crate) step: f64,
    pub(crate) signals_out: &'a mut [f64],
    /// Reversible entropy storage rate declared by the behavior (W/K).
    pub(crate) entropy_storage: f64,
}

impl<'a> Context<'a> {
    /// `offsets` has one entry per port plus a final end marker.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        time: f64,
        states: &'a [f64],
        state_rates: &'a [f64],
        offsets: &'a [usize],
        rate_map: &'a [Option<usize>],
        across: &'a [f64],
        across_rates: &'a [f64],
        signals_in: &'a [f64],
        state_residuals: &'a mut [f64],
        through: &'a mut [f64],
        signals_out: &'a mut [f64],
    ) -> Self {
        Self { time, rate_reads: None, states, state_rates, offsets, rate_map, across, across_rates, signals_in, state_residuals, through, signals_out, entropy_storage: 0.0, noise: &[], noise_cursor: 0, step: 1.0 }
    }

    /// Declare the rate at which this behavior *reversibly* stores entropy
    /// (a capacitance stores `C·Ṫ/T`; a reservoir stores what it absorbs).
    /// The compiler subtracts it from the entropy carried in by heat at the
    /// behavior's thermal ports; what remains is production, which must
    /// not be negative.
    /// Give this evaluation its noise draws for the current step.
    pub fn with_noise(mut self, draws: &'a [f64], step: f64) -> Self {
        self.noise = draws;
        self.step = step;
        self
    }
    /// Add white noise of `intensity` (the strength of its delta correlation,
    /// e.g. `2γkT` for a Langevin force) as a through contribution: over a
    /// step `h` the increment is Gaussian with variance `intensity·h`, so the
    /// equivalent constant through is `√(intensity/h)·ξ`. The draw is held
    /// fixed for the step (drift implicit, diffusion explicit); with no
    /// integrator step in progress it is zero.
    pub fn add_noise(&mut self, port: usize, intensity: f64) {
        let xi = self.noise.get(self.noise_cursor).copied().unwrap_or(0.0);
        self.noise_cursor += 1;
        let value = (intensity / self.step).sqrt() * xi;
        self.add_through(port, value);
    }
    pub fn store_entropy(&mut self, rate: f64) {
        self.entropy_storage += rate;
    }

    pub fn entropy_storage(&self) -> f64 {
        self.entropy_storage
    }

    /// Record rate reads into `reads`: this behavior's states first, then
    /// its port lanes in flat order.
    pub fn with_rate_tracking(mut self, reads: &'a std::cell::RefCell<Vec<bool>>) -> Self {
        self.rate_reads = Some(reads);
        self
    }

    fn note_state_rate(&self, index: usize) {
        if let Some(reads) = self.rate_reads {
            reads.borrow_mut()[index] = true;
        }
    }
    fn note_across_rate(&self, port: usize, lane: usize) {
        if let Some(reads) = self.rate_reads {
            reads.borrow_mut()[self.states.len() + self.offsets[port] + lane] = true;
        }
    }

    pub fn state(&self, index: usize) -> f64 {
        self.states[index]
    }
    pub fn state_rate(&self, index: usize) -> f64 {
        self.note_state_rate(index);
        self.state_rates[index]
    }
    pub fn states(&self) -> &[f64] {
        self.states
    }
    pub fn state_rates(&self) -> &[f64] {
        for i in 0..self.state_rates.len() {
            self.note_state_rate(i);
        }
        self.state_rates
    }
    /// Across value of `port`, lane 0.
    pub fn across(&self, port: usize) -> f64 {
        self.across[self.offsets[port]]
    }
    /// Rate of `port`'s lane 0: the connector's exact rate lane when it has
    /// one, else the step's finite-difference rate.
    pub fn across_rate(&self, port: usize) -> f64 {
        self.across_rate_lane(port, 0)
    }
    pub fn across_lane(&self, port: usize, lane: usize) -> f64 {
        self.across[self.offsets[port] + lane]
    }
    pub fn across_rate_lane(&self, port: usize, lane: usize) -> f64 {
        let flat = self.offsets[port] + lane;
        match self.rate_map.get(flat).copied().flatten() {
            Some(exact) => self.across[exact],
            None => {
                self.note_across_rate(port, lane);
                self.across_rates[flat]
            }
        }
    }
    /// The step's finite-difference rate of a lane, regardless of any exact
    /// rate lane: what an element *providing* the rate lane must use for its
    /// own identity row.
    pub fn across_derivative(&self, port: usize, lane: usize) -> f64 {
        self.note_across_rate(port, lane);
        self.across_rates[self.offsets[port] + lane]
    }
    pub fn across_bundle(&self, port: usize) -> &[f64] {
        &self.across[self.offsets[port]..self.offsets[port + 1]]
    }
    pub fn signal_in(&self, index: usize) -> f64 {
        self.signals_in[index]
    }
    /// Residual for state `index`: zero when the state equation holds.
    pub fn set_state_residual(&mut self, index: usize, value: f64) {
        self.state_residuals[index] = value;
    }
    /// Add a through contribution at `port`, lane 0, positive *into* this
    /// behavior (a node sums these to zero).
    pub fn add_through(&mut self, port: usize, value: f64) {
        self.through[self.offsets[port]] += value;
    }
    pub fn add_through_lane(&mut self, port: usize, lane: usize, value: f64) {
        self.through[self.offsets[port] + lane] += value;
    }
    pub fn set_signal(&mut self, index: usize, value: f64) {
        self.signals_out[index] = value;
    }
}

/// Read-only view used by guards and energy accounting.
pub struct View<'a> {
    pub time: f64,
    pub states: &'a [f64],
    pub offsets: &'a [usize],
    pub rate_map: &'a [Option<usize>],
    pub across: &'a [f64],
    pub across_rates: &'a [f64],
    pub signals_in: &'a [f64],
}

impl View<'_> {
    pub fn state(&self, index: usize) -> f64 {
        self.states[index]
    }
    pub fn across(&self, port: usize) -> f64 {
        self.across[self.offsets[port]]
    }
    pub fn across_lane(&self, port: usize, lane: usize) -> f64 {
        self.across[self.offsets[port] + lane]
    }
    pub fn across_bundle(&self, port: usize) -> &[f64] {
        &self.across[self.offsets[port]..self.offsets[port + 1]]
    }
    pub fn across_rate(&self, port: usize) -> f64 {
        let flat = self.offsets[port];
        match self.rate_map.get(flat).copied().flatten() {
            Some(exact) => self.across[exact],
            None => self.across_rates[flat],
        }
    }
    pub fn signal_in(&self, index: usize) -> f64 {
        self.signals_in[index]
    }
}

/// One alternative start proposed by a nonsmooth element: its own states
/// and, where the branch is only reachable with a different velocity or
/// potential at its ports, across-lane overrides `(port, lane, value)`.
#[derive(Debug, Clone, Default)]
pub struct Branch {
    pub states: Vec<f64>,
    pub across: Vec<(usize, usize, f64)>,
}

/// A behavior's own state standing in for a connector lane on its node
/// (an inertia's speed *is* the shaft's speed lane).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provision {
    pub port: usize,
    pub lane: usize,
    pub state: usize,
}

/// An input a behavior's residual depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    State(usize),
    StateRate(usize),
    Across(usize, usize),
    /// The lane's rate as `Context::across_rate` reads it: the exact rate
    /// lane when one is provided, else the step's finite difference.
    AcrossRate(usize, usize),
    /// The step's finite-difference rate of the lane, as
    /// `Context::across_derivative` reads it, regardless of any exact lane.
    AcrossDerivative(usize, usize),
    Signal(usize),
}

/// An output a behavior writes: a state residual row, a through
/// contribution at a port lane, or a signal output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    State(usize),
    Through(usize, usize),
    Signal(usize),
}

/// Partial derivatives a behavior reports about itself: `∂output/∂input`
/// entries, accumulated. The compiler scatters them into the island's
/// Jacobian in place of finite differences.
#[derive(Debug, Default, Clone)]
pub struct LocalJacobian {
    pub entries: Vec<(Output, Input, f64)>,
}

impl LocalJacobian {
    pub fn set(&mut self, output: Output, input: Input, value: f64) {
        self.entries.push((output, input, value));
    }
    /// `∂(state residual i)/∂(state j)`.
    pub fn state_state(&mut self, i: usize, j: usize, value: f64) {
        self.set(Output::State(i), Input::State(j), value);
    }
    /// `∂(state residual i)/∂(state rate j)`.
    pub fn state_rate(&mut self, i: usize, j: usize, value: f64) {
        self.set(Output::State(i), Input::StateRate(j), value);
    }
    /// `∂(through at port, lane 0)/∂input`.
    pub fn through(&mut self, port: usize, input: Input, value: f64) {
        self.set(Output::Through(port, 0), input, value);
    }
}

/// The equations of one behavior instance. Ports are indexed in the order
/// the descriptor declares them; signal inputs and outputs are indexed in
/// declaration order among their own kind.
pub trait Behavior: Send + Sync {
    fn states(&self) -> Vec<StateDeclaration>;

    /// The acausal port whose frame this behavior owns. Its first states are
    /// the frame's pose/twist bundle; attachments consume that bundle and add
    /// wrenches. Each frame connection must have exactly one declared owner.
    fn owned_frame(&self) -> Option<usize> {
        None
    }

    /// Lanes this behavior's states provide on its nodes. At most one
    /// provider per node lane; a rate lane without a provider gets the
    /// compiler's finite-difference identity row instead.
    fn provides(&self) -> Vec<Provision> {
        Vec::new()
    }

    fn residual(&self, ctx: &mut Context);

    /// Guard functions; an event fires when one crosses from positive to
    /// non-positive.
    fn guards(&self, _view: &View, _out: &mut Vec<f64>) {}

    /// Reset owned states after guard `index` fired. Only this behavior's
    /// states may change.
    fn jump(&mut self, _index: usize, _view: &View, _states: &mut [f64]) {}
    /// Alternative starting points when an implicit step fails to converge
    /// from the smooth predictor. A nonsmooth element uses this to
    /// enumerate its branches — a contact proposes "stick" when the sliding
    /// branch has no solution (Painlevé's paradox) — which is what
    /// time-stepping schemes for nonsmooth mechanics do by hand.
    fn branches(&self, _view: &View, _out: &mut Vec<Branch>) {}
    /// Across lanes this element pins to fixed values: `(port, lane,
    /// value)`. A ground pins its node to zero, a reservoir to its pressure,
    /// an ambient to its temperature. The compiler starts those nodes there,
    /// so the consistent initialisation has nothing to invent.
    fn pinned(&self) -> Vec<(usize, usize, f64)> {
        Vec::new()
    }

    /// Energy stored in this behavior (for conservation diagnostics).
    fn energy(&self, _view: &View) -> f64 {
        0.0
    }

    /// The derivatives of this behavior's outputs with respect to its
    /// inputs at `view`, when it knows them. Return `false` to be
    /// differentiated numerically (the default). An element that answers
    /// must report every nonzero partial.
    fn jacobian(&self, _view: &View, _out: &mut LocalJacobian) -> bool {
        false
    }

    /// Hand an external control element its [`Coupler`] and the contract
    /// the runtime derived from its wiring. Elements that are not seams
    /// return the coupler untouched.
    fn couple(&mut self, coupler: Box<dyn crate::Coupler>, _contract: crate::Contract) -> Result<(), Box<dyn crate::Coupler>> {
        Err(coupler)
    }

    /// A fault that ends the run — what a seam reports when its controller
    /// has gone, timed out, or answered nonsense. The runtime turns it into
    /// an error naming the element after the step in which it appeared.
    fn failure(&self) -> Option<String> {
        None
    }
}

/// Builds a behavior's equations from its instance parameters.
pub type Equations = fn(&BTreeMap<String, f64>) -> Result<Box<dyn Behavior>, EquationError>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EquationError {
    #[error("missing parameter `{0}`")]
    MissingParameter(String),
    #[error("invalid parameter `{0}`: {1}")]
    InvalidParameter(String, String),
}

/// Parameter lookup with a clear error, for use inside [`Equations`].
pub fn param(parameters: &BTreeMap<String, f64>, name: &str) -> Result<f64, EquationError> {
    parameters.get(name).copied().ok_or_else(|| EquationError::MissingParameter(name.to_owned()))
}

pub fn param_or(parameters: &BTreeMap<String, f64>, name: &str, default: f64) -> f64 {
    parameters.get(name).copied().unwrap_or(default)
}
