//! The "Real motor sync" section (hardware-sync.mjs:6-13) and its frame
//! systems.
//!
//! - `sync_frames` (SimSync, after `robot::apply_frames`): connects once
//!   when the panel is first shown with a bench configured (the page
//!   connects on load), applies the sync link's results, calls
//!   `LiveSync::on_frame` for every newly accepted run frame (the page's
//!   `hardwareSync.onFrame()` after each live step) and applies the pause and
//!   reset stop rules.
//! - `sync_panel` (Present): fills [`SyncRoot`] in the page's order —
//!   heading, description, Leg, the mapping rows, Bench motion scale, Sync
//!   motors / Stop motors, the status line, readings, the note, then the
//!   banner and the per-joint charts ("Target & measured motion") — and keeps
//!   a banner strip over the 3D view while a bench is configured (the page
//!   prepends it to the workspace). Controls are rebuilt only when their
//!   state changes; texts are updated in place; charts are redrawn when new
//!   samples arrive.
use super::actions::HardwareAction;
use super::settings::SyncBinding;
use super::view::fixed;
use super::{Hardware, sync};
use crate::app::actions::Act;
use crate::app::{ViewerMode, ViewerSet};
use crate::robot::{RobotAction, RobotView};
use crate::ui_kit::{Corner, FAINT, Kit, Look, OK, RAISED, SUBTLE, TEXT, UiFonts, WARN, size, wrap};
use bevy::prelude::*;
use sim_runtime::hardware::protocol::bench;

/// The section's content node (the panel spawns it inside "Real motor sync").
#[derive(Component)]
pub(crate) struct SyncRoot;

#[derive(Component)]
struct SyncStatus;
#[derive(Component)]
struct SyncReadings;
#[derive(Component)]
struct SyncChartNote;
/// A banner's text (in the section and over the 3D view) and its strip.
#[derive(Component)]
struct SyncBannerText;
#[derive(Component)]
struct SyncBannerStrip;
/// The strip over the 3D view.
#[derive(Component)]
struct SyncOverlay;
/// The chart cards' container (rebuilt when samples arrive).
#[derive(Component)]
struct SyncCharts;

/// The page's trace colours: applied target (#79aaff) and real encoder (#71e3ba).
const TARGET_RGB: [u8; 3] = [0x79, 0xaa, 0xff];
const MEASURED_RGB: [u8; 3] = [0x71, 0xe3, 0xba];

pub(crate) fn build(app: &mut App) {
    app.add_systems(
        Update,
        (sync_frames.in_set(ViewerSet::SimSync).after(crate::robot::RobotSet::Frames), (sync_panel, sync_overlay, sync_texts).chain().in_set(ViewerSet::Present)).run_if(in_state(ViewerMode::Robot)),
    );
}

fn sync_frames(hw: Option<ResMut<Hardware>>, view: Option<Res<RobotView>>, mut acts: MessageWriter<Act<RobotAction>>, mut redraw: MessageWriter<bevy::window::RequestRedraw>, mut last: Local<Option<(u64, u64, Option<u64>)>>) {
    let (Some(mut hw), Some(view)) = (hw, view) else { return };
    let hw = &mut *hw;
    let open = hw.open;
    let s = &mut hw.sync;
    if open {
        s.auto_connect();
    }
    let mut out: Vec<RobotAction> = Vec::new();
    if s.poll() {
        out.push(RobotAction::Run { action: crate::robot::run::RunAction::Pause });
    }
    let key = view.run.as_ref().and_then(|r| r.frame().map(|f| (r.generation(), f.steps, f.completed_steps)));
    if key.is_some() && key != *last {
        s.on_frame(&view, &mut |a| out.push(a));
    }
    *last = key;
    s.watch_run(&view);
    if s.wants_frames() {
        redraw.write(bevy::window::RequestRedraw);
    }
    for action in out {
        acts.write(Act::quiet(action));
    }
}

/// What the controls and charts were last built from.
#[derive(Default)]
struct Built {
    controls: Option<(Entity, u64, bool, bool, bool, bool, bool)>,
    /// The charts' container and the (revision, samples revision) drawn in it.
    charts: Option<(Entity, u64, u64)>,
}

#[allow(clippy::too_many_arguments)]
fn sync_panel(mut commands: Commands, hw: Option<Res<Hardware>>, fonts: Res<UiFonts>, roots: Query<Entity, With<SyncRoot>>, mut images: ResMut<Assets<Image>>, mut handles: Local<Vec<Handle<Image>>>, mut built: Local<Built>) {
    let (Some(hw), Ok(root)) = (hw, roots.single()) else { return };
    let s = &hw.sync;
    let k = Kit::new(&fonts);
    let key = (root, s.revision, s.configured(), s.connected(), s.connecting(), s.start_enabled(), s.selects_enabled());
    if built.controls != Some(key) {
        built.controls = Some(key);
        built.charts = None;
        commands.entity(root).despawn_related::<Children>();
        commands.entity(root).with_children(|p| controls(p, &k, s));
        if s.configured() {
            // Appended after the controls: the charts' container, then its note.
            let grid = commands.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), flex_shrink: 0.0, ..default() }, SyncCharts)).id();
            let note = commands.spawn((k.text(chart_note_of(s), size::DETAIL, FAINT, 0), SyncChartNote)).id();
            commands.entity(root).add_children(&[grid, note]);
            built.charts = Some((grid, u64::MAX, u64::MAX));
        }
    }
    let Some((grid, revision, samples)) = built.charts else { return };
    if (revision, samples) == (s.revision, s.samples_revision) {
        return;
    }
    built.charts = Some((grid, s.revision, s.samples_revision));
    commands.entity(grid).despawn_related::<Children>();
    for (slot, c) in charts(s.samples(), s.rows()).iter().enumerate() {
        while handles.len() <= slot {
            handles.push(images.add(crate::chart::blank_image()));
        }
        let traces: Vec<(&[[f64; 2]], [u8; 3])> = if c.waiting { Vec::new() } else { vec![(c.target.as_slice(), TARGET_RGB), (c.actual.as_slice(), MEASURED_RGB)] };
        // The page's canvas axes: x 0…1 of the session, y exactly ±bound.
        let (pixels, _, _) = crate::chart::rasterize_fixed(&traces, Some((0.0, 1.0)), (-c.bound, c.bound));
        if let Some(mut image) = images.get_mut(&handles[slot]) {
            image.data = Some(pixels);
        }
        let card = commands.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() }).id();
        let title = commands.spawn(k.text(c.title.as_str(), size::CAPTION, TEXT, 1)).id();
        let stats = commands.spawn(k.text(c.stats.as_str(), size::DETAIL, SUBTLE, 0)).id();
        let plot = commands.spawn(k.chart_image(handles[slot].clone(), Node { width: Val::Percent(100.0), aspect_ratio: Some(crate::chart::RASTER.0 as f32 / crate::chart::RASTER.1 as f32), flex_shrink: 0.0, ..default() }, true)).id();
        if !c.waiting {
            let labels = [
                commands.spawn(k.chart_label(format!("±{}°", fixed(c.bound, 1)), Corner::TopLeft)).id(),
                commands.spawn(k.chart_label("0 s", Corner::BottomLeft)).id(),
                commands.spawn(k.chart_label("12 s", Corner::BottomRight)).id(),
            ];
            commands.entity(plot).add_children(&labels);
        }
        commands.entity(card).add_children(&[title, stats, plot]);
        commands.entity(grid).add_child(card);
    }
}

/// One per-joint chart (`draw`, :12).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Chart {
    pub title: String,
    pub stats: String,
    /// Applied target and real encoder: [x in 0..1 of the session, degrees].
    pub target: Vec<[f64; 2]>,
    pub actual: Vec<[f64; 2]>,
    /// The symmetric vertical bound (degrees).
    pub bound: f64,
    pub waiting: bool,
}

/// RMS error (degrees) and the share of reads at the drive limit.
pub(crate) fn rms_and_saturation(values: &[&bench::LiveSample]) -> (f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let n = values.len() as f64;
    let rms = (values.iter().map(|p| (p.measured_deg() - p.target_deg()).powi(2)).sum::<f64>() / n).sqrt();
    let sat = values.iter().filter(|p| p.pwm.abs() >= p.pwm_limit).count() as f64 / n;
    (rms, sat)
}

/// The per-joint charts, one per mapping row.
pub(crate) fn charts(samples: &[bench::LiveSample], rows: &[SyncBinding]) -> Vec<Chart> {
    rows.iter()
        .map(|b| {
            let title = format!("{} · ID {}", b.coordinate.replacen("joint.", "", 1), b.motor_id);
            let values: Vec<&bench::LiveSample> = samples.iter().filter(|s| s.id == b.motor_id).collect();
            if values.is_empty() {
                return Chart { title, stats: "Waiting for measured motion".into(), target: Vec::new(), actual: Vec::new(), bound: 0.5 * 1.2, waiting: true };
            }
            let (rms, sat) = rms_and_saturation(&values);
            let span = (values[0].total_frames.unwrap_or(120.0) - 1.0).max(1.0);
            let x = |p: &bench::LiveSample| p.frame / span;
            let actual: Vec<[f64; 2]> = values.iter().map(|p| [x(p), p.measured_deg()]).collect();
            let target: Vec<[f64; 2]> = values.iter().map(|p| [x(p), p.target_deg()]).collect();
            let bound = actual.iter().chain(&target).map(|p| p[1].abs()).fold(0.5, f64::max) * 1.2;
            Chart { title, stats: stats_text(rms, sat), target, actual, bound, waiting: false }
        })
        .collect()
}

/// A chart's stats line (:12): `rms.toFixed(2)` and `(sat*100).toFixed(0)`,
/// ties away from zero as `toFixed` (1 of 8 reads at the limit is "13%").
pub(crate) fn stats_text(rms: f64, sat: f64) -> String {
    format!("RMS error {}° · at drive limit {}%", fixed(rms, 2), fixed(sat * 100.0, 0))
}

/// The chart note (:13) for `retained` samples.
pub(crate) fn chart_note(retained: usize) -> String {
    format!("Angles relative to measured start. Errors use the previously applied target at each feedback read. {retained} real samples retained.")
}

/// The chart note shown: the page's initial one before `/config`, then [`chart_note`].
pub(crate) fn chart_note_of(s: &sync::LiveSync) -> String {
    if s.connected() { if s.simulated() { format!("Simulated host-loop feedback · no physical FPGA proof. {} simulated samples retained.",s.samples().len()) } else {chart_note(s.samples().len())} } else { sync::CHART_IDLE.into() }
}

/// A row's title: `c.split(' | ')[1].replace(' servo output','')`.
pub(crate) fn row_title(coordinate: &str) -> String {
    coordinate.split(" | ").nth(1).unwrap_or_default().replacen(" servo output", "", 1)
}

/// The section's controls in the page's order (:6).
fn controls(p: &mut ChildSpawnerCommands, k: &Kit, s: &sync::LiveSync) {
    p.spawn(k.text("Real motor sync", size::ITEM, TEXT, 2));
    p.spawn(k.caption(if s.simulated(){"Map three simulated bench motors to one leg. Live targets go to the in-process Bench host loop; no physical motion or FPGA proof."}else{sync::DESCRIPTION}));
    p.spawn(k.caption(s.identity_label()));
    if !s.configured() {
        p.spawn(k.caption(format!("No motor bench configured. {}", sync::NO_BENCH)));
        return;
    }
    let selects = s.selects_enabled();
    if s.connected() {
        p.spawn(wrap()).with_children(|r| {
            r.spawn(k.text("Leg", size::SMALL, SUBTLE, 0));
            r.spawn(k.segments()).with_children(|seg| {
                for leg in s.legs() {
                    seg.spawn(k.segment(leg, HardwareAction::SyncLeg { leg: leg.clone() }, s.leg() == leg, selects));
                }
            });
        });
        for (row, b) in s.rows().iter().enumerate() {
            let title = row_title(&b.coordinate);
            p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), margin: UiRect::vertical(Val::Px(4.0)), flex_shrink: 0.0, ..default() }).with_children(|col| {
                col.spawn(k.text(title, size::SMALL, TEXT, 1));
                col.spawn(wrap()).with_children(|r| {
                    r.spawn(k.segments()).with_children(|seg| {
                        for id in s.motor_ids() {
                            seg.spawn(k.segment(&format!("ID {id}"), HardwareAction::SyncMotor { row, motor_id: *id }, b.motor_id == *id, selects));
                        }
                    });
                    r.spawn(k.segments()).with_children(|seg| {
                        for (polarity, label) in [(1i8, "+"), (-1, "−")] {
                            seg.spawn(k.segment(label, HardwareAction::SyncPolarity { row, polarity }, b.polarity == polarity, selects));
                        }
                    });
                });
            });
        }
        p.spawn(wrap()).with_children(|r| {
            r.spawn(k.text("Bench motion scale", size::SMALL, SUBTLE, 0));
            r.spawn(k.segments()).with_children(|seg| {
                for (scale, label) in sync::SCALES {
                    seg.spawn(k.segment(label, HardwareAction::SyncScale { scale }, s.amplitude() == scale, selects));
                }
            });
        });
    }
    p.spawn(wrap()).with_children(|r| {
        if !s.connected() && !s.connecting() {
            r.spawn(k.button("Connect", HardwareAction::SyncConnect, Look::Secondary, true));
        }
        r.spawn(k.button("Inspect bench · zero drive",HardwareAction::SyncInspect,Look::Secondary,s.inspect_enabled()));
        r.spawn(k.button("Sync motors · 12 seconds", HardwareAction::SyncStart, Look::Primary, s.start_enabled()));
        r.spawn(k.button("Stop motors", HardwareAction::SyncStop, Look::Danger, true));
    });
    p.spawn((k.caption(s.status()), SyncStatus));
    p.spawn((k.mono(s.readings(), size::DETAIL, TEXT), SyncReadings));
    p.spawn(k.note(if s.simulated(){"Simulated host feedback with bounded targets, cancellation and leases; physical FPGA independence, watchdog deployment and stationary readback are not exercised."}else{sync::NOTE}));
    let (banner, ok) = s.banner();
    p.spawn((Node { padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)), border_radius: BorderRadius::all(Val::Px(4.0)), flex_shrink: 0.0, ..default() }, BackgroundColor(RAISED), SyncBannerStrip))
        .with_children(|b| {
            b.spawn((k.text(banner, size::SMALL, if ok { OK } else { WARN }, 2), SyncBannerText));
        });
    p.spawn(k.text("Target & measured motion", size::SMALL, TEXT, 2));
    p.spawn(wrap()).with_children(|r| {
        for (rgb, label) in [(TARGET_RGB, "Applied target"), (MEASURED_RGB, "Real encoder")] {
            let color = Color::srgb_u8(rgb[0], rgb[1], rgb[2]);
            r.spawn((Node { width: Val::Px(12.0), height: Val::Px(3.0), flex_shrink: 0.0, ..default() }, BackgroundColor(color)));
            r.spawn(k.text(label, size::DETAIL, SUBTLE, 0));
        }
    });
}

/// The banner over the 3D view while a bench is configured.
fn sync_overlay(mut commands: Commands, hw: Option<Res<Hardware>>, fonts: Res<UiFonts>, overlays: Query<Entity, With<SyncOverlay>>) {
    let show = hw.as_ref().is_some_and(|h| h.sync.configured());
    match (show, overlays.iter().next()) {
        (true, None) => {
            let k = Kit::new(&fonts);
            let (banner, ok) = hw.as_ref().map(|h| h.sync.banner()).unwrap_or(("", false));
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(crate::robot::TOP + 8.0),
                        left: Val::Px(crate::robot::LEFT + 8.0),
                        max_width: Val::Px(520.0),
                        padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(RAISED),
                    Pickable::IGNORE,
                    SyncOverlay,
                    SyncBannerStrip,
                ))
                .with_children(|b| {
                    b.spawn((k.text(banner, size::SMALL, if ok { OK } else { WARN }, 2), SyncBannerText));
                });
        }
        (false, Some(e)) => commands.entity(e).despawn(),
        _ => {}
    }
}

/// Status, readings, banners and the chart note, updated in place.
#[allow(clippy::type_complexity)]
fn sync_texts(
    hw: Option<Res<Hardware>>,
    mut texts: ParamSet<(
        Query<&mut Text, With<SyncStatus>>,
        Query<&mut Text, With<SyncReadings>>,
        Query<&mut Text, With<SyncChartNote>>,
        Query<(&mut Text, &mut TextColor), With<SyncBannerText>>,
    )>,
) {
    let Some(hw) = hw else { return };
    let s = &hw.sync;
    for mut t in &mut texts.p0() {
        set_text(&mut t, s.status());
    }
    for mut t in &mut texts.p1() {
        set_text(&mut t, s.readings());
    }
    let note = chart_note_of(s);
    for mut t in &mut texts.p2() {
        set_text(&mut t, &note);
    }
    let (banner, ok) = s.banner();
    let color = if ok { OK } else { WARN };
    for (mut t, mut c) in &mut texts.p3() {
        set_text(&mut t, banner);
        if c.0 != color {
            c.0 = color;
        }
    }
}

/// Writes a text only when it differs, so an unchanged one is not re-laid out.
fn set_text(t: &mut Mut<Text>, v: &str) {
    if t.0 != v {
        t.0 = v.to_string();
    }
}
