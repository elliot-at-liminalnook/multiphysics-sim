mod common;
use common::*;
use sim_core::Behavior;
use sim_domain_robot::{Articulated, Options, articulated::embedding::RigidEmbedding};
use std::sync::Arc;

#[test]
fn inverse_load_matches_analytic_pendulum_gravity_and_parallel_axis_inertia() {
    let mut model = empty_model();
    model.gravity = [0.0, 0.0, -9.81];
    model
        .links
        .push(box_link("ground", [0.1; 3], 1.0, [0.0; 3], true));
    model
        .links
        .push(box_link("bob", [0.1; 3], 2.0, [0.2, 0.0, 0.0], false));
    model.joints.push(joint(
        "hinge",
        "revolute",
        Some("ground"),
        "bob",
        [0.0; 3],
        [0.0, 1.0, 0.0],
    ));
    let art = Articulated::new(
        Arc::new(model),
        &Options {
            contact: false,
            flex: false,
            ..Default::default()
        },
    )
    .unwrap();
    let seed = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.0; art.state_count],
        &vec![0.0; art.port_names.len() + 1],
        vec![],
    );
    let name = art.dofs().next().unwrap().1.name.clone();
    let map = RigidEmbedding::new(&art, &[name], Default::default()).unwrap();
    let inertia = 2.0 * (0.1_f64.powi(2) + 0.1_f64.powi(2)) / 12.0 + 2.0 * 0.2_f64.powi(2);
    for angle in [-0.5_f64, 0.0, 0.7] {
        let motion = map.solve(&seed, &[angle], &[0.0]).unwrap();
        let prepared = map.prepare_dynamics(&motion).unwrap();
        for acceleration in [-2.0, 0.0, 3.0] {
            let required = prepared.required_reduced_forces(&[acceleration]).unwrap();
            let expected = inertia * acceleration - 2.0 * 9.81 * 0.2 * angle.cos();
            assert!(
                (required[0] - expected).abs() < 1e-10,
                "angle={angle} actual={} expected={expected}",
                required[0]
            );
            let recovered = prepared.accelerations(required.as_slice()).unwrap();
            assert!((recovered.reduced_accelerations[0] - acceleration).abs() < 1e-10);
        }
    }
}
