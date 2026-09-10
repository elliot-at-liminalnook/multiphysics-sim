use serde_json::{json, Value};
use sim_runtime::{motion_tracking::compare_motion, tracking::CaptureConfig};
fn fixture() -> (Value, Value, CaptureConfig) {
    let pose = |name: &str, x: f64| json!({"name":name,"position_m":[x,0,0],"rotation":[[1,0,0],[0,1,0],[0,0,1]]});
    let f = |t: f64, p: f64, drift: f64| json!({"time_s":t,"reference_time_s":p,"poses":[pose("body",p+drift),pose("foot",2.0*p+drift)]});
    let trajectory = json!({"interpolation":"linear","keyframes":[{"time_s":0,"values":[0]},{"time_s":0.2,"values":[0.2]}]});
    let common = json!({"completed":true,"source":{"cad_sha256":"synthetic-test"},"embedding":{},"initial_coordinates":[0],"independent_coordinates":["joint"],"initial_base_translation_m":[0,0,0]});
    let mut a = common.clone();
    a["simulated_s"] = json!(0.4);
    a["motor_experiment"] = json!({"target_trajectory":trajectory});
    a["motion_gate"] = json!({"clock":{"duration_s":0.2}});
    a["frames"] = json!([
        f(0.0, 0.0, 0.0),
        f(0.1, 0.1, 0.0),
        f(0.2, 0.1, 0.003),
        f(0.3, 0.2, 0.0),
        f(0.4, 0.2, 0.0)
    ]);
    let mut b = common;
    b["trajectory"] = trajectory;
    b["config"] = json!({"initial_base_translation_m":[0,0,0]});
    b["frames"] = json!([f(0.0, 0.0, 0.0), f(0.1, 0.1, 0.0), f(0.2, 0.2, 0.0)]);
    let markers=serde_json::from_value(json!({"experiment_id":"synthetic","coordinate_frame":"world-Z-up","expected_cad_sha256":"synthetic-test","markers":[{"id":"tip","link":"foot","local_point_m":[0,0,0]}]})).unwrap();
    (a, b, markers)
}
#[test]
fn pause_and_final_hold_align_with_plan_but_world_drift_stays_visible() {
    let (a, b, m) = fixture();
    let r = compare_motion(&a, &b, &m, "body").unwrap();
    assert_eq!(r["samples"], 5);
    assert_eq!(r["reference_duration_s"], 0.2);
    let e = &r["sample_errors"][2]["errors"];
    assert!((e["world"]["tip"]["distance_m"].as_f64().unwrap() - 0.003).abs() < 1e-14);
    assert!(e["body_relative"]["tip"]["distance_m"].as_f64().unwrap() < 1e-14);
    assert_eq!(r["sample_errors"][2]["reference_time_s"], 0.1);
    // A real mechanism error must remain visible in both coordinate frames.
    let mut lag = a;
    lag["frames"][2]["poses"][1]["position_m"][0] = json!(0.198);
    let r = compare_motion(&lag, &b, &m, "body").unwrap();
    assert!(
        (r["sample_errors"][2]["errors"]["body_relative"]["tip"]["error_m"][0]
            .as_f64()
            .unwrap()
            + 0.005)
            .abs()
            < 1e-14
    );
}
#[test]
fn incomplete_mismatched_or_invented_phase_cannot_pass() {
    let (a, b, m) = fixture();
    for change in [
        |x: &mut Value| {
            x["frames"][2]
                .as_object_mut()
                .unwrap()
                .remove("reference_time_s")
                .map(|_| ())
                .unwrap()
        },
        |x: &mut Value| x["frames"][2]["reference_time_s"] = json!(0.05),
        |x: &mut Value| x["frames"][1]["reference_time_s"] = json!(0.2),
        |x: &mut Value| x["frames"].as_array_mut().unwrap().truncate(3),
        |x: &mut Value| x["source"]["cad_sha256"] = json!("different"),
        |x: &mut Value| x["initial_base_translation_m"] = json!([1, 0, 0]),
        |x: &mut Value| x["initial_base_rotation_vector_rad"] = json!([0, 0, 0.1]),
        |x: &mut Value| x["completed"] = json!(false),
        |x: &mut Value| x["simulated_s"] = json!(0.5),
    ] {
        let mut bad = a.clone();
        change(&mut bad);
        assert!(compare_motion(&bad, &b, &m, "body").is_err());
    }
    // Refuse missing exact samples instead of silently interpolating a mechanism.
    let mut sparse = b.clone();
    sparse["frames"].as_array_mut().unwrap().remove(1);
    assert!(compare_motion(&a, &sparse, &m, "body")
        .unwrap_err()
        .contains("denser reference"));
    let mut cut = b.clone();
    cut["frames"].as_array_mut().unwrap().pop();
    assert!(compare_motion(&a, &cut, &m, "body")
        .unwrap_err()
        .contains("trajectory endpoint"));
}

#[test]
fn ordinary_reference_still_uses_simulation_time() {
    let (mut a, mut b, m) = fixture();
    b.as_object_mut().unwrap().remove("config"); // Legacy sweep stores placement at the top level.
    a["motion_gate"] = Value::Null;
    a["simulated_s"] = json!(0.2);
    a["frames"] = b["frames"].clone();
    let r = compare_motion(&a, &b, &m, "body").unwrap();
    assert_eq!(r["alignment"], "simulation_time");
    assert!(r["markers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|m| m["maximum_error_m"] == 0.0));
}

#[test]
fn body_pose_error_retains_world_translation_and_rotation() {
    let (mut a, b, m) = fixture();
    a["frames"][2]["poses"][0]["rotation"] = json!([[0, -1, 0], [1, 0, 0], [0, 0, 1]]);
    let r = compare_motion(&a, &b, &m, "body").unwrap();
    let e = &r["sample_errors"][2]["reference_link_pose_error"];
    assert!((e["translation_m"][0].as_f64().unwrap() - 0.003).abs() < 1e-14);
    assert!(
        (e["rotation_angle_rad"].as_f64().unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-14
    );
    assert_eq!(e["coordinate_frame"], "world-Z-up");
}

#[test]
fn typed_numeric_metadata_accepts_json_representation_but_not_changed_values() {
    let (mut a, b, m) = fixture();
    a["motor_experiment"]["target_trajectory"]["keyframes"][0]["time_s"] = json!(0.0);
    a["initial_coordinates"] = json!([0.0]);
    a["initial_base_translation_m"] = json!([0.0, 0.0, 0.0]);
    compare_motion(&a, &b, &m, "body").unwrap();
    a["motor_experiment"]["target_trajectory"]["keyframes"][1]["values"][0] =
        json!(0.20000000000000004);
    assert!(compare_motion(&a, &b, &m, "body").is_err());
}
