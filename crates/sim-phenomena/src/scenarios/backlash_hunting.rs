//! 13. Backlash hunting — `control` `rotational`.
//!
//! A PI position servo drives a load through a compliant gear mesh with a
//! gap. The loop is stable with the gap closed; with any gap it hunts at a
//! fixed frequency whose amplitude scales exactly with the gap.

use crate::world::{record, registry, runtime};
use crate::Report;
use nalgebra::{Complex, DVector};
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, PortId};
use sim_domain_control::elements as ctl;
use sim_domain_rotational::elements as rot;
use sim_dynamics::analysis::{peaks, period};
use sim_dynamics::linear::{leading_mode, linearise};

#[derive(Clone, Copy)]
pub struct Servo {
    pub motor_inertia: f64,
    pub load_inertia: f64,
    pub load_damping: f64,
    pub motor_damping: f64,
    pub mesh_stiffness: f64,
    pub gap: f64,
    pub kp: f64,
    pub ki: f64,
}

impl Default for Servo {
    fn default() -> Self {
        Self { motor_inertia: 1.0e-3, load_inertia: 4.0e-3, load_damping: 0.02, motor_damping: 0.02, mesh_stiffness: 40.0, gap: 0.0, kp: 6.0, ki: 30.0 }
    }
}

pub struct Model {
    pub runtime: Runtime,
    pub motor_shaft: PortId,
    pub load_shaft: PortId,
}

impl Servo {
    /// Motor and load inertias, a backlash mesh between them, an angle
    /// sensor on the load feeding a PI regulator whose command drives a
    /// torque source on the motor.
    pub fn model(&self, registry: &BehaviorRegistry, mesh_stiffness: f64) -> Model {
        let mut m = ModelWorld::default();
        let motor = m.part(registry, "motor", rot::INERTIA, [("inertia", self.motor_inertia), ("damping", self.motor_damping), ("initial.angle", 0.3)]).unwrap();
        let load = m.part(registry, "load", rot::INERTIA, [("inertia", self.load_inertia), ("damping", self.load_damping), ("initial.angle", 0.3)]).unwrap();
        let mesh = m.part(registry, "mesh", rot::BACKLASH_MESH, [("stiffness", mesh_stiffness), ("gap", self.gap)]).unwrap();
        let sensor = m.part(registry, "sensor", rot::ANGLE_SENSOR, []).unwrap();
        let controller = m.part(registry, "controller", ctl::PI_CONTROLLER, [("kp", self.kp), ("ki", self.ki), ("setpoint", 0.0)]).unwrap();
        let drive = m.part(registry, "drive", rot::TORQUE_SOURCE, []).unwrap();
        m.connect([motor.port("shaft"), mesh.port("a"), drive.port("shaft")]);
        m.connect([load.port("shaft"), mesh.port("b"), sensor.port("shaft")]);
        m.connect([sensor.port("angle"), controller.port("measured")]);
        m.connect([controller.port("command"), drive.port("torque")]);
        let runtime = runtime(m, registry);
        Model { runtime, motor_shaft: motor.port("shaft"), load_shaft: load.port("shaft") }
    }
}

pub struct Cycle {
    pub amplitude: f64,
    pub frequency: f64,
    pub time: Vec<f64>,
    pub load: Vec<f64>,
    pub motor: Vec<f64>,
}

pub fn hunt(servo: Servo, registry: &BehaviorRegistry) -> Cycle {
    let mut model = servo.model(registry, servo.mesh_stiffness);
    let ids = [model.runtime.across_id(model.motor_shaft), model.runtime.across_id(model.load_shaft)];
    let trace = record(&mut model.runtime, 40.0, 5.0e-4, 10, &ids);
    let tail = trace.after(25.0);
    let twist = tail.map(|_, x| x[0] - x[1]);
    let amplitude = peaks(&tail.time, &twist).iter().map(|(_, v)| *v).fold(0.0_f64, f64::max);
    let frequency = period(&tail.time, &twist).map(|p| std::f64::consts::TAU / p).unwrap_or(0.0);
    Cycle { amplitude, frequency, time: trace.time.clone(), load: trace.column(1), motor: trace.column(0) }
}

/// Largest real part of the compiled loop with the mesh at stiffness `k`
/// (gap zero), from the island's linearisation about rest.
pub fn max_real_eigenvalue(servo: Servo, registry: &BehaviorRegistry, k: f64) -> f64 {
    let model = Servo { gap: 0.0, ..servo }.model(registry, k);
    let island = &model.runtime.islands[0];
    let rest = vec![0.0; island.state.len()];
    let lin = linearise(&island.system, 0.0, &rest, &rest);
    leading_mode(&lin.eigenvalues()).0
}

/// Harmonic balance of the dead-zone describing function against the
/// linear part `G(jω) = twist / mesh torque`, obtained from the compiled
/// model with the mesh removed (stiffness zero): `(ω*, A/gap)`.
pub fn describing_function_prediction(servo: Servo, registry: &BehaviorRegistry) -> Option<(f64, f64)> {
    let model = Servo { gap: 0.0, ..servo }.model(registry, 0.0);
    let island = &model.runtime.islands[0];
    let n = island.state.len();
    let rest = vec![0.0; n];
    let lin = linearise(&island.system, 0.0, &rest, &rest);
    // Mesh torque τ enters the motor node balance as +τ and the load node as −τ
    // (through into the mesh at a, out at b); the residual is `−b·u`.
    // A node's torque balance row shares the index of its angle lane.
    let motor_row = island.system.port_lanes[&model.motor_shaft][0];
    let load_row = island.system.port_lanes[&model.load_shaft][0];
    let mut b = DVector::zeros(n);
    b[motor_row] = -1.0;
    b[load_row] = 1.0;
    let mut c = DVector::zeros(n);
    c[motor_row] = 1.0;
    c[load_row] = -1.0;
    let g = |w: f64| lin.transfer(&b, &c, Complex::new(0.0, w));
    // Harmonic balance Δ = G·τ, τ = N(A)·Δ ⇒ G(jω)·N(A) = 1: crossover where G is real, positive.
    let mut w = 0.5;
    let mut bracket = None;
    while w < 2000.0 {
        let (p, q) = (g(w), g(w * 1.02));
        if p.im.signum() != q.im.signum() && p.re > 0.0 {
            bracket = Some((w, w * 1.02));
            break;
        }
        w *= 1.02;
    }
    let (mut lo, mut hi) = bracket?;
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if g(mid).im.signum() == g(lo).im.signum() { lo = mid } else { hi = mid }
    }
    let omega = 0.5 * (lo + hi);
    let ratio = 1.0 / g(omega).re / servo.mesh_stiffness;
    if !(0.0..1.0).contains(&ratio) {
        return None;
    }
    let (mut lo, mut hi) = (1.0e-9_f64, 1.0_f64);
    for _ in 0..80 {
        let r = 0.5 * (lo + hi);
        let n = 1.0 - 2.0 / std::f64::consts::PI * (r.asin() + r * (1.0 - r * r).sqrt());
        if n > ratio { lo = r } else { hi = r }
    }
    Some((omega, 2.0 / (lo + hi)))
}

pub fn run() -> Report {
    let mut report = Report::new("backlash-hunting");
    let registry = registry();
    let base = Servo::default();
    report.below("closed-gap loop is stable", max_real_eigenvalue(base, &registry, base.mesh_stiffness), 0.0);
    report.above("softened mesh is unstable", max_real_eigenvalue(base, &registry, 0.1 * base.mesh_stiffness), 0.0);

    let closed = hunt(base, &registry);
    report.series("load angle, no gap", &closed.time, &closed.load, 800);
    report.below("no gap: settles", closed.amplitude, 1.0e-6);

    let gap = 0.01;
    let one = hunt(Servo { gap, ..base }, &registry);
    let two = hunt(Servo { gap: 2.0 * gap, ..base }, &registry);
    report.series("load angle, gap 0.01 rad", &one.time, &one.load, 800);
    report.series("motor angle, gap 0.01 rad", &one.time, &one.motor, 800);
    let tail_from = one.time.partition_point(|t| *t < 38.0);
    let twist: Vec<f64> = one.motor[tail_from..].iter().zip(&one.load[tail_from..]).map(|(m, l)| m - l).collect();
    report.series("twist θm − θl, last 2 s", &one.time[tail_from..], &twist, 800);
    report.measure("limit cycle frequency (rad/s)", one.frequency).measure("twist amplitude / gap", one.amplitude / gap);
    report.above("gap: sustained limit cycle", one.amplitude, 1.2 * gap);
    report.within("amplitude doubles with gap", two.amplitude / one.amplitude, 2.0, 0.01);
    report.within("frequency independent of gap", two.frequency / one.frequency, 1.0, 0.01);

    if let Some((omega, ratio)) = describing_function_prediction(base, &registry) {
        report.measure("describing-function ω (rad/s)", omega).measure("describing-function A/gap", ratio);
        report.within("frequency matches describing function", one.frequency, omega, 0.10);
        report.within("amplitude matches describing function", one.amplitude / gap, ratio, 0.25);
    } else {
        report.holds("describing function predicts a limit cycle", false);
    }
    report
}
