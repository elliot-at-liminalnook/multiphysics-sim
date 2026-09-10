use serde_json::{Value, json};
use sim_runtime::{
    embedded::{CaptureMode, Config, EmbeddedRecording, EmbeddedSession},
    robot_input::InputOrigin,
    session::{Scene, Session},
};

fn pendulum() -> (Value, Config) {
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
fn wheel() -> Value {
    json!({"version":1,"robot":serde_json::from_str::<Value>(include_str!("../../../examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap(),
        "options":{"contact":false,"flex":false},"period_s":0.003,"duration_s":0.03})
}
fn saved(scene: &Scene) -> Value {
    serde_json::to_value(scene).unwrap()
}
fn reload(scene: &Scene) -> Scene {
    serde_json::from_value(saved(scene)).unwrap()
}

#[test]
fn two_cad_forms_preserve_absent_fields_and_unmodeled_evidence_through_edits() {
    let quad: Value = serde_json::from_str(include_str!(
        "../../../examples/full-robot/teacher-baseline/scene.json"
    ))
    .unwrap();
    for mut raw in [wheel(), quad] {
        raw["robot"]["links"][0]
            .as_object_mut()
            .unwrap()
            .remove("mass");
        raw["robot"]["cad_only_note"] =
            json!({"status":"estimated","reference":"synthetic presence fixture"});
        let input = raw["robot"].clone();
        let mut scene: Scene = serde_json::from_value(raw).unwrap();
        assert_eq!(saved(&scene)["robot"], input);
        assert!(saved(&scene).get("robot_input").is_none());
        let binding = scene.input_binding().unwrap();
        assert_eq!(binding.origin, InputOrigin::EpisodeDocument);
        assert!(
            binding
                .input
                .unwrap()
                .defaulted_fields
                .contains(&"/links/0/mass".into())
        );
        scene.robot.links[0].mass = 2.;
        for _ in 0..3 {
            let wire = saved(&scene);
            assert_eq!(wire["robot"]["links"][0]["mass"], 2.);
            let edits = wire["robot_input"]["overrides"].as_array().unwrap();
            let mass = edits
                .iter()
                .find(|e| e["pointer"] == "/links/0/mass")
                .unwrap();
            assert_eq!(mass["original"], json!({"presence":"absent"}));
            scene = reload(&scene);
            assert_eq!(scene.robot_input.as_ref().unwrap().document(), &input);
            let inspected = sim_runtime::robot_contract::inspect(saved(&scene)).unwrap();
            let inspected_edits =
                serde_json::to_value(inspected.robot_input.as_ref().unwrap()).unwrap();
            assert_eq!(inspected_edits, wire["robot_input"]);
            assert!(
                inspected
                    .robot
                    .defaulted_fields
                    .contains(&"/links/0/mass".into())
            );
            assert!(
                inspected
                    .robot
                    .unmodeled_fields
                    .contains(&"/cad_only_note".into())
            );
        }
    }
}

#[test]
fn nonfinite_edits_cannot_disappear_as_null_or_unchanged_defaults() {
    let mut raw = wheel();
    let joint = raw["robot"]["joints"][0]["name"]
        .as_str()
        .unwrap()
        .to_owned();
    raw["robot"]["identification"] = json!({joint.clone(): {}});
    raw["robot"]["motors"][0]["gearbox"]
        .as_object_mut()
        .unwrap()
        .remove("max_output_torque");
    let scene: Scene = serde_json::from_value(raw).unwrap();
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut edited = scene.clone();
        edited
            .robot
            .identification
            .get_mut(&joint)
            .unwrap()
            .backlash = Some(bad);
        let error = serde_json::to_value(&edited).unwrap_err().to_string();
        assert!(
            error.contains("nonfinite") && error.contains("backlash"),
            "{error}"
        );
        assert!(edited.input_binding().is_err());
        edited.robot_input = None;
        assert!(serde_json::to_value(&edited).is_err());
        let mut edited = scene.clone();
        edited.robot.links[0].mass = bad;
        assert!(serde_json::to_value(&edited).is_err());
        if bad != f64::INFINITY {
            let mut edited = scene.clone();
            edited.robot.motors[0].gearbox.max_output_torque = bad;
            assert!(serde_json::to_value(&edited).is_err());
        }
    }
}

#[test]
fn finite_and_unbounded_limits_roundtrip_with_and_without_original_document() {
    let mut raw = wheel();
    raw["robot"]["motors"][0]["gearbox"]["cad_limit_note"] = json!("synthetic test metadata");
    let mut scene: Scene = serde_json::from_value(raw).unwrap();
    let original = scene.robot_input.as_ref().unwrap().document().clone();
    scene.robot.motors[0].gearbox.max_output_torque = f64::INFINITY;
    scene.robot.motors[0].gearbox.max_output_speed = f64::INFINITY;
    let wire = saved(&scene);
    assert_eq!(
        wire["robot"]["motors"][0]["gearbox"]["cad_limit_note"],
        "synthetic test metadata"
    );
    assert!(
        wire["robot"]["motors"][0]["gearbox"]
            .get("max_output_torque")
            .is_none()
    );
    let mut restored = reload(&scene);
    assert_eq!(restored.robot_input.as_ref().unwrap().document(), &original);
    assert_eq!(
        restored.robot.motors[0].gearbox.max_output_torque,
        f64::INFINITY
    );
    assert_eq!(saved(&restored), wire);
    restored.robot_input = None;
    let mut restored = reload(&restored);
    assert_eq!(
        restored.robot_input.as_ref().unwrap().origin(),
        InputOrigin::ParsedModel
    );
    assert_eq!(
        restored.robot.motors[0].gearbox.max_output_speed,
        f64::INFINITY
    );
    restored.robot.motors[0].gearbox.max_output_torque = 3.;
    assert_eq!(
        reload(&restored).robot.motors[0].gearbox.max_output_torque,
        3.
    );
}

#[test]
fn null_and_absent_are_distinct_and_receipts_are_checked() {
    let (mut raw, _) = pendulum();
    raw["robot"]["joints"][0]["limits"] = Value::Null;
    let mut scene: Scene = serde_json::from_value(raw).unwrap();
    scene.robot.joints[0].limits = Some([-1., 1.]);
    let wire = saved(&scene);
    assert_eq!(
        wire["robot_input"]["overrides"][0]["original"],
        json!({"presence":"present","value":null})
    );
    let scene = reload(&scene);
    assert_eq!(
        scene.robot_input.unwrap().document()["joints"][0].get("limits"),
        Some(&Value::Null)
    );
    for mutation in ["value", "pointer", "original", "duplicate", "version"] {
        let mut invalid = wire.clone();
        match mutation {
            "value" => invalid["robot_input"]["overrides"][0]["value"] = json!([0, 2]),
            "pointer" => invalid["robot_input"]["overrides"][0]["pointer"] = json!("/joints~2"),
            "original" => {
                invalid["robot_input"]["overrides"][0]["original"] =
                    json!({"presence":"present","value":"bad limits"})
            }
            "duplicate" => {
                let edit = invalid["robot_input"]["overrides"][0].clone();
                invalid["robot_input"]["overrides"]
                    .as_array_mut()
                    .unwrap()
                    .push(edit);
            }
            "version" => invalid["robot_input"]["version"] = json!(9),
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<Scene>(invalid).is_err(),
            "{mutation}"
        );
    }
    let mut invalid = wire;
    invalid["robot"]["unmodeled"] = json!(2);
    invalid["robot_input"]["overrides"]
        .as_array_mut()
        .unwrap()
        .push(
            json!({"pointer":"/unmodeled","original":{"presence":"present","value":1},"value":2}),
        );
    assert!(serde_json::from_value::<Scene>(invalid).is_err());
}

#[test]
fn reordered_entities_keep_their_own_evidence_and_unbounded_defaults() {
    let mut raw = wheel();
    for link in raw["robot"]["links"].as_array_mut().unwrap() {
        link["mass_sources"] = json!({"entity":link["id"]});
    }
    for key in ["max_output_torque", "max_output_speed"] {
        raw["robot"]["motors"][0]["gearbox"]
            .as_object_mut()
            .unwrap()
            .remove(key);
    }
    let input = raw["robot"].clone();
    let mut scene: Scene = serde_json::from_value(raw).unwrap();
    assert!(
        scene.robot.motors[0]
            .gearbox
            .max_output_torque
            .is_infinite()
    );
    scene.robot.links.reverse();
    assert!(
        scene
            .input_binding()
            .unwrap()
            .input
            .unwrap()
            .defaulted_fields
            .contains(&"/motors/0/gearbox/max_output_torque".into())
    );
    scene.robot.motors.reverse();
    let wire = saved(&scene);
    for link in wire["robot"]["links"].as_array().unwrap() {
        assert_eq!(link["mass_sources"]["entity"], link["id"]);
    }
    assert!(
        wire["robot"]["motors"][1]["gearbox"]
            .get("max_output_torque")
            .is_none()
    );
    let again = reload(&scene);
    assert_eq!(again.robot_input.as_ref().unwrap().document(), &input);
    assert!(
        again.robot.motors[1]
            .gearbox
            .max_output_torque
            .is_infinite()
    );
    assert_eq!(saved(&again), wire);
}

#[test]
fn parsed_models_are_labelled_unknown_and_new_documents_replace_inputs_atomically() {
    let mut scene: Scene = serde_json::from_value(wheel()).unwrap();
    scene.robot_input = None;
    let wire = saved(&scene);
    assert_eq!(wire["robot_input"]["origin"], "parsed_model");
    let mut scene = reload(&scene);
    let binding = scene.input_binding().unwrap();
    assert_eq!(binding.origin, InputOrigin::ParsedModel);
    assert!(binding.input.is_none());
    assert!(sim_runtime::robot_contract::inspect(saved(&scene)).is_err());
    let before = saved(&scene);
    assert!(
        scene
            .replace_robot_input(json!({"motors":"invalid"}))
            .is_err()
    );
    assert_eq!(saved(&scene), before);
    let new = wheel()["robot"].clone();
    scene.replace_robot_input(new.clone()).unwrap();
    assert_eq!(saved(&scene)["robot"], new);
    assert!(saved(&scene).get("robot_input").is_none());
}

#[test]
fn defaults_and_overrides_survive_detailed_and_incremental_episode_replay() {
    let (mut raw, config) = pendulum();
    raw["robot"]["cad_annotation"] = json!({"confidence":"estimated"});
    raw["robot"]["world"]
        .as_object_mut()
        .unwrap()
        .remove("ambient_c");
    let input = raw["robot"].clone();
    let mut scene: Scene = serde_json::from_value(raw).unwrap();
    scene.robot.world.floor_z += 1e-6;
    let mut detailed = Session::new(scene.clone(), 7).unwrap();
    let commands: Vec<_> = detailed.inputs.iter().map(|p| p.initial).collect();
    detailed.step(&commands).unwrap();
    let recording =
        serde_json::from_slice(&serde_json::to_vec(&detailed.recording()).unwrap()).unwrap();
    let replay = Session::replay(recording).unwrap();
    assert_eq!(
        serde_json::to_value(detailed.frame()).unwrap(),
        serde_json::to_value(replay.frame()).unwrap()
    );
    assert_eq!(
        replay.scene.robot_input.as_ref().unwrap().document(),
        &input
    );
    let mut run = EmbeddedSession::new(scene, config.clone(), 7, CaptureMode::Latest).unwrap();
    run.advance(config.steps).unwrap();
    let record: EmbeddedRecording =
        serde_json::from_slice(&serde_json::to_vec(&run.recording()).unwrap()).unwrap();
    let (mut replay, steps) = EmbeddedSession::prepare_replay(record, CaptureMode::Latest).unwrap();
    replay.advance(steps).unwrap();
    assert_eq!(run.frame().unwrap(), replay.frame().unwrap());
    assert_eq!(
        run.diagnostic_metadata()["robot_input"],
        replay.diagnostic_metadata()["robot_input"]
    );
    let binding = replay.robot_input_binding();
    assert!(
        binding
            .input
            .as_ref()
            .unwrap()
            .defaulted_fields
            .contains(&"/world/ambient_c".into())
    );
    assert_eq!(binding.overrides.len(), 1);
    assert_eq!(
        replay.scene().robot_input.as_ref().unwrap().document(),
        &input
    );
}
