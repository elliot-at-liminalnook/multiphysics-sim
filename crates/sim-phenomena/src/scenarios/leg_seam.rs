//! 31. The leg on the seam — `multibody` `electrical` `control` `sensing`.
//!
//! The robot leg that used to be a hand-assembled runtime, re-authored from
//! library parts: a three-link `multibody.chain` in minimal coordinates,
//! brushed motors behind PWM drivers with series current sensors, ideal
//! gears with compliant transmissions, encoders and tachometers on the
//! joints, heel and toe contacts on the foot, and a `control.external` seam
//! through which a Python process runs the same joint-space PD, gravity
//! feed-forward and current loop the old controller had. The old harness's
//! acceptance checks are re-run against it.

use crate::Report;
use crate::scenarios::language_independence::spawn_python;
use crate::world::{registry, runtime};
use sim_compile::Runtime;
use sim_core::{BehaviorId, BehaviorRegistry, Coupler, ModelWorld, StateId};
use sim_domain_bridges::elements as bridge;
use sim_domain_control::external::EXTERNAL;
use sim_domain_electrical::elements as el;
use sim_domain_multibody::chain::CHAIN;
use sim_domain_multibody::contact as ct;
use sim_domain_rotational::elements as rot;
use sim_domain_sensing as sense;
use std::f64::consts::FRAC_PI_2;

pub const JOINTS: [&str; 3] = ["hip", "knee", "ankle"];

#[derive(Clone, Copy)]
pub struct Leg {
    /// (length, mass, centre of mass from the joint) per link.
    pub links: [(f64, f64, f64); 3],
    pub hip_height: f64,
    pub reduction: [f64; 3],
    pub transmission_stiffness: f64,
    pub transmission_damping: f64,
    pub joint_damping: [f64; 3],
    pub motor_resistance: f64,
    pub motor_inductance: f64,
    pub torque_constant: f64,
    pub rotor_inertia: f64,
    pub rotor_drag: f64,
    pub supply: f64,
    pub driver_resistance: f64,
    pub friction: f64,
    pub sole_offset: f64,
    pub heel: f64,
    pub toe: f64,
    pub gravity: f64,
    pub period: f64,
    /// Joint angles in the chain's convention: link 0 along +x at zero.
    pub initial: [f64; 3],
    pub ground: bool,
    /// Penalty contacts instead of rigid ones: what a live view driving the
    /// leg on the ground wants, where four-way contact switching under a
    /// stiff current loop would otherwise stall Newton.
    pub compliant: bool,
    pub encoder_counts: f64,
    pub kp: [f64; 3],
    pub kd: [f64; 3],
}

impl Default for Leg {
    fn default() -> Self {
        // The old leg's numbers: hip −20°, knee +45°, ankle −25° from
        // the vertical/perpendicular conventions it used, which put the
        // sole flat on y = 0 with the hip 0.7734 m up.
        let (h, k, a) = (-0.349_065_850_398_865_9, 0.785_398_163_397_448_3, -0.436_332_312_998_582_4);
        Self {
            links: [(0.40, 2.5, 0.20), (0.40, 1.8, 0.20), (0.22, 0.7, 0.044)],
            hip_height: 0.773_400_32,
            reduction: [9.0, 12.0, 6.0],
            transmission_stiffness: 800.0,
            transmission_damping: 2.0,
            joint_damping: [0.05, 0.04, 0.025],
            motor_resistance: 0.35,
            motor_inductance: 0.6e-3,
            torque_constant: 0.075,
            rotor_inertia: 8.0e-4,
            rotor_drag: 2.0e-4,
            supply: 48.0,
            driver_resistance: 0.05,
            friction: 0.8,
            sole_offset: 0.035,
            heel: 0.35,
            toe: 0.65,
            gravity: 9.806_65,
            period: 1.0e-3,
            initial: [-FRAC_PI_2 + h, k, a + FRAC_PI_2],
            ground: true,
            compliant: false,
            encoder_counts: 0.0,
            kp: [75.0, 65.0, 32.0],
            kd: [8.0, 7.0, 3.5],
        }
    }
}

pub struct Plant {
    pub runtime: Runtime,
    pub seam: BehaviorId,
    pub angles: [StateId; 3],
    pub speeds: [StateId; 3],
    pub currents: [StateId; 3],
    pub tip: [StateId; 2],
    pub normal_forces: Vec<StateId>,
}

impl Leg {
    pub fn model(&self, registry: &BehaviorRegistry) -> Plant {
        let mut m = ModelWorld::default();
        let pivot = m.part(registry, "pivot", ct::PLANAR_RIGID_BODY, [("mass", 1.0e6), ("inertia", 1.0e6), ("gravity", 0.0), ("initial.y", self.hip_height)]).unwrap();
        let mut chain_params: Vec<(&'static str, f64)> = vec![("gravity", self.gravity), ("joint.hip", 0.0), ("joint.knee", 1.0), ("joint.ankle", 2.0)];
        let keys = [
            ["link0.length", "link0.mass", "link0.com", "initial.joint.hip.angle"],
            ["link1.length", "link1.mass", "link1.com", "initial.joint.knee.angle"],
            ["link2.length", "link2.mass", "link2.com", "initial.joint.ankle.angle"],
        ];
        for (k, (length, mass, com)) in self.links.iter().enumerate() {
            chain_params.extend([(keys[k][0], *length), (keys[k][1], *mass), (keys[k][2], *com), (keys[k][3], self.initial[k])]);
        }
        let chain = m.part(registry, "leg", CHAIN, chain_params).unwrap();
        m.connect([pivot.port("frame"), chain.port("base")]);
        let foot = self.links[2].0;
        let mut normal_forces = Vec::new();
        if self.ground {
            let kind = if self.compliant { ct::POINT_PLANE_COMPLIANT } else { ct::POINT_PLANE };
            let contact_parameters = |px| {
                let mut parameters = vec![("px", px), ("py", -self.sole_offset), ("friction", self.friction)];
                if self.compliant {
                    parameters.extend([("stiffness", 2.0e4), ("damping", 300.0)]);
                }
                parameters
            };
            let heel = m.part(registry, "heel", kind, contact_parameters((self.heel - 1.0) * foot)).unwrap();
            let toe = m.part(registry, "toe", kind, contact_parameters((self.toe - 1.0) * foot)).unwrap();
            m.connect([chain.port("tip"), heel.port("frame"), toe.port("frame")]);
            if !self.compliant {
                normal_forces = vec![heel.behavior, toe.behavior];
            }
        } else {
            m.connect([chain.port("tip")]);
        }
        let ground = m.part(registry, "ground", el::GROUND, []).unwrap();
        let mount = m.part(registry, "mount", rot::GROUND, []).unwrap();
        let mut seam_params: Vec<(&'static str, f64)> = vec![("period", self.period)];
        let channel_names: [[&'static str; 4]; 3] = [
            ["sense.hip.angle", "sense.hip.speed", "sense.hip.current", "act.hip.duty"],
            ["sense.knee.angle", "sense.knee.speed", "sense.knee.current", "act.knee.duty"],
            ["sense.ankle.angle", "sense.ankle.speed", "sense.ankle.current", "act.ankle.duty"],
        ];
        for names in &channel_names {
            seam_params.extend(names.iter().map(|n| (*n, 0.0)));
        }
        let seam = m.part(registry, "controller", EXTERNAL, seam_params).unwrap();
        let mut electrical_ground = vec![ground.port("pin")];
        let mut mechanical_ground = vec![mount.port("flange")];
        let mut angles = Vec::new();
        let mut speeds = Vec::new();
        let mut currents = Vec::new();
        let joint_port_names = ["joint.hip", "joint.knee", "joint.ankle"];
        for (k, joint) in JOINTS.iter().enumerate() {
            let name = |what: &str| -> String { format!("{joint}.{what}") };
            let driver = m.part(registry, &name("driver"), sense::PWM_DRIVER, [("supply", self.supply), ("resistance", self.driver_resistance)]).unwrap();
            let ammeter = m.part(registry, &name("ammeter"), sense::CURRENT_SENSOR, []).unwrap();
            let motor = m.part(registry, &name("motor"), bridge::BRUSHED_MOTOR, [("resistance", self.motor_resistance), ("inductance", self.motor_inductance), ("torque_constant", self.torque_constant), ("back_emf_constant", self.torque_constant)]).unwrap();
            let rotor = m.part(registry, &name("rotor"), rot::INERTIA, [("inertia", self.rotor_inertia), ("damping", self.rotor_drag)]).unwrap();
            let gear = m.part(registry, &name("gear"), rot::IDEAL_GEAR, [("ratio", self.reduction[k]), ("initial.input.angle", self.reduction[k] * self.initial[k]), ("initial.output.angle", self.initial[k])]).unwrap();
            let spring = m.part(registry, &name("spring"), rot::SPRING, [("stiffness", self.transmission_stiffness)]).unwrap();
            let damper = m.part(registry, &name("damper"), rot::DAMPER, [("damping", self.transmission_damping)]).unwrap();
            let friction = m.part(registry, &name("friction"), rot::DAMPER, [("damping", self.joint_damping[k])]).unwrap();
            let encoder = m.part(registry, &name("encoder"), sense::ENCODER, [("counts", self.encoder_counts), ("period", if self.encoder_counts > 0. { self.period } else { 0. })]).unwrap();
            let tacho = m.part(registry, &name("tacho"), sense::TACHOMETER, []).unwrap();
            m.connect([driver.port("p"), ammeter.port("p")]);
            m.connect([ammeter.port("n"), motor.port("p")]);
            electrical_ground.extend([motor.port("n"), driver.port("n")]);
            m.connect([motor.port("shaft"), rotor.port("shaft"), gear.port("input")]);
            m.connect([gear.port("output"), spring.port("a"), damper.port("a")]);
            m.connect([chain.port(joint_port_names[k]), spring.port("b"), damper.port("b"), friction.port("a"), encoder.port("shaft"), tacho.port("shaft")]);
            mechanical_ground.extend([motor.port("case"), friction.port("b")]);
            m.connect([encoder.port("angle"), seam.port(channel_names[k][0])]);
            m.connect([tacho.port("speed"), seam.port(channel_names[k][1])]);
            m.connect([ammeter.port("current"), seam.port(channel_names[k][2])]);
            m.connect([seam.port(channel_names[k][3]), driver.port("duty")]);
            angles.push(chain.port(joint_port_names[k]));
            speeds.push((chain.behavior, format!("{joint}.speed")));
            currents.push(ammeter.port("current"));
        }
        m.connect(electrical_ground);
        m.connect(mechanical_ground);
        let runtime = runtime(m, registry);
        let angles = [0, 1, 2].map(|k| runtime.across_id(angles[k]));
        let speeds = [0, 1, 2].map(|k| runtime.state_id(speeds[k].0, &speeds[k].1));
        let currents = [0, 1, 2].map(|k| runtime.signal_id(currents[k]));
        let tip = [runtime.state_id(chain.behavior, "tip.x"), runtime.state_id(chain.behavior, "tip.y")];
        let normal_forces = normal_forces.iter().map(|b| runtime.state_id(*b, "normal_force")).collect();
        Plant { runtime, seam: seam.behavior, angles, speeds, currents, tip, normal_forces }
    }

    fn joined(values: &[f64]) -> String {
        values.iter().map(|v| format!("{v}")).collect::<Vec<_>>().join(",")
    }

    /// The Python controller for this leg: hold `hold` until `step_at`, then `target`.
    pub fn controller(&self, target: [f64; 3], hold: [f64; 3], step_at: f64, off: bool) -> std::io::Result<Box<dyn Coupler>> {
        let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../clients/python/examples/leg_controller.py");
        let links = self.links.iter().map(|(l, m, c)| format!("{l}:{m}:{c}")).collect::<Vec<_>>().join(",");
        // `--flag=value`: a list starting with a minus is not an option.
        let owned = vec![
            format!("--target={}", Self::joined(&target)),
            format!("--hold={}", Self::joined(&hold)),
            format!("--step-at={step_at}"),
            format!("--kp={}", Self::joined(&self.kp)),
            format!("--kd={}", Self::joined(&self.kd)),
            format!("--reduction={}", Self::joined(&self.reduction)),
            format!("--links={links}"),
            format!("--kt={}", self.torque_constant),
            format!("--resistance={}", self.motor_resistance),
            format!("--supply={}", self.supply),
            format!("--gravity={}", self.gravity),
        ];
        let mut args = vec![script.to_str().unwrap(), "--joints", "hip,knee,ankle"];
        args.extend(owned.iter().map(String::as_str));
        if off {
            args.push("--off");
        }
        Ok(Box::new(spawn_python(&args)?))
    }

    /// Torque each joint must supply to hold the pose `q` still under gravity.
    pub fn gravity_torques(&self, q: [f64; 3]) -> [f64; 3] {
        let mut phi = 0.0;
        let (mut x, mut y) = (0.0, 0.0);
        let mut joints = vec![(x, y)];
        let mut coms = Vec::new();
        for ((length, _, com), theta) in self.links.iter().zip(q) {
            phi += theta;
            coms.push((x + com * phi.cos(), y + com * phi.sin()));
            x += length * phi.cos();
            y += length * phi.sin();
            joints.push((x, y));
        }
        [0, 1, 2].map(|j| (j..3).map(|i| self.links[i].1 * self.gravity * (coms[i].0 - joints[j].0)).sum())
    }
}

pub struct Run {
    pub time: Vec<f64>,
    pub angles: [Vec<f64>; 3],
    pub currents: [Vec<f64>; 3],
    pub tip_y: Vec<f64>,
    pub peak_current: [f64; 3],
    pub final_angle: [f64; 3],
    pub final_speed: [f64; 3],
    pub max_normal: f64,
}

pub fn run_leg(leg: &Leg, registry: &BehaviorRegistry, controller: Box<dyn Coupler>, duration: f64, h: f64) -> Run {
    let mut plant = leg.model(registry);
    plant.runtime.attach(plant.seam, controller).expect("seam");
    let mut ids: Vec<StateId> = plant.angles.to_vec();
    ids.extend(plant.currents);
    ids.push(plant.tip[1]);
    ids.extend(plant.normal_forces.iter().copied());
    let trace = plant.runtime.advance_recording(duration, h, 4, &ids).expect("leg runs");
    let column = |k: usize| trace.column(k).to_vec();
    let peak = |k: usize| trace.column(k).iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    let normals = (7..ids.len()).map(|k| trace.column(k).iter().cloned().fold(0.0_f64, f64::max)).fold(0.0_f64, f64::max);
    Run {
        time: trace.time.clone(),
        angles: [column(0), column(1), column(2)],
        currents: [column(3), column(4), column(5)],
        tip_y: column(6),
        peak_current: [peak(3), peak(4), peak(5)],
        final_angle: [0, 1, 2].map(|k| plant.runtime.get(plant.angles[k])),
        final_speed: [0, 1, 2].map(|k| plant.runtime.get(plant.speeds[k])),
        max_normal: normals,
    }
}

pub fn run() -> Report {
    let mut report = Report::new("leg-on-the-seam");
    let registry = registry();
    let leg = Leg::default();
    let h = 5.0e-4;
    // Pose step in free space: hold the start pose, then step every joint.
    let target = [leg.initial[0] + 0.25, leg.initial[1] - 0.35, leg.initial[2] + 0.15];
    let free = Leg { ground: false, ..leg };
    let step = run_leg(&free, &registry, free.controller(target, free.initial, 0.3, false).unwrap(), 1.5, h);
    for k in 0..3 {
        report.series(&format!("{} angle (rad), pose step", JOINTS[k]), &step.time, &step.angles[k], 600);
    }
    report.series("hip current (A), pose step", &step.time, &step.currents[0], 600);
    let pose_error = (0..3).map(|k| (step.final_angle[k] - target[k]).abs()).fold(0.0, f64::max);
    let settled = step.final_speed.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    report.measure("pose step: final pose error (rad)", pose_error);
    report.measure("pose step: settled joint speed (rad/s)", settled);
    report.below("pose step: free-space pose error (old harness: ≤ 0.12)", pose_error, 0.12);
    report.below("pose step: settled joint speed (old harness: ≤ 0.35)", settled, 0.35);
    for k in 0..3 {
        report.measure(&format!("pose step: peak {} current (A)", JOINTS[k]), step.peak_current[k]);
        report.below(&format!("pose step: {} current bounded (old harness: ≤ 18.5 A)", JOINTS[k]), step.peak_current[k], 18.5);
    }
    // Gravity hold in free space: the pose is held and the motors carry
    // the gravity torque the controller feeds forward.
    let hold = run_leg(&free, &registry, free.controller(free.initial, free.initial, 0.0, false).unwrap(), 0.6, h);
    let hold_error = (0..3).map(|k| (hold.final_angle[k] - free.initial[k]).abs()).fold(0.0, f64::max);
    report.measure("gravity hold: pose error (rad)", hold_error);
    report.below("gravity hold: gravity-compensated pose error (old harness: ≤ 0.1)", hold_error, 0.1);
    let gravity = leg.gravity_torques(leg.initial);
    for k in 0..3 {
        let expected = gravity[k].abs() / (leg.reduction[k] * leg.torque_constant);
        let observed = hold.currents[k].last().copied().unwrap_or(0.0).abs();
        report.measure(&format!("gravity hold: {} current (A)", JOINTS[k]), observed);
        report.measure(&format!("gravity hold: {} gravity torque / (N·kt) (A)", JOINTS[k]), expected);
        report.within(&format!("gravity hold: {} current is the gravity torque over N·kt", JOINTS[k]), observed, expected, 0.15);
    }
    report.above("gravity hold: holding takes motor current (old harness: ≥ 0.05 A)", hold.currents[0].last().copied().unwrap_or(0.0).abs(), 0.05);
    // Falsifier: the same leg with the controller sending zeros folds under
    // gravity onto the ground; the contacts carry it.
    let passive = run_leg(&leg, &registry, leg.controller(leg.initial, leg.initial, 0.0, true).unwrap(), 0.5, h);
    report.series("knee angle (rad), passive on the ground", &passive.time, &passive.angles[1], 600);
    report.series("foot height (m), passive on the ground", &passive.time, &passive.tip_y, 600);
    let knee_travel = (passive.final_angle[1] - leg.initial[1]).abs();
    report.measure("passive: knee travel (rad)", knee_travel);
    report.above("passive: gravity moves the passive leg (old harness: ≥ 0.05 rad)", knee_travel, 0.05);
    report.measure("passive: peak contact normal force (N)", passive.max_normal);
    report.above("passive: the foot contacts carry the leg", passive.max_normal, 1.0);
    let lowest = passive.tip_y.iter().cloned().fold(f64::INFINITY, f64::min);
    report.measure("passive: lowest foot-end height (m)", lowest);
    report.above("passive: the foot never falls through the floor", lowest, leg.sole_offset - 0.01);
    report
}
