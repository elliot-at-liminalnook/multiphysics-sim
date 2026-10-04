//! The outliner without a window: the search keeps ancestors and
//! descendants, the collapse state survives an edit and a search, the
//! Shift range and Ctrl toggle write the expected `CadSelect`, a drop
//! builds RoboCAD's `_drop` parent and index, every `cad:tree:*` control
//! fits its pattern and round-trips through REST, and an edit while
//! another is in flight is refused by name with nothing sent.
use super::controls::{MenuRow, controls_of, menu_rows};
use super::handle::select_action;
use super::state::{MenuState, group_paths, move_plan, range, shown};
use super::{TreeArgs, TreeOp};
use crate::app::actions::{self, Action, Call, Origin, Replies};
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::{CadDocument, CadTarget, Connection, Edit, EditDone};
use crate::cad::rest_form::rest_form;
use crate::cad::selection::Fixture;
use serde_json::Value;
use sim_api::Outcome;
use sim_runtime::cad_client::{CadClient, DocState, Health, NodeSummary, SelectionItem};

fn node(id: &str, kind: &str, parent: Option<&str>, name: &str, children: &[&str]) -> NodeSummary {
    NodeSummary {
        id: id.into(),
        kind: kind.into(),
        name: name.into(),
        parent: parent.map(str::to_string),
        children: children.iter().map(|c| c.to_string()).collect(),
        visible: true,
        effective_visible: true,
        ..Default::default()
    }
}

/// RoboCAD's tree at revision 4, in walk order:
/// Frame (g1: Bracket b1, Arm g2 (Arm plate b3), Plate b2), Base b4.
fn tree() -> DocState {
    DocState {
        roots: vec!["g1".into(), "b4".into()],
        nodes: vec![
            node("g1", "group", None, "Frame", &["b1", "g2", "b2"]),
            node("b1", "body", Some("g1"), "Bracket", &[]),
            node("g2", "group", Some("g1"), "Arm", &["b3"]),
            node("b3", "body", Some("g2"), "Arm plate", &[]),
            node("b2", "body", Some("g1"), "Plate", &[]),
            node("b4", "body", None, "Base", &[]),
        ],
        revision: 4,
        ..Default::default()
    }
}

/// Connected to RoboCAD at revision 4.
fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    doc.client = Some(CadClient::new("http://127.0.0.1:8420").unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), revision: 4, ..Default::default() });
    doc.doc = Some(tree());
    doc.doc_key = Some((None, 4));
    doc.open_fixture();
    doc
}

/// Not connected: selection changes are not pushed (no job starts).
fn offline() -> CadDocument {
    let mut doc = document();
    doc.client = None;
    doc.local = None;
    doc.connection = Connection::Lost { error: "RoboCAD GET /: connect: refused".into(), since: std::time::Instant::now() };
    doc
}

fn ids(doc: &CadDocument) -> Vec<String> {
    shown(doc, &[]).into_iter().map(|s| s.row.id).collect()
}

fn body(id: &str) -> SelectionItem {
    SelectionItem(id.into(), "body".into(), 0)
}

/// Applies `action` through the one handler REST, `system_ui` and the window use.
fn apply(action: &CadAction, doc: &mut CadDocument, f: &mut Fixture) -> Outcome {
    let mut plane = crate::cad::sketch::CadActivePlane::default();
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    let mut cx = Cx { settings: &mut crate::app::settings::SettingsOwner::default(), doc, shared: f.shared(), meshes: None, topology: None, view: None, plane: &mut plane, sketches: None, display: None, views: None, files: None, components: &mut crate::cad::components::ComponentsState::default(), composition: &mut crate::cad::composition::CadCompositionState::default(), experiments: &mut crate::cad::experiments::ExperimentsState::default(), review: &mut crate::cad::experiment_review::ReviewState::default(), motion: &mut crate::cad::motion::MotionState::default(), camera: Vec::new() };
    crate::cad::actions::handle(action, &mut call, &mut cx)
}

fn tree_args(op: TreeOp, f: impl FnOnce(&mut TreeArgs)) -> CadAction {
    let mut a = TreeArgs::of(op);
    f(&mut a);
    a.action()
}

fn refused(outcome: &Outcome, needle: &str) -> bool {
    matches!(outcome, Outcome::Done(Err(e)) if e.contains(needle))
}

#[test]
fn the_search_keeps_ancestors_and_descendants() {
    let mut doc = document();
    doc.tree.search = "  ARM ".into();
    // "Arm" and "Arm plate" match; Frame is their ancestor; nothing else.
    assert_eq!(ids(&doc), ["g1", "g2", "b3"]);
    doc.tree.search = "plate".into();
    assert_eq!(ids(&doc), ["g1", "g2", "b3", "b2"]);
    // A group's match brings its whole subtree.
    doc.tree.search = "frame".into();
    assert_eq!(ids(&doc), ["g1", "b1", "g2", "b3", "b2"]);
    doc.tree.search = "nothing".into();
    assert!(ids(&doc).is_empty());
    doc.tree.search = "   ".into();
    assert_eq!(ids(&doc).len(), 6);
}

#[test]
fn the_collapse_state_survives_an_edit_and_a_search() {
    let mut doc = offline();
    let mut f = Fixture::at(4);
    assert!(matches!(apply(&TreeArgs::on(TreeOp::Collapse, "g2").action(), &mut doc, &mut f), Outcome::Done(Ok(_))));
    assert_eq!(ids(&doc), ["g1", "b1", "g2", "b2", "b4"]);
    // A new tree from RoboCAD (an edit renamed Bracket): still collapsed.
    let mut next = tree();
    next.nodes[1].name = "Bracket 2".into();
    next.revision = 5;
    doc.doc = Some(next);
    assert_eq!(ids(&doc), ["g1", "b1", "g2", "b2", "b4"]);
    // A search shows the match expanded without changing the record.
    assert!(matches!(apply(&tree_args(TreeOp::Search, |a| a.text = Some("arm plate".into())), &mut doc, &mut f), Outcome::Done(Ok(_))));
    assert_eq!(ids(&doc), ["g1", "g2", "b3"]);
    assert!(shown(&doc, &[]).iter().all(|s| s.open));
    // Expanding or collapsing while searching is refused by name.
    assert!(refused(&apply(&TreeArgs::on(TreeOp::Expand, "g2").action(), &mut doc, &mut f), "while a search is typed"));
    assert!(refused(&apply(&TreeArgs::of(TreeOp::CollapseAll).action(), &mut doc, &mut f), "while a search is typed"));
    assert!(doc.tree.collapsed.contains("g2"));
    doc.tree.search.clear();
    assert_eq!(ids(&doc), ["g1", "b1", "g2", "b2", "b4"]);
    // Collapse all records every node with children; Expand all clears.
    apply(&TreeArgs::of(TreeOp::CollapseAll).action(), &mut doc, &mut f);
    assert_eq!(ids(&doc), ["g1", "b4"]);
    apply(&TreeArgs::of(TreeOp::ExpandAll).action(), &mut doc, &mut f);
    assert_eq!(ids(&doc).len(), 6);
    // A row without children has nothing to collapse.
    assert!(refused(&apply(&TreeArgs::on(TreeOp::Toggle, "b1").action(), &mut doc, &mut f), "has no children"));
}

#[test]
fn shift_ranges_and_ctrl_toggles_write_the_expected_select() {
    let mut doc = offline();
    let mut f = Fixture::at(4);
    // No anchor yet: Shift selects the row alone.
    assert_eq!(select_action(&doc, "b2", true, false), CadAction::CadSelect { ids: vec!["b2".into()], items: Vec::new(), extend: false, toggle: false, picked_at: None });
    // A press sets the anchor; Shift then selects the shown rows between, replacing.
    apply(&tree_args(TreeOp::Select, |a| a.id = Some("b1".into())), &mut doc, &mut f);
    assert_eq!(doc.tree.anchor.as_deref(), Some("b1"));
    assert_eq!(f.items(), [body("b1")]);
    let shift = tree_args(TreeOp::Select, |a| (a.id, a.extend) = (Some("b2".into()), Some(true)));
    assert!(matches!(apply(&shift, &mut doc, &mut f), Outcome::Done(Ok(_))));
    assert_eq!(f.items(), ["b1", "g2", "b3", "b2"].map(body));
    // Shift keeps the anchor; collapsed rows are not in the range.
    assert_eq!(doc.tree.anchor.as_deref(), Some("b1"));
    doc.tree.collapsed.insert("g2".into());
    assert_eq!(range(&shown(&doc, &[]), Some("b1"), "b4"), ["b1", "g2", "b2", "b4"]);
    // Shift+Ctrl adds the range to the selection.
    assert_eq!(select_action(&doc, "b4", true, true), CadAction::CadSelect { ids: ["b1", "g2", "b2", "b4"].map(String::from).to_vec(), items: Vec::new(), extend: true, toggle: false, picked_at: None });
    // Ctrl toggles one row.
    assert_eq!(select_action(&doc, "b4", false, true), CadAction::CadSelect { ids: vec!["b4".into()], items: Vec::new(), extend: false, toggle: true, picked_at: None });
    apply(&tree_args(TreeOp::Select, |a| (a.id, a.toggle) = (Some("b1".into()), Some(true))), &mut doc, &mut f);
    assert_eq!(f.items(), ["g2", "b3", "b2"].map(body));
    // A right press on an unselected row selects it first, as RoboCAD's `_menu`.
    apply(&tree_args(TreeOp::Menu, |a| (a.id, a.open) = (Some("b4".into()), Some(true))), &mut doc, &mut f);
    assert_eq!(f.items(), [body("b4")]);
    assert_eq!(doc.tree.menu.as_ref().and_then(|m| m.id.as_deref()), Some("b4"));
    assert_eq!(doc.tree.menu.as_ref().map(|m| m.ids.clone()), Some(vec!["b4".to_string()]));
    // A restart ends the menu (its revision belongs to the old service).
    super::restarted(&mut doc);
    assert!(doc.tree.menu.is_none());
}

#[test]
fn a_drop_builds_robocads_parent_and_index() {
    let doc = document();
    let b4 = vec!["b4".to_string()];
    // In front of a sibling: its parent and its index among the parent's children.
    assert_eq!(move_plan(&doc, &b4, None, Some("b2")), Ok((Some("g1".into()), Some(2))));
    assert_eq!(move_plan(&doc, &["b2".to_string()], None, Some("b4")), Ok((None, Some(1))));
    // Onto a group: into it, at its end; nowhere: the top level.
    assert_eq!(move_plan(&doc, &b4, Some("g2"), None), Ok((Some("g2".into()), None)));
    assert_eq!(move_plan(&doc, &b4, None, None), Ok((None, None)));
    // RoboCAD's refusals, by name, before anything is sent.
    assert!(move_plan(&doc, &b4, Some("b1"), None).is_err_and(|e| e.contains("Bracket is not a group")));
    assert!(move_plan(&doc, &["g1".to_string()], Some("g2"), None).is_err_and(|e| e.contains("into itself or its descendants")));
    assert!(move_plan(&doc, &["g1".to_string()], None, Some("b3")).is_err_and(|e| e.contains("into itself or its descendants")));
    assert!(move_plan(&doc, &["b1".to_string()], None, Some("b1")).is_err_and(|e| e.contains("in front of itself")));
    assert!(move_plan(&doc, &[], None, None).is_err_and(|e| e.starts_with("nothing to move")));
    // "Move to group" lists every group but the moving ones and their descendants.
    let state = tree();
    assert_eq!(group_paths(&state, &[]), [("g1".to_string(), "Frame".to_string()), ("g2".to_string(), "Frame / Arm".to_string())]);
    assert!(group_paths(&state, &["g1".to_string()]).is_empty());
}

/// A control's action as `system_ui` lists it parses back to the same value.
fn round_trips(id: &str, action: &CadAction) {
    let Value::Object(mut args) = rest_form(action) else { panic!("{id}: not an object") };
    let name = args.remove("command").and_then(|v| v.as_str().map(str::to_string)).expect("a command name");
    let parsed = <CadAction as Action>::parse(&sim_api::Command { command: name.clone(), args: Value::Object(args) }).unwrap_or_else(|e| panic!("{id}: {name} does not parse: {e}"));
    assert_eq!(&parsed, action, "{id}: REST form and click differ");
}

#[test]
fn every_tree_control_fits_its_pattern_and_round_trips_through_rest() {
    let mut doc = document();
    doc.tree.dialog = Some(super::state::DialogState { ids: vec!["b1".into()], draft: "Legs".into(), error: None, began: 4 });
    doc.tree.menu = Some(MenuState { id: Some("g2".into()), ids: vec!["g2".into()], at: Some([10.5, 200.25]), began: 4 });
    let selection = vec![body("g2")];
    let controls = controls_of(&doc, &selection);
    let patterns = <CadAction as Action>::controls();
    let listed: Vec<&str> = controls.iter().map(|c| c.0.as_str()).collect();
    for expected in ["cad:tree:new_group", "cad:tree:expand_all", "cad:tree:collapse_all", "cad:tree:lock", "cad:tree:unlock", "cad:tree:hide", "cad:tree:show", "cad:tree:group_selection", "cad:tree:move_top", "cad:tree:move-g1", "cad:tree:set_active-g2", "cad:tree:clear_active", "cad:tree:toggle-g1", "cad:tree:dialog_ok", "cad:tree:dialog_cancel", "cad:tree:menu_close"] {
        assert!(listed.contains(&expected), "{expected} is not listed: {listed:?}");
    }
    for (id, _, action, _) in &controls {
        assert!(patterns.iter().any(|p| actions::control_matches(p, id)), "{id} fits no registered pattern");
        round_trips(id, action);
    }
    // States: a group cannot move into itself; edits need a selection.
    let find = |id: &str| controls.iter().find(|c| c.0 == id).unwrap();
    assert!(find("cad:tree:move-g2").3.as_ref().is_err_and(|e| e.contains("into itself")));
    assert!(find("cad:tree:move-g1").3.is_ok());
    assert!(controls_of(&doc, &[]).iter().find(|c| c.0 == "cad:tree:lock").unwrap().3.as_ref().is_err_and(|e| e == "nothing is selected"));
    // The menu's entries (with the menu's ids and revision) round-trip too,
    // and follow RoboCAD's order for one selected group.
    let rows = menu_rows(&doc, &selection);
    let labels: Vec<&str> = rows.iter().filter_map(|r| match r {
        MenuRow::Entry { label, .. } => Some(label.as_str()),
        _ => None,
    }).collect();
    assert_eq!(labels, ["Fit in view", "Isolate", "Hide", "Show", "Lock", "Unlock", "Group selection…", "Top level", "Frame", "Make unique (bake instance)", "Set as active group", "Delete", "Clear active group", "Show all"]);
    for row in &rows {
        if let MenuRow::Entry { label, action, .. } = row {
            round_trips(label, action);
        }
    }
}

#[test]
fn an_edit_while_another_is_in_flight_is_refused_with_nothing_sent() {
    let mut doc = document();
    let mut f = Fixture::at(4);
    doc.edit = Some(Edit { label: "Patch Bracket: visible".into(), job: crate::jobs::Job::finished(0, Ok(EditDone { message: String::new(), result: Value::Null })), started: std::time::Instant::now(), clear_selection: None, activates_plane: false, retarget: None });
    let seq = doc.edit_seq;
    let edits = [
        tree_args(TreeOp::Move, |a| (a.ids, a.parent, a.revision) = (Some(vec!["b4".into()]), Some("g1".into()), Some(4))),
        tree_args(TreeOp::Rename, |a| (a.id, a.text) = (Some("b1".into()), Some("Bracket 2".into()))),
        tree_args(TreeOp::Lock, |a| (a.ids, a.locked) = (Some(vec!["b1".into()]), Some(true))),
        tree_args(TreeOp::Visible, |a| (a.ids, a.visible) = (Some(vec!["b1".into()]), Some(false))),
        tree_args(TreeOp::Group, |a| (a.ids, a.name) = (Some(vec!["b1".into()]), Some("Legs".into()))),
        TreeArgs::on(TreeOp::SetActive, "g2").action(),
    ];
    for edit in &edits {
        let out = apply(edit, &mut doc, &mut f);
        assert!(refused(&out, "another CAD edit is in flight: Patch Bracket: visible"), "{edit:?}: {:?}", match &out {
            Outcome::Done(r) => format!("{r:?}"),
            _ => "pending".into(),
        });
        assert_eq!(doc.edit_seq, seq, "{edit:?} sent a request");
        assert_eq!(doc.edit_label(), Some("Patch Bracket: visible"));
    }
    // A row read at an older revision is refused by name too, once nothing is in flight.
    doc.edit = None;
    let stale = tree_args(TreeOp::Move, |a| (a.ids, a.revision) = (Some(vec!["b4".into()]), Some(3)));
    assert!(refused(&apply(&stale, &mut doc, &mut f), "revision 3, now 4"));
    assert_eq!(doc.edit_seq, seq);
}

#[test]
fn renames_and_groups_are_checked_before_sending() {
    let mut doc = document();
    let mut f = Fixture::at(4);
    // Begin: the field opens on the row with the name.
    apply(&TreeArgs::on(TreeOp::BeginRename, "b1").action(), &mut doc, &mut f);
    assert_eq!(doc.tree.rename.as_ref().map(|r| (r.id.as_str(), r.draft.as_str(), r.began)), Some(("b1", "Bracket", 4)));
    // Unchanged (after stripping): nothing sent, the field closes.
    let seq = doc.edit_seq;
    let same = tree_args(TreeOp::Rename, |a| (a.id, a.text) = (Some("b1".into()), Some("  Bracket ".into())));
    assert!(matches!(apply(&same, &mut doc, &mut f), Outcome::Done(Ok(_))));
    assert!(doc.tree.rename.is_none());
    // Empty: refused by name.
    let empty = tree_args(TreeOp::Rename, |a| (a.id, a.text) = (Some("b1".into()), Some("   ".into())));
    assert!(refused(&apply(&empty, &mut doc, &mut f), "cannot be empty"));
    // The dialog's OK with no name stays open with the reason.
    apply(&tree_args(TreeOp::GroupDialog, |a| (a.open, a.ids) = (Some(true), Some(Vec::new()))), &mut doc, &mut f);
    assert_eq!(doc.tree.dialog.as_ref().map(|d| d.ids.len()), Some(0));
    assert!(refused(&apply(&tree_args(TreeOp::Group, |a| a.name = Some(" ".into())), &mut doc, &mut f), "group name is needed"));
    assert!(doc.tree.dialog.as_ref().is_some_and(|d| d.error.is_some()));
    // Only a group can be the active group; misplaced arguments are named.
    assert!(refused(&apply(&TreeArgs::on(TreeOp::SetActive, "b1").action(), &mut doc, &mut f), "not a group"));
    assert!(refused(&apply(&tree_args(TreeOp::Search, |a| a.locked = Some(true)), &mut doc, &mut f), "locked belongs to op lock"));
    assert_eq!(doc.edit_seq, seq);
}

/// Written T49 fixture: actual tree renderer retains compound pointer behavior
/// while focused keyboard activation captures a stable node id.
#[test]
fn rendered_tree_row_keyboard_activation_keeps_stable_selection_intent() {
    use bevy::prelude::*;
    use bevy::ecs::system::RunSystemOnce;
    use crate::app::actions::Act;
    use crate::ui_kit::{Kit, UiFonts, activation::{Activated, HeldControl, KeyboardOnly, Ordinary}};
    fn render(mut commands: Commands, doc: Res<CadDocument>) {
        let fonts = UiFonts { regular: default(), italic: default(), mono: default(), icons: default(), medium: default(), semibold: default() };
        commands.spawn((Node::default(), DespawnOnExit(crate::app::ModeScope::Cad))).with_children(|p| {
            super::rows::draw(p, &Kit::new(&fonts), &doc, &[]);
        });
    }
    let mut world = World::new();
    world.insert_resource(document());
    world.insert_resource(Messages::<Act<CadAction>>::default());
    world.run_system_once(render).unwrap();
    let mut rows = world.query::<(Entity, &super::rows::TreeRowId)>();
    let entity = rows.iter(&world).find(|(_, row)| row.id == "b1").unwrap().0;
    assert!(world.get::<Ordinary>(entity).is_some());
    assert!(world.get::<KeyboardOnly>(entity).is_some());
    assert!(world.get::<HeldControl>(entity).is_some());
    world.run_system_once(crate::cad::activation::stamp).unwrap();
    world.entity_mut(entity).insert(Activated);
    world.run_system_once(crate::cad::activation::tree_keyboard).unwrap();
    let actions = world.resource_mut::<Messages<Act<CadAction>>>().drain().collect::<Vec<_>>();
    assert_eq!(actions.len(), 1);
    let CadAction::Captured { action, .. } = &actions[0].action else { panic!("keyboard selection must retain source") };
    assert_eq!(**action, select_action(world.resource::<CadDocument>(), "b1", false, false));
    world.resource_mut::<CadDocument>().generation += 1;
    world.run_system_once(crate::cad::activation::refuse).unwrap();
    assert!(world.get::<Activated>(entity).is_none());
}
