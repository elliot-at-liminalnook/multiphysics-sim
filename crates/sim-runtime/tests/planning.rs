use sim_core::Behavior;
use sim_domain_control::trajectory::{Interpolation, Keyframe, Trajectory, TrajectoryConfig};
use sim_domain_robot::articulated::embedding::CoordinateInterval;
use sim_domain_robot::{Articulated, Generalized, Options};
use sim_runtime::{
    planning::*,
    session::Scene,
    tracking::{CaptureConfig, Marker},
};
use std::sync::Arc;

fn fixture() -> (Articulated, Generalized, CaptureConfig, MarkerMotionConfig) {
    fixture_with_bracket(false)
}

#[test]
fn extra_inspections_preserve_motor_commands_and_reconstruct_exact_geometry() {
    let (art, g, markers, config) = fixture();
    let original = plan_marker_motion(&art, &g, &markers, &config).unwrap();
    let t = config.sample_period_s * 0.3;
    let extra = plan_marker_motion_with_inspections(&art, &g, &markers, &config, &[t, t]).unwrap();
    assert_eq!(
        serde_json::to_value(&original.trajectory).unwrap(),
        serde_json::to_value(&extra.trajectory).unwrap()
    );
    assert_eq!(extra.frames.len(), original.frames.len() + 1);
    let f = extra.frames.iter().find(|f| f.time_s == t).unwrap();
    let q = Trajectory::new(original.trajectory)
        .unwrap()
        .sample(t)
        .unwrap()
        .values[0];
    assert!(
        (f.marker_positions_world_m[0][0] - original.frames[0].marker_positions_world_m[0][0] - q)
            .abs()
            < 1e-10
    );
    for t in [-0.1, f64::NAN, 100.0] {
        assert!(plan_marker_motion_with_inspections(&art, &g, &markers, &config, &[t]).is_err());
    }
}

#[test]
fn planner_checks_required_vertical_loads_over_the_declared_phase() {
    let (art, g, mut markers, mut config) = fixture();
    let link = markers.markers[0].link.clone();
    markers.markers = [[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]]
        .into_iter()
        .enumerate()
        .map(|(i, p)| Marker {
            id: format!("support-{i}"),
            link: link.clone(),
            local_point_m: p,
        })
        .collect();
    for knot in &mut config.displacements_world_m.keyframes {
        knot.values = knot.values.repeat(3);
    }
    let weight = art
        .links
        .iter()
        .filter(|l| !l.grounded)
        .map(|l| l.mass)
        .sum::<f64>()
        * (-art.model.gravity[2]);
    config.support_requirements = vec![MarkerSupportRequirement {
        start_s: 0.0,
        end_s: 0.2,
        marker_ids: std::array::from_fn(|i| format!("support-{i}")),
        minimum_forces_n: [weight * 0.2, weight * 0.2, weight * 0.4],
    }];
    let result = plan_marker_motion(&art, &g, &markers, &config).unwrap();
    assert!(result
        .frames
        .iter()
        .all(|f| f.support_load_checks.len() == 1));
    for f in result.frames {
        for (force, fraction) in f.support_load_checks[0]
            .predicted_vertical_forces_n
            .iter()
            .zip([0.25, 0.25, 0.5])
        {
            assert!((force - weight * fraction).abs() < 1e-8);
        }
    }
    let original = config.support_requirements[0].clone();
    config.support_requirements[0].start_s = 0.001;
    config.support_requirements[0].end_s = 0.002;
    assert!(plan_marker_motion(&art, &g, &markers, &config)
        .err()
        .unwrap()
        .contains("knot/midpoint"));
    config.support_requirements[0] = original;
    config.support_requirements[0].minimum_forces_n[2] = weight * 0.6;
    assert!(plan_marker_motion(&art, &g, &markers, &config)
        .err()
        .unwrap()
        .contains("static vertical support minimum failed"));
    config.support_requirements[0].marker_ids[2] = "missing".into();
    assert!(plan_marker_motion(&art, &g, &markers, &config).is_err());
}

fn fixture_with_bracket(
    bracket: bool,
) -> (Articulated, Generalized, CaptureConfig, MarkerMotionConfig) {
    let mut scene: Scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap();
    // Geometry-only synthetic slide derived from the small fixture. This test
    // does not use the rotary actuator dynamics as a linear actuator model.
    scene.robot.source =
        serde_json::json!({"cad_sha256":"synthetic-slide", "provenance":"analytic test fixture"});
    scene.robot.joints[0].kind = "prismatic".into();
    scene.robot.joints[0].axis = [1.0, 0.0, 0.0];
    if !bracket {
        // Remove the old rotary fixture's bracket geometry for the ideal
        // unobstructed slide test. The dedicated collision test retains it.
        scene
            .robot
            .links
            .iter_mut()
            .find(|l| l.name == "ground")
            .unwrap()
            .collision = Default::default();
    }
    let art = Articulated::new(
        Arc::new(scene.robot),
        &Options {
            contact: true,
            flex: false,
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
    let markers = CaptureConfig {
        experiment_id: "slide-test".into(),
        coordinate_frame: "synthetic-world-Z-up-m".into(),
        expected_cad_sha256: Some("synthetic-slide".into()),
        markers: vec![Marker {
            id: "tip".into(),
            link: "pendulum".into(),
            local_point_m: [0.01, 0.02, 0.03],
        }],
    };
    let config = MarkerMotionConfig {
        support_requirements: vec![],
        expected_cad_sha256: "synthetic-slide".into(),
        independent_coordinates: vec!["slide.pivot".into()],
        initial_coordinates: vec![0.0],
        initial_base_translation_m: None,
        base_displacements_world_m: None,
        bounds: vec![CoordinateInterval {
            lower: -0.1,
            upper: 0.1,
            max_step: 0.02,
        }],
        embedding: Default::default(),
        placement: Default::default(),
        sample_period_s: 0.05,
        maximum_interpolation_error_m: 1e-6,
        displacements_world_m: TrajectoryConfig {
            interpolation: Interpolation::Linear,
            keyframes: vec![
                Keyframe {
                    time_s: 0.0,
                    values: vec![0.0; 3],
                },
                Keyframe {
                    time_s: 0.1,
                    values: vec![0.01, 0.0, 0.0],
                },
                Keyframe {
                    time_s: 0.2,
                    values: vec![0.0; 3],
                },
            ],
        },
    };
    (art, g, markers, config)
}

#[test]
fn compiled_marker_motion_matches_analytic_slide_and_preserves_input() {
    let (art, g, markers, config) = fixture();
    let before = g.states.clone();
    let plan = plan_marker_motion(&art, &g, &markers, &config).unwrap();
    let reference = Trajectory::new(config.displacements_world_m.clone()).unwrap();
    assert_eq!(plan.frames.len(), 9);
    assert_eq!(plan.trajectory.keyframes.len(), 5);
    assert!(plan.maximum_marker_error_m < 1e-7);
    for f in &plan.frames {
        assert!(
            (f.joint_positions[0] - reference.sample(f.time_s).unwrap().values[0]).abs() < 1e-7
        );
    }
    assert_eq!(g.states, before);
    assert!(matches!(
        plan.trajectory.interpolation,
        Interpolation::Linear
    ));
}

#[test]
fn marker_compilation_rejects_unreachable_bounds_identity_and_missing_collision_checks() {
    let (mut art, g, markers, config) = fixture();
    for mutate in [
        |c: &mut MarkerMotionConfig| c.bounds[0].upper = 0.001,
        |c: &mut MarkerMotionConfig| c.displacements_world_m.keyframes[1].values[1] = 0.01,
        |c: &mut MarkerMotionConfig| c.expected_cad_sha256 = "wrong".into(),
        |c: &mut MarkerMotionConfig| c.independent_coordinates[0] = "unknown".into(),
        |c: &mut MarkerMotionConfig| c.sample_period_s = 0.03,
        |c: &mut MarkerMotionConfig| c.maximum_interpolation_error_m = f64::NAN,
    ] {
        let mut changed = config.clone();
        mutate(&mut changed);
        assert!(plan_marker_motion(&art, &g, &markers, &changed).is_err());
    }
    let mut wrong = markers.clone();
    wrong.markers.push(wrong.markers[0].clone());
    assert!(plan_marker_motion(&art, &g, &wrong, &config).is_err());
    art.contact_on = false;
    assert!(plan_marker_motion(&art, &g, &markers, &config)
        .err()
        .unwrap()
        .contains("collision inspection"));
}

#[test]
fn midpoint_audit_catches_insufficient_motor_command_sampling() {
    let (art, g, markers, mut config) = fixture();
    config.displacements_world_m.interpolation = Interpolation::QuinticRestToRest;
    config.displacements_world_m.keyframes.truncate(2);
    config.displacements_world_m.keyframes[1].time_s = 1.0;
    config.sample_period_s = 0.5;
    let error = plan_marker_motion(&art, &g, &markers, &config)
        .err()
        .unwrap();
    assert!(error.contains("interpolation allowance"), "{error}");
}

#[test]
fn midpoint_collision_is_rejected_even_when_command_knots_pass() {
    let (art, g, markers, config) = fixture_with_bracket(true);
    let error = plan_marker_motion(&art, &g, &markers, &config)
        .err()
        .unwrap();
    assert!(error.contains("internal contact at 0.025s"), "{error}");
    assert!(error.contains("pendulum / ground"), "{error}");
}

#[test]
fn floor_clearance_uses_rotated_surface_and_rejects_missing_geometry() {
    use sim_runtime::{contact_audit::sampled_floor_clearances, session::LinkPose};
    let (mut art, _, _, _) = fixture();
    art.floor_z = 0.0;
    let i = art.links.iter().position(|l| l.name == "pendulum").unwrap();
    art.links[i].contact = vec![
        nalgebra::Vector3::new(0.1, 0.0, 0.0),
        nalgebra::Vector3::new(-0.2, 0.0, 0.0),
    ];
    let pose = LinkPose {
        name: "pendulum".into(),
        position_m: [0.0, 0.0, 0.3],
        rotation: [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]],
    };
    let names = vec!["pendulum".into()];
    let result = sampled_floor_clearances(&art, &[pose.clone()], &names).unwrap();
    assert!((result[0].minimum_clearance_m - 0.2).abs() < 1e-14);
    assert_eq!(result[0].surface_samples, 2);
    assert!(sampled_floor_clearances(&art, &[], &names).is_err());
    assert!(sampled_floor_clearances(&art, &[pose.clone(), pose.clone()], &names).is_err());
    art.links[i].contact.clear();
    assert!(sampled_floor_clearances(&art, &[pose], &names).is_err());
}

#[test]
fn static_support_report_uses_moving_mass_and_explicit_supports() {
    use sim_runtime::{session::LinkPose, support::static_support_geometry};
    let (art, g, _, _) = fixture();
    let poses: Vec<_> = art
        .poses(&g)
        .iter()
        .zip(&art.links)
        .map(|((r, p), l)| LinkPose {
            name: l.name.clone(),
            position_m: (*p).into(),
            rotation: std::array::from_fn(|i| std::array::from_fn(|j| r[(i, j)])),
        })
        .collect();
    let markers: Vec<_> = [[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]]
        .into_iter()
        .enumerate()
        .map(|(i, p)| Marker {
            id: format!("assumed-{i}"),
            link: "pendulum".into(),
            local_point_m: p,
        })
        .collect();
    let result = static_support_geometry(&art, &poses, &markers).unwrap();
    let body = poses.iter().find(|p| p.name == "pendulum").unwrap();
    for i in 0..3 {
        assert!((result.center_of_mass_world_m[i] - body.position_m[i]).abs() < 1e-14);
    }
    assert!((result.projected_margin.minimum_edge_margin_m - 1.0 / 5.0_f64.sqrt()).abs() < 1e-14);
    assert!(static_support_geometry(&art, &poses[..1], &markers).is_err());
    assert!(static_support_geometry(&art, &poses, &markers[..2]).is_err());
}

#[test]
fn translated_base_reference_compensates_with_joint_motion_without_mutating_seed() {
    let (art, _, markers, mut config) = fixture();
    let mut model = (*art.model).clone();
    model
        .links
        .iter_mut()
        .find(|l| l.name == "ground")
        .unwrap()
        .ground = false;
    let art = Articulated::new(
        Arc::new(model),
        &Options {
            contact: true,
            flex: false,
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
    let before = g.states.clone();
    let mut base = config.displacements_world_m.clone();
    for k in &mut base.keyframes {
        k.values[0] *= 0.4;
    }
    config.base_displacements_world_m = Some(base);
    let plan = plan_marker_motion(&art, &g, &markers, &config).unwrap();
    let reference = Trajectory::new(config.displacements_world_m.clone()).unwrap();
    let initial_x = plan.frames[0]
        .poses
        .iter()
        .find(|p| p.name == "ground")
        .unwrap()
        .position_m[0];
    for f in &plan.frames {
        let desired = reference.sample(f.time_s).unwrap().values[0];
        assert!((f.joint_positions[0] - 0.6 * desired).abs() < 1e-7);
        let base = f.poses.iter().find(|p| p.name == "ground").unwrap();
        assert!((base.position_m[0] - initial_x - 0.4 * desired).abs() < 1e-14);
    }
    assert_eq!(before, g.states);
    config
        .base_displacements_world_m
        .as_mut()
        .unwrap()
        .keyframes[0]
        .values[0] = 0.1;
    assert!(plan_marker_motion(&art, &g, &markers, &config).is_err());
}
