//! `multibody.chain`, the minimal-coordinate serial chain: it reproduces the
//! double pendulum's notes, conserves energy, pulls its base along, and
//! turns under a joint torque exactly as a rigid link should.

use sim_core::{Instance, ModelWorld};
use sim_domain_control::elements as ctl;
use sim_domain_multibody::chain::CHAIN;
use sim_domain_multibody::contact as ct;
use sim_domain_rotational::elements as rot;
use sim_dynamics::linear::linearise;
use sim_phenomena::world::{registry, runtime};

const L: f64 = 0.5;
const M: f64 = 1.0;
const G: f64 = 9.81;

/// A two-link chain of point masses hanging from `base`.
fn hanging(m: &mut ModelWorld, registry: &sim_core::BehaviorRegistry, base: &Instance, swing: f64) -> Instance {
    let chain = m.part(registry, "chain", CHAIN, [
        ("joint.shoulder", 0.0), ("joint.elbow", 1.0), ("gravity", G),
        ("link0.mass", M), ("link0.length", L), ("link0.com", L), ("link0.inertia", 1.0e-6 * M * L * L),
        ("link1.mass", M), ("link1.length", L), ("link1.com", L), ("link1.inertia", 1.0e-6 * M * L * L),
        ("initial.joint.shoulder.angle", -std::f64::consts::FRAC_PI_2 + swing), ("initial.joint.elbow.angle", -2.0 * swing),
    ]).unwrap();
    m.connect([base.port("frame"), chain.port("base")]);
    m.connect([chain.port("tip")]);
    m.connect([chain.port("joint.shoulder")]);
    m.connect([chain.port("joint.elbow")]);
    chain
}

#[test]
fn a_hanging_chain_sounds_the_double_pendulum_notes() {
    let registry = registry();
    let mut m = ModelWorld::default();
    let pivot = m.part(&registry, "pivot", ct::PLANAR_RIGID_BODY, [("mass", 1.0e6), ("inertia", 1.0e6), ("gravity", 0.0)]).unwrap();
    hanging(&mut m, &registry, &pivot, 0.0);
    let rt = runtime(m, &registry);
    let island = &rt.islands[0];
    let rate = vec![0.0; island.state.len()];
    let lin = linearise(&island.system, 0.0, &island.state, &rate);
    let mut freqs: Vec<f64> = lin.eigenvalues().iter().filter(|e| e.im > 0.5 && e.norm() < 1.0e3).map(|e| e.im).collect();
    freqs.sort_by(|a, b| a.total_cmp(b));
    freqs.dedup_by(|a, b| (*a - *b).abs() < 1.0e-6);
    let expected = [(2.0 - 2f64.sqrt()) * G / L, (2.0 + 2f64.sqrt()) * G / L].map(f64::sqrt);
    assert_eq!(freqs.len(), 2, "{freqs:?}");
    for (f, e) in freqs.iter().zip(expected) {
        assert!((f - e).abs() < 1.0e-3 * e, "mode {f} vs {e}");
    }
}

#[test]
fn a_swinging_chain_conserves_energy() {
    let registry = registry();
    let mut m = ModelWorld::default();
    let pivot = m.part(&registry, "pivot", ct::PLANAR_RIGID_BODY, [("mass", 1.0e6), ("inertia", 1.0e6), ("gravity", 0.0)]).unwrap();
    let chain = hanging(&mut m, &registry, &pivot, 0.6);
    let mut rt = runtime(m, &registry);
    let tip_y = rt.state_id(chain.behavior, "tip.y");
    let start = rt.energy();
    let trace = rt.advance_recording(3.0, 1.0e-3, 10, &[tip_y]).unwrap();
    let drift = trace.energy.iter().map(|e| (e - start).abs()).fold(0.0, f64::max);
    let scale = M * G * L;
    assert!(drift < 2.0e-3 * scale, "energy drift {drift} of {scale}");
    let (lo, hi) = trace.column(0).iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), y| (lo.min(*y), hi.max(*y)));
    assert!(hi - lo > 0.1 * L, "the tip swings: {lo}..{hi}");
}

#[test]
fn the_chain_drags_a_free_base_down_with_it() {
    // Weightless base body of 1 kg, two 1 kg links under gravity: the
    // assembly falls at a = 2g/3 once it hangs straight.
    let registry = registry();
    let mut m = ModelWorld::default();
    let base = m.part(&registry, "base", ct::PLANAR_RIGID_BODY, [("mass", 1.0), ("inertia", 1.0), ("gravity", 0.0)]).unwrap();
    hanging(&mut m, &registry, &base, 0.0);
    let mut rt = runtime(m, &registry);
    let vy = rt.state_id(base.behavior, "vy");
    rt.advance(0.5, 1.0e-3).unwrap();
    let expected = -2.0 * G / 3.0 * 0.5;
    assert!((rt.get(vy) - expected).abs() < 1.0e-3 * expected.abs(), "base vy {} vs {expected}", rt.get(vy));
}

#[test]
fn a_joint_torque_turns_a_link_like_a_rigid_body() {
    // One uniform link, no gravity, constant torque τ at the joint:
    // α = τ / (mL²/12 + m(L/2)²) = 3τ/(mL²).
    let registry = registry();
    let mut m = ModelWorld::default();
    let base = m.part(&registry, "base", ct::PLANAR_RIGID_BODY, [("mass", 1.0e6), ("inertia", 1.0e6), ("gravity", 0.0)]).unwrap();
    let chain = m.part(&registry, "chain", CHAIN, [("joint.hub", 0.0), ("gravity", 0.0), ("link0.mass", M), ("link0.length", L)]).unwrap();
    let source = m.part(&registry, "motor", rot::TORQUE_SOURCE, []).unwrap();
    let torque = 0.3;
    let command = m.part(&registry, "command", ctl::CONSTANT, [("value", torque)]).unwrap();
    m.connect([base.port("frame"), chain.port("base")]);
    m.connect([chain.port("tip")]);
    m.connect([chain.port("joint.hub"), source.port("shaft")]);
    m.connect([command.port("value"), source.port("torque")]);
    let mut rt = runtime(m, &registry);
    let speed = rt.state_id(chain.behavior, "hub.speed");
    rt.advance(0.2, 1.0e-3).unwrap();
    let alpha = 3.0 * torque / (M * L * L);
    let expected = alpha * 0.2;
    assert!((rt.get(speed) - expected).abs() < 1.0e-6 * expected, "speed {} vs {expected}", rt.get(speed));
}

#[test]
fn the_tip_frame_follows_the_links() {
    let registry = registry();
    let mut m = ModelWorld::default();
    let pivot = m.part(&registry, "pivot", ct::PLANAR_RIGID_BODY, [("mass", 1.0e6), ("inertia", 1.0e6), ("gravity", 0.0)]).unwrap();
    let chain = hanging(&mut m, &registry, &pivot, 0.0);
    let rt = runtime(m, &registry);
    let (x, y, theta) = (rt.get(rt.state_id(chain.behavior, "tip.x")), rt.get(rt.state_id(chain.behavior, "tip.y")), rt.get(rt.state_id(chain.behavior, "tip.theta")));
    assert!(x.abs() < 1.0e-9 && (y + 2.0 * L).abs() < 1.0e-9, "tip at ({x}, {y})");
    assert!((theta + std::f64::consts::FRAC_PI_2).abs() < 1.0e-9, "tip angle {theta}");
}
