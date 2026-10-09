//! The page's session logic (`web/viewer/calibration-ui.mjs`), ported to one
//! blocking loop on the link thread ([`run`]). The page's closure variables
//! are [`Session`]'s fields (most of them the published [`LinkSnapshot`]);
//! each page function is a method with the page's line in its doc, the
//! page's texts and its order of effects. The sequenced handlers (sweep all,
//! tune, campaign, gait playback) are in [`sequences`].
//!
//! - **Serial, not concurrent.** The page's handlers and its status poll
//!   interleave at `await`s; here they run one at a time, so the page's
//!   `busy`/`starting` guards never see another handler mid-request (they
//!   are kept where the page checks them). Its `setTimeout`/`setInterval`/
//!   `sleep` loops are deadlines ([`Session::next_deadline`]). The two
//!   heartbeats are the exception: they run concurrently, on the beat (below).
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
//!   applied once every earlier epoch is. A remote command
//!   ([`LinkCommand::Checked`]) is refused only for a STOP pressed after it
//!   was queued ([`Session::stopped_since`]), never for the session's own bumps.
//! - **Binding loss.** A failed request revokes a pinned virtual session's
//!   automation (until an explicit reconnect) only when it means the
//!   connection or its execution binding is gone
//!   ([`calibration::binding_lost`]: transport, decode, or the server's 409);
//!   an ordinary refusal (400, "Unknown motor ID") is shown and leaves the binding intact.
//! - **Heartbeats off this thread.** The `motion_update` heartbeat and the
//!   leg gait's lease `gait_update` are posted by the beat ([`beat`], one
//!   `jobs::RunThread`, "hardware-beat"), so a request here that waits up to
//!   8 s for the hardware never lets the server's 1.5 s motion or gait lease
//!   lapse. The session tells it what to carry ([`Session::sync_beat`], on
//!   every publish and command) and asks for an immediate `motion_update`
//!   where the page awaits `update()` ([`Session::update`]); `capture_hold`,
//!   checked against the same per-session sequence, goes through the beat
//!   too, so that domain has one sender. The beat sends nothing while a STOP
//!   is pending, and its periodic failures are acted on here
//!   ([`Session::beat_failure`]) only if their run and epoch still hold.
use super::actions::{Direction, GaitMode};
use super::link::{Inputs, Intent, LinkCommand, LinkSnapshot, POLL_IDLE};
use serde_json::Value;
use sim_runtime::hardware::protocol::calibration::{self, Axis, Input, Status, SweepSample};
use super::local::{Body, CONNECT_TIMEOUT, Client, ClientError, STOP_TIMEOUT};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod beat;
mod buttons;
mod commands;
mod motion;
mod periodic;
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
/// The start of a remote command's error when the change it asked for (a
/// pose, a reset, enable/disable, a speed or PWM ceiling) was applied and
/// saved, but a STOP was pressed before it finished: the caller knows the
/// change holds although motion was stopped.
pub(in crate::robot::hardware) const APPLIED_THEN_STOPPED: &str = "applied, but STOP was pressed afterwards";
/// Why a request through the beat failed: the beat ended, or did not answer
/// within [`Session::beat_wait`] (a heartbeat failure: the session stops).
const BEAT_GONE: &str = "The heartbeat sender did not answer.";
/// Why a pinned virtual session sends nothing but `stop` any more.
const REVOKED: &str = "virtual calibration authorization revoked; reconnect required";
/// Why a pinned virtual session's server no longer reports its execution (its bench was lost).
const BENCH_LOST: &str = "the virtual calibration bench was lost (disconnected); reconnect required";
/// Why a pinned virtual session's server reports another execution.
const IDENTITY_CHANGED: &str = "virtual execution identity changed; reconnect required";
/// Margin on a request's own deadline before the UI may take its wait for a lost connection.
const AWAIT_MARGIN: Duration = Duration::from_secs(1);

/// The link thread's body: handles commands in arrival order and the page's
/// periodic work between them; when the channel closes, sends STOP if
/// [`super::link::drive_active`] (the page's `pagehide` → `loss()`, widened
/// as that function explains), publishes and returns.
pub(super) fn run(client: Client, generation: u64, sequence: Arc<AtomicU64>, epoch: Arc<AtomicU64>, authorization: Arc<std::sync::atomic::AtomicBool>, rx: mpsc::Receiver<LinkCommand>, shared: Arc<Mutex<LinkSnapshot>>) {
    let mut session = Session::new(client, generation, sequence, epoch, shared);
    session.authorization = authorization;
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
    /// The heartbeat sender ([`beat`]); None once [`Session::shutdown`] dropped it.
    beat: Option<beat::Handle>,
    /// The plan last sent to the beat.
    plan: beat::Plan,
    /// How long [`Session::update`] waits for the beat's answer: a periodic
    /// send in flight and then this one, each within the client's timeout.
    beat_wait: Duration,
    sweep_all: Option<SweepAll>,
    sweep_all_runs: u64,
    /// The page's `gaitRun.last` (a sim-only gait's clock).
    gait_last: Instant,
    plays: u64,
    next_poll: Instant,
    next_frame: Instant,
    next_tune: Instant,
    next_campaign: Instant,
    /// The first reason the command being handled failed or declined
    /// ([`Session::message`], [`Session::decline`]); a [`LinkCommand::Checked`]
    /// reports it when the command did not achieve its purpose.
    command_error: Option<String>,
    /// The UI's sticky revocation ([`super::link::Link::authorization`]).
    authorization: Arc<std::sync::atomic::AtomicBool>,
    /// The highest epoch a UI STOP bumped to whose `Stopped` was handled.
    ui_stop_epoch: u64,
    /// [`LinkSnapshot::disconnected`] was set by [`Session::lose_binding`]
    /// (a request found the connection or binding gone), not by a status.
    lost_binding: bool,
}

impl Session {
    pub(super) fn new(client: Client, generation: u64, sequence: Arc<AtomicU64>, epoch: Arc<AtomicU64>, shared: Arc<Mutex<LinkSnapshot>>) -> Self {
        let now = Instant::now();
        let seen_epoch = epoch.load(SeqCst);
        let execution = client.calibration_execution.as_ref().map(|(identity, _)| identity.clone());
        let beat = beat::spawn(client.clone(), sequence.clone(), epoch.clone());
        // The beat may be finishing a periodic request (a heartbeat, or a
        // gait lease) before it sends this one: each costs up to the connect
        // timeout plus its read deadline. A margin on top, so a slow but
        // working server is never taken for a dead beat (that would stop
        // the session).
        let beat_wait = 2 * (client.timeout + CONNECT_TIMEOUT) + beat::LEASE_TIMEOUT + CONNECT_TIMEOUT + Duration::from_secs(1);
        let mut session = Session {
            client,
            sequence,
            epoch,
            shared,
            snap: LinkSnapshot { generation, execution, ..Default::default() },
            inputs: Inputs::default(),
            seen_epoch,
            own_bumps: BTreeSet::new(),
            beat: Some(beat),
            plan: beat::Plan::default(),
            beat_wait,
            sweep_all: None,
            sweep_all_runs: 0,
            gait_last: now,
            plays: 0,
            // The page polls once at load.
            next_poll: now,
            next_frame: now + GAIT_FRAME,
            next_tune: now,
            next_campaign: now,
            command_error: None,
            authorization: Arc::default(),
            ui_stop_epoch: 0,
            lost_binding: false,
        };
        session.sync_beat();
        session
    }

    // ---- plumbing ----

    /// Copies the page's state into the shared snapshot.
    pub(super) fn publish(&mut self) {
        self.snap.authorization_revoked |= self.authorization.load(SeqCst);
        self.snap.revision += 1;
        self.snap.sweep_all = self.sweep_all.is_some();
        *self.shared.lock().unwrap_or_else(|p| p.into_inner()) = self.snap.clone();
        self.sync_beat();
    }
    /// What the heartbeats carry now: the motion session (`update()`'s
    /// `run!=null&&ready`, on the chosen motor, with `input()`), the leg
    /// gait's scale and pause state, and the epochs applied.
    fn beat_plan(&self) -> beat::Plan {
        if self.snap.execution.is_some() && (self.snap.authorization_revoked || self.authorization.load(SeqCst)) {
            return beat::Plan { seen_epoch: self.seen_epoch, ..Default::default() };
        }
        let motion = match (self.snap.run, self.snap.id) {
            (Some(run), Some(id)) if self.snap.ready => Some(beat::MotionBeat { id, run, input: self.input() }),
            _ => None,
        };
        let gait = self.snap.gait.as_ref().filter(|g| g.leg).map(|g| beat::GaitBeat { scale: g.scale, playing: g.playing });
        beat::Plan { seen_epoch: self.seen_epoch, motion, gait }
    }
    /// Sends the beat the current plan if it changed.
    pub(super) fn sync_beat(&mut self) {
        let plan = self.beat_plan();
        if plan == self.plan {
            return;
        }
        if let Some(beat) = &self.beat {
            let _ = beat.send(beat::Beat::Plan(plan.clone()));
        }
        self.plan = plan;
    }
    /// The page's `await send('motion_update', …)` in `update()`: the beat
    /// sends it at once with the current plan, in order with its periodic
    /// ones; `Ok` also when it sent nothing (a STOP posted meanwhile). A
    /// beat that is gone or does not answer within [`Session::beat_wait`]
    /// is a heartbeat failure.
    fn beat_now(&mut self) -> Result<(), beat::Refused> {
        self.sync_beat();
        let Some(beat) = &self.beat else { return Ok(()) };
        let (tx, rx) = mpsc::channel();
        if beat.send(beat::Beat::Now(tx)).is_err() {
            return Err(beat::Refused::local(BEAT_GONE));
        }
        self.set_awaiting(Some(self.beat_wait));
        let answer = rx.recv_timeout(self.beat_wait).unwrap_or_else(|_| Err(beat::Refused::local(BEAT_GONE)));
        self.answered(answer.is_ok());
        answer
    }
    /// `api('command', body)` for a request the server checks against the
    /// motion session's sequence (`capture_hold`): posted by the beat, so it
    /// cannot overtake or be overtaken by a heartbeat. Refused, unsent,
    /// while a STOP is pending (here, and by the beat if one is posted meanwhile).
    fn send_in_session(&mut self, build: beat::Build) -> Result<Value, String> {
        if self.interrupted() {
            return Err(STOP_PENDING.into());
        }
        if self.revoked() {
            return Err(REVOKED.into());
        }
        self.sync_beat();
        let Some(beat) = &self.beat else { return Err(BEAT_GONE.into()) };
        let (tx, rx) = mpsc::channel();
        if beat.send(beat::Beat::Post(build, tx)).is_err() {
            return Err(BEAT_GONE.into());
        }
        self.set_awaiting(Some(self.beat_wait));
        let answer = rx.recv_timeout(self.beat_wait).unwrap_or_else(|_| Err(beat::Refused::local(BEAT_GONE)));
        self.answered(answer.is_ok());
        answer.map_err(|refused| {
            if refused.binding_lost {
                self.lose_binding();
            }
            refused.error
        })
    }
    /// A periodic heartbeat the server refused: the same handling as a
    /// failed `update()`, if its run and epoch still hold.
    fn beat_failure(&mut self) {
        let failure = self.beat.as_ref().and_then(|b| b.lock().take());
        if let Some(f) = failure {
            self.motion_failed(f.run, f.epoch, f.refused);
        }
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
        self.decline(STOP_PENDING);
        let body = calibration::stop(Some(id), self.seq());
        if let Err(e) = self.client.clone().with_timeout(STOP_TIMEOUT).post(COMMAND, &body) {
            self.message(format!("STOP after a request answered during STOP failed: {e}"));
        }
    }
    /// The UI posted a STOP whose `Stopped` has not been handled yet.
    pub(super) fn interrupted(&self) -> bool {
        self.epoch_now() != self.seen_epoch
    }
    /// A UI STOP was pressed after the shared epoch was `epoch`: one still
    /// pending, or one already applied that bumped past `epoch`. The
    /// session's own bumps (select, the `stop()` inside tune, campaign,
    /// clear and flip, a failed heartbeat) never count: they settle into
    /// `seen_epoch` and never raise [`Session::ui_stop_epoch`].
    fn stopped_since(&self, epoch: u64) -> bool {
        self.interrupted() || self.ui_stop_epoch > epoch
    }
    /// A pinned virtual session whose automation was revoked (here, or by the UI).
    fn revoked(&self) -> bool {
        self.snap.execution.is_some() && (self.snap.authorization_revoked || self.authorization.load(SeqCst))
    }
    /// The connection or its execution binding is gone: a pinned virtual
    /// session's automation is revoked until an explicit reconnect, and the
    /// link is marked disconnected ([`LinkSnapshot::disconnected`]; a reason
    /// already there, more specific, is kept).
    fn lose_binding(&mut self) {
        self.snap.authorization_revoked |= self.snap.execution.is_some();
        self.snap.connection_valid = false;
        if self.snap.disconnected.is_none() {
            self.snap.disconnected = Some("the connection or its execution binding was lost or refused".into());
            self.lost_binding = true;
        }
    }
    /// Publishes, without a full publish, that this thread waits up to
    /// `wait` for an answer (None: no longer), so the UI does not take the
    /// status's age during a slow request for a lost connection
    /// ([`LinkSnapshot::awaiting_answer`]).
    fn set_awaiting(&mut self, wait: Option<Duration>) {
        let awaiting = wait.map(|w| (Instant::now(), w + AWAIT_MARGIN));
        self.snap.awaiting = awaiting;
        self.shared.lock().unwrap_or_else(|p| p.into_inner()).awaiting = awaiting;
    }
    /// A request this thread waited on has answered. A success may not
    /// refresh the status (`capture_hold`, `motion_update` answers are not
    /// adopted), so the wait extends by one idle status-poll period
    /// ([`POLL_IDLE`], the poll runs right after the command) before the
    /// status's age may count as lost. A failure ends it at once: a lost
    /// binding has revoked already, and a status that keeps failing must go
    /// stale.
    fn answered(&mut self, ok: bool) {
        self.set_awaiting(ok.then_some(POLL_IDLE));
    }
    /// One request on this thread, marked as awaited for the client's deadline.
    fn request<T>(&mut self, request: impl FnOnce(&Client) -> Result<T, ClientError>) -> Result<T, ClientError> {
        self.set_awaiting(Some(self.client.timeout + CONNECT_TIMEOUT));
        let result = request(&self.client);
        self.answered(result.is_ok());
        result
    }
    /// `api('command', body)`: refused, unsent, while a STOP is pending
    /// (except `stop` itself), and for a revoked virtual session. A failure
    /// that means the binding is gone ([`calibration::binding_lost`])
    /// revokes; an ordinary refusal is only returned.
    fn send(&mut self, body: Body) -> Result<Value, String> {
        let stop_epoch=self.client.calibration_stop_epoch();
        let is_stop = matches!(body.get("action"), Some(sim_runtime::hardware::protocol::Json::Value(Value::String(a))) if a == "stop");
        if !is_stop && self.interrupted() {
            return Err(STOP_PENDING.into());
        }
        if !is_stop && self.revoked() {
            return Err(REVOKED.into());
        }
        self.request(|client| client.post_at_epoch(COMMAND, &body, stop_epoch)).map_err(|e| {
            if calibration::binding_lost(&e) {
                self.lose_binding();
            }
            e.to_string()
        })
    }
    /// A command whose answer is the full status.
    fn send_status(&mut self, body: Body) -> Result<Status, String> {
        self.send(body).and_then(|v| serde_json::from_value(v).map_err(|e| format!("status: {e}")))
    }
    /// `api('status')`; the caller decides what a failure means.
    fn get_status(&mut self) -> Result<Status, ClientError> {
        self.request(|client| client.get_as::<Status>(STATUS))
    }
    /// `state = s` with a status read from the server.
    ///
    /// Also keeps [`LinkSnapshot::disconnected`]: set when this status
    /// reports the bus not connected after the previous adopted one reported
    /// it connected (a STOP that lost readback, a lost bench; with the
    /// server's message), when a pinned virtual execution is gone or
    /// replaced; cleared by a status that reports the bus connected, except
    /// for a pinned virtual session whose binding was revoked (it stays
    /// marked until an explicit reconnect). A server whose bus never
    /// connected (a physical server before its first inspect) is not
    /// disconnected; a status read after a lost request
    /// ([`Session::lose_binding`]) clears that request's mark unless the
    /// binding was revoked. Ends with [`Session::leg_frame`], so the leg
    /// gait clock's base (`GaitRun::t`) and `read_at` change together.
    fn adopt(&mut self, status: Status) {
        let was_connected = self.snap.read_at.is_some() && self.snap.state.connected;
        let lost_binding = std::mem::take(&mut self.lost_binding);
        if self.snap.execution.is_some() && self.snap.execution != status.execution {
            self.snap.authorization_revoked = true;
            self.snap.connection_valid = false;
            // No execution any more: the server lost its virtual bench (its
            // `lose_bus`); another one: the server or bench was replaced.
            // Either revokes the same way.
            let why = if status.execution.is_none() { BENCH_LOST } else { IDENTITY_CHANGED };
            // Every later poll finds a mismatch: the stop below is sent once,
            // not again every 600 ms (nor when a lost bench comes back as
            // another instance, which this link never drove). Only the
            // reason shown follows the server.
            if self.snap.disconnected.as_deref().is_some_and(|d| d == BENCH_LOST || d == IDENTITY_CHANGED) {
                if self.snap.disconnected.as_deref() != Some(why) {
                    self.snap.disconnected = Some(why.into());
                    self.snap.state.message = Some(why.into());
                }
                return;
            }
            self.snap.disconnected = Some(why.into());
            self.message(why.into());
            self.bump_epoch();
            self.stopped_locally();
            // Our motor ids mean nothing on another instance: id-less, and
            // only if this link drove (else it could end another client's session).
            if self.snap.drove {
                let _ = self.client.clone().with_timeout(STOP_TIMEOUT).post(COMMAND, &calibration::stop(None, self.seq()));
            }
            // Never replace accepted teaching/tune/campaign records with another server's state.
            return;
        } else {
            self.snap.connection_valid = true;
            if self.snap.execution.is_some() && !status.connected {
                self.snap.authorization_revoked = true;
                self.snap.connection_valid = false;
                self.stopped_locally();
            }
        }
        // A pinned virtual session whose binding was revoked stays marked
        // until an explicit reconnect, however its server answers now.
        let revoked_pin = self.snap.execution.is_some() && self.snap.authorization_revoked;
        if status.connected {
            if !revoked_pin {
                self.snap.disconnected = None;
            }
        } else if was_connected {
            let detail = [status.message.as_deref(), status.error.as_deref()].into_iter().flatten().map(str::trim).find(|m| !m.is_empty());
            self.snap.disconnected = Some(match detail {
                Some(m) => format!("the server reports its bus disconnected: {m}"),
                None => "the server reports its bus disconnected".into(),
            });
        } else if lost_binding && !self.snap.authorization_revoked {
            self.snap.disconnected = None;
        }
        if let Some(tuning) = &status.tuning {
            if !tuning.stage.is_empty() && self.snap.tune_stages.last() != Some(&tuning.stage) {
                self.snap.tune_stages.push(tuning.stage.clone());
            }
        }
        self.snap.state = status;
        self.snap.read_at = Some(Instant::now());
        // The leg gait clock's base goes with the read it came from, also
        // when this status is published without a render.
        self.leg_frame();
    }
    fn message(&mut self, text: String) {
        self.command_error = Some(text.clone());
        self.snap.state.message = Some(text);
    }
    /// A handler declined without a request or a message of the page's: the
    /// reason a remote caller is given (the first one; the panel is unchanged).
    fn decline(&mut self, why: impl Into<String>) {
        if self.command_error.is_none() {
            self.command_error = Some(why.into());
        }
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
            if let Some(run) = self.snap.run {
                self.snap.learning_terminal = self.latest().and_then(|sample| sample.adaptation.clone()).map(|adaptation| (run, adaptation));
            }
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
}

/// What a remote command saves on the server or in the session as soon as
/// it runs (so a STOP afterwards does not undo it), named for the caller.
fn applied_change(command: &LinkCommand) -> Option<&'static str> {
    Some(match command {
        LinkCommand::Capture { .. } => "the pose",
        LinkCommand::Clear { .. } | LinkCommand::ResetPoses => "the pose reset",
        LinkCommand::SetDisabled => "the enable/disable change",
        LinkCommand::SpeedChanged => "the movement speed",
        LinkCommand::PwmChanged => "the PWM ceiling",
        _ => return None,
    })
}

/// `new Date().toLocaleTimeString()` for the warnings log. The viewer has no
/// timezone database, so this is the UTC time of day, labelled as such.
fn time_of_day() -> String {
    let s = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) % 86_400;
    format!("{:02}:{:02}:{:02} UTC", s / 3600, s / 60 % 60, s % 60)
}
