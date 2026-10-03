//! The outliner's controls (`cad:tree:<id>`, what `system_ui` lists and
//! the tools part's buttons take) and the context menu's entries
//! (RoboCAD's `_context_menu`, widgets.py:403-437), each with its action
//! and why it is disabled now.
use super::handle::{TreeArgs, TreeOp};
use super::state::{group_paths, with_descendants};
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::CadDocument;
use crate::cad::panel::{edit_blocked, own_controls};
use crate::cad::selection::CadItems;
use crate::cad::surfaces::registry;
use sim_runtime::cad_client::SelectionItem;

/// One control: (id, label, action, ready).
pub(crate) type TreeControl = (String, String, CadAction, Result<(), String>);

fn ready(why: Option<String>) -> Result<(), String> {
    why.map_or(Ok(()), Err)
}

/// An edit on the selection: edits must be sendable and something selected.
fn on_selection(doc: &CadDocument, selected: &[String]) -> Result<(), String> {
    ready(edit_blocked(doc).or_else(|| selected.is_empty().then(|| "nothing is selected".to_string())))
}

fn searching(doc: &CadDocument) -> Result<(), String> {
    if doc.tree.searching() { Err("the rows are all expanded while a search is typed; clear the search first".into()) } else { Ok(()) }
}

/// `cad_tree` with `op` and the given fields.
fn args(op: TreeOp, f: impl FnOnce(&mut TreeArgs)) -> CadAction {
    let mut a = TreeArgs::of(op);
    f(&mut a);
    a.action()
}

/// Every outliner control now (`selection`: the shared selection's CAD items).
pub(crate) fn controls_of(doc: &CadDocument, selection: &[SelectionItem]) -> Vec<TreeControl> {
    let selected = selection.nodes();
    let blocked = edit_blocked(doc);
    let mut out: Vec<TreeControl> = Vec::new();
    let mut add = |id: &str, label: String, action: CadAction, ready: Result<(), String>| out.push((format!("cad:tree:{id}"), label, action, ready));
    add("new_group", "New group".into(), args(TreeOp::GroupDialog, |a| (a.open, a.ids) = (Some(true), Some(Vec::new()))), ready(blocked.clone()));
    add("expand_all", "Expand all".into(), TreeArgs::of(TreeOp::ExpandAll).action(), searching(doc));
    add("collapse_all", "Collapse all".into(), TreeArgs::of(TreeOp::CollapseAll).action(), searching(doc));
    add("lock", "Lock".into(), args(TreeOp::Lock, |a| a.locked = Some(true)), on_selection(doc, &selected));
    add("unlock", "Unlock".into(), args(TreeOp::Lock, |a| a.locked = Some(false)), on_selection(doc, &selected));
    add("hide", "Hide".into(), args(TreeOp::Visible, |a| a.visible = Some(false)), on_selection(doc, &selected));
    add("show", "Show".into(), args(TreeOp::Visible, |a| a.visible = Some(true)), on_selection(doc, &selected));
    add("group_selection", "Group selection…".into(), args(TreeOp::GroupDialog, |a| a.open = Some(true)), on_selection(doc, &selected));
    add("move_top", "Move to top level".into(), TreeArgs::of(TreeOp::Move).action(), on_selection(doc, &selected));
    add("clear_active", "Clear active group".into(), TreeArgs::of(TreeOp::SetActive).action(), ready(blocked.clone()));
    if let Some(state) = &doc.doc {
        let moving = with_descendants(state, &selected);
        for (group, path) in group_paths(state, &[]) {
            let into = if moving.contains(&group) { Err(format!("Cannot move a group into itself or its descendants ({path})")) } else { on_selection(doc, &selected) };
            add(&format!("move-{group}"), format!("Move to group {path}"), args(TreeOp::Move, |a| a.parent = Some(group.clone())), into);
            add(&format!("set_active-{group}"), format!("Set {path} as active group"), TreeArgs::on(TreeOp::SetActive, &group).action(), ready(blocked.clone()));
        }
        if !doc.tree.searching() {
            for parent in super::state::parents_of_children(state) {
                let label = format!("{} {}", if doc.tree.collapsed.contains(&parent) { "Expand" } else { "Collapse" }, doc.node_name(&parent));
                add(&format!("toggle-{parent}"), label, TreeArgs::on(TreeOp::Toggle, &parent).action(), Ok(()));
            }
        }
    }
    if let Some(d) = &doc.tree.dialog {
        let ok = ready(blocked.clone().or_else(|| d.draft.trim().is_empty().then(|| "type a group name first".to_string())));
        add("dialog_ok", "OK".into(), args(TreeOp::Group, |a| (a.name, a.revision) = (Some(d.draft.clone()), Some(d.began))), ok);
        add("dialog_cancel", "Cancel".into(), args(TreeOp::GroupDialog, |a| a.open = Some(false)), Ok(()));
    }
    if doc.tree.menu.is_some() {
        add("menu_close", "Close the outliner menu".into(), args(TreeOp::Menu, |a| a.open = Some(false)), Ok(()));
    }
    out
}

/// `system_ui`'s `cad:tree:*` controls.
pub(in crate::cad) fn controls(cx: &Cx) -> Vec<TreeControl> {
    controls_of(cx.doc, &cx.shared.items())
}

/// One row of the context menu: an entry, or a heading over the entries after it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MenuRow {
    Entry { label: String, action: CadAction, ready: Result<(), String>, indent: bool },
    Heading(String),
    Separator,
}

/// A catalogue or registry command as a menu entry (`CadInvoke`, enabled by `registry::ready`).
fn command(id: &str, label: &str, doc: &CadDocument, selection: &[SelectionItem], own: &[crate::cad::panel::Control]) -> Option<MenuRow> {
    let cmd = registry::command(id)?;
    let ready = registry::ready(cmd, doc, selection, own).map_err(|why| registry::status_line(cmd, &why));
    Some(MenuRow::Entry { label: label.to_string(), action: CadAction::CadInvoke { id: id.to_string() }, ready, indent: false })
}

/// The context menu's rows (RoboCAD's `_context_menu`, in its order): with
/// a selection, Fit in view, Isolate, Hide, Show, Lock, Unlock, Group
/// selection…, Move to group (Top level and every group's path, without
/// the moving nodes and their descendants), Make unique (bake instance),
/// Set as active group (one group selected), Delete; always Clear active
/// group and Show all. The outliner's own entries carry the selected ids
/// and the revision the menu opened at, as RoboCAD's lambdas capture `ids`.
pub(crate) fn menu_rows(doc: &CadDocument, selection: &[SelectionItem]) -> Vec<MenuRow> {
    let Some(menu) = &doc.tree.menu else { return Vec::new() };
    let ids = menu.ids.clone();
    let own = own_controls(doc, selection);
    let blocked = edit_blocked(doc);
    let edit = || ready(blocked.clone());
    let with = |op: TreeOp, f: &dyn Fn(&mut TreeArgs)| {
        let mut a = TreeArgs::of(op);
        a.ids = Some(ids.clone());
        a.revision = Some(menu.began);
        f(&mut a);
        a.action()
    };
    let entry = |label: &str, action: CadAction, ready: Result<(), String>| MenuRow::Entry { label: label.to_string(), action, ready, indent: false };
    let mut rows = Vec::new();
    if !ids.is_empty() {
        rows.extend(command("view.focus", "Fit in view", doc, selection, &own));
        rows.push(MenuRow::Separator);
        rows.extend(command("view.isolate", "Isolate", doc, selection, &own));
        rows.push(entry("Hide", with(TreeOp::Visible, &|a| a.visible = Some(false)), edit()));
        rows.push(entry("Show", with(TreeOp::Visible, &|a| a.visible = Some(true)), edit()));
        rows.push(entry("Lock", with(TreeOp::Lock, &|a| a.locked = Some(true)), edit()));
        rows.push(entry("Unlock", with(TreeOp::Lock, &|a| a.locked = Some(false)), edit()));
        rows.push(entry("Group selection…", with(TreeOp::GroupDialog, &|a| a.open = Some(true)), edit()));
        rows.push(MenuRow::Heading("Move to group".into()));
        let mut top = TreeArgs::of(TreeOp::Move);
        (top.ids, top.revision) = (Some(ids.clone()), Some(menu.began));
        rows.push(MenuRow::Entry { label: "Top level".into(), action: top.action(), ready: edit(), indent: true });
        if let Some(state) = &doc.doc {
            for (group, path) in group_paths(state, &ids) {
                let mut a = TreeArgs::of(TreeOp::Move);
                (a.ids, a.parent, a.revision) = (Some(ids.clone()), Some(group), Some(menu.began));
                rows.push(MenuRow::Entry { label: path, action: a.action(), ready: edit(), indent: true });
            }
        }
        rows.push(MenuRow::Separator);
        rows.extend(command("modify.make_unique", "Make unique (bake instance)", doc, selection, &own));
        if let [only] = ids.as_slice()
            && doc.doc.as_ref().is_some_and(|d| d.nodes.iter().any(|n| n.id == *only && n.kind == "group"))
        {
            let mut a = TreeArgs::on(TreeOp::SetActive, only);
            a.revision = Some(menu.began);
            rows.push(entry("Set as active group", a.action(), edit()));
        }
        rows.extend(command("edit.delete", "Delete", doc, selection, &own));
        rows.push(MenuRow::Separator);
    }
    let mut clear = TreeArgs::of(TreeOp::SetActive);
    clear.revision = Some(menu.began);
    rows.push(entry("Clear active group", clear.action(), edit()));
    rows.extend(command("view.show_all", "Show all", doc, selection, &own));
    rows
}
