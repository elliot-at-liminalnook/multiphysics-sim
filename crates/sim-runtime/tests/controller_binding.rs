//! The controller binding (`sim.controller-binding/1`), the drive command
//! channels it appends to the seam, and the external-controller session path.
use sim_domain_robot::PhysicalModel;
use sim_runtime::controller_binding::{self, COMMAND_CHANNELS, ControllerBinding, ControllerIdentity, HEARTBEAT_MAX};
use sim_runtime::session::{ControllerProgram, Session};
use std::path::{Path, PathBuf};

const BASELINE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/wheeled-robot/baseline");

fn model() -> PhysicalModel {
    PhysicalModel::load(&format!("{BASELINE}/robot.simrobot.json")).unwrap()
}
fn binding_path() -> PathBuf {
    controller_binding::binding_path_for(&Path::new(BASELINE).join("robot.simrobot.json"))
}

#[test]
fn binding_sits_beside_the_model() {
    assert_eq!(
        controller_binding::binding_path_for(Path::new("/a/b/robot.simrobot.json")),
        PathBuf::from("/a/b/robot.controller.json")
    );
}

#[test]
fn binding_errors_name_file_and_field() {
    let file = Path::new("examples/x/robot.controller.json");
    let ok = r#"{"schema":"sim.controller-binding/1","controller":{"language":"python","script":"c.py","args":[]},"drive_profile":"robot.drive.json"}"#;
    ControllerBinding::from_json(ok, file).unwrap();

    let unknown = r#"{"schema":"sim.controller-binding/1","controller":{"language":"python","script":"c.py"},"drive_profile":"robot.drive.json","gains":1}"#;
    let error = ControllerBinding::from_json(unknown, file).unwrap_err();
    assert!(error.contains("robot.controller.json") && error.contains("gains"), "{error}");

    let nested = r#"{"schema":"sim.controller-binding/1","controller":{"language":"python","script":"c.py","period":1},"drive_profile":"robot.drive.json"}"#;
    let error = ControllerBinding::from_json(nested, file).unwrap_err();
    assert!(error.contains("robot.controller.json") && error.contains("period"), "{error}");

    let newer = r#"{"schema":"sim.controller-binding/2","controller":{"language":"python","script":"c.py"},"drive_profile":"robot.drive.json","future":true}"#;
    let error = ControllerBinding::from_json(newer, file).unwrap_err();
    assert!(error.contains("robot.controller.json: schema: newer schema sim.controller-binding/2"), "{error}");

    let missing = r#"{"controller":{"language":"python","script":"c.py"},"drive_profile":"robot.drive.json"}"#;
    let error = ControllerBinding::from_json(missing, file).unwrap_err();
    assert!(error.contains("robot.controller.json: schema:"), "{error}");

    let lua = r#"{"schema":"sim.controller-binding/1","controller":{"language":"lua","script":"c.lua"},"drive_profile":"robot.drive.json"}"#;
    let error = ControllerBinding::from_json(lua, file).unwrap_err();
    assert!(error.contains("controller.language") && error.contains("lua"), "{error}");

    let flag = r#"{"schema":"sim.controller-binding/1","controller":{"language":"python","script":"c.py","args":["--drive-json","{}"]},"drive_profile":"robot.drive.json"}"#;
    let error = ControllerBinding::from_json(flag, file).unwrap_err();
    assert!(error.contains("controller.args"), "{error}");
}

#[test]
fn example_binding_resolves_the_drive_channels() {
    let model = model();
    let controlled = controller_binding::load(&binding_path(), &model).unwrap();
    let inputs = &controlled.program.inputs;
    let names: Vec<&str> = inputs.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, COMMAND_CHANNELS);
    let units: Vec<&str> = inputs.iter().map(|i| i.kind.unit()).collect();
    assert_eq!(units, ["m/s", "m/s", "rad/s", "1"]);
    // Bounds come from the profile, not from this test.
    let limits = &controlled.resolved.limits;
    for axis in 0..3 {
        let bound = if limits.supported[axis] { limits.max_speed[axis] } else { 0.0 };
        assert_eq!((inputs[axis].lower.abs(), inputs[axis].upper), (bound, bound), "{}", inputs[axis].name);
        assert_eq!(inputs[axis].initial, 0.0);
    }
    assert!(limits.supported[0] && !limits.supported[1] && limits.supported[2], "the rover is differential");
    assert_eq!((inputs[3].lower, inputs[3].upper, inputs[3].initial), (0.0, HEARTBEAT_MAX, 0.0));

    let external = controlled.program.external.as_ref().unwrap();
    assert!(controlled.program.sources.is_empty());
    assert_eq!(external.language, "python");
    assert!(external.script.is_absolute() && external.script.ends_with("clients/python/examples/diff_drive_rover.py"));
    assert!(external.clients_root.ends_with("clients"));
    assert_eq!(external.script_sha256.len(), 64);
    assert_eq!(external.profile_sha256.as_deref(), Some(controlled.identity.profile_sha256.as_str()));
    // The simloop library is hashed into the program and the identity.
    let library = sim_runtime::session::library_sha256(&external.clients_root).unwrap();
    assert_eq!(library.len(), 64);
    assert_eq!(external.library_sha256.as_deref(), Some(library.as_str()));
    assert_eq!(controlled.identity.library_sha256.as_deref(), Some(library.as_str()));
    let n = external.args.len();
    assert_eq!(external.args[n - 2], "--drive-json");
    let drive: serde_json::Value = serde_json::from_str(&external.args[n - 1]).unwrap();
    assert_eq!(drive["schema"], "sim.drive.resolved/1");
    assert_eq!(drive["kinematics"], "differential");

    // A recording's scene gives the same identity back.
    let identity = controller_binding::identity_of(&controlled.program).unwrap().unwrap();
    assert_eq!(identity, controlled.identity);
    assert!(identity.differences(&controlled.identity).is_empty());

    // The driven scene satisfies Session::new's period rule and serializes with
    // its identity; old fields still round-trip.
    let scene = controller_binding::scene(model, &controlled, controller_binding::DRIVE_DURATION_S);
    let steps = scene.period_s / scene.options.step;
    assert!((steps - steps.round()).abs() <= 1e-8, "{steps}");
    let json = serde_json::to_value(&scene).unwrap();
    assert_eq!(json["controller"]["external"]["script_sha256"], external.script_sha256.as_str());
    assert_eq!(json["controller"]["external"]["library_sha256"], library.as_str());
    // The identity survives a scene round trip (as a recording carries it).
    let back: sim_runtime::session::Scene = serde_json::from_value(json).unwrap();
    let identity = controller_binding::identity_of(back.controller.as_ref().unwrap()).unwrap().unwrap();
    assert_eq!(identity, controlled.identity);
    // ...and the identity itself round-trips (as a recording's meta carries it).
    let meta: ControllerIdentity = serde_json::from_value(serde_json::to_value(&controlled.identity).unwrap()).unwrap();
    assert_eq!(meta, controlled.identity);
}

#[test]
fn identity_differences_name_each_field() {
    let recorded = ControllerIdentity {
        script: "/r/clients/python/examples/a.py".into(),
        script_sha256: "aa".into(),
        library_sha256: Some("ll".into()),
        args: vec![],
        profile: "/r/robot.drive.json".into(),
        profile_sha256: "pp".into(),
    };
    let mut current = recorded.clone();
    current.script_sha256 = "bb".into();
    current.library_sha256 = Some("mm".into());
    current.args = vec!["--verbose".into()];
    current.profile_sha256 = "qq".into();
    let differences = recorded.differences(&current);
    assert_eq!(differences.len(), 4, "{differences:?}");
    assert!(differences[0].starts_with("script_sha256: recorded aa, current bb"));
    assert_eq!(differences[1], "library_sha256: recorded ll, current mm");
    assert!(differences[2].starts_with("args:"));
    assert!(differences[3].starts_with("profile_sha256: recorded pp, current qq"));

    // A recording made before the library was hashed parses and is named.
    let old: ControllerIdentity = serde_json::from_value(serde_json::json!({
        "script": "/r/clients/python/examples/a.py", "script_sha256": "aa", "args": [],
        "profile": "/r/robot.drive.json", "profile_sha256": "pp"
    }))
    .unwrap();
    assert_eq!(old.library_sha256, None);
    let mut same = old.clone();
    same.library_sha256 = Some("ll".into());
    assert_eq!(old.differences(&same), ["library_sha256: recorded (none), current ll"]);
}

#[test]
fn controller_program_without_external_still_parses() {
    let program: ControllerProgram = serde_json::from_value(serde_json::json!({
        "sources": {"entry": "c.rhai", "files": {"c.rhai": "fn control(t,s,c,state){ #{commands:c,state:state} }"}},
        "inputs": []
    }))
    .unwrap();
    assert!(program.external.is_none());
    program.validate().unwrap();
    assert!(serde_json::to_value(&program).unwrap().get("external").is_none(), "old scenes serialize unchanged");

    let external: ControllerProgram = serde_json::from_value(serde_json::json!({
        "external": {"language": "python", "script": "/x/clients/python/c.py", "script_sha256": "00", "args": [], "clients_root": "/x/clients"}
    }))
    .unwrap();
    assert!(external.sources.is_empty());
    external.validate().unwrap();

    let both: ControllerProgram = serde_json::from_value(serde_json::json!({
        "sources": {"entry": "c.rhai", "files": {"c.rhai": ""}},
        "external": {"language": "python", "script": "/x/clients/python/c.py", "script_sha256": "00", "clients_root": "/x/clients"}
    }))
    .unwrap();
    assert!(both.validate().unwrap_err().contains("both"));

    let neither: ControllerProgram = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(neither.validate().unwrap_err().contains("neither"));

    let ruby: ControllerProgram = serde_json::from_value(serde_json::json!({
        "external": {"language": "ruby", "script": "/x/c.rb", "script_sha256": "00", "clients_root": "/x/clients"}
    }))
    .unwrap();
    assert!(ruby.validate().unwrap_err().contains("`ruby`"));

    let unknown = serde_json::from_value::<ControllerProgram>(serde_json::json!({
        "external": {"language": "python", "script": "/x/c.py", "script_sha256": "00", "clients_root": "/x/clients", "env": {}}
    }));
    assert!(unknown.unwrap_err().to_string().contains("env"));
}

#[test]
fn changed_script_is_refused_by_name() {
    let model = model();
    let mut controlled = controller_binding::load(&binding_path(), &model).unwrap();
    let external = controlled.program.external.as_mut().unwrap();
    let on_disk = external.script_sha256.clone();
    external.script_sha256 = "0".repeat(64);
    let script = external.script.display().to_string();
    let scene = controller_binding::scene(model, &controlled, 1.0);
    let error = Session::new(scene, 0).err().expect("a changed script is refused");
    assert_eq!(
        error,
        format!("controller `{script}` changed: sha256 on disk {on_disk}, recorded {}", "0".repeat(64))
    );
}

#[test]
fn changed_library_is_refused_by_name() {
    let model = model();
    let mut controlled = controller_binding::load(&binding_path(), &model).unwrap();
    let external = controlled.program.external.as_mut().unwrap();
    let on_disk = external.library_sha256.clone().unwrap();
    external.library_sha256 = Some("0".repeat(64));
    let library = external.clients_root.join(sim_runtime::session::SIMLOOP_LIBRARY).display().to_string();
    let scene = controller_binding::scene(model, &controlled, 1.0);
    // Refused before the plant is built or python3 is started.
    let error = Session::new(scene, 0).err().expect("a changed library is refused");
    assert_eq!(
        error,
        format!("controller library `{library}` changed: sha256 on disk {on_disk}, recorded {}", "0".repeat(64))
    );
}

#[test]
fn library_hash_is_the_documented_stream() {
    let root = std::env::temp_dir().join(format!("sim-runtime-library-{}", std::process::id()));
    let library = root.join(sim_runtime::session::SIMLOOP_LIBRARY);
    std::fs::create_dir_all(library.join("sub")).unwrap();
    std::fs::write(library.join("b.py"), b"two").unwrap();
    std::fs::write(library.join("sub/a.py"), b"one").unwrap();
    std::fs::write(library.join("notes.txt"), b"ignored").unwrap();
    let got = sim_runtime::session::library_sha256(&root);
    let mut stream = Vec::new();
    for (name, bytes) in [("b.py", &b"two"[..]), ("sub/a.py", &b"one"[..])] {
        stream.extend_from_slice(name.as_bytes());
        stream.push(0);
        stream.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        stream.extend_from_slice(bytes);
    }
    let expected = sim_domain_control::drive::profile::sha256_hex(&stream);
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(got.unwrap(), expected);
}

#[test]
fn embedded_sessions_refuse_external_controllers() {
    let model = model();
    let controlled = controller_binding::load(&binding_path(), &model).unwrap();
    let scene = controller_binding::scene(model, &controlled, 1.0);
    let config: sim_runtime::embedded::Config = serde_json::from_value(serde_json::json!({
        "step_s": 5e-4, "steps": 1, "report_every": 1, "applied_generalized_loads": []
    }))
    .unwrap();
    // Refused before anything else runs: no process is started.
    let error = sim_runtime::embedded::EmbeddedSession::new(scene, config, 0, sim_runtime::embedded::CaptureMode::Latest)
        .err()
        .expect("refused");
    assert!(error.contains("refusing external controller (python)") && error.contains("diff_drive_rover.py"), "{error}");
}

/// Spawns `python3`; run with `cargo test -p sim-runtime --test controller_binding -- --ignored`.
#[test]
#[ignore = "spawns python3 and the example controller"]
fn example_controller_drives_the_wheels_forward() {
    let model = model();
    let controlled = controller_binding::load(&binding_path(), &model).unwrap();
    let scene = controller_binding::scene(model, &controlled, 1.0);
    let mut session = Session::new(scene, 7).unwrap();
    let wheels = &controlled.resolved.geometry.wheels;
    let index = |joint: &str| {
        session.contract.actuators.iter().position(|a| a.name == format!("{joint}.target")).unwrap()
    };
    let indices: Vec<(usize, f64)> = wheels.iter().map(|w| (index(&w.joint), w.sign)).collect();
    let forward = controlled.resolved.limits.max_speed[0] * 0.5;
    let mut frame = None;
    for heartbeat in 1..=10 {
        frame = Some(session.step(&[forward, 0.0, 0.0, heartbeat as f64]).unwrap());
    }
    let frame = frame.unwrap();
    assert!(frame.error.is_none());
    for (k, sign) in indices {
        let target = frame.telemetry.actuators[k];
        assert!(target.is_finite() && sign * target > 0.0, "wheel target {k} = {target} rolls the body forward");
    }
    // Replay rebuilds the same episode from the recording.
    let replayed = Session::replay(session.recording()).unwrap();
    assert_eq!(replayed.frame().telemetry.actuators, frame.telemetry.actuators);
}
