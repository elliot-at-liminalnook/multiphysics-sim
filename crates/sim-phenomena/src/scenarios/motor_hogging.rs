//! 18. Motor hogging — `composite` `electrical` `rotational` `thermal`.
//!
//! Plate 3 re-authored on composite connectors: one drive with two `Motor`
//! sockets, two motors each behind a single plug that bundles winding,
//! shaft and case. The same `|α|·R_th·P = 1` boundary decides whether the
//! warmer winding takes the whole current; the drive reads the hotter case
//! off the plug it already holds.

use super::current_hogging::ParallelPair;
use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_bridges::elements as bridge;
use sim_domain_rotational::elements as rot;
use sim_domain_thermal as th;
use sim_dynamics::analysis::linear_fit;
use sim_dynamics::linear::linearise;

#[derive(Clone, Copy)]
pub enum Load {
    /// Rotors held: the windings are plain resistors and plate 3's boundary applies exactly.
    Locked,
    /// Rotors free on a viscous load: back-EMF shares the bus voltage.
    Spinning { inertia: f64, damping: f64 },
}

#[derive(Clone, Copy)]
pub struct Drive {
    pub pair: ParallelPair,
    pub torque_constant: f64,
    pub load: Load,
}

pub struct Rig {
    pub runtime: Runtime,
    pub temperatures: [StateId; 2],
    pub voltage: StateId,
    pub speeds: [StateId; 2],
    pub angles: [StateId; 2],
    pub hottest: StateId,
}

impl Drive {
    pub fn model(&self, registry: &BehaviorRegistry, asymmetry: f64) -> Rig {
        let pair = self.pair;
        let equilibrium = pair.ambient + pair.symmetric_power() * pair.thermal_resistance;
        let mut m = ModelWorld::default();
        let drive = m.part(registry, "drive", bridge::DUAL_DRIVE, [("current", pair.total_current)]).unwrap();
        let ambient = m.part(registry, "ambient", th::AMBIENT, [("temperature", pair.ambient)]).unwrap();
        let link = (pair.coupling > 0.0).then(|| m.part(registry, "link", th::CONDUCTANCE, [("conductance", pair.coupling)]).unwrap());
        let mut sinks = vec![ambient.port("node")];
        let mut motors = Vec::new();
        for (k, (name, offset)) in [("motor a", asymmetry), ("motor b", -asymmetry)].into_iter().enumerate() {
            let motor = m.part(registry, name, bridge::MOTOR, [("resistance", pair.resistance), ("coefficient", pair.coefficient), ("reference", pair.ambient), ("torque_constant", self.torque_constant)]).unwrap();
            let case = m.part(registry, &format!("{name} case"), th::CAPACITANCE, [("heat_capacity", pair.heat_capacity), ("initial.temperature", equilibrium + offset)]).unwrap();
            let sink = m.part(registry, &format!("{name} sink"), th::CONDUCTANCE, [("resistance", pair.thermal_resistance)]).unwrap();
            // One connection: the socket and the plug fan out member-wise,
            // the plain ports join the member of their own kind.
            let mut socket = vec![drive.port(if k == 0 { "a" } else { "b" }), motor.port("plug"), case.port("node"), sink.port("a")];
            match self.load {
                Load::Locked => {
                    let clamp = m.part(registry, &format!("{name} clamp"), rot::GROUND, []).unwrap();
                    socket.push(clamp.port("flange"));
                }
                Load::Spinning { inertia, damping } => {
                    let rotor = m.part(registry, &format!("{name} rotor"), rot::INERTIA, [("inertia", inertia)]).unwrap();
                    let bearing = m.part(registry, &format!("{name} bearing"), rot::DAMPER, [("damping", damping)]).unwrap();
                    let frame = m.part(registry, &format!("{name} frame"), rot::GROUND, []).unwrap();
                    socket.push(rotor.port("shaft"));
                    socket.push(bearing.port("a"));
                    m.connect([bearing.port("b"), frame.port("flange")]);
                }
            }
            if let Some(link) = &link {
                socket.push(link.port(if k == 0 { "a" } else { "b" }));
            }
            m.connect(socket);
            sinks.push(sink.port("b"));
            motors.push((motor, case));
        }
        m.connect(sinks);
        let runtime = runtime(m, registry);
        let temperatures = [runtime.across_id(motors[0].1.port("node")), runtime.across_id(motors[1].1.port("node"))];
        let voltage = runtime.across_id(motors[0].0.port("plug.electrical"));
        let speeds = [0, 1].map(|k| runtime.across_lane_id(motors[k].0.port("plug.rotational"), 1));
        let angles = [0, 1].map(|k| runtime.across_id(motors[k].0.port("plug.rotational")));
        let hottest = runtime.signal_id(drive.port("hottest"));
        Rig { runtime, temperatures, voltage, speeds, angles, hottest }
    }
}

pub struct Outcome {
    pub share: f64,
    pub growth_rate: f64,
    pub hottest_error: f64,
    pub time: Vec<f64>,
    pub share_trace: Vec<f64>,
    pub temperatures: [Vec<f64>; 2],
    pub speeds: [Vec<f64>; 2],
}

/// Motor a's share of the drive current from its plug: (v − kω)/R(T)/I.
pub fn share(drive: &Drive, v: f64, speed: f64, temperature: f64) -> f64 {
    (v - drive.torque_constant * speed) / drive.pair.device_resistance(temperature) / drive.pair.total_current
}

pub fn run_drive(drive: Drive, registry: &BehaviorRegistry, duration: f64) -> Outcome {
    let mut rig = drive.model(registry, 0.01);
    let ids = [rig.temperatures[0], rig.temperatures[1], rig.voltage, rig.speeds[0], rig.speeds[1], rig.hottest];
    let trace = record(&mut rig.runtime, duration, 0.02, 2, &ids);
    let asymmetry = trace.map(|_, x| x[0] - x[1]);
    let early_end = trace.time.partition_point(|t| *t < duration.min(6.0));
    let points = trace.time[..early_end].iter().zip(&asymmetry[..early_end]).map(|(t, a)| (*t, a.abs().ln())).collect::<Vec<_>>();
    let growth_rate = linear_fit(&points).map(|(m, _)| m).unwrap_or(0.0);
    let share_trace = trace.map(|_, x| share(&drive, x[2], x[3], x[0]));
    let hottest_error = trace.state.iter().map(|x| (x[5] - x[0].max(x[1])).abs()).fold(0.0, f64::max);
    Outcome {
        share: *share_trace.last().unwrap(),
        growth_rate,
        hottest_error,
        time: trace.time.clone(),
        share_trace,
        temperatures: [trace.column(0), trace.column(1)],
        speeds: [trace.column(3), trace.column(4)],
    }
}

/// Growth rate of the differential (T_a − T_b) mode from the compiled
/// model's linearisation next to the even split (a hair off it, so the
/// drive's `max` is differentiable there). Modes faster than the electrical
/// and mechanical constraints (|λ| > 10³/s) are the pencil's infinite
/// eigenvalues, not dynamics.
pub fn compiled_growth_rate(drive: Drive, registry: &BehaviorRegistry) -> f64 {
    let rig = drive.model(registry, 1.0e-3);
    let island = &rig.runtime.islands[0];
    let rate = vec![0.0; island.state.len()];
    let lin = linearise(&island.system, 0.0, &island.state, &rate);
    let mut reals: Vec<f64> = lin.eigenvalues().iter().filter(|e| e.norm() < 1.0e3).map(|e| e.re).collect();
    reals.sort_by(|a, b| b.total_cmp(a));
    reals[0]
}

/// With free rotors the back-EMF adds `k²/c` to each winding's effective
/// resistance, with no temperature dependence: the loop gain scales by
/// `R / (R + k²/c)`.
pub fn free_rotor_gain(drive: &Drive) -> f64 {
    match drive.load {
        Load::Locked => drive.pair.loop_gain(),
        Load::Spinning { damping, .. } => drive.pair.loop_gain() * drive.pair.resistance / (drive.pair.resistance + drive.torque_constant.powi(2) / damping),
    }
}

pub fn base() -> ParallelPair {
    ParallelPair { total_current: 4.0, resistance: 1.0, coefficient: -0.02, thermal_resistance: 10.0, heat_capacity: 1.0, coupling: 0.0, ambient: 300.0 }
}

/// The pair whose loop gain `|α|·R_th·P` equals `gain`, by bisection on the drive current.
pub fn with_gain(sign: f64, gain: f64) -> ParallelPair {
    let base = base();
    let (mut lo, mut hi) = (0.1, 40.0);
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        let pair = ParallelPair { total_current: mid, coefficient: sign * base.coefficient.abs(), ..base };
        if pair.loop_gain() < gain { lo = mid } else { hi = mid }
    }
    ParallelPair { total_current: 0.5 * (lo + hi), coefficient: sign * base.coefficient.abs(), ..base }
}

pub fn run() -> Report {
    let mut report = Report::new("motor-hogging");
    let registry = registry();
    let locked = |pair: ParallelPair| Drive { pair, torque_constant: 0.05, load: Load::Locked };

    let hog = locked(with_gain(-1.0, 1.5));
    let outcome = run_drive(hog, &registry, 400.0);
    report.series("current share of motor a, gain 1.5", &outcome.time, &outcome.share_trace, 1500);
    report.series("case a (K), gain 1.5", &outcome.time, &outcome.temperatures[0], 1500);
    report.series("case b (K), gain 1.5", &outcome.time, &outcome.temperatures[1], 1500);
    report.measure("gain 1.5: hot motor's share", outcome.share);
    report.above("gain 1.5: one motor hogs the drive", outcome.share.max(1.0 - outcome.share), 0.8);
    // An algebraic output under the midpoint rule reads the stage state:
    // it may trail the recorded temperatures by half a step's drift.
    report.below("the drive's `hottest` signal is the hotter case (to half a step's drift)", outcome.hottest_error, 0.02);

    let outcome = run_drive(locked(with_gain(-1.0, 0.6)), &registry, 60.0);
    report.close("gain 0.6: even split", outcome.share, 0.5, 0.01);
    let outcome = run_drive(locked(with_gain(1.0, 0.8)), &registry, 60.0);
    report.close("positive coefficient: even split", outcome.share, 0.5, 0.01);

    for (label, gain) in [("gain 0.9", 0.9), ("gain 1.1", 1.1)] {
        let drive = locked(with_gain(-1.0, gain));
        let predicted = drive.pair.asymmetry_growth_rate();
        let compiled = compiled_growth_rate(drive, &registry);
        let outcome = run_drive(drive, &registry, 6.0);
        report.measure(&format!("plate-3 asymmetry growth rate at {label}"), predicted);
        report.close(&format!("compiled linearisation growth rate at {label}"), compiled, predicted, 2.0e-4);
        report.close(&format!("asymmetry growth rate at {label}"), outcome.growth_rate, predicted, 0.02 * predicted.abs().max(0.01));
    }

    // Falsifier: pin the two cases together and the split stays even.
    let pinned = locked(ParallelPair { coupling: 1.0e3, ..with_gain(-1.0, 1.5) });
    let outcome = run_drive(pinned, &registry, 400.0);
    report.close("cases pinned together: even split despite α < 0", outcome.share, 0.5, 0.01);

    // Rotors free: the shafts turn on the same plug. Back-EMF adds k²/c of
    // temperature-independent resistance per winding, scaling the gain by
    // R/(R + k²/c): a light bearing keeps the hogging, a heavy one removes it.
    for (label, damping) in [("light bearing", 0.1), ("heavy bearing", 1.0e-3)] {
        let spinning = Drive { load: Load::Spinning { inertia: 1.0e-3, damping }, ..hog };
        let outcome = run_drive(spinning, &registry, 400.0);
        let gain = free_rotor_gain(&spinning);
        report.series(&format!("current share of motor a, {label}"), &outcome.time, &outcome.share_trace, 1500);
        report.series(&format!("rotor a speed (rad/s), {label}"), &outcome.time, &outcome.speeds[0], 1500);
        report.series(&format!("rotor b speed (rad/s), {label}"), &outcome.time, &outcome.speeds[1], 1500);
        report.measure(&format!("{label}: loop gain × R/(R + k²/c)"), gain);
        report.measure(&format!("{label}: hot motor's share"), outcome.share.max(1.0 - outcome.share));
        if gain > 1.0 {
            report.above(&format!("{label}: gain {gain:.2} > 1 — one motor still hogs"), outcome.share.max(1.0 - outcome.share), 0.65);
        } else {
            report.close(&format!("{label}: gain {gain:.2} < 1 — even split"), outcome.share, 0.5, 0.01);
        }
    }
    report
}
