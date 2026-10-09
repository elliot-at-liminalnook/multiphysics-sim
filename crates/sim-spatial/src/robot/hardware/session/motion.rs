//! The page's stop, motor selection and motion handlers: the local half of
//! `stop()` :125, the motor pick, `update()` and its failure, Begin, the
//! hold-to-move press and release, and Disable/Enable this motor.
use super::*;

impl Session {
    /// The local half of `stop()` :125: a leg gait ends, sweep all ends, the
    /// session and intent are cleared and nothing is ready.
    pub(super) fn stopped_locally(&mut self) {
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
    pub(in crate::robot::hardware) fn stop(&mut self) {
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
    pub(in crate::robot::hardware) fn select_motor(&mut self, next: u8, hold_all: bool, solo: bool) {
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
    pub(in crate::robot::hardware) fn update(&mut self) {
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
    pub(super) fn motion_failed(&mut self, r: u64, e: u64, err: beat::Refused) {
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
    pub(super) fn begin(&mut self) {
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
    pub(super) fn move_(&mut self, direction: Direction) {
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
    pub(super) fn release(&mut self) {
        if matches!(self.snap.intent, Intent::Upper | Intent::Lower) {
            self.snap.intent = Intent::Hold;
            self.update();
            self.render();
        }
    }
    /// "Disable/Enable this motor" :158-162.
    pub(super) fn set_disabled(&mut self) {
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
