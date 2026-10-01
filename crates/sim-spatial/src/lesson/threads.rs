//! Lesson notes: the lessons' thread adapter for `crate::annotations`
//! (`sim_lesson::LessonAnchor` threads, anchored to quoted text or to scene
//! parts, in `lesson.md.annotations.json`). Edits are submitted to the
//! sidecar's store (`Notes`: `sim-annotate`'s worker writes the file, never
//! the UI thread); the result arrives in `watch::poll`.
use super::*;
use crate::annotations::{self, Committed, ThreadOp, ThreadSource};

/// Lesson notes as a thread source.
pub(crate) struct LessonThreads<'a> {
    pub learn: &'a mut Learn,
}
impl ThreadSource for LessonThreads<'_> {
    type Anchor = LessonAnchor;
    const THREAD_ID: &'static str = "t";
    const COMMENT_ID: &'static str = "c";
    fn threads(&self) -> BTreeMap<String, Thread<LessonAnchor>> {
        self.learn.notes_doc.threads.clone()
    }
    fn thread(&self, id: &str) -> Option<Thread<LessonAnchor>> {
        self.learn.notes_doc.threads.get(id).cloned()
    }
    fn commit(&mut self, label: &str, command: ThreadCommand<LessonAnchor>) -> Result<Committed, String> {
        self.learn.note(label, command).map(Committed::Pending)
    }
}

impl Learn {
    /// One thread op through the annotations service (async; the result arrives in `poll`).
    pub(crate) fn thread_op(&mut self, label: &str, op: ThreadOp<LessonAnchor>) -> Result<annotations::Applied, String> {
        annotations::apply(&mut LessonThreads { learn: self }, label, op)
    }

    /// Submit a note command to the sidecar's store, checked against the
    /// revision shown (the result arrives in `poll`).
    pub fn note(&mut self, label: &str, command: ThreadCommand<LessonAnchor>) -> Result<u64, String> {
        let notes = self.notes.as_mut().ok_or("no lesson is open")?;
        let id = notes.submit(command, Some(self.notes_doc.revision))?;
        self.pending.push((id, label.to_string()));
        Ok(id)
    }

    /// Threads re-attached to today's text (display only).
    pub(crate) fn threads(&self) -> BTreeMap<String, Thread<LessonAnchor>> {
        match &self.index {
            Some(index) => self.notes_doc.refreshed(index),
            None => self.notes_doc.threads.clone(),
        }
    }

    /// A new note's title: its body's first line as plain text.
    pub(crate) fn note_title(body: &str) -> String {
        annotations::first_line(&sim_annotate::plain_comment(body), "Note")
    }

    /// Post the open draft (`body`, trimmed and not empty) of a comment
    /// purpose: an edit of the open thread's comment, a reply to it, or a
    /// new thread on the draft anchor.
    pub(super) fn submit_thread_draft(&mut self, purpose: &Purpose, body: String) -> Result<(), String> {
        match (purpose, self.thread.clone(), self.draft.clone()) {
            (Purpose::EditComment(c), Some(t), _) => {
                self.thread_op("Edit comment", ThreadOp::EditComment { thread: t, comment: c.clone(), body, links: None })?;
            }
            (_, Some(t), _) => {
                let author = self.author.clone();
                self.thread_op("Reply", ThreadOp::Reply { thread: t, body, author, links: vec![] })?;
            }
            (_, None, Some(anchor)) => {
                let (title, author) = (Self::note_title(&body), self.author.clone());
                let id = self.thread_op("New note", ThreadOp::Create { title, targets: vec![anchor], body, author, links: vec![], pin_m: None, view: None })?.thread.ok_or("the new note has no id")?;
                if std::mem::take(&mut self.ask_next) {
                    self.ask_when_saved = Some(id.clone());
                }
                self.thread = Some(id);
                self.draft = None;
            }
            _ => return Err("pick a paragraph or a part to attach the note to".into()),
        }
        self.input = None;
        Ok(())
    }
}
