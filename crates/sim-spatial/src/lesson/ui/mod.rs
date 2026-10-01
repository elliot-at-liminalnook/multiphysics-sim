//! Learn-mode chrome, drawn with the builder's kit so both screens look like
//! one application.
//!
//! ┌ toolbar: Lessons · title │ Read / Annotate / Edit │ undo · editor · Builder ┐
//! ├ lessons + contents ┬──── reading column (scene cards embed the 3D view) ──┬ notes ┤
//! └ status                                                                           ┘
use super::*;
use crate::builder::ui::{equations, markdown_theme, num, paragraph};
use crate::ui_kit::{
    ACCENT, BORDER, Dock, FAINT, Kit, LEFT_WIDTH, Look, OK, RAISED, RIGHT_WIDTH, STATUSBAR, SUBTLE, SURFACE, SWITCHER_STRIP, SliderLook, TEXT, TOPBAR, Tint, UiFonts, WARN, WHEEL_LINE, above_strip, divider, size, wheel_delta, wrap,
};
use bevy::input::mouse::MouseWheel;
use bevy::ui::prelude::AccessibleLabel;

mod cards;
mod margin;
mod outline;
mod scene_card;

use cards::{compare_card, component_card};
use margin::margin;
use outline::outline;
pub(super) use scene_card::{live_text, pace_label, scene_card};

const PAGE_WIDTH: f32 = 820.0;

#[derive(Component)]
pub(crate) struct LearnPanel;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LearnScroll {
    Outline,
    Page,
    Margin,
}
/// A block's container, for scrolling to it.
#[derive(Component)]
pub(crate) struct BlockNode(pub String);
#[derive(Component)]
pub(crate) struct LiveTime;
#[derive(Component)]
pub(crate) struct TimeFill;
/// Playhead line over a chart.
#[derive(Component)]
pub(crate) struct Playhead;
/// The operating point on a phase plot.
#[derive(Component)]
pub(crate) struct PhaseDot {
    pub ids: (String, String),
    pub window: (f64, f64),
    pub range: (f64, f64),
}
/// Covers a chart to the right of the playhead, so the curve draws in step
/// with the scene (and a prediction is not given away before it happens).
#[derive(Component)]
pub(crate) struct ChartMask;
#[derive(Component)]
pub(crate) struct CaptionBox;
#[derive(Component)]
pub(crate) struct CaptionText;
#[derive(Component)]
pub(crate) struct Progress;

pub(super) fn rebuild(
    mut commands: Commands,
    mut learn: ResMut<Learn>,
    panels: Query<Entity, With<LearnPanel>>,
    fonts: Option<Res<UiFonts>>,
    builder: Option<Res<Builder>>,
    scene: Res<SpatialScene>,
    scrolls: Query<(&ScrollPosition, &LearnScroll)>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut kept: Local<[f32; 2]>,
) {
    if !learn.active {
        if !panels.is_empty() {
            for e in &panels {
                commands.entity(e).despawn();
            }
            learn.dirty = true;
        }
        return;
    }
    let Some(fonts) = fonts else { return };
    // Keep the pressed entity alive until the click completes.
    if mouse.pressed(MouseButton::Left) && !learn.switched && learn.input.is_none() {
        return;
    }
    if !learn.dirty {
        return;
    }
    for (p, which) in &scrolls {
        match which {
            LearnScroll::Page => learn.scroll = p.y,
            LearnScroll::Outline => kept[0] = p.y,
            LearnScroll::Margin => kept[1] = p.y,
        }
    }
    for e in &panels {
        commands.entity(e).despawn();
    }
    learn.dirty = false;
    learn.switched = false;
    learn.ui_revision += 1;
    let k = Kit::new(&fonts);
    let l = &*learn;
    toolbar(&mut commands, &k, l);
    outline(&mut commands, &k, l, kept[0]);
    page(&mut commands, &k, l, builder.as_deref(), &scene);
    margin(&mut commands, &k, l, builder.as_deref(), &scene, kept[1]);
    status(&mut commands, &k, l);
    narrate::bar(&mut commands, &k, l);
}

fn toolbar(commands: &mut Commands, k: &Kit, l: &Learn) {
    commands
        .spawn((k.dock(Dock::Top { height: TOPBAR }, Node { padding: UiRect::axes(Val::Px(14.), Val::Px(0.)), align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, ..default() }), LearnPanel))
        .with_children(|bar| {
            bar.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.), ..default() }).with_children(|left| {
                left.spawn(k.text("Lessons", size::PRODUCT, TEXT, 2));
                if let Some(lesson) = &l.lesson {
                    left.spawn(divider());
                    left.spawn(k.text(&lesson.meta.title, 13., SUBTLE, 1));
                }
                if l.lesson_error.is_some() {
                    left.spawn(k.text("  lesson.md has an error (showing the last good version)", 11.5, WARN, 1));
                }
            });
            bar.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.), ..default() }).with_children(|mid| {
                mid.spawn(k.segments()).with_children(|seg| {
                    for (label, mode) in [("Read", PageMode::Read), ("Annotate", PageMode::Annotate), ("Edit", PageMode::Edit)] {
                        seg.spawn(k.segment(label, LessonAction::Mode(mode), l.mode == mode, l.lesson.is_some()));
                    }
                });
                mid.spawn(divider());
                mid.spawn(k.button("Undo", LessonAction::Undo, Look::Ghost, l.lesson.is_some()));
                mid.spawn(k.button("Redo", LessonAction::Redo, Look::Ghost, l.lesson.is_some()));
                mid.spawn(k.button("Open in editor", LessonAction::OpenEditor, Look::Ghost, l.lesson.is_some()));
            });
            bar.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.), ..default() }).with_children(|right| {
                let ready = l.scene.as_ref().is_some_and(|a| a.installed);
                if let Some(a) = &l.scene {
                    right.spawn(k.text(format!("Live: {}", if a.scene.title.is_empty() { &a.id } else { &a.scene.title }), 12., SUBTLE, 0));
                }
                right.spawn(k.button("Open in builder ›", LessonAction::OpenBuilder, Look::Primary, ready));
            });
        });
}

/// Link targets in lesson text: `part:` highlights, web links open.
fn link_action(lesson: &Lesson, link: &sim_markdown::Link) -> Option<LessonAction> {
    if let Some(target) = link.target.strip_prefix("part:") {
        return lesson.resolve_part(target).ok().map(|p| LessonAction::Part(p.system, p.path));
    }
    if link.target.starts_with("https://") || link.target.starts_with("http://") {
        return Some(LessonAction::Link(link.target.clone()));
    }
    None
}

fn page(commands: &mut Commands, k: &Kit, l: &Learn, builder: Option<&Builder>, scene: &SpatialScene) {
    commands
        .spawn((
            // The reading column between the docks (transparent: scene cards show the 3D view through it).
            k.scroll_area(Node { position_type: PositionType::Absolute, left: Val::Px(LEFT_WIDTH), right: Val::Px(RIGHT_WIDTH), top: Val::Px(TOPBAR), bottom: above_strip(STATUSBAR + narrate::reserved(l)), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() }, l.scroll),
            LearnScroll::Page,
            LearnPanel,
        ))
        .with_children(|outer| {
            outer
                .spawn(Node { width: Val::Percent(100.), max_width: Val::Px(PAGE_WIDTH), flex_direction: FlexDirection::Column, row_gap: Val::Px(14.), padding: UiRect { left: Val::Px(36.), right: Val::Px(28.), top: Val::Px(30.), bottom: Val::Px(240.) }, flex_shrink: 0., ..default() })
                .with_children(|col| {
                    let Some(lesson) = &l.lesson else {
                        col.spawn(k.text("Lessons", 28., TEXT, 2));
                        col.spawn(k.text(l.lesson_error.clone().unwrap_or_else(|| "Pick a lesson on the left. Lessons are Markdown files with live system scenes; edit them here or in any editor.".into()), 14., if l.lesson_error.is_some() { WARN } else { SUBTLE }, 0));
                        return;
                    };
                    col.spawn(k.text(&lesson.meta.title, 30., TEXT, 2));
                    if !lesson.meta.summary.is_empty() {
                        col.spawn(k.text(&lesson.meta.summary, 15., SUBTLE, 0));
                    }
                    let mut meta = Vec::new();
                    if let Some(m) = lesson.meta.minutes {
                        meta.push(format!("{m} min"));
                    }
                    if !lesson.meta.requires.is_empty() {
                        meta.push(format!("read first: {}", lesson.meta.requires.join(", ")));
                    }
                    if !lesson.meta.authors.is_empty() {
                        meta.push(lesson.meta.authors.join(", "));
                    }
                    if !meta.is_empty() {
                        col.spawn(k.text(meta.join(" · "), 11.5, FAINT, 0));
                    }
                    if let Some(e) = &l.lesson_error {
                        col.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), padding: UiRect::all(Val::Px(10.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() }, BorderColor::all(WARN), children![k.text(e, 12., WARN, 0)]));
                    }
                    let threads = l.threads();
                    let mut by_block: BTreeMap<String, Vec<String>> = BTreeMap::new();
                    if let Some(index) = &l.index {
                        for t in threads.values().filter(|t| !l.open_only || !t.resolved) {
                            if let Some(b) = t.targets.first().and_then(|a| a.block(index)) {
                                by_block.entry(b.to_string()).or_default().push(t.id.clone());
                            }
                        }
                    }
                    let mut by_scene: BTreeMap<String, usize> = BTreeMap::new();
                    for t in threads.values().filter(|t| !l.open_only || !t.resolved) {
                        if let Some(s) = t.targets.first().and_then(|a| a.scene()) {
                            *by_scene.entry(s.to_string()).or_default() += 1;
                        }
                    }
                    if l.mode == PageMode::Edit {
                        col.spawn(k.button("+ Add a block at the top", LessonAction::InsertAfter(None), Look::Ghost, true));
                        new_block_editor(col, k, l, None);
                    }
                    let gate = l.gate();
                    for (index, b) in lesson.blocks.iter().enumerate() {
                        if gate.is_some_and(|g| index >= g) {
                            if Some(index) == gate {
                                super::practice::locked(col, k, lesson.blocks.len() - index);
                            }
                            continue;
                        }
                        let editing = matches!(l.input.as_ref().map(|i| &i.purpose), Some(Purpose::Block(id, _)) if *id == b.id);
                        if editing {
                            block_editor(col, k, l, &b.id);
                        } else {
                            match &b.kind {
                                BlockKind::Heading { level, text, .. } => {
                                    block_row(col, k, l, b, by_block.get(&b.id), |c| {
                                        let size = match level {
                                            1 => 26.,
                                            2 => 21.,
                                            3 => 17.,
                                            _ => 15.,
                                        };
                                        c.spawn((k.text(text, size, TEXT, 2), Node { margin: UiRect::top(Val::Px(if *level <= 2 { 12. } else { 6. })), ..default() }));
                                    });
                                }
                                BlockKind::Markdown { text } => {
                                    // `{{…}}` numbers come from the model once it has answered.
                                    let doc = sim_markdown::parse(&l.resolved(text));
                                    block_row(col, k, l, b, by_block.get(&b.id), |c| {
                                        // Text runs between figures render as Markdown; figures as images.
                                        let mut run: Vec<sim_markdown::Block> = Vec::new();
                                        let flush = |c: &mut ChildSpawnerCommands, run: &mut Vec<sim_markdown::Block>, links: Vec<sim_markdown::Link>| {
                                            if !run.is_empty() {
                                                let part = sim_markdown::Document { blocks: std::mem::take(run), links };
                                                crate::markdown::render(c, &part, &page_theme(k), |link| link_action(lesson, link));
                                            }
                                        };
                                        for block in &doc.blocks {
                                            match &block.image {
                                                Some(img) => {
                                                    flush(c, &mut run, vec![]);
                                                    super::practice::figure(c, k, l, &img.src, if img.title.is_empty() { &img.alt } else { &img.title });
                                                }
                                                None => run.push(block.clone()),
                                            }
                                        }
                                        flush(c, &mut run, doc.links.clone());
                                    });
                                }
                                BlockKind::Quiz(q) => super::practice::quiz_card(col, k, l, b, q, &page_theme(k)),
                                BlockKind::Reflect(r) => super::practice::reflect_card(col, k, l, b, r, &page_theme(k)),
                                BlockKind::Scene(s) => scene_card(col, k, l, b, s, by_scene.get(&s.id).copied().unwrap_or(0), scene),
                                BlockKind::Component(card) => component_card(col, k, l, b, card, builder),
                                BlockKind::Compare(c) => compare_card(col, k, l, b, c),
                                BlockKind::Equation(e) => super::extras::equation_card(col, k, l, b, e),
                                BlockKind::Measured(m) => super::extras::measured_card(col, k, l, b, m, &page_theme(k)),
                                BlockKind::Task(t) => super::extras::task_card(col, k, l, b, t, &page_theme(k), scene),
                                BlockKind::Lab(lab) => super::extras::lab_card(col, k, l, b, lab, &page_theme(k)),
                                // Remedies appear under the question whose wrong option names them;
                                // authors see a marker where they are written.
                                BlockKind::Remedy(r) => {
                                    if l.mode == PageMode::Edit {
                                        block_row(col, k, l, b, by_block.get(&b.id), |c| {
                                            c.spawn(k.text(format!("Remedy `{}` · shown after a wrong pick: {}", r.id, r.misconception), 12., FAINT, 0));
                                        });
                                    }
                                }
                            }
                        }
                        if l.mode == PageMode::Edit {
                            new_block_editor(col, k, l, Some(&b.id));
                        }
                    }
                });
        });
}

fn page_theme(k: &Kit) -> crate::markdown::Theme {
    let mut t = markdown_theme(k);
    t.text = Color::srgb(0.86, 0.89, 0.92);
    t
}

/// A block with its note badges and (in Edit mode) block controls.
fn block_row(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, b: &sim_lesson::Block, notes: Option<&Vec<String>>, content: impl FnOnce(&mut ChildSpawnerCommands)) {
    let annotate = l.mode == PageMode::Annotate;
    let mut row = col.spawn((Node { border_radius: BorderRadius::all(Val::Px(5.)), column_gap: Val::Px(10.), align_items: AlignItems::FlexStart, flex_shrink: 0., padding: UiRect::axes(Val::Px(6.), Val::Px(3.)), margin: UiRect::left(Val::Px(-6.)), ..default() }, BlockNode(b.id.clone())));
    if annotate {
        row.insert((Button, LessonAction::AnnotateBlock(b.id.clone()), Tint::CLEAR, AccessibleLabel::new("Note on this block"), BackgroundColor(Color::NONE)));
    }
    row.with_children(|r| {
        r.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1., flex_shrink: 1., min_width: Val::Px(0.), ..default() }).with_children(content);
        r.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.), width: Val::Px(66.), flex_shrink: 0., align_items: AlignItems::FlexEnd, ..default() }).with_children(|g| {
            if let Some(ids) = notes {
                let selected = l.thread.as_ref().is_some_and(|t| ids.contains(t));
                g.spawn(k.button(&format!("{} note{}", ids.len(), if ids.len() == 1 { "" } else { "s" }), LessonAction::OpenThread(ids[0].clone()), Look::Chip(selected), true));
            }
            if l.mode == PageMode::Edit {
                g.spawn(k.button("Edit", LessonAction::EditBlock(b.id.clone()), Look::Ghost, l.input.is_none()));
                g.spawn(k.button("Delete", LessonAction::DeleteBlock(b.id.clone()), Look::Danger, l.input.is_none()));
            }
        });
    });
}

fn editor_box(col: &mut ChildSpawnerCommands, k: &Kit, title: &str, text: &str) {
    col.spawn((Node { border_radius: BorderRadius::all(Val::Px(7.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(8.), padding: UiRect::all(Val::Px(12.)), border: UiRect::all(Val::Px(1.)), flex_shrink: 0., ..default() }, BorderColor::all(ACCENT), BackgroundColor(RAISED))).with_children(|c| {
        c.spawn(k.text(title, 11.5, SUBTLE, 1));
        c.spawn((k.mono(format!("{text}|"), size::BODY, TEXT), Node { min_height: Val::Px(40.), ..default() }));
        c.spawn(Node { column_gap: Val::Px(8.), align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn(k.button("Save", LessonAction::SaveEdit, Look::Primary, true));
            r.spawn(k.button("Cancel", LessonAction::CancelEdit, Look::Ghost, true));
            r.spawn(k.text("Markdown · Enter for a new line · Cmd/Ctrl+Enter saves · Esc cancels", 10.5, FAINT, 0));
        });
    });
}

fn block_editor(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, _id: &str) {
    let text = l.input.as_ref().map(|i| i.buffer.as_str()).unwrap_or("");
    editor_box(col, k, "Editing block (changes go through lesson.md with undo)", text);
}

fn new_block_editor(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, after: Option<&String>) {
    if let Some(Input { purpose: Purpose::NewBlock(a), buffer }) = &l.input {
        if a.as_ref() == after {
            editor_box(col, k, "New block (Markdown, or a ```sim-scene``` block)", buffer);
            return;
        }
    }
    if let Some(after) = after {
        col.spawn(Node { justify_content: JustifyContent::Center, flex_shrink: 0., ..default() }).with_children(|r| {
            r.spawn(k.button("+", LessonAction::InsertAfter(Some(after.clone())), Look::Ghost, l.input.is_none()));
        });
    }
}

fn card_frame() -> impl Bundle {
    (Node { border_radius: BorderRadius::all(Val::Px(8.)), flex_direction: FlexDirection::Column, width: Val::Percent(100.), border: UiRect::all(Val::Px(1.)), margin: UiRect::vertical(Val::Px(6.)), flex_shrink: 0., overflow: Overflow::clip(), ..default() }, BorderColor::all(BORDER))
}

fn status(commands: &mut Commands, k: &Kit, l: &Learn) {
    commands
        .spawn((k.dock(Dock::Bottom { height: STATUSBAR }, Node { padding: UiRect::axes(Val::Px(14.), Val::Px(0.)), align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, ..default() }), LearnPanel))
        .with_children(|bar| {
            bar.spawn((k.text(&l.status, 11.5, SUBTLE, 0), Node { flex_shrink: 1., overflow: Overflow::clip(), ..default() }));
            let mut right = Vec::new();
            if let Some(lesson) = &l.lesson {
                right.push(format!("revision {}", &sim_lesson::edit::revision(&lesson.source)[..8]));
                right.push(format!("{} blocks", lesson.blocks.len()));
            }
            right.push(format!("{} notes", l.notes_doc.threads.len()));
            bar.spawn(k.text(right.join(" · "), 11., FAINT, 0));
        });
}

/// Wheel scrolling for the three columns; scroll-to-block requests.
pub(super) fn scroll(
    mut wheel: MessageReader<MouseWheel>,
    window: Single<&Window>,
    keys: Res<ButtonInput<KeyCode>>,
    mut learn: ResMut<Learn>,
    scene: Res<SpatialScene>,
    mut panels: Query<(&mut ScrollPosition, &LearnScroll, &ComputedNode, &UiGlobalTransform)>,
    blocks: Query<(&BlockNode, &ComputedNode, &UiGlobalTransform)>,
) {
    let delta = wheel_delta(&mut wheel, WHEEL_LINE);
    if !learn.active {
        return;
    }
    let scale = window.scale_factor();
    if delta != 0.0 {
        if let Some(p) = window.cursor_position() {
            // Cmd/Ctrl+wheel zooms the scene card, typing or not (the camera reads gesture modifiers while typing).
            let zooming = (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) || keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight)) && scene.learn_view.is_some_and(|v| v.visible.contains(p * scale));
            if !zooming && p.y > TOPBAR && p.y < window.height() - STATUSBAR - SWITCHER_STRIP {
                let which = if p.x < LEFT_WIDTH {
                    LearnScroll::Outline
                } else if p.x > window.width() - RIGHT_WIDTH {
                    LearnScroll::Margin
                } else {
                    LearnScroll::Page
                };
                for (mut position, side, _, _) in &mut panels {
                    if *side == which {
                        position.y = (position.y - delta).max(0.0);
                    }
                }
            }
        }
    }
    if let Some(target) = learn.scroll_to.clone() {
        let block = blocks.iter().find(|(b, n, _)| b.0 == target && n.size().y > 0.0);
        if let Some((_, node, transform)) = block {
            for (mut position, side, pnode, ptransform) in &mut panels {
                if *side != LearnScroll::Page {
                    continue;
                }
                let top = transform.translation.y - node.size().y / 2.0;
                let panel_top = ptransform.translation.y - pnode.size().y / 2.0;
                position.y = (position.y + (top - panel_top) / scale - 18.0).max(0.0);
            }
            learn.scroll_to = None;
        } else if !blocks.iter().any(|(b, ..)| b.0 == target) && !learn.dirty {
            learn.scroll_to = None;
        }
    }
    for (position, side, _, _) in &panels {
        if *side == LearnScroll::Page && (position.y - learn.scroll).abs() > 0.5 {
            learn.bypass_change_detection().scroll = position.y;
        }
    }
}
