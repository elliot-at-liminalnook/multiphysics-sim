//! CAD mode's actions (native-viewer.md §3, §9 phase 1). Every intent in CAD
//! mode is a [`CadAction`]: the tree's rows, 3D picks, the panel's buttons,
//! keys (`keys`), `system_ui` and REST all write `Act<CadAction>`, and
//! [`apply`] (ViewerSet::Actions) is the one handler. Mutations go to
//! RoboCAD's command layer through its REST routes on jobs, so RoboCAD's
//! undo, provenance and `.rcad` file stay its own.
use crate::app::actions::{self, Spec, spec};
use crate::app::ViewerMode;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::path::PathBuf;

/// CAD mode's REST modes.
pub const CAD: &[ViewerMode] = &[ViewerMode::Cad];

/// Every intent of CAD mode. REST commands keep their JSON shape
/// (`{"command": name, ...args}`).
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum CadAction {
    /// `state` (as every mode answers it): the same as `cad_state`.
    State,
    /// The document as this window shows it: connection, service, document
    /// id and revision, dirty, stale, tree, selection, inspected node,
    /// history, commands, edit in flight.
    CadState,
    /// Open a `.rcad` file (start RoboCAD's headless service on it) or attach
    /// to a running RoboCAD at `url`. Exactly one of the two.
    CadOpen {
        #[serde(default)]
        path: Option<PathBuf>,
        #[serde(default)]
        url: Option<String>,
    },
    /// Replace the selection with `ids` (body items; empty clears it), or add
    /// them with `extend`. Pushed to RoboCAD's `/selection`.
    CadSelect {
        #[serde(default)]
        ids: Vec<String>,
        #[serde(default)]
        extend: bool,
    },
    /// `PATCH /nodes/{id}` with `attrs` as RoboCAD accepts them (name,
    /// visible, locked, disabled, material, color, pivot, transform, parent,
    /// index, tessellation_tolerance, plane, sketch): one undo step there.
    CadPatch { id: String, attrs: Map<String, Value> },
    /// `DELETE /nodes/{id}` (RoboCAD's delete command, undoable there).
    CadDelete { id: String },
    CadUndo,
    CadRedo,
    /// `POST /save` (RoboCAD writes its own file; `path` saves as).
    CadSave {
        #[serde(default)]
        path: Option<String>,
    },
    /// Run one of RoboCAD's GUI registry commands (`POST /commands/{id}`;
    /// RoboCAD's desktop window only).
    CadCommand { id: String },
    /// Call an `Ops` method (`POST /ops/{name}`): RoboCAD's command layer,
    /// headless or GUI.
    CadOp {
        name: String,
        #[serde(default)]
        args: Vec<Value>,
        #[serde(default)]
        kwargs: Map<String, Value>,
    },
    /// Refetch the document now.
    CadRefresh,
    /// Frame the native camera on everything, or on node `id` (display only:
    /// RoboCAD's own view is not changed).
    CadFit {
        #[serde(default)]
        id: Option<String>,
    },
    /// Fetch `GET /physical?flex=0` for the inspector's physical section
    /// (RoboCAD derives it; nothing is written).
    CadPhysical,
    /// `system_ui` in CAD mode: `{action: {operation: controls | activate, id?, ui_revision?}}`.
    SystemUi(Map<String, Value>),
}

impl actions::Action for CadAction {
    fn commands() -> Vec<Spec> {
        vec![
            spec("state", CAD, json!({}), "CAD mode: the same answer as cad_state, plus viewer_mode."),
            spec("cad_state", CAD, json!({}), "CAD mode: the RoboCAD document as this window shows it: target (file or service URL), service (self-started or attached; url, pid), connection (connecting | connected | lost with the error verbatim), health (RoboCAD's GET /: path, dirty, gui, nodes, document_id, revision), stale (why the shown tree may be behind RoboCAD, or null), nodes (id, kind, name, parent, depth, visible, effective_visible, locked, disabled), selection (node ids, synced with RoboCAD's /selection), selection_error (why the last read of RoboCAD's /selection failed, or null), unsaved (RoboCAD's dirty flag, or null when it can't be confirmed: not connected, or an edit in flight or just finished), inspected (RoboCAD's node detail for the first selected node, exactly as returned), physical (fetched with cad_physical), history (undo and redo labels), commands (RoboCAD's GUI registry; empty headless), autosave (GUI only), edit (the mutating request in flight, by name), meshes (shown, pending, failed) and status (the last outcome)."),
            spec("cad_open", CAD, json!({"path": "examples/camera-turntable/cad/turntable.rcad"}), "CAD mode: open a .rcad file (path: RoboCAD's headless service is started on it with cad/.venv/bin/python -m robocad.api PATH --port N, and stopped when the document closes) or attach to a running RoboCAD (url, loopback only, e.g. http://127.0.0.1:8420; never stopped). Refused, naming the reason, while an edit is in flight or while a self-started document has unsaved edits or edits whose saved state can't be confirmed (not connected to it; save first: the viewer never saves for you). Answers once the service is starting; poll cad_state for the connection."),
            spec("cad_select", CAD, json!({"ids": ["n1"]}), "CAD mode: select nodes by id (ids; [] clears; extend adds). The same action as a click on a tree row or a body in the 3D view. Pushed to RoboCAD's /selection as [[id, \"body\", 0], ...], so RoboCAD's window shows it too; RoboCAD's own selection changes come back on the next poll."),
            spec("cad_patch", CAD, json!({"id": "n1", "attrs": {"visible": false}}), "CAD mode: PATCH /nodes/{id} with attrs exactly as RoboCAD accepts them (name, visible, locked, disabled, material, color, pivot, transform, parent, index, tessellation_tolerance, plane, sketch): RoboCAD's command layer applies it as one undo step. One edit at a time: refused while another is in flight. Answers RoboCAD's node detail, or its error verbatim."),
            spec("cad_delete", CAD, json!({"id": "n1"}), "CAD mode: DELETE /nodes/{id} through RoboCAD's delete command (undoable there). Refused while another edit is in flight."),
            spec("cad_undo", CAD, json!({}), "CAD mode: POST /undo: RoboCAD undoes its last command; answers the label undone and the history."),
            spec("cad_redo", CAD, json!({}), "CAD mode: POST /redo: RoboCAD redoes; answers the label redone and the history."),
            spec("cad_save", CAD, json!({}), "CAD mode: POST /save: RoboCAD writes its document to its own path (or path, to save as). The viewer never writes the .rcad itself."),
            spec("cad_command", CAD, json!({"id": "view.fit"}), "CAD mode: run a RoboCAD GUI registry command by id (POST /commands/{id}; ids from cad_state.commands). RoboCAD's desktop window only: a headless service answers 409 \"no GUI\", shown verbatim."),
            spec("cad_op", CAD, json!({"name": "rename", "args": ["n1", "Bracket"]}), "CAD mode: call RoboCAD's command layer (POST /ops/{name} {args, kwargs}; names and signatures from RoboCAD's GET /ops). Works headless. Refused while another edit is in flight."),
            spec("cad_refresh", CAD, json!({}), "CAD mode: refetch RoboCAD's /doc, /commands and /autosave now."),
            spec("cad_fit", CAD, json!({}), "CAD mode: frame the native 3D view on every shown body, or on node id. Display only: RoboCAD's own view and the geometry are not changed."),
            spec("cad_physical", CAD, json!({}), "CAD mode: fetch RoboCAD's physical description (GET /physical?flex=0; nothing is written) for the inspector: the link holding the selected body, its mass, centre of mass and inertia, and mass_sources as RoboCAD labels them (declared measurement source, material density, ...), never filled in."),
            spec("system_ui", CAD, json!({"action": {"operation": "controls"}}), "CAD mode: its controls (cad:undo, cad:redo, cad:save, cad:refresh, cad:fit, cad:physical, cad:delete, cad:node:<id> to select a tree row, cad:visible:<id> to toggle visibility, cad:locked:<id> to toggle locked, cad:disabled:<id> to toggle disabled, cad:material:<id>:<mat> to assign material <mat> (an id from RoboCAD's /doc materials) to node <id>, cad:command:<id> for RoboCAD's registry commands), each with enabled and disabled_reason, then the mode switcher's mode:* controls; activate {id} writes the same CAD action a click does."),
        ]
    }
    fn controls() -> &'static [&'static str] {
        &["cad:undo", "cad:redo", "cad:save", "cad:refresh", "cad:fit", "cad:physical", "cad:delete", "cad:node:<id>", "cad:visible:<id>", "cad:locked:<id>", "cad:disabled:<id>", "cad:material:<id>:<mat>", "cad:command:<id>"]
    }
}

// ---- The one handler -------------------------------------------------------

use super::document::{CadDocument, CadTarget, Connection, EditDone};
use super::mesh::CadMeshes;
use super::sync::{self, value};
use crate::app::actions::{Act, Call, InFlight, Origin, Replies};
use crate::app::switch::Documents;
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use sim_api::Outcome;
use sim_runtime::cad_client::CadClient;
use std::collections::HashSet;

/// Actions: CAD mode's one apply system. A click's or key's refusal is the
/// header's status line; a REST caller gets it, or the answer. Mutations
/// run on an edit job; their REST callers wait for RoboCAD's answer
/// (Pending with the edit's sequence in the continuation).
pub(super) fn apply(
    mut messages: ResMut<Messages<Act<CadAction>>>,
    mut in_flight: ResMut<InFlight<CadAction>>,
    mut replies: ResMut<Replies>,
    doc: Option<ResMut<CadDocument>>,
    mut meshes: Option<ResMut<CadMeshes>>,
    mut documents: ResMut<Documents>,
) {
    let Some(mut doc) = doc else {
        actions::apply(&mut messages, &mut in_flight, &mut replies, |_, _| Outcome::Done(Err("CAD mode has no document open".into())));
        return;
    };
    actions::apply(&mut messages, &mut in_flight, &mut replies, |action, call| {
        let outcome = handle(action, call, &mut doc, meshes.as_deref_mut(), &mut documents);
        match call.origin {
            Origin::Rest(_) => outcome,
            origin => {
                if let (Outcome::Done(Err(e)), Origin::Ui) = (&outcome, origin) {
                    doc.show(Err(e.clone()));
                }
                Outcome::Done(Ok(Value::Null))
            }
        }
    });
}

/// One action, from any entry point.
fn handle(action: &CadAction, call: &mut Call, doc: &mut CadDocument, meshes: Option<&mut CadMeshes>, documents: &mut Documents) -> Outcome {
    // A REST caller waiting for the edit it started (also through system_ui activate).
    if let Some(seq) = call.continuation.get("edit").and_then(Value::as_u64) {
        return wait_edit(doc, call, seq);
    }
    let done = |r: Result<Value, String>| Outcome::Done(r);
    match action {
        CadAction::State | CadAction::CadState => done(Ok(state_json(doc, meshes.as_deref()))),
        CadAction::CadOpen { path, url } => done(open(doc, documents, path.as_ref(), url.as_deref())),
        CadAction::CadSelect { ids, extend } => done(select(doc, ids, *extend)),
        CadAction::CadPatch { id, attrs } => {
            if !doc.has_node(id) {
                return done(Err(format!("no node {id} in the shown tree")));
            }
            let name = doc.node_name(id);
            let keys = attrs.keys().cloned().collect::<Vec<_>>().join(", ");
            let (id, attrs) = (id.clone(), attrs.clone());
            let message = format!("Patched {name}: {keys}");
            edit(doc, call, format!("Patch {name}: {keys}"), move |c| c.patch(&id, &attrs).map(|d| EditDone { message, result: value(&d) }))
        }
        CadAction::CadDelete { id } => {
            if !doc.has_node(id) {
                return done(Err(format!("no node {id} in the shown tree")));
            }
            let name = doc.node_name(id);
            let id = id.clone();
            let message = format!("Deleted {name}");
            edit(doc, call, format!("Delete {name}"), move |c| c.delete(&id).map(|d| EditDone { message, result: value(&d) }))
        }
        CadAction::CadUndo => edit(doc, call, "Undo".into(), |c| {
            c.undo().map(|u| EditDone { message: u.undone.as_ref().map_or_else(|| "Nothing to undo".to_string(), |l| format!("Undid {l}")), result: value(&u) })
        }),
        CadAction::CadRedo => edit(doc, call, "Redo".into(), |c| {
            c.redo().map(|r| EditDone { message: r.redone.as_ref().map_or_else(|| "Nothing to redo".to_string(), |l| format!("Redid {l}")), result: value(&r) })
        }),
        CadAction::CadSave { path } => {
            let label = match path {
                Some(p) => format!("Save as {p}"),
                None => "Save".to_string(),
            };
            let path = path.clone();
            edit(doc, call, label, move |c| c.save(path.as_deref()).map(|s| EditDone { message: format!("Saved {}", s.saved), result: value(&s) }))
        }
        CadAction::CadCommand { id } => {
            let id = id.clone();
            edit(doc, call, format!("Command {id}"), move |c| c.run_command(&id).map(|r| EditDone { message: format!("Ran RoboCAD command {}", r.ran), result: value(&r) }))
        }
        CadAction::CadOp { name, args, kwargs } => {
            let (name, args, kwargs) = (name.clone(), args.clone(), kwargs.clone());
            edit(doc, call, format!("Op {name}"), move |c| c.op(&name, &args, &kwargs).map(|r| EditDone { message: format!("Ran {name}"), result: value(&r) }))
        }
        CadAction::CadRefresh => done(Ok(refresh(doc))),
        CadAction::CadFit { id } => done(fit(doc, meshes, id.as_deref())),
        CadAction::CadPhysical => done(sync::fetch_physical(doc).map(|()| json!({"message": "Fetching RoboCAD's physical description (GET /physical?flex=0); it shows in cad_state.physical."}))),
        CadAction::SystemUi(args) => system_ui(doc, call, meshes, documents, args),
    }
}

/// Start a mutating request; a REST caller waits for its answer.
fn edit(doc: &mut CadDocument, call: &mut Call, label: String, work: impl FnOnce(&CadClient) -> Result<EditDone, sim_runtime::cad_client::CadError> + Send + 'static) -> Outcome {
    match sync::start_edit(doc, label, call.rest(), work) {
        Err(e) => Outcome::Done(Err(e)),
        Ok(seq) if call.rest() => {
            *call.continuation = json!({"edit": seq, "generation": doc.generation});
            Outcome::Pending
        }
        Ok(_) => Outcome::Done(Ok(Value::Null)),
    }
}

/// A REST caller's edit: RoboCAD's answer (or error) verbatim once it lands.
/// A cancel does not abort it: the request was already sent.
fn wait_edit(doc: &mut CadDocument, call: &mut Call, seq: u64) -> Outcome {
    if call.continuation.get("generation").and_then(Value::as_u64) != Some(doc.generation) {
        return Outcome::Done(Err("the CAD document was replaced or reconnected while this edit waited; the request had already been sent to RoboCAD: see cad_state for the document as it is now".into()));
    }
    if let Some(result) = doc.edit_results.remove(&seq) {
        return Outcome::Done(result);
    }
    let running = doc.edit.is_some() && doc.edit_seq == seq;
    if call.cancelled {
        if running {
            doc.edit_waited = false;
        }
        return Outcome::Done(Err("cancelled waiting, but the request was already sent to RoboCAD and is not aborted: its outcome shows in cad_state.status".into()));
    }
    if !running {
        return Outcome::Done(Err("the edit ended without an answer for this request; see cad_state.status".into()));
    }
    Outcome::Pending
}

/// `cad_open`: replace the document (refused on an edit in flight or a
/// self-started document's unsaved edits, or edits it cannot confirm saved).
fn open(doc: &mut CadDocument, documents: &mut Documents, path: Option<&PathBuf>, url: Option<&str>) -> Result<Value, String> {
    let target = match (path, url) {
        (Some(p), None) => {
            if !p.to_string_lossy().ends_with(".rcad") {
                return Err(format!("{}: cad_open opens a *.rcad file", p.display()));
            }
            // Known cost: one stat on the UI thread, as the other modes'
            // opens do (the switch's `prepare` too); the file is read by
            // RoboCAD's service, never here.
            if !p.is_file() {
                return Err(format!("{}: no such file", p.display()));
            }
            CadTarget::File(p.clone())
        }
        (None, Some(u)) => {
            CadClient::new(u).map_err(|e| e.to_string())?;
            CadTarget::Service(u.to_string())
        }
        _ => return Err("cad_open takes path (a *.rcad file) or url (a loopback RoboCAD service), exactly one".into()),
    };
    let blockers = doc.switch_blockers();
    if !blockers.is_empty() {
        return Err(format!("Not opening {}: {}", target.describe(), blockers.join("; ")));
    }
    let note = doc.leaving_note();
    // The old document's self-started service is stopped now (also one
    // still starting), or left running if edits appeared since the check.
    let left_running = doc.release_child("opening another CAD document");
    let mut next = CadDocument::new(target.clone());
    next.revision = doc.revision + 1;
    sync::start(&mut next);
    let old = std::mem::replace(doc, next);
    // Its poll joins off the UI thread.
    crate::jobs::drop_off_thread(old, "the previous CAD document");
    documents.cad = Some(target.clone());
    let mut message = format!("Opening {}", target.describe());
    if let Some(note) = &note {
        message.push_str(&format!("; {note}"));
    }
    if let Some(url) = &left_running {
        message.push_str(&format!("; the RoboCAD service the previous document started may hold unsaved edits and is left running at {url} (open it there and save, or stop it)"));
    }
    doc.show(Ok(message.clone()));
    Ok(json!({"opened": target.json(), "message": message, "generation": doc.generation}))
}

/// `cad_select`: the selection now, then pushed to RoboCAD's `/selection`.
fn select(doc: &mut CadDocument, ids: &[String], extend: bool) -> Result<Value, String> {
    if let Some(id) = ids.iter().find(|id| !doc.has_node(id)) {
        return Err(format!("no node {id} in the shown tree"));
    }
    let mut selection = if extend { doc.selection.clone() } else { Vec::new() };
    for id in ids {
        if !selection.contains(id) {
            selection.push(id.clone());
        }
    }
    if selection != doc.selection {
        doc.selection = selection;
        doc.touch();
    }
    let pushed = doc.connected() && doc.selection != doc.remote_selection;
    if pushed {
        sync::push_selection(doc);
    }
    Ok(json!({"selection": doc.selection, "pushed": pushed, "connected": doc.connected()}))
}

/// `cad_refresh`: refetch now; reconnect when the connection is gone (a
/// self-started service that exited is started again).
fn refresh(doc: &mut CadDocument) -> Value {
    if doc.connect.is_some() {
        return json!({"message": "Already connecting."});
    }
    if doc.client.is_none() || doc.child_exit.is_some() {
        sync::start(doc);
        doc.show(Ok(format!("Reconnecting: {}", doc.target.describe())));
        return json!({"message": "Reconnecting.", "generation": doc.generation});
    }
    // Bodies whose mesh failed are fetched again (`mesh::sync`).
    doc.mesh_retry += 1;
    sync::refresh(doc);
    json!({"message": "Refetching RoboCAD's document."})
}

/// `cad_fit`: frame every drawn body, or node `id` and its descendants.
fn fit(doc: &CadDocument, meshes: Option<&mut CadMeshes>, id: Option<&str>) -> Result<Value, String> {
    let meshes = meshes.ok_or("CAD mode's 3D view is not available in this window")?;
    let ids = match id {
        None => None,
        Some(id) => {
            if !doc.has_node(id) {
                return Err(format!("no node {id} in the shown tree"));
            }
            Some(descendants(doc, id))
        }
    };
    let bounds = meshes.bounds(ids.as_ref()).ok_or_else(|| match id {
        Some(id) => format!("nothing to frame: no body of {} is drawn", doc.node_name(id)),
        None => "nothing to frame: no bodies are drawn yet".to_string(),
    })?;
    meshes.frame(bounds);
    Ok(json!({"framed": id.map_or_else(|| "every drawn body".to_string(), |id| doc.node_name(id)), "note": "display only: RoboCAD's view and the geometry are unchanged"}))
}

/// Node `id` and everything under it in the shown tree.
fn descendants(doc: &CadDocument, id: &str) -> HashSet<String> {
    let mut out: HashSet<String> = HashSet::from([id.to_string()]);
    let Some(state) = &doc.doc else { return out };
    // Walk order lists parents before children.
    for n in &state.nodes {
        if n.parent.as_ref().is_some_and(|p| out.contains(p)) {
            out.insert(n.id.clone());
        }
    }
    out
}

/// CAD mode's `system_ui` controls: the panel's own list (`panel::controls`),
/// so a control's label, enabled state and action are the button's.
fn controls(doc: &CadDocument) -> Vec<(String, String, CadAction, Result<(), String>)> {
    super::panel::controls(doc).into_iter().map(|c| (c.id, c.label, c.action, c.ready)).collect()
}

/// An action as its REST command (what `system_ui` lists as a control's action).
pub(super) fn rest_form(action: &CadAction) -> Value {
    match action {
        CadAction::CadUndo => json!({"command": "cad_undo"}),
        CadAction::CadRedo => json!({"command": "cad_redo"}),
        CadAction::CadSave { path } => json!({"command": "cad_save", "path": path}),
        CadAction::CadRefresh => json!({"command": "cad_refresh"}),
        CadAction::CadFit { id } => json!({"command": "cad_fit", "id": id}),
        CadAction::CadPhysical => json!({"command": "cad_physical"}),
        CadAction::CadDelete { id } => json!({"command": "cad_delete", "id": id}),
        CadAction::CadSelect { ids, extend } => json!({"command": "cad_select", "ids": ids, "extend": extend}),
        CadAction::CadPatch { id, attrs } => json!({"command": "cad_patch", "id": id, "attrs": attrs}),
        CadAction::CadCommand { id } => json!({"command": "cad_command", "id": id}),
        CadAction::CadOp { name, args, kwargs } => json!({"command": "cad_op", "name": name, "args": args, "kwargs": kwargs}),
        CadAction::CadOpen { path, url } => json!({"command": "cad_open", "path": path, "url": url}),
        CadAction::State => json!({"command": "state"}),
        CadAction::CadState => json!({"command": "cad_state"}),
        CadAction::SystemUi(args) => json!({"command": "system_ui", "action": args.get("action")}),
    }
}

/// `system_ui`: the controls, or one activated through this handler (the
/// same action a click writes). Ids are stable names, so `ui_revision` is
/// reported but not required.
fn system_ui(doc: &mut CadDocument, call: &mut Call, meshes: Option<&mut CadMeshes>, documents: &mut Documents, args: &Map<String, Value>) -> Outcome {
    let action = args.get("action").cloned().unwrap_or(Value::Null);
    match action["operation"].as_str() {
        Some("controls") => {
            let items: Vec<Value> = controls(doc)
                .into_iter()
                .map(|(id, label, action, ready)| json!({"id": id, "label": label, "enabled": ready.is_ok(), "disabled_reason": ready.err(), "action": rest_form(&action)}))
                .collect();
            Outcome::Done(Ok(json!({"ui_revision": doc.revision, "ready": true, "controls": items, "state": state_json(doc, meshes.as_deref())})))
        }
        Some("activate") => {
            let Some(id) = action["id"].as_str() else { return Outcome::Done(Err("system_ui activate needs an id; request controls".into())) };
            let found = controls(doc).into_iter().find(|(i, ..)| i == id);
            match found {
                None => Outcome::Done(Err(format!("unknown control {id}; request controls"))),
                Some((id, _, _, Err(why))) => Outcome::Done(Err(format!("{id} is disabled: {why}"))),
                Some((_, _, action, Ok(()))) => handle(&action, call, doc, meshes, documents),
            }
        }
        _ => Outcome::Done(Err("system_ui in CAD mode: operation controls, or activate with a control id (cad:* or mode:*)".into())),
    }
}

/// `cad_state`: the document as this window shows it. Nothing is invented:
/// absent values are null.
pub(super) fn state_json(doc: &CadDocument, meshes: Option<&CadMeshes>) -> Value {
    let connection = match &doc.connection {
        Connection::Connecting { what, since } => json!({"state": "connecting", "what": what, "seconds": since.elapsed().as_secs()}),
        Connection::Connected => json!({"state": "connected"}),
        Connection::Lost { error, since } => json!({"state": "lost", "error": error, "seconds": since.elapsed().as_secs()}),
    };
    let nodes: Vec<Value> = match &doc.doc {
        Some(state) => state
            .nodes
            .iter()
            .zip(doc.rows())
            .map(|(n, row)| json!({"id": n.id, "kind": n.kind, "name": n.name, "parent": n.parent, "depth": row.depth, "visible": n.visible, "effective_visible": n.effective_visible, "locked": n.locked, "disabled": n.disabled}))
            .collect(),
        None => Vec::new(),
    };
    let result = |r: &Result<Value, String>| match r {
        Ok(v) => json!({"ok": true, "value": v}),
        Err(e) => json!({"ok": false, "error": e}),
    };
    let meshes = meshes.map(|m| {
        json!({"shown": m.counts.shown, "pending": m.counts.pending, "no_mesh": m.counts.no_mesh,
            "failed": m.counts.failed.iter().map(|(id, e)| json!({"id": id, "error": e})).collect::<Vec<_>>()})
    });
    json!({
        "target": doc.target.json(),
        "document": doc.document_name(),
        "service": {
            "kind": if doc.child.is_some() || matches!(doc.target, CadTarget::File(_)) { "self-started" } else { "attached" },
            "url": doc.url(),
            "pid": doc.child.pid(),
            "exited": doc.child_exit,
            "line": doc.service_line(),
        },
        "connection": connection,
        "health": doc.health.as_ref().map(value),
        "unsaved": doc.unsaved(),
        "document_key": doc.doc_key.as_ref().map(|(id, revision)| json!({"document_id": id, "revision": revision})),
        "stale": doc.stale,
        "nodes": nodes,
        "selection": doc.selection,
        "selection_error": doc.selection_error,
        "inspected": doc.detail.as_ref().map(|(id, revision, r)| json!({"id": id, "revision": revision, "detail": result(&r.as_ref().map(value).map_err(Clone::clone))})),
        "physical": doc.physical.as_ref().map(|(revision, r)| json!({"revision": revision, "result": result(r)})).unwrap_or(Value::Null),
        "physical_pending": doc.physical_job.is_some(),
        "history": doc.doc.as_ref().map(|d| value(&d.history)),
        "commands": doc.commands.as_ref().map(|c| result(&c.as_ref().map(value).map_err(Clone::clone))),
        "autosave": doc.autosave.as_ref().map(|a| result(&a.as_ref().map(value).map_err(Clone::clone))),
        "edit": doc.edit.as_ref().map(|e| json!({"label": e.label, "seconds": e.started.elapsed().as_secs()})),
        "meshes": meshes,
        "status": doc.status.as_ref().map(|s| match s { Ok(t) => json!({"ok": true, "text": t}), Err(e) => json!({"ok": false, "text": e}) }),
        "revision": doc.revision,
        "generation": doc.generation,
    })
}

/// Present: `/v1/state` (with `viewer_mode`) and `/v1/cad_state`, at most every 100 ms.
pub(super) fn publish(rest: Option<ResMut<crate::rest::Rest>>, doc: Option<Res<CadDocument>>, meshes: Option<Res<CadMeshes>>) {
    let (Some(mut rest), Some(doc)) = (rest, doc) else { return };
    if rest.0.snapshot_due() {
        let state = state_json(&doc, meshes.as_deref());
        let mut shown = state.clone();
        shown["viewer_mode"] = json!(ViewerMode::Cad.name());
        rest.0.publish("cad_state", state);
        rest.0.publish("state", shown);
    }
}
