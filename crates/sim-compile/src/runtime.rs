//! Drive compiled islands and commit their unknowns to the model's
//! `StateStore` after every step: the store is what scenarios, tests and
//! viewers read; the dense island vectors are disposable.

use crate::{CompileError, compile_islands, island::Island};
use sim_core::{BehaviorId, BehaviorRegistry, Channel, Contract, Coupler, ModelWorld, PortId, PortSchema, QuantityKind, StateId};
use sim_dynamics::System;
use sim_dynamics::{DynamicsError, Event, Integrator, Simulation, Trace};
use sim_solve::NewtonConfig;

pub struct Runtime {
    pub model: ModelWorld,
    pub islands: Vec<Simulation<Island>>,
    pub time: f64,
    /// Per island, per behavior: the store id carrying its entropy production.
    entropy_ids: Vec<Vec<(BehaviorId, StateId)>>,
    /// Tolerance below which a negative production is rejected (W/K).
    pub second_law_tolerance: f64,
    /// Per-island step overrides for `advance`.
    island_steps: Vec<Option<f64>>,
}

/// A resumable point of a [`Runtime`]: see `Runtime::snapshot`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeSnapshot {
    pub time: f64,
    pub islands: Vec<sim_dynamics::Snapshot>,
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
    #[error("controller of `{element}` failed at t={time}: {message}")]
    Controller { element: String, time: f64, message: String },
    #[error("`{element}` is not an external control element")]
    NotExternal { element: String },
}

impl Runtime {
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
        let mut runtime = Self { model, islands, time: 0.0, entropy_ids, second_law_tolerance: 1.0e-9, island_steps: Vec::new() };
        runtime.commit()?;
        Ok(runtime)
    }

    /// Advance every island by `duration` in steps of `h` (or the island's
    /// own step, see [`Self::set_island_step`]), islands in parallel, then commit.
    pub fn advance(&mut self, duration: f64, h: f64) -> Result<(), RuntimeError> {
        let steps = &self.island_steps;
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
        for result in results {
            result?;
        }
        self.time += duration;
        self.commit()
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

    /// Advance every island by `duration` with step-size control (see
    /// `Simulation::run_adaptive`); returns the accepted steps of the
    /// busiest island.
    pub fn advance_adaptive(&mut self, duration: f64, h0: f64, tolerance: f64, h_min: f64, h_max: f64) -> Result<usize, RuntimeError> {
        let mut most = 0;
        for island in &mut self.islands {
            most = most.max(island.run_adaptive(duration, h0, tolerance, h_min, h_max)?);
        }
        self.time += duration;
        self.commit()?;
        Ok(most)
    }

    /// Advance by `duration` in steps of `h`, sampling the committed values
    /// of `ids` every `every` steps into a [`Trace`] (energy included).
    pub fn advance_recording(&mut self, duration: f64, h: f64, every: usize, ids: &[StateId]) -> Result<Trace, RuntimeError> {
        let mut trace = Trace::default();
        let steps = (duration / h).round().max(1.0) as usize;
        let end = self.time + duration;
        let push = |runtime: &Runtime, trace: &mut Trace| {
            trace.time.push(runtime.time);
            trace.state.push(ids.iter().map(|id| runtime.get(*id)).collect());
            trace.energy.push(runtime.energy());
        };
        push(self, &mut trace);
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
        let event = self.islands[0].run_to_event(max_duration, h)?;
        self.time = self.islands[0].time;
        self.commit()?;
        Ok(event)
    }

    /// Give an external control element (`control.external`) its coupler.
    /// The contract — channel names and units — comes from the wiring: each
    /// sensor channel takes the kind of the signal port feeding it, each
    /// actuator channel the kind of the port it drives.
    pub fn attach(&mut self, behavior: BehaviorId, coupler: Box<dyn Coupler>) -> Result<(), RuntimeError> {
        let contract = self.contract(behavior);
        let element = contract.element.clone();
        let target = self.islands.iter_mut().find_map(|i| i.system.behaviors.iter_mut().find(|(b, _)| *b == behavior).map(|(_, b)| b));
        match target {
            Some(target) => target.couple(coupler, contract).map_err(|_| RuntimeError::NotExternal { element }),
            None => Err(RuntimeError::NotExternal { element }),
        }
    }

    /// Attach a Python controller script (see `sim_couple::python`); the
    /// clients root is the repository's `clients/` directory.
    pub fn attach_python(&mut self, behavior: BehaviorId, clients_root: impl AsRef<std::path::Path>, script: impl AsRef<std::path::Path>, args: &[&str]) -> Result<(), RuntimeError> {
        let coupler = sim_couple::python(clients_root, script, args).map_err(|e| RuntimeError::State(e.to_string()))?;
        self.attach(behavior, Box::new(coupler))
    }

    /// The contract a controller attached to `behavior` would see.
    pub fn contract(&self, behavior: BehaviorId) -> Contract {
        let model = &self.model;
        let element = model.objects[model.behaviors[behavior].object].name.clone();
        let period = model.parameters_of(behavior).get("period").copied().unwrap_or(0.0);
        let mut sensors = Vec::new();
        let mut actuators = Vec::new();
        let mut owned: Vec<(&String, PortId, PortSchema)> = model.ports.iter().filter(|(_, p)| p.owner == behavior).map(|(id, p)| (&p.name, id, p.schema)).collect();
        owned.sort_by(|a, b| a.0.cmp(b.0));
        for (name, id, schema) in owned {
            let peer_kind = model
                .connections
                .iter()
                .find(|c| c.ports.contains(&id))
                .and_then(|c| c.ports.iter().find(|p| **p != id))
                .and_then(|p| match model.ports[*p].schema {
                    PortSchema::SignalIn(k) | PortSchema::SignalOut(k) => Some(k),
                    PortSchema::Acausal(_) => None,
                });
            let channel = |prefix: &str, own: QuantityKind| Channel { name: name.strip_prefix(prefix).unwrap_or(name).to_owned(), kind: peer_kind.unwrap_or(own) };
            match schema {
                PortSchema::SignalIn(k) => sensors.push(channel("sense.", k)),
                PortSchema::SignalOut(k) => actuators.push(channel("act.", k)),
                PortSchema::Acausal(_) => {}
            }
        }
        Contract { element, period, sensors, actuators }
    }

    fn commit(&mut self) -> Result<(), RuntimeError> {
        for island in &self.islands {
            for (behavior, equations) in &island.system.behaviors {
                if let Some(message) = equations.failure() {
                    let element = self.model.objects[self.model.behaviors[*behavior].object].name.clone();
                    return Err(RuntimeError::Controller { element, time: island.time, message });
                }
            }
        }
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
        self.model.state.commit(trial).map_err(|e| RuntimeError::State(e.to_string()))
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
    pub fn snapshot(&self) -> RuntimeSnapshot {
        RuntimeSnapshot { time: self.time, islands: self.islands.iter().map(|i| i.snapshot()).collect() }
    }

    /// Resume from a snapshot of this runtime and commit it.
    pub fn restore(&mut self, snapshot: &RuntimeSnapshot) -> Result<(), RuntimeError> {
        if snapshot.islands.len() != self.islands.len() {
            return Err(RuntimeError::State(format!("snapshot has {} islands, runtime {}", snapshot.islands.len(), self.islands.len())));
        }
        for (island, saved) in self.islands.iter_mut().zip(&snapshot.islands) {
            island.restore(saved)?;
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
