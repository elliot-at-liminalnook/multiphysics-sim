//! Drive compiled islands and commit their unknowns to the model's
//! `StateStore` after every step: the store is what scenarios, tests and
//! viewers read; the dense island vectors are disposable.

use crate::blocks::{BlockFault, BlockState, Scheduler};
use crate::{CompileError, compile_islands, island::Island};
use sim_core::{BehaviorId, BehaviorRegistry, BlockImplementation, Channel, Contract, Coupler, CouplerBlock, ModelWorld, PortId, StateId};
use sim_dynamics::System;
use sim_dynamics::{DynamicsError, Event, Integrator, Simulation, Trace};
use sim_solve::NewtonConfig;

pub struct Runtime {
    pub model: ModelWorld,
    /// Frozen physical metadata used to compile this runtime; no residual lookups.
    pub definitions: sim_core::definitions::FrozenDefinitions,
    pub(crate) observation_identity: std::sync::Arc<()>,
    pub(crate) observation_committed_times: Vec<f64>,
    pub islands: Vec<Simulation<Island>>,
    pub time: f64,
    /// Per island, per behavior: the store id carrying its entropy production.
    entropy_ids: Vec<Vec<(BehaviorId, StateId)>>,
    /// Tolerance below which a negative production is rejected (W/K).
    pub second_law_tolerance: f64,
    /// Per-island step overrides for `advance`.
    island_steps: Vec<Option<f64>>,
    /// The model's blocks and their implementations (executed between island
    /// advances, at committed clock ticks).
    blocks: Scheduler,
    /// How many times an integration segment that fails to converge is
    /// retried from its start with the step halved (0: fail at once). A
    /// segment never contains a block tick, so a retry never re-runs a block.
    pub retry_halvings: usize,
    /// Segments retried with a finer step so far.
    pub step_refinements: usize,
}

/// A resumable point of a [`Runtime`]: see `Runtime::snapshot`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeSnapshot {
    pub time: f64,
    pub islands: Vec<sim_dynamics::Snapshot>,
    /// The block scheduler's state and each implementation's own state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<BlockState>,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Compile(#[from] CompileError),
    #[error(transparent)]
    Dynamics(#[from] DynamicsError),
    #[error("state commit failed: {0}")]
    State(String),
    #[error("second law violated: behavior {behavior:?} produces entropy at {rate:e} W/K at t={time}")]
    SecondLaw { behavior: BehaviorId, rate: f64, time: f64 },
    #[error("block `{block}` failed at t={time}: {message}")]
    Block { block: String, time: f64, message: String },
    #[error("unsupported: {0}")]
    Unsupported(String),
}

impl Runtime {
    /// Opt into snapping each island's clock onto its step grid (see
    /// `Simulation::snap_to_grid`). Used where scheduled switching events must
    /// land on step ends; off by default to keep existing runs bit-identical.
    pub fn set_grid_snapping(&mut self, on: bool) {
        for island in &mut self.islands {
            island.snap_to_grid = on;
        }
    }
    /// Opt into keeping each island's `run` clock on one absolute step grid
    /// across `advance` calls (see `Simulation::grid_clock`). Callers that
    /// advance in many short chunks with sampled controllers need it; off by
    /// default so other runs stay bit-identical to their recorded results.
    pub fn set_grid_clock(&mut self, on: bool) {
        for island in &mut self.islands {
            island.grid_clock = on;
        }
    }
    pub fn grid_snapping(&self) -> bool {
        self.islands.first().is_some_and(|i| i.snap_to_grid)
    }
    pub fn new(mut model: ModelWorld, registry: &BehaviorRegistry, integrator: Integrator) -> Result<Self, RuntimeError> {
        let islands = compile_islands(&mut model, registry)?;
        let islands = islands
            .into_iter()
            .map(|island| {
                let initial = island.reduced_initial();
                let mut sim = Simulation::new(island, integrator, initial);
                sim.record_every = 0;
                sim.make_consistent(NewtonConfig::default())?;
                Ok::<_, DynamicsError>(sim)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut entropy_ids = Vec::new();
        for island in &islands {
            let mut ids = Vec::new();
            for (behavior, _) in &island.system.behaviors {
                let name = format!("{}.entropy_production", model.objects[model.behaviors[*behavior].object].name);
                ids.push((*behavior, model.state.register(name, sim_core::QuantityKind::Entropy, 0.0).map_err(|e| RuntimeError::State(e.to_string()))?));
            }
            entropy_ids.push(ids);
        }
        let definitions = registry.definitions().map_err(|e| RuntimeError::State(e.to_string()))?;
        let mut runtime = Self { model, definitions, observation_identity: std::sync::Arc::new(()), observation_committed_times: Vec::new(), islands, time: 0.0, entropy_ids, second_law_tolerance: 1.0e-9, island_steps: Vec::new(), blocks: Scheduler::default(), retry_halvings: 0, step_refinements: 0 };
        runtime.commit()?;
        let scheduler = {
            let rt = &runtime;
            Scheduler::build(&rt.model, rt.time, &|port| rt.try_signal_id(port), &|behavior, name| rt.state_ids_of(behavior, name))
        };
        runtime.blocks = scheduler.map_err(RuntimeError::from)?;
        Ok(runtime)
    }

    /// The model has blocks (executed at clock ticks).
    pub fn has_blocks(&self) -> bool {
        !self.blocks.blocks.is_empty()
    }

    /// Bind the implementation that runs block `name`. Its interface must be
    /// the block's (names, quantities, feedthrough); a previous one is terminated.
    pub fn bind_block(&mut self, name: &str, implementation: Box<dyn BlockImplementation>) -> Result<(), RuntimeError> {
        self.blocks.bind(name, implementation).map_err(RuntimeError::State)
    }

    /// Bind a host [`Coupler`] (the lockstep seam) to the block whose shadow
    /// element is `behavior`, opening it now with the block's contract (a
    /// controller that cannot start is refused here, naming the block).
    /// `stateless`: the coupler keeps no state between samples, so
    /// checkpoints may include it.
    pub fn bind_coupler(&mut self, behavior: BehaviorId, coupler: Box<dyn Coupler>, stateless: bool) -> Result<(), RuntimeError> {
        let decl = self.model.block_of(behavior).ok_or_else(|| RuntimeError::State("not a block's shadow element".into()))?.clone();
        let block = CouplerBlock::opened(decl.name.clone(), coupler, decl.interface.clone(), stateless, decl.timing.clock.interval(0).unwrap_or(decl.timing.clock.nominal_period()))
            .map_err(|e| RuntimeError::Block { block: decl.name.clone(), time: self.time, message: e.to_string() })?;
        self.bind_block(&decl.name, Box::new(block))
    }

    /// Bind a Python controller script (see `sim_couple::python`) to the
    /// block whose shadow is `behavior`; the clients root is the repository's
    /// `clients/` directory.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn bind_python(&mut self, behavior: BehaviorId, clients_root: impl AsRef<std::path::Path>, script: impl AsRef<std::path::Path>, args: &[&str]) -> Result<(), RuntimeError> {
        let coupler = sim_couple::python(clients_root, script, args).map_err(|e| RuntimeError::State(e.to_string()))?;
        self.bind_coupler(behavior, Box::new(coupler), false)
    }

    /// Run the blocks due now (at a fresh runtime: their first tick, which
    /// initialises each implementation), so a host sees an implementation
    /// that fails to start before the first advance. `advance` does this
    /// itself; a tick never runs twice.
    pub fn start_blocks(&mut self) -> Result<(), RuntimeError> {
        self.run_blocks()
    }

    /// Run the blocks due now and write their held outputs in one batch.
    fn run_blocks(&mut self) -> Result<(), RuntimeError> {
        if self.blocks.blocks.is_empty() {
            return Ok(());
        }
        let state = &self.model.state;
        let writes = self.blocks.tick(self.time, &|id| state.get(id).unwrap_or(f64::NAN))?;
        if !writes.is_empty() {
            self.set_many(&writes)?;
        }
        Ok(())
    }

    /// The next block tick strictly after now (within roundoff), before `end`.
    fn next_boundary(&self, end: f64) -> Option<f64> {
        let tick = self.blocks.next_tick()?;
        let eps = 64.0 * f64::EPSILON * self.time.abs().max(end.abs()).max(1.0);
        (tick > self.time + eps && tick < end - eps).then_some(tick)
    }

    /// Land every island's clock exactly on `t` (a tick reached within roundoff).
    fn land(&mut self, t: f64) {
        self.time = t;
        for island in &mut self.islands {
            island.time = t;
        }
    }

    /// Advance by `duration` in steps of `h` (or each island's own step, see
    /// [`Self::set_island_step`]), islands in parallel. With blocks, the
    /// advance is cut at every clock tick: islands reach the tick, commit,
    /// and the due blocks run there (docs/architecture/composition.md).
    pub fn advance(&mut self, duration: f64, h: f64) -> Result<(), RuntimeError> {
        if self.blocks.blocks.is_empty() {
            return self.advance_islands(duration, h);
        }
        let end = self.time + duration;
        self.run_blocks()?;
        loop {
            let eps = 64.0 * f64::EPSILON * self.time.abs().max(end.abs()).max(h);
            if end - self.time <= eps {
                break;
            }
            let until = self.next_boundary(end).unwrap_or(end);
            self.advance_islands(until - self.time, h)?;
            self.land(until);
            self.commit()?;
            self.run_blocks()?;
        }
        self.land(end);
        Ok(())
    }

    /// One integration segment (no block tick inside): every island steps
    /// `duration`, retried from the segment's start with halved steps up to
    /// `retry_halvings` times when Newton fails, then commit.
    fn advance_islands(&mut self, duration: f64, h: f64) -> Result<(), RuntimeError> {
        let saved: Option<Vec<sim_dynamics::Snapshot>> = (self.retry_halvings > 0).then(|| self.islands.iter().map(|i| i.snapshot()).collect());
        let mut step = h;
        let mut tries = 0;
        loop {
            match self.step_islands(duration, step) {
                Ok(()) => break,
                Err(e) if tries < self.retry_halvings && matches!(e, DynamicsError::Solve { .. } | DynamicsError::NonFinite(_)) => {
                    for (island, snapshot) in self.islands.iter_mut().zip(saved.as_ref().expect("saved for retries")) {
                        island.restore(snapshot)?;
                    }
                    step *= 0.5;
                    tries += 1;
                    self.step_refinements += 1;
                }
                Err(e) => return Err(e.into()),
            }
        }
        self.time += duration;
        self.commit()
    }

    /// Step every island by `duration` in steps of `h` (or the island's own
    /// step, see [`Self::set_island_step`]), islands in parallel.
    fn step_islands(&mut self, duration: f64, h: f64) -> Result<(), DynamicsError> {
        let steps = &self.island_steps;
        #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
        let results: Vec<Result<(), DynamicsError>> = if self.islands.len() > 1 {
            std::thread::scope(|scope| {
                let handles: Vec<_> = self
                    .islands
                    .iter_mut()
                    .enumerate()
                    .map(|(k, island)| {
                        let own = steps.get(k).copied().flatten().unwrap_or(h);
                        scope.spawn(move || island.run(duration, own))
                    })
                    .collect();
                handles.into_iter().map(|handle| handle.join().expect("island thread")).collect()
            })
        } else {
            self.islands.iter_mut().enumerate().map(|(k, island)| island.run(duration, steps.get(k).copied().flatten().unwrap_or(h))).collect()
        };
        #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
        let results: Vec<Result<(), DynamicsError>> = self.islands.iter_mut().enumerate().map(|(k, island)| island.run(duration, steps.get(k).copied().flatten().unwrap_or(h))).collect();
        for result in results {
            result?;
        }
        Ok(())
    }

    /// Give the island containing `behavior` its own step size for
    /// [`Self::advance`]: a multirate model steps its electronics finer
    /// than its mechanics. `None` restores the shared step.
    pub fn set_island_step(&mut self, behavior: BehaviorId, h: Option<f64>) {
        if let Some(k) = self.islands.iter().position(|i| i.system.behaviors.iter().any(|(b, _)| *b == behavior)) {
            if self.island_steps.len() < self.islands.len() {
                self.island_steps.resize(self.islands.len(), None);
            }
            self.island_steps[k] = h;
        }
    }

    /// Effective fixed step sizes for a call to `advance`, including explicit
    /// multirate overrides. Inspection/recording can retain the actual settings.
    pub fn effective_step_sizes(&self, default: f64) -> Vec<f64> {
        (0..self.islands.len()).map(|k| self.island_steps.get(k).copied().flatten().unwrap_or(default)).collect()
    }

    /// Advance every island by `duration` with step-size control (see
    /// `Simulation::run_adaptive`); returns the accepted steps of the
    /// busiest island.
    pub fn advance_adaptive(&mut self, duration: f64, h0: f64, tolerance: f64, h_min: f64, h_max: f64) -> Result<usize, RuntimeError> {
        if !(h_min > 0.0 && h_min <= h_max && h_max.is_finite()) {
            return Err(DynamicsError::StepBounds { h_min, h_max }.into());
        }
        let end = self.time + duration;
        self.run_blocks()?;
        let mut most = 0;
        loop {
            let eps = 64.0 * f64::EPSILON * self.time.abs().max(end.abs()).max(h_min);
            if end - self.time <= eps {
                break;
            }
            let until = self.next_boundary(end).unwrap_or(end);
            let segment = until - self.time;
            for island in &mut self.islands {
                // A segment shorter than `h_min` (a tick just ahead) is one step of its own length.
                most = most.max(island.run_adaptive(segment, h0, tolerance, h_min.min(segment), h_max.min(segment).max(h_min.min(segment)))?);
            }
            self.land(until);
            self.commit()?;
            self.run_blocks()?;
        }
        Ok(most)
    }

    /// Advance by `duration` in steps of `h`, sampling the committed values
    /// of `ids` every `every` steps into a [`Trace`] (energy included).
    pub fn advance_recording(&mut self, duration: f64, h: f64, every: usize, ids: &[StateId]) -> Result<Trace, RuntimeError> {
        if every == 0 {
            return Err(RuntimeError::Unsupported("recording interval must be at least one step".into()));
        }
        // This loop steps every island with the one shared `h`; silently
        // coarsening a multirate island would change the physics.
        if self.island_steps.iter().any(Option::is_some) {
            return Err(RuntimeError::Unsupported("advance_recording does not honour per-island step sizes; use advance".into()));
        }
        let mut trace = Trace::default();
        let steps = (duration / h).round().max(1.0) as usize;
        let end = self.time + duration;
        let push = |runtime: &Runtime, trace: &mut Trace| {
            trace.time.push(runtime.time);
            trace.state.push(ids.iter().map(|id| runtime.get(*id)).collect());
            trace.energy.push(runtime.energy());
        };
        push(self, &mut trace);
        if !self.blocks.blocks.is_empty() {
            // Steps of `h`, each cut short at a block tick; the blocks run there.
            self.run_blocks()?;
            let mut taken = 0usize;
            loop {
                let eps = 64.0 * f64::EPSILON * self.time.abs().max(end.abs()).max(h);
                if end - self.time <= eps {
                    break;
                }
                let until = self.next_boundary(end).unwrap_or(end).min(self.time + h);
                let dt = until - self.time;
                for island in &mut self.islands {
                    island.step(dt)?;
                }
                self.land(until);
                self.commit()?;
                self.run_blocks()?;
                taken += 1;
                if taken % every == 0 {
                    push(self, &mut trace);
                }
            }
            self.land(end);
            if trace.time.last() != Some(&end) {
                push(self, &mut trace);
            }
            return Ok(trace);
        }
        for step in 1..=steps {
            let remaining = end - self.time;
            let dt = if step == steps { remaining } else { h.min(remaining) };
            if dt <= 0.0 {
                break;
            }
            for island in &mut self.islands {
                island.step(dt)?;
            }
            self.time += dt;
            if step % every == 0 || step == steps {
                self.commit()?;
                push(self, &mut trace);
            }
        }
        self.time = end;
        self.commit()?;
        Ok(trace)
    }

    /// Adaptive stepping with a trace: every island steps with error
    /// control and the committed values of `ids` are recorded every
    /// `sample` seconds of simulation time.
    pub fn advance_recording_adaptive(&mut self, duration: f64, sample: f64, h0: f64, tolerance: f64, h_min: f64, h_max: f64, ids: &[StateId]) -> Result<Trace, RuntimeError> {
        let mut trace = Trace::default();
        let end = self.time + duration;
        let push = |runtime: &Runtime, trace: &mut Trace| {
            trace.time.push(runtime.time);
            trace.state.push(ids.iter().map(|id| runtime.get(*id)).collect());
            trace.energy.push(runtime.energy());
        };
        push(self, &mut trace);
        while end - self.time > 1.0e-12 {
            let slice = sample.min(end - self.time);
            self.advance_adaptive(slice, h0, tolerance, h_min, h_max.min(slice))?;
            push(self, &mut trace);
        }
        Ok(trace)
    }

    /// Advance a single-island model to its next event (or `max_duration`).
    pub fn advance_to_event(&mut self, max_duration: f64, h: f64) -> Result<Option<Event>, RuntimeError> {
        if !self.blocks.blocks.is_empty() {
            return Err(RuntimeError::Unsupported("advance_to_event stops at physics events only; a model with blocks advances with advance".into()));
        }
        let event = self.islands[0].run_to_event(max_duration, h)?;
        self.time = self.islands[0].time;
        self.commit()?;
        Ok(event)
    }

    /// What a host controller bound to the block whose shadow is `behavior`
    /// sees: its name, clock period, inputs (sensors) and outputs (actuators).
    pub fn contract(&self, behavior: BehaviorId) -> Contract {
        let decl = self.model.block_of(behavior).expect("a block's shadow element");
        let channels = |ports: &[sim_core::BlockPort]| ports.iter().map(|p| Channel { name: p.name.clone(), kind: p.kind.clone() }).collect();
        Contract { element: decl.name.clone(), period: decl.timing.clock.nominal_period(), sensors: channels(&decl.interface.inputs), actuators: channels(&decl.interface.outputs) }
    }

    /// Set several committed state values, then re-solve the algebraic
    /// unknowns once per island touched (a block tick's writes).
    pub fn set_many(&mut self, values: &[(StateId, f64)]) -> Result<(), RuntimeError> {
        for island in &mut self.islands {
            let mut touched = false;
            for (id, value) in values {
                if let Some(index) = island.system.state_ids.iter().position(|s| s == id) {
                    let Some(reduced) = island.system.reduced_of[index] else {
                        return Err(RuntimeError::State(format!("`{}` is derived from other unknowns and cannot be set", self.model.state.entry(*id).map(|s| s.name.clone()).unwrap_or_default())));
                    };
                    island.state[reduced] = *value;
                    touched = true;
                }
            }
            if touched {
                island.make_consistent(NewtonConfig::default())?;
                island.system.observe_consistent(island.time, &island.state, island.last_rate());
            }
        }
        self.commit()
    }

    /// A behavior's state in every island that has a copy of it: a block's
    /// shadow is in each island that reads its outputs (`build_islands`).
    fn state_ids_of(&self, behavior: BehaviorId, name: &str) -> Vec<StateId> {
        self.islands.iter().filter_map(|island| island.system.state_index(behavior, name).map(|i| island.system.state_ids[i])).collect()
    }

    fn try_signal_id(&self, port: PortId) -> Option<StateId> {
        self.islands.iter().find_map(|island| island.system.port_signal.get(&port).map(|i| island.system.state_ids[*i]))
    }

    fn commit(&mut self) -> Result<(), RuntimeError> {
        let mut trial = self.model.state.begin_trial();
        for (island, ids) in self.islands.iter().zip(&self.entropy_ids) {
            // The store sees every unknown, derived ones included.
            let (full, _) = island.system.expand_at(island.time, &island.state, island.last_rate());
            for (index, id) in island.system.state_ids.iter().enumerate() {
                trial.set(*id, full[index]).map_err(|e| RuntimeError::State(format!("`{}`: {e}", self.model.state.entry(*id).map(|s| s.name.clone()).unwrap_or_default())))?;
            }
            let production = island.system.entropy_production(island.time, &island.state, island.last_rate());
            for ((behavior, id), rate) in ids.iter().zip(production) {
                if rate < -self.second_law_tolerance {
                    return Err(RuntimeError::SecondLaw { behavior: *behavior, rate, time: island.time });
                }
                trial.set(*id, rate).map_err(|e| RuntimeError::State(format!("entropy production of {behavior:?}: {e}")))?;
            }
        }
        self.model.state.commit(trial).map_err(|e| RuntimeError::State(e.to_string()))?;
        self.observation_committed_times = self.islands.iter().map(|island| island.time).collect();
        Ok(())
    }

    /// Stable id of a behavior's entropy production (W/K).
    pub fn entropy_production_id(&self, behavior: BehaviorId) -> StateId {
        self.entropy_ids.iter().flatten().find(|(b, _)| *b == behavior).map(|(_, id)| *id).expect("behavior belongs to this model")
    }

    /// Committed value of a stable state id.
    pub fn get(&self, id: StateId) -> f64 {
        self.model.state.get(id).expect("state id belongs to this model")
    }

    /// Stable id of a behavior's named state.
    pub fn state_id(&self, behavior: BehaviorId, name: &str) -> StateId {
        self.islands
            .iter()
            .find_map(|island| island.system.state_index(behavior, name).map(|i| island.system.state_ids[i]))
            .unwrap_or_else(|| panic!("behavior has no state `{name}`"))
    }

    /// Stable id of the across variable (lane 0) at a port's node.
    pub fn across_id(&self, port: PortId) -> StateId {
        self.across_lane_id(port, 0)
    }

    pub fn across_lane_id(&self, port: PortId, lane: usize) -> StateId {
        self.islands
            .iter()
            .find_map(|island| island.system.port_lanes.get(&port).map(|lanes| island.system.state_ids[lanes[lane]]))
            .expect("port belongs to this model")
    }

    /// Stable id of the value carried by a signal port.
    pub fn signal_id(&self, port: PortId) -> StateId {
        self.islands
            .iter()
            .find_map(|island| island.system.port_signal.get(&port).map(|i| island.system.state_ids[*i]))
            .expect("signal port belongs to this model")
    }

    /// Set a committed state value in every island that carries it (used
    /// for initial conditions and external inputs between steps), then
    /// re-solve the algebraic unknowns for consistency.
    pub fn set(&mut self, id: StateId, value: f64) -> Result<(), RuntimeError> {
        for island in &mut self.islands {
            if let Some(index) = island.system.state_ids.iter().position(|s| *s == id) {
                let Some(reduced) = island.system.reduced_of[index] else {
                    return Err(RuntimeError::State(format!("`{}` is derived from other unknowns and cannot be set", self.model.state.entry(id).map(|s| s.name.clone()).unwrap_or_default())));
                };
                island.state[reduced] = value;
                island.make_consistent(NewtonConfig::default())?;
            }
        }
        self.commit()
    }

    /// Every island's committed state and clock, to come back to with
    /// [`Self::restore`]: an episode's start, a branch point of a search.
    /// Refused (`Unsupported`) when a block's implementation cannot save its
    /// state: a checkpoint without it would not be a checkpoint.
    pub fn snapshot(&self) -> Result<RuntimeSnapshot, RuntimeError> {
        let blocks = self.blocks.checkpoint().map_err(RuntimeError::Unsupported)?;
        Ok(RuntimeSnapshot { time: self.time, islands: self.islands.iter().map(|i| i.snapshot()).collect(), blocks })
    }

    /// Resume from a snapshot of this runtime and commit it.
    pub fn restore(&mut self, snapshot: &RuntimeSnapshot) -> Result<(), RuntimeError> {
        if snapshot.islands.len() != self.islands.len() {
            return Err(RuntimeError::State(format!("snapshot has {} islands, runtime {}", snapshot.islands.len(), self.islands.len())));
        }
        for (island, saved) in self.islands.iter_mut().zip(&snapshot.islands) {
            island.restore(saved)?;
        }
        if self.has_blocks() || !snapshot.blocks.is_empty() {
            self.blocks.restore(&snapshot.blocks).map_err(RuntimeError::Unsupported)?;
        }
        self.time = snapshot.time;
        self.commit()
    }

    /// Seed every island's noise generator (each island offset from `seed`).
    pub fn seed(&mut self, seed: u64) {
        for (k, island) in self.islands.iter_mut().enumerate() {
            island.system.seed_noise(seed.wrapping_add(k as u64 * 7919));
        }
    }

    /// Total stored energy across islands.
    pub fn energy(&self) -> f64 {
        self.islands.iter().filter_map(|i| i.energy()).sum()
    }

    pub fn events(&self) -> usize {
        self.islands.iter().map(|i| i.events.len()).sum()
    }

    /// Read the equations object of a behavior (for reference analyses that
    /// need parameters the model does not expose otherwise).
    pub fn behavior(&self, id: BehaviorId) -> Option<&dyn sim_core::Behavior> {
        self.islands.iter().find_map(|i| i.system.behaviors.iter().find(|(b, _)| *b == id).map(|(_, b)| b.as_ref()))
    }
}

impl From<BlockFault> for RuntimeError {
    fn from(f: BlockFault) -> Self {
        RuntimeError::Block { block: f.block, time: f.time, message: f.message }
    }
}
