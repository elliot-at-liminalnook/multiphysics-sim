//! The one thread panel (native-viewer.md §7): a thread list, a thread's
//! messages, anchor chips, an inline thread card and the reply composer,
//! generic over the anchor type (`sim_annotate::Anchor`) and the host's
//! action component. The builder's Notes tab (system discussions), the
//! lesson margin (lesson notes) and Inspect's notes panel draw with these;
//! the threads come from `crate::annotations` sources, and hosts own what
//! a press means. A reply's text is the host's kit text field's
//! (`ui_kit::text`): the host passes its draft to [`Composer`]. The
//! composer's text area and its Cancel / submit buttons are tagged
//! `KitInput`, so pressing them does not take the keyboard from the field
//! (the host decides: Post submits the draft, Cancel ends it).
use super::text::KitInput;
use super::{ACCENT, ACCENT_BG, BORDER, FAINT, HOVER_BG, Kit, Look, RAISED, SUBTLE, TEXT, Tint, WARN, size, wrap};
use bevy::ui::prelude::AccessibleLabel;
use crate::builder::ui::markdown_theme;
use bevy::prelude::*;
use sim_annotate::{Anchor, Comment, Thread};

/// What the host does when a part of the panel is pressed.
pub(crate) trait Host<A: Anchor> {
    type Action: Component + Clone;
    /// Opening a thread (a card, a title).
    fn open(&self, thread: &str) -> Self::Action;
    /// A comment's "···" menu (None: comments have no menu).
    fn menu(&self, comment: &str) -> Option<Self::Action>;
    fn edit(&self, comment: &str) -> Option<Self::Action>;
    fn delete(&self, comment: &str) -> Option<Self::Action>;
    /// Pressing an anchor chip (None: not pressable).
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
    /// The thread's own colour (a card's frame and title), if it has one.
    fn color(&self, _thread: &str) -> Option<Color> {
        None
    }
    /// A list card's heading; `drawn` is its place in the drawn list. The
    /// default is "{drawn}  {title}" with "  · resolved" when resolved (a
    /// source that numbers its threads itself, as RoboCAD's comments, says so).
    fn heading(&self, thread: &Thread<A>, drawn: usize) -> String {
        format!("{drawn}  {}{}", thread.title, if thread.resolved { "  · resolved" } else { "" })
    }
    /// The message a list card previews (default: the latest).
    fn previewed<'t>(&self, thread: &'t Thread<A>) -> Option<&'t Comment<A>> {
        thread.comments.last()
    }
}

/// Anchor chips: pressable when attached, marked missing otherwise.
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
                    // A missing anchor is named by its own label (what the
                    // source last called it), as each host showed it before.
                    r.spawn(k.text(if a.missing() { format!("{} · missing", a.label()) } else { text }, size::DETAIL, if a.missing() { WARN } else { SUBTLE }, 0));
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
        let edge = host.color(&t.id).unwrap_or(ACCENT);
        body.spawn((
            Button,
            host.open(&t.id),
            bevy::ui::prelude::AccessibleLabel::new(t.title.clone()),
            if selected { Tint::new(ACCENT_BG, HOVER_BG) } else { Tint::RAISED },
            Node { border_radius: BorderRadius::all(Val::Px(7.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(7.), padding: UiRect::all(Val::Px(12.)), flex_shrink: 0., border: UiRect::left(Val::Px(if selected { 2. } else { 0. })), ..default() },
            BackgroundColor(if selected { ACCENT_BG } else { RAISED }),
            BorderColor::all(edge),
        ))
        .with_children(|card| {
            card.spawn(k.text(host.heading(t, count), 14., TEXT, 2));
            if let Some(label) = host.badge(&t.id) {
                card.spawn(k.text(label, size::DETAIL, ACCENT, 1));
            }
            let places: Vec<String> = t.targets.iter().map(|a| if a.missing() { format!("{} (missing)", host.anchor_text(a)) } else { host.anchor_text(a) }).collect();
            card.spawn(k.text(places.join(" · "), size::DETAIL, ACCENT, 0));
            if let Some(c) = host.previewed(t) {
                card.spawn(k.text(sim_markdown::parse(&c.body).plain().chars().take(90).collect::<String>(), size::BODY, SUBTLE, 0));
                if !c.created_at.is_empty() {
                    card.spawn(k.text(format!("{} message{} · {}", t.comments.len(), if t.comments.len() == 1 { "" } else { "s" }, sim_annotate::relative_time(&c.created_at)), 10.5, FAINT, 0));
                }
            }
        });
    }
    count
}

/// Every comment of a thread: author, time, Markdown body, link chips and
/// (for the comment whose menu is open) edit and delete. A comment without
/// an author or a time (an Inspect note's text) shows only its body and links.
pub(crate) fn messages<A: Anchor, H: Host<A>>(body: &mut ChildSpawnerCommands, k: &Kit, host: &H, thread: &Thread<A>, menu: Option<&str>) {
    for c in &thread.comments {
        body.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(7.), padding: UiRect::bottom(Val::Px(8.)), flex_shrink: 0., ..default() }).with_children(|message| {
            if !c.author.is_empty() {
                message.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|r| {
                    r.spawn(k.text(&c.author, size::ITEM, TEXT, 2));
                    if let Some(action) = host.menu(&c.id) {
                        r.spawn(k.button("···", action, Look::Ghost, true));
                    }
                });
            }
            if !c.created_at.is_empty() {
                message.spawn(k.text(format!("{}{}", sim_annotate::relative_time(&c.created_at), if c.edited_at.is_some() { " · edited" } else { "" }), 10.5, FAINT, 0));
            }
            let parsed = sim_markdown::parse(&c.body);
            crate::markdown::render(message, &parsed, &markdown_theme(k), |link| host.link(c, link));
            anchors(message, k, host, &c.links);
            if menu == Some(c.id.as_str()) {
                message.spawn(wrap()).with_children(|r| {
                    if let Some(action) = host.edit(&c.id) {
                        r.spawn(k.button("Edit", action, Look::Ghost, true));
                    }
                    if let Some(action) = host.delete(&c.id) {
                        r.spawn(k.button("Delete", action, Look::Danger, true));
                    }
                });
            }
        });
    }
}

/// A whole thread inline (short threads listed with their messages, as
/// Inspect's notes): a card framed in the thread's colour, its title
/// (pressing it opens the thread) and every message.
pub(crate) fn card<A: Anchor, H: Host<A>>(body: &mut ChildSpawnerCommands, k: &Kit, host: &H, thread: &Thread<A>) {
    let color = host.color(&thread.id).unwrap_or(ACCENT);
    body.spawn((
        Node { border_radius: BorderRadius::all(Val::Px(6.)), flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(10.)), row_gap: Val::Px(5.), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
        BorderColor::all(color),
        BackgroundColor(RAISED),
    ))
    .with_children(|card| {
        // The title keeps the thread's colour, so it is a tinted row rather
        // than a kit button (whose label colour comes from its look).
        card.spawn((
            Button,
            host.open(&thread.id),
            Tint::CLEAR,
            bevy::ui::prelude::AccessibleLabel::new(thread.title.as_str()),
            Node { border_radius: BorderRadius::all(Val::Px(4.)), padding: UiRect::axes(Val::Px(4.), Val::Px(2.)), ..default() },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|b| {
            b.spawn(k.text(&thread.title, 15., color, 2));
        });
        messages(card, k, host, thread, None);
    });
}

/// A reply field that shows the draft (the host's kit text field's, with a
/// caret) while focused.
pub(crate) struct Composer<'a, Act> {
    pub label: &'a str,
    pub draft: Option<&'a str>,
    pub placeholder: &'a str,
    /// The field's least height (a one-line name or title field is shorter).
    pub min_height: f32,
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
    // A multi-line text area (taller, 14 px), not the kit's one-line `input`.
    body.spawn((
        Button,
        c.focus,
        KitInput,
        AccessibleLabel::new(if focused && !shown.is_empty() { format!("{}: {shown}", c.label) } else { c.label.to_string() }),
        Tint::RAISED,
        Node { border_radius: BorderRadius::all(Val::Px(7.)), min_height: Val::Px(c.min_height), max_height: Val::Px(180.), overflow: Overflow::clip(), padding: UiRect::all(Val::Px(10.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() },
        BackgroundColor(RAISED),
        BorderColor::all(if focused { ACCENT } else { BORDER }),
    ))
    .with_children(|field| {
        field.spawn(k.text(if focused { format!("{shown}|") } else { c.placeholder.to_string() }, 14., if focused { TEXT } else { FAINT }, 0));
    });
    body.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, flex_shrink: 0., ..default() }).with_children(|r| {
        if focused {
            r.spawn((k.button("Cancel", c.cancel, Look::Ghost, true), KitInput));
            r.spawn((k.button(c.submit_label, c.submit, Look::Primary, !shown.trim().is_empty()), KitInput));
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
