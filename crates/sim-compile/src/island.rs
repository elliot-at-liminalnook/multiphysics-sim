//! From a validated model to one integrable [`System`] per island.
//!
//! Unknown layout per island: every behavior's states, then the across
//! bundle of every unowned node, then every signal output. Residual rows:
//! one per state (from the owning behavior), one per node through lane
//! (the contributions summed to zero), one per signal output (the
//! producer's value minus the unknown). Owned frame nodes alias their
//! owner's states and add no unknowns or rows.

use crate::{CompileError, CompiledConnection, CompiledConnectionKind};
use sim_core::{
    Input, LocalJacobian, Output,
    Behavior, BehaviorId, BehaviorRegistry, Context, ModelWorld, PortId, PortSchema, QuantityKind,
    StateDeclaration, StateId, View,
};
use sim_dynamics::jacobian::Sparsity;
use sim_dynamics::{JacobianParts, System};
use std::collections::{BTreeMap, HashMap};

/// Where one port reads its across bundle and pushes its through bundle.
#[derive(Debug, Clone)]
enum PortBinding {
    /// Unowned node: per-lane unknown indices (a lane may alias a providing
    /// element's state). A through lane's balance row is its across
    /// unknown's row — a fresh node unknown, or the providing element's
    /// state row, which that element leaves to the balance (its storage is
    /// expressed as through: an inertia's `J·ω̇`, a volume's `V·ρ̇`).
    Node { lanes: Vec<usize>, rows: Vec<usize>, through_width: usize },
    /// Owned frame: across aliases the owner's states starting at `states`;
    /// contributions are summed into `wrench` (an accumulator, not a row)
    /// and land on the owner's rows from `states + row_offset`.
    Owned { states: usize, wrench: usize, width: usize, through_width: usize, row_offset: usize },
    /// A single open port: through must vanish, rows exist, across is free.
    Open { lanes: Vec<usize>, rows: Vec<usize>, through_width: usize },
    /// A composite port: its members' bindings, laid out one after another.
    Composite(Vec<PortBinding>),
}

impl PortBinding {
    fn lane_indices(&self) -> Vec<usize> {
        match self {
            PortBinding::Node { lanes, .. } | PortBinding::Open { lanes, .. } => lanes.clone(),
            PortBinding::Owned { states, width, .. } => (*states..*states + *width).collect(),
            PortBinding::Composite(members) => members.iter().flat_map(|m| m.lane_indices()).collect(),
        }
    }
    /// Add this port's through contributions to the balance rows (or the
    /// owner's wrench accumulator).
    fn scatter(&self, through: &[f64], out: &mut [f64], wrenches: &mut [f64]) {
        match self {
            PortBinding::Node { rows, through_width, .. } | PortBinding::Open { rows, through_width, .. } => {
                for lane in 0..*through_width {
                    out[rows[lane]] += through[lane];
                }
            }
            PortBinding::Owned { wrench, through_width, .. } => {
                for lane in 0..*through_width {
                    wrenches[wrench + lane] += through[lane];
                }
            }
            PortBinding::Composite(members) => {
                let mut offset = 0;
                for member in members {
                    let width = member.width();
                    member.scatter(&through[offset..offset + width], out, wrenches);
                    offset += width;
                }
            }
        }
    }
    /// Rows this port's contributions land on.
    fn written_rows(&self, written: &mut Vec<usize>) {
        match self {
            PortBinding::Node { rows, through_width, .. } | PortBinding::Open { rows, through_width, .. } => {
                written.extend(rows.iter().take(*through_width).copied());
            }
            PortBinding::Owned { states, width, row_offset, .. } => {
                // Attachments write the owner's twist rows.
                written.extend(*states + *row_offset..*states + *width);
            }
            PortBinding::Composite(members) => members.iter().for_each(|m| m.written_rows(written)),
        }
    }
    fn width(&self) -> usize {
        match self {
            PortBinding::Composite(members) => members.iter().map(|m| m.width()).sum(),
            PortBinding::Node { lanes, .. } | PortBinding::Open { lanes, .. } => lanes.len(),
            PortBinding::Owned { width, .. } => *width,
        }
    }
}

#[derive(Debug, Clone)]
struct Slot {
    behavior: BehaviorId,
    state_start: usize,
    state_count: usize,
    ports: Vec<PortBinding>,
    /// Signal inputs: indices of the producing signal unknowns.
    signals_in: Vec<usize>,
    /// Signal outputs: unknown indices (rows share the index space).
    signals_out: Vec<usize>,
    /// Owned frame ports this behavior *owns*: (port index, wrench accumulator start, through width, row offset).
    owned: Vec<(usize, usize, usize, usize)>,
    /// Flat lane offsets per port, with an end marker.
    offsets: Vec<usize>,
    /// Flat lane → flat index of its exact rate lane.
    rate_map: Vec<Option<usize>>,
    /// Thermal lanes for entropy accounting: (port index, lane offset within
    /// the port — nonzero for a thermal member of a composite).
    thermal_ports: Vec<(usize, usize)>,
    guard_offset: usize,
    guard_count: usize,
}

/// Scratch for one behavior's flat port buffers.
#[derive(Default)]
struct Buffers {
    across: Vec<f64>,
    across_rates: Vec<f64>,
    through: Vec<f64>,
    signals: Vec<f64>,
    state_residuals: Vec<f64>,
    signals_out: Vec<f64>,
}

impl Slot {
    /// Fill `b` with this slot's across values, rates and signal inputs.
    fn gather(&self, x: &[f64], rate: &[f64], b: &mut Buffers) {
        let lanes = *self.offsets.last().unwrap_or(&0);
        b.across.clear();
        b.across_rates.clear();
        b.across.reserve(lanes);
        b.across_rates.reserve(lanes);
        for binding in &self.ports {
            for index in binding.lane_indices() {
                b.across.push(x[index]);
                b.across_rates.push(rate[index]);
            }
        }
        b.through.clear();
        b.through.resize(lanes, 0.0);
        b.signals.clear();
        b.signals.extend(self.signals_in.iter().map(|i| x[*i]));
        b.state_residuals.clear();
        b.state_residuals.resize(self.state_count, 0.0);
        b.signals_out.clear();
        b.signals_out.resize(self.signals_out.len(), 0.0);
    }

    fn view<'a>(&'a self, t: f64, x: &'a [f64], b: &'a Buffers) -> View<'a> {
        View { time: t, states: &x[self.state_start..self.state_start + self.state_count], offsets: &self.offsets, rate_map: &self.rate_map, across: &b.across, across_rates: &b.across_rates, signals_in: &b.signals }
    }
}

/// One compiled island: behaviors, layout, and the [`System`] they form.
/// Reusable buffers for the reduced residual.
#[derive(Debug, Default)]
struct Scratch {
    xf: Vec<f64>,
    rf: Vec<f64>,
    full: Vec<f64>,
}

/// Per-step noise draws for every behavior slot.
#[derive(Debug, Default)]
struct Noise {
    rng: u64,
    step: f64,
    draws: Vec<f64>,
}
const DRAWS_PER_SLOT: usize = 4;
impl Noise {
    fn next_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn normal(&mut self) -> f64 {
        let u1 = self.uniform().max(1.0e-300);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

pub struct Island {
    pub behaviors: Vec<(BehaviorId, Box<dyn Behavior>)>,
    slots: Vec<Slot>,
    dimension: usize,
    /// Row layout: states, then node balance rows, then signal rows.
    pub state_rows: usize,
    pub node_rows: usize,
    pub signal_rows: usize,
    wrench_count: usize,
    /// Unknown index → stable state id.
    pub state_ids: Vec<StateId>,
    /// (behavior, state name) → unknown index.
    pub behavior_states: HashMap<(BehaviorId, String), usize>,
    /// Port → unknown indices of its across lanes.
    pub port_lanes: HashMap<PortId, Vec<usize>>,
    /// Rows `x = d(base)/dt` the compiler added for unprovided rate lanes:
    /// (rate lane unknown, base lane unknown).
    derivative_rows: Vec<(usize, usize)>,
    /// Signal port → unknown index of the signal's value.
    pub port_signal: HashMap<PortId, usize>,
    pub initial: Vec<f64>,
    /// Reduced ↔ full index maps: the solver carries `full_of.len()` unknowns.
    pub full_of: Vec<usize>,
    pub reduced_of: Vec<Option<usize>>,
    signal_order: Vec<usize>,
    /// Every slot, producers before consumers: one pass computes signals
    /// and residuals together.
    eval_order: Vec<usize>,
    lane_of_rate: Vec<(usize, usize)>,
    reduced_sparsity: Sparsity,
    reduced_algebraic: Vec<bool>,
    sparsity: Sparsity,
    noise: std::sync::Mutex<Noise>,
    algebraic: Vec<bool>,
    scratch: std::sync::Mutex<Scratch>,
}

impl Island {
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Unknown index of a behavior's state by name.
    pub fn state_index(&self, behavior: BehaviorId, name: &str) -> Option<usize> {
        self.behavior_states.get(&(behavior, name.to_owned())).copied()
    }

}

impl Island {
    /// Entropy production of every behavior at `(t, x, rate)`: the entropy
    /// its thermal ports carry out minus what it declares it stores.
    fn entropy_production_full(&self, t: f64, x: &[f64], rate: &[f64]) -> Vec<f64> {
        let mut b = Buffers::default();
        self.slots
            .iter()
            .zip(&self.behaviors)
            .map(|(slot, (_, behavior))| {
                if slot.thermal_ports.is_empty() {
                    return 0.0;
                }
                slot.gather(x, rate, &mut b);
                let storage;
                {
                    let mut ctx = Context::new(
                        t,
                        &x[slot.state_start..slot.state_start + slot.state_count],
                        &rate[slot.state_start..slot.state_start + slot.state_count],
                        &slot.offsets,
                        &slot.rate_map,
                        &b.across,
                        &b.across_rates,
                        &b.signals,
                        &mut b.state_residuals,
                        &mut b.through,
                        &mut b.signals_out,
                    );
                    behavior.residual(&mut ctx);
                    storage = ctx.entropy_storage();
                }
                // Heat into the behavior at each thermal port carries Q/T in.
                let carried_in: f64 = slot.thermal_ports.iter().map(|(p, lane)| {
                    let temperature = b.across[slot.offsets[*p] + lane];
                    b.through[slot.offsets[*p] + lane] / temperature
                }).sum();
                storage - carried_in
            })
            .collect()
    }
}

impl Island {
    /// The residual over the full unknown vector (every signal and lane present).
    pub fn residual_full(&self, t: f64, x: &[f64], rate: &[f64], out: &mut [f64]) {
        out.iter_mut().for_each(|v| *v = 0.0);
        let mut wrenches = vec![0.0; self.wrench_count];
        let mut b = Buffers::default();
        let noise = self.noise.lock().unwrap();
        for (k, (slot, (_, behavior))) in self.slots.iter().zip(&self.behaviors).enumerate() {
            slot.gather(x, rate, &mut b);
            let draws: &[f64] = noise.draws.get(k * DRAWS_PER_SLOT..(k + 1) * DRAWS_PER_SLOT).unwrap_or(&[]);
            {
                let mut ctx = Context::new(
                    t,
                    &x[slot.state_start..slot.state_start + slot.state_count],
                    &rate[slot.state_start..slot.state_start + slot.state_count],
                    &slot.offsets,
                    &slot.rate_map,
                    &b.across,
                    &b.across_rates,
                    &b.signals,
                    &mut b.state_residuals,
                    &mut b.through,
                    &mut b.signals_out,
                ).with_noise(draws, noise.step);
                behavior.residual(&mut ctx);
            }
            out[slot.state_start..slot.state_start + slot.state_count].copy_from_slice(&b.state_residuals);
            for (port, binding) in slot.ports.iter().enumerate() {
                binding.scatter(&b.through[slot.offsets[port]..slot.offsets[port + 1]], out, &mut wrenches);
            }
            for (k, index) in slot.signals_out.iter().enumerate() {
                out[*index] = x[*index] - b.signals_out[k];
            }
        }
        for (lane, base) in &self.derivative_rows {
            out[*lane] = x[*lane] - rate[*base];
        }
        // Owned frames: the node balance lands on the owner's six twist rows.
        // The owner writes `m·v̇ − F_own` there (or contributes it as through
        // on its own port — both sum the same way), attachments add the
        // through *into* themselves, and the total must vanish.
        for slot in &self.slots {
            for (_, wrench, through_width, row_offset) in &slot.owned {
                let base = slot.state_start + row_offset;
                for lane in 0..*through_width {
                    out[base + lane] += wrenches[wrench + lane];
                }
            }
        }
    }

    /// `residual_full` in producer-before-consumer order, writing each
    /// producer's signal values into `x` before its consumers read them.
    fn residual_ordered(&self, t: f64, x: &mut [f64], rate: &[f64], out: &mut [f64]) {
        out.iter_mut().for_each(|v| *v = 0.0);
        let mut wrenches = vec![0.0; self.wrench_count];
        let mut b = Buffers::default();
        let noise = self.noise.lock().unwrap();
        // Signal producers first, in order (they write `x`); the rest have
        // no ordering constraint and, on a large island, run in parallel.
        let (producers, rest): (Vec<usize>, Vec<usize>) = self.eval_order.iter().copied().partition(|k| !self.slots[*k].signals_out.is_empty());
        // Off unless asked for (`SIM_PARALLEL_RESIDUAL=1`): on the ladders
        // and robots measured so far the per-evaluation overhead ate the gain.
        let parallel = rest.len() >= 64 && std::env::var_os("SIM_PARALLEL_RESIDUAL").is_some();
        let order: Vec<usize> = if parallel { producers.clone() } else { self.eval_order.clone() };
        for &k in &order {
            let (slot, (_, behavior)) = (&self.slots[k], &self.behaviors[k]);
            slot.gather(x, rate, &mut b);
            let draws: &[f64] = noise.draws.get(k * DRAWS_PER_SLOT..(k + 1) * DRAWS_PER_SLOT).unwrap_or(&[]);
            {
                let mut ctx = Context::new(
                    t,
                    &x[slot.state_start..slot.state_start + slot.state_count],
                    &rate[slot.state_start..slot.state_start + slot.state_count],
                    &slot.offsets,
                    &slot.rate_map,
                    &b.across,
                    &b.across_rates,
                    &b.signals,
                    &mut b.state_residuals,
                    &mut b.through,
                    &mut b.signals_out,
                ).with_noise(draws, noise.step);
                behavior.residual(&mut ctx);
            }
            out[slot.state_start..slot.state_start + slot.state_count].copy_from_slice(&b.state_residuals);
            for (port, binding) in slot.ports.iter().enumerate() {
                binding.scatter(&b.through[slot.offsets[port]..slot.offsets[port + 1]], out, &mut wrenches);
            }
            for (j, index) in slot.signals_out.iter().enumerate() {
                if self.reduced_of[*index].is_none() {
                    x[*index] = b.signals_out[j];
                    out[*index] = 0.0;
                } else {
                    out[*index] = x[*index] - b.signals_out[j];
                }
            }
        }
        if parallel {
            use rayon::prelude::*;
            let draws_all = &noise.draws;
            let step = noise.step;
            let n = out.len();
            let x_ref: &[f64] = x;
            // As many partial vectors as threads, not as elements: the
            // reduction is O(threads·n), not O(elements·n).
            let chunk = rest.len().div_ceil(rayon::current_num_threads().max(1)).max(1);
            let partials: Vec<(Vec<f64>, Vec<f64>)> = rest
                .par_chunks(chunk)
                .map(|chunk| {
                    let mut out_part = vec![0.0; n];
                    let mut wrench_part = vec![0.0; self.wrench_count];
                    let mut b = Buffers::default();
                    for &k in chunk {
                        let (slot, (_, behavior)) = (&self.slots[k], &self.behaviors[k]);
                        slot.gather(x_ref, rate, &mut b);
                        let draws: &[f64] = draws_all.get(k * DRAWS_PER_SLOT..(k + 1) * DRAWS_PER_SLOT).unwrap_or(&[]);
                        {
                            let mut ctx = Context::new(
                                t,
                                &x_ref[slot.state_start..slot.state_start + slot.state_count],
                                &rate[slot.state_start..slot.state_start + slot.state_count],
                                &slot.offsets,
                                &slot.rate_map,
                                &b.across,
                                &b.across_rates,
                                &b.signals,
                                &mut b.state_residuals,
                                &mut b.through,
                                &mut b.signals_out,
                            ).with_noise(draws, step);
                            behavior.residual(&mut ctx);
                        }
                        out_part[slot.state_start..slot.state_start + slot.state_count].copy_from_slice(&b.state_residuals);
                        for (port, binding) in slot.ports.iter().enumerate() {
                            binding.scatter(&b.through[slot.offsets[port]..slot.offsets[port + 1]], &mut out_part, &mut wrench_part);
                        }
                    }
                    (out_part, wrench_part)
                })
                .collect();
            for (out_part, wrench_part) in partials {
                for (o, p) in out.iter_mut().zip(&out_part) {
                    *o += p;
                }
                for (w, p) in wrenches.iter_mut().zip(&wrench_part) {
                    *w += p;
                }
            }
        }
        for (lane, base) in &self.derivative_rows {
            out[*lane] = x[*lane] - rate[*base];
        }
        // Owned frames: the node balance lands on the owner's six twist rows.
        // The owner writes `m·v̇ − F_own` there (or contributes it as through
        // on its own port — both sum the same way), attachments add the
        // through *into* themselves, and the total must vanish.
        for slot in &self.slots {
            for (_, wrench, through_width, row_offset) in &slot.owned {
                let base = slot.state_start + row_offset;
                for lane in 0..*through_width {
                    out[base + lane] += wrenches[wrench + lane];
                }
            }
        }
    }

    /// The Jacobian assembled element by element: each behavior's rows are
    /// differentiated by perturbing only its own inputs — its states, its
    /// port lanes, its signals — and re-evaluating only that behavior. The
    /// cost is the sum over elements of (inputs × one element), not the
    /// island's colour count times the whole island, and a behavior that
    /// knows its derivatives can supply them through [`Behavior::jacobian`].
    fn jacobian_full(&self, t: f64, x: &[f64], rate: &[f64], out: &mut JacobianParts) -> bool {
        let n = self.dimension;
        out.clear();
        // Where each wrench accumulator lands: the owner's twist rows.
        let mut wrench_rows = vec![0usize; self.wrench_count];
        for slot in &self.slots {
            for (_, wrench, through_width, row_offset) in &slot.owned {
                for lane in 0..*through_width {
                    wrench_rows[wrench + lane] = slot.state_start + row_offset + lane;
                }
            }
        }
        let noise = self.noise.lock().unwrap();
        // Per-slot work in parallel: each task owns its buffers and returns
        // its triplets; analytic elements scatter, the rest difference.
        let draws_all: Vec<f64> = noise.draws.clone();
        let noise_step = noise.step;
        drop(noise);
        use rayon::prelude::*;
        let slot_parts: Vec<(Vec<(usize, usize, f64)>, Vec<(usize, usize, f64)>)> = (0..self.slots.len())
            .into_par_iter()
            .map(|k| {
                let mut dx: Vec<(usize, usize, f64)> = Vec::new();
                let mut drate: Vec<(usize, usize, f64)> = Vec::new();
                let slot = &self.slots[k];
                let behavior = &self.behaviors[k].1;
                let mut b = Buffers::default();
                let mut wrenches = vec![0.0; self.wrench_count];
                let draws: &[f64] = draws_all.get(k * DRAWS_PER_SLOT..(k + 1) * DRAWS_PER_SLOT).unwrap_or(&[]);
                let eval = |x: &[f64], rate: &[f64], out: &mut [f64], b: &mut Buffers, wrenches: &mut Vec<f64>| {
                    out.iter_mut().for_each(|v| *v = 0.0);
                    wrenches.iter_mut().for_each(|v| *v = 0.0);
                    slot.gather(x, rate, b);
                    {
                        let mut ctx = Context::new(
                            t,
                            &x[slot.state_start..slot.state_start + slot.state_count],
                            &rate[slot.state_start..slot.state_start + slot.state_count],
                            &slot.offsets,
                            &slot.rate_map,
                            &b.across,
                            &b.across_rates,
                            &b.signals,
                            &mut b.state_residuals,
                            &mut b.through,
                            &mut b.signals_out,
                        ).with_noise(draws, noise_step);
                        behavior.residual(&mut ctx);
                    }
                    out[slot.state_start..slot.state_start + slot.state_count].copy_from_slice(&b.state_residuals);
                    for (port, binding) in slot.ports.iter().enumerate() {
                        binding.scatter(&b.through[slot.offsets[port]..slot.offsets[port + 1]], out, wrenches);
                    }
                    for (i, row) in wrench_rows.iter().enumerate() {
                        out[*row] += wrenches[i];
                    }
                    for (j, index) in slot.signals_out.iter().enumerate() {
                        out[*index] = x[*index] - b.signals_out[j];
                    }
                };
                // Analytic first.
                let mut local = LocalJacobian::default();
                let knows = {
                    slot.gather(x, rate, &mut b);
                    let view = View { time: t, states: &x[slot.state_start..slot.state_start + slot.state_count], offsets: &slot.offsets, rate_map: &slot.rate_map, across: &b.across, across_rates: &b.across_rates, signals_in: &b.signals };
                    behavior.jacobian(&view, &mut local)
                };
                if knows {
                    sim_solve::profile::ANALYTIC_SLOTS.count(1);
                    let lanes: Vec<Vec<usize>> = slot.ports.iter().map(|p| p.lane_indices()).collect();
                    let flat: Vec<usize> = lanes.iter().flatten().copied().collect();
                    for (output, input, value) in &local.entries {
                        let row = match *output {
                            Output::State(i) => slot.state_start + i,
                            Output::Through(port, lane) => match &slot.ports[port] {
                                PortBinding::Node { rows, .. } | PortBinding::Open { rows, .. } => rows[lane],
                                PortBinding::Owned { wrench, .. } => wrench_rows[wrench + lane],
                                PortBinding::Composite(_) => {
                                    let mut scratch = vec![0.0; n];
                                    let mut w = vec![0.0; self.wrench_count];
                                    let mut through = vec![0.0; slot.offsets[port + 1] - slot.offsets[port]];
                                    through[lane] = 1.0;
                                    slot.ports[port].scatter(&through, &mut scratch, &mut w);
                                    match scratch.iter().position(|v| *v != 0.0) {
                                        Some(r) => r,
                                        None => wrench_rows[w.iter().position(|v| *v != 0.0).unwrap_or(0)],
                                    }
                                }
                            },
                            Output::Signal(j) => slot.signals_out[j],
                        };
                        let sign = if matches!(output, Output::Signal(_)) { -1.0 } else { 1.0 };
                        match *input {
                            Input::State(j) => dx.push((row, slot.state_start + j, sign * value)),
                            Input::StateRate(j) => drate.push((row, slot.state_start + j, sign * value)),
                            Input::Across(port, lane) => dx.push((row, lanes[port][lane], sign * value)),
                            Input::AcrossRate(port, lane) => match slot.rate_map.get(slot.offsets[port] + lane).copied().flatten() {
                                Some(exact) => dx.push((row, flat[exact], sign * value)),
                                None => drate.push((row, lanes[port][lane], sign * value)),
                            },
                            Input::AcrossDerivative(port, lane) => drate.push((row, lanes[port][lane], sign * value)),
                            Input::Signal(j) => dx.push((row, slot.signals_in[j], sign * value)),
                        }
                    }
                    for index in &slot.signals_out {
                        dx.push((*index, *index, 1.0));
                    }
                    return (dx, drate);
                }
                sim_solve::profile::FD_SLOTS.count(1);
                let mut inputs: Vec<usize> = (slot.state_start..slot.state_start + slot.state_count).collect();
                for binding in &slot.ports {
                    inputs.extend(binding.lane_indices());
                }
                inputs.extend(slot.signals_in.iter().copied());
                inputs.extend(slot.signals_out.iter().copied());
                inputs.sort_unstable();
                inputs.dedup();
                let mut base = vec![0.0; n];
                let mut perturbed = vec![0.0; n];
                let mut xp = x.to_vec();
                let mut rp = rate.to_vec();
                eval(x, rate, &mut base, &mut b, &mut wrenches);
                for &col in &inputs {
                    let rows = &self.sparsity.rows[col];
                    let eps = 1.0e-6 * (1.0 + x[col].abs());
                    xp[col] = x[col] + eps;
                    eval(&xp, rate, &mut perturbed, &mut b, &mut wrenches);
                    xp[col] = x[col];
                    for &row in rows {
                        let v = (perturbed[row] - base[row]) / eps;
                        if v != 0.0 {
                            dx.push((row, col, v));
                        }
                    }
                    if !self.algebraic[col] {
                        let eps = 1.0e-6 * (1.0 + rate[col].abs());
                        rp[col] = rate[col] + eps;
                        eval(x, &rp, &mut perturbed, &mut b, &mut wrenches);
                        rp[col] = rate[col];
                        for &row in rows {
                            let v = (perturbed[row] - base[row]) / eps;
                            if v != 0.0 {
                                drate.push((row, col, v));
                            }
                        }
                    }
                }
                (dx, drate)
            })
            .collect();
        for (dx, drate) in slot_parts {
            out.d_dx.extend(dx);
            out.d_drate.extend(drate);
        }
        for (lane, base_lane) in &self.derivative_rows {
            out.dx(*lane, *lane, 1.0);
            out.drate(*lane, *base_lane, -1.0);
        }
        true
    }

    fn energy_full(&self, t: f64, x: &[f64]) -> Option<f64> {
        let rate = vec![0.0; x.len()];
        let mut b = Buffers::default();
        let mut total = 0.0;
        for (slot, (_, behavior)) in self.slots.iter().zip(&self.behaviors) {
            slot.gather(x, &rate, &mut b);
            total += behavior.energy(&slot.view(t, x, &b));
        }
        Some(total)
    }

    fn guards_full(&self, t: f64, x: &[f64], guards: &mut Vec<f64>) {
        let rate = vec![0.0; x.len()];
        let mut b = Buffers::default();
        for (slot, (_, behavior)) in self.slots.iter().zip(&self.behaviors) {
            slot.gather(x, &rate, &mut b);
            let before = guards.len();
            behavior.guards(&slot.view(t, x, &b), guards);
            debug_assert_eq!(guards.len() - before, slot.guard_count, "guard count must be constant");
        }
    }

    fn begin_step_full(&self, h: f64) {
        let mut noise = self.noise.lock().unwrap();
        noise.step = h;
        let count = self.slots.len() * DRAWS_PER_SLOT;
        noise.draws.clear();
        for _ in 0..count {
            let v = noise.normal();
            noise.draws.push(v);
        }
    }

    fn seed_noise_full(&mut self, seed: u64) {
        let mut noise = self.noise.lock().unwrap();
        noise.rng = seed ^ 0x9E37_79B9_7F4A_7C15 | 1;
        noise.draws.clear();
    }

    fn branches_full(&self, t: f64, x: &[f64]) -> Vec<Vec<f64>> {
        let rate = vec![0.0; x.len()];
        let mut b = Buffers::default();
        let mut out = Vec::new();
        for (slot, (_, behavior)) in self.slots.iter().zip(&self.behaviors) {
            slot.gather(x, &rate, &mut b);
            let mut own = Vec::new();
            behavior.branches(&slot.view(t, x, &b), &mut own);
            for branch in own {
                debug_assert_eq!(branch.states.len(), slot.state_count, "a branch must set every state of its element");
                let mut full = x.to_vec();
                full[slot.state_start..slot.state_start + slot.state_count].copy_from_slice(&branch.states);
                for (port, lane, value) in branch.across {
                    if let Some(index) = slot.ports.get(port).and_then(|p| p.lane_indices().get(lane).copied()) {
                        full[index] = value;
                    }
                }
                out.push(full);
            }
        }
        out
    }

    fn jump_full(&mut self, index: usize, t: f64, x: &mut [f64]) {
        let Some(k) = self.slots.iter().position(|s| index >= s.guard_offset && index < s.guard_offset + s.guard_count) else { return };
        let rate = vec![0.0; x.len()];
        let mut b = Buffers::default();
        let slot = &self.slots[k];
        slot.gather(x, &rate, &mut b);
        let mut states = x[slot.state_start..slot.state_start + slot.state_count].to_vec();
        {
            let snapshot = states.clone();
            let view = View { time: t, states: &snapshot, offsets: &slot.offsets, rate_map: &slot.rate_map, across: &b.across, across_rates: &b.across_rates, signals_in: &b.signals };
            self.behaviors[k].1.jump(index - slot.guard_offset, &view, &mut states);
        }
        x[slot.state_start..slot.state_start + slot.state_count].copy_from_slice(&states);
        // The element's signal outputs follow its states at once: a sampler
        // that fires in the same instant must read the new value, not the
        // unknown the last solve left behind.
        if !slot.signals_out.is_empty() {
            slot.gather(x, &rate, &mut b);
            let mut ctx = Context::new(
                t,
                &x[slot.state_start..slot.state_start + slot.state_count],
                &rate[slot.state_start..slot.state_start + slot.state_count],
                &slot.offsets,
                &slot.rate_map,
                &b.across,
                &b.across_rates,
                &b.signals,
                &mut b.state_residuals,
                &mut b.through,
                &mut b.signals_out,
            );
            self.behaviors[k].1.residual(&mut ctx);
            drop(ctx);
            for (j, index) in slot.signals_out.iter().enumerate() {
                x[*index] = b.signals_out[j];
            }
        }
    }

}

impl Island {
    /// The reduction: which full unknowns the solver carries, which it
    /// derives. Signals are computed from their producers in dependency
    /// order (unless the signal graph has a cycle, in which case they all
    /// stay unknowns); a rate lane nobody provides and nobody
    /// differentiates is its base lane's rate.
    fn reduce(&mut self) {
        let n = self.dimension;
        let mut eliminated = vec![false; n];
        // Producers and the slot graph over signal edges.
        let mut producer: HashMap<usize, usize> = HashMap::new();
        for (k, slot) in self.slots.iter().enumerate() {
            for index in &slot.signals_out {
                producer.insert(*index, k);
            }
        }
        let slots = self.slots.len();
        let mut indegree = vec![0usize; slots];
        let mut consumers: Vec<Vec<usize>> = vec![Vec::new(); slots];
        for (k, slot) in self.slots.iter().enumerate() {
            for index in &slot.signals_in {
                if let Some(p) = producer.get(index) {
                    if *p != k {
                        consumers[*p].push(k);
                        indegree[k] += 1;
                    } else {
                        indegree[k] += 1; // a self-loop: a cycle
                    }
                }
            }
        }
        let mut order = Vec::new();
        let mut ready: Vec<usize> = (0..slots).filter(|k| indegree[*k] == 0).collect();
        while let Some(k) = ready.pop() {
            order.push(k);
            for c in &consumers[k] {
                indegree[*c] -= 1;
                if indegree[*c] == 0 {
                    ready.push(*c);
                }
            }
        }
        let acyclic = order.len() == slots;
        self.eval_order = if acyclic { order.clone() } else { (0..slots).collect() };
        // Opt-in for now: with elimination on, plate 18's linearisation
        // picks a different eigenvalue (the pencil differs in a way not yet
        // understood), so the default keeps every unknown. Enable with
        // `sim_compile::set_elimination(true)` or `SIM_REDUCE=1`.
        let disabled = !(crate::elimination_enabled() || std::env::var_os("SIM_REDUCE").is_some()) || std::env::var_os("SIM_NO_REDUCE").is_some();
        if acyclic && !disabled {
            self.signal_order = order.into_iter().filter(|k| !self.slots[*k].signals_out.is_empty()).collect();
            for slot in &self.slots {
                for index in &slot.signals_out {
                    eliminated[*index] = true;
                }
            }
        } else {
            self.signal_order.clear();
        }
        // Rate lanes: eliminated when nobody reads their own rate.
        self.lane_of_rate.clear();
        for (lane, base) in &self.derivative_rows {
            if !disabled && self.algebraic[*lane] && !eliminated[*base] && !eliminated[*lane] {
                eliminated[*lane] = true;
                self.lane_of_rate.push((*lane, *base));
            }
        }
        self.full_of = (0..n).filter(|i| !eliminated[*i]).collect();
        self.reduced_of = vec![None; n];
        for (r, f) in self.full_of.iter().enumerate() {
            self.reduced_of[*f] = Some(r);
        }
        // What each eliminated unknown depends on, as reduced columns.
        let mut deps: HashMap<usize, Vec<usize>> = HashMap::new();
        for (lane, base) in &self.lane_of_rate {
            deps.insert(*lane, vec![self.reduced_of[*base].expect("base kept")]);
        }
        for k in &self.signal_order {
            let slot = &self.slots[*k];
            let mut inputs: Vec<usize> = (slot.state_start..slot.state_start + slot.state_count).collect();
            for binding in &slot.ports {
                inputs.extend(binding.lane_indices());
            }
            inputs.extend(slot.signals_in.iter().copied());
            let mut cols: Vec<usize> = Vec::new();
            for c in inputs {
                match self.reduced_of[c] {
                    Some(r) => cols.push(r),
                    None => cols.extend(deps.get(&c).cloned().unwrap_or_default()),
                }
            }
            cols.sort_unstable();
            cols.dedup();
            for index in &slot.signals_out {
                deps.insert(*index, cols.clone());
            }
        }
        let m = self.full_of.len();
        let mut rows: Vec<Vec<usize>> = vec![Vec::new(); m];
        let kept_rows = |full_rows: &Vec<usize>, reduced_of: &Vec<Option<usize>>| -> Vec<usize> { full_rows.iter().filter_map(|r| reduced_of[*r]).collect() };
        for (r, f) in self.full_of.iter().enumerate() {
            rows[r] = kept_rows(&self.sparsity.rows[*f], &self.reduced_of);
        }
        for (e, cols) in &deps {
            let touched = kept_rows(&self.sparsity.rows[*e], &self.reduced_of);
            for c in cols {
                for row in &touched {
                    if !rows[*c].contains(row) {
                        rows[*c].push(*row);
                    }
                }
            }
        }
        self.reduced_sparsity = Sparsity::new(rows);
        self.reduced_algebraic = self.full_of.iter().map(|f| self.algebraic[*f]).collect();
    }

    /// Reduced initial vector.
    pub fn reduced_initial(&self) -> Vec<f64> {
        self.full_of.iter().map(|f| self.initial[*f]).collect()
    }

    pub fn reduced_dimension(&self) -> usize {
        self.full_of.len()
    }

    /// The full unknown vector and rate from the reduced ones: kept
    /// unknowns copied, rate lanes from their base's rate, signals from
    /// their producers in dependency order.
    pub fn expand_at(&self, t: f64, x: &[f64], rate: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let n = self.dimension;
        let mut xf = vec![0.0; n];
        let mut rf = vec![0.0; n];
        self.expand_into(t, x, rate, &mut xf, &mut rf);
        (xf, rf)
    }

    /// `expand_at` into caller-owned buffers (no allocation).
    pub fn expand_into(&self, t: f64, x: &[f64], rate: &[f64], xf: &mut Vec<f64>, rf: &mut Vec<f64>) {
        let n = self.dimension;
        xf.clear();
        xf.resize(n, 0.0);
        rf.clear();
        rf.resize(n, 0.0);
        for (r, f) in self.full_of.iter().enumerate() {
            xf[*f] = x[r];
            rf[*f] = rate[r];
        }
        for (lane, base) in &self.lane_of_rate {
            xf[*lane] = rf[*base];
        }
        if !self.signal_order.is_empty() {
            let mut b = Buffers::default();
            let noise = self.noise.lock().unwrap();
            for k in &self.signal_order {
                let slot = &self.slots[*k];
                slot.gather(&xf, &rf, &mut b);
                let draws: &[f64] = noise.draws.get(k * DRAWS_PER_SLOT..(k + 1) * DRAWS_PER_SLOT).unwrap_or(&[]);
                {
                    let mut ctx = Context::new(
                        t,
                        &xf[slot.state_start..slot.state_start + slot.state_count],
                        &rf[slot.state_start..slot.state_start + slot.state_count],
                        &slot.offsets,
                        &slot.rate_map,
                        &b.across,
                        &b.across_rates,
                        &b.signals,
                        &mut b.state_residuals,
                        &mut b.through,
                        &mut b.signals_out,
                    ).with_noise(draws, noise.step);
                    self.behaviors[*k].1.residual(&mut ctx);
                }
                for (j, index) in slot.signals_out.iter().enumerate() {
                    xf[*index] = b.signals_out[j];
                }
            }
        }
    }

    /// `expand_at` with the island's own notion of now (signals are
    /// time-independent except through their inputs).
    pub fn expand(&self, x: &[f64], rate: &[f64]) -> (Vec<f64>, Vec<f64>) {
        self.expand_at(0.0, x, rate)
    }

    fn reduce_vector(&self, full: &[f64]) -> Vec<f64> {
        self.full_of.iter().map(|f| full[*f]).collect()
    }

    /// Entropy production per behavior from the reduced state.
    pub fn entropy_production(&self, t: f64, x: &[f64], rate: &[f64]) -> Vec<f64> {
        let (xf, rf) = self.expand_at(t, x, rate);
        self.entropy_production_full(t, &xf, &rf)
    }
}

impl System for Island {
    fn dimension(&self) -> usize {
        self.full_of.len()
    }

    fn residual(&self, t: f64, x: &[f64], rate: &[f64], out: &mut [f64]) {
        // Scratch is reused across the thousands of evaluations a step
        // takes; the lock is uncontended.
        let mut scratch = self.scratch.lock().unwrap_or_else(|p| p.into_inner());
        let Scratch { xf, rf, full } = &mut *scratch;
        // Kept unknowns and derived lanes first; signals are filled in as
        // their producers are evaluated, so one pass does everything.
        let n = self.dimension;
        xf.clear();
        xf.resize(n, 0.0);
        rf.clear();
        rf.resize(n, 0.0);
        for (r, f) in self.full_of.iter().enumerate() {
            xf[*f] = x[r];
            rf[*f] = rate[r];
        }
        for (lane, base) in &self.lane_of_rate {
            xf[*lane] = rf[*base];
        }
        full.clear();
        full.resize(n, 0.0);
        // Two passes (expand, then the full residual) are the reference;
        // the single ordered pass is faster but still has a bug that
        // breaks the geyser, so it is opt-in (`SIM_ORDERED_RESIDUAL=1`).
        if std::env::var_os("SIM_ORDERED_RESIDUAL").is_some() {
            self.residual_ordered(t, xf, rf, full);
        } else {
            self.expand_into(t, x, rate, xf, rf);
            self.residual_full(t, xf, rf, full);
        }
        for (r, f) in self.full_of.iter().enumerate() {
            out[r] = full[*f];
        }
    }

    fn jacobian(&self, t: f64, x: &[f64], rate: &[f64], out: &mut JacobianParts) -> bool {
        let (xf, rf) = self.expand_at(t, x, rate);
        let mut full = JacobianParts::default();
        if !self.jacobian_full(t, &xf, &rf, &mut full) {
            return false;
        }
        out.clear();
        // Gradients of eliminated unknowns as `(reduced col, ∂/∂x, ∂/∂rate)`.
        let mut gradient: HashMap<usize, Vec<(usize, f64, f64)>> = HashMap::new();
        for (lane, base) in &self.lane_of_rate {
            gradient.insert(*lane, vec![(self.reduced_of[*base].expect("base kept"), 0.0, 1.0)]);
        }
        // Signal rows read `x[s] − signal(inputs)`: the signal's gradient is
        // minus the row's other entries, resolved in producer order.
        let mut row_dx: HashMap<usize, Vec<(usize, f64)>> = HashMap::new();
        let mut row_drate: HashMap<usize, Vec<(usize, f64)>> = HashMap::new();
        for (r, c, v) in &full.d_dx {
            if self.reduced_of[*r].is_none() && self.lane_of_rate.iter().all(|(l, _)| l != r) {
                row_dx.entry(*r).or_default().push((*c, *v));
            }
        }
        for (r, c, v) in &full.d_drate {
            if self.reduced_of[*r].is_none() && self.lane_of_rate.iter().all(|(l, _)| l != r) {
                row_drate.entry(*r).or_default().push((*c, *v));
            }
        }
        for k in &self.signal_order {
            for s in &self.slots[*k].signals_out {
                let mut g: Vec<(usize, f64, f64)> = Vec::new();
                for (c, v) in row_dx.get(s).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if c == s {
                        continue;
                    }
                    match self.reduced_of[*c] {
                        Some(rc) => g.push((rc, -v, 0.0)),
                        None => {
                            for (rc, a, b) in gradient.get(c).map(|v| v.as_slice()).unwrap_or(&[]) {
                                g.push((*rc, -v * a, -v * b));
                            }
                        }
                    }
                }
                for (c, v) in row_drate.get(s).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if let Some(rc) = self.reduced_of[*c] {
                        g.push((rc, 0.0, -v));
                    }
                }
                gradient.insert(*s, g);
            }
        }
        for (r, c, v) in &full.d_dx {
            let Some(rr) = self.reduced_of[*r] else { continue };
            match self.reduced_of[*c] {
                Some(rc) => out.dx(rr, rc, *v),
                None => {
                    for (rc, a, b) in gradient.get(c).map(|v| v.as_slice()).unwrap_or(&[]) {
                        out.dx(rr, *rc, v * a);
                        out.drate(rr, *rc, v * b);
                    }
                }
            }
        }
        for (r, c, v) in &full.d_drate {
            let Some(rr) = self.reduced_of[*r] else { continue };
            if let Some(rc) = self.reduced_of[*c] {
                out.drate(rr, rc, *v);
            }
        }
        true
    }

    fn energy(&self, t: f64, x: &[f64]) -> Option<f64> {
        let mut scratch = self.scratch.lock().unwrap_or_else(|p| p.into_inner());
        let Scratch { xf, rf, full } = &mut *scratch;
        full.clear();
        full.resize(x.len(), 0.0);
        self.expand_into(t, x, full, xf, rf);
        self.energy_full(t, xf)
    }

    fn guards(&self, t: f64, x: &[f64], guards: &mut Vec<f64>) {
        let mut scratch = self.scratch.lock().unwrap_or_else(|p| p.into_inner());
        let Scratch { xf, rf, full } = &mut *scratch;
        full.clear();
        full.resize(x.len(), 0.0);
        self.expand_into(t, x, full, xf, rf);
        self.guards_full(t, xf, guards)
    }

    fn begin_step(&self, h: f64) {
        self.begin_step_full(h)
    }

    fn seed_noise(&mut self, seed: u64) {
        self.seed_noise_full(seed)
    }

    fn branches(&self, t: f64, x: &[f64]) -> Vec<Vec<f64>> {
        let zero = vec![0.0; x.len()];
        let (xf, _) = self.expand_at(t, x, &zero);
        self.branches_full(t, &xf).into_iter().map(|full| self.reduce_vector(&full)).collect()
    }

    fn jump(&mut self, index: usize, t: f64, x: &mut [f64]) {
        let zero = vec![0.0; x.len()];
        let (mut xf, _) = self.expand_at(t, x, &zero);
        self.jump_full(index, t, &mut xf);
        let reduced = self.reduce_vector(&xf);
        x.copy_from_slice(&reduced);
    }

    fn sparsity(&self) -> Option<Sparsity> {
        Some(self.reduced_sparsity.clone())
    }

    fn algebraic(&self) -> Option<Vec<bool>> {
        Some(self.reduced_algebraic.clone())
    }
}


/// A behavior's acausal ports in binding order: fixed ports as declared,
/// a family's members sorted by name.
fn acausal_ports_in_order(model: &ModelWorld, descriptor: &sim_core::BehaviorDescriptor, id: BehaviorId) -> Vec<PortId> {
    let mut out = Vec::new();
    for declared in &descriptor.ports {
        if !matches!(declared.schema, PortSchema::Acausal(_)) {
            continue;
        }
        if declared.name.contains('*') {
            let mut members: Vec<(String, PortId)> = model.ports.iter().filter(|(_, p)| p.owner == id && declared.matches(&p.name)).map(|(pid, p)| (p.name.clone(), pid)).collect();
            members.sort();
            out.extend(members.into_iter().map(|(_, p)| p));
        } else if let Some((pid, _)) = model.ports.iter().find(|(_, p)| p.owner == id && p.name == declared.name) {
            out.push(pid);
        }
    }
    out
}

/// Compile every behavior of `model` into islands: connected components over
/// acausal *and* signal connections.
pub fn build_islands(
    model: &mut ModelWorld,
    registry: &BehaviorRegistry,
    connections: &[CompiledConnection],
) -> Result<Vec<Island>, CompileError> {
    // Union–find over behaviors through every connection.
    let behavior_ids: Vec<BehaviorId> = model.behaviors.keys().collect();
    let index_of: HashMap<BehaviorId, usize> = behavior_ids.iter().enumerate().map(|(i, b)| (*b, i)).collect();
    let mut parent: Vec<usize> = (0..behavior_ids.len()).collect();
    fn find(p: &mut Vec<usize>, i: usize) -> usize {
        if p[i] != i {
            let r = find(p, p[i]);
            p[i] = r;
        }
        p[i]
    }
    for connection in connections {
        let owners: Vec<usize> = connection.ports.iter().map(|p| index_of[&model.ports[*p].owner]).collect();
        for pair in owners.windows(2) {
            let (a, b) = (find(&mut parent, pair[0]), find(&mut parent, pair[1]));
            if a != b {
                parent[a] = b;
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<BehaviorId>> = BTreeMap::new();
    for (i, id) in behavior_ids.iter().enumerate() {
        groups.entry(find(&mut parent, i)).or_default().push(*id);
    }
    groups.into_values().map(|members| build_island(model, registry, connections, &members)).collect()
}

fn build_island(
    model: &mut ModelWorld,
    registry: &BehaviorRegistry,
    connections: &[CompiledConnection],
    members: &[BehaviorId],
) -> Result<Island, CompileError> {
    let member_set: std::collections::HashSet<BehaviorId> = members.iter().copied().collect();
    // Instantiate equations.
    let mut behaviors: Vec<(BehaviorId, Box<dyn Behavior>)> = Vec::new();
    let mut declarations: Vec<Vec<StateDeclaration>> = Vec::new();
    for id in members {
        let instance = &model.behaviors[*id];
        let descriptor = registry.get(&instance.kind).map_err(|_| CompileError::UnregisteredBehavior { behavior: *id, kind: instance.kind.0.clone() })?;
        let equations = descriptor.equations.ok_or_else(|| CompileError::NoEquations { behavior: *id, kind: instance.kind.0.clone() })?;
        descriptor.validate_parameters(&model.parameters_of(*id)).map_err(|e| CompileError::Equations { behavior: *id, message: e.to_string() })?;
        let behavior = equations(&model.parameters_of(*id)).map_err(|e| CompileError::Equations { behavior: *id, message: e.to_string() })?;
        declarations.push(behavior.states());
        behaviors.push((*id, behavior));
    }
    // Layout: states.
    let mut dimension = 0;
    let mut slots: Vec<Slot> = Vec::new();
    let mut initial = Vec::new();
    let mut names: Vec<(String, QuantityKind)> = Vec::new();
    let mut behavior_states = HashMap::new();
    for ((id, _), decls) in behaviors.iter().zip(&declarations) {
        let start = dimension;
        for (k, d) in decls.iter().enumerate() {
            behavior_states.insert((*id, d.name.clone()), start + k);
            initial.push(d.initial);
            names.push((format!("{}.{}", model.objects[model.behaviors[*id].object].name, d.name), d.kind));
        }
        dimension += decls.len();
        slots.push(Slot { behavior: *id, state_start: start, state_count: decls.len(), ports: Vec::new(), signals_in: Vec::new(), signals_out: Vec::new(), owned: Vec::new(), offsets: vec![0], rate_map: Vec::new(), thermal_ports: Vec::new(), guard_offset: 0, guard_count: 0 });
    }
    let state_rows = dimension;
    // Providers: (port id, lane) → unknown index of the providing state.
    // Pins: (port id, lane) → the value an element holds that lane at.
    let mut provided: HashMap<(PortId, usize), usize> = HashMap::new();
    let mut pinned: HashMap<(PortId, usize), f64> = HashMap::new();
    let mut frame_owners = std::collections::HashSet::new();
    let mut acausal_counts = HashMap::<BehaviorId, usize>::new();
    for (_, port) in model.ports.iter().filter(|(_, p)| p.members.is_empty() && matches!(p.schema, PortSchema::Acausal(_))) {
        *acausal_counts.entry(port.owner).or_default() += 1;
    }
    for ((id, behavior), slot) in behaviors.iter().zip(&slots) {
        let descriptor = registry.get(&model.behaviors[*id].kind).unwrap();
        let acausal: Vec<PortId> = acausal_ports_in_order(model, descriptor, *id);
        if let Some(index) = behavior.owned_frame() {
            let port = acausal.get(index).copied().ok_or_else(|| CompileError::Equations {
                behavior: *id, message: "owned frame index is not an acausal port".into() })?;
            if !matches!(model.ports[port].schema, PortSchema::Acausal(kind) if kind.is_owned()) {
                return Err(CompileError::Equations { behavior: *id, message: "owned frame must use a Frame or PlanarFrame connector".into() });
            }
            frame_owners.insert(port);
        }
        let port_of = |index: usize| acausal[index];
        for provision in behavior.provides() {
            provided.insert((port_of(provision.port), provision.lane), slot.state_start + provision.state);
        }
        for (port, lane, value) in behavior.pinned() {
            let pid = port_of(port);
            // A composite port's pin lands on the member that holds the lane.
            let target = if let PortSchema::Acausal(sim_core::ConnectorKind::Composite(members)) = model.ports[pid].schema {
                let mut offset = 0;
                let mut found = (pid, lane);
                for (k, member) in members.iter().enumerate() {
                    if lane < offset + member.across_width() {
                        found = (model.ports[pid].members[k], lane - offset);
                        break;
                    }
                    offset += member.across_width();
                }
                found
            } else {
                (pid, lane)
            };
            pinned.insert(target, value);
        }
    }
    // Nodes: one per acausal connection among members.
    let mut port_binding: HashMap<PortId, PortBinding> = HashMap::new();
    let mut port_lanes: HashMap<PortId, Vec<usize>> = HashMap::new();
    let mut derivative_rows: Vec<(usize, usize)> = Vec::new();
    let mut node_rows = 0;
    let mut wrench_count = 0;
    let mut owned_ports: Vec<(PortId, usize, usize, usize)> = Vec::new();
    for connection in connections {
        let CompiledConnectionKind::Acausal(kind) = connection.kind else { continue };
        if !connection.ports.iter().any(|p| member_set.contains(&model.ports[*p].owner)) {
            continue;
        }
        let width = kind.across_width();
        let through_width = kind.through_width();
        if kind.is_owned() {
            let owners: Vec<_> = connection.ports.iter().filter(|p| frame_owners.contains(p)).copied().collect();
            if owners.len() != 1 {
                let ports = connection.ports.iter().map(|p| {
                    let port = &model.ports[*p];
                    format!("{}.{}", model.objects[model.behaviors[port.owner].object].name, port.name)
                }).collect::<Vec<_>>().join(", ");
                return Err(CompileError::Equations { behavior: model.ports[connection.ports[0]].owner,
                    message: format!("frame connection [{ports}] requires exactly one frame owner; found {}", owners.len()) });
            }
            // Ownership is declared by the equations, never by connection order.
            let owner_port = owners[0];
            let owner = model.ports[owner_port].owner;
            let slot = slots.iter().position(|s| s.behavior == owner).unwrap();
            let states = slots[slot].state_start;
            if slots[slot].state_count < width {
                return Err(CompileError::Equations { behavior: owner, message: format!("frame owner must declare at least {width} states") });
            }
            for (lane_index, lane) in kind.lanes().iter().enumerate() {
                if let Some(value) = node_initial(model, connection, lane_index, lane.across, lane.across_kind, &pinned, &acausal_counts)? {
                    initial[states + lane_index] = value;
                }
            }
            if kind == sim_core::ConnectorKind::Frame {
                let q = &initial[states + 3..states + 7];
                let norm_squared = q.iter().map(|value| value * value).sum::<f64>();
                if !norm_squared.is_finite() || (norm_squared - 1.).abs() > 1e-9 {
                    let ports = connection.ports.iter().map(|id| {
                        let port = &model.ports[*id];
                        format!("{}.{}", model.objects[model.behaviors[port.owner].object].name, port.name)
                    }).collect::<Vec<_>>().join(", ");
                    return Err(CompileError::Equations { behavior: owner,
                        message: format!("frame connection [{ports}] has invalid initial quaternion {q:?}; qw/qx/qy/qz must have unit length after resolving connected initial values") });
                }
            }
            let wrench = wrench_count;
            wrench_count += through_width;
            let row_offset = kind.owned_wrench_offset();
            owned_ports.push((owner_port, wrench, through_width, row_offset));
            for port in &connection.ports {
                port_binding.insert(*port, PortBinding::Owned { states, wrench, width, through_width, row_offset });
                port_lanes.insert(*port, (states..states + width).collect());
            }
        } else {
            let lanes_meta = kind.lanes();
            let mut lane_index = vec![0usize; width];
            for (l, lane) in lanes_meta.iter().enumerate().take(width) {
                let specified = node_initial(model, connection, l, lane.across, lane.across_kind, &pinned, &acausal_counts)?;
                let providers: Vec<usize> = connection.ports.iter().filter_map(|p| provided.get(&(*p, l)).copied()).collect();
                if providers.len() > 1 {
                    return Err(CompileError::Equations { behavior: model.ports[connection.ports[0]].owner, message: format!("lane `{}` has more than one provider on one node", lane.across) });
                }
                if let Some(state) = providers.first() {
                    lane_index[l] = *state;
                    if let Some(value) = specified { initial[*state] = value; }
                    continue;
                }
                lane_index[l] = dimension;
                dimension += 1;
                initial.push(specified.unwrap_or(0.));
                names.push((format!("node.{}", lane.across), lane.across_kind));
            }
            node_rows += through_width;
            debug_assert!(lanes_meta.iter().take(through_width).all(|l| l.through != "-"));
            // Each through lane balances on its own across unknown's row.
            let rows: Vec<usize> = lane_index[..through_width].to_vec();
            for (l, lane) in lanes_meta.iter().enumerate().take(width) {
                if let Some(base) = lane.derivative_of {
                    if !connection.ports.iter().any(|p| provided.contains_key(&(*p, l))) {
                        derivative_rows.push((lane_index[l], lane_index[base]));
                    }
                }
            }
            let binding = if connection.ports.len() == 1 {
                PortBinding::Open { lanes: lane_index.clone(), rows: rows.clone(), through_width }
            } else {
                PortBinding::Node { lanes: lane_index.clone(), rows: rows.clone(), through_width }
            };
            for port in &connection.ports {
                port_binding.insert(*port, binding.clone());
                port_lanes.insert(*port, lane_index.clone());
            }
        }
    }
    // Node rows come after state rows, then signal rows; fix up signal indices.
    let mut signal_rows = 0;
    let mut signal_index: HashMap<PortId, usize> = HashMap::new();
    for connection in connections {
        let CompiledConnectionKind::Signal(kind) = connection.kind else { continue };
        if !connection.ports.iter().any(|p| member_set.contains(&model.ports[*p].owner)) {
            continue;
        }
        let producer = connection.ports.iter().find(|p| matches!(model.ports[**p].schema, PortSchema::SignalOut(_))).copied().unwrap();
        let index = dimension;
        dimension += 1;
        signal_rows += 1;
        initial.push(0.0);
        names.push((format!("signal.{}", model.ports[producer].name), kind));
        for port in &connection.ports {
            signal_index.insert(*port, index);
        }
    }
    let _ = (state_rows, node_rows, signal_rows);
    // Bind ports per slot in descriptor order.
    let mut guard_offset = 0;
    for (slot, (id, behavior)) in slots.iter_mut().zip(&behaviors) {
        let descriptor = registry.get(&model.behaviors[*id].kind).unwrap();
        let mut ports_by_name: HashMap<&str, PortId> = HashMap::new();
        for (pid, port) in &model.ports {
            if port.owner == *id {
                ports_by_name.insert(port.name.as_str(), pid);
            }
        }
        // Fixed ports in descriptor order; a family's members sorted by name.
        let mut bound_ports: Vec<(PortId, PortSchema)> = Vec::new();
        for declared in &descriptor.ports {
            if declared.name.contains('*') {
                let mut members: Vec<(&str, PortId)> = ports_by_name.iter().filter(|(n, _)| declared.matches(n)).map(|(n, p)| (*n, *p)).collect();
                members.sort_by(|a, b| a.0.cmp(b.0));
                bound_ports.extend(members.into_iter().map(|(_, p)| (p, declared.schema)));
            } else {
                bound_ports.push((ports_by_name[declared.name], declared.schema));
            }
        }
        let mut slot_kinds: Vec<sim_core::ConnectorKind> = Vec::new();
        for (pid, schema) in bound_ports {
            let declared_name = model.ports[pid].name.clone();
            match schema {
                PortSchema::Acausal(kind) => {
                    slot_kinds.push(kind);
                    let binding = if let sim_core::ConnectorKind::Composite(members) = kind {
                        let mut bound = Vec::new();
                        for (k, (member, member_pid)) in members.iter().zip(&model.ports[pid].members).enumerate() {
                            if member.is_owned() {
                                return Err(CompileError::Equations { behavior: *id, message: format!("composite member {k} of `{declared_name}` is an owned frame") });
                            }
                            if *member == sim_core::ConnectorKind::Thermal {
                                slot.thermal_ports.push((slot.ports.len(), kind.member_offset(k)));
                            }
                            bound.push(port_binding.get(member_pid).cloned().ok_or(CompileError::DanglingPort { port: *member_pid })?);
                        }
                        let binding = PortBinding::Composite(bound);
                        port_lanes.insert(pid, binding.lane_indices());
                        binding
                    } else {
                        if kind == sim_core::ConnectorKind::Thermal {
                            slot.thermal_ports.push((slot.ports.len(), 0));
                        }
                        port_binding.get(&pid).cloned().ok_or(CompileError::DanglingPort { port: pid })?
                    };
                    if let Some((_, wrench, through_width, row_offset)) = owned_ports.iter().find(|(p, _, _, _)| *p == pid) {
                        slot.owned.push((slot.ports.len(), *wrench, *through_width, *row_offset));
                    }
                    slot.ports.push(binding);
                }
                PortSchema::SignalIn(_) => slot.signals_in.push(*signal_index.get(&pid).ok_or(CompileError::DanglingPort { port: pid })?),
                PortSchema::SignalOut(_) => slot.signals_out.push(*signal_index.get(&pid).ok_or(CompileError::DanglingPort { port: pid })?),
            }
        }
        slot.offsets = std::iter::once(0)
            .chain(slot.ports.iter().scan(0, |acc, binding| {
                *acc += binding.width();
                Some(*acc)
            }))
            .collect();
        // Exact rate lanes: flat lane → flat index of the lane that is its derivative.
        let mut rate_map = vec![None; *slot.offsets.last().unwrap()];
        for (port, kind) in slot_kinds.iter().enumerate() {
            for (l, lane) in kind.lanes().iter().enumerate() {
                if let Some(base) = lane.derivative_of {
                    rate_map[slot.offsets[port] + base] = Some(slot.offsets[port] + l);
                }
            }
        }
        slot.rate_map = rate_map;
        // Count guards once at the initial state.
        let mut b = Buffers::default();
        slot.gather(&initial, &vec![0.0; initial.len()], &mut b);
        let mut guards = Vec::new();
        behavior.guards(&slot.view(0.0, &initial, &b), &mut guards);
        slot.guard_offset = guard_offset;
        slot.guard_count = guards.len();
        guard_offset += guards.len();
    }
    // Register stable state ids for every unknown.
    let state_ids = names.iter().zip(&initial).map(|((name, kind), value)| model.state.register(name.clone(), *kind, *value).map_err(|e| CompileError::State(format!("`{name}`: {e}")))).collect::<Result<Vec<_>, _>>()?;
    for slot in &slots {
        model.behaviors[slot.behavior].state = state_ids[slot.state_start..slot.state_start + slot.state_count].to_vec();
    }
    // Sparsity: each unknown a behavior touches can affect every row it writes.
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); dimension];
    for slot in &slots {
        let mut touched: Vec<usize> = (slot.state_start..slot.state_start + slot.state_count).collect();
        let mut written: Vec<usize> = touched.clone();
        for binding in &slot.ports {
            touched.extend(binding.lane_indices());
            binding.written_rows(&mut written);
        }
        touched.extend(slot.signals_in.iter().copied());
        touched.extend(slot.signals_out.iter().copied());
        written.extend(slot.signals_out.iter().copied());
        for column in &touched {
            for row in &written {
                if !rows[*column].contains(row) {
                    rows[*column].push(*row);
                }
            }
        }
    }
    for (lane, base) in &derivative_rows {
        for column in [*lane, *base] {
            if !rows[column].contains(lane) {
                rows[column].push(*lane);
            }
        }
    }
    let mut island = Island {
        behaviors,
        slots,
        dimension,
        state_rows,
        node_rows,
        signal_rows,
        wrench_count,
        state_ids,
        behavior_states,
        port_lanes,
        derivative_rows,
        port_signal: signal_index,
        noise: std::sync::Mutex::new(Noise { rng: 0x9E37_79B9_7F4A_7C15, step: 1.0, draws: Vec::new() }),
        initial,
        sparsity: Sparsity::new(rows),
        algebraic: vec![true; dimension],
        full_of: Vec::new(),
        reduced_of: Vec::new(),
        signal_order: Vec::new(),
        eval_order: Vec::new(),
        lane_of_rate: Vec::new(),
        reduced_sparsity: Sparsity::new(Vec::new()),
        reduced_algebraic: Vec::new(),
        scratch: std::sync::Mutex::new(Scratch::default()),
    };
    // An unknown is differential exactly when some behavior reads its rate:
    // its own state rate, or the across rate of a port lane on its node.
    {
        let zero = vec![0.0; dimension];
        let x = island.initial.clone();
        let mut b = Buffers::default();
        for (slot, (_, behavior)) in island.slots.iter().zip(&island.behaviors) {
            slot.gather(&x, &zero, &mut b);
            let lanes = *slot.offsets.last().unwrap();
            let reads = std::cell::RefCell::new(vec![false; slot.state_count + lanes]);
            {
                let mut ctx = Context::new(
                    0.0,
                    &x[slot.state_start..slot.state_start + slot.state_count],
                    &zero[slot.state_start..slot.state_start + slot.state_count],
                    &slot.offsets,
                    &slot.rate_map,
                    &b.across,
                    &b.across_rates,
                    &b.signals,
                    &mut b.state_residuals,
                    &mut b.through,
                    &mut b.signals_out,
                )
                .with_rate_tracking(&reads);
                behavior.residual(&mut ctx);
            }
            let reads = reads.borrow();
            for k in 0..slot.state_count {
                if reads[k] {
                    island.algebraic[slot.state_start + k] = false;
                }
            }
            for (port, binding) in slot.ports.iter().enumerate() {
                for (lane, index) in binding.lane_indices().into_iter().enumerate() {
                    if reads[slot.state_count + slot.offsets[port] + lane] {
                        island.algebraic[index] = false;
                    }
                }
            }
        }
        for (_, base) in &island.derivative_rows {
            island.algebraic[*base] = false;
        }
        island.reduce();
    }
    Ok(island)
}

/// Resolve explicit qualified/short initial values and fixed constraints.
/// A missing value preserves an owner's/provider's native initial state.
/// Multiple explicit assignments must agree; connection order is irrelevant.
fn node_initial(model: &ModelWorld, connection: &CompiledConnection, lane_index: usize,
    lane: &str, kind: QuantityKind, pinned: &HashMap<(PortId, usize), f64>,
    acausal_counts: &HashMap<BehaviorId, usize>) -> Result<Option<f64>, CompileError> {
    let mut selected: Option<(String, f64)> = None;
    for pid in &connection.ports {
        let port = &model.ports[*pid];
        let behavior = &model.behaviors[port.owner];
        let name = &model.objects[behavior.object].name;
        let mut keys = vec![format!("initial.{}.{lane}", port.name)];
        if acausal_counts.get(&port.owner) == Some(&1) { keys.push(format!("initial.{lane}")); }
        let mut assignments: Vec<_> = keys.iter().filter_map(|key| behavior.parameters.get(key)
            .map(|q| (format!("{name}.{key}"), q.value_si))).collect();
        if let Some(value) = pinned.get(&(*pid, lane_index)) {
            assignments.push((format!("fixed {name}.{}.{lane}", port.name), *value));
        }
        for (label, value) in assignments {
            if !value.is_finite() {
                return Err(CompileError::Equations { behavior: port.owner, message: format!("{label} must be finite [{}]", kind.unit()) });
            }
            if let Some((previous, before)) = &selected {
                if value != *before {
                    return Err(CompileError::Equations { behavior: port.owner,
                        message: format!("conflicting initial values: {previous} = {before} and {label} = {value} [{}]", kind.unit()) });
                }
            } else { selected = Some((label, value)); }
        }
    }
    Ok(selected.map(|(_, value)| value))
}
