//! 32. The quadruped's trot — `multibody` `control` `sensing`.
//!
//! A planar quadruped from library parts: a floating rigid body, four
//! two-link `multibody.chain` legs hanging from two hips (a left and a
//! right leg at each), `actuator.servo` torque sources at every joint,
//! encoders and tachometers into a `control.external` seam, and a compliant
//! point contact under each foot. A Python process closes the loop with a trot:
//! diagonal pairs alternate, stance feet sweep backward under their hips,
//! swing feet return along an arc, inverse kinematics turns foot targets
//! into joint targets and PD into torques. The published number is
//! kinematic: with stance feet planted, the body advances one stride per
//! gait period.

use crate::Report;
use crate::scenarios::language_independence::spawn_python;
use crate::world::{damped_runtime, registry};
use sim_compile::Runtime;
use sim_core::{BehaviorId, BehaviorRegistry, Coupler, ModelWorld, StateId};
use sim_domain_control::external::EXTERNAL;
use sim_domain_multibody::chain::CHAIN;
use sim_domain_multibody::contact as ct;
use sim_domain_sensing as sense;

pub const LEGS: [&str; 4] = ["fl", "fr", "rl", "rr"];

#[derive(Clone, Copy)]
pub struct Quadruped {
    pub body_mass: f64,
    pub body_inertia: f64,
    pub hip_x: f64,
    pub hip_y: f64,
    pub thigh: (f64, f64),
    pub shank: (f64, f64),
    pub friction: f64,
    pub gravity: f64,
    pub period: f64,
    pub servo_bandwidth: f64,
    pub torque_limit: f64,
    pub stride: f64,
    pub gait_period: f64,
    pub lift: f64,
    pub start: f64,
    pub kp: f64,
    pub kd: f64,
}

impl Default for Quadruped {
    fn default() -> Self {
        Self {
            body_mass: 12.0,
            body_inertia: 0.5,
            hip_x: 0.3,
            hip_y: -0.05,
            thigh: (0.25, 1.0),
            shank: (0.25, 0.6),
            friction: 0.8,
            gravity: 9.81,
            period: 4.0e-3,
            servo_bandwidth: 50.0,
            torque_limit: 30.0,
            stride: 0.12,
            gait_period: 0.6,
            lift: 0.03,
            start: 0.5,
            kp: 150.0,
            kd: 6.0,
        }
    }
}

pub struct Plant {
    pub runtime: Runtime,
    pub seam: BehaviorId,
    pub body: [StateId; 6],
    /// Per leg: hip angle, knee angle (node across values).
    pub joints: [[StateId; 2]; 4],
    pub feet: [[StateId; 2]; 4],
}

impl Quadruped {
    /// Standing pose: foot straight under the hip at height `h`; the same
    /// inverse kinematics the controller uses.
    pub fn standing(&self) -> (f64, f64, f64) {
        let (l1, l2) = (self.thigh.0, self.shank.0);
        let height = 0.478;
        let c = ((height * height - l1 * l1 - l2 * l2) / (2.0 * l1 * l2)).clamp(-1.0, 1.0);
        let knee = -c.acos();
        let hip = (-height).atan2(0.0) - (l2 * knee.sin()).atan2(l1 + l2 * knee.cos());
        (hip, knee, height)
    }

    pub fn model(&self, registry: &BehaviorRegistry) -> Plant {
        let (hip0, knee0, height) = self.standing();
        let mut m = ModelWorld::default();
        let body = m.part(registry, "body", ct::PLANAR_RIGID_BODY, [("mass", self.body_mass), ("inertia", self.body_inertia), ("gravity", self.gravity), ("initial.y", height - self.hip_y - 0.002)]).unwrap();
        let mut seam_params: Vec<(&'static str, f64)> = vec![("period", self.period)];
        let names: [[&'static str; 4]; 4] = [
            ["sense.fl.hip.angle", "sense.fl.hip.speed", "sense.fl.knee.angle", "sense.fl.knee.speed"],
            ["sense.fr.hip.angle", "sense.fr.hip.speed", "sense.fr.knee.angle", "sense.fr.knee.speed"],
            ["sense.rl.hip.angle", "sense.rl.hip.speed", "sense.rl.knee.angle", "sense.rl.knee.speed"],
            ["sense.rr.hip.angle", "sense.rr.hip.speed", "sense.rr.knee.angle", "sense.rr.knee.speed"],
        ];
        let acts: [[&'static str; 2]; 4] = [["act.fl.hip.torque", "act.fl.knee.torque"], ["act.fr.hip.torque", "act.fr.knee.torque"], ["act.rl.hip.torque", "act.rl.knee.torque"], ["act.rr.hip.torque", "act.rr.knee.torque"]];
        for k in 0..4 {
            seam_params.extend(names[k].iter().map(|n| (*n, 0.0)));
            seam_params.extend(acts[k].iter().map(|n| (*n, 0.0)));
        }
        let seam = m.part(registry, "controller", EXTERNAL, seam_params).unwrap();
        let mut body_ports = vec![body.port("frame")];
        let mut joints = Vec::new();
        let mut feet = Vec::new();
        for (k, leg) in LEGS.iter().enumerate() {
            let front = leg.starts_with('f');
            let chain = m.part(registry, leg, CHAIN, [
                ("gravity", self.gravity), ("ax", if front { self.hip_x } else { -self.hip_x }), ("ay", self.hip_y),
                ("joint.hip", 0.0), ("joint.knee", 1.0),
                ("link0.length", self.thigh.0), ("link0.mass", self.thigh.1),
                ("link1.length", self.shank.0), ("link1.mass", self.shank.1),
                ("initial.joint.hip.angle", hip0), ("initial.joint.knee.angle", knee0),
            ]).unwrap();
            body_ports.push(chain.port("base"));
            let foot = m.part(registry, &format!("{leg}.foot"), ct::POINT_PLANE_COMPLIANT, [("friction", self.friction), ("stiffness", 2.0e4), ("damping", 300.0)]).unwrap();
            m.connect([chain.port("tip"), foot.port("frame")]);
            let mut pair = Vec::new();
            for (j, joint) in ["hip", "knee"].iter().enumerate() {
                let servo = m.part(registry, &format!("{leg}.{joint}.servo"), sense::SERVO, [("bandwidth", self.servo_bandwidth), ("torque_limit", self.torque_limit)]).unwrap();
                let encoder = m.part(registry, &format!("{leg}.{joint}.encoder"), sense::ENCODER, []).unwrap();
                let tacho = m.part(registry, &format!("{leg}.{joint}.tacho"), sense::TACHOMETER, []).unwrap();
                let port = if j == 0 { "joint.hip" } else { "joint.knee" };
                m.connect([chain.port(port), servo.port("shaft"), encoder.port("shaft"), tacho.port("shaft")]);
                m.connect([encoder.port("angle"), seam.port(names[k][2 * j])]);
                m.connect([tacho.port("speed"), seam.port(names[k][2 * j + 1])]);
                m.connect([seam.port(acts[k][j]), servo.port("command")]);
                m.connect([servo.port("current")]);
                pair.push(chain.port(port));
            }
            joints.push([pair[0], pair[1]]);
            feet.push((chain.behavior, "tip.x", "tip.y"));
        }
        m.connect(body_ports);
        // Four unilateral contacts: the L-stable rule damps the impact ringing.
        let runtime = damped_runtime(m, registry);
        let body_ids = ["x", "y", "theta", "vx", "vy", "omega"].map(|n| runtime.state_id(body.behavior, n));
        let joints = [0, 1, 2, 3].map(|k| [runtime.across_id(joints[k][0]), runtime.across_id(joints[k][1])]);
        let feet = [0, 1, 2, 3].map(|k| [runtime.state_id(feet[k].0, feet[k].1), runtime.state_id(feet[k].0, feet[k].2)]);
        Plant { runtime, seam: seam.behavior, body: body_ids, joints, feet }
    }

    /// The Python gait controller (the plate's reference).
    pub fn controller(&self, stride: f64) -> std::io::Result<Box<dyn Coupler>> {
        self.controller_in(stride, Lang::Python)
    }

    /// The same gait in `lang`. The C program is compiled on first use into
    /// `target/simloop/` from `clients/c/examples/quadruped_gait.c`.
    pub fn controller_in(&self, stride: f64, lang: Lang) -> std::io::Result<Box<dyn Coupler>> {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let (_, _, height) = self.standing();
        let owned = vec![
            format!("--stride={stride}"), format!("--period={}", self.gait_period), format!("--lift={}", self.lift), format!("--height={height}"),
            format!("--l1={}", self.thigh.0), format!("--l2={}", self.shank.0), format!("--kp={}", self.kp), format!("--kd={}", self.kd), format!("--start={}", self.start),
        ];
        match lang {
            Lang::Python => {
                let script = root.join("clients/python/examples/quadruped_gait.py");
                let mut args = vec![script.to_str().unwrap()];
                args.extend(owned.iter().map(String::as_str));
                Ok(Box::new(spawn_python(&args)?))
            }
            Lang::Dylib => {
                let mut coupler = sim_couple::DynamicCoupler::compile(root.join("clients/c/examples/quadruped_gait_dl.c"), root.join("target/simloop/libquadruped_gait.dylib")).map_err(std::io::Error::other)?;
                coupler.configure(stride, self.gait_period, self.lift, height, self.kp, self.kd, self.start).map_err(std::io::Error::other)?;
                Ok(Box::new(coupler))
            }
            Lang::C => {
                let args: Vec<&str> = owned.iter().map(String::as_str).collect();
                Ok(Box::new(sim_couple::c(root.join("clients"), root.join("clients/c/examples/quadruped_gait.c"), root.join("target/simloop/quadruped_gait"), &args)?))
            }
        }
    }
}

/// Which client runs the gait.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Python,
    C,
    /// The C law as a shared library called in-process (no pipe at all).
    Dylib,
}

pub struct Walk {
    pub time: Vec<f64>,
    pub x: Vec<f64>,
    pub height: Vec<f64>,
    pub pitch: Vec<f64>,
}

pub fn walk(quadruped: &Quadruped, registry: &BehaviorRegistry, stride: f64, duration: f64) -> Walk {
    let mut plant = quadruped.model(registry);
    plant.runtime.attach(plant.seam, quadruped.controller(stride).unwrap()).expect("seam");
    let ids = [plant.body[0], plant.body[1], plant.body[2]];
    let trace = plant.runtime.advance_recording(duration, 1.0e-3, 8, &ids).expect("the quadruped runs");
    Walk { time: trace.time.clone(), x: trace.column(0), height: trace.column(1), pitch: trace.column(2) }
}

pub fn run() -> Report {
    let mut report = Report::new("quadruped-gait");
    let registry = registry();
    let q = Quadruped::default();
    let duration = q.start + 5.0 * q.gait_period;
    // The falsifier first: the same gait with zero stride marches on the
    // spot, and what little it creeps is the compliant legs' ratchet.
    let march = walk(&q, &registry, 0.0, duration);
    report.series("body x (m), zero stride", &march.time, &march.x, 600);
    let drift = march.x.last().unwrap() - march.x[0];
    let (lo_march, _) = march.height.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), h| (lo.min(*h), hi.max(*h)));
    let standing = march.height[0];
    report.measure("standing height (m)", standing);
    report.measure("zero stride: creep over the run (m)", drift);
    let trot = walk(&q, &registry, q.stride, duration);
    report.series("body x (m), trotting", &trot.time, &trot.x, 600);
    report.series("body height (m), trotting", &trot.time, &trot.height, 600);
    report.series("body pitch (rad), trotting", &trot.time, &trot.pitch, 600);
    let (lowest, highest) = trot.height.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), h| (lo.min(*h), hi.max(*h)));
    let worst_pitch = trot.pitch.iter().fold(0.0_f64, |m, p| m.max(p.abs()));
    report.measure("lowest body height while trotting (m)", lowest);
    report.measure("highest body height while trotting (m)", highest);
    report.measure("worst pitch while trotting (rad)", worst_pitch);
    report.holds("the body stays up: height within 25% of standing", lowest > 0.75 * standing && highest < 1.25 * standing);
    report.below("the body stays level: pitch under 0.35 rad", worst_pitch, 0.35);
    // Progress from the start of the trot, net of the creep, against the
    // kinematic prediction of one stride per gait period.
    let at = |w: &Walk, t: f64| w.time.iter().position(|x| *x >= t).map(|i| w.x[i]).unwrap_or(*w.x.last().unwrap());
    let progressed = trot.x.last().unwrap() - at(&trot, q.start);
    let creep = march.x.last().unwrap() - at(&march, q.start);
    let predicted = q.stride * 5.0;
    report.measure("distance walked over five gait periods (m)", progressed);
    report.measure("net of the zero-stride creep (m)", progressed - creep);
    report.measure("stride × periods (m)", predicted);
    report.above("the quadruped walks forward", progressed, 0.5 * predicted);
    report.within("net advance is about a stride per period", progressed - creep, predicted, 0.5);
    report.below("falsifier: zero stride goes nowhere", drift.abs(), 0.25 * predicted);
    report.above("falsifier: … and still stands", lo_march, 0.75 * standing);
    // The same gait as a shared library called in-process: no seam frames.
    if let Ok(c) = q.controller_in(q.stride, Lang::Dylib) {
        let mut plant = q.model(&registry);
        plant.runtime.attach(plant.seam, c).expect("seam");
        let started = std::time::Instant::now();
        let trace = plant.runtime.advance_recording(duration, 1.0e-3, 8, &[plant.body[0]]).expect("the quadruped runs");
        let worst = trace.column(0).iter().zip(&trot.x).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        report.measure("worst |Python − in-process C| body x (m)", worst);
        report.measure("in-process C run wall time (s)", started.elapsed().as_secs_f64());
        report.below("the in-process controller walks the same walk", worst, 1.0e-6);
    }
    // The same gait written in C, over the same seam: the robot cannot tell.
    match q.controller_in(q.stride, Lang::C) {
        Ok(c) => {
            let mut plant = q.model(&registry);
            plant.runtime.attach(plant.seam, c).expect("seam");
            let trace = plant.runtime.advance_recording(duration, 1.0e-3, 8, &[plant.body[0]]).expect("the quadruped runs");
            let worst = trace.column(0).iter().zip(&trot.x).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
            report.series("body x (m), trotting, C controller", &trace.time, &trace.column(0), 600);
            report.measure("worst |Python − C| body x (m)", worst);
            report.below("the C controller walks the same walk", worst, 1.0e-6);
        }
        Err(e) => {
            report.holds(&format!("a C compiler is available ({e})"), false);
        }
    }
    report
}
