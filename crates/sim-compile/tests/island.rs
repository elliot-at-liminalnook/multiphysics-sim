//! End-to-end: author a model, compile it, integrate it, read the store.

use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld};
use sim_domain_rotational::elements as rot;
use sim_dynamics::Integrator;

fn registry() -> BehaviorRegistry {
    let mut registry = BehaviorRegistry::default();
    rot::register(&mut registry).unwrap();
    registry
}

#[test]
fn torsional_oscillator_conserves_energy_and_hits_its_frequency() {
    let registry = registry();
    let mut model = ModelWorld::default();
    let rotor = model.part(&registry, "rotor", rot::INERTIA, [("inertia", 2.0), ("initial.angle", 0.3)]).unwrap();
    let spring = model.part(&registry, "spring", rot::SPRING, [("stiffness", 8.0)]).unwrap();
    let wall = model.part(&registry, "wall", rot::GROUND, []).unwrap();
    model.connect([rotor.port("shaft"), spring.port("a")]);
    model.connect([spring.port("b"), wall.port("flange")]);

    let mut runtime = Runtime::new(model, &registry, Integrator::implicit_midpoint()).unwrap();
    let angle = runtime.across_id(rotor.port("shaft"));
    let speed = runtime.state_id(rotor.behavior, "speed");
    assert_eq!(runtime.get(angle), 0.3);
    let e0 = runtime.energy();
    assert!((e0 - 0.5 * 8.0 * 0.09).abs() < 1.0e-12);

    // ω = √(k/J) = 2 rad/s: one period returns to the start.
    let period = std::f64::consts::TAU / 2.0;
    runtime.advance(period, 1.0e-3).unwrap();
    assert!((runtime.get(angle) - 0.3).abs() < 2.0e-4, "angle {}", runtime.get(angle));
    assert!(runtime.get(speed).abs() < 2.0e-3);
    assert!((runtime.energy() - e0).abs() < 1.0e-9, "energy drift {}", runtime.energy() - e0);
    // The wall's reaction is the spring torque.
    let reaction = runtime.state_id(wall.behavior, "reaction");
    assert!((runtime.get(reaction) - 8.0 * runtime.get(angle)).abs() < 1.0e-6);
}

#[test]
fn ideal_gear_constrains_angles_and_balances_power() {
    let registry = registry();
    let mut model = ModelWorld::default();
    let input = model.part(&registry, "input", rot::INERTIA, [("inertia", 1.0), ("initial.speed", 2.0), ("initial.angle", 0.0)]).unwrap();
    let gear = model.part(&registry, "gear", rot::IDEAL_GEAR, [("ratio", 4.0)]).unwrap();
    let output = model.part(&registry, "output", rot::INERTIA, [("inertia", 3.0), ("initial.speed", 0.5)]).unwrap();
    model.connect([input.port("shaft"), gear.port("input")]);
    model.connect([gear.port("output"), output.port("shaft")]);
    let mut runtime = Runtime::new(model, &registry, Integrator::implicit_midpoint()).unwrap();
    runtime.advance(1.0, 1.0e-2).unwrap();
    let theta_in = runtime.get(runtime.across_id(input.port("shaft")));
    let theta_out = runtime.get(runtime.across_id(gear.port("output")));
    assert!((theta_in - 4.0 * theta_out).abs() < 1.0e-6, "drift {}", theta_in - 4.0 * theta_out);
    // Free-spinning: reflected inertia keeps the speed constant.
    let speed = runtime.get(runtime.state_id(input.behavior, "speed"));
    assert!((speed - 2.0).abs() < 1.0e-6, "speed {speed}");
}

#[test]
fn qualified_initial_rate_reaches_its_provider_and_drives_motion() {
    let registry = registry();
    let mut model = ModelWorld::default();
    let rotor = model.part(&registry, "rotor", rot::INERTIA,
        [("inertia", 2.), ("initial.shaft.speed", 4.)]).unwrap();
    model.connect([rotor.port("shaft")]);
    let mut runtime = Runtime::new(model, &registry, Integrator::implicit_midpoint()).unwrap();
    let speed = runtime.state_id(rotor.behavior, "speed");
    assert_eq!(runtime.get(speed), 4.);
    assert_eq!(runtime.get(runtime.across_lane_id(rotor.port("shaft"), 1)), 4.);
    runtime.advance(0.1, 0.001).unwrap();
    assert!((runtime.get(runtime.across_id(rotor.port("shaft"))) - 0.4).abs() < 1e-10);
}

#[test]
fn conflicting_initial_values_and_fixed_constraints_report_both_sources() {
    let registry = registry();
    for (parameters, expected) in [
        (vec![("inertia", 2.), ("initial.speed", 2.), ("initial.shaft.speed", 4.)],
            vec!["rotor.initial.speed", "rotor.initial.shaft.speed", "rad/s"]),
        (vec![("inertia", 2.), ("initial.angle", 1.)],
            vec!["rotor.initial.angle", "fixed wall.flange.angle", "rad"]),
    ] {
        let mut model = ModelWorld::default();
        let rotor = model.part(&registry, "rotor", rot::INERTIA, parameters).unwrap();
        let wall = model.part(&registry, "wall", rot::GROUND, []).unwrap();
        model.connect([wall.port("flange"), rotor.port("shaft")]);
        let error = Runtime::new(model, &registry, Integrator::implicit_midpoint()).err().unwrap().to_string();
        assert!(error.contains("conflicting initial values"), "{error}");
        for fragment in expected { assert!(error.contains(fragment), "{error}"); }
    }
    // Repeating the same initial value is harmless, including qualified aliases.
    let mut model = ModelWorld::default();
    let rotor = model.part(&registry, "rotor", rot::INERTIA,
        [("inertia", 1.), ("initial.speed", 2.), ("initial.shaft.speed", 2.)]).unwrap();
    model.connect([rotor.port("shaft")]);
    let rt = Runtime::new(model, &registry, Integrator::implicit_midpoint()).unwrap();
    assert_eq!(rt.get(rt.state_id(rotor.behavior, "speed")), 2.);
}

#[test]
fn frame_owner_and_initial_conditions_are_independent_of_connection_order() {
    use sim_domain_multibody::contact as body;
    use sim_domain_sensing as sense;
    let run = |sensor_first| {
        let mut registry = registry();
        body::register(&mut registry).unwrap(); sense::register(&mut registry).unwrap();
        let mut model = ModelWorld::default();
        let body = model.part(&registry, "body", body::PLANAR_RIGID_BODY,
            [("mass", 1.), ("inertia", 0.1), ("gravity", 0.)]).unwrap();
        // This sensor has more states than the frame width. Counting states is
        // therefore insufficient to distinguish a body from an attachment.
        let imu = model.part(&registry, "imu", sense::IMU,
            [("period", 0.001), ("bandwidth", 20.), ("initial.frame.x", 0.75), ("initial.vx", 2.)]).unwrap();
        model.connect(if sensor_first { [imu.port("frame"), body.port("frame")] } else { [body.port("frame"), imu.port("frame")] });
        let mut rt = Runtime::new(model, &registry, Integrator::implicit_midpoint()).unwrap();
        assert_eq!(rt.get(rt.state_id(body.behavior, "x")), 0.75);
        assert_eq!(rt.get(rt.state_id(body.behavior, "vx")), 2.);
        let ids = [rt.state_id(body.behavior, "x"), rt.signal_id(imu.port("ax"))];
        let trace = rt.advance_recording(0.1, 0.001, 1, &ids).unwrap();
        assert!((trace.state.last().unwrap()[0] - 0.95).abs() < 1e-10);
        trace.state
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn frame_connections_reject_missing_or_multiple_owners() {
    use sim_domain_multibody::contact as body;
    use sim_domain_sensing as sense;
    let mut registry = registry();
    body::register(&mut registry).unwrap(); sense::register(&mut registry).unwrap();
    for count in [0, 2] {
        let mut model = ModelWorld::default();
        let imu = model.part(&registry, "imu", sense::IMU, [("period", 0.01)]).unwrap();
        let mut ports = vec![imu.port("frame")];
        for k in 0..count {
            let body = model.part(&registry, &format!("body{k}"), body::PLANAR_RIGID_BODY,
                [("mass", 1.), ("inertia", 0.1)]).unwrap();
            ports.push(body.port("frame"));
        }
        model.connect(ports);
        let error = Runtime::new(model, &registry, Integrator::implicit_midpoint()).err().unwrap().to_string();
        assert!(error.contains("imu.frame") && error.contains("exactly one frame owner") && error.contains(&format!("found {count}")), "{error}");
    }
}

#[test]
fn frame_quaternions_are_validated_after_combining_owner_and_attachment_values() {
    use sim_domain_multibody::elements as mb;
    let mut registry = registry(); mb::register(&mut registry).unwrap();
    for (qw, qx, valid) in [(0.8, 0.6, true), (0., 0., false), (2., 0., false), (1., 0.6, false)] {
        for attachment_first in [true, false] {
            let mut model = ModelWorld::default();
            let body = model.part(&registry, "body", mb::RIGID_BODY,
                [("mass", 1.), ("ixx", 1.), ("iyy", 1.), ("izz", 1.), ("initial.qx", qx)]).unwrap();
            let contact = model.part(&registry, "sphere", mb::SPHERE_CONTACT,
                [("radius", 0.1), ("stiffness", 1000.), ("initial.frame.qw", qw), ("initial.frame.z", 1.)]).unwrap();
            model.connect(if attachment_first { [contact.port("frame"), body.port("frame")] } else { [body.port("frame"), contact.port("frame")] });
            let result = Runtime::new(model, &registry, Integrator::implicit_midpoint());
            if valid {
                let mut runtime = result.unwrap();
                assert_eq!(runtime.get(runtime.state_id(body.behavior, "qw")), qw);
                assert_eq!(runtime.get(runtime.state_id(body.behavior, "qx")), qx);
                runtime.advance(0.1, 0.001).unwrap();
                assert!((runtime.get(runtime.state_id(body.behavior, "qw")) - qw).abs() < 1e-12);
            } else {
                let error = result.err().unwrap().to_string();
                for expected in ["body.frame", "sphere.frame", "initial quaternion", "unit length"] {
                    assert!(error.contains(expected), "{error}");
                }
            }
        }
    }
}
