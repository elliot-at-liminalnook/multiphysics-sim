//! The heartbeats, off the link thread: one `jobs::RunThread`
//! ("hardware-beat") that posts the motion session's `motion_update` and the
//! leg gait's lease `gait_update`, so a request the link thread is waiting
//! on (up to 8 s for the hardware) never lets the server's 1.5 s motion or
//! gait lease lapse. The page runs these concurrently with its other
//! fetches (`heartbeat()` :325, the gait's `setInterval` :261).
//!
//! - **One sender per server domain.** The server checks `motion_update`
//!   and `capture_hold` against the session's last sequence as it parses
//!   them (not in its worker queue) and rejects a lower one as stale, so
//!   every request of that domain goes out from this thread, one at a time,
//!   drawing its sequence from the shared counter as it sends: they reach
//!   the server in sequence order. The worker's own commands stay on the
//!   link thread (also one at a time), `stop` is never sequence-checked, and
//!   `gait_update` carries no sequence.
//! - **The plan.** The link thread sends [`Beat::Plan`] whenever what the
//!   heartbeats would carry changes ([`Plan`]); the periodic sends use the
//!   newest plan. `motion_update` goes [`HEARTBEAT`] after the previous one
//!   answered (the page's `await update(); setTimeout(heartbeat,100)`) and
//!   at once on [`Beat::Now`] (the page's `await update()` on an intent
//!   change), whose result the link thread waits for. `gait_update` goes
//!   every [`GAIT_HEARTBEAT`] and at once when the scale or pause state
//!   changes, with [`LEASE_TIMEOUT`], its errors ignored (the page's `.catch`).
//! - **STOP.** Nothing is sent while the shared epoch differs from the
//!   plan's `seen_epoch`: a STOP the link thread has not applied, or a bump
//!   whose new plan has not arrived yet (skipping is the safe direction).
//!   A periodic `motion_update` failure is recorded with the run and the
//!   epoch it was sent under ([`Failure`]); the link thread acts on it only
//!   if both still hold.
//! - **Shutdown.** Dropping the handle closes the channel; the thread checks
//!   it between sends and returns (it is never joined).
use super::{COMMAND, STOP_PENDING};
use crate::jobs::RunThread;
use crate::robot::hardware::link::{GAIT_HEARTBEAT, HEARTBEAT};
use serde_json::Value;
use sim_runtime::hardware_client::calibration::{self, Input};
use sim_runtime::hardware_client::{Body, Client};
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// The timeout of one gait lease update: well inside the server's 1.5 s
/// lease, so a hung one is given up in time for the next to renew it.
pub(super) const LEASE_TIMEOUT: Duration = Duration::from_millis(1000);
/// How long the beat sleeps with nothing scheduled (a command or a closed
/// channel wakes it at once).
const IDLE: Duration = Duration::from_secs(1);

/// What the heartbeats carry, as of the link thread's last change.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Plan {
    /// The link thread's `seen_epoch`: nothing is sent while the shared
    /// epoch differs from it.
    pub seen_epoch: u64,
    /// A motion session open, ready, on a chosen motor.
    pub motion: Option<MotionBeat>,
    /// A leg gait playing on the server.
    pub gait: Option<GaitBeat>,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct MotionBeat {
    pub id: u8,
    pub run: u64,
    /// The page's `input()` at the time of the plan.
    pub input: Input,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GaitBeat {
    pub scale: f64,
    pub playing: bool,
}

/// A periodic `motion_update` the server refused or that failed in transit.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Failure {
    pub run: u64,
    /// The shared epoch when it was sent.
    pub epoch: u64,
    pub error: String,
}

/// Builds a request of the motion session's sequence domain from its sequence.
pub(super) type Build = Box<dyn FnOnce(u64) -> Body + Send>;

pub(super) enum Beat {
    Plan(Plan),
    /// Send `motion_update` for the current plan now and answer with its
    /// result (`Ok` also when nothing was sent: no motion, or a STOP pending).
    Now(mpsc::Sender<Result<(), String>>),
    /// Post another request of the motion session's sequence domain
    /// (`capture_hold`), in order with the heartbeats; refused with
    /// [`STOP_PENDING`] while a STOP is pending.
    Post(Build, mpsc::Sender<Result<Value, String>>),
    /// Answers once every earlier message (and any request in flight) is done.
    #[cfg(test)]
    Flush(mpsc::Sender<()>),
}

/// The beat thread's handle: its channel, and the last periodic failure.
pub(super) type Handle = RunThread<Beat, Option<Failure>>;

/// Starts the beat for `client`, drawing sequences from `sequence` and
/// gating on `epoch` (both shared with the link thread and the UI's STOP).
pub(super) fn spawn(client: Client, sequence: Arc<AtomicU64>, epoch: Arc<AtomicU64>) -> Handle {
    RunThread::spawn("hardware-beat", None, move |rx, failed| {
        let now = Instant::now();
        let lease = client.clone().with_timeout(LEASE_TIMEOUT);
        Beater { client, lease, sequence, epoch, failed, plan: Plan::default(), next_motion: now + HEARTBEAT, next_gait: now + GAIT_HEARTBEAT, gait_now: false }.run(rx)
    })
    .join_bound(Duration::ZERO)
}

struct Beater {
    client: Client,
    /// `client` with [`LEASE_TIMEOUT`].
    lease: Client,
    sequence: Arc<AtomicU64>,
    epoch: Arc<AtomicU64>,
    failed: Arc<Mutex<Option<Failure>>>,
    plan: Plan,
    next_motion: Instant,
    next_gait: Instant,
    /// The gait's scale or pause state changed: send at once.
    gait_now: bool,
}

impl Beater {
    fn run(mut self, rx: mpsc::Receiver<Beat>) {
        loop {
            let wait = self.due().map_or(IDLE, |due| due.saturating_duration_since(Instant::now()));
            match rx.recv_timeout(wait) {
                Ok(message) => self.handle(message),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            // Everything queued first: a plan and the `Now` after it are
            // handled before a periodic send.
            loop {
                match rx.try_recv() {
                    Ok(message) => self.handle(message),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }
            self.run_due();
        }
    }
    /// The earliest periodic send, if any is scheduled.
    fn due(&self) -> Option<Instant> {
        let motion = self.plan.motion.as_ref().map(|_| self.next_motion);
        let gait = self.plan.gait.as_ref().map(|_| if self.gait_now { Instant::now() } else { self.next_gait });
        match (motion, gait) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
    fn handle(&mut self, message: Beat) {
        match message {
            Beat::Plan(plan) => {
                let now = Instant::now();
                if plan.motion.is_some() && self.plan.motion.is_none() {
                    self.next_motion = now + HEARTBEAT;
                }
                match (&self.plan.gait, &plan.gait) {
                    (None, Some(_)) => self.next_gait = now + GAIT_HEARTBEAT,
                    (Some(old), Some(new)) if old != new => self.gait_now = true,
                    (_, None) => self.gait_now = false,
                    _ => {}
                }
                self.plan = plan;
            }
            Beat::Now(reply) => {
                let result = match self.motion() {
                    Some(Err(failure)) => Err(failure.error),
                    _ => Ok(()),
                };
                let _ = reply.send(result);
            }
            Beat::Post(build, reply) => {
                let result = if self.applied() {
                    let body = build(self.seq());
                    self.client.post(COMMAND, &body).map_err(|e| e.to_string())
                } else {
                    Err(STOP_PENDING.into())
                };
                let _ = reply.send(result);
            }
            #[cfg(test)]
            Beat::Flush(reply) => {
                let _ = reply.send(());
            }
        }
    }
    fn run_due(&mut self) {
        let now = Instant::now();
        if self.plan.gait.is_some() && (self.gait_now || now >= self.next_gait) {
            self.gait();
            self.gait_now = false;
            self.next_gait = Instant::now() + GAIT_HEARTBEAT;
        }
        if self.plan.motion.is_some() && now >= self.next_motion {
            if let Some(Err(failure)) = self.motion() {
                *self.failed.lock().unwrap_or_else(|p| p.into_inner()) = Some(failure);
            }
            // Also when skipped (a STOP pending), so the beat does not spin.
            self.next_motion = Instant::now() + HEARTBEAT;
        }
    }
    /// Every epoch is applied as of the plan (no STOP pending).
    fn applied(&self) -> bool {
        self.epoch.load(SeqCst) == self.plan.seen_epoch
    }
    /// The page's `++sequence`, drawn as the request is sent.
    fn seq(&self) -> u64 {
        self.sequence.fetch_add(1, SeqCst) + 1
    }
    /// One `motion_update` for the plan; None when the plan has no motion
    /// session or a STOP is pending (nothing sent). The next periodic one
    /// is [`HEARTBEAT`] after this one answered.
    fn motion(&mut self) -> Option<Result<(), Failure>> {
        let motion = self.plan.motion.clone()?;
        let epoch = self.epoch.load(SeqCst);
        if epoch != self.plan.seen_epoch {
            return None;
        }
        let body = calibration::motion_update(motion.id, self.seq(), motion.run, &motion.input);
        let result = self.client.post(COMMAND, &body).map(|_| ()).map_err(|e| Failure { run: motion.run, epoch, error: e.to_string() });
        self.next_motion = Instant::now() + HEARTBEAT;
        Some(result)
    }
    /// One `gait_update` (errors ignored, as the page's `.catch`), unless a
    /// STOP is pending.
    fn gait(&mut self) {
        let Some(gait) = self.plan.gait else { return };
        if !self.applied() {
            return;
        }
        let _ = self.lease.post(COMMAND, &calibration::gait_update(gait.scale, gait.playing));
    }
}
