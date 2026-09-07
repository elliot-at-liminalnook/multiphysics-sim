mod common;
use common::*;
use sim_core::Behavior;
use sim_domain_robot::articulated::{embedding::RigidEmbedding, friction::FloorFrictionModel};
use sim_domain_robot::{Articulated, Generalized, Options};
use std::sync::Arc;

fn slider(speed_scale: f64, initial_speed: f64) -> (Articulated, Generalized) {
    let mut model = empty_model();
    model.gravity = [0.0; 3];
    model
        .links
        .push(box_link("ground", [0.1; 3], 1.0, [0.0, 0.0, -1.0], true));
    // Five bottom samples carry 2 kg * g of normal load. The rail holds height.
    let depth = 2.0 * 9.81 / (5.0 * model.world.floor_stiffness);
    model.links.push(box_link(
        "body",
        [0.1; 3],
        2.0,
        [0.0, 0.0, 0.05 - depth],
        false,
    ));
    model.joints.push(joint(
        "rail",
        "prismatic",
        Some("ground"),
        "body",
        [0.0, 0.0, 0.05 - depth],
        [1.0, 0.0, 0.0],
    ));
    let art = Articulated::new(
        Arc::new(model),
        &Options {
            flex: false,
            floor_friction: FloorFrictionModel::RegularizedCoulomb {
                slip_speed_m_s: speed_scale,
            },
            initial_speeds: [("slide.rail".into(), initial_speed)].into(),
            ..Default::default()
        },
    )
    .unwrap();
    let g = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.0; art.state_count],
        &vec![0.0; art.port_names.len() + 1],
        vec![],
    );
    (art, g)
}

#[test]
fn unloading_patch_respects_combined_sliding_twisting_capacity_without_memory() {
    for depth in [1e-9, 1e-5, 1e-3] {
        let mut model = empty_model();
        model.gravity = [0.0; 3];
        model.links.push(box_link(
            "body",
            [0.1; 3],
            2.0,
            [0.0, 0.0, 0.05 - depth],
            false,
        ));
        let art = Articulated::new(
            Arc::new(model),
            &Options {
                flex: false,
                initial_twist: [0.3, -0.4, 0.0, 0.0, 0.0, 10.0],
                floor_friction: FloorFrictionModel::RegularizedCoulomb {
                    slip_speed_m_s: 0.001,
                },
                ..Default::default()
            },
        )
        .unwrap();
        let mut g = art.generalized(
            art.states().iter().map(|s| s.initial).collect(),
            vec![0.0; art.state_count],
            &[0.0],
            vec![],
        );
        let eval = art.evaluate(&g);
        let normal: f64 = eval.contacts.iter().map(|c| c.force.z).sum();
        let fx: f64 = eval.contacts.iter().map(|c| c.force.x).sum();
        let fy: f64 = eval.contacts.iter().map(|c| c.force.y).sum();
        let radius = (eval
            .contacts
            .iter()
            .map(|c| (c.point.x.powi(2) + c.point.y.powi(2)) * c.force.z)
            .sum::<f64>()
            / normal)
            .sqrt();
        let torque = -eval.base_wrench[0][5];
        let capacity = art.links[0].floor_mu.1 * normal;
        assert!(fx.hypot(fy).hypot(torque / radius) <= capacity * (1.0 + 1e-12));
        assert!(fx * 0.3 + fy * (-0.4) + torque * 10.0 < 0.0);
        let zs = art.links[0].bristle_state;
        g.states[zs..zs + 3].copy_from_slice(&[1.0, -2.0, 30.0]);
        let changed = art.evaluate(&g);
        assert_eq!(eval.base_wrench, changed.base_wrench);
        assert_eq!(
            &changed.bristle_rates[zs..zs + 3],
            &[-200.0, 400.0, -6000.0]
        );
    }
}

#[test]
fn a_single_contact_point_has_no_independent_twist_resistance() {
    let mut model = empty_model();
    model.gravity = [0.0; 3];
    let mut body = box_link("body", [0.1; 3], 2.0, [0.0, 0.0, 0.0499], false);
    body.collision.hull = vec![[0.0, 0.0, -0.05]];
    body.collision.vertices = body.collision.hull.clone();
    model.links.push(body);
    let art = Articulated::new(
        Arc::new(model),
        &Options {
            flex: false,
            initial_twist: [0.0, 0.0, 0.0, 0.0, 0.0, 10.0],
            floor_friction: FloorFrictionModel::RegularizedCoulomb {
                slip_speed_m_s: 0.001,
            },
            ..Default::default()
        },
    )
    .unwrap();
    let g = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.0; art.state_count],
        &[0.0],
        vec![],
    );
    let evaluated = art.evaluate(&g);
    assert_eq!(evaluated.contacts.len(), 1);
    assert_eq!(evaluated.base_wrench[0][5], 0.0);
}

#[test]
fn loaded_rail_creeps_at_the_predicted_regularization_speed() {
    for epsilon in [0.001, 0.0001] {
        let (art, mut g) = slider(epsilon, 0.0);
        let map = RigidEmbedding::new(&art, &["slide.rail".into()], Default::default()).unwrap();
        let capacity = art.links[1].floor_mu.1 * 2.0 * 9.81;
        for i in 0..100 {
            g = map
                .step_implicit(&g, i as f64 * 0.001, 0.001, &Default::default(), |_, _| {
                    Ok(vec![capacity * 0.5])
                })
                .unwrap()
                .endpoint
                .generalized;
        }
        let expected = epsilon * 0.5_f64.atanh();
        assert!(
            (g.qd[0] / expected - 1.0).abs() < 1e-5,
            "{} vs {expected}",
            g.qd[0]
        );
        assert!(g.q[0] > 0.0 && g.q[0] < expected * 0.101);
    }
}

#[test]
fn sliding_distance_refines_and_kinetic_energy_never_increases() {
    let (art, seed) = slider(0.001, 0.2);
    let map = RigidEmbedding::new(&art, &["slide.rail".into()], Default::default()).unwrap();
    let acceleration = art.links[1].floor_mu.1 * 9.81;
    // Integral v/[a*tanh(v/epsilon)] dv, from rest to v0. At v0/epsilon=200,
    // the exponentially small finite-upper-limit correction is negligible.
    let expected = 0.2_f64.powi(2) / (2.0 * acceleration)
        + std::f64::consts::PI.powi(2) * 0.001_f64.powi(2) / (12.0 * acceleration);
    let mut errors = Vec::new();
    for n in [200, 400] {
        let mut g = seed.clone();
        let h = 0.2 / n as f64;
        for i in 0..n {
            let speed = g.qd[0];
            g = map
                .step_implicit(&g, i as f64 * h, h, &Default::default(), |_, _| {
                    Ok(vec![0.0])
                })
                .unwrap()
                .endpoint
                .generalized;
            assert!(g.qd[0].abs() <= speed.abs() + 1e-10);
        }
        assert!(g.qd[0].abs() < 1e-7);
        errors.push((g.q[0] - expected).abs());
    }
    assert!(
        errors[1] < 6e-5 && errors[0] / errors[1] > 1.8,
        "{errors:?}"
    );
}

#[test]
fn registry_and_reduced_model_share_the_selected_contact_law() {
    let (art, _) = slider(0.001, 0.2);
    let mut rig = Rig::new(
        (*art.model).clone(),
        &[
            ("flex", 0.0),
            ("floor.regularized_slip_speed", 0.001),
            ("initial.slide.rail.velocity", 0.2),
        ],
        euler(),
    );
    assert!(!rig.art.floor_friction.is_bristle());
    let map = RigidEmbedding::new(&rig.art, &["slide.rail".into()], Default::default()).unwrap();
    let mut g = rig.generalized();
    for i in 0..20 {
        g = map
            .step_implicit(&g, i as f64 * 0.001, 0.001, &Default::default(), |_, _| {
                Ok(vec![0.0])
            })
            .unwrap()
            .endpoint
            .generalized;
        rig.runtime.advance(0.001, 0.001).unwrap();
        let actual = rig.generalized();
        assert!((actual.q[0] - g.q[0]).abs() < 1e-8);
        assert!((actual.qd[0] - g.qd[0]).abs() < 1e-7);
    }
    assert!(FloorFrictionModel::from_registry_speed(-1.0).is_err());
    assert!(FloorFrictionModel::from_registry_speed(f64::NAN).is_err());
}
