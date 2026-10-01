//! CAD mode's model tree (left dock): one row per node of RoboCAD's
//! document in its walk order, indented by depth. A row press writes
//! `CadSelect {ids: [id]}` (the node as a body item, `[id, "body", 0]`,
//! what a 3D pick in body mode writes; `cad:node:<id>` is the same action);
//! a row is lit when any item of its node is selected (a face too);
//! its visibility chip writes `CadPatch {id, attrs: {"visible": !visible}}`,
//! RoboCAD's own flag (the chip reads "Disabled" when the node itself is
//! disabled, and "Hidden by parent" when it is visible and enabled itself
//! but an ancestor hides or disables it). Rows are compact single lines
//! (kind tag, name, a "locked" flag, the chip): the kit's `list_item` is a
//! two-line row with a 28 px component icon, which CAD node kinds have none
//! of, so the rows are built like robot mode's link rows (`Button` +
//! `Tint::selectable` + `AccessibleLabel`). A selection change restyles the
//! rows in place ([`highlight`]); the tree is rebuilt only when its rows
//! change.
use super::actions::CadAction;
use super::document::{CadDocument, Connection};
use super::panel::{CadButton, edit_blocked, patch};
use crate::builder::ui_api::Enabled;
use crate::ui_kit::{ACCENT, ACCENT_BG, DANGER, FAINT, Kit, SUBTLE, TEXT, Tint, WARN, size};
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
use serde_json::Value;
use std::fmt::Write;

/// Indent per tree level (px).
const INDENT: f32 = 14.0;

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
    doc.rows().is_empty().then(|| ("The document has no nodes.".to_string(), SUBTLE))
}

/// A tree row: its node id and its accessible label without the selection.
#[derive(Component)]
pub(super) struct TreeRowId {
    id: String,
    label: String,
}

/// A row's visibility chip (enabled while edits can be sent).
#[derive(Component)]
pub(super) struct EyeChip;

/// What the tree shows now (rows and their states), not the selection or
/// whether edits can be sent: [`highlight`] restyles those in place, so a
/// click or an edit does not respawn a large tree.
pub(super) fn key(doc: &CadDocument) -> String {
    let mut key = format!("{:?}\n", empty_state(doc).map(|s| s.0));
    for r in doc.rows() {
        let _ = writeln!(key, "{}|{}|{}|{}|{}|{}|{}|{}", r.id, r.depth, r.kind, r.name, r.effective_visible, r.visible, r.locked, r.disabled);
    }
    key
}

fn row_label(base: &str, selected: bool) -> String {
    if selected { format!("{base}, selected") } else { base.to_string() }
}

/// Restyle the spawned rows for the current selection (fill, left edge,
/// accessible label) and enable their visibility chips while edits can be
/// sent. Rows spawned this frame are drawn that way already.
pub(super) fn highlight(commands: &mut Commands, doc: &CadDocument, rows: &mut Query<(Entity, &TreeRowId, &mut Tint, &mut BorderColor, &AccessibleLabel)>, eyes: &mut Query<&mut Enabled, With<EyeChip>>) {
    let editable = Enabled(edit_blocked(doc).is_none());
    for mut enabled in eyes.iter_mut() {
        enabled.set_if_neq(editable);
    }
    for (entity, row, mut tint, mut edge, label) in rows.iter_mut() {
        let selected = doc.selection.iter().any(|s| s.0 == row.id);
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

/// The rows.
pub(super) fn build(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument) {
    if let Some((text, color)) = empty_state(doc) {
        p.spawn((k.text(text, size::SMALL, color, 0), Node { margin: UiRect::all(Val::Px(6.0)), ..default() }));
        return;
    }
    let editable = edit_blocked(doc).is_none();
    for row in doc.rows() {
        let mut flags = vec![row.kind.clone()];
        if row.locked {
            flags.push("locked".to_string());
        }
        if row.disabled || !row.effective_visible {
            flags.push(eye(row.visible, row.disabled, row.effective_visible).to_lowercase());
        }
        let label = format!("{}, {}", row.name, flags.join(", "));
        p.spawn((
            Button,
            CadButton(CadAction::CadSelect { ids: vec![row.id.clone()], items: Vec::new(), extend: false, toggle: false }),
            Tint::selectable(row.selected),
            AccessibleLabel::new(row_label(&label, row.selected)),
            TreeRowId { id: row.id.clone(), label },
            Node {
                margin: UiRect::left(Val::Px(row.depth as f32 * INDENT)),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                column_gap: Val::Px(6.0),
                align_items: AlignItems::Center,
                border: UiRect::left(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(if row.selected { ACCENT } else { Color::NONE }),
            BackgroundColor(if row.selected { ACCENT_BG } else { Color::NONE }),
        ))
        .with_children(|r| {
            r.spawn((k.text(row.kind.clone(), size::DETAIL, FAINT, 1), Node { min_width: Val::Px(46.0), flex_shrink: 0.0, ..default() }));
            r.spawn((k.text(row.name.clone(), size::BODY, if row.effective_visible { TEXT } else { SUBTLE }, 1), Node { flex_grow: 1.0, flex_shrink: 1.0, min_width: Val::Px(0.0), ..default() }));
            if row.locked {
                r.spawn(k.text("locked", size::DETAIL, WARN, 0));
            }
            r.spawn((k.chip(eye(row.visible, row.disabled, row.effective_visible), CadButton(patch(&row.id, "visible", Value::Bool(!row.visible))), row.effective_visible, editable), EyeChip));
        });
    }
}
