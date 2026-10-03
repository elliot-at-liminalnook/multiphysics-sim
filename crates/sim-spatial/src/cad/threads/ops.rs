//! `cad_threads`' one handler ([`handle`]) and its ops: each intent on
//! RoboCAD's threads, from a button, a key, `system_ui` or REST. A commit
//! goes through the annotations service (`annotations::apply` with
//! [`CadThreadSource`]) or, for a whole-thread change (a reattached pin, a
//! part's label), the source's own validate and commit; a REST caller
//! waits for the threads at RoboCAD's revision first (`read::wait`), a
//! window's commit on a list being read again is refused by name.
//! RoboCAD's dock behaviour is cited at each op (ui/comments.py).
use super::source::{self, CadThreadSource};
use super::{ADD, CadAnchor, DELETE_COMMENT, DELETE_THREAD, DRAFTING, EDIT_COMMENT, Field, Filter, LabelDialog, REPLY, ThreadsArgs, ThreadsOp, UPDATE, annotate, isolation, read, state_json};
use crate::annotations::{self, Committed, ThreadOp, ThreadSource};
use crate::app::actions::{Call, Origin};
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::CadDocument;
use crate::cad::selection::CadItems;
use serde_json::{Value, json};
use sim_annotate::{Thread, ThreadCommand};
use sim_api::Outcome;
use sim_runtime::cad_client::{AnchorStatus, CadThread, part_link};

fn done(r: Result<Value, String>) -> Outcome {
    Outcome::Done(r)
}

/// Something the dock shows changed (the panels refresh on the document's revision).
fn changed(doc: &mut CadDocument, value: Value) -> Outcome {
    doc.touch();
    done(Ok(value))
}

/// The thread an op is about: `thread`, else the dock's current one.
fn thread_arg(args: &ThreadsArgs, doc: &CadDocument) -> Result<String, String> {
    args.thread.clone().or_else(|| doc.threads.current.clone()).ok_or_else(|| "no comment thread is selected: open one (thread)".to_string())
}

/// The revision a commit about RoboCAD's threads goes out at: `revision`
/// (when the threads were read and hold `thread`), else the one the shown
/// threads were read at. A window's commit on a list being read again is
/// refused by name; a REST caller waits for it (also with `revision`, when
/// the list was never read or lacks `thread`).
fn began(args: &ThreadsArgs, doc: &mut CadDocument, call: &mut Call, thread: Option<&str>) -> Result<Option<u64>, Outcome> {
    let holds = |doc: &CadDocument| read::listed(doc).is_some() && thread.is_none_or(|t| read::thread(doc, t).is_some());
    if let Some(r) = args.revision
        && holds(&*doc)
    {
        return Ok(Some(r));
    }
    match read::wait(doc, call) {
        Err(e) => return Err(done(Err(e))),
        Ok(true) => return Err(Outcome::Pending),
        Ok(false) => {}
    }
    if !read::current(doc) {
        let now = doc.shown_revision();
        return Err(done(Err(match read::read_at(doc) {
            Some(at) => format!("RoboCAD's comments are being read again (they were read at revision {at}, the document is at {now}); nothing was sent: try again in a moment"),
            None => "RoboCAD's comments have not been read yet; nothing was sent: try again in a moment".to_string(),
        })));
    }
    if let Some(t) = thread.filter(|_| !holds(&*doc)) {
        return Err(done(Err(format!("no comment thread {t} in RoboCAD's comments as last read"))));
    }
    Ok(args.revision.or(read::read_at(doc)))
}

/// Apply one annotations op through RoboCAD's source; `compose`: the
/// composer's draft it sends (kept until RoboCAD takes it).
fn commit(cx: &mut Cx, call: &mut Call, began: Option<u64>, label: &str, op: ThreadOp<CadAnchor>, compose: Option<String>) -> Outcome {
    let mut source = CadThreadSource::new(cx.doc, call, began);
    let result = annotations::apply(&mut source, label, op);
    let outcome = source.outcome.take();
    drop(source);
    sent(cx.doc, result.map(|a| a.committed), outcome, compose)
}

/// Captured review uses the same annotation adapter, validation, edit and undo
/// path as surface comments. Evidence names captured time/source, not live pose.
pub(crate) fn annotate_evidence(cx: &mut Cx, call: &mut Call, evidence: serde_json::Value, body: String) -> Outcome {
    let parsed = serde_json::from_value::<sim_runtime::cad_client::threads::ExperimentEvidence>(evidence.clone());
    match parsed.map_err(|e| e.to_string()).and_then(|e| e.validate()) {
        Ok(()) => {},
        Err(e) => return done(Err(format!("evidence: {e}"))),
    }
    let revision = cx.doc.shown_revision();
    let author = cx.doc.threads.author.clone();
    let op = ThreadOp::Create {
        title: "Experiment evidence".into(), targets: vec![CadAnchor::Evidence { evidence }],
        body, author, links: Vec::new(), pin_m: None, view: None,
    };
    commit(cx, call, Some(revision), ADD, op, None)
}

/// Put a whole thread (a reattached pin, a part's label) as one RoboCAD call.
pub(super) fn put(cx: &mut Cx, call: &mut Call, began: Option<u64>, label: &str, thread: Thread<CadAnchor>) -> Outcome {
    let mut source = CadThreadSource::new(cx.doc, call, began);
    let result = source.validate(&thread).and_then(|()| source.commit(label, ThreadCommand::PutThread { thread }));
    let outcome = source.outcome.take();
    drop(source);
    sent(cx.doc, result, outcome, None)
}

fn sent(doc: &mut CadDocument, result: Result<Committed, String>, outcome: Option<Outcome>, compose: Option<String>) -> Outcome {
    match result {
        Err(e) => {
            if compose.is_some() {
                doc.threads.error = Some(super::plain(&e));
                doc.touch();
            }
            done(Err(e))
        }
        Ok(committed) => {
            if let (Some(body), Committed::Pending(seq)) = (compose, committed) {
                // The body sent: the draft ends when it lands only if it is still this text.
                doc.threads.sending = Some((seq, body));
                doc.threads.error = None;
            }
            outcome.unwrap_or_else(|| done(Ok(json!({"sent": true}))))
        }
    }
}

/// The thread as the annotations service sees it, or why not.
fn known(doc: &CadDocument, id: &str) -> Result<CadThread, String> {
    read::thread(doc, id).cloned().ok_or_else(|| format!("no comment thread {id} in RoboCAD's comments as last read"))
}

/// The thread holding message `comment`.
fn thread_of_comment(doc: &CadDocument, comment: &str) -> Result<String, String> {
    read::listed(doc).and_then(|l| l.iter().find(|t| t.comments.iter().any(|c| c.id == comment))).map(|t| t.id.clone()).ok_or_else(|| format!("annotation or comment not found: {comment}"))
}

/// The guard of the draft-disabled actions.
pub(super) fn not_drafting(doc: &CadDocument) -> Result<(), String> {
    if doc.threads.drafting() { Err(DRAFTING.to_string()) } else { Ok(()) }
}

/// RoboCAD's `select` rule (`open`): with a draft open, another thread is
/// not opened. Show on model, Fit in view and Show only linked parts go
/// through `select` in RoboCAD's `/threads/{id}/show` (api.py:408-409),
/// so a REST or window request about the draft's own thread is accepted.
fn not_drafting_other(doc: &CadDocument, thread: Option<&str>) -> Result<(), String> {
    let st = &doc.threads;
    match thread {
        Some(id) if st.drafting() && st.current.as_deref() != Some(id) => Err("Post or cancel your current draft before opening another thread".into()),
        _ => Ok(()),
    }
}

/// The dock's draft guard (RoboCAD's `update_send` disables the button),
/// for a window press only: RoboCAD's `PATCH /threads/{id}` takes a REST
/// caller's change whatever the window's draft.
fn ui_not_drafting(doc: &CadDocument, call: &Call) -> Result<(), String> {
    if matches!(call.origin, Origin::Ui) { not_drafting(doc) } else { Ok(()) }
}

/// The composer's post refused because what it is for is stale or gone
/// (`threads::draft_gone`): the reason under the composer and in the
/// answer, nothing sent, the draft kept. None: nothing to refuse.
fn refuse_draft(doc: &mut CadDocument) -> Option<Outcome> {
    let why = super::draft_gone(doc)?;
    doc.threads.error = Some(why.clone());
    doc.touch();
    Some(done(Err(why)))
}

/// `CadThreads`, from any entry point.
pub(in crate::cad) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    let CadAction::CadThreads(args) = action else { return done(Err("not a comment-threads action".into())) };
    match args.op {
        ThreadsOp::State => done(Ok(state_json(cx.doc))),
        ThreadsOp::List => match read::wait(cx.doc, call) {
            Err(e) => done(Err(e)),
            Ok(true) => Outcome::Pending,
            Ok(false) => done(Ok(state_json(cx.doc))),
        },
        ThreadsOp::Dock => {
            let open = args.open.unwrap_or(!cx.doc.threads.open);
            cx.doc.threads.open = open;
            changed(cx.doc, json!({"open": open}))
        }
        ThreadsOp::Annotate => annotate::start(cx, call, None),
        ThreadsOp::Place => annotate::place(args, call, cx),
        ThreadsOp::Cancel => done(Ok(annotate::cancel(cx.doc))),
        ThreadsOp::Discard => {
            cx.doc.threads.end_draft();
            // A post in flight no longer ends a draft when it lands.
            cx.doc.threads.sending = None;
            changed(cx.doc, json!({"draft": null}))
        }
        ThreadsOp::Filter => {
            if let Err(e) = not_drafting(cx.doc) {
                return done(Err(e));
            }
            let filter = args.filter.unwrap_or_default();
            cx.doc.threads.filter = filter;
            changed(cx.doc, json!({"filter": filter}))
        }
        ThreadsOp::SelectedOnly => {
            if let Err(e) = not_drafting(cx.doc) {
                return done(Err(e));
            }
            let on = args.on.unwrap_or(!cx.doc.threads.selected_only);
            cx.doc.threads.selected_only = on;
            changed(cx.doc, json!({"selected_only": on}))
        }
        ThreadsOp::Open => match thread_arg(args, cx.doc) {
            Ok(id) => done(open(cx.doc, &id)),
            Err(e) => done(Err(e)),
        },
        ThreadsOp::Create => create(args, call, cx),
        ThreadsOp::Reply => {
            let id = match thread_arg(args, cx.doc) {
                Ok(id) => id,
                Err(e) => return done(Err(e)),
            };
            // The composer's reply to a thread gone since it was opened: said under the composer.
            let st = &cx.doc.threads;
            let composer = st.pending.is_none() && st.editing.is_none() && st.current.as_deref() == Some(id.as_str()) && args.body.as_deref().is_none_or(|b| b == st.compose);
            if composer {
                if let Some(refused) = refuse_draft(cx.doc) {
                    return refused;
                }
            }
            let began = match began(args, cx.doc, call, Some(&id)) {
                Ok(b) => b,
                Err(o) => return o,
            };
            let st = &cx.doc.threads;
            let body = args.body.clone().unwrap_or_else(|| st.compose.clone());
            let compose = (body == st.compose).then(|| body.clone());
            let author = args.author.clone().unwrap_or_else(|| st.author.clone());
            commit(cx, call, began, REPLY, ThreadOp::Reply { thread: id, body, author, links: Vec::new() }, compose)
        }
        ThreadsOp::Edit => {
            let Some(comment) = args.comment.clone().or_else(|| cx.doc.threads.editing.clone()) else { return done(Err("edit needs comment: the message to change".into())) };
            // The composer's Save edit of a message gone since Edit message: said under the composer.
            if cx.doc.threads.editing.as_deref() == Some(comment.as_str()) {
                if let Some(refused) = refuse_draft(cx.doc) {
                    return refused;
                }
            }
            let began = match began(args, cx.doc, call, None) {
                Ok(b) => b,
                Err(o) => return o,
            };
            let thread = match thread_of_comment(cx.doc, &comment) {
                Ok(t) => t,
                Err(e) => return done(Err(e)),
            };
            let body = args.body.clone().unwrap_or_else(|| cx.doc.threads.compose.clone());
            let compose = (body == cx.doc.threads.compose).then(|| body.clone());
            commit(cx, call, began, EDIT_COMMENT, ThreadOp::EditComment { thread, comment, body, links: None }, compose)
        }
        ThreadsOp::EditMessage => done(edit_message(cx.doc, args.comment.as_deref())),
        ThreadsOp::Menu => {
            let Some(comment) = args.comment.clone() else { return done(Err("menu needs comment".into())) };
            let st = &mut cx.doc.threads;
            st.menu = if st.menu.as_deref() == Some(comment.as_str()) { None } else { Some(comment) };
            let menu = st.menu.clone();
            changed(cx.doc, json!({"menu": menu}))
        }
        ThreadsOp::Delete => {
            let Some(comment) = args.comment.clone() else { return done(Err("delete needs comment: the message to delete".into())) };
            if let Err(e) = not_drafting(cx.doc) {
                return done(Err(e));
            }
            let began = match began(args, cx.doc, call, None) {
                Ok(b) => b,
                Err(o) => return o,
            };
            let thread = match thread_of_comment(cx.doc, &comment) {
                Ok(t) => t,
                Err(e) => return done(Err(e)),
            };
            commit(cx, call, began, DELETE_COMMENT, ThreadOp::DeleteComment { thread, comment }, None)
        }
        ThreadsOp::DeleteThread => {
            let id = match thread_arg(args, cx.doc).and_then(|id| not_drafting(cx.doc).map(|()| id)) {
                Ok(id) => id,
                Err(e) => return done(Err(e)),
            };
            let began = match began(args, cx.doc, call, Some(&id)) {
                Ok(b) => b,
                Err(o) => return o,
            };
            commit(cx, call, began, DELETE_THREAD, ThreadOp::Delete { thread: id }, None)
        }
        ThreadsOp::Resolve => {
            // RoboCAD's dock disables Resolve while a draft is open (`update_send`); its PATCH route does not.
            let id = match thread_arg(args, cx.doc).and_then(|id| ui_not_drafting(cx.doc, call).map(|()| id)) {
                Ok(id) => id,
                Err(e) => return done(Err(e)),
            };
            let began = match began(args, cx.doc, call, Some(&id)) {
                Ok(b) => b,
                Err(o) => return o,
            };
            let t = match known(cx.doc, &id) {
                Ok(t) => t,
                Err(e) => return done(Err(e)),
            };
            let resolved = args.resolved.unwrap_or(!t.resolved());
            commit(cx, call, began, UPDATE, ThreadOp::Resolve { thread: t.id, resolved }, None)
        }
        ThreadsOp::Reattach => match (args.node.as_deref(), thread_arg(args, cx.doc)) {
            (_, Err(e)) => done(Err(e)),
            (None, Ok(id)) => annotate::start(cx, call, Some(id)),
            // RoboCAD's `begin` (the window's click) refuses a reattach while a
            // draft is open; its PATCH route does not.
            (Some(_), Ok(_)) if matches!(call.origin, Origin::Ui) && cx.doc.threads.drafting() => done(Err("Post or cancel your current draft before placing another pin".into())),
            (Some(_), Ok(id)) => annotate::reattach(args, &id, call, cx),
        },
        ThreadsOp::Link => link(args, call, cx),
        ThreadsOp::Part => {
            let Some(node) = args.node.clone() else { return done(Err("part needs node: a linked part".into())) };
            done(isolation::highlight(cx, call, &node))
        }
        ThreadsOp::Label => label(args, call, cx),
        ThreadsOp::LabelCancel => {
            cx.doc.threads.label = None;
            changed(cx.doc, json!({"label": null}))
        }
        // Show on model, Fit in view and Show only linked parts: refused only
        // for another thread than the draft's (`not_drafting_other`).
        ThreadsOp::Show => match thread_arg(args, cx.doc).and_then(|id| not_drafting_other(cx.doc, Some(&id)).map(|()| id)) {
            Ok(id) => done(isolation::show(cx, call, &id)),
            Err(e) => done(Err(e)),
        },
        ThreadsOp::Fit => match thread_arg(args, cx.doc).and_then(|id| not_drafting_other(cx.doc, Some(&id)).map(|()| id)) {
            Ok(id) => done(isolation::fit(cx, call, &id)),
            Err(e) => done(Err(e)),
        },
        ThreadsOp::ShowParts => {
            let thread = args.thread.clone().or_else(|| cx.doc.threads.current.clone());
            if let Err(e) = not_drafting_other(cx.doc, thread.as_deref()) {
                return done(Err(e));
            }
            done(isolation::view_parts(cx, call, thread.as_deref(), args.ids.as_deref(), true))
        }
        ThreadsOp::Return => done(isolation::end(cx, call)),
        ThreadsOp::PartLink => {
            let Some(node) = args.node.clone() else { return done(Err("part_link needs node: the part:ID of the link".into())) };
            let thread = args.thread.clone().or_else(|| cx.doc.threads.current.clone());
            done(isolation::part_link(cx, call, thread.as_deref(), &node))
        }
        ThreadsOp::InsertLink => done(insert_link(cx)),
        ThreadsOp::Draft => {
            let text = args.text.clone().unwrap_or_default();
            cx.doc.threads.compose = text.clone();
            cx.doc.threads.error = None;
            changed(cx.doc, json!({"draft": text}))
        }
        ThreadsOp::Author => {
            let text = args.text.clone().unwrap_or_default();
            cx.doc.threads.author = text.clone();
            changed(cx.doc, json!({"author": text}))
        }
    }
}

/// Open a thread (RoboCAD's `select`, comments.py:310-324): the dock shown,
/// the filter All, Selected parts only off, the thread current.
pub(crate) fn open(doc: &mut CadDocument, id: &str) -> Result<Value, String> {
    let st = &doc.threads;
    if st.drafting() && st.current.as_deref() != Some(id) {
        return Err("Post or cancel your current draft before opening another thread".into());
    }
    known(doc, id)?;
    let st = &mut doc.threads;
    st.open = true;
    st.filter = Filter::All;
    st.selected_only = false;
    if st.current.as_deref() != Some(id) {
        st.current = Some(id.to_string());
        st.part = None;
        st.menu = None;
        st.label = None;
    }
    doc.touch();
    Ok(json!({"thread": id}))
}

/// Post annotation: the placed pin (or REST's `node` and `point`).
fn create(args: &ThreadsArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    // The placed pin's face index is RoboCAD's numbering at the revision it
    // was clicked at: never sent once the shown document moved past it.
    if args.node.is_none() && super::stale_pin(cx.doc).is_some() {
        if let Some(refused) = refuse_draft(cx.doc) {
            return refused;
        }
    }
    let doc = &*cx.doc;
    let (node, point, face, view, began) = match (&args.node, &doc.threads.pending) {
        (Some(node), _) => {
            let Some(point) = args.point else { return done(Err("create needs point: [x, y, z] mm on the part's surface".into())) };
            if !doc.has_node(node) {
                return done(Err("annotation part does not exist".into()));
            }
            (node.clone(), point, args.face, args.view.clone().unwrap_or_default(), Some(args.revision.unwrap_or_else(|| doc.shown_revision())))
        }
        (None, Some(p)) => (p.node.clone(), p.point, p.face, p.view.clone(), Some(p.revision)),
        (None, None) => return done(Err("No pin is placed: Annotate model, then click a surface".into())),
    };
    let st = &doc.threads;
    let body = args.body.clone().unwrap_or_else(|| st.compose.clone());
    let compose = (args.node.is_none() && body == st.compose).then(|| body.clone());
    let author = args.author.clone().unwrap_or_else(|| st.author.clone());
    let name = doc.node_name(&node);
    let surface = CadAnchor::Surface { node_id: node, point, face: None, face_index: face, view, state: AnchorStatus::Attached, node_name: name.clone() };
    let op = ThreadOp::Create { title: name, targets: vec![surface], body, author, links: Vec::new(), pin_m: None, view: None };
    commit(cx, call, began, ADD, op, compose)
}

/// Edit message: the message into the composer (`edit_message`).
fn edit_message(doc: &mut CadDocument, comment: Option<&str>) -> Result<Value, String> {
    let comment = comment.ok_or("edit_message needs comment")?;
    not_drafting(doc)?;
    let thread = thread_of_comment(doc, comment)?;
    let body = read::thread(doc, &thread).and_then(|t| t.comments.iter().find(|c| c.id == comment)).map(|c| c.body.clone()).unwrap_or_default();
    let st = &mut doc.threads;
    st.current = Some(thread);
    st.pending = None;
    st.editing = Some(comment.to_string());
    st.compose = body;
    st.menu = None;
    st.claim = Some(Field::Compose);
    doc.touch();
    Ok(json!({"editing": comment}))
}

/// Link selected parts (`link_selection`): the selection's nodes (or
/// `ids`) not yet linked, added to the thread's linked parts.
fn link(args: &ThreadsArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let id = match thread_arg(args, cx.doc).and_then(|id| not_drafting(cx.doc).map(|()| id)) {
        Ok(id) => id,
        Err(e) => return done(Err(e)),
    };
    let began = match began(args, cx.doc, call, Some(&id)) {
        Ok(b) => b,
        Err(o) => return o,
    };
    let t = match known(cx.doc, &id) {
        Ok(t) => t,
        Err(e) => return done(Err(e)),
    };
    let ids = args.ids.clone().unwrap_or_else(|| cx.shared.items().nodes());
    if ids.is_empty() {
        return done(Err("Select parts in the outliner or viewport first".into()));
    }
    let linked: Vec<&str> = t.linked_parts.iter().map(|p| p.part.node_id.as_str()).collect();
    let new: Vec<CadAnchor> = ids.iter().filter(|id| !linked.contains(&id.as_str())).map(|id| CadAnchor::part(id, &cx.doc.node_name(id))).collect();
    if new.is_empty() {
        return done(Err("The selected parts are already linked to this discussion".into()));
    }
    commit(cx, call, began, UPDATE, ThreadOp::Link { thread: t.id, targets: new }, None)
}

/// Rename part label…: the dialog (no `label`), or the label saved.
fn label(args: &ThreadsArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let id = match thread_arg(args, cx.doc).and_then(|id| not_drafting(cx.doc).map(|()| id)) {
        Ok(id) => id,
        Err(e) => return done(Err(e)),
    };
    // Only a save is a commit; opening the dialog reads the list as it is.
    let began = match args.label.is_some().then(|| began(args, cx.doc, call, Some(&id))) {
        None => None,
        Some(Ok(b)) => b,
        Some(Err(o)) => return o,
    };
    let t = match known(cx.doc, &id) {
        Ok(t) => t,
        Err(e) => return done(Err(e)),
    };
    let Some(node) = args.node.clone().or_else(|| cx.doc.threads.part.clone()) else { return done(Err("Choose a part in Parts in this discussion first".into())) };
    let Some(part) = t.linked_parts.iter().find(|p| p.part.node_id == node) else { return done(Err(format!("{} is not a part of this discussion", cx.doc.node_name(&node)))) };
    let Some(text) = args.label.clone() else {
        let text = part.part.label.clone().unwrap_or_else(|| cx.doc.node_name(&node));
        cx.doc.threads.label = Some(LabelDialog { thread: t.id.clone(), node, text });
        cx.doc.threads.claim = Some(Field::Label);
        cx.doc.touch();
        return done(Ok(json!({"dialog": "Part label for this discussion"})));
    };
    let mut thread = source::thread_of(&t);
    for a in thread.targets.iter_mut().skip(1) {
        if let CadAnchor::Part { node_id, label, .. } = a
            && *node_id == node
        {
            *label = Some(text.clone());
        }
    }
    let outcome = put(cx, call, began, UPDATE, thread);
    if !matches!(outcome, Outcome::Done(Err(_))) {
        cx.doc.threads.label = None;
        cx.doc.touch();
    }
    outcome
}

/// Insert part link from selection (`insert_part_link`): a link per
/// selected node, labelled as the thread names it, else by the part's
/// name, appended to the composer.
fn insert_link(cx: &mut Cx) -> Result<Value, String> {
    let ids = cx.shared.items().nodes();
    if ids.is_empty() {
        return Err("Select a part in the outliner or viewport first".into());
    }
    let doc = &*cx.doc;
    let labels: Vec<(String, Option<String>)> = doc.threads.current.as_deref().and_then(|id| read::thread(doc, id)).map(|t| t.linked_parts.iter().map(|p| (p.part.node_id.clone(), p.part.label.clone())).collect()).unwrap_or_default();
    let links: Vec<String> = ids
        .iter()
        .filter(|id| doc.doc.as_ref().is_some_and(|d| d.nodes.iter().any(|n| n.id == **id)))
        .map(|id| {
            let label = labels.iter().find(|(n, _)| n == id).and_then(|(_, l)| l.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| doc.node_name(id));
            part_link(&label, id)
        })
        .collect();
    let text = links.join(", ");
    let st = &mut cx.doc.threads;
    st.compose.push_str(&text);
    st.claim = Some(Field::Compose);
    cx.doc.touch();
    Ok(json!({"inserted": text}))
}
