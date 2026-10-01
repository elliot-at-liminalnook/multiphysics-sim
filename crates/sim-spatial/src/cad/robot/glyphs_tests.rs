//! The robot glyphs' shapes (windowless): a revolute joint's axis line and
//! ring, a cable's sag, the triad's default axes, visibility and selection,
//! and the cache key.
use super::*;
use crate::cad::document::{CadDocument, CadTarget};
use serde_json::json;
use sim_runtime::cad_client::{DocState, NodeSummary};

fn document() -> CadDocument {
    CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()))
}

fn revolute() -> RobotJoint {
    RobotJoint { id: "j1".into(), kind: "revolute".into(), pivot: [10.0, 20.0, 30.0], axis: [0.0, 0.0, 2.0], ..Default::default() }
}

fn detail(id: &str, kind: &str, robot: serde_json::Value) -> NodeDetail {
    NodeDetail { summary: NodeSummary { id: id.into(), kind: kind.into(), ..Default::default() }, robot: Some(robot), ..Default::default() }
}

fn cable(from: [f64; 3], to: [f64; 3]) -> NodeDetail {
    detail("c1", "cable", json!({"kind": "cable", "from_body": "b1", "from_point": from, "to_body": "b2", "to_point": to, "length": 0.25, "mass": null, "stiffness": null, "damping": null, "segments": 4}))
}

#[test]
fn a_revolute_joint_has_an_axis_line_through_the_pivot_and_a_ring_around_it() {
    let g = joint_glyph(&revolute());
    let pivot = DVec3::new(10.0, 20.0, 30.0);
    assert_eq!((g.node.as_str(), g.kind, g.anchor), ("j1", Kind::Joint, pivot));
    // Axis line: 1.5 sizes each way along the (normalised) axis.
    let axis = &g.lines[0];
    assert_eq!(axis.scale, Scale::Size);
    assert_eq!(axis.color, [1.0, 0.75, 0.2], "RoboCAD's revolute colour");
    let pts = g.place(axis, 10.0, 0.5);
    assert_eq!(pts, vec![pivot - DVec3::Z * 15.0, pivot + DVec3::Z * 15.0]);
    for p in &pts {
        assert!((*p - pivot).cross(DVec3::Z).length() < 1e-12, "on the line through the pivot along the axis");
    }
    // The ring: 33 points one size from the pivot, square to the axis.
    let ring = g.place(&g.lines[1], 10.0, 0.5);
    assert_eq!(ring.len(), 33);
    for p in &ring {
        assert!(((*p - pivot).length() - 10.0).abs() < 1e-9);
        assert!((*p - pivot).dot(DVec3::Z).abs() < 1e-9);
    }
    // The dot: pixel-sized (9 px wide), whatever the glyph size.
    let dot = g.lines.iter().find(|l| l.scale == Scale::Pixels).unwrap();
    assert_eq!(g.place(dot, 10.0, 0.5), vec![pivot - DVec3::X * 2.25, pivot + DVec3::X * 2.25]);
    // Sizes as RoboCAD's: 18 px, 26 px selected.
    assert_eq!((Kind::Joint.size_px(false), Kind::Joint.size_px(true)), (18.0, 26.0));
}

#[test]
fn a_cable_sags_below_its_straight_chord() {
    let g = cable_glyph(&cable([0.0, 0.0, 100.0], [200.0, 0.0, 100.0])).unwrap();
    let line = &g.lines[0];
    assert_eq!(line.scale, Scale::Model);
    let pts = g.place(line, 1.0, 1.0);
    assert_eq!(pts.len(), 13);
    assert_eq!(pts[0], DVec3::new(0.0, 0.0, 100.0));
    assert!((pts[12] - DVec3::new(200.0, 0.0, 100.0)).length() < 1e-9);
    for p in &pts[1..12] {
        assert!(p.z < 100.0 - 1e-9, "every inner point lies below the chord: {p}");
    }
    // The middle drops 0.15 × the chord (200 mm → 30 mm).
    assert!((pts[6] - DVec3::new(100.0, 0.0, 70.0)).length() < 1e-9, "{}", pts[6]);
    // Selected, it is brighter.
    assert_eq!((line.color, line.selected), ([0.8, 0.45, 0.15], [0.95, 0.6, 0.2]));
    // A sensor is not a cable.
    assert!(cable_glyph(&detail("s1", "sensor", json!({"kind": "imu", "body": "b1", "point": [0, 0, 0]}))).is_none());
}

#[test]
fn glyphs_come_from_the_reads_and_follow_visibility_and_selection() {
    let summary = RobotSummary {
        joints: vec![revolute()],
        motors: vec![
            RobotMotor { id: "m1".into(), mount_point: Some([0.0, 0.0, 0.0]), shaft_tip: Some([0.0, 0.0, 12.0]), ..Default::default() },
            // No shaft tip: nothing to draw.
            RobotMotor { id: "m2".into(), mount_point: Some([0.0, 0.0, 0.0]), ..Default::default() },
        ],
        ..Default::default()
    };
    let sensors = vec![detail("s1", "sensor", json!({"kind": "imu", "body": "b1", "point": [1.0, 2.0, 3.0], "axes": null}))];
    let cables = vec![cable([0.0, 0.0, 0.0], [10.0, 0.0, 0.0])];
    let glyphs = glyph_lines(Some(&summary), &sensors, &cables);
    let nodes: Vec<(&str, Kind)> = glyphs.iter().map(|g| (g.node.as_str(), g.kind)).collect();
    assert_eq!(nodes, [("j1", Kind::Joint), ("m1", Kind::Motor), ("s1", Kind::Sensor), ("c1", Kind::Cable)]);
    assert_eq!(glyphs[1].lines[0].points, vec![DVec3::ZERO, DVec3::new(0.0, 0.0, 12.0)]);
    // A sensor without axes gets the world's, red, green, blue.
    let triad: Vec<(Vec<DVec3>, [f32; 3])> = glyphs[2].lines.iter().filter(|l| l.scale == Scale::Size).map(|l| (l.points.clone(), l.color)).collect();
    assert_eq!(triad, vec![(vec![DVec3::ZERO, DVec3::X], [1.0, 0.3, 0.3]), (vec![DVec3::ZERO, DVec3::Y], [0.3, 1.0, 0.3]), (vec![DVec3::ZERO, DVec3::Z], [0.3, 0.5, 1.0])]);

    // Shown only for effective_visible nodes of the shown tree; no tree, none.
    let mut doc = document();
    assert_eq!(style(&doc, &[], &glyphs), vec![(false, false); 4]);
    let node = |id: &str, visible: bool| NodeSummary { id: id.into(), effective_visible: visible, visible, ..Default::default() };
    doc.doc = Some(DocState { nodes: vec![node("j1", true), node("m1", false), node("s1", true), node("c1", true)], ..Default::default() });
    let selected = [SelectionItem("c1".into(), "body".into(), 0)];
    assert_eq!(style(&doc, &selected, &glyphs), vec![(true, false), (false, false), (true, false), (true, true)]);
}

#[test]
fn the_cache_key_changes_only_with_the_read_revision() {
    let mut doc = document();
    let unread = key(&doc);
    assert_eq!(unread, (doc.generation, None));
    doc.robot.data.key = Some((doc.generation, 4));
    let read = key(&doc);
    assert_ne!(read, unread);
    assert_eq!(read, (doc.generation, Some(4)));
    // A panel refresh, a hover or a status line is not a new read.
    doc.touch();
    doc.hover = Some(SelectionItem("j1".into(), "body".into(), 0));
    doc.show(Ok("done".into()));
    assert_eq!(key(&doc), read);
    // The reads at the next revision are.
    doc.robot.data.key = Some((doc.generation, 5));
    assert_eq!(key(&doc), (doc.generation, Some(5)));
}
