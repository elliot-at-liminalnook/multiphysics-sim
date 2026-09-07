mod common;
use common::*;
use sim_core::Behavior;
use sim_domain_robot::{Articulated, Options};
use std::sync::Arc;

fn model() -> Arc<sim_domain_robot::model::PhysicalModel> {
    let mut model = empty_model();
    model.gravity = [0.0; 3];
    let mut body = box_link("body", [0.1; 3], 1.0, [0.0, 0.0, 0.0499], false);
    body.collision.hull = vec![[0.0, 0.0, -0.05]];
    body.collision.vertices = body.collision.hull.clone();
    model.links.push(body);
    Arc::new(model)
}

#[test]
fn floor_dissipation_preserves_static_load_and_opposes_normal_motion() {
    // One spring penetrated 0.1 mm supports 20 N. At alpha=100 s/m,
    // approaching at 10 mm/s doubles load; separating at 5 mm/s halves it.
    for (speed, expected_force) in [(-0.01, 40.0), (0.0, 20.0), (0.005, 10.0), (0.02, 0.0)] {
        let art = Articulated::new(model(), &Options {
            flex: false,
            floor_dissipation_s_m: Some(100.0),
            initial_twist: [0.0, 0.0, speed, 0.0, 0.0, 0.0],
            ..Default::default()
        }).unwrap();
        let g = art.generalized(art.states().iter().map(|s| s.initial).collect(),
            vec![0.0; art.state_count], &[0.0], vec![]);
        let force: f64 = art.evaluate(&g).contacts.iter().map(|c| c.force.z).sum();
        assert!((force - expected_force).abs() < 1e-9, "{speed}: {force}");
        // Force in excess of the conservative spring does no positive work.
        assert!((force - 20.0) * speed <= 1e-12);
        // The override applies only to the floor, not inter-link damping.
        assert_eq!(art.restitution_damping, 0.2);
    }
}

#[test]
fn floor_dissipation_default_is_compatible_and_invalid_values_are_rejected() {
    let mut registry = sim_core::BehaviorRegistry::default();
    sim_domain_robot::register(&mut registry).unwrap();
    let descriptor = registry.get(&sim_domain_robot::articulated::ARTICULATED.into()).unwrap();
    let parameter = descriptor.parameters.as_ref().unwrap().iter()
        .find(|p| p.name == "floor.dissipation").unwrap();
    assert_eq!(parameter.unit, "s/m");
    assert_eq!(parameter.default, None);
    assert!(!parameter.required);
    assert_eq!(parameter.minimum, Some(0.0));
    let original = Articulated::new(model(), &Options::default()).unwrap();
    assert_eq!(original.floor_dissipation_s_m, original.restitution_damping);
    for value in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(Articulated::new(model(), &Options {
            floor_dissipation_s_m: Some(value), ..Default::default()
        }).is_err());
    }
    let explicit = Articulated::new(model(), &Options {
        floor_dissipation_s_m: Some(original.floor_dissipation_s_m),
        ..Default::default()
    }).unwrap();
    let g = original.generalized(original.states().iter().map(|s| s.initial).collect(),
        vec![0.0; original.state_count], &[0.0], vec![]);
    assert_eq!(original.evaluate(&g).base_wrench, explicit.evaluate(&g).base_wrench);
}

#[test]
fn registered_floor_dissipation_reaches_the_compiled_equations() {
    // At fixed initial penetration, compare initial acceleration to F/m.
    // This exercises the registry factory, not the direct Articulated options.
    for (coefficient, expected_acceleration) in [(0.0, 20.0), (0.2, 20.04), (100.0, 40.0)] {
        let mut rig = Rig::new(model().as_ref().clone(), &[
            ("floor.dissipation", coefficient), ("initial.base.vz", -0.01),
        ], euler());
        let before = rig.state("base.vz");
        let h = 1e-7;
        rig.runtime.advance(h, h).unwrap();
        let acceleration = (rig.state("base.vz") - before) / h;
        assert!((acceleration - expected_acceleration).abs() < 0.02,
            "alpha={coefficient}: acceleration={acceleration}");
    }
}
