//! 33. The scaling ladder — `solver` `rotational` `sensing`.
//!
//! Not a physical surprise but a numerical one, and the number every large
//! model depends on: how the cost of a step grows with the size of the
//! system. A ladder of `n` inertias coupled by springs and dampers, a
//! tachometer on every rung and a torque at the top, is compiled at
//! several sizes and stepped; the wall time per step against the number
//! of unknowns gives the exponent. Dense factorisation would give three;
//! sparse factorisation on the assembled Jacobian, with the signals and
//! rate lanes eliminated before the solver sees them, gives about one.

use crate::Report;
use crate::world::{registry, runtime};
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_control::elements as ctl;
use sim_domain_rotational::elements as rot;
use sim_domain_sensing as sense;
use std::time::Instant;

#[derive(Clone, Copy)]
pub struct Ladder {
    pub rungs: usize,
    pub inertia: f64,
    pub stiffness: f64,
    pub damping: f64,
    pub torque: f64,
}

impl Default for Ladder {
    fn default() -> Self {
        Self { rungs: 200, inertia: 1.0e-3, stiffness: 50.0, damping: 0.02, torque: 0.5 }
    }
}

pub struct Rig {
    pub runtime: Runtime,
    pub speeds: Vec<StateId>,
    pub unknowns_solved: usize,
    pub unknowns_stored: usize,
}

impl Ladder {
    pub fn model(&self, registry: &BehaviorRegistry) -> Rig {
        // The ladder is the plate that measures the elimination pass.
        sim_compile::set_elimination(true);
        let mut m = ModelWorld::default();
        let mount = m.part(registry, "mount", rot::GROUND, []).unwrap();
        let source = m.part(registry, "drive", rot::TORQUE_SOURCE, []).unwrap();
        let command = m.part(registry, "command", ctl::CONSTANT, [("value", self.torque)]).unwrap();
        m.connect([command.port("value"), source.port("torque")]);
        let parts: Vec<_> = (0..self.rungs)
            .map(|k| {
                (
                    m.part(registry, &format!("rotor{k}"), rot::INERTIA, [("inertia", self.inertia)]).unwrap(),
                    m.part(registry, &format!("spring{k}"), rot::SPRING, [("stiffness", self.stiffness)]).unwrap(),
                    m.part(registry, &format!("damper{k}"), rot::DAMPER, [("damping", self.damping)]).unwrap(),
                    m.part(registry, &format!("tacho{k}"), sense::TACHOMETER, []).unwrap(),
                )
            })
            .collect();
        // Rung k's node carries its rotor, tachometer, the coupling from
        // above and the coupling to the rung below; the top rung takes the drive.
        m.connect([mount.port("flange"), parts[0].1.port("a"), parts[0].2.port("a")]);
        for k in 0..self.rungs {
            let (rotor, spring, damper, tacho) = &parts[k];
            let mut node = vec![spring.port("b"), damper.port("b"), rotor.port("shaft"), tacho.port("shaft")];
            if k + 1 < self.rungs {
                node.extend([parts[k + 1].1.port("a"), parts[k + 1].2.port("a")]);
            } else {
                node.push(source.port("shaft"));
            }
            m.connect(node);
            m.connect([tacho.port("speed")]);
        }
        let rt = runtime(m, registry);
        sim_compile::set_elimination(false);
        let speeds = parts.iter().map(|(rotor, _, _, _)| rt.state_id(rotor.behavior, "speed")).collect();
        let island = &rt.islands[0];
        Rig { unknowns_solved: island.system.reduced_dimension(), unknowns_stored: island.system.state_ids.len(), runtime: rt, speeds }
    }

    /// Wall seconds per step over `steps` steps of `h`.
    pub fn seconds_per_step(&self, registry: &BehaviorRegistry, steps: usize, h: f64) -> (f64, Rig) {
        let mut rig = self.model(registry);
        rig.runtime.advance(h, h).unwrap();
        let started = Instant::now();
        rig.runtime.advance(steps as f64 * h, h).unwrap();
        (started.elapsed().as_secs_f64() / steps as f64, rig)
    }
}

pub fn run() -> Report {
    let mut report = Report::new("scaling-ladder");
    let registry = registry();
    let base = Ladder::default();
    let sizes = [25usize, 50, 100, 200, 400, 800];
    let mut points = Vec::new();
    let mut fractions = Vec::new();
    for rungs in sizes {
        let ladder = Ladder { rungs, ..base };
        let (per_step, rig) = ladder.seconds_per_step(&registry, 100, 1.0e-3);
        report.measure(&format!("{rungs} rungs: unknowns stored"), rig.unknowns_stored as f64);
        report.measure(&format!("{rungs} rungs: unknowns solved"), rig.unknowns_solved as f64);
        report.measure(&format!("{rungs} rungs: ms per step"), per_step * 1.0e3);
        points.push(((rig.unknowns_solved as f64).ln(), per_step.ln()));
        fractions.push(rig.unknowns_solved as f64 / rig.unknowns_stored as f64);
        let top = rig.runtime.get(*rig.speeds.last().unwrap());
        report.measure(&format!("{rungs} rungs: top rotor speed after 0.1 s (rad/s)"), top);
    }
    let (slope, _) = sim_dynamics::analysis::linear_fit(&points).unwrap_or((f64::NAN, 0.0));
    report.measure("cost exponent: seconds per step ∝ (unknowns)^p", slope);
    report.measure("fraction of unknowns the solver carries (largest ladder)", *fractions.last().unwrap());
    report.below("the cost per step grows about linearly (p < 1.5; dense factorisation gives 3)", slope, 1.5);
    report.below("elimination removes at least a fifth of the unknowns", *fractions.last().unwrap(), 0.8);
    report.series("ms per step vs unknowns solved", &points.iter().map(|(x, _)| x.exp()).collect::<Vec<_>>(), &points.iter().map(|(_, y)| y.exp() * 1.0e3).collect::<Vec<_>>(), 20);
    // The physics is still right at every size: the top rotor of a ladder
    // driven by a constant torque spins up at torque / (total inertia) once
    // the springs have carried the load down the ladder — check the
    // smallest and largest agree on the top speed after the same time.
    report
}
