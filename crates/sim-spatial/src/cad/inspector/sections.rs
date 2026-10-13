//! The inspector's lower sections: the physical link holding the inspected
//! body (the in-process physical model at the shown revision) with the
//! physical rows under it ([`super::rows`]), the node's flags, material
//! and Delete, the edit history and the GUI command registry.
use super::{current_revision, field, inline, lines, node, provenance};
use crate::cad::document::CadDocument;
use crate::cad::panel::{CadButton, Control, control, controls, edit_blocked, material};
use crate::cad::selection::CadItems;
use crate::ui_kit::{DANGER, Kit, Look, SUBTLE, TEXT, size, wrap};
use bevy::prelude::*;
use serde_json::Value;
use crate::cad::types::SelectionItem;

pub(in crate::cad) fn physical_key(doc: &CadDocument, edit: &super::PhysicalEdit, selection: &[SelectionItem]) -> String {
    // Not the model itself (collision meshes make it large): its revision,
    // whether it failed, and the selection pick the shown link.
    let physical = doc.physical.as_ref().map(|(r, result)| (*r, result.as_ref().err()));
    // The physical rows under it have their own key (`rows::key`).
    format!("{:?} {}", (selection.first_node(), physical, current_revision(doc), doc.physical_job.is_some()), super::rows::key(doc, edit, selection))
}

/// The physical link holding the inspected body, then the physical rows
/// (`rows`: colour, joint, joint physics, results, exact measurements,
/// material properties).
pub(in crate::cad) fn physical(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, edit: &super::PhysicalEdit, selection: &[SelectionItem]) {
    if let Some(local) = &doc.local {
        for (label, mass) in [("Visible assembly (including instances)", &local.masses.assembly), ("Physical source assembly (body/sheet inventory)", &local.masses.physical_assembly)] {
            p.spawn(k.section(label));
            field(p, k, "Mass", &mass.mass_kg.to_string(), "kg");
            field(p, k, "Centroid", &super::numbers(&mass.centroid_m), "m");
            for (i, row) in mass.inertia_kg_m2.iter().enumerate() { field(p, k, &format!("Inertia row {}", i + 1), &super::numbers(row), "kg·m²"); }
            field(p, k, "Origin", &mass.origin, "");
            lines(p, k, "Provenance", &mass.provenance);
        }
        field(p, k, "Model identity", &local.masses.model_identity, "");
        field(p, k, "Derivation identity", &local.masses.derivation_identity, "");
        p.spawn(k.note("Exact B-rep properties are authoritative. Triangles are display only."));
    }
    link(p, k, doc, selection);
    super::rows::draw(p, k, doc, edit, selection);
}

/// The physical link holding the inspected body, from the physical model.
fn link(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, selection: &[SelectionItem]) {
    let Some(id) = selection.first_node() else { return };
    p.spawn(k.section("Physical link"));
    if doc.physical_job.is_some() {
        p.spawn(k.caption("Deriving the physical model…"));
        return;
    }
    let hint = "Press Physical in the toolbar to fetch RoboCAD's physical model for this revision (GET /physical; nothing is written).";
    let Some((revision, result)) = &doc.physical else {
        p.spawn(k.caption(hint));
        return;
    };
    if Some(*revision) != current_revision(doc) {
        p.spawn(k.caption(format!("The physical model shown was fetched at revision {revision}; the document has changed since. {hint}")));
        return;
    }
    let model = match result {
        Err(e) => {
            p.spawn(k.text(e.clone(), size::SMALL, DANGER, 0));
            return;
        }
        Ok(model) => model,
    };
    let link = model.get("links").and_then(Value::as_array).and_then(|links| links.iter().find(|l| l.get("members").and_then(Value::as_array).is_some_and(|m| m.iter().any(|m| m.as_str() == Some(id)))));
    let Some(link) = link else {
        p.spawn(k.caption("This node is not a member of any link in RoboCAD's physical model (links are made of bodies)."));
        return;
    };
    p.spawn(k.note("As RoboCAD's physical model gives it, in SI units (kg, m); the link merges every body fixed to it."));
    for (key, label, unit) in [("name", "Link", ""), ("id", "Link id", ""), ("material", "Material", ""), ("mass", "Mass", "kg"), ("com", "Centre of mass", "m"), ("inertia", "Inertia about the centre of mass", "kg·m²"), ("bbox", "Bounding box from the centre of mass", "m")] {
        if let Some(v) = link.get(key) {
            match inline(v, 0) {
                Some(text) => field(p, k, label, &text, unit),
                None => lines(p, k, label, v),
            }
        }
    }
    if let Some(members) = link.get("members").and_then(Value::as_array) {
        field(p, k, "Bodies in the link", &members.len().to_string(), "");
    }
    if let Some(source) = link.get("mass_sources").and_then(|s| s.get(id)) {
        match source.as_str() {
            Some(text) => provenance(p, k, &format!("mass_sources[{id}]"), text),
            None => lines(p, k, &format!("mass_sources[{id}]"), source),
        }
    }
}

pub(in crate::cad) fn attributes_key(doc: &CadDocument, selection: &[SelectionItem]) -> String {
    let sel = selection.first_node();
    let n = sel.and_then(|id| node(doc, id)).map(|n| (n.visible, n.locked, n.disabled, &n.material));
    format!("{:?}", (sel, n, doc.doc.as_ref().map(|d| &d.materials), edit_blocked(doc)))
}

/// A chip for control `id` (from `panel::controls`), if it is listed.
fn control_chip(p: &mut ChildSpawnerCommands, k: &Kit, all: &[Control], id: &str, label: &str, on: bool) {
    if let Some(c) = control(all, id) {
        p.spawn(k.chip(label, CadButton(c.action.clone()), on, c.ready.is_ok()));
    }
}

/// The node's flags, its material and Delete: each one `CadPatch`/`CadDelete`,
/// the action `panel::controls` lists for it.
pub(in crate::cad) fn attributes(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, selection: &[SelectionItem]) {
    let Some(id) = selection.first_node() else { return };
    let Some(n) = node(doc, id) else { return };
    p.spawn(k.section("Edit"));
    let blocked = edit_blocked(doc);
    p.spawn(k.caption(match &blocked {
        Some(why) => format!("Editing is unavailable: {why}."),
        None => "Each change is one step in RoboCAD's undo history.".to_string(),
    }));
    let all = controls(doc, selection);
    p.spawn(wrap()).with_children(|r| {
        for (key, label, on) in [("visible", "Visible", n.visible), ("locked", "Locked", n.locked), ("disabled", "Disabled", n.disabled)] {
            control_chip(r, k, &all, &format!("cad:{key}:{id}"), label, on);
        }
    });
    p.spawn(k.text("Material", size::BODY, TEXT, 1));
    let materials: Vec<(String, String)> = doc.doc.as_ref().map(|d| d.materials.iter().filter_map(material).collect()).unwrap_or_default();
    if materials.is_empty() {
        p.spawn(k.caption("The archive lists no materials."));
    } else {
        p.spawn(wrap()).with_children(|r| {
            for (material_id, label) in materials {
                let on = n.material.as_deref() == Some(material_id.as_str());
                control_chip(r, k, &all, &format!("cad:material:{id}:{material_id}"), &label, on);
            }
        });
    }
    p.spawn(Node { margin: UiRect::top(Val::Px(6.0)), ..wrap() }).with_children(|r| {
        if let Some(c) = control(&all, "cad:delete") {
            r.spawn(k.button("Delete", CadButton(c.action.clone()), Look::Danger, c.ready.is_ok()));
        }
    });
}

/// Which of the retained history blocks a text node shows.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryText {
    Undo,
    Redo,
}

/// One history block's text: the labels most recent first, or the empty note.
fn history_lines(doc: &CadDocument, which: HistoryText) -> String {
    let (labels, empty) = match which {
        HistoryText::Undo => (&doc.history.undo, "Nothing to undo."),
        HistoryText::Redo => (&doc.history.redo, "Nothing to redo."),
    };
    if labels.is_empty() {
        return empty.to_string();
    }
    labels.iter().rev().map(|(l, _)| l.as_str()).collect::<Vec<_>>().join("\n")
}

/// RoboCAD's undo and redo labels, most recent first (read-only). The
/// retained pilot part: spawned once per document source, its two text
/// blocks are updated in place by [`sync_history`] instead of rebuilt.
pub(in crate::cad) fn history(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument) {
    p.spawn(k.section("History"));
    for (title, which) in [("Undo (most recent first)", HistoryText::Undo), ("Redo (most recent first)", HistoryText::Redo)] {
        p.spawn(k.text(title, size::BODY, TEXT, 1));
        p.spawn((k.text(history_lines(doc, which), size::SMALL, SUBTLE, 0), Node { margin: UiRect::left(Val::Px(8.0)), ..default() }, which));
    }
}

/// Keep the retained history blocks current as edits land (the document's
/// change tick is the trigger; identical text is not reassigned).
pub(crate) fn sync_history(doc: Option<Res<CadDocument>>, mut texts: Query<(&mut Text, &HistoryText)>) {
    let Some(doc) = doc else { return };
    if !doc.is_changed() {
        return;
    }
    for (mut text, which) in &mut texts {
        let want = history_lines(&doc, *which);
        if text.0 != want {
            text.0 = want;
        }
    }
}

pub(in crate::cad) fn commands_key(doc: &CadDocument) -> String {
    format!("{:?}", (&doc.commands, doc.health.as_ref().map(|h| h.gui), edit_blocked(doc)))
}

/// The modelling commands: every operation runs in process (menus, palette, REST).
pub(in crate::cad) fn commands(p: &mut ChildSpawnerCommands, k: &Kit, _doc: &CadDocument, _selection: &[SelectionItem]) {
    p.spawn(k.section("Modelling commands"));
    p.spawn(k.caption("Every modelling operation runs in process, from the menus and the command palette (Ctrl+K)."));
}
