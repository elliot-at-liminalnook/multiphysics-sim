use serde_json::json;
use sim_runtime::{
    embedded::{CaptureMode, Config, EmbeddedSession},
    session::Scene,
};

fn fixture() -> (Scene, Config) {
    let scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap();
    let mut config: serde_json::Value = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.policy.json"
    ))
    .unwrap();
    config["step_s"] = json!(0.002);
    config["steps"] = json!(100);
    config["report_every"] = json!(10);
    config["audit_contact_steps"] = json!(false);
    config["motors"].as_object_mut().unwrap().remove("events");
    config["motors"]["effective"] = json!({"version":1,"assumption_reference":"Synthetic effective response fixture; no hardware calibration",
        "components":[{"dof":"joint.pivot","parameters":{"stiffness":1.0,"damping":0.1,"stall_torque":0.22,"no_load_speed":13.0}}]});
    (scene, serde_json::from_value(config).unwrap())
}

#[test]
fn effective_profile_drives_physics_and_replays_without_fabricated_electrical_readings() {
    let (scene, config) = fixture();
    let source = serde_json::to_value(&scene.robot).unwrap();
    let mut run = EmbeddedSession::new(scene, config, 17, CaptureMode::Latest).unwrap();
    assert_eq!(run.frame().unwrap()["joint_positions"][0], 0.0);
    run.set_inputs(&[0.4]).unwrap();
    run.advance(20).unwrap();
    run.set_inputs(&[-0.2]).unwrap();
    run.advance(80).unwrap();
    let f = run.frame().unwrap();
    assert!(f["joint_positions"][0].as_f64().unwrap().abs() > 1e-4);
    assert!(f["motor_readings"][0].get("current_a").is_none());
    assert!(f.get("servo_commands").is_none());
    assert!(f["motor_states"].as_array().unwrap().is_empty());
    assert_eq!(f["actuator_profile"]["calibrated"], false);
    assert!(
        f["motor_readings"][0]["shaft_torque_nm"]
            .as_f64()
            .unwrap()
            .abs()
            <= 0.22
    );
    assert_eq!(
        serde_json::to_value(&run.recording().scene.robot).unwrap(),
        source
    );
    let (mut replay, n) =
        EmbeddedSession::prepare_replay(run.recording(), CaptureMode::Latest).unwrap();
    replay.advance(n).unwrap();
    assert_eq!(f, replay.frame().unwrap());
}

#[test]
fn malformed_profiles_fail_before_any_advance() {
    let (s, c) = fixture();
    for i in 0..6 {
        let mut value = serde_json::to_value(&c).unwrap();
        match i {
            0 => value["motors"]["events"] = json!({}),
            1 => value["motors"]["effective"]["components"] = json!([]),
            2 => value["motors"]["effective"]["assumption_reference"] = json!(""),
            3 => value["motors"]["effective"]["components"][0]["dof"] = json!("wrong"),
            4 => {
                value["motors"]["effective"]["components"][0]["parameters"]["stall_torque"] =
                    json!(-1)
            }
            5 => value["motors"]["servos"] = json!([]),
            _ => unreachable!(),
        }
        assert!(
            EmbeddedSession::new(
                s.clone(),
                serde_json::from_value(value).unwrap(),
                0,
                CaptureMode::Latest
            )
            .is_err()
        );
    }
}
