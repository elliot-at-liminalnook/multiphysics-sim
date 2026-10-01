//! The narration bar under the reading column, the explainer in the
//! outline, and their per-frame text.
use super::*;

/// Seek bar under the subtitles.
#[derive(Component)]
pub(crate) struct NarrationBar;
#[derive(Component)]
pub(crate) struct NarrationFill;
#[derive(Component)]
pub(crate) struct Subtitle;
/// The whole section's words (the transcript setting).
#[derive(Component)]
pub(crate) struct Transcript;
#[derive(Component)]
pub(crate) struct TranscriptRest;
#[derive(Component)]
pub(crate) struct SubtitleRest;
#[derive(Component)]
pub(crate) struct NarrationTime;
#[derive(Component)]
pub(crate) struct JobProgress;

/// Input: pressing or dragging the narration bar (a kit slider over the
/// section's fraction) seeks within the section, every frame it is held
/// (the same `Seek { time_s }` as REST `lesson_narration`).
pub(in crate::lesson) fn seek(bars: Query<(&bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction), With<NarrationBar>>, learn: Res<Learn>, mut out: MessageWriter<Act<super::super::actions::LessonCommand>>) {
    for (value, pressed, interaction) in &bars {
        if !crate::ui_kit::slider_held(pressed, interaction) {
            continue;
        }
        let Some(n) = learn.narration.as_ref() else { continue };
        let time_s = value.0.clamp(0., 1.) as f64 * n.duration();
        out.write(Act::ui(super::super::actions::LessonCommand::Ui(LessonAction::Narrate(NarrateAction::Seek { time_s }))));
    }
}

/// Height kept free under the reading column for the narration bar.
pub(in crate::lesson) fn reserved(l: &Learn) -> f32 {
    match &l.narration {
        None => 0.,
        Some(n) if n.playing || n.time > 0. || n.section > 0 => 132.,
        Some(_) => 76.,
    }
}

/// The narration bar at the bottom of the reading column.
pub(in crate::lesson) fn bar(commands: &mut Commands, k: &Kit, l: &Learn) {
    let Some(n) = &l.narration else { return };
    let started = n.playing || n.time > 0. || n.section > 0;
    commands
        .spawn((
            Node { border_radius: BorderRadius::all(Val::Px(9.)), position_type: PositionType::Absolute, left: Val::Px(LEFT_WIDTH + 24.), right: Val::Px(RIGHT_WIDTH + 24.), bottom: Val::Px(STATUSBAR + 10.), max_height: Val::Px(reserved(l) - 16.), overflow: Overflow::clip(), flex_direction: FlexDirection::Column, row_gap: Val::Px(8.), padding: UiRect::axes(Val::Px(14.), Val::Px(10.)), border: UiRect::all(Val::Px(1.)), ..default() },
            BackgroundColor(Color::srgba(0.07, 0.086, 0.106, 0.96)),
            BorderColor::all(BORDER),
            GlobalZIndex(20),
            super::super::ui::LearnPanel,
        ))
        .with_children(|b| {
            b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.), ..default() }).with_children(|r| {
                let act = |a: NarrateAction| LessonAction::Narrate(a);
                r.spawn(k.button(if n.playing { "Pause" } else if started { "Resume" } else { "Listen" }, act(if n.playing { NarrateAction::Pause } else { NarrateAction::Play }), Look::Primary, true));
                r.spawn(k.button("‹", act(NarrateAction::Prev), Look::Ghost, true));
                r.spawn(k.button("›", act(NarrateAction::Next), Look::Ghost, n.section + 1 < n.explainer.sections.len()));
                r.spawn(k.button("Say that again", act(NarrateAction::Replay), Look::Ghost, started));
                let s = &n.explainer.sections[n.section];
                let kind = match n.timings.get(n.section).map(|t| t.kind) {
                    Some(TimingKind::Aligned) => "voice",
                    Some(TimingKind::Estimated) => "voice · estimated timing",
                    _ => "no audio yet · subtitles only",
                };
                // A section whose text changed since its voice was made plays silently: say so plainly.
                let silent = n.explainer.audio(&n.manifest, s).is_none() && n.explainer.sections.iter().any(|x| n.explainer.audio(&n.manifest, x).is_some());
                let kind = if silent { "no voice for this section's current text · subtitles only" } else { kind };
                r.spawn((Node { flex_direction: FlexDirection::Column, flex_grow: 1., min_width: Val::Px(0.), ..default() }, children![k.text(format!("{} / {} · {}", n.section + 1, n.explainer.sections.len(), s.title), 13., TEXT, 2), k.text(kind, 10.5, if silent { WARN } else { FAINT }, if silent { 1 } else { 0 })]));
                if silent && n.job.is_none() {
                    r.spawn(k.button("Make its voice", act(NarrateAction::Generate { section: Some(s.id.clone()) }), Look::Ghost, true));
                }
                if let Some(w) = &n.wait {
                    r.spawn(k.text(match w { Wait::SceneStops => "waiting for the scene", Wait::SceneReady(_) => "waiting for the scene to load", Wait::Quiz(_) => "your turn: answer the question" }, 11., ACCENT, 1));
                }
                r.spawn((k.text("", 11.5, SUBTLE, 0), NarrationTime));
                r.spawn(k.button("Stop", act(NarrateAction::Stop), Look::Ghost, started));
            });
            if started {
                b.spawn((Node { flex_wrap: FlexWrap::Wrap, ..default() }, children![(Text::new(""), TextFont { font: k.f.medium.clone().into(), font_size: FontSize::Px(15.), ..default() }, TextColor(TEXT), TextLayout::linebreak(bevy::text::LineBreak::WordBoundary), Subtitle, children![(TextSpan::new(""), TextFont { font: k.f.regular.clone().into(), font_size: FontSize::Px(15.), ..default() }, TextColor(FAINT), SubtitleRest)])]));
                if l.settings.transcript {
                    b.spawn((k.scroll_area(Node { border_radius: BorderRadius::all(Val::Px(6.)), max_height: Val::Px(110.), padding: UiRect::all(Val::Px(8.)), border: UiRect::all(Val::Px(1.)), ..default() }, 0.), BorderColor::all(BORDER))).with_children(|t| {
                        t.spawn((Text::new(""), TextFont { font: k.f.regular.clone().into(), font_size: FontSize::Px(13.), ..default() }, TextColor(TEXT), TextLayout::linebreak(bevy::text::LineBreak::WordBoundary), Transcript, children![(TextSpan::new(""), TextFont { font: k.f.regular.clone().into(), font_size: FontSize::Px(13.), ..default() }, TextColor(FAINT), TranscriptRest)]));
                    });
                }
                let at = (n.time / n.duration().max(1e-9)).clamp(0., 1.) as f32;
                b.spawn(k.slider(SliderLook::Scrub, at, NarrationBar, "Narration position"))
                    .with_children(|bar| {
                        bar.spawn((Node { border_radius: BorderRadius::all(Val::Px(3.)), width: Val::Percent(0.), height: Val::Percent(100.), ..default() }, BackgroundColor(ACCENT), NarrationFill, Pickable::IGNORE));
                    });
            }
        });
}

/// The explainer's sections in the outline, with audio state and generation.
pub(in crate::lesson) fn outline(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn) {
    let Some(n) = &l.narration else { return };
    col.spawn(k.section("Explainer"));
    let plan = sim_voice::plan(&n.explainer);
    let missing: f64 = plan.iter().filter(|p| p.status != sim_voice::Status::Current).map(|p| p.estimated_usd).sum();
    for (i, (item, s)) in plan.iter().zip(&n.explainer.sections).enumerate() {
        let current = i == n.section && (n.playing || n.time > 0.);
        col.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.), flex_shrink: 0., ..default() }).with_children(|r| {
            r.spawn((Node { flex_grow: 1., min_width: Val::Px(0.), ..default() }, children![k.button(&format!("{}{}", if current { "▸ " } else { "" }, s.title), LessonAction::Narrate(NarrateAction::Section { index: i }), Look::Ghost, true)]));
            let (label, color) = match item.status {
                sim_voice::Status::Current => ("voice", OK),
                sim_voice::Status::Stale => ("changed", WARN),
                sim_voice::Status::Missing => ("silent", FAINT),
            };
            r.spawn(k.text(label, 10.5, color, 1));
            r.spawn(k.button(if item.status == sim_voice::Status::Current { "↻" } else { "Make" }, LessonAction::Narrate(NarrateAction::Generate { section: Some(s.id.clone()) }), Look::Ghost, n.job.is_none()));
        });
    }
    match &n.job {
        Some(j) => {
            col.spawn(k.text(&j.label, 11.5, TEXT, 1));
            col.spawn((k.text("", 11., SUBTLE, 0), JobProgress));
        }
        None if missing > 0. => {
            col.spawn(k.button(&format!("Make missing voice (~${missing:.2})"), LessonAction::Narrate(NarrateAction::Generate { section: None }), Look::Secondary, true));
            col.spawn(k.text("Gemini 3.8 Flash TTS via OpenRouter; timing aligned with whisper-1. Estimated cost; capped at $1 per run.", 10.5, FAINT, 0));
        }
        None => {}
    }
    if let Some(r) = &n.last_report {
        col.spawn(k.text(r, 10.5, SUBTLE, 0));
    }
}

/// Subtitles, clock, progress (no rebuild).
#[allow(clippy::type_complexity)]
pub(in crate::lesson) fn live(
    learn: Res<Learn>,
    mut said: Query<&mut Text, (With<Subtitle>, Without<NarrationTime>, Without<JobProgress>)>,
    mut rest: Query<&mut TextSpan, With<SubtitleRest>>,
    mut clock: Query<&mut Text, (With<NarrationTime>, Without<Subtitle>, Without<JobProgress>)>,
    mut progress: Query<&mut Text, (With<JobProgress>, Without<Subtitle>, Without<NarrationTime>)>,
    mut fill: Query<&mut Node, With<NarrationFill>>,
    mut transcript: Query<&mut Text, (With<Transcript>, Without<Subtitle>, Without<NarrationTime>, Without<JobProgress>)>,
    mut transcript_rest: Query<&mut TextSpan, (With<TranscriptRest>, Without<SubtitleRest>)>,
) {
    let Some(n) = &learn.narration else { return };
    if !transcript.is_empty() {
        let (said, rest) = n.transcript();
        let rest = if said.is_empty() { rest } else { format!(" {rest}") };
        for mut t in &mut transcript {
            if t.0 != said {
                t.0 = said.clone();
            }
        }
        for mut t in &mut transcript_rest {
            if t.0 != rest {
                t.0 = rest.clone();
            }
        }
    }
    let (a, b) = n.subtitle();
    for mut t in &mut said {
        if t.0 != a {
            t.0 = a.clone();
        }
    }
    let b = if a.is_empty() { b } else { format!(" {b}") };
    for mut t in &mut rest {
        if t.0 != b {
            t.0 = b.clone();
        }
    }
    let value = format!("{:.0}:{:02.0} / {:.0}:{:02.0}", (n.time / 60.).floor(), n.time % 60., (n.duration() / 60.).floor(), n.duration() % 60.);
    for mut t in &mut clock {
        if t.0 != value {
            t.0 = value.clone();
        }
    }
    if let Some(j) = &n.job {
        let p = j.progress();
        for mut t in &mut progress {
            if t.0 != p {
                t.0 = p.clone();
            }
        }
    }
    let w = Val::Percent(((n.time / n.duration().max(1e-9)).clamp(0., 1.) * 100.) as f32);
    for mut node in &mut fill {
        if node.width != w {
            node.width = w;
        }
    }
}
