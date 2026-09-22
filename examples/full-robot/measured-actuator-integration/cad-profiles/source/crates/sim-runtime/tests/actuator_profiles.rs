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
