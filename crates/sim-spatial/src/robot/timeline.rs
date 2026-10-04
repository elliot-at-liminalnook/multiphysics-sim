//! The inspector's Timeline block for a live run (`run::history`): a kit
//! timebar over the kept frame history (the browser's timeline slider, which
//! there shows a live run's progress), a line with the live and reviewed
//! times, and Live. A press or drag on the bar writes `RobotAction::History
//! {t}` (REST `robot_history`) while held and only when the time changes;
//! otherwise the bar follows the reviewed time, or the latest frame.
use super::*;
use crate::app::actions::Act;
use crate::ui_kit::SliderLook;

#[derive(Component)]
pub(super) struct TimelineRoot;
#[derive(Component)]
pub(super) struct TimelineBar;
#[derive(Component)]
pub(super) struct TimelineFill;
#[derive(Component)]
pub(super) struct TimelineText;
#[derive(Component)]
pub(super) struct TimelineButton;

fn fraction(t: f64, (first, last): (f64, f64)) -> f32 {
    if last > first { ((t - first) / (last - first)).clamp(0.0, 1.0) as f32 } else { 1.0 }
}

/// The run with a history timeline: a live run (not a recorded preset, not a planar file).
fn timeline_run(view: &RobotView) -> Option<&RunController> {
    view.run.as_ref().filter(|r| r.recorded().is_none() && view.planar.is_none())
}

/// Present: the Timeline block (built once per root; values every frame).
#[allow(clippy::type_complexity)]
pub(super) fn timeline_panel(
    mut commands: Commands,
    view: Res<RobotView>,
    fonts: Res<UiFonts>,
    root: Single<Entity, With<TimelineRoot>>,
    mut shown: Local<Option<Entity>>,
    bars: Query<(Entity, &bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction), With<TimelineBar>>,
    mut fill: Query<&mut Node, With<TimelineFill>>,
    mut text: Query<&mut Text, With<TimelineText>>,
    mut buttons: Query<(&RobotAction, &mut Enabled), With<TimelineButton>>,
) {
    let Some(run) = timeline_run(&view) else {
        if shown.take().is_some() {
            commands.entity(*root).despawn_related::<Children>();
        }
        return;
    };
    let span = run.history_span();
    if *shown != Some(*root) {
        let k = Kit { f: &fonts };
        let header = commands.spawn(k.section("Timeline")).id();
        let label = commands.spawn(k.text(format!("the run's last {} s of frames, for review (nothing is re-simulated; Run continues from the live state)", run::HISTORY_S), size::CAPTION, SUBTLE, 0)).id();
        let at = span.map_or(1.0, |s| fraction(run.review_time().unwrap_or(s.1), s));
        let bar = commands
            .spawn(Node { align_items: AlignItems::Center, padding: UiRect::vertical(Val::Px(4.0)), flex_shrink: 0.0, ..default() })
            .with_children(|r| {
                r.spawn(k.slider(SliderLook::Timebar, at, TimelineBar, "Run history timeline")).with_children(|t| {
                    t.spawn((Node { border_radius: BorderRadius::all(Val::Px(5.0)), width: Val::Percent(at * 100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(ACCENT), TimelineFill, Pickable::IGNORE));
                });
            })
            .id();
        let row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).id();
        let live = RobotAction::History { t: None };
        let enabled = check(&view, &live).is_ok() && run.review_time().is_some();
        let b = commands.spawn((k.button("Live", live, Look::Secondary, enabled), TimelineButton)).id();
        let line = commands.spawn((k.text("", size::CAPTION, TEXT, 0), TimelineText, Node { flex_shrink: 1.0, ..default() })).id();
        commands.entity(row).add_children(&[b, line]);
        commands.entity(*root).add_children(&[header, label, bar, row]);
        *shown = Some(*root);
    }
    let at = span.map_or(1.0, |s| fraction(run.review_time().unwrap_or(s.1), s));
    for (entity, value, pressed, interaction) in &bars {
        let held = crate::ui_kit::slider_held(pressed, interaction);
        if !held && (value.0 - at).abs() > 1e-4 {
            commands.entity(entity).insert(bevy::ui_widgets::SliderValue(at));
        }
        let shown_at = if held { value.0 } else { at };
        for mut node in &mut fill {
            let width = Val::Percent(shown_at * 100.0);
            if node.width != width {
                node.width = width;
            }
        }
    }
    let live_t = run.frame().map(|f| f.time);
    let line = match (span, run.reviewed()) {
        (None, _) => "no frames yet: Run or Step".to_string(),
        (Some((a, b)), Some(f)) => format!("reviewing t {:.3} s (frame {:.3} s) · kept {a:.2}–{b:.2} s · live t {} s", run.review_time().unwrap_or(f.time), f.time, live_t.map_or("—".into(), |t| format!("{t:.3}"))),
        (Some((a, b)), None) => format!("live t {} s · kept {a:.2}–{b:.2} s · drag the bar to review", live_t.map_or("—".into(), |t| format!("{t:.3}"))),
    };
    for mut t in &mut text {
        if t.0 != line {
            t.0 = line.clone();
        }
    }
    for (action, enabled) in &mut buttons {
        enable(enabled, check(&view, action).is_ok() && run.review_time().is_some());
    }
}

/// Input: a press or drag on the timeline writes `History {t}` when the time changes.
pub(super) fn timeline_seek(bars: Query<(&bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction), With<TimelineBar>>, view: Res<RobotView>, mut sent: Local<Option<f64>>, mut out: MessageWriter<Act<RobotAction>>) {
    let mut held = false;
    for (value, pressed, interaction) in &bars {
        if !crate::ui_kit::slider_held(pressed, interaction) {
            continue;
        }
        held = true;
        let Some((first, last)) = timeline_run(&view).and_then(RunController::history_span) else { continue };
        let t = (first + f64::from(value.0.clamp(0.0, 1.0)) * (last - first)).clamp(first, last);
        if *sent != Some(t) {
            *sent = Some(t);
            let action = RobotAction::History { t: Some(t) };
            if check(&view, &action).is_ok() {
                out.write(Act::ui(action));
            }
        }
    }
    if !held {
        *sent = None;
    }
}
