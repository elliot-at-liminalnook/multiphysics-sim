//! 3. Current hogging — `electrical` `thermal`.
//!
//! Two thermistors in parallel on one current source, each with a thermal
//! capacitance and a conductance to ambient, optionally coupled to each
//! other. With a negative coefficient and enough loop gain the marginally
//! hotter device takes the whole load.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_bridges::elements as bridge;
use sim_domain_electrical::elements as el;
use sim_domain_thermal as th;
use sim_dynamics::analysis::linear_fit;
use sim_dynamics::linear::{leading_mode, linearise};

#[derive(Clone, Copy)]
pub struct ParallelPair {
    pub total_current: f64,
    pub resistance: f64,
    pub coefficient: f64,
    pub thermal_resistance: f64,
    pub heat_capacity: f64,
    pub coupling: f64,
    pub ambient: f64,
}

impl ParallelPair {
    pub fn device_resistance(&self, temperature: f64) -> f64 {
        self.resistance * (self.coefficient * (temperature - self.ambient)).exp()
    }
    /// Per-device power at the symmetric equilibrium: first root of
    /// `P − I²R(T_amb + P·R_th)/4`, by bisection.
    pub fn symmetric_power(&self) -> f64 {
        let residual = |power: f64| power - self.total_current.powi(2) * self.device_resistance(self.ambient + power * self.thermal_resistance) / 4.0;
        let mut hi = self.total_current.powi(2) * self.resistance / 4.0;
        while residual(hi) < 0.0 {
            hi *= 1.02;
            if hi > 1.0e12 {
                return f64::NAN;
            }
        }
        let mut lo = 0.0;
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if residual(mid) < 0.0 { lo = mid } else { hi = mid }
        }
        0.5 * (lo + hi)
    }
    pub fn loop_gain(&self) -> f64 {
        self.coefficient.abs() * self.thermal_resistance * self.symmetric_power()
    }
    pub fn asymmetry_growth_rate(&self) -> f64 {
        let signed_gain = -self.coefficient.signum() * self.loop_gain();
        ((signed_gain - 1.0) / self.thermal_resistance - 2.0 * self.coupling) / self.heat_capacity
    }
}

pub struct Board {
    pub runtime: Runtime,
    pub t1: StateId,
    pub t2: StateId,
    pub v: StateId,
    pub device1: sim_core::Instance,
}

impl ParallelPair {
    pub fn model(&self, registry: &BehaviorRegistry, asymmetry: f64) -> Board {
        let equilibrium = self.ambient + self.symmetric_power() * self.thermal_resistance;
        let mut m = ModelWorld::default();
        let source = m.part(registry, "source", el::CURRENT_SOURCE, [("current", self.total_current)]).unwrap();
        let ground = m.part(registry, "ground", el::GROUND, []).unwrap();
        let ambient = m.part(registry, "ambient", th::AMBIENT, [("temperature", self.ambient)]).unwrap();
        let mut devices = Vec::new();
        let mut ambient_ports = vec![ambient.port("node")];
        let link = (self.coupling > 0.0).then(|| m.part(registry, "link", th::CONDUCTANCE, [("conductance", self.coupling)]).unwrap());
        for (k, (name, offset)) in [("device1", asymmetry), ("device2", -asymmetry)].into_iter().enumerate() {
            let device = m.part(registry, name, bridge::THERMISTOR, [("resistance", self.resistance), ("coefficient", self.coefficient), ("reference", self.ambient)]).unwrap();
            let mass = m.part(registry, &format!("{name} mass"), th::CAPACITANCE, [("heat_capacity", self.heat_capacity), ("initial.temperature", equilibrium + offset)]).unwrap();
            let sink = m.part(registry, &format!("{name} sink"), th::CONDUCTANCE, [("resistance", self.thermal_resistance)]).unwrap();
            let mut node = vec![device.port("heat"), mass.port("node"), sink.port("a")];
            if let Some(link) = &link {
                node.push(link.port(if k == 0 { "a" } else { "b" }));
            }
            m.connect(node);
            ambient_ports.push(sink.port("b"));
            devices.push((device, mass));
        }
        m.connect(ambient_ports);
        m.connect([source.port("p"), devices[0].0.port("p"), devices[1].0.port("p")]);
        m.connect([source.port("n"), devices[0].0.port("n"), devices[1].0.port("n"), ground.port("pin")]);
        let runtime = runtime(m, registry);
        let t1 = runtime.across_id(devices[0].1.port("node"));
        let t2 = runtime.across_id(devices[1].1.port("node"));
        let v = runtime.across_id(devices[0].0.port("p"));
        Board { runtime, t1, t2, v, device1: devices.remove(0).0 }
    }
}

pub struct Outcome {
    pub share: f64,
    pub growth_rate: f64,
    pub time: Vec<f64>,
    pub share_trace: Vec<f64>,
    pub t1: Vec<f64>,
    pub t2: Vec<f64>,
}

/// Device 1's share of the current, from the node voltage and its resistance.
fn share(pair: &ParallelPair, v: f64, t1: f64) -> f64 {
    v / pair.device_resistance(t1) / pair.total_current
}

pub fn run_pair(pair: ParallelPair, registry: &BehaviorRegistry, duration: f64) -> Outcome {
    let mut board = pair.model(registry, 0.01);
    let ids = [board.t1, board.t2, board.v];
    let trace = record(&mut board.runtime, duration, 0.02, 2, &ids);
    let asymmetry = trace.map(|_, x| x[0] - x[1]);
    let early_end = trace.time.partition_point(|t| *t < duration.min(6.0));
    let points = trace.time[..early_end].iter().zip(&asymmetry[..early_end]).map(|(t, a)| (*t, a.abs().ln())).collect::<Vec<_>>();
    let growth_rate = linear_fit(&points).map(|(m, _)| m).unwrap_or(0.0);
    let share_trace = trace.map(|_, x| share(&pair, x[2], x[0]));
    Outcome { share: *share_trace.last().unwrap(), growth_rate, time: trace.time.clone(), share_trace, t1: trace.column(0), t2: trace.column(1) }
}

/// Growth rate of the differential (T₁ − T₂) mode from the compiled
/// model's linearisation at the even split.
pub fn compiled_growth_rate(pair: ParallelPair, registry: &BehaviorRegistry) -> f64 {
    let board = pair.model(registry, 0.0);
    let island = &board.runtime.islands[0];
    let rate = vec![0.0; island.state.len()];
    let lin = linearise(&island.system, 0.0, &island.state, &rate);
    let eigen = lin.eigenvalues();
    // Two thermal modes: common (always stable, faster) and differential.
    let mut reals: Vec<f64> = eigen.iter().map(|e| e.re).filter(|r| r.is_finite()).collect();
    reals.sort_by(|a, b| b.total_cmp(a));
    let _ = leading_mode(&eigen);
    reals[0]
}

pub fn run() -> Report {
    let mut report = Report::new("current-hogging");
    let registry = registry();
    let base = ParallelPair { total_current: 4.0, resistance: 1.0, coefficient: -0.02, thermal_resistance: 10.0, heat_capacity: 1.0, coupling: 0.0, ambient: 300.0 };
    let with_gain = |sign: f64, gain: f64| {
        let (mut lo, mut hi) = (0.1, 40.0);
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            let pair = ParallelPair { total_current: mid, coefficient: sign * base.coefficient.abs(), ..base };
            if pair.loop_gain() < gain { lo = mid } else { hi = mid }
        }
        ParallelPair { total_current: 0.5 * (lo + hi), coefficient: sign * base.coefficient.abs(), ..base }
    };

    let ptc = with_gain(1.0, 0.8);
    let outcome = run_pair(ptc, &registry, 60.0);
    report.measure("positive coefficient: loop gain", ptc.loop_gain());
    report.close("positive coefficient: even split", outcome.share, 0.5, 0.01);
    report.below("positive coefficient: asymmetry decays", outcome.growth_rate, ptc.asymmetry_growth_rate() * 0.9);

    let ntc_low = with_gain(-1.0, 0.6);
    let outcome = run_pair(ntc_low, &registry, 60.0);
    report.close("negative coefficient, gain 0.6: still even", outcome.share, 0.5, 0.01);

    let ntc_high = with_gain(-1.0, 1.5);
    let outcome = run_pair(ntc_high, &registry, 400.0);
    report.series("current share of device 1, gain 1.5", &outcome.time, &outcome.share_trace, 1500);
    report.series("T₁ (K), gain 1.5", &outcome.time, &outcome.t1, 1500);
    report.series("T₂ (K), gain 1.5", &outcome.time, &outcome.t2, 1500);
    report.measure("negative coefficient: hot device share", outcome.share);
    report.above("negative coefficient, gain 1.5: one device hogs", outcome.share.max(1.0 - outcome.share), 0.8);

    for (label, gain) in [("gain 0.9", 0.9), ("gain 1.1", 1.1)] {
        let pair = with_gain(-1.0, gain);
        let predicted = pair.asymmetry_growth_rate();
        let compiled = compiled_growth_rate(pair, &registry);
        let outcome = run_pair(pair, &registry, 6.0);
        report.measure(&format!("predicted asymmetry growth rate at {label}"), predicted);
        report.close(&format!("compiled linearisation growth rate at {label}"), compiled, predicted, 2.0e-4);
        report.close(&format!("asymmetry growth rate at {label}"), outcome.growth_rate, predicted, 0.02 * predicted.abs().max(0.01));
    }
    let outcome = run_pair(with_gain(-1.0, 0.9), &registry, 60.0);
    report.close("gain 0.9: even split holds", outcome.share, 0.5, 0.01);

    let pinned = ParallelPair { coupling: 1.0e3, ..ntc_high };
    let outcome = run_pair(pinned, &registry, 400.0);
    report.close("shared temperature: even split despite α < 0", outcome.share, 0.5, 0.01);
    report
}
