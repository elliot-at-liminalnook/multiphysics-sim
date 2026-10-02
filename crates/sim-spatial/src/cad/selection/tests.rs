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

/// The shared selection with CAD's entry at the shown tree's revision (3).
fn shared() -> Fixture {
    Fixture::at(3)
}

#[test]
fn select_replaces_extends_toggles_and_ids_are_body_items() {
    let mut doc = document();
    let mut f = shared();
    doc.candidates = Some(Candidates { items: vec![item("b1", "face", 0)], extend: false, toggle: false, revision: Some(3) });
    let r0 = doc.revision;
    let answer = select(&mut doc, &mut f.shared(), &["b1".into()], &[], false, false, None).unwrap();
    assert_eq!(f.items(), vec![item("b1", "body", 0)]);
    assert_eq!(answer["selection"], json!([["b1", "body", 0]]), "cad_select answers the shared selection's items");
    assert!(doc.candidates.is_none(), "a choice closes the Alt menu");
    assert!(doc.revision > r0);
    assert_eq!(answer["connected"], json!(false));
    assert_eq!(answer["pushed"], json!(false));
    // Shift appends (once), Ctrl toggles.
    select(&mut doc, &mut f.shared(), &[], &[item("b2", "face", 4), item("b1", "body", 0)], true, false, None).unwrap();
    assert_eq!(f.items(), vec![item("b1", "body", 0), item("b2", "face", 4)]);
    select(&mut doc, &mut f.shared(), &[], &[item("b1", "body", 0), item("b2", "edge", 1)], false, true, None).unwrap();
    assert_eq!(f.items(), vec![item("b2", "face", 4), item("b2", "edge", 1)]);
    assert_eq!(doc.status, Some(Ok("2 selected".to_string())));
    // Replace; empty clears.
    select(&mut doc, &mut f.shared(), &[], &[item("b2", "vertex", 3)], false, false, None).unwrap();
    assert_eq!(f.items(), vec![item("b2", "vertex", 3)]);
    select(&mut doc, &mut f.shared(), &[], &[], false, false, None).unwrap();
    assert!(f.items().is_empty() && doc.status.is_none());
    // Refusals name the item.
    let e = select(&mut doc, &mut f.shared(), &[], &[item("b1", "solid", 0)], false, false, None).unwrap_err();
    assert!(e.contains("solid") && e.contains("b1"), "{e}");
    let e = select(&mut doc, &mut f.shared(), &["nope".into()], &[], false, false, None).unwrap_err();
    assert!(e.contains("nope"), "{e}");
    let e = select(&mut doc, &mut f.shared(), &[], &[item("b1", "face", -1)], false, false, None).unwrap_err();
    assert!(e.contains("negative"), "{e}");
}

#[test]
fn a_mode_switch_clears_the_selection_hover_and_menu() {
    let mut doc = document();
    let mut f = shared();
    f.set(vec![item("b1", "body", 0)]);
    doc.hover = Some(item("b2", "body", 0));
    doc.candidates = Some(Candidates::default());
    set_mode(&mut doc, &mut f.shared(), SelectMode::Edge);
    assert_eq!(doc.select_mode, SelectMode::Edge);
    assert!(f.items().is_empty() && doc.hover.is_none() && doc.candidates.is_none());
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
    let mut f = shared();
    select_all(&mut doc, &mut f.shared()).unwrap();
    let ids: Vec<String> = f.items().iter().map(|i| i.0.clone()).collect();
    assert_eq!(ids, ["b1", "b2", "s1", "m1", "i1"]);
    assert!(f.items().iter().all(|i| i.1 == "body" && i.2 == 0));
    f.set(vec![item("b1", "face", 2), item("s1", "body", 0)]);
    invert(&mut doc, &mut f.shared()).unwrap();
    let ids: Vec<String> = f.items().iter().map(|i| i.0.clone()).collect();
    assert_eq!(ids, ["b2", "m1", "i1"]);
}

#[test]
fn same_material_takes_the_first_selected_nodes_material() {
    let mut doc = document();
    let mut f = shared();
    assert!(same_material(&mut doc, &mut f.shared()).unwrap_err().contains("nothing is selected"));
    f.set(vec![item("b2", "face", 1)]);
    let answer = same_material(&mut doc, &mut f.shared()).unwrap();
    assert_eq!(answer["material"], json!("pla"));
    // Hidden b3 too (RoboCAD's same_material does not filter visibility).
    let ids: Vec<String> = f.items().iter().map(|i| i.0.clone()).collect();
    assert_eq!(ids, ["b1", "b2", "b3"]);
    f.set(vec![item("s1", "body", 0)]);
    let e = same_material(&mut doc, &mut f.shared()).unwrap_err();
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
    let mut f = shared();
    doc.select_mode = SelectMode::Edge;
    f.set(vec![item("b1", "edge", 7)]);
    let mut meshes = CadMeshes::default();
    let mut topology = CadTopology::default();
    let e = edges_to_faces(&mut doc, &mut f.shared(), Some(&meshes), Some(&topology)).unwrap_err();
    assert!(e.contains("Nb1") && e.contains("loading"), "{e}");
    topology.insert("b1", NodeTopology { revision: 3, edges: vec![EdgeInfo { index: 7, points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]], ..Default::default() }], ..Default::default() });
    let e = edges_to_faces(&mut doc, &mut f.shared(), Some(&meshes), Some(&topology)).unwrap_err();
    assert!(e.contains("tessellation of Nb1"), "{e}");
    meshes.insert_drawn("b1", 3, mesh);
    assert_eq!(faces_of_edge(&meshes, &topology, "b1", 7), vec![0, 1]);
    edges_to_faces(&mut doc, &mut f.shared(), Some(&meshes), Some(&topology)).unwrap();
    assert_eq!(doc.select_mode, SelectMode::Face);
    assert_eq!(f.items(), vec![item("b1", "face", 0), item("b1", "face", 1)]);
    assert_eq!(f.selection.cad_stamped(f.id()), vec![(item("b1", "face", 0), 3), (item("b1", "face", 1), 3)], "converted at the topology's revision");
    assert!(edges_to_faces(&mut doc, &mut f.shared(), Some(&meshes), Some(&topology)).unwrap_err().contains("no edges"));
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
    let mut f = shared();
    let mut meshes = CadMeshes::default();
    // b1 at the origin (inside), b2 2 m to the right (about 48 px: outside).
    meshes.insert_drawn("b1", 3, cube([0.0, 0.0, 0.0], 100.0));
    meshes.insert_drawn("b2", 3, cube([2000.0, 0.0, 0.0], 100.0));
    let v = view();
    let rect = [120.0, 80.0, 100.0, 60.0];
    f.set(vec![item("s1", "body", 0)]);
    let answer = box_select(&mut doc, &mut f.shared(), &meshes, None, &v, rect, false).unwrap();
    assert_eq!(f.items(), vec![item("b1", "body", 0)]);
    assert_eq!(answer["found"], json!(1));
    // Extend keeps what was selected.
    f.set(vec![item("s1", "body", 0)]);
    box_select(&mut doc, &mut f.shared(), &meshes, None, &v, rect, true).unwrap();
    assert_eq!(f.items(), vec![item("s1", "body", 0), item("b1", "body", 0)]);

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
    let answer = box_select(&mut doc, &mut f.shared(), &meshes, Some(&topology), &v, rect, false).unwrap();
    assert_eq!(f.items(), vec![item("b1", "edge", 0)]);
    // b2's topology is not loaded: named, not tested.
    assert_eq!(answer["not_loaded"], json!(["Nb2"]));
    doc.select_mode = SelectMode::Vertex;
    box_select(&mut doc, &mut f.shared(), &meshes, Some(&topology), &v, rect, false).unwrap();
    assert_eq!(f.items(), vec![item("b1", "vertex", 0)]);
}

#[test]
fn a_face_selection_is_pushed_with_its_items_and_mode() {
    let mut doc = document();
    let mut f = shared();
    doc.select_mode = SelectMode::Face;
    f.set(vec![item("b1", "face", 2)]);
    let (items, mode) = super::super::sync::selection_body(&doc, f.items());
    assert_eq!(items, vec![item("b1", "face", 2)]);
    assert_eq!(mode, Some("face"));
    // As RoboCAD's PUT /selection body.
    assert_eq!(serde_json::to_value(&items).unwrap(), json!([["b1", "face", 2]]));
}

#[test]
fn robocads_moded_selection_is_adopted_but_not_during_a_push() {
    let mut doc = document();
    let mut f = shared();
    doc.client = Some(sim_runtime::cad_client::CadClient::new("http://127.0.0.1:8420").unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, gui: true, revision: 3, ..Default::default() });
    let adopt = |doc: &mut CadDocument, f: &mut Fixture, sent: Instant, s: Selection| super::super::sync::adopt_selection(doc, &mut f.shared(), sent, s);
    let sent = Instant::now();
    let remote = Selection { items: vec![item("b2", "edge", 5)], mode: Some("edge".into()) };
    assert!(adopt(&mut doc, &mut f, sent, remote.clone()));
    assert_eq!(doc.select_mode, SelectMode::Edge);
    assert_eq!(f.items(), vec![item("b2", "edge", 5)]);
    // The same answer again changes nothing.
    assert!(!adopt(&mut doc, &mut f, sent, remote));
    // A headless answer (no mode) keeps the viewer's mode.
    let headless = Selection { items: vec![item("b1", "face", 1)], mode: None };
    assert!(adopt(&mut doc, &mut f, sent, headless));
    assert_eq!(doc.select_mode, SelectMode::Edge);
    assert_eq!(f.items(), vec![item("b1", "face", 1)]);
    // An item naming a node the (current) shown tree lacks is left out;
    // RoboCAD's copy keeps it as read. While the tree is behind, it is kept.
    let ghost = Selection { items: vec![item("b1", "face", 1), item("gone", "body", 0)], mode: None };
    assert!(!adopt(&mut doc, &mut f, sent, ghost.clone()));
    assert_eq!(f.items(), vec![item("b1", "face", 1)]);
    assert_eq!(doc.remote_selection, ghost.items);
    doc.remote_selection.clear();
    doc.stale = Some("refetching revision 4".into());
    assert!(adopt(&mut doc, &mut f, sent, ghost.clone()));
    assert_eq!(f.items(), ghost.items);
    doc.stale = None;
    f.set(vec![item("b1", "face", 1)]);
    // While our push is in flight, or for a read sent before it answered, nothing is adopted.
    doc.selection_job = Some(Job::finished(doc.generation, Ok(Vec::new())));
    let other = Selection { items: vec![item("b1", "body", 0)], mode: Some("body".into()) };
    assert!(!adopt(&mut doc, &mut f, Instant::now(), other.clone()));
    assert_eq!((doc.select_mode, f.items()), (SelectMode::Edge, vec![item("b1", "face", 1)]));
    doc.selection_job = None;
    doc.selection_pushed_at = Some(Instant::now());
    assert!(!adopt(&mut doc, &mut f, sent.checked_sub(Duration::from_millis(1)).unwrap_or(sent), other));
    assert_eq!(doc.select_mode, SelectMode::Edge);
}

/// (a) RoboCAD's `/selection` echo does not loop: an adopted selection is
/// not pushed back; a local change (any writer, here the shared apply as
/// an `Act<SelectionAction>` makes it) is pushed exactly once; a change
/// while a push is in flight is pushed once more when that push answers.
/// The client points at a closed port: a push spawned here never reaches
/// a RoboCAD on this machine, and each one is answered by hand.
#[test]
fn the_selection_echo_does_not_loop() {
    let mut doc = document();
    let mut f = shared();
    doc.client = Some(sim_runtime::cad_client::CadClient::new("http://127.0.0.1:9").unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, gui: false, revision: 3, ..Default::default() });
    // Adopted from RoboCAD: no push, and nothing differs from RoboCAD's copy.
    let remote = Selection { items: vec![item("b1", "body", 0)], mode: None };
    assert!(super::super::sync::adopt_selection(&mut doc, &mut f.shared(), Instant::now(), remote));
    publish_changes(&mut doc, f.shared().view());
    assert!(doc.selection_job.is_none(), "an adopted selection is not pushed back");
    assert!(!differs_from_remote(&doc, &f.items()));
    // A local change: exactly one push, and seeing it again pushes nothing more.
    f.shared().apply(Op::Set, [(item("b2", "body", 0), None)]).unwrap();
    publish_changes(&mut doc, f.shared().view());
    assert!(doc.selection_job.is_some() && !doc.selection_again, "one push started");
    publish_changes(&mut doc, f.shared().view());
    assert!(!doc.selection_again, "the same change is not pushed twice");
    // A change while that push is in flight waits for it.
    f.shared().apply(Op::Add, [(item("s1", "body", 0), None)]).unwrap();
    publish_changes(&mut doc, f.shared().view());
    assert!(doc.selection_again, "queued behind the push in flight");
    // RoboCAD answers the first push: the newest is pushed, once.
    doc.selection_job = Some(Job::finished(doc.generation, Ok(vec![item("b2", "body", 0)])));
    super::super::sync::finish_selection(&mut doc, f.shared().view());
    assert!(doc.selection_job.is_some() && !doc.selection_again, "one more push for the queued change");
    assert_eq!(doc.remote_selection, vec![item("b2", "body", 0)]);
    // It answers too: nothing more is pushed, and RoboCAD's copy is the selection.
    doc.selection_job = Some(Job::finished(doc.generation, Ok(f.items())));
    super::super::sync::finish_selection(&mut doc, f.shared().view());
    assert!(doc.selection_job.is_none() && !doc.selection_again);
    assert!(!differs_from_remote(&doc, &f.items()));
}

/// (b) Revisions: a face picked against an older tree is refused by name;
/// when a new tree is shown, a body item is restamped, a face keeps the
/// revision it was picked at (CAD's own stale refusals use it), and an
/// item naming a node absent from the current tree is dropped and named
/// (kept while the tree is behind RoboCAD's revision).
#[test]
fn picks_carry_their_revision_and_a_new_tree_rechecks_them() {
    let mut doc = document();
    let mut f = shared();
    let e = select(&mut doc, &mut f.shared(), &[], &[item("b2", "face", 4)], false, false, Some(2)).unwrap_err();
    assert!(e.contains("[b2, face, 4]") && e.contains("revision 2") && e.contains("revision 3"), "{e}");
    assert!(f.items().is_empty(), "nothing applied");
    // Picked at the shown revision: taken; a body item names no index and carries no pick revision.
    select(&mut doc, &mut f.shared(), &["b1".into(), "s1".into()], &[item("b2", "face", 4)], false, false, Some(3)).unwrap();
    assert_eq!(f.items(), vec![item("b1", "body", 0), item("s1", "body", 0), item("b2", "face", 4)]);
    // Revision 4 while RoboCAD is already further on: s1 is absent but kept.
    let mut tree = doc.doc.clone().unwrap();
    tree.nodes.retain(|n| n.id != "s1");
    tree.revision = 4;
    assert!(follow_tree(&mut f.shared(), 4, &tree, false).is_empty());
    assert_eq!(f.registry.revision(f.id()), Some(4));
    // Revision 5, current: s1 is dropped and named; b1 restamped; the face keeps revision 3.
    tree.revision = 5;
    let dropped = follow_tree(&mut f.shared(), 5, &tree, true);
    assert_eq!(dropped, ["[s1, body, 0]"]);
    assert_eq!(f.selection.dropped, dropped);
    assert_eq!(f.selection.cad_stamped(f.id()), vec![(item("b1", "body", 0), 5), (item("b2", "face", 4), 3)]);
}

/// (c) A REST `cad_select` (parsed and run through CAD's one handler, the
/// adapter) and the same items as an `Act<SelectionAction>` (the shared
/// apply system) leave the same selection.
#[test]
fn cad_select_and_a_selection_action_give_the_same_selection() {
    use crate::app::actions::{Act, Action, Call, Origin, Replies};
    use crate::app::{ViewerMode, ViewerSet};
    use crate::document::{DocumentKind, DocumentRegistry, Source};
    use crate::selection::{Item, Selection as OneSelection, SelectionAction};
    // Through the REST adapter.
    let mut doc = document();
    let mut f = Fixture::new();
    let rest = json!({"ids": ["b1"], "items": [["b2", "face", 4]], "extend": false});
    let action = <CadAction as Action>::parse(&sim_api::Command { command: "cad_select".into(), args: rest }).unwrap();
    let mut plane = crate::cad::sketch::CadActivePlane::default();
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    let mut cx = Cx { doc: &mut doc, shared: f.shared(), meshes: None, topology: None, view: None, plane: &mut plane, sketches: None, display: None, views: None, files: None, components: &mut crate::cad::components::ComponentsState::default(), composition: &mut crate::cad::composition::CadCompositionState::default(), camera: Vec::new() };
    assert!(matches!(crate::cad::actions::handle(&action, &mut call, &mut cx), Outcome::Done(Ok(_))));
    // Through the shared action and its one apply system.
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin)).insert_state(ViewerMode::Cad).configure_sets(Update, (ViewerSet::Input, ViewerSet::Actions).chain());
    crate::selection::build(&mut app);
    let id = app.world_mut().resource_mut::<DocumentRegistry>().open(ViewerMode::Cad, DocumentKind::Cad, Source::Url { url: "http://127.0.0.1:8420".into() }).id;
    app.world_mut().write_message(Act::ui(SelectionAction::set(id, [Item::Cad(item("b1", "body", 0)), Item::Cad(item("b2", "face", 4))])));
    app.update();
    let world = app.world().resource::<OneSelection>();
    assert_eq!(world.cad_stamped(id), f.selection.cad_stamped(f.id()));
    assert_eq!(world.cad(id), vec![item("b1", "body", 0), item("b2", "face", 4)]);
}

/// The registry's CAD entry and the shown tree move to `revision` (a new
/// tree arrived; items keep the revision they were picked at).
fn advance(doc: &mut CadDocument, f: &mut Fixture, revision: u64) {
    let id = f.id();
    f.registry.set_revision(id, revision);
    doc.doc_key = Some((None, revision));
}

/// An Alt-menu choice carries the revision the menu was gathered at: after
/// the tree moved on it is refused by name and the stale menu closes; from
/// a menu gathered at the current revision it applies.
#[test]
fn a_candidate_chosen_after_the_tree_moved_is_refused_by_name() {
    let mut doc = document();
    let mut f = shared();
    candidates(&mut doc, &[item("b1", "face", 2), item("b2", "face", 0)], false, false).unwrap();
    assert_eq!(doc.candidates.as_ref().and_then(|c| c.revision), Some(3));
    advance(&mut doc, &mut f, 4);
    let e = select(&mut doc, &mut f.shared(), &[], &[item("b1", "face", 2)], false, false, None).unwrap_err();
    assert!(e.contains("[b1, face, 2]") && e.contains("revision 3") && e.contains("revision 4"), "{e}");
    assert!(f.items().is_empty(), "nothing applied");
    assert!(doc.candidates.is_none(), "the stale menu closes");
    // Gathered at the shown revision: the choice applies (and closes the menu).
    candidates(&mut doc, &[item("b1", "face", 2), item("b2", "face", 0)], false, false).unwrap();
    select(&mut doc, &mut f.shared(), &[], &[item("b1", "face", 2)], false, false, None).unwrap();
    assert_eq!(f.selection.cad_stamped(f.id()), vec![(item("b1", "face", 2), 4)]);
    assert!(doc.candidates.is_none());
    // A body candidate names no index: it applies at the current revision.
    candidates(&mut doc, &[item("b1", "body", 0)], false, false).unwrap();
    advance(&mut doc, &mut f, 5);
    select(&mut doc, &mut f.shared(), &[], &[item("b1", "body", 0)], false, false, None).unwrap();
    assert_eq!(f.items(), vec![item("b1", "body", 0)]);
    // A select that is not the menu's choice (another extend) is not stamped by it.
    candidates(&mut doc, &[item("b2", "face", 0)], false, false).unwrap();
    advance(&mut doc, &mut f, 6);
    select(&mut doc, &mut f.shared(), &[], &[item("b2", "face", 0)], true, false, None).unwrap();
    assert_eq!(f.items(), vec![item("b1", "body", 0), item("b2", "face", 0)]);
}

/// A box select's edges carry their topology's revision: a topology the
/// shown tree has moved past is refused by name; edges → faces refuses an
/// edge picked at another revision than the topology it would convert with.
#[test]
fn a_box_or_edges_to_faces_over_a_moved_tree_is_refused_by_name() {
    let mut doc = document();
    let mut f = shared();
    let mut meshes = CadMeshes::default();
    meshes.insert_drawn("b1", 3, cube([0.0, 0.0, 0.0], 100.0));
    let mut topology = CadTopology::default();
    topology.insert("b1", NodeTopology { revision: 3, edges: vec![EdgeInfo { index: 0, points: vec![[-100.0, 0.0, 0.0], [100.0, 0.0, 0.0]], ..Default::default() }], ..Default::default() });
    doc.select_mode = SelectMode::Edge;
    let rect = [120.0, 80.0, 100.0, 60.0];
    advance(&mut doc, &mut f, 4);
    let e = box_select(&mut doc, &mut f.shared(), &meshes, Some(&topology), &view(), rect, false).unwrap_err();
    assert!(e.contains("[b1, edge, 0]") && e.contains("revision 3") && e.contains("revision 4"), "{e}");
    assert!(f.items().is_empty());
    // The topology at the shown revision: applied, stamped with it.
    topology.insert("b1", NodeTopology { revision: 4, edges: vec![EdgeInfo { index: 0, points: vec![[-100.0, 0.0, 0.0], [100.0, 0.0, 0.0]], ..Default::default() }], ..Default::default() });
    box_select(&mut doc, &mut f.shared(), &meshes, Some(&topology), &view(), rect, false).unwrap();
    assert_eq!(f.selection.cad_stamped(f.id()), vec![(item("b1", "edge", 0), 4)]);

    // Edges → faces: edge 7 picked at revision 4, the tree and topology now at 5.
    f.set(vec![item("b1", "edge", 7)]);
    advance(&mut doc, &mut f, 5);
    let mut meshes = CadMeshes::default();
    meshes.insert_drawn("b1", 5, two_faces());
    let mut topology = CadTopology::default();
    topology.insert("b1", NodeTopology { revision: 5, edges: vec![EdgeInfo { index: 7, points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]], ..Default::default() }], ..Default::default() });
    let e = edges_to_faces(&mut doc, &mut f.shared(), Some(&meshes), Some(&topology)).unwrap_err();
    assert!(e.contains("edge 7 of Nb1") && e.contains("revision 4") && e.contains("revision 5"), "{e}");
    assert_eq!(f.items(), vec![item("b1", "edge", 7)], "nothing converted");
    // Picked again at the topology's revision: converted.
    f.set(vec![item("b1", "edge", 7)]);
    edges_to_faces(&mut doc, &mut f.shared(), Some(&meshes), Some(&topology)).unwrap();
    assert_eq!(f.selection.cad_stamped(f.id()), vec![(item("b1", "face", 0), 5), (item("b1", "face", 1), 5)]);
}
