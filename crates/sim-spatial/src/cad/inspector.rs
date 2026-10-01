//! CAD mode's right dock: the inspected node (the first selected), the
//! physical link holding it, its editable attributes, RoboCAD's history and
//! its command registry. Everything is shown as RoboCAD returned it: no
//! value is computed, defaulted or filled in here. Nested JSON is rendered
//! generically ("a.b" property rows, arrays of numbers inline, long arrays
//! cut with a count). Provenance keys inside RoboCAD's JSON (`source`,
//! `provenance`, `mass_sources`, `radius_source`, `flex_patch_source`,
//! `reference`) are shown as kit chips; where RoboCAD gives none, nothing is
//! shown. A node summary's own `source` is not provenance: it is the node an
//! instance was made from, shown as the plain field "Instance of". Mass
//! values RoboCAD sent as null (or non-finite) read "null in RoboCAD's
//! answer", never a number. The attribute chips and Delete take their
//! action and enabled state from `panel::controls`, so they match
//! `system_ui`.
use super::actions::CadAction;
use super::document::CadDocument;
use super::document::Connection;
use super::panel::{CadButton, Control, HEADLESS_COMMANDS, Inert, NameDraft, NameField, column, command_label, control, controls, edit_blocked, material};
use crate::ui_kit::{DANGER, Kit, Look, SUBTLE, TEXT, VALUE, WARN, size, wrap};
use bevy::prelude::*;
use serde_json::Value;
use sim_runtime::cad_client::NodeSummary;
use std::collections::BTreeMap;

/// Keys whose values are provenance labels (shown as chips).
const PROVENANCE: [&str; 6] = ["source", "provenance", "mass_sources", "radius_source", "flex_patch_source", "reference"];
/// Whether `key` names a provenance label (`[&'static str]::contains`
/// would need a `&'static str`).
fn is_provenance(key: &str) -> bool {
    PROVENANCE.iter().any(|p| *p == key)
}
/// Items of an array shown before the rest is counted.
const MAX_ITEMS: usize = 20;
/// Values of an inline array shown before the rest is counted.
const MAX_INLINE: usize = 12;
/// A value longer than this goes under its key instead of beside it.
const BESIDE: usize = 34;

/// One rendered line of a JSON value.
#[derive(Debug, PartialEq)]
enum Line {
    Value { key: String, value: String },
    Provenance { key: String, value: String },
    More { key: String, hidden: usize },
}

fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::Null => Some("null".to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// An array of scalars (or of arrays of scalars: a matrix) on one line,
/// "[1, 2, 3]", cut after `MAX_INLINE` values with the count.
fn inline(v: &Value, depth: usize) -> Option<String> {
    match v {
        Value::Array(items) if depth < 2 => {
            let shown = items.iter().take(MAX_INLINE).map(|item| inline(item, depth + 1)).collect::<Option<Vec<_>>>()?;
            let rest = items.len().saturating_sub(MAX_INLINE);
            Some(if rest == 0 { format!("[{}]", shown.join(", ")) } else { format!("[{}, … {} more of {}]", shown.join(", "), rest, items.len()) })
        }
        Value::Array(_) | Value::Object(_) => None,
        other => scalar(other),
    }
}

/// Flatten `v` under `key` into lines; `provenance`: inside a provenance key.
fn flatten(key: &str, v: &Value, provenance: bool, out: &mut Vec<Line>) {
    let leaf = |value: String| if provenance { Line::Provenance { key: key.to_string(), value } } else { Line::Value { key: key.to_string(), value } };
    if let Some(value) = inline(v, 0) {
        out.push(leaf(value));
        return;
    }
    let join = |child: &str| if key.is_empty() { child.to_string() } else { format!("{key}.{child}") };
    match v {
        Value::Object(map) if map.is_empty() => out.push(leaf("{}".to_string())),
        Value::Object(map) => {
            for (child, value) in map {
                flatten(&join(child), value, provenance || is_provenance(child), out);
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().take(MAX_ITEMS).enumerate() {
                flatten(&join(&i.to_string()), item, provenance, out);
            }
            if items.len() > MAX_ITEMS {
                out.push(Line::More { key: key.to_string(), hidden: items.len() - MAX_ITEMS });
            }
        }
        _ => {}
    }
}

/// A key and its value: beside it when short (the kit's property row),
/// under it when long (so it wraps in the dock).
fn field(p: &mut ChildSpawnerCommands, k: &Kit, key: &str, value: &str, unit: &str) {
    if key.chars().count() + value.chars().count() + unit.chars().count() <= BESIDE {
        k.property(p, key, value, unit, None::<Inert>, false);
    } else {
        let text = if unit.is_empty() { value.to_string() } else { format!("{value} {unit}") };
        p.spawn(Node { padding: UiRect::vertical(Val::Px(2.0)), ..column(1.0) }).with_children(|c| {
            c.spawn(k.text(key, size::BODY, SUBTLE, 0));
            c.spawn(k.text(text, size::BODY, VALUE, 1));
        });
    }
}

/// A provenance label as RoboCAD wrote it: its key and a kit chip.
fn provenance(p: &mut ChildSpawnerCommands, k: &Kit, key: &str, value: &str) {
    p.spawn(Node { padding: UiRect::vertical(Val::Px(2.0)), ..column(3.0) }).with_children(|c| {
        c.spawn(k.text(key, size::BODY, SUBTLE, 0));
        c.spawn(k.chip(value, Inert, true, true)).entry::<Node>().and_modify(|mut n| {
            n.flex_shrink = 1.0;
            n.max_width = Val::Percent(100.0);
            n.align_self = AlignSelf::FlexStart;
        });
    });
}

/// Every line of `v` (keys relative to `name`; a scalar is keyed `name`).
fn lines(p: &mut ChildSpawnerCommands, k: &Kit, name: &str, v: &Value) {
    let mut out = Vec::new();
    flatten("", v, is_provenance(name), &mut out);
    for line in out {
        match line {
            Line::Value { key, value } => field(p, k, if key.is_empty() { name } else { key.as_str() }, &value, ""),
            Line::Provenance { key, value } => provenance(p, k, if key.is_empty() { name } else { key.as_str() }, &value),
            Line::More { key, hidden } => {
                p.spawn(k.note(format!("… {hidden} more items in {} not shown", if key.is_empty() { name } else { key.as_str() })));
            }
        }
    }
}

/// Numbers as Rust prints them (RoboCAD's values, unrounded).
fn numbers(v: &[f64]) -> String {
    format!("[{}]", v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))
}

/// The inspected node's summary from RoboCAD's `/doc`.
fn node<'a>(doc: &'a CadDocument, id: &str) -> Option<&'a NodeSummary> {
    doc.doc.as_ref()?.nodes.iter().find(|n| n.id == id)
}

pub(super) fn name_key(doc: &CadDocument, draft: &NameDraft) -> String {
    let sel = doc.selected();
    format!("{:?}", (sel, sel.and_then(|id| node(doc, id)).map(|n| (&n.name, &n.kind)), &draft.editing, &draft.refusal))
}

/// The inspector's head: the node's name (editable), kind and id.
pub(super) fn name(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, draft: &NameDraft) {
    let Some(id) = doc.selected() else {
        k.header(p, "Nothing selected", "Click a row in the model tree or a body in the 3D view.");
        return;
    };
    let Some(n) = node(doc, id) else {
        k.header(p, id, "Not in the document RoboCAD last sent (it may have been deleted).");
        return;
    };
    k.header(p, &n.name, &format!("{} · {}", n.kind, n.id));
    let editing = draft.editing.as_ref().filter(|(edit, _)| edit == id);
    let shown = editing.map_or(n.name.as_str(), |(_, text)| text.as_str());
    p.spawn(k.input(shown, "Name", NameField { id: id.to_string(), name: n.name.clone() }, editing.is_some()));
    match (editing, &draft.refusal) {
        (Some(_), Some(why)) => {
            p.spawn(k.text(why.clone(), size::SMALL, WARN, 0));
        }
        (Some(_), None) => {
            p.spawn(k.note("Enter renames it in RoboCAD (one undo step); Escape cancels."));
        }
        (None, _) => {
            p.spawn(k.note("Click the name to rename it."));
        }
    }
}

pub(super) fn inspector_key(doc: &CadDocument) -> String {
    let sel = doc.selected();
    let n = sel.and_then(|id| node(doc, id));
    let detail = doc.detail.as_ref().filter(|(id, ..)| Some(id.as_str()) == sel);
    let source = n.and_then(|n| n.source.as_deref()).map(|s| instance_of(doc, s));
    format!("{:?}", (sel, n, source, detail, doc.connected(), waiting(doc)))
}

/// "Instance of": the source node's name and id (the id alone when the
/// shown tree does not have it).
fn instance_of(doc: &CadDocument, source: &str) -> String {
    let name = doc.node_name(source);
    if name == source { source.to_string() } else { format!("{name} ({source})") }
}

/// Why no detail can come now (not connected), and whether that is an error.
fn waiting(doc: &CadDocument) -> Option<(String, bool)> {
    if doc.connected() {
        return None;
    }
    Some(match &doc.connection {
        Connection::Connecting { what, .. } => (format!("Waiting for the connection to RoboCAD: {what}."), false),
        Connection::Lost { error, .. } => (format!("No detail: not connected to RoboCAD: {error}"), true),
        Connection::Connected => ("Waiting for the connection to RoboCAD.".to_string(), false),
    })
}

/// A value RoboCAD may have sent as null (or non-finite): never a number then.
const NULL_VALUE: &str = "null in RoboCAD's answer";

fn maybe(v: Option<f64>) -> String {
    v.map_or_else(|| NULL_VALUE.to_string(), |x| x.to_string())
}

/// A mass-block value with its unit, or the null text without one.
fn mass_field(p: &mut ChildSpawnerCommands, k: &Kit, key: &str, v: Option<f64>, unit: &str) {
    field(p, k, key, &maybe(v), if v.is_some() { unit } else { "" });
}

/// A mass-block vector element-wise (each null named as such); the unit
/// only when some element is a number.
fn mass_vector(p: &mut ChildSpawnerCommands, k: &Kit, key: &str, v: &[Option<f64>], unit: &str) {
    let text = format!("[{}]", v.iter().map(|x| maybe(*x)).collect::<Vec<_>>().join(", "));
    field(p, k, key, &text, if v.iter().any(Option::is_some) { unit } else { "" });
}

/// The node: summary, transform, then RoboCAD's detail for it.
pub(super) fn inspector(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument) {
    let Some(id) = doc.selected() else { return };
    let Some(n) = node(doc, id) else { return };
    p.spawn(k.section("Node"));
    field(p, k, "Kind", &n.kind, "");
    field(p, k, "Id", &n.id, "");
    field(p, k, "Parent", n.parent.as_deref().unwrap_or("null (a root)"), "");
    field(p, k, "Children", &n.children.len().to_string(), "");
    field(p, k, "Effective visibility", if n.effective_visible { "shown" } else { "hidden" }, "");
    field(p, k, "Material", n.material.as_deref().unwrap_or("not set (null)"), "");
    if let Some(c) = &n.color {
        field(p, k, "Colour", &numbers(c), "");
    }
    if let Some(pivot) = &n.pivot {
        field(p, k, "Pivot", &numbers(pivot), "mm");
    }
    if let Some(source) = &n.source {
        field(p, k, "Instance of", &instance_of(doc, source), "");
    }
    if let Some(v) = &n.component_instance {
        lines(p, k, "component_instance", v);
    }
    if let Some(v) = &n.component_member {
        lines(p, k, "component_member", v);
    }
    p.spawn(k.section("Transform"));
    lines(p, k, "transform", &n.transform);
    p.spawn(k.section("Detail"));
    match &doc.detail {
        Some((detail_id, revision, result)) if detail_id == id => match result {
            Err(e) => {
                p.spawn(k.text(e.clone(), size::SMALL, DANGER, 0));
            }
            Ok(d) => {
                p.spawn(k.note(format!("RoboCAD's GET /nodes/{id} at revision {revision}")));
                if let Some(kind) = &d.body_kind {
                    field(p, k, "Body kind", kind, "");
                }
                if let Some(m) = &d.mass {
                    mass_field(p, k, "Volume", m.volume_mm3, "mm³");
                    mass_field(p, k, "Area", m.area_mm2, "mm²");
                    mass_field(p, k, "Mass", m.mass_g, "g");
                    mass_vector(p, k, "Centroid", &m.centroid, "mm");
                    mass_vector(p, k, "Bounding box min", &m.bbox_min, "mm");
                    mass_vector(p, k, "Bounding box max", &m.bbox_max, "mm");
                    mass_vector(p, k, "Size", &m.size, "mm");
                }
                if let Some(f) = d.face_count {
                    field(p, k, "Faces", &f.to_string(), "");
                }
                if let Some(e) = d.edge_count {
                    field(p, k, "Edges", &e.to_string(), "");
                }
                for (name, value) in [("joint", &d.joint), ("robot", &d.robot), ("sketch", &d.sketch), ("plane", &d.plane), ("measure", &d.measure), ("mirror_plane", &d.mirror_plane), ("mesh", &d.mesh), ("image", &d.image)] {
                    if let Some(v) = value {
                        p.spawn(k.section(name));
                        lines(p, k, name, v);
                    }
                }
            }
        },
        _ if !doc.connected() => {
            if let Some((text, error)) = waiting(doc) {
                p.spawn(k.text(text, size::SMALL, if error { DANGER } else { SUBTLE }, 0));
            }
        }
        _ => {
            p.spawn(k.caption("Loading…"));
        }
    }
}

/// The revision of the shown `/doc` (what `sync::fetch_physical` stamps).
fn current_revision(doc: &CadDocument) -> Option<u64> {
    doc.doc_key.as_ref().map(|k| k.1)
}

pub(super) fn physical_key(doc: &CadDocument) -> String {
    // Not the model itself (collision meshes make it large): its revision,
    // whether it failed, and the selection pick the shown link.
    let physical = doc.physical.as_ref().map(|(r, result)| (*r, result.as_ref().err()));
    format!("{:?}", (doc.selected(), physical, current_revision(doc), doc.physical_job.is_some()))
}

/// The physical link holding the inspected body, from `GET /physical?flex=0`.
pub(super) fn physical(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument) {
    let Some(id) = doc.selected() else { return };
    p.spawn(k.section("Physical link"));
    if doc.physical_job.is_some() {
        p.spawn(k.caption("Fetching RoboCAD's physical model…"));
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

pub(super) fn attributes_key(doc: &CadDocument) -> String {
    let sel = doc.selected();
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
pub(super) fn attributes(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument) {
    let Some(id) = doc.selected() else { return };
    let Some(n) = node(doc, id) else { return };
    p.spawn(k.section("Edit"));
    let blocked = edit_blocked(doc);
    p.spawn(k.caption(match &blocked {
        Some(why) => format!("Editing is unavailable: {why}."),
        None => "Each change is one step in RoboCAD's undo history.".to_string(),
    }));
    let all = controls(doc);
    p.spawn(wrap()).with_children(|r| {
        for (key, label, on) in [("visible", "Visible", n.visible), ("locked", "Locked", n.locked), ("disabled", "Disabled", n.disabled)] {
            control_chip(r, k, &all, &format!("cad:{key}:{id}"), label, on);
        }
    });
    p.spawn(k.text("Material", size::BODY, TEXT, 1));
    let materials: Vec<(String, String)> = doc.doc.as_ref().map(|d| d.materials.iter().filter_map(material).collect()).unwrap_or_default();
    if materials.is_empty() {
        p.spawn(k.caption("RoboCAD listed no materials."));
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

/// RoboCAD's undo and redo labels, most recent first (read-only).
pub(super) fn history(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument) {
    p.spawn(k.section("History"));
    let Some(d) = &doc.doc else {
        p.spawn(k.caption("RoboCAD has not sent its history yet."));
        return;
    };
    for (title, labels, empty) in [("Undo (most recent first)", &d.history.undo, "Nothing to undo."), ("Redo (most recent first)", &d.history.redo, "Nothing to redo.")] {
        p.spawn(k.text(title, size::BODY, TEXT, 1));
        if labels.is_empty() {
            p.spawn(k.note(empty));
        }
        for label in labels.iter().rev() {
            p.spawn((k.text(label.clone(), size::SMALL, SUBTLE, 0), Node { margin: UiRect::left(Val::Px(8.0)), ..default() }));
        }
    }
}

pub(super) fn commands_key(doc: &CadDocument) -> String {
    format!("{:?}", (&doc.commands, doc.health.as_ref().map(|h| h.gui), edit_blocked(doc)))
}

/// RoboCAD's GUI command registry, by category; a press runs the command there.
pub(super) fn commands(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument) {
    p.spawn(k.section("RoboCAD commands"));
    let Some(health) = &doc.health else {
        p.spawn(k.caption("Waiting for RoboCAD to answer."));
        return;
    };
    let map = match &doc.commands {
        Some(Err(e)) => {
            p.spawn(k.text(e.clone(), size::SMALL, DANGER, 0));
            return;
        }
        Some(Ok(map)) if health.gui && !map.is_empty() => map,
        Some(Ok(_)) if health.gui => {
            p.spawn(k.caption("RoboCAD listed no commands."));
            return;
        }
        Some(Ok(_)) => {
            p.spawn(k.caption(HEADLESS_COMMANDS));
            return;
        }
        None if !health.gui => {
            p.spawn(k.caption(HEADLESS_COMMANDS));
            return;
        }
        None => {
            p.spawn(k.caption("Loading RoboCAD's command registry…"));
            return;
        }
    };
    let blocked = edit_blocked(doc);
    if let Some(why) = &blocked {
        p.spawn(k.text(format!("Commands are unavailable: {why}."), size::SMALL, WARN, 0));
    }
    let all = controls(doc);
    let mut groups: BTreeMap<&str, Vec<(&String, &sim_runtime::cad_client::CommandInfo)>> = BTreeMap::new();
    for (id, info) in map {
        groups.entry(info.category.as_str()).or_default().push((id, info));
    }
    for (category, items) in groups {
        p.spawn(k.text(if category.is_empty() { "Other" } else { category }, size::BODY, TEXT, 1));
        p.spawn(wrap()).with_children(|r| {
            for (id, info) in items {
                // The action and enabled state `system_ui` lists (`panel::controls`).
                let ready = control(&all, &format!("cad:command:{id}")).map_or(blocked.is_none(), |c| c.ready.is_ok());
                r.spawn(k.button(&command_label(&info.label, &info.keys), CadButton(CadAction::CadCommand { id: id.clone() }), Look::Secondary, ready));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Nested keys flatten to "a.b"; number arrays and matrices stay inline;
    /// provenance keys (and what is under them) become chips; long arrays
    /// are cut with a count; nothing is added that the value lacks.
    #[test]
    fn values_flatten_as_returned() {
        let points: Vec<Value> = (0..25).map(|i| json!({ "x": i })).collect();
        let v = json!({"mass_properties": {"mass_kg": 0.25, "source": "scale 2026-09-01"}, "axis": [0.0, 0.0, 1.0], "I": [[1, 0], [0, 1]], "mass_sources": {"n1": "CAD volume and material density"}, "points": points});
        let mut out = Vec::new();
        flatten("", &v, false, &mut out);
        assert!(out.contains(&Line::Value { key: "mass_properties.mass_kg".into(), value: "0.25".into() }));
        assert!(out.contains(&Line::Provenance { key: "mass_properties.source".into(), value: "scale 2026-09-01".into() }));
        assert!(out.contains(&Line::Value { key: "axis".into(), value: "[0.0, 0.0, 1.0]".into() }));
        assert!(out.contains(&Line::Value { key: "I".into(), value: "[[1, 0], [0, 1]]".into() }));
        assert!(out.contains(&Line::Provenance { key: "mass_sources.n1".into(), value: "CAD volume and material density".into() }));
        assert!(out.contains(&Line::Value { key: "points.19.x".into(), value: "19".into() }));
        assert!(!out.iter().any(|l| matches!(l, Line::Value { key, .. } if key == "points.20.x")));
        assert!(out.contains(&Line::More { key: "points".into(), hidden: 5 }));
        let numbers: Vec<u32> = (0..15).collect();
        assert_eq!(inline(&json!(numbers), 0).unwrap(), "[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, … 3 more of 15]");
    }
}
