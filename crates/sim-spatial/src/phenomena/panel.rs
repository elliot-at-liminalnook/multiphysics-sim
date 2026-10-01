//! Phenomena mode's panels, all from the UI kit: a header (title, status
//! line), the exhibit list (left dock), the inspector (right dock: title,
//! summary, the error, verdict, the knob slider and its step buttons,
//! readouts as property rows, time, speed and the run buttons) and the
//! signal's strip chart (under the 3D view).
//!
//! The inspector is rebuilt only when the shown frame's generation or
//! exhibit changes; everything else is set in place, and only when its text
//! changed. Steady labels: numbers keep a fixed number of significant
//! digits and refresh four times a second; the chart redraws at most ten
//! times a second, and only when a sample was added.
//!
//! The kit fonts (IBM Plex Sans) lack a few characters the exhibits use;
//! [`glyphs`] maps those, and every exhibit text goes through it.
use super::ExhibitRef;
use super::actions::PhenomenaAction;
use super::gallery::{Gallery, controls};
use super::run::{CHART_INTERVAL, Frame, knob_target};
use crate::app::ModeScope;
use crate::app::actions::Act;
use crate::builder::ui_api::Enabled;
use crate::ui_kit::{ACCENT, Corner, DANGER, Dock, Kit, LEFT_WIDTH, Look, RIGHT_WIDTH, SUBTLE, SWITCHER_STRIP, SliderLook, TEXT, TOPBAR, Tint, UiFonts, VALUE, WARN, WHEEL_LINE, size, slider_held, wheel_delta, wrap};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use sim_phenomena::exhibit::Knob;

/// The chart strip's height under the 3D view.
pub(super) const CHART_HEIGHT: f32 = 210.0;
/// Seconds between refreshes of the numbers (steady labels).
const VALUE_REFRESH: f64 = 0.25;
/// Seconds between chart redraws.
const CHART_REFRESH: f64 = 0.1;
/// The key bindings, shown in the status line.
const HINT: &str = "] next  ·  [ previous  ·  1–9, 0 exhibits 1–10  ·  ←/→ knob (Shift ×5)  ·  R reset  ·  Space pause  ·  ↑/↓ speed  ·  right-drag orbit, middle or Shift+right-drag pan, wheel zoom";

/// A kit widget's meaning: the phenomena action a press writes.
#[derive(Component, Clone, Debug)]
pub(super) struct PhenomenaButton(pub(super) PhenomenaAction);

/// The `system_ui` control a button stands for (its `Enabled` and label follow it).
#[derive(Component)]
pub(super) struct ControlId(&'static str);

/// A row of the exhibit list (its selection is set in place).
#[derive(Component)]
pub(super) struct ExhibitRow(usize);

/// The scroll areas the wheel moves.
#[derive(Component)]
pub(super) struct ExhibitList;
#[derive(Component)]
pub(super) struct Inspector;

/// The knob slider (the kit slider's marker).
#[derive(Component)]
pub(super) struct KnobSlider;

/// A run row of the inspector.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Row {
    Readout(usize),
    Time,
    Speed,
    Signal,
}

/// A chart axis label.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Axis {
    Top,
    Bottom,
    Window,
}

/// What a panel entity shows (set in place by `refresh` and `chart`).
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Mark {
    Title,
    Status,
    /// The exhibit's error (DANGER) or the `--exhibit` notice (WARN); hidden when neither.
    Alert,
    Verdict,
    KnobValue,
    KnobFill,
    /// A property row's container (its value is the second text under it).
    Value(Row),
    ChartTitle,
    ChartLabel(Axis),
}

/// What the inspector was built for.
#[derive(Clone, PartialEq, Debug)]
enum Shown {
    Nothing,
    Waiting(String),
    /// (A change in the number of readouts rebuilds it too.)
    Exhibit { generation: u64, current: usize, readouts: usize },
}

/// The panels' own state (removed by `phenomena::leave`).
#[derive(Resource)]
pub(super) struct Panels {
    shown: Shown,
    /// How many exhibits the list shows.
    list: usize,
    chart: Handle<Image>,
    /// (generation, samples pushed) the chart image shows.
    chart_key: Option<(u64, u64)>,
    chart_at: f64,
    values_at: f64,
    /// Refresh the numbers now (the inspector was just rebuilt).
    force: bool,
}

/// The kit fonts (IBM Plex Sans) have no glyph for these characters the
/// exhibits use (checked against the fonts' cmap tables): ⁻ ⇒ ∝ ▸ ∓ ⟨ ⟩ ᵀ ẋ ṗ.
/// Everything else in the exhibits' strings (Greek, sub- and superscript
/// digits, arrows, ×, −, ≈, ≤, ≥, °, √, ′, …, ∞, the combining dot above) is
/// covered and kept.
pub(crate) fn glyphs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '⁻' => out.push('−'),
            '⇒' => out.push('→'),
            '∝' => out.push('~'),
            '▸' => out.push('›'),
            // (2 ∓ √2) and (2 ± √2) name the same two values.
            '∓' => out.push('±'),
            '⟨' => out.push('‹'),
            '⟩' => out.push('›'),
            'ᵀ' => out.push_str("^T"),
            'ẋ' => out.push_str("x\u{307}"),
            'ṗ' => out.push_str("p\u{307}"),
            _ => out.push(c),
        }
    }
    out
}

/// A number with four significant digits and a fixed number of decimals
/// for its magnitude (so a label does not jitter in width), scientific
/// outside 0.001…100 000.
pub(crate) fn steady(v: f64) -> String {
    if !v.is_finite() {
        return format!("{v}");
    }
    let a = v.abs();
    if a != 0.0 && !(1e-3..1e5).contains(&a) {
        return format!("{v:.3e}");
    }
    let decimals = if a == 0.0 { 3 } else { (3 - a.log10().floor() as i32).clamp(0, 6) as usize };
    format!("{v:.decimals$}")
}

/// The speed multiplier as the keys reach it: ×4, ×1/8; else three decimals.
fn speed_text(speed: f64) -> String {
    if speed >= 1.0 && speed.fract() == 0.0 {
        format!("×{speed:.0}")
    } else if speed < 1.0 && (1.0 / speed).fract() == 0.0 {
        format!("×1/{:.0}", 1.0 / speed)
    } else {
        format!("×{speed:.3}")
    }
}

/// The knob as the inspector shows it.
fn knob_text(knob: &Knob, value: f64) -> String {
    let unit = glyphs(knob.unit);
    if unit.is_empty() { steady(value) } else { format!("{} {unit}", steady(value)) }
}

/// Where `value` sits on the knob's range, 0…1.
fn fraction(knob: &Knob, value: f64) -> f32 {
    let span = knob.max - knob.min;
    if span > 0.0 { ((value - knob.min) / span).clamp(0.0, 1.0) as f32 } else { 0.0 }
}

/// A row's key, value and unit.
fn row_text(row: Row, f: &Frame) -> Option<(String, String, String)> {
    Some(match row {
        Row::Readout(i) => {
            let r = f.readouts.get(i)?;
            (glyphs(&r.label), steady(r.value), glyphs(r.unit))
        }
        Row::Time => ("Time".into(), format!("{:.3}", f.time), glyphs(f.time_unit)),
        Row::Speed => ("Speed".into(), format!("{}{}", speed_text(f.speed), if f.paused { " (paused)" } else { "" }), String::new()),
        Row::Signal => (glyphs(f.signal.0), steady(f.signal.1), String::new()),
    })
}

/// A property row's value text, as `Kit::property` writes it.
fn shown_value(value: &str, unit: &str) -> String {
    if unit.is_empty() { value.to_string() } else { format!("{value} {unit}") }
}

/// OnEnter(ModeScope::Phenomena): the four docks and the chart image.
pub(super) fn setup(mut commands: Commands, fonts: Res<UiFonts>, mut images: ResMut<Assets<Image>>) {
    let k = Kit::new(&fonts);
    let chart = images.add(crate::chart::blank_image());
    commands.spawn((
        k.dock(Dock::Top { height: TOPBAR }, Node { padding: UiRect::axes(Val::Px(18.0), Val::Px(6.0)), flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, row_gap: Val::Px(2.0), ..default() }),
        DespawnOnExit(ModeScope::Phenomena),
        children![(k.title("Phenomena"), Mark::Title), (k.caption("Building the exhibits…"), Mark::Status)],
    ));
    commands
        .spawn((k.dock(Dock::Left { top: TOPBAR, bottom: 0.0, width: LEFT_WIDTH }, Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(12.0)), row_gap: Val::Px(8.0), ..default() }), DespawnOnExit(ModeScope::Phenomena)))
        .with_children(|p| {
            k.header(p, "Exhibits", "Click one, or ] and [ for the next and previous; 1–9 and 0 open the first ten.");
            p.spawn((k.scroll_area(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), flex_grow: 1.0, min_height: Val::Px(0.0), ..default() }, 0.0), ExhibitList));
        });
    // Every dock ends above the switcher strip (`SWITCHER_STRIP`, added by
    // the kit), so the right dock and chart strip need no switcher room.
    commands
        .spawn((
            k.dock(
                Dock::Right { top: TOPBAR, bottom: 0.0, width: RIGHT_WIDTH },
                Node { flex_direction: FlexDirection::Column, padding: UiRect { left: Val::Px(16.0), right: Val::Px(16.0), top: Val::Px(14.0), bottom: Val::Px(14.0) }, row_gap: Val::Px(6.0), ..default() },
            ),
            DespawnOnExit(ModeScope::Phenomena),
        ))
        .with_children(|p| {
            // The error stays above the scrolled part: never scrolled away.
            p.spawn((k.text("", size::SMALL, DANGER, 1), Node { display: Display::None, flex_shrink: 0.0, ..default() }, Mark::Alert));
            p.spawn((k.scroll_area(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), flex_grow: 1.0, min_height: Val::Px(0.0), ..default() }, 0.0), Inspector));
        });
    commands
        .spawn((
            k.dock(
                Dock::Under { left: LEFT_WIDTH, right: RIGHT_WIDTH, bottom: 0.0, height: CHART_HEIGHT },
                Node { flex_direction: FlexDirection::Column, padding: UiRect { left: Val::Px(12.0), right: Val::Px(12.0), top: Val::Px(8.0), bottom: Val::Px(8.0) }, row_gap: Val::Px(4.0), ..default() },
            ),
            DespawnOnExit(ModeScope::Phenomena),
        ))
        .with_children(|p| {
            p.spawn((k.text("Chart", size::SMALL, SUBTLE, 1), Mark::ChartTitle));
            p.spawn(k.chart_image(chart.clone(), Node { flex_grow: 1.0, min_height: Val::Px(40.0), ..default() }, true)).with_children(|c| {
                c.spawn((k.chart_label("", Corner::TopLeft), Mark::ChartLabel(Axis::Top)));
                c.spawn((k.chart_label("", Corner::BottomLeft), Mark::ChartLabel(Axis::Bottom)));
                c.spawn((k.chart_label("", Corner::BottomRight), Mark::ChartLabel(Axis::Window)));
            });
        });
    commands.insert_resource(Panels { shown: Shown::Nothing, list: 0, chart, chart_key: None, chart_at: 0.0, values_at: 0.0, force: true });
}

/// Input: a press on a kit button or list row writes its action (a
/// disabled button writes nothing; its control names why).
pub(super) fn buttons(clicks: Query<(&Interaction, &PhenomenaButton, Option<&Enabled>), Changed<Interaction>>, mut out: MessageWriter<Act<PhenomenaAction>>) {
    for (interaction, button, enabled) in &clicks {
        if *interaction == Interaction::Pressed && enabled.is_none_or(|e| e.0) {
            out.write(Act::ui(button.0.clone()));
        }
    }
}

/// Input: the knob slider. While held its value is a local preview
/// (`Gallery::knob_drag`, snapped to the knob's step); the release commits
/// it as `PhenomenaKnob { value }`, the REST command's action.
pub(super) fn slider(tracks: Query<(&bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction), With<KnobSlider>>, gallery: Option<ResMut<Gallery>>, mut out: MessageWriter<Act<PhenomenaAction>>) {
    let Some(mut gallery) = gallery else { return };
    let Some(knob) = gallery.ready().and_then(|f| f.knob.clone()) else { return };
    let mut held = None;
    for (value, pressed, interaction) in &tracks {
        if slider_held(pressed, interaction) {
            // The kit slider's value is the pointer's fraction of the track, 0..=1.
            held = Some(knob_target(&knob, Some(knob.min + value.0.clamp(0.0, 1.0) as f64 * (knob.max - knob.min)), None));
        }
    }
    let requested = gallery.requested;
    match (held, gallery.knob_drag) {
        (Some(v), previous) => {
            if previous != Some((v, requested)) {
                gallery.knob_drag = Some((v, requested));
            }
        }
        // Released after a switch (the slider went with the rebuilt panel): the value was for another exhibit.
        (None, Some((_, at))) if at != requested => gallery.knob_drag = None,
        (None, Some((v, _))) => {
            gallery.knob_drag = None;
            // A click that leaves the value where it is commits nothing: a knob change rebuilds the exhibit.
            // The exhibit reports its value back (possibly off the step grid or by an ulp), so compare snapped values with a tolerance.
            let shown = knob_target(&knob, Some(knob.value), None);
            if (v - shown).abs() > knob.step.abs().max(f64::EPSILON) * 1e-9 {
                out.write(Act::ui(PhenomenaAction::PhenomenaKnob { value: Some(v), steps: None }));
            }
        }
        (None, None) => {}
    }
}

/// SimSync: the wheel scrolls the exhibit list or the inspector under the pointer.
pub(super) fn scroll(mut wheel: MessageReader<MouseWheel>, window: Option<Single<&Window, With<PrimaryWindow>>>, mut areas: Query<(&mut ScrollPosition, Has<ExhibitList>), Or<(With<ExhibitList>, With<Inspector>)>>) {
    let delta = wheel_delta(&mut wheel, WHEEL_LINE);
    let Some(window) = window else { return };
    let Some(p) = window.cursor_position().filter(|p| delta != 0.0 && p.y > TOPBAR && p.y < window.height() - SWITCHER_STRIP) else { return };
    let list = if p.x < LEFT_WIDTH {
        true
    } else if p.x > window.width() - RIGHT_WIDTH {
        false
    } else {
        return;
    };
    for (mut position, is_list) in &mut areas {
        if is_list == list {
            // `ui_kit::clamp_scroll_positions` clamps the far end after layout.
            position.y = (position.y - delta).max(0.0);
        }
    }
}

/// Present: the exhibit list when the catalogue arrives, and the inspector
/// when the shown generation or exhibit changes (or while it waits, when
/// the reason changes).
pub(super) fn rebuild(mut commands: Commands, fonts: Res<UiFonts>, gallery: Option<Res<Gallery>>, panels: Option<ResMut<Panels>>, list: Option<Single<Entity, With<ExhibitList>>>, inspector: Option<Single<Entity, With<Inspector>>>) {
    let (Some(gallery), Some(mut panels)) = (gallery, panels) else { return };
    let k = Kit::new(&fonts);
    let frame = gallery.ready();
    if let Some(list) = list {
        let count = frame.map_or(0, |f| f.catalogue.len());
        if panels.list != count {
            panels.list = count;
            let mut entity = commands.entity(*list);
            entity.despawn_related::<Children>();
            if let Some(f) = frame {
                entity.with_children(|p| {
                    for (i, e) in f.catalogue.iter().enumerate() {
                        let subtitle = if i < 10 { format!("Exhibit {}  ·  key {}", i + 1, (i + 1) % 10) } else { format!("Exhibit {}", i + 1) };
                        let action = PhenomenaButton(PhenomenaAction::PhenomenaSelect { exhibit: ExhibitRef::Number(i + 1) });
                        p.spawn((k.list_item(sim_core::icons::for_type(e.title), SUBTLE, &glyphs(e.title), &subtitle, action, i == f.current), ExhibitRow(i)));
                    }
                });
            }
        }
    }
    let Some(inspector) = inspector else { return };
    let want = match frame {
        Some(f) => Shown::Exhibit { generation: f.generation, current: f.current, readouts: f.readouts.len() },
        None => Shown::Waiting(gallery.not_ready()),
    };
    if panels.shown == want {
        return;
    }
    panels.shown = want;
    panels.force = true;
    let mut entity = commands.entity(*inspector);
    entity.despawn_related::<Children>();
    entity.with_children(|p| match frame {
        Some(f) => exhibit(p, &k, f),
        None if gallery.frame.as_ref().is_some_and(|f| f.failed.is_some()) || gallery.stopped() => {
            p.spawn(k.title("The exhibits could not be built"));
            p.spawn(k.text(gallery.not_ready(), size::SMALL, DANGER, 1));
        }
        None => {
            p.spawn(k.title("Building the exhibits…"));
            p.spawn(k.caption("sim_phenomena::exhibits::all() runs on the phenomena-run thread; the gallery opens when it is done."));
        }
    });
}

/// The inspector for the shown exhibit.
fn exhibit(p: &mut ChildSpawnerCommands, k: &Kit, f: &Frame) {
    let entry = f.entry();
    p.spawn(k.title(glyphs(entry.title)));
    p.spawn(k.caption(glyphs(entry.summary)));
    p.spawn(k.section("Verdict"));
    p.spawn((k.text(glyphs(&f.verdict), size::BODY, TEXT, 1), Mark::Verdict));
    if let Some(knob) = &f.knob {
        p.spawn(k.section("Knob"));
        p.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(10.0), flex_shrink: 0.0, ..default() }).with_children(|r| {
            r.spawn(k.text(glyphs(knob.label), size::BODY, SUBTLE, 0));
            r.spawn((k.text(knob_text(knob, knob.value), size::BODY, VALUE, 1), Mark::KnobValue));
        });
        // The kit slider grows along a row.
        p.spawn(Node { align_items: AlignItems::Center, padding: UiRect::vertical(Val::Px(4.0)), flex_shrink: 0.0, ..default() }).with_children(|r| {
            r.spawn(k.slider(SliderLook::Track, fraction(knob, knob.value), KnobSlider, &format!("Knob: {}", glyphs(knob.label)))).with_children(|t| {
                t.spawn((Node { border_radius: BorderRadius::all(Val::Px(6.0)), width: Val::Percent(fraction(knob, knob.value) * 100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(ACCENT), Mark::KnobFill, Pickable::IGNORE));
            });
        });
        let unit = glyphs(knob.unit);
        p.spawn(k.note(format!("{} – {} {unit}  ·  step {}  ·  ←/→ one step, Shift five", steady(knob.min), steady(knob.max), steady(knob.step))));
        p.spawn(wrap()).with_children(|r| {
            r.spawn((k.button("− step", PhenomenaButton(PhenomenaAction::PhenomenaKnob { value: None, steps: Some(-1.0) }), Look::Secondary, true), ControlId("phenomena:knob_down")));
            r.spawn((k.button("+ step", PhenomenaButton(PhenomenaAction::PhenomenaKnob { value: None, steps: Some(1.0) }), Look::Secondary, true), ControlId("phenomena:knob_up")));
        });
    }
    if !f.readouts.is_empty() {
        p.spawn(k.section("Readouts"));
    }
    let rows = (0..f.readouts.len()).map(Row::Readout).chain([Row::Signal, Row::Time, Row::Speed]);
    for (i, row) in rows.enumerate() {
        if i == f.readouts.len() {
            p.spawn(k.section("Run"));
        }
        let Some((key, value, unit)) = row_text(row, f) else { continue };
        p.spawn((Node { flex_direction: FlexDirection::Column, flex_shrink: 0.0, ..default() }, Mark::Value(row))).with_children(|c| k.property(c, &key, &value, &unit, None::<PhenomenaButton>, false));
    }
    p.spawn(wrap()).with_children(|r| {
        r.spawn((k.button(if f.paused { "Run" } else { "Pause" }, PhenomenaButton(PhenomenaAction::PhenomenaPause { paused: None }), Look::Primary, true), ControlId("phenomena:pause")));
        r.spawn((k.button("Reset", PhenomenaButton(PhenomenaAction::PhenomenaReset), Look::Secondary, true), ControlId("phenomena:reset")));
        r.spawn((k.button("Slower ÷2", PhenomenaButton(PhenomenaAction::PhenomenaSpeed { speed: None, steps: Some(-1) }), Look::Secondary, true), ControlId("phenomena:speed_down")));
        r.spawn((k.button("Faster ×2", PhenomenaButton(PhenomenaAction::PhenomenaSpeed { speed: None, steps: Some(1) }), Look::Secondary, true), ControlId("phenomena:speed_up")));
        r.spawn((k.button("‹ Previous", PhenomenaButton(PhenomenaAction::PhenomenaPrevious), Look::Ghost, true), ControlId("phenomena:previous")));
        r.spawn((k.button("Next ›", PhenomenaButton(PhenomenaAction::PhenomenaNext), Look::Ghost, true), ControlId("phenomena:next")));
    });
}

/// Every text entity under `root`, depth first (`root` included).
fn text_entities(root: Entity, children: &Query<&Children>, texts: &Query<&mut Text>, out: &mut Vec<Entity>) {
    if texts.contains(root) {
        out.push(root);
    }
    if let Ok(kids) = children.get(root) {
        for kid in kids.iter() {
            text_entities(kid, children, texts, out);
        }
    }
}

fn set_text(texts: &mut Query<&mut Text>, entity: Entity, value: &str) {
    if let Ok(mut text) = texts.get_mut(entity) {
        if text.0 != value {
            text.0 = value.to_string();
        }
    }
}

/// Present: everything shown in place. The header, the alert, the list's
/// selection, the knob's value and fill follow every frame; the verdict,
/// the rows and the buttons' enabled state four times a second (and at
/// once after a rebuild).
#[allow(clippy::too_many_arguments)]
pub(super) fn refresh(
    time: Res<Time>,
    gallery: Option<Res<Gallery>>,
    panels: Option<ResMut<Panels>>,
    marks: Query<(Entity, &Mark)>,
    mut rows: Query<(&ExhibitRow, &mut Tint, &mut BorderColor)>,
    mut buttons: Query<(&ControlId, &mut Enabled, Option<&Children>)>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
    mut colors: Query<&mut TextColor>,
    mut nodes: Query<&mut Node>,
) {
    let (Some(gallery), Some(mut panels)) = (gallery, panels) else { return };
    let now = time.elapsed_secs_f64();
    let due = panels.force || now - panels.values_at >= VALUE_REFRESH;
    if due {
        panels.values_at = now;
        panels.force = false;
    }
    let frame = gallery.ready();
    let title = match frame {
        Some(f) => format!("Phenomena  ·  {:02} / {}  {}", f.current + 1, f.catalogue.len(), glyphs(f.entry().title)),
        None => "Phenomena".to_string(),
    };
    let failed = frame.is_none() && (gallery.frame.as_ref().is_some_and(|f| f.failed.is_some()) || gallery.stopped());
    let (status, status_color) = match (&gallery.status, frame) {
        (Some(Err(e)), _) => (e.clone(), DANGER),
        (Some(Ok(t)), _) => (t.clone(), SUBTLE),
        (None, Some(_)) => (HINT.to_string(), SUBTLE),
        (None, None) if failed => (format!("Not available: {}", gallery.not_ready()), DANGER),
        (None, None) => ("Building the exhibits…".to_string(), SUBTLE),
    };
    let stopped = frame.is_some() && gallery.stopped();
    let alert = frame.and_then(|f| match (&f.error, &f.notice) {
        (Some(e), _) => Some((format!("Simulation error: {}", glyphs(e)), DANGER)),
        _ if stopped => Some(("The phenomena run thread has stopped: switch to another mode and back to restart it.".to_string(), DANGER)),
        (None, Some(n)) => Some((n.clone(), WARN)),
        (None, None) => None,
    });
    let knob = frame.and_then(|f| f.knob.as_ref());
    let knob_value = knob.map(|k| gallery.knob_drag.map_or(k.value, |(v, _)| v));
    for (entity, mark) in &marks {
        match *mark {
            Mark::Title => set_text(&mut texts, entity, &title),
            Mark::Status => {
                set_text(&mut texts, entity, &status);
                if let Ok(mut c) = colors.get_mut(entity) {
                    if c.0 != status_color {
                        c.0 = status_color;
                    }
                }
            }
            Mark::Alert => {
                let (text, color) = alert.clone().unwrap_or((String::new(), DANGER));
                set_text(&mut texts, entity, &text);
                if let Ok(mut c) = colors.get_mut(entity) {
                    if c.0 != color {
                        c.0 = color;
                    }
                }
                if let Ok(mut node) = nodes.get_mut(entity) {
                    let display = if alert.is_some() { Display::Flex } else { Display::None };
                    if node.display != display {
                        node.display = display;
                    }
                }
            }
            Mark::KnobValue => {
                if let (Some(k), Some(v)) = (knob, knob_value) {
                    set_text(&mut texts, entity, &knob_text(k, v));
                }
            }
            Mark::KnobFill => {
                if let (Some(k), Some(v), Ok(mut node)) = (knob, knob_value, nodes.get_mut(entity)) {
                    let width = Val::Percent(fraction(k, v) * 100.0);
                    if node.width != width {
                        node.width = width;
                    }
                }
            }
            Mark::Verdict if due => {
                if let Some(f) = frame {
                    set_text(&mut texts, entity, &glyphs(&f.verdict));
                }
            }
            Mark::Value(row) if due => {
                let Some((_, value, unit)) = frame.and_then(|f| row_text(row, f)) else { continue };
                let mut found = Vec::new();
                text_entities(entity, &children, &texts, &mut found);
                // The key is the first text of a property row, the value the second.
                if let Some(target) = found.get(1).copied() {
                    set_text(&mut texts, target, &shown_value(&value, &unit));
                }
            }
            Mark::ChartTitle => {
                let label = frame.map_or_else(|| "Chart".to_string(), |f| format!("{}  ·  sampled every 1/30 s of advancing real time, last 1800 samples", glyphs(f.signal.0)));
                set_text(&mut texts, entity, &label);
            }
            Mark::Verdict | Mark::Value(_) | Mark::ChartLabel(_) => {}
        }
    }
    for (row, mut tint, mut border) in &mut rows {
        let selected = frame.is_some_and(|f| f.current == row.0);
        tint.set_if_neq(Tint::selectable(selected));
        border.set_if_neq(BorderColor::all(if selected { ACCENT } else { Color::NONE }));
    }
    if due {
        let controls = controls(&gallery);
        for (id, mut enabled, kids) in &mut buttons {
            let Some(control) = controls.iter().find(|c| c.id == id.0) else { continue };
            let on = control.ready.is_ok();
            if enabled.0 != on {
                enabled.0 = on;
            }
            // Pause reads "Run" while paused.
            if id.0 == "phenomena:pause" {
                if let Some(label) = kids.and_then(|k| k.iter().next()) {
                    set_text(&mut texts, label, &control.label);
                }
            }
        }
    }
}

/// Present: the strip chart, redrawn when a sample was added (at most every
/// 0.1 s) or at once when the chart restarted, with its y range and window
/// as corner labels.
pub(super) fn chart(time: Res<Time>, gallery: Option<Res<Gallery>>, panels: Option<ResMut<Panels>>, mut images: ResMut<Assets<Image>>, labels: Query<(&Mark, &Children)>, mut texts: Query<&mut Text>) {
    let (Some(gallery), Some(mut panels)) = (gallery, panels) else { return };
    let Some(f) = gallery.ready() else { return };
    let key = (f.generation, f.chart_count);
    if panels.chart_key == Some(key) {
        return;
    }
    let now = time.elapsed_secs_f64();
    let restarted = panels.chart_key.is_none_or(|(generation, _)| generation != f.generation);
    if !restarted && now - panels.chart_at < CHART_REFRESH {
        return;
    }
    panels.chart_key = Some(key);
    panels.chart_at = now;
    let points: Vec<[f64; 2]> = f.chart.iter().enumerate().map(|(i, v)| [i as f64 * CHART_INTERVAL, *v]).collect();
    let (pixels, range, _) = crate::chart::rasterize_span(&[(&points, crate::chart::COLORS[1])], Some(60.0));
    if let Some(mut image) = images.get_mut(&panels.chart) {
        image.data = Some(pixels);
    }
    let drawn = points.len() >= 2;
    for (mark, kids) in &labels {
        let Mark::ChartLabel(axis) = *mark else { continue };
        let value = match axis {
            _ if !drawn => String::new(),
            Axis::Top => steady(range.1),
            Axis::Bottom => steady(range.0),
            Axis::Window => format!("{:.0} s", points.len() as f64 * CHART_INTERVAL),
        };
        for kid in kids.iter() {
            set_text(&mut texts, kid, &value);
        }
    }
}
