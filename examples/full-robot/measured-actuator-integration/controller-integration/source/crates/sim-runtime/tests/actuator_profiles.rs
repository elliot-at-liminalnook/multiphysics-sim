use sim_domain_robot::actuator_profile::*;
use sim_runtime::{BuildOptions, PhysicalRobot};
use std::collections::BTreeMap;

fn scene() -> sim_runtime::session::Scene {
    serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap()
}
fn parameter(value: f64, unit: &str) -> Parameter {
    Parameter {
        value,
        unit: unit.into(),
        provenance: Provenance::Estimated,
        uncertainty: None,
        evidence: "synthetic".into(),
    }
}
fn profiles(id: &str) -> Profiles {
    let registry = sim_runtime::registry();
    let base = sim_runtime::experiment_study::baseline().unwrap().0;
    let params = |kind: &str, values: &BTreeMap<String, f64>| {
        registry
            .get(&kind.into())
            .unwrap()
            .parameters
            .as_ref()
            .unwrap()
            .iter()
            .filter(|p| !p.name.contains('.'))
            .map(|p| {
                (
                    p.name.clone(),
                    parameter(values.get(&p.name).copied().or(p.default).unwrap(), &p.unit),
                )
            })
            .collect()
    };
    Profiles {
        version: 1,
        families: BTreeMap::from([(
            "family".into(),
            Family {
                version: 1,
                description: "Synthetic explicit physical profile".into(),
                shaft_coordinate: "motor_output_relative_to_housing".into(),
                limitations: vec!["Synthetic; not measured or accepted".into()],
                evidence: BTreeMap::from([(
                    "synthetic".into(),
                    Evidence {
                        path: "synthetic-fixture".into(),
                        sha256: "0".repeat(64),
                        scope: "Unit tests only".into(),
                    },
                )]),
                motor: params(sim_domain_robot::MOTOR_UNIT, &base.motor),
                driver: params(sim_domain_robot::H_BRIDGE, &base.bridge),
                controller: FixedPd {
                    gains: sim_domain_control::fixed_pd::Gains {
                        kp_q8: 4096,
                        kd_q8: 0,
                        kv_q8: 4096,
                        limit: 350,
                    },
                    period: parameter(0.01, "s"),
                    latency: parameter(0., "s"),
                    encoder_quantum: parameter(std::f64::consts::TAU / 4096., "rad"),
                    evidence: "synthetic".into(),
                    implementation_blake3: "0".repeat(64),
                },
            },
        )]),
        bindings: BTreeMap::from([(
            id.into(),
            Binding {
                feedback: None,
                family: "family".into(),
                version: 1,
                physical_unit: None,
                deviations: BTreeMap::new(),
            },
        )]),
    }
}

#[test]
fn profile_resolution_is_idempotent_and_uses_registry_units_bounds_and_identity() {
    let mut model = scene().robot;
    let id = model.motors[0].id.clone();
    let mut p = profiles(&id);
    p.bindings.get_mut(&id).unwrap().physical_unit = Some("bench-unit-A".into());
    p.bindings
        .get_mut(&id)
        .unwrap()
        .deviations
        .insert("motor.resistance".into(), parameter(0.2, "Ω"));
    model.actuator_profiles = Some(p.clone());
    let registry = sim_runtime::registry();
    model.resolve_actuator_profiles(&registry).unwrap();
    let expected = p.families["family"].motor["resistance"].value + 0.2;
    for _ in 0..2 {
        model.resolve_actuator_profiles(&registry).unwrap();
        assert_eq!(
            model.motors[0].resolved_actuator.as_ref().unwrap().motor["resistance"],
            expected
        );
        assert_eq!(
            sim_domain_robot::motor::cad_motor_unit_parameters(
                &model.motors[0],
                0.03,
                300.,
                true,
                false
            )["resistance"],
            expected
        );
        assert_eq!(
            sim_domain_robot::motor::cad_motor_unit_parameters(
                &model.motors[0],
                0.03,
                300.,
                true,
                false
            )["backlash"],
            0.03
        );
    }
    let value = model.to_json_value_checked().unwrap();
    assert!(value["motors"][0].get("resolved_actuator").is_none());
    let mut restored: sim_domain_robot::PhysicalModel = serde_json::from_value(value).unwrap();
    assert!(restored.motors[0].resolved_actuator.is_none());
    restored.resolve_actuator_profiles(&registry).unwrap();
    assert_eq!(
        restored.motors[0].resolved_actuator.as_ref().unwrap().motor["resistance"],
        expected
    );
    let mut bad = p.clone();
    bad.families
        .get_mut("family")
        .unwrap()
        .motor
        .get_mut("resistance")
        .unwrap()
        .unit = "A".into();
    assert!(bad.resolve(&model, &registry).is_err());
    let mut bad = p.clone();
    bad.bindings.get_mut(&id).unwrap().physical_unit = None;
    assert!(bad.resolve(&model, &registry).is_err());
    let mut bad = p.clone();
    bad.bindings.get_mut(&id).unwrap().version = 2;
    assert!(bad.resolve(&model, &registry).is_err());
    let mut bad = p.clone();
    bad.bindings
        .get_mut(&id)
        .unwrap()
        .deviations
        .get_mut("motor.resistance")
        .unwrap()
        .value = -100.;
    assert!(bad.resolve(&model, &registry).is_err());
    let mut bad = p;
    bad.families
        .get_mut("family")
        .unwrap()
        .motor
        .remove("loss_speed_scale");
    assert!(bad.resolve(&model, &registry).is_err());
    let mut missing_joint = model.clone();
    missing_joint.joints.clear();
    assert!(
        missing_joint
            .resolve_actuator_profiles(&registry)
            .unwrap_err()
            .contains("existing CAD joint")
    );
    // A failed edit cannot partially overwrite already resolved parameters.
    assert_eq!(
        missing_joint.motors[0]
            .resolved_actuator
            .as_ref()
            .unwrap()
            .motor["resistance"],
        expected
    );
}

fn driven_current(resistance: f64) -> f64 {
    let mut model = scene().robot;
    model.gravity = [0.; 3];
    model.control.period_s = 0.001;
    model.control.latency_s = 0.;
    let mut p = profiles(&model.motors[0].id);
    p.families
        .get_mut("family")
        .unwrap()
        .motor
        .get_mut("resistance")
        .unwrap()
        .value = resistance;
    let name = model.motors[0].name.clone();
    model.actuator_profiles = Some(p);
    let mut robot = PhysicalRobot::build(
        model,
        &sim_runtime::registry(),
        &BuildOptions {
            driver_control: true,
            contact: false,
            flex: false,
            step: 0.0001,
            ..Default::default()
        },
    )
    .unwrap();
    let seam = robot.seam.unwrap();
    let index = robot
        .runtime
        .contract(seam)
        .actuators
        .iter()
        .position(|p| p.name == format!("{name}.duty"))
        .unwrap();
    robot
        .runtime
        .attach(
            seam,
            Box::new(sim_core::FnCoupler(
                move |_: f64, _: &[f64], a: &mut [f64]| a[index] = 0.1,
            )),
        )
        .unwrap();
    let behavior = robot
        .runtime
        .model
        .behaviors
        .iter()
        .find(|(_, b)| robot.runtime.model.objects[b.object].name == format!("{name}.unit"))
        .unwrap()
        .0;
    let current = robot.runtime.state_id(behavior, "current");
    for _ in 0..50 {
        robot.advance(0.0001).unwrap();
    }
    robot.runtime.get(current)
}
#[test]
fn resolved_profile_changes_the_shared_physical_motor_not_only_metadata() {
    let a = driven_current(3.7);
    let b = driven_current(7.4);
    assert!(
        a > 0. && b > 0. && a > b * 1.2,
        "profile resistance must change winding response: {a}, {b}"
    );
}
#[test]
fn profiles_cannot_silently_run_catalog_position_firmware() {
    let mut s = scene();
    s.robot.actuator_profiles = Some(profiles(&s.robot.motors[0].id));
    let e = PhysicalRobot::build(s.robot, &sim_runtime::registry(), &BuildOptions::default())
        .err()
        .unwrap();
    assert!(e.contains("explicit PWM"));
}

fn controlled_scene() -> (sim_runtime::session::Scene, sim_runtime::embedded::Config) {
    let mut scene = scene();
    scene.options.driver_control = true;
    scene.robot.gravity = [0.; 3];
    let id = scene.robot.motors[0].id.clone();
    let mut p = profiles(&id);
    let c = &mut p.families.get_mut("family").unwrap().controller;
    c.implementation_blake3 =
        blake3::hash(include_bytes!("../../sim-domain-control/src/fixed_pd.rs"))
            .to_hex()
            .to_string();
    c.latency = parameter(0.004, "s");
    p.bindings.get_mut(&id).unwrap().feedback = Some(Feedback {
        encoder_zero: parameter(2048., "1"),
        encoder_direction: parameter(1., "1"),
        sample_phase: parameter(0., "s"),
        initial_target: parameter(0., "rad"),
    });
    scene.robot.actuator_profiles = Some(p);
    let mut config: sim_runtime::embedded::Config = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.embedded.json"
    ))
    .unwrap();
    config.motors.as_mut().unwrap().controller =
        Some(sim_runtime::embedded::SampledController::CadFixedPd);
    (scene, config)
}

#[test]
fn cad_fixed_pd_runs_in_shared_session_and_replays_across_host_chunks() {
    use sim_runtime::embedded::{CaptureMode, EmbeddedSession};
    let (scene, config) = controlled_scene();
    let mut session =
        EmbeddedSession::new(scene.clone(), config.clone(), 7, CaptureMode::Full).unwrap();
    session.advance(config.steps).unwrap();
    let (mut replay, steps) =
        EmbeddedSession::prepare_replay(session.recording(), CaptureMode::Latest).unwrap();
    for _ in 0..steps {
        replay.advance(1).unwrap();
    }
    assert_eq!(session.frame().unwrap(), replay.frame().unwrap());
    assert_eq!(
        session.diagnostic_metadata()["sampled_controller"]["component"],
        sim_domain_control::sampled_fixed_pd::KIND
    );
    let mut off = scene;
    off.robot
        .actuator_profiles
        .as_mut()
        .unwrap()
        .families
        .get_mut("family")
        .unwrap()
        .controller
        .gains
        .limit = 0;
    let mut inactive = EmbeddedSession::new(off, config.clone(), 7, CaptureMode::Latest).unwrap();
    inactive.advance(config.steps).unwrap();
    let driven = session.frame().unwrap();
    let stopped = inactive.frame().unwrap();
    assert_ne!(driven["motor_states"], stopped["motor_states"]);
    assert_ne!(driven["joint_positions"], stopped["joint_positions"]);
}

#[test]
fn cad_fixed_pd_rejects_missing_mapping_wrong_controller_and_silent_fallback() {
    use sim_runtime::embedded::{CaptureMode, EmbeddedSession};
    let (scene, config) = controlled_scene();
    let id = scene.robot.motors[0].id.clone();
    let mut bad = scene.clone();
    bad.robot
        .actuator_profiles
        .as_mut()
        .unwrap()
        .bindings
        .get_mut(&id)
        .unwrap()
        .feedback = None;
    assert!(
        EmbeddedSession::new(bad, config.clone(), 0, CaptureMode::Latest)
            .err()
            .unwrap()
            .contains("origin/polarity")
    );
    let mut bad = scene.clone();
    bad.robot
        .actuator_profiles
        .as_mut()
        .unwrap()
        .families
        .get_mut("family")
        .unwrap()
        .controller
        .implementation_blake3 = "0".repeat(64);
    assert!(
        EmbeddedSession::new(bad, config.clone(), 0, CaptureMode::Latest)
            .err()
            .unwrap()
            .contains("implementation identity")
    );
    let mut bad = config;
    bad.motors.as_mut().unwrap().controller = None;
    assert!(
        EmbeddedSession::new(scene, bad, 0, CaptureMode::Latest)
            .err()
            .unwrap()
            .contains("cannot be bypassed")
    );
}

#[test]
fn controller_environment_and_headless_session_share_profiled_motor_dynamics() {
    use sim_runtime::embedded::{CaptureMode, EmbeddedSession};
    use sim_runtime::environment::{EmbeddedEnvironment, Task};
    let (scene, mut config) = controlled_scene();
    let policy_config: sim_runtime::embedded::Config = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.policy.json"
    ))
    .unwrap();
    config.policy = policy_config.policy;
    config.steps = 160;
    let task:Task=serde_json::from_value(serde_json::json!({
        "version":1,"observation_source":"ideal_runtime_teacher_only","period_s":0.02,
        "observations":[{"name":"angle","source":{"kind":"coordinate_position","coordinate":"joint.pivot"}},
            {"name":"current","source":{"kind":"motor_current","motor":"servo"}}],
        "rewards":[],"termination_bounds":[]
    })).unwrap();
    let mut environment = EmbeddedEnvironment::new(scene.clone(), config.clone(), task, 9).unwrap();
    let mut headless = EmbeddedSession::new(scene, config, 9, CaptureMode::Latest).unwrap();
    for command in [0.2, -0.2] {
        environment.step(&[command]).unwrap();
        headless.set_inputs(&[command]).unwrap();
        headless.advance(80).unwrap();
        let frame = environment.frame().unwrap();
        // The environment adds episode status; every underlying physics and
        // controller field must match the headless session exactly.
        for (key, value) in headless.frame().unwrap().as_object().unwrap() {
            assert_eq!(&frame[key], value, "host mismatch in {key}");
        }
    }
}

#[test]
fn cad_collision_derivation_survives_runtime_model_round_trip() {
    let mut model = scene().robot;
    let evidence = serde_json::json!({"algorithm":"solid_union_ray_or_cad_v2","solid_count":1,
        "kernel_membership":[{"body_id":"thin-solid","solid_index":0,"volume_m3":1.1138807321828736e-15}],
        "kernel_tolerance_m":1e-9});
    model.links[0].collision.sign_derivation = Some(evidence.clone());
    let restored: sim_domain_robot::PhysicalModel =
        serde_json::from_value(model.to_json_value_checked().unwrap()).unwrap();
    assert_eq!(restored.links[0].collision.sign_derivation, Some(evidence));
}
