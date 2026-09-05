//! `contact.wheel`: a driven wheel rolls a body along the plane at
//! a = τ / (r·(m + I/r²)) without slipping.
use sim_core::ModelWorld;
use sim_domain_control::elements as ctl;
use sim_domain_multibody::contact as ct;
use sim_domain_rotational::elements as rot;
use sim_phenomena::world::{damped_runtime, registry};

#[test]
fn a_driven_wheel_rolls_the_body_forward() {
    let registry = registry();
    let (mass, radius, inertia, torque) = (10.0, 0.3, 0.2, 3.0);
    let mut m = ModelWorld::default();
    // The body rests on one wheel (a unicycle held level by a large inertia).
    let body = m.part(&registry, "body", ct::PLANAR_RIGID_BODY, [("mass", mass), ("inertia", 1.0e3), ("gravity", 9.81), ("initial.y", radius)]).unwrap();
    let wheel = m.part(&registry, "wheel", ct::WHEEL, [("radius", radius), ("inertia", inertia)]).unwrap();
    let motor = m.part(&registry, "motor", rot::TORQUE_SOURCE, []).unwrap();
    let command = m.part(&registry, "command", ctl::CONSTANT, [("value", torque)]).unwrap();
    m.connect([body.port("frame"), wheel.port("frame")]);
    m.connect([wheel.port("axle"), motor.port("shaft")]);
    m.connect([command.port("value"), motor.port("torque")]);
    let mut rt = damped_runtime(m, &registry);
    let vx = rt.state_id(body.behavior, "vx");
    let spin = rt.state_id(wheel.behavior, "spin");
    rt.advance(0.5, 5.0e-4).unwrap();
    // A positive axle torque spins the wheel positive (counter-clockwise, y
    // up), whose contact point moves +x, so traction drives the body −x.
    let expected = -torque / (radius * (mass + inertia / (radius * radius))) * 0.5;
    let observed = rt.get(vx);
    assert!((observed - expected).abs() < 0.05 * expected.abs(), "vx {observed} vs {expected}");
    assert!((rt.get(spin) * radius + observed).abs() < 0.02 * expected.abs(), "rolling: slip {}", rt.get(spin) * radius + observed);
}

#[test]
fn drag_slows_a_coasting_body_as_one_over_t() {
    // Pure quadratic drag: v(t) = v0 / (1 + c·v0·t/m).
    let registry = registry();
    let mut m = ModelWorld::default();
    let body = m.part(&registry, "body", ct::PLANAR_RIGID_BODY, [("mass", 2.0), ("inertia", 1.0), ("gravity", 0.0), ("initial.vx", 10.0)]).unwrap();
    let drag = m.part(&registry, "drag", ct::DRAG, [("coefficient", 0.5)]).unwrap();
    m.connect([body.port("frame"), drag.port("frame")]);
    let mut rt = damped_runtime(m, &registry);
    let vx = rt.state_id(body.behavior, "vx");
    rt.advance(2.0, 1.0e-3).unwrap();
    let expected = 10.0 / (1.0 + 0.5 * 10.0 * 2.0 / 2.0);
    assert!((rt.get(vx) - expected).abs() < 0.01 * expected, "vx {} vs {expected}", rt.get(vx));
}
