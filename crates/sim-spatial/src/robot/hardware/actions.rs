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
//! (AGENTS.md). Status, gaits, export, connect, STOP and turning the mirror
//! on or off stay available to automation. The mirror's bindings (leg, joint,
//! polarity, alignment) count as motion: Leg/Both gait playback builds its
//! `gait_start` bindings from them, and "Save sim alignment here" saves the
//! alignment angle as the motor's reference.
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
    // ---- mirror (its bindings also drive a Leg/Both gait: `gait_bindings`) ----
    /// Show or hide the mirror (display only; allowed from automation).
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
    /// the live sync's motor mapping and scale; and the mirror's bindings
    /// (leg, joint, polarity, alignment), because `gait_play` builds the
    /// real leg's `gait_start` bindings from them (`Mirror::gait_bindings`)
    /// and "Save sim alignment here" saves the alignment angle as the motor's
    /// reference (`Mirror::alignment_angle`). Allowed from automation: reads,
    /// export, STOP (and the gait's and live sync's stops), connect, showing
    /// the panel and its sections, turning the mirror on or off, and a loss
    /// other than `Loss::Leaving` (refused remotely by `handlers::loss`).
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
            | H::MirrorLeg { .. }
            | H::MirrorJoint { .. }
            | H::MirrorPolarity { .. }
            | H::MirrorAlign { .. }
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
            | H::SyncConnect
            | H::SyncStop => false,
        }
    }

    /// The refusal for a motion action from REST or `system_ui`, naming it.
    pub fn remote_refusal(&self) -> String {
        format!(
            "hardware `{}` starts, changes or arms motion and needs an operator at the window: REST and system_ui may read status, list gaits, export, connect, turn the mirror on or off and STOP only",
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
                "Any Leg calibration panel intent in its serde form (as system_ui lists it). Intents that start, change or arm motion (select, enable, jog, speed, sweep, tune, campaign, gait choice/play, drive settings, the operator's confirmations, raw step, live sync mapping and start, the mirror's leg/joint/polarity/alignment bindings that a Leg/Both gait and the saved alignment use, …) are refused from REST by name; STOP, reads, export, connect, sections, turning the mirror on or off (mirror_enabled) and focus_lost/panel_closed losses are allowed (loss leaving is refused: it is the window closing).",
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

mod input;

use super::handlers::{Answer, handle, inputs_changed, remote_check};
use super::link::{self, LinkCommand};
use super::Hardware;
use crate::app::actions::{Act, InFlight, Origin, Replies};
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::jobs::{Job, Pool};
use input::{buttons, jog_buttons, keys, sliders, window_loss};
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use sim_api::Outcome;
use sim_runtime::hardware_client::ServerKind;
use std::time::Instant;

/// Registers the action type and the shared-owner preference adapter, the
/// lifecycle (OnEnter/OnExit of the Robot scope) and the systems: input
/// mappings in Input's window step (after the REST poll), [`apply`] in Actions (after
/// robot mode's own, `RobotSet::Actions`, which passes `system_ui` activations on), the job
/// results in JobResults; and [`stop_on_exit`] in `Last`, after Bevy's
/// exit systems, in every mode (it needs only the `Hardware` resource).
pub(crate) fn build(app: &mut App) {
    actions::register::<HardwareAction>(app);
    app.add_systems(OnEnter(ModeScope::Robot), enter).add_systems(OnExit(ModeScope::Robot), leave).add_systems(
        Update,
        (
            (buttons, jog_buttons, keys, window_loss, sliders).chain().in_set(crate::app::InputSet::Window),
            apply.after(crate::robot::RobotSet::Actions).in_set(ViewerSet::Actions),
            poll_jobs.in_set(ViewerSet::JobResults),
        )
            .run_if(in_state(ViewerMode::Robot)),
    )
    .add_systems(Last, stop_on_exit.after(bevy::window::ExitSystems));
}

/// Last, the frame an `AppExit` is written (by Bevy's `exit_on_all_closed`
/// after the window closed, or by anything else, e.g. a quit from code):
/// with the panel's state present, STOP on the immediate path, then written
/// synchronously to the calibration server ([`link::Link::post_stop_sync`],
/// a no-op when the window-close loss already wrote it or no motor is known)
/// and to the motor bench (`LiveSync::post_stop_on_leave`, only a session
/// this viewer opened), because the detached STOP jobs and link thread die
/// with the process. Runs once; blocks the main thread for at most about
/// 1 s per server, as the app is exiting anyway.
///
/// What remains unguarded: an exit that writes no `AppExit` and never
/// returns through winit's `exiting` (SIGKILL, a crash or abort, power
/// loss) skips this, the window-close loss and the link's drop alike. Then
/// the servers' own leases (the 1.5 s gait lease, the motion session's
/// heartbeat lease) and the FPGA watchdog stop the motors, but a tune,
/// campaign or sweep-all, which hold no lease, keeps running server-side
/// until it ends or someone sends STOP. (Cmd+Q on macOS and the world being
/// cleared at teardown are covered by the link's `Drop`.)
fn stop_on_exit(mut exits: MessageReader<bevy::app::AppExit>, hw: Option<ResMut<Hardware>>, mut done: Local<bool>) {
    if exits.read().count() == 0 || std::mem::replace(&mut *done, true) {
        return;
    }
    let Some(mut hw) = hw else { return };
    stop_immediate(&mut hw);
    if let Some(link) = hw.link.as_ref() {
        link.post_stop_sync("app exit");
    }
    hw.sync.post_stop_on_leave();
}

/// OnEnter(Robot): the panel's state from the launch's servers and the
/// shared owner preferences (already in memory: no file read
/// here); with `--hardware` the panel is open and connects (status only:
/// nothing moves until the operator selects a motor).
fn enter(mut commands: Commands, documents: Option<Res<crate::app::switch::Documents>>, preferences: Res<crate::app::settings::SettingsOwner>) {
    let config = documents.map(|d| d.hardware.clone()).unwrap_or_default();
    let mut hw = Hardware::new(config, preferences.hardware.clone());
    hw.preferences_loaded = preferences.ready;
    if hw.config.calibration.is_some() {
        connect(&mut hw);
    }
    commands.insert_resource(hw);
}

/// OnExit(Robot): STOP on the immediate path first, the live sync stopped,
/// the shared owner already retains accepted preferences, then the state
/// is dropped off the UI thread (the link thread sends STOP again as its
/// channel closes; the STOP job completes on its own).
fn leave(world: &mut World) {
    let Some(mut hw) = world.remove_resource::<Hardware>() else { return };
    stop_immediate(&mut hw);
    hw.sync.stop_ours("Robot mode was left");
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
    mut preferences: ResMut<crate::app::settings::SettingsOwner>,
) {
    let Some(mut hw) = hw else {
        actions::apply(&mut messages, &mut in_flight, &mut replies, |_, _| Outcome::Done(Err("the Leg calibration panel is not open in this mode".into())));
        return;
    };
    if preferences.ready && !hw.preferences_loaded {
        seed_preferences(&mut hw, &preferences.hardware);
    }
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
        // Every accepted choice claims its field even when its value equals
        // the startup default. Untouched persisted choices can still load.
        let accepted = !matches!(&answer, Answer::Done(Err(_)));
        let paths = preference_paths(action);
        if accepted && !paths.is_empty() {
            preferences.set_hardware_claimed(hw.settings.clone(), paths);
            // A partial pre-ready edit must still receive the owner's merged
            // publication on the next frame after readiness.
            hw.preferences_loaded = preferences.ready;
        } else if accepted && !preferences.ready && action.starts_motion() {
            // An active interaction conservatively owns the entire current
            // snapshot, protecting alignment and drive choices underneath it.
            preferences.set_hardware_claimed(hw.settings.clone(), vec![String::new()]);
            hw.preferences_loaded = true;
        }
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

/// Seed only remembered choices. No link command, connection, controller,
/// operator confirmation or session activation is produced by publication.
fn seed_preferences(hw: &mut Hardware, settings: &super::settings::Settings) {
    if let Some(mode) = settings.calibration.drive_mode {
        hw.form.inputs.drive_mode = mode;
    }
    if let Some(on) = settings.calibration.hold_others {
        hw.form.inputs.hold_others = on;
    }
    hw.mirror.load_preferences(&settings.mirror);
    hw.sync.load_preferences(&settings.sync);
    hw.settings = settings.clone();
    hw.preferences_loaded = true;
    hw.ui_revision += 1;
}

fn preference_paths(action: &HardwareAction) -> Vec<String> {
    use HardwareAction as H;
    let paths: Vec<String> = match action {
        H::DriveMode { .. } => vec!["/calibration/drive_mode".into()],
        H::HoldOthers { .. } => vec!["/calibration/hold_others".into()],
        H::MirrorEnabled { .. } => vec!["/mirror/enabled".into()],
        H::MirrorLeg { .. } => vec!["/mirror/leg".into()],
        H::MirrorJoint { id, .. } => vec![format!("/mirror/bindings/{id}/joint")],
        H::MirrorPolarity { id, .. } => vec![format!("/mirror/bindings/{id}/polarity")],
        H::MirrorAlign { id, .. } => vec![format!("/mirror/bindings/{id}/align")],
        H::SyncLeg { .. } => vec!["/sync/leg".into(), "/sync/bindings".into()],
        H::SyncMotor { .. } | H::SyncPolarity { .. } => vec!["/sync/bindings".into()],
        H::SyncScale { .. } => vec!["/sync/amplitude".into()],
        H::SyncStart => vec!["/sync".into()],
        _ => Vec::new(),
    };
    paths
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

#[cfg(test)]
mod tests;
