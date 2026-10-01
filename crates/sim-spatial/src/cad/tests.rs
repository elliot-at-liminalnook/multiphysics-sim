//! CAD mode's controls without a window: every control `system_ui` lists
//! (the panel's own list, `panel::controls`) fits a registered pattern
//! (`CadAction::controls`), and its action, written as the REST command
//! `system_ui` shows, parses back to the same `CadAction` value a click
//! writes. So REST, `system_ui` and the panel's buttons cannot drift apart.
use super::actions::{CadAction, rest_form};
use super::document::{CadDocument, CadTarget, Connection, Edit, EditDone};
use crate::app::actions::{self, Action};
use serde_json::{Value, json};
use sim_runtime::cad_client::{CommandInfo, DocState, Health, History, NodeSummary, SelectionItem};

fn node(id: &str, parent: Option<&str>, name: &str) -> NodeSummary {
    NodeSummary { id: id.into(), kind: "body".into(), name: name.into(), parent: parent.map(str::to_string), visible: true, effective_visible: true, ..Default::default() }
}

/// A connected document with a group, two bodies (one selected), two
/// materials, an undo step and one GUI registry command.
fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    doc.client = Some(sim_runtime::cad_client::CadClient::new("http://127.0.0.1:8420").unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), gui: true, nodes: 3, revision: 4, ..Default::default() });
    doc.doc = Some(DocState {
        nodes: vec![node("g1", None, "Frame"), node("b1", Some("g1"), "Bracket"), node("b2", Some("g1"), "Plate")],
        materials: vec![json!({"id": "pla", "name": "PLA"}), json!({"id": "al6061", "name": "Aluminium 6061"})],
        history: History { undo: vec!["Move".into()], redo: vec![] },
        revision: 4,
        ..Default::default()
    });
    doc.selection = vec![SelectionItem("b1".into(), "body".into(), 0)];
    doc.commands = Some(Ok([("view.fit".to_string(), CommandInfo { label: "Fit view".into(), category: "View".into(), keys: json!("Home") })].into_iter().collect()));
    doc
}

#[test]
fn every_cad_control_fits_a_pattern_and_round_trips_through_rest() {
    let doc = document();
    let controls = super::panel::controls(&doc);
    let patterns = <CadAction as Action>::controls();
    let ids: Vec<&str> = controls.iter().map(|c| c.id.as_str()).collect();
    for expected in ["cad:undo", "cad:redo", "cad:save", "cad:refresh", "cad:fit", "cad:physical", "cad:delete", "cad:node:b1", "cad:visible:b2", "cad:locked:b1", "cad:disabled:b1", "cad:material:b1:al6061", "cad:command:view.fit", "cad:mode:face", "cad:mode:point", "cad:select_all", "cad:invert_selection", "cad:select_same_material", "cad:edges_to_faces", "cad:tool:move", "cad:tool:push_pull", "cad:tool:measure", "cad:cancel"] {
        assert!(ids.contains(&expected), "{expected} is not listed: {ids:?}");
    }
    for c in &controls {
        assert!(patterns.iter().any(|p| actions::control_matches(p, &c.id)), "{} fits no registered pattern", c.id);
        let Value::Object(mut args) = rest_form(&c.action) else { panic!("{}: not an object", c.id) };
        let name = args.remove("command").and_then(|v| v.as_str().map(str::to_string)).expect("a command name");
        let parsed = <CadAction as Action>::parse(&sim_api::Command { command: name.clone(), args: Value::Object(args) }).unwrap_or_else(|e| panic!("{}: {name} does not parse: {e}", c.id));
        assert_eq!(parsed, c.action, "{}: REST form and click differ", c.id);
    }
    // States follow the document: nothing to redo; undo names RoboCAD's label.
    let find = |id: &str| controls.iter().find(|c| c.id == id).unwrap();
    assert!(find("cad:redo").ready.is_err());
    assert!(find("cad:undo").ready.is_ok() && find("cad:undo").label.contains("Move"));
    // A tree row and a 3D pick write the same selection.
    assert_eq!(find("cad:node:b2").action, CadAction::CadSelect { ids: vec!["b2".into()], items: Vec::new(), extend: false, toggle: false });
}

#[test]
fn edits_are_refused_while_one_is_in_flight_or_disconnected() {
    let mut doc = document();
    doc.edit = Some(Edit { label: "Patch Bracket: visible".into(), job: crate::jobs::Job::finished(0, Ok(EditDone { message: String::new(), result: Value::Null })), started: std::time::Instant::now(), clear_selection: None });
    let controls = super::panel::controls(&doc);
    for id in ["cad:undo", "cad:save", "cad:delete", "cad:visible:b1", "cad:locked:b1"] {
        let c = controls.iter().find(|c| c.id == id).unwrap();
        assert!(c.ready.as_ref().is_err_and(|e| e.contains("in flight") && e.contains("Patch Bracket")), "{id}: {:?}", c.ready);
    }
    doc.edit = None;
    doc.connection = Connection::Lost { error: "RoboCAD GET /: connect: refused".into(), since: std::time::Instant::now() };
    let controls = super::panel::controls(&doc);
    for id in ["cad:undo", "cad:save", "cad:delete", "cad:visible:b1", "cad:physical"] {
        let c = controls.iter().find(|c| c.id == id).unwrap();
        assert!(c.ready.is_err(), "{id} is enabled while the connection is lost");
    }
    // Refresh stays enabled: it is the retry.
    assert!(controls.iter().find(|c| c.id == "cad:refresh").unwrap().ready.is_ok());
}
