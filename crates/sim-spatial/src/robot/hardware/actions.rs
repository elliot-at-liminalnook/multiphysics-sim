//! The panel's one action type, [`HardwareAction`]: every intent of the
//! browser's calibration page, mirror and live sync, written by the panel's
//! buttons, keys, focus loss, `system_ui` (`hardware:<name>` controls, listed
//! and activated through robot mode's `system_ui`) and REST
//! (`hardware_status`, `hardware_stop`, `hardware_export`, `hardware_gaits`,
//! `hardware {action}`), and applied by [`apply`] in `ViewerSet::Actions`.
//!
//! **Refusal rule.** An action that can start or change motion
//! ([`HardwareAction::starts_motion`]) is refused when its origin is
//! `Origin::Rest` or `Origin::SystemUi`, and the refusal names it: motion
//! needs a pointer or key in the window, with the operator present
//! (AGENTS.md). Status, gaits, export, STOP and display settings stay
//! available to automation.
// Implementation below the enum: the panel part (input systems, apply).
use crate::app::actions::{self, Spec, spec};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Direction of a hold-to-move press: Q is upper, A is lower.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Upper,
    Lower,
}
impl Direction {
    /// The `motion` value the page sends.
    pub fn motion(self) -> &'static str {
        match self {
            Direction::Upper => "upper",
            Direction::Lower => "lower",
        }
    }
}

/// A taught pose to save here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Boundary {
    Lower,
    Upper,
    /// "Save sim alignment here" (the mirror's alignment joint angle goes with it).
    Reference,
}
impl Boundary {
    pub fn name(self) -> &'static str {
        match self {
            Boundary::Lower => "lower",
            Boundary::Upper => "upper",
            Boundary::Reference => "reference",
        }
    }
}

/// Where a gait plays (the page's radio group).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GaitMode {
    /// Sim only: animates the simulated robot.
    #[default]
    Sim,
    /// Leg only: drives the real leg.
    Leg,
    /// Both, on one clock.
    Both,
}

/// The servo drive for a motion session (the page's Control mode select).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveMode {
    /// "PWM · host feedback loop (default)".
    #[default]
    Pwm,
    /// "Servo position loop · experimental".
    ServoPosition,
    /// "Servo speed loop · experimental".
    ServoSpeed,
}
impl DriveMode {
    pub const ALL: [DriveMode; 3] = [DriveMode::Pwm, DriveMode::ServoPosition, DriveMode::ServoSpeed];
    /// The `drive_mode` value the page sends.
    pub fn wire(self) -> &'static str {
        match self {
            DriveMode::Pwm => "pwm",
            DriveMode::ServoPosition => "servo_position",
            DriveMode::ServoSpeed => "servo_speed",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            DriveMode::Pwm => "PWM · host feedback loop (default)",
            DriveMode::ServoPosition => "Servo position loop · experimental",
            DriveMode::ServoSpeed => "Servo speed loop · experimental",
        }
    }
}

/// The mirror's alignment pose for a motor (CAD home, or mid-travel).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    Home,
    Mid,
}

/// Why drive is being stopped without a STOP press (the page's `loss()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Loss {
    /// The window lost focus (the page's `visibilitychange` to hidden).
    FocusLost,
    /// The panel was closed (× or the header toggle).
    PanelClosed,
    /// Robot mode was left, or the window is closing (the page's `pagehide`).
    Leaving,
}

/// Every intent of the Leg calibration and hardware panel.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareAction {
    // ---- the panel ----
    /// The header's "Leg calibration" button (closing stops drive, as the page's toggle).
    TogglePanel,
    /// The panel's × (stops drive).
    ClosePanel,
    /// Connect to the calibration server (token from its page, then status polling; no motion).
    Connect,
    /// Open or close a collapsible section (opening Gait playback loads the gait list).
    ToggleSection { section: super::Section },
    // ---- reads and STOP (allowed from REST and system_ui) ----
    /// REST `hardware_status`: the panel's state and the last server status, with its age.
    Status,
    /// STOP: the button, Z, Escape, REST `hardware_stop`, `system_ui` hardware:stop.
    Stop,
    /// Drive stops for a reason other than STOP (focus loss, panel close, leaving).
    Loss { reason: Loss },
    /// Download calibration: `/calibration/export` plus the mirror's display
    /// binding, written to a new file under the server's output directory.
    Export,
    /// REST `hardware_gaits` and opening Gait playback: `/calibration/gaits`.
    LoadGaits,
    // ---- motor selection ----
    /// A motor chip (selecting connects and checks it at zero drive; with
    /// "hold the other enabled motors" it energizes their hold).
    Select { id: u8 },
    /// "Disable this motor" / "Enable this motor".
    SetDisabled,
    /// "Sweep all enabled motors" / "Stop sweeping all".
    SweepAll,
    /// "Hold the other enabled motors in place while one moves".
    HoldOthers { on: bool },
    // ---- hold-to-move ----
    /// Q or "Upper ↑" pressed, A or "Lower ↓" pressed.
    JogPress { direction: Direction },
    /// Q/A or the button released: hold here.
    JogRelease { direction: Direction },
    /// Movement speed slider (0–100).
    Speed { percent: f64 },
    /// "Move to a taught pose" slider (0–100, lower → upper), while dragged.
    Target { percent: f64 },
    /// The target slider released (the page's `change`: one more heartbeat).
    TargetCommit,
    // ---- poses ----
    /// "Save lower here", "Save upper here", "Save sim alignment here".
    Capture { boundary: Boundary },
    /// "Reset poses" / "Reset lower pose" / "Re-teach both poses".
    ResetPoses,
    /// Advanced: "Reset lower only".
    ClearLower,
    /// Advanced: "Reset upper only".
    ClearUpper,
    /// "Try saved range" / "Pause & hold".
    Sweep,
    /// "Learn motion in the middle" / "Pause learning & hold".
    Learn,
    // ---- tune and campaign ----
    /// "The motor is mid-travel with room to move both ways".
    TuneConfirm { on: bool },
    /// "Tune this motor".
    Tune,
    /// "The leg is suspended with clear space around every joint" (campaign).
    CampaignConfirm { on: bool },
    /// "Run campaign" (resume false) and "Resume" (true).
    Campaign { resume: bool },
    // ---- gait playback ----
    GaitSelect { index: usize },
    GaitMode { mode: GaitMode },
    /// Playback speed slider (5–100 % of the gait's timing).
    GaitSpeed { percent: f64 },
    /// Leg effort slider (10–100 % of measured motor capability).
    GaitEffort { percent: f64 },
    /// "The leg is suspended with clear space around every joint (needed for Leg and Both)".
    GaitConfirm { on: bool },
    /// "Play" / "Pause" / "Resume".
    GaitPlay,
    /// The gait's "Stop".
    GaitStop,
    // ---- advanced ----
    DriveMode { mode: DriveMode },
    /// "PWM ceiling (%)", 0–100 in 0.1 steps.
    PwmCeiling { percent: f64 },
    /// "Swap upper / lower direction".
    Flip,
    /// "Single raw step" value, −4095..=4095.
    RawStepValue { delta: i32 },
    /// "Send raw step".
    RawStep,
    // ---- mirror (display only) ----
    MirrorEnabled { on: bool },
    /// "+X", "-X", "+Y" or "-Y".
    MirrorLeg { leg: String },
    /// A motor's CAD joint ("Hip servo output", "Worm servo output", "Foot servo output").
    MirrorJoint { id: u8, joint: String },
    MirrorPolarity { id: u8, polarity: i8 },
    MirrorAlign { id: u8, align: Align },
    // ---- live motor sync (serve_motor_bench) ----
    /// Connect to the motor bench (token from its page, `/config`, status polling; no motion).
    SyncConnect,
    SyncLeg { leg: String },
    /// A joint row's motor ID (row index in the leg's coordinate order).
    SyncMotor { row: usize, motor_id: u8 },
    SyncPolarity { row: usize, polarity: i8 },
    /// "Bench motion scale": 0.03, 0.05 or 0.09.
    SyncScale { scale: f64 },
    /// "Sync motors · 12 seconds".
    SyncStart,
    /// "Stop motors".
    SyncStop,
}

impl HardwareAction {
    /// The action's name as refusals and `system_ui` ids write it (its serde tag).
    pub fn name(&self) -> String {
        match serde_json::to_value(self) {
            Ok(Value::String(s)) => s,
            Ok(Value::Object(m)) => m.keys().next().cloned().unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Whether this action can start, change or arm motion, and so is
    /// refused from REST and `system_ui`: energizing or moving a motor, a
    /// session, sweep, tune, campaign, gait on the leg, raw step or live
    /// sync; the speed, effort, target, PWM ceiling, drive mode and
    /// hold-others of motion; the operator's safety confirmations (tune,
    /// campaign, gait); enabling a motor; the gait and raw step chosen; and
    /// the live sync's motor mapping and scale. Allowed from automation:
    /// reads, export, STOP (and the gait's and live sync's stops), connect,
    /// showing the panel and its sections, and the mirror's display settings.
    pub fn starts_motion(&self) -> bool {
        use HardwareAction as H;
        match self {
            H::Select { .. }
            | H::SetDisabled
            | H::SweepAll
            | H::HoldOthers { .. }
            | H::JogPress { .. }
            | H::JogRelease { .. }
            | H::Speed { .. }
            | H::Target { .. }
            | H::TargetCommit
            | H::Capture { .. }
            | H::ResetPoses
            | H::ClearLower
            | H::ClearUpper
            | H::Sweep
            | H::Learn
            | H::TuneConfirm { .. }
            | H::Tune
            | H::CampaignConfirm { .. }
            | H::Campaign { .. }
            | H::GaitSelect { .. }
            | H::GaitMode { .. }
            | H::GaitSpeed { .. }
            | H::GaitEffort { .. }
            | H::GaitConfirm { .. }
            | H::GaitPlay
            | H::DriveMode { .. }
            | H::PwmCeiling { .. }
            | H::Flip
            | H::RawStepValue { .. }
            | H::RawStep
            | H::SyncLeg { .. }
            | H::SyncMotor { .. }
            | H::SyncPolarity { .. }
            | H::SyncScale { .. }
            | H::SyncStart => true,
            H::TogglePanel
            | H::ClosePanel
            | H::Connect
            | H::ToggleSection { .. }
            | H::Status
            | H::Stop
            | H::Loss { .. }
            | H::Export
            | H::LoadGaits
            | H::GaitStop
            | H::MirrorEnabled { .. }
            | H::MirrorLeg { .. }
            | H::MirrorJoint { .. }
            | H::MirrorPolarity { .. }
            | H::MirrorAlign { .. }
            | H::SyncConnect
            | H::SyncStop => false,
        }
    }

    /// The refusal for a motion action from REST or `system_ui`, naming it.
    pub fn remote_refusal(&self) -> String {
        format!(
            "hardware `{}` starts, changes or arms motion and needs an operator at the window: REST and system_ui may read status, list gaits, export, connect, change the mirror's display and STOP only",
            self.name()
        )
    }
}

/// The REST form: its commands (their names are what `app::actions` checks
/// against the registry).
pub(crate) mod wire {
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
    pub enum Command {
        HardwareStatus,
        HardwareStop,
        HardwareExport,
        HardwareGaits,
        /// Any panel intent by its serde form; motion ones are refused by the handler.
        Hardware { action: super::HardwareAction },
    }
}

impl actions::Action for HardwareAction {
    fn commands() -> Vec<Spec> {
        let r = crate::app::actions::ROBOT;
        vec![
            spec("hardware_status", r, json!({}), "Leg calibration panel: the link (connected, stale, age), the page's session state and the calibration server's last status. Read-only."),
            spec("hardware_stop", r, json!({}), "STOP the calibration server's drive (the panel's Stop / Z) on the immediate path, and the live motor sync if it runs. Always allowed."),
            spec("hardware_export", r, json!({}), "Download calibration: /calibration/export plus the mirror's display-only binding, written to a new file under the calibration output directory; answers its path."),
            spec("hardware_gaits", r, json!({}), "The calibration server's gait list (/calibration/gaits), as the Gait playback select lists it."),
            spec(
                "hardware",
                r,
                json!({"action": {"mirror_enabled": {"on": true}}}),
                "Any Leg calibration panel intent in its serde form (as system_ui lists it). Intents that start, change or arm motion (select, enable, jog, speed, sweep, tune, campaign, gait choice/play, drive settings, the operator's confirmations, raw step, live sync mapping and start, …) are refused from REST by name; STOP, reads, export, connect, sections and the mirror's display settings are allowed.",
            ),
        ]
    }
    fn parse(command: &sim_api::Command) -> Result<Self, String> {
        Ok(match sim_api::decode::<wire::Command>(command)? {
            wire::Command::HardwareStatus => HardwareAction::Status,
            wire::Command::HardwareStop => HardwareAction::Stop,
            wire::Command::HardwareExport => HardwareAction::Export,
            wire::Command::HardwareGaits => HardwareAction::LoadGaits,
            wire::Command::Hardware { action } => action,
        })
    }
    fn accepts() -> Vec<&'static str> {
        actions::variants::<wire::Command>()
    }
    fn controls() -> &'static [&'static str] {
        &["hardware:<name>"]
    }
}

// ======================================================================
// The panel part: lifecycle, input mappings, the one apply system and the
// job results (connect, immediate STOP answers, export, the snapshot).
// ======================================================================

use super::handlers::{Answer, handle, inputs_changed, remote_check};
use super::link::{self, LinkCommand};
use super::panel::{JogButton, PanelSlider};
use super::Hardware;
use crate::app::actions::{Act, InFlight, Origin, Replies};
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::builder::ui_api::Enabled;
use crate::jobs::{Job, Pool};
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use sim_api::Outcome;
use sim_runtime::hardware_client::ServerKind;
use std::time::Instant;

/// The operator's saved panel preferences (drive mode, hold-others, mirror
/// and live sync bindings), read once from `settings::path()` when the app
/// is built, before the event loop starts, so entering Robot mode never
/// reads a file on the UI thread. [`enter`] clones them into
/// [`Hardware::new`]; [`leave`] writes the panel's `Hardware::settings`
/// back, so the next entry sees what was changed (each change is also
/// saved to the file off the UI thread by `Settings::save`).
#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct Preferences(pub super::settings::Settings);

/// Registers the action type, the saved preferences ([`Preferences`]), the
/// lifecycle (OnEnter/OnExit of the Robot scope) and the systems: input
/// mappings in Input (after the REST poll), [`apply`] in Actions (after
/// robot mode's own, which passes `system_ui` activations on), the job
/// results in JobResults.
pub(crate) fn build(app: &mut App) {
    actions::register::<HardwareAction>(app);
    app.insert_resource(Preferences(super::settings::load()));
    app.add_systems(OnEnter(ModeScope::Robot), enter).add_systems(OnExit(ModeScope::Robot), leave).add_systems(
        Update,
        (
            (buttons, jog_buttons, keys, window_loss, sliders).chain().after(actions::serve).in_set(ViewerSet::Input),
            apply.after(super::super::actions::apply).in_set(ViewerSet::Actions),
            poll_jobs.in_set(ViewerSet::JobResults),
        )
            .run_if(in_state(ViewerMode::Robot)),
    );
}

/// OnEnter(Robot): the panel's state from the launch's servers and the
/// saved preferences ([`Preferences`], already in memory: no file read
/// here); with `--hardware` the panel is open and connects (status only:
/// nothing moves until the operator selects a motor).
fn enter(mut commands: Commands, documents: Option<Res<crate::app::switch::Documents>>, preferences: Option<Res<Preferences>>) {
    let config = documents.map(|d| d.hardware.clone()).unwrap_or_default();
    let mut hw = Hardware::new(config, preferences.map(|p| p.0.clone()).unwrap_or_default());
    if hw.config.calibration.is_some() {
        connect(&mut hw);
    }
    commands.insert_resource(hw);
}

/// OnExit(Robot): STOP on the immediate path first, the live sync stopped,
/// the preferences kept for the next entry ([`Preferences`]), then the state
/// is dropped off the UI thread (the link thread sends STOP again as its
/// channel closes; the STOP job completes on its own).
fn leave(world: &mut World) {
    let Some(mut hw) = world.remove_resource::<Hardware>() else { return };
    stop_immediate(&mut hw);
    hw.sync.stop_ours("Robot mode was left");
    if let Some(mut preferences) = world.get_resource_mut::<Preferences>() {
        preferences.0 = hw.settings.clone();
    }
    crate::jobs::drop_off_thread(hw, "the hardware panel");
}

/// Posts STOP on its own connection (`link::stop_now`, with the link's
/// newest motor) and tells the link it was sent, with the epoch `stop_now`
/// bumped to (always, also when no motor is known: the link sends nothing
/// but `stop` until that `Stopped` arrives); forgets held jog presses.
pub(super) fn stop_immediate(hw: &mut Hardware) {
    hw.form.held_upper = false;
    hw.form.held_lower = false;
    let Some(link) = hw.link.as_ref() else { return };
    let (epoch, job) = link::stop_now(link, link.snapshot().id);
    if let Some(job) = job {
        hw.stops.push(job);
    }
    link.send(LinkCommand::Stopped { epoch });
}

/// Token discovery and the client, off the UI thread; `poll_jobs` starts
/// the link. A link already open is stopped and dropped first.
pub(super) fn connect(hw: &mut Hardware) {
    if hw.connecting.is_some() {
        return;
    }
    if hw.link.is_some() {
        stop_immediate(hw);
        if let Some(old) = hw.link.take() {
            crate::jobs::drop_off_thread(old, "the previous hardware link");
        }
        hw.snapshot = Default::default();
    }
    hw.generation += 1;
    hw.notice = None;
    let target = hw.target();
    hw.connecting = Some(Job::spawn(Pool::Dedicated, hw.generation, "hardware connect", move |_| {
        sim_runtime::hardware_client::token::connect(&target.url, target.token_file.as_deref(), ServerKind::Calibration).map_err(|e| e.to_string())
    }));
}

/// Input: a pressed panel button's action (not while disabled). The jog
/// buttons are hold-to-move ([`jog_buttons`]).
fn buttons(clicks: Query<(&Interaction, &HardwareAction, Option<&Enabled>), (Changed<Interaction>, Without<JogButton>)>, mut out: MessageWriter<Act<HardwareAction>>) {
    for (interaction, action, enabled) in &clicks {
        if *interaction == Interaction::Pressed && enabled.is_none_or(|e| e.0) {
            out.write(Act::ui(action.clone()));
        }
    }
}

/// Input: "Upper ↑"/"Lower ↓" pressed is `JogPress`; the press ending
/// (released anywhere: `bevy::ui` keeps `Pressed` until the left button is
/// released, like the page's pointer capture) is `JogRelease`.
fn jog_buttons(mut jogs: Query<(&Interaction, &mut JogButton, Option<&Enabled>)>, mut out: MessageWriter<Act<HardwareAction>>) {
    for (interaction, mut jog, enabled) in &mut jogs {
        let pressed = *interaction == Interaction::Pressed;
        if pressed && !jog.held && enabled.is_none_or(|e| e.0) {
            jog.held = true;
            out.write(Act::ui(HardwareAction::JogPress { direction: jog.direction }));
        } else if !pressed && jog.held {
            jog.held = false;
            out.write(Act::ui(HardwareAction::JogRelease { direction: jog.direction }));
        }
    }
}

/// Input: the page's keys (:290-296) while the panel is shown and no
/// Ctrl/Cmd/Alt is held: Z or Escape is STOP; Q/A press is `JogPress`
/// (a key repeat is not a press in Bevy). A Q/A release is `JogRelease`
/// whether or not the panel is shown (the page's keyup); [`apply`] ignores
/// a release it did not accept a press for, and holds when both are held.
fn keys(keys: Res<ButtonInput<KeyCode>>, hw: Option<Res<Hardware>>, mut out: MessageWriter<Act<HardwareAction>>) {
    const JOG: [(KeyCode, Direction); 2] = [(KeyCode::KeyQ, Direction::Upper), (KeyCode::KeyA, Direction::Lower)];
    let Some(hw) = hw else { return };
    let modified = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::AltLeft, KeyCode::AltRight]);
    if hw.open && !modified {
        if keys.any_just_pressed([KeyCode::KeyZ, KeyCode::Escape]) {
            out.write(Act::ui(HardwareAction::Stop));
        }
        for (code, direction) in JOG {
            if keys.just_pressed(code) {
                out.write(Act::ui(HardwareAction::JogPress { direction }));
            }
        }
    }
    for (code, direction) in JOG {
        if keys.just_released(code) {
            out.write(Act::ui(HardwareAction::JogRelease { direction }));
        }
    }
}

/// Input: the window losing focus stops drive (the page's
/// `visibilitychange`), a close request too (`pagehide`; the window is
/// despawned a frame later, so this frame's apply still runs; its handler
/// also posts STOP synchronously, `handlers::post_stop_on_leave`).
fn window_loss(mut focus: MessageReader<bevy::window::WindowFocused>, mut close: MessageReader<bevy::window::WindowCloseRequested>, hw: Option<Res<Hardware>>, mut out: MessageWriter<Act<HardwareAction>>) {
    let lost = focus.read().filter(|e| !e.focused).count() > 0;
    let closing = close.read().count() > 0;
    if hw.is_none() {
        return;
    }
    if closing {
        out.write(Act::quiet(HardwareAction::Loss { reason: Loss::Leaving }));
    } else if lost {
        out.write(Act::quiet(HardwareAction::Loss { reason: Loss::FocusLost }));
    }
}

/// What the slider input last held.
#[derive(Default)]
struct HeldSlider {
    which: Option<PanelSlider>,
    /// A target was sent during this hold (its release is the page's `change`).
    moved_target: bool,
}

/// Input: the panel's sliders while held (the page's `input` events, in
/// each input's steps); releasing the target slider after moving it is
/// `TargetCommit` (its `change`).
fn sliders(sliders: Query<(&bevy::ui_widgets::SliderValue, Has<bevy::ui::Pressed>, &Interaction, Has<bevy::ui::InteractionDisabled>, &PanelSlider)>, hw: Option<Res<Hardware>>, mut held: Local<HeldSlider>, mut out: MessageWriter<Act<HardwareAction>>) {
    let Some(hw) = hw else {
        *held = HeldSlider::default();
        return;
    };
    let f = &hw.form;
    let mut now = None;
    for (value, pressed, interaction, disabled, which) in &sliders {
        if disabled || !crate::ui_kit::slider_held(pressed, interaction) {
            continue;
        }
        now = Some(*which);
        let x = value.0.clamp(0.0, 1.0) as f64;
        let action = match which {
            PanelSlider::Speed => Some((x * 100.0).round()).filter(|v| *v != f.inputs.speed_percent).map(|percent| HardwareAction::Speed { percent }),
            PanelSlider::Target => Some((x * 1000.0).round() / 10.0).filter(|v| *v != f.target_percent).map(|percent| HardwareAction::Target { percent }),
            PanelSlider::GaitSpeed => Some(5.0 + (x * 95.0).round()).filter(|v| *v != f.inputs.gait_speed_percent).map(|percent| HardwareAction::GaitSpeed { percent }),
            PanelSlider::GaitEffort => Some(10.0 + (x * 90.0).round()).filter(|v| *v != f.inputs.gait_effort_percent).map(|percent| HardwareAction::GaitEffort { percent }),
            PanelSlider::Pwm => Some((x * 1000.0).round() / 10.0).filter(|v| *v != f.inputs.pwm_percent).map(|percent| HardwareAction::PwmCeiling { percent }),
        };
        if let Some(action) = action {
            if matches!(action, HardwareAction::Target { .. }) {
                held.moved_target = true;
            }
            out.write(Act::ui(action));
        }
    }
    if held.which == Some(PanelSlider::Target) && now != Some(PanelSlider::Target) && held.moved_target {
        out.write(Act::ui(HardwareAction::TargetCommit));
    }
    if now != Some(PanelSlider::Target) {
        held.moved_target = false;
    }
    held.which = now;
}

/// Actions: the panel's one apply system. Motion from REST or `system_ui`
/// is refused by name; a remote action whose control is disabled now is
/// refused with the control's reason. A click's or key's refusal is the
/// panel's notice (a `system_ui` one too); REST gets the outcome, with
/// `hardware_status`'s JSON as the answer of an accepted command.
pub(crate) fn apply(
    mut messages: ResMut<Messages<Act<HardwareAction>>>,
    mut in_flight: ResMut<InFlight<HardwareAction>>,
    mut replies: ResMut<Replies>,
    hw: Option<ResMut<Hardware>>,
    view: Option<Res<crate::robot::RobotView>>,
    mut robot_out: MessageWriter<Act<crate::robot::RobotAction>>,
) {
    let Some(mut hw) = hw else {
        actions::apply(&mut messages, &mut in_flight, &mut replies, |_, _| Outcome::Done(Err("the Leg calibration panel is not open in this mode".into())));
        return;
    };
    let now = Instant::now();
    actions::apply(&mut messages, &mut in_flight, &mut replies, |action, call| {
        let hw = &mut *hw;
        let answer = if call.remote() && action.starts_motion() {
            Answer::Done(Err(action.remote_refusal()))
        } else if let Err(e) = remote_check(hw, action, call) {
            Answer::Done(Err(e))
        } else {
            handle(hw, action, call, now, view.as_deref(), &mut |a| {
                robot_out.write(Act::quiet(a));
            })
        };
        match (answer, call.origin) {
            (Answer::Pending, _) => Outcome::Pending,
            (Answer::Done(result), Origin::Rest(_)) => Outcome::Done(result.map(|v| v.unwrap_or_else(|| super::view::status_json(hw, now)))),
            (Answer::Done(Err(e)), Origin::Ui | Origin::SystemUi) => {
                hw.notice = Some(e);
                Outcome::Done(Ok(Value::Null))
            }
            (Answer::Done(Ok(_)), Origin::Ui) => {
                hw.notice = None;
                Outcome::Done(Ok(Value::Null))
            }
            (Answer::Done(_), _) => Outcome::Done(Ok(Value::Null)),
        }
    });
}

/// JobResults: the connect job (a new link, sent the form's inputs), STOP
/// answers (to the link), the export, then the link's snapshot for this
/// frame and the counters the form follows (tune/campaign confirmations
/// unchecked when one finishes, the speed back to 0 when a sweep starts).
fn poll_jobs(hw: Option<ResMut<Hardware>>) {
    let Some(mut hw) = hw else { return };
    let hw = &mut *hw;
    if let Some(result) = hw.connecting.as_ref().and_then(|j| j.poll()) {
        let generation = hw.connecting.take().map_or(hw.generation, |j| j.generation());
        match result {
            Ok(client) => {
                let link = link::Link::spawn(client, generation);
                // A new link counts its speed resets from 0.
                hw.form.inputs.speed_reset = 0;
                link.send(LinkCommand::Inputs(hw.form.inputs.clone()));
                hw.link = Some(link);
                hw.notice = None;
                hw.form.seen_tune_done = 0;
                hw.form.seen_campaign_done = 0;
                hw.form.seen_speed_reset = 0;
                hw.ui_revision += 1;
            }
            Err(e) => hw.notice = Some(format!("Could not connect to {}: {e}", hw.url())),
        }
    }
    // An immediate STOP's answer goes to the link that posted it (a STOP
    // posted before a reconnect is answered to nobody: its link is gone).
    let mut answered = Vec::new();
    hw.stops.retain(|job| match job.poll() {
        Some(result) => {
            answered.push((job.generation(), result));
            false
        }
        None => true,
    });
    if let Some(link) = hw.link.as_ref() {
        for (_, result) in answered.into_iter().filter(|(generation, _)| *generation == link.generation) {
            link.send(LinkCommand::StopAnswered(result));
        }
    }
    if let Some(result) = hw.export.as_ref().and_then(|j| j.poll()) {
        let seq = hw.export.take().map_or(hw.export_seq, |j| j.generation());
        hw.export_line = Some(match &result {
            Ok(path) => format!("Saved {}", path.display()),
            Err(e) => format!("Download failed: {e}"),
        });
        hw.export_done = Some((seq, result));
    }
    hw.snapshot = hw.link.as_ref().map(|l| l.snapshot()).unwrap_or_default();
    let (tune, campaign, reset) = (hw.snapshot.tune_done, hw.snapshot.campaign_done, hw.snapshot.speed_reset);
    let f = &mut hw.form;
    if tune > f.seen_tune_done {
        f.tune_ok = false;
    }
    if campaign > f.seen_campaign_done {
        f.campaign_ok = false;
    }
    let speed_reset = reset > f.seen_speed_reset;
    if speed_reset {
        // The link reset the speed (a sweep started): the slider goes to 0,
        // and the inputs say which reset they include, so the link does not
        // take an older speed sent before it for a new one.
        f.inputs.speed_percent = 0.0;
        f.inputs.speed_reset = reset;
    }
    (f.seen_tune_done, f.seen_campaign_done, f.seen_speed_reset) = (tune, campaign, reset);
    if speed_reset {
        inputs_changed(hw);
    }
}
