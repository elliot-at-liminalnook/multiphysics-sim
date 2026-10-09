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
//! `system_ui`. When the first selected item is a face, edge, vertex or
//! point, a "Face 3" / "Edge 7" / "Vertex 2" / "Point on face 4" section
//! comes first, from RoboCAD's topology of the node (`CadTopology`: `GET
//! /nodes/{id}/faces|edges|vertices`), values as RoboCAD returned them (mm,
//! mm²), "fetching…" while they load and the fetch's error verbatim.
//! The node's pivot and, for instances and reference meshes and images,
//! its transform are editable ([`editors`]: one `CadPatch` per Enter).
//!
//! Modules: [`node`] (the head, the sub-body item, the node and its
//! detail), [`editors`] (pivot, transform, tessellation tolerance),
//! [`sections`] (the physical link, attributes, history, commands) and,
//! for cad-physical-inspect, [`physical_edit`] (`cad_inspector`: its action,
//! handler, controls and state), [`rows`] (the physical rows RoboCAD's
//! properties panel shows: colour, the joint and its physics overrides,
//! the results line, exact measurements, material properties), [`entry`]
//! (their typing), [`exact`] (the exact-measurement job) and [`refresh`]
//! (the physical model fetched again for each revision while it is shown).
mod editors;
mod entry;
mod exact;
mod node;
mod physical_edit;
#[cfg(test)]
mod physical_tests;
mod refresh;
mod rows;
mod sections;

pub use editors::EditDraft;
pub(super) use editors::EDITOR;
/// The editors' typing system.
use editors::entry as editor_entry;
pub(super) use node::{inspector, inspector_key, name, name_key};
pub(crate) use physical_edit::{CoreParts, InspectorArgs, PhysicalEdit, py_g};
pub(super) use physical_edit::{build_physical, handle_physical, physical_controls, physical_specs, physical_state_json};
pub(super) use sections::{attributes, attributes_key, commands, commands_key, history, physical, physical_key};

use super::document::CadDocument;
use super::panel::{Inert, column};
use crate::app::{ViewerMode};
use crate::ui_kit::{Kit, SUBTLE, VALUE, size};
use bevy::prelude::*;
use serde_json::Value;
use crate::cad::types::NodeSummary;

/// The editors' field and their typing (Input).
pub(super) fn build(app: &mut App) {
    use crate::ui_kit::text::TextFieldApp;
    app.add_text_field(editors::EDITOR, editors::editor_field()).add_systems(
        Update,
        editor_entry
            .in_set(crate::cad::CadKeySet::Focus)
            .run_if(in_state(ViewerMode::Cad)),
    );
}

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

/// The revision of the shown `/doc` (what `sync::fetch_physical` stamps).
fn current_revision(doc: &CadDocument) -> Option<u64> {
    doc.doc_key.as_ref().map(|k| k.1)
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
