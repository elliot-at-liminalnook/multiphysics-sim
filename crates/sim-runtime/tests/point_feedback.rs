use nalgebra::{DMatrix, DVector, Vector3};
use sim_core::Behavior;
use sim_domain_robot::{
    Articulated,
    articulated::embedding::{EmbeddedPoint, RigidEmbedding},
};
use sim_runtime::{
    body_feedback::point_correction,
    point_feedback::{PointFeedback, PointFeedbackConfig},
    session::Scene,
};
use std::sync::Arc;

#[test]
fn distinct_point_objectives_and_inactive_points_are_resolved_together() {
    let j = DMatrix::from_column_slice(3, 1, &[0.2, 0.0, 0.0]);
    let targets = [Vector3::new(0.01, 0.0, 0.0), Vector3::new(-0.02, 0.0, 0.0)];
    let q = point_correction(&[j.clone(), j.clone()], &[1.0, 1.0], &targets, 1e-6, 1.0).unwrap();
    assert!((q[0] + 0.025).abs() < 1e-10);
    let q = point_correction(&[j.clone(), j.clone()], &[1.0, 0.0], &targets, 1e-6, 1.0).unwrap();
    assert!((q[0] - 0.05).abs() < 1e-10);
    assert!(point_correction(&[j], &[1.0], &[], 1e-6, 1.0).is_err());
}

#[test]
fn loaded_floor_velocity_objective_respects_activation_and_existing_joint_cap() {
    use sim_domain_control::load_damping::LoadDampingConfig;
    let mut scene: Scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json")).unwrap();
    scene.robot.source = serde_json::json!({"cad_sha256":"floor-feedback-fixture"});
    let mut art = Articulated::new(Arc::new(scene.robot), &Default::default()).unwrap();
    let mut g = art.generalized(art.states().iter().map(|s|s.initial).collect(),
        vec![0.;art.state_count], &vec![0.;art.port_names.len()+1], vec![]);
    g.qd[0] = 0.1;
    let names = art.dofs().map(|(_,d)|d.name.clone()).collect::<Vec<_>>();
    let link = art.links.iter().position(|l|l.name=="pendulum").unwrap();
    art.floor_z = art.evaluate_kinematics_only(&g)[link].p.z + 0.1;
    let map = RigidEmbedding::new(&art,&names,Default::default()).unwrap();
    let point = EmbeddedPoint {link,local_point_m:[0.03,0.02,0.]};
    let (_, values) = map.point_jacobians(&g,&g.q,std::slice::from_ref(&point)).unwrap();
    let target: [f64;3] = values[0].0.into();
    let config: PointFeedbackConfig = serde_json::from_value(serde_json::json!({
        "expected_cad_sha256":"floor-feedback-fixture","coordinate_frame":"fixture-world-m",
        "markers":[{"id":"tip","link":"pendulum","local_point_m":point.local_point_m}],
        "position_world_m":{"keyframes":[{"time_s":0,"values":target}]},
        "activation":{"keyframes":[{"time_s":0,"values":[1]}]},
        "damping_m_per_rad":0.001,"maximum_correction_rad":0.002
    })).unwrap();
    assert!(serde_json::to_value(&config).unwrap().get("floor_velocity_damping").is_none());
    let original = PointFeedback::new(&art,config.clone()).unwrap().sample(&art,&map,&g,0.).unwrap();
    assert!(original.floor_velocity_damping.is_none());
    let mut enabled = config.clone();
    enabled.floor_velocity_damping = Some(LoadDampingConfig {velocity_damping_s:0.2,full_support_force_n:1.});
    let helper = PointFeedback::new(&art,enabled.clone()).unwrap();
    let on = helper.sample(&art,&map,&g,0.).unwrap();
    let d = on.floor_velocity_damping.as_ref().unwrap();
    assert!(d.normal_force_n[0] > 0.);
    let velocity = Vector3::from(d.contact_velocity_world_m_s[0]);
    let delta = Vector3::from(d.displacement_world_m[0]);
    assert!(velocity.norm() > 1e-5); assert!(velocity.dot(&delta) < 0.); assert_eq!(delta.z,0.);
    assert!(on.correction_rad[0].abs() <= 0.002);
    let suggested = &values[0].1 * DVector::from_vec(on.correction_rad.clone());
    assert!(suggested.dot(&velocity) < 0.);
    // Velocity-only control must not pull a translating robot toward an old
    // world path. It retains exactly the same load-weighted damping command.
    let mut velocity_only = enabled.clone();
    velocity_only.position_gain = 0.;
    let velocity_helper = PointFeedback::new(&art,velocity_only.clone()).unwrap();
    let shifted = [target[0]+0.5,target[1]-0.4,target[2]+0.3];
    let independent = velocity_helper.sample_target(&art,&map,&g,0.,&[shifted],&[1.]).unwrap();
    assert_eq!(independent.correction_rad,on.correction_rad);
    velocity_only.floor_velocity_damping = None;
    let disabled = PointFeedback::new(&art,velocity_only.clone()).unwrap()
        .sample_target(&art,&map,&g,0.,&[shifted],&[1.]).unwrap();
    assert_eq!(disabled.correction_rad,vec![0.]);
    velocity_only.position_gain = -0.1;
    assert!(PointFeedback::new(&art,velocity_only.clone()).is_err());
    velocity_only.position_gain = f64::NAN;
    assert!(PointFeedback::new(&art,velocity_only).is_err());
    let off = helper.sample_target(&art,&map,&g,0.,&[target],&[0.]).unwrap();
    assert_eq!(off.correction_rad,vec![0.]);
    enabled.floor_velocity_damping.as_mut().unwrap().velocity_damping_s=0.;
    let zero = PointFeedback::new(&art,enabled).unwrap().sample(&art,&map,&g,0.).unwrap();
    assert_eq!(zero.correction_rad, original.correction_rad);
    drop(map); art.floor_z=-10.;
    let map = RigidEmbedding::new(&art,&names,Default::default()).unwrap();
    let unloaded = helper.sample(&art,&map,&g,0.).unwrap();
    assert_eq!(unloaded.floor_velocity_damping.unwrap().normal_force_n,vec![0.]);
    assert_eq!(unloaded.correction_rad, original.correction_rad);
    drop(map); art.contact_on=false;
    let mut invalid = config;
    invalid.floor_velocity_damping=Some(LoadDampingConfig {velocity_damping_s:0.2,full_support_force_n:1.});
    assert!(PointFeedback::new(&art,invalid).is_err());
}

#[test]
fn actual_world_marker_error_drives_bounded_phase_activated_angular_suggestion() {
    let mut scene: Scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap();
    scene.robot.source = serde_json::json!({"cad_sha256":"synthetic-point-feedback"});
    let art = Articulated::new(Arc::new(scene.robot), &Default::default()).unwrap();
    let g = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.0; art.state_count],
        &vec![0.0; art.port_names.len() + 1],
        vec![],
    );
    let names = art.dofs().map(|(_, d)| d.name.clone()).collect::<Vec<_>>();
    let map = RigidEmbedding::new(&art, &names, Default::default()).unwrap();
    let point = EmbeddedPoint {
        link: art.links.iter().position(|l| l.name == "pendulum").unwrap(),
        local_point_m: [0.03, 0.02, 0.0],
    };
    let (_, values) = map.point_jacobians(&g, &g.q, std::slice::from_ref(&point)).unwrap();
    let target = values[0].0 + &values[0].1 * DVector::from_element(1, 0.01);
    let config: PointFeedbackConfig = serde_json::from_value(serde_json::json!({
        "expected_cad_sha256":"synthetic-point-feedback","coordinate_frame":"fixture-world-m",
        "markers":[{"id":"tip","link":"pendulum","local_point_m":[0.03,0.02,0.0]}],
        "position_world_m":{"keyframes":[{"time_s":0,"values":target.as_slice()}]},
        "activation":{"keyframes":[{"time_s":0,"values":[0]},{"time_s":1,"values":[1]}]},
        "damping_m_per_rad":0.001,"maximum_correction_rad":0.02
    }))
    .unwrap();
    let helper = PointFeedback::new(&art, config.clone()).unwrap();
    assert!(helper.sample_target(&art, &map, &g, -1., &[[0.;3]], &[1.]).is_err());
    let off = helper.sample(&art, &map, &g, 0.0).unwrap();
    assert_eq!(off.correction_rad, vec![0.0]);
    let on = helper.sample(&art, &map, &g, 1.0).unwrap();
    assert!((on.correction_rad[0] - 0.01).abs() < 1e-5);
    let halfway = helper.sample(&art, &map, &g, 0.5).unwrap();
    assert!((halfway.correction_rad[0] - 0.005).abs() < 1e-5);
    assert_eq!(
        on.actual_positions_world_m[0],
        off.actual_positions_world_m[0]
    );
    // A small suggested move decreases the actual nonlinear point error.
    let (_, shifted) = map
        .point_jacobians(&g, &[g.q[0] + on.correction_rad[0]], &[point])
        .unwrap();
    assert!((target - shifted[0].0).norm() < (target - values[0].0).norm() * 0.01);
    let mut capped = config.clone();
    capped.maximum_correction_rad = 0.003;
    assert!(
        (PointFeedback::new(&art, capped)
            .unwrap()
            .sample(&art, &map, &g, 1.0)
            .unwrap()
            .correction_rad[0]
            - 0.003)
            .abs()
            < 1e-14
    );
    for change in [0, 1, 2, 3] {
        let mut bad = config.clone();
        match change {
            0 => bad.expected_cad_sha256 = "other".into(),
            1 => bad.activation.keyframes[1].values[0] = 1.1,
            2 => bad.position_world_m.keyframes[0]
                .values
                .pop()
                .map(|_| ())
                .unwrap(),
            _ => bad.markers[0].link = "missing".into(),
        };
        assert!(PointFeedback::new(&art, bad).is_err());
    }
}

#[test]
fn point_feedback_commands_execute_through_the_shared_session_and_replay() {
    use sim_runtime::embedded::{CaptureMode, Config, EmbeddedSession};
    let mut scene: Scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap();
    let mut config: Config = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.policy.json"
    ))
    .unwrap();
    scene.robot.source = serde_json::json!({"cad_sha256":"point-policy-fixture"});
    config.policy.as_mut().unwrap().point_feedback = Some(
        serde_json::from_value(serde_json::json!({
            "expected_cad_sha256":"point-policy-fixture","coordinate_frame":"fixture-world-m",
            "markers":[{"id":"tip","link":"pendulum","local_point_m":[0.0,0.0,0.0]}],
            "position_world_m":{"keyframes":[{"time_s":0,"values":[0.05,0.0,0.15]}]},
            "activation":{"keyframes":[{"time_s":0,"values":[1]}]},
            "damping_m_per_rad":0.001,"maximum_correction_rad":0.02
        }))
        .unwrap(),
    );
    scene.controller.as_mut().unwrap().sources=serde_json::from_value(serde_json::json!({"entry":"point.rhai","files":{"point.rhai":"fn control(t,s,c,state){c[\"pivot.target\"]=s[\"command.position\"]+s[\"pivot.point_correction\"]; #{commands:c,state:state}}"}})).unwrap();
    config.steps = 16;
    config.report_every = 4;
    let mut run = EmbeddedSession::new(scene, config, 17, CaptureMode::Full).unwrap();
    run.advance(16).unwrap();
    let frame = run.frame().unwrap();
    let correction = frame["policy"]["observations"]["pivot.point_correction"]
        .as_f64()
        .unwrap();
    assert!(correction.abs() > 1e-6 && correction.abs() <= 0.02);
    assert!(
        (frame["policy"]["targets"]["pivot.target"].as_f64().unwrap()
            - frame["policy"]["observations"]["command.position"]
                .as_f64()
                .unwrap()
            - correction)
            .abs()
            < 1e-12
    );
    let (mut replay, n) =
        EmbeddedSession::prepare_replay(run.recording(), CaptureMode::Full).unwrap();
    replay.advance(n).unwrap();
    assert_eq!(frame["poses"], replay.frame().unwrap()["poses"]);
    assert_eq!(frame["policy"], replay.frame().unwrap()["policy"]);
}
