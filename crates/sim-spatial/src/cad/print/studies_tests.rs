//! The print studies without a window: the registry's picks in its order
//! with RoboCAD's labels, each study's body and RoboCAD's refusals from a
//! local test archive with a print study at its revision, the dialogs'
//! precheck while the registry is unread or failed, a refused start, and a
//! print block coloured through the shared stress rule (red at a safety
//! factor of 1).
use super::PrintCall;
use super::overlay;
use super::studies::{self, NO_STUDY, REGISTRY_READING, Request, StudyPlan};
use crate::app::actions::{Call, Origin, Replies};
use crate::cad::document::{CadDocument, CadTarget};
use crate::cad::ops::{self, Env, Resolved};
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use sim_domain_robot::stress_results::colormap;
use crate::cad::types::{FilamentInfo, NodeResult, Ordered, PrintRegistry, PrinterInfo, SplitRequest};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn registry() -> PrintRegistry {
    PrintRegistry {
        printers: Ordered(vec![
            ("bambu-h2c".into(), PrinterInfo { name: "Bambu H2C".into(), usable_mm: vec![325.0, 320.0, 320.0] }),
            ("prusa-mk4".into(), PrinterInfo { name: "Prusa MK4".into(), usable_mm: vec![250.0, 210.0, 220.0] }),
        ]),
        materials: Ordered(vec![("pla-basic".into(), FilamentInfo::default()), ("petg-hf".into(), FilamentInfo::default())]),
        ..Default::default()
    }
}

fn study_json() -> Value {
    json!({
        "printer": "bambu-h2c",
        "material": "pla-basic",
        "safety_target": 2.5,
        "parts": [
            {"node": "b1", "fixtures": [{"region": {"bottom": true}}], "loads": [{"region": {"contact": "b2"}, "direction": [0, 0, -1], "magnitude": 20}]},
            {"node": "b2", "fixtures": [], "loads": []},
        ],
    })
}

/// An archive with bodies b1 "Bracket" and b2 "Plate", a split group g1
/// "Arm split" (its `robot.print_split`) with pieces p1 and p2, and
/// `study` as `robot_settings.print_study`.
fn archive(study: Value) -> sim_cad::ArchiveDocument {
    let path = Path::new("/tmp/print-studies-test.rcad");
    let empty = sim_cad::ArchiveDocument::from_bytes(path, sim_cad::edit::empty_archive(None).unwrap(), &|| false, &|_| {}).unwrap();
    let stamps = sim_cad::annotations::Stamps::default();
    let mut edit = sim_cad::Edit::of(&empty);
    let mut ids: Vec<(String, &str)> = Vec::new();
    {
        let mut cx = sim_cad::ops::Ctx { doc: &empty, stamps: &stamps, edit: &mut edit, centroid: &|_| None, cancelled: &|| false };
        let cube = |x: f64| sim_cad::kernel::Built { kind: sim_cad::kernel::Kind::Solid, brep: sim_cad::kernel::build(&sim_cad::kernel::Shape::Box { corner: [x, 0.0, 0.0], size: [10.0, 10.0, 10.0] }, &|| false).unwrap() };
        ids.push((cx.add_built(cube(0.0), "Bracket", None, None).unwrap(), "b1"));
        ids.push((cx.add_built(cube(20.0), "Plate", None, None).unwrap(), "b2"));
        let mut robot = Map::new();
        robot.insert("robot".into(), json!({"print_split": {"seams": [], "hardware": []}}));
        let group = cx.add_node("group", "Arm split", None, robot).unwrap();
        ids.push((cx.add_built(cube(40.0), "Arm 1", None, Some(&group)).unwrap(), "p1"));
        ids.push((cx.add_built(cube(60.0), "Arm 2", None, Some(&group)).unwrap(), "p2"));
        ids.push((group, "g1"));
    }
    edit.manifest["robot_settings"]["print_study"] = study;
    // The generated ids (12 hex digits, unique) become the test's names.
    let mut text = serde_json::to_string(&edit.manifest).unwrap();
    for (generated, wanted) in &ids {
        text = text.replace(generated.as_str(), wanted);
        if let Some(b) = edit.entries.remove(&format!("brep/{generated}.brep")) {
            edit.entries.insert(format!("brep/{wanted}.brep"), b);
        }
    }
    edit.manifest = serde_json::from_str(&text).unwrap();
    empty.apply(edit).unwrap()
}

/// A local document showing [`archive`] at revision 4 (each install is the
/// next revision), with the registry read.
fn document_with(study: Value) -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::File(PathBuf::from("/tmp/print-studies-test.rcad")));
    let snapshot = Arc::new(crate::cad::sync::snapshot_of(Arc::new(archive(study)), &|| false, &|_| {}).unwrap());
    for _ in 0..4 {
        crate::cad::local::install(&mut doc, snapshot.clone());
    }
    doc.connection = crate::cad::Connection::Connected;
    assert_eq!(doc.shown_revision(), 4);
    doc.print.studies.registry = Some(((doc.generation, doc.mesh_retry), Ok(registry())));
    doc
}

fn document() -> CadDocument {
    document_with(study_json())
}

fn values(id: &str, given: Value) -> Map<String, Value> {
    let Value::Object(given) = given else { panic!("an object") };
    ops::values(ops::entry(id).unwrap(), &given).unwrap()
}

fn selected(nodes: &[&str]) -> Resolved {
    Resolved { nodes: nodes.iter().map(|n| n.to_string()).collect(), revision: 4, ..Default::default() }
}

fn build(doc: &CadDocument, call: PrintCall, id: &str, nodes: &[&str], given: Value) -> Result<StudyPlan, String> {
    studies::build(call, ops::entry(id).unwrap(), &selected(nodes), &values(id, given), doc, &Env::default())
}

fn body(plan: StudyPlan) -> Value {
    match plan.request {
        Request::Start(v) => v,
        other => panic!("expected a /print/{{kind}} body, got {other:?}"),
    }
}

#[test]
fn the_registry_picks_keep_its_order_and_robocads_labels() {
    let doc = document();
    let printers = studies::picks("printers", &doc);
    assert_eq!(printers, vec![("bambu-h2c".to_string(), "bambu-h2c (325 × 320 × 320 mm)".to_string()), ("prusa-mk4".to_string(), "prusa-mk4 (250 × 210 × 220 mm)".to_string())]);
    let ids = |v: Vec<(String, String)>| v.into_iter().map(|(k, l)| (k.clone(), k == l)).collect::<Vec<_>>();
    assert_eq!(ids(studies::picks("printer_ids", &doc)), vec![("bambu-h2c".to_string(), true), ("prusa-mk4".to_string(), true)]);
    assert_eq!(ids(studies::picks("filaments", &doc)), vec![("pla-basic".to_string(), true), ("petg-hf".to_string(), true)]);
    assert!(studies::picks("motors", &doc).is_empty());
    // Unread: no choices (the dialogs are refused by precheck meanwhile).
    let mut unread = document();
    unread.print.studies.registry = None;
    assert!(studies::picks("printers", &unread).is_empty());
}

#[test]
fn split_sends_the_chosen_printer_and_joint_as_a_background_job() {
    let doc = document();
    let plan = build(&doc, PrintCall::Split, "print.split", &["b1"], json!({"printer": "prusa-mk4", "joint": "dovetail"})).unwrap();
    assert_eq!(plan.kind, "split");
    assert_eq!(plan.revision, 4);
    let want = SplitRequest { node: "b1".into(), printer: Some("prusa-mk4".into()), joint: Some("dovetail".into()), expected_revision: Some(4), background: true, ..SplitRequest::default() };
    assert_eq!(plan.request, Request::Split(want));
    assert_eq!(plan.message, "Started split of Bracket for the prusa-mk4");
    // The joint defaults to RoboCAD's first choice.
    let Request::Split(r) = build(&doc, PrintCall::Split, "print.split", &["b1"], json!({"printer": "bambu-h2c"})).unwrap().request else { panic!("a split") };
    assert_eq!(r.joint.as_deref(), Some("auto"));
    // A printer the registry does not list, or none, is refused by name.
    let e = build(&doc, PrintCall::Split, "print.split", &["b1"], json!({"printer": "ender-3"})).unwrap_err();
    assert_eq!(e, "ender-3 is not a printer of the printing registry (its printers: bambu-h2c, prusa-mk4)");
    let e = build(&doc, PrintCall::Split, "print.split", &["b1"], json!({})).unwrap_err();
    assert_eq!(e, "Split selected for printing…: choose a printer");
}

#[test]
fn strength_and_plan_send_the_study_with_the_revision_or_robocads_explanation() {
    let doc = document();
    let mut want = study_json();
    want["expected_revision"] = json!(4);
    let plan = build(&doc, PrintCall::Strength, "print.strength", &[], json!({})).unwrap();
    assert_eq!(plan.kind, "analyze");
    assert_eq!(body(plan), want);
    let plan = build(&doc, PrintCall::Plan, "print.plan", &[], json!({})).unwrap();
    assert_eq!(plan.kind, "plan");
    assert_eq!(body(plan), want);
    // No study: RoboCAD's explanation, for both.
    let none = document_with(Value::Null);
    assert_eq!(build(&none, PrintCall::Strength, "print.strength", &[], json!({})).unwrap_err(), NO_STUDY);
    assert_eq!(build(&none, PrintCall::Plan, "print.plan", &[], json!({})).unwrap_err(), NO_STUDY);
    // A selection read at another revision: refused by name, nothing guessed.
    let r = Resolved { revision: 3, ..Default::default() };
    let e = studies::build(PrintCall::Strength, ops::entry("print.strength").unwrap(), &r, &values("print.strength", json!({})), &doc, &Env::default()).unwrap_err();
    assert_eq!(e, "the document is at revision 4, the selection was read at 3; try again");
}

#[test]
fn strength_split_takes_the_first_study_part_selected_with_the_studys_settings() {
    let doc = document();
    // Selection order b2, b1: the study's order decides (RoboCAD's `next(p for p in parts …)`).
    let plan = build(&doc, PrintCall::StrengthSplit, "print.strength_split", &["b2", "b1"], json!({})).unwrap();
    assert_eq!(plan.kind, "strength_split");
    let part = study_json()["parts"][0].clone();
    assert_eq!(body(plan), json!({"printer": "bambu-h2c", "material": "pla-basic", "safety_target": 2.5, "node": "b1", "part": part, "expected_revision": 4}));
    let refusal = ops::entry("print.strength_split").unwrap().refusal;
    assert_eq!(build(&doc, PrintCall::StrengthSplit, "print.strength_split", &["p1"], json!({})).unwrap_err(), refusal);
    assert_eq!(build(&doc, PrintCall::StrengthSplit, "print.strength_split", &[], json!({})).unwrap_err(), refusal);
}

#[test]
fn assembly_takes_a_selected_split_or_a_selected_pieces_split() {
    let doc = document();
    let group = |nodes: &[&str]| body(build(&doc, PrintCall::Assembly, "print.assembly", nodes, json!({})).unwrap());
    assert_eq!(group(&["g1"]), json!({"group": "g1", "expected_revision": 4}));
    assert_eq!(group(&["p2"]), json!({"group": "g1", "expected_revision": 4}));
    assert_eq!(group(&["b1", "p1"]), json!({"group": "g1", "expected_revision": 4}));
    let refusal = ops::entry("print.assembly").unwrap().refusal;
    assert_eq!(build(&doc, PrintCall::Assembly, "print.assembly", &["b1"], json!({})).unwrap_err(), refusal);
    assert_eq!(build(&doc, PrintCall::Assembly, "print.assembly", &[], json!({})).unwrap_err(), refusal);
}

#[test]
fn coupons_send_the_split_or_none_with_the_chosen_printer_and_filament() {
    let doc = document();
    let given = json!({"printer": "prusa-mk4", "material": "petg-hf"});
    let plan = build(&doc, PrintCall::Coupons, "print.coupons", &["p1"], given.clone()).unwrap();
    assert_eq!(plan.kind, "coupons");
    assert_eq!(body(plan), json!({"group": "g1", "printer": "prusa-mk4", "material": "petg-hf", "expected_revision": 4}));
    // Nothing selected (or no split): material bars only.
    assert_eq!(body(build(&doc, PrintCall::Coupons, "print.coupons", &[], given.clone()).unwrap())["group"], Value::Null);
    assert_eq!(body(build(&doc, PrintCall::Coupons, "print.coupons", &["b2"], given).unwrap())["group"], Value::Null);
    let e = build(&doc, PrintCall::Coupons, "print.coupons", &[], json!({"printer": "bambu-h2c", "material": "abs"})).unwrap_err();
    assert_eq!(e, "abs is not a filament of the printing registry (its filaments: pla-basic, petg-hf)");
}

#[test]
fn the_split_and_coupon_dialogs_wait_for_the_registry() {
    let mut doc = document();
    let split = ops::entry("print.split").unwrap();
    let coupons = ops::entry("print.coupons").unwrap();
    let strength = ops::entry("print.strength").unwrap();
    let ops_split = ops::entry("ops.print_split").unwrap();
    assert_eq!(studies::precheck(split, &doc, &[]), None);
    assert_eq!(studies::precheck(ops_split, &doc, &[]), None);
    doc.print.studies.registry = None;
    assert_eq!(studies::precheck(split, &doc, &[]).as_deref(), Some(REGISTRY_READING));
    // ops.print_split's printer pick would be empty (its default blanked).
    assert_eq!(studies::precheck(ops_split, &doc, &[]).as_deref(), Some(REGISTRY_READING));
    assert_eq!(studies::precheck(coupons, &doc, &[]).as_deref(), Some(REGISTRY_READING));
    assert_eq!(studies::precheck(strength, &doc, &[]), None);
    doc.print.studies.registry = Some(((doc.generation, doc.mesh_retry), Err("registry.json: not JSON".into())));
    assert_eq!(studies::precheck(coupons, &doc, &[]).as_deref(), Some("the printing registry could not be read: registry.json: not JSON"));
    assert_eq!(studies::precheck(ops_split, &doc, &[]).as_deref(), Some("the printing registry could not be read: registry.json: not JSON"));
    // A read from another generation is not this document's.
    doc.print.studies.registry = Some(((doc.generation + 1000, 0), Ok(registry())));
    assert_eq!(studies::precheck(split, &doc, &[]).as_deref(), Some(REGISTRY_READING));
}

#[test]
fn a_start_read_before_the_document_moved_is_refused_and_starts_nothing() {
    let mut doc = document();
    let plan = build(&doc, PrintCall::Strength, "print.strength", &[], json!({})).unwrap();
    let snapshot = doc.local.clone().unwrap();
    crate::cad::local::install(&mut doc, snapshot);
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    let out = studies::send(&mut doc, &mut call, plan);
    assert!(matches!(&out, Outcome::Done(Err(e)) if e == "the document moved to revision 5 since this was read at revision 4; read it again"), "{out:?}");
    assert!(doc.print.jobs.jobs.is_empty());
}

#[test]
fn the_registry_reads_the_repositorys_file_in_its_order() {
    let r = studies::read_registry().unwrap();
    assert!(r.printers.get("bambu-h2c").is_some_and(|p| p.usable_mm.len() == 3));
    assert!(!r.materials.is_empty());
    assert!(r.sha256.as_deref().is_some_and(|s| s.len() == 64));
}

#[test]
fn a_print_block_colours_its_body_through_the_shared_rule() {
    let block = |sf: Value| NodeResult { results: json!({"section": "print", "safety_factor": sf, "passes": false, "cad_revision": 4}), yield_strength_pa: Some(3.0e7) };
    let positions = [[0.0f32, 0.0, 0.0], [120.0, -40.0, 9.0]];
    // A safety factor of 1 is failure: red everywhere, whatever the material's yield.
    let at_failure = overlay::inputs(&block(json!(1.0))).expect("a print block with a safety factor");
    let colours = crate::cad::results::cad_colours(&at_failure, at_failure.com_m.unwrap(), &positions);
    assert_eq!(colours, vec![colormap(1.0); 2]);
    // A failure index of 0.1 % is the scale's blue end, uniformly (log10 may
    // round a hair off -3, so compare the channels, not exact values).
    let safe = overlay::inputs(&block(json!(1000.0))).unwrap();
    let colours = crate::cad::results::cad_colours(&safe, safe.com_m.unwrap(), &positions);
    assert_eq!(colours[0], colours[1]);
    let blue = colormap(0.0);
    for (got, want) in colours[0].iter().zip(blue) {
        assert!((got - want).abs() < 1e-4, "{:?} is not the blue end {blue:?}", colours[0]);
    }
    // No usable safety factor, or another section: not coloured.
    assert!(overlay::inputs(&block(Value::Null)).is_none());
    assert!(overlay::inputs(&block(json!(0.0))).is_none());
    assert!(overlay::inputs(&block(json!(-2.0))).is_none());
    assert!(overlay::inputs(&NodeResult { results: json!({"section": "links", "safety_factor": 1.0}), yield_strength_pa: None }).is_none());
    // Published one revision after the snapshot it analysed: current until the next edit.
    let b = json!({"cad_revision": 4});
    assert_eq!(overlay::staleness(&b, 5), "current");
    assert_eq!(overlay::staleness(&b, 7), "stale (computed at revision 4, now 7)");
    assert_eq!(overlay::staleness(&json!({}), 7), "unknown (RoboCAD recorded no cad_revision)");
    // The results panel's line ends without nested parentheses.
    assert_eq!(overlay::tag(&b, 5), " (current)");
    assert_eq!(overlay::tag(&b, 7), "; stale (computed at revision 4, now 7)");
    assert_eq!(overlay::tag(&json!({}), 7), "; staleness unknown (RoboCAD recorded no cad_revision)");
}
