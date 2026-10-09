//! The Comments section of Robot mode's inspector (`Section::Comments`),
//! drawn with the one thread panel (`ui_kit::threads`) through
//! [`RobotHost`], and its composer's typing on the kit's text field
//! ([`COMPOSE`]).
//!
//! Top to bottom: the section title; the status line (where the comments
//! are, the revision they were read at, or why they cannot be read here);
//! Open in CAD and Read again; then, once the CAD source's `.rcad` is read
//! (in process), the Open / Resolved / All filter, the thread list
//! (RoboCAD's "n · part", each with its warning line), and the shown
//! thread: its location, All comments, Resolve/Reopen, the messages (part
//! chips and part links select their link), the composer (Reply or Save
//! edit) and the undo note. New threads are placed in CAD mode.
//!
//! The composer refuses the keyboard while the Leg calibration panel is
//! shown (its Q/A hold-to-move and Z STOP keys are read whatever has
//! focus) or the document picker is open, and gives it up when either
//! opens, the section is left or no thread is shown.
use super::{LinkKeys, Parents, Placed, RobotThreads, ThreadsAct, anchor_link, base_of, cad_path, line, link_note, placements, shown};
use crate::app::actions::Act;
use super::NO_UNDO;
use crate::cad::threads::{CadAnchor, attachment, thread_of};
use crate::robot::{RobotAction, RobotView};
use crate::ui_kit::activation::Activated;
use crate::ui_kit::text::{EnterKey, FieldEvent, FieldId, FieldMsg, TextDraft, TextField, TextFocus};
use crate::ui_kit::threads::{self as kit, Composer, Host, Shown};
use crate::ui_kit::{FAINT, Kit, Look, SUBTLE, TEXT, UiFonts, WARN, size, wrap};
use bevy::prelude::*;
use sim_annotate::{Comment, Thread};
use std::collections::{BTreeMap, HashMap};

/// The composer's kit field.
pub(crate) const COMPOSE: FieldId = FieldId("robot.threads.compose");
const HARDWARE_HAS_KEYS: &str = "The Leg calibration panel is open and its keys (Q/A move, Z/Escape STOP) stay live: close it to type a reply.";

/// The composer's field (Enter posts, Shift+Enter types a newline).
pub(crate) fn compose_field() -> TextField {
    TextField::new("Comment reply").placeholder("Write a reply…").enter(EnterKey::ShiftNewline)
}

/// Where the section is drawn (in the inspector's scroll area, `ui::setup`).
#[derive(Component)]
pub(crate) struct ThreadsRoot;

/// A press on the composer's parts.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComposeInput {
    Compose,
    Submit,
    Cancel,
}

fn threads_action(act: ThreadsAct) -> RobotAction {
    RobotAction::Threads { act }
}

/// The CAD source's threads drawn with the shared thread panel in Robot mode.
struct RobotHost<'a> {
    st: &'a RobotThreads,
    links: &'a [LinkKeys],
    parents: &'a Parents,
    placed: BTreeMap<String, Placed>,
    /// RoboCAD's list number of each thread ("✓" when resolved).
    numbers: HashMap<String, String>,
}
impl RobotHost<'_> {
    /// Selecting the link a node is on (None: on no link: shown, not
    /// pressable). Through the thread act, not `RobotAction::SelectLink`, so
    /// the inspector keeps its scroll.
    fn select(&self, node: &str) -> Option<RobotAction> {
        let i = super::link_of(self.links, self.parents, node)?;
        Some(threads_action(ThreadsAct::SelectLink { index: i, name: self.links[i].name.clone() }))
    }
}
impl Host<CadAnchor> for RobotHost<'_> {
    type Action = RobotAction;
    fn open(&self, thread: &str) -> RobotAction {
        threads_action(ThreadsAct::Open { thread: thread.to_string() })
    }
    fn menu(&self, comment: &str) -> Option<RobotAction> {
        Some(threads_action(ThreadsAct::Menu { comment: comment.to_string() }))
    }
    fn edit(&self, comment: &str) -> Option<RobotAction> {
        Some(threads_action(ThreadsAct::EditMessage { comment: comment.to_string() }))
    }
    fn delete(&self, comment: &str) -> Option<RobotAction> {
        Some(threads_action(ThreadsAct::Delete { thread: self.st.current.clone(), comment: comment.to_string() }))
    }
    /// A part chip selects the link its node is on.
    fn anchor(&self, a: &CadAnchor) -> Option<RobotAction> {
        let i = anchor_link(a, self.links, self.parents)?;
        Some(threads_action(ThreadsAct::SelectLink { index: i, name: self.links[i].name.clone() }))
    }
    /// `[label](part:ID)` selects the link the part is on.
    fn link(&self, _comment: &Comment<CadAnchor>, link: &sim_markdown::Link) -> Option<RobotAction> {
        self.select(link.target.strip_prefix(crate::cad::types::PART_LINK_SCHEME)?)
    }
    fn anchor_text(&self, a: &CadAnchor) -> String {
        use sim_annotate::Anchor;
        match a {
            CadAnchor::Surface { .. } => format!("Pin on {}", a.label()),
            _ => a.label(),
        }
    }
    fn warning(&self, thread: &str) -> Option<String> {
        self.placed.get(thread).and_then(|p| p.warning.clone())
    }
    fn selected(&self, thread: &str) -> bool {
        self.st.current.as_deref() == Some(thread)
    }
    /// RoboCAD's "n · part".
    fn heading(&self, thread: &Thread<CadAnchor>, drawn: usize) -> String {
        let n = self.numbers.get(&thread.id).cloned().unwrap_or_else(|| drawn.to_string());
        format!("{n} · {}", thread.title)
    }
    /// RoboCAD previews the first message.
    fn previewed<'t>(&self, thread: &'t Thread<CadAnchor>) -> Option<&'t Comment<CadAnchor>> {
        thread.comments.first()
    }
}

/// Present: the section, rebuilt when the thread state changes or what it
/// is drawn from (the section, the subject, the loaded model) does.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(mut commands: Commands, view: Res<RobotView>, st: Res<RobotThreads>, fonts: Res<UiFonts>, roots: Query<Entity, With<ThreadsRoot>>, mut last: Local<Option<(Entity, String)>>) {
    let Ok(root) = roots.single() else {
        *last = None;
        return;
    };
    let on = shown(&view);
    let key = format!("{on}|{:?}|{:?}|{}", base_of(&view), link_note(view.cad_link.as_ref()), view.ui_revision);
    if !st.is_changed() && last.as_ref().is_some_and(|(e, k)| *e == root && *k == key) {
        return;
    }
    *last = Some((root, key));
    commands.entity(root).despawn_related::<Children>();
    if !on {
        return;
    }
    let k = Kit { f: &fonts };
    commands.entity(root).with_children(|p| body(p, &k, &view, &st));
}

/// The section's contents (see the module doc).
fn body(p: &mut ChildSpawnerCommands, k: &Kit, view: &RobotView, st: &RobotThreads) {
    p.spawn(k.section("Comments"));
    let base = base_of(view);
    let listed = st.open_listed(base.as_ref());
    p.spawn(k.text(line(view, st), size::DETAIL, if listed.is_some() { SUBTLE } else { WARN }, 0));
    if view.cad_link.as_ref().and_then(cad_path).is_some() {
        p.spawn(wrap()).with_children(|r| {
            r.spawn(k.button("Open in CAD", threads_action(ThreadsAct::OpenInCad { thread: st.current.clone() }), Look::Secondary, true));
            r.spawn(k.button("Read again", threads_action(ThreadsAct::Refresh), Look::Ghost, base.is_some()));
        });
    }
    let Some(listed) = listed else {
        if let Some(e) = &st.error {
            p.spawn(k.text(e.clone(), size::DETAIL, WARN, 0));
        }
        return;
    };
    let links = LinkKeys::of(view);
    let numbers: HashMap<String, String> = listed.threads.iter().enumerate().map(|(i, t)| (t.id.clone(), if t.resolved() { "✓".to_string() } else { (i + 1).to_string() })).collect();
    let host = RobotHost { st, links: &links, parents: &listed.parents, placed: placements(listed, &links, link_note(view.cad_link.as_ref())), numbers };
    kit::filter_row(p, k, st.filter, |shown: Shown| threads_action(ThreadsAct::Filter { shown }));
    let drawn: Vec<Thread<CadAnchor>> = listed.threads.iter().filter(|t| st.filter.keeps(t.resolved())).map(thread_of).collect();
    if kit::list(p, k, &host, drawn.iter()) == 0 {
        p.spawn(k.caption(match (listed.threads.is_empty(), st.filter) {
            (true, _) => "No comments in this CAD document yet.",
            (_, Shown::Resolved) => "No resolved comment threads.",
            (_, Shown::Open) => "No open comment threads.",
            (_, Shown::All) => "No comment threads.",
        }));
    }
    p.spawn(k.text("New comments are placed in CAD mode: Annotate model, then click a surface.", size::DETAIL, FAINT, 0));
    let Some(t) = st.current.as_deref().and_then(|id| listed.threads.iter().find(|t| t.id == id)) else {
        if let Some(e) = &st.error {
            p.spawn(k.text(e.clone(), size::DETAIL, WARN, 0));
        }
        if !listed.threads.is_empty() {
            p.spawn(k.text("Open a thread to read and reply.", size::DETAIL, FAINT, 0));
        }
        return;
    };
    let shown_thread = thread_of(t);
    p.spawn(k.text(format!("{} · {}", t.node_name, attachment(t.anchor_status)), size::BODY, TEXT, 1));
    let busy = st.in_flight.busy();
    p.spawn(wrap()).with_children(|r| {
        r.spawn(k.button("All comments", threads_action(ThreadsAct::Close), Look::Ghost, !st.drafting()));
        let (label, resolved) = if t.resolved() { ("Reopen", false) } else { ("Resolve", true) };
        r.spawn(k.button(label, threads_action(ThreadsAct::Resolve { thread: Some(t.id.clone()), resolved: Some(resolved) }), Look::Secondary, !busy));
    });
    kit::messages(p, k, &host, &shown_thread, st.menu.as_deref());
    let drafting = st.drafting() || st.focus;
    let editing = st.editing.is_some();
    kit::composer(
        p,
        k,
        Composer {
            identity: format!("robot-composer:{:?}:{:?}", st.current, st.editing),
            label: if editing { "Edit message" } else { "Reply" },
            draft: drafting.then_some(st.compose.as_str()),
            placeholder: "Write a reply…",
            min_height: 64.,
            focus: ComposeInput::Compose,
            submit: ComposeInput::Submit,
            // The kit's composer has no disabled state: while a change is being
            // sent the button says so and `post` sends nothing.
            submit_label: if busy { "Sending…" } else if editing { "Save edit" } else { "Reply" },
            cancel: ComposeInput::Cancel,
            author: None,
            error: st.error.as_deref(),
        },
    );
    if st.sending.is_some() {
        p.spawn(k.text("Saving to the CAD file…", size::DETAIL, SUBTLE, 0));
    }
    p.spawn(k.text(NO_UNDO, size::DETAIL, FAINT, 0));
}

/// The composer's post: its draft, unless empty or another change is being sent.
fn post(st: &mut RobotThreads, out: &mut MessageWriter<Act<RobotAction>>) {
    if st.in_flight.busy() {
        st.error = Some(super::BUSY.into());
    } else if !st.compose.trim().is_empty() {
        out.write(Act::ui(threads_action(ThreadsAct::Post)));
    }
}

/// Input (robot mode's Input chain, before its keys): the composer's field
/// messages and presses, mirrored into `RobotThreads` (see the module doc).
#[allow(clippy::too_many_arguments)]
pub(crate) fn input(
    mut st: ResMut<RobotThreads>,
    presses: Query<&ComposeInput, With<Activated>>,
    mut msgs: MessageReader<FieldMsg>,
    mut text: TextFocus,
    view: Res<RobotView>,
    hardware: Option<Res<crate::robot::hardware::Hardware>>,
    picker: Option<Res<crate::app::picker::Picker>>,
    mut out: MessageWriter<Act<RobotAction>>,
) {
    let events: Vec<FieldEvent> = msgs.read().filter(|m| m.field == COMPOSE).map(|m| m.event.clone()).collect();
    if text.suspended(COMPOSE) {
        return;
    }
    let pressed: Vec<ComposeInput> = presses.iter().copied().collect();
    let held = hardware.is_some_and(|h| h.open);
    let picking = picker.is_some_and(|p| p.open.is_some());
    // No field to type into: the section is hidden, or the shown thread is
    // not in the open list (none, gone after a re-read, or the source closed).
    let listed = st.open_listed(base_of(&view).as_ref());
    let gone = !shown(&view) || !st.current.as_ref().is_some_and(|id| listed.is_some_and(|l| l.threads.iter().any(|t| t.id == *id)));
    let focused = text.focused(COMPOSE);
    // Read first: a `ResMut` deref would mark the state changed every frame.
    {
        let s = &*st;
        if events.is_empty() && pressed.is_empty() && !s.claim && !s.release && s.focus == focused && !(focused && (held || picking || gone)) {
            return;
        }
    }
    let st = &mut *st;
    for event in events {
        match event {
            FieldEvent::Changed(draft) => {
                if st.compose != draft.text {
                    st.compose = draft.text;
                    st.error = None;
                }
            }
            FieldEvent::Submit(typed) => {
                st.compose = typed;
                post(st, &mut out);
                text.blur(COMPOSE);
            }
            // The kit has taken the keyboard already.
            FieldEvent::Cancel => {
                out.write(Act::ui(threads_action(ThreadsAct::Discard)));
            }
            FieldEvent::Tab { .. } | FieldEvent::Arrow { .. } | FieldEvent::Blur => {}
        }
    }
    for press in pressed {
        match press {
            ComposeInput::Compose if held => st.error = Some(HARDWARE_HAS_KEYS.into()),
            ComposeInput::Compose if picking || gone => {}
            ComposeInput::Compose => {
                text.focus_draft(COMPOSE, TextDraft::new(st.compose.clone(), false));
            }
            ComposeInput::Submit => {
                post(st, &mut out);
                text.blur(COMPOSE);
            }
            ComposeInput::Cancel => {
                out.write(Act::ui(threads_action(ThreadsAct::Discard)));
                text.blur(COMPOSE);
            }
        }
    }
    // The draft was posted and taken, or cancelled: the keyboard goes.
    if st.release {
        st.release = false;
        text.blur(COMPOSE);
        text.set(COMPOSE, TextDraft::default());
    }
    // Edit message filled the composer: it takes the keyboard with that text.
    if st.claim {
        st.claim = false;
        if held {
            st.error = Some(HARDWARE_HAS_KEYS.into());
        } else if !picking && !gone {
            text.focus_draft(COMPOSE, TextDraft::new(st.compose.clone(), false));
        }
    }
    if text.focused(COMPOSE) && (held || picking || gone) {
        text.blur(COMPOSE);
    }
    // The state is the draft's record: a focused field shows it.
    if text.focused(COMPOSE) && text.draft(COMPOSE).is_some_and(|d| d.text != st.compose) {
        text.set(COMPOSE, TextDraft::new(st.compose.clone(), false));
    }
    let focus = text.focused(COMPOSE);
    if st.focus != focus {
        st.focus = focus;
    }
}
