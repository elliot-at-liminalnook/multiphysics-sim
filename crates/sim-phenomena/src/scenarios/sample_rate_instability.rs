//! 14. Sample-rate instability — `control` `electrical` `rotational`.
//!
//! A brushed motor speed loop under proportional control with a zero-order
//! hold. The continuous loop is stable at any gain; the sampled loop has an
//! exact critical sample period, `coth(T/2τ) = KpK`.

use crate::world::{registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_bridges::elements as bridge;
use sim_domain_control::elements as ctl;
use sim_domain_electrical::elements as el;
use sim_domain_rotational::elements as rot;
use sim_dynamics::linear::linearise;

#[derive(Clone, Copy)]
pub struct MotorLoop {
    pub resistance: f64,
    pub torque_constant: f64,
    pub back_emf_constant: f64,
    pub inertia: f64,
    pub viscous_drag: f64,
    pub loop_gain: f64,
}

impl Default for MotorLoop {
    fn default() -> Self {
        Self { resistance: 0.6, torque_constant: 0.05, back_emf_constant: 0.05, inertia: 2.0e-4, viscous_drag: 2.0e-4, loop_gain: 3.0 }
    }
}

pub struct Plant {
    pub runtime: Runtime,
    pub speed: StateId,
    pub held: StateId,
}

/// Which speed the controller samples: the shaft's exact speed lane, or
/// the step-average angle rate a finite-difference sensor reports.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sensor {
    Exact,
    StepAverage,
}

impl MotorLoop {
    /// First-order plant constants from the closed-form motor algebra.
    pub fn time_constant(&self) -> f64 {
        self.inertia * self.resistance / (self.viscous_drag * self.resistance + self.torque_constant * self.back_emf_constant)
    }
    pub fn gain(&self) -> f64 {
        self.torque_constant / (self.viscous_drag * self.resistance + self.torque_constant * self.back_emf_constant)
    }
    fn kp(&self) -> f64 {
        self.loop_gain / self.gain()
    }

    /// Controlled voltage source → brushed motor (quasi-static winding) →
    /// inertia, with a speed sensor feeding a sampled proportional
    /// controller back to the source.
    pub fn model(&self, registry: &BehaviorRegistry, period: f64, kp: f64) -> Plant {
        self.model_with(registry, period, kp, Sensor::Exact)
    }

    pub fn model_with(&self, registry: &BehaviorRegistry, period: f64, kp: f64, sensor: Sensor) -> Plant {
        let mut m = ModelWorld::default();
        let source = m.part(registry, "source", el::CONTROLLED_VOLTAGE_SOURCE, []).unwrap();
        let ground = m.part(registry, "ground", el::GROUND, []).unwrap();
        let motor = m.part(registry, "motor", bridge::BRUSHED_MOTOR, [("resistance", self.resistance), ("inductance", 0.0), ("torque_constant", self.torque_constant), ("back_emf_constant", self.back_emf_constant)]).unwrap();
        let rotor = m.part(registry, "rotor", rot::INERTIA, [("inertia", self.inertia), ("damping", self.viscous_drag), ("initial.speed", 10.0)]).unwrap();
        let mount = m.part(registry, "mount", rot::GROUND, []).unwrap();
        let sensor = m.part(registry, "tacho", if sensor == Sensor::Exact { rot::SPEED_SENSOR } else { rot::AVERAGE_SPEED_SENSOR }, []).unwrap();
        let controller = m.part(registry, "controller", ctl::SAMPLED_PROPORTIONAL, [("gain", kp), ("period", period), ("limit", 1.0e9)]).unwrap();
        m.connect([source.port("p"), motor.port("p")]);
        m.connect([source.port("n"), motor.port("n"), ground.port("pin")]);
        m.connect([motor.port("shaft"), rotor.port("shaft"), sensor.port("shaft")]);
        m.connect([motor.port("case"), mount.port("flange")]);
        m.connect([sensor.port("speed"), controller.port("measured")]);
        m.connect([controller.port("command"), source.port("voltage")]);
        let runtime = runtime(m, registry);
        let speed = runtime.state_id(rotor.behavior, "speed");
        let held = runtime.state_id(controller.behavior, "held");
        Plant { runtime, speed, held }
    }
}

/// Closed-loop pole of the ZOH-sampled proportional loop.
pub fn discrete_pole(loop_gain: f64, period: f64, time_constant: f64) -> f64 {
    let decay = (-period / time_constant).exp();
    decay - loop_gain * (1.0 - decay)
}

/// Sample period at which the pole reaches −1: `coth(T/2τ) = KpK`.
pub fn critical_period(loop_gain: f64, time_constant: f64) -> f64 {
    time_constant * ((loop_gain + 1.0) / (loop_gain - 1.0)).ln()
}

/// Plant time constant and DC gain measured on the compiled open-loop
/// plant's linearisation (the eigenvalue and the speed-per-volt response).
pub fn compiled_plant_constants(motor: MotorLoop, registry: &BehaviorRegistry) -> (f64, f64) {
    let plant = motor.model(registry, 1.0e9, 0.0);
    let island = &plant.runtime.islands[0];
    let n = island.state.len();
    let rest = vec![0.0; n];
    let lin = linearise(&island.system, 0.0, &rest, &rest);
    let eigen = lin.eigenvalues();
    let pole = eigen.iter().map(|e| e.re).filter(|r| r.is_finite() && *r < -1.0e-9).fold(f64::NEG_INFINITY, f64::max);
    // DC gain: steady speed for one volt, from a slow ramp of the held command.
    let mut steady = motor.model(registry, 1.0e9, 0.0);
    let held = steady.held;
    // Let the t = 0 sample fire (it holds zero), then plant one volt.
    steady.runtime.advance(1.0e-4, 1.0e-4).unwrap();
    steady.runtime.set(steady.speed, 0.0).unwrap();
    steady.runtime.set(held, 1.0).unwrap();
    steady.runtime.advance(1.0, 1.0e-4).unwrap();
    (-1.0 / pole, steady.runtime.get(steady.speed))
}

/// Continuous proportional loop (a PI with no integral action) through the
/// chosen sensor; returns the measured decay rate of the speed error.
pub fn continuous_loop_decay(motor: MotorLoop, registry: &BehaviorRegistry, kp: f64, sensor: Sensor, h: f64) -> f64 {
    let mut m = ModelWorld::default();
    let source = m.part(registry, "source", el::CONTROLLED_VOLTAGE_SOURCE, []).unwrap();
    let ground = m.part(registry, "ground", el::GROUND, []).unwrap();
    let plant = m.part(registry, "motor", bridge::BRUSHED_MOTOR, [("resistance", motor.resistance), ("inductance", 0.0), ("torque_constant", motor.torque_constant), ("back_emf_constant", motor.back_emf_constant)]).unwrap();
    let rotor = m.part(registry, "rotor", rot::INERTIA, [("inertia", motor.inertia), ("damping", motor.viscous_drag), ("initial.speed", 10.0)]).unwrap();
    let mount = m.part(registry, "mount", rot::GROUND, []).unwrap();
    let tacho = m.part(registry, "tacho", if sensor == Sensor::Exact { rot::SPEED_SENSOR } else { rot::AVERAGE_SPEED_SENSOR }, []).unwrap();
    let controller = m.part(registry, "controller", ctl::PI_CONTROLLER, [("kp", kp), ("ki", 0.0), ("setpoint", 0.0)]).unwrap();
    m.connect([source.port("p"), plant.port("p")]);
    m.connect([source.port("n"), plant.port("n"), ground.port("pin")]);
    m.connect([plant.port("shaft"), rotor.port("shaft"), tacho.port("shaft")]);
    m.connect([plant.port("case"), mount.port("flange")]);
    m.connect([tacho.port("speed"), controller.port("measured")]);
    m.connect([controller.port("command"), source.port("voltage")]);
    let mut rt = runtime(m, registry);
    let speed = rt.state_id(rotor.behavior, "speed");
    let closed_rate = (1.0 + motor.loop_gain) / motor.time_constant();
    let duration = 3.0 / closed_rate;
    let trace = rt.advance_recording(duration, h, 1, &[speed]).unwrap();
    let points = trace.time.iter().zip(trace.column(0)).filter(|(_, v)| *v > 1.0e-6).map(|(t, v)| (*t, v.ln())).collect::<Vec<_>>();
    -sim_dynamics::analysis::linear_fit(&points).map(|(m, _)| m).unwrap_or(0.0)
}

/// Continuous loop with a speed-trip latch on the shaft: the instant the
/// speed falls through `threshold`, and the analytic instant τ'·ln(ω₀/threshold).
pub fn speed_trip_time(motor: MotorLoop, registry: &BehaviorRegistry, kp: f64, threshold: f64, h: f64) -> (f64, f64) {
    let mut m = ModelWorld::default();
    let source = m.part(registry, "source", el::CONTROLLED_VOLTAGE_SOURCE, []).unwrap();
    let ground = m.part(registry, "ground", el::GROUND, []).unwrap();
    let plant = m.part(registry, "motor", bridge::BRUSHED_MOTOR, [("resistance", motor.resistance), ("inductance", 0.0), ("torque_constant", motor.torque_constant), ("back_emf_constant", motor.back_emf_constant)]).unwrap();
    let rotor = m.part(registry, "rotor", rot::INERTIA, [("inertia", motor.inertia), ("damping", motor.viscous_drag), ("initial.speed", 10.0)]).unwrap();
    let mount = m.part(registry, "mount", rot::GROUND, []).unwrap();
    let tacho = m.part(registry, "tacho", rot::SPEED_SENSOR, []).unwrap();
    let trip = m.part(registry, "trip", rot::SPEED_TRIP, [("threshold", threshold)]).unwrap();
    let controller = m.part(registry, "controller", ctl::PI_CONTROLLER, [("kp", kp), ("ki", 0.0), ("setpoint", 0.0)]).unwrap();
    m.connect([source.port("p"), plant.port("p")]);
    m.connect([source.port("n"), plant.port("n"), ground.port("pin")]);
    m.connect([plant.port("shaft"), rotor.port("shaft"), tacho.port("shaft"), trip.port("shaft")]);
    m.connect([plant.port("case"), mount.port("flange")]);
    m.connect([tacho.port("speed"), controller.port("measured")]);
    m.connect([controller.port("command"), source.port("voltage")]);
    let mut rt = runtime(m, registry);
    let trip_time = rt.state_id(trip.behavior, "trip_time");
    let closed = motor.time_constant() / (1.0 + motor.loop_gain);
    rt.advance(5.0 * closed, h).unwrap();
    (rt.get(trip_time), closed * (10.0 / threshold).ln())
}

/// Run the sampled loop from an initial speed error; return the empirical
/// per-sample ratio (the pole) and the sampled speeds.
fn sampled_loop(motor: MotorLoop, registry: &BehaviorRegistry, period: f64, kp: f64, samples: usize) -> (f64, Vec<f64>, Vec<f64>) {
    sampled_loop_with(motor, registry, period, kp, samples, Sensor::Exact, (period / 200.0).min(1.0e-4))
}

fn sampled_loop_with(motor: MotorLoop, registry: &BehaviorRegistry, period: f64, kp: f64, samples: usize, sensor: Sensor, h: f64) -> (f64, Vec<f64>, Vec<f64>) {
    let mut plant = motor.model_with(registry, period, kp, sensor);
    let mut speeds = vec![plant.runtime.get(plant.speed)];
    let mut times = vec![0.0];
    for _ in 0..samples {
        plant.runtime.advance(period, h).unwrap();
        speeds.push(plant.runtime.get(plant.speed));
        times.push(plant.runtime.time);
    }
    let ratios = speeds.windows(2).filter(|w| w[0].abs() > 1.0e-6).map(|w| w[1] / w[0]).collect::<Vec<_>>();
    let pole = ratios[ratios.len() / 2..].iter().sum::<f64>() / (ratios.len() - ratios.len() / 2) as f64;
    (pole, times, speeds)
}

pub fn run() -> Report {
    let mut report = Report::new("sample-rate-instability");
    let registry = registry();
    let motor = MotorLoop::default();
    let (tau, k) = (motor.time_constant(), motor.gain());
    let critical = critical_period(motor.loop_gain, tau);
    let (tau_compiled, k_compiled) = compiled_plant_constants(motor, &registry);
    report
        .measure("plant time constant τ (s)", tau)
        .measure("plant gain K (rad/s per V)", k)
        .measure("critical sample period (s)", critical);
    report.within("compiled plant pole gives τ", tau_compiled, tau, 1.0e-3);
    report.within("compiled plant DC gain gives K", k_compiled, k, 1.0e-2);

    let kp = motor.kp();
    for (label, fraction) in [("0.5 T_c", 0.5), ("0.9 T_c", 0.9), ("1.1 T_c", 1.1)] {
        let period = fraction * critical;
        let (observed, times, speeds) = sampled_loop(motor, &registry, period, kp, 24);
        let predicted = discrete_pole(motor.loop_gain, period, tau);
        report.series(&format!("speed samples at {label}"), &times, &speeds, 200);
        report.close(&format!("pole at {label} matches z = e^(−T/τ) − KpK(1 − e^(−T/τ))"), observed, predicted, 1.0e-6);
    }
    // The exact speed lane, demonstrated: on continuous reads a
    // step-average sensor coincides with it under the midpoint rule (both
    // are the mid-step speed), but a *guard* sees the lane at an instant.
    {
        let closed_rate = (1.0 + motor.loop_gain) / tau;
        let h = 1.0 / closed_rate / 10.0;
        let exact = continuous_loop_decay(motor, &registry, kp, Sensor::Exact, h);
        let average = continuous_loop_decay(motor, &registry, kp, Sensor::StepAverage, h);
        report.measure("continuous loop decay rate, exact lane", exact).measure("continuous loop decay rate, step-average sensor", average).measure("analytic closed-loop rate (1 + KpK)/τ", closed_rate);
        report.within("continuous loop rate within 1% at h = τ'/10", exact, closed_rate, 0.01);
        report.close("step-average sensor coincides with the lane on continuous reads", average, exact, 1.0e-9);
        // A trip latch on the lane fires when ω falls through ω₀/e: at t = τ' exactly.
        let (trip, expected) = speed_trip_time(motor, &registry, kp, 10.0 / std::f64::consts::E, h / 20.0);
        report.measure("speed-trip time (s)", trip).measure("analytic trip time τ' = τ/(1 + KpK)", expected);
        report.within("guard on the speed lane trips at the analytic instant", trip, expected, 1.0e-5);
    }
    let (stable, _, speeds) = sampled_loop(motor, &registry, 0.9 * critical, kp, 40);
    report.below("0.9 T_c: converges", speeds.last().unwrap().abs(), 0.5);
    report.below("0.9 T_c: |pole| < 1", stable.abs(), 1.0);
    let (unstable, _, speeds) = sampled_loop(motor, &registry, 1.1 * critical, kp, 40);
    report.above("1.1 T_c: diverges", speeds.last().unwrap().abs(), 20.0);
    report.above("1.1 T_c: |pole| > 1", unstable.abs(), 1.0);

    let (mut lo, mut hi) = (0.5 * critical, 1.5 * critical);
    for _ in 0..12 {
        let mid = 0.5 * (lo + hi);
        let (pole, _, _) = sampled_loop(motor, &registry, mid, kp, 24);
        if pole.abs() < 1.0 { lo = mid } else { hi = mid }
    }
    report.within("empirical critical period", 0.5 * (lo + hi), critical, 0.01);

    let (_, _, speeds) = sampled_loop(motor, &registry, tau / 1000.0, 100.0 / k, 4000);
    report.below("T → 0: stable even at KpK = 100", speeds.last().unwrap().abs(), 1.0e-3);
    report
}
