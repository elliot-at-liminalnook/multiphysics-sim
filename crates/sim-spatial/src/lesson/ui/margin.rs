//! The right dock: the picked part, notes and the agent.
use super::*;

/// Lesson threads drawn with the shared annotation views.
struct MarginHost<'a> {
    l: &'a Learn,
}
impl Host<LessonAnchor> for MarginHost<'_> {
    type Action = LessonAction;
    fn open(&self, thread: &str) -> LessonAction {
        LessonAction::OpenThread(thread.into())
    }
    fn menu(&self, comment: &str) -> LessonAction {
        LessonAction::CommentMenu(comment.into())
    }
    fn edit(&self, comment: &str) -> LessonAction {
        LessonAction::EditComment(comment.into())
    }
    fn delete(&self, comment: &str) -> LessonAction {
        LessonAction::DeleteComment(comment.into())
    }
    fn anchor(&self, a: &LessonAnchor) -> Option<LessonAction> {
        match a {
            LessonAnchor::Text { .. } => self.l.index.as_ref().and_then(|i| a.block(i)).map(|b| LessonAction::Goto(b.into())),
            LessonAnchor::Scene { scene, part, time_s, .. } => Some(LessonAction::ShowAt { scene: scene.clone(), time: *time_s, part: part.clone() }),
        }
    }
    fn link(&self, _c: &sim_annotate::Comment<LessonAnchor>, link: &sim_markdown::Link) -> Option<LessonAction> {
        self.l.lesson.as_ref().and_then(|lesson| link_action(lesson, link))
    }
    fn badge(&self, thread: &str) -> Option<String> {
        self.l.agent.badge(thread)
    }
    fn selected(&self, thread: &str) -> bool {
        self.l.thread.as_deref() == Some(thread)
    }
}

pub(super) fn margin(commands: &mut Commands, k: &Kit, l: &Learn, builder: Option<&Builder>, scene: &SpatialScene, offset: f32) {
    let host = MarginHost { l };
    let threads = l.threads();
    commands
        .spawn((k.dock(Dock::Right { top: TOPBAR, bottom: STATUSBAR, width: RIGHT_WIDTH }, Node { flex_direction: FlexDirection::Column, ..default() }), LearnPanel))
        .with_children(|panel| {
            panel
                .spawn((k.scroll_area(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(10.), padding: UiRect::all(Val::Px(16.)), flex_grow: 1., min_height: Val::Px(0.), ..default() }, offset), LearnScroll::Margin))
                .with_children(|body| {
                    if let Some(part) = &l.picked {
                        let component = scene.description.components.get(part);
                        let entry = component.and_then(|c| builder.and_then(|b| b.element_entry(&c.component_type)));
                        body.spawn((Node { border_radius: BorderRadius::all(Val::Px(7.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.), padding: UiRect::all(Val::Px(12.)), flex_shrink: 0., ..default() }, BackgroundColor(RAISED))).with_children(|c| {
                            c.spawn(k.text("SELECTED PART", 10.5, FAINT, 2));
                            c.spawn(k.text(part, 14., TEXT, 2));
                            if let Some(comp) = component {
                                c.spawn(k.text(entry.map(|e| format!("{} · {}", e.display_name, comp.component_type)).unwrap_or_else(|| comp.component_type.clone()), 11.5, ACCENT, 0));
                            }
                            if let Some(n) = entry.and_then(|e| e.notes.as_ref()) {
                                c.spawn(k.text(&n.summary, 12., SUBTLE, 0));
                            }
                            c.spawn(wrap()).with_children(|r| {
                                r.spawn(k.button("Note on this part", LessonAction::NoteOnPart, Look::Secondary, l.scene.is_some()));
                                r.spawn(k.button("Clear", LessonAction::ClearPart, Look::Ghost, true));
                            });
                        });
                    }
                    let thread = l.thread.as_ref().and_then(|id| threads.get(id));
                    if let Some(t) = thread {
                        body.spawn(k.button("‹ All notes", LessonAction::ThreadList, Look::Ghost, true));
                        body.spawn(k.title(&t.title));
                        annotate::anchors(body, k, &host, &t.targets);
                        body.spawn(wrap()).with_children(|r| {
                            r.spawn(k.button(if t.resolved { "Reopen" } else { "Resolve" }, LessonAction::Resolve, Look::Ghost, true));
                            r.spawn(k.button("Delete note", LessonAction::DeleteThread, Look::Danger, true));
                        });
                        agent_card(body, k, l, &t.id);
                        annotate::messages(body, k, &host, t, l.menu.as_deref());
                    } else if let Some(d) = &l.draft {
                        k.header(body, "New note", "Attached to");
                        annotate::anchors(body, k, &host, std::slice::from_ref(d));
                    } else {
                        body.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, flex_shrink: 0., ..default() }).with_children(|r| {
                            r.spawn(k.title("Notes"));
                            r.spawn(k.button(if l.open_only { "Open notes" } else { "All notes" }, LessonAction::OpenOnly, Look::Chip(false), true));
                        });
                        body.spawn(k.text(
                            match l.mode {
                                PageMode::Annotate => "Click a paragraph, or a part in the live scene, to start a note.",
                                _ => "Switch to Annotate to note a paragraph or a part. Notes live beside lesson.md and follow the text when it is edited.",
                            },
                            11.5,
                            SUBTLE,
                            0,
                        ));
                        let shown = threads.values().filter(|t| !l.open_only || !t.resolved);
                        if annotate::list(body, k, &host, shown) == 0 {
                            body.spawn(k.text("No notes on this lesson yet.", 13., SUBTLE, 0));
                        }
                    }
                });
            let composing = l.thread.is_some() || l.draft.is_some() || matches!(l.input.as_ref().map(|i| &i.purpose), Some(Purpose::Author));
            if composing {
                panel.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.), padding: UiRect::all(Val::Px(12.)), border: UiRect::top(Val::Px(1.)), flex_shrink: 0., ..default() }, BorderColor::all(BORDER))).with_children(|f| {
                    let draft = l.input.as_ref().filter(|i| matches!(i.purpose, Purpose::Comment | Purpose::Author | Purpose::EditComment(_))).map(|i| i.buffer.as_str());
                    let (label, submit) = match l.input.as_ref().map(|i| &i.purpose) {
                        Some(Purpose::Author) => ("Your name", "Save"),
                        Some(Purpose::EditComment(_)) => ("Edit message", "Save"),
                        _ if l.thread.is_none() => ("Write a note", "Post note"),
                        _ => ("Reply", "Post reply"),
                    };
                    annotate::composer(f, k, Composer { label, draft, placeholder: "Write…", focus: LessonAction::Compose, submit: LessonAction::Submit, submit_label: submit, cancel: LessonAction::CancelDraft, author: Some((&l.author, LessonAction::Author)), error: None });
                });
            }
        });
}

fn agent_card(body: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, thread: &str) {
    let run = l.agent.latest(thread);
    body.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(5.), padding: UiRect::all(Val::Px(9.)), flex_shrink: 0., ..default() }, BackgroundColor(RAISED))).with_children(|card| {
        card.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|row| {
            row.spawn(k.text("Ask Codex", 12., TEXT, 1));
            match run.filter(|r| r.status.active()) {
                Some(r) => {
                    row.spawn(k.button("Stop", LessonAction::AgentCancel(r.id.clone()), Look::Ghost, true));
                }
                None => {
                    row.spawn(k.button("Ask", LessonAction::Ask, Look::Ghost, l.agent.ready()));
                }
            }
        });
        match run {
            Some(r) => {
                card.spawn(k.text(&r.activity, 11., SUBTLE, 0));
                if let Some(e) = &r.error {
                    card.spawn(k.text(e, 11., WARN, 0));
                }
            }
            None => {
                card.spawn(k.text("Codex answers from the lesson, the scene's system and the repository (read-only). Manual only: nothing is sent unless you ask.", 11., SUBTLE, 0));
            }
        }
        if let Some(e) = l.agent.error() {
            card.spawn(k.text(e, 11., WARN, 0));
        }
    });
}
