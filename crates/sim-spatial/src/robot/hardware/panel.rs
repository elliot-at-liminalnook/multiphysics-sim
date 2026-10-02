//! The Leg calibration panel's widgets: a right-side kit dock over robot
//! mode's inspector, shown while `Hardware::open`, in the page's order and
//! with its labels (web/viewer/calibration-ui.mjs :10-62; the sections are
//! spawned by [`super::panel_sections`]). The top bar (title, STOP, ×, the
//! connection line) stays pinned above the scroll area; every collapsible
//! section's header also has a compact Stop, so STOP is reachable from
//! every section (native-viewer.md §8).
//!
//! - **One source for controls.** [`control_list`] gives every panel button
//!   (its stable `system_ui` id `hardware:<name>`, label, action, whether it
//!   is enabled now and why not, and its on state) from the rendered
//!   [`PanelView`]. [`refresh`] copies it into the buttons (`Enabled`,
//!   `Look`, the action, the label) and [`controls`] hands it to robot
//!   mode's `system_ui`, so both always agree.
//! - **Refresh** (Present): the view is rendered every frame while the panel
//!   is open (pure string work); widgets are written only when the view, the
//!   controls, the form or the connection line changed. Dynamic lists (motor
//!   chips, the gait list, statistics, recent runs, the motion chart's
//!   labels) are rebuilt only when their content changes.
//! - **Native equivalents.** A disabled motor's chip reads "⊘ Knee 1" (the
//!   page strikes it through; the kit has no strike style). Checkboxes are
//!   kit chip toggles; the gait select is a list of kit rows; the PWM
//!   ceiling is a slider (0–100 in 0.1 steps) and the raw step a stepper of
//!   kit buttons (−100 … +100, ±); the statistics table is one line per
//!   motor ("RMS error 1.23° · Peak …").
use super::actions::{Direction, DriveMode, GaitMode, HardwareAction};
use super::view::{self, Form, PanelView};
use super::{Hardware, Section};
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::builder::ui_api::Enabled;
use crate::ui_kit::{DANGER, Kit, Look, OK, SWITCHER_STRIP, UiFonts, WARN, slider_held, wheel_delta};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;
use bevy::ui::{FocusPolicy, InteractionDisabled, Pressed};
use bevy::ui_widgets::SliderValue;
use bevy::window::PrimaryWindow;
use std::time::Instant;

/// The panel's width (the page's 390 px; robot mode's inspector column).
pub(super) const WIDTH: f32 = super::super::RIGHT;

/// A node shown or hidden by the refresh.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Shown {
    /// The panel (while `Hardware::open`).
    Root,
    /// A section's body (while open).
    Section(Section),
    /// The Connect button (no link, or a stale one, and not connecting).
    Connect,
    /// The motion chart (a session to plot).
    Chart,
}

/// A text the refresh rewrites (hidden while empty).
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum PanelText {
    Connection,
    Notice,
    Sequence,
    Warnings,
    Status,
    SpeedValue,
    /// Lower, upper, reference pose texts.
    Pose(usize),
    Capture,
    Learning,
    TuneStatus,
    CampaignStatus,
    GaitSpeedValue,
    GaitEffortValue,
    GaitStatus,
    Position,
    TargetLabel,
    TargetNote,
    PwmValue,
    RawStep,
    Telemetry,
    MotionDirection,
    MotionStatus,
    MotionPlaceholder(usize),
    ExportLine,
}

/// A panel button's `system_ui` id (without the `hardware:` prefix).
#[derive(Component, Clone, Debug)]
pub(super) struct ControlId(pub String);

/// The button's label follows its control's label (Disable/Enable, Play/Pause, …).
#[derive(Component)]
pub(super) struct FollowLabel;

/// A hold-to-move button: a press is `JogPress`, leaving the press `JogRelease`.
#[derive(Component)]
pub(crate) struct JogButton {
    pub direction: Direction,
    pub held: bool,
}

/// The panel's sliders.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PanelSlider {
    Speed,
    Target,
    GaitSpeed,
    GaitEffort,
    Pwm,
}

/// A slider's fill (its value as a width).
#[derive(Component)]
pub(super) struct SliderFill(pub PanelSlider);

/// A container rebuilt when its content changes.
#[derive(Component)]
pub(super) struct PanelList {
    pub kind: ListKind,
    pub key: Option<String>,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ListKind {
    Chips,
    Gaits,
    Stats,
    Runs,
    MotionLabels,
}

#[derive(Component)]
pub(super) struct DialImage;
#[derive(Component)]
pub(super) struct MotionImage;
/// The scroll area under the top bar.
#[derive(Component)]
pub(super) struct PanelScroll;

/// One panel control, as the panel shows it and `system_ui` lists it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Control {
    pub id: String,
    pub label: String,
    pub action: HardwareAction,
    pub ready: Result<(), String>,
    /// A toggle's state (lit chip, chosen segment, held jog button, open section).
    pub on: Option<bool>,
}

/// The page's section titles (`<summary>`).
pub(super) fn section_title(s: Section) -> &'static str {
    match s {
        Section::Tune => "Tune this motor",
        Section::Campaign => "Characterization campaign",
        Section::Gait => "Gait playback",
        Section::GaitRuns => "Recent leg runs",
        Section::Mirror => "Simulated leg mirror",
        Section::Advanced => "Advanced settings & feedback",
        Section::Sync => "Real motor sync",
    }
}
/// A section header's label: its title after an open/closed marker.
pub(super) fn section_label(s: Section, open: bool) -> String {
    format!("{} {}", if open { "▾" } else { "▸" }, section_title(s))
}
pub(super) fn section_id(s: Section) -> &'static str {
    match s {
        Section::Tune => "section_tune",
        Section::Campaign => "section_campaign",
        Section::Gait => "section_gait",
        Section::GaitRuns => "section_gait_runs",
        Section::Mirror => "section_mirror",
        Section::Advanced => "section_advanced",
        Section::Sync => "section_sync",
    }
}

pub(super) const TUNE_OK: &str = "The motor is mid-travel with room to move both ways";
pub(super) const CAMPAIGN_OK: &str = "The leg is suspended with clear space around every joint";
pub(super) const GAIT_OK: &str = "The leg is suspended with clear space around every joint (needed for Leg and Both)";
pub(super) const HOLD_OTHERS: &str = "Hold the other enabled motors in place while one moves";
/// Why a command the virtual bench does not simulate (flip, raw step) is
/// disabled on a virtual calibration link. Gait playback on the leg (Leg
/// only, Both) is in scope: the virtual bench runs `gait_start` and its
/// lease, and its results are labelled simulated (`view::render_gait`).
pub(crate) const OUT_OF_VIRTUAL_SCOPE: &str = "Outside virtual calibration scope: the virtual bench does not simulate this command";
pub(super) const NOT_CONNECTED: &str = "the Leg calibration panel is not connected to the calibration server (Connect first)";
/// The raw step stepper: (id suffix, label, change).
pub(super) const STEPS: [(&str, &str, i32); 6] = [("minus_100", "−100", -100), ("minus_10", "−10", -10), ("minus_1", "−1", -1), ("plus_1", "+1", 1), ("plus_10", "+10", 10), ("plus_100", "+100", 100)];

pub(super) fn gait_mode_label(mode: GaitMode) -> &'static str {
    match mode {
        GaitMode::Sim => "Sim only",
        GaitMode::Leg => "Leg only",
        GaitMode::Both => "Both",
    }
}
fn gait_mode_name(mode: GaitMode) -> &'static str {
    match mode {
        GaitMode::Sim => "sim",
        GaitMode::Leg => "leg",
        GaitMode::Both => "both",
    }
}

/// The rendered view with the panel's own states: no link (motion blocked,
/// as for a stale status) and the mirror's gait notice (a gait load or
/// sample error, which the page writes into the gait status line).
pub(crate) fn panel_view(hw: &Hardware, now: Instant) -> PanelView {
    let mut v = view::render(&hw.snapshot, &hw.form, now);
    if hw.link.is_none() {
        v.block(if hw.connecting.is_some() { format!("Connecting to {}…", hw.url()) } else { "Not connected to the calibration server.".into() });
    }
    if hw.snapshot.authorization_revoked || (hw.link.is_some() && !hw.snapshot.connection_valid) {
        v.block("Connection or execution identity lost; reconnect required before calibration automation.".into());
    }
    // A block replaces the status line: the disconnection is said again
    // (`view::render` said it first; idempotent). Only a lost binding blocks:
    // a physical bus lost on a STOP readback is reconnected by selecting a motor.
    if hw.link.is_some() {
        view::mark_disconnected(&mut v, &hw.snapshot, now);
    }
    if hw.snapshot.execution.as_ref().is_some_and(|i| i.is_virtual_calibration()) {
        v.status = format!("VIRTUAL · simulated bench telemetry and results, not physical measurements. {}", v.status);
        v.tune_status = format!("VIRTUAL · {}", v.tune_status);
        if !hw.snapshot.tune_stages.is_empty() { v.tune_status += &format!("\nCaptured stages: {}", hw.snapshot.tune_stages.join(" → ")); }
        v.campaign_status = format!("VIRTUAL · {}", v.campaign_status);
    }
    if let Some(notice) = hw.mirror.gait_notice() {
        v.gait.status = notice.to_string();
    }
    v
}

/// The link's execution is a virtual calibration bench (its results are simulated).
pub(crate) fn is_virtual(hw: &Hardware) -> bool {
    hw.snapshot.execution.as_ref().is_some_and(|i| i.is_virtual_calibration())
}

/// Every panel control in the page's order, with its state now.
pub(crate) fn control_list(hw: &Hardware, v: &PanelView) -> Vec<Control> {
    let f = &hw.form;
    let mut out = Vec::new();
    let mut add = |id: String, label: String, action: HardwareAction, ready: Result<(), String>, on: Option<bool>| out.push(Control { id, label, action, ready, on });
    // Motion: the block (stale, offline) is the first reason.
    let why_m = |enabled: bool, reason: &str| if enabled { Ok(()) } else { Err(v.blocked.clone().unwrap_or_else(|| reason.to_string())) };
    let why = |enabled: bool, reason: &str| if enabled { Ok(()) } else { Err(reason.to_string()) };
    let s = |x: &str| x.to_string();
    // Commands the virtual bench does not simulate (its server answers them
    // with an ordinary refusal): disabled, for the operator too.
    let virtual_bench = is_virtual(hw);
    let in_scope = |enabled: bool, reason: &str| if virtual_bench { Err(OUT_OF_VIRTUAL_SCOPE.to_string()) } else { why_m(enabled, reason) };
    add(s("toggle_panel"), s("Leg calibration"), HardwareAction::TogglePanel, Ok(()), Some(hw.open));
    add(s("close"), s("Close calibration"), HardwareAction::ClosePanel, Ok(()), None);
    add(s("stop"), s("Stop"), HardwareAction::Stop, Ok(()), None);
    add(s("connect"), s(if hw.link.is_some() { "Reconnect" } else { "Connect" }), HardwareAction::Connect, why(hw.connecting.is_none(), "already connecting"), None);
    for chip in &v.chips {
        add(format!("select_{}", chip.id), chip.label.clone(), HardwareAction::Select { id: chip.id }, why_m(chip.enabled, "a motor is being connected and checked"), Some(chip.pressed));
    }
    add(s("set_disabled"), v.disable.text.clone(), HardwareAction::SetDisabled, why_m(v.disable.enabled, "choose a motor first (not while one is connecting or sweep-all runs)"), None);
    add(s("sweep_all"), v.sweep_all.text.clone(), HardwareAction::SweepAll, why_m(v.sweep_all.enabled, "a motor is being connected and checked"), None);
    add(s("hold_others"), s(HOLD_OTHERS), HardwareAction::HoldOthers { on: !f.inputs.hold_others }, Ok(()), Some(f.inputs.hold_others));
    // Hold-to-move as a toggle for activation: while the form holds a press
    // (the operator's, or an accepted remote one) the control is its release,
    // which is never refused (`HardwareAction::authorize`); the window's
    // button pairs press and release itself (`actions::input::jog_buttons`)
    // and keeps its own label.
    for (name, label, release, direction, held, on) in [
        ("jog_upper", "Q  Upper ↑", "Release ↑ (hold)", Direction::Upper, f.held_upper, v.held_upper),
        ("jog_lower", "A  Lower ↓", "Release ↓ (hold)", Direction::Lower, f.held_lower, v.held_lower),
    ] {
        if held {
            add(s(name), s(release), HardwareAction::JogRelease { direction }, Ok(()), Some(on));
        } else {
            add(s(name), s(label), HardwareAction::JogPress { direction }, why_m(v.jog_enabled, "no motor is ready: select one first"), Some(on));
        }
    }
    use super::actions::Boundary;
    for (i, (id, label, boundary)) in [("capture_lower", "Save lower here", Boundary::Lower), ("capture_upper", "Save upper here", Boundary::Upper), ("capture_reference", "Save sim alignment here", Boundary::Reference)].into_iter().enumerate() {
        let ready = why_m(v.poses[i].1, "save a pose while a ready motor holds still");
        add(s(id), s(label), HardwareAction::Capture { boundary }, ready, None);
    }
    add(s("sweep"), v.sweep.text.clone(), HardwareAction::Sweep, why_m(v.sweep.enabled, "needs a ready motor with both poses taught in this encoder session"), None);
    add(s("reset_poses"), v.reset.text.clone(), HardwareAction::ResetPoses, why_m(v.reset.enabled, "no motor is ready: select one first"), None);
    add(s("learn"), v.learn.text.clone(), HardwareAction::Learn, why_m(v.learn.enabled, "needs a ready motor with both poses taught in this encoder session"), None);
    for section in Section::ALL {
        let open = f.open.contains(&section);
        add(s(section_id(section)), section_label(section, open), HardwareAction::ToggleSection { section }, Ok(()), Some(open));
    }
    add(s("tune_confirm"), s(TUNE_OK), HardwareAction::TuneConfirm { on: !f.tune_ok }, Ok(()), Some(f.tune_ok));
    add(s("tune"), v.tune.text.clone(), HardwareAction::Tune, why_m(v.tune.enabled, "needs a ready motor, no tune or sweep-all running, and the confirmation checked"), None);
    add(s("campaign_confirm"), s(CAMPAIGN_OK), HardwareAction::CampaignConfirm { on: !f.campaign_ok }, Ok(()), Some(f.campaign_ok));
    let campaign = "needs a chosen motor, no tune, campaign or sweep-all running, and the confirmation checked";
    add(s("campaign"), v.campaign.text.clone(), HardwareAction::Campaign { resume: false }, why_m(v.campaign.enabled, campaign), None);
    add(s("campaign_resume"), s("Resume"), HardwareAction::Campaign { resume: true }, why_m(v.campaign.enabled, campaign), None);
    for (index, label) in v.gait.options.iter().enumerate() {
        add(format!("gait_select_{index}"), label.clone(), HardwareAction::GaitSelect { index }, Ok(()), Some(index == f.gait_index));
    }
    // Leg only and Both are in scope on a virtual bench too (the normal
    // readiness rules apply: the confirmation, no tune or campaign, a fresh status).
    for mode in [GaitMode::Sim, GaitMode::Leg, GaitMode::Both] {
        add(format!("gait_mode_{}", gait_mode_name(mode)), s(gait_mode_label(mode)), HardwareAction::GaitMode { mode }, why(v.gait.modes_enabled, "not while a gait plays"), Some(f.gait_mode == mode));
    }
    add(s("gait_confirm"), s(GAIT_OK), HardwareAction::GaitConfirm { on: !f.gait_ok }, Ok(()), Some(f.gait_ok));
    let play_reason = "needs a gait (and for Leg or Both, the confirmation), and no tune or campaign running";
    add(s("gait_play"), v.gait.play.text.clone(), HardwareAction::GaitPlay, why_m(v.gait.play.enabled, play_reason), None);
    add(s("gait_stop"), s("Stop"), HardwareAction::GaitStop, why(v.gait.stop_enabled, "no gait is playing"), None);
    for mode in DriveMode::ALL {
        add(format!("drive_mode_{}", mode.wire()), s(mode.label()), HardwareAction::DriveMode { mode }, Ok(()), Some(f.inputs.drive_mode == mode));
    }
    add(s("flip"), s("Swap upper / lower direction"), HardwareAction::Flip, in_scope(v.flip_enabled, "choose a motor first"), None);
    add(s("clear_lower"), s("Reset lower only"), HardwareAction::ClearLower, why_m(v.clear_enabled, "choose a motor first"), None);
    add(s("clear_upper"), s("Reset upper only"), HardwareAction::ClearUpper, why_m(v.clear_enabled, "choose a motor first"), None);
    for (name, label, change) in STEPS {
        add(format!("raw_step_{name}"), s(label), HardwareAction::RawStepValue { delta: (f.step + change).clamp(-4095, 4095) }, Ok(()), None);
    }
    add(s("raw_step_negate"), s("±"), HardwareAction::RawStepValue { delta: -f.step }, Ok(()), None);
    add(s("raw_step"), s("Send raw step"), HardwareAction::RawStep, in_scope(v.raw_step_enabled, "choose a motor and a step other than 0"), None);
    let export = if hw.link.is_none() {
        Err(NOT_CONNECTED.to_string())
    } else if hw.export.is_some() {
        Err("a download is already being written".to_string())
    } else {
        Ok(())
    };
    add(s("export"), s("Download calibration"), HardwareAction::Export, export, None);
    let mirror = hw.mirror.settings.enabled;
    add(s("mirror_on"), s("Show the real leg on the suspended simulated robot"), HardwareAction::MirrorEnabled { on: true }, Ok(()), Some(mirror));
    add(s("mirror_off"), s("Stop showing the real leg on the simulated robot"), HardwareAction::MirrorEnabled { on: false }, Ok(()), Some(!mirror));
    add(s("sync_connect"), s("Connect to the motor bench"), HardwareAction::SyncConnect, why(!hw.sync.connecting(), "already connecting to the motor bench"), None);
    let sync_start = if !hw.sync.connected() { Err("connect to the motor bench first".to_string()) } else { why(!hw.sync.active(), "live sync is already running") };
    add(s("sync_start"), s("Sync motors · 12 seconds"), HardwareAction::SyncStart, sync_start, None);
    add(s("sync_stop"), s("Stop motors"), HardwareAction::SyncStop, Ok(()), None);
    out
}

/// `system_ui`: every panel control as (id `hardware:<name>`, label, action,
/// Ok if enabled now else why not). Robot mode lists them after its own and
/// refuses the ones that start motion.
pub(crate) fn controls(hw: &Hardware) -> Vec<(String, String, HardwareAction, Result<(), String>)> {
    let v = panel_view(hw, Instant::now());
    control_list(hw, &v).into_iter().map(|c| (format!("hardware:{}", c.id), c.label, c.action, c.ready)).collect()
}

pub(crate) fn build(app: &mut App) {
    app.add_systems(OnEnter(ModeScope::Robot), spawn).add_systems(Update, (scroll, refresh).chain().in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Robot)));
}

/// OnEnter(Robot): the panel, hidden until `Hardware::open`.
fn spawn(mut commands: Commands, fonts: Res<UiFonts>, mut images: ResMut<Assets<Image>>) {
    let k = Kit::new(&fonts);
    let mut dial = super::dial::blank_image();
    dial.data = Some(super::dial::rasterize(0.5, 0.5));
    let dial = images.add(dial);
    let motion = images.add(crate::chart::blank_image());
    commands
        .spawn((
            k.dock(crate::ui_kit::Dock::Right { top: super::super::TOP, bottom: 0.0, width: WIDTH }, Node { flex_direction: FlexDirection::Column, display: Display::None, ..default() }),
            // Over robot mode's docks (its header controls are ZIndex(1)).
            ZIndex(2),
            // A `Node` defaults to `FocusPolicy::Pass`: without Block a click on
            // the panel's empty areas would reach robot mode's inspector under
            // it (`ui_focus_system` stops at the first Block node; the top bar
            // and every widget are children, so they are above it in the stack).
            FocusPolicy::Block,
            // The page's `aria-label` (calibration-ui.mjs:10).
            AccessibleLabel::new("Leg calibration"),
            Shown::Root,
        ))
        .with_children(|root| {
            super::panel_sections::top_bar(root, &k);
            root.spawn((k.scroll_area(Node { flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, ..default() }, 0.0), PanelScroll)).with_children(|area| {
                area.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), padding: UiRect::all(Val::Px(14.0)), flex_shrink: 0.0, ..default() }).with_children(|body| super::panel_sections::body(body, &k, dial, motion));
            });
        });
}

/// The wheel over the open panel scrolls it (robot.rs `scroll` leaves the
/// inspector underneath alone while the panel covers it).
fn scroll(hw: Option<Res<Hardware>>, mut wheel: MessageReader<MouseWheel>, windows: Query<&Window, With<PrimaryWindow>>, mut areas: Query<&mut ScrollPosition, With<PanelScroll>>) {
    let delta = wheel_delta(&mut wheel, 24.0);
    if delta == 0.0 || !hw.is_some_and(|h| h.open) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    if window.cursor_position().is_some_and(|p| p.x >= window.width() - WIDTH && p.y > super::super::TOP && p.y < window.height() - SWITCHER_STRIP) {
        for mut position in &mut areas {
            position.0.y = (position.0.y - delta).max(0.0);
        }
    }
}

/// What the refresh last wrote.
#[derive(Default)]
struct Drawn {
    root: Option<Entity>,
    key: Option<(PanelView, Vec<Control>, Form, [String; 3])>,
    needles: Option<(f64, f64)>,
    chart: Option<Option<super::motion_view::MotionChart>>,
}

/// A slider's value (0..=1) from the form, or the view for the target.
fn slider_fraction(which: PanelSlider, f: &Form, v: &PanelView) -> Option<f32> {
    let x = match which {
        PanelSlider::Speed => f.inputs.speed_percent / 100.0,
        PanelSlider::Target => v.target_value.unwrap_or(f.target_percent) / 100.0,
        PanelSlider::GaitSpeed => (f.inputs.gait_speed_percent - 5.0) / 95.0,
        PanelSlider::GaitEffort => (f.inputs.gait_effort_percent - 10.0) / 90.0,
        PanelSlider::Pwm => f.inputs.pwm_percent / 100.0,
    };
    x.is_finite().then(|| x.clamp(0.0, 1.0) as f32)
}

fn text_of(t: PanelText, v: &PanelView, hw: &Hardware, connection: &str) -> String {
    let f = &hw.form;
    match t {
        PanelText::Connection => connection.to_string(),
        PanelText::Notice => hw.notice.clone().unwrap_or_default(),
        PanelText::Sequence => v.sequence.clone(),
        PanelText::Warnings => v.warnings.clone(),
        PanelText::Status => v.status.clone(),
        PanelText::SpeedValue => v.speed_text.clone(),
        PanelText::Pose(i) => v.poses.get(i).map(|p| p.0.clone()).unwrap_or_default(),
        PanelText::Capture => v.capture.clone(),
        PanelText::Learning => v.learning.clone(),
        PanelText::TuneStatus => v.tune_status.clone(),
        PanelText::CampaignStatus => v.campaign_status.clone(),
        PanelText::GaitSpeedValue => v.gait.speed_text.clone(),
        PanelText::GaitEffortValue => v.gait.effort_text.clone(),
        PanelText::GaitStatus => v.gait.status.clone(),
        PanelText::Position => v.position.clone(),
        PanelText::TargetLabel => v.target_label.clone(),
        PanelText::TargetNote => if hw.snapshot.execution.as_ref().is_some_and(|i| i.is_virtual_calibration()) {
            "VIRTUAL: green = simulated encoder · blue = requested. Results are simulated, not physical measurements. Z disables torque; release holds. Losing focus stops drive.".into()
        } else { super::panel_sections::TARGET_NOTE.into() },
        PanelText::PwmValue => format!("{}%", view::fixed(f.inputs.pwm_percent, 1)),
        PanelText::RawStep => f.step.to_string(),
        PanelText::Telemetry => v.telemetry.clone(),
        PanelText::MotionDirection => v.motion.direction.clone(),
        PanelText::MotionStatus => v.motion.status.clone(),
        PanelText::MotionPlaceholder(i) => v.motion.placeholder.as_ref().and_then(|p| p.get(i).cloned()).unwrap_or_default(),
        PanelText::ExportLine => hw.export_line.clone().unwrap_or_default(),
    }
}

/// The connection line: the link (url · link generation) and its state.
fn connection_line(hw: &Hardware, v: &PanelView) -> String {
    match (&hw.link, &hw.connecting) {
        (_, Some(_)) => format!("Connecting to {}…", hw.url()),
        (Some(link), None) => format!("{} · link {} · {}{}", hw.url(), link.generation,
            if hw.snapshot.execution.as_ref().is_some_and(|i| i.is_virtual_calibration()) { "VIRTUAL simulated bench" } else { "physical or unknown execution" },
            if v.blocked.is_some() { " · stale/reconnect required" } else { "" }),
        (None, None) => format!("Not connected · {}", hw.url()),
    }
}

/// Present: copy the rendered view into the widgets (see the module doc).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn refresh(
    mut commands: Commands,
    hw: Option<Res<Hardware>>,
    fonts: Res<UiFonts>,
    mut images: ResMut<Assets<Image>>,
    mut drawn: Local<Drawn>,
    mut shown: Query<(Entity, &Shown, &mut Node), Without<PanelText>>,
    mut texts: Query<(&PanelText, &mut Text, &mut TextColor, &mut Node), Without<Shown>>,
    mut buttons: Query<(&ControlId, &mut Enabled, &mut Look, &mut HardwareAction, Option<&Children>, Has<FollowLabel>)>,
    mut labels: Query<&mut Text, (Without<PanelText>, Without<ControlId>)>,
    mut lists: Query<(Entity, &mut PanelList)>,
    sliders: Query<(Entity, &PanelSlider, &SliderValue, Has<Pressed>, &Interaction, Has<InteractionDisabled>)>,
    mut fills: Query<(&SliderFill, &mut Node), (Without<Shown>, Without<PanelText>)>,
    pictures: Query<(&ImageNode, Has<DialImage>), Or<(With<DialImage>, With<MotionImage>)>>,
) {
    let Some(hw) = hw else { return };
    let mut root = None;
    for (entity, shown, mut node) in &mut shown {
        if *shown == Shown::Root {
            root = Some(entity);
            let display = if hw.open { Display::Flex } else { Display::None };
            if node.display != display {
                node.display = display;
            }
        }
    }
    if root != drawn.root {
        *drawn = Drawn { root, ..default() };
    }
    if !hw.open || root.is_none() {
        return;
    }
    let now = Instant::now();
    let v = panel_view(&hw, now);
    let f = &hw.form;
    // Sliders follow the form (the target follows the server) unless held.
    let mut values = Vec::new();
    for (entity, which, value, pressed, interaction, disabled) in &sliders {
        // `SliderValue` is an immutable component: replace it, don't mutate it.
        let mut shown = value.0;
        if !slider_held(pressed, interaction)
            && let Some(x) = slider_fraction(*which, f, &v)
            && shown != x
        {
            commands.entity(entity).insert(SliderValue(x));
            shown = x;
        }
        values.push((*which, shown));
        let off = match which {
            PanelSlider::Target => !v.target_enabled,
            PanelSlider::GaitEffort => !v.gait.effort_enabled,
            _ => false,
        };
        if off && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        } else if !off && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
    for (fill, mut node) in &mut fills {
        if let Some((_, x)) = values.iter().find(|(w, _)| *w == fill.0) {
            let width = Val::Percent(x * 100.0);
            if node.width != width {
                node.width = width;
            }
        }
    }
    let connection = connection_line(&hw, &v);
    let controls = control_list(&hw, &v);
    let key = (v.clone(), controls.clone(), f.clone(), [connection.clone(), hw.notice.clone().unwrap_or_default(), hw.export_line.clone().unwrap_or_default()]);
    if drawn.key.as_ref() == Some(&key) {
        return;
    }
    drawn.key = Some(key);
    for (_, shown, mut node) in &mut shown {
        let on = match shown {
            Shown::Root => continue,
            Shown::Section(s) => f.open.contains(s),
            Shown::Connect => hw.connecting.is_none() && (hw.link.is_none() || v.blocked.is_some()),
            Shown::Chart => v.motion.chart.is_some(),
        };
        let display = if on { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for (which, mut text, mut color, mut node) in &mut texts {
        let value = text_of(*which, &v, &hw, &connection);
        let display = if value.is_empty() { Display::None } else { Display::Flex };
        if node.display != display {
            node.display = display;
        }
        if text.0 != value {
            text.0 = value;
        }
        if *which == PanelText::MotionStatus {
            color.set_if_neq(TextColor(if v.motion.mismatch { DANGER } else { OK }));
        }
        if *which == PanelText::Notice || *which == PanelText::Warnings {
            color.set_if_neq(TextColor(WARN));
        }
    }
    for (id, mut enabled, mut look, mut action, children, follow) in &mut buttons {
        let Some(c) = controls.iter().find(|c| c.id == id.0) else { continue };
        enabled.set_if_neq(Enabled(c.ready.is_ok()));
        if let Some(on) = c.on {
            let restyled = match *look {
                Look::Chip(_) => Look::Chip(on),
                Look::Segment(_) => Look::Segment(on),
                Look::Primary | Look::Secondary => {
                    if on {
                        Look::Primary
                    } else {
                        Look::Secondary
                    }
                }
                other => other,
            };
            look.set_if_neq(restyled);
        }
        if *action != c.action {
            *action = c.action.clone();
        }
        if follow && let Some(children) = children {
            for child in children.iter() {
                if let Ok(mut text) = labels.get_mut(child)
                    && text.0 != c.label
                {
                    text.0 = c.label.clone();
                }
            }
        }
    }
    // The motion chart: redrawn when its traces change; its labels are a list.
    let chart = v.motion.chart.clone();
    let mut chart_labels = None;
    if drawn.chart.as_ref() != Some(&chart) {
        if let Some(c) = chart.as_ref() {
            // The page's axes (fixed, so one sample draws and its labels
            // match what is drawn).
            let traces = [(c.requested.as_slice(), crate::chart::COLORS[2]), (c.measured.as_slice(), crate::chart::COLORS[0])];
            let (pixels, ..) = crate::chart::rasterize_fixed(&traces, Some(c.x_range()), c.y_range());
            if let Some((image, _)) = pictures.iter().find(|(_, dial)| !*dial)
                && let Some(mut image) = images.get_mut(&image.image)
            {
                image.data = Some(pixels);
            }
            chart_labels = Some(c.labels());
        }
        drawn.chart = Some(chart);
    }
    let k = Kit::new(&fonts);
    for (entity, mut list) in &mut lists {
        let key = match list.kind {
            ListKind::Chips => format!("{:?}", v.chips.iter().map(|c| (c.id, &c.label)).collect::<Vec<_>>()),
            // Selection is a chip look the controls update; only the list rebuilds.
            ListKind::Gaits => format!("{:?}", v.gait.options),
            ListKind::Stats => format!("{:?}", v.gait.stats),
            ListKind::Runs => format!("{:?}", v.gait.runs),
            ListKind::MotionLabels => match &chart_labels {
                Some(l) => format!("{l:?}"),
                None => continue,
            },
        };
        if list.key.as_ref() == Some(&key) {
            continue;
        }
        list.key = Some(key);
        let kind = list.kind;
        commands.entity(entity).despawn_related::<Children>();
        commands.entity(entity).with_children(|p| match kind {
            ListKind::Chips => super::panel_sections::chips(p, &k, &v),
            ListKind::Gaits => super::panel_sections::gaits(p, &k, &v, f.gait_index),
            ListKind::Stats => super::panel_sections::stats(p, &k, &v.gait.stats),
            ListKind::Runs => super::panel_sections::runs(p, &k, &v.gait.runs),
            ListKind::MotionLabels => {
                if let Some(l) = &chart_labels {
                    super::panel_sections::chart_labels(p, &k, l);
                }
            }
        });
    }
    if let Some(n) = v.needles
        && drawn.needles != Some(n)
    {
        drawn.needles = Some(n);
        if let Some((image, _)) = pictures.iter().find(|(_, dial)| *dial)
            && let Some(mut image) = images.get_mut(&image.image)
        {
            image.data = Some(super::dial::rasterize(n.0, n.1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every listed control has a unique single-segment id that fits the
    /// registered pattern; without a link every motion control is disabled
    /// with the offline reason, and STOP is enabled.
    #[test]
    fn controls_have_stable_ids_and_stop_is_always_enabled() {
        let hw = Hardware::new(super::super::HardwareConfig::default(), super::super::settings::Settings::default());
        let listed = controls(&hw);
        let mut ids = std::collections::BTreeSet::new();
        for (id, label, action, ready) in &listed {
            assert!(crate::app::actions::control_matches("hardware:<name>", id), "{id} does not fit hardware:<name>");
            assert!(ids.insert(id.clone()), "{id} is listed twice");
            assert!(!label.is_empty(), "{id} has no label");
            // The controls that act through the link (form-only settings such as the
            // confirmations stay editable offline, though REST may not change them).
            use HardwareAction as H;
            let through_link = matches!(
                action,
                H::Select { .. } | H::SweepAll | H::JogPress { .. } | H::JogRelease { .. } | H::Speed { .. } | H::Target { .. } | H::TargetCommit | H::Capture { .. } | H::ResetPoses
                    | H::ClearLower | H::ClearUpper | H::Sweep | H::Learn | H::Tune | H::Campaign { .. } | H::GaitPlay | H::PwmCeiling { .. } | H::Flip | H::RawStep
            );
            if through_link {
                assert!(ready.is_err(), "{id} is enabled without a link");
            }
        }
        for id in ["hardware:stop", "hardware:select_1", "hardware:select_3", "hardware:export", "hardware:connect", "hardware:jog_upper", "hardware:tune", "hardware:gait_play", "hardware:mirror_on", "hardware:sync_start", "hardware:section_gait_runs", "hardware:raw_step_negate"] {
            assert!(ids.contains(id), "{id} is not listed");
        }
        let stop = listed.iter().find(|(id, ..)| id == "hardware:stop").unwrap();
        assert_eq!(stop.3, Ok(()));
        // The panel's header shows the offline reason as its status.
        assert_eq!(panel_view(&hw, Instant::now()).status, "Not connected to the calibration server.");
    }
}
