//! The in-process physical export on a robot built from an empty archive:
//! a one-joint arm (a grounded base, an MG996R servo mounted on it, a PLA
//! arm on the servo's joint and a steel payload fixed to the arm's tip).
//! The export names its assumptions, refuses nothing it cannot derive, and
//! the simulator builds and drives what it writes.
use serde_json::{Map, Value, json};
use sim_cad::ArchiveDocument;
use sim_cad::ops::{Ctx, run};
use std::collections::HashMap;

struct World {
    doc: ArchiveDocument,
}
impl World {
    fn new() -> Self {
        let bytes = sim_cad::edit::empty_archive(None).unwrap();
        let path = std::env::temp_dir().join("arm.rcad");
        World { doc: ArchiveDocument::from_bytes(&path, bytes, &|| false, &|_| {}).unwrap() }
    }
    fn op(&mut self, name: &str, args: Value, kwargs: Value) -> Value {
        let geometry = sim_cad::geometry::load_geometry(&self.doc, &|| false, &|_| {}).unwrap();
        let centroids: HashMap<String, [f64; 3]> = geometry.iter().map(|b| (b.node_id.clone(), b.properties.centroid_mm)).collect();
        let stamps = sim_cad::annotations::Stamps::new();
        let mut edit = sim_cad::Edit::of(&self.doc);
        let centroid = |id: &str| centroids.get(id).copied();
        let mut cx = Ctx { doc: &self.doc, stamps: &stamps, edit: &mut edit, centroid: &centroid, cancelled: &|| false };
        let args = args.as_array().cloned().unwrap_or_default();
        let kwargs: Map<String, Value> = kwargs.as_object().cloned().unwrap_or_default();
        let answer = run(&mut cx, name, &args, &kwargs).unwrap_or_else(|e| panic!("{name}: {e}"));
        let next = self.doc.apply(edit).unwrap();
        self.doc = ArchiveDocument::from_bytes(&next.path, next.original_bytes.clone(), &|| false, &|_| {}).unwrap();
        answer
    }
    fn export(&self) -> Value {
        let g = sim_cad::geometry::load_geometry(&self.doc, &|| false, &|_| {}).unwrap();
        let masses = sim_cad::mass::derive_document(&self.doc, &g).unwrap();
        sim_cad::physical::export(&self.doc, &g, &masses, &sim_cad::physical::Options { exported_at: "test".into(), ..Default::default() }).unwrap()
    }
}

fn s(v: &Value) -> String {
    v.as_str().unwrap().to_string()
}

/// The arm, built as the design assistant would: returns the joint id.
fn build(w: &mut World) -> String {
    let base = s(&w.op("box", json!([[-40, -40, 0], [80, 40, 80]]), json!({"name": "Base"})));
    w.op("set_material", json!([[base], "pla"]), json!({}));
    w.op("set_ground", json!([base]), json!({}));
    let servo = s(&w.op("add_motor", json!(["mg996r", [0, 0, 60], [0, 1, 0]]), json!({"mount_on": base, "name": "Shoulder servo"})));
    let arm = s(&w.op("box", json!([[-10, 6, 50], [150, 6, 20]]), json!({"name": "Arm"})));
    w.op("set_material", json!([[arm], "pla"]), json!({}));
    let payload = s(&w.op("box", json!([[120, 12, 50], [20, 6, 20]]), json!({"name": "Payload"})));
    w.op("set_material", json!([[payload], "steel"]), json!({}));
    w.op("connect_fixed", json!([arm, payload]), json!({"name": "payload bolts"}));
    let joint = s(&w.op("add_joint", json!(["revolute", base, arm, [0, 6, 60], [0, -1, 0]]), json!({"lower": -0.5, "upper": 2.0, "motor": servo, "name": "Shoulder"})));
    w.op("add_sensor", json!(["encoder", arm, [0, 6, 60]]), json!({"joint": joint, "name": "Shoulder encoder"}));
    joint
}

#[test]
fn an_arm_built_from_an_empty_archive_exports_a_model_the_simulator_runs() {
    let mut w = World::new();
    assert!(w.doc.manifest["materials"].as_array().unwrap().len() >= 13, "a new document has the stock materials");
    let joint = build(&mut w);
    let model = w.export();

    // Links: Base (with the servo merged in), Arm (with the payload merged in).
    let links = model["links"].as_array().unwrap();
    let names: Vec<&str> = links.iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Base", "Arm"]);
    assert_eq!(links[0]["ground"], json!(true));
    let arm = &links[1];
    // PLA 150 × 6 × 20 mm at 1.24 g/cm³ plus steel 20 × 6 × 20 mm at 7.85 g/cm³.
    let expected = 18000e-9 * 1240.0 + 2400e-9 * 7850.0;
    assert!((arm["mass"].as_f64().unwrap() - expected).abs() < 1e-9, "{}", arm["mass"]);
    let com_x = (18000e-9 * 1240.0 * 0.065 + 2400e-9 * 7850.0 * 0.130) / expected;
    assert!((arm["com"][0].as_f64().unwrap() - com_x).abs() < 1e-6);
    assert!(!arm["collision"]["vertices"].as_array().unwrap().is_empty());
    assert_eq!(model["source"]["merged"].as_array().unwrap().len(), 1);

    // The joint and its motor; drive backlash is unmeasured and blocks.
    let j = &model["joints"][0];
    assert_eq!((j["parent"].as_str(), j["child"].as_str(), j["motor"].as_str()), (Some("Base"), Some("Arm"), Some("Shoulder servo")));
    assert_eq!(j["limits"], json!([-0.5, 2.0]));
    assert_eq!(model["motors"][0]["joint"], json!("Shoulder"));
    assert_eq!(model["sensors"][0]["link"], json!("Arm"));
    let blocking: Vec<&Value> = model["source"]["assumptions"].as_array().unwrap().iter().filter(|a| a["blocking"] == json!(true)).collect();
    assert_eq!(blocking.len(), 1, "{blocking:?}");
    assert!(blocking[0]["at"].as_str().unwrap().ends_with("drive_backlash"));
    // The simulator refuses it for the same reason.
    let parsed = sim_domain_robot::model::PhysicalModel::parse(&model.to_string()).unwrap();
    let err = sim_runtime::PhysicalRobot::build(parsed, &sim_runtime::registry(), &sim_runtime::BuildOptions::default()).err().unwrap();
    assert!(err.contains("drive backlash is unmeasured"), "{err}");

    // Authored in CAD (as RoboCAD stores it), the model builds and the servo holds and lifts the arm.
    w.op("set_joint_physics", json!([joint]), json!({"drive_backlash": {"width_rad": 0.017, "provenance": "estimated", "reference": "MG996R spline play, typical"}}));
    assert!(w.doc.node(&joint).unwrap()["robot"]["physics"]["drive_backlash"].is_object());
    let model = w.export();
    assert_eq!(model["source"]["blocking_assumptions"], json!(0));
    assert_eq!(model["joints"][0]["physics"]["source"], json!("declared"));
    let parsed = sim_domain_robot::model::PhysicalModel::parse(&model.to_string()).unwrap();
    let mut robot = sim_runtime::PhysicalRobot::build(parsed, &sim_runtime::registry(), &sim_runtime::BuildOptions::default()).unwrap();
    robot.advance(0.5).unwrap();
    let held = robot.joint_angles()[0];
    assert!(held.abs() < 0.05, "the servo holds the arm level: {held}");
    robot.set_target(0, 1.0);
    robot.advance(1.5).unwrap();
    let lifted = robot.joint_angles()[0];
    assert!((lifted - 1.0).abs() < 0.05, "the servo lifts the arm to 1 rad: {lifted}");

    // The acceptance test the project states: lift to 1 rad within 1 s, hold it, stay in reserve.
    let test: sim_runtime::acceptance::Test = serde_json::from_value(json!({
        "name": "lift the payload", "duration_s": 3.0,
        "trajectory": [{"t": 0.0, "targets": {"Shoulder": 0.0}}, {"t": 1.0, "targets": {"Shoulder": 1.0}}],
        "criteria": [
            {"kind": "reaches", "joint": "Shoulder", "target": 1.0, "tolerance": 0.05, "by_s": 1.5},
            {"kind": "torque_margin", "min": 0.3},
            {"kind": "winding_temperature", "margin_c": 20.0},
            {"kind": "no_limit_hits"},
            {"kind": "yield_margin", "min": 1.0},
        ],
    })).unwrap();
    let report = sim_runtime::acceptance::run_model(&model, &test, &|| false, &|_| {}).unwrap();
    let status: Vec<&str> = report["outcomes"].as_array().unwrap().iter().map(|o| o["status"].as_str().unwrap()).collect();
    assert_eq!(status, ["pass", "pass", "pass", "pass", "not_assessed"], "{}", report["summary"]);
    assert_eq!(report["verdict"], json!("incomplete"));
    assert_eq!(report["evidence"], json!(false));
    // It ran as a composed system: the robot generated from the model, no
    // controller blocks, the test bench commanding the open servo target.
    assert_eq!(report["system"]["controllers"], json!([]));
    assert_eq!(report["system"]["test_bench"]["commands"], json!(["robot.Shoulder.target"]));
    assert!(report["fingerprint"]["artifacts"]["robot"].is_string(), "{}", report["fingerprint"]);
    // Without the unassessable criterion it passes, and counts as evidence (no blocking assumption).
    let mut passing = test.clone();
    passing.criteria.pop();
    let report = sim_runtime::acceptance::run_model(&model, &passing, &|| false, &|_| {}).unwrap();
    assert_eq!((report["verdict"].as_str(), report["evidence"].as_bool()), (Some("passed"), Some(true)), "{}", report["summary"]);
    // A 6 kg payload overloads the servo: the same test fails, naming the motor.
    let mut heavy = model.clone();
    heavy["links"][1]["mass"] = json!(6.0);
    let report = sim_runtime::acceptance::run_model(&heavy, &passing, &|| false, &|_| {}).unwrap();
    assert_eq!(report["verdict"], json!("failed"), "{}", report["summary"]);
}
