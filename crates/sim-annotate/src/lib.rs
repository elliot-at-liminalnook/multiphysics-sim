//! Shared annotation model for every host: system discussions (anchored to
//! instance lineage), schematic notes (anchored to selections) and lessons
//! (anchored to quoted text and scene parts). Annotations are authored
//! presentation artifacts; they never feed physics, CAD or simulation.
//!
//! What is shared:
//! - [`Thread`] / [`Comment`] generic over an [`Anchor`] (what a note points at),
//! - validation limits and messages ([`validate_thread`]),
//! - re-attachment after the annotated source changes ([`refresh`]),
//! - bounded inverse-command undo ([`history`]),
//! - a revisioned sidecar document of threads ([`ThreadDocument`]) and a
//!   locked, atomically written file store for any revisioned document ([`store`]).
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, BTreeSet};

pub mod history;
pub mod store;
pub mod text;

/// What an annotation points at. Implementations decide how identity
/// survives edits to the annotated source (lineage, quoted text, scene part).
pub trait Anchor: Clone + PartialEq + Serialize + DeserializeOwned {
    /// Whatever the anchor resolves against (a path index, a text index...).
    type Index: ?Sized;
    /// Shape limits only; resolution is [`Anchor::refresh`].
    fn validate(&self) -> Result<(), String>;
    /// Short human label for chips and lists.
    fn label(&self) -> String;
    /// True when the last refresh could not find the annotated thing.
    fn missing(&self) -> bool;
    /// Re-attach after the source changed. Must be deterministic; returns
    /// whether the anchor is attached. A detached anchor is kept, never dropped.
    fn refresh(&mut self, index: &Self::Index) -> bool;
}

/// A saved 3D camera (orbit) and display toggles. Shared by schematic saved
/// views, system discussions and lesson scene annotations.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PhysicalView {
    pub focus: [f32; 3],
    pub radius: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub exploded: bool,
    pub connections: bool,
    pub hidden: BTreeSet<String>,
}
impl PhysicalView {
    pub fn validate(&self) -> Result<(), String> {
        if !self
            .focus
            .iter()
            .chain([self.radius, self.yaw, self.pitch].iter())
            .all(|v| v.is_finite())
            || self.radius <= 0.
            || self.pitch.abs() > 1.5
        {
            return Err("invalid discussion camera".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thread<A> {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub resolved: bool,
    pub targets: Vec<A>,
    #[serde(default = "Vec::new")]
    pub comments: Vec<Comment<A>>,
    /// Optional local-space anchor on the first target (metres).
    #[serde(default)]
    pub pin_m: Option<[f32; 3]>,
    #[serde(default)]
    pub view: Option<PhysicalView>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Comment<A> {
    pub id: String,
    pub author: String,
    pub body: String,
    pub created_at: String,
    #[serde(default)]
    pub edited_at: Option<String>,
    #[serde(default = "Vec::new")]
    pub links: Vec<A>,
}
impl<A> Comment<A> {
    /// Replies written by the Codex answer service.
    pub fn is_agent(&self) -> bool {
        self.id.starts_with("agent-") || self.author.eq_ignore_ascii_case("codex")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Discussions<A> {
    #[serde(default = "BTreeMap::new")]
    pub threads: BTreeMap<String, Thread<A>>,
}
impl<A> Default for Discussions<A> {
    fn default() -> Self {
        Self { threads: BTreeMap::new() }
    }
}
impl<A> Discussions<A> {
    pub fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }
}

pub const MAX_THREADS: usize = 1000;

/// Shape limits shared by every host. Messages are stable (tests and REST
/// clients match on them).
pub fn validate_thread<A: Anchor>(t: &Thread<A>) -> Result<(), String> {
    let text = |s: &str, max: usize| !s.trim().is_empty() && s.len() <= max;
    if !text(&t.id, 256)
        || !text(&t.title, 256)
        || t.targets.is_empty()
        || t.targets.len() > 200
        || t.comments.len() > 1000
        || t.pin_m.is_some_and(|p| !p.iter().all(|v| v.is_finite()))
    {
        return Err("invalid discussion thread, targets or pin".into());
    }
    if let Some(v) = &t.view {
        v.validate()?;
    }
    for target in t
        .targets
        .iter()
        .chain(t.comments.iter().flat_map(|c| &c.links))
    {
        target.validate()?;
    }
    let mut ids = BTreeSet::new();
    for c in &t.comments {
        if !text(&c.id, 256)
            || !ids.insert(&c.id)
            || !text(&c.author, 120)
            || !text(&c.body, 20000)
            || !text(&c.created_at, 100)
            || c.links.len() > 200
        {
            return Err(
                "comments need unique ID, author, timestamp and body (at most 20000 bytes)".into(),
            );
        }
    }
    Ok(())
}

/// Re-attach every thread target and comment link.
pub fn refresh<A: Anchor>(discussions: &mut Discussions<A>, index: &A::Index) {
    for thread in discussions.threads.values_mut() {
        refresh_thread(thread, index);
    }
}
pub fn refresh_thread<A: Anchor>(thread: &mut Thread<A>, index: &A::Index) {
    for t in thread
        .targets
        .iter_mut()
        .chain(thread.comments.iter_mut().flat_map(|c| c.links.iter_mut()))
    {
        t.refresh(index);
    }
}

/// Readable text for hosts that render the persistent link chips separately:
/// `[label](part:...)` and `[label](group:...)` become `label`.
pub fn plain_comment(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        if let Some((label, link)) = tail.split_once("](") {
            if link.starts_with("part:") || link.starts_with("group:") {
                if let Some((_, after)) = link.split_once(')') {
                    out.push_str(label);
                    rest = after;
                    continue;
                }
            }
        }
        out.push('[');
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Compact UI timestamp; the stored timestamp remains an exact UTC instant
/// (seconds since the Unix epoch).
pub fn relative_time(timestamp: &str) -> String {
    let Some(seconds) = timestamp.parse::<u64>().ok() else {
        return timestamp.into();
    };
    let age = now().saturating_sub(seconds);
    if age < 60 {
        "just now".into()
    } else if age < 3600 {
        format!("{}m ago", age / 60)
    } else if age < 86400 {
        format!("{}h ago", age / 3600)
    } else {
        format!("{}d ago", age / 86400)
    }
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
/// Timestamp string stored in comments.
pub fn stamp() -> String {
    now().to_string()
}
/// Unique-enough ID for new threads and comments (`prefix-<nanos>`).
pub fn uid(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}

/// Edits to a [`ThreadDocument`]. Every edit except navigation is undoable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
#[serde(bound(serialize = "A: Serialize", deserialize = "A: DeserializeOwned"))]
pub enum ThreadCommand<A> {
    Undo,
    Redo,
    PutThread { thread: Thread<A> },
    DeleteThread { id: String },
    AddComment { thread: String, comment: Comment<A> },
    EditComment { thread: String, comment: String, body: String, edited_at: String },
    DeleteComment { thread: String, comment: String },
    Resolve { thread: String, resolved: bool },
}

/// A standalone, revisioned set of threads stored in its own sidecar file
/// (for sources that are not system documents, such as lessons).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(serialize = "A: Serialize", deserialize = "A: DeserializeOwned"))]
pub struct ThreadDocument<A> {
    pub version: u32,
    /// What the threads annotate (lesson ID, file name...). A document for
    /// another subject is rejected.
    pub subject: String,
    pub revision: u64,
    #[serde(default = "BTreeMap::new")]
    pub threads: BTreeMap<String, Thread<A>>,
    #[serde(default = "Vec::new")]
    pub undo: Vec<ThreadCommand<A>>,
    #[serde(default = "Vec::new")]
    pub redo: Vec<ThreadCommand<A>>,
}
impl<A: Anchor> ThreadDocument<A> {
    pub fn new(subject: &str) -> Self {
        Self { version: 1, subject: subject.into(), revision: 0, threads: BTreeMap::new(), undo: vec![], redo: vec![] }
    }
    pub fn validate(&self, subject: &str) -> Result<(), String> {
        if self.version != 1 || self.subject != subject {
            return Err(format!("annotations belong to `{}`, not `{subject}`", self.subject));
        }
        if self.threads.len() > MAX_THREADS {
            return Err(format!("annotation limit: {MAX_THREADS} threads"));
        }
        history::validate(&self.undo, &self.redo, |c| matches!(c, ThreadCommand::Undo | ThreadCommand::Redo))?;
        for (id, t) in &self.threads {
            if id != &t.id {
                return Err("thread key does not match its ID".into());
            }
            validate_thread(t)?;
        }
        Ok(())
    }
    /// Apply one command: records the inverse for undo, bumps the revision and
    /// validates the result. On error nothing changes.
    pub fn apply(&mut self, command: ThreadCommand<A>, subject: &str) -> Result<(), String> {
        let revision = self.revision.checked_add(1).ok_or("annotation revision exhausted")?;
        let mut next = self.clone();
        let step = match &command {
            ThreadCommand::Undo => history::Step::Undo,
            ThreadCommand::Redo => history::Step::Redo,
            _ => history::Step::Do,
        };
        let command = match step {
            history::Step::Undo => next.undo.pop().ok_or("nothing to undo")?,
            history::Step::Redo => next.redo.pop().ok_or("nothing to redo")?,
            history::Step::Do => command,
        };
        let thread_id = match &command {
            ThreadCommand::PutThread { thread } => thread.id.clone(),
            ThreadCommand::DeleteThread { id } => id.clone(),
            ThreadCommand::AddComment { thread, .. }
            | ThreadCommand::EditComment { thread, .. }
            | ThreadCommand::DeleteComment { thread, .. }
            | ThreadCommand::Resolve { thread, .. } => thread.clone(),
            ThreadCommand::Undo | ThreadCommand::Redo => return Err("invalid recursive annotation history".into()),
        };
        // Every edit is undone by restoring the whole thread as it was.
        let inverse = match next.threads.get(&thread_id) {
            Some(old) => ThreadCommand::PutThread { thread: old.clone() },
            None => ThreadCommand::DeleteThread { id: thread_id.clone() },
        };
        let unknown = || format!("unknown thread {thread_id}");
        match command {
            ThreadCommand::PutThread { thread } => {
                next.threads.insert(thread.id.clone(), thread);
            }
            ThreadCommand::DeleteThread { id } => {
                next.threads.remove(&id).ok_or_else(unknown)?;
            }
            ThreadCommand::AddComment { thread, comment } => {
                let t = next.threads.get_mut(&thread).ok_or_else(unknown)?;
                if t.comments.iter().any(|c| c.id == comment.id) {
                    return Err(format!("comment {} already exists", comment.id));
                }
                t.comments.push(comment);
            }
            ThreadCommand::EditComment { thread, comment, body, edited_at } => {
                let t = next.threads.get_mut(&thread).ok_or_else(unknown)?;
                let c = t.comments.iter_mut().find(|c| c.id == comment).ok_or("unknown comment")?;
                c.body = body;
                c.edited_at = Some(edited_at);
            }
            ThreadCommand::DeleteComment { thread, comment } => {
                let t = next.threads.get_mut(&thread).ok_or_else(unknown)?;
                let before = t.comments.len();
                t.comments.retain(|c| c.id != comment);
                if t.comments.len() == before {
                    return Err("unknown comment".into());
                }
            }
            ThreadCommand::Resolve { thread, resolved } => {
                next.threads.get_mut(&thread).ok_or_else(unknown)?.resolved = resolved;
            }
            ThreadCommand::Undo | ThreadCommand::Redo => unreachable!(),
        }
        history::record(&mut next.undo, &mut next.redo, inverse, step);
        next.revision = revision;
        next.validate(subject)?;
        *self = next;
        Ok(())
    }
    /// Re-attach anchors against the current source. Presentation only; the
    /// revision is unchanged (hosts refresh on display, persist on edit).
    pub fn refreshed(&self, index: &A::Index) -> BTreeMap<String, Thread<A>> {
        let mut threads = self.threads.clone();
        for t in threads.values_mut() {
            refresh_thread(t, index);
        }
        threads
    }
}
impl<A: Anchor + Send + 'static> store::Revisioned for ThreadDocument<A> {
    type Command = ThreadCommand<A>;
    type Context = String;
    fn empty(subject: &String) -> Self {
        Self::new(subject)
    }
    fn revision(&self) -> u64 {
        self.revision
    }
    fn validate(&self, subject: &String) -> Result<(), String> {
        ThreadDocument::validate(self, subject)
    }
    fn apply(&mut self, command: ThreadCommand<A>, subject: &String) -> Result<(), String> {
        ThreadDocument::apply(self, command, subject)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Key(String);
    impl Anchor for Key {
        type Index = BTreeSet<String>;
        fn validate(&self) -> Result<(), String> {
            if self.0.is_empty() { Err("invalid discussion reference".into()) } else { Ok(()) }
        }
        fn label(&self) -> String {
            self.0.clone()
        }
        fn missing(&self) -> bool {
            false
        }
        fn refresh(&mut self, index: &BTreeSet<String>) -> bool {
            index.contains(&self.0)
        }
    }
    fn thread(id: &str) -> Thread<Key> {
        Thread { id: id.into(), title: "Why".into(), resolved: false, targets: vec![Key("a".into())], comments: vec![], pin_m: None, view: None }
    }
    fn comment(id: &str) -> Comment<Key> {
        Comment { id: id.into(), author: "User".into(), body: "hello".into(), created_at: "1".into(), edited_at: None, links: vec![] }
    }
    #[test]
    fn thread_document_undo_redo_restores_whole_threads() {
        let mut d = ThreadDocument::<Key>::new("lesson");
        d.apply(ThreadCommand::PutThread { thread: thread("t") }, "lesson").unwrap();
        d.apply(ThreadCommand::AddComment { thread: "t".into(), comment: comment("c") }, "lesson").unwrap();
        d.apply(ThreadCommand::EditComment { thread: "t".into(), comment: "c".into(), body: "edited".into(), edited_at: "2".into() }, "lesson").unwrap();
        assert_eq!(d.threads["t"].comments[0].body, "edited");
        d.apply(ThreadCommand::Undo, "lesson").unwrap();
        assert_eq!(d.threads["t"].comments[0].body, "hello");
        d.apply(ThreadCommand::Undo, "lesson").unwrap();
        assert!(d.threads["t"].comments.is_empty());
        d.apply(ThreadCommand::Redo, "lesson").unwrap();
        assert_eq!(d.threads["t"].comments.len(), 1);
        d.apply(ThreadCommand::Undo, "lesson").unwrap();
        d.apply(ThreadCommand::Undo, "lesson").unwrap();
        assert!(d.threads.is_empty());
        assert_eq!(d.revision, 8);
        // Duplicate comment IDs and other subjects are rejected without change.
        d.apply(ThreadCommand::Redo, "lesson").unwrap();
        d.apply(ThreadCommand::AddComment { thread: "t".into(), comment: comment("c") }, "lesson").unwrap();
        let before = d.clone();
        assert!(d.apply(ThreadCommand::AddComment { thread: "t".into(), comment: comment("c") }, "lesson").is_err());
        assert!(d.apply(ThreadCommand::Resolve { thread: "t".into(), resolved: true }, "other").is_err());
        assert_eq!(d, before);
    }
    #[test]
    fn validation_messages_and_plain_text_are_stable() {
        let mut t = thread("t");
        t.targets.clear();
        assert_eq!(validate_thread(&t).unwrap_err(), "invalid discussion thread, targets or pin");
        let mut t = thread("t");
        t.targets.push(Key(String::new()));
        assert_eq!(validate_thread(&t).unwrap_err(), "invalid discussion reference");
        assert_eq!(plain_comment("see [the worm](part:gearbox/worm) and [x](y)"), "see the worm and [x](y)");
    }
}
