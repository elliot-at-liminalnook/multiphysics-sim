//! The preview's life (display transforms of the moved bodies until
//! RoboCAD's answer is drawn) and `cad_state.tool_state`.
use super::{FieldCommit, MESH_WAIT, Phase, RELEASE_WAIT, hint, mm, mode_label, r6};
use crate::cad::document::CadDocument;
use crate::cad::mesh::{CadBody, CadMeshes};
use bevy::prelude::*;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::time::Instant;

/// Whether every moved body is drawn from a revision newer than `began`
/// (or no longer drawn). `drawn(id)`: `CadMeshes::drawn_revision`.
fn landed(drawn: &impl Fn(&str) -> Option<u64>, bodies: &[String], began: u64) -> bool {
    bodies.iter().all(|id| drawn(id).is_none_or(|r| r > began))
}

/// The preview's life (see the module doc): dropped when its drag is gone,
/// its commit was never applied, its edit failed, its meshes landed, or the
/// document was replaced.
pub(super) fn settle(doc: &mut CadDocument, drawn: impl Fn(&str) -> Option<u64>) {
    let generation = doc.generation;
    let running = doc.edit.as_ref().map(|_| doc.edit_seq);
    let failed = matches!(doc.status, Some(Err(_)));
    let dragging = doc.tool_state.drag.is_some();
    let Some(p) = doc.tool_state.preview.as_mut() else { return };
    let clear = p.generation != generation
        || match &mut p.phase {
            Phase::Live => !dragging,
            Phase::Released { at, .. } => at.elapsed() > RELEASE_WAIT,
            Phase::Committed { seq, ended } => {
                if running == Some(*seq) {
                    false
                } else if let Some(t) = ended {
                    t.elapsed() > MESH_WAIT || landed(&drawn, &p.bodies, p.began)
                } else {
                    // The first frame after the edit ended: its outcome is the status line.
                    *ended = Some(Instant::now());
                    failed || landed(&drawn, &p.bodies, p.began)
                }
            }
        };
    if clear {
        doc.tool_state.preview = None;
    }
}

/// SimSync (after `mesh::sync`): every drawn body's display transform is
/// the preview's, or identity (also a moved body whose newer mesh already
/// landed while others still wait).
pub(in crate::cad) fn previews(doc: Option<ResMut<CadDocument>>, meshes: Option<Res<CadMeshes>>, mut bodies: Query<(&CadBody, &mut Transform)>) {
    let mut want: HashMap<String, Transform> = HashMap::new();
    if let Some(mut doc) = doc
        && doc.tool_state.preview.is_some()
    {
        let meshes = meshes.as_deref();
        settle(&mut doc, |id| meshes.and_then(|m| m.drawn_revision(id)));
        if let Some(p) = &doc.tool_state.preview {
            let t = p.delta.transform();
            // A body already drawn from a newer revision shows RoboCAD's
            // result itself: offsetting it again would move it twice.
            for id in p.bodies.iter().filter(|id| meshes.and_then(|m| m.drawn_revision(id)).is_none_or(|r| r <= p.began)) {
                want.insert(id.clone(), t);
            }
        }
    }
    for (body, mut transform) in &mut bodies {
        let target = want.get(&body.id).copied().unwrap_or(Transform::IDENTITY);
        if *transform != target {
            *transform = target;
        }
    }
}

// ---- cad_state.tool_state ----------------------------------------------------------

fn result_json(r: &Result<f64, String>) -> Value {
    match r {
        Ok(v) => json!({"ok": true, "value": v}),
        Err(e) => json!({"ok": false, "error": e}),
    }
}

/// `cad_state.tool_state`: the tool, its pivot, the drag and preview, the
/// push/pull target, the numeric fields with their drafts and evaluations,
/// the measure picks, the snap and the readout. Built key by key (small
/// `json!` literals: the macro's recursion limit).
pub(in crate::cad) fn state_json(doc: &CadDocument) -> Value {
    let s = &doc.tool_state;
    let mut out = Map::new();
    out.insert("tool".into(), json!(doc.tool.name()));
    out.insert("hint".into(), json!(hint(doc.tool)));
    out.insert("mode_label".into(), json!(mode_label(doc)));
    out.insert("pivot".into(), s.pivot.map_or(Value::Null, |(p, rule)| json!({"point": mm(p), "rule": rule.name()})));
    out.insert("hover_handle".into(), json!(s.hover));
    out.insert("axis".into(), json!(s.axis));
    let drag = s.drag.as_ref().map_or(Value::Null, |d| {
        let mut m = Map::new();
        m.insert("kind".into(), json!(d.tool.name()));
        m.insert("handle".into(), json!(d.handle));
        m.insert("origin".into(), json!(mm(d.origin)));
        m.insert("axis".into(), json!(mm(d.axis)));
        m.insert("revision_began".into(), json!(d.began));
        m.insert("delta".into(), d.delta.map_or(Value::Null, |x| x.json()));
        Value::Object(m)
    });
    out.insert("drag".into(), drag);
    let preview = s.preview.as_ref().map_or(Value::Null, |p| {
        let phase = match &p.phase {
            Phase::Live => "live",
            Phase::Released { .. } => "released",
            Phase::Committed { ended: None, .. } => "committed",
            Phase::Committed { ended: Some(_), .. } => "waiting for meshes",
        };
        json!({"phase": phase, "revision_began": p.began, "bodies": p.bodies, "delta": p.delta.json()})
    });
    out.insert("preview".into(), preview);
    let target = s.push.as_ref().map_or(Value::Null, |t| json!({"node": t.node, "face": t.face, "revision": t.revision}));
    let push_drag = s.push_drag.as_ref().map_or(Value::Null, |d| json!({"node": d.node, "face": d.face, "distance": r6(d.distance), "revision_began": d.began}));
    out.insert("push_pull".into(), json!({"target": target, "drag": push_drag}));
    let n = &s.numeric;
    let fields: Vec<Value> = n
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mut m = Map::new();
            m.insert("name".into(), json!(f.name));
            m.insert("kind".into(), json!(f.kind.name()));
            m.insert("value".into(), json!(f.value));
            m.insert("text".into(), json!(n.texts.get(i)));
            m.insert("result".into(), n.results.get(i).map_or(Value::Null, result_json));
            m.insert("read_only".into(), json!(matches!(f.commit, FieldCommit::ReadOnly(_))));
            Value::Object(m)
        })
        .collect();
    out.insert("numeric".into(), json!({"fields": fields, "focus": n.focus, "revision_began": n.began}));
    out.insert("dimension_entry".into(), s.dimension.as_ref().map_or(Value::Null, |e| json!({"node": e.node, "face": e.face, "field": e.field.name})));
    let first = s.measure.first.as_ref().map_or(Value::Null, |p| json!({"item": p.item, "point": p.point}));
    let last = s.measure.last.as_ref().map_or(Value::Null, |m| m.json());
    out.insert("measure".into(), json!({"first": first, "last": last}));
    out.insert("snap".into(), s.snap.as_ref().map_or(Value::Null, |x| json!({"kind": x.kind.name(), "point": mm(x.point), "node": x.node, "readout": x.readout()})));
    out.insert("readout".into(), json!(s.readout));
    Value::Object(out)
}
