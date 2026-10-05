//! The block scheduler: executes the model's blocks at committed clock ticks,
//! between island advances, with the semantics of
//! docs/architecture/composition.md ("One tick, precisely"):
//! apply pending end-of-step outputs, execute due blocks in dependency order
//! (inputs sampled from the committed plant state before any write at this
//! instant, or from other blocks' applied outputs), pass outputs through the
//! output delay, and write every changed output to the shadow elements' held
//! states in one batch.
use sim_core::{BlockDecl, BlockImplementation, Checkpoint, ModelWorld, PortSchema, StateId};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;

/// Where a block input reads from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Source {
    /// A plant signal: the committed value of this unknown.
    Plant(StateId),
    /// Output `output` of block `block` (its applied value).
    Block { block: usize, output: usize },
}

pub(crate) struct RuntimeBlock {
    pub decl: BlockDecl,
    /// Behind a mutex only so the runtime stays `Sync`; every call is
    /// made through `&mut` (`get_mut`), except checkpoints.
    pub implementation: Option<Mutex<Box<dyn BlockImplementation>>>,
    pub inputs: Vec<Source>,
    /// The shadow element's held states, one per output.
    pub output_ids: Vec<StateId>,
    /// Index of the next tick.
    pub next: u64,
    pub initialized: bool,
    /// End-of-step outputs computed at the last tick, applied at the next.
    pub pending: Option<Vec<f64>>,
    /// The block's signal outputs now (after the output delay).
    pub applied: Vec<f64>,
    pub in_fifo: VecDeque<Vec<f64>>,
    pub out_fifo: VecDeque<Vec<f64>>,
    pub calls: u64,
    pub terminated: bool,
}

/// A failure the scheduler reports (the runtime names it `RuntimeError::Block`).
#[derive(Clone, Debug, PartialEq)]
pub struct BlockFault {
    pub block: String,
    pub time: f64,
    pub message: String,
}

#[derive(Default)]
pub(crate) struct Scheduler {
    pub t0: f64,
    pub blocks: Vec<RuntimeBlock>,
    /// Execution order: producers with feedthrough before their consumers.
    pub order: Vec<usize>,
    pub fault: Option<BlockFault>,
}

/// The scheduler's own state, for runtime checkpoints.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BlockState {
    pub next: u64,
    pub initialized: bool,
    pub pending: Option<Vec<f64>>,
    pub applied: Vec<f64>,
    pub in_fifo: Vec<Vec<f64>>,
    pub out_fifo: Vec<Vec<f64>>,
    pub calls: u64,
    /// The implementation's own state (`Checkpoint::State`), when it has one.
    pub implementation: Option<Vec<u8>>,
}

impl BlockState {
    /// The state as flat numbers, for numeric snapshot formats (a learning
    /// environment's `Vec<f64>`). None when the implementation has opaque
    /// state of its own.
    pub fn to_numbers(&self) -> Option<Vec<f64>> {
        if self.implementation.is_some() {
            return None;
        }
        let width = self.applied.len();
        let mut out = vec![self.next as f64, self.initialized as u8 as f64, self.calls as f64, width as f64];
        out.extend(&self.applied);
        out.push(self.pending.is_some() as u8 as f64);
        out.extend(self.pending.iter().flatten());
        let inputs = self.in_fifo.first().map_or(0, Vec::len);
        out.extend([self.in_fifo.len() as f64, inputs as f64]);
        out.extend(self.in_fifo.iter().flatten());
        out.push(self.out_fifo.len() as f64);
        out.extend(self.out_fifo.iter().flatten());
        Some(out)
    }

    /// Read [`Self::to_numbers`]' format; returns the state and how many
    /// numbers it used.
    pub fn from_numbers(v: &[f64]) -> Option<(Self, usize)> {
        let mut at = 0;
        let mut take = |n: usize| -> Option<&[f64]> {
            let s = v.get(at..at + n)?;
            at += n;
            Some(s)
        };
        let head = take(4)?.to_vec();
        let width = head[3] as usize;
        let applied = take(width)?.to_vec();
        let pending = if take(1)?[0] != 0.0 { Some(take(width)?.to_vec()) } else { None };
        let fifo = take(2)?.to_vec();
        let (count, inputs) = (fifo[0] as usize, fifo[1] as usize);
        let in_fifo = (0..count).map(|_| take(inputs).map(<[f64]>::to_vec)).collect::<Option<Vec<_>>>()?;
        let count = take(1)?[0] as usize;
        let out_fifo = (0..count).map(|_| take(width).map(<[f64]>::to_vec)).collect::<Option<Vec<_>>>()?;
        let state = Self { next: head[0] as u64, initialized: head[1] != 0.0, pending, applied, in_fifo, out_fifo, calls: head[2] as u64, implementation: None };
        Some((state, at))
    }
}

fn exclusive(m: Mutex<Box<dyn BlockImplementation>>) -> Box<dyn BlockImplementation> {
    m.into_inner().unwrap_or_else(|p| p.into_inner())
}

fn roundoff(a: f64, b: f64, period: f64) -> f64 {
    64.0 * f64::EPSILON * a.abs().max(b.abs()).max(period)
}

impl Scheduler {
    /// Resolve every block's input sources and outputs, and the execution
    /// order. `signal` finds a port's committed signal unknown; `state` a
    /// behavior's state by name. Errors name the block and the port.
    pub fn build(model: &ModelWorld, t0: f64, signal: &dyn Fn(sim_core::PortId) -> Option<StateId>, state: &dyn Fn(sim_core::BehaviorId, &str) -> Option<StateId>) -> Result<Self, BlockFault> {
        let fault = |block: &str, message: String| BlockFault { block: block.to_owned(), time: t0, message };
        let index_of: std::collections::HashMap<sim_core::BehaviorId, usize> = model.blocks.iter().enumerate().map(|(k, b)| (b.behavior, k)).collect();
        let mut blocks = Vec::with_capacity(model.blocks.len());
        for decl in &model.blocks {
            decl.timing.validate().map_err(|e| fault(&decl.name, e))?;
            // Outputs before the first write are their start values; a block
            // that first ticks after the run's start must declare them.
            if decl.timing.clock.first() > 0.0 {
                if let Some(port) = decl.interface.outputs.iter().find(|p| p.start.is_none()) {
                    return Err(fault(&decl.name, format!("output `{}` has no start value, but the block's first tick is {} s after the start: declare one, or tick at the start", port.name, decl.timing.clock.first())));
                }
            }
            let mut inputs = Vec::with_capacity(decl.interface.inputs.len());
            for port in &decl.interface.inputs {
                let id = model.ports.iter().find(|(_, p)| p.owner == decl.behavior && p.name == port.name && matches!(p.schema, PortSchema::SignalIn(_))).map(|(id, _)| id)
                    .ok_or_else(|| fault(&decl.name, format!("input `{}` has no port in the model", port.name)))?;
                let connection = model.connections.iter().find(|c| c.ports.contains(&id))
                    .ok_or_else(|| fault(&decl.name, format!("input `{}` is not connected to anything: wire it to a signal output", port.name)))?;
                let driver = connection.ports.iter().copied().find(|p| matches!(model.ports[*p].schema, PortSchema::SignalOut(_)))
                    .ok_or_else(|| fault(&decl.name, format!("input `{}` is connected, but nothing drives it (no signal output on its net)", port.name)))?;
                let owner = model.ports[driver].owner;
                let source = match index_of.get(&owner) {
                    Some(&producer) => {
                        let name = &model.ports[driver].name;
                        let output = model.blocks[producer].interface.outputs.iter().position(|p| &p.name == name)
                            .ok_or_else(|| fault(&decl.name, format!("input `{}` reads `{}` of block `{}`, which is not one of its outputs", port.name, name, model.blocks[producer].name)))?;
                        Source::Block { block: producer, output }
                    }
                    None => Source::Plant(signal(id).ok_or_else(|| fault(&decl.name, format!("input `{}` has no compiled signal", port.name)))?),
                };
                inputs.push(source);
            }
            let output_ids = (0..decl.interface.outputs.len())
                .map(|k| state(decl.behavior, &format!("out.{k}")).ok_or_else(|| fault(&decl.name, format!("output `{}` has no held state", decl.interface.outputs[k].name))))
                .collect::<Result<Vec<_>, _>>()?;
            let applied = decl.interface.outputs.iter().map(|p| p.start.unwrap_or(0.0)).collect();
            blocks.push(RuntimeBlock {
                decl: decl.clone(), implementation: None, inputs, output_ids, next: 0, initialized: false, pending: None, applied,
                in_fifo: VecDeque::new(), out_fifo: VecDeque::new(), calls: 0, terminated: false,
            });
        }
        let order = Self::order(&blocks).map_err(|(block, message)| fault(&block, message))?;
        Ok(Self { t0, blocks, order, fault: None })
    }

    /// Topological order over same-instant dependencies: an edge from a
    /// producer that has feedthrough and no output delay. A cycle is an
    /// algebraic loop.
    fn order(blocks: &[RuntimeBlock]) -> Result<Vec<usize>, (String, String)> {
        let n = blocks.len();
        let same_instant = |b: &RuntimeBlock| b.decl.interface.feedthrough && b.decl.timing.output_delay == 0;
        let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (consumer, b) in blocks.iter().enumerate() {
            for source in &b.inputs {
                if let Source::Block { block, .. } = source {
                    if same_instant(&blocks[*block]) && !deps[consumer].contains(block) {
                        deps[consumer].push(*block);
                    }
                }
            }
        }
        // 0 unvisited, 1 on the stack, 2 done.
        let mut mark = vec![0u8; n];
        let mut order = Vec::with_capacity(n);
        fn visit(k: usize, deps: &[Vec<usize>], mark: &mut [u8], order: &mut Vec<usize>, stack: &mut Vec<usize>, blocks: &[RuntimeBlock]) -> Result<(), (String, String)> {
            match mark[k] {
                2 => return Ok(()),
                1 => {
                    let start = stack.iter().position(|s| *s == k).unwrap_or(0);
                    let cycle: Vec<&str> = stack[start..].iter().chain(std::iter::once(&k)).map(|i| blocks[*i].decl.name.as_str()).collect();
                    return Err((blocks[k].decl.name.clone(), format!("algebraic loop through blocks {}: each answers at the same instant (feedthrough, no output delay); give one an output delay of a sample", cycle.join(" → "))));
                }
                _ => {}
            }
            mark[k] = 1;
            stack.push(k);
            for &d in &deps[k] {
                visit(d, deps, mark, order, stack, blocks)?;
            }
            stack.pop();
            mark[k] = 2;
            order.push(k);
            Ok(())
        }
        for k in 0..n {
            visit(k, &deps, &mut mark, &mut order, &mut Vec::new(), blocks)?;
        }
        Ok(order)
    }

    pub fn bind(&mut self, name: &str, implementation: Box<dyn BlockImplementation>) -> Result<(), String> {
        let block = self.blocks.iter_mut().find(|b| b.decl.name == name).ok_or_else(|| format!("no block named `{name}`"))?;
        let offered = implementation.interface();
        let declared = &block.decl.interface;
        let signals = |ports: &[sim_core::BlockPort]| ports.iter().map(|p| (p.name.clone(), p.kind.clone())).collect::<Vec<_>>();
        if signals(&offered.inputs) != signals(&declared.inputs) || signals(&offered.outputs) != signals(&declared.outputs) {
            return Err(format!("block `{name}`: {} offers inputs {:?} and outputs {:?}, but the block declares inputs {:?} and outputs {:?}", implementation.label(), signals(&offered.inputs), signals(&offered.outputs), signals(&declared.inputs), signals(&declared.outputs)));
        }
        if offered.feedthrough != declared.feedthrough {
            return Err(format!("block `{name}`: {} has feedthrough {}, the block declares {}", implementation.label(), offered.feedthrough, declared.feedthrough));
        }
        if let Some(old) = block.implementation.take() {
            exclusive(old).terminate();
        }
        block.implementation = Some(Mutex::new(implementation));
        block.terminated = false;
        Ok(())
    }

    /// The earliest tick at or after `time` (None without blocks).
    pub fn next_tick(&self) -> Option<f64> {
        self.blocks.iter().filter_map(|b| b.decl.timing.clock.tick(self.t0, b.next)).min_by(f64::total_cmp)
    }

    /// Whether block `k` ticks at `time`.
    fn due(&self, k: usize, time: f64) -> bool {
        let b = &self.blocks[k];
        b.decl.timing.clock.tick(self.t0, b.next).is_some_and(|tick| tick <= time + roundoff(time, tick, b.decl.timing.clock.nominal_period()))
    }

    /// Run the blocks due at `time`: `read` gives a committed plant value;
    /// returns the held states to write (one batch).
    pub fn tick(&mut self, time: f64, read: &dyn Fn(StateId) -> f64) -> Result<Vec<(StateId, f64)>, BlockFault> {
        if let Some(f) = &self.fault {
            return Err(f.clone());
        }
        let due: Vec<usize> = self.order.iter().copied().filter(|&k| self.due(k, time)).collect();
        if due.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(&k) = due.iter().find(|&&k| self.blocks[k].implementation.is_none()) {
            let b = &self.blocks[k];
            let f = BlockFault { block: b.decl.name.clone(), time, message: format!("no implementation is bound ({})", b.decl.implementation.describe()) };
            self.fault = Some(f.clone());
            return Err(f);
        }
        // Plant inputs: the committed state before any write at this instant.
        let plant: Vec<Vec<Option<f64>>> = self.blocks.iter().map(|b| b.inputs.iter().map(|s| match s { Source::Plant(id) => Some(read(*id)), Source::Block { .. } => None }).collect()).collect();
        let mut changed: Vec<usize> = Vec::new();
        // 1. Every due block's output for this instant that was decided
        //    before it: a pending end-of-step output, or the delayed output
        //    of a feedthrough block (computed `output_delay` ticks ago).
        for &k in &due {
            if let Some(pending) = self.blocks[k].pending.take() {
                self.emit(k, pending);
                changed.push(k);
                continue;
            }
            let b = &mut self.blocks[k];
            if b.decl.interface.feedthrough && b.decl.timing.output_delay > 0 && b.out_fifo.len() >= b.decl.timing.output_delay {
                b.applied = b.out_fifo.pop_front().expect("nonempty");
                changed.push(k);
            }
        }
        // 2. Blocks without feedthrough at their first tick give their
        //    initial outputs (which do not depend on this instant's inputs);
        //    each sees the other blocks as they stood after step 1, so the
        //    result does not depend on the order the blocks were declared in.
        let before: Vec<Vec<f64>> = self.blocks.iter().map(|b| b.applied.clone()).collect();
        for &k in &due {
            if self.blocks[k].initialized || self.blocks[k].decl.interface.feedthrough {
                continue;
            }
            let inputs: Vec<f64> = self.blocks[k].inputs.iter().zip(&plant[k]).map(|(s, p)| match (s, p) {
                (_, Some(v)) => *v,
                (Source::Block { block, output }, None) => before[*block][*output],
                (Source::Plant(_), None) => unreachable!("plant inputs were read"),
            }).collect();
            if let Err(message) = self.initial_outputs(k, time, &inputs) {
                let f = BlockFault { block: self.blocks[k].decl.name.clone(), time, message };
                self.fault = Some(f.clone());
                return Err(f);
            }
            changed.push(k);
        }
        // 3. Execute in dependency order. From here only a feedthrough
        //    block without output delay changes its signal, and the order
        //    puts it before its consumers: every read is order-independent.
        for &k in &due {
            let inputs: Vec<f64> = self.blocks[k].inputs.iter().zip(&plant[k]).map(|(s, p)| match (s, p) {
                (_, Some(v)) => *v,
                (Source::Block { block, output }, None) => self.blocks[*block].applied[*output],
                (Source::Plant(_), None) => unreachable!("plant inputs were read"),
            }).collect();
            let result = self.execute(k, time, inputs);
            if let Err(message) = result {
                let f = BlockFault { block: self.blocks[k].decl.name.clone(), time, message };
                self.fault = Some(f.clone());
                return Err(f);
            }
            changed.push(k);
        }
        // 4. One batch of held-state writes.
        changed.sort_unstable();
        changed.dedup();
        let mut writes = Vec::new();
        for k in changed {
            let b = &self.blocks[k];
            writes.extend(b.output_ids.iter().copied().zip(b.applied.iter().copied()));
        }
        Ok(writes)
    }

    /// The first tick of a block without feedthrough: initialize it and
    /// emit its initial outputs. `sampled` is what its inputs read now (the
    /// input delay line is still empty, so that is also what it sees).
    fn initial_outputs(&mut self, k: usize, time: f64, sampled: &[f64]) -> Result<(), String> {
        let clock = &self.blocks[k].decl.timing.clock;
        let dt = clock.interval(self.blocks[k].next).unwrap_or(clock.nominal_period());
        let mut outputs = self.blocks[k].applied.clone();
        self.call(k, true, time, dt, sampled, &mut outputs)?;
        Self::validate(&self.blocks[k], &outputs, time)?;
        self.blocks[k].initialized = true;
        self.emit(k, outputs);
        Ok(())
    }

    /// One block's tick: input delay, the implementation call(s),
    /// validation, then its outputs (now, or pending for the next tick).
    fn execute(&mut self, k: usize, time: f64, sampled: Vec<f64>) -> Result<(), String> {
        let (seen, first, feedthrough, interval, nominal) = {
            let b = &mut self.blocks[k];
            // Input delay: the implementation sees the frame `input_delay`
            // samples old (before the line fills, the oldest frame there).
            let seen = if b.decl.timing.input_delay == 0 {
                sampled
            } else {
                b.in_fifo.push_back(sampled);
                if b.in_fifo.len() > b.decl.timing.input_delay { b.in_fifo.pop_front().expect("nonempty") } else { b.in_fifo.front().cloned().expect("nonempty") }
            };
            let clock = &b.decl.timing.clock;
            (seen, !b.initialized, b.decl.interface.feedthrough, clock.interval(b.next), clock.nominal_period())
        };
        {
            let b = &mut self.blocks[k];
            b.initialized = true;
            b.next += 1;
        }
        let dt = interval.unwrap_or(nominal);
        if feedthrough {
            // Feedthrough outputs are values at this tick: the signal now,
            // or after the output delay (queued; `tick` applies it).
            let mut outputs = self.blocks[k].applied.clone();
            self.call(k, first, time, dt, &seen, &mut outputs)?;
            Self::validate(&self.blocks[k], &outputs, time)?;
            let b = &mut self.blocks[k];
            if b.decl.timing.output_delay == 0 {
                b.applied = outputs;
            } else {
                b.out_fifo.push_back(outputs);
            }
            return Ok(());
        }
        debug_assert!(!first, "`tick` initializes blocks without feedthrough before executing");
        // End of step: the step from this tick (inputs held) gives the
        // values at the next tick, applied there. After a schedule's last
        // tick there is no next tick, so no step.
        if let Some(dt) = interval {
            let mut next = self.blocks[k].applied.clone();
            self.call(k, false, time, dt, &seen, &mut next)?;
            Self::validate(&self.blocks[k], &next, time + dt)?;
            self.blocks[k].pending = Some(next);
        }
        Ok(())
    }

    /// One implementation call, timed against the block's deadline.
    fn call(&mut self, k: usize, initialize: bool, time: f64, dt: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        let b = &mut self.blocks[k];
        let implementation = b.implementation.as_mut().expect("bound").get_mut().unwrap_or_else(|p| p.into_inner());
        let started = Instant::now();
        let result = if initialize { implementation.initialize(time, dt, inputs, outputs) } else { implementation.step(time, dt, inputs, outputs) };
        let elapsed = started.elapsed().as_secs_f64();
        b.calls += 1;
        result.map_err(|e| format!("{}: {e}", implementation.label()))?;
        if let Some(deadline) = b.decl.timing.deadline_s {
            if elapsed > deadline {
                return Err(format!("{} missed its deadline: the call took {:.3} ms, the budget is {:.3} ms", implementation.label(), elapsed * 1e3, deadline * 1e3));
            }
        }
        Ok(())
    }

    fn validate(b: &RuntimeBlock, outputs: &[f64], at: f64) -> Result<(), String> {
        for (value, port) in outputs.iter().zip(&b.decl.interface.outputs) {
            if !value.is_finite() {
                return Err(format!("output `{}` is {value} (for t = {at})", port.name));
            }
            if port.min.is_some_and(|m| *value < m) || port.max.is_some_and(|m| *value > m) {
                return Err(format!("output `{}` = {value} is outside its declared range [{}, {}] (for t = {at})", port.name, port.min.map_or("-∞".into(), |m| m.to_string()), port.max.map_or("∞".into(), |m| m.to_string())));
            }
        }
        Ok(())
    }

    /// An output for this instant, known before the tick's executions (a
    /// pending end-of-step output, an initial output), becomes the block's
    /// signal after the output delay.
    fn emit(&mut self, k: usize, outputs: Vec<f64>) {
        let b = &mut self.blocks[k];
        if b.decl.timing.output_delay == 0 {
            b.applied = outputs;
            return;
        }
        b.out_fifo.push_back(outputs);
        if b.out_fifo.len() > b.decl.timing.output_delay {
            b.applied = b.out_fifo.pop_front().expect("nonempty");
        }
    }

    pub fn terminate_all(&mut self) {
        for b in &mut self.blocks {
            if !b.terminated {
                if let Some(i) = b.implementation.as_mut() {
                    i.get_mut().unwrap_or_else(|p| p.into_inner()).terminate();
                }
                b.terminated = true;
            }
        }
    }

    pub fn checkpoint(&self) -> Result<Vec<BlockState>, String> {
        self.blocks.iter().map(|b| {
            let implementation = match b.implementation.as_ref().map(|i| i.lock().unwrap_or_else(|p| p.into_inner()).checkpoint()) {
                None | Some(Checkpoint::Stateless) => None,
                Some(Checkpoint::State(bytes)) => Some(bytes),
                Some(Checkpoint::Unsupported(why)) => return Err(format!("block `{}`: {why}", b.decl.name)),
            };
            Ok(BlockState { next: b.next, initialized: b.initialized, pending: b.pending.clone(), applied: b.applied.clone(), in_fifo: b.in_fifo.iter().cloned().collect(), out_fifo: b.out_fifo.iter().cloned().collect(), calls: b.calls, implementation })
        }).collect()
    }

    pub fn restore(&mut self, states: &[BlockState]) -> Result<(), String> {
        if states.len() != self.blocks.len() {
            return Err(format!("checkpoint has {} blocks, the runtime {}", states.len(), self.blocks.len()));
        }
        for (b, s) in self.blocks.iter_mut().zip(states) {
            if let Some(i) = b.implementation.as_mut() {
                let i = i.get_mut().unwrap_or_else(|p| p.into_inner());
                match &s.implementation {
                    Some(bytes) => i.restore(bytes)?,
                    None => match i.checkpoint() {
                        Checkpoint::Stateless => {}
                        _ => return Err(format!("block `{}`: the checkpoint has no state for {}", b.decl.name, i.label())),
                    },
                }
            }
            b.next = s.next;
            b.initialized = s.initialized;
            b.pending = s.pending.clone();
            b.applied = s.applied.clone();
            b.in_fifo = s.in_fifo.iter().cloned().collect();
            b.out_fifo = s.out_fifo.iter().cloned().collect();
            b.calls = s.calls;
        }
        self.fault = None;
        Ok(())
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.terminate_all();
    }
}
