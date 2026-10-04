//! Comment threads stored in the archive (`manifest.annotations`), in
//! RoboCAD's schema so either editor reads the other's files. Reference:
//! RoboCAD's annotations.py (`thread_detail`, `thread_parts`,
//! `validate_part_refs`, `evidence_reference`, `camera_view`, `AnnotationOps`).
//!
//! Additions RoboCAD ignores: a comment's `author_kind` (`person` | `agent`;
//! an agent's comment id also starts with `agent-`, which the shared thread
//! panel shows as an agent's reply).
//!
//! Geometry stamps: a pin stores RoboCAD's own stamp of its part
//! (`crate::stamp`, reproduced exactly), so pins read the same in both
//! editors; a pin is attached while its stamp is one of the part's current
//! stamps (drawn or freshly read, or this editor's earlier `sim-cad:`
//! fingerprint), and needs review once the part's geometry changed.
use crate::edit::{Edit, new_id, now_iso};
use crate::ArchiveDocument;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};

pub const MAX_TEXT: usize = 20000;
pub const MAX_PARTS: usize = 200;

/// The current stamps of nodes (the first is the one a new pin stores):
/// filled for the nodes that carry pins; others are computed when asked.
pub type Stamps = HashMap<String, Vec<String>>;

/// Node `id`'s current stamps: RoboCAD's (as drawn, then as read) and this
/// editor's fingerprint; empty for a node without B-rep geometry.
pub fn current_stamps(doc: &ArchiveDocument, stamps: &Stamps, id: &str) -> Vec<String> {
    if let Some(s) = stamps.get(id) {
        return s.clone();
    }
    let mut out: Vec<String> = crate::stamp::stamps(doc, id).map(|s| s.to_vec()).unwrap_or_default();
    if let Ok(f) = crate::geometry::fingerprint(doc, id) {
        out.push(f);
    }
    out
}

/// The stamps of every node that carries a pin (what a snapshot keeps).
pub fn pinned_stamps(doc: &ArchiveDocument) -> Stamps {
    let ids: std::collections::BTreeSet<String> = doc.manifest["annotations"].as_object().into_iter().flatten().filter_map(|(_, t)| t["anchor"]["node_id"].as_str().map(str::to_string)).collect();
    ids.into_iter().filter(|id| doc.node(id).is_some()).map(|id| {
        let s = current_stamps(doc, &Stamps::new(), &id);
        (id, s)
    }).collect()
}

/// RoboCAD's `text(value, name)`: trimmed, non-empty, at most 20000 characters.
pub fn text(value: Option<&str>, name: &str) -> Result<String, String> {
    let v = value.map(str::trim).unwrap_or("");
    if v.is_empty() {
        return Err(format!("{name} must not be empty"));
    }
    if v.chars().count() > MAX_TEXT {
        return Err(format!("{name} is too long"));
    }
    Ok(v.to_string())
}

fn finite3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array().filter(|a| a.len() == 3)?;
    let mut out = [0.; 3];
    for i in 0..3 {
        if a[i].is_boolean() {
            return None;
        }
        out[i] = a[i].as_f64().filter(|x| x.is_finite())?;
    }
    Some(out)
}

/// RoboCAD's `camera_view`: only the camera's own keys, each in range.
pub fn camera_view(view: Option<&Value>) -> Result<Map<String, Value>, String> {
    let Some(view) = view.filter(|v| !v.is_null()) else { return Ok(Map::new()) };
    let view = view.as_object().ok_or("view must be an object")?;
    for (key, value) in view {
        match key.as_str() {
            "mode" => {
                if !matches!(value.as_str(), Some("turntable" | "trackball")) {
                    return Err("invalid camera mode".into());
                }
            }
            "rot" => {
                let rows: Option<Vec<[f64; 3]>> = value.as_array().filter(|r| r.len() == 3).and_then(|r| r.iter().map(finite3).collect());
                let ok = rows.is_some_and(|m| {
                    let orthonormal = (0..3).all(|i| (0..3).all(|j| {
                        let dot: f64 = (0..3).map(|k| m[i][k] * m[j][k]).sum();
                        (dot - if i == j { 1. } else { 0. }).abs() <= 1e-5
                    }));
                    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
                    orthonormal && (det - 1.).abs() <= 1e-5
                });
                if !ok {
                    return Err("camera rotation must be an orthonormal 3 by 3 matrix".into());
                }
            }
            "orthographic" => {
                if !value.is_boolean() {
                    return Err("orthographic must be boolean".into());
                }
            }
            "target" => {
                if finite3(value).is_none() {
                    return Err("camera target needs three finite coordinates".into());
                }
            }
            "distance" | "yaw" | "pitch" | "fov" => {
                let v = value.as_f64().filter(|v| v.is_finite() && !value.is_boolean()).ok_or("camera values must be finite numbers")?;
                if key == "distance" && v <= 0. || key == "fov" && !(1. ..=170.).contains(&v) {
                    return Err("camera distance or field of view is out of range".into());
                }
            }
            _ => return Err("unsupported annotation camera field".into()),
        }
    }
    Ok(view.clone())
}

fn node<'d>(doc: &'d ArchiveDocument, id: &str) -> Option<&'d Value> {
    doc.node(id)
}

/// `thread_parts`: stored refs (or, on older threads, the anchor part or
/// the evidence parts), then every `[label](part:ID)` link in a comment.
pub fn thread_parts(thread: &Value) -> Vec<Value> {
    let mut refs: Vec<Value> = match thread.get("part_refs").and_then(Value::as_array) {
        Some(r) => r.clone(),
        None => match thread["anchor"]["node_id"].as_str() {
            Some(id) => vec![json!({"node_id": id})],
            None => thread["evidence"]["node_ids"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|id| json!({"node_id": id})).collect(),
        },
    };
    let mut seen: HashSet<String> = refs.iter().filter_map(|r| r["node_id"].as_str().map(str::to_string)).collect();
    for c in thread["comments"].as_array().into_iter().flatten() {
        for (label, id) in part_links(c["body"].as_str().unwrap_or("")) {
            if seen.insert(id.clone()) {
                refs.push(json!({"node_id": id, "label": label}));
            }
        }
    }
    refs
}

/// `PART_LINK`: every `[label](part:ID)` in `body` (label without `]` or a
/// newline, id of letters, digits, `_` and `-`).
pub fn part_links(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else { break };
        let label = &after[..close];
        let tail = &after[close + 1..];
        if !label.is_empty() && !label.contains('\n') && !label.contains('[') && let Some(link) = tail.strip_prefix("(part:") {
            let id: String = link.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
            if !id.is_empty() && link[id.len()..].starts_with(')') {
                out.push((label.to_string(), id.clone()));
                rest = &link[id.len() + 1..];
                continue;
            }
        }
        rest = after;
    }
    out
}

/// A pin's state now: `evidence`, `missing`, `attached` or `needs_review`.
pub fn anchor_status(doc: &ArchiveDocument, stamps: &Stamps, anchor: &Value) -> &'static str {
    let Some(id) = anchor["node_id"].as_str() else { return "evidence" };
    if node(doc, id).is_none() {
        return "missing";
    }
    let current = current_stamps(doc, stamps, id);
    match anchor["geometry"].as_str() {
        None if current.is_empty() => "attached",
        Some(stored) if current.iter().any(|c| c == stored) => "attached",
        _ => "needs_review",
    }
}

/// `thread_detail`: the stored thread plus `node_name`, `anchor_status` and
/// `linked_parts` (each ref with the part's current name and whether it exists).
pub fn detail(doc: &ArchiveDocument, stamps: &Stamps, thread: &Value) -> Value {
    let mut out = thread.clone();
    let anchor = &thread["anchor"];
    let name = |id: &str| node(doc, id).and_then(|n| n["name"].as_str()).map(str::to_string);
    out["node_name"] = json!(match anchor["node_id"].as_str() {
        None => "Experiment evidence".to_string(),
        Some(id) => name(id).unwrap_or_else(|| "Deleted part".into()),
    });
    out["anchor_status"] = json!(anchor_status(doc, stamps, anchor));
    out["linked_parts"] = Value::Array(
        thread_parts(thread)
            .into_iter()
            .map(|mut r| {
                let id = r["node_id"].as_str().unwrap_or("").to_string();
                r["name"] = json!(name(&id).unwrap_or_else(|| "Deleted part".into()));
                r["available"] = json!(node(doc, &id).is_some());
                r
            })
            .collect(),
    );
    out
}

/// `AnnotationOps.threads`: every thread (filtered by part, status or run),
/// as [`detail`] answers it, oldest first.
pub fn list(doc: &ArchiveDocument, stamps: &Stamps, node_id: Option<&str>, status: Option<&str>, run_id: Option<&str>) -> Result<Vec<Value>, String> {
    if !matches!(status, None | Some("open" | "resolved")) {
        return Err("status must be open or resolved".into());
    }
    let mut out: Vec<Value> = doc.manifest["annotations"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .filter(|t| {
            node_id.is_none_or(|id| {
                t["anchor"]["node_id"] == id
                    || t["evidence"]["node_ids"].as_array().is_some_and(|ids| ids.iter().any(|n| n == id))
                    || thread_parts(t).iter().any(|r| r["node_id"] == id)
            }) && status.is_none_or(|s| t["status"] == s)
                && run_id.is_none_or(|r| t["evidence"]["run_id"] == r)
        })
        .map(|t| detail(doc, stamps, t))
        .collect();
    out.sort_by(|a, b| a["created_at"].as_str().cmp(&b["created_at"].as_str()));
    Ok(out)
}

/// `validate_part_refs`: at most 200 unique, existing (or already linked) parts,
/// each with an optional label (≤ 120), description (≤ 1000) and saved view.
pub fn validate_part_refs(doc: &ArchiveDocument, refs: &Value, previous: &[Value]) -> Result<Vec<Value>, String> {
    let refs = refs.as_array().filter(|r| r.len() <= MAX_PARTS).ok_or("Linked parts must be a list of at most 200 parts")?;
    let retained: HashSet<&str> = previous.iter().filter_map(|r| r["node_id"].as_str()).collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for value in refs {
        let mut r = match value {
            Value::String(id) => json!({"node_id": id}),
            Value::Object(_) => value.clone(),
            _ => return Err("A linked part needs node_id, with optional label, description and view".into()),
        };
        let keys = r.as_object().expect("object").keys().cloned().collect::<Vec<_>>();
        if keys.iter().any(|k| !["node_id", "label", "description", "view"].contains(&k.as_str())) {
            return Err("A linked part needs node_id, with optional label, description and view".into());
        }
        let id = r["node_id"].as_str().filter(|id| seen.insert(id.to_string())).ok_or("Linked part IDs must be unique strings")?.to_string();
        if node(doc, &id).is_none() && !retained.contains(id.as_str()) {
            return Err(format!("Linked part does not exist: {id}"));
        }
        for (key, limit) in [("label", 120), ("description", 1000)] {
            if let Some(v) = r.get(key) && !v.as_str().is_some_and(|s| s.chars().count() <= limit) {
                return Err(format!("Part {key} must be text of at most {limit} characters"));
            }
        }
        if let Some(view) = r.get("view").cloned() {
            r["view"] = crate::saved_views::validate_state(&view)?;
        }
        out.push(r);
    }
    Ok(out)
}

/// `evidence_reference`: a run id, optional signal, time range, script
/// location, physical hash and parts.
pub fn evidence_reference(value: &Value) -> Result<Value, String> {
    let o = value.as_object().ok_or("Invalid experiment evidence reference")?;
    if o.keys().any(|k| !["run_id", "signal", "time_range", "source", "physical_hash", "node_ids"].contains(&k.as_str())) {
        return Err("Invalid experiment evidence reference".into());
    }
    text(o.get("run_id").and_then(Value::as_str), "Run ID")?;
    for key in ["signal", "physical_hash"] {
        if let Some(v) = o.get(key) {
            text(v.as_str(), key)?;
        }
    }
    if let Some(ids) = o.get("node_ids") {
        for id in ids.as_array().ok_or("Evidence node_ids must be an array")? {
            text(id.as_str(), "CAD part ID")?;
        }
    }
    if let Some(t) = o.get("time_range") {
        let ok = t.as_array().filter(|a| a.len() == 2).and_then(|a| Some((a[0].as_f64()?, a[1].as_f64()?))).is_some_and(|(a, b)| a.is_finite() && b.is_finite() && 0. <= a && a <= b);
        if !ok {
            return Err("Evidence time range requires ordered nonnegative seconds".into());
        }
    }
    if let Some(s) = o.get("source") {
        let s = s.as_object().filter(|s| s.keys().all(|k| ["path", "line", "column"].contains(&k.as_str()))).ok_or("Invalid script location")?;
        text(s.get("path").and_then(Value::as_str), "Script path")?;
        for key in ["line", "column"] {
            if let Some(v) = s.get(key) && !v.as_u64().is_some_and(|n| n >= 1) {
                return Err("Script line and column must be positive integers".into());
            }
        }
    }
    Ok(value.clone())
}

/// Who wrote a comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorKind {
    Person,
    Agent,
}
impl AuthorKind {
    pub fn name(self) -> &'static str {
        match self {
            AuthorKind::Person => "person",
            AuthorKind::Agent => "agent",
        }
    }
    pub fn parse(v: Option<&str>) -> Result<AuthorKind, String> {
        match v {
            None | Some("person") => Ok(AuthorKind::Person),
            Some("agent") => Ok(AuthorKind::Agent),
            Some(other) => Err(format!("author_kind must be person or agent, not {other}")),
        }
    }
    fn comment_id(self) -> String {
        match self {
            AuthorKind::Person => new_id(),
            AuthorKind::Agent => format!("agent-{}", new_id()),
        }
    }
}

fn comment(body: String, author: String, kind: AuthorKind) -> Value {
    let ts = now_iso();
    json!({"id": kind.comment_id(), "author": author, "author_kind": kind.name(), "body": body, "created_at": ts, "updated_at": ts})
}

/// `anchor`: the part, the point (mm) and the part's geometry stamp; a face
/// index stores that face's description.
fn anchor(doc: &ArchiveDocument, stamps: &Stamps, node_id: &str, point: &Value, face: Option<i64>) -> Result<Value, String> {
    if node(doc, node_id).is_none() {
        return Err("annotation part does not exist".into());
    }
    let point = finite3(point).ok_or("anchor point must contain three finite millimetre coordinates")?;
    let mut out = json!({"node_id": node_id, "point": point, "geometry": current_stamps(doc, stamps, node_id).first()});
    if let Some(face) = face {
        let bytes = doc.entry(&format!("brep/{node_id}.brep")).ok_or("annotation face does not exist")?;
        let topo = crate::kernel::topology(bytes, &|| false)?;
        let f = usize::try_from(face).ok().and_then(|i| topo.faces.get(i)).ok_or("annotation face does not exist")?;
        out["face"] = serde_json::to_value(f).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// A new thread's fields (`create_thread`).
#[derive(Clone, Debug, Default)]
pub struct NewThread {
    pub node_id: Option<String>,
    pub point: Option<Value>,
    pub face: Option<i64>,
    pub body: String,
    pub author: String,
    pub author_kind: Option<String>,
    pub view: Option<Value>,
    pub evidence: Option<Value>,
    pub part_refs: Option<Value>,
    pub inspection_view: Option<Value>,
}

/// `create_thread`: the new thread's id and its first comment's id.
pub fn create(edit: &mut Edit, doc: &ArchiveDocument, stamps: &Stamps, t: NewThread) -> Result<(String, String), String> {
    let evidence = t.evidence.as_ref().filter(|e| !e.is_null()).map(evidence_reference).transpose()?;
    let anchor = match (&t.node_id, evidence.is_some()) {
        (Some(id), _) => anchor(doc, stamps, id, t.point.as_ref().unwrap_or(&Value::Null), t.face)?,
        (None, true) => json!({"node_id": null, "point": null, "geometry": null}),
        (None, false) => return Err("An annotation requires a part or experiment evidence".into()),
    };
    let body = text(Some(&t.body), "Comment")?;
    let author = text(Some(&t.author), "Author")?;
    let kind = AuthorKind::parse(t.author_kind.as_deref())?;
    let view = camera_view(t.view.as_ref())?;
    let first = comment(body, author, kind);
    let cid = first["id"].as_str().expect("id").to_string();
    let tid = new_id();
    let ts = first["created_at"].clone();
    let mut thread = json!({"id": tid, "anchor": anchor, "view": view, "status": "open", "created_at": ts, "updated_at": ts, "comments": [first]});
    if let Some(e) = evidence {
        thread["evidence"] = e;
    }
    if let Some(refs) = &t.part_refs {
        thread["part_refs"] = Value::Array(validate_part_refs(doc, refs, &[])?);
    }
    if let Some(v) = &t.inspection_view {
        thread["inspection_view"] = crate::saved_views::validate_state(v)?;
    }
    edit.object_mut("annotations").insert(tid.clone(), thread);
    Ok((tid, cid))
}

/// `update_thread`'s fields: only those given change.
#[derive(Clone, Debug, Default)]
pub struct ThreadPatch {
    pub status: Option<String>,
    pub node_id: Option<String>,
    pub point: Option<Value>,
    pub face: Option<i64>,
    pub view: Option<Value>,
    pub evidence: Option<Value>,
    pub part_refs: Option<Value>,
    pub inspection_view: Option<Value>,
}

fn thread_mut<'e>(edit: &'e mut Edit, id: &str) -> Result<&'e mut Value, String> {
    edit.object_mut("annotations").get_mut(id).ok_or_else(|| format!("annotation or comment not found: {id}"))
}

/// `update_thread`.
pub fn update(edit: &mut Edit, doc: &ArchiveDocument, stamps: &Stamps, id: &str, p: ThreadPatch) -> Result<(), String> {
    let mut t = thread_mut(edit, id)?.clone();
    if let Some(s) = &p.status {
        if !matches!(s.as_str(), "open" | "resolved") {
            return Err("status must be open or resolved".into());
        }
        t["status"] = json!(s);
    }
    if p.node_id.is_some() || p.point.is_some() {
        let node_id = p.node_id.clone().or_else(|| t["anchor"]["node_id"].as_str().map(str::to_string)).ok_or("annotation part does not exist")?;
        let point = p.point.clone().unwrap_or_else(|| t["anchor"]["point"].clone());
        t["anchor"] = anchor(doc, stamps, &node_id, &point, p.face)?;
    }
    if let Some(v) = &p.view {
        t["view"] = Value::Object(camera_view(Some(v))?);
    }
    if let Some(e) = &p.evidence {
        t["evidence"] = evidence_reference(e)?;
    }
    if let Some(refs) = &p.part_refs {
        let previous = thread_parts(&t);
        t["part_refs"] = Value::Array(validate_part_refs(doc, refs, &previous)?);
    }
    if let Some(v) = &p.inspection_view {
        t["inspection_view"] = crate::saved_views::validate_state(v)?;
    }
    t["updated_at"] = json!(now_iso());
    *thread_mut(edit, id)? = t;
    Ok(())
}

/// `delete_thread`.
pub fn delete(edit: &mut Edit, id: &str) -> Result<(), String> {
    edit.object_mut("annotations").remove(id).map(|_| ()).ok_or_else(|| format!("annotation or comment not found: {id}"))
}

/// `add_comment`: the new comment's id.
pub fn reply(edit: &mut Edit, thread: &str, body: &str, author: &str, kind: AuthorKind) -> Result<String, String> {
    let c = comment(text(Some(body), "Comment")?, text(Some(author), "Author")?, kind);
    let id = c["id"].as_str().expect("id").to_string();
    let ts = c["created_at"].clone();
    let t = thread_mut(edit, thread)?;
    t["comments"].as_array_mut().ok_or("annotation comments must be a list")?.push(c);
    t["updated_at"] = ts;
    Ok(id)
}

/// A reply with a given id (an agent's run id: posting the same run twice
/// is refused, so a reply is attached once).
pub fn reply_with_id(edit: &mut Edit, thread: &str, id: &str, body: &str, author: &str, kind: AuthorKind) -> Result<String, String> {
    let mut c = comment(text(Some(body), "Comment")?, text(Some(author), "Author")?, kind);
    c["id"] = json!(id);
    let ts = c["created_at"].clone();
    let t = thread_mut(edit, thread)?;
    let list = t["comments"].as_array_mut().ok_or("annotation comments must be a list")?;
    if list.iter().any(|x| x["id"] == id) {
        return Err(format!("comment {id} is already in the thread"));
    }
    list.push(c);
    t["updated_at"] = ts;
    Ok(id.to_string())
}

/// `update_comment` (Some body) or `delete_comment` (None): the thread's id.
pub fn change_comment(edit: &mut Edit, comment: &str, body: Option<&str>) -> Result<String, String> {
    let body = body.map(|b| text(Some(b), "Comment")).transpose()?;
    let ts = now_iso();
    for (tid, t) in edit.object_mut("annotations").iter_mut() {
        let Some(comments) = t["comments"].as_array_mut() else { continue };
        let Some(at) = comments.iter().position(|c| c["id"] == comment) else { continue };
        match &body {
            None if comments.len() == 1 => return Err("delete the thread to remove its last comment".into()),
            None => {
                comments.remove(at);
            }
            Some(b) => {
                comments[at]["body"] = json!(b);
                comments[at]["updated_at"] = json!(ts);
            }
        }
        t["updated_at"] = json!(ts);
        return Ok(tid.clone());
    }
    Err(format!("annotation or comment not found: {comment}"))
}

/// After a known rigid move of node `id` (`matrix`, `kernel::placement`):
/// its attached pins move with it and take the new stamp (RoboCAD's
/// `Ops.transform` keeps pins on a moved body the same way).
pub fn move_pins(edit: &mut Edit, doc: &ArchiveDocument, stamps: &Stamps, id: &str, matrix: &[f64; 12], new_stamp: &str) {
    for t in edit.object_mut("annotations").values_mut() {
        if t["anchor"]["node_id"] != id || anchor_status(doc, stamps, &t["anchor"]) != "attached" {
            continue;
        }
        if let Some(p) = finite3(&t["anchor"]["point"]) {
            t["anchor"]["point"] = json!(crate::kernel::apply(matrix, p));
        }
        t["anchor"]["geometry"] = json!(new_stamp);
        if let Some(a) = t["anchor"].as_object_mut() {
            a.remove("face");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_links_follow_robocads_pattern() {
        assert_eq!(part_links("see [the motor](part:ab12-x) and [bad](part:) [x](part:c_d)"), vec![("the motor".into(), "ab12-x".into()), ("x".into(), "c_d".into())]);
        assert!(part_links("[no\nlabel](part:a)").is_empty());
    }

    #[test]
    fn camera_views_are_checked_like_robocads() {
        assert!(camera_view(Some(&json!({"distance": 100, "fov": 40, "mode": "turntable"}))).is_ok());
        assert!(camera_view(Some(&json!({"fov": 200}))).is_err());
        assert!(camera_view(Some(&json!({"zoom": 1}))).is_err());
        assert!(camera_view(Some(&json!({"rot": [[1, 0, 0], [0, 1, 0], [0, 0, -1]]}))).is_err(), "a reflection is not a rotation");
    }
}
