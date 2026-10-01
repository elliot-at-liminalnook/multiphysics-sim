//! Live dimensions (RoboCAD's `live_dimensions` and `edit_dimension_at`,
//! ui/app.py:654-705) for the numeric bar in the Select tool:
//!
//! - a cylindrical face: "Ø name", its diameter (`set_diameter`);
//! - a sphere or torus: "R name", shown read-only with RoboCAD's message;
//! - two planar faces of one node: "Distance" when parallel (the second
//!   moves, `set_distance`), else "Angle" (`set_angle`);
//! - a circular edge: "Ø edge i", the diameter of the cylinder it bounds
//!   (the face from `selection::faces_of_edge`).
//!
//! A double-click on a face in face mode puts its own dimension in the
//! bar, focused: a cylinder's diameter, or a planar face's distance to the
//! first opposite parallel planar face of the same node, the clicked face
//! moving. Values come from the shown revision's topology; nothing here
//! sends anything (Enter commits through `transform::commit`).
use super::{Field, FieldCommit, FieldKind, cursor_in_view, ray_hit};
use crate::app::actions::Act;
use crate::cad::actions::{CadAction, Dimension};
use crate::cad::document::{CadDocument, CadTool, SelectMode};
use crate::cad::measure::{face_angle, face_distance, parallel};
use crate::cad::mesh::{CadBody, CadMeshes};
use crate::cad::topology::{CadTopology, NodeTopology};
use crate::cad::view::CadView;
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::ray_cast::MeshRayCast;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use sim_runtime::cad_client::{FaceInfo, SelectionItem};
use std::time::{Duration, Instant};

/// Two presses closer than this in time and space are a double-click.
pub const DOUBLE_CLICK: Duration = Duration::from_millis(400);
pub const DOUBLE_CLICK_PIXELS: f32 = 5.0;
/// RoboCAD's message for a sphere's or torus's radius.
pub const ROUND_READ_ONLY: &str = "Sphere/torus radius editing: use Scale about the centre";

/// A double-clicked face's dimension, kept while that face stays selected
/// at the revision it was read at.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub node: String,
    pub face: i64,
    pub revision: u64,
    pub field: Field,
    /// The selection has become the face (the click's `CadSelect` landed).
    pub matched: bool,
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The selection's live dimensions.
pub fn live(doc: &CadDocument, topology: Option<&CadTopology>, meshes: Option<&CadMeshes>) -> Vec<Field> {
    let Some(topology) = topology else { return Vec::new() };
    // Indices first seen at an older revision may name other faces now (an edit can renumber them): no live field to commit against.
    if super::selection_revision(doc) != doc.shown_revision() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let refs: Vec<(String, &FaceInfo)> = doc.selected_of("face").into_iter().filter_map(|(node, index)| topology.get(&node)?.faces.iter().find(|f| f.index == index).map(|f| (node, f))).collect();
    for (node, f) in &refs {
        let name = doc.node_name(node);
        let radius = f.radius.filter(|r| *r != 0.0);
        if f.kind == "cylinder"
            && let Some(r) = radius
        {
            out.push(Field::new(format!("Ø {name}"), FieldKind::Length, 2.0 * r, FieldCommit::Dimension { node: node.clone(), dimension: Dimension::Diameter, faces: vec![f.index] }));
        } else if (f.kind == "sphere" || f.kind == "torus")
            && let Some(r) = radius
        {
            out.push(Field::new(format!("R {name}"), FieldKind::Length, r, FieldCommit::ReadOnly(ROUND_READ_ONLY.into())));
        }
    }
    if let [(na, a), (nb, b)] = refs.as_slice()
        && na == nb
        && a.kind == "plane"
        && b.kind == "plane"
    {
        let faces = vec![a.index, b.index];
        if parallel(a, b) {
            if let Some(d) = face_distance(a, b) {
                out.push(Field::new("Distance", FieldKind::Length, d, FieldCommit::Dimension { node: na.clone(), dimension: Dimension::Distance, faces }));
            }
        } else if let Some(m) = face_angle(a, b) {
            out.push(Field::new("Angle", FieldKind::Angle, m.value, FieldCommit::Dimension { node: na.clone(), dimension: Dimension::Angle, faces }));
        }
    }
    if let Some(meshes) = meshes {
        for (node, index) in doc.selected_of("edge") {
            let Some(t) = topology.get(&node) else { continue };
            let Some(r) = t.edges.iter().find(|e| e.index == index).and_then(|e| e.radius).filter(|r| *r != 0.0) else { continue };
            let faces = crate::cad::selection::faces_of_edge(meshes, topology, &node, index);
            let cylinder = faces.into_iter().find(|fi| t.faces.iter().any(|f| f.index == *fi && f.kind == "cylinder"));
            if let Some(face) = cylinder {
                out.push(Field::new(format!("Ø edge {index}"), FieldKind::Length, 2.0 * r, FieldCommit::Dimension { node: node.clone(), dimension: Dimension::Diameter, faces: vec![face] }));
            }
        }
    }
    out
}

/// RoboCAD's `edit_dimension_at` for face `face` of `node`.
pub fn edit_at(t: &NodeTopology, node: &str, face: i64) -> Option<Field> {
    let f = t.faces.iter().find(|x| x.index == face)?;
    if f.kind == "cylinder"
        && let Some(r) = f.radius.filter(|r| *r != 0.0)
    {
        return Some(Field::new("diameter", FieldKind::Length, 2.0 * r, FieldCommit::Dimension { node: node.into(), dimension: Dimension::Diameter, faces: vec![face] }));
    }
    if f.kind != "plane" {
        return None;
    }
    let n = f.normal?;
    let opposite = t.faces.iter().find(|g| g.kind == "plane" && g.normal.is_some_and(|m| (dot(m, n) + 1.0).abs() < 1e-3))?;
    let d = face_distance(f, opposite)?;
    // set_distance(nid, opposite, f, v): the clicked face moves.
    Some(Field::new("distance", FieldKind::Length, d, FieldCommit::Dimension { node: node.into(), dimension: Dimension::Distance, faces: vec![opposite.index, face] }))
}

/// Keep a double-clicked entry only while its face is the selection (once
/// the click's selection has landed), the tool is Select and the revision
/// is the one it was read at.
pub fn keep_entry(doc: &mut CadDocument) {
    let Some(entry) = &doc.tool_state.dimension else { return };
    let here = doc.selection.len() == 1 && doc.selection[0] == SelectionItem(entry.node.clone(), "face".into(), entry.face);
    let matched = entry.matched;
    let keep = doc.tool == CadTool::Select && entry.revision == doc.shown_revision() && (here || !matched);
    if !keep {
        doc.tool_state.dimension = None;
    } else if here && !matched {
        if let Some(e) = doc.tool_state.dimension.as_mut() {
            e.matched = true;
        }
    }
}

/// SimSync: a double-click on a face (Select tool, face mode) puts its
/// dimension in the numeric bar, focused.
#[allow(clippy::too_many_arguments)]
pub(super) fn double_click(
    doc: Option<ResMut<CadDocument>>,
    view: Option<Res<CadView>>,
    topology: Option<Res<CadTopology>>,
    meshes: Option<Res<CadMeshes>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    mut cast: MeshRayCast,
    bodies: Query<&CadBody>,
    mut out: MessageWriter<Act<CadAction>>,
    mut last: Local<Option<(Instant, Vec2)>>,
) {
    let (Some(mut doc), Some(view), Some(topology), Some(meshes)) = (doc, view, topology, meshes) else { return };
    // A catalogue interaction (a placement, a pick-then-form tool) owns the clicks.
    if doc.tool != CadTool::Select || doc.select_mode != SelectMode::Face || doc.ops.active.is_some() {
        *last = None;
        return;
    }
    if !buttons.is_some_and(|b| b.just_pressed(MouseButton::Left)) {
        return;
    }
    let Some(cursor) = cursor_in_view(windows.single().ok(), &view, hover.as_deref(), &nodes) else { return };
    let double = last.is_some_and(|(at, p)| at.elapsed() <= DOUBLE_CLICK && p.distance(cursor) <= DOUBLE_CLICK_PIXELS);
    if !double {
        *last = Some((Instant::now(), cursor));
        return;
    }
    *last = None;
    let Some(hit) = ray_hit(&doc, &mut cast, &view, cursor, &bodies) else { return };
    let shown = doc.shown_revision();
    if meshes.drawn_revision(&hit.node).is_some_and(|r| r != shown) {
        let name = doc.node_name(&hit.node);
        doc.show(Err(format!("{name} is being redrawn for revision {shown}; double-click again in a moment")));
        return;
    }
    let Some(face) = hit.triangle.and_then(|t| meshes.face_at(&hit.node, t, shown)) else { return };
    let Some(t) = topology.get(&hit.node) else {
        let name = doc.node_name(&hit.node);
        doc.show(Err(format!("The faces of {name} are still loading; double-click again in a moment")));
        return;
    };
    // RoboCAD selects the face either way.
    out.write(Act::ui(CadAction::CadSelect { ids: Vec::new(), items: vec![SelectionItem(hit.node.clone(), "face".into(), face)], extend: false, toggle: false }));
    let Some(field) = edit_at(t, &hit.node, face) else { return };
    let revision = doc.shown_revision();
    doc.tool_state.dimension = Some(Entry { node: hit.node, face, revision, field, matched: false });
    doc.tool_state.numeric.focus_request = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(index: i64, kind: &str, centroid: [f64; 3], normal: [f64; 3], radius: Option<f64>) -> FaceInfo {
        FaceInfo { index, kind: kind.into(), centroid: Some(centroid), normal: Some(normal), radius, ..Default::default() }
    }

    #[test]
    fn a_double_clicked_face_gives_its_diameter_or_its_distance_to_the_opposite_face() {
        let t = NodeTopology {
            revision: 3,
            faces: vec![face(0, "plane", [0.0, 0.0, 0.0], [0.0, 0.0, -1.0], None), face(1, "plane", [0.0, 0.0, 12.0], [0.0, 0.0, 1.0], None), face(2, "cylinder", [0.0, 0.0, 6.0], [1.0, 0.0, 0.0], Some(4.0)), face(3, "bspline", [0.0; 3], [0.0, 1.0, 0.0], None)],
            ..Default::default()
        };
        let d = edit_at(&t, "b1", 1).unwrap();
        assert_eq!((d.name.as_str(), d.value), ("distance", 12.0));
        assert_eq!(d.commit, FieldCommit::Dimension { node: "b1".into(), dimension: Dimension::Distance, faces: vec![0, 1] });
        let c = edit_at(&t, "b1", 2).unwrap();
        assert_eq!((c.name.as_str(), c.value), ("diameter", 8.0));
        assert!(edit_at(&t, "b1", 3).is_none());
    }
}
