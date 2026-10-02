//! The Fastener hole and Clearance offset edits without a window: the
//! spec RoboCAD's dialog makes (depth 0 is through), the point a hole
//! needs, the clearance calls grouped by node in selection order, the
//! remembered values as the next form's presets, and the fastener tool's
//! pick refusals (not a face, no or another revision, no tool active).
use super::edits::{self, EditPlan, FastenerClick, LastFastener};
use super::fastener_tool::{self, check_pick, point_for, take_click};
use super::{PrintArgs, PrintCall, PrintOp};
use crate::app::actions::{Call, Origin, Replies};
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::{CadDocument, CadTarget, Connection};
use crate::cad::ops::{self, Env, FormState, Resolved};
use crate::cad::selection::Fixture;
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{CadClient, DocState, FastenerSpec, Health, NodeSummary, SelectionItem};

fn node(id: &str, name: &str) -> NodeSummary {
    NodeSummary { id: id.into(), kind: "body".into(), name: name.into(), visible: true, effective_visible: true, ..Default::default() }
}

/// Two bodies at RoboCAD's revision 4, connected.
fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    doc.client = Some(CadClient::new("http://127.0.0.1:8420").unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), revision: 4, ..Default::default() });
    doc.doc = Some(DocState { nodes: vec![node("b1", "Bracket"), node("b2", "Plate")], revision: 4, ..Default::default() });
    doc.doc_key = Some((None, 4));
    doc
}

fn face(node: &str, index: i64) -> SelectionItem {
    SelectionItem(node.into(), "face".into(), index)
}

fn values(id: &str, given: Value) -> Map<String, Value> {
    let Value::Object(given) = given else { panic!("an object") };
    ops::values(ops::entry(id).unwrap(), &given).unwrap()
}

fn fastener_plan(given: Value) -> Result<EditPlan, String> {
    let doc = document();
    let r = Resolved { nodes: vec!["b1".into()], faces: vec![("b1".into(), 2)], revision: 4, ..Default::default() };
    edits::build(PrintCall::Fastener, ops::entry("tool.fastener").unwrap(), &r, &values("tool.fastener", given), &doc, &Env::default())
}

#[test]
fn the_fastener_spec_is_robocads_dialog() {
    // Depth 0 is "through": `depth.value() or None`.
    let plan = fastener_plan(json!({"size": "M4", "kind": "tap", "extra": "0.15", "depth": "0", "point": [1.0, 2.0, 3.0]})).unwrap();
    let spec = FastenerSpec { size: "M4".into(), kind: "tap".into(), extra_clearance: 0.15, depth: None };
    assert_eq!(plan, EditPlan::Fastener { node: "b1".into(), face: 2, point: [1.0, 2.0, 3.0], spec: spec.clone(), revision: 4 });
    assert_eq!(spec.label(), "M4 tap");
    // A depth is sent as given; the defaults are RoboCAD's dialog's.
    let EditPlan::Fastener { spec, .. } = fastener_plan(json!({"depth": "12.5", "point": "0, 0, 5"})).unwrap() else { panic!("a fastener plan") };
    assert_eq!(spec, FastenerSpec { size: "M3".into(), kind: "clearance".into(), extra_clearance: 0.0, depth: Some(12.5) });
}

#[test]
fn a_hole_needs_a_point_and_a_face() {
    assert_eq!(fastener_plan(json!({})), Err("Click a face to place a hole".to_string()));
    let doc = document();
    let r = Resolved { revision: 4, ..Default::default() };
    let v = values("tool.fastener", json!({"point": [1.0, 2.0, 3.0]}));
    assert_eq!(edits::build(PrintCall::Fastener, ops::entry("tool.fastener").unwrap(), &r, &v, &doc, &Env::default()), Err("Click a face to place a hole".to_string()));
}

#[test]
fn clearance_calls_go_per_node_in_selection_order() {
    let doc = document();
    let faces = vec![("b2".to_string(), 1), ("b1".to_string(), 4), ("b2".to_string(), 3), ("b1".to_string(), 5)];
    let r = Resolved { nodes: vec!["b2".into(), "b1".into()], faces, revision: 4, ..Default::default() };
    let plan = edits::build(PrintCall::Clearance, ops::entry("tool.clearance").unwrap(), &r, &values("tool.clearance", json!({"amount": "-0.3"})), &doc, &Env::default()).unwrap();
    assert_eq!(plan, EditPlan::Clearance { groups: vec![("b2".into(), vec![1, 3]), ("b1".into(), vec![4, 5])], amount: -0.3, revision: 4 });
    // No faces: RoboCAD's error.
    let none = Resolved { revision: 4, ..Default::default() };
    assert_eq!(edits::build(PrintCall::Clearance, ops::entry("tool.clearance").unwrap(), &none, &values("tool.clearance", json!({})), &doc, &Env::default()), Err("Select holes, bosses or faces to offset".to_string()));
}

fn seeded(doc: &CadDocument, defaults: &crate::app::settings::CadDefaults, id: &str) -> Vec<String> {
    let entry = ops::entry(id).unwrap();
    let mut texts: Vec<String> = entry.params.iter().map(|p| p.default.to_string()).collect();
    edits::seed(entry, doc, &Env { defaults: Some(defaults), ..Default::default() }, &mut texts);
    texts
}

#[test]
fn the_forms_open_with_the_remembered_values() {
    let doc = document();
    let mut defaults = crate::app::settings::CadDefaults::default();
    // RoboCAD's first values: M3 clearance, 0, through; 0.2.
    assert_eq!(seeded(&doc, &defaults, "tool.fastener"), ["M3", "clearance", "0", "0", ""]);
    assert_eq!(seeded(&doc, &defaults, "tool.clearance"), ["0.2"]);
    defaults.fastener = LastFastener { size: "M5".into(), kind: "insert".into(), extra: 0.1, depth: 12.5 };
    defaults.clearance = -0.456;
    // The spin boxes' two decimals; the point stays empty.
    assert_eq!(seeded(&doc, &defaults, "tool.fastener"), ["M5", "insert", "0.1", "12.5", ""]);
    assert_eq!(seeded(&doc, &defaults, "tool.clearance"), ["-0.46"]);
}

#[test]
fn a_refused_edit_sends_nothing_and_remembers_nothing() {
    let mut doc = document();
    doc.client = None;
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    let mut settings = crate::app::settings::SettingsOwner::default();
    let out = edits::send(&mut doc, &mut call, EditPlan::Clearance { groups: vec![("b1".into(), vec![1])], amount: 0.4, revision: 4 }, &mut settings);
    assert!(matches!(&out, Outcome::Done(Err(e)) if e.starts_with("not connected to RoboCAD")));
    assert_eq!(settings.cad.clearance, edits::FIRST_CLEARANCE);
    assert!(doc.edit.is_none());
    // A pick made before RoboCAD's document moved on is refused by name.
    let mut doc = document();
    let spec = FastenerSpec { size: "M4".into(), ..FastenerSpec::default() };
    let mut settings = crate::app::settings::SettingsOwner::default();
    let out = edits::send(&mut doc, &mut call, EditPlan::Fastener { node: "b1".into(), face: 2, point: [0.0; 3], spec, revision: 3 }, &mut settings);
    assert!(matches!(&out, Outcome::Done(Err(e)) if e.contains("revision 3, now 4")));
    assert_eq!(settings.cad.fastener, LastFastener::default());
}

fn active(doc: &mut CadDocument) {
    let entry = ops::entry("tool.fastener").unwrap();
    doc.ops.active = Some(entry.id);
    doc.ops.form = Some(FormState { op: entry.id, texts: entry.params.iter().map(|p| p.default.to_string()).collect(), focus: None, select_all: false, began: 4, error: None });
}

fn pick_args(item: Option<SelectionItem>, picked_at: Option<u64>) -> PrintArgs {
    PrintArgs { op: PrintOp::Pick, item, picked_at, ..PrintArgs::default() }
}

#[test]
fn picks_are_refused_by_name() {
    let mut doc = document();
    let args = pick_args(Some(face("b1", 2)), Some(4));
    assert!(check_pick(&doc, &args).is_err_and(|e| e.starts_with("no fastener tool is active")));
    assert_eq!(fastener_tool::active_tool(&doc), None);
    active(&mut doc);
    assert_eq!(check_pick(&doc, &args), Ok(("tool.fastener", face("b1", 2), 4)));
    // Not a face, not in the tree, no item.
    assert!(check_pick(&doc, &pick_args(Some(SelectionItem("b1".into(), "body".into(), 0)), Some(4))).is_err_and(|e| e.contains("not a body item")));
    assert!(check_pick(&doc, &pick_args(Some(face("zz", 2)), Some(4))).is_err_and(|e| e.contains("no node zz")));
    assert!(check_pick(&doc, &pick_args(None, Some(4))).is_err_and(|e| e.starts_with("pick takes item")));
    // No revision, or another one.
    assert!(check_pick(&doc, &pick_args(Some(face("b1", 2)), None)).is_err_and(|e| e.starts_with("pass picked_at")));
    assert!(check_pick(&doc, &pick_args(Some(face("b1", 2)), Some(3))).is_err_and(|e| e.contains("picked at revision 3") && e.contains("now 4")));
}

#[test]
fn the_point_is_the_click_then_the_typed_point_then_the_face() {
    let mut doc = document();
    let click = FastenerClick { item: face("b1", 2), picked_at: 4, point: [1.0, 2.0, 3.0], snap: Some([1.0, 2.0, 0.0]) };
    doc.print.edits.click = Some(click.clone());
    // A note for another face is dropped.
    assert_eq!(take_click(&mut doc, &face("b1", 3), 4), None);
    assert_eq!(doc.print.edits.click, None);
    doc.print.edits.click = Some(click.clone());
    let noted = take_click(&mut doc, &face("b1", 2), 4);
    assert_eq!(noted.as_ref(), Some(&click));
    let unread = || -> Result<[f64; 3], String> { Err("unread".into()) };
    // The snap, else the hit; the face is not read.
    assert_eq!(point_for(noted.as_ref(), "9, 9, 9", unread), Ok(json!([1.0, 2.0, 0.0])));
    let unsnapped = FastenerClick { snap: None, ..click };
    assert_eq!(point_for(Some(&unsnapped), "", unread), Ok(json!([1.0, 2.0, 3.0])));
    // No click: the typed point, else the face's own.
    assert_eq!(point_for(None, "9, 9, 9", unread), Ok(json!("9, 9, 9")));
    assert_eq!(point_for(None, " ", || Ok([4.0, 5.0, 6.0])), Ok(json!([4.0, 5.0, 6.0])));
    assert_eq!(point_for(None, "", unread), Err("unread".to_string()));
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
fn a_click_pick_is_one_fastener_run_refused_with_nothing_sent() {
    let mut doc = document();
    let mut f = Fixture::at(4);
    let pick = CadAction::CadPrint(pick_args(Some(face("b1", 2)), Some(4)));
    // No tool: refused.
    assert!(matches!(apply(&pick, &mut doc, &mut f), Outcome::Done(Err(e)) if e.starts_with("no fastener tool")));
    // The tool's click on a disconnected document: the run is refused by
    // name, shown in the tool's form, nothing sent or remembered, the tool stays.
    active(&mut doc);
    doc.client = None;
    doc.print.edits.click = Some(FastenerClick { item: face("b1", 2), picked_at: 4, point: [1.0, 2.0, 3.0], snap: None });
    let out = apply(&pick, &mut doc, &mut f);
    assert!(matches!(&out, Outcome::Done(Err(e)) if e.starts_with("not connected to RoboCAD")));
    assert!(doc.ops.form.as_ref().and_then(|f| f.error.as_deref()).is_some_and(|e| e.starts_with("not connected")));
    assert_eq!(doc.ops.active, Some("tool.fastener"));
    assert_eq!(doc.print.edits.click, None);
    assert_eq!(doc.print.edits.last_pick, Some((face("b1", 2), 4)));
    assert!(doc.edit.is_none());
    // Without a click or a typed point, the face's point needs the topology.
    doc.client = Some(CadClient::new("http://127.0.0.1:8420").unwrap());
    assert!(matches!(apply(&pick, &mut doc, &mut f), Outcome::Done(Err(e)) if e.starts_with("the faces are not available")));
    assert!(doc.edit.is_none());
}

/// Malformed persisted choices are refused before publication, so forms
/// cannot acquire an invalid size from preferences.
#[test]
fn a_malformed_fastener_default_is_not_published() {
    let mut settings = crate::app::settings::SettingsOwner::default();
    let before = settings.cad.clone();
    let mut invalid = before.clone();
    invalid.fastener.size = "M9".into();
    assert!(settings.set_cad(invalid).is_err());
    assert_eq!(settings.cad, before);
    assert_eq!(settings.revision, 0);
}

/// The remembered clearance is said to be this window's.
#[test]
fn the_state_says_whose_clearance_is_remembered() {
    let doc = document();
    let state = edits::state_json(&doc, None);
    assert_eq!(state["last_clearance"], json!(edits::FIRST_CLEARANCE));
    assert_eq!(state["last_clearance_note"], json!(edits::LAST_CLEARANCE_NOTE));
}

/// Document replacement discards picks, not remembered tool defaults.
#[test]
fn replacement_seeds_forms_and_state_from_the_same_global_defaults() {
    let mut settings = crate::app::settings::SettingsOwner::default();
    let mut defaults = settings.cad.clone();
    defaults.fastener = LastFastener { size: "M5".into(), kind: "insert".into(), extra: 0.2, depth: 12.0 };
    defaults.clearance = -0.45;
    defaults.wall_threshold = Some(0.8);
    settings.set_cad(defaults).unwrap();
    let replacement = document();
    assert!(replacement.print.edits.click.is_none());
    assert_eq!(seeded(&replacement, &settings.cad, "tool.fastener"), ["M5", "insert", "0.2", "12", ""]);
    assert_eq!(seeded(&replacement, &settings.cad, "tool.clearance"), ["-0.45"]);
    let state = super::state_json(&replacement, Some(&settings.cad));
    assert_eq!(state["checks"]["threshold"], json!(0.8));
    assert_eq!(state["edits"]["last_clearance"], json!(-0.45));
    assert_eq!(state["edits"]["last_fastener"]["size"], "M5");
}

#[test]
fn malformed_dimensions_do_not_change_defaults_or_revision() {
    let mut settings = crate::app::settings::SettingsOwner::default();
    let mutations: [fn(&mut crate::app::settings::CadDefaults); 4] = [
        |d: &mut crate::app::settings::CadDefaults| d.wall_threshold = Some(0.0),
        |d: &mut crate::app::settings::CadDefaults| d.fastener.extra = f64::NAN,
        |d: &mut crate::app::settings::CadDefaults| d.fastener.depth = 501.0,
        |d: &mut crate::app::settings::CadDefaults| d.clearance = 5.1,
    ];
    for mutate in mutations {
        let mut invalid = settings.cad.clone();
        mutate(&mut invalid);
        assert!(settings.set_cad(invalid).is_err());
        assert_eq!(settings.cad, crate::app::settings::CadDefaults::default());
        assert_eq!(settings.revision, 0);
    }
}
