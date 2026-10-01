//! Real motor sync: the port of `web/viewer/hardware-sync.mjs` against
//! `serve_motor_bench`. Robot mode's live preset run publishes named motor
//! targets on its frames (`robot::run::Frame::motor_targets`, the session's
//! `servo_targets_rad` and coordinate names); this streams the newest of them
//! to the bench's `/live/*` routes. The bench maps them to three motors, the
//! FPGA runs the loop, the watchdog and the 12-second session; nothing here
//! computes a motor command or plant physics.
//!
//! - **Link.** `token::connect(…, ServerKind::MotorBench)` and `GET /config`
//!   on a `jobs::Pool::Dedicated` job (`api('/config')`); `/status` on its own
//!   `Pool::Dedicated` job, one at a time, 150 ms after each answer (:29); one
//!   `jobs::RunThread` ("hardware-sync") posts `/live/open` and, while active,
//!   the newest sample every 50 ms if newer than the last sent (:21, :31).
//! - **STOP never queues and is never gated on local state**: `/stop` goes
//!   out on its own `Pool::Dedicated` job (`complete_on_drop`) whenever a
//!   session is or may be open (active, opening, stopping, bench busy), and
//!   again if an open lands after a stop. Dropping [`LiveSync`] (the page's
//!   `pagehide`) also writes it at once with `Client::send_only`.
//!   [`LiveSync::post_stop_on_leave`] is the same synchronous write for the
//!   window closing (`handlers.rs` calls it before the process exits).
//! - **Start is gated on the run**: a session opens only when the run is
//!   already running or `RunController::check(RunAction::Start)` accepts
//!   Start; a refused Start (a recorded preset, a replay or a cancelled partial
//!   replay, a gait preview holding the run, an ended or failed run) is shown
//!   as the status and no `/live/open` is posted, so the motors never hold the
//!   initial target while the simulation stands still.
//! - **Stop rules** (the page's): Stop motors; the run paused or reset;
//!   input that is no longer live (a replay in progress or a run a replay
//!   replaced, a recorded preset, a gait preview holding the run, the episode
//!   ended, no named targets); a failed sample post; a failed poll. Native
//!   additions: the run failed or ended without a done frame, and Start not
//!   seen running within [`START_GRACE`].
//! - Preferences: the page's `walking-hardware-map-v1` is
//!   `settings::SyncSettings`, saved on every change and at start.
mod apply;
mod page;
mod thread;

pub use apply::apply;
pub use page::{banner_text, distinct, legs, live_input, mapping, reading_lines, sample_from, source_text};

use super::ServerTarget;
use super::mirror::SceneId;
use super::settings::{SyncBinding, SyncSettings};
use crate::jobs::{Job, Pool, RunThread};
use crate::robot::{RobotAction, RobotView};
use crate::robot::run::{Phase, RunAction};
use serde_json::{Value, json};
use sim_runtime::hardware_client::{Body, Client, STOP_TIMEOUT, ServerKind, bench, token};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thread::worker;

/// Sample posting period (the page's `setInterval(send, 50)`).
pub const SEND_PERIOD: Duration = Duration::from_millis(50);
/// Status poll period after each answer (`setTimeout(poll, 150)`).
pub const POLL_PERIOD: Duration = Duration::from_millis(150);
/// "Bench motion scale" options.
pub const SCALES: [(f64, &str); 3] = [(0.03, "3%"), (0.05, "5%"), (0.09, "9%")];
pub const DESCRIPTION: &str = "Map three bench motors to one leg. WASD drives the live simulation; its current motor targets are sent to the FPGA.";
pub const NOTE: &str = "100 Hz FPGA feedback · buffered live targets · 10% PWM ceiling · stops on pause/reset, stale simulation, lost connection, or session end. Targets are buffered about 80–160 ms ahead. Bench motion is scaled relative to the start; loaded-leg tracking is not yet validated.";
pub const NOT_LIVE: &str = "Reset the episode, then choose a live walking controller with named motor targets.";
pub const DISTINCT: &str = "Assign a different motor ID to each joint.";
pub const INITIAL_BANNER: &str = "Simulation only — real motors are not connected. Use Sync motors to connect.";
pub const READY: &str = "Ready. Choose a leg, then start sync and steer with WASD.";
pub const CHART_IDLE: &str = "Connect motors to compare tracking while steering with WASD.";
/// Shown instead of the page's absent section when no bench was given at launch.
pub const NO_BENCH: &str = "Start serve_motor_bench and launch with --motor-bench http://127.0.0.1:PORT (add --motor-bench-token-file FILE if its page does not carry the token).";
/// Deliberately refused (the page's `play()` silently does nothing while it mirrors).
pub const MIRROR_ON: &str = "Turn the leg mirror off first: the simulation cannot run while it mirrors the real leg.";
/// The run failed while a session was open (native: the page's run cannot fail).
pub const RUN_FAILED: &str = "Simulation failed";
/// The run did not reach Running within [`START_GRACE`] of the session's Start.
pub const NOT_STARTED: &str = "Simulation did not start";
/// How long after a session's Start the run may take to report Running
/// before the session is stopped (a Start the run thread refused or lost).
pub const START_GRACE: Duration = Duration::from_secs(2);
/// The write timeout of [`LiveSync::post_stop_on_leave`] (the connect timeout is the client's 500 ms).
pub const LEAVE_STOP_TIMEOUT: Duration = Duration::from_millis(500);

pub enum SyncCommand {
    /// The newest live sample (`latest`).
    Latest(bench::Sample),
    /// `/live/open`; then `lastSent = initial.sequence` and one `send()`.
    Open { body: Body, last_sent: u64 },
    /// The UI stopped: no more samples.
    Deactivate,
}

/// What the sync thread hands back; the UI takes each result once.
#[derive(Default)]
pub struct SyncShared {
    generation: u64,
    open: Option<Result<(), String>>,
    send_error: Option<String>,
}
impl crate::jobs::Stamped for SyncShared {
    fn generation(&self) -> u64 {
        self.generation
    }
}

/// The page's closure state.
pub struct LiveSync {
    target: Option<ServerTarget>,
    saved: SyncSettings,
    connecting: Option<Job<(Client, bench::Config)>>,
    client: Option<Client>,
    config: Option<bench::Config>,
    thread: Option<RunThread<SyncCommand, SyncShared>>,
    /// The `/status` request in flight (one at a time), whether an open answered
    /// since it was sent (its answer may predate the session), and when the next is due.
    status_job: Option<Job<bench::Status>>,
    status_stale: bool,
    next_poll: Instant,
    generation: u64,
    /// The form: leg, bench motion scale and the mapping rows.
    leg: String,
    amplitude: f64,
    rows: Vec<SyncBinding>,
    legs: Vec<String>,
    active: bool,
    preparing: bool,
    stopping: bool,
    server_busy: bool,
    polled: bool,
    /// The panel connected once by itself (the page connects on load).
    attempted: bool,
    seq: u64,
    session_run: Option<String>,
    status: String,
    banner: String,
    readings: String,
    samples: Vec<bench::LiveSample>,
    stops: Vec<Job<Value>>,
    /// STOP requests posted so far (for `hardware_status`).
    stops_posted: u64,
    /// The run a session started on (generation, preset identity) and whether it has been seen running since.
    started_run: Option<(u64, Option<SceneId>)>,
    was_running: bool,
    /// When this session asked the run to Start (cleared once it is seen running).
    start_sent: Option<Instant>,
    /// The synchronous leave STOP was written (Drop does not write it again).
    left: std::sync::atomic::AtomicBool,
    /// Bumped when the section's structure changes (connection, leg, rows, enabled state).
    pub revision: u64,
    /// Bumped when the charts must be redrawn.
    pub samples_revision: u64,
}

impl LiveSync {
    pub fn new(target: Option<ServerTarget>, settings: &SyncSettings) -> Self {
        Self {
            target,
            saved: settings.clone(),
            connecting: None,
            client: None,
            config: None,
            thread: None,
            status_job: None,
            status_stale: false,
            next_poll: Instant::now(),
            generation: 0,
            leg: String::new(),
            amplitude: settings.amplitude.filter(|a| SCALES.iter().any(|(s, _)| s == a)).unwrap_or(SCALES[0].0),
            rows: Vec::new(),
            legs: Vec::new(),
            active: false,
            preparing: false,
            stopping: false,
            server_busy: false,
            polled: false,
            attempted: false,
            seq: 0,
            session_run: None,
            status: "Connecting…".into(),
            banner: INITIAL_BANNER.into(),
            readings: String::new(),
            samples: Vec::new(),
            stops: Vec::new(),
            stops_posted: 0,
            started_run: None,
            was_running: false,
            start_sent: None,
            left: std::sync::atomic::AtomicBool::new(false),
            revision: 1,
            samples_revision: 1,
        }
    }

    pub fn configured(&self) -> bool {
        self.target.is_some()
    }
    pub fn connected(&self) -> bool {
        self.config.is_some()
    }
    pub fn connecting(&self) -> bool {
        self.connecting.is_some()
    }
    pub fn active(&self) -> bool {
        self.active
    }
    /// Work in flight whose answer the panel waits for (keep frames coming).
    pub fn wants_frames(&self) -> bool {
        self.active || self.preparing || self.stopping || self.connecting.is_some() || !self.stops.is_empty()
    }
    /// Connect once, when the panel is first shown with a bench configured.
    pub fn auto_connect(&mut self) {
        if self.target.is_some() && !self.attempted {
            self.attempted = true;
            let _ = self.connect();
        }
    }
    /// A session is open or opening (the run must keep publishing targets).
    pub fn engaged(&self) -> bool {
        self.active || self.preparing
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    /// The banner text and whether motors are synced (OK) or not (WARN).
    pub fn banner(&self) -> (&str, bool) {
        (&self.banner, self.active)
    }
    pub fn readings(&self) -> &str {
        &self.readings
    }
    pub fn samples(&self) -> &[bench::LiveSample] {
        &self.samples
    }
    pub fn rows(&self) -> &[SyncBinding] {
        &self.rows
    }
    pub fn legs(&self) -> &[String] {
        &self.legs
    }
    pub fn leg(&self) -> &str {
        &self.leg
    }
    pub fn amplitude(&self) -> f64 {
        self.amplitude
    }
    pub fn motor_ids(&self) -> &[u8] {
        self.config.as_ref().map_or(&[], |c| c.ids.as_slice())
    }
    /// `hw-start` enabled: after the first poll, with the config, nothing running or stopping (:25).
    pub fn start_enabled(&self) -> bool {
        self.config.is_some() && self.polled && !self.server_busy && !self.active && !self.preparing && !self.stopping
    }
    /// The selects (leg, rows, scale) enabled (:25).
    pub fn selects_enabled(&self) -> bool {
        !(self.server_busy || self.active || self.preparing)
    }
    /// The preferences to persist (the page's `save()`).
    pub fn to_save(&self) -> SyncSettings {
        SyncSettings { leg: Some(self.leg.clone()), amplitude: Some(self.amplitude), bindings: self.rows.clone() }
    }

    /// For `hardware_status`.
    pub fn state_json(&self) -> Value {
        json!({"configured": self.configured(), "url": self.target.as_ref().map(|t| &t.url), "connected": self.connected(), "connecting": self.connecting(),
            "active": self.active, "preparing": self.preparing, "stopping": self.stopping, "server_busy": self.server_busy,
            "status": self.status, "banner": self.banner, "leg": self.leg, "amplitude": self.amplitude,
            "bindings": self.rows.iter().map(|b| json!({"coordinate": b.coordinate, "motor_id": b.motor_id, "polarity": b.polarity})).collect::<Vec<_>>(),
            "samples": self.samples.len(), "session_run": self.session_run, "readings": self.readings, "stops_posted": self.stops_posted})
    }

    /// `stateText(s)` (:16).
    fn state_text(&mut self, s: impl Into<String>) {
        let s = s.into();
        self.banner = banner_text(self.active, self.preparing, &s);
        self.status = s;
    }

    /// Start the connection (token from the bench's page or the token file, then `/config`).
    pub fn connect(&mut self) -> Result<(), String> {
        let Some(target) = self.target.clone() else { return Err(format!("No motor bench configured. {NO_BENCH}")) };
        if self.connecting.is_some() || self.config.is_some() {
            return Ok(());
        }
        self.attempted = true;
        self.generation += 1;
        self.state_text("Connecting…");
        self.connecting = Some(Job::spawn(Pool::Dedicated, self.generation, "motor-bench connect", move |_| {
            let client = token::connect(&target.url, target.token_file.as_deref(), ServerKind::MotorBench).map_err(|e| e.to_string())?;
            let config = client.get_as::<bench::Config>(bench::CONFIG).map_err(|e| e.to_string())?;
            Ok((client, config))
        }));
        Ok(())
    }

    /// `onFrame()` (:32): a new live frame was accepted.
    pub fn on_frame(&mut self, view: &RobotView, run: &mut dyn FnMut(RobotAction)) {
        self.seq += 1;
        match sample_from(live_input(view), self.seq) {
            Ok(sample) => {
                if self.engaged() {
                    if let Some(t) = self.thread.as_ref() {
                        let _ = t.send(SyncCommand::Latest(sample));
                    }
                }
            }
            Err(e) => {
                if self.active {
                    self.stop(&e);
                    run(RobotAction::Run { action: RunAction::Pause });
                }
            }
        }
    }

    /// The run's pause and reset rules (`setPlaying(false)` → `stop('Simulation paused')`, reset).
    pub fn watch_run(&mut self, view: &RobotView) {
        if self.engaged() {
            self.watch(run_seen(view), Instant::now());
        }
    }

    /// [`Self::watch_run`] on what was read from the run.
    fn watch(&mut self, seen: RunSeen, now: Instant) {
        if !self.engaged() {
            return;
        }
        let running = seen.phase == Some(Phase::Running);
        if running {
            self.start_sent = None;
        }
        if seen.key != self.started_run {
            self.stop("Simulation reset");
        } else if self.was_running && !running {
            self.stop("Simulation paused");
        } else if seen.phase == Some(Phase::Failed) {
            self.stop(RUN_FAILED);
        } else if let Err(e) = seen.live {
            // Not live any more without a new frame (a gait preview, a replay, the run ended).
            self.stop(&e);
        } else if self.start_sent.is_some_and(|at| now.saturating_duration_since(at) > START_GRACE) {
            self.stop(NOT_STARTED);
        }
        self.was_running = running;
    }

    /// `hw-start` (:23).
    pub fn start(&mut self, view: Option<&RobotView>, run: &mut dyn FnMut(RobotAction)) -> Result<(), String> {
        if self.server_busy || self.active || self.preparing || self.stopping {
            return Ok(());
        }
        if self.thread.is_none() || self.config.is_none() {
            return Err("the motor bench is not connected; connect first".into());
        }
        if view.is_some_and(|v| v.mirror.is_some()) {
            self.state_text(MIRROR_ON);
            return Ok(());
        }
        let input = view.map_or_else(StartInput::none, |v| StartInput::of(v, self.seq));
        self.start_with(input, run, Instant::now());
        Ok(())
    }

    /// The body of `hw-start` on what was read from the run: the page's
    /// checks (`currentSample()`, distinct motors), then the run's own Start
    /// check, all through the page's error path (status, pause, no session).
    fn start_with(&mut self, input: StartInput, run: &mut dyn FnMut(RobotAction), now: Instant) {
        let checked = input.initial.and_then(|initial| distinct(&self.rows).map(|()| initial)).and_then(|initial| input.start.map(|send| (initial, send)));
        let (initial, send_start) = match checked {
            Ok(i) => i,
            Err(e) => {
                self.preparing = false;
                self.active = false;
                self.state_text(e);
                run(RobotAction::Run { action: RunAction::Pause });
                return;
            }
        };
        let bindings: Vec<bench::Binding> = self.rows.iter().map(|b| bench::Binding { coordinate: b.coordinate.clone(), motor_id: b.motor_id, polarity: b.polarity }).collect();
        self.preparing = true;
        self.state_text("Starting bounded motor session…");
        self.started_run = input.key;
        self.was_running = input.running;
        self.start_sent = None;
        // A new session: leaving writes its STOP again.
        self.left.store(false, std::sync::atomic::Ordering::SeqCst);
        if send_start {
            self.start_sent = Some(now);
            run(RobotAction::Run { action: RunAction::Start });
        }
        let body = bench::open(&bindings, self.amplitude, &input.source, &initial);
        if let Some(t) = self.thread.as_ref() {
            let _ = t.send(SyncCommand::Open { body, last_sent: initial.sequence });
        }
    }

    /// A session is or may be open on the bench: ours (open, opening, stopping) or the bench busy.
    fn may_be_open(&self) -> bool {
        self.active || self.preparing || self.stopping || self.server_busy
    }
    /// A session this viewer opened (or is opening, or is stopping). The bench's
    /// own `active` is not enough: it is also set by another client's session
    /// (the browser's /walking/ page), which this viewer's focus loss, panel
    /// close or mode exit must not end.
    fn ours(&self) -> bool {
        self.active || self.preparing || self.stopping
    }

    /// [`Self::stop`] for a loss of control (focus loss, panel close, leaving):
    /// only a session this viewer opened. The operator's Stop motors and STOP
    /// use `stop`, which also ends a session the bench reports from elsewhere.
    pub fn stop_ours(&mut self, reason: &str) {
        if self.ours() {
            self.stop(reason);
        }
    }

    /// `stop(reason)` (:22): end the session; post `/stop` on its own job whenever
    /// a session is or may be open (an open in flight, one the bench still reports).
    pub fn stop(&mut self, reason: &str) {
        if !self.may_be_open() {
            return;
        }
        self.active = false;
        self.preparing = false;
        self.stopping = true;
        self.state_text(format!("{reason} — verifying physical stop…"));
        if let Some(t) = self.thread.as_ref() {
            let _ = t.send(SyncCommand::Deactivate);
        }
        self.queue_stop();
    }

    /// `/stop` on its own job, its answer applied by `poll`.
    fn queue_stop(&mut self) {
        if let Some(job) = self.post_stop() {
            self.stops.push(job);
            self.stops_posted += 1;
        }
    }

    fn post_stop(&self) -> Option<Job<Value>> {
        let client = self.client.clone()?.with_timeout(STOP_TIMEOUT);
        Some(Job::spawn(Pool::Dedicated, self.generation, "motor-bench stop", move |_| client.post(bench::STOP, &bench::stop()).map_err(|e| e.to_string())).complete_on_drop())
    }

    /// Applies finished work (connect, STOP answers, the thread's open,
    /// sample and poll results); returns true when the run must be paused.
    pub fn poll(&mut self) -> bool {
        let mut pause = false;
        if let Some(result) = self.connecting.as_ref().and_then(Job::poll) {
            self.connecting = None;
            match result {
                Ok((client, config)) => self.connected_with(client, config),
                Err(e) => self.state_text(e),
            }
        }
        let mut failures = Vec::new();
        self.stops.retain(|job| match job.poll() {
            None => true,
            Some(Ok(_)) => false,
            Some(Err(e)) => {
                failures.push(e);
                false
            }
        });
        for e in failures {
            self.state_text(format!("Stop request unavailable; FPGA watchdog remains independent. {e}"));
        }
        let Some((open, send_error)) = self.thread.as_ref().map(|t| {
            let mut s = t.lock();
            (s.open.take(), s.send_error.take())
        }) else {
            return pause;
        };
        if let Some(result) = open {
            // A status read sent before this answer must not be taken as the session's.
            self.status_stale = self.status_job.is_some();
            match result {
                Ok(()) if self.preparing => {
                    self.active = true;
                    self.preparing = false;
                    self.state_text("Syncing live WASD targets…");
                }
                // Stopped while opening (the bench ignored that /stop): never activate; stop it now.
                Ok(()) => {
                    if let Some(t) = self.thread.as_ref() {
                        let _ = t.send(SyncCommand::Deactivate);
                    }
                    self.stopping = true;
                    self.queue_stop();
                }
                Err(e) if self.preparing => {
                    self.preparing = false;
                    self.active = false;
                    self.state_text(e);
                    pause = true;
                }
                Err(_) => {}
            }
        }
        if let Some(e) = send_error {
            if self.active {
                self.state_text(e);
                self.stop("Reference transport failed");
            }
        }
        match self.poll_status() {
            None => {}
            Some(Ok(s)) => {
                self.polled = true;
                if self.server_busy != s.active {
                    self.server_busy = s.active;
                    self.revision += 1;
                }
                if self.active || self.stopping {
                    self.session_run = s.run.clone();
                    self.samples = s.samples;
                    self.samples_revision += 1;
                    if !self.samples.is_empty() {
                        self.readings = reading_lines(&self.samples, &self.rows);
                    }
                    if !s.active && !self.preparing {
                        // Completed or stopped elsewhere: the sync thread posts no further sample.
                        if let Some(t) = self.thread.as_ref() {
                            let _ = t.send(SyncCommand::Deactivate);
                        }
                        self.active = false;
                        self.stopping = false;
                        pause = true;
                        let run = self.session_run.clone().unwrap_or_else(|| "null".into());
                        self.state_text(format!("{} Saved: {run}", s.result.unwrap_or_default().summary()));
                    }
                }
            }
            Some(Err(e)) => {
                self.polled = true;
                if self.active || self.preparing {
                    self.stop("Bridge disconnected");
                    pause = true;
                } else {
                    self.state_text(e);
                }
            }
        }
        pause
    }

    /// The `/status` job: its answer (None while it runs or when stale), and
    /// the next one started `POLL_PERIOD` after it (one at a time).
    fn poll_status(&mut self) -> Option<Result<bench::Status, String>> {
        let mut answer = None;
        if let Some(result) = self.status_job.as_ref().and_then(Job::poll) {
            self.status_job = None;
            self.next_poll = Instant::now() + POLL_PERIOD;
            answer = (!std::mem::take(&mut self.status_stale)).then_some(result);
        }
        let client = self.client.clone().filter(|_| self.status_job.is_none() && Instant::now() >= self.next_poll);
        if let Some(client) = client {
            self.status_job = Some(Job::spawn(Pool::Dedicated, self.generation, "motor-bench status", move |_| client.get_as::<bench::Status>(bench::STATUS).map_err(|e| e.to_string())));
        }
        answer
    }

    /// `/config` answered (:30): legs, the saved leg and scale, the rows, the thread.
    fn connected_with(&mut self, client: Client, config: bench::Config) {
        self.legs = legs(&config.coordinates);
        self.leg = self.saved.leg.clone().filter(|l| self.legs.contains(l)).or_else(|| self.legs.first().cloned()).unwrap_or_default();
        self.rows = mapping(&config.coordinates, &config.ids, &self.leg, Some(self.saved.bindings.as_slice()));
        self.samples.clear();
        self.samples_revision += 1;
        let (generation, worker_client) = (self.generation, client.clone());
        let initial = SyncShared { generation, ..Default::default() };
        // Never another thread here (connect needs no config), but never join one on the UI thread.
        if let Some(old) = self.thread.take() {
            crate::jobs::drop_off_thread(old, "hardware-sync");
        }
        self.thread = Some(RunThread::spawn("hardware-sync", initial, move |rx, out| worker(worker_client, rx, out)));
        self.client = Some(client);
        self.config = Some(config);
        self.next_poll = Instant::now();
        self.state_text(READY);
    }

    fn editable(&self) -> Result<(), String> {
        if self.config.is_none() {
            return Err("the motor bench is not connected; connect first".into());
        }
        if !self.selects_enabled() {
            return Err("the motor mapping is locked while a motor session runs; stop it first".into());
        }
        Ok(())
    }
}

impl LiveSync {
    /// The window is closing (the page's `pagehide`, a keepalive fetch): when
    /// this viewer owns the bench session, the bench STOP is written at once
    /// on its own connection and never answered ([`Client::send_only`]). It
    /// blocks the calling thread for at most the client's 500 ms connect
    /// timeout plus a loopback write bounded by [`LEAVE_STOP_TIMEOUT`]; once
    /// per LiveSync (Drop does not write it again). No-op otherwise.
    pub fn post_stop_on_leave(&self) {
        if !self.ours() || self.left.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let Some(client) = self.client.as_ref() else { return };
        if let Err(e) = client.clone().with_timeout(LEAVE_STOP_TIMEOUT).send_only(bench::STOP, &bench::stop()) {
            // A later exit path (the drop at teardown) tries once more.
            self.left.store(false, std::sync::atomic::Ordering::SeqCst);
            bevy::log::warn!("motor bench STOP on leaving could not be written: {e} (the STOP job and the FPGA watchdog remain)");
        }
    }
}

impl Drop for LiveSync {
    /// Leaving Robot mode or the window closing: STOP written at once
    /// ([`LiveSync::post_stop_on_leave`]) and on a job; the sync thread (which
    /// may be mid-post) is released off the UI thread.
    fn drop(&mut self) {
        if self.ours() {
            self.post_stop_on_leave();
            drop(self.post_stop());
        }
        if let Some(thread) = self.thread.take() {
            crate::jobs::drop_off_thread(thread, "hardware-sync");
        }
    }
}

/// The run's identity for the reset rule: its generation and preset.
fn run_key(view: &RobotView) -> Option<(u64, Option<SceneId>)> {
    let run = view.run.as_ref()?;
    Some((run.generation(), run.preset().map(|p| SceneId::Preset(Arc::downgrade(p)))))
}

/// What `start` reads from the run.
struct StartInput {
    /// `currentSample()`.
    initial: Result<bench::Sample, String>,
    source: String,
    key: Option<(u64, Option<SceneId>)>,
    /// The run reports Running.
    running: bool,
    /// Whether to send Start (false when already running or requested), or why the run refuses it.
    start: Result<bool, String>,
}
impl StartInput {
    fn none() -> Self {
        Self { initial: Err(NOT_LIVE.into()), source: String::new(), key: None, running: false, start: Err(NOT_LIVE.into()) }
    }
    fn of(view: &RobotView, seq: u64) -> Self {
        // A planar v2 file has no live controller: refused by name, not as "reset the episode".
        if view.is_planar() {
            let why = crate::robot::planar::LIVE_SYNC;
            return Self { initial: Err(why.into()), source: String::new(), key: None, running: false, start: Err(why.into()) };
        }
        let r = view.run.as_ref();
        let running = r.is_some_and(|r| r.phase() == Phase::Running);
        // Pause is accepted exactly when Start was requested and still holds (a run starting to build counts).
        let requested = running || r.is_some_and(|r| r.check(RunAction::Pause).is_ok());
        let start = match r {
            None => Err(NOT_LIVE.into()),
            Some(_) if requested => Ok(false),
            Some(r) => r.check(RunAction::Start).map(|()| true),
        };
        Self { initial: sample_from(live_input(view), seq), source: source_text(view), key: run_key(view), running, start }
    }
}

/// What the stop rules read from the run each frame.
struct RunSeen {
    key: Option<(u64, Option<SceneId>)>,
    phase: Option<Phase>,
    /// The latest frame still counts as live input (`currentSample()` without a new sample).
    live: Result<(), String>,
}
fn run_seen(view: &RobotView) -> RunSeen {
    RunSeen { key: run_key(view), phase: view.run.as_ref().map(|r| r.phase()), live: sample_from(live_input(view), 0).map(|_| ()) }
}

#[cfg(test)]
mod tests;
