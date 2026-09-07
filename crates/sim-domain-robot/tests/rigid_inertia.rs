mod common;
use common::*;
use nalgebra::{DVector, Matrix3};
use sim_core::Behavior;
use sim_domain_robot::{Articulated, Generalized, Options};
use std::sync::Arc;

fn point(art: &Articulated) -> Generalized {
    let mut states: Vec<_> = art.states().iter().map(|s| s.initial).collect();
    for b in &art.bases {
        states[b.state + 3..b.state + 7].copy_from_slice(&[0.9, 0.1, -0.2, 0.3]);
        for k in 7..13 {
            states[b.state + k] = 0.03 * k as f64;
        }
    }
    for (i, (_, d)) in art.dofs().enumerate() {
        states[d.qd_state] = 0.13 * (i as f64 + 0.2).cos();
        if let Some(q) = d.q_state {
            states[q] = 0.02 * (i as f64 + 0.1).sin();
        }
    }
    art.generalized(
        states,
        vec![0.0; art.state_count],
        &(0..art.port_names.len())
            .map(|i| 0.19 * (i as f64 + 0.3).sin())
            .collect::<Vec<_>>(),
        vec![],
    )
}

fn forces(art: &Articulated, g: &Generalized) -> DVector<f64> {
    let e = art.evaluate_with(g, false);
    DVector::from_iterator(
        art.bases.iter().filter(|b| !b.grounded).count() * 6 + g.q.len(),
        art.bases
            .iter()
            .enumerate()
            .filter(|(_, b)| !b.grounded)
            .flat_map(|(i, _)| e.base_wrench[i])
            .chain(
                e.joints
                    .iter()
                    .flat_map(|j| j.tau_needed.iter().zip(&j.tau_passive).map(|(a, b)| a - b)),
            ),
    )
}

#[test]
fn grounded_pendulum_inertia_matches_parallel_axis_formula() {
    let mut model = empty_model();
    model.links = vec![
        box_link("base", [0.1; 3], 1.0, [0.0; 3], true),
        box_link("arm", [0.8, 0.1, 0.1], 2.0, [0.5, 0.0, 0.0], false),
    ];
    model.joints.push(joint(
        "hinge",
        "revolute",
        Some("base"),
        "arm",
        [0.0; 3],
        [0.0, 0.0, 1.0],
    ));
    let expected = model.links[1].inertia[2][2] + 2.0 * 0.5_f64.powi(2);
    let art = Articulated::new(
        Arc::new(model),
        &Options {
            flex: false,
            ..Default::default()
        },
    )
    .unwrap();
    let mass = art.rigid_mass_matrix(&point(&art)).unwrap();
    assert_eq!(mass.shape(), (1, 1));
    assert!((mass[(0, 0)] - expected).abs() < 1e-14);
}

#[test]
fn branched_mixed_joints_and_multiple_bases_match_inverse_dynamics_and_energy() {
    let mut model = empty_model();
    model.links = vec![
        box_link("base", [0.2; 3], 2.0, [0.0, 0.0, 1.0], false),
        box_link("ball", [0.1, 0.2, 0.3], 0.7, [0.2, 0.1, 0.9], false),
        box_link("compliant", [0.15; 3], 0.5, [0.4, 0.1, 0.8], false),
        box_link("slide", [0.1; 3], 0.4, [-0.2, 0.1, 0.9], false),
        box_link("ground", [0.1; 3], 1.0, [1.0, 0.0, 1.0], true),
        box_link("hinge", [0.2; 3], 0.3, [1.1, 0.2, 0.7], false),
        box_link("free", [0.2, 0.3, 0.4], 0.9, [2.0, 0.0, 1.0], false),
    ];
    model.joints = vec![
        joint(
            "ball",
            "ball",
            Some("base"),
            "ball",
            [0.1, 0.0, 1.0],
            [0.2, 0.3, 0.8],
        ),
        joint(
            "fixed",
            "fixed",
            Some("ball"),
            "compliant",
            [0.3, 0.1, 0.9],
            [0.4, 0.3, 0.7],
        ),
        joint(
            "slide",
            "prismatic",
            Some("base"),
            "slide",
            [-0.1, 0.0, 1.0],
            [0.5, -0.2, 0.7],
        ),
        joint(
            "hinge",
            "revolute",
            Some("ground"),
            "hinge",
            [1.0, 0.1, 1.0],
            [-0.2, 0.6, 0.4],
        ),
    ];
    let art = Articulated::new(
        Arc::new(model),
        &Options {
            flex: false,
            ..Default::default()
        },
    )
    .unwrap();
    for phase in [0.0, 0.4, -0.7] {
        let mut g = point(&art);
        for q in &mut g.q {
            *q += phase;
        }
        let mass = art.rigid_mass_matrix(&g).unwrap();
        let nb = art.bases.iter().filter(|b| !b.grounded).count() * 6;
        assert_eq!(nb, 12);
        assert_eq!(mass.nrows(), nb + 11);
        let bias = forces(&art, &g);
        for col in 0..mass.ncols() {
            let mut probe = g.clone();
            if col < nb {
                let b = art
                    .bases
                    .iter()
                    .filter(|b| !b.grounded)
                    .nth(col / 6)
                    .unwrap();
                probe.rates[b.state + 7 + col % 6] = 1.0;
            } else {
                let i = col - nb;
                let (_, d) = art.dofs().nth(i).unwrap();
                probe.rates[d.qd_state] = 1.0;
                probe.qdd[i] = 1.0;
            }
            let reference = forces(&art, &probe) - &bias;
            assert!(
                (mass.column(col) - reference).amax() < 2e-10,
                "column {col} phase {phase}"
            );
        }
        assert!((&mass - mass.transpose()).amax() < 1e-14);
        assert!(mass.clone().cholesky().is_some());
        let speed = DVector::from_iterator(
            mass.ncols(),
            art.bases
                .iter()
                .filter(|b| !b.grounded)
                .flat_map(|b| g.states[b.state + 7..b.state + 13].iter().copied())
                .chain(g.qd.iter().copied()),
        );
        // Grounded bases have no kinetic coordinates: keep their twist zero.
        for b in art.bases.iter().filter(|b| b.grounded) {
            g.states[b.state + 7..b.state + 13].fill(0.0);
        }
        let eval = art.evaluate_with(&g, false);
        let energy: f64 = art
            .links
            .iter()
            .zip(&eval.links)
            .map(|(l, k)| {
                let iw: Matrix3<f64> = k.r * l.inertia * k.r.transpose();
                0.5 * (l.mass * k.vel.norm_squared() + k.w.dot(&(iw * k.w)))
            })
            .sum();
        assert!((0.5 * speed.dot(&(&mass * &speed)) - energy).abs() < 1e-13);
    }
}

#[test]
fn rejects_invalid_inputs_and_modal_flexibility() {
    let mut model = empty_model();
    model
        .links
        .push(box_link("base", [0.1; 3], 1.0, [0.0; 3], false));
    let art = Articulated::new(
        Arc::new(model.clone()),
        &Options {
            flex: false,
            ..Default::default()
        },
    )
    .unwrap();
    let mut g = point(&art);
    g.q.push(0.0);
    assert!(art.rigid_mass_matrix(&g).is_err());
    assert!(art.rigid_closure_velocity_jacobian(&g).is_err());
    g = point(&art);
    g.states[0] = f64::NAN;
    assert!(art.rigid_mass_matrix(&g).is_err());
    assert!(art.rigid_closure_velocity_jacobian(&g).is_err());
    model.links[0].flex = Some(sim_domain_robot::model::Flex {
        modes: 1,
        modal_mass: vec![0.1],
        modal_stiffness: vec![100.0],
        boundary_shapes: vec![vec![]],
        participation: vec![[0.0; 6]],
        ..Default::default()
    });
    let flexible = Articulated::new(
        Arc::new(model),
        &Options {
            flex: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(flexible.links[0].flex.is_some());
    assert!(
        flexible
            .rigid_mass_matrix(&point(&flexible))
            .unwrap_err()
            .contains("modal")
    );
    assert!(
        flexible
            .rigid_closure_velocity_jacobian(&point(&flexible))
            .unwrap_err()
            .contains("modal")
    );
}
