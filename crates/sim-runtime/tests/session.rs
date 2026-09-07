use sim_runtime::session::{Scene, Session};
fn scene() -> Scene {
    serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap()
}
#[test]
fn experimental_solver_changes_require_explicit_scene_options() {
    let options = scene().options;
    assert!(!options.hybrid_articulated_jacobian);
    assert!(!options.articulated_rate_partials);
    assert!(!options.structural_loop_identities);
    assert!(!options.analytic_motor_jacobian);
    assert!(!options.backlash_events);
    assert!(!options.event_jacobian_reuse);
    assert!(!options.guarded_backtracking);
    assert!(options.floor_friction.is_bristle());
    assert!(options.floor_dissipation_s_m.is_none());
    assert!(serde_json::to_value(&options).unwrap().get("floor_dissipation_s_m").is_none());
    assert!(serde_json::to_value(&options).unwrap().get("floor_friction").is_none());
}

#[test]
fn floor_dissipation_override_is_recorded_and_replayed() {
    let mut spec = scene();
    spec.options.contact = true;
    spec.options.step = 0.000125;
    spec.robot.world.floor_z += 1e-6;
    spec.options.floor_dissipation_s_m = Some(100.0);
    let mut session = Session::new(spec.clone(), 0).unwrap();
    assert_eq!(session.robot.art.floor_dissipation_s_m, 100.0);
    for _ in 0..4 { session.step(&[0.2]).unwrap(); }
    let recording = session.recording();
    assert_eq!(recording.scene.options.floor_dissipation_s_m, Some(100.0));
    let replay = Session::replay(recording).unwrap();
    assert_eq!(serde_json::to_value(session.frame()).unwrap(), serde_json::to_value(replay.frame()).unwrap());
    spec.options.floor_dissipation_s_m = Some(-1.0);
    assert!(Session::new(spec, 0).is_err());
}

#[test]
fn regularized_floor_friction_is_explicit_replayed_and_numerically_checked() {
    use sim_domain_robot::articulated::friction::FloorFrictionModel;
    use sim_runtime::validation::{compare_recording, CompareConfig};
    let mut spec=scene();
    spec.options.contact=true;
    spec.options.step=0.000125;
    spec.robot.world.floor_z+=1e-6;
    spec.options.floor_friction=FloorFrictionModel::RegularizedCoulomb { slip_speed_m_s:0.001 };
    let mut session=Session::new(spec.clone(),0).unwrap();
    assert!(!session.robot.art.floor_friction.is_bristle());
    for _ in 0..4 { session.step(&[0.2]).unwrap(); }
    let recording=session.recording();
    let encoded=serde_json::to_value(&recording).unwrap();
    assert_eq!(encoded["scene"]["options"]["floor_friction"]["slip_speed_m_s"],serde_json::json!(0.001));
    let replay=Session::replay(serde_json::from_value(encoded).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(session.frame()).unwrap(),serde_json::to_value(replay.frame()).unwrap());
    let report=compare_recording(&recording,&CompareConfig::default()).unwrap();
    assert!(report.passed,"{}",serde_json::to_string_pretty(&report).unwrap());
    assert!(!report.reference_options.floor_friction.is_bristle());
    spec.options.floor_friction=FloorFrictionModel::RegularizedCoulomb { slip_speed_m_s:0.0 };
    assert!(Session::new(spec,0).is_err());
}
#[test]
fn guarded_backtracking_is_recorded_and_applied_to_the_runtime() {
    let mut s = scene();
    s.options.guarded_backtracking = true;
    let mut session = Session::new(s, 19).unwrap();
    for island in &session.robot.runtime.islands {
        match island.integrator {
            sim_dynamics::Integrator::BackwardEuler(c) => assert!(c.guarded_backtracking),
            _ => panic!("unexpected robot integrator"),
        }
    }
    for _ in 0..5 { session.step(&[0.2]).unwrap(); }
    let recording = session.recording();
    assert!(recording.scene.options.guarded_backtracking);
    let roundtrip = serde_json::from_slice(&serde_json::to_vec(&recording).unwrap()).unwrap();
    let replay = Session::replay(roundtrip).unwrap();
    assert_eq!(serde_json::to_value(session.frame()).unwrap(), serde_json::to_value(replay.frame()).unwrap());
}
#[test]
fn controller_ticks_include_reporting_endpoints_at_each_physics_step() {
    for h in [0.0005, 0.00025, 0.000125] {
        let mut s = scene();
        s.options.step = h;
        let mut session = Session::new(s, 19).unwrap();
        for frame in 1..=5 {
            let end = session.step(&[0.2]).unwrap();
            assert!((end.telemetry.sample_time - frame as f64 * 0.02).abs() < 1e-12);
        }
    }
}

#[test]
fn episode_drives_real_plant_and_replays_controller_and_physics() {
    let mut session = Session::new(scene(), 19).unwrap();
    assert_eq!(session.contract.actuators.len(), 1);
    assert_eq!(session.inputs[0].name, "command.position");
    let before = session.robot.time();
    assert!(session.step(&[2.]).is_err());
    assert_eq!(session.robot.time(), before);
    for _ in 0..20 {
        session.step(&[0.2]).unwrap();
    }
    let end = session.frame();
    assert!((end.time_s - 0.4).abs() < 1e-9);
    assert!(
        end.joint_positions[0] > 0.1,
        "real motor must move the pendulum: {:?}",
        end.joint_positions
    );
    assert!((end.telemetry.actuators[0] - 0.2).abs() < 1e-12);
    let replay = Session::replay(session.recording()).unwrap().frame();
    assert_eq!(
        serde_json::to_value(&end).unwrap(),
        serde_json::to_value(&replay).unwrap()
    );
    let reset = session.reset(19).unwrap();
    assert_eq!(reset.time_s, 0.);
    assert!(reset.joint_positions[0].abs() < 1e-12);
    assert!(session.recording().actions.is_empty());
}
#[test]
fn invalid_timing_is_rejected_before_building_the_plant() {
    let mut s = scene();
    s.options.step = 0.;
    assert!(Session::new(s, 0).err().unwrap().contains("step"));
    let mut s = scene();
    s.period_s = 0.0201;
    assert!(Session::new(s, 0).err().unwrap().contains("integer"));
}

#[test]
fn recorded_physical_trajectory_agrees_with_independent_numerical_jacobians() {
    compare_derivative_modes(false,false);
}

#[test]
fn experimental_hybrid_trajectory_agrees_with_independent_numerical_jacobians() {
    compare_derivative_modes(true,false);
}

#[test]
fn experimental_motor_derivatives_agree_with_independent_numerical_trajectory() {
    compare_derivative_modes(false,true);
}

#[test]
fn contact_boundary_trajectory_matches_numerical_reference_without_retries() {
    check_contact_trajectory(false,false);
}

#[test]
fn experimental_backlash_events_match_numerical_reference_and_replay() {
    check_contact_trajectory(true,false);
}

#[test]
fn experimental_event_matrix_reuse_has_an_independent_reference() {
    check_contact_trajectory(true,true);
}

fn check_contact_trajectory(events:bool, reuse:bool) {
    use sim_runtime::session::Recording;
    use sim_runtime::validation::{compare_recording,CompareConfig};
    let mut scene = scene();
    scene.options.contact = true;
    scene.options.backlash_events = events;
    scene.options.event_jacobian_reuse = reuse;
    scene.options.step = 0.000125;
    scene.robot.world.floor_z += 1e-6;
    let recording = Recording {version:1,scene,seed:0,actions:vec![vec![0.2];20]};
    let report = compare_recording(&recording,&CompareConfig::default()).unwrap();
    assert!(report.passed,"{}",serde_json::to_string_pretty(&report).unwrap());
    assert_eq!(report.compared_frames,21);
    assert_eq!(report.candidate_options.event_jacobian_reuse,reuse);
    assert!(!report.reference_options.event_jacobian_reuse);
    let session = Session::replay(recording).unwrap();
    assert!(!session.frame().contacts.is_empty(),"fixture must load an SDF contact pair");
    assert!(session.robot.runtime.islands.iter().all(|i|i.stats.subdivided_steps==0));
}

fn compare_derivative_modes(hybrid: bool, motor: bool) {
    use sim_runtime::session::Recording;
    use sim_runtime::validation::{compare_recording,CompareConfig};
    let mut scene = scene();
    scene.options.hybrid_articulated_jacobian = hybrid;
    scene.options.analytic_motor_jacobian = motor;
    let recording=Recording{version:1,scene,seed:19,actions:vec![vec![0.2];20]};
    let report=compare_recording(&recording,&CompareConfig::default()).unwrap();
    assert!(report.passed,"{}",serde_json::to_string_pretty(&report).unwrap());
    assert_eq!(report.compared_frames,21);
    assert_eq!(report.frames.len(), 21);
    assert!(report.frames.iter().all(|f| f.mismatches == 0 && f.max_error_ratio <= 1.0));
    assert!(!report.frames.last().unwrap().candidate_solver.is_empty());
    assert!(report.comparisons>1000);
    assert!(report.first_difference.is_none());
    assert_eq!(report.errors_by_unit.values().map(|s| s.comparisons).sum::<usize>(),report.comparisons);
    let probe = Session::new(recording.scene.clone(), 19).unwrap();
    let per_frame = probe.robot.runtime.model.state.iter().count()
        + probe.frame().poses.len() * 12 + 3;
    assert_eq!(report.comparisons, 21 * per_frame,
        "every stored channel must be compared, including repeated node names");
}

#[test]
fn timestep_comparison_preserves_failure_onset_and_physical_error_magnitudes() {
    use sim_runtime::{session::Recording, validation::{compare_recording, CompareConfig}};
    let recording = Recording {version:1,scene:scene(),seed:19,actions:vec![vec![0.2];5]};
    let config = CompareConfig {reference_substeps:4,..Default::default()};
    let report = compare_recording(&recording,&config).unwrap();
    assert!(!report.passed,"the coarse trajectory must not pass the derivative-level tolerance");
    let first = report.first_difference.as_ref().unwrap();
    assert_eq!(first.frame,report.frames.iter().find(|f| f.mismatches>0).unwrap().frame);
    assert!(first.error>first.tolerance);
    assert_eq!(report.errors_by_unit.values().map(|s| s.mismatches).sum::<usize>(),report.mismatches);
    for (unit,summary) in &report.errors_by_unit {
        let samples = report.frames.iter().filter_map(|f| f.errors_by_unit.get(unit)).collect::<Vec<_>>();
        assert_eq!(samples.iter().map(|s| s.comparisons).sum::<usize>(),summary.comparisons);
        assert_eq!(samples.iter().map(|s| s.maximum_absolute_error).fold(0.0,f64::max),summary.maximum_absolute_error);
        assert!(summary.rms_error.is_finite() && summary.rms_error<=summary.maximum_absolute_error+1e-14);
        assert_eq!(summary.maximum_error_sample.as_ref().unwrap().unit,*unit);
    }
}

#[test]
fn experimental_articulated_rate_partials_match_independent_trajectory() {
    use sim_runtime::{session::Recording,validation::{compare_recording,CompareConfig}};
    let mut scene=scene();
    scene.options.articulated_rate_partials=true;
    let recording=Recording {version:1,scene,seed:19,actions:vec![vec![0.2];20]};
    let report=compare_recording(&recording,&CompareConfig::default()).unwrap();
    assert!(report.passed,"{}",serde_json::to_string_pretty(&report).unwrap());
    assert!(report.candidate_options.articulated_rate_partials);
    assert!(!report.candidate_options.hybrid_articulated_jacobian);
    assert!(report.reference_options.numerical_jacobian);
    assert_eq!(report.compared_frames,21);
}

#[test]
fn shared_comparison_boundaries_preserve_controller_schedule_and_are_recorded() {
    use sim_runtime::{session::Recording, validation::{compare_recording, CompareConfig}};
    let recording = Recording { version:1, scene:scene(), seed:19, actions:vec![vec![0.2];5] };
    let points = vec![0.002125, 0.00725];
    let config = CompareConfig { shared_step_breakpoints_s:points.clone(), ..Default::default() };
    let report = compare_recording(&recording,&config).unwrap();
    assert!(report.passed,"{}",serde_json::to_string_pretty(&report).unwrap());
    assert_eq!(report.config.shared_step_breakpoints_s,points);
    let baseline = compare_recording(&recording,&CompareConfig::default()).unwrap();
    let work = &report.frames.last().unwrap().candidate_solver[0];
    let ordinary = &baseline.frames.last().unwrap().candidate_solver[0];
    assert_eq!(work.events,ordinary.events);
    assert!(work.steps>ordinary.steps);
}

#[test]
fn shared_prefix_preserves_controller_history_and_excludes_warmup_from_comparisons() {
    use sim_runtime::{session::Recording, validation::{compare_recording, CompareConfig}};
    let recording = Recording { version:1, scene:scene(), seed:19,
        actions:vec![vec![0.2],vec![-0.1],vec![0.3],vec![0.0],vec![0.1]] };
    let config = CompareConfig { shared_prefix_frames:3, ..Default::default() };
    let report = compare_recording(&recording,&config).unwrap();
    assert!(report.passed,"{}",serde_json::to_string_pretty(&report).unwrap());
    let prefix = report.shared_prefix.as_ref().unwrap();
    assert!(prefix.exact_state_match);
    assert_eq!(prefix.frames,3);
    assert!((prefix.time_s-0.06).abs()<1e-12);
    assert_eq!(report.compared_frames,3);
    assert_eq!(report.frames.iter().map(|f| f.frame).collect::<Vec<_>>(),vec![3,4,5]);
    assert_eq!(report.frames[0].max_error_ratio,0.0);
    assert!(report.frames[0].candidate_solver[0].events>0);
    assert_eq!(report.frames[0].candidate_solver[0].events, report.frames[0].reference_solver[0].events);
    let ordinary = compare_recording(&recording,&CompareConfig::default()).unwrap();
    assert!(ordinary.shared_prefix.is_none());
    assert_eq!(report.comparisons*2,ordinary.comparisons);
    let refined = compare_recording(&recording,&CompareConfig {
        reference_substeps:4, ..config.clone()
    }).unwrap();
    assert!(!refined.passed,"a shared prefix must not mask subsequent timestep error");
    assert_eq!(refined.frames[0].max_error_ratio,0.0);
    assert!(refined.first_difference.unwrap().frame>config.shared_prefix_frames);
    assert!(compare_recording(&recording,&CompareConfig {
        shared_prefix_frames:recording.actions.len(), ..config
    }).is_err(),"a comparison must retain an independent suffix");
}

#[test]
fn paired_attempt_capture_excludes_prefix_and_preserves_comparison() {
    use sim_runtime::{session::Recording, validation::{compare_recording, CompareConfig}};
    let recording = Recording {version:1,scene:scene(),seed:19,
        actions:vec![vec![0.2],vec![-0.1],vec![0.3]]};
    let config = CompareConfig {shared_prefix_frames:2,..Default::default()};
    let ordinary = compare_recording(&recording,&config).unwrap();
    assert!(ordinary.attempt_audit.is_none());
    let audited = compare_recording(&recording,&CompareConfig {attempt_audit_limit:128,..config.clone()}).unwrap();
    assert_eq!(serde_json::to_value(&ordinary.frames).unwrap(),serde_json::to_value(&audited.frames).unwrap());
    assert_eq!(ordinary.passed,audited.passed);
    let audit=audited.attempt_audit.as_ref().unwrap();
    assert!(!audit.capacity_reached);
    let time=audited.shared_prefix.as_ref().unwrap().time_s;
    for side in [&audit.candidate,&audit.reference] {
        let islands=side["islands"].as_array().unwrap();
        assert!(!islands.is_empty());
        for island in islands {
            let attempts=island["attempts"].as_array().unwrap();
            assert!(!attempts.is_empty());
            assert_eq!(attempts[0]["solve"]["start_time"].as_f64().unwrap(),time);
            assert!(attempts.iter().all(|a|a["solve"]["start_time"].as_f64().unwrap()>=time));
        }
    }
    assert_eq!(audit.candidate["islands"][0]["attempts"][0]["solve"]["initial_state"],
        audit.reference["islands"][0]["attempts"][0]["solve"]["initial_state"]);
    let limited=compare_recording(&recording,&CompareConfig {attempt_audit_limit:1,..config.clone()}).unwrap();
    assert!(limited.attempt_audit.unwrap().capacity_reached);
    assert_eq!(serde_json::to_value(&ordinary.frames).unwrap(),serde_json::to_value(&limited.frames).unwrap());
    assert!(compare_recording(&recording,&CompareConfig {attempt_audit_limit:10001,..config}).is_err());
}

#[test]
fn solver_point_audit_preserves_replay_and_reconstructs_physical_rates() {
    let mut ordinary=Session::new(scene(),19).unwrap();
    let mut audited=Session::new(scene(),19).unwrap();
    audited.set_attempt_audit_limit(128).unwrap();
    for command in [0.2,-0.1,0.3] {
        let a=ordinary.step(&[command]).unwrap();
        let b=audited.step(&[command]).unwrap();
        assert_eq!(serde_json::to_value(a).unwrap(),serde_json::to_value(b).unwrap());
    }
    let report=sim_runtime::validation::implicit_attempt_report(&audited).unwrap();
    assert!(!report["islands"][0]["attempts"].as_array().unwrap().is_empty());
    let points=&audited.robot.runtime.islands[0].implicit_attempts;
    assert!(points.iter().any(|p| {
        audited.robot.generalized_at_solver_point(0,p).unwrap().is_some_and(|g| g.qdd.iter().any(|a| a.abs()>1e-3))
    }));
    assert!(report["islands"][0]["attempts"][0]["physical_at_stage"].is_object());
    let count = audited.robot.runtime.islands[0].implicit_attempts.len();
    assert!(audited.set_attempt_audit_limit(10_001).is_err());
    assert_eq!(audited.robot.runtime.islands[0].implicit_attempts.len(),count);
    let frame = serde_json::to_value(audited.frame()).unwrap();
    audited.set_attempt_audit_limit(0).unwrap();
    assert!(audited.robot.runtime.islands[0].implicit_attempts.is_empty());
    assert_eq!(audited.robot.runtime.islands[0].attempt_audit_limit(),0);
    assert_eq!(frame,serde_json::to_value(audited.frame()).unwrap());
}

#[test]
fn committed_contact_impulses_are_additive_and_require_complete_known_coverage() {
    use sim_runtime::contact_audit::committed_contact_impulses;
    let mut spec=scene();spec.options.contact=true;spec.options.step=0.000125;
    spec.robot.world.floor_z+=1e-6;
    let mut s=Session::new(spec,0).unwrap();s.set_attempt_audit_limit(1024).unwrap();
    s.step(&[0.2]).unwrap();let mid=s.frame().time_s;
    s.step(&[0.2]).unwrap();let end=s.frame().time_s;
    let first=committed_contact_impulses(&s,0.0,mid).unwrap();
    let second=committed_contact_impulses(&s,mid,end).unwrap();
    let whole=committed_contact_impulses(&s,0.0,end).unwrap();
    assert!(!whole.contacts.is_empty());
    assert!(whole.contacts.iter().any(|c|c.impulse_ns.iter().any(|v|v.abs()>0.0)), "{whole:?}");
    assert_eq!(whole.committed_steps,first.committed_steps+second.committed_steps);
    for contact in &whole.contacts {
        for axis in 0..3 {
            let find=|r:&sim_runtime::contact_audit::ContactImpulseReport|r.contacts.iter()
                .find(|c|c.link==contact.link && c.other==contact.other).map_or(0.0,|c|c.impulse_ns[axis]);
            assert!((contact.impulse_ns[axis]-find(&first)-find(&second)).abs()<1e-12);
        }
    }
    let point=s.robot.runtime.islands[0].implicit_attempts.iter_mut().find(|p|p.committed==Some(true)).unwrap();
    point.committed=None;
    assert!(committed_contact_impulses(&s,0.0,end).unwrap_err().contains("unknown"));
    s.set_attempt_audit_limit(1).unwrap();let start=s.frame().time_s;
    s.step(&[0.2]).unwrap();
    assert!(committed_contact_impulses(&s,start,s.frame().time_s).unwrap_err().contains("capacity"));
}
