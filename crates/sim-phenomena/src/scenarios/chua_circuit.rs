//! 15. Chua's circuit — `electrical`.
//!
//! Two capacitors, an inductor, a resistor and a piecewise-linear negative
//! resistance, wired as a circuit. Sweeping α (= C₂/C₁) walks a
//! period-doubling cascade into the double scroll; the run is also the
//! standing determinism test.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_compile::Runtime;
use sim_domain_electrical::elements as el;
use sim_dynamics::analysis::{largest_lyapunov_exponent, minimal_period, peaks};

/// Dimensionless Chua circuit: with R = 1 and C₂ = 1, α = 1/C₁ and β = 1/L.
#[derive(Clone, Copy)]
pub struct Chua {
    pub alpha: f64,
    pub beta: f64,
    pub m0: f64,
    pub m1: f64,
}

impl Default for Chua {
    fn default() -> Self {
        Self { alpha: 15.6, beta: 28.0, m0: -8.0 / 7.0, m1: -5.0 / 7.0 }
    }
}

pub struct Circuit {
    pub runtime: Runtime,
    pub v1: StateId,
    pub v2: StateId,
    pub i_l: StateId,
}

impl Chua {
    pub fn model(&self, registry: &BehaviorRegistry, initial: [f64; 3]) -> Circuit {
        let mut m = ModelWorld::default();
        let c1 = m.part(registry, "C1", el::CAPACITOR, [("capacitance", 1.0 / self.alpha), ("initial.p.voltage", initial[0])]).unwrap();
        let c2 = m.part(registry, "C2", el::CAPACITOR, [("capacitance", 1.0), ("initial.p.voltage", initial[1])]).unwrap();
        let r = m.part(registry, "R", el::RESISTOR, [("resistance", 1.0)]).unwrap();
        let l = m.part(registry, "L", el::INDUCTOR, [("inductance", 1.0 / self.beta), ("initial.current", initial[2])]).unwrap();
        let diode = m.part(registry, "diode", el::CHUA_DIODE, [("m0", self.m0), ("m1", self.m1)]).unwrap();
        let ground = m.part(registry, "ground", el::GROUND, []).unwrap();
        m.connect([c1.port("p"), r.port("p"), diode.port("p")]);
        m.connect([c2.port("p"), r.port("n"), l.port("p")]);
        m.connect([c1.port("n"), c2.port("n"), l.port("n"), diode.port("n"), ground.port("pin")]);
        let runtime = runtime(m, registry);
        let v1 = runtime.across_id(c1.port("p"));
        let v2 = runtime.across_id(c2.port("p"));
        let i_l = runtime.state_id(l.behavior, "current");
        Circuit { runtime, v1, v2, i_l }
    }
}

const STEP: f64 = 4.0e-3;
const CASCADE_BETA: f64 = 100.0 / 7.0;

/// Orbit period from the repeat length of successive maxima of v₁ after
/// transients, started beside the positive equilibrium. `0` = aperiodic.
fn orbit_period(alpha: f64, registry: &BehaviorRegistry) -> usize {
    let chua = Chua { alpha, beta: CASCADE_BETA, ..Chua::default() };
    let mut circuit = chua.model(registry, [1.6, 0.0, -1.5]);
    let ids = [circuit.v1];
    circuit.runtime.advance(450.0, STEP).unwrap();
    let trace = record(&mut circuit.runtime, 250.0, STEP, 1, &ids);
    let maxima = peaks(&trace.time, &trace.column(0)).into_iter().map(|(_, v)| v).collect::<Vec<_>>();
    let recent = &maxima[maxima.len().saturating_sub(64)..];
    minimal_period(recent, 2.0e-4, 16).unwrap_or(0)
}

pub fn run() -> Report {
    let mut report = Report::new("chua-circuit");
    let registry = registry();

    // Coarse over the period-1/2 band, fine where period 4 and 8 live.
    let mut sweep = Vec::new();
    let mut alpha = 8.10;
    while alpha < 8.47 {
        sweep.push((alpha, orbit_period(alpha, &registry)));
        alpha += if (8.42..8.456).contains(&alpha) { 0.001 } else if (8.37..8.46).contains(&alpha) { 0.0025 } else { 0.01 };
    }
    let first_reaching = |target: usize| sweep.iter().find(|(_, p)| *p == target).map(|(a, _)| *a);
    let cascade = [first_reaching(2), first_reaching(4), first_reaching(8)];
    report.series("orbit period vs α", &sweep.iter().map(|(a, _)| *a).collect::<Vec<_>>(), &sweep.iter().map(|(_, p)| *p as f64).collect::<Vec<_>>(), 100);
    report.holds("period-1 orbit at the start of the sweep", sweep[0].1 == 1);
    report.holds("sweep ends aperiodic (chaos)", sweep.last().map(|(_, p)| *p == 0).unwrap_or(false));
    report.holds("period-2 appears", cascade[0].is_some());
    report.holds("period-4 appears after period-2", cascade[1].is_some() && cascade[1] > cascade[0]);
    if let [Some(a1), Some(a2), Some(a3)] = cascade {
        let delta = (a2 - a1) / (a3 - a2);
        report.measure("α at period 2", a1).measure("α at period 4", a2).measure("α at period 8", a3).measure("Feigenbaum ratio estimate", delta);
        report.within("bifurcation ratio approaches δ = 4.669", delta, 4.669, 0.45);
    } else {
        report.holds("period-8 resolved in the sweep", false);
    }

    let canonical = Chua::default();
    let mut circuit = canonical.model(&registry, [0.1, 0.0, 0.0]);
    let ids = [circuit.v1, circuit.v2, circuit.i_l];
    let trace = record(&mut circuit.runtime, 300.0, STEP, 2, &ids);
    let settled = trace.after(50.0);
    report.series("x(t) double scroll", &settled.time, &settled.column(0), 3000);
    report.series("x–z projection", &settled.column(0), &settled.column(2), 3000);
    let start = [circuit.runtime.get(circuit.v1) + 1.0e-3, circuit.runtime.get(circuit.v2), circuit.runtime.get(circuit.i_l)];
    let mut probe = canonical.model(&registry, start);
    let exponent = largest_lyapunov_exponent(start.to_vec(), 1.0e-6, 0.5, 1200, |x, dt| {
        for (id, value) in [(probe.v1, x[0]), (probe.v2, x[1]), (probe.i_l, x[2])] {
            probe.runtime.set(id, value).unwrap();
        }
        probe.runtime.advance(dt, STEP).unwrap();
        x.copy_from_slice(&[probe.runtime.get(probe.v1), probe.runtime.get(probe.v2), probe.runtime.get(probe.i_l)]);
    });
    report.measure("largest Lyapunov exponent", exponent);
    report.above("chaotic: positive Lyapunov exponent", exponent, 0.1);
    report.holds("both scrolls visited", settled.column(0).iter().any(|x| *x > 1.0) && settled.column(0).iter().any(|x| *x < -1.0));

    let mut again = canonical.model(&registry, [0.1, 0.0, 0.0]);
    let repeat = record(&mut again.runtime, 300.0, STEP, 2, &[again.v1, again.v2, again.i_l]);
    report.holds("repeat run is bitwise identical", trace.state == repeat.state);

    let passive = Chua { m0: 0.5, m1: 0.5, ..canonical };
    let mut c = passive.model(&registry, [0.1, 0.0, 0.0]);
    c.runtime.advance(100.0, STEP).unwrap();
    let rest = [c.runtime.get(c.v1), c.runtime.get(c.v2), c.runtime.get(c.i_l)].iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    report.below("passive diode: decays to rest", rest, 1.0e-6);
    report
}
