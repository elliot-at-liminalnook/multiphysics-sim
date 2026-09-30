//! Learn-mode chrome, drawn with the builder's kit so both screens look like
//! one application.
//!
//! ┌ toolbar: Lessons · title │ Read / Annotate / Edit │ undo · editor · Builder ┐
//! ├ lessons + contents ┬──── reading column (scene cards embed the 3D view) ──┬ notes ┤
//! └ status                                                                           ┘
use super::*;
use crate::annotate::{self, Composer, Host};
use crate::builder::ui::{
    ACCENT_BG, BAR, BORDER, FAINT, HOVER_BG, Kit, Look, OK, RAISED, STATUSBAR, SUBTLE, SURFACE, TEXT, TOPBAR, Tint, UiFonts, WARN, divider, equations, markdown_theme, num, paragraph, wrap,
};
use crate::builder::ui::{LEFT_WIDTH, RIGHT_WIDTH};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};

const ACCENT: Color = crate::ACCENT;
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
    let k = Kit { f: &fonts };
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
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(0.), right: Val::Px(0.), top: Val::Px(0.), height: Val::Px(TOPBAR), padding: UiRect::axes(Val::Px(14.), Val::Px(0.)), align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, border: UiRect::bottom(Val::Px(1.)), ..default() },
            BackgroundColor(BAR),
            BorderColor::all(BORDER),
            LearnPanel,
        ))
        .with_children(|bar| {
            bar.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.), ..default() }).with_children(|left| {
                left.spawn(k.text("Lessons", 14., TEXT, 2));
                if let Some(lesson) = &l.lesson {
                    left.spawn(divider());
                    left.spawn(k.text(&lesson.meta.title, 13., SUBTLE, 1));
                }
                if l.lesson_error.is_some() {
                    left.spawn(k.text("  lesson.md has an error (showing the last good version)", 11.5, WARN, 1));
                }
            });
            bar.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.), ..default() }).with_children(|mid| {
                mid.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), padding: UiRect::all(Val::Px(2.)), border: UiRect::all(Val::Px(1.)), column_gap: Val::Px(2.), ..default() }, BorderColor::all(BORDER))).with_children(|seg| {
                    for (label, mode) in [("Read", PageMode::Read), ("Annotate", PageMode::Annotate), ("Edit", PageMode::Edit)] {
                        seg.spawn(k.button(label, LessonAction::Mode(mode), Look::Segment(l.mode == mode), l.lesson.is_some()));
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

fn outline(commands: &mut Commands, k: &Kit, l: &Learn, offset: f32) {
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(0.), top: Val::Px(TOPBAR), bottom: Val::Px(STATUSBAR), width: Val::Px(LEFT_WIDTH), flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(14.)), row_gap: Val::Px(4.), overflow: Overflow::scroll_y(), border: UiRect::right(Val::Px(1.)), ..default() },
            ScrollPosition(Vec2::new(0.0, offset)),
            BackgroundColor(SURFACE),
            BorderColor::all(BORDER),
            LearnScroll::Outline,
            LearnPanel,
        ))
        .with_children(|col| {
            super::practice::review_outline(col, k, l);
            col.spawn(k.section("Lessons"));
            if l.entries.is_empty() {
                col.spawn(k.text(format!("No lessons in {}. A lesson is <slug>/lesson.md.", l.dir.display()), 12., SUBTLE, 0));
            }
            // Lessons grouped by category (`categories.yaml`); a header folds its group.
            for g in sim_lesson::categories::group(&l.entries, &l.categories) {
                let folded = l.folded.contains(&g.category.id);
                let holds_current = g.lessons.iter().any(|e| l.slug() == Some(e.slug.as_str()));
                col.spawn((
                    Button,
                    LessonAction::ToggleCategory(g.category.id.clone()),
                    Tint { idle: Color::NONE, hover: HOVER_BG },
                    Node { border_radius: BorderRadius::all(Val::Px(4.)), flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding: UiRect { left: Val::Px(6.), right: Val::Px(6.), top: Val::Px(8.), bottom: Val::Px(4.) }, flex_shrink: 0., ..default() },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|row| {
                    row.spawn(k.text(format!("{} {}", if folded { "›" } else { "↓" }, g.category.title), 12., if holds_current { ACCENT } else { SUBTLE }, 1));
                    row.spawn(k.text(format!("{}", g.lessons.len()), 11., FAINT, 0));
                });
                if folded {
                    continue;
                }
                for e in g.lessons {
                    let current = l.slug() == Some(e.slug.as_str());
                    let subtitle = match &e.error {
                        Some(_) => "does not parse — open it to see the line".to_string(),
                        None => format!("{}{} scene{}", e.minutes.map(|m| format!("{m} min · ")).unwrap_or_default(), e.scenes, if e.scenes == 1 { "" } else { "s" }),
                    };
                    col.spawn((
                        Button,
                        LessonAction::Open(e.slug.clone()),
                        Tint { idle: if current { ACCENT_BG } else { Color::NONE }, hover: if current { ACCENT_BG } else { HOVER_BG } },
                        Node { border_radius: BorderRadius::all(Val::Px(4.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(2.), padding: UiRect::axes(Val::Px(9.), Val::Px(7.)), border: UiRect::left(Val::Px(2.)), flex_shrink: 0., ..default() },
                        BorderColor::all(if current { ACCENT } else { Color::NONE }),
                        BackgroundColor(if current { ACCENT_BG } else { Color::NONE }),
                    ))
                    .with_children(|row| {
                        let mark = if l.visited.contains(&e.slug) && !current { "· " } else { "" };
                        row.spawn(k.text(format!("{mark}{}", e.title), 13., TEXT, 1));
                        row.spawn(k.text(subtitle, 11., if e.error.is_some() { WARN } else { SUBTLE }, 0));
                    });
                }
            }
            concepts_outline(col, k, l);
            if let Some(lesson) = &l.lesson {
                col.spawn(k.section("In this lesson"));
                let gate = l.gate();
                for (b, level, text, _) in lesson.headings() {
                    let position = lesson.blocks.iter().position(|x| x.id == b.id).unwrap_or(0);
                    if gate.is_some_and(|g| position >= g) {
                        col.spawn((Node { padding: UiRect { left: Val::Px(8. + 12. * (level.saturating_sub(1)) as f32), right: Val::Px(6.), top: Val::Px(4.), bottom: Val::Px(4.) }, flex_shrink: 0., ..default() }, children![k.text(format!("{text} · locked"), 12., FAINT, 0)]));
                        continue;
                    }
                    col.spawn((
                        Button,
                        LessonAction::Goto(b.id.clone()),
                        Tint { idle: Color::NONE, hover: HOVER_BG },
                        Node { border_radius: BorderRadius::all(Val::Px(4.)), padding: UiRect { left: Val::Px(8. + 12. * (level.saturating_sub(1)) as f32), right: Val::Px(6.), top: Val::Px(4.), bottom: Val::Px(4.) }, flex_shrink: 0., ..default() },
                        BackgroundColor(Color::NONE),
                        children![k.text(text, if level <= 1 { 13. } else { 12.5 }, if level <= 1 { TEXT } else { SUBTLE }, if level <= 1 { 1 } else { 0 })],
                    ));
                }
                narrate::outline(col, k, l);
                settings_outline(col, k, l);
                let scenes: Vec<_> = lesson.scenes().map(|(_, s)| s.clone()).collect();
                if !scenes.is_empty() {
                    col.spawn(k.section("Scenes"));
                    for s in scenes {
                        let live = l.scene.as_ref().is_some_and(|a| a.id == s.id);
                        col.spawn(k.button(&format!("{}{}", if live { "Live · " } else { "" }, if s.title.is_empty() { &s.id } else { &s.title }), LessonAction::ShowAt { scene: s.id.clone(), time: None, part: None }, Look::Ghost, true));
                    }
                }
            }
        });
}

/// Mastery per concept and the suggested next lesson.
fn concepts_outline(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn) {
    if l.concepts.is_empty() {
        return;
    }
    let mastery = sim_lesson::concepts::mastery(&l.concepts, &l.all_lessons, &l.progress);
    let done = |x: &Lesson| x.quizzes().filter(|(_, q)| q.gates()).all(|(_, q)| l.progress.passed(&x.slug, &q.id));
    col.spawn(k.section("Concepts"));
    if let Some((slug, why)) = sim_lesson::concepts::next_lesson(&l.all_lessons, &mastery, done) {
        if l.slug() == Some(slug.as_str()) {
            col.spawn(k.text("Keep going: this lesson is the next step.", 11.5, SUBTLE, 0));
        } else {
            let title = l.all_lessons.iter().find(|x| x.slug == slug).map(|x| x.meta.title.clone()).unwrap_or(slug.clone());
            col.spawn(k.button(&format!("Next: {title}"), LessonAction::Open(slug), Look::Secondary, true));
            col.spawn(k.text(why, 10.5, FAINT, 0));
        }
    }
    // The open lesson's concepts (what it needs, then what it teaches); else all with progress.
    let wanted: Vec<String> = l.lesson.as_ref().map(|x| x.meta.needs.iter().chain(&x.meta.teaches).cloned().collect()).unwrap_or_default();
    let shown: Vec<&sim_lesson::concepts::Mastery> = if wanted.is_empty() { mastery.iter().filter(|m| m.solid + m.shaky > 0).collect() } else { wanted.iter().filter_map(|c| mastery.iter().find(|m| &m.concept == c)).collect() };
    for m in shown.into_iter().filter(|m| m.questions > 0) {
        let color = if m.level >= sim_lesson::concepts::MASTERED { OK } else if m.level > 0. { ACCENT } else { BORDER };
        let target = m.taught_by.first().cloned();
        let mut row = col.spawn(Node { border_radius: BorderRadius::all(Val::Px(4.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(3.), padding: UiRect::axes(Val::Px(6.), Val::Px(4.)), flex_shrink: 0., ..default() });
        if let Some(t) = &target {
            row.insert((Button, LessonAction::Open(t.clone()), Tint { idle: Color::NONE, hover: HOVER_BG }, BackgroundColor(Color::NONE)));
        }
        row.with_children(|r| {
            r.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|t| {
                t.spawn(k.text(&m.title, 12., TEXT, 0));
                t.spawn(k.text(format!("{}/{}", m.solid, m.questions), 10.5, FAINT, 0));
            });
            r.spawn((Node { border_radius: BorderRadius::all(Val::Px(2.)), width: Val::Percent(100.), height: Val::Px(4.), ..default() }, BackgroundColor(Color::srgb(0.16, 0.19, 0.23)), Pickable::IGNORE)).with_children(|bar| {
                bar.spawn((Node { border_radius: BorderRadius::all(Val::Px(2.)), width: Val::Percent((m.level * 100.) as f32), height: Val::Percent(100.), ..default() }, BackgroundColor(color), Pickable::IGNORE));
            });
        });
    }
}

/// Reading and motion preferences.
fn settings_outline(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn) {
    col.spawn(k.section("Reading"));
    col.spawn(wrap()).with_children(|r| {
        r.spawn(k.button("Reduce motion", LessonAction::Setting(Setting::ReducedMotion), Look::Chip(l.settings.reduced_motion), true));
        r.spawn(k.button("Transcript", LessonAction::Setting(Setting::Transcript), Look::Chip(l.settings.transcript), true));
    });
    col.spawn(wrap()).with_children(|r| {
        r.spawn(k.text("Text", 11., SUBTLE, 0));
        for s in [1.0f32, 1.15, 1.3] {
            r.spawn(k.button(&format!("{:.0} %", s * 100.), LessonAction::Setting(Setting::TextSize(s)), Look::Chip((l.settings.text_scale - s).abs() < 0.01), true));
        }
    });
    col.spawn(wrap()).with_children(|r| {
        r.spawn(k.text("Voice", 11., SUBTLE, 0));
        for s in [0.8f32, 1.0, 1.25] {
            r.spawn(k.button(&format!("×{s}"), LessonAction::Setting(Setting::NarrationSpeed(s)), Look::Chip((l.settings.narration_speed - s).abs() < 0.01), true));
        }
    });
    col.spawn(k.text("Keys: 1–4 pick an option of the next open question, Enter checks it, Space plays, ←/→ step events (or nudge a slider you clicked the name of).", 10.5, FAINT, 0));
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
            Node { position_type: PositionType::Absolute, left: Val::Px(LEFT_WIDTH), right: Val::Px(RIGHT_WIDTH), top: Val::Px(TOPBAR), bottom: Val::Px(STATUSBAR + narrate::reserved(l)), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, overflow: Overflow::scroll_y(), ..default() },
            ScrollPosition(Vec2::new(0.0, l.scroll)),
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
        row.insert((Button, LessonAction::AnnotateBlock(b.id.clone()), Tint { idle: Color::NONE, hover: HOVER_BG }, BackgroundColor(Color::NONE)));
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
        c.spawn((Text::new(format!("{text}|")), TextFont { font: k.f.mono.clone().into(), font_size: FontSize::Px(12.5), ..default() }, TextColor(TEXT), TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter), Node { min_height: Val::Px(40.), ..default() }));
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

pub(super) fn scene_card(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, b: &sim_lesson::Block, s: &Scene, notes: usize, scene: &SpatialScene) {
    let active = l.scene.as_ref().filter(|a| a.id == s.id);
    let title = if s.title.is_empty() { format!("Scene · {}", s.id) } else { s.title.clone() };
    col.spawn((card_frame(), BlockNode(b.id.clone()))).with_children(|card| {
        // Header.
        card.spawn((Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, padding: UiRect::axes(Val::Px(12.), Val::Px(8.)), column_gap: Val::Px(8.), ..default() }, BackgroundColor(RAISED))).with_children(|h| {
            h.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.), flex_shrink: 1., min_width: Val::Px(0.), ..default() }).with_children(|t| {
                t.spawn(k.text(&title, 14., TEXT, 2));
                let label = match active {
                    Some(a) => match (&a.run, &a.error) {
                        (_, Some(_)) => "Could not load".to_string(),
                        (Some(run), None) => format!("{} · recorded {:.2} s{}", run.fidelity, run.duration_s, if a.sandbox.as_ref().is_some_and(|sb| sb.modified) { " · your builder changes" } else { "" }),
                        (None, None) => "Recording on the shared runtime…".into(),
                    },
                    None => format!("{} · {} s", s.system, num(s.run.duration_s)),
                };
                t.spawn(k.text(label, 11., SUBTLE, 0));
            });
            h.spawn(Node { column_gap: Val::Px(6.), align_items: AlignItems::Center, flex_shrink: 0., ..default() }).with_children(|r| {
                if notes > 0 {
                    r.spawn(k.button(&format!("{notes} note{}", if notes == 1 { "" } else { "s" }), LessonAction::ShowAt { scene: s.id.clone(), time: None, part: None }, Look::Chip(false), true));
                }
                if active.is_some() {
                    r.spawn(k.button("Ask about this moment", LessonAction::AskMoment, Look::Ghost, active.is_some_and(|a| a.run.is_some())));
                    r.spawn(k.button("Note", LessonAction::NoteOnScene, Look::Ghost, true));
                    r.spawn(k.button("Open in builder", LessonAction::OpenBuilder, Look::Secondary, active.is_some_and(|a| a.installed)));
                }
            });
        });
        // Viewport: transparent while live so the 3D view shows through.
        let height = s.height.unwrap_or(360.);
        let live = active.is_some_and(|a| a.installed && a.error.is_none());
        let mut view = card.spawn((Node { width: Val::Percent(100.), height: Val::Px(height), justify_content: JustifyContent::Center, align_items: AlignItems::Center, flex_shrink: 0., ..default() }, SceneViewport(s.id.clone())));
        if !live {
            view.insert(BackgroundColor(SURFACE));
        }
        view.with_children(|v| {
            let locked = l.scene_locked(&s.id);
            match active {
                None if locked.is_some() => {
                    v.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(10.), align_items: AlignItems::Center, max_width: Val::Percent(80.), ..default() }).with_children(|c| {
                        c.spawn(k.text("Make your prediction above to unlock this scene", 14., TEXT, 2));
                        c.spawn(k.text("Committing to a guess before you watch makes the result far more memorable, whether you were right or not.", 12., SUBTLE, 0));
                        c.spawn(k.button("Go to the prediction", LessonAction::Goto(locked.clone().unwrap_or_default()), Look::Secondary, true));
                    });
                }
                None => {
                    v.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(10.), align_items: AlignItems::Center, max_width: Val::Percent(80.), ..default() }).with_children(|c| {
                        if !s.caption.is_empty() {
                            c.spawn(k.text(&s.caption, 13., SUBTLE, 0));
                        }
                        c.spawn(k.button("Show this scene", LessonAction::Activate(s.id.clone()), Look::Primary, true));
                    });
                }
                Some(a) if a.error.is_some() => {
                    v.spawn((k.text(a.error.clone().unwrap_or_default(), 12., WARN, 0), Node { max_width: Val::Percent(86.), ..default() }));
                }
                Some(a) if !a.installed => {
                    v.spawn(k.text("Loading the scene…", 13., SUBTLE, 0));
                }
                Some(a) => {
                    if a.run.is_none() || a.recording {
                        v.spawn((k.text("Recording…", 12., TEXT, 1), Progress, Node { border_radius: BorderRadius::all(Val::Px(4.)), position_type: PositionType::Absolute, top: Val::Px(10.), right: Val::Px(12.), padding: UiRect::axes(Val::Px(8.), Val::Px(4.)), ..default() }, BackgroundColor(Color::srgba(0.06, 0.09, 0.12, 0.85))));
                    }
                    v.spawn((Node { border_radius: BorderRadius::all(Val::Px(5.)), position_type: PositionType::Absolute, left: Val::Px(12.), bottom: Val::Px(12.), max_width: Val::Percent(80.), padding: UiRect::axes(Val::Px(10.), Val::Px(6.)), display: Display::None, ..default() }, BackgroundColor(Color::srgba(0.06, 0.09, 0.12, 0.88)), CaptionBox))
                        .with_children(|c| {
                            c.spawn((k.text("", 13., TEXT, 1), CaptionText));
                        });
                    let hint = if l.mode == PageMode::Annotate { "Click a part to note it" } else { "Right-drag orbit · Shift-drag pan · Cmd/Ctrl+scroll zoom" };
                    v.spawn((k.text(hint, 10.5, FAINT, 0), Node { position_type: PositionType::Absolute, top: Val::Px(10.), left: Val::Px(12.), ..default() }));
                }
            }
        });
        // Controls.
        if let Some(a) = active.filter(|a| a.installed && a.run.is_some()) {
            card.spawn((Node { align_items: AlignItems::Center, column_gap: Val::Px(8.), padding: UiRect::axes(Val::Px(12.), Val::Px(8.)), ..default() }, BackgroundColor(RAISED))).with_children(|c| {
                c.spawn(k.button(if a.playing { "Pause" } else { "Play" }, if a.playing { LessonAction::Pause } else { LessonAction::Play }, Look::Primary, true));
                c.spawn(k.button("Restart", LessonAction::Restart, Look::Ghost, true));
                c.spawn((Button, Timebar, bevy::ui::RelativeCursorPosition::default(), LessonAction::Seek, Node { border_radius: BorderRadius::all(Val::Px(5.)), flex_grow: 1., height: Val::Px(10.), border: UiRect::all(Val::Px(1.)), ..default() }, BackgroundColor(SURFACE), BorderColor::all(BORDER)))
                    .with_children(|bar| {
                        bar.spawn((Node { border_radius: BorderRadius::all(Val::Px(5.)), width: Val::Percent(0.), height: Val::Percent(100.), ..default() }, BackgroundColor(ACCENT), TimeFill, Pickable::IGNORE));
                        // Event marks: captions, parameter changes and pauses (←/→ jump between them).
                        let d = a.duration().max(1e-9);
                        for t in super::event_times(a) {
                            bar.spawn((Node { position_type: PositionType::Absolute, left: Val::Percent((t / d * 100.) as f32), top: Val::Px(-3.), width: Val::Px(2.), height: Val::Px(14.), ..default() }, BackgroundColor(Color::srgba(1., 1., 1., 0.55)), Pickable::IGNORE));
                        }
                    });
                c.spawn((k.text("0.00 s", 12., TEXT, 1), LiveTime, Node { width: Val::Px(250.), ..default() }));
                for x in [0.25, 1.0, 4.0] {
                    c.spawn(k.button(&format!("×{}", num(x)), LessonAction::Speed(x), Look::Chip((a.user_speed - x).abs() < 1e-9), true));
                }
                c.spawn(k.button(if a.explore { "Guided" } else { "Free explore" }, LessonAction::Explore, Look::Chip(a.explore), true));
            });
            // What the view draws: the same switches the builder floats over its 3D view.
            card.spawn((Node { align_items: AlignItems::Center, column_gap: Val::Px(8.), padding: UiRect { left: Val::Px(12.), right: Val::Px(12.), top: Val::Px(0.), bottom: Val::Px(8.) }, ..default() }, BackgroundColor(RAISED))).with_children(|c| {
                c.spawn(k.text("Show", 11.5, SUBTLE, 1));
                c.spawn((crate::physics_view::LayerChips, Node { flex_grow: 1., flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(4.), row_gap: Val::Px(4.), ..default() }));
            });
        }
        // Sliders: change a value, let go, and the scene re-records.
        if let Some(a) = active.filter(|a| !a.scene.sliders.is_empty()) {
            card.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.), padding: UiRect::axes(Val::Px(12.), Val::Px(10.)), ..default() }, BackgroundColor(RAISED))).with_children(|c| {
                c.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|r| {
                    r.spawn(k.text(if a.recording { "Re-simulating with your values…" } else { "Try it: drag a slider and let go to re-run the physics" }, 11.5, SUBTLE, 1));
                    r.spawn(k.button("Lesson values", LessonAction::ResetSliders, Look::Ghost, !a.overrides.is_empty()));
                });
                for sl in &a.scene.sliders {
                    c.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(10.), ..default() }).with_children(|r| {
                        // Clicking the name gives the slider the arrow keys.
                        let focused = l.focus_slider.as_deref() == Some(sl.parameter.as_str());
                        r.spawn((Button, LessonAction::SliderFocus(sl.parameter.clone()), Node { border_radius: BorderRadius::all(Val::Px(4.)), width: Val::Px(150.), padding: UiRect::axes(Val::Px(4.), Val::Px(2.)), border: UiRect::all(Val::Px(1.)), ..default() }, BorderColor::all(if focused { ACCENT } else { Color::NONE }), children![k.text(if sl.label.is_empty() { &sl.parameter } else { &sl.label }, 12., TEXT, 1)]));
                        r.spawn((Button, super::SliderTrack(sl.parameter.clone()), bevy::ui::RelativeCursorPosition::default(), Node { border_radius: BorderRadius::all(Val::Px(6.)), flex_grow: 1., height: Val::Px(12.), border: UiRect::all(Val::Px(1.)), ..default() }, BackgroundColor(SURFACE), BorderColor::all(BORDER)))
                            .with_children(|t| {
                                t.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), width: Val::Percent(0.), height: Val::Percent(100.), ..default() }, BackgroundColor(ACCENT), super::SliderFill(sl.parameter.clone()), Pickable::IGNORE));
                            });
                        r.spawn((k.text("", 12., TEXT, 1), super::SliderValue(sl.parameter.clone()), Node { width: Val::Px(90.), ..default() }));
                    });
                }
            });
        }
        // The goal and the things to try sit with the sliders they use.
        if let Some(a) = active.filter(|a| a.scene.challenge.is_some() || (a.explore && !a.scene.hints.is_empty())) {
            card.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.), padding: UiRect { left: Val::Px(12.), right: Val::Px(12.), top: Val::Px(10.), bottom: Val::Px(0.) }, ..default() }).with_children(|body| {
            // A goal to reach with the sliders, judged on each run.
            if let Some(ch) = &a.scene.challenge {
                body.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.), padding: UiRect::all(Val::Px(12.)), border: UiRect::left(Val::Px(3.)), flex_shrink: 0., ..default() }, BackgroundColor(SURFACE), BorderColor::all(ACCENT))).with_children(|c| {
                    let met = a.challenge.as_ref().is_some_and(|r| r.iter().all(|x| x.passed)) && !a.overrides.is_empty();
                    c.spawn(k.text(if met { "Challenge met" } else { "Challenge" }, 12., if met { OK } else { ACCENT }, 2));
                    crate::markdown::render(c, &sim_markdown::parse(&ch.goal), &crate::builder::ui::markdown_theme(k), |_| None::<LessonAction>);
                    if !a.overrides.is_empty() {
                        for r in a.challenge.iter().flatten() {
                            c.spawn(k.text(format!("{} {}", if r.passed { "met:" } else { "not yet:" }, if r.why.is_empty() { &r.message } else { &r.why }), 11.5, if r.passed { OK } else { SUBTLE }, 0));
                        }
                        if !met && !ch.hint.is_empty() {
                            c.spawn(k.text(format!("Hint: {}", ch.hint), 11.5, FAINT, 0));
                        }
                    } else {
                        c.spawn(k.text("Move the sliders, let go, and the run is judged.", 11.5, FAINT, 0));
                    }
                });
            }
            // Free exploration: things to try.
            if !a.scene.hints.is_empty() && a.explore {
                body.spawn(k.section("Things to try"));
                for h in &a.scene.hints {
                    body.spawn(k.text(format!("· {h}"), 12.5, TEXT, 0));
                }
            }
            });
        }
        // Caption, charts, claims, sandbox state.
        card.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.), padding: UiRect::all(Val::Px(12.)), ..default() }).with_children(|body| {
            if !s.caption.is_empty() && active.is_some() {
                body.spawn(k.text(&s.caption, 12.5, SUBTLE, 0));
            }
            let Some(a) = active else { return };
            for chart in &a.charts {
                body.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|r| {
                    r.spawn(k.text(format!("{}{}", chart.key, if chart.unit.is_empty() { String::new() } else { format!(" ({})", chart.unit) }), 11.5, TEXT, 1));
                    r.spawn(k.text(format!("{} … {}", num(chart.range.0), num(chart.range.1)), 10.5, FAINT, 0));
                });
                // Image, then the reveal mask and playhead as later siblings,
                // so they draw over the curve.
                body.spawn((Node { border_radius: BorderRadius::all(Val::Px(4.)), width: Val::Percent(100.), aspect_ratio: Some(720. / 200.), flex_shrink: 0., border: UiRect::all(Val::Px(1.)), ..default() }, BorderColor::all(Color::NONE), Interaction::default(), bevy::ui::RelativeCursorPosition::default(), super::ChartHover(chart.key.clone(), chart.window.0, chart.window.1), narrate::ChartNode(chart.key.clone(), chart.window.0, chart.window.1))).with_children(|img| {
                    let window = chart.window;
                    img.spawn((Node { border_radius: BorderRadius::all(Val::Px(4.)), position_type: PositionType::Absolute, width: Val::Percent(100.), height: Val::Percent(100.), ..default() }, ImageNode::new(chart.image.clone()), Pickable::IGNORE));
                    img.spawn((Node { position_type: PositionType::Absolute, top: Val::Px(0.), height: Val::Percent(100.), left: Val::Percent(0.), width: Val::Percent(100.), ..default() }, BackgroundColor(Color::srgba(0.07, 0.09, 0.11, 0.93)), ChartMask, ChartWindow(window.0, window.1), Pickable::IGNORE));
                    img.spawn((Node { position_type: PositionType::Absolute, top: Val::Px(0.), bottom: Val::Px(0.), width: Val::Px(2.), left: Val::Percent(0.), ..default() }, BackgroundColor(Color::srgba(1., 1., 1., 0.7)), Playhead, ChartWindow(window.0, window.1), Pickable::IGNORE));
                });
            }
            for chart in &a.phase_charts {
                body.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|r| {
                    r.spawn(k.text(&chart.title, 11.5, TEXT, 1));
                    r.spawn(k.text(format!("x: {} {} … {} ({})", chart.x.0, num(chart.window.0), num(chart.window.1), chart.x.1), 10.5, FAINT, 0));
                });
                body.spawn((Node { width: Val::Percent(100.), aspect_ratio: Some(720. / 200.), flex_shrink: 0., ..default() }, narrate::ChartNode(chart.ids.1.clone(), chart.window.0, chart.window.1))).with_children(|img| {
                    img.spawn((Node { border_radius: BorderRadius::all(Val::Px(4.)), position_type: PositionType::Absolute, width: Val::Percent(100.), height: Val::Percent(100.), ..default() }, ImageNode::new(chart.image.clone()), Pickable::IGNORE));
                    img.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), position_type: PositionType::Absolute, width: Val::Px(12.), height: Val::Px(12.), margin: UiRect { left: Val::Px(-6.), top: Val::Px(-6.), ..default() }, border: UiRect::all(Val::Px(2.)), ..default() }, BackgroundColor(ACCENT), BorderColor::all(Color::WHITE), PhaseDot { ids: chart.ids.clone(), window: chart.window, range: chart.range }, Pickable::IGNORE));
                });
                body.spawn(k.text(format!("y: {} {} … {} ({}) · the dot is the operating point now; the line is the whole run", chart.y.0, num(chart.range.0), num(chart.range.1), chart.y.1), 10.5, FAINT, 0));
            }
            if let Some(run) = &a.run {
                if !a.overrides.is_empty() && !run.checks.is_empty() {
                    body.spawn(k.text("The claims below are written for the lesson's values; with yours they may not hold. Use Lesson values to return.", 11.5, FAINT, 0));
                }
                if !run.checks.is_empty() && a.overrides.is_empty() {
                    body.spawn(k.section("What this scene shows"));
                    for c in &run.checks {
                        body.spawn(Node { column_gap: Val::Px(8.), align_items: AlignItems::FlexStart, flex_shrink: 0., ..default() }).with_children(|r| {
                            r.spawn(k.text(if c.passed { "holds" } else { "fails" }, 11.5, if c.passed { OK } else { WARN }, 2));
                            r.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.), flex_shrink: 1., min_width: Val::Px(0.), ..default() }).with_children(|t| {
                                if !c.why.is_empty() {
                                    t.spawn(k.text(&c.why, 12.5, TEXT, 1));
                                }
                                t.spawn(k.text(&c.message, 11., SUBTLE, 0));
                            });
                        });
                    }
                }
                if let Some(e) = &run.error {
                    body.spawn(k.text(format!("The run stopped early: {e}"), 11.5, WARN, 0));
                }
                if !run.applied.is_empty() {
                    body.spawn(k.text(run.applied.iter().map(|(t, key, v)| format!("t = {t:.3} s: {key} = {}", num(*v))).collect::<Vec<_>>().join(" · "), 10.5, FAINT, 0));
                }
            }
            if let Some(sb) = &a.sandbox {
                if sb.modified {
                    body.spawn(wrap()).with_children(|r| {
                        r.spawn(k.text(if sb.outdated { "You changed this scene in the builder, and the lesson's system changed since." } else { "This scene shows your changes from the builder." }, 11.5, WARN, 1));
                        r.spawn(k.button("Reset to the lesson", LessonAction::ResetSandbox, Look::Ghost, true));
                        r.spawn(k.button("Save to the system file", LessonAction::SaveSandbox, Look::Ghost, !sb.outdated));
                    });
                }
            }
            let _ = scene;
        });
    });
}

/// Time window of a chart (for placing its playhead).
#[derive(Component)]
pub(crate) struct ChartWindow(pub f64, pub f64);

fn component_card(col: &mut ChildSpawnerCommands, k: &Kit, _l: &Learn, b: &sim_lesson::Block, card: &sim_lesson::ComponentCard, builder: Option<&Builder>) {
    let entry = builder.and_then(|b| b.element_entry(&card.component));
    let category = entry.and_then(|e| e.notes.as_ref().map(|n| n.category.clone())).unwrap_or_default();
    let tag = crate::builder::ui::tag_color(crate::builder::ui::category(if category.is_empty() { entry.map(|e| e.domain.as_str()).unwrap_or("") } else { &category }));
    col.spawn((Node { border_radius: BorderRadius::all(Val::Px(7.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.), padding: UiRect::all(Val::Px(14.)), border: UiRect::left(Val::Px(3.)), margin: UiRect::vertical(Val::Px(4.)), flex_shrink: 0., ..default() }, BackgroundColor(RAISED), BorderColor::all(tag), BlockNode(b.id.clone()))).with_children(|c| {
        c.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn(k.text(entry.map(|e| e.display_name.clone()).unwrap_or_else(|| card.component.clone()), 16., TEXT, 2));
            r.spawn((Text::new(&card.component), TextFont { font: k.f.mono.clone().into(), font_size: FontSize::Px(11.), ..default() }, TextColor(FAINT)));
        });
        let Some(e) = entry else {
            c.spawn(k.text("This component is not in the registry (see sim-lesson check).", 12., WARN, 0));
            return;
        };
        let Some(n) = &e.notes else {
            c.spawn(k.text("No notes for this component yet; its ports and parameters are in the builder library.", 12., SUBTLE, 0));
            return;
        };
        let show: Vec<&str> = if card.show.is_empty() { vec!["summary", "equations", "tradeoffs"] } else { card.show.iter().map(String::as_str).collect() };
        for section in show {
            match section {
                "summary" => {
                    c.spawn(k.text(&n.summary, 13., TEXT, 0));
                }
                "explanation" => paragraph(c, k, "How it works", &n.explanation),
                "equations" => equations(c, k, &n.equations.iter().map(String::as_str).collect::<Vec<_>>()),
                "tradeoffs" => paragraph(c, k, "Trade-offs", &n.tradeoffs),
                "limits" => paragraph(c, k, "Limits", &n.limits),
                "parameters" => {
                    c.spawn(k.section("Parameters"));
                    for p in &e.parameters {
                        c.spawn(k.text(format!("{}{} — {}", p.name, if p.unit.is_empty() { String::new() } else { format!(" ({})", p.unit) }, if p.help.is_empty() { "no description" } else { &p.help }), 12., SUBTLE, 0));
                    }
                }
                _ => {}
            }
        }
    });
}

fn compare_card(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, b: &sim_lesson::Block, c: &sim_lesson::Compare) {
    let state = l.compares.get(&c.id);
    col.spawn((card_frame(), BlockNode(b.id.clone()))).with_children(|card| {
        card.spawn((Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, padding: UiRect::axes(Val::Px(12.), Val::Px(8.)), ..default() }, BackgroundColor(RAISED))).with_children(|h| {
            h.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.), ..default() }).with_children(|t| {
                t.spawn(k.text(if c.title.is_empty() { format!("Comparison · {}", c.study) } else { c.title.clone() }, 14., TEXT, 2));
                t.spawn(k.text(format!("Saved study `{}` in {} · detailed model", c.study, c.system), 11., SUBTLE, 0));
            });
            let running = state.is_some_and(|s| s.job.is_some());
            if running {
                let (done, total) = state.map(|s| (s.progress.0.load(Ordering::Relaxed), s.progress.1.load(Ordering::Relaxed))).unwrap_or((0, 0));
                h.spawn(k.text(format!("Running {done}/{total}…"), 12., SUBTLE, 0));
            } else {
                h.spawn(k.button(if state.is_some_and(|s| s.result.is_some()) { "Run again" } else { "Run comparison" }, LessonAction::RunCompare(c.id.clone()), Look::Primary, true));
            }
        });
        card.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.), padding: UiRect::all(Val::Px(12.)), ..default() }).with_children(|body| {
            if !c.caption.is_empty() {
                body.spawn(k.text(&c.caption, 12.5, SUBTLE, 0));
            }
            match state.and_then(|s| s.result.as_ref()) {
                None => {
                    body.spawn(k.text("Runs every variant on the shared runtime (cached once run).", 11.5, FAINT, 0));
                }
                Some(Err(e)) => {
                    body.spawn(k.text(e, 12., WARN, 0));
                }
                Some(Ok(run)) => {
                    let table = compare_table(run);
                    crate::markdown::table(body, &page_theme(k), &table);
                    if run.variants.iter().any(|v| v.2.is_some()) {
                        body.spawn(k.text("A variant that failed shows its error instead of values.", 11., FAINT, 0));
                    }
                }
            }
        });
    });
}

/// One row per variant, one column per metric; numbers right-aligned.
fn compare_table(run: &CompareRun) -> sim_markdown::Table {
    use sim_markdown::Align;
    let mut metrics: Vec<String> = Vec::new();
    for (_, values, _) in &run.variants {
        for (label, _) in values {
            if !metrics.contains(label) {
                metrics.push(label.clone());
            }
        }
    }
    let head: Vec<String> = std::iter::once("Variant".to_string()).chain(metrics.iter().cloned()).collect();
    let rows: Vec<Vec<String>> = run
        .variants
        .iter()
        .map(|(label, values, error)| {
            let mut row = vec![label.clone()];
            match error {
                Some(e) => row.push(format!("failed: {e}")),
                None => row.extend(metrics.iter().map(|m| values.iter().find(|(l, _)| l == m).map(|(_, v)| if v.is_finite() { num(*v) } else { "–".into() }).unwrap_or_else(|| "–".into()))),
            }
            row
        })
        .collect();
    let align: Vec<Align> = std::iter::once(Align::Left).chain(metrics.iter().map(|_| Align::Right)).collect();
    sim_markdown::Table::plain(&head, &rows, &align)
}

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

fn margin(commands: &mut Commands, k: &Kit, l: &Learn, builder: Option<&Builder>, scene: &SpatialScene, offset: f32) {
    let host = MarginHost { l };
    let threads = l.threads();
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, right: Val::Px(0.), top: Val::Px(TOPBAR), bottom: Val::Px(STATUSBAR), width: Val::Px(RIGHT_WIDTH), flex_direction: FlexDirection::Column, border: UiRect::left(Val::Px(1.)), ..default() },
            BackgroundColor(SURFACE),
            BorderColor::all(BORDER),
            LearnPanel,
        ))
        .with_children(|panel| {
            panel
                .spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(10.), padding: UiRect::all(Val::Px(16.)), flex_grow: 1., overflow: Overflow::scroll_y(), ..default() }, ScrollPosition(Vec2::new(0.0, offset)), LearnScroll::Margin))
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
                        body.spawn(k.text(&t.title, 18., TEXT, 2));
                        annotate::anchors(body, k, &host, &t.targets);
                        body.spawn(wrap()).with_children(|r| {
                            r.spawn(k.button(if t.resolved { "Reopen" } else { "Resolve" }, LessonAction::Resolve, Look::Ghost, true));
                            r.spawn(k.button("Delete note", LessonAction::DeleteThread, Look::Danger, true));
                        });
                        agent_card(body, k, l, &t.id);
                        annotate::messages(body, k, &host, t, l.menu.as_deref());
                    } else if let Some(d) = &l.draft {
                        body.spawn(k.text("New note", 18., TEXT, 2));
                        body.spawn(k.text("Attached to", 11., SUBTLE, 0));
                        annotate::anchors(body, k, &host, std::slice::from_ref(d));
                    } else {
                        body.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, flex_shrink: 0., ..default() }).with_children(|r| {
                            r.spawn(k.text("Notes", 18., TEXT, 2));
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

fn status(commands: &mut Commands, k: &Kit, l: &Learn) {
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(0.), right: Val::Px(0.), bottom: Val::Px(0.), height: Val::Px(STATUSBAR), padding: UiRect::axes(Val::Px(14.), Val::Px(0.)), align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, border: UiRect::top(Val::Px(1.)), ..default() },
            BackgroundColor(BAR),
            BorderColor::all(BORDER),
            LearnPanel,
        ))
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
    let delta = wheel.read().fold(0.0, |sum, e| {
        sum + match e.unit {
            MouseScrollUnit::Line => e.y * 28.0,
            MouseScrollUnit::Pixel => e.y,
        }
    });
    if !learn.active {
        return;
    }
    let scale = window.scale_factor();
    if delta != 0.0 {
        if let Some(p) = window.cursor_position() {
            let zooming = (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) || keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight)) && scene.learn_view.is_some_and(|v| v.visible.contains(p * scale));
            if !zooming && p.y > TOPBAR && p.y < window.height() - STATUSBAR {
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

/// Simulated time in units that suit the scene's length.
fn clock(t: f64, duration: f64) -> String {
    if duration < 0.5 { format!("{:.1} ms", t * 1e3) } else if duration < 10. { format!("{t:.3} s") } else { format!("{t:.2} s") }
}

/// How the scene is playing relative to real time, in words.
pub(crate) fn pace_label(pace: sim_script::pacing::Pace, user_speed: f64, playing: bool) -> String {
    use sim_script::pacing::{HoldReason, Pace};
    if !playing {
        return "paused".into();
    }
    match pace {
        Pace::Hold { reason, .. } if playing => match reason {
            HoldReason::Read => "holding to read".into(),
            HoldReason::Look => "holding to look".into(),
            HoldReason::Camera => "holding while the view moves".into(),
        },
        Pace::Run { speed } => {
            let s = speed * user_speed;
            if (s - 1.).abs() < 0.02 {
                "real time".into()
            } else if s < 1. {
                format!("slow motion, 1/{} speed", crate::builder::ui::num((1. / s).round().max(2.)))
            } else {
                format!("{}× faster than real time", crate::builder::ui::num((s * 10.).round() / 10.))
            }
        }
        Pace::Hold { .. } => "paused".into(),
    }
}

/// Per-frame text and positions that follow playback (no rebuild).
#[allow(clippy::type_complexity)]
pub(super) fn live_text(
    learn: Res<Learn>,
    mut texts: ParamSet<(Query<&mut Text, With<LiveTime>>, Query<&mut Text, With<CaptionText>>, Query<&mut Text, With<Progress>>)>,
    mut fills: Query<&mut Node, (With<TimeFill>, Without<Playhead>, Without<CaptionBox>)>,
    mut heads: Query<(&mut Node, &ChartWindow), (With<Playhead>, Without<TimeFill>, Without<CaptionBox>, Without<ChartMask>)>,
    mut boxes: Query<&mut Node, (With<CaptionBox>, Without<TimeFill>, Without<Playhead>, Without<ChartMask>)>,
    mut masks: Query<(&mut Node, &ChartWindow), (With<ChartMask>, Without<TimeFill>, Without<Playhead>, Without<CaptionBox>)>,
    mut dots: Query<(&mut Node, &PhaseDot), (Without<ChartMask>, Without<TimeFill>, Without<Playhead>, Without<CaptionBox>)>,
) {
    if !learn.active {
        return;
    }
    let Some(a) = &learn.scene else { return };
    let duration = a.duration();
    let t = a.time;
    for mut text in &mut texts.p0() {
        let magnified = if a.scene.magnify != 1. { format!(" · motion ×{} (display only)", crate::builder::ui::num(a.scene.magnify)) } else { String::new() };
        let value = format!("{} / {} · {}{magnified}", clock(t, duration), clock(duration, duration), pace_label(a.pace(), a.user_speed, a.playing));
        if text.0 != value {
            text.0 = value;
        }
    }
    let caption = a.timeline.state_at(t).caption.unwrap_or_default();
    for mut text in &mut texts.p1() {
        if text.0 != caption {
            text.0 = caption.clone();
        }
    }
    for mut node in &mut boxes {
        let display = if caption.is_empty() { Display::None } else { Display::Flex };
        if node.display != display {
            node.display = display;
        }
    }
    if let Some(p) = a.progress() {
        for mut text in &mut texts.p2() {
            let value = format!("Recording on the shared runtime… {:.0} %", p * 100.0);
            if text.0 != value {
                text.0 = value;
            }
        }
    }
    let fraction = (t / duration).clamp(0.0, 1.0) as f32;
    for mut node in &mut fills {
        let w = Val::Percent(fraction * 100.0);
        if node.width != w {
            node.width = w;
        }
    }
    if let Some(frame) = a.run.as_ref().and_then(|r| r.frame_interpolated(t)) {
        for (mut node, dot) in &mut dots {
            let v = |id: &str| sim_inspect::animation::scalar(Some(&frame), id).map(|s| s.value);
            let (Some(x), Some(y)) = (v(&dot.ids.0), v(&dot.ids.1)) else { continue };
            let fx = ((x - dot.window.0) / (dot.window.1 - dot.window.0).max(1e-12)).clamp(0., 1.);
            let fy = 1. - ((y - dot.range.0) / (dot.range.1 - dot.range.0).max(1e-12)).clamp(0., 1.);
            let (left, top) = (Val::Percent(fx as f32 * 100.), Val::Percent(fy as f32 * 100.));
            if node.left != left || node.top != top {
                node.left = left;
                node.top = top;
            }
        }
    }
    for (mut node, window) in &mut masks {
        let span = (window.1 - window.0).max(1e-12);
        let x = (((t - window.0) / span).clamp(0.0, 1.0) * 100.0) as f32;
        if node.left != Val::Percent(x) {
            node.left = Val::Percent(x);
            node.width = Val::Percent(100. - x);
        }
    }
    for (mut node, window) in &mut heads {
        let span = (window.1 - window.0).max(1e-12);
        let x = Val::Percent((((t - window.0) / span).clamp(0.0, 1.0) * 100.0) as f32);
        if node.left != x {
            node.left = x;
        }
    }
}
