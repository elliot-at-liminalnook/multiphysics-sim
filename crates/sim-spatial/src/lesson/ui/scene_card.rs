//! The scene card (the live 3D view, its controls, sliders, charts and
//! claims) and the per-frame text that follows playback.
use super::*;

pub(in crate::lesson) fn scene_card(col: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, b: &sim_lesson::Block, s: &Scene, notes: usize, scene: &SpatialScene) {
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
                // The kit slider over the run's fraction; `seek` turns a press or drag into `SeekTo`.
                let at = (a.time / a.duration().max(1e-9)).clamp(0., 1.) as f32;
                c.spawn((k.slider(SliderLook::Timebar, at, LessonAction::Seek, "Timeline"), Timebar))
                    .with_children(|bar| {
                        bar.spawn((Node { border_radius: BorderRadius::all(Val::Px(5.)), width: Val::Percent(0.), height: Val::Percent(100.), ..default() }, BackgroundColor(ACCENT), TimeFill, Pickable::IGNORE));
                        // Event marks: captions, parameter changes and pauses (←/→ jump between them).
                        let d = a.duration().max(1e-9);
                        for t in super::super::event_times(a) {
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
                        let name = if sl.label.is_empty() { &sl.parameter } else { &sl.label };
                        r.spawn((Button, crate::ui_kit::activation::Ordinary, LessonAction::SliderFocus(sl.parameter.clone()), AccessibleLabel::new(name.as_str()), Node { border_radius: BorderRadius::all(Val::Px(4.)), width: Val::Px(150.), padding: UiRect::axes(Val::Px(4.), Val::Px(2.)), border: UiRect::all(Val::Px(1.)), ..default() }, BackgroundColor(Color::NONE), BorderColor::all(if focused { ACCENT } else { Color::NONE }), children![k.text(name, 12., TEXT, 1)]));
                        // The kit slider over the parameter's fraction of its range; `sliders` previews and sets it.
                        let at = a.slider_value(scene, &sl.parameter).map(|v| ((v - sl.min) / (sl.max - sl.min).max(1e-12)).clamp(0., 1.) as f32).unwrap_or(0.);
                        r.spawn(k.slider(SliderLook::Track, at, super::super::SliderTrack(sl.parameter.clone()), name))
                            .with_children(|t| {
                                t.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.)), width: Val::Percent(0.), height: Val::Percent(100.), ..default() }, BackgroundColor(ACCENT), super::super::SliderFill(sl.parameter.clone()), Pickable::IGNORE));
                            });
                        r.spawn((k.text("", 12., TEXT, 1), super::super::SliderValue(sl.parameter.clone()), Node { width: Val::Px(90.), ..default() }));
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
                // A pointer surface (hover previews the moment, `chart_hover`); its
                // border lights when the chart's part is hovered.
                body.spawn((Node { border_radius: BorderRadius::all(Val::Px(4.)), width: Val::Percent(100.), aspect_ratio: Some(720. / 200.), flex_shrink: 0., border: UiRect::all(Val::Px(1.)), ..default() }, BorderColor::all(Color::NONE), k.pointer_surface(&format!("{} chart", chart.key), false), super::super::ChartHover(chart.key.clone(), chart.window.0, chart.window.1), narrate::ChartNode(chart.key.clone(), chart.window.0, chart.window.1))).with_children(|img| {
                    let window = chart.window;
                    img.spawn((k.chart_image(chart.image.clone(), Node { border_radius: BorderRadius::all(Val::Px(4.)), position_type: PositionType::Absolute, width: Val::Percent(100.), height: Val::Percent(100.), ..default() }, false), Pickable::IGNORE));
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
                    img.spawn((k.chart_image(chart.image.clone(), Node { border_radius: BorderRadius::all(Val::Px(4.)), position_type: PositionType::Absolute, width: Val::Percent(100.), height: Val::Percent(100.), ..default() }, false), Pickable::IGNORE));
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
        });
    });
}

/// Time window of a chart (for placing its playhead).
#[derive(Component)]
pub(crate) struct ChartWindow(pub f64, pub f64);

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
pub(in crate::lesson) fn live_text(
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
