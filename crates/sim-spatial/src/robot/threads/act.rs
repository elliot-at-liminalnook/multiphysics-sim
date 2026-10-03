//! Robot mode's thread acts (`RobotAction::Threads`): the panel's presses,
//! the composer and REST `robot_threads`, applied by [`handle`] from robot
//! mode's one apply system (RobotSet::Actions).
use super::*;
use super::super::picked;
use crate::annotations::{self, ThreadOp};
use crate::app::actions::{Act, Call, Origin};
use crate::app::switch::sources::{cad_target, document_source};
use crate::app::switch::{Document, ModeSwitch, WindowAction};
use crate::cad::CadTarget;
use crate::cad::threads::{DRAFTING, Reveal, RevealThread};
use crate::selection::Selection;
use serde::Serialize;
use sim_api::Outcome;

/// What a REST caller of a change waits on (its sequence).
const WAIT_COMMIT: &str = "robot_threads_wait";
/// What a REST `state` or `refresh` waits on (the read at the current key).
const WAIT_READ: &str = "robot_threads_read";

/// Every thread intent of Robot mode (`RobotAction::Threads`): the panel's
/// presses, the composer, and REST `robot_threads`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(crate) enum ThreadsAct {
    /// The threads as `robot_state.cad_threads` shows them.
    State,
    /// Read RoboCAD's threads again now.
    Refresh,
    Filter { shown: Shown },
    /// Show a thread; its link is selected.
    Open { thread: String },
    /// Back to the list.
    Close,
    /// A message's menu (Edit, Delete) shown or hidden.
    Menu { comment: String },
    /// A message's text into the composer to edit it.
    EditMessage { comment: String },
    /// The composer's Cancel.
    Discard,
    /// The composer's Reply or Save edit (its draft).
    Post,
    Reply { thread: Option<String>, body: String, author: Option<String> },
    Edit { thread: Option<String>, comment: String, body: String },
    Delete { thread: Option<String>, comment: String },
    /// Resolve or reopen (`resolved`; absent toggles).
    Resolve { thread: Option<String>, resolved: Option<bool> },
    /// Switch to CAD mode on the CAD source, revealing `thread` (default the shown one).
    OpenInCad { thread: Option<String> },
    /// A part chip or part link in the section selects the link it is on
    /// (`picked::select`, as `RobotAction::SelectLink`, keeping the
    /// inspector's scroll so the section stays where it was read).
    SelectLink { index: usize, name: String },
}

/// Open in CAD before the attached service's answer is known.
pub(crate) const READING: &str = "Reading the CAD source's comments… try again";

/// REST `robot_threads`'s arguments as an act (errors name the op and the argument).
pub(crate) fn from_rest(op: Option<&str>, thread: Option<String>, comment: Option<String>, body: Option<String>, author: Option<String>, resolved: Option<bool>) -> Result<ThreadsAct, String> {
    let name = op.unwrap_or("state");
    let given = [("thread", thread.is_some()), ("comment", comment.is_some()), ("body", body.is_some()), ("author", author.is_some()), ("resolved", resolved.is_some())];
    let only = |allowed: &[&str]| -> Result<(), String> {
        match given.iter().find(|(arg, on)| *on && !allowed.contains(arg)) {
            Some((arg, _)) => Err(format!("robot_threads op `{name}` takes no {arg}")),
            None => Ok(()),
        }
    };
    let need = |value: Option<String>, arg: &str| value.ok_or_else(|| format!("robot_threads op `{name}` needs {arg}"));
    Ok(match name {
        "state" => {
            only(&[])?;
            ThreadsAct::State
        }
        "refresh" => {
            only(&[])?;
            ThreadsAct::Refresh
        }
        "open" => {
            only(&["thread"])?;
            ThreadsAct::Open { thread: need(thread, "thread")? }
        }
        "reply" => {
            only(&["thread", "body", "author"])?;
            ThreadsAct::Reply { thread, body: need(body, "body")?, author }
        }
        "edit" => {
            only(&["thread", "comment", "body"])?;
            ThreadsAct::Edit { thread, comment: need(comment, "comment")?, body: need(body, "body")? }
        }
        "delete" => {
            only(&["thread", "comment"])?;
            ThreadsAct::Delete { thread, comment: need(comment, "comment")? }
        }
        "resolve" => {
            only(&["thread", "resolved"])?;
            ThreadsAct::Resolve { thread, resolved }
        }
        "open_in_cad" => {
            only(&["thread"])?;
            ThreadsAct::OpenInCad { thread }
        }
        other => return Err(format!("unknown robot_threads op `{other}`; valid ops: state, open, reply, edit, delete, resolve, open_in_cad, refresh")),
    })
}

/// The CAD document Open in CAD switches to: the attached service when it
/// has the CAD source open (`attached`), else the CAD file.
pub(crate) fn cad_document(attached: Option<&str>, cad: &Path) -> Document {
    match attached {
        Some(url) => Document::Url(url.to_string()),
        None => Document::Path(cad.to_path_buf()),
    }
}

/// The CAD target that switch makes (`app::switch` builds `CadTarget::Service(url)`
/// from a URL and `CadTarget::File(path)` from a path, unchanged), so CAD
/// mode's reveal finds its document.
pub(crate) fn reveal_target(document: &Document) -> Option<CadTarget> {
    cad_target(&document_source(document))
}

/// Open in CAD: the switch to CAD mode on the CAD source, with `thread` to
/// reveal there once CAD mode has read its threads. The switch is applied
/// later (`app::switch::handle`, this frame or the next); if it is refused
/// the reveal is dropped there (`drop_reveal`), so none stays pending.
fn open_in_cad(st: &RobotThreads, view: &RobotView, registry: &DocumentRegistry, reveal: Option<&mut RevealThread>, window: &mut MessageWriter<Act<WindowAction>>, thread: Option<String>) -> Result<(), String> {
    // The draft would be lost with the switch.
    if st.drafting() {
        return Err(DRAFTING.into());
    }
    let status = view.cad_link.as_ref().ok_or("the robot has not loaded: its CAD source is not known yet")?;
    let cad = cad_path(status).ok_or_else(|| format!("{}: there is no CAD file to open", source_line(status)))?;
    let attached = attached_document(st, base_of(view, registry).as_ref())?;
    let document = cad_document(attached.as_deref(), cad);
    let target = reveal_target(&document).ok_or("no CAD target for this document")?;
    let reveal = reveal.ok_or("CAD mode is not part of this window")?;
    // No thread: no reveal (an earlier request for another thread goes).
    reveal.0 = thread.or_else(|| st.current.clone()).map(|thread| Reveal::new(target, thread));
    window.write(Act { action: WindowAction::Switch(ModeSwitch { mode: ViewerMode::Cad, document: Some(document) }), origin: Origin::Ui });
    Ok(())
}

/// The attached service's URL when Open in CAD is to attach to it (it has
/// the CAD source open), None when the CAD file is to be opened (no
/// attached service, or it answered with another document or not at all:
/// a closed RoboCAD remembered by URL), or why it cannot be decided yet.
fn attached_document(st: &RobotThreads, base: Option<&Base>) -> Result<Option<String>, String> {
    let Some(base) = base else { return Ok(None) };
    match st.read.reach(base) {
        None => Err(READING.into()),
        Some(read::Reach::Open) if st.read.listed_for(base).is_some() => Ok(Some(base.url.clone())),
        Some(read::Reach::Open) => Err(READING.into()),
        Some(read::Reach::Elsewhere(_) | read::Reach::Failed(_)) => Ok(None),
    }
}

/// One change through the annotations service (None: nothing pending).
fn commit(st: &mut RobotThreads, view: &RobotView, registry: &DocumentRegistry, op: ThreadOp<CadAnchor>) -> Result<Option<u64>, String> {
    let file = source_file(view.cad_link.as_ref());
    let mut source = RobotCadThreads { st, base: base_of(view, registry), file };
    Ok(match annotations::apply(&mut source, "", op)?.committed {
        Committed::Pending(seq) => Some(seq),
        Committed::Done => None,
    })
}

/// The thread a message is in (`thread` when given).
fn owner(st: &RobotThreads, base: Option<&Base>, thread: Option<&String>, comment: &str) -> Result<String, String> {
    if let Some(t) = thread {
        return Ok(t.clone());
    }
    st.open_listed(base)
        .and_then(|l| l.threads.iter().find(|t| t.comments.iter().any(|c| c.id == comment)))
        .map(|t| t.id.clone())
        .ok_or_else(|| format!("annotation or comment not found: {comment}"))
}

/// One act; `Ok(Some(seq))` when a change was sent.
fn act_on(act: &ThreadsAct, st: &mut RobotThreads, view: &RobotView, registry: &DocumentRegistry, selection: &mut Selection, reveal: Option<&mut RevealThread>, window: &mut MessageWriter<Act<WindowAction>>) -> Result<Option<u64>, String> {
    let base = base_of(view, registry);
    match act {
        ThreadsAct::State => Ok(None),
        ThreadsAct::Refresh => {
            st.read.again();
            st.error = None;
            Ok(None)
        }
        ThreadsAct::Filter { shown } => {
            st.filter = *shown;
            Ok(None)
        }
        ThreadsAct::Open { thread } => {
            if st.drafting() && st.current.as_deref() != Some(thread.as_str()) {
                return Err(DRAFTING.into());
            }
            let links = LinkKeys::of(view);
            let listed = st.open_listed(base.as_ref()).ok_or_else(|| not_open(&source_file(view.cad_link.as_ref())))?;
            let t = listed.threads.iter().find(|t| t.id == *thread).ok_or_else(|| format!("no comment thread {thread} in RoboCAD's comments as last read"))?;
            let link = place(&thread_of(t), &links, &listed.parents, None).link;
            st.current = Some(thread.clone());
            st.menu = None;
            st.error = None;
            if let Some(i) = link {
                picked::select(selection, registry, i, links[i].name.clone())?;
            }
            Ok(None)
        }
        ThreadsAct::Close => {
            if st.drafting() {
                return Err(DRAFTING.into());
            }
            st.current = None;
            st.menu = None;
            Ok(None)
        }
        ThreadsAct::Menu { comment } => {
            st.menu = if st.menu.as_deref() == Some(comment.as_str()) { None } else { Some(comment.clone()) };
            Ok(None)
        }
        ThreadsAct::EditMessage { comment } => {
            if st.drafting() {
                return Err(DRAFTING.into());
            }
            let body = st
                .open_listed(base.as_ref())
                .and_then(|l| l.threads.iter().find(|t| Some(&t.id) == st.current.as_ref()))
                .and_then(|t| t.comments.iter().find(|c| c.id == *comment))
                .map(|c| c.body.clone())
                .ok_or_else(|| format!("annotation or comment not found: {comment}"))?;
            st.editing = Some(comment.clone());
            st.compose = body;
            st.menu = None;
            st.error = None;
            st.claim = true;
            Ok(None)
        }
        ThreadsAct::Discard => {
            st.end_draft();
            Ok(None)
        }
        ThreadsAct::Post => {
            let thread = st.current.clone().ok_or("open a comment thread to reply")?;
            let body = st.compose.clone();
            let op = match st.editing.clone() {
                Some(comment) => ThreadOp::EditComment { thread, comment, body: body.clone(), links: None },
                None => ThreadOp::Reply { thread, body: body.clone(), author: st.author.clone(), links: Vec::new() },
            };
            let seq = commit(st, view, registry, op)?;
            if let Some(seq) = seq {
                st.sending = Some((seq, body));
                st.error = None;
            }
            Ok(seq)
        }
        ThreadsAct::Reply { thread, body, author } => {
            let thread = thread.clone().or_else(|| st.current.clone()).ok_or("robot_threads reply needs thread (no thread is shown)")?;
            let author = author.clone().unwrap_or_else(|| st.author.clone());
            commit(st, view, registry, ThreadOp::Reply { thread, body: body.clone(), author, links: Vec::new() })
        }
        ThreadsAct::Edit { thread, comment, body } => {
            let thread = owner(st, base.as_ref(), thread.as_ref(), comment)?;
            commit(st, view, registry, ThreadOp::EditComment { thread, comment: comment.clone(), body: body.clone(), links: None })
        }
        ThreadsAct::Delete { thread, comment } => {
            if st.drafting() {
                return Err(DRAFTING.into());
            }
            let thread = owner(st, base.as_ref(), thread.as_ref(), comment)?;
            commit(st, view, registry, ThreadOp::DeleteComment { thread, comment: comment.clone() })
        }
        ThreadsAct::Resolve { thread, resolved } => {
            let thread = thread.clone().or_else(|| st.current.clone()).ok_or("robot_threads resolve needs thread (no thread is shown)")?;
            let resolved = match resolved {
                Some(r) => *r,
                None => {
                    let listed = st.open_listed(base.as_ref()).ok_or_else(|| not_open(&source_file(view.cad_link.as_ref())))?;
                    !listed.threads.iter().find(|t| t.id == thread).ok_or_else(|| format!("no comment thread {thread} in RoboCAD's comments as last read"))?.resolved()
                }
            };
            commit(st, view, registry, ThreadOp::Resolve { thread, resolved })
        }
        ThreadsAct::OpenInCad { thread } => {
            open_in_cad(st, view, registry, reveal, window, thread.clone())?;
            Ok(None)
        }
        ThreadsAct::SelectLink { index, name } => {
            match view.link_name(*index) {
                Some(actual) if actual == name.as_str() => {}
                Some(actual) => return Err(format!("link {index} is `{actual}`, not `{name}`")),
                None => return Err(format!("no link {index} in the loaded model")),
            }
            picked::select(selection, registry, *index, name.clone())?;
            Ok(None)
        }
    }
}

/// The read at the current key has answered (or there is nothing to read).
fn settled(view: &RobotView, registry: &DocumentRegistry, st: &RobotThreads) -> bool {
    base_of(view, registry).is_none_or(|b| st.read.current(&b))
}

/// A REST call's end: its answer, and REST no longer asks for reads.
fn finish(st: &mut RobotThreads, result: Result<Value, String>) -> Outcome {
    st.asked = false;
    Outcome::Done(result)
}

/// `{cad_threads}` as a REST answer.
fn answer(st: &mut RobotThreads, view: &RobotView, registry: &DocumentRegistry) -> Outcome {
    let value = json!({"cad_threads": state_json(view, registry, st)});
    finish(st, Ok(value))
}

/// Robot mode's thread acts, applied in its one handler (`actions::apply`,
/// RobotSet::Actions). A click's refusal shows under the composer. A REST
/// call first waits for the read at the current key (so a change made
/// before the first read is not refused as "not open"), then acts; a REST
/// change then waits for RoboCAD's answer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle(act: &ThreadsAct, call: &mut Call, st: &mut RobotThreads, view: &RobotView, registry: &DocumentRegistry, selection: &mut Selection, reveal: Option<&mut RevealThread>, window: &mut MessageWriter<Act<WindowAction>>) -> Outcome {
    if !call.rest() {
        if let Err(e) = act_on(act, st, view, registry, selection, reveal, window) {
            st.error = Some(e);
        }
        return Outcome::Done(Ok(Value::Null));
    }
    // A change sent: wait for RoboCAD's answer.
    if let Some(seq) = call.continuation.get(WAIT_COMMIT).and_then(Value::as_u64) {
        if st.in_flight.waits(seq) {
            if call.cancelled {
                return finish(st, Err("cancelled waiting for RoboCAD's answer: the change may still be applied".into()));
            }
            return Outcome::Pending;
        }
        return match st.answers.remove(&seq) {
            Some(Ok(_)) => answer(st, view, registry),
            Some(Err(e)) => finish(st, Err(e)),
            None => finish(st, Err("RoboCAD's answer to this change is no longer waited for (the robot document or Robot mode was left)".into())),
        };
    }
    // Wait for the read at the current key: then answer (`state`, `refresh`) or act.
    let stage = call.continuation.get(WAIT_READ).and_then(Value::as_str).map(str::to_string);
    let acts = match stage {
        Some(stage) => {
            if call.cancelled {
                return finish(st, Err("cancelled waiting for RoboCAD's comments".into()));
            }
            if !settled(view, registry, st) {
                st.asked = true;
                return Outcome::Pending;
            }
            stage == "act"
        }
        None => {
            st.asked = true;
            let acts = !matches!(act, ThreadsAct::State | ThreadsAct::Refresh);
            if matches!(act, ThreadsAct::Refresh) {
                st.read.again();
                st.error = None;
            }
            if !settled(view, registry, st) {
                let then = if acts { "act" } else { "answer" };
                *call.continuation = json!({ "robot_threads_read": then });
                return Outcome::Pending;
            }
            acts
        }
    };
    if !acts {
        return answer(st, view, registry);
    }
    match act_on(act, st, view, registry, selection, reveal, window) {
        Ok(Some(seq)) => {
            *call.continuation = json!({ "robot_threads_wait": seq });
            Outcome::Pending
        }
        Ok(None) => answer(st, view, registry),
        Err(e) => finish(st, Err(e)),
    }
}
