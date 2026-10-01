//! The catalogue's tests (cad-modify): the data is well formed, every
//! entry builds calls to its route, the argument shapes are what RoboCAD's
//! `ArgConverter` reads (cad/robocad/api.py:146-244), and the selection
//! and run refusals hold. Windowless: std and serde_json only.
use super::args::{Built, build, edge_ref};
use super::resolve::{Resolved, resolve};
use super::*;
use crate::cad::analysis_overlay::Read;
use crate::cad::document::{CadTarget, Connection, Edit};
use crate::cad::transform::face_ref;
use sim_runtime::cad_client::{CadClient, DocState, Health, NodeSummary};

fn node(id: &str, kind: &str, name: &str) -> NodeSummary {
    NodeSummary { id: id.into(), kind: kind.into(), name: name.into(), visible: true, effective_visible: true, ..Default::default() }
}

fn item(node: &str, kind: &str, index: i64) -> SelectionItem {
    SelectionItem(node.into(), kind.into(), index)
}

/// A connected document at revision 4 (shown and RoboCAD's) with two
/// bodies, a sheet, an instance and a curve; nothing selected.
fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    doc.client = Some(CadClient::new("http://127.0.0.1:8420").unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), revision: 4, ..Default::default() });
    doc.doc = Some(DocState {
        nodes: vec![node("b1", "body", "Bracket"), node("b2", "body", "Plate"), node("s1", "sheet", "Skin"), node("i1", "instance", "Bracket instance"), node("c1", "curve", "Path")],
        revision: 4,
        ..Default::default()
    });
    doc.doc_key = Some((None, 4));
    doc
}

fn op(id: &str) -> &'static OpEntry {
    entry(id).unwrap_or_else(|| panic!("{id} is not in the catalogue"))
}

/// Parameters as REST sends them.
fn given(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
}

/// The calls of an edit, or a panic naming what was built.
fn calls(built: Built) -> Vec<crate::cad::transform::OpCall> {
    match built {
        Built::Edit { calls, .. } => calls,
        other => panic!("expected an edit, got {other:?}"),
    }
}

/// A selection that satisfies every need: bodies b1, b2 then the curve c1,
/// two edges and a face of b1, b2 as the dependent offset's target, a view
/// direction and a cursor snap.
fn everything() -> Resolved {
    Resolved {
        nodes: vec!["b1".into(), "b2".into(), "c1".into()],
        edges: vec![("b1".into(), 0), ("b1".into(), 1)],
        faces: vec![("b1".into(), 2)],
        other: Some("b2".into()),
        view_dir: Some([0.0, 0.0, -1.0]),
        snap: Some([1.0, 2.0, 3.0]),
        revision: 4,
    }
}

#[test]
fn catalogue_data_is_well_formed() {
    let mut seen: Vec<&str> = Vec::new();
    for e in CATALOGUE {
        assert!(!seen.contains(&e.id), "{} is listed twice", e.id);
        seen.push(e.id);
        assert!(!e.label.is_empty() && !e.category.is_empty() && !e.route.is_empty() && !e.source.is_empty(), "{}: label, category, route and source are required", e.id);
        assert!(e.needs == Needs::Nothing || !e.refusal.is_empty(), "{}: a selection need has a refusal", e.id);
        if matches!(e.flow, Flow::PickThenForm(_) | Flow::Place(_)) {
            assert!(!e.hint.is_empty(), "{}: an interactive operation has RoboCAD's hint", e.id);
        }
        for p in e.params {
            assert!(e.params.iter().filter(|q| q.name == p.name).count() == 1, "{}: parameter {} is listed twice", e.id, p.name);
            if !p.default.is_empty() {
                param_value(p, &Value::String(p.default.into())).unwrap_or_else(|err| panic!("{}: {}'s default {:?} does not evaluate: {err}", e.id, p.name, p.default));
            }
            if let Some((on, is)) = p.when {
                let q = e.params.iter().find(|q| q.name == on).unwrap_or_else(|| panic!("{}: {} depends on unknown parameter {on}", e.id, p.name));
                match q.kind {
                    FieldKind::Choice { options } => assert!(options.contains(&is), "{}: {} depends on {on} = {is}, not one of its options", e.id, p.name),
                    other => panic!("{}: {} depends on {on}, which is not a choice ({other:?})", e.id, p.name),
                }
            }
        }
        for a in e.args.iter().chain(e.kwargs.iter().map(|(_, a)| a)) {
            match a {
                Arg::Param(name) => assert!(e.params.iter().any(|p| p.name == *name), "{}: argument names unknown parameter {name}", e.id),
                Arg::Const(text) => {
                    serde_json::from_str::<Value>(text).unwrap_or_else(|err| panic!("{}: constant {text} is not JSON: {err}", e.id));
                }
                _ => {}
            }
        }
        // Defaults alone give values (empty defaults are left out).
        values(e, &Map::new()).unwrap_or_else(|err| panic!("{}: defaults do not evaluate: {err}", e.id));
    }
}

#[test]
fn every_entry_builds_calls_to_its_route() {
    let mut doc = document();
    doc.ops.clipboard = Some((4, json!({"robocad_clipboard": true, "items": [{"id": "b1"}]})));
    let r = everything();
    for e in CATALOGUE {
        // The cad-sketch shapes read the sketch cache, the active plane and
        // clicked points: their builders are tested in `sketch::tests`.
        if matches!(e.shape, Shape::Sketch(_) | Shape::SketchEdit(_) | Shape::Extrude { .. } | Shape::View(_)) {
            continue;
        }
        // A sample for each parameter without a default (REST-only Ops methods).
        let sample: Map<String, Value> = e
            .params
            .iter()
            .filter(|p| p.default.is_empty())
            .map(|p| {
                let text = match p.kind {
                    FieldKind::Vector { .. } => "1, 2, 3".to_string(),
                    FieldKind::Number { .. } => "5".to_string(),
                    FieldKind::Json => "{\"part\": [0]}".to_string(),
                    FieldKind::Choice { options } => options[0].to_string(),
                    FieldKind::Check => "false".to_string(),
                    FieldKind::Text => "text".to_string(),
                };
                (p.name.to_string(), Value::String(text))
            })
            .collect();
        let values = values(e, &sample).unwrap_or_else(|err| panic!("{}: {err}", e.id));
        let built = build(e, &r, &values, &doc, &Env::default()).unwrap_or_else(|err| panic!("{} does not build: {err}", e.id));
        match e.shape {
            Shape::Copy | Shape::ControlPoints | Shape::CurvatureComb | Shape::Continuity => assert!(matches!(built, Built::Read(_)), "{}: {built:?}", e.id),
            Shape::Paste => assert!(matches!(built, Built::Paste { .. }), "{}: {built:?}", e.id),
            _ => {
                let calls = calls(built);
                assert!(!calls.is_empty(), "{}: no calls", e.id);
                for c in &calls {
                    let want = if e.shape == Shape::Array { "array_rect" } else { e.route };
                    assert_eq!(c.name, want, "{}", e.id);
                    assert!(!c.label.is_empty(), "{}: a call has a label", e.id);
                }
            }
        }
    }
    // The Array dialog's radial kind calls array_radial.
    let array = op("tool.array");
    let values = values(array, &given(&[("kind", json!("radial"))])).unwrap();
    assert_eq!(calls(build(array, &r, &values, &doc, &Env::default()).unwrap())[0].name, "array_radial");
}

#[test]
fn fillet_and_chamfer_send_one_call_per_node_with_edge_refs() {
    let doc = document();
    let r = Resolved { nodes: vec!["b1".into(), "b2".into()], edges: vec![("b1".into(), 0), ("b2".into(), 3), ("b1".into(), 1)], ..Default::default() };
    let fillet = op("tool.fillet");
    let calls_ = calls(build(fillet, &r, &values(fillet, &Map::new()).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(calls_.len(), 2, "one call per node, in first-appearance order");
    assert_eq!(calls_[0].name, "fillet");
    assert_eq!(calls_[0].args, vec![json!("b1"), json!([edge_ref("b1", 0), edge_ref("b1", 1)]), json!(1.0)]);
    assert_eq!(calls_[1].args, vec![json!("b2"), json!([{"node": "b2", "edge": 3}]), json!(1.0)]);
    assert!(calls_[0].kwargs.is_empty());

    let variable = op("tool.fillet_variable");
    let c = calls(build(variable, &r, &values(variable, &Map::new()).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].args[2..], [json!(1.0), json!(2.0)]);

    let chamfer = op("tool.chamfer");
    let c = calls(build(chamfer, &r, &values(chamfer, &Map::new()).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].args, vec![json!("b1"), json!([edge_ref("b1", 0), edge_ref("b1", 1)]), json!({"distance": 1.0})], "45° is not sent");
    let c = calls(build(chamfer, &r, &values(chamfer, &given(&[("distance", json!("2 mm")), ("angle", json!(30))])).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].args[2], json!({"distance": 2.0, "angle_deg": 30.0}));
}

#[test]
fn shell_draft_and_faces_use_face_refs() {
    let doc = document();
    let shell = op("tool.shell");
    let r = Resolved { nodes: vec!["b1".into(), "b2".into()], faces: vec![("b1".into(), 4)], ..Default::default() };
    let c = calls(build(shell, &r, &values(shell, &Map::new()).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c.len(), 2, "every selected node is shelled, with its own open faces");
    assert_eq!(c[0].args, vec![json!("b1"), json!(2.0), json!([face_ref("b1", 4)])]);
    assert_eq!(c[1].args, vec![json!("b2"), json!(2.0), json!([])]);

    let draft = op("tool.draft");
    let c = calls(build(draft, &r, &values(draft, &Map::new()).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c.len(), 1, "only nodes with selected faces");
    assert_eq!(c[0].args, vec![json!("b1"), json!([face_ref("b1", 4)]), json!([0, 0, 1]), json!(2.0), json!("xy")]);
}

#[test]
fn booleans_mirror_and_instance_match_robocad_handlers() {
    let doc = document();
    let r = Resolved { nodes: vec!["b1".into(), "b2".into(), "s1".into()], ..Default::default() };
    for (id, op_name) in [("modify.union", "union"), ("modify.subtract", "subtract"), ("modify.intersect", "intersect")] {
        let e = op(id);
        let c = calls(build(e, &r, &Map::new(), &doc, &Env::default()).unwrap());
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name, "boolean");
        assert_eq!(c[0].args, vec![json!("b1"), json!(["b2", "s1"]), json!(op_name)]);
    }
    let mirror = op("tool.mirror_live");
    let c = calls(build(mirror, &r, &values(mirror, &Map::new()).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].args, vec![json!(["b1", "b2", "s1"]), json!("yz")]);
    assert_eq!(c[0].kwargs.get("live"), Some(&json!(true)));
    let instance = op("tool.instance");
    let c = calls(build(instance, &r, &Map::new(), &doc, &Env::default()).unwrap());
    assert_eq!(c.len(), 3);
    assert_eq!(c[2].args, vec![json!("s1"), json!({"translation": [20.0, 0.0, 0.0]})]);
}

#[test]
fn array_dialog_builds_rect_or_radial() {
    let doc = document();
    let array = op("tool.array");
    let r = Resolved { nodes: vec!["b1".into()], ..Default::default() };
    let c = calls(build(array, &r, &values(array, &Map::new()).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].name, "array_rect");
    assert_eq!(c[0].args, vec![json!(["b1"]), json!([3, 1, 1])]);
    let mut kwargs = Map::new();
    kwargs.insert("spacing".into(), json!([10.0, 10.0, 10.0]));
    kwargs.insert("as_instances".into(), json!(false));
    kwargs.insert("merge".into(), json!(false));
    assert_eq!(c[0].kwargs, kwargs);
    let v = values(array, &given(&[("mode", json!("count + total extent")), ("merge", json!(true))])).unwrap();
    let c = calls(build(array, &r, &v, &doc, &Env::default()).unwrap());
    assert_eq!(c[0].kwargs.get("extent"), Some(&json!([10.0, 10.0, 10.0])));
    assert!(!c[0].kwargs.contains_key("spacing"));
    assert_eq!(c[0].kwargs.get("merge"), Some(&json!(true)));
    let v = values(array, &given(&[("kind", json!("radial")), ("plane", json!("xz"))])).unwrap();
    assert!(!v.contains_key("count_x"), "the rectangular rows are left out");
    let c = calls(build(array, &r, &v, &doc, &Env::default()).unwrap());
    assert_eq!(c[0].name, "array_radial");
    assert_eq!(c[0].args, vec![json!(["b1"]), json!(6), json!([0.0, 0.0, 0.0]), json!([0.0, -1.0, 0.0])]);
    assert_eq!(c[0].kwargs.get("total_angle"), Some(&json!(360.0)));
}

#[test]
fn primitives_follow_the_primitive_tool() {
    let doc = document();
    let r = Resolved::default();
    let boxed = op("tool.box");
    let c = calls(build(boxed, &r, &values(boxed, &Map::new()).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!((c[0].name, c[0].args.clone()), ("box", vec![json!([0.0, 0.0, 0.0]), json!([20.0, 20.0, 10.0])]));
    // A negative height extrudes down; a zero height is 1 mm (ui/tools.py:517-520).
    let c = calls(build(boxed, &r, &values(boxed, &given(&[("height", json!(-5))])).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].args, vec![json!([0.0, 0.0, -5.0]), json!([20.0, 20.0, 5.0])]);
    let c = calls(build(boxed, &r, &values(boxed, &given(&[("height", json!(0))])).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].args[1], json!([20.0, 20.0, 1.0]));
    let centre = op("tool.box_center");
    let c = calls(build(centre, &r, &values(centre, &given(&[("center", json!([10, 10, 0]))])).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].args, vec![json!([0.0, 0.0, 0.0]), json!([20.0, 20.0, 10.0])], "centred in the plane, base on it");
    let cylinder = op("tool.cylinder");
    let c = calls(build(cylinder, &r, &values(cylinder, &given(&[("height", json!(-10))])).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].args, vec![json!([0.0, 0.0, 0.0]), json!([0.0, 0.0, -1.0]), json!(5.0), json!(10.0)]);
    let sphere = op("tool.sphere");
    let c = calls(build(sphere, &r, &values(sphere, &Map::new()).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].args, vec![json!([0.0, 0.0, 0.0]), json!(5.0)]);
}

#[test]
fn resolve_refuses_with_robocad_messages() {
    let mut doc = document();
    doc.selection = vec![item("b1", "body", 0)];
    assert_eq!(resolve(op("modify.union"), &doc, &Env::default(), None), Err("Select the target body first, then the tools".to_string()));
    assert_eq!(resolve(op("modify.region"), &doc, &Env::default(), None), Err("Select exactly two bodies".to_string()));
    assert_eq!(resolve(op("tool.thicken"), &doc, &Env::default(), None), Err("Select a sheet".to_string()), "a body is not a sheet");
    let edges = [item("b1", "edge", 0), item("b2", "edge", 1)];
    assert_eq!(resolve(op("tool.full_round"), &doc, &Env::default(), Some(&edges)), Err("Select two edges of the same body".to_string()));
    let ok = [item("b1", "edge", 0), item("b1", "edge", 1)];
    assert_eq!(resolve(op("tool.full_round"), &doc, &Env::default(), Some(&ok)).unwrap().edges, vec![("b1".to_string(), 0), ("b1".to_string(), 1)]);
    assert_eq!(resolve(op("tool.fillet"), &doc, &Env::default(), None), Err("Select one or more edges first".to_string()));
    assert_eq!(resolve(op("edit.delete"), &doc, &Env::default(), Some(&[item("zz", "body", 0)])), Err("no node zz in the shown tree".to_string()));
    // Kinds filter as RoboCAD's handlers do; the order is the selection's.
    let picked = [item("i1", "body", 0), item("s1", "body", 0), item("b1", "face", 3)];
    assert_eq!(resolve(op("tool.thicken"), &doc, &Env::default(), Some(&picked)).unwrap().nodes, vec!["s1".to_string()]);
    assert_eq!(resolve(op("modify.make_unique"), &doc, &Env::default(), Some(&picked)).unwrap().nodes, vec!["i1".to_string()]);
    // Dependent offset: a face, then a body that owns none of the selected faces.
    let r = resolve(op("tool.dependent_offset"), &doc, &Env::default(), Some(&[item("b1", "face", 2), item("b2", "body", 0)])).unwrap();
    assert_eq!(r.other.as_deref(), Some("b2"));
    assert!(resolve(op("tool.dependent_offset"), &doc, &Env::default(), Some(&[item("b1", "face", 2)])).is_err());
}

#[test]
fn a_selection_seen_at_an_older_revision_is_refused() {
    let mut doc = document();
    doc.selection = vec![item("b1", "face", 2)];
    doc.tool_state.selection_seen = Some((doc.selection.clone(), 3));
    let err = resolve(op("tool.remove_fillets"), &doc, &Env::default(), None).unwrap_err();
    assert!(err.contains("revision 3") && err.contains("reselect"), "{err}");
    // A body operation does not read face indices.
    assert!(resolve(op("edit.delete"), &doc, &Env::default(), None).is_ok());
    doc.tool_state.selection_seen = Some((doc.selection.clone(), 4));
    assert!(resolve(op("tool.remove_fillets"), &doc, &Env::default(), None).is_ok());
}

#[test]
fn runs_are_refused_in_flight_stale_or_with_unknown_parameters() {
    let mut doc = document();
    let items = [item("b1", "body", 0), item("b2", "body", 0)];
    let union = op("modify.union");
    assert!(prepare(&doc, &Env::default(), union, &Map::new(), Some(&items), Some(4)).is_ok());
    doc.edit = Some(Edit { label: "Patch Bracket: visible".into(), job: crate::jobs::Job::finished(0, Ok(EditDone { message: String::new(), result: Value::Null })), started: std::time::Instant::now(), clear_selection: None, activates_plane: false });
    let err = prepare(&doc, &Env::default(), union, &Map::new(), Some(&items), None).unwrap_err();
    assert!(err.contains("in flight") && err.contains("Patch Bracket"), "{err}");
    doc.edit = None;
    let err = prepare(&doc, &Env::default(), union, &Map::new(), Some(&items), Some(3)).unwrap_err();
    assert!(err.contains("revision 3") && err.contains("nothing was sent"), "{err}");
    let fillet = op("tool.fillet");
    let edges = [item("b1", "edge", 0)];
    let err = prepare(&doc, &Env::default(), fillet, &given(&[("bogus", json!(1))]), Some(&edges), Some(4)).unwrap_err();
    assert!(err.contains("takes no parameter bogus"), "{err}");
    let err = prepare(&doc, &Env::default(), op("edit.paste"), &Map::new(), None, None).unwrap_err();
    assert_eq!(err, op("edit.paste").refusal);
    // A required parameter with no default is named.
    let r = prepare(&doc, &Env::default(), op("ops.set_radius"), &Map::new(), Some(&[item("b1", "face", 1)]), Some(4)).unwrap_err();
    assert!(r.contains("radius") && r.contains("required"), "{r}");
}

#[test]
fn explicit_face_and_edge_items_need_their_revision() {
    let doc = document();
    let faces = [item("b1", "face", 2)];
    let points = op("tool.control_points");
    let err = prepare(&doc, &Env::default(), points, &Map::new(), Some(&faces), None).unwrap_err();
    assert_eq!(err, "pass revision: the RoboCAD revision the face and edge indices in items were read at");
    assert_eq!(prepare(&doc, &Env::default(), points, &Map::new(), Some(&faces), Some(4)), Ok(Built::Read(Read::ControlPoints { node: "b1".into(), face: 2 })));
    let err = prepare(&doc, &Env::default(), points, &Map::new(), Some(&faces), Some(3)).unwrap_err();
    assert!(err.contains("revision 3") && err.contains("nothing was sent") && err.contains("the form"), "{err}");
    let edges = [item("b1", "edge", 0)];
    assert!(prepare(&doc, &Env::default(), op("tool.fillet"), &Map::new(), Some(&edges), None).unwrap_err().starts_with("pass revision"));
    // Body items name no indices; the selection carries its own revision.
    assert!(prepare(&doc, &Env::default(), op("modify.union"), &Map::new(), Some(&[item("b1", "body", 0), item("b2", "body", 0)]), None).is_ok());
}

#[test]
fn extract_components_sends_the_revision_its_caller_read_at() {
    let doc = document();
    let extract = op("ops.extract_components");
    let body = [item("b1", "body", 0)];
    let components = given(&[("components", json!("{\"part\": [0]}"))]);
    let err = prepare(&doc, &Env::default(), extract, &components, Some(&body), None).unwrap_err();
    assert!(err.contains("pass revision"), "{err}");
    let err = prepare(&doc, &Env::default(), extract, &components, Some(&body), Some(3)).unwrap_err();
    assert!(err.contains("revision 3") && err.contains("nothing was sent"), "{err}");
    let sent = calls(prepare(&doc, &Env::default(), extract, &components, Some(&body), Some(4)).unwrap());
    assert_eq!(sent[0].kwargs.get("expected_revision"), Some(&json!(4)));
}

#[test]
fn when_gates_read_the_canonical_option_and_refuse_what_does_not_apply() {
    let array = op("tool.array");
    let v = values(array, &given(&[("kind", json!("Radial"))])).unwrap();
    assert_eq!(v.get("kind"), Some(&json!("radial")));
    assert_eq!(v.get("count"), Some(&json!(6)), "the radial rows hold for \"Radial\"");
    assert!(!v.contains_key("count_x"));
    let err = values(array, &given(&[("count", json!(4))])).unwrap_err();
    assert_eq!(err, "count applies only when kind is radial");
    let err = values(array, &given(&[("kind", json!("radial")), ("count_x", json!(2))])).unwrap_err();
    assert_eq!(err, "count_x applies only when kind is rectangular");
    // The form shows the radial rows for "Radial".
    let mut doc = document();
    open_form(&mut doc, array);
    form_set(&mut doc, "kind", &json!("Radial")).unwrap();
    let form = form_json(&doc);
    let shown = |name: &str| form["fields"].as_array().unwrap().iter().find(|f| f["name"] == name).map(|f| f["shown"].clone());
    assert_eq!(shown("count"), Some(json!(true)));
    assert_eq!(shown("count_x"), Some(json!(false)));
}

#[test]
fn form_set_joins_string_arrays_without_quotes() {
    let mut doc = document();
    open_form(&mut doc, op("tool.box"));
    form_set(&mut doc, "corner", &json!(["1 mm", "2", 3])).unwrap();
    let texts = &doc.ops.form.as_ref().unwrap().texts;
    let i = op("tool.box").params.iter().position(|p| p.name == "corner").unwrap();
    assert_eq!(texts[i], "1 mm, 2, 3");
    assert_eq!(param_value(&op("tool.box").params[i], &json!(["1 mm", "2", 3])), Ok(json!([1.0, 2.0, 3.0])));
}

#[test]
fn an_operation_that_needs_nothing_names_no_selected_node() {
    let doc = document();
    let r = Resolved { nodes: vec!["b1".into()], ..Default::default() };
    let boxed = op("ops.box");
    let c = calls(build(boxed, &r, &values(boxed, &Map::new()).unwrap(), &doc, &Env::default()).unwrap());
    assert!(!c[0].label.contains("Bracket"), "{}", c[0].label);
    assert!(c[0].label.starts_with("Box"), "{}", c[0].label);
}

#[test]
fn primitive_anchors_are_projected_onto_the_plane_and_come_last() {
    let doc = document();
    let r = Resolved::default();
    let boxed = op("tool.box");
    let c = calls(build(boxed, &r, &values(boxed, &given(&[("corner", json!([1, 2, 5]))])).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].args, vec![json!([1.0, 2.0, 0.0]), json!([20.0, 20.0, 10.0])]);
    let c = calls(build(boxed, &r, &values(boxed, &given(&[("corner", json!([1, 2, 5])), ("height", json!(-4))])).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].args[0], json!([1.0, 2.0, -4.0]), "a negative height extrudes down from the plane");
    let centre = op("tool.box_center");
    let c = calls(build(centre, &r, &values(centre, &given(&[("center", json!([10, 10, 7]))])).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].args[0], json!([0.0, 0.0, 0.0]));
    let cylinder = op("tool.cylinder");
    let c = calls(build(cylinder, &r, &values(cylinder, &given(&[("base", json!([1, 2, 5]))])).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].args[0], json!([1.0, 2.0, 0.0]));
    let sphere = op("tool.sphere");
    let c = calls(build(sphere, &r, &values(sphere, &given(&[("center", json!([1, 2, 5]))])).unwrap(), &doc, &Env::default()).unwrap());
    assert_eq!(c[0].args[0], json!([1.0, 2.0, 5.0]), "the sphere keeps its centre");
    // The sizes come first (RoboCAD's NumericField order), the anchor last.
    for (id, first) in [("tool.box", "width"), ("tool.box_center", "width"), ("tool.cylinder", "diameter"), ("tool.sphere", "diameter")] {
        let params = op(id).params;
        assert_eq!(params.first().map(|p| p.name), Some(first), "{id}");
        assert!(matches!(params.last().map(|p| p.name), Some("corner" | "center" | "base")), "{id}");
    }
}

#[test]
fn comb_and_continuity_read_the_last_node_with_a_body() {
    let mut doc = document();
    if let Some(d) = doc.doc.as_mut() {
        d.nodes.push(node("k1", "sketch", "Sketch"));
    }
    let comb = op("inspect.curvature");
    let r = Resolved { nodes: vec!["c1".into(), "k1".into()], ..Default::default() };
    assert_eq!(build(comb, &r, &Map::new(), &doc, &Env::default()), Ok(Built::Read(Read::CurvatureComb { node: "c1".into() })), "a sketch has no body: skipped");
    let r = Resolved { nodes: vec!["k1".into()], ..Default::default() };
    assert!(build(comb, &r, &Map::new(), &doc, &Env::default()).unwrap_err().starts_with("Select a curve"));
    let continuity = op("inspect.continuity");
    let r = Resolved { nodes: vec!["b1".into(), "k1".into()], ..Default::default() };
    assert_eq!(build(continuity, &r, &Map::new(), &doc, &Env::default()), Ok(Built::Read(Read::Continuity { node: "b1".into() })));
    let r = Resolved { nodes: vec!["k1".into()], ..Default::default() };
    assert!(build(continuity, &r, &Map::new(), &doc, &Env::default()).unwrap_err().starts_with("Select a body"));
}
