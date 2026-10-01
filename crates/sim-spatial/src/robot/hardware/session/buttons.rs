//! The page's pose and motion buttons: the target slider (:299), the pose
//! buttons (:301-304), reset (:305-306), flip (:307), Try saved range
//! (:308-314), Learn (:315-319) and Send raw step (:320).
use super::Session;
use crate::robot::hardware::actions::Boundary;
use crate::robot::hardware::link::Intent;
use sim_runtime::hardware_client::calibration;

impl Session {
    /// The target slider :299: a raw target between the taught poses, 4 counts inside them.
    pub(super) fn target(&mut self, fraction: f64) {
        let a = self.axis();
        let (Some(lower), Some(upper)) = (a.lower, a.upper) else { return };
        let span = (upper - lower) as f64;
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
    pub(super) fn capture(&mut self, boundary: Boundary, reference_joint_rad: Option<f64>) {
        let Some(id) = self.snap.id else { return };
        let name = boundary.name();
        let joint = if name == "reference" { reference_joint_rad } else { None };
        let result = match self.snap.run {
            Some(run) => {
                let input = self.input();
                self.send(calibration::capture_hold(id, self.seq(), name, run, &input, joint)).map(|_| ())
            }
            None => self.send_status(calibration::capture(id, self.seq(), name, joint)).map(|s| self.adopt(s)),
        };
        if let Err(e) = result {
            self.snap.state.capture_message = Some(e);
        }
        self.render();
    }
    /// `reset(boundary)` :305.
    pub(super) fn reset(&mut self, boundary: &str) {
        let Some(id) = self.snap.id else { return };
        if self.snap.busy {
            return;
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
        let Some(id) = self.snap.id else { return };
        if self.snap.busy {
            return;
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
        let Some(id) = self.snap.id else { return };
        if !(-4095..=4095).contains(&delta) || delta == 0 || !self.pwm_valid() {
            return;
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
