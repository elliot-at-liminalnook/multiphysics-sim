//! The Robot panel without a window: RoboCAD's texts from a filled
//! description, a row press and REST `cad_select` leave the same shared
//! selection, every control round-trips through its REST form, the
//! double-click rule and the show/hide toggle.
use super::*;
use crate::app::actions::{self as app_actions, Action, Origin, Replies};
use crate::cad::document::CadTarget;
use crate::cad::rest_form::rest_form;
use crate::cad::robot::data::Bundle;
use crate::cad::selection::Fixture;
use crate::cad::types::{Battery, DocState, NodeResult, NodeResults, NodeSummary, RobotJoint, RobotMotor};
use std::collections::BTreeMap;

fn node(id: &str, kind: &str, name: &str, material: Option<&str>) -> NodeSummary {
    NodeSummary { id: id.into(), kind: kind.into(), name: name.into(), material: material.map(str::to_string), visible: true, effective_visible: true, ..Default::default() }
}

/// RoboCAD's answers: two links, a motor on the base driving the elbow, an
/// IMU on the arm, a 3S battery and results with the arm's margins.
fn bundle() -> Bundle {
    let summary = RobotSummary {
        joints: vec![RobotJoint {
            id: "j1".into(),
            name: "Elbow".into(),
            kind: "revolute".into(),
            parent: Some("b1".into()),
            child: "b2".into(),
            lower: Some(-std::f64::consts::FRAC_PI_2),
            upper: Some(std::f64::consts::FRAC_PI_2),
            motor: Some("m1".into()),
            parent_name: Some("Base".into()),
            child_name: Some("Arm".into()),
            motor_name: Some("Servo".into()),
            ..Default::default()
        }],
        motors: vec![RobotMotor { id: "m1".into(), name: "Servo".into(), spec: Some("sts3215".into()), spec_name: Some("STS3215".into()), mounted_on: Some("b1".into()), drives: Some("j1".into()), ..Default::default() }],
        links: 3,
        dof: Some(1),
        ground: vec!["b1".into()],
        validation_scope: "topology only; exact geometry checks are explicit".into(),
        ..Default::default()
    };
    let margins = Margins { yield_margin: Some(1.234), tg_margin_c: Some(12.4), ..Default::default() };
    let arm = NodeResult { results: json!({"section": "links", "peak_stress_pa": 12.34e6, "peak_temperature_c": 40.6}), yield_strength_pa: None };
    let results = NodeResults { revision: 3, path: Some("/tmp/runs/walk.simresult.json".into()), stale: Some(false), margins: BTreeMap::from([("b2".to_string(), margins)]), nodes: BTreeMap::from([("b2".to_string(), arm)]), ..Default::default() };
    let imu = NodeDetail { summary: node("s1", "sensor", "IMU", None), robot: Some(json!({"kind": "imu", "body": "b2", "point": [0.0, 0.0, 0.0]})), ..Default::default() };
    Bundle {
        summary: Ok(summary),
        results: Ok(results),
        sensors: Ok(vec![imu]),
        cables: Ok(Vec::new()),
        battery: Ok(Some(Battery { cells: 3, chemistry: "LiPo".into(), nominal_voltage: 11.1, ..Default::default() })),
        control: Ok(None),
        uncertainty: Ok(None),
        profiles: Ok(Value::Null),
        motors: None,
    }
}

/// Not connected (nothing is pushed to RoboCAD), the tree and the
/// description both at revision 3.
fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    doc.doc = Some(DocState {
        nodes: vec![node("b1", "body", "Base", Some("pla")), node("b2", "body", "Arm", None), node("m1", "body", "Servo", None), node("j1", "joint", "Elbow", None), node("s1", "sensor", "IMU", None)],
        materials: vec![json!({"id": "pla", "name": "PLA"})],
        revision: 3,
        ..Default::default()
    });
    doc.doc_key = Some((None, 3));
    doc.robot.data.key = Some((doc.generation, 3));
    doc.robot.data.bundle = Some(bundle());
    doc
}

fn rows(v: &View, title: &str) -> Vec<(String, String, String)> {
    v.branches.iter().find(|b| b.title == title).map_or_else(Vec::new, |b| b.rows.iter().map(|r| (r.id.clone(), r.detail.clone(), r.margin.clone())).collect())
}

#[test]
fn the_panel_shows_robocads_summary_tree_margins_and_validity() {
    let doc = document();
    let v = view(&doc, &[]);
    assert_eq!(v.read.text, "RoboCAD's robot description at revision 3");
    assert_eq!(v.summary.as_deref(), Some("3 bodies, 1 joints, 1 DoF, 1 motors, 1 sensors, 0 cables. Ground: Base. Power: 11.1 V LiPo.  Results: walk.simresult.json"));
    // Links: the bodies, the motor body excepted; the margins as margin_text.
    assert_eq!(rows(&v, "Links"), vec![("b1".into(), "PLA".into(), String::new()), ("b2".into(), "—, peak 12.3 MPa, 41 °C".into(), "yield +1.23  Tg +12°C".into())]);
    assert_eq!(rows(&v, "Joints"), vec![("j1".into(), "revolute: Base → Arm  [-90°, 90°]  motor Servo".into(), String::new())]);
    assert_eq!(rows(&v, "Motors"), vec![("m1".into(), "STS3215: on Base, drives Elbow".into(), String::new())]);
    assert_eq!(rows(&v, "Sensors & cables"), vec![("s1".into(), "imu on Arm".into(), String::new())]);
    assert_eq!(v.issues, vec![Line::new("✓ robot is valid", Tone::Good)]);
    // RoboCAD's buttons in its order (the four non-catalogue ones once their actions exist).
    let labels: Vec<&str> = v.buttons.iter().map(|b| b.label).collect();
    assert_eq!(&labels[..10], ["Add joint…", "Add motor…", "Joint from selection…", "Infer joints", "Assign motor…", "Fix together", "Toggle ground", "Add sensor…", "Add cable…", "Battery / control…"]);
    // No document open: every edit says so; the stress overlay (display only) does not;
    // the simulator export says there is no document to export.
    for b in &v.buttons {
        match b.id {
            "view.stress" => assert!(b.ready.is_ok()),
            "sim.export" => assert_eq!(b.ready, Err(crate::cad::results::NO_DOCUMENT.to_string())),
            _ => assert!(b.ready.as_ref().is_err_and(|e| e.contains("no CAD document is open")), "{}: {:?}", b.id, b.ready),
        }
    }
    // A failed read is named, never shown as a value.
    let mut doc = document();
    if let Some(b) = doc.robot.data.bundle.as_mut() {
        b.sensors = Err("GET /sensors: 500".into());
        b.summary.as_mut().unwrap().issues = vec![crate::cad::types::RobotIssue { severity: "error".into(), message: "Elbow: child body is missing".into(), node: Some("j1".into()) }];
    }
    let v = view(&doc, &[]);
    assert!(v.summary.as_deref().unwrap().contains("? sensors"));
    assert!(v.notes.iter().any(|n| n.text.contains("Sensors could not be read") && n.text.contains("500")));
    assert_eq!(v.issues, vec![Line::new("Error: Elbow: child body is missing", Tone::Bad)]);
}

#[test]
fn margins_and_numbers_format_as_robocad() {
    let m = Margins { bearing_margin: Some(-0.456), screw_shear_margin: Some(2.0), stall_margin: Some(0.5), mount_tg_margin_c: Some(-3.6), ..Default::default() };
    assert_eq!(margin_text(Some(&m)), "bearing -0.46  screw +2.00  stall +0.50  mount Tg -4°C");
    assert_eq!(margin_text(None), "");
    assert_eq!(g(11.1), "11.1");
    assert_eq!(g(7.4), "7.4");
    assert_eq!(g(12.0), "12");
}

/// Applies `action` through the one handler REST and `system_ui` use.
fn apply(action: &CadAction, doc: &mut CadDocument, f: &mut Fixture) -> Outcome {
    let mut plane = crate::cad::sketch::CadActivePlane::default();
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    let mut cx = Cx { settings: &mut crate::app::settings::SettingsOwner::default(), doc, shared: f.shared(), meshes: None, topology: None, view: None, plane: &mut plane, sketches: None, display: None, views: None, files: None, components: &mut crate::cad::components::ComponentsState::default(), composition: &mut crate::cad::composition::CadCompositionState::default(), experiments: &mut crate::cad::experiments::ExperimentsState::default(), review: &mut crate::cad::experiment_review::ReviewState::default(), motion: &mut crate::cad::motion::MotionState::default(), camera: Vec::new() };
    crate::cad::actions::handle(action, &mut call, &mut cx)
}

#[test]
fn a_row_press_and_rest_cad_select_give_the_same_selection() {
    let mut doc = document();
    let mut clicked = Fixture::at(3);
    let row = controls(&doc, &[]).into_iter().find(|c| c.0 == "cad:robot:row:j1").expect("the joint's row is listed").2;
    assert!(matches!(apply(&row, &mut doc, &mut clicked), Outcome::Done(Ok(_))));
    let mut other = document();
    let mut rest = Fixture::at(3);
    let parsed = <CadAction as Action>::parse(&sim_api::Command { command: "cad_select".into(), args: json!({"ids": ["j1"]}) }).unwrap();
    assert!(matches!(apply(&parsed, &mut other, &mut rest), Outcome::Done(Ok(_))));
    assert_eq!(clicked.items(), vec![SelectionItem("j1".into(), "body".into(), 0)]);
    assert_eq!(clicked.selection.cad_stamped(clicked.id()), rest.selection.cad_stamped(rest.id()));
    // The selected row is lit.
    let v = view(&doc, &clicked.items());
    assert!(v.branches.iter().flat_map(|b| &b.rows).all(|r| r.selected == (r.id == "j1")));
}

#[test]
fn every_robot_panel_control_fits_a_pattern_and_round_trips_through_rest() {
    let doc = document();
    let controls = controls(&doc, &[]);
    let ids: Vec<&str> = controls.iter().map(|c| c.0.as_str()).collect();
    for expected in ["cad:robot:panel", "cad:robot:refresh", "cad:robot:row:b1", "cad:robot:row:j1", "cad:robot:row:m1", "cad:robot:row:s1", "cad:robot:button:robot.add_joint", "cad:robot:button:robot.power"] {
        assert!(ids.contains(&expected), "{expected} is not listed: {ids:?}");
    }
    let patterns = <CadAction as Action>::controls();
    for (id, _, action, _) in &controls {
        assert!(patterns.iter().any(|p| app_actions::control_matches(p, id)), "{id} fits no registered pattern");
        let Value::Object(mut args) = rest_form(action) else { panic!("{id}: not an object") };
        let name = args.remove("command").and_then(|v| v.as_str().map(str::to_string)).expect("a command name");
        let parsed = <CadAction as Action>::parse(&sim_api::Command { command: name.clone(), args: Value::Object(args) }).unwrap_or_else(|e| panic!("{id}: {name} does not parse: {e}"));
        // `picked_at` is not a REST argument (the window's stamp; a row's items are body items, which it does not touch).
        let mut clicked = action.clone();
        if let CadAction::CadSelect { picked_at, .. } = &mut clicked {
            *picked_at = None;
        }
        assert_eq!(parsed, clicked, "{id}: REST form and click differ");
    }
}

#[test]
fn a_row_press_carries_the_revision_the_description_was_read_at() {
    let mut doc = document();
    // The tree has moved on to revision 5; the description is still revision 3's.
    doc.doc_key = Some((None, 5));
    assert_eq!(doc.robot.data.read_at(), Some(3));
    let press = controls(&doc, &[]).into_iter().find(|c| c.0 == "cad:robot:row:j1").expect("the joint's row is listed").2;
    assert_eq!(press, CadAction::CadSelect { ids: vec!["j1".into()], items: Vec::new(), extend: false, toggle: false, picked_at: doc.robot.data.read_at() });
    assert_eq!(select_action(&doc, "j1"), press, "the drawn row's button writes the same action");
    // Its node is a body item: selected, with no index revision to refuse.
    let mut f = Fixture::at(5);
    assert!(matches!(apply(&press, &mut doc, &mut f), Outcome::Done(Ok(_))));
    assert_eq!(f.items(), vec![SelectionItem("j1".into(), "body".into(), 0)]);
    // Nothing read yet: no stamp.
    let mut unread = document();
    unread.robot.data.key = None;
    assert!(matches!(select_action(&unread, "j1"), CadAction::CadSelect { picked_at: None, .. }));
}

#[test]
fn a_joint_rows_double_click_opens_edit_joint() {
    let t0 = Instant::now();
    let mut last = None;
    assert_eq!(press(&mut last, "j1", true, t0), None);
    assert_eq!(press(&mut last, "j1", true, t0 + Duration::from_millis(300)), Some(CadAction::CadInvoke { id: "ops.set_joint".into() }));
    // A third press starts again; too slow, another row or a link is no double-click.
    assert_eq!(press(&mut last, "j1", true, t0 + Duration::from_millis(400)), None);
    assert_eq!(press(&mut last, "j1", true, t0 + Duration::from_millis(900)), None);
    assert_eq!(press(&mut last, "j2", true, t0 + Duration::from_millis(1000)), None);
    assert_eq!(press(&mut last, "b1", false, t0 + Duration::from_millis(1100)), None);
    assert_eq!(press(&mut last, "b1", false, t0 + Duration::from_millis(1200)), None);
}

#[test]
fn the_panel_toggles_and_a_hidden_panel_lists_only_its_toggle_and_refresh() {
    let mut doc = document();
    let mut f = Fixture::at(3);
    let toggle = CadAction::CadRobot(RobotArgs { op: RobotOp::Panel, ..RobotArgs::default() });
    let r0 = doc.revision;
    assert!(matches!(apply(&toggle, &mut doc, &mut f), Outcome::Done(Ok(_))));
    assert!(!doc.robot.panel.open && doc.revision > r0);
    let ids: Vec<String> = controls(&doc, &[]).into_iter().map(|c| c.0).collect();
    assert_eq!(ids, ["cad:robot:panel", "cad:robot:refresh"]);
    assert_eq!(state_json(&doc)["open"], json!(false));
    let show = CadAction::CadRobot(RobotArgs { op: RobotOp::Panel, open: Some(true), ..RobotArgs::default() });
    let _ = apply(&show, &mut doc, &mut f);
    let _ = apply(&show, &mut doc, &mut f);
    assert!(doc.robot.panel.open, "open: true shows, never toggles");
}
