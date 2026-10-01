//! The calibration server's link: one `jobs::RunThread` ("hardware-link")
//! running the page's session logic ([`super::session`]) over
//! `sim_runtime::hardware_client`, the commands the UI sends it
//! ([`LinkCommand`], one per page handler), the form values the page reads
//! when it sends ([`Inputs`]), and what it publishes ([`LinkSnapshot`]).
//!
//! - **Order and sequence.** The thread sends every request itself, one at a
//!   time, in the order the commands arrive, each `/calibration/command`
//!   with the next value of the shared sequence counter ([`Link::sequence`]),
//!   which the immediate STOP path draws from too, so sequences only grow.
//! - **Periods** (the page's): status poll 600 ms, 150 ms while a motion
//!   session or a leg gait runs (after each answer, as `setTimeout`); the
//!   `motion_update` heartbeat every 100 ms while a session is open; the
//!   gait lease `gait_update` every 300 ms while a leg gait runs (each from
//!   its own job, so a slow request on this thread never lets the server's
//!   1.5 s lease expire); sweep-all progress every 250 ms, tune 300 ms,
//!   campaign 500 ms.
//! - **Epoch.** [`Link::epoch`] is the page's `epoch`: the UI bumps it before
//!   posting an immediate STOP, so an answer to a request that was in flight
//!   when STOP was pressed is dropped exactly as the page drops it. Unlike
//!   the page, a dropped successful `select`, `motion_start`, `sweep_all`,
//!   `gait_start`, `tune` or `campaign` is followed by a `stop` at once: the
//!   server may have parsed it after the UI's STOP (a `select` clears the
//!   stop latch), and nothing else would end what it energized.
//! - **Stop on drop.** Dropping [`Link`] closes the channel; the thread sends
//!   STOP (when [`drive_active`]) before it returns. The UI also posts STOP
//!   on the immediate path first ([`stop_now`]), so neither waits for the
//!   other. The thread is
//!   never joined: the UI drops the link off its own thread
//!   (`jobs::drop_off_thread`), and the shutdown STOP may take up to
//!   `STOP_TIMEOUT`.
//! - **Staleness.** The snapshot carries when the server's status was last
//!   read; the panel shows an older one as stale ([`STALE_AFTER`]), never as live.
use super::actions::{Boundary, Direction, DriveMode, GaitMode};
use sim_runtime::hardware_client::calibration::{self, GaitBinding, GaitEntry, Status};
use sim_runtime::hardware_client::{Client, STOP_TIMEOUT};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Status poll period while idle (the page's `setTimeout(poll, 600)`).
pub const POLL_IDLE: Duration = Duration::from_millis(600);
/// Status poll period while a motion session or a leg gait runs (150 ms).
pub const POLL_ACTIVE: Duration = Duration::from_millis(150);
/// The hold-to-move heartbeat (`motion_update`) period while a session is open.
pub const HEARTBEAT: Duration = Duration::from_millis(100);
/// The gait lease heartbeat (`gait_update`) period while a leg gait runs.
pub const GAIT_HEARTBEAT: Duration = Duration::from_millis(300);
/// Sweep-all progress, tune and campaign waits (the page's `sleep`s).
pub const SWEEP_ALL_TICK: Duration = Duration::from_millis(250);
pub const TUNE_TICK: Duration = Duration::from_millis(300);
pub const CAMPAIGN_TICK: Duration = Duration::from_millis(500);
/// A status older than this is shown as stale (four idle polls without an answer).
pub const STALE_AFTER: Duration = Duration::from_millis(2400);

/// The page's `intent`: what a motion session is asked to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Intent {
    #[default]
    Hold,
    Upper,
    Lower,
    Sweep,
    Learn,
    Target,
}
impl Intent {
    /// The `motion` value sent.
    pub fn motion(self) -> &'static str {
        match self {
            Intent::Hold => "hold",
            Intent::Upper => "upper",
            Intent::Lower => "lower",
            Intent::Sweep => "sweep",
            Intent::Learn => "learn",
            Intent::Target => "target",
        }
    }
}

/// The form values the page reads when it sends (`input()`, the gait
/// sliders): the UI sends them whenever one changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    /// Movement speed slider, 0–100 (`speed()` maps it to counts/s).
    pub speed_percent: f64,
    /// PWM ceiling, 0–100 in 0.1 steps (`drive_pwm` = round(×10)).
    pub pwm_percent: f64,
    /// "Hold the other enabled motors in place while one moves".
    pub hold_others: bool,
    pub drive_mode: DriveMode,
    /// Gait playback speed, 5–100 (% of the gait's timing).
    pub gait_speed_percent: f64,
    /// Gait leg effort, 10–100 (% of measured capability).
    pub gait_effort_percent: f64,
    /// How many speed resets ([`LinkSnapshot::speed_reset`]) the UI had
    /// applied to its speed slider when it sent these values. The link keeps
    /// its own speed (0 after a reset) while this is behind, so an `Inputs`
    /// sent before the UI saw a reset cannot put the old speed back.
    pub speed_reset: u64,
}
impl Default for Inputs {
    /// The page's initial values (speed 0, PWM 100, hold others checked, PWM
    /// drive, gait speed 100, effort 50).
    fn default() -> Self {
        Self { speed_percent: 0.0, pwm_percent: 100.0, hold_others: true, drive_mode: DriveMode::Pwm, gait_speed_percent: 100.0, gait_effort_percent: 50.0, speed_reset: 0 }
    }
}

/// One page handler, run on the link thread in arrival order.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkCommand {
    /// New form values (no request by itself; the page reads them at send time).
    Inputs(Inputs),
    /// A motor chip (`selectMotor(id)`; a running sweep-all stops with its text).
    Select { id: u8 },
    /// "Disable/Enable this motor".
    SetDisabled,
    /// "Sweep all enabled motors" / "Stop sweeping all".
    SweepAll,
    /// Q / A / the jog buttons pressed (`move(direction)`).
    Press { direction: Direction },
    /// Released (`release()`).
    Release,
    /// Both Q and A held (`keys.size > 1`: hold).
    BothKeys,
    /// The movement speed changed (`update()`; the value came in `Inputs`).
    SpeedChanged,
    /// The PWM ceiling changed and is valid (`update()`).
    PwmChanged,
    /// The target slider moved: fraction 0–1 between the taught poses.
    Target { fraction: f64 },
    /// The target slider released (`update()`).
    TargetCommit,
    /// Save a pose (the mirror's alignment angle goes with `reference`).
    Capture { boundary: Boundary, reference_joint_rad: Option<f64> },
    /// "Reset poses" (the boundary chosen from the pose the motor is beyond, else both).
    ResetPoses,
    /// Advanced: reset one pose (`lower` or `upper`).
    Clear { boundary: Boundary },
    Flip,
    /// "Try saved range" / "Pause & hold".
    Sweep,
    /// "Learn motion in the middle" / "Pause learning & hold".
    Learn,
    /// "Send raw step" with the step field's value.
    RawStep { delta: i16 },
    /// "Tune this motor" (the UI checked the confirmation box).
    Tune,
    /// "Run campaign" / "Resume" (the UI checked the confirmation box).
    Campaign { resume: bool },
    /// Opening Gait playback, or REST `hardware_gaits`: GET /calibration/gaits.
    LoadGaits,
    /// "Play" with no gait playing: fetch the compiled gait; for Leg/Both,
    /// the mirror's bindings (computed by the UI from the same status) and
    /// the skipped motors' reasons.
    GaitPlay { entry: GaitEntry, mode: GaitMode, bindings: Vec<GaitBinding>, skipped: Vec<String> },
    /// "Pause" / "Resume" of a playing gait.
    GaitToggle,
    /// The gait's "Stop".
    GaitStop,
    /// The playback speed slider moved while a gait plays.
    GaitScale,
    /// STOP was posted on the immediate path ([`stop_now`]): the page's
    /// `stop()` without its request (clear intent, end sessions and the leg
    /// gait, not ready). `epoch` is the value [`stop_now`] bumped the shared
    /// epoch to: the link counts that STOP (and every earlier one) as
    /// applied, but not a later STOP whose own `Stopped` is still queued.
    Stopped { epoch: u64 },
    /// The immediate STOP's answer: the new status (`state = await send('stop')`)
    /// or its error (`state.message = e.message`).
    StopAnswered(Result<serde_json::Value, String>),
    /// Focus loss, panel close, leaving (`loss()`): stop if [`drive_active`]
    /// says something may be driving, and the UI has not already.
    Loss,
}

/// A gait playing (the page's `gaitRun`).
#[derive(Clone, Debug, PartialEq)]
pub struct GaitRun {
    pub mode: GaitMode,
    /// The gait's period (s), from `gait_playback::Gait`'s info.
    pub period_s: f64,
    /// Gait time (s): the leg's `state.gait.t` for Leg/Both, else advanced
    /// on the link thread by wall time × scale while playing.
    pub t: f64,
    pub playing: bool,
    /// Playback speed fraction (0.05–1).
    pub scale: f64,
    /// Leg or Both: the server drives the leg.
    pub leg: bool,
    /// Motors not driven, with why.
    pub skipped: Vec<String>,
    /// The server reported the leg gait running at least once.
    pub started: bool,
}

/// What the link publishes (the page's closure state).
#[derive(Clone, Debug, Default)]
pub struct LinkSnapshot {
    /// The link's generation (each connect is a new link, a later generation).
    pub generation: u64,
    /// Bumped on every publish (the panel re-renders on a change).
    pub revision: u64,
    /// The page's `state`: the last server status adopted, with the page's
    /// local edits (`state.message = e.message`, `state.capture_message`).
    pub state: Status,
    /// When `state` was last read from the server (None: never).
    pub read_at: Option<Instant>,
    pub id: Option<u8>,
    pub ready: bool,
    pub busy: bool,
    pub run: Option<u64>,
    pub starting: bool,
    pub intent: Intent,
    pub target_raw: f64,
    pub sweeping: bool,
    pub learning: bool,
    pub tuning: bool,
    pub campaigning: bool,
    /// A sweep-all is running (`sweepAllRun`).
    pub sweep_all: bool,
    /// The sequence line (`sequenceText`).
    pub sequence_text: String,
    /// The warnings log, newest first, at most 6: (local time, text).
    pub warnings: Vec<(String, String)>,
    pub gait: Option<GaitRun>,
    /// A gait-status line that replaces the rendered one until the next
    /// render rule applies (gait list unavailable, a play error).
    pub gait_notice: Option<String>,
    /// The gait list (`gaits`), once loaded.
    pub gaits: Vec<GaitEntry>,
    pub gaits_loaded: bool,
    /// The compiled gait fetched for the current play, for the mirror to
    /// sample: (a number that changes per play, the compiled gait, its trial name).
    pub compiled_gait: Option<(u64, Arc<serde_json::Value>, String)>,
    /// Counters the UI follows: the tune and campaign confirmation boxes are
    /// unchecked when these grow; the speed slider goes to 0 when `speed_reset` grows.
    pub tune_done: u64,
    pub campaign_done: u64,
    pub speed_reset: u64,
}
impl crate::jobs::Stamped for LinkSnapshot {
    fn generation(&self) -> u64 {
        self.generation
    }
}
impl LinkSnapshot {
    /// The server status is older than [`STALE_AFTER`] (or was never read).
    pub fn stale(&self, now: Instant) -> bool {
        self.read_at.is_none_or(|t| now.duration_since(t) > STALE_AFTER)
    }
}

/// Whether a loss (focus loss, panel close, leaving, the link dropped) must
/// send STOP: a motor is ready, starting, selecting or in a session, or a
/// sweep-all, tune, campaign or leg gait is running.
///
/// The page's `loss()` (:321) checks only `ready||starting||run!=null`, so
/// after `visibilitychange` it leaves a playing leg gait (until its lease
/// expires), a tune, a campaign and a sweep-all between motors driving the
/// hardware with nobody watching. Stopping those too is a deliberate safety
/// difference from the page. The UI applies the same rule to its snapshot
/// and the link thread to its own state.
pub fn drive_active(s: &LinkSnapshot) -> bool {
    s.ready || s.starting || s.run.is_some() || s.busy || s.sweep_all || s.tuning || s.campaigning || s.gait.as_ref().is_some_and(|g| g.leg)
}

/// The UI's handle on a link (a `Hardware` field).
pub struct Link {
    pub(super) thread: crate::jobs::RunThread<LinkCommand, LinkSnapshot>,
    pub client: Client,
    pub generation: u64,
    /// The page's `sequence` counter, shared with the immediate STOP path.
    pub sequence: Arc<AtomicU64>,
    /// The page's `epoch`, shared: bumped by the UI before an immediate STOP.
    pub epoch: Arc<AtomicU64>,
}

impl Link {
    /// Starts the link thread ("hardware-link") for `client`: it polls the
    /// server at once and then runs [`super::session::run`] until the link
    /// is dropped. The first snapshot is empty, stamped `generation`.
    ///
    /// Dropping the link never waits for the thread (`join_bound(ZERO)`):
    /// the UI drops it off its own thread (`jobs::drop_off_thread`), and the
    /// thread's shutdown STOP may take up to `STOP_TIMEOUT`, longer than
    /// `jobs::JOIN_BOUND`, which would only log a "did not stop" warning.
    /// The detached thread still sends that STOP and then returns.
    pub fn spawn(client: Client, generation: u64) -> Link {
        let sequence = Arc::new(AtomicU64::new(0));
        let epoch = Arc::new(AtomicU64::new(0));
        let (thread_client, thread_sequence, thread_epoch) = (client.clone(), sequence.clone(), epoch.clone());
        let thread = crate::jobs::RunThread::spawn("hardware-link", LinkSnapshot { generation, ..Default::default() }, move |rx, shared| {
            super::session::run(thread_client, generation, thread_sequence, thread_epoch, rx, shared)
        })
        .join_bound(Duration::ZERO);
        Link { thread, client, generation, sequence, epoch }
    }
    /// Queues one page handler. A link whose thread has ended ignores it
    /// (the panel shows the last snapshot, which goes stale).
    pub fn send(&self, command: LinkCommand) {
        let _ = self.thread.send(command);
    }
    /// The published snapshot.
    pub fn snapshot(&self) -> LinkSnapshot {
        self.thread.latest(self.generation).unwrap_or_else(|| self.thread.lock().clone())
    }
}

/// The immediate STOP (the page's `stop()` request): bumps the shared epoch
/// (so an answer to a request in flight is dropped, as the page's `++epoch`
/// does) and, when a motor is chosen, posts `stop` with the next sequence on
/// a fresh connection from its own `Pool::Dedicated` job with the short
/// STOP timeout. Returns the epoch it bumped to, and the job (None when `id`
/// is None: the page's `if(id==null)return`).
///
/// It cannot queue behind the link thread: that thread sends one request at
/// a time and may be waiting up to 8 s for a hardware reply (a select
/// proving watchdogs, a jog), and STOP must not wait for it. The server
/// latches its stop and cancel flags as soon as it parses a `stop` request,
/// before it queues the hardware job (serve_actuator_calibration.rs
/// `handle()`, `if request.action == "stop"`), so a STOP on its own
/// connection takes effect even while the link's request is in progress.
///
/// The caller must then send [`LinkCommand::Stopped`] with the returned
/// epoch (always, also when there is no job: the link thread treats a bumped
/// epoch as a STOP it has not yet applied and sends nothing but `stop` until
/// that STOP's `Stopped` arrives; carrying the epoch keeps a second STOP
/// pressed before the first `Stopped` was handled pending until its own
/// `Stopped`), and later forward the job's result as
/// [`LinkCommand::StopAnswered`]. The job is `complete_on_drop`: dropping
/// its handle never cancels the STOP.
pub fn stop_now(link: &Link, id: Option<u8>) -> (u64, Option<crate::jobs::Job<serde_json::Value>>) {
    let epoch = link.epoch.fetch_add(1, Ordering::SeqCst) + 1;
    let Some(id) = id else { return (epoch, None) };
    let client = link.client.clone();
    let sequence = link.sequence.clone();
    let job = crate::jobs::Job::spawn(crate::jobs::Pool::Dedicated, link.generation, "hardware STOP", move |_| {
        let body = calibration::stop(Some(id), sequence.fetch_add(1, Ordering::SeqCst) + 1);
        client.with_timeout(STOP_TIMEOUT).post("/calibration/command", &body).map_err(|e| e.to_string())
    });
    (epoch, Some(job.complete_on_drop()))
}
