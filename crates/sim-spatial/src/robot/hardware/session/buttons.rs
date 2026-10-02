//! The page's pose and motion buttons: the target slider (:299), the pose
//! buttons (:301-304), reset (:305-306), flip (:307), Try saved range
//! (:308-314), Learn (:315-319) and Send raw step (:320).
use super::Session;
use crate::robot::hardware::actions::Boundary;
use crate::robot::hardware::link::Intent;
use serde_json::Value;
use sim_runtime::hardware_client::calibration::{self, Status};

/// Why a `capture_hold` answer that is not the server's status (an older
/// server's `{"ok":true}`, sent before its hold session saved anything) is
/// an error: the pose may not be saved.
pub(in crate::robot::hardware) const CAPTURE_UNCONFIRMED: &str = "the server accepted the capture but did not confirm the save; check the pose before relying on it";

/// `capture_hold`'s answer: the server answers once its hold session saved
/// the pose, with its full status (as `/calibration/status`); a refusal
/// ("Still settling…", "Pose not saved: …") is a 400 and never reaches
/// here. The status types are tolerant (any object reads as a status), so
/// the answer must carry the status's own `connected` and `calibration`.
fn saved_status(answer: Value) -> Result<Status, String> {
    let is_status = answer.get("connected").is_some_and(Value::is_boolean) && answer.get("calibration").is_some_and(Value::is_object);
    if !is_status {
        return Err(CAPTURE_UNCONFIRMED.into());
    }
    serde_json::from_value(answer).map_err(|e| format!("{CAPTURE_UNCONFIRMED} (status: {e})"))
}

impl Session {
    /// The target slider :299: a raw target between the taught poses, 4 counts inside them.
    pub(super) fn target(&mut self, fraction: f64) {
        let a = self.axis();
        let (Some(lower), Some(upper)) = (a.lower, a.upper) else { return self.decline("both poses must be taught first") };
        let span = upper as f64 - lower as f64;
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) || span.abs() <= 8.0 {
            self.message("target needs a finite fraction and taught poses more than eight counts apart".into());
            return;
        }
        // Math.sign (0 for equal poses; f64::signum would give 1).
        let sign = if span > 0.0 {
            1.0
        } else if span < 0.0 {
            -1.0
        } else {
            0.0
        };
        self.snap.target_raw = lower as f64 + sign * 4.0 + (span - sign * 8.0) * fraction;
        self.snap.intent = Intent::Target;
        self.snap.sweeping = false;
        self.snap.learning = false;
        self.begin();
        self.render();
    }
    /// The pose buttons :301-304: `capture_hold` in a held session, else `capture`.
    ///
    /// `capture_hold` is checked against the session's heartbeat sequence,
    /// so the beat posts it (in order with the heartbeats, its sequence drawn
    /// as it is sent); a sequence drawn here could reach the server before a
    /// heartbeat in flight with a lower one, and either would be refused as stale.
    ///
    /// The server answers `capture_hold` only after its hold session saved
    /// the pose (at most its 1 s wait for the session plus 2 s for the
    /// outcome; the beat's request timeout is the client's, 10 s, and
    /// [`Session::beat_wait`] longer), with its full status, which is adopted
    /// at once: the panel and the mirror (its next `mirror_sync` sees the new
    /// revision) show the saved pose. An answer that is not a status is an
    /// error ([`CAPTURE_UNCONFIRMED`]). A status answered after a UI STOP was
    /// pressed is left to the next poll (the STOP's own answer may be newer).
    pub(super) fn capture(&mut self, boundary: Boundary, reference_joint_rad: Option<f64>) {
        let Some(id) = self.snap.id else { return self.decline("no motor is selected") };
        let name = boundary.name();
        let joint = if name == "reference" { reference_joint_rad } else { None };
        let result = match self.snap.run {
            Some(run) => {
                let input = self.input();
                let e = self.epoch_now();
                match self.send_in_session(Box::new(move |sequence| calibration::capture_hold(id, sequence, name, run, &input, joint))).and_then(saved_status) {
                    Ok(status) => {
                        if e == self.epoch_now() {
                            self.adopt(status);
                        }
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            }
            None => self.send_status(calibration::capture(id, self.seq(), name, joint)).map(|s| self.adopt(s)),
        };
        if let Err(e) = result {
            self.command_error = Some(e.clone());
            self.snap.state.capture_message = Some(e);
        }
        self.render();
    }
    /// `reset(boundary)` :305.
    pub(super) fn reset(&mut self, boundary: &str) {
        let Some(id) = self.snap.id else { return self.decline("no motor is selected") };
        if self.snap.busy {
            return self.decline("a motor request is still in progress");
        }
        self.stop();
        match self.send_status(calibration::clear(id, self.seq(), boundary)) {
            Ok(s) => {
                self.adopt(s);
                // The page's `selectMotor(id)`: the current id.
                if let Some(id) = self.snap.id {
                    self.select_motor(id, false, false);
                }
            }
            Err(e) => {
                self.message(e);
                self.render();
            }
        }
    }
    /// "Flip direction" :307.
    pub(super) fn flip(&mut self) {
        let Some(id) = self.snap.id else { return self.decline("no motor is selected") };
        if self.snap.busy {
            return self.decline("a motor request is still in progress");
        }
        self.stop();
        self.select_motor(id, false, false);
        let Some(id) = self.snap.id else { return };
        match self.send_status(calibration::flip(id, self.seq())) {
            Ok(s) => self.adopt(s),
            Err(e) => self.message(e),
        }
        self.render();
    }
    /// "Try saved range" / "Pause & hold" :308-314.
    pub(super) fn sweep(&mut self) {
        self.snap.learning = false;
        if self.snap.sweeping || self.snap.intent == Intent::Target {
            self.snap.intent = Intent::Hold;
            self.snap.sweeping = false;
            self.update();
            self.render();
            return;
        }
        // Reuse the same energized session when changing intent; no off/on kick.
        if self.snap.run.is_none() {
            self.snap.intent = Intent::Hold;
            self.begin();
        }
        if self.snap.run.is_some() {
            self.snap.intent = Intent::Sweep;
            self.snap.sweeping = true;
            // `$('speed').value='0'`: the UI follows `speed_reset`.
            self.inputs.speed_percent = 0.0;
            self.snap.speed_reset += 1;
            self.update();
        }
        self.render();
    }
    /// "Learn motion in the middle" / "Pause learning & hold" :315-319.
    pub(super) fn learn(&mut self) {
        if self.snap.learning {
            self.snap.intent = Intent::Hold;
            self.snap.learning = false;
            self.update();
            self.render();
            return;
        }
        self.snap.learning_terminal = None;
        if self.snap.run.is_none() {
            self.snap.intent = Intent::Hold;
            self.begin();
        }
        if self.snap.run.is_some() {
            self.snap.intent = Intent::Learn;
            self.snap.learning = true;
            self.snap.sweeping = false;
            self.update();
        }
        self.render();
    }
    /// "Send raw step" :320 (the step field's validity: a nonzero whole number within ±4095).
    pub(super) fn raw_step(&mut self, delta: i16) {
        let Some(id) = self.snap.id else { return self.decline("no motor is selected") };
        if !(-4095..=4095).contains(&delta) || delta == 0 || !self.pwm_valid() {
            return self.decline("a raw step needs a nonzero step within ±4095 and a valid PWM ceiling");
        }
        self.stop();
        self.select_motor(id, false, false);
        let Some(id) = self.snap.id else { return };
        match self.send_status(calibration::jog(id, self.seq(), delta, self.drive_pwm())) {
            Ok(s) => {
                self.adopt(s);
                self.snap.ready = self.snap.state.enabled_id == Some(id);
            }
            Err(e) => self.message(e),
        }
        self.render();
    }
}
