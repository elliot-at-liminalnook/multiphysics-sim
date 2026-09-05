//! Step-size control on a compiled plant: the swinging chain reaches the
//! same state as a fine fixed grid, in fewer steps.
use sim_core::ModelWorld;
use sim_domain_multibody::chain::CHAIN;
use sim_domain_multibody::contact as ct;
use sim_phenomena::world::{registry, runtime};

fn chain() -> (sim_compile::Runtime, sim_core::StateId) {
    let registry = registry();
    let mut m = ModelWorld::default();
    let pivot = m.part(&registry, "pivot", ct::PLANAR_RIGID_BODY, [("mass", 1.0e6), ("inertia", 1.0e6), ("gravity", 0.0)]).unwrap();
    let chain = m.part(&registry, "chain", CHAIN, [("joint.a", 0.0), ("joint.b", 1.0), ("gravity", 9.81), ("link0.mass", 1.0), ("link0.length", 0.5), ("link1.mass", 1.0), ("link1.length", 0.5), ("initial.joint.a.angle", -1.0), ("initial.joint.b.angle", -0.8)]).unwrap();
    m.connect([pivot.port("frame"), chain.port("base")]);
    m.connect([chain.port("tip")]);
    m.connect([chain.port("joint.a")]);
    m.connect([chain.port("joint.b")]);
    let rt = runtime(m, &registry);
    let tip = rt.state_id(chain.behavior, "tip.x");
    (rt, tip)
}

#[test]
fn adaptive_matches_a_fine_grid_in_fewer_steps() {
    let (mut fine, tip_fine) = chain();
    fine.advance(1.5, 2.0e-4).unwrap();
    let (mut adaptive, tip_adaptive) = chain();
    let steps = adaptive.advance_adaptive(1.5, 1.0e-3, 1.0e-4, 1.0e-5, 2.0e-2).unwrap();
    let difference = (fine.get(tip_fine) - adaptive.get(tip_adaptive)).abs();
    assert!(difference < 5.0e-3, "tip x differs by {difference}");
    assert!(steps < 3000, "{steps} adaptive steps vs 7500 fixed");
    eprintln!("adaptive: {steps} steps, tip difference {difference:.2e}");
}
