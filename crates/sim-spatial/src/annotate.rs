//! Reusable annotation views for any host: a thread list, a thread's
//! messages and a reply composer, generic over the anchor type
//! (`sim_annotate::Anchor`) and the host's action component. The builder's
//! Notes tab (system discussions) and the lesson margin (lesson threads)
//! draw with these; hosts own persistence, drafts and what a click does.
use crate::builder::ui::markdown_theme;
use crate::ui_kit::{ACCENT, ACCENT_BG, BORDER, FAINT, HOVER_BG, Kit, Look, RAISED, SUBTLE, TEXT, Tint, WARN, size, wrap};
use bevy::prelude::*;
use sim_annotate::{Anchor, Comment, Thread};

/// What the host does when a view element is clicked.
pub(crate) trait Host<A: Anchor> {
    type Action: Component + Clone;
    fn open(&self, thread: &str) -> Self::Action;
    fn menu(&self, comment: &str) -> Self::Action;
    fn edit(&self, comment: &str) -> Self::Action;
    fn delete(&self, comment: &str) -> Self::Action;
    /// Clicking an anchor chip (None: not clickable).
    fn anchor(&self, anchor: &A) -> Option<Self::Action>;
    /// A Markdown link inside a comment body.
    fn link(&self, comment: &Comment<A>, link: &sim_markdown::Link) -> Option<Self::Action>;
    /// Chip text for an anchor.
    fn anchor_text(&self, anchor: &A) -> String {
        anchor.label()
    }
    /// Status line under a thread card (e.g. "Codex working").
    fn badge(&self, _thread: &str) -> Option<String> {
        None
    }
    /// Whether this thread is the selected one (list highlight).
    fn selected(&self, _thread: &str) -> bool {
        false
    }
}

/// Anchor chips: clickable when attached, marked missing otherwise.
pub(crate) fn anchors<A: Anchor, H: Host<A>>(body: &mut ChildSpawnerCommands, k: &Kit, host: &H, anchors: &[A]) {
    if anchors.is_empty() {
        return;
    }
    body.spawn(wrap()).with_children(|r| {
        for a in anchors {
            let text = host.anchor_text(a);
            match host.anchor(a).filter(|_| !a.missing()) {
                Some(action) => {
                    r.spawn(k.chip(&format!("↗ {text}"), action, false, true));
                }
                None => {
                    r.spawn(k.text(if a.missing() { format!("{text} · missing") } else { text }, size::DETAIL, if a.missing() { WARN } else { SUBTLE }, 0));
                }
            }
        }
    });
}

/// One card per thread; returns how many were drawn.
pub(crate) fn list<'t, A: Anchor + 't, H: Host<A>>(body: &mut ChildSpawnerCommands, k: &Kit, host: &H, threads: impl IntoIterator<Item = &'t Thread<A>>) -> usize {
    let mut count = 0;
    for t in threads {
        count += 1;
        let selected = host.selected(&t.id);
        body.spawn((
            Button,
            host.open(&t.id),
            bevy::ui::prelude::AccessibleLabel::new(t.title.clone()),
            if selected { Tint::new(ACCENT_BG, HOVER_BG) } else { Tint::RAISED },
            Node { border_radius: BorderRadius::all(Val::Px(7.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(7.), padding: UiRect::all(Val::Px(12.)), flex_shrink: 0., border: UiRect::left(Val::Px(if selected { 2. } else { 0. })), ..default() },
            BackgroundColor(if selected { ACCENT_BG } else { RAISED }),
            BorderColor::all(ACCENT),
        ))
        .with_children(|card| {
            card.spawn(k.text(format!("{count}  {}{}", t.title, if t.resolved { "  · resolved" } else { "" }), 14., TEXT, 2));
            if let Some(label) = host.badge(&t.id) {
                card.spawn(k.text(label, size::DETAIL, ACCENT, 1));
            }
            let places: Vec<String> = t.targets.iter().map(|a| if a.missing() { format!("{} (missing)", host.anchor_text(a)) } else { host.anchor_text(a) }).collect();
            card.spawn(k.text(places.join(" · "), size::DETAIL, ACCENT, 0));
            if let Some(c) = t.comments.last() {
                card.spawn(k.text(sim_markdown::parse(&c.body).plain().chars().take(90).collect::<String>(), size::BODY, SUBTLE, 0));
                card.spawn(k.text(format!("{} message{} · {}", t.comments.len(), if t.comments.len() == 1 { "" } else { "s" }, sim_annotate::relative_time(&c.created_at)), 10.5, FAINT, 0));
            }
        });
    }
    count
}

/// Every comment of a thread: author, time, Markdown body, link chips and
/// (for the comment whose menu is open) edit/delete.
pub(crate) fn messages<A: Anchor, H: Host<A>>(body: &mut ChildSpawnerCommands, k: &Kit, host: &H, thread: &Thread<A>, menu: Option<&str>) {
    for c in &thread.comments {
        body.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(7.), padding: UiRect::bottom(Val::Px(8.)), flex_shrink: 0., ..default() }).with_children(|message| {
            message.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|r| {
                r.spawn(k.text(&c.author, size::ITEM, TEXT, 2));
                r.spawn(k.button("···", host.menu(&c.id), Look::Ghost, true));
            });
            message.spawn(k.text(format!("{}{}", sim_annotate::relative_time(&c.created_at), if c.edited_at.is_some() { " · edited" } else { "" }), 10.5, FAINT, 0));
            let parsed = sim_markdown::parse(&c.body);
            crate::markdown::render(message, &parsed, &markdown_theme(k), |link| host.link(c, link));
            anchors(message, k, host, &c.links);
            if menu == Some(c.id.as_str()) {
                message.spawn(wrap()).with_children(|r| {
                    r.spawn(k.button("Edit", host.edit(&c.id), Look::Ghost, true));
                    r.spawn(k.button("Delete", host.delete(&c.id), Look::Danger, true));
                });
            }
        });
    }
}

/// A reply field that shows the draft (with a caret) while focused.
pub(crate) struct Composer<'a, Act> {
    pub label: &'a str,
    pub draft: Option<&'a str>,
    pub placeholder: &'a str,
    pub focus: Act,
    pub submit: Act,
    pub submit_label: &'a str,
    pub cancel: Act,
    pub author: Option<(&'a str, Act)>,
    pub error: Option<&'a str>,
}
pub(crate) fn composer<Act: Component + Clone>(body: &mut ChildSpawnerCommands, k: &Kit, c: Composer<Act>) {
    let focused = c.draft.is_some();
    let shown = c.draft.unwrap_or("");
    body.spawn(k.text(c.label, size::SMALL, SUBTLE, 1));
    body.spawn((
        Button,
        c.focus,
        Tint::RAISED,
        Node { border_radius: BorderRadius::all(Val::Px(7.)), min_height: Val::Px(64.), max_height: Val::Px(180.), overflow: Overflow::clip(), padding: UiRect::all(Val::Px(10.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
        BackgroundColor(RAISED),
        BorderColor::all(if focused { ACCENT } else { BORDER }),
    ))
    .with_children(|field| {
        field.spawn(k.text(if focused { format!("{shown}|") } else { c.placeholder.to_string() }, 14., if focused { TEXT } else { FAINT }, 0));
    });
    body.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, flex_shrink: 0., ..default() }).with_children(|r| {
        if focused {
            r.spawn(k.button("Cancel", c.cancel, Look::Ghost, true));
            r.spawn(k.button(c.submit_label, c.submit, Look::Primary, !shown.trim().is_empty()));
        } else if let Some((name, action)) = c.author {
            r.spawn(k.button(name, action, Look::Ghost, true));
        }
    });
    if let Some(error) = c.error {
        body.spawn(k.text(error, size::DETAIL, WARN, 0));
    }
    if focused {
        body.spawn(k.text("Enter to post · Shift+Enter for a new line · Esc cancels", 10.5, FAINT, 0));
    }
}
