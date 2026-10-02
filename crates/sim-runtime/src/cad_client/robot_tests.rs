//! The robot client against the in-process fake RoboCAD of `tests.rs`:
//! answers as `api.py`/`robotics.robot_summary`/`node_detail` write them
//! (json.dumps' separators; shapes captured from a headless RoboCAD with a
//! ground block, a thigh, an MG996R on a revolute hip, an IMU and a cable),
//! the exact request line and body of every write, tolerant reads and
//! RoboCAD's original errors alongside outcome-uncertainty hints.
use super::tests::{Answer, assert_request, ok, serve};
use super::*;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// `GET /robot` of the captured document, with a `NaN` upper limit, a
/// malformed joint, an unknown field and an issue added.
const ROBOT: &str = r#"{"joints": [{"id": "eadb58c25180", "name": "hip", "type": "revolute", "parent": "c99a12634995", "child": "1e3a27930924", "pivot": [0.0, 0.0, 200.0], "axis": [0.0, 1.0, 0.0], "lower": -1.5, "upper": NaN, "motor": "945518089db5", "gear_ratio": 2.0, "damping": 0.0, "friction": 0.0, "home": 0.0, "stroke": 0.0, "parent_name": "ground", "child_name": "thigh", "motor_name": "hip motor"}, "not a joint", {"id": "j2", "name": "knee", "type": "revolute", "parent": null, "child": "s1", "pivot": [0.0, 0.0, 90.0], "future": 1}], "motors": [{"id": "945518089db5", "name": "hip motor", "kind": "motor", "spec": "mg996r", "mount_point": [0.0, -20.0, 200.0], "shaft_axis": [0.0, 1.0, 0.0], "shaft_tip": [0.0, -13.5, 200.0], "rotation_deg": 0.0, "mounted_on": "c99a12634995", "drives": "eadb58c25180", "spec_name": "MG996R servo"}], "links": 3, "dof": 2, "has_closed_loops": false, "ground": ["c99a12634995"], "issues": [{"severity": "info", "message": "knee: no motor assigned (passive joint)", "node": "j2"}], "validation_scope": "topology only; exact geometry checks are explicit"}"#;

/// `GET /nodes/{id}` of the captured IMU (mass omitted: a sensor has no body).
const IMU: &str = r#"{"id": "ac6ce2812807", "kind": "sensor", "name": "imu", "parent": null, "children": [], "visible": true, "locked": false, "disabled": false, "material": null, "color": null, "pivot": null, "source": null, "transform": {"translation": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "angle_deg": 0.0, "scale": 1.0}, "effective_visible": true, "component_instance": null, "component_member": null, "robot": {"kind": "imu", "body": "1e3a27930924", "point": [0.0, 0.0, 100.0], "axes": null, "joint": null, "joint_name": null, "rate_hz": 500.0}}"#;

/// The captured cable, with a 150 mm length (RoboCAD stores metres).
const CABLE: &str = r#"{"id": "db3882517a95", "kind": "cable", "name": "lead", "parent": null, "children": [], "visible": true, "locked": false, "disabled": false, "material": null, "color": null, "pivot": null, "source": null, "transform": {"translation": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "angle_deg": 0.0, "scale": 1.0}, "effective_visible": true, "component_instance": null, "component_member": null, "robot": {"kind": "cable", "from_body": "c99a12634995", "from_point": [0.0, -20.0, 190.0], "to_body": "1e3a27930924", "to_point": [0.0, -5.0, 100.0], "length": 0.15, "mass": null, "stiffness": null, "damping": null, "segments": 4}}"#;

/// `robot_settings` after `set_battery(cells=2)` and `set_control(targets={"hip": 0.2})`.
const SETTINGS: &str = r#"{"battery": {"cells": 2, "chemistry": "lipo", "nominal_voltage": 7.4, "internal_resistance": 0.04, "capacity_ah": 1.0, "initial_soc": 1.0, "cutoff_voltage": 6.0}, "control": {"period_s": 0.02, "latency_s": 0.004, "targets": {"hip": 0.2}, "mode": "hold", "trajectory": []}}"#;

/// `MotorSpec("sg90", …).to_json()` as `GET /motors` writes it.
const SG90: &str = r#"{"id": "sg90", "name": "SG90 micro servo", "kind": "servo", "shape": "box", "size": [22.8, 12.2, 22.5], "shaft_diameter": 4.8, "shaft_length": 3.5, "mass_g": 9.0, "stall_torque": 0.18, "no_load_speed": 10.471975511965976, "gear_ratio": 1.0, "voltage": 5.0, "rotor_inertia": 0.0, "mount_holes": [[0.0, 13.9, 2.0], [0.0, -13.9, 2.0]], "flange": [5.9, 2.0], "stroke": 0.0, "color": [0.3, 0.55, 0.85], "notes": "180° hobby servo; spline output; tabs at ±13.9 mm on the long axis"}"#;

fn op_answer(result: &str, undo: &str) -> Answer {
    ok(&format!(r#"{{"result": {result}, "history": {{"undo": [{undo}], "redo": []}}}}"#))
}

#[test]
fn robot_reads_the_summary_tolerantly() {
    let (c, server) = serve(vec![ok(ROBOT)]);
    let port = c.endpoint.port;
    let r = c.robot().unwrap();
    assert_eq!(r.joints.len(), 2, "the string is dropped, not the summary");
    let hip = &r.joints[0];
    assert_eq!((hip.id.as_str(), hip.name.as_str(), hip.kind.as_str()), ("eadb58c25180", "hip", "revolute"));
    assert_eq!((hip.parent.as_deref(), hip.child.as_str()), (Some("c99a12634995"), "1e3a27930924"));
    assert_eq!((hip.pivot, hip.axis), ([0.0, 0.0, 200.0], [0.0, 1.0, 0.0]));
    assert_eq!((hip.lower, hip.upper), (Some(-1.5), None), "NaN reads as None");
    assert_eq!((hip.motor.as_deref(), hip.gear_ratio, hip.motor_name.as_deref()), (Some("945518089db5"), 2.0, Some("hip motor")));
    // Missing fields take Joint's dataclass defaults; the unknown one is ignored.
    let knee = &r.joints[1];
    assert_eq!((knee.parent.as_deref(), knee.axis, knee.gear_ratio, knee.lower), (None, [0.0, 0.0, 1.0], 1.0, None));
    let m = &r.motors[0];
    assert_eq!((m.spec.as_deref(), m.spec_name.as_deref(), m.drives.as_deref()), (Some("mg996r"), Some("MG996R servo"), Some("eadb58c25180")));
    assert_eq!((m.mount_point, m.shaft_axis, m.shaft_tip), (Some([0.0, -20.0, 200.0]), Some([0.0, 1.0, 0.0]), Some([0.0, -13.5, 200.0])));
    assert_eq!((r.links, r.dof, r.has_closed_loops), (3, Some(2), false));
    assert_eq!(r.ground, vec!["c99a12634995".to_string()]);
    assert_eq!(r.issues, vec![RobotIssue { severity: "info".into(), message: "knee: no motor assigned (passive joint)".into(), node: Some("j2".into()) }]);
    assert!(r.validation_scope.starts_with("topology only"));
    // The wire name of `kind` is Python's `type`.
    assert_eq!(serde_json::to_value(hip).unwrap()["type"], json!("revolute"));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /robot HTTP/1.1", port, None);
}

#[test]
fn robot_exact_runs_the_op_with_exact_and_names_the_route_when_unreadable() {
    let (c, server) = serve(vec![ok(&format!(r#"{{"result": {ROBOT}, "history": {{"undo": [], "redo": []}}}}"#)), op_answer("\"nope\"", "")]);
    let port = c.endpoint.port;
    assert_eq!(c.robot_exact().unwrap().joints[0].name, "hip");
    let e = c.robot_exact().unwrap_err();
    assert_eq!((e.method, e.route.as_str(), e.status), ("POST", "/ops/robot", None));
    assert!(e.message.starts_with("unexpected answer: "), "{e}");
    let seen = server.join().unwrap();
    for s in &seen {
        assert_request(s, "POST /ops/robot HTTP/1.1", port, Some(r#"{"args":[],"kwargs":{"exact":true}}"#));
    }
}

#[test]
fn motors_read_the_library_and_drop_a_malformed_entry() {
    let body = format!(r#"{{"sg90": {SG90}, "broken": {{"id": "broken", "size": "big"}}, "bare": {{"id": "bare", "name": "Bare"}}}}"#);
    let (c, server) = serve(vec![ok(&body), ok("[]")]);
    let port = c.endpoint.port;
    let lib = c.motors().unwrap();
    assert_eq!(lib.keys().collect::<Vec<_>>(), ["bare", "sg90"]);
    let sg = &lib["sg90"];
    assert_eq!((sg.name.as_str(), sg.kind.as_str(), sg.shape.as_str()), ("SG90 micro servo", "servo", "box"));
    assert_eq!((sg.size, sg.mass_g, sg.stall_torque, sg.no_load_speed), ([22.8, 12.2, 22.5], 9.0, 0.18, 10.471975511965976));
    assert_eq!((sg.mount_holes.clone(), sg.flange), (vec![[0.0, 13.9, 2.0], [0.0, -13.9, 2.0]], Some([5.9, 2.0])));
    assert_eq!(sg.notes, "180° hobby servo; spline output; tabs at ±13.9 mm on the long axis");
    // Missing fields: MotorSpec's dataclass defaults.
    let bare = &lib["bare"];
    assert_eq!((bare.gear_ratio, bare.voltage, bare.color, bare.flange), (1.0, 5.0, [0.25, 0.27, 0.31], None));
    let e = c.motors().unwrap_err();
    assert!(e.message.starts_with("unexpected answer: expected the motor library"), "{e}");
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /motors HTTP/1.1", port, None);
}

#[test]
fn sensors_and_cables_list_add_and_read_their_metadata() {
    let (c, server) = serve(vec![ok(&format!("[{IMU}, 7]")), Answer::Json(201, IMU.into()), ok(&format!("[{CABLE}]")), Answer::Json(201, CABLE.into())]);
    let port = c.endpoint.port;
    let sensors = c.sensors().unwrap();
    assert_eq!(sensors.len(), 1, "the number is dropped");
    let imu = SensorMeta::of(&sensors[0]).unwrap();
    assert_eq!(imu, SensorMeta { kind: "imu".into(), body: "1e3a27930924".into(), point: [0.0, 0.0, 100.0], axes: None, joint: None, joint_name: None, rate_hz: Some(500.0) });
    assert_eq!(CableMeta::of(&sensors[0]), None, "a sensor is not a cable");
    let request = SensorRequest { kind: "imu".into(), body: "1e3a27930924".into(), point: [0.0, 0.0, 100.0], name: Some("imu".into()), rate_hz: Some(500.0), ..SensorRequest::default() };
    assert_eq!(c.add_sensor(&request).unwrap().summary.id, "ac6ce2812807");
    let cable = CableMeta::of(&c.cables().unwrap()[0]).unwrap();
    assert_eq!((cable.length, cable.mass, cable.segments), (Some(0.15), None, 4), "metres as stored");
    assert_eq!((cable.from_point, cable.to_body.as_str()), ([0.0, -20.0, 190.0], "1e3a27930924"));
    let request = CableRequest { from_body: "c99a12634995".into(), from_point: [0.0, -20.0, 190.0], to_body: "1e3a27930924".into(), to_point: [0.0, -5.0, 100.0], length: Some(150.0), name: Some("lead".into()), ..CableRequest::default() };
    assert_eq!(c.add_cable(&request).unwrap().summary.kind, "cable");
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /sensors HTTP/1.1", port, None);
    assert_request(&seen[1], "POST /sensors HTTP/1.1", port, Some(r#"{"kind":"imu","body":"1e3a27930924","point":[0.0,0.0,100.0],"name":"imu","rate_hz":500.0}"#));
    assert_request(&seen[2], "GET /cables HTTP/1.1", port, None);
    assert_request(&seen[3], "POST /cables HTTP/1.1", port, Some(r#"{"from_body":"c99a12634995","from_point":[0.0,-20.0,190.0],"to_body":"1e3a27930924","to_point":[0.0,-5.0,100.0],"length":150.0,"name":"lead"}"#));
}

#[test]
fn settings_read_null_as_none_and_write_only_the_given_keys() {
    let (c, server) = serve(vec![
        ok("null"),
        ok(SETTINGS),
        ok(r#"{"period_s": 0.02, "latency_s": 0.004, "targets": {"hip": 0.2, "knee": NaN}, "mode": "hold", "trajectory": []}"#),
        ok(SETTINGS),
        ok(r#"{"dimension_m": {"sigma": 0.00015}, "mass": {"sigma_fraction": 0.05}, "seed": 0}"#),
        ok(SETTINGS),
        ok("null"),
        ok(r#"{"hip motor": {"family": "mg996r"}}"#),
    ]);
    let port = c.endpoint.port;
    assert_eq!(c.battery().unwrap(), None);
    let battery = BatteryRequest { cells: 3, chemistry: "liion".into(), capacity_ah: 2.0, ..BatteryRequest::default() };
    let settings = c.set_battery(&battery).unwrap();
    assert_eq!(serde_json::from_value::<Battery>(settings["battery"].clone()).unwrap().nominal_voltage, 7.4);
    let control = c.control().unwrap().unwrap();
    assert_eq!((control.period_s, control.mode.as_str()), (0.02, "hold"));
    assert_eq!(control.targets, BTreeMap::from([("hip".to_string(), 0.2)]), "the NaN target is dropped, not the setting");
    let request = ControlRequest { period_s: 0.01, targets: Some(BTreeMap::from([("hip".to_string(), 0.2)])), ..ControlRequest::default() };
    c.set_control(&request).unwrap();
    let u = c.uncertainty().unwrap().unwrap();
    assert_eq!(u["mass"], json!({"sigma_fraction": 0.05}));
    let mut sigmas = Map::new();
    sigmas.insert("mass".into(), json!(0.05));
    c.set_uncertainty(&sigmas).unwrap();
    assert_eq!(c.actuator_profiles().unwrap(), Value::Null);
    c.set_actuator_profiles(&json!({"hip motor": {"family": "mg996r"}})).unwrap();
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /battery HTTP/1.1", port, None);
    assert_request(&seen[1], "PUT /battery HTTP/1.1", port, Some(r#"{"cells":3,"chemistry":"liion","capacity_ah":2.0,"initial_soc":1.0}"#));
    assert_request(&seen[2], "GET /control HTTP/1.1", port, None);
    assert_request(&seen[3], "PUT /control HTTP/1.1", port, Some(r#"{"period_s":0.01,"latency_s":0.004,"targets":{"hip":0.2}}"#));
    assert_request(&seen[4], "GET /uncertainty HTTP/1.1", port, None);
    assert_request(&seen[5], "PUT /uncertainty HTTP/1.1", port, Some(r#"{"mass":0.05}"#));
    assert_request(&seen[6], "GET /actuator-profiles HTTP/1.1", port, None);
    assert_request(&seen[7], "POST /actuator-profiles HTTP/1.1", port, Some(r#"{"profiles":{"hip motor":{"family":"mg996r"}}}"#));
}

/// Every typed wrapper sends one `POST /ops/{name}` with the Python
/// signature's positional order (`null` for `None`) and keywords only for
/// `**fields`.
#[test]
fn ops_wrappers_send_the_python_signature() {
    let answers = vec![
        op_answer("\"j1\"", "\"Joint\""),
        op_answer("\"j1\"", "\"Edit joint\""),
        op_answer("null", "\"Rename\""),
        op_answer("\"j2\"", "\"Joint\""),
        op_answer("\"m1\"", "\"Add motor\""),
        op_answer("\"m1\"", "\"Mount motor\""),
        op_answer("\"j1\"", "\"Attach motor\""),
        op_answer("\"a1\"", "\"Ground\""),
        op_answer("[]", ""),
        op_answer(SETTINGS, "\"Robot setting\""),
        op_answer("{\"updated\": []}", "\"Configure robot\""),
    ];
    let (c, server) = serve(answers);
    let port = c.endpoint.port;
    let joint = AddJoint { parent: Some("a1".into()), child: "b2".into(), pivot: [0.0, 0.0, 200.0], axis: [0.0, 1.0, 0.0], lower: Some(-1.5), upper: Some(1.5), name: Some("hip".into()), ..AddJoint::default() };
    let done = c.add_joint(&joint).unwrap();
    assert_eq!((done.result, done.history.undo), (json!("j1"), vec!["Joint".to_string()]));
    let mut fields = Map::new();
    fields.insert("damping".into(), json!(0.01));
    fields.insert("lower".into(), json!(-1.0));
    c.set_joint("j1", &fields).unwrap();
    c.rename("j1", "hip joint").unwrap();
    c.connect_fixed("a1", "b2", None, Some("bolted")).unwrap();
    let motor = AddMotor { spec_id: "mg996r".into(), mount_point: [0.0, -20.0, 200.0], shaft_dir: [0.0, 1.0, 0.0], mount_on: Some("a1".into()), cut_mount: true, ..AddMotor::default() };
    c.add_motor(&motor).unwrap();
    c.mount_motor("m1", None).unwrap();
    c.attach_motor("j1", Some("m1"), 2.0).unwrap();
    c.set_ground("a1", true).unwrap();
    c.infer_joints().unwrap();
    c.set_robot_setting("world", &json!({"gravity": [0.0, 0.0, -9.81]})).unwrap();
    c.configure_robot(42, Some(&json!({"a1": {"name": "base"}})), None, None, None).unwrap();
    let seen = server.join().unwrap();
    let bodies = [
        ("add_joint", r#"{"args":["revolute","a1","b2",[0.0,0.0,200.0],[0.0,1.0,0.0],-1.5,1.5,null,1.0,"hip"],"kwargs":{}}"#),
        ("set_joint", r#"{"args":["j1"],"kwargs":{"damping":0.01,"lower":-1.0}}"#),
        ("rename", r#"{"args":["j1","hip joint"],"kwargs":{}}"#),
        ("connect_fixed", r#"{"args":["a1","b2",null,"bolted"],"kwargs":{}}"#),
        ("add_motor", r#"{"args":["mg996r",[0.0,-20.0,200.0],[0.0,1.0,0.0],0.0,"a1",true,null],"kwargs":{}}"#),
        ("mount_motor", r#"{"args":["m1",null],"kwargs":{}}"#),
        ("attach_motor", r#"{"args":["j1","m1",2.0],"kwargs":{}}"#),
        ("set_ground", r#"{"args":["a1",true],"kwargs":{}}"#),
        ("infer_joints", r#"{"args":[],"kwargs":{}}"#),
        ("set_robot_setting", r#"{"args":["world",{"gravity":[0.0,0.0,-9.81]}],"kwargs":{}}"#),
        ("configure_robot", r#"{"args":[42,{"a1":{"name":"base"}},null,null,null],"kwargs":{}}"#),
    ];
    assert_eq!(seen.len(), bodies.len());
    for (s, (name, body)) in seen.iter().zip(bodies) {
        assert_request(s, &format!("POST /ops/{name} HTTP/1.1"), port, Some(body));
    }
}

#[test]
fn refusals_carry_robocads_text_and_status() {
    let (c, server) = serve(vec![
        Answer::Json(422, r#"{"error": "unknown motor x; see the library"}"#.into()),
        Answer::Json(500, r#"{"error": "KernelError: sensor kind must be imu, encoder, current or force", "trace": "Traceback ..."}"#.into()),
        Answer::Json(409, r#"{"error": "Expected document revision 41; current revision is 42. Fetch the current document and rebuild the candidate before applying it."}"#.into()),
    ]);
    let e = c.add_motor(&AddMotor { spec_id: "x".into(), ..AddMotor::default() }).unwrap_err();
    assert_eq!(e.to_string(), "RoboCAD POST /ops/add_motor: unknown motor x; see the library (HTTP 422)");
    let e = c.add_sensor(&SensorRequest { kind: "gps".into(), body: "a1".into(), ..SensorRequest::default() }).unwrap_err();
    // A mutating 5xx preserves the server refusal and status while keeping
    // the source outcome uncertain: the command may have committed first.
    assert_eq!((e.method, e.route.as_str(), e.status), ("POST", "/sensors", Some(500)));
    assert_eq!(e.message.strip_suffix(": RoboCAD may still apply it; refresh before retrying"), Some("KernelError: sensor kind must be imu, encoder, current or force"));
    let e = c.configure_robot(41, None, None, None, None).unwrap_err();
    assert!(e.no_gui() && e.message.starts_with("Expected document revision 41"), "{e:?}");
    server.join().unwrap();
}

#[test]
fn defaults_match_robocads_signatures() {
    assert_eq!((AddJoint::default().kind.as_str(), AddJoint::default().axis, AddJoint::default().gear_ratio), ("revolute", [0.0, 0.0, 1.0], 1.0));
    assert_eq!(BatteryRequest::default(), BatteryRequest { cells: 2, chemistry: "lipo".into(), capacity_ah: 1.0, internal_resistance: None, initial_soc: 1.0 });
    assert_eq!((ControlRequest::default().period_s, ControlRequest::default().latency_s), (0.02, 0.004));
    assert_eq!(serde_json::to_string(&ControlRequest::default()).unwrap(), r#"{"period_s":0.02,"latency_s":0.004}"#);
    assert_eq!(CableMeta::default().segments, 4);
    assert_eq!(serde_json::to_string(&CableRequest::default()).unwrap(), r#"{"from_body":"","from_point":[0.0,0.0,0.0],"to_body":"","to_point":[0.0,0.0,0.0]}"#);
}
