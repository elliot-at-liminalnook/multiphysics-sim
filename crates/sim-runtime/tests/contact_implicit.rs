use sim_core::Behavior;
use sim_domain_multibody::smooth_contact::{SmoothContact, SmoothContactConfig};
use sim_domain_robot::{Articulated, Options};
use sim_runtime::{
    contact_implicit::{ContactImplicitConfig, ContactImplicitPlanner},
    session::Scene,
    tracking::{CaptureConfig, Marker},
};
use sim_solve::least_squares::{LeastSquaresConfig, VariableBound};
use std::{collections::BTreeMap, sync::Arc};

#[test]
fn planning_contact_is_discoverable_with_explicit_units_and_required_parameters() {
    use sim_core::{PortSchema, QuantityKind};
    let registry = sim_runtime::registry();
    let descriptor = registry
        .get(&sim_domain_multibody::smooth_contact::SMOOTH_CONTACT.into())
        .unwrap();
    assert_eq!(
        descriptor.ports[0].schema,
        PortSchema::SignalIn(QuantityKind::Length)
    );
    assert!(
        descriptor.ports[1..4]
            .iter()
            .all(|p| p.schema == PortSchema::SignalIn(QuantityKind::LinearVelocity))
    );
    assert!(
        descriptor.ports[4..]
            .iter()
            .all(|p| p.schema == PortSchema::SignalOut(QuantityKind::Force))
    );
    let factory = descriptor.equations.unwrap();
    assert!(factory(&BTreeMap::new()).is_err());
    let c = config(vec![vec![0.; 6]; 3]).contact;
    let mut parameters: BTreeMap<String, f64> =
        serde_json::from_value(serde_json::to_value(c).unwrap()).unwrap();
    assert!(factory(&parameters).is_ok());
    parameters.insert("smoothing_m".into(), 0.);
    assert!(factory(&parameters).is_err());
    assert_eq!(descriptor.parameters.as_ref().unwrap().len(), 5);
}

fn fixture() -> (Articulated, CaptureConfig) {
    let mut scene: Scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap();
    let mut body = scene
        .robot
        .links
        .iter()
        .find(|l| l.name == "pendulum")
        .unwrap()
        .clone();
    body.name = "analytic mass".into();
    body.mass = 1.;
    body.com = [0.; 3];
    body.ground = false;
    body.flex = None;
    body.inertia = [[0.1, 0., 0.], [0., 0.1, 0.], [0., 0., 0.1]];
    scene.robot.links = vec![body];
    scene.robot.joints.clear();
    scene.robot.motors.clear();
    scene.robot.transmissions.clear();
    scene.robot.source = serde_json::json!({"cad_sha256":"analytic-unit-mass"});
    scene.robot.gravity = [0., 0., -9.81];
    scene.robot.world.floor_z = 0.;
    scene.robot.world.terrain = None;
    let art = Articulated::new(
        Arc::new(scene.robot),
        &Options {
            contact: false,
            flex: false,
            ..Default::default()
        },
    )
    .unwrap();
    let markers = CaptureConfig {
        experiment_id: "contact-emergence".into(),
        coordinate_frame: "world-Z-up-m".into(),
        expected_cad_sha256: Some("analytic-unit-mass".into()),
        markers: vec![Marker {
            id: "com".into(),
            link: "analytic mass".into(),
            local_point_m: [0.; 3],
        }],
    };
    (art, markers)
}
fn config(reference: Vec<Vec<f64>>) -> ContactImplicitConfig {
    ContactImplicitConfig {
        expected_cad_sha256: "analytic-unit-mass".into(),
        independent_coordinates: vec![],
        embedding: Default::default(),
        actuators: BTreeMap::new(),
        contact: SmoothContactConfig {
            stiffness_n_m: 2000.,
            smoothing_m: 0.001,
            dissipation_velocity_m_s: 0.2,
            friction_coefficient: 0.3,
            stiction_velocity_m_s: 0.01,
        },
        step_s: 0.02,
        initial_velocity: vec![0.; 6],
        periodic_horizontal_translation: false,
        periodic_cubic_subdivisions: None,
        periodic_collocation_phases: None,
        position_reference: reference,
        position_scales: vec![1.; 6],
        velocity_reference: vec![0.; 6],
        velocity_scales: vec![1.; 6],
        force_tolerance_n: 0.001,
        moment_tolerance_nm: 0.001,
        torque_tolerance_nm: 0.001,
        torque_effort_scale_nm: 1.,
        maximum_point_penetration_m: 0.02,
        penetration_scale_m: 0.001,
        contact_sliding_work_scale_j: None,
    }
}
#[test]
fn gravity_and_state_contact_balance_without_actuation_or_contact_schedule() {
    let (art, markers) = fixture();
    let seed = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.; art.state_count],
        &vec![0.; art.port_names.len() + 1],
        vec![],
    );
    let c = config(vec![vec![0.; 6]; 3]);
    let law = SmoothContact::new(c.contact.clone()).unwrap();
    // Analytic equilibrium of softplus(k,sigma) against 1 kg * gravity.
    let z = -0.001 * (9.81_f64 / (2000. * 0.001)).exp_m1().ln();
    let positions = vec![vec![0., 0., z, 0., 0., 0.]; 3];
    let mut c = config(positions.clone());
    let p = ContactImplicitPlanner::new(&art, &seed, &markers, c.clone()).unwrap();
    let r = p.evaluate(&positions).unwrap();
    assert!(r.within_planning_tolerances);
    assert!(r.maximum_force_error_n < 1e-10);
    assert!((law.sample(z, [0.; 3]).unwrap().force_n[0] - 9.81).abs() < 1e-10);
    assert!(r.frames.iter().all(|f| f.motor_torques_nm.is_empty()));
    // The same body's free-flight inverse dynamics require no applied wrench.
    let dt = c.step_s;
    c.initial_velocity[2] = 1.;
    let mut positions = vec![vec![0., 0., 10., 0., 0., 0.]];
    let mut velocity = 1.;
    for _ in 0..2 {
        velocity -= 9.81 * dt;
        let z = positions.last().unwrap()[2] + velocity * dt;
        positions.push(vec![0., 0., z, 0., 0., 0.]);
    }
    c.position_reference = positions.clone();
    let r = ContactImplicitPlanner::new(&art, &seed, &markers, c)
        .unwrap()
        .evaluate(&positions)
        .unwrap();
    assert!(r.maximum_force_error_n < 1e-9);
    assert!(
        r.frames
            .iter()
            .all(|f| f.contact_forces_world_n == vec![[0.; 3]])
    );
}

#[test]
fn tilted_surface_contacts_match_runtime_samples_and_generate_edge_moments() {
    use nalgebra::{UnitQuaternion, Vector3};
    use sim_runtime::tracking::compiled_surface_markers;
    let (mut art, mut markers) = fixture();
    art.links[0].contact = [-0.01, 0.01]
        .into_iter()
        .flat_map(|x| {
            [-0.01, 0.01]
                .into_iter()
                .map(move |y| Vector3::new(x, y, -0.02))
        })
        .collect();
    let links = vec!["analytic mass".to_string()];
    assert!(compiled_surface_markers(&art, &links, "wrong-CAD").is_err());
    assert!(
        compiled_surface_markers(
            &art,
            &[links[0].clone(), links[0].clone()],
            "analytic-unit-mass"
        )
        .is_err()
    );
    assert!(compiled_surface_markers(&art, &["missing".into()], "analytic-unit-mass").is_err());
    markers.markers = compiled_surface_markers(&art, &links, "analytic-unit-mass").unwrap();
    assert_eq!(markers.markers.len(), 4);
    let seed = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.; art.state_count],
        &vec![0.; art.port_names.len() + 1],
        vec![],
    );
    let angle = std::f64::consts::FRAC_PI_4;
    let positions = vec![vec![0., 0., 0.02, angle, 0., 0.]; 3];
    let c = config(positions.clone());
    let law = SmoothContact::new(c.contact.clone()).unwrap();
    let planner = ContactImplicitPlanner::new(&art, &seed, &markers, c).unwrap();
    let report = planner.evaluate(&positions).unwrap();
    let rotation = UnitQuaternion::from_scaled_axis(Vector3::new(angle, 0., 0.));
    let mut force = Vector3::zeros();
    let mut moment = Vector3::zeros();
    for (i, point) in art.links[0].contact.iter().enumerate() {
        let arm = rotation * point;
        let gap = 0.02 + arm.z;
        assert!((report.frames[0].gaps_m[i] - gap).abs() < 1e-12);
        let f = Vector3::new(0., 0., law.sample(gap, [0.; 3]).unwrap().force_n[0]);
        force += f;
        moment += arm.cross(&f);
    }
    assert!(
        moment.x.abs() > 0.01,
        "edge contact must produce a nonzero moment"
    );
    assert!((report.frames[0].unactuated_wrench[2] - (9.81 - force.z)).abs() < 1e-10);
    assert!((report.frames[0].unactuated_wrench[3] + moment.x).abs() < 1e-10);
    let geometry = planner.audit_geometry(&positions, 1).unwrap();
    let detailed = planner.audit_geometry_detailed(&positions, 1).unwrap();
    assert_eq!(geometry.len(), detailed.len());
    for (compact, full) in geometry.iter().zip(&detailed) {
        assert!(compact.inter_link_penetrations.is_none());
        assert!(compact.poses.is_none());
        let poses = full.poses.as_ref().unwrap();
        assert_eq!(poses.len(), 1);
        assert_eq!(poses[0].name, "analytic mass");
        assert_eq!(poses[0].position_m, [0., 0., 0.02]);
        let expected_rotation = rotation.to_rotation_matrix();
        for i in 0..3 {
            for j in 0..3 {
                assert!((poses[0].rotation[i][j] - expected_rotation[(i, j)]).abs() < 1e-12);
            }
        }
        let pairs = full.inter_link_penetrations.as_ref().unwrap();
        assert_eq!(
            full.maximum_inter_link_penetration_m,
            pairs.iter().map(|p| p.penetration_m).fold(0., f64::max)
        );
        let mut json = serde_json::to_value(full).unwrap();
        json.as_object_mut()
            .unwrap()
            .remove("inter_link_penetrations");
        json.as_object_mut().unwrap().remove("poses");
        assert_eq!(serde_json::to_value(compact).unwrap(), json);
    }
    let minimum_gap = report.frames[0]
        .gaps_m
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    assert!(minimum_gap < 0.);
    assert!((geometry[0].floor_clearances[0].minimum_clearance_m - minimum_gap).abs() < 1e-12);
}

#[test]
fn resolved_contact_profile_uses_material_pair_and_compiled_dissipation() {
    let (mut art, _) = fixture();
    // Distinct from the authored world scalar: inspection must use resolved data.
    art.links[0].floor_mu = (0.37, 0.23);
    art.floor_dissipation_s_m = 0.7;
    let p = sim_runtime::contact_audit::floor_contact_profile(
        &art,
        &["analytic mass".into()],
        "analytic-unit-mass",
    )
    .unwrap();
    assert_eq!(p.links[0].kinetic_friction, 0.23);
    assert_eq!(p.links[0].static_friction, 0.37);
    assert_eq!(p.normal_dissipation_s_m, 0.7);
    assert_eq!(p.stiffness_per_sample_n_m, art.floor_k);
}

#[test]
fn optional_sliding_cost_equals_analytic_friction_work_without_changing_forces() {
    let (art, markers) = fixture();
    let seed = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.; art.state_count],
        &vec![0.; art.port_names.len() + 1],
        vec![],
    );
    let z = -0.001 * (9.81_f64 / (2000. * 0.001)).exp_m1().ln();
    let speed = 0.1;
    let dt = 0.02;
    let positions = (0..3)
        .map(|k| vec![speed * k as f64 * dt, 0., z, 0., 0., 0.])
        .collect::<Vec<_>>();
    let mut c = config(positions.clone());
    c.initial_velocity[0] = speed;
    let unweighted = ContactImplicitPlanner::new(&art, &seed, &markers, c.clone())
        .unwrap()
        .evaluate(&positions)
        .unwrap();
    assert!(unweighted.contact_sliding_work_j.is_none());
    let scale = 0.1;
    c.contact_sliding_work_scale_j = Some(scale);
    let weighted = ContactImplicitPlanner::new(&art, &seed, &markers, c.clone())
        .unwrap()
        .evaluate(&positions)
        .unwrap();
    let energy = 2. * dt * 0.3 * 9.81 * speed * speed / (0.01_f64.powi(2) + speed * speed).sqrt();
    assert!((weighted.contact_sliding_work_j.unwrap() - energy).abs() < 1e-12);
    let cost = |r: &Vec<f64>| 0.5 * r.iter().map(|v| v * v).sum::<f64>();
    assert!(
        (cost(&weighted.residuals) - cost(&unweighted.residuals) - energy / scale).abs() < 1e-9
    );
    assert_eq!(
        weighted.frames[0].contact_forces_world_n,
        unweighted.frames[0].contact_forces_world_n
    );
    c.contact_sliding_work_scale_j = Some(0.);
    assert!(ContactImplicitPlanner::new(&art, &seed, &markers, c).is_err());
}

#[test]
fn optimizer_discovers_touchdown_from_a_separated_stationary_guess() {
    let (art, markers) = fixture();
    let seed = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.; art.state_count],
        &vec![0.; art.port_names.len() + 1],
        vec![],
    );
    let positions = vec![vec![0., 0., 0.05, 0., 0., 0.]; 21];
    let c = config(positions.clone());
    let planner = ContactImplicitPlanner::new(&art, &seed, &markers, c).unwrap();
    let before = planner.evaluate(&positions).unwrap();
    assert!(before.maximum_force_error_n > 9.8);
    assert!(
        before
            .frames
            .iter()
            .all(|f| f.contact_forces_world_n[0][2] < 1e-15)
    );
    let bounds = vec![
        (0..6)
            .map(|i| if i == 2 {
                VariableBound {
                    lower: -0.02,
                    upper: 0.1,
                }
            } else {
                VariableBound {
                    lower: 0.,
                    upper: 0.,
                }
            })
            .collect::<Vec<_>>();
        20
    ];
    let search = LeastSquaresConfig {
        maximum_iterations: 100,
        maximum_evaluations: 5000,
        difference_step: 1e-5,
        initial_damping: 0.01,
        gradient_tolerance: 1e-6,
    };
    // Numerical continuation changes only the declared planning smoothing.
    // Every stage keeps the original fixed initial state and dynamics tolerances.
    let mut guess = positions.clone();
    let mut last = None;
    for smoothing in [0.01, 0.003, 0.001] {
        let mut c = config(positions.clone());
        c.contact.smoothing_m = smoothing;
        let planner = ContactImplicitPlanner::new(&art, &seed, &markers, c).unwrap();
        let result = planner.optimize(&guess, &bounds, &search, |_| {}).unwrap();
        eprintln!(
            "smoothing {smoothing}, force {}, {:?}",
            result.report.maximum_force_error_n, result.search.termination
        );
        guess = result.positions.clone();
        last = Some(result);
    }
    let result = last.unwrap();
    assert!(
        result.report.maximum_force_error_n < 0.001,
        "{}",
        result.report.maximum_force_error_n
    );
    assert!(
        result
            .report
            .frames
            .iter()
            .any(|f| f.gaps_m[0] < 0. && f.contact_forces_world_n[0][2] > 9.81)
    );
    assert!(
        result
            .report
            .frames
            .iter()
            .take(3)
            .all(|f| f.contact_forces_world_n[0][2] < 1e-6)
    );
    assert_eq!(result.positions[0], positions[0]);
}

#[test]
fn cached_three_knot_evaluation_is_exact_for_probes_and_invalid_inputs() {
    let (art, mut markers) = fixture();
    markers.markers[0].local_point_m = [0.03, -0.02, -0.01];
    let seed = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.; art.state_count],
        &vec![0.; art.port_names.len() + 1],
        vec![],
    );
    let positions = (0..8)
        .map(|k| {
            let t = k as f64 * 0.02;
            vec![
                0.1 * t,
                -0.03 * t,
                0.009 + 0.001 * t,
                0.2 + t * t,
                -0.1 + t,
                0.3 - t,
            ]
        })
        .collect::<Vec<_>>();
    for work in [None, Some(0.02)] {
        let mut c = config(positions.clone());
        c.initial_velocity = vec![0.1, -0.03, 0.001, 0.02, -0.03, 0.01];
        c.contact_sliding_work_scale_j = work;
        let planner = ContactImplicitPlanner::new(&art, &seed, &markers, c).unwrap();
        let mut cached = planner.evaluator();
        let mut check = |q: &[Vec<f64>]| {
            let expected = serde_json::to_vec(&planner.evaluate(q).unwrap()).unwrap();
            let actual = serde_json::to_vec(&cached.evaluate(q).unwrap()).unwrap();
            assert!(
                actual == expected,
                "cached report differs from uncached physics"
            );
        };
        check(&positions);
        check(&positions);
        // Forward/backward finite differences, reversions, and every endpoint/rotation.
        for k in 1..positions.len() {
            for j in 0..6 {
                for delta in [1e-5, -1e-5] {
                    let mut q = positions.clone();
                    q[k][j] += delta;
                    check(&q);
                }
            }
        }
        check(&positions);
        assert!(cached.reused_frames > cached.computed_frames);
        let before = cached.computed_frames;
        let mut last = positions.clone();
        last[7][0] += 1e-5;
        cached.evaluate(&last).unwrap();
        assert_eq!(cached.computed_frames - before, 1);
        let mut invalid = positions.clone();
        invalid[0][0] += 1.;
        assert!(cached.evaluate(&invalid).is_err());
        invalid = positions.clone();
        invalid[3][0] = f64::NAN;
        assert!(cached.evaluate(&invalid).is_err());
        assert!(
            serde_json::to_vec(&cached.evaluate(&positions).unwrap()).unwrap()
                == serde_json::to_vec(&planner.evaluate(&positions).unwrap()).unwrap()
        );
    }
}

#[test]
fn periodic_boundary_closes_velocity_and_repeats_the_same_physics() {
    let (art, mut markers) = fixture();
    markers.markers[0].local_point_m = [0.02, -0.01, 0.];
    let seed = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.; art.state_count],
        &vec![0.; art.port_names.len() + 1],
        vec![],
    );
    let mut q = (0..5)
        .map(|k| {
            let phase = std::f64::consts::TAU * k as f64 / 4.;
            vec![
                0.002 * k as f64,
                0.,
                -0.004 + 0.0001 * phase.sin(),
                0.02 * phase.sin(),
                0.01 * phase.cos(),
                0.,
            ]
        })
        .collect::<Vec<_>>();
    q[4] = q[0].clone();
    q[4][0] += 0.008;
    let mut c = config(q.clone());
    c.periodic_horizontal_translation = true;
    assert!(
        ContactImplicitPlanner::new(&art, &seed, &markers, c.clone()).is_err(),
        "must not ignore a supplied initial velocity"
    );
    c.initial_velocity.clear();
    c.contact_sliding_work_scale_j = Some(0.01);
    let planner = ContactImplicitPlanner::new(&art, &seed, &markers, c.clone()).unwrap();
    let report = planner.evaluate(&q).unwrap();
    let boundary = report.periodic_boundary.as_ref().unwrap();
    assert_eq!(
        boundary.initial_velocity,
        report.frames.last().unwrap().velocity
    );
    assert_eq!(boundary.translation_m, [0.008, 0., 0.]);
    let mut cache = planner.evaluator();
    cache.evaluate(&q).unwrap();
    let mut changed = q.clone();
    changed[3][2] += 1e-5;
    let cached = cache.evaluate(&changed).unwrap();
    let direct = planner.evaluate(&changed).unwrap();
    assert!(serde_json::to_vec(&cached).unwrap() == serde_json::to_vec(&direct).unwrap());
    assert_ne!(
        cached.frames[0].acceleration, report.frames[0].acceleration,
        "last unique knot must affect first-frame acceleration through the periodic seam"
    );
    let mut invalid = q.clone();
    invalid[4][2] += 1e-12;
    assert!(
        planner.evaluate(&invalid).is_err(),
        "even a tiny endpoint closure error is rejected"
    );
    let mut twice = q.clone();
    twice.extend(q[1..].iter().map(|p| {
        let mut p = p.clone();
        p[0] += 0.008;
        p
    }));
    c.periodic_horizontal_translation = false;
    c.initial_velocity = boundary.initial_velocity.clone();
    c.position_reference = twice.clone();
    let repeated = ContactImplicitPlanner::new(&art, &seed, &markers, c)
        .unwrap()
        .evaluate(&twice)
        .unwrap();
    for k in 0..8 {
        for (a, b) in repeated.frames[k]
            .unactuated_wrench
            .iter()
            .zip(&report.frames[k % 4].unactuated_wrench)
        {
            assert!(
                (a - b).abs() < 1e-8,
                "translated repetitions must have the same inverse dynamics"
            );
        }
    }
}

#[test]
fn periodic_optimizer_can_change_its_first_pose_and_preserves_exact_closure() {
    let (art, markers) = fixture();
    let seed = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.; art.state_count],
        &vec![0.; art.port_names.len() + 1],
        vec![],
    );
    let mut q = (0..5)
        .map(|k| vec![0.002 * k as f64, 0., -0.003, 0., 0., 0.])
        .collect::<Vec<_>>();
    let mut c = config(q.clone());
    c.periodic_horizontal_translation = true;
    c.initial_velocity.clear();
    c.contact.friction_coefficient = 0.;
    c.velocity_reference[0] = 0.1;
    let planner = ContactImplicitPlanner::new(&art, &seed, &markers, c).unwrap();
    let mut bounds = q[..4]
        .iter()
        .enumerate()
        .map(|(k, p)| {
            p.iter()
                .enumerate()
                .map(|(j, v)| {
                    if j == 2 {
                        VariableBound {
                            lower: -0.01,
                            upper: 0.,
                        }
                    } else if j == 0 && k > 0 {
                        VariableBound {
                            lower: -0.02,
                            upper: 0.04,
                        }
                    } else {
                        VariableBound {
                            lower: *v,
                            upper: *v,
                        }
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    bounds.push(vec![
        VariableBound {
            lower: 0.,
            upper: 0.02,
        },
        VariableBound {
            lower: 0.,
            upper: 0.,
        },
    ]);
    let search = LeastSquaresConfig {
        maximum_iterations: 60,
        maximum_evaluations: 2000,
        difference_step: 1e-6,
        initial_damping: 0.1,
        gradient_tolerance: 1e-7,
    };
    let result = planner
        .optimize_scaled(&q, &bounds, &search, 0.25, |_| {})
        .unwrap();
    assert!(
        result.report.within_planning_tolerances,
        "{}",
        result.report.maximum_force_error_n
    );
    assert!((result.positions[0][2] - q[0][2]).abs() > 1e-4);
    assert_eq!(
        result.positions[0][2..],
        result.positions.last().unwrap()[2..]
    );
    assert!(result.report.periodic_boundary.unwrap().initial_velocity[0] > 0.09);
    // A fixed at-rest start would require a spurious initial horizontal force.
    q = result.positions;
    let mut c = config(q.clone());
    c.contact.friction_coefficient = 0.;
    let startup = ContactImplicitPlanner::new(&art, &seed, &markers, c)
        .unwrap()
        .evaluate(&q)
        .unwrap();
    assert!(startup.maximum_force_error_n > 1.);
}

#[test]
fn cubic_collocation_uses_analytic_acceleration_and_is_grid_independent() {
    let (art, markers) = fixture();
    let seed = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.; art.state_count],
        &vec![0.; art.port_names.len() + 1],
        vec![],
    );
    let q = vec![
        vec![0., 0., -0.004, 0.01, 0.02, 0.],
        vec![0.002, 0.001, -0.003, 0.02, 0., 0.01],
        vec![0.004, -0.001, -0.0045, -0.01, 0.01, 0.],
        vec![0.006, 0., -0.005, 0., -0.01, -0.01],
        vec![0.008, 0., -0.004, 0.01, 0.02, 0.],
    ];
    let mut c = config(q.clone());
    c.periodic_horizontal_translation = true;
    c.initial_velocity.clear();
    c.periodic_cubic_subdivisions = Some(2);
    let planner = ContactImplicitPlanner::new(&art, &seed, &markers, c.clone()).unwrap();
    let coarse = planner.evaluate(&q).unwrap();
    assert_eq!(coarse.frames.len(), 8);
    assert!(
        coarse
            .frames
            .iter()
            .any(|f| f.acceleration.iter().any(|a| a.abs() > 1.))
    );
    for f in &coarse.frames {
        for j in 0..3 {
            let expected =
                f.acceleration[j] + if j == 2 { 9.81 } else { 0. } - f.contact_forces_world_n[0][j];
            assert!((f.unactuated_wrench[j] - expected).abs() < 1e-9);
            assert!((f.unactuated_wrench[3 + j] - 0.1 * f.acceleration[3 + j]).abs() < 1e-8);
        }
    }
    let boundary = coarse.periodic_boundary.as_ref().unwrap();
    assert_eq!(
        boundary.initial_velocity,
        coarse.frames.last().unwrap().velocity
    );
    assert_ne!(
        boundary.initial_position.as_ref().unwrap(),
        &q[0],
        "B-spline controls are not interpolation knots"
    );
    c.periodic_cubic_subdivisions = Some(4);
    let fine = ContactImplicitPlanner::new(&art, &seed, &markers, c.clone())
        .unwrap()
        .evaluate(&q)
        .unwrap();
    for (k, frame) in coarse.frames.iter().enumerate() {
        let other = &fine.frames[2 * k + 1];
        assert_eq!(frame.position, other.position);
        assert_eq!(frame.velocity, other.velocity);
        assert_eq!(frame.acceleration, other.acceleration);
        assert_eq!(frame.unactuated_wrench, other.unactuated_wrench);
    }
    let geometry = planner.audit_geometry(&q, 2).unwrap();
    for (k, f) in coarse.frames.iter().enumerate() {
        assert!((geometry[k + 1].time_s - f.time_s).abs() < 1e-15);
    }
    c.periodic_cubic_subdivisions = Some(0);
    assert!(ContactImplicitPlanner::new(&art, &seed, &markers, c.clone()).is_err());
    c.periodic_cubic_subdivisions = Some(4);
    c.periodic_horizontal_translation = false;
    assert!(ContactImplicitPlanner::new(&art, &seed, &markers, c).is_err());
}

#[test]
fn cubic_cache_matches_every_control_probe_and_constant_translation_balances_gravity() {
    let (art, markers) = fixture();
    let seed = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.; art.state_count],
        &vec![0.; art.port_names.len() + 1],
        vec![],
    );
    let z = -0.001 * (9.81_f64 / (2000. * 0.001)).exp_m1().ln();
    let q = (0..5)
        .map(|k| vec![0.002 * k as f64, 0., z, 0., 0., 0.])
        .collect::<Vec<_>>();
    let mut c = config(q.clone());
    c.periodic_horizontal_translation = true;
    c.initial_velocity.clear();
    c.periodic_cubic_subdivisions = Some(4);
    c.contact.friction_coefficient = 0.;
    c.contact_sliding_work_scale_j = Some(0.01);
    let planner = ContactImplicitPlanner::new(&art, &seed, &markers, c).unwrap();
    let r = planner.evaluate(&q).unwrap();
    assert!(r.within_planning_tolerances);
    assert!(r.maximum_force_error_n < 1e-10);
    let mut cache = planner.evaluator();
    cache.evaluate(&q).unwrap();
    for k in 0..4 {
        for j in 0..6 {
            for sign in [-1., 1.] {
                let mut probe = q.clone();
                probe[k][j] += sign * 1e-5;
                if k == 0 && j >= 2 {
                    probe[4][j] = probe[0][j];
                }
                let actual = cache.evaluate(&probe).unwrap();
                let expected = planner.evaluate(&probe).unwrap();
                assert!(
                    serde_json::to_vec(&actual).unwrap() == serde_json::to_vec(&expected).unwrap(),
                    "cubic cached control probe mismatch"
                );
            }
        }
    }
    let mut probe = q;
    probe[4][0] += 1e-4;
    assert!(
        serde_json::to_vec(&cache.evaluate(&probe).unwrap()).unwrap()
            == serde_json::to_vec(&planner.evaluate(&probe).unwrap()).unwrap()
    );
    assert!(cache.reused_frames > 0);
}

#[test]
fn irregular_cubic_collocation_preserves_physics_weights_and_cache() {
    let (art, markers) = fixture();
    let seed = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.; art.state_count],
        &vec![0.; art.port_names.len() + 1],
        vec![],
    );
    let q = (0..5)
        .map(|k| vec![0.002 * k as f64, 0., -0.001, 0., 0., 0.])
        .collect::<Vec<_>>();
    let mut c = config(q.clone());
    c.periodic_horizontal_translation = true;
    c.initial_velocity.clear();
    c.periodic_cubic_subdivisions = Some(4);
    c.contact_sliding_work_scale_j = Some(0.01);
    let uniform = ContactImplicitPlanner::new(&art, &seed, &markers, c.clone())
        .unwrap()
        .evaluate(&q)
        .unwrap();
    c.periodic_collocation_phases = Some(vec![0.03125, 0.1, 0.25, 0.27, 0.5, 0.875, 1.]);
    let planner = ContactImplicitPlanner::new(&art, &seed, &markers, c.clone()).unwrap();
    let irregular = planner.evaluate(&q).unwrap();
    assert_eq!(irregular.frames.len(), 7);
    // Constant velocity/contact force: changing quadrature cells must preserve
    // integrated cost and work rather than charging once per inserted point.
    let norm = |r: &sim_runtime::contact_implicit::ContactImplicitReport| {
        r.residuals.iter().map(|v| v * v).sum::<f64>()
    };
    assert!((norm(&uniform) - norm(&irregular)).abs() < 1e-8 * norm(&uniform));
    assert!(
        (uniform.contact_sliding_work_j.unwrap() - irregular.contact_sliding_work_j.unwrap()).abs()
            < 1e-14
    );
    for &(i, j) in &[(2, 3), (4, 7), (5, 13), (6, 15)] {
        assert_eq!(irregular.frames[i].position, uniform.frames[j].position);
        assert_eq!(
            irregular.frames[i].unactuated_wrench,
            uniform.frames[j].unactuated_wrench
        );
    }
    let mut cache = planner.evaluator();
    cache.evaluate(&q).unwrap();
    for k in 0..4 {
        for j in 0..6 {
            let mut probe = q.clone();
            probe[k][j] += 1e-5;
            if k == 0 && j >= 2 {
                probe[4][j] = probe[0][j];
            }
            let a = cache.evaluate(&probe).unwrap();
            let b = planner.evaluate(&probe).unwrap();
            assert_eq!(
                serde_json::to_vec(&a).unwrap(),
                serde_json::to_vec(&b).unwrap()
            );
            for f in &b.frames {
                for j in 0..3 {
                    let expected = f.acceleration[j] + if j == 2 { 9.81 } else { 0. }
                        - f.contact_forces_world_n[0][j];
                    assert!((f.unactuated_wrench[j] - expected).abs() < 1e-9);
                }
            }
        }
    }
    for phases in [
        vec![],
        vec![1.],
        vec![0., 1.],
        vec![0.5, 0.5, 1.],
        vec![0.6, 0.5, 1.],
        vec![0.5, 0.9],
        vec![f64::NAN, 1.],
        vec![0.5, f64::INFINITY],
    ] {
        c.periodic_collocation_phases = Some(phases);
        assert!(ContactImplicitPlanner::new(&art, &seed, &markers, c.clone()).is_err());
    }
    c.periodic_collocation_phases = Some(vec![0.5, 1.]);
    c.periodic_cubic_subdivisions = None;
    assert!(ContactImplicitPlanner::new(&art, &seed, &markers, c).is_err());
}

#[test]
fn equality_partition_and_solver_enforce_unactuated_startup_dynamics() {
    use sim_solve::{
        equality_dogleg::{EqualityDoglegConfig, EqualityTermination},
        least_squares::VariableBound,
    };
    let (art, markers) = fixture();
    let seed = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.; art.state_count],
        &vec![0.; art.port_names.len() + 1],
        vec![],
    );
    let z = -0.001 * (9.81_f64 / (2000. * 0.001)).exp_m1().ln();
    let q = vec![
        vec![0., 0., z, 0., 0., 0.],
        vec![0.001, 0., z + 0.0001, 0., 0., 0.],
        vec![0.002, 0., z + 0.0001, 0., 0., 0.],
    ];
    let mut c = config(q.clone());
    c.contact_sliding_work_scale_j = Some(0.01);
    let planner = ContactImplicitPlanner::new(&art, &seed, &markers, c.clone()).unwrap();
    let report = planner.evaluate(&q).unwrap();
    let split = planner.equality_residuals(&report).unwrap();
    let full = report.residuals.iter().map(|v| v * v).sum::<f64>();
    let partition = split.objective.iter().map(|v| v * v).sum::<f64>()
        + c.step_s * split.equalities.iter().map(|v| v * v).sum::<f64>();
    assert!((full - partition).abs() < 1e-10 * full);
    assert!(report.contact_sliding_work_j.unwrap() > 0.);
    c.contact.friction_coefficient = 0.;
    c.velocity_reference[0] = 0.1;
    let planner = ContactImplicitPlanner::new(&art, &seed, &markers, c).unwrap();
    let bounds = vec![
        vec![
            VariableBound {
                lower: -0.1,
                upper: 0.1
            };
            6
        ];
        2
    ];
    let params = planner.parameterization(&q, &bounds).unwrap();
    assert_eq!(params.decode(&params.values).unwrap(), q);
    assert!(params.decode(&params.values[..11]).is_err());
    let mut invalid = params.values.clone();
    invalid[0] = f64::NAN;
    assert!(params.decode(&invalid).is_err());
    let mut search = EqualityDoglegConfig {
        globalization: Default::default(),
        maximum_iterations: 40,
        maximum_evaluations: 5000,
        difference_step: 1e-7,
        initial_radius: 1.,
        maximum_radius: 10.,
        minimum_radius: 1e-12,
        hessian_regularization: 1e-10,
        scaling_exponent: 0.25,
        stationarity_tolerance: 1e-6,
        equality_tolerance: 0.01,
        linear_tolerance: 1e-6,
    };
    for mode in [
        sim_solve::equality_dogleg::EqualityGlobalization::LagrangianDogleg,
        sim_solve::equality_dogleg::EqualityGlobalization::ExactPenaltyNewton,
    ] {
        search.globalization = mode;
        let r = planner
            .optimize_equalities(&q, &bounds, &search, |_| {})
            .unwrap();
        assert_eq!(
            r.search.termination,
            EqualityTermination::Converged,
            "{:?}",
            r.search
        );
        assert!(r.report.within_planning_tolerances);
        assert!(
            r.positions.iter().all(|q| q[0].abs() < 1e-8),
            "unactuated frictionless body cannot accelerate toward target"
        );
    }
}

#[test]
fn feasibility_restoration_ignores_task_cost_and_recovers_unactuated_dynamics() {
    let (art, markers) = fixture();
    let seed = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.; art.state_count],
        &vec![0.; art.port_names.len() + 1],
        vec![],
    );
    let z = -0.001 * (9.81_f64 / (2000. * 0.001)).exp_m1().ln();
    let q = vec![
        vec![0., 0., z, 0., 0., 0.],
        vec![0.001, 0., z + 0.0001, 0., 0., 0.],
        vec![0.002, 0., z + 0.0001, 0., 0., 0.],
    ];
    let mut c = config(q.clone());
    c.contact.friction_coefficient = 0.;
    c.contact_sliding_work_scale_j = Some(0.01);
    c.velocity_reference[0] = 0.1;
    let planner = ContactImplicitPlanner::new(&art, &seed, &markers, c.clone()).unwrap();
    let report = planner.evaluate(&q).unwrap();
    let physical = planner.feasibility_residuals(&report).unwrap();
    assert_eq!(physical.len(), 14); // one point plus six balance rows, two frames
    let inequalities = planner.physical_inequalities(&report).unwrap();
    assert_eq!(inequalities.len(), 26);
    for (f, row) in report.frames.iter().zip(inequalities.chunks_exact(13)) {
        assert_eq!(row[0] <= 0., -f.gaps_m[0] <= c.maximum_point_penetration_m);
        for j in 0..6 {
            let tolerance = if j < 3 {
                c.force_tolerance_n
            } else {
                c.moment_tolerance_nm
            };
            assert_eq!(
                row[1 + 2 * j] <= 0. && row[2 + 2 * j] <= 0.,
                f.unactuated_wrench[j].abs() <= tolerance
            );
        }
    }
    let mut edge = report.clone();
    for f in &mut edge.frames {
        f.gaps_m.fill(0.);
        f.unactuated_wrench.fill(0.);
    }
    for axis in 0..6 {
        let tolerance = if axis < 3 {
            c.force_tolerance_n
        } else {
            c.moment_tolerance_nm
        };
        for sign in [-1., 1.] {
            edge.frames[0].unactuated_wrench[axis] = sign * tolerance;
            assert!(
                planner
                    .physical_inequalities(&edge)
                    .unwrap()
                    .iter()
                    .all(|v| *v <= 0.)
            );
            edge.frames[0].unactuated_wrench[axis] = sign * tolerance * 1.001;
            assert!(
                planner
                    .physical_inequalities(&edge)
                    .unwrap()
                    .iter()
                    .any(|v| *v > 0.)
            );
            edge.frames[0].unactuated_wrench[axis] = 0.;
        }
    }
    edge.frames[0].gaps_m[0] = -c.maximum_point_penetration_m;
    assert!(
        planner
            .physical_inequalities(&edge)
            .unwrap()
            .iter()
            .all(|v| *v <= 0.)
    );
    edge.frames[0].gaps_m[0] -= 1e-6;
    assert!(
        planner
            .physical_inequalities(&edge)
            .unwrap()
            .iter()
            .any(|v| *v > 0.)
    );
    edge.frames[0].velocity.clear();
    assert!(planner.physical_inequalities(&edge).is_err());
    for (f, row) in report.frames.iter().zip(physical.chunks_exact(7)) {
        assert!(
            (row[0]
                - (-f.gaps_m[0] - c.maximum_point_penetration_m).max(0.) / c.penetration_scale_m)
                .abs()
                < 1e-12
        );
        for j in 0..6 {
            let scale = if j < 3 {
                c.force_tolerance_n
            } else {
                c.moment_tolerance_nm
            };
            assert!((row[j + 1] - f.unactuated_wrench[j] / scale).abs() < 1e-10);
        }
    }
    let mut changed = c.clone();
    changed.velocity_reference[0] = 10.;
    changed.position_reference[1][0] = 0.5;
    changed.contact_sliding_work_scale_j = None;
    let other = ContactImplicitPlanner::new(&art, &seed, &markers, changed).unwrap();
    let other_report = other.evaluate(&q).unwrap();
    assert_ne!(report.residuals, other_report.residuals);
    assert_eq!(
        physical,
        other.feasibility_residuals(&other_report).unwrap()
    );
    let mut invalid = report.clone();
    invalid.frames[0].time_s += 0.1;
    assert!(planner.feasibility_residuals(&invalid).is_err());
    let bounds = vec![
        vec![
            VariableBound {
                lower: -0.1,
                upper: 0.1
            };
            6
        ];
        2
    ];
    let search = LeastSquaresConfig {
        maximum_iterations: 60,
        maximum_evaluations: 5000,
        difference_step: 1e-7,
        initial_damping: 1.,
        gradient_tolerance: 1e-7,
    };
    let restored = planner
        .restore_feasibility(&q, &bounds, &search, 0.25, None, |_| {})
        .unwrap();
    assert!(
        restored.report.within_planning_tolerances,
        "{:?}",
        restored.search
    );
    assert!(restored.report.maximum_force_error_n < 1e-5);
    assert!(
        restored.positions.iter().all(|q| q[0].abs() < 1e-8),
        "frictionless unactuated body cannot accelerate toward a task reference"
    );
    assert_eq!(restored.positions[0], q[0]);
    assert_eq!(
        restored.search.residuals,
        planner.feasibility_residuals(&restored.report).unwrap()
    );
}

#[test]
fn slip_component_is_shared_with_typed_ports_and_required_units() {
    use sim_core::{PortSchema, QuantityKind};
    let registry = sim_runtime::registry();
    let d = registry
        .get(&sim_domain_control::contact_slip::CONTACT_SLIP.into())
        .unwrap();
    assert_eq!(d.ports[0].schema, PortSchema::SignalIn(QuantityKind::Time));
    assert_eq!(d.ports[1].schema, PortSchema::SignalIn(QuantityKind::Force));
    assert_eq!(
        d.ports[3].schema,
        PortSchema::SignalIn(QuantityKind::LinearVelocity)
    );
    assert_eq!(
        d.ports[5].schema,
        PortSchema::SignalOut(QuantityKind::Dimensionless)
    );
    assert_eq!(d.parameters.as_ref().unwrap().len(), 3);
    let factory = d.equations.unwrap();
    assert!(factory(&BTreeMap::new()).is_err());
    let mut parameters = BTreeMap::from([
        ("duration_s".into(), 1.),
        ("displacement_m".into(), 0.2),
        ("load_threshold_n".into(), 1.),
    ]);
    assert!(factory(&parameters).is_ok());
    parameters.insert("load_threshold_n".into(), 0.);
    assert!(factory(&parameters).is_err());
}

#[test]
fn periodic_slip_report_and_optional_objective_do_not_confuse_stationarity_with_acceptance() {
    use sim_runtime::contact_implicit::ContactSlipObjective;
    let (art, markers) = fixture();
    let seed = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.; art.state_count],
        &vec![0.; art.port_names.len() + 1],
        vec![],
    );
    let z = -0.001 * (9.81_f64 / (2000. * 0.001)).exp_m1().ln();
    let q = (0..=4)
        .map(|k| vec![0.02 * k as f64, 0., z, 0., 0., 0.])
        .collect::<Vec<_>>();
    let mut c = config(q.clone());
    c.contact.friction_coefficient = 0.;
    c.periodic_horizontal_translation = true;
    c.initial_velocity.clear();
    c.periodic_cubic_subdivisions = Some(2);
    let planner = ContactImplicitPlanner::new(&art, &seed, &markers, c).unwrap();
    let report = planner.evaluate(&q).unwrap();
    let objective = ContactSlipObjective {
        point_groups: vec!["foot".into()],
        load_threshold_n: 1.,
        target_ratio: 0.05,
        ratio_scale: 0.1,
        constrain_loaded_slip: false,
        loaded_slip_barrier_weight: None,
        use_continuous_mean_bound: false,
    };
    let slip = planner.slip_report(&report, &objective).unwrap();
    assert!((slip.groups[0].sampled_loaded_slip_ratio - 1.).abs() < 1e-12);
    assert!((slip.groups[0].rms_slip_upper_bound - 1.).abs() < 1e-12);
    assert!((slip.shaping_residuals[0] - 9.5).abs() < 1e-11);
    assert!(!slip.within_sampled_slip_limit);
    assert!(report.within_planning_tolerances);
    assert!(slip.groups[0].continuous_mean_slip_upper_bound.is_none());
    let mut mean_objective = objective.clone();
    mean_objective.use_continuous_mean_bound = true;
    let mean_report = planner.slip_report(&report, &mean_objective).unwrap();
    assert!((mean_report.groups[0].continuous_mean_slip_upper_bound.unwrap() - 1.).abs() < 1e-12);
    assert!((mean_report.shaping_residuals[0] - 9.5).abs() < 1e-11);
    mean_objective.loaded_slip_barrier_weight = Some(1e-4);
    assert!(planner.inequality_residuals(&report, &mean_objective).is_err());
    mean_objective.target_ratio = 1.1;
    assert!(planner.inequality_residuals(&report, &mean_objective).is_ok());
    let unconstrained_rows = planner.inequality_residuals(&report, &objective).unwrap();
    assert_eq!(
        unconstrained_rows.inequalities,
        planner.physical_inequalities(&report).unwrap()
    );
    let mut limited = objective.clone();
    limited.constrain_loaded_slip = true;
    let limited_rows = planner.inequality_residuals(&report, &limited).unwrap();
    assert_eq!(limited_rows.objective, unconstrained_rows.objective);
    assert_eq!(
        &limited_rows.inequalities[..unconstrained_rows.inequalities.len()],
        unconstrained_rows.inequalities
    );
    assert!((limited_rows.inequalities.last().unwrap() - 9.5).abs() < 1e-11);
    let mut boundary = limited.clone();
    let mut guarded = limited.clone();
    guarded.loaded_slip_barrier_weight = Some(0.0001);
    assert!(planner.inequality_residuals(&report, &guarded).is_err());
    guarded.target_ratio = slip.groups[0].sampled_loaded_slip_ratio;
    assert!(planner.inequality_residuals(&report, &guarded).is_err());
    guarded.target_ratio += 0.1;
    let guarded_rows = planner.inequality_residuals(&report, &guarded).unwrap();
    let row = (slip.groups[0].sampled_loaded_slip_ratio - guarded.target_ratio) / guarded.ratio_scale;
    assert_eq!(*guarded_rows.objective.last().unwrap(), 0.01 / -row);
    guarded.constrain_loaded_slip = false;
    assert_eq!(planner.inequality_residuals(&report, &guarded).unwrap().inequalities,
        unconstrained_rows.inequalities);
    boundary.target_ratio = slip.groups[0].sampled_loaded_slip_ratio;
    assert_eq!(
        *planner
            .inequality_residuals(&report, &boundary)
            .unwrap()
            .inequalities
            .last()
            .unwrap(),
        0.
    );
    boundary.target_ratio += 0.1;
    assert!(
        *planner
            .inequality_residuals(&report, &boundary)
            .unwrap()
            .inequalities
            .last()
            .unwrap()
            < 0.
    );
    let mut invalid = objective.clone();
    invalid.point_groups.clear();
    assert!(planner.slip_report(&report, &invalid).is_err());
    invalid = objective.clone();
    invalid.ratio_scale = 0.;
    assert!(planner.slip_report(&report, &invalid).is_err());
    let mut bounds = q[..4]
        .iter()
        .map(|q| {
            q.iter()
                .map(|&v| VariableBound { lower: v, upper: v })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    bounds.push(vec![
        VariableBound {
            lower: 0.08,
            upper: 0.08,
        },
        VariableBound {
            lower: 0.,
            upper: 0.,
        },
    ]);
    let search = LeastSquaresConfig {
        maximum_iterations: 2,
        maximum_evaluations: 100,
        difference_step: 1e-7,
        initial_damping: 1.,
        gradient_tolerance: 1e-7,
    };
    let result = planner
        .restore_feasibility_with_slip(&q, &bounds, &search, 0.25, None, Some(&objective), |_| {})
        .unwrap();
    assert!(planner.restore_feasibility_with_slip(
        &q, &bounds, &search, 0.25, None, Some(&guarded), |_| {}
    ).is_err());
    assert!(
        planner
            .restore_feasibility_with_slip(&q, &bounds, &search, 0.25, None, Some(&limited), |_| {})
            .is_err()
    );
    let constrained_config =
        sim_solve::inequality_augmented_lagrangian::AugmentedLagrangianConfig {
            maximum_outer_iterations: 2,
            maximum_evaluations: 100,
            initial_penalty: 1.,
            maximum_penalty: 100.,
            penalty_growth: 10.,
            required_reduction: 0.5,
            constraint_tolerance: 1e-6,
            complementarity_tolerance: 1e-6,
            scaling_exponent: 0.25,
            inner: search.clone(),
        };
    let constrained = planner
        .optimize_slip_with_inequalities(&q, &bounds, &constrained_config, &objective, |_| {})
        .unwrap();
    assert_eq!(constrained.positions, q);
    let physically_balanced_but_slipping = planner
        .optimize_slip_with_inequalities(&q, &bounds, &constrained_config, &limited, |_| {})
        .unwrap();
    assert!(
        physically_balanced_but_slipping
            .report
            .within_planning_tolerances
    );
    assert!(
        !physically_balanced_but_slipping
            .search
            .within_constraint_tolerance
    );
    assert_ne!(
        physically_balanced_but_slipping.search.termination,
        sim_solve::inequality_augmented_lagrangian::AugmentedTermination::StationaryWithinTolerance
    );
    let mut warm = sim_solve::inequality_augmented_lagrangian::AugmentedWarmStart {
        values: constrained.search.values.clone(),
        residuals: constrained.search.residuals.clone(),
        multipliers: constrained.search.multipliers.clone(),
        next_penalty: 1.,
        previous_shifted_norm: 0.,
        completed_outer_iterations: 1,
    };
    let resumed = planner
        .optimize_slip_with_inequalities_warm_started(
            &q,
            &bounds,
            &constrained_config,
            &objective,
            Some(&warm),
            |_| {},
        )
        .unwrap();
    assert_eq!(resumed.positions, q);
    assert_eq!(
        serde_json::to_value(&resumed.report).unwrap(),
        serde_json::to_value(&constrained.report).unwrap()
    );
    let mut changed_objective = objective.clone();
    changed_objective.ratio_scale *= 2.;
    assert!(
        planner
            .optimize_slip_with_inequalities_warm_started(
                &q,
                &bounds,
                &constrained_config,
                &changed_objective,
                Some(&warm),
                |_| {}
            )
            .is_err()
    );
    warm.values[0] += 0.001;
    assert!(
        planner
            .optimize_slip_with_inequalities_warm_started(
                &q,
                &bounds,
                &constrained_config,
                &objective,
                Some(&warm),
                |_| {}
            )
            .is_err()
    );
    assert!(constrained.search.within_constraint_tolerance);
    assert!(constrained.report.within_planning_tolerances);
    assert_eq!(
        constrained.search.residuals.inequalities,
        planner.physical_inequalities(&report).unwrap()
    );
    assert!(
        !planner
            .slip_report(&constrained.report, &objective)
            .unwrap()
            .within_sampled_slip_limit
    );
    assert_eq!(result.positions, q);
    assert_eq!(
        result.search.termination,
        sim_solve::least_squares::Termination::Stationary
    );
    let mut expected = planner.feasibility_residuals(&report).unwrap();
    expected.extend(slip.shaping_residuals);
    assert_eq!(result.search.residuals, expected);
    assert!(
        !planner
            .slip_report(&result.report, &objective)
            .unwrap()
            .within_sampled_slip_limit
    );
    let old = planner
        .restore_feasibility(&q, &bounds, &search, 0.25, None, |_| {})
        .unwrap();
    let explicit_none = planner
        .restore_feasibility_with_slip(&q, &bounds, &search, 0.25, None, None, |_| {})
        .unwrap();
    assert_eq!(
        serde_json::to_vec(&old).unwrap(),
        serde_json::to_vec(&explicit_none).unwrap()
    );
}
