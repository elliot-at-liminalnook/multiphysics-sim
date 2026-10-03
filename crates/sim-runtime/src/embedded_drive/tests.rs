//! The embedded drive built from the committed wheeled-robot files (model,
//! binding, drive profile, Rhai adapter and embedded config, all included
//! as text as the browser fetches them), the adapter's behaviour through
//! the Rhai controller alone, and one driven period of the session with its
//! replay. Written by reading; not yet executed.
use super::*;
use crate::controller_binding::{COMMAND_CHANNELS, DRIVE_DURATION_S, HEARTBEAT_MAX, drive_inputs};
use sim_core::{Channel, Contract, Coupler, QuantityKind};
use sim_domain_control::drive::geometry::Provenance;
use sim_domain_control::drive::kinematics::BodyTwist;

const MODEL: &str = include_str!("../../../../examples/wheeled-robot/baseline/robot.simrobot.json");
const BINDING: &str = include_str!("../../../../examples/wheeled-robot/baseline/robot.controller.json");
const PROFILE: &str = include_str!("../../../../examples/wheeled-robot/baseline/robot.drive.json");
const ADAPTER: &str = include_str!("../../../../examples/wheeled-robot/drive-adapter.rhai");
const CONFIG: &str = include_str!("../../../../examples/wheeled-robot/drive-adapter.config.json");
const MODEL_PATH: &str = "examples/wheeled-robot/baseline/robot.simrobot.json";
const BINDING_PATH: &str = "examples/wheeled-robot/baseline/robot.controller.json";
const PERIOD: f64 = 0.02;

fn files() -> BTreeMap<String, String> {
    [("robot.drive.json", PROFILE), ("../drive-adapter.rhai", ADAPTER), ("../drive-adapter.config.json", CONFIG)]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}
fn built() -> EmbeddedDrive {
    build(MODEL, MODEL_PATH, Path::new(BINDING_PATH), BINDING, &files(), DRIVE_DURATION_S).unwrap()
}

#[test]
fn the_binding_lists_the_files_its_embedded_program_needs() {
    let binding = ControllerBinding::from_json(BINDING, Path::new(BINDING_PATH)).unwrap();
    assert_eq!(files_to_read(&binding, Path::new(BINDING_PATH)).unwrap(), vec!["robot.drive.json", "../drive-adapter.rhai", "../drive-adapter.config.json"]);
    // Without `embedded` the refusal names the field and the file.
    let mut value: Value = serde_json::from_str(BINDING).unwrap();
    value.as_object_mut().unwrap().remove("embedded");
    let older = ControllerBinding::from_json(&value.to_string(), Path::new(BINDING_PATH)).unwrap();
    let e = files_to_read(&older, Path::new(BINDING_PATH)).unwrap_err();
    assert!(e.starts_with(&format!("{BINDING_PATH}: embedded: absent")), "{e}");
    let e = build(MODEL, MODEL_PATH, Path::new(BINDING_PATH), &value.to_string(), &files(), DRIVE_DURATION_S).unwrap_err();
    assert!(e.contains("embedded: absent"), "{e}");
}

#[test]
fn the_scene_is_built_from_the_model_binding_and_profile() {
    let drive = built();
    assert_eq!(drive.schema, EMBEDDED_DRIVE_SCHEMA);
    // The resolved drive: profile limits, geometry derived from the CAD model.
    assert_eq!(drive.resolved.kinematics, "differential");
    assert_eq!(drive.resolved.geometry.joints(), vec!["left axle", "right axle"]);
    assert!(matches!(drive.resolved.geometry.track_width_m.provenance, Provenance::Derived { .. }), "{:?}", drive.resolved.geometry.track_width_m);
    assert!(matches!(drive.resolved.geometry.wheel_radius_m.provenance, Provenance::Derived { .. }));
    // The command inputs: the four channels, bounded by the profile's max speeds.
    let program = drive.scene.controller.as_ref().unwrap();
    assert_eq!(serde_json::to_value(&program.inputs).unwrap(), serde_json::to_value(drive_inputs(&drive.resolved)).unwrap());
    let names: Vec<&str> = program.inputs.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, COMMAND_CHANNELS);
    let bounds: Vec<(f64, f64)> = program.inputs.iter().map(|c| (c.lower, c.upper)).collect();
    assert_eq!(bounds, vec![(-0.26, 0.26), (0.0, 0.0), (-4.3, 4.3), (0.0, HEARTBEAT_MAX)]);
    assert!(program.external.is_none());
    // The adapter's sources, keyed by the entry's file name.
    assert_eq!(program.sources.entry, "drive-adapter.rhai");
    assert_eq!(program.sources.files.keys().collect::<Vec<_>>(), vec!["drive-adapter.rhai"]);
    assert_eq!(program.sources.files["drive-adapter.rhai"], ADAPTER);
    // Parameters: the period, the resolved drive, the initial targets and the identity.
    assert_eq!(program.parameters["period_s"], serde_json::json!(PERIOD));
    assert_eq!(program.parameters[DRIVE_PARAMETER], serde_json::to_value(&drive.resolved).unwrap());
    assert_eq!(program.parameters["initial_targets"], serde_json::json!({"left axle": 0.0, "right axle": 0.0}));
    assert_eq!(program.parameters[IDENTITY_PARAMETER], serde_json::to_value(&drive.identity).unwrap());
    // The scene: the model's control period, the drive benchmark's build options.
    assert_eq!((drive.scene.period_s, drive.scene.duration_s), (PERIOD, DRIVE_DURATION_S));
    assert!(drive.scene.options.contact && !drive.scene.options.flex);
    // The config: horizon, stride and CAD hash derived here.
    let cad = "9cd32aa97d620cf7e342fe994eccd5904cd0a2b235e60092e877d516d8a17a7a";
    assert_eq!(drive.config.report_every, 160);
    assert_eq!(drive.config.steps, 30_000 * 160);
    assert_eq!(drive.config.motors.as_ref().unwrap().expected_cad_sha256.as_deref(), Some(cad));
    assert!(drive.fidelity.contains("drive-adapter.rhai") && drive.fidelity.contains("diff_drive_rover.py") && drive.fidelity.contains("Uncalibrated"), "{}", drive.fidelity);
    // The identity a replay is checked against.
    let id = &drive.identity;
    assert_eq!(id.binding, BINDING_PATH);
    assert_eq!(id.entry, "drive-adapter.rhai");
    assert_eq!(id.script_sha256, sources_sha256(&program.sources));
    assert_eq!(id.config_sha256, sha256_hex(CONFIG.as_bytes()));
    assert_eq!(id.profile, "examples/wheeled-robot/baseline/robot.drive.json");
    assert_eq!(id.profile_sha256, sha256_hex(PROFILE.as_bytes()));
    assert_eq!(drive.resolved.profile_sha256, id.profile_sha256);
    assert_eq!(id.model_sha256, sha256_hex(MODEL.as_bytes()));
    assert_eq!(id.cad_sha256.as_deref(), Some(cad));
    assert!(id.differences(id).is_empty());
    let mut other = id.clone();
    other.script_sha256 = "x".into();
    other.cad_sha256 = None;
    assert_eq!(other.differences(id), vec![format!("script_sha256: recorded x, current {}", id.script_sha256), format!("cad_sha256: recorded (none), current {cad}")]);
    // Paths are recorded information, not refusal reasons (they differ between hosts and URL layouts).
    let mut moved = id.clone();
    moved.binding = "/srv/rover/robot.controller.json".into();
    moved.profile = "/srv/rover/robot.drive.json".into();
    assert!(moved.differences(id).is_empty());
    // The fidelity label says the servo boundaries are the model's values.
    assert!(drive.fidelity.contains("imposed at the model's values"), "{}", drive.fidelity);
    // The sources hash is over keys and texts.
    let one = sim_script::Sources::single("a.rhai", "x");
    assert_ne!(sources_sha256(&one), sources_sha256(&sim_script::Sources::single("b.rhai", "x")));
    assert_eq!(sources_sha256(&one), sha256_hex(b"a.rhai\0x\0"));
}

#[test]
fn the_builder_refuses_by_file_and_field() {
    let refuse = |files: BTreeMap<String, String>| build(MODEL, MODEL_PATH, Path::new(BINDING_PATH), BINDING, &files, DRIVE_DURATION_S).unwrap_err();
    let mut missing = files();
    missing.remove("../drive-adapter.rhai");
    let e = refuse(missing);
    assert!(e.starts_with(&format!("{BINDING_PATH}: embedded.entry: `../drive-adapter.rhai` was not supplied")), "{e}");
    let edit = |f: &dyn Fn(&mut Value)| {
        let mut config: Value = serde_json::from_str(CONFIG).unwrap();
        f(&mut config);
        let mut all = files();
        all.insert("../drive-adapter.config.json".into(), config.to_string());
        refuse(all)
    };
    let e = edit(&|c| c["step_s"] = serde_json::json!(0.003));
    assert!(e.starts_with("examples/wheeled-robot/baseline/../drive-adapter.config.json: step_s:"), "{e}");
    let e = edit(&|c| c["motors"]["expected_cad_sha256"] = serde_json::json!("abc"));
    assert!(e.contains("motors.expected_cad_sha256: abc is not the model's source.cad_sha256"), "{e}");
    // The committed ±12000 rad covers 600 s at the largest wheel rate:
    // (0.26 m/s + 4.3 rad/s × 0.06 m) / 0.03 m = 17.27 rad/s × 600 s = 10360 rad.
    let e = edit(&|c| c["policy"]["target_bounds_rad"] = serde_json::json!({"joint.left axle": [-200, 200], "joint.right axle": [-200, 200]}));
    assert!(e.contains("policy.target_bounds_rad.joint.left axle: [-200, 200] rad is narrower than the session needs") && e.contains("× the 600 s session"), "{e}");
    let drive = built();
    let rate = super::max_wheel_rate(&drive.resolved).unwrap();
    let mixed = drive.resolved.geometry.differential().unwrap().mix(BodyTwist::new(0.26, 0.0, 4.3)).unwrap();
    assert!((rate - mixed[0].abs().max(mixed[1].abs())).abs() < 1e-12 && rate > 17.0 && rate * DRIVE_DURATION_S < 12_000.0, "{rate}");
    // Servo boundaries are the model's values, refused naming both.
    let e = edit(&|c| c["motors"]["servos"][1]["supply_voltage_v"] = serde_json::json!(7.4));
    assert!(e.contains("motors.servos[1].supply_voltage_v: 7.4 V, but") && e.contains("motors[1] (right drive).electrical.supply_voltage is 6 V"), "{e}");
    let e = edit(&|c| c["motors"]["servos"][0]["winding_temperature_k"] = serde_json::json!(300.0));
    assert!(e.contains("motors.servos[0].winding_temperature_k: 300 K, but") && e.contains("motors[0] (left drive).thermal.ambient_c is 25 °C"), "{e}");
    let e = edit(&|c| {
        c.as_object_mut().unwrap().remove("policy");
    });
    assert!(e.contains(": policy: absent"), "{e}");
    let e = edit(&|c| c["policy"]["target_bounds_rad"] = serde_json::json!({"joint.left axle": [-1, 1]}));
    assert!(e.contains("policy.target_bounds_rad"), "{e}");
    // A further file outside the entry's directory is refused naming it.
    let mut binding: Value = serde_json::from_str(BINDING).unwrap();
    binding["embedded"]["files"] = serde_json::json!(["../../elsewhere/lib.rhai"]);
    let mut all = files();
    all.insert("../../elsewhere/lib.rhai".into(), "fn f() { 1 }".into());
    let e = build(MODEL, MODEL_PATH, Path::new(BINDING_PATH), &binding.to_string(), &all, DRIVE_DURATION_S).unwrap_err();
    assert!(e.contains("embedded.files[0]: `../../elsewhere/lib.rhai` is not inside the entry's directory `..`"), "{e}");
    // One inside it is captured by its path relative to the entry.
    binding["embedded"]["files"] = serde_json::json!(["../lib/util.rhai"]);
    all.insert("../lib/util.rhai".into(), "fn f() { 1 }".into());
    let drive = build(MODEL, MODEL_PATH, Path::new(BINDING_PATH), &binding.to_string(), &all, DRIVE_DURATION_S).unwrap();
    let sources = &drive.scene.controller.as_ref().unwrap().sources;
    assert_eq!(sources.files.keys().collect::<Vec<_>>(), vec!["drive-adapter.rhai", "lib/util.rhai"]);
}

/// The adapter alone, on the contract the embedded policy gives it (the
/// four command channels and the wheel targets): it mixes through the Rust
/// mixer, integrates once per sample time, and its deadman ramps a stale request.
#[test]
fn the_adapter_mixes_through_the_shared_drive_functions() {
    let drive = built();
    let program = drive.scene.controller.clone().unwrap();
    let mut c = sim_script::RhaiController::new(program.sources.clone(), sim_script::parameter_map(&program.parameters).unwrap()).unwrap();
    let channel = |name: &str, kind: QuantityKind| Channel { name: name.into(), kind };
    c.open(&Contract {
        element: "embedded.policy".into(),
        period: PERIOD,
        sensors: vec![
            channel("command.forward", QuantityKind::LinearVelocity),
            channel("command.lateral", QuantityKind::LinearVelocity),
            channel("command.yaw", QuantityKind::AngularVelocity),
            channel("command.heartbeat", QuantityKind::Dimensionless),
        ],
        actuators: vec![channel("left axle.target", QuantityKind::Angle), channel("right axle.target", QuantityKind::Angle)],
    })
    .unwrap();
    let mixer = drive.resolved.geometry.differential().unwrap();
    let rate = mixer.mix(BodyTwist::new(0.26, 0.0, 0.0)).unwrap();
    let close = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12;

    // t = 0, heartbeat 1, forward 0.26 m/s: one period of the mixed rate.
    let mut out = [0.0; 2];
    c.sample(0.0, &[0.26, 0.0, 0.0, 1.0], &mut out).unwrap();
    assert!(close(out, [PERIOD * rate[0], PERIOD * rate[1]]), "{out:?} vs rate {rate:?}");
    // The same time sampled again (a retried slice) does not integrate twice.
    let first = out;
    c.sample(0.0, &[0.26, 0.0, 0.0, 1.0], &mut out).unwrap();
    assert_eq!(out, first);
    // Live samples keep integrating while the heartbeat is fresh (age < 0.5 s).
    for k in 1..=24 {
        c.sample(k as f64 * PERIOD, &[0.26, 0.0, 0.0, 1.0], &mut out).unwrap();
    }
    let n = 25.0 * PERIOD;
    assert!(close(out, [n * rate[0], n * rate[1]]), "{out:?}");
    // 0.6 s after the heartbeat last rose: expired, the twist ramps from 0.26 at stop_decel (1.0 m/s^2).
    let before = out;
    c.sample(0.6, &[0.26, 0.0, 0.0, 1.0], &mut out).unwrap();
    let ramped = mixer.mix(BodyTwist::new(0.26 - 1.0 * PERIOD, 0.0, 0.0)).unwrap();
    assert!(close(out, [before[0] + PERIOD * ramped[0], before[1] + PERIOD * ramped[1]]), "{out:?}");
    let state = c.state_json().unwrap();
    assert_eq!(state["deadman"]["expired"], serde_json::json!(true), "{state}");
    assert!(state["history"].as_array().unwrap().len() <= 16);
}

/// One driven period through the session (contact physics: slower than the
/// pure tests), its recording, and the replay of that recording.
#[test]
fn a_driven_period_is_recorded_and_replays() {
    let drive = built();
    // The worker receives the drive as JSON.
    let drive: EmbeddedDrive = serde_json::from_str(&serde_json::to_string(&drive).unwrap()).unwrap();
    let mut s = DriveSession::new(drive, 7).unwrap();
    assert_eq!(s.stride(), 160);
    let status = s.request(&DriveRequest::Axes { forward: 1.0, lateral: 0.0, yaw: 0.0 }).unwrap();
    assert_eq!((status.heartbeat, status.time_s), (1, 0.0));
    assert!(s.request(&DriveRequest::Axes { forward: 0.0, lateral: 1.0, yaw: 0.0 }).unwrap_err().contains("lateral"));
    s.advance(1).unwrap();
    assert!((s.time() - PERIOD).abs() < 1e-12, "{}", s.time());
    // The limiter on sim time: one period at max_accel (0.5 m/s^2).
    let status = s.status();
    assert!((status.commanded.forward_m_s - 0.5 * PERIOD).abs() < 1e-12 && !status.expired, "{status:?}");
    assert_eq!(s.session().input_values(), &[0.5 * PERIOD, 0.0, 0.0, 1.0]);
    let recording = s.recording();
    assert_eq!(recording.completed_steps, 160);
    assert_eq!(recording.input_events.len(), 1);
    assert_eq!((recording.input_events[0].at_step, recording.input_events[0].values.clone()), (0, vec![0.5 * PERIOD, 0.0, 0.0, 1.0]));

    // A recording from another drive program is refused naming the field.
    let mut changed = recording.clone();
    changed.scene.controller.as_mut().unwrap().parameters[IDENTITY_PARAMETER]["script_sha256"] = serde_json::json!("x");
    let e = s.prepare_replay(changed).unwrap_err();
    assert!(e.contains("script_sha256: recorded x, current "), "{e}");
    let mut foreign = recording.clone();
    foreign.scene.controller.as_mut().unwrap().parameters.as_object_mut().unwrap().remove(IDENTITY_PARAMETER);
    assert!(s.prepare_replay(foreign).unwrap_err().starts_with("not an embedded drive recording"));

    // Its own recording replays: requests are refused until it ends, then the deadman counts as expired.
    assert_eq!(s.prepare_replay(recording).unwrap(), 1);
    assert!(s.replaying());
    assert!(s.request(&DriveRequest::Stop).unwrap_err().contains("replaying a recording"));
    s.advance(1).unwrap();
    assert!(!s.replaying());
    let status = s.status();
    assert!(status.expired && (status.commanded.forward_m_s - 0.5 * PERIOD).abs() < 1e-12 && status.heartbeat == 1, "{status:?}");
    // Live again: a fresh request is accepted.
    assert_eq!(s.request(&DriveRequest::Stop).unwrap().heartbeat, 2);
}

/// At the horizon requests and further periods are refused by name; the
/// session runs exactly its whole periods (here one, a 0.02 s drive).
#[test]
fn the_horizon_is_refused_by_name() {
    let drive = build(MODEL, MODEL_PATH, Path::new(BINDING_PATH), BINDING, &files(), PERIOD).unwrap();
    assert_eq!(drive.config.steps, 160);
    let mut s = DriveSession::new(drive, 1).unwrap();
    s.request(&DriveRequest::Axes { forward: 1.0, lateral: 0.0, yaw: 0.0 }).unwrap();
    s.advance(5).unwrap();
    assert_eq!(s.session().remaining_steps(), 0);
    let e = s.advance(1).unwrap_err();
    assert_eq!(e, "the drive's 0.02 s horizon is reached; reload to start a new session");
    assert_eq!(s.request(&DriveRequest::Stop).unwrap_err(), e);
    assert_eq!(s.frame().unwrap()["done"], serde_json::json!(true));
}
