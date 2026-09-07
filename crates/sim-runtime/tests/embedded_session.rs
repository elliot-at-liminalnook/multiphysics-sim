use sim_runtime::{
    embedded::{CaptureMode, Config, EmbeddedSession},
    session::Scene,
};

fn fixture() -> (Scene, Config) {
    (
        serde_json::from_str(include_str!(
            "../../../examples/interactive/pendulum.scene.json"
        ))
        .unwrap(),
        serde_json::from_str(include_str!(
            "../../../examples/interactive/pendulum.embedded.json"
        ))
        .unwrap(),
    )
}

#[test]
fn mechanical_subdivision_is_explicit_preserves_direct_accuracy_and_replays() {
    let (scene, mut config) = fixture();
    config.mechanical_subdivision = Some(Default::default());
    assert!(EmbeddedSession::new(scene.clone(), config.clone(), 0, CaptureMode::Latest)
        .err().unwrap().contains("coupled motor events"));
    config.motors = None;
    config.audit_contact_steps = false;
    config.applied_generalized_loads = vec![0.01];
    let mut direct_config = config.clone();
    direct_config.mechanical_subdivision = None;
    let mut direct = EmbeddedSession::new(scene.clone(), direct_config, 0, CaptureMode::Full).unwrap();
    let mut refined = EmbeddedSession::new(scene, config, 0, CaptureMode::Full).unwrap();
    direct.advance(80).unwrap();refined.advance(80).unwrap();
    // The hybrid scheduler computes h=(t+duration)-t. Even without retries,
    // this can differ from duration by clock roundoff. Compare every physical
    // field within roundoff, while requiring exact replay of the same path below.
    fn compare(a: &serde_json::Value, b: &serde_json::Value) {
        use serde_json::Value;
        match (a, b) {
            (Value::Number(a), Value::Number(b)) => {
                let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
                assert!((a-b).abs() <= 1e-12 * (1.0+a.abs().max(b.abs())), "{a} != {b}");
            }
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len());
                for (a, b) in a.iter().zip(b) { compare(a,b); }
            }
            (Value::Object(a), Value::Object(b)) => {
                assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
                for (key,a) in a { compare(a,&b[key]); }
            }
            _ => assert_eq!(a,b),
        }
    }
    compare(&direct.frame().unwrap(), &refined.frame().unwrap());
    assert_eq!(refined.implicit_step_diagnostics().len(),80);
    assert_eq!(refined.interval_diagnostics().len(),80);
    assert!(refined.interval_diagnostics().iter().all(|d|d.continuous_attempts==1&&d.rejected_trials==0&&d.events.is_empty()));
    let (mut replay, steps)=EmbeddedSession::prepare_replay(refined.recording(),CaptureMode::Latest).unwrap();
    replay.retain_solver_diagnostics(true);
    replay.advance(steps).unwrap();
    assert_eq!(refined.frame().unwrap(),replay.frame().unwrap());
    assert_eq!(replay.implicit_step_diagnostics().len(),80);
    assert_eq!(replay.interval_diagnostics().len(),80);
    replay.retain_solver_diagnostics(false);
    assert!(replay.implicit_step_diagnostics().is_empty());
    assert!(replay.interval_diagnostics().is_empty());
}

#[test]
fn condensed_motor_session_records_configuration_and_replays_across_chunks() {
    for (colored, endpoint_scale) in [(false,false),(true,false),(false,true),(true,true)] {
    let (scene, mut config) = fixture();
    let implicit = config.implicit.get_or_insert_with(Default::default);
    implicit.condense_auxiliary = true;
    implicit.color_auxiliary_jacobian = colored;
    implicit.auxiliary_rate_unknowns = true;
    implicit.auxiliary_endpoint_correction_scale = endpoint_scale;
    let mut session = EmbeddedSession::new(scene, config, 9, CaptureMode::Full).unwrap();
    session.advance(80).unwrap();
    assert_eq!(session.report().unwrap()["implicit"]["condense_auxiliary"], true);
    assert_eq!(session.report().unwrap()["implicit"]["color_auxiliary_jacobian"].as_bool().unwrap_or(false),colored);
    assert_eq!(session.report().unwrap()["implicit"]["auxiliary_endpoint_correction_scale"].as_bool().unwrap_or(false),endpoint_scale);
    let (mut replay, steps) = EmbeddedSession::prepare_replay(session.recording(), CaptureMode::Latest).unwrap();
    replay.advance(7).unwrap();
    replay.advance(steps-7).unwrap();
    assert_eq!(session.frame().unwrap(), replay.frame().unwrap());
}
}

#[test]
fn motor_reductions_are_explicit_shared_parameters_and_replay_without_cad_mutation() {
    use sim_domain_robot::motor::MotorDynamics::*;
    for mode in [QuasistaticWinding,QuasistaticRotor,Quasistatic] {
        let (mut scene,config)=fixture();
        let physical_source=serde_json::to_value(&scene.robot).unwrap();
        scene.options.motor_dynamics=mode;
        let mut session=EmbeddedSession::new(scene,config,9,CaptureMode::Full).unwrap();
        session.advance(80).unwrap();
        let report=session.report().unwrap();
        assert_eq!(report["scene_options"]["motor_dynamics"],serde_json::to_value(mode).unwrap());
        for (name,value) in mode.parameter_flags() {
            assert_eq!(report["motor_components"][0]["parameters"][name],value);
        }
        let recording=session.recording();
        assert_eq!(serde_json::to_value(&recording.scene.robot).unwrap(),physical_source);
        let (mut replay,steps)=EmbeddedSession::prepare_replay(recording,CaptureMode::Latest).unwrap();
        replay.advance(steps).unwrap();
        assert_eq!(session.frame().unwrap(),replay.frame().unwrap());
    }
}

#[test]
fn chunking_preserves_motor_firmware_and_full_diagnostics() {
    let (scene, config) = fixture();
    let mut uninterrupted =
        EmbeddedSession::new(scene.clone(), config.clone(), 7, CaptureMode::Full).unwrap();
    let mut chunked =
        EmbeddedSession::new(scene.clone(), config.clone(), 7, CaptureMode::Full).unwrap();
    let mut latest = EmbeddedSession::new(scene, config, 7, CaptureMode::Latest).unwrap();
    uninterrupted.advance(80).unwrap();
    for n in [1, 7, 11, 3, 29, 29] {
        chunked.advance(n).unwrap();
        latest.advance(n).unwrap();
        // Drawing and inspecting between advances must not mutate physics.
        assert_eq!(chunked.frame().unwrap(), latest.frame().unwrap());
        assert_eq!(chunked.frame().unwrap(), chunked.frame().unwrap());
    }
    assert!(chunked.done());
    assert_eq!(chunked.frame().unwrap(), uninterrupted.frame().unwrap());
    let mut a = uninterrupted.report().unwrap();
    let mut b = chunked.report().unwrap();
    let metadata=latest.diagnostic_metadata();
    assert_eq!(metadata,uninterrupted.diagnostic_metadata());
    for (key,value) in metadata.as_object().unwrap() { assert_eq!(&a[key],value,"{key}"); }
    a.as_object_mut().unwrap().remove("stepping_wall_s");
    b.as_object_mut().unwrap().remove("stepping_wall_s");
    assert_eq!(a, b);
    assert!(latest.report().is_err());
    let before = chunked.frame().unwrap();
    chunked.advance(1).unwrap();
    assert_eq!(before, chunked.frame().unwrap());
}

#[test]
fn invalid_advance_does_not_mutate_and_partial_report_is_not_complete() {
    let (scene, config) = fixture();
    let mut session = EmbeddedSession::new(scene, config, 0, CaptureMode::Full).unwrap();
    let before = session.frame().unwrap();
    assert!(session.advance(0).is_err());
    assert_eq!(before, session.frame().unwrap());
    session.advance(1).unwrap();
    assert_eq!(session.report().unwrap()["completed"], false);
    assert_eq!(session.completed_steps(), 1);
    assert_eq!(session.remaining_steps(), 79);
}

#[test]
fn recorded_prefix_replays_and_rejects_invalid_horizons() {
    let (scene, config) = fixture();
    let mut session = EmbeddedSession::new(scene, config, 17, CaptureMode::Latest).unwrap();
    session.advance(43).unwrap();
    let recording = session.recording();
    let encoded = serde_json::to_string(&recording).unwrap();
    let (mut replay, steps) = EmbeddedSession::prepare_replay(
        serde_json::from_str(&encoded).unwrap(),
        CaptureMode::Latest,
    )
    .unwrap();
    assert_eq!(steps, 43);
    replay.advance(19).unwrap();
    replay.advance(24).unwrap();
    assert_eq!(session.frame().unwrap(), replay.frame().unwrap());
    // Earlier recordings have no failure field and retain prefix semantics.
    for version in [1, 2] {
        let mut legacy = recording.clone();
        legacy.version = version;
        let (mut old, steps) =
            EmbeddedSession::prepare_replay(legacy, CaptureMode::Latest).unwrap();
        old.advance(steps).unwrap();
        assert_eq!(session.frame().unwrap(), old.frame().unwrap());
    }
    let mut forged = recording.clone();
    forged.failure = Some("invented failure".into());
    let (mut bad_replay, steps) =
        EmbeddedSession::prepare_replay(forged, CaptureMode::Latest).unwrap();
    assert!(bad_replay
        .advance(steps)
        .unwrap_err()
        .contains("recorded failure did not reproduce"));
    let mut bad = recording;
    bad.completed_steps = 81;
    assert!(EmbeddedSession::prepare_replay(bad, CaptureMode::Latest).is_err());
}

#[test]
fn feedback_timeout_is_identical_across_chunks_and_latches() {
    use serde_json::json;
    let (mut scene, config) = fixture();
    scene.robot.source["cad_sha256"] = json!("explicit-test-source");
    let mut value = serde_json::to_value(config).unwrap();
    value["motors"]["expected_cad_sha256"] = json!("explicit-test-source");
    value["motors"]["target_coordinates"] = json!(["joint.pivot"]);
    value["motors"]["target_trajectory"] = json!({"interpolation":"linear","keyframes":[{"time_s":0.0,"values":[0.2]},{"time_s":0.02,"values":[0.2]}]});
    value["motion_gate"] = json!({"clock":{"period_s":0.002,"duration_s":0.02,"guard_start_s":0.004,"guard_end_s":0.016,"qualification_s":0.002,"maximum_pause_s":0.004},"support_links":["pendulum"],"minimum_upward_force_n":1.0,"observation_source":"ideal_runtime_floor_force"});
    let config: Config = serde_json::from_value(value).unwrap();
    let mut a = EmbeddedSession::new(scene.clone(), config.clone(), 0, CaptureMode::Full).unwrap();
    let mut b = EmbeddedSession::new(scene, config, 0, CaptureMode::Full).unwrap();
    assert!(a.advance(80).unwrap_err().contains("timed out"));
    while !b.done() {
        let _ = b.advance(3);
    }
    assert_eq!(a.error(), b.error());
    assert_eq!(a.frame().unwrap(), b.frame().unwrap());
    assert!(a.completed_steps() > 0 && a.remaining_steps() > 0);
    let before = a.interactive_frame().unwrap();
    assert!(a.advance(1).is_err());
    assert_eq!(before, a.interactive_frame().unwrap());
    let (mut replay, n) =
        EmbeddedSession::prepare_replay(a.recording(), CaptureMode::Latest).unwrap();
    assert!(
        replay.advance(n).is_err(),
        "replay must reproduce the failed attempt, not just the successful prefix"
    );
    assert_eq!(a.error(), replay.error());
    assert_eq!(a.frame().unwrap(), replay.frame().unwrap());
    let mut mismatched = a.recording();
    mismatched.failure = Some("different failure".into());
    let (mut replay, n) = EmbeddedSession::prepare_replay(mismatched, CaptureMode::Latest).unwrap();
    assert!(replay
        .advance(n)
        .unwrap_err()
        .contains("replay failure mismatch"));

    // A failure produced by the end-of-horizon check needs no extra attempt.
    let recipe = a.recording();
    let mut short = recipe.config;
    short.steps = 24; // 6ms: guard is waiting, but its timeout has not elapsed.
    let mut horizon = EmbeddedSession::new(recipe.scene, short, 0, CaptureMode::Latest).unwrap();
    assert!(horizon.advance(24).unwrap_err().contains("horizon ended"));
    let (mut replay, n) =
        EmbeddedSession::prepare_replay(horizon.recording(), CaptureMode::Latest).unwrap();
    assert_eq!(n, 24);
    assert_eq!(replay.advance(n).unwrap_err(), horizon.error().unwrap());
    assert_eq!(replay.frame().unwrap(), horizon.frame().unwrap());
}

#[test]
fn sampled_rhai_inputs_are_bounded_scheduled_and_replayed() {
    let (scene, _) = fixture();
    let mut config: Config = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.policy.json"
    ))
    .unwrap();
    config.steps = 240;
    let mut run = EmbeddedSession::new(scene, config, 23, CaptureMode::Latest).unwrap();
    assert_eq!(run.inputs()[0].name, "command.position");
    let before = run.frame().unwrap();
    assert!(run.set_inputs(&[2.0]).is_err());
    assert_eq!(before, run.frame().unwrap());
    run.advance(43).unwrap();
    run.set_inputs(&[0.6]).unwrap();
    run.advance(37).unwrap();
    assert_eq!(
        run.frame().unwrap()["policy"]["targets"]["pivot.target"],
        0.2
    );
    run.advance(1).unwrap();
    assert_eq!(
        run.frame().unwrap()["policy"]["targets"]["pivot.target"],
        0.6
    );
    run.advance(39).unwrap();
    run.set_inputs(&[-0.1]).unwrap();
    run.advance(120).unwrap();
    let recording = run.recording();
    assert_eq!(recording.input_events.len(), 2);
    let (mut replay, steps) =
        EmbeddedSession::prepare_replay(recording, CaptureMode::Latest).unwrap();
    assert_eq!(steps, 240);
    for _ in 0..24 {
        replay.advance(10).unwrap();
    }
    assert_eq!(run.frame().unwrap(), replay.frame().unwrap());
    assert_eq!(
        run.interactive_frame().unwrap()["policy_inputs"],
        replay.interactive_frame().unwrap()["policy_inputs"]
    );
    assert_eq!(run.policy_metadata()["deployable"], false);
}

#[test]
fn rhai_observes_actual_joint_state_and_rejects_out_of_bounds_outputs() {
    use serde_json::json;
    let (mut scene, _) = fixture();
    let config: Config = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.policy.json"
    ))
    .unwrap();
    scene.controller.as_mut().unwrap().sources.files.insert("controller.rhai".into(),"fn control(t,s,c,state){ c[\"pivot.target\"] = s[\"pivot.angle\"] + 0.1; #{commands:c,state:state} }".into());
    let mut run =
        EmbeddedSession::new(scene.clone(), config.clone(), 0, CaptureMode::Latest).unwrap();
    run.advance(1).unwrap();
    assert_eq!(
        run.frame().unwrap()["policy"]["targets"]["pivot.target"],
        json!(0.1)
    );
    scene.controller.as_mut().unwrap().sources.files.insert(
        "controller.rhai".into(),
        "fn control(t,s,c,state){ c[\"pivot.target\"] = 2.0; #{commands:c,state:state} }".into(),
    );
    let mut invalid = EmbeddedSession::new(scene, config, 0, CaptureMode::Latest).unwrap();
    let error = invalid.advance(1).unwrap_err();
    assert!(error.contains("software/CAD command bounds at 0 s: pivot.target requested 2 rad; allowed ["), "{error}");
    assert_eq!(invalid.completed_steps(), 0);
    assert!(invalid.done());
    let (mut replay, steps) =
        EmbeddedSession::prepare_replay(invalid.recording(), CaptureMode::Latest).unwrap();
    assert_eq!(steps, 1);
    assert_eq!(replay.advance(steps).unwrap_err(), invalid.error().unwrap());
    assert_eq!(replay.frame().unwrap(), invalid.frame().unwrap());
}

#[test]
fn task_observations_reach_rhai_and_replay_with_input_changes() {
    let (mut scene, _) = fixture();
    scene.robot.source["cad_sha256"] = serde_json::json!("synthetic-observation-test");
    let mut config: serde_json::Value = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.policy.json"
    ))
    .unwrap();
    config["policy"]["task_observations"] = serde_json::json!({"observation_source":"ideal_rigid_body_diagnostics","expected_cad_sha256":"synthetic-observation-test","reference_link":"ground","markers":[{"id":"tip","link":"pendulum","local_point_m":[0.0,0.0,0.0]}],"floor_forces":false});
    config["steps"] = serde_json::json!(160);
    scene.controller.as_mut().unwrap().sources.files.insert("controller.rhai".into(),"fn control(t,s,c,state){ c[\"pivot.target\"]=s[\"command.position\"]+s[\"marker.tip.position.z\"]; #{commands:c,state:state} }".into());
    let mut run = EmbeddedSession::new(
        scene,
        serde_json::from_value(config).unwrap(),
        0,
        CaptureMode::Latest,
    )
    .unwrap();
    run.advance(41).unwrap();
    run.set_inputs(&[0.3]).unwrap();
    run.advance(119).unwrap();
    let f = run.frame().unwrap();
    let o = &f["policy"]["observations"];
    assert_eq!(
        f["policy"]["targets"]["pivot.target"].as_f64().unwrap(),
        0.3 + o["marker.tip.position.z"].as_f64().unwrap()
    );
    assert_eq!(o["body.gravity_direction.z"], -1.0);
    let (mut replay, n) =
        EmbeddedSession::prepare_replay(run.recording(), CaptureMode::Latest).unwrap();
    for _ in 0..n {
        replay.advance(1).unwrap();
    }
    assert_eq!(run.frame().unwrap(), replay.frame().unwrap());
    assert_eq!(run.policy_metadata()["deployable"], false);
}
