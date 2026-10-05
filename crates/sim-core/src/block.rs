//! Blocks: executable implementations (an imported FMU, a host-supplied
//! controller) joined to the coupled model through signals and executed by
//! the runtime's scheduler at committed clock ticks, never inside residuals
//! (docs/architecture/composition.md, "Time semantics").
//!
//! The model records a block as [`BlockDecl`]: its typed inputs and outputs,
//! timing and an [`ImplementationRef`] naming what runs it. The coupled model
//! sees it as a shadow element of type [`BLOCK`] whose signal inputs only read
//! and whose signal outputs equal held states (zero rate). The runtime binds a
//! [`BlockImplementation`] to each block and writes the held states at ticks.
use crate::{BehaviorId, QuantityKind};
use serde::{Deserialize, Serialize};

/// The shadow element type every block compiles to.
pub const BLOCK: &str = "block";

/// When a block ticks, relative to the run's start `t0`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Clock {
    /// Ticks at `t0 + offset + k·period` (on the absolute grid, never by
    /// repeated addition).
    Periodic {
        /// Seconds between ticks.
        period: f64,
        /// Seconds from the run's start to the first tick.
        #[serde(default)]
        offset: f64,
    },
    /// Ticks at `t0 + times[k]`: a recorded acquisition schedule. No tick
    /// after the last.
    Times { times: Vec<f64> },
}

impl Clock {
    pub fn periodic(period: f64) -> Self {
        Self::Periodic { period, offset: 0.0 }
    }
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Periodic { period, offset } => {
                if !(period.is_finite() && *period > 0.0) {
                    return Err(format!("clock period must be finite and positive, not {period}"));
                }
                if !(offset.is_finite() && *offset >= 0.0) {
                    return Err(format!("clock offset must be finite and non-negative, not {offset}"));
                }
            }
            Self::Times { times } => {
                if times.is_empty() {
                    return Err("a clock schedule needs at least one time".into());
                }
                if let Some(t) = times.iter().find(|t| !(t.is_finite() && **t >= 0.0)) {
                    return Err(format!("clock schedule times must be finite and non-negative, not {t}"));
                }
                if let Some(w) = times.windows(2).find(|w| w[0] >= w[1]) {
                    return Err(format!("clock schedule times must increase strictly ({} then {})", w[0], w[1]));
                }
            }
        }
        Ok(())
    }
    /// Tick `k` after a run that started at `t0` (None past a schedule's end).
    pub fn tick(&self, t0: f64, k: u64) -> Option<f64> {
        match self {
            Self::Periodic { period, offset } => Some(t0 + offset + k as f64 * period),
            Self::Times { times } => times.get(k as usize).map(|t| t0 + t),
        }
    }
    /// Seconds from the run's start to the first tick.
    pub fn first(&self) -> f64 {
        match self {
            Self::Periodic { offset, .. } => *offset,
            Self::Times { times } => times.first().copied().unwrap_or(0.0),
        }
    }
    /// Seconds from tick `k` to tick `k + 1` (None after a schedule's last).
    pub fn interval(&self, k: u64) -> Option<f64> {
        match self {
            Self::Periodic { period, .. } => Some(*period),
            Self::Times { times } => {
                let k = k as usize;
                (k + 1 < times.len()).then(|| times[k + 1] - times[k])
            }
        }
    }
    /// The nominal period a controller is told (a schedule's shortest interval).
    pub fn nominal_period(&self) -> f64 {
        match self {
            Self::Periodic { period, .. } => *period,
            Self::Times { times } => times.windows(2).map(|w| w[1] - w[0]).fold(f64::INFINITY, f64::min).min(1.0).max(f64::MIN_POSITIVE),
        }
    }
}

/// One typed signal of a block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockPort {
    pub name: String,
    pub kind: QuantityKind,
    /// The value before the block's first write (outputs), or the value an
    /// unconnected input reads (inputs never are unconnected: refused).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<f64>,
    /// Declared range: an output outside it is a block fault.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

impl BlockPort {
    pub fn new(name: impl Into<String>, kind: QuantityKind) -> Self {
        Self { name: name.into(), kind, start: None, min: None, max: None }
    }
    pub fn start(mut self, value: f64) -> Self {
        self.start = Some(value);
        self
    }
    pub fn range(mut self, min: Option<f64>, max: Option<f64>) -> Self {
        self.min = min;
        self.max = max;
        self
    }
}

/// What an implementation offers: its signals and whether an output at a
/// tick depends on the inputs of the same tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockInterface {
    pub inputs: Vec<BlockPort>,
    pub outputs: Vec<BlockPort>,
    /// `true`: outputs computed at a tick apply at that tick (a zero-time
    /// task). `false`: outputs computed at a tick are the implementation's
    /// values at its next tick (FMI Co-Simulation's communication step).
    pub feedthrough: bool,
}

impl BlockInterface {
    /// Ports from `(name, kind)` pairs, without start values or ranges.
    pub fn signals<'a>(inputs: impl IntoIterator<Item = (&'a str, QuantityKind)>, outputs: impl IntoIterator<Item = (&'a str, QuantityKind)>, feedthrough: bool) -> Self {
        Self {
            inputs: inputs.into_iter().map(|(n, k)| BlockPort::new(n, k)).collect(),
            outputs: outputs.into_iter().map(|(n, k)| BlockPort::new(n, k)).collect(),
            feedthrough,
        }
    }
    /// Every output starts at `value` before the first write.
    pub fn starting_at(mut self, value: f64) -> Self {
        for p in &mut self.outputs {
            p.start = Some(value);
        }
        self
    }
}

/// When and how a block runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockTiming {
    pub clock: Clock,
    /// Whole samples between a sampled input and what the implementation sees.
    #[serde(default)]
    pub input_delay: usize,
    /// Whole samples between an implementation output and the block's signal.
    #[serde(default)]
    pub output_delay: usize,
    /// Wall-clock budget per call; a longer call is a block fault.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_s: Option<f64>,
}

impl BlockTiming {
    pub fn periodic(period: f64) -> Self {
        Self { clock: Clock::periodic(period), input_delay: 0, output_delay: 0, deadline_s: None }
    }
    pub fn with_clock(clock: Clock) -> Self {
        Self { clock, input_delay: 0, output_delay: 0, deadline_s: None }
    }
    pub fn validate(&self) -> Result<(), String> {
        self.clock.validate()?;
        if self.input_delay > 4096 || self.output_delay > 4096 {
            return Err("input_delay and output_delay are at most 4096 samples".into());
        }
        if let Some(d) = self.deadline_s {
            if !(d.is_finite() && d > 0.0) {
                return Err(format!("deadline_s must be finite and positive, not {d}"));
            }
        }
        Ok(())
    }
}

/// What runs a block, as the model records it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ImplementationRef {
    /// An FMI 3 Co-Simulation FMU: the archive's path, the SHA-256 of its
    /// bytes (a run binds to the exact artifact) and the values of the FMU
    /// parameters it sets before initialisation (by variable name, SI).
    Fmi3 {
        path: String,
        sha256: String,
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        parameters: std::collections::BTreeMap<String, f64>,
    },
    /// Supplied by the host at run time (a teleoperation policy, a learning
    /// agent, a scripted controller): bound with `Runtime::bind_block`.
    Host { name: String },
}

impl ImplementationRef {
    pub fn describe(&self) -> String {
        match self {
            Self::Fmi3 { path, .. } => format!("FMI 3 co-simulation FMU {path}"),
            Self::Host { name } => format!("host implementation `{name}`"),
        }
    }
}

/// A block in a model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockDecl {
    /// The block's identity (an instance path such as `arm/controller`).
    pub name: String,
    /// Its shadow element in the coupled model.
    pub behavior: BehaviorId,
    pub interface: BlockInterface,
    pub timing: BlockTiming,
    pub implementation: ImplementationRef,
}

/// What `BlockImplementation::checkpoint` can give.
pub enum Checkpoint {
    /// Nothing outside the runtime's own records: a restore is exact.
    Stateless,
    /// Opaque state the implementation restores itself.
    State(Vec<u8>),
    /// The implementation's state cannot be captured; checkpoints refuse.
    Unsupported(String),
}

/// The executable side of a block (owned by the runtime).
pub trait BlockImplementation: Send {
    /// How errors name it (`FMI 3 FMU thermostat.fmu`, `python controller …`).
    fn label(&self) -> String;
    /// The interface it implements; the runtime checks it against the
    /// block's declaration before the first tick.
    fn interface(&self) -> BlockInterface;
    /// The first tick at time `t` (`period` the clock's): initialise with
    /// `inputs`, write the initial `outputs`.
    fn initialize(&mut self, t: f64, period: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String>;
    /// Every later tick: one step of `dt` from `t` with `inputs` held; write
    /// `outputs` (feedthrough: the values at `t`; otherwise at `t + dt`).
    fn step(&mut self, t: f64, dt: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String>;
    /// The run is over (also after a fault): release everything.
    fn terminate(&mut self) {}
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::Unsupported(format!("{} cannot save its state", self.label()))
    }
    fn restore(&mut self, _state: &[u8]) -> Result<(), String> {
        Err(format!("{} cannot restore a state", self.label()))
    }
}

/// A host [`crate::Coupler`] as a block implementation: the lockstep seam
/// (`sample` answers at the same instant, so `feedthrough = true`). The
/// contract is opened at the first tick with the block's own signals.
pub struct CouplerBlock {
    coupler: Box<dyn crate::Coupler>,
    interface: BlockInterface,
    name: String,
    stateless: bool,
    opened: bool,
}

impl CouplerBlock {
    /// `stateless`: the coupler keeps no state of its own between samples
    /// (a pure function of its inputs and shared host state), so runtime
    /// checkpoints may include this block.
    pub fn new(name: impl Into<String>, coupler: Box<dyn crate::Coupler>, interface: BlockInterface, stateless: bool) -> Self {
        Self { coupler, interface, name: name.into(), stateless, opened: false }
    }
    /// Open the coupler now (its handshake: a process says hello and is
    /// ready) with the block's contract at `period`, so a controller that
    /// cannot start is refused when it is bound, not at the first tick.
    pub fn opened(name: impl Into<String>, coupler: Box<dyn crate::Coupler>, interface: BlockInterface, stateless: bool, period: f64) -> Result<Self, crate::CouplerError> {
        let mut block = Self::new(name, coupler, interface, stateless);
        let contract = block.contract(period);
        block.coupler.open(&contract)?;
        block.opened = true;
        Ok(block)
    }
    fn contract(&self, period: f64) -> crate::Contract {
        let channels = |ports: &[BlockPort]| ports.iter().map(|p| crate::Channel { name: p.name.clone(), kind: p.kind.clone() }).collect();
        crate::Contract { element: self.name.clone(), period, sensors: channels(&self.interface.inputs), actuators: channels(&self.interface.outputs) }
    }
}

impl BlockImplementation for CouplerBlock {
    fn label(&self) -> String {
        format!("host controller on `{}`", self.name)
    }
    fn interface(&self) -> BlockInterface {
        self.interface.clone()
    }
    fn initialize(&mut self, t: f64, period: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        if !self.opened {
            let contract = self.contract(period);
            self.coupler.open(&contract).map_err(|e| e.to_string())?;
            self.opened = true;
        }
        self.coupler.sample(t, inputs, outputs).map_err(|e| e.to_string())
    }
    fn step(&mut self, t: f64, dt: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        // Feedthrough: the sample at the tick itself, `t + dt` being the next.
        let _ = dt;
        self.coupler.sample(t, inputs, outputs).map_err(|e| e.to_string())
    }
    fn terminate(&mut self) {
        self.coupler.close();
    }
    fn checkpoint(&self) -> Checkpoint {
        if self.stateless { Checkpoint::Stateless } else { Checkpoint::Unsupported(format!("the host controller on `{}` keeps state the runtime cannot capture", self.name)) }
    }
    fn restore(&mut self, _state: &[u8]) -> Result<(), String> {
        if self.stateless { Ok(()) } else { Err(format!("the host controller on `{}` cannot be restored", self.name)) }
    }
}

/// The shadow element: `outputs` held states (zero rate) driving the
/// block's signal outputs; signal inputs only read. Its ports are the
/// block's own (`ModelWorld::add_block`), so the descriptor declares none.
struct Shadow {
    outputs: usize,
    starts: Vec<f64>,
}

impl crate::Behavior for Shadow {
    fn states(&self) -> Vec<crate::StateDeclaration> {
        (0..self.outputs).map(|k| crate::StateDeclaration::new(format!("out.{k}"), QuantityKind::Dimensionless, self.starts[k])).collect()
    }
    fn residual(&self, ctx: &mut crate::Context) {
        for k in 0..self.outputs {
            ctx.set_state_residual(k, ctx.state_rate(k));
            ctx.set_signal(k, ctx.state(k));
        }
    }
    fn jacobian(&self, _view: &crate::View, out: &mut crate::LocalJacobian) -> bool {
        for k in 0..self.outputs {
            out.state_rate(k, k, 1.0);
            out.set(crate::Output::Signal(k), crate::Input::State(k), 1.0);
        }
        true
    }
}

fn shadow(p: &std::collections::BTreeMap<String, f64>) -> Result<Box<dyn crate::Behavior>, crate::EquationError> {
    let outputs = crate::param(p, "outputs")?.max(0.0).round() as usize;
    let starts = (0..outputs).map(|k| p.get(&format!("start.{k}")).copied().unwrap_or(0.0)).collect();
    Ok(Box::new(Shadow { outputs, starts }))
}

/// Register the shadow element type ([`BLOCK`]).
pub fn register(registry: &mut crate::BehaviorRegistry) -> Result<(), crate::RegistryError> {
    let mut descriptor = crate::BehaviorDescriptor::new(BLOCK, "Block (executed by the runtime)", Vec::new(), shadow);
    descriptor.dynamic_ports = true;
    registry.register(descriptor)
}
