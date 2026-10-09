//! The robot tools' tests (windowless): RoboCAD's MotorTool and
//! JointTool geometry, a face pick at another revision refused, the
//! validation verdict, the motor library's rows and the controls.
use super::*;
use crate::cad::document::{CadTarget, Connection};
use crate::cad::types::{DocState, Health, MotorSpec, NodeSummary, RobotIssue};
use std::collections::BTreeMap;

fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), revision: 4, ..Default::default() });
    doc.doc = Some(DocState { nodes: vec![NodeSummary { id: "b1".into(), kind: "body".into(), name: "Bracket".into(), visible: true, effective_visible: true, ..Default::default() }], revision: 4, ..Default::default() });
    doc.doc_key = Some((None, 4));
    doc
}

fn plane_face(normal: [f64; 3]) -> FaceInfo {
    FaceInfo { index: 2, kind: "plane".into(), normal: Some(normal), ..Default::default() }
}

/// A cylinder of axis +Z through (10, 0, 0).
fn cylinder() -> FaceInfo {
    FaceInfo { index: 3, kind: "cylinder".into(), normal: Some([1.0, 0.0, 0.0]), axis_point: Some([10.0, 0.0, 0.0]), axis_dir: Some([0.0, 0.0, 2.0]), radius: Some(5.0), ..Default::default() }
}

#[test]
fn the_motor_shaft_points_into_the_body() {
    // A flat face: the shaft along −normal (unit).
    assert_eq!(motor_mount(&plane_face([0.0, 0.0, 2.0]), [1.0, 2.0, 3.0]), Ok([-0.0, -0.0, -1.0]));
    // A cylinder: radial from the axis through the point, inwards.
    let s = motor_mount(&cylinder(), [10.0, 5.0, 7.0]).unwrap();
    assert!((s[0]).abs() < 1e-12 && (s[1] + 1.0).abs() < 1e-12 && s[2].abs() < 1e-12, "{s:?}");
    // No normal: refused, nothing placed.
    assert!(motor_mount(&FaceInfo::default(), [0.0; 3]).is_err());
}

#[test]
fn the_joint_axis_is_a_cylinders_axis_or_a_faces_normal() {
    // A cylinder: the click projected onto its axis, the axis unit.
    assert_eq!(joint_axis(&cylinder(), [13.0, 4.0, 7.5]), Ok(([10.0, 0.0, 7.5], [0.0, 0.0, 1.0])));
    // A flat face: its normal through the click.
    assert_eq!(joint_axis(&plane_face([0.0, 3.0, 0.0]), [1.0, 2.0, 3.0]), Ok(([1.0, 2.0, 3.0], [0.0, 1.0, 0.0])));
}

#[test]
fn a_face_pick_at_another_revision_is_refused() {
    let doc = document();
    let face = SelectionItem("b1".into(), "face".into(), 2);
    assert_eq!(check_pick(&doc, &face, Some(4)), Ok(()));
    assert!(check_pick(&doc, &face, Some(3)).is_err_and(|e| e.contains("picked at revision 3") && e.contains("now 4")));
    assert!(check_pick(&doc, &face, None).is_err_and(|e| e.starts_with("pass picked_at")));
    // A body needs no revision; an unknown node is refused.
    assert_eq!(check_pick(&doc, &SelectionItem("b1".into(), "body".into(), 0), None), Ok(()));
    assert!(check_pick(&doc, &SelectionItem("zz".into(), "body".into(), 0), None).is_err());
}

#[test]
fn validation_is_robocads_status_or_its_issues() {
    let ok = RobotSummary { links: 3, dof: Some(2), joints: Vec::new(), ..Default::default() };
    assert_eq!(verdict(&ok), (true, "robot valid: 3 bodies, 0 joints, 2 DoF".to_string(), Vec::new()));
    let closed = RobotSummary { links: 4, dof: None, ..Default::default() };
    assert_eq!(verdict(&closed).1, "robot valid: 4 bodies, 0 joints, closed-loop mobility requires constraint analysis");
    let bad = RobotSummary { issues: vec![RobotIssue { severity: "error".into(), message: "hip: child body is missing".into(), node: None }, RobotIssue { severity: "warning".into(), message: "no ground".into(), node: None }], ..Default::default() };
    let (valid, message, issues) = verdict(&bad);
    assert!(!valid);
    assert_eq!(issues, ["[error] hip: child body is missing", "[warning] no ground"]);
    assert_eq!(message, "Robot validation: [error] hip: child body is missing; [warning] no ground");
}

#[test]
fn the_library_rows_are_robocads_text() {
    let mut doc = document();
    assert_eq!(library::json(&doc), json!({"reading": true}));
    let mut lib = BTreeMap::new();
    lib.insert("ds3218".to_string(), MotorSpec { name: "DS3218 servo".into(), kind: "servo".into(), stall_torque: 2.0, no_load_speed: 6.98, mass_g: 60.0, notes: "20 kg·cm".into(), ..Default::default() });
    doc.robot.data.motors = Some((doc.generation, Ok(lib)));
    let rows = library::rows(&doc).unwrap();
    assert_eq!(rows, [format!("{:<28} {:<13} {:>7} N·m {:>6} rad/s {:>6} g   {}", "DS3218 servo", "servo", "2", "6.98", "60", "20 kg·cm")]);
}

#[test]
fn the_tools_controls_validate_and_toggle_the_library() {
    let mut doc = document();
    let c = controls(&doc);
    assert_eq!(c[0].0, "cad:robot:validate");
    assert_eq!(c[0].2, RobotArgs::of(RobotOp::Validate, None));
    assert_eq!((c[1].0.as_str(), c[1].2.clone()), ("cad:robot:library", RobotArgs::of(RobotOp::Library, Some(true))));
    doc.robot.tools.library_open = true;
    assert_eq!(controls(&doc)[1].2, RobotArgs::of(RobotOp::Library, Some(false)));
    // The click tools start over.
    doc.robot.tools.joint = JointPicks { stage: 2, parent: None, child: Some("b1".into()) };
    doc.robot.tools.reset_picks();
    assert_eq!(doc.robot.tools.joint, JointPicks::default());
    // A face click wants a face; the joint tool's first two clicks a body.
    assert!(click::wants_face(RobotTool::Motor, 0) && !click::wants_face(RobotTool::Joint, 1) && click::wants_face(RobotTool::Joint, 2));
}
