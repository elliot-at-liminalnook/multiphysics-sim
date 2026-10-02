//! The panel's one action type, [`HardwareAction`]: every intent of the
//! browser's calibration page, mirror and live sync, written by the panel's
//! buttons, keys, focus loss, `system_ui` (`hardware:<name>` controls, listed
//! and activated through robot mode's `system_ui`) and REST
//! (`hardware_status`, `hardware_stop`, `hardware_export`, `hardware_gaits`,
//! `hardware {action}`), and applied by [`apply`] in `ViewerSet::Actions`.
//!
//! **Authorization.** Physical and unknown executions require an operator at
//! the window. Remote HW-01–HW-09 calibration and gait playback (HW-10/HW-11:
//! selecting, the mode, speed, effort, confirmation and Play) alone can use a
//! verified virtual identity pinned to a fresh connection generation.
//! [`HardwareAction::authorize`] is shared by listings and authoritative
//! dispatch. Raw step, flip, live sync and the mirror's bindings remain
//! refused remotely on every link. STOP and the gait's Stop bypass this
//! policy.
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

    /// Whether this action can start, change or arm motion, and so needs an
    /// operator at the window, or from REST and `system_ui` the remote
    /// policy of [`HardwareAction::authorize`] (a pinned fresh virtual
    /// execution, and only for the calibration and gait intents it lists): energizing or moving a motor, a
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

    /// The only remote-motion policy, reused by listing and authoritative dispatch.
    pub(crate) fn authorize(&self, hw: &super::Hardware, now: std::time::Instant) -> Result<(), String> {
        if !self.starts_motion() { return Ok(()); }
        let link = hw.link.as_ref().map(|link| (link.generation, link.snapshot()));
        self.authorize_with(link.as_ref().map(|(generation, s)| (*generation, s)), hw.generation, now)
    }

    /// [`HardwareAction::authorize`] against a link's generation and its
    /// newest snapshot (None: no link) and the panel's current generation.
    ///
    /// Allowed remotely, only through the virtual check (a verified virtual
    /// identity, the current generation, a connection and bus that hold, no
    /// revocation or disconnection, a fresh status): HW-01–HW-09 calibration,
    /// and gait playback, whose form intents (select, mode, speed, effort,
    /// confirmation) and Play (Leg and Both drive the virtual bench's leg;
    /// pause and resume) are everything `starts_motion` lists for the gait.
    /// Never allowed remotely: flip, raw step, the mirror's bindings (they
    /// become `gait_start`'s bindings and the saved alignment angle) and
    /// live sync. A release (move to hold) needs only a link; STOP and the
    /// gait's Stop are not motion starts and are never refused.
    pub(crate) fn authorize_with(&self, link: Option<(u64, &super::link::LinkSnapshot)>, generation: u64, now: std::time::Instant) -> Result<(), String> {
        if !self.starts_motion() { return Ok(()); }
        use HardwareAction as H;
        // A release is a move to hold: like STOP it is never refused for a
        // stale status or a replaced generation (refusing it would leave the
        // motor moving); it needs only a link to send it to. The link thread
        // holds, or STOPs if the session may no longer be driven.
        if matches!(self, H::JogRelease { .. }) {
            return if link.is_some() { Ok(()) } else { Err(format!("hardware `{}`: not connected", self.name())) };
        }
        if !matches!(self, H::Select { .. } | H::SetDisabled | H::SweepAll | H::HoldOthers { .. }
            | H::JogPress { .. } | H::JogRelease { .. } | H::Speed { .. } | H::Target { .. }
            | H::TargetCommit | H::Capture { .. } | H::ResetPoses | H::ClearLower | H::ClearUpper
            | H::Sweep | H::Learn | H::TuneConfirm { .. } | H::Tune
            | H::CampaignConfirm { .. } | H::Campaign { .. } | H::DriveMode { .. } | H::PwmCeiling { .. }
            | H::GaitSelect { .. } | H::GaitMode { .. } | H::GaitSpeed { .. } | H::GaitEffort { .. }
            | H::GaitConfirm { .. } | H::GaitPlay) {
            return Err(self.remote_refusal());
        }
        let Some((link_generation, s)) = link else { return Err(self.remote_refusal()); };
        sim_runtime::hardware_client::calibration::authorize_virtual(
            s.execution.as_ref(), s.generation, generation,
            s.connection_valid && s.state.connected && !s.authorization_revoked && s.disconnected.is_none() && link_generation == generation,
            !s.stale(now),
        ).map_err(|why| format!("hardware `{}`: {why}", self.name()))
    }

    /// The refusal for a motion action from REST or `system_ui`, naming it.
    pub fn remote_refusal(&self) -> String {
        format!(
            "hardware `{}` starts, changes or arms motion and needs an operator at the window: remote calibration requires a verified fresh virtual execution; physical, unknown and out-of-scope motion is refused",
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
        /// Any panel intent; remote calibration requires pinned virtual authorization.
        Hardware { action: super::HardwareAction },
    }
}

impl actions::Action for HardwareAction {
    fn commands() -> Vec<Spec> {
        let r = crate::app::actions::ROBOT;
        vec![
            spec("hardware_status", r, json!({}), "Leg calibration panel: the link (connected, stale, age), the page's session state and the calibration server's last status. Read-only."),
            spec("hardware_stop", r, json!({}), "STOP the calibration server's drive (the panel's Stop / Z) on the immediate path, and the live motor sync if it runs. Always allowed."),
            spec("hardware_export", r, json!({}), "Download calibration: /calibration/export plus the mirror's display-only binding, written to a new file under the calibration output directory; answers {path, simulated}. A virtual bench's download is labelled simulated (execution and simulated: true in the file, a -virtual file name)."),
            spec("hardware_gaits", r, json!({}), "The calibration server's gait list (/calibration/gaits), as the Gait playback select lists it."),
            spec(
                "hardware",
                r,
                json!({"action": {"mirror_enabled": {"on": true}}}),
                "Any Leg calibration intent. HW-01–HW-09 remote calibration and gait playback (gait_select, gait_mode, gait_speed, gait_effort, gait_confirm, gait_play: Leg and Both drive the virtual bench's leg, labelled VIRTUAL (simulated)) require a fresh pinned virtual execution identity and connection generation; replies await the authoritative session consumer (gait_play answers once the gait started, or with why not). Physical, unknown, stale, replaced and disconnected motion is refused, and so are flip, raw step, live sync and the mirror's bindings on every link. STOP, gait_stop, reads, export, connect, sections and focus_lost/panel_closed losses remain allowed; leaving is the actual window close.",
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

use super::handlers::{Answer, handle, inputs_changed};
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
            apply.after(crate::robot::RobotSet::Actions).before(crate::app::close::CloseSet::Apply).in_set(ViewerSet::Actions),
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
    // REST calls still waiting on this panel (a queued calibration command's
    // acknowledgement, an export) are answered now: `apply` does not run
    // outside Robot mode, and on a return they would be answered against the
    // next connection. The STOP below does not depend on them.
    let mut carried = world.get_resource_mut::<InFlight<HardwareAction>>().map(|mut f| std::mem::take(&mut *f)).unwrap_or_default();
    if let Some(mut replies) = world.get_resource_mut::<Replies>() {
        carried.abandon(&mut replies, "left Robot mode before the hardware command finished; STOP was requested");
    }
    let Some(mut hw) = world.remove_resource::<Hardware>() else { return };
    stop_immediate(&mut hw);
    hw.sync.stop_ours("Robot mode was left");
    crate::jobs::drop_off_thread(hw, "the hardware panel");
}

/// An automatic STOP (loss, leaving, a reconnect, a revoked or cancelled
/// remote command): posts STOP on its own connection (`link::stop_now`, with
/// the link's newest motor; id-less only if this link drove,
/// [`link::LinkSnapshot::drove`]) and tells the link it was sent, with the
/// epoch `stop_now` bumped to (always, also when nothing was posted: the link
/// sends nothing but `stop` until that `Stopped` arrives); forgets held jog
/// presses and the tune/campaign confirmations.
pub(super) fn stop_immediate(hw: &mut Hardware) {
    stop_with(hw, false);
}

/// The operator's STOP (the panel's Stop, Z/Escape, REST `hardware_stop`):
/// as [`stop_immediate`], but always posted, id-less when no motor is known.
pub(super) fn operator_stop(hw: &mut Hardware) {
    stop_with(hw, true);
}

fn stop_with(hw: &mut Hardware, operator: bool) {
    super::handlers::release_holds(hw);
    hw.form.tune_ok = false;
    hw.form.campaign_ok = false;
    let Some(link) = hw.link.as_ref() else { return };
    let snapshot = link.snapshot();
    let (epoch, job) = link::stop_now(link, snapshot.id, operator || snapshot.drove);
    if let Some(job) = job {
        hw.stops.push(job);
    }
    link.send(LinkCommand::Stopped { epoch });
}

/// Ordinary close requests stop our sessions before preference publication.
/// This is the existing immediate jobs path, without the bounded blocking
/// shutdown fallback retained by window loss, AppExit and Link::drop.
pub(crate) fn request_close_stop(hw: &mut Hardware) {
    hw.sync.stop_ours("Window closing");
    stop_immediate(hw);
}

/// Token discovery and the client, off the UI thread; `poll_jobs` starts
/// the link. A link already open is stopped and dropped first.
///
/// Reconnect re-pins through the same verification whenever the server
/// reports a virtual execution now: a restarted bench or server has a new
/// identity, pinned with this connect's new generation (the client carries
/// both; `Link::spawn` hands them to the session, which publishes them as
/// the snapshot's `execution` and `generation`, and `hw.generation` is the
/// new one). Otherwise the new link is unpinned (physical or unknown) and
/// [`HardwareAction::authorize`] refuses remote motion on it; when the link
/// it replaces was pinned to a virtual bench, the panel's notice says so
/// ([`BENCH_GONE`]).
pub(super) fn connect(hw: &mut Hardware) {
    if hw.connecting.is_some() {
        return;
    }
    // The link being replaced was pinned to a virtual bench: if the new one
    // is not (the server reports no virtual execution now), `poll_jobs`
    // says why remote motion is refused. Kept across a failed connect.
    hw.replaced_virtual |= hw.link.as_ref().is_some_and(|link| link.client.calibration_execution.as_ref().is_some_and(|(identity, _)| identity.is_virtual_calibration()));
    if hw.link.is_some() {
        stop_immediate(hw);
        if let Some(old) = hw.link.take() {
            crate::jobs::drop_off_thread(old, "the previous hardware link");
        }
        hw.snapshot = Default::default();
    }
    hw.generation = sim_runtime::hardware_client::next_connection_generation();
    hw.notice = None;
    let target = hw.target();
    let generation = hw.generation;
    hw.connecting = Some(Job::spawn(Pool::Dedicated, hw.generation, "hardware connect", move |_| {
        let client = sim_runtime::hardware_client::token::connect(&target.url, target.token_file.as_deref(), ServerKind::Calibration).map_err(|e| e.to_string())?;
        let status = client.get_as::<sim_runtime::hardware_client::calibration::Status>("/calibration/status").map_err(|e| e.to_string())?;
        Ok(match status.execution.filter(|identity| identity.is_virtual_calibration()) {
            Some(identity) => {
                let pinned = client.with_calibration_execution(identity.clone(), generation);
                let body = sim_runtime::hardware_client::calibration::inspect(1);
                let inspected: sim_runtime::hardware_client::calibration::Status = serde_json::from_value(
                    pinned.post("/calibration/command", &body).map_err(|e| e.to_string())?
                ).map_err(|e| format!("virtual inspect status: {e}"))?;
                // This link has not selected anything yet: no STOP (an id-less
                // one could end another client's session on that server).
                if inspected.execution.as_ref() != Some(&identity) || !inspected.connected {
                    return Err("virtual inspect identity or acquisition connection changed; reconnect required".into());
                }
                pinned
            },
            None => client,
        })
    }));
}

/// The panel's notice after a reconnect that replaced a link pinned to a
/// virtual bench with an unpinned one.
pub(super) const BENCH_GONE: &str = "The virtual bench this panel was pinned to is gone and the server reports no virtual execution now: this link is not pinned (physical or unknown), so remote motion is refused. Restart the virtual bench server, then Reconnect to pin its new identity.";

/// Actions: the panel's one apply system. Remote calibration uses the shared
/// fail-closed virtual authorization; a control disabled at submission is
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
    closing: Option<Res<crate::app::close::CloseOwner>>,
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
        // A release (move to hold) is never refused, as STOP is not.
        let answer = if closing.as_ref().is_some_and(|close| close.pending()) && action.starts_motion() && !matches!(action, HardwareAction::JogRelease { .. }) && call.continuation.get("hardware_ticket").is_none() {
            Answer::Done(Err("Window closure is pending; cancel close before starting, changing or arming motion".into()))
        } else {
            handle(hw, action, call, now, view.as_deref(), &mut |a| {
                robot_out.write(Act::quiet(a));
            })
        };
        // Every accepted choice claims its field even when its value equals
        // the startup default. Untouched persisted choices can still load.
        // Accepted means finally Ok: a remote command waiting on the link
        // thread (Pending) claims nothing until its ticket resolves Ok, when
        // this runs again for it.
        let accepted = matches!(&answer, Answer::Done(Ok(_)));
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

/// Seed remembered choices and publish the form's host-only Inputs through
/// the same path as an accepted form edit. This does not request hardware,
/// connect, confirm an operator action or activate a session.
pub(super) fn seed_preferences(hw: &mut Hardware, settings: &super::settings::Settings) {
    if let Some(mode) = settings.calibration.drive_mode {
        hw.form.inputs.drive_mode = mode;
    }
    if let Some(on) = settings.calibration.hold_others {
        hw.form.inputs.hold_others = on;
    }
    // Connection can precede settings readiness. Keep the connected session's
    // send-time configuration coherent with the displayed form, in FIFO order
    // before any explicit Select/Press handled later in this apply system.
    inputs_changed(hw);
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
                let pinned = client.calibration_execution.as_ref().is_some_and(|(identity, _)| identity.is_virtual_calibration());
                let link = link::Link::spawn(client, generation);
                // A new link counts its speed resets from 0.
                hw.form.inputs.speed_reset = 0;
                link.send(LinkCommand::Inputs(hw.form.inputs.clone()));
                hw.link = Some(link);
                hw.notice = None;
                if std::mem::take(&mut hw.replaced_virtual) && !pinned {
                    hw.notice = Some(BENCH_GONE.into());
                }
                // The suspended-leg confirmation attests the leg on the link it
                // was given for (remotely, only a virtual one): a new link
                // starts unchecked, so a confirmation never carries from a
                // virtual bench to a physical leg.
                hw.form.gait_ok = false;
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
        let line = match &result {
            Ok(exported) => format!("Saved {}", exported.path.display()),
            Err(e) => format!("Download failed: {e}"),
        };
        // Labelled when the link was pinned to a virtual bench, or the
        // server labelled the document simulated itself.
        let simulated = hw.export_virtual || result.as_ref().is_ok_and(|e| e.simulated);
        hw.export_line = Some(if simulated { format!("{} {line}", super::handlers::VIRTUAL_EXPORT) } else { line });
        hw.export_done = Some((seq, result));
    }
    hw.snapshot = hw.link.as_ref().map(|l| l.snapshot()).unwrap_or_default();
    // Remote jog presses nobody waits on (a one-way activation): a refusal
    // puts the held flag back and shows the refusal (a REST one is settled
    // as it resolves). The results are moved out and back, not cloned.
    if !hw.pending_presses.is_empty() {
        let results = std::mem::take(&mut hw.snapshot.command_results);
        super::handlers::settle_presses(hw, &results);
        hw.snapshot.command_results = results;
    }
    // Staleness revokes this generation permanently, even if an in-flight
    // request later produces a fresh-looking answer. STOP does not wait for it.
    // A status that aged only because the link thread is waiting for a
    // request that may still answer (a select proving watchdogs, a tune
    // start: the server waits up to 8 s for its hardware, the client 10 s) is
    // not a lost binding; that request's own failure revokes if it is
    // (`calibration::binding_lost`), and its deadline bounds the wait. The
    // panel still shows the status as stale, and queued remote commands still
    // need a fresh status when the link thread takes them.
    let now = Instant::now();
    let revoke = hw.snapshot.execution.is_some() && hw.snapshot.read_at.is_some()
        && ((hw.snapshot.stale(now) && !hw.snapshot.awaiting_answer(now)) || !hw.snapshot.connection_valid || hw.snapshot.authorization_revoked);
    if revoke && hw.link.as_ref().is_some_and(|link| !link.authorization.swap(true, std::sync::atomic::Ordering::SeqCst)) {
        stop_immediate(hw);
        hw.snapshot.authorization_revoked = true;
    }
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
