//! The robot entries' tests (cad-physical-inspect): the catalogue rows,
//! the picks' lists, the dialogs' presets from a seeded selection, and the
//! Ops calls each form builds, with the arguments exactly as RoboCAD's
//! handlers pass them (commands.py:935-1166). Windowless.
use super::args::{Built, build};
use super::form::{form_json, open_form_with};
use super::robot_args::{Done, NO_FACE, Plan};
use super::*;
use crate::cad::document::{CadTarget, Connection, Edit};
use crate::cad::robot::data::Bundle;
use crate::cad::sketch::{ActivePlane, BasePlane, CadActivePlane};
use sim_runtime::cad_client::{Battery, CadClient, DocState, Health, MotorSpec, NodeResults, NodeSummary, RobotJoint, RobotMotor, RobotSummary};
use std::collections::BTreeMap;

fn node(id: &str, kind: &str, name: &str) -> NodeSummary {
    NodeSummary { id: id.into(), kind: kind.into(), name: name.into(), visible: true, effective_visible: true, ..Default::default() }
}

fn body(id: &str) -> SelectionItem {
    SelectionItem(id.into(), "body".into(), 0)
}

/// A connected document at revision 4: bodies b1 "Bracket", b2 "Plate",
/// b3 "Leg", a motor body m1 "Servo", a joint j1 "hip", and RoboCAD's
/// robot description (hip revolute b1 → b3, motor m1) and motor library.
fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    doc.client = Some(CadClient::new("http://127.0.0.1:8420").unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), revision: 4, ..Default::default() });
    doc.doc = Some(DocState {
        nodes: vec![node("b1", "body", "Bracket"), node("b2", "body", "Plate"), node("b3", "body", "Leg"), node("m1", "body", "Servo"), node("j1", "joint", "hip")],
        revision: 4,
        ..Default::default()
    });
    doc.doc_key = Some((None, 4));
    let hip = RobotJoint { id: "j1".into(), name: "hip".into(), kind: "revolute".into(), parent: Some("b1".into()), child: "b3".into(), pivot: [1.0, 2.0, 3.0], axis: [0.0, 1.0, 0.0], lower: Some(-std::f64::consts::FRAC_PI_2), upper: Some(0.5), gear_ratio: 2.5, damping: 0.01, ..Default::default() };
    let summary = RobotSummary { joints: vec![hip], motors: vec![RobotMotor { id: "m1".into(), name: "Servo".into(), ..Default::default() }], links: 3, dof: Some(1), ..Default::default() };
    doc.robot.data.bundle = Some(Bundle {
        summary: Ok(summary),
        results: Ok(NodeResults::default()),
        sensors: Ok(Vec::new()),
        cables: Ok(Vec::new()),
        battery: Ok(Some(Battery { cells: 3, chemistry: "liion".into(), capacity_ah: 2.2, ..Default::default() })),
        control: Ok(None),
        uncertainty: Ok(None),
        profiles: Ok(Value::Null),
        motors: None,
    });
    doc.robot.data.key = Some((doc.generation, 4));
    let mut lib = BTreeMap::new();
    lib.insert("ds3218".to_string(), MotorSpec { id: "ds3218".into(), name: "DS3218 servo".into(), kind: "servo".into(), stall_torque: 2.0, mass_g: 60.0, ..Default::default() });
    doc.robot.data.motors = Some((doc.generation, Ok(lib)));
    doc
}

fn op(id: &str) -> &'static OpEntry {
    entry(id).unwrap_or_else(|| panic!("{id} is not in the catalogue"))
}

/// The form's drafts as `CadRun` parameters (what OK sends).
fn drafts(doc: &CadDocument) -> Map<String, Value> {
    let form = doc.ops.form.as_ref().expect("a form is open");
    let e = op(form.op);
    e.params.iter().zip(&form.texts).map(|(p, t)| (p.name.to_string(), Value::String(t.clone()))).collect()
}

fn plan(e: &OpEntry, r: &Resolved, given: &Map<String, Value>, doc: &CadDocument) -> Plan {
    let values = values(e, given).unwrap_or_else(|err| panic!("{}: {err}", e.id));
    match build(e, r, &values, doc, &Env::default()) {
        Ok(Built::Robot(plan)) => plan,
        other => panic!("{}: expected a robot plan, got {other:?}", e.id),
    }
}

fn texts_of(doc: &CadDocument) -> BTreeMap<&'static str, String> {
    let form = doc.ops.form.as_ref().unwrap();
    op(form.op).params.iter().map(|p| p.name).zip(form.texts.iter().cloned()).collect()
}

#[test]
fn robot_entries_are_robocads_tools_and_dialogs() {
    for id in ["robot.add_motor", "robot.add_joint", "robot.joint_dialog", "robot.infer", "robot.assign_motor", "robot.fixed", "robot.ground", "robot.add_sensor", "robot.add_cable", "robot.power", "ops.set_joint", "ops.mount_motor", "ops.configure_robot", "ops.set_robot_setting"] {
        let e = op(id);
        assert_eq!(e.category, "Robot", "{id}");
        if id.starts_with("robot.") {
            assert!(matches!(e.shape, Shape::Robot(_)), "{id}");
        }
    }
    assert_eq!((op("robot.add_motor").flow, op("robot.add_joint").flow), (Flow::RobotPick(RobotTool::Motor), Flow::RobotPick(RobotTool::Joint)));
    assert_eq!(op("robot.add_joint").keys, &["Ctrl+Shift+J"]);
    assert_eq!(op("robot.add_motor").keys, &["Ctrl+Shift+M"]);
    assert_eq!(op("robot.add_motor").hint, "Click a face to mount the motor there (housing outside, shaft into the body) • Esc to finish");
    // The parts the other panels own are not here.
    for id in ["ops.set_joint_physics", "ops.set_material", "ops.set_material_props", "ops.set_color"] {
        assert!(entry(id).is_none(), "{id}");
    }
}

#[test]
fn picks_are_robocads_combo_boxes() {
    let doc = document();
    let keys = |source: &str| picks(source, &doc).into_iter().map(|(k, _)| k).collect::<Vec<_>>();
    // `_robot_bodies`: bodies that are not motors, in tree order.
    assert_eq!(keys("bodies"), ["b1", "b2", "b3"]);
    assert_eq!(picks("bodies_or_world", &doc)[0], (String::new(), "(world)".to_string()));
    assert_eq!(picks("bodies_or_pick", &doc)[0].1, "(pick by clicking a face)");
    assert_eq!(keys("joints"), ["j1"]);
    assert_eq!(keys("motors_placed"), ["m1"]);
    assert_eq!(picks("motors", &doc), [("ds3218".to_string(), "DS3218 servo   servo  2 N·m  60 g".to_string())]);
    assert_eq!(picks("joint_types", &doc)[0].1, "revolute: hinge with angle limits (servo, geared motor)");
    assert_eq!(picks("joint_types", &doc)[5].1, "loop_revolute:");
    assert_eq!(picks("sensor_kinds", &doc).len(), 4);
}

#[test]
fn g_formats_as_python() {
    for (v, want) in [(0.0, "0"), (1.0, "1"), (0.5, "0.5"), (200.0, "200"), (2.2, "2.2"), (123456.7, "123457"), (1234567.0, "1.23457e+06"), (1e-5, "1e-05"), (0.0001, "0.0001"), (-90.0, "-90"), (9.999995, "10"), (1.0 / 3.0, "0.333333")] {
        assert_eq!(g(v), want, "{v}");
    }
}

#[test]
fn the_joint_dialog_presets_from_the_selection_and_the_active_plane() {
    let mut doc = document();
    let e = op("robot.joint_dialog");
    let selection = [body("b2"), body("b3")];
    let plane = CadActivePlane { plane: Some(ActivePlane::Base(BasePlane::Xz)), ..Default::default() };
    let env = Env { selection: &selection, plane: Some(&plane), ..Default::default() };
    open_form_with(&mut doc, e, Some(&env));
    let t = texts_of(&doc);
    assert_eq!((t["parent"].as_str(), t["child"].as_str(), t["type"].as_str()), ("b2", "b3", "revolute"));
    assert_eq!(t["axis"], "0, -1, 0", "the XZ plane's normal");
    // One body: the child; the parent stays the world.
    doc.ops.form = None;
    let one = [body("b3")];
    open_form_with(&mut doc, e, Some(&Env { selection: &one, ..Default::default() }));
    let t = texts_of(&doc);
    assert_eq!((t["parent"].as_str(), t["child"].as_str(), t["pivot"].as_str()), ("", "b3", "0, 0, 0"));
    // The picks are in cad_state.ops.form.
    assert_eq!(form_json(&doc)["fields"][1]["picks"][0], json!(["", "(world)"]));
    // OK: add_joint as RoboCAD calls it; limits in degrees are sent in radians.
    let mut given = drafts(&doc);
    given.insert("lower".into(), json!("-90"));
    given.insert("upper".into(), json!("45"));
    given.insert("damping".into(), json!("0.5"));
    let p = plan(e, &Resolved::default(), &given, &doc);
    assert_eq!(p.calls.len(), 1);
    assert_eq!(p.calls[0].name, "add_joint");
    let a = &p.calls[0].args;
    assert_eq!(a[..5], [json!("revolute"), Value::Null, json!("b3"), json!([0.0, 0.0, 0.0]), json!([0.0, 0.0, 1.0])]);
    assert!((a[5].as_f64().unwrap() + std::f64::consts::FRAC_PI_2).abs() < 1e-12 && (a[6].as_f64().unwrap() - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    assert_eq!(a[7..], [Value::Null, json!(1.0), Value::Null]);
    // Damping not zero: RoboCAD's second call, set_joint(jid, damping=…).
    assert_eq!(p.done, Done::Added { what: "joint", damping: Some(0.5) });
    // A prismatic joint's limits are sent as typed; no damping, no second call.
    let mut prismatic = given.clone();
    prismatic.insert("type".into(), json!("prismatic"));
    prismatic.insert("damping".into(), json!("0"));
    let p = plan(e, &Resolved::default(), &prismatic, &doc);
    assert_eq!((p.calls[0].args[5].clone(), p.calls[0].args[6].clone()), (json!(-90.0), json!(45.0)));
    assert_eq!(p.done, Done::Added { what: "joint", damping: None });
    // No child: RoboCAD's refusal, nothing sent.
    let mut childless = given;
    childless.insert("child".into(), json!(""));
    let values = values(e, &childless).unwrap();
    assert_eq!(build(e, &Resolved::default(), &values, &doc, &Env::default()), Err("a joint needs a child body".to_string()));
}

#[test]
fn edit_joint_presets_the_joint_and_renames_only_a_changed_name() {
    let mut doc = document();
    let e = op("ops.set_joint");
    let selection = [SelectionItem("j1".into(), "body".into(), 0)];
    open_form_with(&mut doc, e, Some(&Env { selection: &selection, ..Default::default() }));
    let t = texts_of(&doc);
    assert_eq!((t["parent"].as_str(), t["child"].as_str(), t["lower"].as_str(), t["upper"].as_str()), ("b1", "b3", "-90", "28.6479"));
    assert_eq!((t["gear_ratio"].as_str(), t["damping"].as_str(), t["name"].as_str(), t["pivot"].as_str()), ("2.5", "0.01", "hip", "1, 2, 3"));
    let r = Resolved { nodes: vec!["j1".into()], ..Default::default() };
    let p = plan(e, &r, &drafts(&doc), &doc);
    assert_eq!(p.calls.len(), 1, "the name is unchanged: no rename");
    assert_eq!((p.calls[0].name, &p.calls[0].args), ("set_joint", &vec![json!("j1")]));
    let k = &p.calls[0].kwargs;
    assert_eq!((k["type"].clone(), k["parent"].clone(), k["child"].clone(), k["motor"].clone(), k["gear_ratio"].clone()), (json!("revolute"), json!("b1"), json!("b3"), Value::Null, json!(2.5)));
    let mut renamed = drafts(&doc);
    renamed.insert("name".into(), json!("knee"));
    let p = plan(e, &r, &renamed, &doc);
    assert_eq!(p.calls[1].name, "rename");
    assert_eq!(p.calls[1].args, vec![json!("j1"), json!("knee")]);
    assert_eq!(p.done, Done::Say("joint knee updated".into()));
}

#[test]
fn the_motor_tool_sends_add_motor_with_the_pick() {
    let mut doc = document();
    let e = op("robot.add_motor");
    open_form_with(&mut doc, e, Some(&Env::default()));
    // A combo box is on its first entry: the library's first motor; mount "(pick by clicking a face)".
    let t = texts_of(&doc);
    assert_eq!((t["spec"].as_str(), t["mount_on"].as_str(), t["cut"].as_str()), ("ds3218", "", "true"));
    assert_eq!(note(e, &doc.ops.form.as_ref().unwrap().texts, &doc).as_deref(), Some(", 0×0×0 mm, shaft Ø0×0 mm, no mount holes, 0 rad/s no-load, 5 V. "));
    // OK without a click: nothing to place.
    let values_ = values(e, &drafts(&doc)).unwrap();
    assert_eq!(build(e, &Resolved::default(), &values_, &doc, &Env::default()), Err(NO_FACE.to_string()));
    // The pick's point, shaft direction and the clicked body.
    let mut given = drafts(&doc);
    given.insert("point".into(), json!([1.0, 2.0, 3.0]));
    given.insert("shaft_dir".into(), json!([0.0, 0.0, -1.0]));
    given.insert("mount_on".into(), json!("b1"));
    let p = plan(e, &Resolved::default(), &given, &doc);
    assert_eq!(p.calls[0].name, "add_motor");
    assert_eq!(p.calls[0].args, vec![json!("ds3218"), json!([1.0, 2.0, 3.0]), json!([0.0, 0.0, -1.0]), json!(0.0), json!("b1"), json!(true), Value::Null]);
    assert_eq!(p.done, Done::Say("motor placed on Bracket; Assign motor… links it to a joint".into()));
    // The last motor presets the next dialog.
    doc.robot.tools.last_motor = Some(vec![("spec".into(), "ds3218".into()), ("rotation".into(), "90".into()), ("cut".into(), "false".into())]);
    doc.ops.form = None;
    open_form_with(&mut doc, e, Some(&Env::default()));
    let t = texts_of(&doc);
    assert_eq!((t["rotation"].as_str(), t["cut"].as_str()), ("90", "false"));
}

#[test]
fn assign_fix_ground_sensor_and_cable_call_as_robocad() {
    let mut doc = document();
    // Assign motor: the selected motor and joint preset; attach_motor(joint, motor, gear).
    let e = op("robot.assign_motor");
    let selection = [body("m1"), SelectionItem("j1".into(), "body".into(), 0)];
    open_form_with(&mut doc, e, Some(&Env { selection: &selection, ..Default::default() }));
    let p = plan(e, &Resolved::default(), &drafts(&doc), &doc);
    assert_eq!((p.calls[0].name, p.calls[0].args.clone()), ("attach_motor", vec![json!("j1"), json!("m1"), json!(1.0)]));
    assert_eq!(p.done, Done::Say("Servo now drives hip".into()));
    assert_eq!(robot_form::precheck(e, &doc, &selection), None);
    // Fix together: one connect_fixed per child, the first is the parent.
    let e = op("robot.fixed");
    let r = Resolved { nodes: vec!["b1".into(), "b2".into(), "b3".into()], ..Default::default() };
    let p = plan(e, &r, &Map::new(), &doc);
    let calls: Vec<(&str, Vec<Value>)> = p.calls.iter().map(|c| (c.name, c.args.clone())).collect();
    assert_eq!(calls, vec![("connect_fixed", vec![json!("b1"), json!("b2")]), ("connect_fixed", vec![json!("b1"), json!("b3")])]);
    assert_eq!(p.done, Done::Say("2 bodies fixed to Bracket".into()));
    // Toggle ground: each body's flag is read when it runs.
    let p = plan(op("robot.ground"), &Resolved { nodes: vec!["b2".into()], ..Default::default() }, &Map::new(), &doc);
    assert_eq!(p.done, Done::Ground { ids: vec!["b2".into()], names: "Plate".into() });
    // A sensor on the selected body: add_sensor(kind, body, point, None, name, joint, rate_hz=…).
    let e = op("robot.add_sensor");
    doc.ops.form = None;
    let selection = [body("b2")];
    open_form_with(&mut doc, e, Some(&Env { selection: &selection, ..Default::default() }));
    let p = plan(e, &Resolved::default(), &drafts(&doc), &doc);
    assert_eq!(p.calls[0].args, vec![json!("imu"), json!("b2"), json!([0.0, 0.0, 0.0]), Value::Null, Value::Null, Value::Null]);
    assert_eq!(p.calls[0].kwargs, Map::from_iter([("rate_hz".to_string(), json!(200.0))]));
    // A cable: mass typed in g is sent in kg; an empty length is RoboCAD's auto (None).
    let e = op("robot.add_cable");
    doc.ops.form = None;
    let selection = [body("b1"), body("b3")];
    open_form_with(&mut doc, e, Some(&Env { selection: &selection, ..Default::default() }));
    let mut given = drafts(&doc);
    given.insert("mass".into(), json!("4"));
    let p = plan(e, &Resolved::default(), &given, &doc);
    assert_eq!(p.calls[0].args, vec![json!("b1"), json!([0.0, 0.0, 0.0]), json!("b3"), json!([0.0, 0.0, 0.0]), Value::Null, json!(0.004), Value::Null, Value::Null]);
}

#[test]
fn the_power_dialog_sends_battery_control_and_uncertainty_in_order() {
    let mut doc = document();
    let e = op("robot.power");
    open_form_with(&mut doc, e, Some(&Env::default()));
    let t = texts_of(&doc);
    // The document's battery; the hip's target (none set: 0°).
    assert_eq!((t["cells"].as_str(), t["chemistry"].as_str(), t["capacity_ah"].as_str(), t["targets"].as_str()), ("3", "liion", "2.2", "{\"hip\":0.0}"));
    let mut given = drafts(&doc);
    given.insert("targets".into(), json!("{\"hip\": 90}"));
    let p = plan(e, &Resolved::default(), &given, &doc);
    let names: Vec<&str> = p.calls.iter().map(|c| c.name).collect();
    assert_eq!(names, ["set_battery", "set_control", "set_uncertainty"]);
    assert_eq!(p.calls[0].kwargs, Map::from_iter([("cells".to_string(), json!(3)), ("chemistry".to_string(), json!("liion")), ("capacity_ah".to_string(), json!(2.2))]));
    let hip = p.calls[1].kwargs["targets"]["hip"].as_f64().unwrap();
    assert!((hip - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert_eq!(p.calls[1].kwargs["period_s"], json!(0.02));
    assert!((p.calls[2].kwargs["dimension_m"].as_f64().unwrap() - 0.15e-3).abs() < 1e-15);
    assert_eq!(p.calls[2].kwargs["friction"], json!(0.2));
    // No cells: the battery setting is removed (set_robot_setting("battery", None)).
    given.insert("cells".into(), json!("0"));
    let p = plan(e, &Resolved::default(), &given, &doc);
    assert_eq!((p.calls[0].name, p.calls[0].args.clone()), ("set_robot_setting", vec![json!("battery"), Value::Null]));
    // A name that is not a motion joint is refused by name.
    given.insert("targets".into(), json!("{\"elbow\": 10}"));
    let values_ = values(e, &given).unwrap();
    assert!(build(e, &Resolved::default(), &values_, &doc, &Env::default()).is_err_and(|err| err.contains("elbow is not a revolute")));
}

#[test]
fn rest_only_robot_ops_take_robocads_signatures() {
    let doc = document();
    let p = plan(op("ops.mount_motor"), &Resolved::default(), &Map::from_iter([("motor".to_string(), json!("m1"))]), &doc);
    assert_eq!(p.calls[0].args, vec![json!("m1"), Value::Null]);
    let r = Resolved { revision: 7, ..Default::default() };
    let p = plan(op("ops.configure_robot"), &r, &Map::from_iter([("updates".to_string(), json!({"b1": {}}))]), &doc);
    assert_eq!((p.calls[0].name, p.calls[0].args.clone()), ("configure_robot", vec![json!(7)]));
    assert_eq!(p.calls[0].kwargs, Map::from_iter([("updates".to_string(), json!({"b1": {}}))]));
    let e = op("ops.set_robot_setting");
    let values_ = values(e, &Map::from_iter([("key".to_string(), json!("world")), ("value".to_string(), json!({"floor_z": 0}))])).unwrap();
    let Ok(Built::Edit { calls, .. }) = build(e, &Resolved::default(), &values_, &doc, &Env::default()) else { panic!("an edit") };
    assert_eq!((calls[0].name, calls[0].args.clone()), ("set_robot_setting", vec![json!("world"), json!({"floor_z": 0})]));
}

#[test]
fn assign_motor_refuses_before_its_dialog_without_motors_or_joints() {
    let mut doc = document();
    if let Some(Ok(s)) = doc.robot.data.bundle.as_mut().map(|b| b.summary.as_mut()) {
        s.motors.clear();
    }
    assert_eq!(robot_form::precheck(op("robot.assign_motor"), &doc, &[]), Some("add a motor and a joint first".to_string()));
}

#[test]
fn power_and_edit_joint_refuse_until_the_description_is_read_at_the_shown_revision() {
    let joint = [SelectionItem("j1".into(), "body".into(), 0)];
    let (power, edit) = (op("robot.power"), op("ops.set_joint"));
    let doc = document();
    assert_eq!((robot_form::precheck(power, &doc, &[]), robot_form::precheck(edit, &doc, &joint)), (None, None));
    // A read from an older revision: both refuse by name, and so does the power dialog's OK.
    let mut old = document();
    old.robot.data.key = Some((old.generation, 3));
    let reading = "the robot description is still being read (revision 4); try again in a moment".to_string();
    assert_eq!(robot_form::precheck(power, &old, &[]), Some(reading.clone()));
    assert_eq!(robot_form::precheck(edit, &old, &joint), Some(reading.clone()));
    let mut given = Map::new();
    for (k, v) in [("cells", "3"), ("chemistry", "lipo"), ("capacity_ah", "1"), ("period_s", "0.02"), ("latency_s", "0.004"), ("targets", "{}"), ("dimension", "0.15"), ("friction", "0.2")] {
        given.insert(k.to_string(), json!(v));
    }
    let values_ = values(power, &given).unwrap();
    assert_eq!(build(power, &Resolved::default(), &values_, &old, &Env::default()), Err(reading));
    // A failed battery read is not "no battery": the dialog does not open (its OK would delete it).
    let mut failed = document();
    if let Some(b) = failed.robot.data.bundle.as_mut() {
        b.battery = Err("HTTP 500".into());
    }
    assert_eq!(robot_form::precheck(power, &failed, &[]), Some("RoboCAD's battery setting could not be read, so the current one is not known: HTTP 500".to_string()));
    // A failed control read: OK would reset every target left out.
    let mut failed = document();
    if let Some(b) = failed.robot.data.bundle.as_mut() {
        b.control = Err("HTTP 500".into());
    }
    assert!(build(power, &Resolved::default(), &values_, &failed, &Env::default()).is_err_and(|e| e.contains("control loop setting could not be read")));
    // A joint the description does not have (added since it was read, under the same revision's tree).
    let mut unknown = document();
    if let Some(Ok(s)) = unknown.robot.data.bundle.as_mut().map(|b| b.summary.as_mut()) {
        s.joints.clear();
    }
    assert_eq!(robot_form::precheck(edit, &unknown, &joint), Some("hip is not in RoboCAD's robot description at revision 4, so its values are not known".to_string()));
}

#[test]
fn selection_seeded_forms_are_seeded_again_each_time_they_open() {
    let mut doc = document();
    if let Some(d) = doc.doc.as_mut() {
        d.nodes.push(node("j2", "joint", "knee"));
    }
    if let Some(Ok(s)) = doc.robot.data.bundle.as_mut().map(|b| b.summary.as_mut()) {
        s.joints.push(RobotJoint { id: "j2".into(), name: "knee".into(), kind: "prismatic".into(), parent: Some("b3".into()), child: "b2".into(), lower: Some(-5.0), upper: Some(5.0), gear_ratio: 1.0, ..Default::default() });
    }
    let e = op("ops.set_joint");
    let hip = [SelectionItem("j1".into(), "body".into(), 0)];
    open_form_with(&mut doc, e, Some(&Env { selection: &hip, ..Default::default() }));
    super::form::form_set(&mut doc, "damping", &json!("0.3")).unwrap();
    // Opened again for the knee while the hip's form is open: the knee's values, none of the hip's drafts.
    let knee = [SelectionItem("j2".into(), "body".into(), 0)];
    open_form_with(&mut doc, e, Some(&Env { selection: &knee, ..Default::default() }));
    let t = texts_of(&doc);
    assert_eq!((t["name"].as_str(), t["type"].as_str(), t["lower"].as_str(), t["damping"].as_str()), ("knee", "prismatic", "-5", "0"));
    // A form that is not preset from the selection keeps its drafts.
    let m = op("ops.mount_motor");
    open_form_with(&mut doc, m, Some(&Env::default()));
    super::form::form_set(&mut doc, "body", &json!("b2")).unwrap();
    open_form_with(&mut doc, m, Some(&Env::default()));
    assert_eq!(texts_of(&doc)["body"], "b2");
}

#[test]
fn robot_adds_note_the_selection_to_replace_with_the_created_node() {
    let mut doc = document();
    let selection = [body("b2")];
    let in_flight = || Edit { label: "Sensor imu on Plate".into(), job: crate::jobs::Job::finished(0, Ok(EditDone { message: String::new(), result: Value::Null })), started: std::time::Instant::now(), clear_selection: None, activates_plane: false, retarget: None };
    doc.edit = Some(in_flight());
    doc.edit_seq = 7;
    started(&mut doc, op("robot.add_sensor"), false, &selection);
    assert_eq!(doc.ops.selects_created, Some((7, selection.to_vec())));
    // A REST run naming its own items leaves the user's selection alone.
    started(&mut doc, op("robot.add_sensor"), true, &selection);
    assert_eq!(doc.ops.selects_created, None);
    // Other robot edits leave the selection as it is.
    started(&mut doc, op("robot.assign_motor"), false, &selection);
    assert_eq!(doc.ops.selects_created, None);
    for id in ["robot.add_motor", "robot.joint_dialog", "robot.add_cable"] {
        started(&mut doc, op(id), false, &selection);
        assert!(doc.ops.selects_created.is_some(), "{id}");
    }
}

#[test]
fn a_joint_tool_pick_the_dialog_cannot_take_is_named() {
    let mut doc = document();
    // The motor body m1 clicked as the child: not one of the dialog's bodies.
    let preset = [("parent", "b1".to_string()), ("child", "m1".to_string()), ("pivot", "0, 0, 0".to_string()), ("axis", "0, 0, 1".to_string())];
    let opened = open_preset(&mut doc, &Env::default(), "robot.joint_dialog", &preset).unwrap();
    assert_eq!(opened["dropped"], json!(["Servo is not one of the dialog's choices (a motor is not a link), so the child was not preset: choose it in the dialog"]));
    assert_eq!(texts_of(&doc)["parent"], "b1");
    assert!(doc.status.as_ref().is_some_and(|s| s.as_ref().is_err_and(|e| e.starts_with("Robot: joint from the two selected bodies…: Servo is not"))));
}

/// `prepare` of a REST `cad_run ops.set_joint` on joint j1 with `given`.
fn run_set_joint(doc: &CadDocument, given: &[(&str, Value)]) -> Result<Plan, String> {
    let params: Map<String, Value> = given.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    let items = [SelectionItem("j1".into(), "body".into(), 0)];
    match prepare(doc, &Env::default(), op("ops.set_joint"), &params, Some(&items), None)? {
        Built::Robot(plan) => Ok(plan),
        other => panic!("expected a robot plan, got {other:?}"),
    }
}

#[test]
fn a_partial_rest_edit_joint_keeps_the_joints_other_values() {
    let doc = document();
    let p = run_set_joint(&doc, &[("lower", json!(-30))]).unwrap();
    assert_eq!(p.calls.len(), 1, "the name is the joint's own: no rename");
    assert_eq!((p.calls[0].name, &p.calls[0].args), ("set_joint", &vec![json!("j1")]));
    let k = &p.calls[0].kwargs;
    assert_eq!((k["type"].clone(), k["parent"].clone(), k["child"].clone(), k["motor"].clone()), (json!("revolute"), json!("b1"), json!("b3"), Value::Null));
    assert_eq!((k["pivot"].clone(), k["axis"].clone(), k["gear_ratio"].clone(), k["damping"].clone()), (json!([1.0, 2.0, 3.0]), json!([0.0, 1.0, 0.0]), json!(2.5), json!(0.01)));
    // The given lower limit in degrees, sent in radians; the upper the joint's own, not the form's rounded 28.6479°.
    assert!((k["lower"].as_f64().unwrap() + 30f64.to_radians()).abs() < 1e-12);
    assert!((k["upper"].as_f64().unwrap() - 0.5).abs() < 1e-12);
    // Every field given (what the window form's OK sends): used as given.
    let all: Vec<(&str, Value)> = op("ops.set_joint").params.iter().map(|p| (p.name, json!(p.default))).collect();
    let p = run_set_joint(&doc, &all).unwrap();
    assert_eq!((p.calls[0].kwargs["parent"].clone(), p.calls[0].kwargs["gear_ratio"].clone(), p.calls[0].kwargs["lower"].clone()), (Value::Null, json!(1.0), Value::Null));
    // A kind change across prismatic without the limits the joint has: their unit would change.
    assert!(run_set_joint(&doc, &[("type", json!("prismatic"))]).is_err_and(|e| e.contains("pass lower and upper")));
    assert!(run_set_joint(&doc, &[("type", json!("prismatic")), ("lower", json!(-5)), ("upper", json!(5))]).is_ok());
}

/// Edit joint's description `doc` with `f` applied to joint j1.
fn with_hip(f: impl FnOnce(&mut RobotJoint)) -> CadDocument {
    let mut doc = document();
    if let Some(Ok(s)) = doc.robot.data.bundle.as_mut().map(|b| b.summary.as_mut()) {
        f(&mut s.joints[0]);
    }
    doc
}

#[test]
fn a_partial_rest_edit_joint_sends_the_joints_values_exactly() {
    // Values the form would round (pivot 1234.57, gear ratio 2.57) survive exactly.
    let doc = with_hip(|j| {
        j.pivot = [1234.5678, -0.000123456789, 3.0];
        j.axis = [0.6, 0.0, 0.8];
        j.gear_ratio = 2.567;
        j.damping = 0.000123456;
        j.upper = Some(0.123456789);
    });
    let p = run_set_joint(&doc, &[("lower", json!(-30))]).unwrap();
    let k = &p.calls[0].kwargs;
    assert_eq!((k["pivot"].clone(), k["axis"].clone()), (json!([1234.5678, -0.000123456789, 3.0]), json!([0.6, 0.0, 0.8])));
    assert_eq!((k["gear_ratio"].clone(), k["damping"].clone()), (json!(2.567), json!(0.000123456)));
    assert!((k["upper"].as_f64().unwrap() - 0.123456789).abs() < 1e-12);
    // A prismatic joint's limits are filled in mm as stored.
    let doc = with_hip(|j| {
        j.kind = "prismatic".into();
        j.lower = Some(-12.345678);
        j.upper = None;
    });
    let k = run_set_joint(&doc, &[("damping", json!(0.5))]).unwrap().calls[0].kwargs.clone();
    assert!((k["lower"].as_f64().unwrap() + 12.345678).abs() < 1e-12);
    assert_eq!((k["upper"].clone(), k["damping"].clone()), (Value::Null, json!(0.5)));
}

#[test]
fn a_current_value_the_dialog_refuses_is_named_as_the_joints() {
    let doc = with_hip(|j| j.gear_ratio = 0.005);
    let e = run_set_joint(&doc, &[("lower", json!(-30))]).unwrap_err();
    assert!(e.starts_with("Edit joint: hip's current gear_ratio (0.005) fills the parameter not given") && e.ends_with("; pass gear_ratio"), "{e}");
    // Given, the field is the caller's: refused as any given value is, or sent.
    assert!(run_set_joint(&doc, &[("lower", json!(-30)), ("gear_ratio", json!(0.005))]).is_err_and(|e| !e.contains("current")));
    assert!(run_set_joint(&doc, &[("lower", json!(-30)), ("gear_ratio", json!(1.5))]).is_ok());
}

#[test]
fn a_partial_rest_edit_joint_refuses_without_the_current_description() {
    // Read at an older revision: refused by name, nothing built.
    let mut old = document();
    old.robot.data.key = Some((old.generation, 3));
    let e = run_set_joint(&old, &[("lower", json!(-30))]).unwrap_err();
    assert!(e.starts_with("Edit joint: hip's current values fill the parameters not given") && e.ends_with("the robot description is still being read (revision 4); try again in a moment"), "{e}");
    // A joint the description does not have.
    let mut unknown = document();
    if let Some(Ok(s)) = unknown.robot.data.bundle.as_mut().map(|b| b.summary.as_mut()) {
        s.joints.clear();
    }
    assert!(run_set_joint(&unknown, &[("lower", json!(-30))]).is_err_and(|e| e.ends_with("hip is not in RoboCAD's robot description at revision 4, so its values are not known")));
}
