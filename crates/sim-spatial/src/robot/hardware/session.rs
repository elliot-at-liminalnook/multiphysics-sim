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
use super::actions::Direction;
use super::link::{Inputs, Intent, LinkCommand, LinkSnapshot, POLL_IDLE};
use serde_json::Value;
use sim_runtime::hardware_client::calibration::{self, Axis, Input, Status, SweepSample};
use sim_runtime::hardware_client::{Body, CONNECT_TIMEOUT, Client, ClientError, STOP_TIMEOUT};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod beat;
mod buttons;
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
    /// session's automation is revoked until an explicit reconnect.
    fn lose_binding(&mut self) {
        self.snap.authorization_revoked |= self.snap.execution.is_some();
        self.snap.connection_valid = false;
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
        let is_stop = matches!(body.get("action"), Some(sim_runtime::hardware_client::Json::Value(Value::String(a))) if a == "stop");
        if !is_stop && self.interrupted() {
            return Err(STOP_PENDING.into());
        }
        if !is_stop && self.revoked() {
            return Err(REVOKED.into());
        }
        self.request(|client| client.post(COMMAND, &body)).map_err(|e| {
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
    fn adopt(&mut self, status: Status) {
        if self.snap.execution.is_some() && self.snap.execution != status.execution {
            self.snap.authorization_revoked = true;
            self.snap.connection_valid = false;
            // No execution any more: the server lost its virtual bench (its
            // `lose_bus`); another one: the server or bench was replaced.
            // Either revokes the same way.
            self.message(if status.execution.is_none() {
                "the virtual calibration bench was lost (disconnected); reconnect required".into()
            } else {
                "virtual execution identity changed; reconnect required".into()
            });
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
        if let Some(tuning) = &status.tuning {
            if !tuning.stage.is_empty() && self.snap.tune_stages.last() != Some(&tuning.stage) {
                self.snap.tune_stages.push(tuning.stage.clone());
            }
        }
        self.snap.state = status;
        self.snap.read_at = Some(Instant::now());
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

    // ---- commands ----

    pub(super) fn handle(&mut self, command: LinkCommand) {
        match command {
            LinkCommand::Checked { ticket, epoch, generation, inputs, command } => {
                self.snap.authorization_revoked |= self.authorization.load(SeqCst);
                self.command_error = None;
                let is_tune = matches!(&*command, LinkCommand::Tune);
                let is_campaign = matches!(&*command, LinkCommand::Campaign { .. });
                let (tune_done, campaign_done) = (self.snap.tune_done, self.snap.campaign_done);
                // The generation the UI authorized it for must still be this session's.
                let authorization = calibration::authorize_virtual(self.snap.execution.as_ref(), generation, self.snap.generation,
                    self.snap.connection_valid && self.snap.state.connected && !self.snap.authorization_revoked, !self.snap.stale(Instant::now()));
                let result = if matches!(&*command, LinkCommand::Release) {
                    self.checked_release(epoch, authorization)
                } else if self.stopped_since(epoch) {
                    Err(STOP_PENDING.into())
                } else if let Err(error) = authorization {
                    Err(error)
                } else if let Err(error) = self.validate_command(&command) {
                    Err(error)
                } else {
                    let applies = applied_change(&command);
                    let previous = inputs.map(|inputs| {
                        let previous = self.inputs.clone();
                        self.set_inputs(inputs);
                        previous
                    });
                    let outcome = self.run_checked(*command);
                    // A UI STOP pressed while it ran ended it (its own bumps
                    // don't count); a change it already saved is reported as such.
                    let result = match (outcome, applies) {
                        (Ok(()), Some(what)) if self.stopped_since(epoch) => Err(format!("{APPLIED_THEN_STOPPED}: {what} was saved; motion was stopped")),
                        (_, _) if self.stopped_since(epoch) => Err(STOP_PENDING.into()),
                        (outcome, _) => outcome,
                    };
                    // Form values the command brought stay only with a change that holds.
                    if let (Some(previous), Err(e)) = (previous, &result)
                        && !e.starts_with(APPLIED_THEN_STOPPED)
                    {
                        self.inputs = previous;
                    }
                    result
                };
                // Only where the sequence did not already count its end (the box unchecks once).
                if result.is_err() {
                    if is_tune && self.snap.tune_done == tune_done { self.snap.tune_done += 1; }
                    if is_campaign && self.snap.campaign_done == campaign_done { self.snap.campaign_done += 1; }
                }
                self.snap.command_results.insert(ticket, result);
                // Bound receipts; tickets are monotonic, consumers fail closed if evicted.
                while self.snap.command_results.len() > 256 {
                    self.snap.command_results.pop_first();
                }
                self.publish();
            }
            LinkCommand::Inputs(inputs) => self.set_inputs(inputs),
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
                let boundary = self.reset_boundary();
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
                self.ui_stop_epoch = self.ui_stop_epoch.max(epoch);
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
        // `Inputs` changes what the heartbeat carries without a publish.
        self.sync_beat();
    }

    /// New form values. Values sent before the UI applied the latest speed
    /// reset keep the session's speed (0 since the reset).
    fn set_inputs(&mut self, inputs: Inputs) {
        let speed = self.inputs.speed_percent;
        let stale = inputs.speed_reset < self.snap.speed_reset;
        self.inputs = inputs;
        if stale {
            self.inputs.speed_percent = speed;
        }
    }

    /// A remote release (Q/A or the jog button let go): a move to hold, so
    /// like STOP it is not refused for a stale status, another generation or
    /// a revoked binding. A STOP already pressed has ended the motion. When
    /// the session may not be driven any more (the authorization check
    /// failed), hold cannot be requested through it: STOP instead.
    fn checked_release(&mut self, epoch: u64, authorization: Result<(), String>) -> Result<(), String> {
        if self.stopped_since(epoch) {
            return Ok(());
        }
        if authorization.is_err() {
            self.stop();
        } else {
            self.release();
        }
        self.command_error.take().map_or(Ok(()), Err)
    }

    /// "Reset poses"'s boundary :306: the pose the motor is beyond, else both.
    fn reset_boundary(&self) -> &'static str {
        match self.outside_pose() {
            Some("reference") | None => "both",
            Some(b) => b,
        }
    }

    /// Runs a remote command and judges it by what it achieved. The page's
    /// handlers decline silently where its disabled controls would have
    /// prevented the click (no motor ready, busy, a select the server did
    /// not honour, …), so the absence of a message is not success: each
    /// command that has a purpose to check is Ok exactly when that purpose
    /// holds afterwards (then also when a step on the way, such as the
    /// `stop()` before a tune, reported an error), else Err with the first
    /// reason recorded or a description of what did not happen. The rest are
    /// Ok unless an error was recorded.
    fn run_checked(&mut self, command: LinkCommand) -> Result<(), String> {
        use LinkCommand as C;
        let was_sweeping = self.snap.sweeping || self.snap.intent == Intent::Target;
        let was_learning = self.snap.learning;
        let was_sweeping_all = self.sweep_all.is_some();
        let (was_tuning, was_campaigning) = (self.snap.tuning, self.snap.campaigning);
        let chosen = self.snap.id;
        let was_disabled = chosen.and_then(|id| self.axis_of(id)).map(|a| a.disabled);
        let clearing = match &command {
            C::Clear { boundary } => Some(boundary.name()),
            C::ResetPoses => Some(self.reset_boundary()),
            _ => None,
        };
        let check = command.clone();
        self.handle(command);
        let (achieved, what): (bool, String) = match check {
            C::Select { id } => (
                self.snap.id == Some(id) && (self.snap.ready || self.axis_of(id).is_some_and(|a| a.disabled)),
                format!("motor {id} was not enabled"),
            ),
            C::Press { direction } => {
                let intent = match direction {
                    Direction::Upper => Intent::Upper,
                    Direction::Lower => Intent::Lower,
                };
                (self.snap.run.is_some() && self.snap.intent == intent, "the hold-to-move session did not start".into())
            }
            C::Target { .. } => (self.snap.run.is_some() && self.snap.intent == Intent::Target, "the move to the target did not start".into()),
            C::Sweep if was_sweeping => (!self.snap.sweeping && self.snap.intent != Intent::Target, "the sweep did not pause".into()),
            C::Sweep => (self.snap.run.is_some() && self.snap.sweeping, "the saved-range sweep did not start".into()),
            C::Learn if was_learning => (!self.snap.learning, "learning did not pause".into()),
            C::Learn => (
                self.snap.run.is_some() && (self.snap.learning || self.snap.learning_terminal.is_some()),
                "learning did not start".into(),
            ),
            C::SweepAll if was_sweeping_all => (self.sweep_all.is_none(), "sweep-all did not stop".into()),
            C::SweepAll => (self.sweep_all.is_some(), format!("sweep-all did not start: {}", self.snap.sequence_text)),
            C::Tune => (!was_tuning && self.snap.tuning, if was_tuning { "a tune is already running".into() } else { "the tune did not start".into() }),
            C::Campaign { .. } => (
                !was_campaigning && self.snap.campaigning,
                if was_campaigning { "a campaign is already running".into() } else { "the campaign did not start".into() },
            ),
            C::SetDisabled => match (chosen, was_disabled) {
                (Some(id), Some(was)) => (self.axis_of(id).is_some_and(|a| a.disabled != was), format!("motor {id} was not {}", if was { "enabled" } else { "disabled" })),
                _ => (false, "no motor with a calibration is selected".into()),
            },
            C::ResetPoses | C::Clear { .. } => {
                let cleared = chosen.and_then(|id| self.axis_of(id)).is_some_and(|a| match clearing {
                    Some("lower") => a.lower.is_none(),
                    Some("upper") => a.upper.is_none(),
                    _ => a.lower.is_none() && a.upper.is_none(),
                });
                (cleared, "the pose was not cleared".into())
            }
            _ => return self.command_error.take().map_or(Ok(()), Err),
        };
        let error = self.command_error.take();
        if achieved {
            return Ok(());
        }
        Err(error.unwrap_or_else(|| match self.snap.state.message.as_deref().filter(|m| !m.is_empty()) {
            Some(m) => format!("{what} (server: {m})"),
            None => what,
        }))
    }

    /// Validate again at queue consumption against the latest authoritative session.
    fn validate_command(&self, command: &LinkCommand) -> Result<(), String> {
        use LinkCommand as C;
        if matches!(command, C::Press { .. } | C::Target { .. } | C::TargetCommit | C::Capture { .. } | C::Sweep | C::Learn | C::Tune)
            && (!self.snap.ready || self.snap.busy) { return Err("motor is no longer ready".into()); }
        if matches!(command, C::Target { .. } | C::Sweep | C::Learn) {
            let axis = self.axis();
            if axis.lower.zip(axis.upper).is_none_or(|(lower, upper)| lower.abs_diff(upper) <= 8)
                || self.outside_pose() == Some("reference") {
                return Err("current encoder session needs two taught poses more than eight counts apart".into());
            }
        }
        if let C::Select { id } = command {
            if !self.snap.state.calibration.as_ref().is_some_and(|c| c.axes.contains_key(id)) {
                return Err(format!("unknown calibration motor {id}"));
            }
        }
        Ok(())
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
        // The page's `if(id==null)return`, unless this link drove (then id-less).
        if self.snap.id.is_none() && !self.snap.drove {
            return;
        }
        match self.send_status(calibration::stop(self.snap.id, self.seq())) {
            Ok(status) => self.adopt(status),
            Err(e) => self.message(e),
        }
        self.render();
    }
    /// `selectMotor(next, holdAll, solo)` :126-132.
    pub(super) fn select_motor(&mut self, next: u8, hold_all: bool, solo: bool) {
        self.snap.learning_terminal = None;
        if self.axis_of(next).is_some_and(|a| a.disabled) {
            if self.snap.busy {
                return self.decline("a motor request is still in progress");
            }
            self.stop();
            self.snap.id = Some(next);
            self.render();
            return;
        }
        if self.snap.busy {
            return self.decline("a motor request is still in progress");
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
        // Published with the render below, before the request: from here an
        // automatic STOP may be id-less ([`LinkSnapshot::drove`]).
        self.snap.drove = true;
        self.render();
        let hold_others = !solo && (hold_all || self.inputs.hold_others);
        match self.send_status(calibration::select(next, self.seq(), hold_others)) {
            Ok(status) => {
                if e == self.epoch_now() {
                    self.adopt(status);
                    self.snap.ready = self.snap.state.enabled_id == Some(next) && !self.snap.authorization_revoked;
                    if !self.snap.ready {
                        let why = if self.snap.authorization_revoked {
                            REVOKED.to_string()
                        } else {
                            match self.snap.state.enabled_id {
                                Some(other) => format!("the server enabled motor {other}, not motor {next}"),
                                None => format!("the server did not enable motor {next}"),
                            }
                        };
                        self.decline(why);
                    }
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
    /// `update()` :133-145 on an intent change: the beat sends
    /// `motion_update` with the new plan at once and this waits for it (the
    /// page's `await update()`). The periodic heartbeat is the beat's own;
    /// its failures come here through [`Session::beat_failure`]. Unlike the
    /// page's `heartbeatBusy`, a change while a heartbeat is in flight is
    /// not dropped: it follows that heartbeat.
    pub(super) fn update(&mut self) {
        let (Some(r), Some(_)) = (self.snap.run, self.snap.id) else { return };
        // A pending STOP ends the session; don't answer it with a heartbeat failure.
        if !self.snap.ready || self.interrupted() {
            return;
        }
        let e = self.epoch_now();
        let Err(err) = self.beat_now() else { return };
        self.motion_failed(r, e, err);
    }
    /// `update()`'s `catch` :136-143 for a heartbeat of run `r` sent under
    /// epoch `e` that failed with `err`. The session stops either way; a
    /// pinned virtual session's automation is revoked only when the failure
    /// means the binding is gone (a refused heartbeat, e.g. a stale sequence, is not).
    fn motion_failed(&mut self, r: u64, e: u64, err: beat::Refused) {
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
        if err.binding_lost {
            self.lose_binding();
        }
        self.stop();
        self.message(err.error);
        self.render();
    }
    /// `begin()` :146-154.
    fn begin(&mut self) {
        if !self.snap.ready || self.snap.busy || self.snap.starting {
            return self.decline("no motor is ready to move");
        }
        if self.snap.run.is_some() {
            self.update();
            return;
        }
        if !self.pwm_valid() {
            return self.decline("the PWM ceiling must be from 0 to 100 %");
        }
        let Some(id) = self.snap.id else { return self.decline("no motor is selected") };
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
            return self.decline("no motor is ready to move");
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
        let Some(id) = self.snap.id else { return self.decline("no motor is selected") };
        if self.snap.busy || self.sweep_all.is_some() {
            return self.decline("not while a motor request or a sweep-all runs");
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
