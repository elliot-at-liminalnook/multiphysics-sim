//! 34. Cruise control on a hill — `multibody` `wheels` `control` `seam`.
//!
//! A two-wheel car: a body on two `contact.wheel`s, the rear axle driven
//! by a servo the seam commands from a PI speed law on the axle
//! tachometer. On the flat it holds the set speed with almost no torque;
//! on a hill the integrator finds the torque the grade demands,
//! `m·g·sin(θ)·r`, and the speed stays. Hold the flat-road torque on the
//! hill instead and the car slows to a crawl.

use crate::Report;
use crate::world::{damped_runtime, registry};
use sim_compile::Runtime;
use sim_core::{BehaviorId, BehaviorRegistry, Coupler, FnCoupler, ModelWorld, StateId};
use sim_domain_control::external::EXTERNAL;
use sim_domain_multibody::contact as ct;
use sim_domain_sensing as sense;

#[derive(Clone, Copy)]
pub struct Car {
    pub mass: f64,
    pub inertia: f64,
    pub wheelbase: f64,
    pub radius: f64,
    pub wheel_inertia: f64,
    pub slope: f64,
    pub setpoint: f64,
    pub kp: f64,
    pub ki: f64,
    pub torque_limit: f64,
    pub period: f64,
}

impl Default for Car {
    fn default() -> Self {
        Self { mass: 800.0, inertia: 300.0, wheelbase: 2.4, radius: 0.3, wheel_inertia: 1.0, slope: 0.0, setpoint: 10.0, kp: 60.0, ki: 40.0, torque_limit: 600.0, period: 1.0e-2 }
    }
}

pub struct Plant {
    pub runtime: Runtime,
    pub seam: BehaviorId,
    pub vx: StateId,
    pub torque: StateId,
}

impl Car {
    pub fn model(&self, registry: &BehaviorRegistry) -> Plant {
        let mut m = ModelWorld::default();
        let body = m.part(registry, "body", ct::PLANAR_RIGID_BODY, [("mass", self.mass), ("inertia", self.inertia), ("gravity", 9.81), ("slope", self.slope), ("initial.y", self.radius + 0.2)]).unwrap();
        let wheel = |m: &mut ModelWorld, name: &str, px: f64| m.part(registry, name, ct::WHEEL, [("px", px), ("py", -0.2), ("radius", self.radius), ("inertia", self.wheel_inertia)]).unwrap();
        let rear = wheel(&mut m, "rear", -0.5 * self.wheelbase);
        let front = wheel(&mut m, "front", 0.5 * self.wheelbase);
        let servo = m.part(registry, "servo", sense::SERVO, [("bandwidth", 10.0), ("torque_limit", self.torque_limit)]).unwrap();
        let tacho = m.part(registry, "tacho", sense::TACHOMETER, []).unwrap();
        let seam = m.part(registry, "cruise", EXTERNAL, [("period", self.period), ("sense.axle_speed", 0.0), ("act.torque", 0.0)]).unwrap();
        m.connect([body.port("frame"), rear.port("frame"), front.port("frame")]);
        m.connect([rear.port("axle"), servo.port("shaft"), tacho.port("shaft")]);
        m.connect([front.port("axle")]);
        m.connect([servo.port("current")]);
        m.connect([tacho.port("speed"), seam.port("sense.axle_speed")]);
        m.connect([seam.port("act.torque"), servo.port("command")]);
        let runtime = damped_runtime(m, registry);
        let vx = runtime.state_id(body.behavior, "vx");
        let torque = runtime.state_id(servo.behavior, "torque");
        Plant { runtime, seam: seam.behavior, vx, torque }
    }

    /// PI on axle speed; forward is −x, so the target spin is +v/r.
    pub fn controller(&self) -> Box<dyn Coupler> {
        let (kp, ki, period, target, limit) = (self.kp, self.ki, self.period, self.setpoint / self.radius, self.torque_limit);
        let mut integral = 0.0;
        Box::new(FnCoupler(move |_t: f64, s: &[f64], a: &mut [f64]| {
            let error = target - s[0];
            let raw = kp * error + ki * integral;
            let out = raw.clamp(-limit, limit);
            if out == raw || (raw > 0.0) != (error > 0.0) {
                integral += error * period;
            }
            a[0] = out;
        }))
    }

    /// Constant torque, open loop.
    pub fn fixed_torque(torque: f64) -> Box<dyn Coupler> {
        Box::new(FnCoupler(move |_t: f64, _s: &[f64], a: &mut [f64]| a[0] = torque))
    }

    pub fn drive(&self, registry: &BehaviorRegistry, controller: Box<dyn Coupler>, duration: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let mut plant = self.model(registry);
        plant.runtime.attach(plant.seam, controller).expect("seam");
        let trace = plant.runtime.advance_recording(duration, 2.0e-3, 5, &[plant.vx, plant.torque]).expect("the car drives");
        (trace.time.clone(), trace.column(0).iter().map(|v| -v).collect(), trace.column(1))
    }
}

pub fn run() -> Report {
    let mut report = Report::new("cruise-control");
    let registry = registry();
    let car = Car::default();
    let (time, speed, torque) = car.drive(&registry, car.controller(), 12.0);
    report.series("speed (m/s), flat road", &time, &speed, 600);
    report.series("axle torque (N·m), flat road", &time, &torque, 600);
    let flat_speed = *speed.last().unwrap();
    let flat_torque = *torque.last().unwrap();
    report.measure("flat: final speed (m/s)", flat_speed);
    report.measure("flat: steady torque (N·m)", flat_torque);
    report.within("flat: the car holds the set speed", flat_speed, car.setpoint, 0.02);
    let hill = Car { slope: 0.06, ..car };
    let (time, speed, torque) = hill.drive(&registry, hill.controller(), 12.0);
    report.series("speed (m/s), 6% hill", &time, &speed, 600);
    report.series("axle torque (N·m), 6% hill", &time, &torque, 600);
    let hill_speed = *speed.last().unwrap();
    let hill_torque = *torque.last().unwrap();
    let grade_torque = hill.mass * 9.81 * hill.slope.sin() * hill.radius;
    report.measure("hill: final speed (m/s)", hill_speed);
    report.measure("hill: steady torque (N·m)", hill_torque);
    report.measure("hill: m·g·sin(θ)·r (N·m)", grade_torque);
    report.within("hill: the car still holds the set speed", hill_speed, car.setpoint, 0.02);
    report.within("hill: the integrator found the grade torque", (hill_torque - flat_torque).abs(), grade_torque, 0.1);
    // Falsifier: the flat road's torque, held open loop, on the hill.
    let (time, speed, _) = hill.drive(&registry, Car::fixed_torque(flat_torque), 12.0);
    report.series("speed (m/s), 6% hill at the flat road's torque", &time, &speed, 600);
    report.measure("open loop on the hill: final speed (m/s)", *speed.last().unwrap());
    report.below("falsifier: open loop, the hill wins (speed lost)", *speed.last().unwrap(), 0.8 * car.setpoint);
    report
}
