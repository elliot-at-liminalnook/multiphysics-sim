//! The left dock: review, lessons by category, concepts, this lesson's
//! headings, reading settings and scenes.
use super::*;

pub(super) fn outline(commands: &mut Commands, k: &Kit, l: &Learn, offset: f32) {
    commands
        .spawn((k.dock(Dock::Left { top: TOPBAR, bottom: STATUSBAR, width: LEFT_WIDTH }, Node { flex_direction: FlexDirection::Column, ..default() }), LearnPanel))
        .with_children(|panel| {
            panel.spawn((k.scroll_area(Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(14.)), row_gap: Val::Px(4.), flex_grow: 1., min_height: Val::Px(0.), ..default() }, offset), LearnScroll::Outline)).with_children(|col| outline_body(col, k, l));
        });
}

/// The outline's contents: review, lessons by category, concepts, this
/// lesson's headings, the explainer, reading settings and scenes.
fn outline_body(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn) {
    super::super::practice::review_outline(col, k, l);
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
            Tint::CLEAR,
            AccessibleLabel::new(g.category.title.as_str()),
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
                Tint::selectable(current),
                AccessibleLabel::new(format!("{}, {subtitle}", e.title)),
                Node { border_radius: BorderRadius::all(Val::Px(4.)), flex_direction: FlexDirection::Column, row_gap: Val::Px(2.), padding: UiRect::axes(Val::Px(9.), Val::Px(7.)), border: UiRect::left(Val::Px(2.)), flex_shrink: 0., ..default() },
                BorderColor::all(if current { ACCENT } else { Color::NONE }),
                BackgroundColor(Tint::selectable(current).idle),
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
                Tint::CLEAR,
                AccessibleLabel::new(text),
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
            row.insert((Button, LessonAction::Open(t.clone()), Tint::CLEAR, AccessibleLabel::new(format!("{}, {}/{}", m.title, m.solid, m.questions)), BackgroundColor(Color::NONE)));
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
