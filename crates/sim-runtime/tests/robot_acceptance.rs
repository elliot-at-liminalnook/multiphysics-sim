//! Robot acceptance tests run the system the robot is composed in
//! (docs/architecture/composition.md, "Robot projects"): the same compile,
//! bind and scheduling path as any system, the controllers that are there,
//! and evidence bound to what ran.

use serde_json::json;
use sim_runtime::acceptance::{self, Test};
use sim_runtime::robot_project::{Project, ProjectFile};
use sim_runtime::system_evidence::{Standing, Verdict};
use sim_runtime::{composition_examples, registry};
use sim_system::SystemStore;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("robot-acceptance-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn rover_model() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/wheeled-robot/baseline/robot.simrobot.json")
}

fn test(value: serde_json::Value) -> Test {
    serde_json::from_value(value).unwrap()
}

fn statuses(report: &serde_json::Value) -> Vec<String> {
    report["outcomes"].as_array().unwrap().iter().map(|o| format!("{}: {}", o["status"].as_str().unwrap(), o["detail"].as_str().unwrap())).collect()
}

/// A project around the rover model, its system made on first use.
fn project(name: &str) -> Project {
    let d = dir(name);
    let project = Project::create(&d, ProjectFile::new("rover", "").unwrap()).unwrap();
    std::fs::copy(rover_model(), project.model()).unwrap();
    project
}

#[test]
fn a_robot_alone_is_commanded_by_the_test_bench_through_the_scheduler() {
    let project = project("alone");
    let registry = registry();
    let document = project.ensure_system(&registry).unwrap();
    assert!(project.system().is_file(), "the project's system is an ordinary file");
    // Made once: a second call reads the same file.
    assert_eq!(project.ensure_system(&registry).unwrap(), document);
    let t = test(json!({
        "name": "turn the left wheel", "duration_s": 0.4,
        "trajectory": [{"t": 0.0, "targets": {"left axle": 0.0}}, {"t": 0.2, "targets": {"left axle": 0.3}}],
        "criteria": [{"kind": "tracking", "joint": "left axle", "max_error": 3.0, "after_s": 0.0}, {"kind": "torque_margin", "min": 0.0}, {"kind": "no_limit_hits"}],
    }));
    let report = acceptance::run(&document, project.dir(), &registry, &t, &BTreeMap::new(), &|| false, &|_| {}).unwrap();
    assert_eq!(report["system"]["controllers"], json!([]));
    assert_eq!(report["system"]["test_bench"]["commands"], json!(["robot.left axle.target"]));
    // The joint the test does not command is held, and said to be.
    assert_eq!(report["system"]["test_bench"]["holds"][0]["input"], json!("robot.right axle.target"));
    // Every series has one value per sample.
    let n = report["trace"]["t"].as_array().unwrap().len();
    assert_eq!(n, 41);
    for group in ["joints", "commands", "targets_received"] {
        for (name, series) in report["trace"][group].as_object().unwrap() {
            assert_eq!(series.as_array().unwrap().len(), n, "{group}.{name}");
        }
    }
    // The servo received the command through the bench's block: at the
    // model's control period and latency, so never ahead of the test's own.
    let sent = report["trace"]["commands"]["left axle"].as_array().unwrap();
    let got = report["trace"]["targets_received"]["left axle"].as_array().unwrap();
    assert_eq!(got.last().unwrap().as_f64(), Some(0.3));
    assert!(sent.iter().zip(got).all(|(s, g)| g.as_f64().unwrap() <= s.as_f64().unwrap() + 1e-12), "sent {sent:?}\ngot {got:?}");
    assert!(statuses(&report)[0].starts_with("pass") || statuses(&report)[0].starts_with("fail"), "tracking was assessed: {:?}", statuses(&report));
    assert!(statuses(&report)[0].contains("the test's command"), "{:?}", statuses(&report));
}

#[test]
fn a_robot_with_controllers_is_tested_with_them_and_never_overridden() {
    let d = dir("controlled");
    let registry = registry();
    let path = composition_examples::rover(&d, &registry).unwrap();
    let document = SystemStore::new(&path).load().unwrap();
    // No trajectory: the two controller FMUs drive the wheels.
    let t = test(json!({
        "name": "follow the controllers", "duration_s": 0.4,
        "criteria": [{"kind": "tracking", "joint": "left axle", "max_error": 3.0, "after_s": 0.0}, {"kind": "no_fall"}],
    }));
    let report = acceptance::run(&document, &d, &registry, &t, &BTreeMap::new(), &|| false, &|_| {}).unwrap();
    let blocks: Vec<&str> = report["system"]["controllers"].as_array().unwrap().iter().map(|c| c["block"].as_str().unwrap()).collect();
    assert_eq!(blocks, ["left_controller", "right_controller"]);
    assert_eq!(report["system"]["test_bench"]["commands"], json!([]));
    assert_eq!(report["system"]["test_bench"]["holds"], json!([]), "nothing is left for the bench to drive");
    assert!(statuses(&report)[0].contains("controller"), "tracking is judged against what the controller sent: {:?}", statuses(&report));
    // The controller's sine reached the servo.
    let got: Vec<f64> = report["trace"]["targets_received"]["left axle"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!(got.iter().any(|v| v.abs() > 0.05), "{got:?}");
    // A test may not take over an input a controller drives.
    let mut takeover = t.clone();
    takeover.trajectory = test(json!({"name": "x", "duration_s": 1.0, "trajectory": [{"t": 0.0, "targets": {"left axle": 0.5}}], "criteria": [{"kind": "no_fall"}]})).trajectory;
    let err = acceptance::run(&document, &d, &registry, &takeover, &BTreeMap::new(), &|| false, &|_| {}).unwrap_err();
    assert!(err.contains("already drives") && err.contains("left axle"), "{err}");
    // Nor command something that is not an input.
    takeover.trajectory[0].targets = BTreeMap::from([("left_controller.target".to_string(), 0.5)]);
    let err = acceptance::run(&document, &d, &registry, &takeover, &BTreeMap::new(), &|| false, &|_| {}).unwrap_err();
    assert!(err.contains("neither a driven joint"), "{err}");
}

#[test]
fn a_result_goes_stale_when_anything_it_ran_changes() {
    let d = dir("stale");
    let registry = registry();
    let path = composition_examples::rover(&d, &registry).unwrap();
    let document = SystemStore::new(&path).load().unwrap();
    let t = test(json!({"name": "upright", "duration_s": 0.1, "criteria": [{"kind": "no_fall"}]}));
    let extra = BTreeMap::from([("the design's part meshes (CAD file)".to_string(), "aa".to_string())]);
    let report = acceptance::run(&document, &d, &registry, &t, &extra, &|| false, &|_| {}).unwrap();
    assert_eq!(report["verdict"], json!("passed"), "{}", report["summary"]);
    let now = |document: &sim_system::SystemDocument, t: &Test, extra: &BTreeMap<String, String>| acceptance::standing(Some(&report), &acceptance::fingerprint(document, &d, t, extra));
    assert_eq!(now(&document, &t, &extra), Standing::Current { verdict: Verdict::Passed });
    assert_eq!(acceptance::standing(None, &acceptance::fingerprint(&document, &d, &t, &extra)), Standing::NotAssessed);
    let changed = |s: Standing| match s {
        Standing::Stale { verdict: Verdict::Passed, changed } => changed.join("; "),
        other => panic!("expected a stale pass, got {other:?}"),
    };
    // The test.
    let mut longer = t.clone();
    longer.duration_s = 0.2;
    assert!(changed(now(&document, &longer, &extra)).contains("the test changed"));
    // A file the judgement reads (the CAD file, for part strength).
    let other = BTreeMap::from([("the design's part meshes (CAD file)".to_string(), "bb".to_string())]);
    assert!(changed(now(&document, &t, &other)).contains("CAD file"));
    // The system (a controller parameter).
    let store = SystemStore::new(&path);
    store.apply(&registry, "retune", &[sim_system::Command::SetParameter { at: String::new(), name: "left_controller".into(), parameter: "amplitude".into(), binding: Some(sim_system::ParameterBinding::value(0.1)) }], None).unwrap();
    assert!(changed(now(&store.load().unwrap(), &t, &extra)).contains("the system's model changed"));
    // The robot model's bytes.
    let model = d.join("rover.simrobot.json");
    let mut bytes = std::fs::read(&model).unwrap();
    bytes.push(b'\n');
    std::fs::write(&model, bytes).unwrap();
    assert!(changed(now(&document, &t, &extra)).contains("`rover`"));
    // A report from before fingerprints is never current.
    let mut old = report.clone();
    old.as_object_mut().unwrap().remove("fingerprint");
    assert!(matches!(acceptance::standing(Some(&old), &acceptance::fingerprint(&document, &d, &t, &extra)), Standing::Stale { .. }));
}

#[test]
fn a_re_exported_model_refreshes_the_robot_in_the_project_s_system() {
    let project = project("refresh");
    let registry = registry();
    let before = project.ensure_system(&registry).unwrap();
    // The model loses a motor: its joint no longer offers a target.
    let mut model: serde_json::Value = serde_json::from_slice(&std::fs::read(project.model()).unwrap()).unwrap();
    let dropped = model["motors"].as_array_mut().unwrap().pop().unwrap();
    let motor = dropped["name"].as_str().unwrap().to_string();
    for j in model["joints"].as_array_mut().unwrap() {
        if j["motor"] == json!(motor) {
            j.as_object_mut().unwrap().remove("motor");
        }
    }
    std::fs::write(project.model(), serde_json::to_vec(&model).unwrap()).unwrap();
    let after = project.ensure_system(&registry).unwrap();
    let ports = |d: &sim_system::SystemDocument| match &d.definitions[&d.root].instances[acceptance::ROBOT].kind {
        sim_system::InstanceKind::Generated { ports, .. } => ports.keys().filter(|p| p.ends_with(".target")).cloned().collect::<Vec<_>>(),
        _ => panic!("not generated"),
    };
    assert_eq!(ports(&before).len(), 2);
    assert_eq!(ports(&after).len(), 1, "{:?}", ports(&after));
    assert_eq!(SystemStore::new(project.system()).load().unwrap(), after, "the refreshed system is saved");
}
