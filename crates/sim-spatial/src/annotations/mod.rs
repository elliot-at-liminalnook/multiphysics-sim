//! The one annotations and discussions service (native-viewer.md §7): the
//! listing, adding, replying, editing, deleting, resolving and anchoring of
//! threads, over `sim_annotate`'s types and its `Anchor` trait, for every
//! mode.
//!
//! - **Sources.** Each source of threads is an anchor adapter implementing
//!   [`ThreadSource`]: its anchor type, its threads as stored, the id
//!   prefixes it has always written, how it validates a whole thread, and
//!   [`ThreadSource::commit`], which applies one `sim_annotate::ThreadCommand`
//!   to its own document in its own on-disk format. The adapters:
//!   - Inspect notes (`notes.rs`, `sim_inspect::annotations` in
//!     `*.annotations.json`): a note is shown as a one-message thread
//!     (`notes::NoteAnchor`); saved views and navigation stay Inspect's;
//!   - system discussions (`builder/discussion.rs`, `sim_system::display`
//!     threads inside the system document, saved by the builder's own save
//!     path with the system's undo);
//!   - lesson notes (`lesson/threads.rs`, `sim_lesson::LessonAnchor` threads
//!     in `lesson.md.annotations.json`).
//! - **Operations.** Every intent is a [`ThreadOp`]; [`apply`] lowers it to
//!   one command ([`lower`]: new ids and timestamps here; title, link, pin
//!   and link-carrying edits as a whole-thread put), validates a put thread
//!   with the source's [`ThreadSource::validate`] (by default
//!   `sim_annotate::validate_thread`, whose messages REST clients match)
//!   and commits it. [`edited`] applies a comment edit, a comment delete or
//!   a resolve to one thread, for sources that store only whole threads.
//! - **Undo** is the source's: the sidecars' inverse-command stacks
//!   ([`ThreadOp::Undo`], [`ThreadOp::Redo`]) and the system document's
//!   undo for system discussions.
//! - **File work** never runs on the UI thread: the sidecar sources submit
//!   to `sim_annotate::store::Store`, the shared crate's background worker
//!   (a locked read-check-apply-write per edit, idle re-reads for edits made
//!   elsewhere), and report [`Committed::Pending`] with its request id.
//! - **Drawing** is `ui_kit::threads`: one thread list, thread messages,
//!   anchor chips and composer for every source.
//! - **Selection.** A note on what is selected reads the shared
//!   `crate::selection::Selection`, and following an anchor writes it, in
//!   each adapter: Inspect's document's items (`notes::select`, through
//!   `inspect::Owner`) and the builder's names at its level
//!   (`builder::picked::Picked`).
use sim_annotate::{Anchor, Comment, PhysicalView, Thread, ThreadCommand};
use std::collections::BTreeMap;

/// One source of threads (an anchor adapter).
pub(crate) trait ThreadSource {
    type Anchor: Anchor;
    /// Prefix of a new thread's id (`{prefix}-{nanos}`, `sim_annotate::uid`).
    const THREAD_ID: &'static str;
    /// Prefix of a new comment's id.
    const COMMENT_ID: &'static str;
    /// Every thread as stored (anchors as last saved; display refreshes them).
    fn threads(&self) -> BTreeMap<String, Thread<Self::Anchor>>;
    /// One thread as stored.
    fn thread(&self, id: &str) -> Option<Thread<Self::Anchor>> {
        self.threads().remove(id)
    }
    /// Whether two anchors name the same thing (`ThreadOp::Link` adds only new ones).
    fn same(a: &Self::Anchor, b: &Self::Anchor) -> bool {
        a == b
    }
    /// Shape limits of a whole thread about to be put.
    fn validate(&self, thread: &Thread<Self::Anchor>) -> Result<(), String> {
        sim_annotate::validate_thread(thread)
    }
    /// Apply one command to the source's own document (and file). `label`
    /// is what its history or status shows.
    fn commit(&mut self, label: &str, command: ThreadCommand<Self::Anchor>) -> Result<Committed, String>;
}

/// Where a committed command is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Committed {
    /// Applied now (the source's document already shows it).
    Done,
    /// Submitted to the source's file worker; the result comes with this request id.
    Pending(u64),
}

/// What [`apply`] did.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Applied {
    /// The thread the op created or changed (None: deleted, or undo/redo).
    pub thread: Option<String>,
    pub committed: Committed,
}

/// Every intent on threads, as each mode's buttons, keys and REST adapters write it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ThreadOp<A> {
    /// A new thread with its first comment.
    Create { title: String, targets: Vec<A>, body: String, author: String, links: Vec<A>, pin_m: Option<[f32; 3]>, view: Option<PhysicalView> },
    /// A new comment written now.
    Reply { thread: String, body: String, author: String, links: Vec<A> },
    /// A comment written elsewhere, kept as it is (an agent's reply, keyed by its run).
    Post { thread: String, comment: Comment<A> },
    /// New text for a comment; `links: Some` replaces its links too.
    EditComment { thread: String, comment: String, body: String, links: Option<Vec<A>> },
    DeleteComment { thread: String, comment: String },
    Resolve { thread: String, resolved: bool },
    Delete { thread: String },
    Retitle { thread: String, title: String },
    /// Add targets the thread does not have yet (`ThreadSource::same`).
    Link { thread: String, targets: Vec<A> },
    Pin { thread: String, pin_m: Option<[f32; 3]> },
    Undo,
    Redo,
}

/// A comment written now (`created_at`: Unix seconds, `sim_annotate::stamp`).
pub(crate) fn comment<A>(prefix: &str, author: String, body: String, links: Vec<A>) -> Comment<A> {
    Comment { id: sim_annotate::uid(prefix), author, body, created_at: sim_annotate::stamp(), edited_at: None, links }
}

/// A title from a body's first line (at most 80 characters), else `fallback`.
pub(crate) fn first_line(text: &str, fallback: &str) -> String {
    text.lines().next().unwrap_or(fallback).chars().take(80).collect()
}

/// `op` as one command for `source`, and the thread it is about.
pub(crate) fn lower<S: ThreadSource>(source: &S, op: ThreadOp<S::Anchor>) -> Result<(Option<String>, ThreadCommand<S::Anchor>), String> {
    let existing = |id: &str| source.thread(id).ok_or_else(|| "unknown thread".to_string());
    Ok(match op {
        ThreadOp::Create { title, targets, body, author, links, pin_m, view } => {
            let id = sim_annotate::uid(S::THREAD_ID);
            let thread = Thread { id: id.clone(), title, resolved: false, targets, comments: vec![comment(S::COMMENT_ID, author, body, links)], pin_m, view };
            (Some(id), ThreadCommand::PutThread { thread })
        }
        ThreadOp::Reply { thread, body, author, links } => (Some(thread.clone()), ThreadCommand::AddComment { thread, comment: comment(S::COMMENT_ID, author, body, links) }),
        ThreadOp::Post { thread, comment } => (Some(thread.clone()), ThreadCommand::AddComment { thread, comment }),
        ThreadOp::EditComment { thread, comment, body, links: None } => (Some(thread.clone()), ThreadCommand::EditComment { thread, comment, body, edited_at: sim_annotate::stamp() }),
        ThreadOp::EditComment { thread, comment, body, links: Some(links) } => {
            let mut t = existing(&thread)?;
            let c = t.comments.iter_mut().find(|c| c.id == comment).ok_or("unknown comment")?;
            c.body = body;
            c.links = links;
            c.edited_at = Some(sim_annotate::stamp());
            (Some(thread), ThreadCommand::PutThread { thread: t })
        }
        ThreadOp::DeleteComment { thread, comment } => (Some(thread.clone()), ThreadCommand::DeleteComment { thread, comment }),
        ThreadOp::Resolve { thread, resolved } => (Some(thread.clone()), ThreadCommand::Resolve { thread, resolved }),
        ThreadOp::Delete { thread } => (None, ThreadCommand::DeleteThread { id: thread }),
        ThreadOp::Retitle { thread, title } => {
            let mut t = existing(&thread)?;
            t.title = title;
            (Some(thread), ThreadCommand::PutThread { thread: t })
        }
        ThreadOp::Link { thread, targets } => {
            let mut t = existing(&thread)?;
            for target in targets {
                if !t.targets.iter().any(|r| S::same(r, &target)) {
                    t.targets.push(target);
                }
            }
            (Some(thread), ThreadCommand::PutThread { thread: t })
        }
        ThreadOp::Pin { thread, pin_m } => {
            let mut t = existing(&thread)?;
            t.pin_m = pin_m;
            (Some(thread), ThreadCommand::PutThread { thread: t })
        }
        ThreadOp::Undo => (None, ThreadCommand::Undo),
        ThreadOp::Redo => (None, ThreadCommand::Redo),
    })
}

/// Lower, validate and commit one op.
pub(crate) fn apply<S: ThreadSource>(source: &mut S, label: &str, op: ThreadOp<S::Anchor>) -> Result<Applied, String> {
    let (thread, command) = lower(source, op)?;
    if let ThreadCommand::PutThread { thread } = &command {
        source.validate(thread)?;
    }
    let committed = source.commit(label, command)?;
    Ok(Applied { thread, committed })
}

/// A comment edit, comment delete or resolve applied to one thread (as
/// `sim_annotate::ThreadDocument::apply` applies them), for sources that
/// store whole threads.
pub(crate) fn edited<A: Anchor>(mut t: Thread<A>, command: ThreadCommand<A>) -> Result<Thread<A>, String> {
    match command {
        ThreadCommand::EditComment { comment, body, edited_at, .. } => {
            let c = t.comments.iter_mut().find(|c| c.id == comment).ok_or("unknown comment")?;
            c.body = body;
            c.edited_at = Some(edited_at);
        }
        ThreadCommand::DeleteComment { comment, .. } => {
            let before = t.comments.len();
            t.comments.retain(|c| c.id != comment);
            if t.comments.len() == before {
                return Err("unknown comment".into());
            }
        }
        ThreadCommand::Resolve { resolved, .. } => t.resolved = resolved,
        _ => return Err("not an edit of one thread".into()),
    }
    Ok(t)
}

/// The thread a command is about (None: undo or redo).
pub(crate) fn subject<A>(command: &ThreadCommand<A>) -> Option<&str> {
    match command {
        ThreadCommand::PutThread { thread } => Some(&thread.id),
        ThreadCommand::DeleteThread { id } => Some(id),
        ThreadCommand::AddComment { thread, .. } | ThreadCommand::EditComment { thread, .. } | ThreadCommand::DeleteComment { thread, .. } | ThreadCommand::Resolve { thread, .. } => Some(thread),
        ThreadCommand::Undo | ThreadCommand::Redo => None,
    }
}

#[cfg(test)]
mod tests;
