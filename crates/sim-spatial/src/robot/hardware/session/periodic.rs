//! The session's periodic work (`setTimeout`/`setInterval` loops as
//! deadlines): the status poll, the sequences' ticks and the sim-only gait's
//! frames, and the shutdown STOP when the link's channel closes.
use super::{COMMAND, GAIT_FRAME, Session};
use crate::robot::hardware::link::{HEARTBEAT, Intent, POLL_ACTIVE, POLL_IDLE};
use sim_runtime::hardware_client::STOP_TIMEOUT;
use sim_runtime::hardware_client::calibration::{self, Status};
use std::time::Instant;

impl Session {
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
                // Only a lost connection or binding revokes a virtual
                // session; any failed poll stops what may be driving.
                if calibration::binding_lost(&err) {
                    self.lose_binding();
                }
                if self.drive_active() {
                    self.stop();
                }
                self.message(err.to_string());
                self.render();
            }
        }
        let active = self.snap.run.is_some() || self.leg_gait();
        self.next_poll = Instant::now() + if active { POLL_ACTIVE } else { POLL_IDLE };
    }
    pub(super) fn leg_gait(&self) -> bool {
        self.snap.gait.as_ref().is_some_and(|g| g.leg)
    }
    fn sim_gait_playing(&self) -> bool {
        self.snap.gait.as_ref().is_some_and(|g| !g.leg && g.playing)
    }
    /// The earliest periodic work due.
    pub(in crate::robot::hardware) fn next_deadline(&self) -> Instant {
        let mut due = self.next_poll;
        // The beat's failures are taken at least as often as it sends.
        if self.plan.motion.is_some() {
            due = due.min(Instant::now() + HEARTBEAT);
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
    /// Runs the periodic work due at `now`: a periodic heartbeat failure
    /// the beat recorded, the status poll and the sequences' ticks. The
    /// heartbeats themselves (`heartbeat()` :325, the gait lease :261) are
    /// the beat's.
    pub(in crate::robot::hardware) fn run_due(&mut self, now: Instant) {
        self.beat_failure();
        if now >= self.next_poll {
            self.poll();
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
        self.sync_beat();
    }
    /// [`crate::robot::hardware::link::drive_active`] on this session's state.
    pub(super) fn drive_active(&mut self) -> bool {
        self.snap.sweep_all = self.sweep_all.is_some();
        crate::robot::hardware::link::drive_active(&self.snap)
    }
    /// The channel closed (the link dropped): the page's `loss()` on
    /// `pagehide`, widened ([`crate::robot::hardware::link::drive_active`]), with the short
    /// STOP timeout so the thread ends promptly. Sent also while a STOP is
    /// pending (the UI's STOP may not have reached the server). The beat is
    /// dropped first: its channel closes and it sends nothing more.
    pub(super) fn shutdown(&mut self) {
        // Silence the beat before it goes: a beat mid-`run_due` checks the
        // epoch before each send, so no heartbeat can renew the lease
        // ahead of the STOP below.
        self.bump_epoch();
        self.beat = None;
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
