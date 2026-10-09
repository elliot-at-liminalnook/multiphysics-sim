//! The Comments section at the top of the right dock (`panel::Part::Comments`;
//! RoboCAD's `CommentsPanel`, ui/comments.py:128-308), drawn while shown
//! (`view.comments` shows it, Close hides it). The thread list, the
//! messages, the anchor chips and the composer are the one thread panel's
//! (`ui_kit::threads`) through [`CadHost`]; every button takes its action
//! and enabled state from `threads::controls_of` (what `system_ui` lists),
//! and writes it as `panel::CadButton` does.
//!
//! Top to bottom, as RoboCAD lays it out: "＋ Annotate model", the Open /
//! All / Resolved filter and "Selected parts only"; the list ("n · part"
//! with the first message, ✓ for a resolved one) or the read's state; the
//! location line ("part · Attached to surface", the other attachment
//! states, the evidence run, or "New annotation on …"); Show on model, Fit
//! in view, Reattach…, Resolve/Reopen; "Parts in this discussion" (label,
//! description, " · deleted"; a press highlights the part), Link selected
//! parts, Rename part label… (its dialog inline: "Part label for this
//! discussion", "Plain-language label:"), Show only linked parts, Return to
//! assembly; the messages (part links press to show the part alone; "···"
//! opens Edit and Delete); the author; Insert part link from selection; the
//! composer (Reply, Post annotation or Save edit; Cancel); Delete thread.
use super::source::thread_of;
use super::{CadAnchor, Field, Filter, ThreadsArgs, ThreadsOp, attachment, controls_of, read, shown_threads};
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::panel::{CadButton, Control};
use crate::cad::selection::CadItems;
use crate::ui_kit::threads::{self, Composer, Host};
use crate::ui_kit::{ACCENT, DANGER, FAINT, Kit, Look, SUBTLE, TEXT, WARN, size, wrap};
use bevy::prelude::*;
use sim_annotate::{Comment, Thread};
use crate::cad::types::{AnchorStatus, SelectionItem};
use std::collections::HashMap;

/// A press on the composer's parts and the dock's text fields (`input`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThreadsInput {
    Compose,
    Submit,
    Cancel,
    Author,
    Label,
}

/// RoboCAD's threads drawn with the shared thread panel.
struct CadHost<'a> {
    doc: &'a CadDocument,
    /// RoboCAD's list number of each thread ("✓" when resolved).
    numbers: HashMap<&'a str, String>,
}
impl Host<CadAnchor> for CadHost<'_> {
    type Action = CadButton;
    fn open(&self, thread: &str) -> CadButton {
        CadButton(ThreadsArgs::thread(ThreadsOp::Open, Some(thread)))
    }
    fn menu(&self, comment: &str) -> Option<CadButton> {
        Some(CadButton(ThreadsArgs { op: ThreadsOp::Menu, comment: Some(comment.into()), ..ThreadsArgs::default() }.action()))
    }
    fn edit(&self, comment: &str) -> Option<CadButton> {
        Some(CadButton(ThreadsArgs { op: ThreadsOp::EditMessage, comment: Some(comment.into()), ..ThreadsArgs::default() }.action()))
    }
    fn delete(&self, comment: &str) -> Option<CadButton> {
        Some(CadButton(ThreadsArgs { op: ThreadsOp::Delete, comment: Some(comment.into()), ..ThreadsArgs::default() }.action()))
    }
    /// A chip selects its part through the shared selection (as a tree row does).
    fn anchor(&self, a: &CadAnchor) -> Option<CadButton> {
        let node = a.node()?;
        exists(self.doc, node).then(|| CadButton(CadAction::CadSelect { ids: vec![node.to_string()], items: Vec::new(), extend: false, toggle: false, picked_at: None }))
    }
    /// `[label](part:ID)`: selects the part and shows it alone (`open_part_link`).
    fn link(&self, _comment: &Comment<CadAnchor>, link: &sim_markdown::Link) -> Option<CadButton> {
        let id = link.target.strip_prefix(crate::cad::types::PART_LINK_SCHEME)?;
        exists(self.doc, id).then(|| CadButton(ThreadsArgs { op: ThreadsOp::PartLink, node: Some(id.to_string()), ..ThreadsArgs::default() }.action()))
    }
    fn anchor_text(&self, a: &CadAnchor) -> String {
        use sim_annotate::Anchor;
        match a {
            CadAnchor::Surface { .. } => format!("Pin on {}", a.label()),
            _ => a.label(),
        }
    }
    fn selected(&self, thread: &str) -> bool {
        self.doc.threads.current.as_deref() == Some(thread)
    }
    fn color(&self, thread: &str) -> Option<Color> {
        read::thread(self.doc, thread).filter(|t| t.anchor_status == AnchorStatus::NeedsReview).map(|_| super::pins::REVIEW)
    }
    /// RoboCAD's "n · part" (comments.py:257-259).
    fn heading(&self, thread: &Thread<CadAnchor>, drawn: usize) -> String {
        let n = self.numbers.get(thread.id.as_str()).cloned().unwrap_or_else(|| drawn.to_string());
        format!("{n} · {}", thread.title)
    }
    /// RoboCAD previews the first message.
    fn previewed<'t>(&self, thread: &'t Thread<CadAnchor>) -> Option<&'t Comment<CadAnchor>> {
        thread.comments.first()
    }
}

/// Whether node `id` is in the shown tree.
fn exists(doc: &CadDocument, id: &str) -> bool {
    doc.doc.as_ref().is_some_and(|d| d.nodes.iter().any(|n| n.id == id))
}

/// The control `cad:threads:<id>`.
fn control<'c>(all: &'c [Control], id: &str) -> Option<&'c Control> {
    let id = format!("cad:threads:{id}");
    all.iter().find(|c| c.id == id)
}

/// A kit button for control `id` (nothing when it is not listed).
fn button(p: &mut ChildSpawnerCommands, k: &Kit, all: &[Control], id: &str, look: Look) {
    if let Some(c) = control(all, id) {
        p.spawn(k.button(&c.label, CadButton(c.action.clone()), look, c.ready.is_ok()));
    }
}

/// A kit chip for control `id`.
fn chip(p: &mut ChildSpawnerCommands, k: &Kit, all: &[Control], id: &str, on: bool) {
    if let Some(c) = control(all, id) {
        p.spawn(k.chip(&c.label, CadButton(c.action.clone()), on, c.ready.is_ok()));
    }
}

/// What the section shows now (the panel redraws it when this changes).
pub(in crate::cad) fn key(doc: &CadDocument, selection: &[SelectionItem]) -> String {
    let st = &doc.threads;
    if !st.open {
        return "closed".into();
    }
    let ready: Vec<(String, bool)> = controls_of(doc, selection).into_iter().map(|c| (c.label, c.ready.is_ok())).collect();
    format!(
        "{:?}",
        (
            (read::listed(doc), read::line(doc), st.filter, st.selected_only, &st.current, &st.part, &st.menu),
            (&st.compose, &st.pending, &st.editing, &st.author, &st.label, st.focus, &st.error, st.sending.as_ref().map(|s| s.0)),
            (st.tool.is_some(), st.isolation.as_ref().map(|i| &i.parts), selection.nodes(), doc.doc.as_ref().map(|d| d.nodes.len()), ready, super::draft_gone(doc)),
            (st.current.as_deref().and_then(|t| st.ai.line(t)), st.ai.auto()),
        )
    )
}

/// The section (see the module doc); nothing while it is hidden.
pub(in crate::cad) fn draw(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, selection: &[SelectionItem]) {
    let st = &doc.threads;
    if !st.open {
        return;
    }
    let all = controls_of(doc, selection);
    let listed = read::listed(doc).unwrap_or(&[]);
    let numbers: HashMap<&str, String> = listed.iter().enumerate().map(|(i, t)| (t.id.as_str(), if t.resolved() { "✓".to_string() } else { (i + 1).to_string() })).collect();
    let host = CadHost { doc, numbers };
    p.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::FlexEnd, flex_shrink: 0., ..default() }).with_children(|r| {
        r.spawn(k.section("Comments"));
        button(r, k, &all, "close", Look::Ghost);
    });
    p.spawn(wrap()).with_children(|r| {
        button(r, k, &all, "annotate", Look::Secondary);
        for f in Filter::ALL {
            chip(r, k, &all, &format!("filter-{}", f.name()), st.filter == f);
        }
    });
    p.spawn(wrap()).with_children(|r| chip(r, k, &all, "selected_only", st.selected_only));
    if st.tool.is_some() {
        p.spawn(k.text(super::HINT, size::DETAIL, ACCENT, 0));
        p.spawn(wrap()).with_children(|r| button(r, k, &all, "cancel", Look::Ghost));
    }
    if let Some(line) = read::line(doc) {
        p.spawn(k.text(line, size::DETAIL, WARN, 0));
    }
    let shown: Vec<Thread<CadAnchor>> = shown_threads(doc, selection).into_iter().map(|(_, t)| thread_of(t)).collect();
    if threads::list(p, k, &host, shown.iter()) == 0 && read::listed(doc).is_some() {
        p.spawn(k.caption(match (listed.is_empty(), st.filter, st.selected_only) {
            (true, ..) => "No comments in this document yet.",
            (_, _, true) => "No comment threads on the selected parts.",
            (_, Filter::Resolved, _) => "No resolved comment threads.",
            _ => "No open comment threads.",
        }));
    }
    let current = st.current.as_deref().and_then(|id| read::thread(doc, id));
    // The location line (comments.py:283-287, 306, 341).
    let location = match (&st.pending, current) {
        (Some(pin), _) => format!("New annotation on {}", doc.node_name(&pin.node)),
        (None, Some(t)) => {
            let mut line = format!("{} · {}", t.node_name, attachment(t.anchor_status));
            if let Some(ev) = &t.evidence {
                let run: String = ev["run_id"].as_str().unwrap_or("").chars().take(8).collect();
                line.push_str(&format!("\nRun {run} · {} · {} s", ev["signal"].as_str().unwrap_or(""), time_range(ev.get("time_range"))));
            }
            line
        }
        (None, None) if listed.is_empty() => "Select Annotate model, then click a surface.".to_string(),
        (None, None) => "Click Annotate model to start a discussion on a surface.".to_string(),
    };
    let colour = match current.map(|t| t.anchor_status) {
        Some(AnchorStatus::Missing | AnchorStatus::NeedsReview) if st.pending.is_none() => WARN,
        _ => TEXT,
    };
    p.spawn(k.text(location, size::BODY, colour, 1));
    if current.is_some() {
        p.spawn(wrap()).with_children(|r| {
            for id in ["show", "fit", "reattach", "resolve"] {
                button(r, k, &all, id, Look::Secondary);
            }
        });
    }
    if let Some(t) = current {
        p.spawn(k.caption("Parts in this discussion · press to highlight a part"));
        for part in &t.linked_parts {
            let Some(c) = control(&all, &format!("part-{}", part.part.node_id)) else { continue };
            let label = part.part.label.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| part.name.clone());
            let description = part.part.description.clone().filter(|d| !d.is_empty()).or_else(|| (part.available && part.name != label).then(|| part.name.clone()));
            let text = format!("{label}{}{}", description.map_or_else(String::new, |d| format!("\n{d}")), if part.available { "" } else { " · deleted" });
            let on = st.part.as_deref() == Some(part.part.node_id.as_str());
            p.spawn(k.chip(&text, CadButton(c.action.clone()), on, c.ready.is_ok()));
        }
        p.spawn(wrap()).with_children(|r| {
            button(r, k, &all, "link_selected", Look::Secondary);
            button(r, k, &all, "label", Look::Secondary);
        });
        if let Some(dialog) = st.label.as_ref().filter(|l| l.thread == t.id) {
            label_dialog(p, k, doc, dialog);
        }
    }
    if current.is_some() || st.isolation.is_some() {
        p.spawn(wrap()).with_children(|r| {
            button(r, k, &all, "show_parts", Look::Secondary);
            button(r, k, &all, "return", Look::Secondary);
        });
    }
    if st.isolation.is_some() {
        p.spawn(k.text("Showing linked parts only · Esc or Return to assembly restores your view", size::DETAIL, ACCENT, 0));
    }
    if let Some(t) = current {
        // Messages, with a deleted part's link named as RoboCAD's `comment_html` names it.
        let mut shown = thread_of(t);
        for c in &mut shown.comments {
            for (label, id) in crate::cad::types::part_links(&c.body) {
                if !exists(doc, &id) {
                    c.body = c.body.replace(&crate::cad::types::part_link(&label, &id), &format!("{label} (part deleted)"));
                }
            }
        }
        threads::messages(p, k, &host, &shown, st.menu.as_deref());
        // The AI: what it is doing on this thread, Ask AI and the automatic answers.
        if let Some(line) = st.ai.line(&t.id) {
            p.spawn(k.text(line, size::DETAIL, ACCENT, 0));
        }
        p.spawn(wrap()).with_children(|r| {
            button(r, k, &all, "ask", Look::Secondary);
            chip(r, k, &all, "ai_auto", st.ai.auto());
        });
    }
    let drafting = st.drafting() || st.focus == Some(Field::Compose);
    // A draft stays drawn (with its Cancel) when its thread or message is
    // gone since: RoboCAD's composer is always there.
    let gone = super::draft_gone(doc);
    if current.is_some() || st.pending.is_some() || st.drafting() {
        p.spawn(k.caption("Author"));
        p.spawn(k.input(&st.author, "Your display name", ThreadsInput::Author, st.focus == Some(Field::Author)));
        p.spawn(wrap()).with_children(|r| button(r, k, &all, "insert_link", Look::Ghost));
        let (label, placeholder) = match (&st.pending, &st.editing) {
            (Some(_), _) => ("New annotation", "What would you like to discuss about this surface?"),
            (None, Some(_)) => ("Edit message", "Write a reply…"),
            _ => ("Reply", "Write a reply…"),
        };
        let submit = control(&all, "post").map_or("Reply", |c| c.label.as_str());
        threads::composer(
            p,
            k,
            Composer {
                identity: format!("cad-composer:{:?}:{:?}:{:?}",st.current,st.editing,st.pending),
                label,
                draft: drafting.then_some(st.compose.as_str()),
                placeholder,
                min_height: 64.,
                focus: ThreadsInput::Compose,
                submit: ThreadsInput::Submit,
                submit_label: submit,
                cancel: ThreadsInput::Cancel,
                author: None,
                error: st.error.as_deref().or(gone.as_deref()),
            },
        );
        if st.sending.is_some() {
            p.spawn(k.text("Saving…", size::DETAIL, SUBTLE, 0));
        }
    }
    if current.is_some() {
        p.spawn(wrap()).with_children(|r| button(r, k, &all, "delete_thread", Look::Danger));
    }
    if current.is_none() && st.pending.is_none() && !listed.is_empty() {
        p.spawn(k.text("Open a thread to read and reply.", size::DETAIL, FAINT, 0));
    }
}

/// An evidence time range as RoboCAD's location line prints it: Python's
/// `str()` of `ev.get('time_range', [])` (`[0.5, 2.0]`, `[]` when absent,
/// `None` for null).
pub(super) fn time_range(v: Option<&serde_json::Value>) -> String {
    v.map_or_else(|| "[]".to_string(), |v| python_str(v, true))
}

/// Python's `str()` of a JSON value (`repr()` inside a list or a dict).
fn python_str(v: &serde_json::Value, top: bool) -> String {
    use serde_json::Value;
    match v {
        Value::Null => "None".into(),
        Value::Bool(b) => (if *b { "True" } else { "False" }).to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) if top => s.clone(),
        Value::String(s) => format!("'{s}'"),
        Value::Array(items) => format!("[{}]", items.iter().map(|i| python_str(i, false)).collect::<Vec<_>>().join(", ")),
        Value::Object(m) => format!("{{{}}}", m.iter().map(|(k, i)| format!("'{k}': {}", python_str(i, false))).collect::<Vec<_>>().join(", ")),
    }
}

/// RoboCAD's `QInputDialog` "Part label for this discussion" /
/// "Plain-language label:", inline under the parts.
fn label_dialog(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, dialog: &super::LabelDialog) {
    let st = &doc.threads;
    p.spawn(k.text("Part label for this discussion", size::ITEM, TEXT, 2));
    p.spawn(k.caption("Plain-language label:"));
    p.spawn(k.input(&dialog.text, "", ThreadsInput::Label, st.focus == Some(Field::Label)));
    let too_long = dialog.text.chars().count() > super::source::MAX_LABEL;
    if too_long {
        p.spawn(k.text(format!("Part label must be text of at most {} characters", super::source::MAX_LABEL), size::DETAIL, DANGER, 0));
    }
    let ok = CadButton(ThreadsArgs { op: ThreadsOp::Label, thread: Some(dialog.thread.clone()), node: Some(dialog.node.clone()), label: Some(dialog.text.clone()), ..ThreadsArgs::default() }.action());
    let ready = !too_long && doc.edit_refusal().is_none();
    p.spawn(wrap()).with_children(|r| {
        r.spawn(k.button("OK", ok, Look::Primary, ready));
        r.spawn(k.button("Cancel", CadButton(ThreadsArgs::of(ThreadsOp::LabelCancel).action()), Look::Ghost, true));
    });
}
