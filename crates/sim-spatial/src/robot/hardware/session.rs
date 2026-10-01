//! The page's session logic (`web/viewer/calibration-ui.mjs`), ported to one
//! blocking loop on the link thread ([`run`]). The page's closure variables
//! are [`Session`]'s fields (most of them the published [`LinkSnapshot`]);
//! each page function is a method with the page's line in its doc, the
//! page's texts and its order of effects. The sequenced handlers (sweep all,
//! tune, campaign, gait playback) are in [`sequences`].
//!
//! - **Serial, not concurrent.** The page's handlers, its status poll and its
//!   heartbeat interleave at `await`s; here they run one at a time, so the
//!   page's `busy`/`starting`/`heartbeatBusy` guards never see another
//!   handler mid-request (they are kept where the page checks them). Its
//!   `setTimeout`/`setInterval`/`sleep` loops are deadlines ([`Session::next_deadline`]).
//! - **Epoch.** Where the page captures `const e=epoch` and later checks
//!   `e===epoch`, the shared atomic is read; the UI bumps it for an
//!   immediate STOP ([`super::link::stop_now`]) and the session where the
//!   page does `++epoch`.
//! - **A STOP not yet applied.** Between the UI's bump and its
//!   [`LinkCommand::Stopped`] reaching this thread, commands queued before
//!   the STOP may still be handled. The page would have sent their requests
//!   before its STOP; here they would reach the server after it, so while the
//!   shared epoch is ahead of the last one this session saw ([`Session::interrupted`])
//!   no request but `stop` is sent ([`Session::send`] refuses with
//!   [`STOP_PENDING`]) and the heartbeats are skipped. Each `Stopped`
//!   carries the epoch its STOP bumped to, so with two STOPs pressed while
//!   this thread is busy, the first `Stopped` leaves the second pending
//!   (a command queued between them still sends nothing). The session's own
//!   bumps made while a STOP is pending ([`Session::own_bumps`]) count as
//!   applied once every earlier epoch is.
//! - **Gait lease.** Each `gait_update` is posted from its own
//!   `Pool::Dedicated` job ([`Session::gait_lease`]), at most one at a time,
//!   so the server's 1.5 s lease never waits behind a slow request here.
use super::actions::Direction;
use super::link::{GAIT_HEARTBEAT, HEARTBEAT, Inputs, Intent, LinkCommand, LinkSnapshot, POLL_ACTIVE, POLL_IDLE};
use crate::jobs::Job;
use serde_json::Value;
use sim_runtime::hardware_client::calibration::{self, Axis, Input, Status, SweepSample};
use sim_runtime::hardware_client::{Body, Client, STOP_TIMEOUT};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod buttons;
mod sequences;
#[cfg(test)]
mod tests;

const COMMAND: &str = "/calibration/command";
const STATUS: &str = "/calibration/status";
/// How often a sim-only gait's clock advances (the page's animation frames).
const GAIT_FRAME: Duration = Duration::from_millis(20);
/// Why a request was not sent: the UI posted STOP and its `Stopped` has not
/// reached the link thread yet.
pub(super) const STOP_PENDING: &str = "STOP was pressed; this request was not sent.";
/// How soon a gait lease update is retried when the previous one is still
/// in flight (a pause or speed change must not wait a whole lease period).
const LEASE_RETRY: Duration = Duration::from_millis(20);
/// The timeout of one gait lease update: well inside the server's 1.5 s
/// lease, so a hung one is given up in time for the next to renew it.
const LEASE_TIMEOUT: Duration = Duration::from_millis(1000);

/// The link thread's body: handles commands in arrival order and the page's
/// periodic work between them; when the channel closes, sends STOP if
/// [`super::link::drive_active`] (the page's `pagehide` → `loss()`, widened
/// as that function explains), publishes and returns.
pub(super) fn run(client: Client, generation: u64, sequence: Arc<AtomicU64>, epoch: Arc<AtomicU64>, rx: mpsc::Receiver<LinkCommand>, shared: Arc<Mutex<LinkSnapshot>>) {
    let mut session = Session::new(client, generation, sequence, epoch, shared);
    session.publish();
    loop {
        let wait = session.next_deadline().saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(command) => session.handle(command),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                session.shutdown();
                return;
            }
        }
        session.run_due(Instant::now());
    }
}

/// Sweep all in progress (the page's `sweepAllRun` and its loop's locals).
pub(super) struct SweepAll {
    /// Identifies this run (the page's `mine`).
    token: u64,
    /// `axes[k].role` of the calibration read when it started.
    roles: std::collections::BTreeMap<u8, String>,
    /// Half-cycle count per motor when first seen (the page's `start`).
    start: std::collections::BTreeMap<u8, u64>,
    /// The next 250 ms progress check.
    next: Instant,
}

pub(super) struct Session {
    client: Client,
    sequence: Arc<AtomicU64>,
    epoch: Arc<AtomicU64>,
    shared: Arc<Mutex<LinkSnapshot>>,
    /// The page's closure state, published as is.
    snap: LinkSnapshot,
    inputs: Inputs,
    /// Every epoch up to this one is applied (the session's own bumps, and
    /// each `Stopped`); the shared epoch ahead of it is a STOP pending.
    seen_epoch: u64,
    /// The session's own bumps above `seen_epoch` (made while a STOP was
    /// pending); `seen_epoch` moves over them once the STOPs before them are applied.
    own_bumps: BTreeSet<u64>,
    /// The gait lease update in flight (`gait_update`), if any.
    lease: Option<Job<()>>,
    sweep_all: Option<SweepAll>,
    sweep_all_runs: u64,
    /// The page's `gaitRun.last` (a sim-only gait's clock).
    gait_last: Instant,
    plays: u64,
    next_poll: Instant,
    next_heartbeat: Instant,
    next_lease: Instant,
    next_frame: Instant,
    next_tune: Instant,
    next_campaign: Instant,
}

impl Session {
    pub(super) fn new(client: Client, generation: u64, sequence: Arc<AtomicU64>, epoch: Arc<AtomicU64>, shared: Arc<Mutex<LinkSnapshot>>) -> Self {
        let now = Instant::now();
        let seen_epoch = epoch.load(SeqCst);
        Session {
            client,
            sequence,
            epoch,
            shared,
            snap: LinkSnapshot { generation, ..Default::default() },
            inputs: Inputs::default(),
            seen_epoch,
            own_bumps: BTreeSet::new(),
            lease: None,
            sweep_all: None,
            sweep_all_runs: 0,
            gait_last: now,
            plays: 0,
            // The page polls once at load.
            next_poll: now,
            next_heartbeat: now + HEARTBEAT,
            next_lease: now + GAIT_HEARTBEAT,
            next_frame: now + GAIT_FRAME,
            next_tune: now,
            next_campaign: now,
        }
    }

    // ---- plumbing ----

    /// Copies the page's state into the shared snapshot.
    pub(super) fn publish(&mut self) {
        self.snap.revision += 1;
        self.snap.sweep_all = self.sweep_all.is_some();
        *self.shared.lock().unwrap_or_else(|p| p.into_inner()) = self.snap.clone();
    }
    /// The page's `++sequence`.
    fn seq(&self) -> u64 {
        self.sequence.fetch_add(1, SeqCst) + 1
    }
    fn epoch_now(&self) -> u64 {
        self.epoch.load(SeqCst)
    }
    /// The page's `++epoch`. A STOP the UI posted and this session has not
    /// applied yet stays pending (`seen_epoch` is not advanced past it; this
    /// bump is applied once it is).
    fn bump_epoch(&mut self) -> u64 {
        let now = self.epoch.fetch_add(1, SeqCst) + 1;
        self.own_bumps.insert(now);
        self.settle_epoch();
        now
    }
    /// Moves `seen_epoch` over the session's own bumps that directly follow it.
    fn settle_epoch(&mut self) {
        while self.own_bumps.remove(&(self.seen_epoch + 1)) {
            self.seen_epoch += 1;
        }
        let seen = self.seen_epoch;
        self.own_bumps.retain(|&e| e > seen);
    }
    /// A `Stopped` arrived: the UI's STOP that bumped the epoch to `epoch`,
    /// and every one before it, is applied.
    fn stop_applied(&mut self, epoch: u64) {
        self.seen_epoch = self.seen_epoch.max(epoch);
        self.settle_epoch();
    }
    /// A request that succeeded but whose answer is dropped because the UI
    /// posted STOP while it was in flight: the server may have parsed it
    /// after that STOP (a `select` clears the stop latch; `motion_start`,
    /// `sweep_all`, `gait_start`, `tune` and `campaign` then energize), so
    /// stop again at once (`stop` is sent while interrupted). Not in the
    /// page, which can leave the motor driving. Its answer is not adopted
    /// (the UI's STOP answer and the next poll are); a failure is shown.
    fn stop_after_dropped(&mut self, id: u8) {
        let body = calibration::stop(Some(id), self.seq());
        if let Err(e) = self.client.clone().with_timeout(STOP_TIMEOUT).post(COMMAND, &body) {
            self.message(format!("STOP after a request answered during STOP failed: {e}"));
        }
    }
    /// The UI posted a STOP whose `Stopped` has not been handled yet.
    pub(super) fn interrupted(&self) -> bool {
        self.epoch_now() != self.seen_epoch
    }
    /// `api('command', body)`: refused, unsent, while a STOP is pending
    /// (except `stop` itself).
    fn send(&self, body: Body) -> Result<Value, String> {
        let is_stop = matches!(body.get("action"), Some(sim_runtime::hardware_client::Json::Value(Value::String(a))) if a == "stop");
        if !is_stop && self.interrupted() {
            return Err(STOP_PENDING.into());
        }
        self.client.post(COMMAND, &body).map_err(|e| e.to_string())
    }
    /// A command whose answer is the full status.
    fn send_status(&self, body: Body) -> Result<Status, String> {
        self.send(body).and_then(|v| serde_json::from_value(v).map_err(|e| format!("status: {e}")))
    }
    /// `api('status')`.
    fn get_status(&self) -> Result<Status, String> {
        self.client.get_as::<Status>(STATUS).map_err(|e| e.to_string())
    }
    /// `state = s` with a status read from the server.
    fn adopt(&mut self, status: Status) {
        self.snap.state = status;
        self.snap.read_at = Some(Instant::now());
    }
    fn message(&mut self, text: String) {
        self.snap.state.message = Some(text);
    }

    // ---- the page's helpers (:66-81) ----

    /// `axis()` :67.
    fn axis(&self) -> Axis {
        self.snap.id.and_then(|id| self.axis_of(id)).unwrap_or_default()
    }
    fn axis_of(&self, id: u8) -> Option<Axis> {
        self.snap.state.calibration.as_ref().and_then(|c| c.axes.get(&id)).cloned()
    }
    /// `speed()` :68.
    fn speed(&self) -> f64 {
        let maximum = self.snap.state.maximum_speed_counts_s.unwrap_or(500.0);
        5.0 * (maximum / 5.0).powf(self.inputs.speed_percent / 100.0)
    }
    fn drive_pwm(&self) -> u16 {
        (self.inputs.pwm_percent * 10.0).round().clamp(0.0, 1000.0) as u16
    }
    /// `$('pwm').reportValidity()`: 0–100 (the UI validates the field; a
    /// value outside it is refused here as the browser refuses it).
    fn pwm_valid(&self) -> bool {
        (0.0..=100.0).contains(&self.inputs.pwm_percent)
    }
    /// `input()` :69.
    fn input(&self) -> Input {
        Input {
            speed_counts_s: self.speed(),
            drive_pwm: self.drive_pwm(),
            motion: self.snap.intent.motion().into(),
            target_raw: self.snap.target_raw,
            hold_others: self.inputs.hold_others,
            drive_mode: self.inputs.drive_mode.wire().into(),
        }
    }
    /// `state.sweep?.motor_id===id ? state.sweep.latest : null` (render :83).
    fn latest(&self) -> Option<&SweepSample> {
        self.snap.state.sweep.as_ref().filter(|s| s.motor_id == self.snap.id).and_then(|s| s.latest.as_ref())
    }
    /// `outsidePose()` :75-81.
    fn outside_pose(&self) -> Option<&'static str> {
        let id = self.snap.id?;
        let a = self.axis();
        let p = self.snap.state.samples.get(&id)?.position();
        if a.coordinate_session.as_deref().is_some_and(|c| !c.is_empty()) && a.coordinate_session != self.snap.state.coordinate_session {
            return Some("reference");
        }
        let reverse = match (a.lower, a.upper) {
            (Some(lower), Some(upper)) => upper < lower,
            _ => a.reverse,
        };
        if let Some(upper) = a.upper.map(|u| u as f64)
            && (if reverse { p < upper } else { p > upper })
        {
            return Some("upper");
        }
        if let Some(lower) = a.lower.map(|l| l as f64)
            && (if reverse { p > lower } else { p < lower })
        {
            return Some("lower");
        }
        None
    }
    /// `clearInput()` :123 (keys and pointer are the UI's).
    fn clear_input(&mut self) {
        self.snap.intent = Intent::Hold;
    }

    /// The parts of `render()` that change state (:82-122): the warnings log
    /// :92-95, the leg gait's frame and end rule (`gaitFrame` :244,
    /// `renderGait` :228), and learning complete → hold :117; then publish.
    pub(super) fn render(&mut self) {
        self.merge_warnings();
        self.leg_frame();
        if self.snap.gait.as_ref().is_some_and(|g| g.leg && g.started) && self.snap.state.gait.as_ref().is_some_and(|g| !g.running) {
            self.end_gait();
        }
        if self.learning_complete() && self.snap.learning {
            self.snap.learning = false;
            self.snap.intent = Intent::Hold;
            self.update();
        }
        self.publish();
    }
    fn learning_complete(&self) -> bool {
        self.latest().and_then(|l| l.adaptation.as_ref()).is_some_and(|a| a.learning_complete)
    }
    /// :92-95: each fresh warning not equal to the newest is prepended; at most 6.
    fn merge_warnings(&mut self) {
        let mut fresh: Vec<String> = self.latest().map(|l| l.warnings.clone()).unwrap_or_default();
        if let Some(sweep) = &self.snap.state.sweep {
            fresh.extend(sweep.axes.values().flat_map(|a| a.warnings.iter().cloned()));
        }
        if fresh.is_empty() {
            return;
        }
        let at = time_of_day();
        for w in fresh {
            if self.snap.warnings.first().map(|(_, text)| text) != Some(&w) {
                self.snap.warnings.insert(0, (at.clone(), w));
            }
        }
        self.snap.warnings.truncate(6);
    }

    // ---- commands ----

    pub(super) fn handle(&mut self, command: LinkCommand) {
        match command {
            LinkCommand::Inputs(inputs) => {
                // Values sent before the UI applied the latest speed reset
                // keep the session's speed (0 since the reset).
                let speed = self.inputs.speed_percent;
                let stale = inputs.speed_reset < self.snap.speed_reset;
                self.inputs = inputs;
                if stale {
                    self.inputs.speed_percent = speed;
                }
            }
            // The motor chips :157.
            LinkCommand::Select { id } => {
                if self.sweep_all.is_some() {
                    self.snap.sequence_text = "Sweep-all stopped: another motor was chosen.".into();
                }
                self.select_motor(id, false, false);
            }
            LinkCommand::SetDisabled => self.set_disabled(),
            LinkCommand::SweepAll => self.sweep_all(),
            LinkCommand::Press { direction } => self.move_(direction),
            LinkCommand::Release => self.release(),
            // Q and A both held :294.
            LinkCommand::BothKeys => {
                if self.snap.ready {
                    self.snap.intent = Intent::Hold;
                    self.update();
                    self.render();
                }
            }
            // :297.
            LinkCommand::SpeedChanged => {
                self.update();
                self.render();
            }
            LinkCommand::PwmChanged => {
                if self.pwm_valid() {
                    self.update();
                }
            }
            LinkCommand::Target { fraction } => self.target(fraction),
            // :300.
            LinkCommand::TargetCommit => self.update(),
            LinkCommand::Capture { boundary, reference_joint_rad } => self.capture(boundary, reference_joint_rad),
            // :306.
            LinkCommand::ResetPoses => {
                let boundary = match self.outside_pose() {
                    Some("reference") | None => "both",
                    Some(b) => b,
                };
                self.reset(boundary);
            }
            LinkCommand::Clear { boundary } => self.reset(boundary.name()),
            LinkCommand::Flip => self.flip(),
            LinkCommand::Sweep => self.sweep(),
            LinkCommand::Learn => self.learn(),
            LinkCommand::RawStep { delta } => self.raw_step(delta),
            LinkCommand::Tune => self.tune(),
            LinkCommand::Campaign { resume } => self.campaign(resume),
            LinkCommand::LoadGaits => self.load_gaits(),
            LinkCommand::GaitPlay { entry, mode, bindings, skipped } => self.gait_play(entry, mode, bindings, skipped),
            LinkCommand::GaitToggle => self.gait_toggle(),
            LinkCommand::GaitStop => self.gait_stop(),
            LinkCommand::GaitScale => self.gait_scale(),
            LinkCommand::Stopped { epoch } => {
                self.stopped_locally();
                self.stop_applied(epoch);
                self.render();
            }
            LinkCommand::StopAnswered(answer) => {
                match answer.and_then(|v| serde_json::from_value::<Status>(v).map_err(|e| format!("status: {e}"))) {
                    Ok(status) => self.adopt(status),
                    Err(e) => self.message(e),
                }
                self.render();
            }
            // `loss()` :321, widened ([`super::link::drive_active`]).
            LinkCommand::Loss => {
                if self.drive_active() {
                    self.stop();
                }
            }
        }
    }

    /// The local half of `stop()` :125: a leg gait ends, sweep all ends, the
    /// session and intent are cleared and nothing is ready.
    fn stopped_locally(&mut self) {
        if self.snap.gait.as_ref().is_some_and(|g| g.leg) {
            self.end_gait();
        }
        self.sweep_all = None;
        self.clear_input();
        self.snap.run = None;
        self.snap.starting = false;
        self.snap.sweeping = false;
        self.snap.learning = false;
        self.snap.ready = false;
    }
    /// `stop()` :125, sending its own request.
    pub(super) fn stop(&mut self) {
        self.stopped_locally();
        self.bump_epoch();
        self.render();
        let Some(id) = self.snap.id else { return };
        match self.send_status(calibration::stop(Some(id), self.seq())) {
            Ok(status) => self.adopt(status),
            Err(e) => self.message(e),
        }
        self.render();
    }
    /// `selectMotor(next, holdAll, solo)` :126-132.
    pub(super) fn select_motor(&mut self, next: u8, hold_all: bool, solo: bool) {
        if self.axis_of(next).is_some_and(|a| a.disabled) {
            if self.snap.busy {
                return;
            }
            self.stop();
            self.snap.id = Some(next);
            self.render();
            return;
        }
        if self.snap.busy {
            return;
        }
        let e = self.bump_epoch();
        self.clear_input();
        self.snap.run = None;
        self.snap.starting = false;
        self.snap.sweeping = false;
        self.snap.learning = false;
        self.snap.ready = false;
        self.snap.busy = true;
        self.snap.id = Some(next);
        self.render();
        let hold_others = !solo && (hold_all || self.inputs.hold_others);
        match self.send_status(calibration::select(next, self.seq(), hold_others)) {
            Ok(status) => {
                if e == self.epoch_now() {
                    self.adopt(status);
                    self.snap.ready = true;
                } else {
                    self.stop_after_dropped(next);
                }
            }
            Err(err) => {
                if e == self.epoch_now() {
                    self.message(err);
                }
            }
        }
        self.snap.busy = false;
        self.render();
    }
    /// `update()` :133-145: the heartbeat, and every intent change.
    pub(super) fn update(&mut self) {
        let (Some(r), Some(id)) = (self.snap.run, self.snap.id) else { return };
        // A pending STOP ends the session; don't answer it with a heartbeat failure.
        if !self.snap.ready || self.interrupted() {
            return;
        }
        let e = self.epoch_now();
        let input = self.input();
        let Err(err) = self.send(calibration::motion_update(id, self.seq(), r, &input)) else { return };
        if e != self.epoch_now() || Some(r) != self.snap.run {
            return;
        }
        // A session that ended on the server (finished sweep, fault already
        // stopped and verified) is not a heartbeat failure: adopt its final state.
        if let Ok(s) = self.get_status()
            && s.sweep.as_ref().is_some_and(|w| w.run_id == Some(r) && !w.running)
        {
            self.adopt(s);
            self.snap.run = None;
            self.snap.ready = false;
            self.clear_input();
            self.snap.sweeping = false;
            self.render();
            return;
        }
        self.stop();
        self.message(err);
        self.render();
    }
    /// `begin()` :146-154.
    fn begin(&mut self) {
        if !self.snap.ready || self.snap.busy || self.snap.starting {
            return;
        }
        if self.snap.run.is_some() {
            self.update();
            return;
        }
        if !self.pwm_valid() {
            return;
        }
        let Some(id) = self.snap.id else { return };
        let e = self.epoch_now();
        self.snap.starting = true;
        self.publish();
        let input = self.input();
        match self.send_status(calibration::motion_start(id, self.seq(), &input)) {
            Ok(s) => {
                if e == self.epoch_now() && self.snap.ready {
                    self.snap.run = s.sweep.as_ref().and_then(|w| w.run_id);
                    self.adopt(s);
                    self.update();
                } else if e != self.epoch_now() {
                    self.stop_after_dropped(id);
                }
            }
            Err(err) => {
                if e == self.epoch_now() {
                    self.snap.ready = false;
                    self.message(err);
                }
            }
        }
        if e == self.epoch_now() {
            self.snap.starting = false;
        }
        self.render();
    }
    /// `move(direction)` :155.
    fn move_(&mut self, direction: Direction) {
        if !self.snap.ready || self.snap.busy {
            return;
        }
        self.snap.intent = match direction {
            Direction::Upper => Intent::Upper,
            Direction::Lower => Intent::Lower,
        };
        self.snap.sweeping = false;
        self.snap.learning = false;
        self.begin();
        self.render();
    }
    /// `release()` :156.
    fn release(&mut self) {
        if matches!(self.snap.intent, Intent::Upper | Intent::Lower) {
            self.snap.intent = Intent::Hold;
            self.update();
            self.render();
        }
    }
    /// "Disable/Enable this motor" :158-162.
    fn set_disabled(&mut self) {
        let Some(id) = self.snap.id else { return };
        if self.snap.busy || self.sweep_all.is_some() {
            return;
        }
        let off = !self.axis().disabled;
        if off {
            self.stop();
        }
        match self.send_status(calibration::set_disabled(id, self.seq(), off)) {
            Ok(s) => self.adopt(s),
            Err(e) => self.message(e),
        }
        self.render();
    }

    // ---- periodic work ----

    /// The status poll :326, rescheduled after each answer.
    fn poll(&mut self) {
        let e = self.epoch_now();
        match self.get_status() {
            Ok(s) => {
                if e == self.epoch_now() && !self.snap.busy && !self.snap.starting {
                    let (sweep_run, sweep_running, holding) =
                        s.sweep.as_ref().map_or((None, false, false), |w| (w.run_id, w.running, w.latest.as_ref().is_some_and(|l| l.holding)));
                    let enabled = s.enabled_id;
                    self.adopt(s);
                    if enabled != self.snap.id && self.snap.run.is_none() {
                        self.snap.ready = false;
                    }
                    if self.snap.run.is_some() && sweep_run == self.snap.run && self.snap.intent == Intent::Target && holding {
                        self.snap.intent = Intent::Hold;
                        self.update();
                    }
                    if self.snap.run.is_some() && sweep_run == self.snap.run && !sweep_running {
                        self.snap.run = None;
                        self.snap.ready = false;
                        self.clear_input();
                        self.snap.sweeping = false;
                    }
                    self.render();
                }
            }
            Err(err) => {
                if self.snap.ready {
                    self.stop();
                }
                self.message(err);
                self.render();
            }
        }
        let active = self.snap.run.is_some() || self.leg_gait();
        self.next_poll = Instant::now() + if active { POLL_ACTIVE } else { POLL_IDLE };
    }
    fn leg_gait(&self) -> bool {
        self.snap.gait.as_ref().is_some_and(|g| g.leg)
    }
    fn sim_gait_playing(&self) -> bool {
        self.snap.gait.as_ref().is_some_and(|g| !g.leg && g.playing)
    }
    /// The earliest periodic work due.
    pub(super) fn next_deadline(&self) -> Instant {
        let mut due = self.next_poll;
        if self.snap.run.is_some() && self.snap.ready {
            due = due.min(self.next_heartbeat);
        }
        if self.leg_gait() {
            due = due.min(self.next_lease);
        }
        if let Some(run) = &self.sweep_all {
            due = due.min(run.next);
        }
        if self.snap.tuning {
            due = due.min(self.next_tune);
        }
        if self.snap.campaigning {
            due = due.min(self.next_campaign);
        }
        if self.sim_gait_playing() {
            due = due.min(self.next_frame);
        }
        due
    }
    /// Runs the periodic work due at `now`.
    pub(super) fn run_due(&mut self, now: Instant) {
        if now >= self.next_poll {
            self.poll();
        }
        // `heartbeat()` :325: `update()` every 100 ms (it returns at once without a session).
        if now >= self.next_heartbeat {
            self.update();
            self.next_heartbeat = Instant::now() + HEARTBEAT;
        }
        if self.leg_gait() && now >= self.next_lease {
            self.next_lease = Instant::now() + GAIT_HEARTBEAT;
            // Skipped while a STOP is pending; retried sooner while the
            // previous update is in flight.
            self.gait_lease();
        }
        if self.sweep_all.as_ref().is_some_and(|r| now >= r.next) {
            self.sweep_all_tick();
        }
        if self.snap.tuning && now >= self.next_tune {
            self.tune_tick();
        }
        if self.snap.campaigning && now >= self.next_campaign {
            self.campaign_tick();
        }
        if self.sim_gait_playing() && now >= self.next_frame {
            self.sim_frame();
            self.publish();
            self.next_frame = Instant::now() + GAIT_FRAME;
        }
    }
    /// [`super::link::drive_active`] on this session's state.
    fn drive_active(&mut self) -> bool {
        self.snap.sweep_all = self.sweep_all.is_some();
        super::link::drive_active(&self.snap)
    }
    /// The channel closed (the link dropped): the page's `loss()` on
    /// `pagehide`, widened ([`super::link::drive_active`]), with the short
    /// STOP timeout so the thread ends promptly. Sent also while a STOP is
    /// pending (the UI's STOP may not have reached the server).
    fn shutdown(&mut self) {
        let active = self.drive_active();
        if let Some(id) = self.snap.id
            && active
        {
            let body = calibration::stop(Some(id), self.seq());
            match self.client.clone().with_timeout(STOP_TIMEOUT).post(COMMAND, &body) {
                Ok(v) => match serde_json::from_value::<Status>(v) {
                    Ok(s) => self.adopt(s),
                    Err(e) => self.message(format!("status: {e}")),
                },
                Err(e) => self.message(e.to_string()),
            }
        }
        self.stopped_locally();
        self.publish();
    }
}

/// `new Date().toLocaleTimeString()` for the warnings log. The viewer has no
/// timezone database, so this is the UTC time of day, labelled as such.
fn time_of_day() -> String {
    let s = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) % 86_400;
    format!("{:02}:{:02}:{:02} UTC", s / 3600, s / 60 % 60, s % 60)
}
