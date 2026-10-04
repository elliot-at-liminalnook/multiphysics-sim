//! The inspector's head and node sections: the inspected node's name
//! (editable), the sub-body item (face, edge, vertex, point) from RoboCAD's
//! topology, the node's summary, its pivot and transform editors
//! ([`super::editors`]) and RoboCAD's `GET /nodes/{id}` detail.
use super::{NULL_VALUE, field, lines, mass_field, mass_vector, node, numbers};
use crate::cad::document::{CadDocument, Connection};
use crate::cad::panel::{NameDraft, NameField};
use crate::cad::selection::CadItems;
use crate::cad::topology::CadTopology;
use crate::ui_kit::{DANGER, Kit, SUBTLE, WARN, size};
use bevy::prelude::*;
use sim_runtime::cad_client::{FaceInfo, SelectionItem};

pub(in crate::cad) fn name_key(doc: &CadDocument, selection: &[SelectionItem], draft: &NameDraft) -> String {
    let sel = selection.first_node();
    format!("{:?}", (sel, sel.and_then(|id| node(doc, id)).map(|n| (&n.name, &n.kind)), draft.editing(), &draft.refusal))
}

/// The inspector's head: the node's name (editable), kind and id.
pub(in crate::cad) fn name(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, selection: &[SelectionItem], draft: &NameDraft) {
    let Some(id) = selection.first_node() else {
        k.header(p, "Nothing selected", "Click a row in the model tree or a body in the 3D view.");
        return;
    };
    let Some(n) = node(doc, id) else {
        k.header(p, id, "Not in the current document snapshot (it may have been replaced or deleted).");
        return;
    };
    k.header(p, &n.name, &format!("{} · {}", n.kind, n.id));
    if doc.local.is_some() {
        p.spawn(k.note("Archived source is read only; renaming awaits Rust modelling migration."));
        return;
    }
    let editing = draft.editing().filter(|(edit, _)| *edit == id);
    let shown = editing.map_or(n.name.as_str(), |(_, text)| text);
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

pub(in crate::cad) fn inspector_key(doc: &CadDocument, selection: &[SelectionItem], topology: Option<&CadTopology>) -> String {
    let sel = selection.first_node();
    let n = sel.and_then(|id| node(doc, id));
    let detail = doc.detail.as_ref().filter(|(id, ..)| Some(id.as_str()) == sel);
    let source = n.and_then(|n| n.source.as_deref()).map(|s| instance_of(doc, s));
    format!("{:?}", (sel, n, source, detail, doc.connected(), waiting(doc), sub_key(doc, selection, topology), super::editors::key(doc)))
}

/// The first selected item when it is a face, edge, vertex or point.
fn sub_item(selection: &[SelectionItem]) -> Option<&SelectionItem> {
    selection.first().filter(|i| i.1 != "body")
}

/// What the sub-body section shows: the item, the selection's size and the
/// node's topology state (the element shown, the error, or loading).
fn sub_key(doc: &CadDocument, selection: &[SelectionItem], topology: Option<&CadTopology>) -> String {
    let Some(SelectionItem(node, kind, index)) = sub_item(selection) else { return String::new() };
    let state = topology.map(|t| match t.get(node) {
        Some(topo) => {
            let element = match kind.as_str() {
                "edge" => format!("{:?}", topo.edges.iter().find(|e| e.index == *index).map(|e| (&e.kind, e.length, e.radius, e.center, e.start, e.end, e.midpoint))),
                "vertex" => format!("{:?}", topo.vertices.iter().find(|v| v.index == *index)),
                _ => format!("{:?}", topo.faces.iter().find(|f| f.index == *index)),
            };
            format!("{} {element}", topo.revision)
        }
        None => format!("{:?} {}", t.error(node), t.pending(node)),
    });
    format!("{node} {kind} {index} {} {} {state:?}", selection.len(), doc.node_name(node))
}

/// Three coordinates as RoboCAD sent them, or the null text.
fn point_text(v: Option<[f64; 3]>) -> (String, bool) {
    match v {
        Some(p) => (numbers(&p), true),
        None => (NULL_VALUE.to_string(), false),
    }
}

/// A vector row with its unit only when RoboCAD sent it.
fn point_field(p: &mut ChildSpawnerCommands, k: &Kit, key: &str, v: Option<[f64; 3]>, unit: &str) {
    let (text, present) = point_text(v);
    field(p, k, key, &text, if present { unit } else { "" });
}

/// A face's rows (`face_json`): kind, area, normal, centroid, radius and its
/// diameter, the axis for revolved surfaces.
fn face_rows(p: &mut ChildSpawnerCommands, k: &Kit, f: &FaceInfo) {
    field(p, k, "Surface", &f.kind, "");
    mass_field(p, k, "Area", f.area, "mm²");
    point_field(p, k, "Normal", f.normal, "");
    point_field(p, k, "Centroid", f.centroid, "mm");
    if let Some(r) = f.radius {
        field(p, k, "Radius", &r.to_string(), "mm");
        field(p, k, "Ø", &(2.0 * r).to_string(), "mm");
    }
    if f.axis_point.is_some() || f.axis_dir.is_some() {
        point_field(p, k, "Axis point", f.axis_point, "mm");
        point_field(p, k, "Axis direction", f.axis_dir, "");
    }
}

/// The sub-body section ("Face 3", "Edge 7", "Vertex 2", "Point on face 4").
fn sub_body(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, selection: &[SelectionItem], topology: Option<&CadTopology>) {
    let Some(SelectionItem(node, kind, index)) = sub_item(selection) else { return };
    let title = match kind.as_str() {
        "face" => format!("Face {index}"),
        "edge" => format!("Edge {index}"),
        "vertex" => format!("Vertex {index}"),
        "point" => format!("Point on face {index}"),
        other => format!("{other} {index}"),
    };
    let name = doc.node_name(node);
    p.spawn(k.section(&title));
    field(p, k, "Of", &name, "");
    if selection.len() > 1 {
        field(p, k, "Selected items", &selection.len().to_string(), "");
    }
    let Some(topology) = topology else {
        p.spawn(k.caption("This window holds no topology (no 3D view)."));
        return;
    };
    let Some(topo) = topology.get(node) else {
        match topology.error(node) {
            Some(e) => {
                p.spawn(k.text(e.to_string(), size::SMALL, DANGER, 0));
            }
            None => {
                p.spawn(k.caption("fetching…"));
            }
        }
        return;
    };
    p.spawn(k.note(format!("Exact topology of {name} at revision {} (mm)", topo.revision)));
    let missing = |what: &str| format!("RoboCAD listed no {what} {index} for {name} at revision {}.", topo.revision);
    match kind.as_str() {
        "edge" => match topo.edges.iter().find(|e| e.index == *index) {
            Some(e) => {
                field(p, k, "Curve", &e.kind, "");
                mass_field(p, k, "Length", e.length, "mm");
                if let Some(r) = e.radius {
                    field(p, k, "Radius", &r.to_string(), "mm");
                    field(p, k, "Ø", &(2.0 * r).to_string(), "mm");
                }
                if e.center.is_some() {
                    point_field(p, k, "Centre", e.center, "mm");
                }
                point_field(p, k, "Start", e.start, "mm");
                point_field(p, k, "End", e.end, "mm");
                point_field(p, k, "Midpoint", e.midpoint, "mm");
            }
            None => {
                p.spawn(k.caption(missing("edge")));
            }
        },
        "vertex" => match topo.vertices.iter().find(|v| v.index == *index) {
            Some(v) => point_field(p, k, "Position", v.point, "mm"),
            None => {
                p.spawn(k.caption(missing("vertex")));
            }
        },
        _ => match topo.faces.iter().find(|f| f.index == *index) {
            Some(f) => {
                if kind == "point" {
                    p.spawn(k.note("A point pick names the face it lies on; the face:"));
                }
                face_rows(p, k, f);
            }
            None => {
                p.spawn(k.caption(missing("face")));
            }
        },
    }
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
    if doc.client.is_none() {
        return Some(match &doc.connection {
            Connection::Connecting { what, .. } => (format!("Local archive loading: {what}."), false),
            Connection::Lost { error, .. } => (format!("Local archive unavailable: {error}"), true),
            Connection::Connected => ("No local document snapshot is available.".to_string(), false),
        });
    }
    Some(match &doc.connection {
        Connection::Connecting { what, .. } => (format!("Waiting for the connection to RoboCAD: {what}."), false),
        Connection::Lost { error, .. } => (format!("No detail: no CAD document is open: {error}"), true),
        Connection::Connected => ("Waiting for the connection to RoboCAD.".to_string(), false),
    })
}

/// The sub-body item (if the first selected is one), then the node:
/// summary, transform, then RoboCAD's detail for it.
pub(in crate::cad) fn inspector(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, selection: &[SelectionItem], topology: Option<&CadTopology>) {
    let Some(id) = selection.first_node() else { return };
    let Some(n) = node(doc, id) else { return };
    sub_body(p, k, doc, selection, topology);
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
    if let Some(source) = &n.source {
        field(p, k, "Instance of", &instance_of(doc, source), "");
    }
    if let Some(v) = &n.component_instance {
        lines(p, k, "component_instance", v);
    }
    if let Some(v) = &n.component_member {
        lines(p, k, "component_member", v);
    }
    if doc.local.is_none() { super::editors::editors(p, k, doc, n); }
    if let Some(local) = &doc.local {
        p.spawn(k.section("Exact physical properties"));
        if let Some(body) = local.geometry.iter().find(|body| body.node_id == id) {
            field(p, k, "B-rep volume", &body.properties.volume_mm3.to_string(), "mm³");
        }
        if let Some(mass) = local.masses.bodies.get(id) {
            field(p, k, "Mass", &mass.mass_kg.to_string(), "kg");
            field(p, k, "Centroid", &numbers(&mass.centroid_m), "m");
            for (i, row) in mass.inertia_kg_m2.iter().enumerate() {
                field(p, k, &format!("Inertia row {}", i + 1), &numbers(row), "kg·m²");
            }
            field(p, k, "Origin", &mass.origin, "");
            lines(p, k, "Source", &mass.source);
            lines(p, k, "Provenance", &mass.provenance);
            field(p, k, "Included in", mass.included_in.as_deref().unwrap_or("standalone"), "");
        } else { p.spawn(k.note("This node has no applicable mass declaration or solid body.")); }
        if let Some(raw) = local.archive.node(id) {
            p.spawn(k.section("Archived metadata"));
            lines(p, k, "Node", raw);
        }
        return;
    }
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
