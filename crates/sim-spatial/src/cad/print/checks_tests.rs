//! The wall check and validation without a window (cad-print): Python's
//! float text in RoboCAD's status lines, `validate_for_export`'s messages,
//! the refusals and node lists `build` makes (an empty one included), the read cache, the
//! remembered threshold, the landing (older generations dropped), the
//! stale points, and the overhang shading's coupling to the build plate.
use super::PrintCall;
use super::checks::{self, CheckPlan, NodeThin, ValidateMeta, WallMeta, cache_key, issue_line, py_float, split_cached, validation_lines, validation_status, wall_status};
use crate::cad::display::{CadDisplay, DisplayArgs, DisplaySetting, apply_display};
use crate::cad::document::{CadDocument, CadTarget, Connection};
use crate::cad::ops::{Env, Resolved, entry, values};
use crate::jobs::Job;
use bevy::ecs::message::Messages;
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use serde_json::{Map, Value, json};
use sim_runtime::cad_client::{CadClient, DocState, Health, NodeSummary, ThinRegion, Validation, ValidationIssue};

fn node(id: &str, kind: &str, name: &str, shown: bool) -> NodeSummary {
    NodeSummary { id: id.into(), kind: kind.into(), name: name.into(), visible: shown, effective_visible: shown, ..Default::default() }
}

/// A connected document at revision 4: body b1 "Bracket" and sheet s1
/// "Skin" shown, body b2 "Plate" hidden, an instance, a mesh and a joint.
fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    doc.client = Some(CadClient::new("http://127.0.0.1:8420").unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), revision: 4, ..Default::default() });
    doc.doc = Some(DocState {
        nodes: vec![node("b1", "body", "Bracket", true), node("b2", "body", "Plate", false), node("s1", "sheet", "Skin", true), node("i1", "instance", "Bracket copy", true), node("m1", "mesh", "Scan", true), node("j1", "joint", "hip", true)],
        revision: 4,
        ..Default::default()
    });
    doc.doc_key = Some((None, 4));
    doc
}

fn region(x: f64) -> ThinRegion {
    ThinRegion { point: [x, 0.0, 1.0], thickness: 0.6, face: 2 }
}

fn wall_check(threshold: Value) -> Map<String, Value> {
    Map::from_iter([("threshold".to_string(), threshold)])
}

/// Python's `str(float)`, as RoboCAD's f-strings print the threshold.
#[test]
fn floats_print_as_python_does() {
    for (v, text) in [(1.2, "1.2"), (2.0, "2.0"), (0.25, "0.25"), (20.0, "20.0"), (0.1, "0.1"), (-0.0, "-0.0"), (1e-4, "0.0001"), (1.5e-7, "1.5e-07"), (1e16, "1e+16"), (123456.75, "123456.75")] {
        assert_eq!(py_float(v), text, "{v}");
    }
}

/// ui/app.py:1126 and printing.py:147-148, ui/app.py:1131-1135.
#[test]
fn status_texts_are_robocads() {
    assert_eq!(wall_status(3, 1.2), "3 thin region(s) under 1.2 mm");
    assert_eq!(wall_status(0, 2.0), "No walls thinner than 2.0 mm");
    assert_eq!(wall_status(1, 0.25), "1 thin region(s) under 0.25 mm");
    let located = ValidationIssue { severity: "error".into(), message: "self-intersecting faces".into(), location: Some([1.26, -3.0, 10.04]), fix: Some("heal the body".into()) };
    assert_eq!(issue_line("Bracket", &located), "Bracket: self-intersecting faces near (1.3, -3.0, 10.0) — heal the body");
    let bare = ValidationIssue { severity: "warning".into(), message: "open shell".into(), location: None, fix: Some(String::new()) };
    assert_eq!(issue_line("Skin", &bare), "Skin: open shell");
    let good = Validation { valid: true, watertight: true, ..Default::default() };
    let bad = Validation { valid: true, watertight: false, issues: vec![bare.clone(), located.clone()], ..Default::default() };
    let silent = Validation { valid: false, watertight: true, ..Default::default() };
    let (ok, lines) = validation_lines(&[("Bracket".into(), good.clone()), ("Skin".into(), good.clone())]);
    assert!(ok && lines.is_empty());
    assert_eq!(validation_status(ok, 2, &lines), Ok("2 body(ies): valid and watertight.".to_string()));
    let (ok, lines) = validation_lines(&[("Bracket".into(), good), ("Skin".into(), bad), ("Lid".into(), silent)]);
    assert!(!ok);
    assert_eq!(lines, ["Skin: open shell", "Skin: self-intersecting faces near (1.3, -3.0, 10.0) — heal the body", "Lid: not a valid solid"]);
    assert_eq!(validation_status(ok, 3, &lines), Err(lines.join("; ")));
}

#[test]
fn build_takes_the_selection_else_the_visible_bodies_and_refuses_nothing() {
    let doc = document();
    let wall = entry("print.wall_check").unwrap();
    let defaults = values(wall, &Map::new()).unwrap();
    assert_eq!(defaults["threshold"], json!(1.2));
    let none = Resolved::default();
    // RoboCAD's `doc.bodies(True)`: shown bodies and sheets, in the tree's order.
    assert_eq!(checks::build(PrintCall::WallCheck, wall, &none, &defaults, &doc, &Env::default()), Ok(CheckPlan::Wall { threshold: 1.2, nodes: vec!["b1".into(), "s1".into()] }));
    // The selected nodes, in selection order, whatever their kind (a 404 is skipped by the job).
    let picked = Resolved { nodes: vec!["s1".into(), "j1".into(), "b2".into()], ..Default::default() };
    assert_eq!(checks::build(PrintCall::WallCheck, wall, &picked, &wall_check(json!(0.8)), &doc, &Env::default()), Ok(CheckPlan::Wall { threshold: 0.8, nodes: vec!["s1".into(), "j1".into(), "b2".into()] }));
    assert!(checks::build(PrintCall::WallCheck, wall, &none, &wall_check(json!(-1.0)), &doc, &Env::default()).is_err());
    assert!(checks::build(PrintCall::WallCheck, wall, &none, &Map::new(), &doc, &Env::default()).is_err());
    // Validation ignores the selection.
    let validate = entry("print.validate").unwrap();
    assert_eq!(checks::build(PrintCall::Validate, validate, &picked, &Map::new(), &doc, &Env::default()), Ok(CheckPlan::Validate { bodies: vec![("b1".into(), "Bracket".into()), ("s1".into(), "Skin".into())] }));
    // Nothing selected or visible: RoboCAD's loop runs over an empty list
    // (ui/app.py:1117-1126), so the plan is empty and lands at once with
    // "No walls thinner than …".
    let mut hidden = document();
    for n in &mut hidden.doc.as_mut().unwrap().nodes {
        n.effective_visible = false;
    }
    assert_eq!(checks::build(PrintCall::WallCheck, wall, &none, &defaults, &hidden, &Env::default()), Ok(CheckPlan::Wall { threshold: 1.2, nodes: Vec::new() }));
    let answer = checks::start_wall(&mut hidden, 1.2, Vec::new()).unwrap();
    assert_eq!((answer["cached"].clone(), answer["sent"].clone()), (json!(0), json!(0)));
    assert_eq!(hidden.status, Some(Ok("No walls thinner than 1.2 mm".to_string())));
    assert!(hidden.print.checks.wall_job.is_none());
}

/// Cached reads are keyed by (generation, node, revision, threshold); a
/// check of cached nodes sends nothing and lands at once, remembering its threshold.
#[test]
fn a_repeat_check_of_unchanged_nodes_sends_nothing() {
    let mut doc = document();
    let g = doc.generation;
    assert_ne!(cache_key(g, "b1", 4, 1.2), cache_key(g, "b1", 4, 1.25));
    assert_ne!(cache_key(g, "b1", 4, 1.2), cache_key(g, "b1", 5, 1.2));
    assert_ne!(cache_key(g, "b1", 4, 1.2), cache_key(g + 1, "b1", 4, 1.2));
    let cached: [(&str, NodeThin); 2] = [("b1", Some(vec![region(1.0), region(2.0)])), ("s1", None)];
    for (id, thin) in cached {
        doc.print.checks.cache.insert(cache_key(g, id, 4, 1.2), thin);
    }
    let nodes = vec!["b1".to_string(), "s1".to_string()];
    let (known, missing) = split_cached(&doc.print.checks, g, 4, 1.2, &nodes);
    assert_eq!((known.len(), missing.len()), (2, 0));
    let (known, missing) = split_cached(&doc.print.checks, g, 4, 0.8, &nodes);
    assert_eq!((known.len(), missing), (0, nodes.clone()));
    let (_, missing) = split_cached(&doc.print.checks, g, 5, 1.2, &nodes);
    assert_eq!(missing, nodes);

    // The form opens at RoboCAD's 1.2 until a check has run.
    let wall = entry("print.wall_check").unwrap();
    let mut texts = vec!["1.2".to_string()];
    super::seed(wall, &doc, &Env::default(), &mut texts);
    assert_eq!(texts, ["1.2"]);
    assert_eq!(checks::state_json(&doc)["threshold"], json!(1.2));

    let answer = checks::start_wall(&mut doc, 1.2, nodes.clone()).unwrap();
    assert_eq!((answer["cached"].clone(), answer["sent"].clone()), (json!(2), json!(0)));
    assert_eq!(doc.status, Some(Ok("2 thin region(s) under 1.2 mm".to_string())));
    let state = checks::state_json(&doc);
    assert_eq!(state["wall_check_running"], Value::Null);
    assert_eq!(state["wall_check"]["nodes"], json!([{"id": "b1", "name": "Bracket", "count": 2, "geometry": true}, {"id": "s1", "name": "Skin", "count": null, "geometry": false}]));
    assert_eq!((state["wall_check"]["shown"].clone(), state["wall_check"]["stale"].clone()), (json!(true), json!(false)));
    assert_eq!(state["open_edge_check"], json!(checks::OPEN_EDGE_NOTE));

    // The next form opens at the threshold that ran.
    doc.print.checks.cache.insert(cache_key(g, "b1", 4, 0.8), Some(Vec::new()));
    checks::start_wall(&mut doc, 0.8, vec!["b1".into()]).unwrap();
    assert_eq!(doc.status, Some(Ok("No walls thinner than 0.8 mm".to_string())));
    let mut texts = vec!["1.2".to_string()];
    super::seed(wall, &doc, &Env::default(), &mut texts);
    assert_eq!(texts, ["0.8"]);
}

/// The points are drawn and clearable only at the shown revision.
#[test]
fn points_show_at_their_revision_and_clear() {
    let mut doc = document();
    let g = doc.generation;
    doc.print.checks.cache.insert(cache_key(g, "b1", 4, 1.2), Some(vec![region(1.0)]));
    checks::start_wall(&mut doc, 1.2, vec!["b1".into()]).unwrap();
    assert_eq!(checks::drawn(&doc).map(|w| w.points.clone()), Some(vec![Vec3::new(1.0, 0.0, 1.0)]));
    let controls = checks::controls(&doc);
    assert_eq!(controls.len(), 1);
    assert_eq!((controls[0].0.as_str(), controls[0].1.as_str()), ("cad:print:clear_checks", "Clear wall check marks"));
    assert_eq!(controls[0].2, super::PrintArgs::of(super::PrintOp::Clear));
    // A newer shown revision: kept, not drawn, said so.
    doc.doc_key = Some((None, 5));
    assert!(checks::drawn(&doc).is_none() && checks::controls(&doc).is_empty());
    let state = checks::state_json(&doc);
    assert_eq!((state["wall_check"]["stale"].clone(), state["wall_check"]["shown"].clone()), (json!(true), json!(false)));
    assert!(state["wall_check"]["note"].as_str().is_some_and(|n| n.contains("not drawn")));
    doc.doc_key = Some((None, 4));
    let answer = checks::clear(&mut doc);
    assert_eq!(answer["cleared"], json!(1));
    assert!(checks::drawn(&doc).is_none() && checks::controls(&doc).is_empty());
}

/// JobResults: a result of an older generation is dropped; this
/// generation's lands with RoboCAD's text, errors naming the node.
#[test]
fn results_land_for_this_generation_only() {
    let mut doc = document();
    let g = doc.generation;
    let meta = |nodes: &[&str]| WallMeta { threshold: 1.2, revision: 4, generation: g, nodes: nodes.iter().map(|n| n.to_string()).collect(), known: Vec::new() };
    doc.print.checks.wall_job = Some((meta(&["b1"]), Job::finished(g + 1000, Ok(vec![("b1".to_string(), Some(vec![region(1.0)]))]))));
    let mut world = World::default();
    world.init_resource::<Messages<bevy::window::RequestRedraw>>();
    world.insert_resource(doc);
    world.run_system_once(checks::receive).unwrap();
    {
        let doc = world.resource::<CadDocument>();
        assert!(doc.print.checks.wall_job.is_none() && doc.print.checks.wall.is_none() && doc.status.is_none());
    }
    world.resource_mut::<CadDocument>().print.checks.wall_job = Some((meta(&["b1", "m1"]), Job::finished(g, Ok(vec![("b1".to_string(), Some(vec![region(1.0), region(3.0)])), ("m1".to_string(), None)]))));
    world.run_system_once(checks::receive).unwrap();
    {
        let doc = world.resource::<CadDocument>();
        assert_eq!(doc.status, Some(Ok("2 thin region(s) under 1.2 mm".to_string())));
        assert_eq!(doc.print.checks.wall.as_ref().map(|w| w.counts.clone()), Some(vec![("b1".to_string(), Some(2)), ("m1".to_string(), None)]));
        assert!(doc.print.checks.cache.contains_key(&cache_key(g, "m1", 4, 1.2)), "a node without geometry is cached too");
    }
    world.resource_mut::<CadDocument>().print.checks.wall_job = Some((meta(&["b1"]), Job::finished(g, Err("b1: RoboCAD GET /nodes/b1/thin?threshold=1.2: kernel failed".to_string()))));
    world.run_system_once(checks::receive).unwrap();
    assert_eq!(world.resource::<CadDocument>().status, Some(Err("Wall thickness check: Bracket: RoboCAD GET /nodes/b1/thin?threshold=1.2: kernel failed".to_string())));

    let bad = Validation { valid: false, watertight: false, issues: vec![ValidationIssue { severity: "error".into(), message: "invalid solid".into(), location: Some([0.0, 0.0, 0.0]), fix: None }], ..Default::default() };
    let reports = vec![("Bracket".to_string(), Validation { valid: true, watertight: true, ..Default::default() }), ("Skin".to_string(), bad)];
    world.resource_mut::<CadDocument>().print.checks.validate_job = Some((ValidateMeta { revision: 4, generation: g, bodies: 2 }, Job::finished(g, Ok(reports))));
    world.run_system_once(checks::receive).unwrap();
    let doc = world.resource::<CadDocument>();
    assert_eq!(doc.status, Some(Err("Skin: invalid solid near (0.0, 0.0, 0.0)".to_string())));
    let state = checks::state_json(doc);
    assert_eq!((state["validation"]["ok"].clone(), state["validation"]["lines"].clone(), state["validation"]["bodies"].clone()), (json!(false), json!(["Skin: invalid solid near (0.0, 0.0, 0.0)"]), json!(2)));
}

/// A late result for an older revision keeps the newer cached reads and
/// says its points are not drawn.
#[test]
fn a_late_result_for_an_older_revision_does_not_prune_or_claim_drawn() {
    let mut doc = document();
    let g = doc.generation;
    doc.doc_key = Some((None, 5));
    doc.print.checks.cache.insert(cache_key(g, "s1", 5, 1.2), None);
    let meta = WallMeta { threshold: 1.2, revision: 4, generation: g, nodes: vec!["b1".into()], known: Vec::new() };
    checks::land_wall(&mut doc, meta, Ok(vec![("b1".to_string(), Some(vec![region(1.0)]))]));
    assert!(doc.print.checks.cache.contains_key(&cache_key(g, "s1", 5, 1.2)), "the newer read is kept");
    assert!(doc.print.checks.cache.contains_key(&cache_key(g, "b1", 4, 1.2)));
    assert!(checks::drawn(&doc).is_none());
    let Some(Ok(status)) = doc.status.clone() else { panic!("a status") };
    assert!(status.starts_with("1 thin region(s) under 1.2 mm (read at revision 4") && status.contains("not drawn"), "{status}");
    // A current landing prunes the older revision's reads.
    let meta = WallMeta { threshold: 1.2, revision: 5, generation: g, nodes: vec!["s1".into()], known: Vec::new() };
    checks::land_wall(&mut doc, meta, Ok(vec![("s1".to_string(), None)]));
    assert!(!doc.print.checks.cache.contains_key(&cache_key(g, "b1", 4, 1.2)));
    assert_eq!(doc.status, Some(Ok("No walls thinner than 1.2 mm".to_string())));
}

/// "Toggle overhang shading" is a display toggle; the build plate sets it
/// (RoboCAD's `toggle_build_plate`), its own toggle flips only it.
#[test]
fn overhang_shading_follows_the_build_plate() {
    let mut d = CadDisplay::default();
    let Some(crate::cad::actions::CadAction::CadDisplay(overhangs)) = super::command_action("print.overhangs") else { panic!("print.overhangs is a display action") };
    assert_eq!(overhangs, DisplayArgs { toggle: Some(DisplaySetting::Overhangs), ..Default::default() });
    apply_display(&mut d, &DisplayArgs { toggle: Some(DisplaySetting::BuildPlate), ..Default::default() }).unwrap();
    assert!(d.build_plate && d.overhangs);
    apply_display(&mut d, &overhangs).unwrap();
    assert!(d.build_plate && !d.overhangs);
    apply_display(&mut d, &overhangs).unwrap();
    assert!(d.build_plate && d.overhangs);
    apply_display(&mut d, &DisplayArgs { build_plate: Some(false), ..Default::default() }).unwrap();
    assert!(!d.build_plate && !d.overhangs);
}
