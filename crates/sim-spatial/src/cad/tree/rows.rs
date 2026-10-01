//! The outliner's drawing: the rows (`Part::Tree`: [`key`], [`draw`],
//! restyled in place by [`highlight`]) and the tools above them
//! (`Part::TreeTools`: [`tools_key`], [`tools`]). Both parts are rebuilt
//! only when their key changes; the key holds everything they show.
use super::controls::controls_of;
use super::state::{DropTarget, Shown, shown};
use crate::cad::actions::CadAction;
use crate::cad::document::{CadDocument, Connection};
use crate::cad::panel::{CadButton, edit_blocked, patch};
use crate::builder::ui_api::Enabled;
use crate::ui_kit::{ACCENT, ACCENT_BG, DANGER, FAINT, Kit, Look, SUBTLE, TEXT, Tint, WARN, size, wrap};
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
use serde_json::Value;
use sim_runtime::cad_client::SelectionItem;
use std::fmt::Write;

/// Indent per tree level (px).
const INDENT: f32 = 14.0;
/// The room a disclosure chip takes ("+" or "−" with the chip's padding
/// and border): rows without children keep it, so names line up.
const DISCLOSURE: f32 = 26.0;
/// RoboCAD's active-group colour, `QColor(120, 200, 255)` (widgets.py:300).
/// The kit has no blue token; this is RoboCAD's own value.
pub(crate) const ACTIVE_GROUP: Color = Color::srgb(0.471, 0.784, 1.0);

/// A tree row: its node id, its accessible label without the selection,
/// and whether it is a group (a drop into it is possible).
#[derive(Component)]
pub(in crate::cad) struct TreeRowId {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) group: bool,
}

/// A row's visibility chip (enabled while edits can be sent).
#[derive(Component)]
pub(in crate::cad) struct EyeChip;

/// The empty space under the rows: a drop there moves to the top level
/// (RoboCAD's drop on no item); a right press opens the menu for the selection.
#[derive(Component)]
pub(in crate::cad) struct TreeEnd;

/// A field of the outliner: a press gives it the keyboard.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cad) enum TreeField {
    Search,
    Rename,
}

/// What the tree shows when it has no rows: the reason, truthfully.
fn empty_state(doc: &CadDocument) -> Option<(String, Color)> {
    if doc.doc.is_none() {
        return Some(match &doc.connection {
            Connection::Connecting { what, .. } => (format!("Connecting: {what}… The tree appears once RoboCAD answers."), SUBTLE),
            Connection::Lost { error, .. } => (format!("No document from RoboCAD: {error}"), DANGER),
            // A failed first `/doc` (not a refetch under way): its error.
            Connection::Connected => match doc.stale.as_deref().filter(|s| !s.starts_with("refetching revision")) {
                Some(error) => (format!("No document from RoboCAD: {error}"), DANGER),
                None => ("Loading the document from RoboCAD…".to_string(), SUBTLE),
            },
        });
    }
    doc.rows(&[]).is_empty().then(|| ("The document has no nodes.".to_string(), SUBTLE))
}

/// The drop marker a row shows during a drag: into it, or a line above it.
fn marker(doc: &CadDocument, id: &str) -> Option<bool> {
    match doc.tree.drag.as_ref()?.target.as_ref()? {
        DropTarget::Into(t) if t == id => Some(true),
        DropTarget::Before(t) if t == id => Some(false),
        _ => None,
    }
}

/// What the tree shows now (the shown rows and their states, the search,
/// the active group, the rename row and its draft, the drop marker), not
/// the selection or whether edits can be sent: [`highlight`] restyles
/// those in place, so a click or an edit does not respawn a large tree.
pub(in crate::cad) fn key(doc: &CadDocument) -> String {
    let t = &doc.tree;
    let mut key = format!("{:?}\n{:?}\n{:?}\n", empty_state(doc).map(|s| s.0), t.query(), t.rename);
    if let Some(drag) = &t.drag {
        let _ = writeln!(key, "{:?}", drag.target);
    }
    for s in shown(doc, &[]) {
        let r = &s.row;
        let _ = writeln!(key, "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", r.id, r.depth, r.kind, r.name, r.effective_visible, r.visible, r.locked, r.disabled, s.children, s.open, s.active);
    }
    key
}

fn row_label(base: &str, selected: bool) -> String {
    if selected { format!("{base}, selected") } else { base.to_string() }
}

/// Restyle the spawned rows for the current selection (fill, left edge,
/// accessible label) and enable their visibility chips while edits can be
/// sent. Rows spawned this frame are drawn that way already.
pub(in crate::cad) fn highlight(commands: &mut Commands, doc: &CadDocument, selection: &[SelectionItem], rows: &mut Query<(Entity, &TreeRowId, &mut Tint, &mut BorderColor, &AccessibleLabel)>, eyes: &mut Query<&mut Enabled, With<EyeChip>>) {
    let editable = Enabled(edit_blocked(doc).is_none());
    for mut enabled in eyes.iter_mut() {
        enabled.set_if_neq(editable);
    }
    for (entity, row, mut tint, mut edge, label) in rows.iter_mut() {
        let selected = selection.iter().any(|s| s.0 == row.id);
        tint.set_if_neq(Tint::selectable(selected));
        edge.set_if_neq(BorderColor::all(if selected { ACCENT } else { Color::NONE }));
        let text = row_label(&row.label, selected);
        if label.0 != text {
            // `AccessibleLabel` is immutable: replace it.
            commands.entity(entity).insert(AccessibleLabel::new(text));
        }
    }
}

/// The visibility chip's label: the node's effective visibility in RoboCAD
/// (`effective_visible` is false for a disabled node or one under a hidden
/// or disabled ancestor).
fn eye(visible: bool, disabled: bool, effective: bool) -> &'static str {
    match (disabled, effective, visible) {
        (true, ..) => "Disabled",
        (false, true, _) => "Shown",
        (false, false, true) => "Hidden by parent",
        (false, false, false) => "Hidden",
    }
}

/// The name's colour: grey when not effectively visible (RoboCAD's
/// override comes last), RoboCAD's blue for the active group, else text.
fn name_colour(s: &Shown) -> Color {
    if !s.row.effective_visible {
        SUBTLE
    } else if s.active {
        ACTIVE_GROUP
    } else {
        TEXT
    }
}

/// The rows (`selection`: the shared selection's CAD items).
pub(in crate::cad) fn draw(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, selection: &[SelectionItem]) {
    if let Some((text, color)) = empty_state(doc) {
        p.spawn((k.text(text, size::SMALL, color, 0), Node { margin: UiRect::all(Val::Px(6.0)), ..default() }));
        return;
    }
    let editable = edit_blocked(doc).is_none();
    let rows = shown(doc, selection);
    if rows.is_empty() {
        p.spawn((k.text("No row matches the search.", size::SMALL, SUBTLE, 0), Node { margin: UiRect::all(Val::Px(6.0)), ..default() }));
    }
    let searching = doc.tree.searching();
    for s in &rows {
        let row = &s.row;
        let mut flags = vec![row.kind.clone()];
        if s.active {
            flags.push("active group".to_string());
        }
        if row.locked {
            flags.push("locked".to_string());
        }
        if row.disabled || !row.effective_visible {
            flags.push(eye(row.visible, row.disabled, row.effective_visible).to_lowercase());
        }
        if s.children && !searching {
            flags.push(if s.open { "expanded" } else { "collapsed" }.to_string());
        }
        let label = format!("{}, {}", row.name, flags.join(", "));
        let renaming = doc.tree.rename.as_ref().filter(|r| r.id == row.id);
        let mark = marker(doc, &row.id);
        let mut entity = p.spawn((
            Button,
            Tint::selectable(row.selected),
            AccessibleLabel::new(row_label(&label, row.selected)),
            TreeRowId { id: row.id.clone(), label, group: s.group },
            Node {
                margin: UiRect::left(Val::Px(row.depth as f32 * INDENT)),
                padding: UiRect::axes(Val::Px(4.0), Val::Px(3.0)),
                column_gap: Val::Px(6.0),
                align_items: AlignItems::Center,
                border: UiRect::left(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(if row.selected { ACCENT } else { Color::NONE }),
            BackgroundColor(if row.selected { ACCENT_BG } else { Color::NONE }),
        ));
        // A drop into a group: an outline (no layout change, so the row under the pointer stays).
        if mark == Some(true) {
            entity.insert(Outline::new(Val::Px(1.0), Val::Px(0.0), ACCENT));
        }
        entity.with_children(|r| {
            // A drop in front of the row: a line over its top edge, out of the layout.
            if mark == Some(false) {
                r.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), top: Val::Px(-1.0), height: Val::Px(2.0), ..default() }, BackgroundColor(ACCENT), Pickable::IGNORE));
            }
            if s.children && !searching {
                let (text, verb) = if s.open { ("−", "Collapse") } else { ("+", "Expand") };
                // The kit chip as drawn (its look sets the padding); the label names the row.
                r.spawn(k.chip(text, CadButton(super::TreeArgs::on(super::TreeOp::Toggle, &row.id).action()), false, true)).insert(AccessibleLabel::new(format!("{verb} {}", row.name)));
            } else {
                r.spawn((Node { width: Val::Px(DISCLOSURE), flex_shrink: 0.0, ..default() }, Pickable::IGNORE));
            }
            r.spawn((k.text(row.kind.clone(), size::DETAIL, FAINT, 1), Node { min_width: Val::Px(46.0), flex_shrink: 0.0, ..default() }, Pickable::IGNORE));
            match renaming {
                Some(rename) => {
                    // The kit field, stretched across the name's room (a replaced `Node`: kit
                    // inputs carry no `Look`, so nothing restores the kit's own layout).
                    r.spawn(k.input_selectable(&rename.draft, &row.name, TreeField::Rename, true, rename.selected)).insert((
                        Node { flex_grow: 1.0, flex_shrink: 1.0, min_width: Val::Px(0.0), padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(5.0)), ..default() },
                        AccessibleLabel::new(format!("New name for {}", row.name)),
                    ));
                }
                None => {
                    r.spawn((k.text(row.name.clone(), size::BODY, name_colour(s), 1), Node { flex_grow: 1.0, flex_shrink: 1.0, min_width: Val::Px(0.0), ..default() }, Pickable::IGNORE));
                }
            }
            if row.locked {
                r.spawn((k.text("locked", size::DETAIL, WARN, 0), Pickable::IGNORE));
            }
            r.spawn((k.chip(eye(row.visible, row.disabled, row.effective_visible), CadButton(patch(&row.id, "visible", Value::Bool(!row.visible))), row.effective_visible, editable), EyeChip));
        });
    }
    // The room under the rows: a drop target for the top level.
    let line = doc.tree.drag.as_ref().is_some_and(|d| d.target == Some(DropTarget::TopLevel));
    p.spawn((
        Node { min_height: Val::Px(28.0), flex_grow: 1.0, flex_shrink: 0.0, border: UiRect::top(Val::Px(2.0)), ..default() },
        BorderColor::all(if line { ACCENT } else { Color::NONE }),
        TreeEnd,
        AccessibleLabel::new("End of the model tree (drop here for the top level)"),
    ));
    if doc.tree.drag.is_none() && !rows.is_empty() {
        p.spawn((k.caption("Drag rows onto a group to move them into it, or in front of a row to reorder. Double-click a name to rename it; right-click for more."), Pickable::IGNORE));
    }
}

/// What the tools part shows now.
pub(in crate::cad) fn tools_key(doc: &CadDocument, selection: &[SelectionItem]) -> String {
    let ready: Vec<(String, bool)> = controls_of(doc, selection).into_iter().filter(|c| TOOLS.contains(&c.0.as_str())).map(|c| (c.0, c.3.is_ok())).collect();
    let active = doc.doc.as_ref().and_then(|d| d.active_group.as_ref()).map(|id| doc.node_name(id));
    format!("{:?}", (&doc.tree.search, doc.tree.search_focused, ready, active, doc.doc.is_some()))
}

/// The tools part's buttons, in order.
const TOOLS: [&str; 3] = ["cad:tree:new_group", "cad:tree:expand_all", "cad:tree:collapse_all"];

/// The outliner's tools (RoboCAD's search field and its New group, Expand
/// all and Collapse all buttons), and the active group's name.
pub(in crate::cad) fn tools(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, selection: &[SelectionItem]) {
    if doc.doc.is_none() {
        return;
    }
    let t = &doc.tree;
    p.spawn(k.input(&t.search, "Search (Ctrl+F)…", TreeField::Search, t.search_focused)).insert(AccessibleLabel::new("Search the model tree"));
    let controls = controls_of(doc, selection);
    p.spawn(wrap()).with_children(|r| {
        for id in TOOLS {
            if let Some((_, label, action, ready)) = controls.iter().find(|c| c.0 == id) {
                r.spawn(k.button(label, CadButton(action.clone()), Look::Secondary, ready.is_ok()));
            }
        }
    });
    if let Some(id) = doc.doc.as_ref().and_then(|d| d.active_group.as_ref()) {
        p.spawn(k.text(format!("Active group: {}", doc.node_name(id)), size::CAPTION, ACTIVE_GROUP, 1));
    }
}

/// The action a press on the empty room under the rows writes with the
/// right button: the menu for the selection (no row).
pub(in crate::cad) fn end_menu(at: Vec2) -> CadAction {
    let mut a = super::TreeArgs::of(super::TreeOp::Menu);
    (a.open, a.at) = (Some(true), Some([at.x, at.y]));
    a.action()
}
