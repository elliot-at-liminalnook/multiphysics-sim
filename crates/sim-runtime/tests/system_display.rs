//! Display bindings come from the compiled model: the winch's coupling
//! halves and armature turn with their own ports, the load slides with its
//! rope, and the power each part gives or takes balances on a live run.
use sim_runtime::system_builder;
use sim_runtime::system_session::{Command, ModelSource, SystemSession};
use sim_inspect::animation::{FlowDomain, InternalElement, scalar};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn the_winch_shows_its_drive_train_and_balances_power() {
    let (registry, _) = sim_runtime::registry_with_parts(&root().join("library/parts"));
    let doc = sim_runtime::lesson::load_system(&root().join("examples/systems-builder/worm-drive/winch.system.json"), &registry).unwrap();
    let config = system_builder::config_for(&doc);
    let compiled = system_builder::compile(&doc, &registry, config.clone()).unwrap();
    let a = compiled.animation.clone().unwrap();
    let part = |id: &str| format!("part/{id}");
    // Housings with two shafts show their pieces; whole single shafts spin.
    assert!(a.internals.iter().any(|b| b.part == part("motor") && matches!(b.element, InternalElement::Armature { .. })));
    assert_eq!(a.internals.iter().filter(|b| b.part == part("gearbox/coupling")).count(), 2);
    for spinning in ["drum", "gearbox/worm", "rotor"] {
        assert!(a.rotations.iter().any(|r| r.part == part(spinning)), "{spinning}");
    }
    // The load (and the force marker on it) slide; the end stop stays put.
    assert!(a.translations.iter().any(|t| t.part == part("load")));
    assert!(!a.translations.iter().any(|t| t.part == part("stop")));
    assert_eq!(a.tethers.len(), 1);
    assert_eq!(a.tethers[0].part, part("load"));
    for domain in [FlowDomain::Electrical, FlowDomain::Rotational, FlowDomain::Translational] {
        assert!(a.flows.iter().any(|f| f.domain == domain), "{domain:?}");
    }

    let source = ModelSource { model: compiled.flat.model.clone(), registry: registry.clone(), identities: compiled.flat.identities.clone(), source_hash: compiled.flat.source_hash.clone(), revision: doc.revision.max(1), base: None };
    let mut session = SystemSession::new(compiled.launch.run_id.clone(), config.clone(), move |c| source.build(c)).unwrap();
    session.subscribe(a.observables().into_iter().collect()).unwrap();
    session.execute(Command::Start).unwrap();
    while session.status().time < 0.8 {
        session.tick().unwrap();
    }
    let frame = session.latest();
    // Lifting: the load has risen along +Y with the drum (x = r·θ).
    let load = a.translations.iter().find(|t| t.part == part("load")).unwrap();
    let height = scalar(Some(frame), &load.observable).unwrap().value;
    let drum = a.rotations.iter().find(|r| r.part == part("drum")).unwrap();
    let angle = scalar(Some(frame), &drum.observable).unwrap().value;
    assert!(height > 0.1 && (height - 0.01 * angle).abs() < 1e-4, "height {height} m, drum {angle} rad");
    assert_eq!(load.axis, [0., 1., 0.]);
    // What one part gives, the others take: the sum over all ports is zero.
    let powers: Vec<f64> = a.flows.iter().filter_map(|f| f.power(Some(frame))).collect();
    let largest = powers.iter().map(|p| p.abs()).fold(0., f64::max);
    let sum: f64 = powers.iter().sum();
    assert!(largest > 1., "the supply delivers watts ({largest})");
    assert!(sum.abs() < 1e-6 * largest.max(1.) + 1e-6, "power balance {sum} W of {largest} W");
    // The supply gives, the motor's shaft gives mechanical power to the drive.
    let supply: f64 = a.flows.iter().filter(|f| f.component == "supply").filter_map(|f| f.power(Some(frame))).sum();
    assert!(supply < -1., "the supply delivers power ({supply} W)");
}

fn animation_of(path: &str) -> sim_inspect::animation::AnimationDescription {
    let (registry, _) = sim_runtime::registry_with_parts(&root().join("library/parts"));
    let doc = sim_runtime::lesson::load_system(&root().join(path), &registry).unwrap();
    let compiled = system_builder::compile(&doc, &registry, system_builder::config_for(&doc)).unwrap();
    compiled.animation.unwrap()
}

#[test]
fn a_gravity_link_is_drawn_as_a_swinging_arm_that_its_riders_follow() {
    let a = animation_of("lessons/imu-tilt/leg.system.json");
    // The leg is a rod and bob about its joint, not a spinning blob.
    let arm = a.internals.iter().find(|b| b.part == "part/leg" && matches!(b.element, InternalElement::Arm)).expect("leg arm");
    assert!((arm.length - 0.2).abs() < 1e-6, "reach is the centre-of-mass distance");
    assert!(!a.rotations.iter().any(|r| r.part == "part/leg"));
    // The IMU, placed down the leg, turns about the hip rather than its own centre.
    let imu = a.rotations.iter().find(|r| r.part == "part/imu").expect("imu rides the leg");
    assert_eq!(imu.pivot, arm.center);
}

#[test]
fn a_drive_wheel_rolls_and_carries_its_drive_module() {
    let a = animation_of("lessons/wheel-traction/rover.system.json");
    // The wheel turns with its axle and slides with the chassis.
    assert!(a.rotations.iter().any(|r| r.part == "part/rover/tyre"));
    let travel = a.translations.iter().find(|t| t.part == "part/rover/tyre").expect("tyre slides");
    let body = a.translations.iter().find(|t| t.part == "part/rover/body").expect("chassis slides");
    assert_eq!(travel.axis, body.axis, "the wheel travels with the chassis");
    // The rest of the drive module rides along: motor, gearbox and hub.
    for part in ["part/rover/motor", "part/rover/gearbox", "part/rover/hub"] {
        assert!(a.translations.iter().any(|t| t.part == part), "{part}");
    }
    // The throttle outside the module stays put.
    assert!(!a.translations.iter().any(|t| t.part == "part/throttle"));
}
