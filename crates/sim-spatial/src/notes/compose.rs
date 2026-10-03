//! The notes panel's state and typing: the Open / Resolved / All filter, the
//! open note, its comment menu, and the reply composer and author on the
//! kit's one text field (`ui_kit::text`).
//!
//! - **State** ([`NotesUi`], a resource): what the panel shows and every
//!   draft, kept apart from the drawn entities and from the sidecar's
//!   document, so a redraw or a re-read of the file loses nothing. A note's
//!   [`Draft`] holds its reply and, apart from it, an open edit of the
//!   note's text or of one reply: Save or Cancel of the edit gives the
//!   composer back the reply.
//! - **The composer** ([`COMPOSE`]): a press on its area gives it the
//!   keyboard with the draft; Enter posts (Shift+Enter types a newline),
//!   Escape drops the draft (the edit, else the reply), a press elsewhere
//!   keeps it. Post writes the view's `annotations` action
//!   (`Request::Reply`, with the reply's own id, or `Request::EditComment`,
//!   as REST sends them).
//! - **Results.** A post waits ([`Waiting`]) until the sidecar shows it (the
//!   reply's id, or the comment's new text): only then is the text dropped.
//!   An error the panel shows after the post fails it: the text stays, with
//!   the error, and Post sends it again. A reply keeps its id until it
//!   lands, so a repeated post is refused by the sidecar ("already exists")
//!   rather than written twice. Another window's edits never settle a draft.
//! - **The author** ([`AUTHOR`], "You" until changed).
use super::panel::NoteAction;
use super::*;
use crate::app::actions::Act;
use crate::inspect::InspectAction;
use crate::ui_kit::text::{EnterKey, FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFieldApp, TextFocus};
use crate::ui_kit::threads::Shown;

/// The reply composer.
pub(crate) const COMPOSE: FieldId = FieldId("inspect.notes.compose");
/// The replies' author.
pub(crate) const AUTHOR: FieldId = FieldId("inspect.notes.author");

/// Which of the panel's fields has the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NoteField {
    Compose,
    Author,
}

/// An open edit of one comment of a note (the note's id: its text).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Edit {
    pub comment: String,
    pub text: String,
}

/// What a draft posted, waiting for the sidecar to show it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Sent {
    /// The reply with the draft's `reply_id`.
    Reply,
    /// Comment `comment`'s new text.
    Edit { comment: String, body: String },
}

/// A post in flight: what was sent, and the panel's error when it was
/// (a different error appearing after it is this post's failure).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Waiting {
    pub sent: Sent,
    pub error_before: Option<String>,
}

/// One note's draft.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Draft {
    /// The reply being written.
    pub reply: String,
    /// The id the reply is posted with, from its first post until it lands.
    pub reply_id: Option<String>,
    /// An open edit; while it is, the composer shows it, not the reply.
    pub edit: Option<Edit>,
    pub waiting: Option<Waiting>,
    /// Why the last post failed (shown under the composer).
    pub error: Option<String>,
}
impl Draft {
    /// The composer's text: the edit's, else the reply's.
    pub(crate) fn text(&self) -> &str {
        self.edit.as_ref().map_or(self.reply.as_str(), |e| e.text.as_str())
    }
    fn text_mut(&mut self) -> &mut String {
        match &mut self.edit {
            Some(e) => &mut e.text,
            None => &mut self.reply,
        }
    }
    /// Nothing left to keep.
    fn is_empty(&self) -> bool {
        self.reply.is_empty() && self.reply_id.is_none() && self.edit.is_none() && self.waiting.is_none() && self.error.is_none()
    }
}

/// The notes panel's own state (see the module doc).
#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct NotesUi {
    /// Which notes are listed (Open by default: resolved notes are hidden).
    pub shown: Shown,
    /// The note drawn with its menus, actions and composer.
    pub open: Option<String>,
    /// The comment of the open note whose "···" menu is open.
    pub menu: Option<String>,
    /// Each note's draft, by note id.
    pub drafts: BTreeMap<String, Draft>,
    /// The note the composer field types into.
    pub composing: Option<String>,
    pub author: String,
    /// The field that has the keyboard (mirrored, so the panel redraws).
    pub focus: Option<NoteField>,
}
impl Default for NotesUi {
    fn default() -> Self {
        Self { shown: Shown::Open, open: None, menu: None, drafts: BTreeMap::new(), composing: None, author: "You".into(), focus: None }
    }
}

/// The request posting `note`'s draft (None: nothing to post); the draft
/// waits for it. `error_now` is the panel's error at the post.
fn post(draft: &mut Draft, note: &str, author: &str, error_now: &Option<String>) -> Option<notes::Request> {
    let body = draft.text().trim().to_string();
    if body.is_empty() {
        return None;
    }
    let (request, sent) = match &draft.edit {
        Some(edit) => (notes::Request::EditComment { note: note.to_string(), comment: edit.comment.clone(), body: body.clone() }, Sent::Edit { comment: edit.comment.clone(), body }),
        None => {
            let id = draft.reply_id.get_or_insert_with(|| sim_annotate::uid("note")).clone();
            (notes::Request::Reply { note: note.to_string(), body, author: author.to_string(), id: Some(id) }, Sent::Reply)
        }
    };
    draft.waiting = Some(Waiting { sent, error_before: error_now.clone() });
    draft.error = None;
    Some(request)
}

/// A waiting draft against the document and the panel's error: landed
/// (its text dropped), failed (kept, with the error) or still waiting.
fn settle(draft: &mut Draft, note: Option<&notes::Note>, error_now: &Option<String>) {
    let Some(waiting) = draft.waiting.clone() else { return };
    let landed = match &waiting.sent {
        Sent::Reply => {
            let id = draft.reply_id.clone().unwrap_or_default();
            match note.and_then(|n| n.replies.iter().find(|r| r.id == id)) {
                Some(reply) => {
                    // The text written since (after a failure the post did
                    // not have) is kept as a new reply.
                    if reply.body == draft.reply.trim() {
                        draft.reply.clear();
                    }
                    draft.reply_id = None;
                    true
                }
                None => false,
            }
        }
        Sent::Edit { comment, body } => {
            let now = note.and_then(|n| if n.id == *comment { Some(n.text.as_str()) } else { n.replies.iter().find(|r| r.id == *comment).map(|r| r.body.as_str()) });
            if now == Some(body.as_str()) {
                draft.edit = None;
                true
            } else {
                false
            }
        }
    };
    if landed {
        draft.waiting = None;
        draft.error = None;
    } else if error_now.is_some() && *error_now != waiting.error_before {
        draft.waiting = None;
        draft.error = error_now.clone();
    }
}

/// The text of comment `comment` of `note` (its own text, or a reply's).
fn comment_text(doc: &notes::Document, note: &str, comment: &str) -> Option<String> {
    let n = doc.notes.get(note)?;
    if n.id == comment {
        return Some(n.text.clone());
    }
    n.replies.iter().find(|r| r.id == comment).map(|r| r.body.clone())
}

/// Which of the panel's fields has the keyboard.
fn focused(text: &TextFocus) -> Option<NoteField> {
    if text.focused(COMPOSE) {
        Some(NoteField::Compose)
    } else if text.focused(AUTHOR) {
        Some(NoteField::Author)
    } else {
        None
    }
}

/// Input: the composer's and the author's field messages, the presses that
/// start, post or drop a draft, and posts settled against the sidecar (see
/// the module doc). The panel's other buttons are `panel::clicks`.
pub(crate) fn compose(
    scene: Res<SpatialScene>,
    mut ui: ResMut<NotesUi>,
    presses: Query<&NoteAction, With<crate::ui_kit::activation::Activated>>,
    mut msgs: MessageReader<FieldMsg>,
    mut text: TextFocus,
    mut out: MessageWriter<Act<InspectAction>>,
) {
    let messages: Vec<FieldMsg> = msgs.read().filter(|m| m.field == COMPOSE || m.field == AUTHOR).cloned().collect();
    if text.suspended(COMPOSE) || text.suspended(AUTHOR) {
        return;
    }
    let pressed: Vec<NoteAction> = presses.iter().filter(|a| matches!(a, NoteAction::Compose(_) | NoteAction::Edit(..) | NoteAction::Post(_) | NoteAction::Cancel(_) | NoteAction::Author)).cloned().collect();
    // Read first: a `ResMut` deref would mark the state changed every frame.
    let waiting = ui.drafts.values().any(|d| d.waiting.is_some());
    if messages.is_empty() && pressed.is_empty() && !waiting && ui.focus == focused(&text) {
        return;
    }
    let mut next = ui.clone();
    let doc = scene.note_document();
    // The error the panel shows (the handler's, else the Store's).
    let error_now = scene.note_error.clone().or_else(|| scene.annotations.as_ref().and_then(|s| s.error()));
    if waiting {
        for (note, draft) in next.drafts.iter_mut() {
            settle(draft, doc.notes.get(note), &error_now);
        }
    }
    let author = match next.author.trim() {
        "" => "You".to_string(),
        name => name.to_string(),
    };
    let mut requests = Vec::new();
    for m in messages {
        match (m.field, m.event) {
            (COMPOSE, FieldEvent::Changed(d)) => {
                if let Some(note) = next.composing.clone() {
                    let draft = next.drafts.entry(note).or_default();
                    // A post in flight is not retyped (its text is what it waits to see).
                    if draft.waiting.is_none() {
                        *draft.text_mut() = d.text;
                        draft.error = None;
                    }
                }
            }
            (COMPOSE, FieldEvent::Submit(typed)) => {
                if let Some(note) = next.composing.take() {
                    let draft = next.drafts.entry(note.clone()).or_default();
                    if draft.waiting.is_none() {
                        *draft.text_mut() = typed;
                    }
                    requests.extend(post(draft, &note, &author, &error_now));
                }
                text.blur(COMPOSE);
            }
            // The kit has taken the keyboard already.
            (COMPOSE, FieldEvent::Cancel) => {
                if let Some(note) = next.composing.take() {
                    cancel(&mut next, &note);
                }
            }
            (AUTHOR, FieldEvent::Changed(d)) => next.author = d.text,
            (AUTHOR, FieldEvent::Submit(_)) => text.blur(AUTHOR),
            _ => {}
        }
    }
    for action in pressed {
        match action {
            NoteAction::Compose(note) => {
                let draft = next.drafts.entry(note.clone()).or_default();
                // A post in flight keeps its text until it lands or fails.
                if draft.waiting.is_none() {
                    let shown = draft.text().to_string();
                    next.composing = Some(note.clone());
                    next.open = Some(note);
                    text.focus_draft(COMPOSE, TextDraft::new(shown, false));
                }
            }
            NoteAction::Edit(note, comment) => {
                next.menu = None;
                let draft = next.drafts.entry(note.clone()).or_default();
                if draft.waiting.is_some() {
                    draft.error = Some("Wait for the post to be saved first".into());
                } else if draft.edit.as_ref().is_some_and(|e| e.comment != comment) {
                    draft.error = Some("Save or cancel the edit you have open first".into());
                } else if let Some(body) = draft.edit.as_ref().map(|e| e.text.clone()).or_else(|| comment_text(&doc, &note, &comment)) {
                    draft.edit = Some(Edit { comment, text: body.clone() });
                    draft.error = None;
                    next.composing = Some(note.clone());
                    next.open = Some(note);
                    text.focus_draft(COMPOSE, TextDraft::new(body, false));
                }
            }
            NoteAction::Post(note) => {
                if let Some(draft) = next.drafts.get_mut(&note) {
                    requests.extend(post(draft, &note, &author, &error_now));
                }
                if next.composing.as_deref() == Some(note.as_str()) {
                    next.composing = None;
                }
                text.blur(COMPOSE);
            }
            NoteAction::Cancel(note) => {
                cancel(&mut next, &note);
                if next.composing.as_deref() == Some(note.as_str()) {
                    next.composing = None;
                    text.blur(COMPOSE);
                }
            }
            NoteAction::Author => {
                let author = next.author.clone();
                text.focus_draft(AUTHOR, TextDraft::new(author, true));
            }
            _ => {}
        }
    }
    next.drafts.retain(|_, d| !d.is_empty());
    next.focus = focused(&text);
    ui.set_if_neq(next);
    for action in requests {
        out.write(Act::ui(InspectAction::Annotations { action }));
    }
}

/// Cancel `note`'s draft: its open edit (the reply comes back), else its
/// reply. A post in flight still lands; its draft no longer waits for it.
fn cancel(ui: &mut NotesUi, note: &str) {
    if let Some(draft) = ui.drafts.get_mut(note) {
        if draft.edit.take().is_none() {
            draft.reply.clear();
            draft.reply_id = None;
        }
        draft.waiting = None;
        draft.error = None;
    }
}

/// SpatialViewerPlugin: the panel's state and its two fields.
pub(crate) fn build(app: &mut App) {
    app.init_resource::<NotesUi>()
        .add_text_field(COMPOSE, TextField::new("Reply to the note").placeholder("Write a reply…").enter(EnterKey::ShiftNewline))
        .add_text_field(AUTHOR, TextField::new("Reply author").placeholder("Your display name").select_on_focus());
}

#[cfg(test)]
mod tests {
    use super::*;
    fn note() -> notes::Note {
        notes::Note { id: "n".into(), label: "L".into(), text: "text".into(), targets: SelectionTarget::None, links: vec![], color: [0, 0, 0], replies: vec![], resolved: false }
    }
    fn landed(n: &mut notes::Note, id: &str, body: &str) {
        n.replies.push(Comment { id: id.into(), author: "You".into(), body: body.into(), created_at: "1".into(), edited_at: None, links: vec![] });
    }
    /// A reply waits for its own id: another window's edit does not settle
    /// it, a repeated post keeps the id, an error after the post fails it
    /// with the text kept, and its landing drops the text.
    #[test]
    fn a_reply_settles_on_its_own_result() {
        let mut draft = Draft { reply: "hello ".into(), ..Default::default() };
        let Some(notes::Request::Reply { id: Some(id), body, .. }) = post(&mut draft, "n", "You", &None) else { panic!("a reply") };
        assert_eq!(body, "hello");
        let mut n = note();
        // Another window's edit: the note changed, the reply is not there.
        n.text = "changed elsewhere".into();
        settle(&mut draft, Some(&n), &None);
        assert!(draft.waiting.is_some() && draft.reply == "hello ");
        // A post again sends the same id (the sidecar refuses a second copy).
        let Some(notes::Request::Reply { id: Some(again), .. }) = post(&mut draft, "n", "You", &None) else { panic!("a reply") };
        assert_eq!(again, id);
        // An error shown after the post fails it; the text and id stay.
        settle(&mut draft, Some(&n), &Some("annotation file is busy".into()));
        assert!(draft.waiting.is_none());
        assert_eq!(draft.error.as_deref(), Some("annotation file is busy"));
        assert_eq!((draft.reply.as_str(), draft.reply_id.as_deref()), ("hello ", Some(id.as_str())));
        // Sent again, it lands: the text goes.
        post(&mut draft, "n", "You", &Some("annotation file is busy".into())).unwrap();
        settle(&mut draft, Some(&n), &Some("annotation file is busy".into()));
        assert!(draft.waiting.is_some(), "the error that was already shown is not this post's");
        landed(&mut n, &id, "hello");
        settle(&mut draft, Some(&n), &None);
        assert!(draft.is_empty());
    }
    /// An edit is kept apart from the reply being written: Save lands the
    /// edit and gives the reply back; Cancel does too.
    #[test]
    fn an_edit_keeps_the_reply_draft() {
        let mut ui = NotesUi::default();
        ui.drafts.insert("n".into(), Draft { reply: "half a reply".into(), edit: Some(Edit { comment: "n".into(), text: "new text".into() }), ..Default::default() });
        assert_eq!(ui.drafts["n"].text(), "new text");
        let draft = ui.drafts.get_mut("n").unwrap();
        let Some(notes::Request::EditComment { comment, body, .. }) = post(draft, "n", "You", &None) else { panic!("an edit") };
        assert_eq!((comment.as_str(), body.as_str()), ("n", "new text"));
        let mut n = note();
        settle(draft, Some(&n), &None);
        assert!(draft.edit.is_some(), "not landed yet");
        n.text = "new text".into();
        settle(draft, Some(&n), &None);
        assert!(draft.edit.is_none() && draft.waiting.is_none());
        assert_eq!(draft.text(), "half a reply");
        draft.edit = Some(Edit { comment: "n".into(), text: "other".into() });
        cancel(&mut ui, "n");
        assert_eq!(ui.drafts["n"].text(), "half a reply");
        cancel(&mut ui, "n");
        assert!(ui.drafts["n"].is_empty());
    }
}
