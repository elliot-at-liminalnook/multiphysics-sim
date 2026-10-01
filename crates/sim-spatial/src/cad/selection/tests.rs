//! Selection without a window: the handler's arms, box select, edges →
//! faces, the push body and adoption.
use super::*;
use crate::cad::document::{CadTarget, Candidates, Connection};
use crate::jobs::Job;
use bevy::camera::CameraProjection;
use bevy::math::Affine3A;
use sim_runtime::cad_client::{DocState, EdgeInfo, Health, NodeSummary, Selection, VertexInfo};
use std::time::{Duration, Instant};
use super::super::topology::NodeTopology;

fn node(id: &str, kind: &str, material: Option<&str>, visible: bool) -> NodeSummary {
    NodeSummary { id: id.into(), kind: kind.into(), name: format!("N{id}"), visible, effective_visible: visible, material: material.map(str::to_string), ..Default::default() }
}

/// Not connected (nothing is pushed): bodies b1, b2 (PLA), b3 (hidden),
/// a sheet s1, a sketch k1 and a mesh m1.
fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    doc.doc = Some(DocState {
        nodes: vec![node("b1", "body", Some("pla"), true), node("b2", "body", Some("pla"), true), node("b3", "body", Some("pla"), false), node("s1", "sheet", None, true), node("k1", "sketch", None, true), node("m1", "mesh", None, true), node("i1", "instance", Some("al"), true)],
        revision: 3,
        ..Default::default()
    });
    doc.doc_key = Some((None, 3));
    doc
}

fn item(n: &str, k: &str, i: i64) -> SelectionItem {
    SelectionItem(n.into(), k.into(), i)
}

#[test]
fn select_replaces_extends_toggles_and_ids_are_body_items() {
    let mut doc = document();
    doc.candidates = Some(Candidates { items: vec![item("b1", "face", 0)], extend: false, toggle: false });
    let r0 = doc.revision;
    let answer = select(&mut doc, &["b1".into()], &[], false, false).unwrap();
    assert_eq!(doc.selection, vec![item("b1", "body", 0)]);
    assert!(doc.candidates.is_none(), "a choice closes the Alt menu");
    assert!(doc.revision > r0);
    assert_eq!(answer["connected"], json!(false));
    assert_eq!(answer["pushed"], json!(false));
    // Shift appends (once), Ctrl toggles.
    select(&mut doc, &[], &[item("b2", "face", 4), item("b1", "body", 0)], true, false).unwrap();
    assert_eq!(doc.selection, vec![item("b1", "body", 0), item("b2", "face", 4)]);
    select(&mut doc, &[], &[item("b1", "body", 0), item("b2", "edge", 1)], false, true).unwrap();
    assert_eq!(doc.selection, vec![item("b2", "face", 4), item("b2", "edge", 1)]);
    assert_eq!(doc.status, Some(Ok("2 selected".to_string())));
    // Replace; empty clears.
    select(&mut doc, &[], &[item("b2", "vertex", 3)], false, false).unwrap();
    assert_eq!(doc.selection, vec![item("b2", "vertex", 3)]);
    select(&mut doc, &[], &[], false, false).unwrap();
    assert!(doc.selection.is_empty() && doc.status.is_none());
    // Refusals name the item.
    let e = select(&mut doc, &[], &[item("b1", "solid", 0)], false, false).unwrap_err();
    assert!(e.contains("solid") && e.contains("b1"), "{e}");
    let e = select(&mut doc, &["nope".into()], &[], false, false).unwrap_err();
    assert!(e.contains("nope"), "{e}");
    let e = select(&mut doc, &[], &[item("b1", "face", -1)], false, false).unwrap_err();
    assert!(e.contains("negative"), "{e}");
}

#[test]
fn a_mode_switch_clears_the_selection_hover_and_menu() {
    let mut doc = document();
    doc.selection = vec![item("b1", "body", 0)];
    doc.hover = Some(item("b2", "body", 0));
    doc.candidates = Some(Candidates::default());
    set_mode(&mut doc, SelectMode::Edge);
    assert_eq!(doc.select_mode, SelectMode::Edge);
    assert!(doc.selection.is_empty() && doc.hover.is_none() && doc.candidates.is_none());
    assert_eq!(doc.status, Some(Ok("Selection mode: edge".to_string())));
    // A hover is display only: no panel refresh.
    let r = doc.revision;
    hover(&mut doc, Some(&item("b1", "edge", 2)));
    assert_eq!(doc.hover, Some(item("b1", "edge", 2)));
    assert_eq!(doc.revision, r);
}

#[test]
fn select_all_and_invert_take_visible_bodies_sheets_curves_instances_and_meshes() {
    let mut doc = document();
    select_all(&mut doc);
    let ids: Vec<&str> = doc.selection.iter().map(|i| i.0.as_str()).collect();
    assert_eq!(ids, ["b1", "b2", "s1", "m1", "i1"]);
    assert!(doc.selection.iter().all(|i| i.1 == "body" && i.2 == 0));
    doc.selection = vec![item("b1", "face", 2), item("s1", "body", 0)];
    invert(&mut doc);
    let ids: Vec<&str> = doc.selection.iter().map(|i| i.0.as_str()).collect();
    assert_eq!(ids, ["b2", "m1", "i1"]);
}

#[test]
fn same_material_takes_the_first_selected_nodes_material() {
    let mut doc = document();
    assert!(same_material(&mut doc).unwrap_err().contains("nothing is selected"));
    doc.selection = vec![item("b2", "face", 1)];
    let answer = same_material(&mut doc).unwrap();
    assert_eq!(answer["material"], json!("pla"));
    // Hidden b3 too (RoboCAD's same_material does not filter visibility).
    let ids: Vec<&str> = doc.selection.iter().map(|i| i.0.as_str()).collect();
    assert_eq!(ids, ["b1", "b2", "b3"]);
    doc.selection = vec![item("s1", "body", 0)];
    let e = same_material(&mut doc).unwrap_err();
    assert!(e.contains("Ns1") && e.contains("no material"), "{e}");
}

/// Face 0: a 10 mm square in z = 0; face 1: a square in y = 0 sharing
/// the edge (0,0,0)–(10,0,0); face 2: a square in x = 10 meeting the
/// edge only at its end (10,0,0).
fn two_faces() -> MeshData {
    MeshData {
        vertices: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, -10.0], [10.0, 0.0, -10.0], [10.0, 10.0, -10.0], [10.0, 5.0, 0.0]],
        triangles: vec![[0, 1, 2], [0, 2, 3], [0, 4, 5], [0, 5, 1], [1, 5, 6], [1, 6, 7]],
        triangle_face: vec![0, 0, 1, 1, 2, 2],
        face_count: 3,
    }
}

#[test]
fn edges_become_the_faces_their_triangles_share() {
    let mesh = two_faces();
    assert_eq!(faces_along(&mesh, &[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]), vec![0, 1]);
    // A sampled arc's sag is allowed for: a polyline bowed 0.05 mm still finds them.
    assert_eq!(faces_along(&mesh, &[[0.0, 0.0, 0.0], [5.0, 0.05, 0.0], [10.0, 0.0, 0.0]]), vec![0, 1]);
    // The vertical edge at x = 0 (0,0,0)–(0,0,−10) bounds face 1 only.
    assert_eq!(faces_along(&mesh, &[[0.0, 0.0, 0.0], [0.0, 0.0, -10.0]]), vec![1]);
    assert!(faces_along(&mesh, &[[0.0, 0.0, 0.0]]).is_empty());

    // Through the action: needs the mesh and the topology at the same revision.
    let mut doc = document();
    doc.select_mode = SelectMode::Edge;
    doc.selection = vec![item("b1", "edge", 7)];
    let mut meshes = CadMeshes::default();
    let mut topology = CadTopology::default();
    let e = edges_to_faces(&mut doc, Some(&meshes), Some(&topology)).unwrap_err();
    assert!(e.contains("Nb1") && e.contains("loading"), "{e}");
    topology.insert("b1", NodeTopology { revision: 3, edges: vec![EdgeInfo { index: 7, points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]], ..Default::default() }], ..Default::default() });
    let e = edges_to_faces(&mut doc, Some(&meshes), Some(&topology)).unwrap_err();
    assert!(e.contains("tessellation of Nb1"), "{e}");
    meshes.insert_drawn("b1", 3, mesh);
    assert_eq!(faces_of_edge(&meshes, &topology, "b1", 7), vec![0, 1]);
    edges_to_faces(&mut doc, Some(&meshes), Some(&topology)).unwrap();
    assert_eq!(doc.select_mode, SelectMode::Face);
    assert_eq!(doc.selection, vec![item("b1", "face", 0), item("b1", "face", 1)]);
    assert!(edges_to_faces(&mut doc, Some(&meshes), Some(&topology)).unwrap_err().contains("no edges"));
}

/// A 0.5 mm wall: the top strip (face 0) and the outer side (face 1)
/// meet at the edge y = 0, z = 0; the inner side (face 2) runs 0.5 mm
/// away and is not taken; a sliver (face 3) touching the edge across it
/// (a side 0.05 mm long, within the tolerance but perpendicular) is not
/// taken either.
#[test]
fn a_thin_walls_far_side_is_not_an_edges_face() {
    let mesh = MeshData {
        vertices: vec![
            [0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 0.5, 0.0], [0.0, 0.5, 0.0],
            [0.0, 0.0, -10.0], [10.0, 0.0, -10.0], [10.0, 0.5, -10.0], [0.0, 0.5, -10.0],
            [0.0, 0.05, 0.0],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3], [0, 4, 5], [0, 5, 1], [3, 2, 6], [3, 6, 7], [0, 8, 4]],
        triangle_face: vec![0, 0, 1, 1, 2, 2, 3],
        face_count: 4,
    };
    assert_eq!(faces_along(&mesh, &[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]), vec![0, 1]);
    // The inner side's own top edge takes the strip and the inner side only.
    assert_eq!(faces_along(&mesh, &[[0.0, 0.5, 0.0], [10.0, 0.5, 0.0]]), vec![0, 2]);
}

/// As view.rs's test view: a camera 5 m from the model origin over a
/// 200 × 100 px view at (10, 20); the origin projects to (110, 70) and
/// 100 mm is about 2.4 px there.
fn view() -> CadView {
    let projection = PerspectiveProjection { aspect_ratio: 2.0, ..default() };
    let clip_from_view = projection.get_clip_from_view();
    let world_from_view = Affine3A::from_translation(Vec3::new(0.0, 0.0, 5.0));
    let root = super::super::mesh::root_transform();
    let world_from_model = Affine3A::from_scale_rotation_translation(root.scale, root.rotation, root.translation);
    CadView {
        valid: true,
        world_from_model,
        model_from_world: world_from_model.inverse(),
        view_from_world: world_from_view.inverse(),
        world_from_view,
        clip_from_view,
        view_from_clip: clip_from_view.inverse(),
        min: Vec2::new(10.0, 20.0),
        size: Vec2::new(200.0, 100.0),
    }
}

fn cube(centre: [f64; 3], half: f64) -> MeshData {
    let [x, y, z] = centre;
    MeshData { vertices: vec![[x - half, y - half, z - half], [x + half, y + half, z + half], [x + half, y - half, z - half]], triangles: vec![[0, 1, 2]], triangle_face: vec![0], face_count: 1 }
}

#[test]
fn box_select_takes_bodies_edges_and_vertices_inside_the_rectangle() {
    let mut doc = document();
    let mut meshes = CadMeshes::default();
    // b1 at the origin (inside), b2 2 m to the right (about 48 px: outside).
    meshes.insert_drawn("b1", 3, cube([0.0, 0.0, 0.0], 100.0));
    meshes.insert_drawn("b2", 3, cube([2000.0, 0.0, 0.0], 100.0));
    let v = view();
    let rect = [120.0, 80.0, 100.0, 60.0];
    doc.selection = vec![item("s1", "body", 0)];
    let answer = box_select(&mut doc, &meshes, None, &v, rect, false).unwrap();
    assert_eq!(doc.selection, vec![item("b1", "body", 0)]);
    assert_eq!(answer["found"], json!(1));
    // Extend keeps what was selected.
    doc.selection = vec![item("s1", "body", 0)];
    box_select(&mut doc, &meshes, None, &v, rect, true).unwrap();
    assert_eq!(doc.selection, vec![item("s1", "body", 0), item("b1", "body", 0)]);

    // Edge mode: an edge whose samples all lie inside; vertex mode: vertices inside.
    let mut topology = CadTopology::default();
    topology.insert(
        "b1",
        NodeTopology {
            revision: 3,
            edges: vec![
                EdgeInfo { index: 0, points: vec![[-100.0, 0.0, 0.0], [100.0, 0.0, 0.0]], ..Default::default() },
                EdgeInfo { index: 1, points: vec![[0.0, 0.0, 0.0], [2000.0, 0.0, 0.0]], ..Default::default() },
            ],
            vertices: vec![VertexInfo { index: 0, point: Some([0.0, 0.0, 100.0]) }, VertexInfo { index: 1, point: Some([2000.0, 0.0, 0.0]) }],
            ..Default::default()
        },
    );
    doc.select_mode = SelectMode::Edge;
    let answer = box_select(&mut doc, &meshes, Some(&topology), &v, rect, false).unwrap();
    assert_eq!(doc.selection, vec![item("b1", "edge", 0)]);
    // b2's topology is not loaded: named, not tested.
    assert_eq!(answer["not_loaded"], json!(["Nb2"]));
    doc.select_mode = SelectMode::Vertex;
    box_select(&mut doc, &meshes, Some(&topology), &v, rect, false).unwrap();
    assert_eq!(doc.selection, vec![item("b1", "vertex", 0)]);
}

#[test]
fn a_face_selection_is_pushed_with_its_items_and_mode() {
    let mut doc = document();
    doc.select_mode = SelectMode::Face;
    doc.selection = vec![item("b1", "face", 2)];
    let (items, mode) = super::super::sync::selection_body(&doc);
    assert_eq!(items, vec![item("b1", "face", 2)]);
    assert_eq!(mode, Some("face"));
    // As RoboCAD's PUT /selection body.
    assert_eq!(serde_json::to_value(&items).unwrap(), json!([["b1", "face", 2]]));
}

#[test]
fn robocads_moded_selection_is_adopted_but_not_during_a_push() {
    let mut doc = document();
    doc.client = Some(sim_runtime::cad_client::CadClient::new("http://127.0.0.1:8420").unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, gui: true, revision: 3, ..Default::default() });
    let sent = Instant::now();
    let remote = Selection { items: vec![item("b2", "edge", 5)], mode: Some("edge".into()) };
    assert!(super::super::sync::adopt_selection(&mut doc, sent, remote.clone()));
    assert_eq!(doc.select_mode, SelectMode::Edge);
    assert_eq!(doc.selection, vec![item("b2", "edge", 5)]);
    // The same answer again changes nothing.
    assert!(!super::super::sync::adopt_selection(&mut doc, sent, remote));
    // A headless answer (no mode) keeps the viewer's mode.
    let headless = Selection { items: vec![item("b1", "face", 1)], mode: None };
    assert!(super::super::sync::adopt_selection(&mut doc, sent, headless));
    assert_eq!(doc.select_mode, SelectMode::Edge);
    assert_eq!(doc.selection, vec![item("b1", "face", 1)]);
    // An item naming a node the (current) shown tree lacks is left out;
    // RoboCAD's copy keeps it as read. While the tree is behind, it is kept.
    let ghost = Selection { items: vec![item("b1", "face", 1), item("gone", "body", 0)], mode: None };
    assert!(!super::super::sync::adopt_selection(&mut doc, sent, ghost.clone()));
    assert_eq!(doc.selection, vec![item("b1", "face", 1)]);
    assert_eq!(doc.remote_selection, ghost.items);
    doc.remote_selection.clear();
    doc.stale = Some("refetching revision 4".into());
    assert!(super::super::sync::adopt_selection(&mut doc, sent, ghost.clone()));
    assert_eq!(doc.selection, ghost.items);
    doc.stale = None;
    doc.selection = vec![item("b1", "face", 1)];
    // While our push is in flight, or for a read sent before it answered, nothing is adopted.
    doc.selection_job = Some(Job::finished(doc.generation, Ok(Vec::new())));
    let other = Selection { items: vec![item("b1", "body", 0)], mode: Some("body".into()) };
    assert!(!super::super::sync::adopt_selection(&mut doc, Instant::now(), other.clone()));
    assert_eq!((doc.select_mode, doc.selection.clone()), (SelectMode::Edge, vec![item("b1", "face", 1)]));
    doc.selection_job = None;
    doc.selection_pushed_at = Some(Instant::now());
    assert!(!super::super::sync::adopt_selection(&mut doc, sent.checked_sub(Duration::from_millis(1)).unwrap_or(sent), other));
    assert_eq!(doc.select_mode, SelectMode::Edge);
}
