//! The chat loop an agent runs over CAD comments: `get` (a thread with its
//! context), `watch` (wait for new comments), `seen` (per-reader unread
//! marks, viewer memory) and the `cad_threads` feed published for
//! `GET /v1/cad_threads` and `GET /v1/events/cad_threads`. Everything is
//! read from the open archive (`manifest.annotations`), so a reply posted in
//! the window, by REST or by another agent shows the same way.
use super::{ThreadsArgs, ThreadsState};
use crate::app::actions::Call;
use crate::cad::document::CadDocument;
use serde_json::{Value, json};
use sim_api::Outcome;
use std::time::{SystemTime, UNIX_EPOCH};

/// The default reader of `watch` and `seen`.
const READER: &str = "agent";

/// Who wrote a stored comment: its `author_kind`, else an `agent-` id is an
/// agent's (the shared panel's rule), else a person's.
pub(crate) fn author_kind(c: &Value) -> &'static str {
    match c["author_kind"].as_str() {
        Some("agent") => "agent",
        Some(_) => "person",
        None if c["id"].as_str().is_some_and(|id| id.starts_with("agent-")) => "agent",
        None => "person",
    }
}

/// Every comment of the open archive as `(thread, comment)`, oldest first.
fn comments(doc: &CadDocument) -> Vec<(&Value, &Value)> {
    let Some(local) = &doc.local else { return Vec::new() };
    let mut out: Vec<(&Value, &Value)> = local.archive.manifest["annotations"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .flat_map(|t| t["comments"].as_array().into_iter().flatten().map(move |c| (t, c)))
        .collect();
    out.sort_by(|a, b| a.1["created_at"].as_str().unwrap_or("").cmp(b.1["created_at"].as_str().unwrap_or("")));
    out
}

fn node_name(doc: &CadDocument, id: Option<&str>) -> String {
    match id {
        None => "Experiment evidence".into(),
        Some(id) => doc.local.as_ref().and_then(|l| l.archive.node(id)).and_then(|n| n["name"].as_str()).unwrap_or("Deleted part").to_string(),
    }
}

/// One comment as the feed and `watch` answer it.
fn entry(doc: &CadDocument, t: &Value, c: &Value) -> Value {
    json!({
        "thread": t["id"], "thread_status": t["status"], "node_id": t["anchor"]["node_id"],
        "node_name": node_name(doc, t["anchor"]["node_id"].as_str()),
        "comment": c["id"], "author": c["author"], "author_kind": author_kind(c),
        "body": c["body"], "created_at": c["created_at"],
    })
}

/// The comments newer than `since` (all with an empty one), by `kind` when given.
fn newer<'d>(doc: &'d CadDocument, since: &str, kind: Option<&str>) -> Vec<(&'d Value, &'d Value)> {
    comments(doc).into_iter().filter(|(_, c)| c["created_at"].as_str().unwrap_or("") > since && kind.is_none_or(|k| author_kind(c) == k)).collect()
}

fn newest(doc: &CadDocument) -> String {
    comments(doc).last().and_then(|(_, c)| c["created_at"].as_str()).unwrap_or("").to_string()
}

/// `reader`'s unread comments (written by someone of the other kind for the
/// default readers "agent" and "you"; any kind for another reader).
fn unread(doc: &CadDocument, st: &ThreadsState, reader: &str) -> usize {
    let since = st.seen.get(reader).map(String::as_str).unwrap_or("");
    let kind = match reader {
        READER => Some("person"),
        "you" => Some("agent"),
        _ => None,
    };
    newer(doc, since, kind).len()
}

/// The `cad_threads` feed: every thread's status and last comment, and
/// each reader's seen mark and unread count.
pub(crate) fn feed(doc: &CadDocument) -> Value {
    let Some(local) = &doc.local else { return json!({"open": false}) };
    let mut threads: Vec<Value> = local.archive.manifest["annotations"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .map(|t| {
            let list = t["comments"].as_array().cloned().unwrap_or_default();
            let last = list.last().map(|c| json!({"comment": c["id"], "author": c["author"], "author_kind": author_kind(c), "created_at": c["created_at"], "body": c["body"].as_str().map(|b| b.chars().take(280).collect::<String>())}));
            json!({
                "id": t["id"], "status": t["status"], "node_id": t["anchor"]["node_id"],
                "node_name": node_name(doc, t["anchor"]["node_id"].as_str()),
                "anchor_status": sim_cad::annotations::anchor_status(&local.archive, &local.pin_stamps, &t["anchor"]),
                "comments": list.len(), "last": last, "updated_at": t["updated_at"],
            })
        })
        .collect();
    threads.sort_by(|a, b| b["updated_at"].as_str().cmp(&a["updated_at"].as_str()));
    let st = &doc.threads;
    let mut readers: Vec<&str> = st.seen.keys().map(String::as_str).collect();
    for r in [READER, "you"] {
        if !readers.contains(&r) {
            readers.push(r);
        }
    }
    let readers: serde_json::Map<String, Value> = readers.into_iter().map(|r| (r.to_string(), json!({"seen": st.seen.get(r), "unread": unread(doc, st, r)}))).collect();
    json!({
        "open": true, "document": doc.document_name(), "revision": doc.shown_revision(),
        "newest": newest(doc), "threads": threads, "readers": readers,
        "how": "cad_threads {op: watch, reader, author_kind: person} waits for new comments; reply with {op: reply, thread, body, author, author_kind: agent}",
    })
}

/// `get`: one thread with its context.
pub(crate) fn get(doc: &CadDocument, id: &str) -> Result<Value, String> {
    let local = doc.local.as_ref().ok_or("no CAD document is open")?;
    let stored = local.archive.manifest["annotations"].get(id).ok_or_else(|| format!("no comment thread {id}"))?;
    let mut thread = sim_cad::annotations::detail(&local.archive, &local.pin_stamps, stored);
    if let Some(list) = thread["comments"].as_array_mut() {
        for c in list {
            c["author_kind"] = json!(author_kind(c));
        }
    }
    let node = thread["anchor"]["node_id"].as_str().map(str::to_string);
    let part = node.as_deref().and_then(|n| local.archive.node(n)).map(|n| {
        json!({
            "id": n["id"], "name": n["name"], "kind": n["kind"], "material": n["material"],
            "visible": local.archive.visible(n["id"].as_str().unwrap_or("")),
            "mass": local.masses.bodies.get(n["id"].as_str().unwrap_or("")).map(crate::cad::sync::value),
        })
    });
    let views: Vec<Value> = sim_cad::saved_views::list(&local.archive)
        .into_iter()
        .filter(|v| node.as_deref().is_some_and(|n| v["parts"].as_array().is_some_and(|p| p.iter().any(|x| x == n))))
        .map(|v| json!({"id": v["id"], "name": v["name"], "description": v["description"]}))
        .collect();
    Ok(json!({"thread": thread, "part": part, "views_of_part": views, "revision": doc.shown_revision()}))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// `watch` (see `ThreadsOp::Watch`).
pub(crate) fn watch(args: &ThreadsArgs, call: &mut Call, doc: &mut CadDocument) -> Outcome {
    let reader = args.reader.clone().unwrap_or_else(|| READER.into());
    if let Some(k) = args.author_kind.as_deref().filter(|k| !matches!(*k, "person" | "agent")) {
        return Outcome::Done(Err(format!("author_kind must be person or agent, not {k}")));
    }
    let (since, deadline) = match call.continuation.get("watch") {
        Some(w) => (w["since"].as_str().unwrap_or("").to_string(), w["deadline"].as_u64().unwrap_or(0)),
        None => {
            if doc.local.is_none() {
                return Outcome::Done(Err("no CAD document is open".into()));
            }
            let since = args.since.clone().or_else(|| doc.threads.seen.get(&reader).cloned()).unwrap_or_default();
            let timeout = args.timeout_s.unwrap_or(20.);
            if !(timeout.is_finite() && (0. ..=120.).contains(&timeout)) {
                return Outcome::Done(Err("timeout_s must be 0–120 seconds".into()));
            }
            (since, now_ms() + (timeout * 1000.) as u64)
        }
    };
    if call.cancelled {
        return Outcome::Done(Err("cancelled while watching for comments".into()));
    }
    let found: Vec<Value> = newer(doc, &since, args.author_kind.as_deref()).into_iter().map(|(t, c)| entry(doc, t, c)).collect();
    if found.is_empty() && now_ms() < deadline {
        *call.continuation = json!({"watch": {"since": since, "deadline": deadline}});
        return Outcome::Pending;
    }
    if let Some(latest) = found.last().and_then(|e| e["created_at"].as_str()) {
        doc.threads.seen.insert(reader.clone(), latest.to_string());
        doc.touch();
    }
    Outcome::Done(Ok(json!({
        "comments": found, "timed_out": found.is_empty(), "since": since,
        "reader": reader, "seen": doc.threads.seen.get(&reader), "revision": doc.shown_revision(),
    })))
}

/// `seen`: move `reader`'s mark.
pub(crate) fn seen(args: &ThreadsArgs, doc: &mut CadDocument) -> Result<Value, String> {
    doc.local.as_ref().ok_or("no CAD document is open")?;
    let reader = args.reader.clone().unwrap_or_else(|| READER.into());
    let at = args.since.clone().unwrap_or_else(|| newest(doc));
    doc.threads.seen.insert(reader.clone(), at.clone());
    doc.touch();
    Ok(json!({"reader": reader, "seen": at, "unread": unread(doc, &doc.threads, &reader)}))
}
