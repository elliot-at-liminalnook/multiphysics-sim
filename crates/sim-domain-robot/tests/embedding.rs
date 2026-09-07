mod common;
use common::*;
use sim_core::Behavior;
use sim_domain_robot::articulated::embedding::{DependentSolve, EmbeddingConfig, RigidEmbedding};
use sim_domain_robot::{Articulated, Generalized, Options};
use std::sync::Arc;

fn slider_crank(floating: bool) -> (Articulated, Generalized) {
    slider_crank_length(floating, 0.2)
}
fn slider_crank_length(floating: bool, length: f64) -> (Articulated, Generalized) {
    let mut m = empty_model();
    m.links
        .push(box_link("base", [0.1; 3], 1.0, [0.0; 3], !floating));
    m.links.push(box_link(
        "crank",
        [0.01, 0.01, 0.05],
        0.02,
        [0.0, 0.0, 0.025],
        false,
    ));
    m.links.push(box_link(
        "coupler",
        [0.01, 0.01, length],
        0.04,
        [0.0, 0.0, 0.05 - length * 0.5],
        false,
    ));
    m.links.push(box_link(
        "slider",
        [0.02; 3],
        0.03,
        [0.0, 0.0, 0.05 - length],
        false,
    ));
    let y = [0.0, 1.0, 0.0];
    m.joints.push(joint(
        "motor",
        "revolute",
        Some("base"),
        "crank",
        [0.0; 3],
        y,
    ));
    m.joints.push(joint(
        "link",
        "revolute",
        Some("crank"),
        "coupler",
        [0.0, 0.0, 0.05],
        y,
    ));
    m.joints.push(joint(
        "slide",
        "prismatic",
        Some("base"),
        "slider",
        [0.0, 0.0, 0.05 - length],
        [0.0, 0.0, 1.0],
    ));
    m.joints.push(joint(
        "closure",
        "loop_revolute",
        Some("coupler"),
        "slider",
        [0.0, 0.0, 0.05 - length],
        y,
    ));
    let art = Articulated::new(
        Arc::new(m),
        &Options {
            contact: false,
            flex: false,
            ..Options::default()
        },
    )
    .unwrap();
    let states = art.states().iter().map(|s| s.initial).collect();
    let g = art.generalized(
        states,
        vec![0.0; art.state_count],
        &vec![0.0; art.port_names.len() + 1],
        vec![],
    );
    (art, g)
}
fn expected(theta: f64) -> (f64, f64) {
    (
        (0.25 * theta.sin()).asin() - theta,
        0.05 * theta.cos() - (0.04 - 0.0025 * theta.sin().powi(2)).sqrt() + 0.15,
    )
}

#[test]
fn point_jacobians_match_closed_linkage_position_differences_with_rotated_base() {
    use sim_domain_robot::articulated::embedding::EmbeddedPoint;
    let (art, mut seed) = slider_crank(true);
    let s = art.bases[0].state;
    seed.states[s..s + 3].copy_from_slice(&[0.2, -0.3, 0.4]);
    seed.states[s + 3..s + 7].copy_from_slice(&[0.9_f64.cos(), 0.0, 0.0, 0.9_f64.sin()]);
    let map =
        RigidEmbedding::new(&art, &["joint.motor".into()], EmbeddingConfig::default()).unwrap();
    let points = vec![
        EmbeddedPoint {
            link: 1,
            local_point_m: [0.03, -0.02, 0.04],
        },
        EmbeddedPoint {
            link: 2,
            local_point_m: [0.01, 0.02, -0.03],
        },
        EmbeddedPoint {
            link: 3,
            local_point_m: [0.0; 3],
        },
    ];
    for theta in [-0.8, 0.3, 0.9] {
        let (_, actual) = map.point_jacobians(&seed, &[theta], &points).unwrap();
        for h in [1e-4, 1e-5] {
            let (_, plus) = map.point_jacobians(&seed, &[theta + h], &points).unwrap();
            let (_, minus) = map.point_jacobians(&seed, &[theta - h], &points).unwrap();
            for i in 0..points.len() {
                let numeric = (plus[i].0 - minus[i].0) / (2.0 * h);
                assert!((actual[i].1.column(0) - numeric).norm() < 1e-8);
            }
        }
    }
    assert!(
        map.point_jacobians(
            &seed,
            &[0.3],
            &[EmbeddedPoint {
                link: 100,
                local_point_m: [0.0; 3]
            }]
        )
        .is_err()
    );
}

#[test]
fn bounded_plane_placement_matches_analytic_slider_crank_and_rejects_unreachable_target() {
    use sim_domain_robot::articulated::embedding::{
        CoordinateInterval, EmbeddedPoint, PlanePlacementConfig, PointPlaneTarget,
    };
    let (art, seed) = slider_crank(false);
    let map =
        RigidEmbedding::new(&art, &["joint.motor".into()], EmbeddingConfig::default()).unwrap();
    let seed = map.solve(&seed, &[0.3], &[0.0]).unwrap().generalized;
    let original = seed.q.clone();
    let bounds = [CoordinateInterval {
        lower: 0.2,
        upper: 1.0,
        max_step: 0.1,
    }];
    let target = PointPlaneTarget {
        point: EmbeddedPoint {
            link: 3,
            local_point_m: [0.0; 3],
        },
        normal_world: [0.0, 0.0, 1.0],
        offset_m: -0.15 + expected(0.7).1,
    };
    let config = PlanePlacementConfig {
        tolerance_m: 1e-10,
        ..PlanePlacementConfig::default()
    };
    let fit = map
        .place_points_on_planes(&seed, &[target.clone()], &bounds, &config)
        .unwrap();
    assert!((fit.coordinates[0] - 0.7).abs() < 1e-8);
    assert!(fit.maximum_plane_error_m < 1e-10);
    assert!(fit.motion.maximum_scaled_position_error < 1e-8);
    assert_eq!(fit.motion.generalized.qd, vec![0.0; 3]);
    let impossible = PointPlaneTarget {
        offset_m: 1.0,
        ..target.clone()
    };
    assert!(
        map.place_points_on_planes(&seed, &[impossible], &bounds, &config)
            .is_err()
    );
    let fixed = [CoordinateInterval {
        lower: 0.3,
        upper: 0.3,
        max_step: 0.1,
    }];
    assert!(
        map.place_points_on_planes(&seed, &[target.clone()], &fixed, &config)
            .is_err()
    );
    let invalid = PointPlaneTarget {
        normal_world: [0.0; 3],
        ..target
    };
    assert!(
        map.place_points_on_planes(&seed, &[invalid], &bounds, &config)
            .is_err()
    );
    assert_eq!(seed.q, original);
}

#[test]
fn full_point_placement_matches_rotated_analytic_slider_and_rejects_lateral_target() {
    use sim_domain_robot::articulated::embedding::{
        CoordinateInterval, EmbeddedPoint, PlanePlacementConfig, PointTarget,
    };
    let (art, mut seed) = slider_crank(true);
    let base = art.bases[0].state;
    seed.states[base..base + 3].copy_from_slice(&[0.2, -0.3, 0.4]);
    let rotation =
        nalgebra::UnitQuaternion::from_scaled_axis(nalgebra::Vector3::new(0.4, 0.2, -0.3));
    let q = rotation.quaternion();
    seed.states[base + 3..base + 7].copy_from_slice(&[q.w, q.i, q.j, q.k]);
    let map = RigidEmbedding::new(&art, &["joint.motor".into()], Default::default()).unwrap();
    let seed = map.solve(&seed, &[0.3], &[0.0; 7]).unwrap().generalized;
    let original = seed.states.clone();
    let point = EmbeddedPoint {
        link: 3,
        local_point_m: [0.01, 0.02, 0.03],
    };
    let position = nalgebra::Vector3::new(0.2, -0.3, 0.4)
        + rotation * nalgebra::Vector3::new(0.01, 0.02, -0.15 + expected(0.7).1 + 0.03);
    let target = PointTarget {
        point,
        position_world_m: position.into(),
    };
    let bounds = [CoordinateInterval {
        lower: 0.2,
        upper: 1.0,
        max_step: 0.1,
    }];
    let config = PlanePlacementConfig {
        tolerance_m: 1e-10,
        ..Default::default()
    };
    let fit = map
        .place_points(&seed, &[target.clone()], &bounds, &config)
        .unwrap();
    assert!((fit.coordinates[0] - 0.7).abs() < 1e-8);
    assert!(fit.maximum_position_error_m <= config.tolerance_m);
    assert_eq!(
        &fit.motion.generalized.states[base..base + 7],
        &seed.states[base..base + 7]
    );
    let mut unreachable = target.clone();
    let displaced = position + rotation * nalgebra::Vector3::new(0.01, 0.0, 0.0);
    unreachable.position_world_m = displaced.into();
    assert!(
        map.place_points(&seed, &[unreachable], &bounds, &config)
            .is_err()
    );
    let mut invalid = target;
    invalid.position_world_m[0] = f64::NAN;
    assert!(
        map.place_points(&seed, &[invalid], &bounds, &config)
            .is_err()
    );
    assert!(map.place_points(&seed, &[], &bounds, &config).is_err());
    assert_eq!(seed.states, original);
}

#[test]
fn placement_preserves_base_pose_and_respects_dependent_joint_limits() {
    use sim_domain_robot::articulated::embedding::{
        CoordinateInterval, EmbeddedPoint, PlanePlacementConfig, PointPlaneTarget,
    };
    let (mut art, seed) = slider_crank(true);
    art.joints
        .iter_mut()
        .find(|j| j.name == "link")
        .unwrap()
        .dofs[0]
        .lower = Some(-0.4);
    let map =
        RigidEmbedding::new(&art, &["joint.motor".into()], EmbeddingConfig::default()).unwrap();
    let mut seed = map.solve(&seed, &[0.3], &[0.0; 7]).unwrap().generalized;
    let base = art.bases[0].state;
    seed.states[base + 2] += 0.12;
    let target = |theta| PointPlaneTarget {
        point: EmbeddedPoint {
            link: 3,
            local_point_m: [0.0; 3],
        },
        normal_world: [0.0, 0.0, 1.0],
        offset_m: -0.15 + 0.12 + expected(theta).1,
    };
    let bounds = [CoordinateInterval {
        lower: 0.2,
        upper: 1.0,
        max_step: 0.1,
    }];
    let config = PlanePlacementConfig::default();
    let fit = map
        .place_points_on_planes(&seed, &[target(0.4)], &bounds, &config)
        .unwrap();
    assert!((fit.coordinates[0] - 0.4).abs() < 1e-5);
    assert_eq!(
        &fit.motion.generalized.states[base..base + 7],
        &seed.states[base..base + 7]
    );
    // Motor angle 0.7 is in the caller interval, but requires a dependent
    // coupler angle below -0.4. Placement must not discard that authored stop.
    assert!(
        map.place_points_on_planes(&seed, &[target(0.7)], &bounds, &config)
            .is_err()
    );
}

#[test]
fn nonlinear_slider_crank_matches_geometry_tangent_and_curvature() {
    let (art, mut seed) = slider_crank(false);
    let map =
        RigidEmbedding::new(&art, &["joint.motor".into()], EmbeddingConfig::default()).unwrap();
    let indices: Vec<_> = art.dofs().map(|(_, d)| d.name.clone()).collect();
    let b = indices.iter().position(|n| n == "joint.link").unwrap();
    let s = indices.iter().position(|n| n == "slide.slide").unwrap();
    for i in 0..81 {
        let theta = -1.0 + i as f64 * 0.025;
        let motion = map.solve(&seed, &[theta], &[0.7]).unwrap();
        let (link, slide) = expected(theta);
        assert!((motion.generalized.q[b] - link).abs() < 1e-8);
        assert!((motion.generalized.q[s] - slide).abs() < 1e-9);
        let h = 1e-4;
        let plus = expected(theta + h);
        let minus = expected(theta - h);
        for (j, p, m, x) in [(b, plus.0, minus.0, link), (s, plus.1, minus.1, slide)] {
            assert!((motion.tangent[(j, 0)] - (p - m) / (2.0 * h)).abs() < 1e-8);
            assert!(
                (motion.acceleration_bias[j] - (p - 2.0 * x + m) / (h * h) * 0.49).abs() < 1e-7
            );
        }
        assert!(motion.maximum_scaled_position_error < 1e-8);
        assert!(motion.maximum_scaled_velocity_error < 1e-8);
        assert!(motion.maximum_scaled_acceleration_error < 1e-8);
        seed = motion.generalized;
    }
}

#[test]
fn analytic_positions_preserve_branches_floating_dynamics_and_rank_checks() {
    for floating in [false, true] {
        let (art, original) = slider_crank(floating);
        let config = EmbeddingConfig {
            direct_closure_jacobian: true,
            dependent_solve: DependentSolve::PivotedQr,
            ..Default::default()
        };
        let numeric = RigidEmbedding::new(&art, &["joint.motor".into()], config.clone()).unwrap();
        let analytic = RigidEmbedding::new(
            &art,
            &["joint.motor".into()],
            EmbeddingConfig {
                analytic_mechanism_positions: true,
                ..config.clone()
            },
        )
        .unwrap();
        let audit = art.audit_slider_cranks();
        let c = audit[0].candidate.as_ref().unwrap();
        for branch in [-1, 1] {
            let mut seed = original.clone();
            let initial = c.coordinates(0.0, branch, 0.0).unwrap();
            seed.q[c.dof_indices[1]] = initial.coupler_rad;
            seed.q[c.dof_indices[2]] = initial.slider_m;
            let mut velocities = vec![0.7; analytic.reduced_dimension()];
            if floating {
                velocities[..6].copy_from_slice(&[0.2, -0.1, 0.3, 0.4, 0.5, -0.2]);
            }
            for theta in [-0.6, -0.3, 0.0, 0.3, 0.6] {
                let a = analytic.solve(&seed, &[theta], &velocities).unwrap();
                let b = numeric.solve(&seed, &[theta], &velocities).unwrap();
                assert_eq!(a.position_iterations, 0);
                for (x, y) in a.generalized.q.iter().zip(&b.generalized.q) {
                    assert!((x - y).abs() < 1e-10);
                }
                assert!((&a.tangent - &b.tangent).amax() < 1e-9);
                assert!((&a.acceleration_bias - &b.acceleration_bias).amax() < 1e-8);
                assert!(
                    (a.minimum_scaled_singular_value - b.minimum_scaled_singular_value).abs()
                        < 1e-9
                );
                let loads = vec![0.13; analytic.full_dimension()];
                let aa = analytic.accelerations(&a, &loads).unwrap();
                let bb = numeric.accelerations(&b, &loads).unwrap();
                assert!((&aa.full_accelerations - &bb.full_accelerations).amax() < 1e-7);
                seed = a.generalized;
            }
        }
        let rejecting = RigidEmbedding::new(
            &art,
            &["joint.motor".into()],
            EmbeddingConfig {
                analytic_mechanism_positions: true,
                absolute_rank_tolerance: 100.0,
                ..config
            },
        )
        .unwrap();
        assert!(
            rejecting
                .solve(&original, &[0.0], &vec![0.0; rejecting.reduced_dimension()])
                .is_err()
        );
        assert!(
            RigidEmbedding::new(
                &art,
                &["joint.link".into()],
                EmbeddingConfig {
                    analytic_mechanism_positions: true,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
    let (art, seed) = slider_crank_length(false, 0.04);
    let map = RigidEmbedding::new(
        &art,
        &["joint.motor".into()],
        EmbeddingConfig {
            analytic_mechanism_positions: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        map.solve(&seed, &[std::f64::consts::FRAC_PI_2], &[0.0])
            .is_err()
    );
}

#[test]
fn analytic_slider_crank_matches_independent_formula_and_iterative_chart() {
    for floating in [false, true] {
        let (art, mut seed) = slider_crank(floating);
        if floating {
            let s = art.bases[0].state;
            seed.states[s..s + 3].copy_from_slice(&[0.3, -0.2, 0.5]);
            seed.states[s + 3..s + 7].copy_from_slice(&[0.8, 0.2, -0.1, 0.3]);
        }
        let audit = art.audit_slider_cranks();
        assert_eq!(audit.len(), 1);
        let chart = audit[0].candidate.as_ref().expect("valid slider crank");
        assert_eq!(chart.reference_branch, -1);
        let map = RigidEmbedding::new(
            &art,
            &["joint.motor".into()],
            EmbeddingConfig {
                direct_closure_jacobian: true,
                dependent_solve: DependentSolve::PivotedQr,
                ..Default::default()
            },
        )
        .unwrap();
        for i in 0..81 {
            let theta = -1.0 + i as f64 * 0.025;
            let x = chart
                .coordinates(theta, chart.reference_branch, seed.q[chart.dof_indices[1]])
                .unwrap();
            let e = expected(theta);
            assert!((x.coupler_rad - e.0).abs() < 1e-12);
            assert!((x.slider_m - e.1).abs() < 1e-12);
            let mut velocity = vec![0.0; map.reduced_dimension()];
            *velocity.last_mut().unwrap() = 0.7;
            let m = map.solve(&seed, &[theta], &velocity).unwrap();
            let nb = usize::from(floating) * 6;
            for (k, &j) in chart.dof_indices[1..].iter().enumerate() {
                assert!((m.tangent[(nb + j, nb)] - x.first_derivative[k]).abs() < 1e-10);
                assert!(
                    (m.acceleration_bias[nb + j] - 0.49 * x.second_derivative[k]).abs() < 1e-10
                );
            }
            seed = m.generalized;
        }
        let other = chart.coordinates(0.0, 1, 0.0).unwrap();
        assert!((other.slider_m - 0.4).abs() < 1e-12);
        assert!(chart.coordinates(f64::NAN, -1, 0.0).is_err());
        assert!(chart.coordinates(0.0, 0, 0.0).is_err());
    }
}

#[test]
fn analytic_slider_crank_rejects_wrong_axes_topology_and_toggle() {
    let (mut art, _) = slider_crank(false);
    let loop_b = art.loops[0].b;
    let slide = art.links[loop_b].parent_joint.unwrap();
    art.joints[slide].r_j =
        nalgebra::Rotation3::from_euler_angles(0.2, 0.3, 0.1).into_inner() * art.joints[slide].r_j;
    assert!(art.audit_slider_cranks()[0].candidate.is_none());
    let (mut art, _) = slider_crank(false);
    art.loops[0].axis = None;
    assert!(art.audit_slider_cranks()[0].candidate.is_none());
    let (art, _) = slider_crank_length(false, 0.05);
    let a = art.audit_slider_cranks();
    let c = a[0].candidate.as_ref().unwrap();
    assert!(c.coordinates(std::f64::consts::FRAC_PI_2, -1, 0.0).is_err());
    let (art, _) = slider_crank_length(false, 0.04);
    let a = art.audit_slider_cranks();
    let c = a[0].candidate.as_ref().unwrap();
    assert!(c.coordinates(std::f64::consts::FRAC_PI_2, -1, 0.0).is_err());
}

#[test]
fn analytic_slider_crank_handles_offset_guide_and_opposite_coupler_axis() {
    for reversed in [false, true] {
        let (original, _) = slider_crank(false);
        let mut model = (*original.model).clone();
        model
            .links
            .iter_mut()
            .find(|l| l.name == "slider")
            .unwrap()
            .com[0] += 0.02;
        for j in &mut model.joints {
            if j.name == "slide" || j.name == "closure" {
                j.origin[0] += 0.02;
            }
            if reversed && j.name == "link" {
                j.axis = [0.0, -1.0, 0.0];
            }
        }
        let art = Articulated::new(
            Arc::new(model),
            &Options {
                flex: false,
                contact: false,
                ..Default::default()
            },
        )
        .unwrap();
        let audit = art.audit_slider_cranks();
        let c = audit[0].candidate.as_ref().unwrap();
        let mut seed = art.generalized(
            art.states().iter().map(|s| s.initial).collect(),
            vec![0.0; art.state_count],
            &vec![0.0; art.port_names.len() + 1],
            vec![],
        );
        let map = RigidEmbedding::new(
            &art,
            &["joint.motor".into()],
            EmbeddingConfig {
                direct_closure_jacobian: true,
                dependent_solve: DependentSolve::PivotedQr,
                ..Default::default()
            },
        )
        .unwrap();
        for theta in [-0.8_f64, -0.3, 0.0, 0.4, 1.0] {
            let x = c.coordinates(theta, -1, seed.q[c.dof_indices[1]]).unwrap();
            let slide =
                0.15 + 0.05 * theta.cos() - (0.0404 - (0.02 - 0.05 * theta.sin()).powi(2)).sqrt();
            assert!((x.slider_m - slide).abs() < 1e-12);
            let m = map.solve(&seed, &[theta], &[0.7]).unwrap();
            for (k, &j) in c.dof_indices[1..].iter().enumerate() {
                assert!((m.generalized.q[j] - [x.coupler_rad, x.slider_m][k]).abs() < 1e-10);
                assert!((m.tangent[(j, 0)] - x.first_derivative[k]).abs() < 1e-10);
                assert!((m.acceleration_bias[j] - 0.49 * x.second_derivative[k]).abs() < 1e-9);
            }
            seed = m.generalized;
        }
    }
}

#[test]
fn floating_base_motion_is_retained_and_original_rows_close() {
    let (art, seed) = slider_crank(true);
    let map =
        RigidEmbedding::new(&art, &["joint.motor".into()], EmbeddingConfig::default()).unwrap();
    assert_eq!((map.full_dimension(), map.reduced_dimension()), (9, 7));
    let velocity = [0.2, -0.1, 0.3, 0.4, 0.5, -0.2, 0.7];
    let motion = map.solve(&seed, &[0.3], &velocity).unwrap();
    let base = &art.bases[0];
    assert_eq!(
        &motion.generalized.states[base.state + 7..base.state + 13],
        &velocity[..6]
    );
    for row in art.original_closure(&motion.generalized) {
        assert!(
            row.position.abs() < 1e-9 && row.velocity.abs() < 1e-9 && row.acceleration.abs() < 1e-9,
            "{row:?}"
        );
    }
    // Independent full inverse dynamics acceleration response checks the
    // projected inertia, including the retained floating-base coupling.
    let mass = art.rigid_mass_matrix(&motion.generalized).unwrap();
    let reduced = motion.tangent.transpose() * mass * &motion.tangent;
    assert!(reduced.clone().cholesky().is_some());
    let mut accelerated = motion.generalized.clone();
    let delta = &motion.tangent
        * nalgebra::DVector::from_column_slice(&[0.3, -0.1, 0.7, -0.2, 0.4, 0.1, 0.9]);
    for k in 0..6 {
        accelerated.rates[base.state + 7 + k] += delta[k];
    }
    for (i, (_, d)) in art.dofs().enumerate() {
        accelerated.qdd[i] += delta[6 + i];
        accelerated.rates[d.qd_state] += delta[6 + i];
    }
    let force = |g: &Generalized| {
        let e = art.evaluate(g);
        nalgebra::DVector::from_iterator(
            9,
            e.base_wrench[0].into_iter().chain(
                e.joints
                    .iter()
                    .flat_map(|j| j.tau_needed.iter().zip(&j.tau_passive).map(|(a, b)| a - b)),
            ),
        )
    };
    let actual = motion.tangent.transpose() * (force(&accelerated) - force(&motion.generalized));
    let expected =
        reduced * nalgebra::DVector::from_column_slice(&[0.3, -0.1, 0.7, -0.2, 0.4, 0.1, 0.9]);
    assert!((actual - expected).amax() < 1e-10);
}

#[test]
fn invalid_charts_and_inconsistent_closure_are_rejected_without_mutating_seed() {
    let (art, seed) = slider_crank(false);
    assert!(RigidEmbedding::new(&art, &["unknown".into()], Default::default()).is_err());
    assert!(
        RigidEmbedding::new(
            &art,
            &["joint.motor".into(), "joint.motor".into()],
            Default::default()
        )
        .is_err()
    );
    // Leaving both motor and coupler independent overconstrains the slider.
    let map = RigidEmbedding::new(
        &art,
        &["joint.motor".into(), "joint.link".into()],
        Default::default(),
    )
    .unwrap();
    assert!(map.solve(&seed, &[0.0, 0.0], &[0.0, 0.0]).is_err());
    // Specifying no independent coordinate leaves a physical free direction.
    let map = RigidEmbedding::new(&art, &[], Default::default()).unwrap();
    assert!(map.solve(&seed, &[], &[]).unwrap_err().contains("singular"));
    assert!(seed.q.iter().all(|q| *q == 0.0));
}

#[test]
fn geometric_toggle_rejects_a_chart_that_loses_rank() {
    let (art, mut seed) = slider_crank_length(false, 0.05);
    let motor = art
        .dofs()
        .position(|(_, d)| d.name == "joint.motor")
        .unwrap();
    seed.q[motor] = std::f64::consts::FRAC_PI_2;
    for (dependent_solve, direct_closure_jacobian) in
        [DependentSolve::Svd, DependentSolve::PivotedQr]
            .into_iter()
            .flat_map(|method| [false, true].map(move |direct| (method, direct)))
    {
        let map = RigidEmbedding::new(
            &art,
            &["joint.motor".into()],
            EmbeddingConfig {
                dependent_solve,
                direct_closure_jacobian,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            art.original_closure(&seed)
                .iter()
                .all(|r| r.position.abs() < 1e-14)
        );
        assert!(
            map.solve(&seed, &[std::f64::consts::FRAC_PI_2], &[0.0])
                .unwrap_err()
                .contains("singular")
        );
    }
}

#[test]
fn reduced_dynamics_matches_full_constrained_solve_under_external_load() {
    let (art, seed) = slider_crank(false);
    for (dependent_solve, direct_closure_jacobian) in
        [DependentSolve::Svd, DependentSolve::PivotedQr]
            .into_iter()
            .flat_map(|method| [false, true].map(move |direct| (method, direct)))
    {
        let map = RigidEmbedding::new(
            &art,
            &["joint.motor".into()],
            EmbeddingConfig {
                dependent_solve,
                direct_closure_jacobian,
                ..Default::default()
            },
        )
        .unwrap();
        for theta in [-0.8, 0.0, 0.7] {
            let motion = map.solve(&seed, &[theta], &[0.6]).unwrap();
            let applied = vec![0.2, -0.1, 0.4];
            let answer = map.accelerations(&motion, &applied).unwrap();
            // Independent original-row acceleration KKT system; SVD permits its
            // redundant rows, while inertia makes the acceleration unique.
            let audit = art
                .audit_constraints(&motion.generalized, &Default::default())
                .unwrap();
            let n = 3;
            let m = audit.rows.len();
            let jac = nalgebra::DMatrix::from_fn(m, n, |i, j| {
                audit.scaled_velocity_matrix[i][j] * audit.row_scales[i] / audit.column_scales[j]
            });
            let mut zero = motion.generalized.clone();
            zero.qdd.fill(0.0);
            zero.rates.fill(0.0);
            let eval = art.evaluate(&zero);
            let bias: Vec<_> = eval
                .joints
                .iter()
                .flat_map(|j| j.tau_needed.iter().zip(&j.tau_passive).map(|(a, b)| a - b))
                .collect();
            let rows = art.original_closure(&zero);
            let mass = art.rigid_mass_matrix(&zero).unwrap();
            let mut kkt = nalgebra::DMatrix::zeros(n + m, n + m);
            kkt.view_mut((0, 0), (n, n)).copy_from(&mass);
            kkt.view_mut((0, n), (n, m)).copy_from(&(-jac.transpose()));
            kkt.view_mut((n, 0), (m, n)).copy_from(&jac);
            let rhs = nalgebra::DVector::from_iterator(
                n + m,
                applied
                    .iter()
                    .zip(bias)
                    .map(|(a, b)| a - b)
                    .chain(rows.iter().map(|r| -r.acceleration)),
            );
            let independent = kkt.clone().svd(true, true).solve(&rhs, 1e-12).unwrap();
            assert!((&kkt * &independent - &rhs).amax() < 1e-10);
            assert!((independent.rows(0, n) - &answer.full_accelerations).amax() < 1e-8);
            assert!(
                art.original_closure(&answer.generalized)
                    .iter()
                    .all(|r| r.acceleration.abs() < 1e-9)
            );
        }
    }
}

#[test]
fn qr_chart_matches_analytic_linkage_under_base_rotation_and_translation() {
    let (art, seed) = slider_crank(true);
    let map = RigidEmbedding::new(
        &art,
        &["joint.motor".into()],
        EmbeddingConfig {
            dependent_solve: DependentSolve::PivotedQr,
            direct_closure_jacobian: true,
            ..Default::default()
        },
    )
    .unwrap();
    let indices: Vec<_> = art.dofs().map(|(_, d)| d.name.clone()).collect();
    let b = indices.iter().position(|n| n == "joint.link").unwrap();
    let s = indices.iter().position(|n| n == "slide.slide").unwrap();
    for rotation in [1e-9, 1e-5, 0.3, 1.7] {
        let mut posed = seed.clone();
        let bs = art.bases[0].state;
        posed.states[bs..bs + 3].copy_from_slice(&[0.7, -0.2, 1.1]);
        let orientation = nalgebra::UnitQuaternion::from_scaled_axis(nalgebra::Vector3::new(
            rotation,
            -2.0 * rotation,
            0.3 * rotation,
        ));
        posed.states[bs + 3..bs + 7].copy_from_slice(&[
            orientation.w,
            orientation.i,
            orientation.j,
            orientation.k,
        ]);
        for theta in [-1.0, -1e-8, 0.0, 1e-8, 0.8] {
            let motion = map
                .solve(&posed, &[theta], &[0.1, 0.2, -0.3, 0.4, -0.1, 0.2, 0.7])
                .unwrap();
            let (link, slide) = expected(theta);
            assert!((motion.generalized.q[b] - link).abs() < 1e-9);
            assert!((motion.generalized.q[s] - slide).abs() < 1e-10);
            let h = 1e-4;
            let plus = expected(theta + h);
            let minus = expected(theta - h);
            assert!((motion.tangent[(6 + b, 6)] - (plus.0 - minus.0) / (2.0 * h)).abs() < 1e-8);
            assert!((motion.tangent[(6 + s, 6)] - (plus.1 - minus.1) / (2.0 * h)).abs() < 1e-8);
            let answer = map.accelerations(&motion, &[0.0; 9]).unwrap();
            assert!(
                art.original_closure(&answer.generalized)
                    .iter()
                    .all(|r| r.position.abs() < 1e-10
                        && r.velocity.abs() < 1e-9
                        && r.acceleration.abs() < 1e-8)
            );
        }
    }
}

#[test]
fn implicit_qr_linkage_refines_toward_independent_explicit_reference() {
    let (art, seed) = slider_crank(false);
    let reference_map =
        RigidEmbedding::new(&art, &["joint.motor".into()], Default::default()).unwrap();
    let map = RigidEmbedding::new(
        &art,
        &["joint.motor".into()],
        EmbeddingConfig {
            dependent_solve: DependentSolve::PivotedQr,
            direct_closure_jacobian: true,
            ..Default::default()
        },
    )
    .unwrap();
    let start = reference_map
        .solve(&seed, &[0.2], &[0.4])
        .unwrap()
        .generalized;
    let mut reference = start.clone();
    let duration = 0.01;
    for i in 0..1000 {
        reference = reference_map
            .step_midpoint(
                &reference,
                i as f64 * duration / 1000.0,
                duration / 1000.0,
                |_, _| Ok(vec![0.0; 3]),
            )
            .unwrap()
            .endpoint
            .generalized;
    }
    let mut errors = Vec::new();
    for n in [20, 40, 80] {
        let mut g = start.clone();
        let h = duration / n as f64;
        for i in 0..n {
            let step = map
                .step_implicit(&g, i as f64 * h, h, &Default::default(), |_, _| {
                    Ok(vec![0.0; 3])
                })
                .unwrap();
            assert!(step.diagnostics.maximum_scaled_velocity_residual < 1e-9);
            g = step.endpoint.generalized;
            assert!(
                art.original_closure(&g)
                    .iter()
                    .all(|r| r.position.abs() < 1e-10
                        && r.velocity.abs() < 1e-9
                        && r.acceleration.abs() < 1e-8)
            );
        }
        // Dimensionless norm with a one-second velocity scale, comparing every
        // coordinate rather than only the independently integrated motor.
        errors.push(
            g.q.iter()
                .zip(&reference.q)
                .chain(g.qd.iter().zip(&reference.qd))
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt(),
        );
    }
    assert!(errors[2] < 1e-3, "{errors:?}");
    assert!(
        errors
            .windows(2)
            .all(|e| e[0] / e[1] > 1.9 && e[0] / e[1] < 2.1),
        "{errors:?}"
    );
}

#[test]
fn integrated_linkage_preserves_closure_and_refines_energy_error() {
    let (art, seed) = slider_crank(false);
    let map = RigidEmbedding::new(&art, &["joint.motor".into()], Default::default()).unwrap();
    let start = map.solve(&seed, &[0.2], &[0.4]).unwrap().generalized;
    let gravity = nalgebra::Vector3::from_column_slice(&art.model.gravity);
    let energy = |g: &Generalized| {
        art.evaluate(g)
            .links
            .iter()
            .zip(&art.links)
            .map(|(k, l)| {
                0.5 * l.mass * k.vel.norm_squared()
                    + 0.5 * k.w.dot(&(k.r * l.inertia * k.r.transpose() * k.w))
                    - l.mass * gravity.dot(&k.p)
            })
            .sum::<f64>()
    };
    let e0 = energy(&start);
    let a = map
        .accelerations(&map.solve(&start, &[0.2], &[0.4]).unwrap(), &[0.0; 3])
        .unwrap()
        .reduced_accelerations[0];
    let eps = 1e-6;
    let plus = map
        .solve(&start, &[0.2 + eps * 0.4], &[0.4 + eps * a])
        .unwrap();
    let minus = map
        .solve(&start, &[0.2 - eps * 0.4], &[0.4 - eps * a])
        .unwrap();
    assert!(
        ((energy(&plus.generalized) - energy(&minus.generalized)) / (2.0 * eps)).abs() < 1e-8,
        "continuous energy derivative"
    );
    let mut errors = Vec::new();
    // This unactuated linkage reaches >35 rad/s. Resolve that motion before
    // checking asymptotic order; 1–2 ms steps are outside that error regime.
    for steps in [1600, 3200, 6400] {
        let h = 0.2 / steps as f64;
        let mut g = start.clone();
        for i in 0..steps {
            g = map
                .step_midpoint(&g, i as f64 * h, h, |_, _| Ok(vec![0.0; 3]))
                .unwrap()
                .endpoint
                .generalized;
            assert!(
                art.original_closure(&g)
                    .iter()
                    .all(|r| r.position.abs() < 1e-10
                        && r.velocity.abs() < 1e-9
                        && r.acceleration.abs() < 1e-8)
            );
        }
        errors.push((energy(&g) - e0).abs());
    }
    assert!(
        errors[2] < 1e-7
            && errors
                .windows(2)
                .all(|e| e[0] / e[1] > 3.7 && e[0] / e[1] < 4.3),
        "{errors:?}"
    );
}

#[test]
fn blocked_factorization_preserves_closed_motion_across_poses_and_toggle_rejection() {
    for method in [DependentSolve::Svd, DependentSolve::PivotedQr] {
        for floating in [false, true] {
            let (art, seed) = slider_crank(floating);
            let config = EmbeddingConfig {
                direct_closure_jacobian: true,
                dependent_solve: method,
                ..Default::default()
            };
            let ordinary =
                RigidEmbedding::new(&art, &["joint.motor".into()], config.clone()).unwrap();
            let blocked = RigidEmbedding::new(
                &art,
                &["joint.motor".into()],
                EmbeddingConfig {
                    block_dependent_factorization: true,
                    ..config
                },
            )
            .unwrap();
            for angle in [-0.7, -0.1, 0.0, 0.1, 0.7] {
                let mut velocity = vec![0.0; ordinary.reduced_dimension()];
                if floating {
                    velocity[..6].copy_from_slice(&[0.3, -0.1, 0.2, 0.4, 0.1, -0.2]);
                }
                *velocity.last_mut().unwrap() = 0.7;
                let a = ordinary.solve(&seed, &[angle], &velocity).unwrap();
                let b = blocked.solve(&seed, &[angle], &velocity).unwrap();
                assert!(
                    a.generalized
                        .q
                        .iter()
                        .zip(&b.generalized.q)
                        .all(|(x, y)| (x - y).abs() < 1e-12)
                );
                assert!((&a.tangent - &b.tangent).amax() < 1e-12);
                assert!((&a.acceleration_bias - &b.acceleration_bias).amax() < 1e-12);
                assert!(
                    b.maximum_scaled_position_error < 1e-8
                        && b.maximum_scaled_velocity_error < 1e-8
                        && b.maximum_scaled_acceleration_error < 1e-8
                );
            }
        }
        let (art, mut seed) = slider_crank_length(false, 0.05);
        let motor = art
            .dofs()
            .position(|(_, d)| d.name == "joint.motor")
            .unwrap();
        seed.q[motor] = std::f64::consts::FRAC_PI_2;
        let map = RigidEmbedding::new(
            &art,
            &["joint.motor".into()],
            EmbeddingConfig {
                dependent_solve: method,
                direct_closure_jacobian: true,
                block_dependent_factorization: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            map.solve(&seed, &[seed.q[motor]], &[0.0])
                .unwrap_err()
                .contains("singular")
        );
    }
}
