use nalgebra::{DMatrix, Rotation3, Vector3};
use sim_core::Behavior;
use sim_domain_robot::{Articulated, Options, articulated::embedding::RigidEmbedding};
use sim_runtime::body_feedback::{BodyFeedback, BodyFeedbackConfig, stance_correction};
use sim_runtime::session::Scene;
use std::sync::Arc;

#[test]
fn weighted_stance_solve_preserves_frames_and_bounds_without_loading_swing_joints() {
    let j = DMatrix::from_row_slice(3, 2, &[0.2, 0.0, 0.0, 0.4, 0.0, 0.0]);
    let delta = Vector3::new(-0.01, 0.02, 0.0);
    let q = stance_correction(&[j.clone()], &[1.0], delta, 1e-6, 1.0).unwrap();
    assert!((q[0] + 0.05).abs() < 1e-10 && (q[1] - 0.05).abs() < 1e-10);
    let r = Rotation3::from_euler_angles(0.4, -0.3, 1.2)
        .matrix()
        .clone_owned();
    let rotated = r * j.clone();
    let matrix = DMatrix::from_column_slice(3, 2, rotated.as_slice());
    let q2 = stance_correction(&[matrix], &[1.0], r * delta, 1e-6, 1.0).unwrap();
    assert!(q.iter().zip(q2).all(|(a, b)| (a - b).abs() < 1e-12));
    assert_eq!(
        stance_correction(&[j.clone()], &[0.0], delta, 1e-6, 1.0).unwrap(),
        vec![0.0; 2]
    );
    let limited = stance_correction(&[j.clone()], &[1.0], delta, 1e-6, 0.01).unwrap();
    assert!((limited.iter().map(|x| x.abs()).fold(0.0, f64::max) - 0.01).abs() < 1e-14);
    assert!((limited[0] / limited[1] - q[0] / q[1]).abs() < 1e-12);
    assert!(stance_correction(&[j.clone()], &[1.1], delta, 1e-6, 1.0).is_err());
    assert!(stance_correction(&[j], &[1.0], delta, 0.0, 1.0).is_err());
}

#[test]
fn body_feedback_uses_reference_phase_and_current_floor_support_without_mutating_physics() {
    let mut scene: Scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap();
    scene.robot.source = serde_json::json!({"cad_sha256":"synthetic-body-feedback"});
    for link in &mut scene.robot.links {
        link.ground = false;
    }
    scene.robot.world.floor_z = 0.3; // Analytic inspection fixture, never integrated.
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
    let names = art.dofs().map(|(_, d)| d.name.clone()).collect::<Vec<_>>();
    let map = RigidEmbedding::new(&art, &names, Default::default()).unwrap();
    let body = art.evaluate_kinematics_only(&g)[art.bases[0].link].p;
    let config:BodyFeedbackConfig=serde_json::from_value(serde_json::json!({
        "expected_cad_sha256":"synthetic-body-feedback","coordinate_frame":"fixture-world-m",
        "reference_link":"ground","support_markers":[{"id":"foot","link":"pendulum","local_point_m":[0,0,0]}],
        "position_world_m":{"keyframes":[{"time_s":0,"values":[body.x,body.y,body.z]}, {"time_s":1,"values":[body.x+0.01,body.y,body.z]}]},
        "velocity_damping_s":0.1,"damping_m_per_rad":0.001,"maximum_correction_rad":1.0,"full_support_force_n":1.0
    })).unwrap();
    let helper = BodyFeedback::new(&art, config.clone()).unwrap();
    assert!(helper.sample_target(&art, &map, &g, f64::NAN, [0.;3], [0.;3]).is_err());
    let paused = helper.sample(&art, &map, &g, 0.5, false).unwrap();
    assert!((paused.position_error_world_m[0] - 0.005).abs() < 1e-14);
    assert_eq!(paused.support_weights, vec![1.0]);
    let moving = helper.sample(&art, &map, &g, 0.5, true).unwrap();
    assert_ne!(paused.correction_rad, moving.correction_rad); // Pauses zero reference velocity.
    let (_, js) = map
        .point_jacobians(
            &g,
            &g.q,
            &[sim_domain_robot::articulated::embedding::EmbeddedPoint {
                link: art.links.iter().position(|l| l.name == "pendulum").unwrap(),
                local_point_m: [0.0; 3],
            }],
        )
        .unwrap();
    let foot_shift = &js[0].1 * nalgebra::DVector::from_vec(paused.correction_rad);
    assert!(
        foot_shift[0] < 0.0,
        "stance foot correction must oppose requested body shift"
    );
    assert_eq!(
        g.states,
        art.states().iter().map(|s| s.initial).collect::<Vec<_>>()
    );
    let mut yaw_config = config.clone();
    yaw_config.yaw_feedback = Some(serde_json::from_value(serde_json::json!({
        "controller":{"position_gain":1.0,"velocity_damping_s":0.1,"maximum_correction_rad":0.02},
        "yaw_rad":{"keyframes":[{"time_s":0,"values":[0.0]},{"time_s":1,"values":[0.1]}]}
    })).unwrap());
    let yaw_helper = BodyFeedback::new(&art, yaw_config).unwrap();
    let paused = yaw_helper.sample(&art, &map, &g, 0.5, false).unwrap();
    let moving = yaw_helper.sample(&art, &map, &g, 0.5, true).unwrap();
    assert_eq!(paused.yaw_feedback.unwrap().target_yaw_rate_rad_s, 0.);
    assert!((moving.yaw_feedback.unwrap().target_yaw_rate_rad_s - 0.1).abs() < 1e-14);
    let planned = yaw_helper.sample_pose_target(&art, &map, &g, 0.5, body.into(), [0.;3], [0.25, 0.03]).unwrap();
    let yaw = planned.yaw_feedback.unwrap();
    assert_eq!(yaw.target_yaw_rad, 0.25);
    assert_eq!(yaw.target_yaw_rate_rad_s, 0.03);
    assert!(yaw.correction_rad > 0. && yaw.correction_rad <= 0.02);
    let foot = art.evaluate_kinematics_only(&g)[art.links.iter().position(|l| l.name == "pendulum").unwrap()].p;
    let displacement = Vector3::from(yaw.stance_displacements_world_m[0]);
    let tangent = Vector3::z().cross(&(foot - body));
    assert!(displacement.dot(&tangent) < 0., "stance suggestion must oppose requested body yaw");
    assert_eq!(g.states, art.states().iter().map(|s| s.initial).collect::<Vec<_>>());
    let mut bad = config.clone();
    bad.expected_cad_sha256 = "different".into();
    assert!(BodyFeedback::new(&art, bad).is_err());
    let mut bad = config;
    bad.support_markers.push(bad.support_markers[0].clone());
    assert!(BodyFeedback::new(&art, bad).is_err());
}

#[test]
fn airborne_feedback_reaches_rhai_and_replays_without_inventing_support() {
    use sim_runtime::embedded::{CaptureMode, Config, EmbeddedSession};
    let mut scene: Scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap();
    let mut config: Config = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.policy.json"
    ))
    .unwrap();
    scene.robot.source = serde_json::json!({"cad_sha256":"synthetic-airborne-feedback"});
    scene.options.contact = true;
    scene.period_s = 0.001;
    scene.robot.world.floor_z = -1.0;
    for link in &mut scene.robot.links {
        link.ground = false;
    }
    let p = scene
        .robot
        .links
        .iter()
        .find(|l| l.name == "ground")
        .unwrap()
        .com;
    let feedback:BodyFeedbackConfig=serde_json::from_value(serde_json::json!({
        "expected_cad_sha256":"synthetic-airborne-feedback","coordinate_frame":"fixture-world-m",
        "reference_link":"ground","support_markers":[{"id":"foot","link":"pendulum","local_point_m":[0,0,0]}],
        "position_world_m":{"keyframes":[{"time_s":0,"values":[p[0]+0.01,p[1],p[2]]}]},
        "velocity_damping_s":0.0,"damping_m_per_rad":0.001,"maximum_correction_rad":0.02,"full_support_force_n":1.0
    })).unwrap();
    config.policy.as_mut().unwrap().body_feedback = Some(feedback);
    config.applied_generalized_loads = vec![0.0; 7];
    config.steps = 16;
    config.report_every = 4;
    let program = scene.controller.as_mut().unwrap();
    program.sources=serde_json::from_value(serde_json::json!({"entry":"airborne.rhai","files":{"airborne.rhai":"fn control(t,s,c,state){c[\"pivot.target\"]=s[\"command.position\"]+s[\"pivot.body_correction\"]; #{commands:c,state:state}}"}})).unwrap();
    let mut run = EmbeddedSession::new(scene, config, 13, CaptureMode::Full).unwrap();
    run.advance(16).unwrap();
    let frame = run.frame().unwrap();
    assert!(frame["error"].is_null());
    assert_eq!(
        frame["policy"]["observations"]["pivot.body_correction"],
        0.0
    );
    assert_eq!(
        frame["policy"]["body_feedback"]["support_weights"],
        serde_json::json!([0.0])
    );
    assert!(
        frame["policy"]["body_feedback"]["position_error_world_m"][0]
            .as_f64()
            .unwrap()
            > 0.009
    );
    let (mut replay, n) =
        EmbeddedSession::prepare_replay(run.recording(), CaptureMode::Full).unwrap();
    replay.advance(n).unwrap();
    assert_eq!(frame["poses"], replay.frame().unwrap()["poses"]);
    assert_eq!(frame["policy"], replay.frame().unwrap()["policy"]);
}
