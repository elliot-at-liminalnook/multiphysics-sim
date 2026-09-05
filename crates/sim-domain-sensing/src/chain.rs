//! The measurement pipeline every sensor shares: a first-order bandwidth
//! lag, an Erlang latency chain, and a sampler that holds, quantises, adds
//! deterministic noise and suffers faults. Each stage is a state and each
//! fault an event, so a controller sees exactly what a real sensor shows.

use sim_core::{Input, LocalJacobian, Output};
use sim_core::{Context, EquationError, ParameterDeclaration as P, QuantityKind, StateDeclaration, View, param_or};
use std::collections::BTreeMap;
use std::f64::consts::TAU;

/// The sampler's health, kept in the `fault_mode` state.
const ARMED: f64 = 0.0;
const STUCK: f64 = 1.0;
const DROPPED: f64 = 2.0;
const RECOVERED: f64 = 3.0;

/// Timing and sampling declarations shared by every sensor. IMU channel
/// magnitudes are declared separately because acceleration and rate have
/// different dimensions.
pub(crate) fn parameters(unit: Option<&str>) -> Vec<P> {
    let mut parameters = vec![
        P::optional("bandwidth", "Hz", 0.).nonnegative(),
        P::optional("latency", "s", 0.).nonnegative(),
        P::optional("stages", "1", 4.).integer(1., 1024.),
        P::optional("period", "s", 0.).nonnegative(),
        P::optional("seed", "1", 0.).integer(0., 9_007_199_254_740_991.),
        P::optional("fault.mode", "1", 0.).integer(0., 3.),
        P::optional("fault.time", "s", f64::INFINITY).nonnegative(),
        P::optional("fault.duration", "s", 0.).nonnegative(),
        P::optional("fault.samples", "1", 0.).integer(0., 9_007_199_254_740_991.),
    ];
    if let Some(unit) = unit {
        parameters.extend([
            P::optional("quantum", unit, 0.).nonnegative(),
            P::optional("noise", unit, 0.).nonnegative(),
        ]);
    }
    parameters
}

/// Raw value → optional bandwidth lag → optional latency chain → optional
/// sample-and-hold. `raw` mirrors the physical value as an algebraic state
/// because a sample is a jump, which sees no rates; the sampler taps the
/// last continuous state, whichever that is.
#[derive(Clone)]
pub(crate) struct Chain {
    bandwidth: f64,
    latency: f64,
    stages: usize,
    period: f64,
    pub quantum: f64,
    noise: f64,
    seed: u64,
    /// 0 none, 1 stuck, 2 dropout, 3 latency spike.
    fault: u8,
    fault_time: f64,
    fault_duration: f64,
    fault_samples: f64,
    /// Index of the chain's first state within its behavior.
    base: usize,
}

impl Chain {
    pub fn new(p: &BTreeMap<String, f64>) -> Result<Self, EquationError> {
        let period = param_or(p, "period", 0.0);
        let fault = param_or(p, "fault.mode", 0.0) as u8;
        if fault != 0 && period <= 0.0 {
            return Err(EquationError::InvalidParameter("fault.mode".into(), "faults are sampling events and need `period > 0`".into()));
        }
        for name in ["noise", "quantum"] {
            if param_or(p, name, 0.) > 0. && period <= 0. {
                return Err(EquationError::InvalidParameter(name.into(), "sampled noise and quantisation need `period > 0`".into()));
            }
        }
        Ok(Self {
            bandwidth: param_or(p, "bandwidth", 0.0),
            latency: param_or(p, "latency", 0.0),
            stages: param_or(p, "stages", 4.0) as usize,
            period,
            quantum: param_or(p, "quantum", 0.0),
            noise: param_or(p, "noise", 0.0),
            seed: param_or(p, "seed", 0.0) as u64,
            fault,
            fault_time: param_or(p, "fault.time", f64::INFINITY),
            fault_duration: param_or(p, "fault.duration", 0.0),
            fault_samples: param_or(p, "fault.samples", 0.0),
            base: 0,
        })
    }

    /// The same pipeline owning states from `base` on (one per channel).
    pub fn at(&self, base: usize) -> Self {
        Self { base, ..self.clone() }
    }

    pub fn stream(mut self, channel: u64) -> Self {
        self.seed ^= channel.wrapping_mul(0xD1B5_4A32_D192_ED03);
        self
    }

    fn stages_present(&self) -> usize {
        if self.latency > 0.0 { self.stages } else { 0 }
    }
    fn sampled(&self) -> bool {
        self.period > 0.0
    }
    /// Last continuous state: the sampler's tap.
    fn tap(&self) -> usize {
        self.base + (self.bandwidth > 0.0) as usize + self.stages_present()
    }
    fn held(&self) -> usize {
        self.tap() + 1
    }
    pub fn len(&self) -> usize {
        self.tap() - self.base + 1 + if self.sampled() { 3 } else { 0 }
    }

    pub fn states(&self, prefix: &str, kind: QuantityKind) -> Vec<StateDeclaration> {
        let name = |n: String| if prefix.is_empty() { n } else { format!("{prefix}.{n}") };
        let mut out = vec![StateDeclaration::new(name("raw".into()), kind, 0.0)];
        if self.bandwidth > 0.0 {
            out.push(StateDeclaration::new(name("filtered".into()), kind, 0.0));
        }
        out.extend((0..self.stages_present()).map(|k| StateDeclaration::new(name(format!("stage{k}")), kind, 0.0)));
        if self.sampled() {
            out.push(StateDeclaration::new(name("held".into()), kind, 0.0));
            out.push(StateDeclaration::new(name("next_sample".into()), QuantityKind::Time, 0.0));
            out.push(StateDeclaration::new(name("fault_mode".into()), QuantityKind::Dimensionless, ARMED));
        }
        out
    }

    /// Write the chain's residuals for physical value `raw`; returns the
    /// value the sensor should publish.
    pub fn residual(&self, ctx: &mut Context, raw: f64) -> f64 {
        let mut k = self.base;
        ctx.set_state_residual(k, ctx.state(k) - raw);
        let mut value = raw;
        // First-order lag `y' = rate · (input − y)`.
        let lag = |ctx: &mut Context, k: usize, input: f64, rate: f64| {
            ctx.set_state_residual(k, ctx.state_rate(k) - rate * (input - ctx.state(k)));
            ctx.state(k)
        };
        if self.bandwidth > 0.0 {
            k += 1;
            value = lag(ctx, k, value, TAU * self.bandwidth);
        }
        // Erlang delay: `stages` lags with total time constant `latency`.
        for _ in 0..self.stages_present() {
            k += 1;
            value = lag(ctx, k, value, self.stages as f64 / self.latency);
        }
        if self.sampled() {
            for s in k + 1..k + 4 {
                ctx.set_state_residual(s, ctx.state_rate(s));
            }
            value = ctx.state(k + 1);
        }
        value
    }

    /// Partials of the chain's rows and of the published value, given the
    /// partials of the raw physical value with respect to the sensor's inputs.
    /// Returns the partials of the published value.
    pub fn jacobian(&self, out: &mut LocalJacobian, raw: &[(Input, f64)]) -> Vec<(Input, f64)> {
        use std::f64::consts::TAU;
        let mut k = self.base;
        out.state_state(k, k, 1.0);
        for (input, v) in raw {
            out.set(Output::State(k), *input, -v);
        }
        // Each lag row: rate(k) − c·(input − state(k)); the first stage's
        // input is the raw value, later ones the previous state.
        let mut value: Vec<(Input, f64)> = raw.to_vec();
        let mut lags = Vec::new();
        if self.bandwidth > 0.0 {
            lags.push(TAU * self.bandwidth);
        }
        lags.extend(std::iter::repeat_n(self.stages as f64 / self.latency, self.stages_present()));
        for c in lags {
            k += 1;
            out.state_rate(k, k, 1.0);
            out.state_state(k, k, c);
            for (input, v) in &value {
                out.set(Output::State(k), *input, -c * v);
            }
            value = vec![(Input::State(k), 1.0)];
        }
        if self.sampled() {
            for s in k + 1..k + 4 {
                out.state_rate(s, s, 1.0);
            }
            value = vec![(Input::State(k + 1), 1.0)];
        }
        value
    }

    pub fn guard_count(&self) -> usize {
        if self.sampled() { 2 } else { 0 }
    }

    /// Guard 0: the next sample instant. Guard 1: the fault's onset while
    /// armed, its end while dropped out, never otherwise.
    pub fn guards(&self, view: &View, out: &mut Vec<f64>) {
        if !self.sampled() {
            return;
        }
        out.push(view.state(self.held() + 1) - view.time);
        let mode = view.state(self.held() + 2);
        out.push(if mode == ARMED {
            self.fault_time - view.time
        } else if mode == DROPPED {
            self.fault_time + self.fault_duration - view.time
        } else {
            1.0
        });
    }

    pub fn jump(&self, index: usize, view: &View, states: &mut [f64]) {
        let (held, next, mode) = (self.held(), self.held() + 1, self.held() + 2);
        if index == 0 {
            if states[mode] != STUCK && states[mode] != DROPPED {
                let sample = (states[next] / self.period).round() as u64;
                states[held] = self.quantise(view.state(self.tap())) + self.noise * gaussian(self.seed, sample);
            }
            states[next] += self.period;
        } else if states[mode] == ARMED {
            states[mode] = match self.fault {
                1 => STUCK,
                2 => {
                    states[held] = 0.0;
                    DROPPED
                }
                // A latency spike: the sampler sleeps through `samples` periods.
                3 => {
                    states[next] += self.fault_samples * self.period;
                    RECOVERED
                }
                _ => RECOVERED,
            };
        } else {
            // The dropout ends; the next sample restores the reading.
            states[mode] = RECOVERED;
        }
    }

    fn quantise(&self, value: f64) -> f64 {
        if self.quantum > 0.0 { (value / self.quantum).round() * self.quantum } else { value }
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Standard-normal draw for sample `index` of stream `seed`: Box–Muller on
/// two splitmix64 words hashed from the pair, so a trace depends on nothing
/// but its seed and sample count.
fn gaussian(seed: u64, index: u64) -> f64 {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ index.wrapping_mul(0xD1B5_4A32_D192_ED03);
    let mut unit = || (splitmix64(&mut s) >> 11) as f64 / (1u64 << 53) as f64;
    let (u, v) = (unit(), unit());
    (-2.0 * (1.0 - u).ln()).sqrt() * (TAU * v).cos()
}
