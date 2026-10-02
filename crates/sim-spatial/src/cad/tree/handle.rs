//! `cad_tree` ([`TreeArgs`], `CadAction::CadTree`): the outliner's intents
//! and their one handler ([`handle`]). Display ops (search, expand,
//! collapse, the menu, the dialog, the rename row) change `TreeState`;
//! selection goes through `selection::handle` as a row press's
//! `CadSelect`; each edit (rename, move, lock, show/hide, group, active
//! group) is exactly one RoboCAD call through `actions::edit_at`, refused
//! by name with nothing sent when it cannot go.
use super::state::{Claim, DialogState, DropTarget, MenuState, RenameState, group_paths, index_of, move_plan, parents_of_children, range, shown};
use crate::app::actions::{Call, Spec};
use crate::cad::actions::{CadAction, Cx, edit_at};
use crate::cad::document::{CadDocument, EditDone};
use crate::cad::selection::CadItems;
use crate::cad::sync::value;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sim_api::Outcome;

/// What `cad_tree` does.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TreeOp {
    /// The outliner's state (as `state_json` shows it).
    #[default]
    State,
    /// Filter the rows by `text` (RoboCAD's search: names containing it,
    /// with their ancestors and descendants, every row expanded).
    Search,
    /// Show node `id`'s children (recorded; not while searching).
    Expand,
    /// Hide node `id`'s children (recorded; not while searching).
    Collapse,
    /// Expand or collapse node `id`.
    Toggle,
    ExpandAll,
    CollapseAll,
    /// A row press on `id`: alone, `extend` (Shift: the shown rows from the
    /// anchor to it) or `toggle` (Ctrl/Cmd: add or remove it).
    Select,
    /// Open the inline name field on row `id` (a double-click).
    BeginRename,
    /// Rename `id` to `text` (stripped; unchanged does nothing; empty is refused).
    Rename,
    /// Close the inline name field without renaming.
    EndRename,
    /// Move `ids` (the selection when absent) into group `parent`, in front
    /// of node `before`, or to the top level (neither).
    Move,
    /// Lock (`locked` true) or unlock `ids` (the selection when absent).
    Lock,
    /// Show (`visible` true) or hide `ids` (the selection when absent).
    Visible,
    /// Group `ids` (absent: the open dialog's nodes, else the selection;
    /// [] makes an empty group) as a new group named `name`.
    Group,
    /// Make group `id` RoboCAD's active group; no `id` clears it.
    SetActive,
    /// The context menu: `open` true on row `id` (selected first when it
    /// is not) at `at`; false closes it.
    Menu,
    /// The "Organize components" dialog for `ids` (absent: the selection;
    /// [] for New group): `open` true or false.
    GroupDialog,
}

/// `cad_tree`'s arguments.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct TreeArgs {
    #[serde(default)]
    pub op: TreeOp,
    /// A node (expand, collapse, toggle, select, begin_rename, rename, set_active, menu).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Nodes (move, lock, visible, group, group_dialog; absent: the selection).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<String>>,
    /// The search text (search) or the new name (rename).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The new group's name (group).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The group to move into (move).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The node to move in front of (move).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Shift (select).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extend: Option<bool>,
    /// Ctrl/Cmd (select).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toggle: Option<bool>,
    /// Lock or unlock (lock).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    /// Show or hide (visible).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Open or close (menu, group_dialog).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
    /// Where the menu opens (menu; window logical px).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[f32; 2]>,
    /// The shown revision the row, menu, drag or dialog was read at
    /// (rename, move, lock, visible, group, set_active): refused by name
    /// when RoboCAD's document changed since. Absent checks only that an
    /// edit can be sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}

impl TreeArgs {
    pub(crate) fn of(op: TreeOp) -> TreeArgs {
        TreeArgs { op, ..TreeArgs::default() }
    }
    pub(crate) fn on(op: TreeOp, id: &str) -> TreeArgs {
        TreeArgs { op, id: Some(id.to_string()), ..TreeArgs::default() }
    }
    pub(crate) fn action(self) -> CadAction {
        CadAction::CadTree(self)
    }

    /// Each argument belongs to some ops; any other use is refused by name.
    fn check(&self) -> Result<(), String> {
        use TreeOp::*;
        let rules: [(bool, &str, &[TreeOp]); 13] = [
            (self.id.is_some(), "id", &[Expand, Collapse, Toggle, Select, BeginRename, Rename, SetActive, Menu]),
            (self.ids.is_some(), "ids", &[Move, Lock, Visible, Group, GroupDialog]),
            (self.text.is_some(), "text", &[Search, Rename]),
            (self.name.is_some(), "name", &[Group]),
            (self.parent.is_some(), "parent", &[Move]),
            (self.before.is_some(), "before", &[Move]),
            (self.extend.is_some(), "extend", &[Select]),
            (self.toggle.is_some(), "toggle", &[Select]),
            (self.locked.is_some(), "locked", &[Lock]),
            (self.visible.is_some(), "visible", &[Visible]),
            (self.open.is_some(), "open", &[Menu, GroupDialog]),
            (self.at.is_some(), "at", &[Menu]),
            (self.revision.is_some(), "revision", &[Rename, Move, Lock, Visible, Group, SetActive]),
        ];
        for (given, name, ops) in rules {
            if given && !ops.contains(&self.op) {
                let names: Vec<String> = ops.iter().map(|o| serde_json::to_value(o).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()).collect();
                return Err(format!("{name} belongs to op {}", names.join(", ")));
            }
        }
        Ok(())
    }
}

/// A node `id` of the shown tree, or the refusal naming it.
fn known(doc: &CadDocument, id: Option<&str>, op: &str) -> Result<String, String> {
    let id = id.ok_or_else(|| format!("{op} needs id (a node of the shown tree)"))?;
    if doc.doc.as_ref().is_some_and(|d| d.nodes.iter().any(|n| n.id == id)) {
        Ok(id.to_string())
    } else {
        Err(format!("no node {id} in the shown tree"))
    }
}

fn is_group(doc: &CadDocument, id: &str) -> bool {
    doc.doc.as_ref().is_some_and(|d| d.nodes.iter().any(|n| n.id == id && n.kind == "group"))
}

const WHILE_SEARCHING: &str = "the tree shows every match expanded while a search is typed; clear the search to expand or collapse rows";

/// The `CadSelect` a row press writes, as Qt's ExtendedSelection: Shift
/// selects the shown rows from the anchor to `id`, replacing the
/// selection; Shift+Ctrl/Cmd adds that range; Ctrl/Cmd toggles `id`;
/// otherwise `id` alone.
pub(crate) fn select_action(doc: &CadDocument, id: &str, extend: bool, toggle: bool) -> CadAction {
    let ids = if extend { range(&shown(doc, &[]), doc.tree.anchor.as_deref(), id) } else { vec![id.to_string()] };
    CadAction::CadSelect { ids, items: Vec::new(), extend: extend && toggle, toggle: toggle && !extend, picked_at: None }
}

/// The nodes an edit acts on: `ids`, else the selection's nodes.
fn targets(args: &TreeArgs, cx: &Cx) -> Vec<String> {
    args.ids.clone().unwrap_or_else(|| cx.shared.items().nodes())
}

/// Names for a status line: "Bracket", "Bracket, Plate" or "5 nodes".
fn names(doc: &CadDocument, ids: &[String]) -> String {
    if ids.len() > 3 { format!("{} nodes", ids.len()) } else { ids.iter().map(|i| doc.node_name(i)).collect::<Vec<_>>().join(", ") }
}

/// An edit's outcome: refused (Err) or started.
fn started(outcome: &Outcome) -> bool {
    !matches!(outcome, Outcome::Done(Err(_)))
}

/// `CadTree`, from any entry point.
pub(in crate::cad) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    let CadAction::CadTree(args) = action else { return Outcome::Done(Err("not an outliner action".into())) };
    if let Err(e) = args.check() {
        return Outcome::Done(Err(e));
    }
    let done = Outcome::Done;
    match args.op {
        TreeOp::State => done(Ok(state_json(cx.doc))),
        TreeOp::Search => {
            let text = args.text.clone().unwrap_or_default();
            let doc = &mut *cx.doc;
            if doc.tree.search != text {
                doc.tree.search = text;
                doc.touch();
            }
            done(Ok(json!({"search": doc.tree.search, "shown": shown(doc, &[]).len()})))
        }
        TreeOp::Expand | TreeOp::Collapse | TreeOp::Toggle => done(disclose(cx.doc, args)),
        TreeOp::ExpandAll | TreeOp::CollapseAll => done(all(cx.doc, args.op == TreeOp::CollapseAll)),
        TreeOp::Select => {
            let id = match known(cx.doc, args.id.as_deref(), "select") {
                Ok(id) => id,
                Err(e) => return done(Err(e)),
            };
            let (extend, toggle) = (args.extend.unwrap_or(false), args.toggle.unwrap_or(false));
            let select = select_action(cx.doc, &id, extend, toggle);
            let outcome = crate::cad::selection::handle(&select, call, cx);
            if started(&outcome) && !extend && cx.doc.tree.anchor.as_deref() != Some(id.as_str()) {
                cx.doc.tree.anchor = Some(id);
            }
            outcome
        }
        TreeOp::BeginRename => {
            let doc = &mut *cx.doc;
            let id = match known(doc, args.id.as_deref(), "begin_rename") {
                Ok(id) => id,
                Err(e) => return done(Err(e)),
            };
            let draft = doc.node_name(&id);
            doc.tree.rename = Some(RenameState { id: id.clone(), draft, selected: true, began: doc.shown_revision() });
            doc.tree.claim = Some(Claim::Rename);
            doc.tree.menu = None;
            doc.touch();
            done(Ok(json!({"renaming": id})))
        }
        TreeOp::Rename => rename(cx.doc, call, args),
        TreeOp::EndRename => {
            if cx.doc.tree.rename.take().is_some() {
                cx.doc.touch();
            }
            done(Ok(json!({"renaming": null})))
        }
        TreeOp::Move => {
            let ids = targets(args, cx);
            let doc = &mut *cx.doc;
            if doc.tree.drag.take().is_some() {
                doc.touch();
            }
            let (parent, index) = match move_plan(doc, &ids, args.parent.as_deref(), args.before.as_deref()) {
                Ok(plan) => plan,
                Err(e) => return done(Err(e)),
            };
            let place = match &parent {
                Some(p) => format!("into {}", doc.node_name(p)),
                None => "to the top level".to_string(),
            };
            let message = format!("Moved {} {place}", names(doc, &ids));
            edit_at(doc, call, args.revision, "Move in outliner".into(), move |c| c.move_nodes(&ids, parent.as_deref(), index).map(|r| EditDone { message, result: value(&r) }))
        }
        TreeOp::Lock | TreeOp::Visible => flags(cx, call, args),
        TreeOp::Group => group(cx, call, args),
        TreeOp::SetActive => {
            let doc = &mut *cx.doc;
            let id = match &args.id {
                None => None,
                Some(id) => match known(doc, Some(id.as_str()), "set_active") {
                    Ok(id) if is_group(doc, &id) => Some(id),
                    Ok(id) => return done(Err(format!("{} is not a group: only a group can be the active group", doc.node_name(&id)))),
                    Err(e) => return done(Err(e)),
                },
            };
            doc.tree.menu = None;
            let (label, message) = match &id {
                Some(id) => (format!("Set active group {}", doc.node_name(id)), format!("Active group: {}", doc.node_name(id))),
                None => ("Clear active group".to_string(), "Active group cleared".to_string()),
            };
            edit_at(doc, call, args.revision, label, move |c| c.set_active_group(id.as_deref()).map(|r| EditDone { message, result: value(&r) }))
        }
        TreeOp::Menu => menu(cx, call, args),
        TreeOp::GroupDialog => {
            let open = args.open.unwrap_or(true);
            let ids = targets(args, cx);
            let doc = &mut *cx.doc;
            doc.tree.menu = None;
            if !open {
                doc.tree.dialog = None;
                doc.touch();
                return done(Ok(json!({"dialog": null})));
            }
            if let Some(missing) = ids.iter().find(|id| !doc.has_node(id)) {
                return done(Err(format!("no node {missing} in the shown tree")));
            }
            doc.tree.form_sequence = doc.tree.form_sequence.wrapping_add(1);
            doc.tree.dialog = Some(DialogState { ids: ids.clone(), draft: String::new(), error: None, began: doc.shown_revision() });
            doc.tree.claim = Some(Claim::Dialog);
            doc.touch();
            done(Ok(json!({"dialog": {"ids": ids}, "message": "Organize components: type the group name and OK (op group with name)"})))
        }
    }
}

/// Expand, collapse or toggle one row.
fn disclose(doc: &mut CadDocument, args: &TreeArgs) -> Result<Value, String> {
    let id = known(doc, args.id.as_deref(), "expand and collapse")?;
    if doc.tree.searching() {
        return Err(WHILE_SEARCHING.into());
    }
    if !doc.doc.as_ref().is_some_and(|d| d.nodes.iter().any(|n| n.parent.as_deref() == Some(id.as_str()))) {
        return Err(format!("{} has no children to expand or collapse", doc.node_name(&id)));
    }
    let collapse = match args.op {
        TreeOp::Expand => false,
        TreeOp::Collapse => true,
        _ => !doc.tree.collapsed.contains(&id),
    };
    let changed = if collapse { doc.tree.collapsed.insert(id.clone()) } else { doc.tree.collapsed.remove(&id) };
    if changed {
        doc.touch();
    }
    Ok(json!({"id": id, "expanded": !collapse}))
}

/// Expand all (nothing recorded collapsed) or collapse all (every node with children).
fn all(doc: &mut CadDocument, collapse: bool) -> Result<Value, String> {
    if doc.tree.searching() {
        return Err(WHILE_SEARCHING.into());
    }
    let next = if collapse { doc.doc.as_ref().map(parents_of_children).unwrap_or_default() } else { Default::default() };
    if doc.tree.collapsed != next {
        doc.tree.collapsed = next;
        doc.touch();
    }
    Ok(json!({"collapsed": doc.tree.collapsed}))
}

/// `rename`: one `PATCH /nodes/{id} {"name"}` (RoboCAD's `_renamed`:
/// stripped, sent only when changed).
fn rename(doc: &mut CadDocument, call: &mut Call, args: &TreeArgs) -> Outcome {
    let open = doc.tree.rename.clone();
    let id = match known(doc, args.id.as_deref().or(open.as_ref().map(|r| r.id.as_str())), "rename") {
        Ok(id) => id,
        Err(e) => return Outcome::Done(Err(e)),
    };
    let ours = open.as_ref().filter(|r| r.id == id);
    let Some(text) = args.text.as_deref() else { return Outcome::Done(Err("rename needs text (the new name)".into())) };
    let name = text.trim().to_string();
    let old = doc.node_name(&id);
    if name == old {
        if ours.is_some() {
            doc.tree.rename = None;
            doc.touch();
        }
        return Outcome::Done(Ok(json!({"renamed": false, "message": format!("{old} keeps its name")})));
    }
    if name.is_empty() {
        return Outcome::Done(Err("A name cannot be empty: type one, or Escape to keep the current name".into()));
    }
    let began = args.revision.or(ours.map(|r| r.began));
    let mut attrs = Map::new();
    attrs.insert("name".into(), Value::String(name.clone()));
    let message = format!("Renamed {old} to {name}");
    let target = id.clone();
    let outcome = edit_at(doc, call, began, format!("Rename {old}"), move |c| c.patch(&target, &attrs).map(|d| EditDone { message, result: value(&d) }));
    if started(&outcome) && ours.is_some() {
        doc.tree.rename = None;
        doc.touch();
    }
    outcome
}

/// `lock` and `visible`: one `set_locked` or `set_visible` on the nodes.
fn flags(cx: &mut Cx, call: &mut Call, args: &TreeArgs) -> Outcome {
    let ids = targets(args, cx);
    let doc = &mut *cx.doc;
    doc.tree.menu = None;
    if ids.is_empty() {
        return Outcome::Done(Err("nothing is selected".into()));
    }
    if let Some(missing) = ids.iter().find(|id| !doc.has_node(id)) {
        return Outcome::Done(Err(format!("no node {missing} in the shown tree")));
    }
    let who = names(doc, &ids);
    if args.op == TreeOp::Lock {
        let Some(locked) = args.locked else { return Outcome::Done(Err("lock needs locked (true or false)".into())) };
        let verb = if locked { "Lock" } else { "Unlock" };
        let message = format!("{verb}ed {who}");
        return edit_at(doc, call, args.revision, verb.to_string(), move |c| c.set_locked(&ids, locked).map(|r| EditDone { message, result: value(&r) }));
    }
    let Some(visible) = args.visible else { return Outcome::Done(Err("visible needs visible (true or false)".into())) };
    let (verb, done) = if visible { ("Show", "Shown") } else { ("Hide", "Hidden") };
    let message = format!("{done}: {who}");
    edit_at(doc, call, args.revision, verb.to_string(), move |c| c.op("set_visible", &[json!(ids), json!(visible)], &Map::new()).map(|r| EditDone { message, result: value(&r) }))
}

/// `group`: the dialog's OK, one `Ops.group(ids, name)` (RoboCAD's `_group`).
fn group(cx: &mut Cx, call: &mut Call, args: &TreeArgs) -> Outcome {
    let ids = args.ids.clone().or_else(|| cx.doc.tree.dialog.as_ref().map(|d| d.ids.clone())).unwrap_or_else(|| cx.shared.items().nodes());
    let doc = &mut *cx.doc;
    let name = args.name.as_deref().unwrap_or("").trim().to_string();
    let refused = if name.is_empty() {
        Some("A group name is needed: type one, or Cancel".to_string())
    } else {
        ids.iter().find(|id| !doc.has_node(id)).map(|missing| format!("no node {missing} in the shown tree"))
    };
    let began = args.revision.or(doc.tree.dialog.as_ref().map(|d| d.began));
    let outcome = match refused {
        Some(why) => Outcome::Done(Err(why)),
        None => {
            let message = if ids.is_empty() { format!("Added the empty group {name}") } else { format!("Grouped {} as {name}", names(doc, &ids)) };
            let label = format!("Group {name}");
            edit_at(doc, call, began, label, move |c| c.group(&ids, &name).map(|r| EditDone { message, result: value(&r) }))
        }
    };
    // The dialog closes once the edit is sent; a refusal stays in it.
    if doc.tree.dialog.is_some() {
        match &outcome {
            Outcome::Done(Err(e)) => {
                if let Some(dialog) = doc.tree.dialog.as_mut() {
                    dialog.error = Some(e.clone());
                }
            }
            _ => doc.tree.dialog = None,
        }
        doc.touch();
    }
    outcome
}

/// `menu`: open on a row (selecting it first when it is not selected, as
/// RoboCAD's `_menu`) or close.
fn menu(cx: &mut Cx, call: &mut Call, args: &TreeArgs) -> Outcome {
    if !args.open.unwrap_or(true) {
        if cx.doc.tree.menu.take().is_some() {
            cx.doc.touch();
        }
        return Outcome::Done(Ok(json!({"menu": null})));
    }
    let id = match &args.id {
        None => None,
        Some(id) => match known(cx.doc, Some(id.as_str()), "menu") {
            Ok(id) => Some(id),
            Err(e) => return Outcome::Done(Err(e)),
        },
    };
    if let Some(id) = &id
        && !cx.shared.items().iter().any(|i| i.0 == *id)
    {
        let select = CadAction::CadSelect { ids: vec![id.clone()], items: Vec::new(), extend: false, toggle: false, picked_at: None };
        let outcome = crate::cad::selection::handle(&select, call, cx);
        if !started(&outcome) {
            return outcome;
        }
        cx.doc.tree.anchor = Some(id.clone());
    }
    // The nodes its entries act on, frozen now (as RoboCAD's lambdas capture `ids`).
    let ids = cx.shared.items().nodes();
    let doc = &mut *cx.doc;
    doc.tree.menu = Some(MenuState { id: id.clone(), ids, at: args.at, began: doc.shown_revision() });
    doc.tree.rename = None;
    doc.touch();
    Outcome::Done(Ok(json!({"menu": {"id": id}, "message": "the outliner's context menu is open; its entries are the cad:tree controls"})))
}

/// The outliner's state (`state.tree` in the CAD snapshot).
pub(in crate::cad) fn state_json(doc: &CadDocument) -> Value {
    let t = &doc.tree;
    let active = doc.doc.as_ref().and_then(|d| d.active_group.clone());
    let rows: Vec<Value> = shown(doc, &[]).iter().map(|s| json!({"id": s.row.id, "depth": s.row.depth, "children": s.children, "expanded": s.open, "group": s.group})).collect();
    let groups: Vec<Value> = doc.doc.as_ref().map(|d| group_paths(d, &[])).unwrap_or_default().into_iter().map(|(id, path)| json!({"id": id, "path": path})).collect();
    json!({
        "search": t.search,
        "searching": t.searching(),
        "collapsed": t.collapsed,
        "anchor": t.anchor,
        "active_group": active.as_ref().map(|id| json!({"id": id, "name": doc.node_name(id)})),
        "rows": rows,
        "groups": groups,
        "rename": t.rename.as_ref().map(|r| json!({"id": r.id, "text": r.draft, "revision": r.began})),
        "menu": t.menu.as_ref().map(|m| json!({"id": m.id, "ids": m.ids, "at": m.at, "revision": m.began})),
        "dialog": t.dialog.as_ref().map(|d| json!({"title": "Organize components", "ids": d.ids, "name": d.draft, "error": d.error, "revision": d.began})),
        "drag": t.drag.as_ref().map(|d| json!({"ids": d.ids, "target": d.target.as_ref().map(target_json), "revision": d.began})),
        "index_of": doc.doc.as_ref().map(|d| d.nodes.iter().filter_map(|n| index_of(d, &n.id).map(|i| (n.id.clone(), json!(i)))).collect::<Map<String, Value>>()),
    })
}

/// A drop target as `move` takes it.
fn target_json(target: &DropTarget) -> Value {
    match target {
        DropTarget::Into(id) => json!({"parent": id}),
        DropTarget::Before(id) => json!({"before": id}),
        DropTarget::TopLevel => json!({"top_level": true}),
    }
}

/// The outliner's REST command (`cad_tree`).
pub(in crate::cad) fn specs() -> Vec<Spec> {
    vec![crate::app::actions::spec(
        "cad_tree",
        crate::cad::actions::CAD,
        json!({"op": "move", "ids": ["b1"], "parent": "g1"}),
        "CAD mode: the model tree's outliner (RoboCAD's OutlinerPanel). op: state (cad_state.tree: search, collapsed, the shown rows with depth and expansion, the active group, groups with their \"A / B\" paths, the rename row, the context menu, the Organize components dialog, a drag in progress); search (text: the rows whose names contain it, case-insensitive, with their ancestors and descendants, all expanded; the collapse state is kept and not changed while searching; empty text clears it); expand, collapse, toggle (id: a row with children; refused while searching); expand_all, collapse_all (refused while searching); select (id, a row press: alone, extend true as Shift selects the shown rows from the anchor (the last row pressed without Shift) to id, toggle true as Ctrl/Cmd adds or removes it; the same cad_select a row press writes); begin_rename (id: the inline name field, as a double-click); rename (id, text: stripped; an unchanged name sends nothing, an empty one is refused; one PATCH /nodes/{id} {name}); end_rename; move (ids, the selection when absent; parent: into that group; before: in front of that node, under its parent at its index, as RoboCAD's drop; neither: the top level; refused by name for a target that is not a group or a group into itself or its descendants; one Ops.move_nodes); lock (locked true | false on ids or the selection; one Ops.set_locked); visible (visible true | false; one Ops.set_visible); group (name, required; ids, else the open dialog's nodes, else the selection; [] makes an empty group as New group; one Ops.group); set_active (id: a group; absent clears the active group; one Ops.set_active_group); menu (open true with id: the row's context menu, selecting it first when it is not selected; open false closes it); group_dialog (open true: the Organize components dialog with \"Group name:\" for ids, the selection when absent, [] for New group; open false closes it). revision (rename, move, lock, visible, group, set_active): the shown revision the row or menu was read at; refused by name when RoboCAD's document changed since. Every edit is refused by name, with nothing sent, while another edit is in flight or when not connected. system_ui lists cad:tree:*.",
    )]
}
