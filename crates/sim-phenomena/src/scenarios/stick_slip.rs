//! 12. Stick–slip self-excitation — `mechanical` `contact`.
//!
//! A block on a moving belt held by a spring. Velocity-weakening friction
//! makes the sliding equilibrium unstable below a critical belt speed.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_core::{BehaviorRegistry, ModelWorld};
use sim_domain_translational::elements as tr;
use sim_dynamics::analysis::{envelope_rate, max, min, peaks};
use sim_dynamics::linear::{leading_mode, linearise};

/// Slip speed over which the Coulomb sign is regularised: two orders below
/// every slip speed the checks operate at.
pub const REGULARISATION: f64 = 5.0e-4;

#[derive(Clone, Copy)]
pub struct BeltBlock {
    pub mass: f64,
    pub stiffness: f64,
    pub damping: f64,
    pub normal_force: f64,
    pub static_friction: f64,
    pub kinetic_friction: f64,
    pub stribeck_velocity: f64,
    pub belt_speed: f64,
}

impl Default for BeltBlock {
    fn default() -> Self {
        Self { mass: 1.0, stiffness: 100.0, damping: 0.5, normal_force: 10.0, static_friction: 0.6, kinetic_friction: 0.3, stribeck_velocity: 0.1, belt_speed: 0.2 }
    }
}

impl BeltBlock {
    pub fn friction(&self) -> tr::BeltFriction {
        tr::BeltFriction { normal_force: self.normal_force, static_friction: self.static_friction, kinetic_friction: self.kinetic_friction, stribeck_velocity: self.stribeck_velocity, regularisation: REGULARISATION, belt_speed: self.belt_speed }
    }
    /// Belt speed at which −dF/dv equals the viscous damping.
    pub fn critical_speed(&self) -> f64 {
        self.stribeck_velocity * (self.normal_force * (self.static_friction - self.kinetic_friction) / (self.stribeck_velocity * self.damping)).ln()
    }
    /// Mass, spring to a wall, damper to the wall, belt friction on the mass.
    pub fn model(&self, registry: &BehaviorRegistry) -> (ModelWorld, sim_core::Instance) {
        let equilibrium = self.friction().force(self.belt_speed) / self.stiffness;
        let mut m = ModelWorld::default();
        let block = m.part(registry, "block", tr::MASS, [("mass", self.mass), ("initial.position", equilibrium), ("initial.velocity", 1.0e-3)]).unwrap();
        let spring = m.part(registry, "spring", tr::SPRING, [("stiffness", self.stiffness)]).unwrap();
        let damper = m.part(registry, "damper", tr::DAMPER, [("damping", self.damping)]).unwrap();
        let wall = m.part(registry, "wall", tr::GROUND, []).unwrap();
        let belt = m.part(registry, "belt", tr::BELT_FRICTION, [
            ("normal_force", self.normal_force), ("static_friction", self.static_friction), ("kinetic_friction", self.kinetic_friction),
            ("stribeck_velocity", self.stribeck_velocity), ("regularisation", REGULARISATION), ("belt_speed", self.belt_speed),
        ]).unwrap();
        m.connect([block.port("axis"), spring.port("a"), damper.port("a"), belt.port("axis")]);
        m.connect([spring.port("b"), damper.port("b"), wall.port("axis")]);
        (m, block)
    }
}

pub struct Outcome {
    pub growth_rate: f64,
    pub velocity_swing: f64,
    pub stick_fraction: f64,
    pub time: Vec<f64>,
    pub position: Vec<f64>,
    pub velocity: Vec<f64>,
}

pub fn run_belt(block: BeltBlock, registry: &BehaviorRegistry, duration: f64) -> Outcome {
    let (model, mass) = block.model(registry);
    let mut rt = runtime(model, registry);
    let ids = [rt.across_id(mass.port("axis")), rt.state_id(mass.behavior, "velocity")];
    let trace = record(&mut rt, duration, 2.0e-4, 5, &ids);
    let velocity = trace.column(1);
    let early_end = trace.time.partition_point(|t| *t < duration.min(8.0));
    let growth_rate = envelope_rate(&trace.time[..early_end], &velocity[..early_end]).unwrap_or(0.0);
    let tail = trace.after(duration * 0.6);
    let tail_velocity = tail.column(1);
    let stick_fraction = tail_velocity.iter().filter(|v| (*v - block.belt_speed).abs() < 0.02 * block.belt_speed).count() as f64 / tail_velocity.len() as f64;
    Outcome { growth_rate, velocity_swing: max(&tail_velocity) - min(&tail_velocity), stick_fraction, time: trace.time.clone(), position: trace.column(0), velocity }
}

/// Growth rate of the sliding equilibrium from the compiled model's linearisation.
pub fn linear_growth_rate(block: BeltBlock, registry: &BehaviorRegistry) -> f64 {
    let (model, mass) = block.model(registry);
    let rt = runtime(model, registry);
    let island = &rt.islands[0];
    let mut x = island.state.clone();
    // Linearise at the sliding equilibrium: velocity equal to none; the
    // block's velocity state at its (tiny) initial value is close enough.
    let v = island.system.state_index(mass.behavior, "velocity").unwrap();
    x[v] = 0.0;
    let rate = vec![0.0; x.len()];
    let lin = linearise(&island.system, 0.0, &x, &rate);
    leading_mode(&lin.eigenvalues()).0
}

pub fn run() -> Report {
    let mut report = Report::new("stick-slip");
    let registry = registry();
    let base = BeltBlock::default();
    let critical = base.critical_speed();
    report.measure("critical belt speed (m/s)", critical);

    let slow = run_belt(BeltBlock { belt_speed: 0.5 * critical, ..base }, &registry, 40.0);
    report.series("block velocity, belt at 0.5 v_c", &slow.time, &slow.velocity, 1500);
    report.series("block position, belt at 0.5 v_c", &slow.time, &slow.position, 1500);
    report.above("0.5 v_c: limit cycle", slow.velocity_swing, 0.5 * critical);
    report.above("0.5 v_c: sticks part of each cycle", slow.stick_fraction, 0.1);
    report.holds("0.5 v_c: velocity swing has peaks", !peaks(&slow.time, &slow.velocity).is_empty());

    let fast = run_belt(BeltBlock { belt_speed: 2.0 * critical, ..base }, &registry, 40.0);
    report.series("block velocity, belt at 2 v_c", &fast.time, &fast.velocity, 1500);
    report.below("2 v_c: perturbation decays", fast.velocity_swing, 1.0e-3);

    for (label, fraction) in [("0.9 v_c", 0.9), ("1.1 v_c", 1.1)] {
        let block = BeltBlock { belt_speed: fraction * critical, ..base };
        let predicted = -(block.damping + block.friction().slope(block.belt_speed)) / (2.0 * block.mass);
        let from_compiled = linear_growth_rate(block, &registry);
        let outcome = run_belt(block, &registry, 8.0);
        report.measure(&format!("predicted growth rate at {label}"), predicted);
        report.close(&format!("compiled linearisation growth rate at {label}"), from_compiled, predicted, 0.02);
        report.close(&format!("growth rate at {label}"), outcome.growth_rate, predicted, 0.02);
    }

    let coulomb = BeltBlock { static_friction: base.kinetic_friction, belt_speed: 0.5 * critical, ..base };
    let outcome = run_belt(coulomb, &registry, 40.0);
    report.below("Coulomb friction: block settles", outcome.velocity_swing, 1.0e-3);
    report
}
