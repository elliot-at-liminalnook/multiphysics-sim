//! Reusable, measured encoder-range traversal using shared governor/PID components.
//! No robot topology, inferred joint angles, or plant physics.
use super::calibration::AxisCalibration;
use serde::{Deserialize, Serialize};
use sim_domain_control::{
    adaptive_braking::{Config as AdaptiveConfig, Model as AdaptiveModel},
    pwm_feedback::{EncoderEstimate, Pid, PidState},
    reference_governor::{Config as Governor, State as Reference},
};
const RAD: f64 = std::f64::consts::TAU / 4096.;
/// Speed at which a disturbed hold interpolates back to its rest pose.
const HOLD_RETURN_COUNTS_S: f64 = 40.;
/// Longest a released hold follows a still-moving part before holding it.
const MAXIMUM_COAST_S: f64 = 0.6;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SweepTuning {
    pub period_s: f64,
    pub maximum_feedback_interval_s: f64,
    pub maximum_speed_counts_s: f64,
    pub maximum_acceleration_counts_s2: f64,
    pub response_rate_per_s: f64,
    pub following_error_counts: f64,
    pub velocity_filter_s: f64,
    pub stalled_at_limit_s: f64,
    pub pid: Pid,
    pub provenance: String,
    pub adaptive: AdaptiveConfig,
    /// Ceiling for operator-held jogs (Q/A). Held jogs honour the requested
    /// speed up to this value and the braking-distance limit before a taught
    /// pose, without waiting for demonstrated stops. Absent: jogs use the
    /// demonstrated-speed envelope like automatic motion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_jog_speed_limit_counts_s: Option<f64>,
    /// Deceleration of a held jog's reference arriving at a taught pose. The
    /// pose stays a hard stop; manual jogs do not creep toward it. Absent:
    /// the ordinary reference acceleration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_jog_deceleration_counts_s2: Option<f64>,
    /// Coasting allowance for a held jog arriving at a taught pose: the
    /// reference stops this many seconds of its current speed short of the
    /// pose, so response lag carries the part onto the pose, not past it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_jog_stop_lead_s: Option<f64>,
    /// Hold tolerance. Once a hold has stopped within this many counts it is
    /// accepted with no drive, so a sticky axis does not hunt; drifting beyond
    /// it makes the hold correct again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_deadband_counts: Option<f64>,
    /// Largest change of drive duty per second, so effort ramps instead of jerking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort_slew_per_s: Option<f64>,
    /// Acceleration of a held jog's reference. Tuned motors follow it with
    /// feed-forward, so it can be much faster than the automatic-motion limit.
    /// Absent: the ordinary reference acceleration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_jog_acceleration_counts_s2: Option<f64>,
    /// `false`: tracked references (supervised gait playback) are exempt from
    /// the stopping-distance slowing and predictive brake, like held jogs. The
    /// tracked reference is already governed and clamped inside the taught
    /// poses, and the controller brakes actively; the envelope's passive-braking
    /// estimate latches a brake at gait speeds. Absent/`true`: envelope applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_stopping_envelope: Option<bool>,
}
/// How the host drives a servo during a session. The same reference, bounds
/// and supervisor apply in every mode; only the command written differs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveMode {
    /// Open-loop PWM with the host feedback loop (identification and default).
    #[default]
    Pwm,
    /// Servo's internal position loop, streamed goals from the host reference.
    ServoPosition,
    /// Servo's internal speed loop, host reference speed plus position trim.
    ServoSpeed,
}
impl DriveMode {
    /// Servo control-mode register (0x21) value.
    pub fn register(self) -> u8 {
        match self {
            Self::ServoPosition => 0,
            Self::ServoSpeed => 1,
            Self::Pwm => 2,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct SweepInput {
    pub speed_counts_s: f64,
    pub pwm_limit: u16,
}
impl SweepInput {
    pub fn validate(self, c: &SweepTuning) -> Result<(), String> {
        if !self.speed_counts_s.is_finite()
            || !(0.1..=c.maximum_speed_counts_s).contains(&self.speed_counts_s)
            || self.pwm_limit > 1000
        {
            return Err(format!(
                "Sweep speed must be 0.1..{} counts/s and PWM ceiling 0..100%",
                c.maximum_speed_counts_s
            ));
        }
        Ok(())
    }
}
impl SweepTuning {
    pub fn validate(&self) -> Result<(), String> {
        self.pid.validate()?;
        self.adaptive.validate()?;
        if [
            self.period_s,
            self.maximum_feedback_interval_s,
            self.maximum_speed_counts_s,
            self.maximum_acceleration_counts_s2,
            self.response_rate_per_s,
            self.following_error_counts,
            self.velocity_filter_s,
            self.stalled_at_limit_s,
        ]
        .iter()
        .any(|x| !x.is_finite() || *x <= 0.)
            || self.period_s > self.maximum_feedback_interval_s
            || self.maximum_feedback_interval_s > 0.15
            || self.maximum_feedback_interval_s * self.response_rate_per_s > 0.25
            || self.provenance.is_empty()
            || self.operator_jog_speed_limit_counts_s.is_some_and(|v| !v.is_finite() || v <= 0. || v > self.maximum_speed_counts_s)
            || self.operator_jog_deceleration_counts_s2.is_some_and(|v| !v.is_finite() || v <= 0.)
            || self.operator_jog_stop_lead_s.is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
            || self.hold_deadband_counts.is_some_and(|v| !v.is_finite() || !(0.0..=64.0).contains(&v))
            || self.effort_slew_per_s.is_some_and(|v| !v.is_finite() || v <= 0.)
            || self.operator_jog_acceleration_counts_s2.is_some_and(|v| !v.is_finite() || v <= 0.)
        {
            return Err("Invalid explicit sweep tuning or missing provenance".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct SweepSample {
    pub elapsed_ms: u128,
    pub position_raw: u16,
    pub position_continuous: i32,
    pub target_raw: f64,
    /// Reference speed at this sample (counts/s), for servo speed/position modes.
    pub target_velocity_counts_s: f64,
    pub velocity_counts_s: f64,
    pub requested_speed_counts_s: f64,
    pub pwm: i16,
    pub toward_upper: bool,
    pub half_cycles: u64,
    pub holding: bool,
    pub adaptation: AdaptationSample,
    /// Operator-visible conditions that no longer stop motion: tracking
    /// resynchronised, feedback gaps, stalls, travel outside taught poses.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct AdaptationSample {
    pub status: String,
    pub permitted_speed_counts_s: f64,
    pub stopping_distance_counts: f64,
    pub decreasing_stops: usize,
    pub increasing_stops: usize,
    pub decreasing_speed_counts_s: f64,
    pub increasing_speed_counts_s: f64,
    pub decreasing_acceleration_counts_s2: f64,
    pub increasing_acceleration_counts_s2: f64,
    pub braking: bool,
    pub learning: bool,
    pub learning_complete: bool,
    pub evidence: Option<AdaptiveModel>,
}
/// Intent in the part coordinate frame. Hold preserves feedback effort against load.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MotionCommand {
    Sweep,
    Learn,
    Jog(i8),
    Hold,
    Target(f64),
    /// Follow an externally timed smooth reference directly: encoder counts
    /// and its velocity (counts/s), e.g. a gait sampled on a shared clock.
    /// The caller bounds its speed and acceleration; the pose window still
    /// clamps it and the feedback/feed-forward law is the shared one.
    Track(f64, f64),
}
pub struct RangeSweep {
    tuning: SweepTuning,
    pub raw_lower: i32,
    pub raw_upper: i32,
    lower_pose: f64,
    upper_pose: f64,
    toward_upper: bool,
    reference: Reference,
    estimate: EncoderEstimate,
    pid: PidState,
    last_time: f64,
    stationary_since: f64,
    stationary_position: i32,
    saturated_since: Option<f64>,
    half_cycles: u64,
    command: MotionCommand,
    reverse: bool,
    adaptive: AdaptiveModel,
    brake_latch: Option<f64>,
    /// Encoder direction whose motion needed the brake; None after a context change.
    brake_direction: Option<usize>,
    learning_active: bool,
    learning_phase: u8,
    learning_direction: i8,
    phase_started: f64,
    previous_pwm: Option<u16>,
    environment: Option<(f64, u8)>,
    context_changed: bool,
    fully_taught: bool,
    /// Friction feed-forward from this motor's identification (duty).
    friction_duty: f64,
    /// Measured counts/s per unit duty beyond friction; enables velocity
    /// feed-forward so the drive follows the reference open-loop, as in tuning.
    speed_gain: Option<f64>,
    /// Passive braking of a tuned motor (counts/s²): half the deceleration its
    /// measured moving friction gives with drive removed.
    tuned_braking: Option<f64>,
    last_duty: f64,
    /// Hold has settled and is accepting errors within the tolerance.
    settled: bool,
    /// Released and still moving: the hold follows until the part stops.
    coasting: bool,
    coast_started: f64,
    /// Where a hold came to rest; disturbances are corrected back toward it.
    hold_target_rad: f64,
    was_moving: bool,
}
impl RangeSweep {
    pub fn new(
        axis: &AxisCalibration,
        position: i32,
        clearance: u16,
        tuning: SweepTuning,
    ) -> Result<Self, String> {
        axis.validate()?;
        let mut tuning = tuning;
        // Gains identified for this motor replace the shared provisional ones.
        if let Some(m) = &axis.tuning {
            tuning.pid = m.pid.clone();
        }
        tuning.validate()?;
        let friction_duty = axis.tuning.as_ref().map_or(0., |m| m.friction_duty);
        let speed_gain = axis.tuning.as_ref().map(|m| m.gain_counts_s_per_duty).filter(|k| k.is_finite() && *k > 1.);
        let tuned_braking = axis.tuning.as_ref().and_then(|m| {
            let a = 0.5 * m.gain_counts_s_per_duty * m.friction_duty / m.time_constant_s.max(0.01);
            (a.is_finite() && a > 0.).then(|| a.max(200.))
        });
        let (lo, hi) = axis.encoder_bounds();
        let (lo, hi) = (
            lo.ok_or("Teach both poses before sweeping")?,
            hi.ok_or("Teach both poses before sweeping")?,
        );
        if clearance < 4
            || hi - lo <= clearance.saturating_mul(2).saturating_add(8) as i32
            || !(lo..=hi).contains(&position)
        {
            return Err("Sweep needs a non-wrapping taught range wider than twice its clearance plus 8 counts, and the motor inside that range".into());
        }
        let (a, b) = (
            (lo + clearance as i32) as f64 * RAD,
            (hi - clearance as i32) as f64 * RAD,
        );
        let (lower_pose, upper_pose) = if axis.reversed() { (b, a) } else { (a, b) };
        let mut estimate = EncoderEstimate::default();
        estimate.observe(
            0.,
            position as f64 * RAD,
            std::f64::consts::TAU,
            tuning.velocity_filter_s,
        )?;
        Ok(Self {
            tuning,
            raw_lower: lo,
            raw_upper: hi,
            lower_pose,
            upper_pose,
            toward_upper: true,
            reference: Reference {
                angle_rad: position as f64 * RAD,
                velocity_rad_s: 0.,
            },
            estimate,
            pid: PidState::default(),
            last_time: 0.,
            stationary_since: 0.,
            stationary_position: position,
            saturated_since: None,
            half_cycles: 0,
            command: MotionCommand::Sweep,
            reverse: axis.reversed(),
            adaptive: AdaptiveModel::default(),
            brake_latch: None,
            brake_direction: None,
            learning_active: false,
            learning_phase: 0,
            learning_direction: 1,
            phase_started: 0.,
            previous_pwm: None,
            environment: None,
            context_changed: false,
            friction_duty,
            speed_gain,
            tuned_braking,
            last_duty: 0.,
            settled: false,
            coasting: false,
            coast_started: 0.,
            hold_target_rad: position as f64 * RAD,
            was_moving: false,
            fully_taught: true,
        })
    }
    pub fn teaching(
        axis: &AxisCalibration,
        position: i32,
        tuning: SweepTuning,
    ) -> Result<Self, String> {
        axis.validate()?;
        let (lo, hi) = axis.encoder_bounds();
        let lo = lo.unwrap_or(-8_000_000);
        let hi = hi.unwrap_or(8_000_000);
        if hi <= lo + 16 || !(lo..=hi).contains(&position) {
            return Err(
                "Position is outside saved physical poses; reset the appropriate pose to re-teach"
                    .into(),
            );
        }
        // Fallback endpoints bound numeric representation, not mechanical travel. Zero is not a limit.
        let mut envelope = axis.clone();
        envelope.lower = Some(if axis.reversed() { hi } else { lo });
        envelope.upper = Some(if axis.reversed() { lo } else { hi });
        let mut c = Self::new(&envelope, position, 4, tuning)?;
        c.command = MotionCommand::Hold;
        c.fully_taught = axis.lower.is_some() && axis.upper.is_some();
        Ok(c)
    }
    pub fn set_taught_bounds(&mut self, axis: &AxisCalibration) -> Result<(), String> {
        axis.validate()?;
        let (lo, hi) = axis.encoder_bounds();
        let lo = lo.unwrap_or(-8_000_000);
        let hi = hi.unwrap_or(8_000_000);
        if hi <= lo + 8 {
            return Err("Taught envelope is too narrow".into());
        }
        if axis.reversed() != self.reverse {
            return Err(
                "Saved pose would reverse the taught direction; swap direction explicitly first"
                    .into(),
            );
        }
        if self.raw_lower != lo || self.raw_upper != hi {
            self.adaptive.reset("Taught poses changed");
            self.context_changed = true;
        }
        self.fully_taught = axis.lower.is_some() && axis.upper.is_some();
        self.raw_lower = lo;
        self.raw_upper = hi;
        self.reverse = axis.reversed();
        let margin = ((hi - lo) / 4).min(64).max(4);
        let (a, b) = ((lo + margin) as f64 * RAD, (hi - margin) as f64 * RAD);
        (self.lower_pose, self.upper_pose) = if self.reverse { (b, a) } else { (a, b) };
        Ok(())
    }
    pub fn observe_environment(&mut self, voltage: f64, temperature: u8) {
        if let Some((v, t)) = self.environment {
            if (voltage - v).abs() > v * 0.05 || temperature.abs_diff(t) > 10 {
                self.adaptive.reset("Supply or temperature changed");
                self.context_changed = true;
                self.environment = Some((voltage, temperature));
            }
        } else {
            self.environment = Some((voltage, temperature));
        }
    }
    pub fn update(
        &mut self,
        time: f64,
        position: i32,
        input: SweepInput,
    ) -> Result<SweepSample, String> {
        self.control(time, position, input, MotionCommand::Sweep)
    }
    pub fn control(
        &mut self,
        time: f64,
        position: i32,
        input: SweepInput,
        command: MotionCommand,
    ) -> Result<SweepSample, String> {
        input.validate(&self.tuning)?;
        let mut warnings = Vec::new();
        let mut dt = time - self.last_time;
        if !dt.is_finite() || dt <= 0. {
            return Err("Sweep feedback time did not advance".into());
        }
        if dt > self.tuning.maximum_feedback_interval_s {
            warnings.push(format!("Feedback gap of {:.0} ms; control step clamped", dt * 1000.));
            dt = self.tuning.maximum_feedback_interval_s;
        }
        if !(self.raw_lower..=self.raw_upper).contains(&position) {
            warnings.push(format!("Outside taught poses at {position}; drive further outward is blocked"));
        }
        // Learning and envelopes observe the nearest in-range position.
        let inside = position.clamp(self.raw_lower, self.raw_upper);
        let (_, velocity) = self.estimate.observe(
            time,
            position as f64 * RAD,
            std::f64::consts::TAU,
            self.tuning.velocity_filter_s,
        )?;
        if position.abs_diff(self.stationary_position) >= 2 {
            self.stationary_since = time;
            self.stationary_position = position;
        }
        if input.pwm_limit == 0 {
            return Err("PWM ceiling is zero; sweep stopped".into());
        }
        let requested_command = command;
        let requested_speed = input.speed_counts_s;
        let mut c = self.tuning.adaptive.clone();
        // A tuned motor's braking was measured: automatic motion uses it for the
        // stopping envelope instead of crawling until stops are demonstrated.
        if let Some(a) = self.tuned_braking {
            c.fallback_deceleration_rad_s2 = c.fallback_deceleration_rad_s2.max(a * RAD);
            c.bootstrap_speed_rad_s = c.bootstrap_speed_rad_s.max(self.tuning.maximum_speed_counts_s * RAD);
        }
        let position_rad = inside as f64 * RAD;
        let bounds = [self.raw_lower as f64 * RAD, self.raw_upper as f64 * RAD];
        if self.previous_pwm.is_some_and(|p| p != input.pwm_limit) {
            self.adaptive.reset("Effort ceiling changed");
            self.context_changed = true;
        }
        self.previous_pwm = Some(input.pwm_limit);
        // Learning trials start at a fraction of the requested speed rather
        // than the commissioning crawl; the stopping-distance limit (fallback
        // deceleration until stops are measured) still caps each trial.
        if requested_command == MotionCommand::Learn {
            c.bootstrap_speed_rad_s = c.bootstrap_speed_rad_s.max(c.learning_start_fraction * requested_speed * RAD);
        }
        let mut command = command;
        let span = bounds[1] - bounds[0];
        let center = (bounds[0] + bounds[1]) * 0.5;
        if requested_command == MotionCommand::Learn {
            if !self.fully_taught {
                return Err("Teach both poses before learning response".into());
            }
            if !self.learning_active {
                self.learning_phase = 0;
                self.phase_started = time;
                self.learning_active = true;
            }
            if span < 8. * (c.boundary_margin_rad + c.position_uncertainty_rad) {
                return Err("Taught range too small for interior response learning".into());
            }
            if self.learning_phase == 3
                && (0..2).any(|d| {
                    self.adaptive.demonstrated_speed(&c, d, time) < requested_speed * RAD * 0.95
                })
            {
                self.learning_phase = 0;
            }
            match self.learning_phase {
                0 => {
                    command = MotionCommand::Target(center / RAD);
                    if (position_rad - center).abs() < 4. * RAD && velocity.abs() < 2. * RAD {
                        self.learning_phase = 1;
                        self.phase_started = time;
                    }
                }
                1 => {
                    command = MotionCommand::Jog(self.learning_direction);
                    if time - self.phase_started >= 2.
                        || (position_rad - center).abs() > span * 0.12
                    {
                        self.learning_phase = 2;
                        self.phase_started = time;
                        command = MotionCommand::Hold;
                    }
                }
                3 => {
                    command = MotionCommand::Hold;
                }
                _ => {
                    command = MotionCommand::Hold;
                    if time - self.phase_started > 1.
                        && !self.adaptive.measuring_stop()
                        && velocity.abs() < RAD
                        && (self.reference.angle_rad - position_rad).abs() < 3. * RAD
                    {
                        self.learning_direction = -self.learning_direction;
                        self.learning_phase = if (0..2).all(|d| {
                            self.adaptive.accepted_stops(&c, d, time) >= c.minimum_stops
                                && self.adaptive.demonstrated_speed(&c, d, time)
                                    >= requested_speed * RAD * 0.95
                        }) {
                            3
                        } else {
                            0
                        };
                        self.phase_started = time;
                    }
                }
            }
        } else {
            self.learning_active = false;
        }
        let direction = match command {
            MotionCommand::Jog(d) => usize::from((d > 0) != self.reverse),
            MotionCommand::Target(t) => usize::from(t * RAD > position_rad),
            MotionCommand::Track(_, v) => usize::from(v > 0.),
            MotionCommand::Sweep => usize::from(self.toward_upper != self.reverse),
            _ => usize::from(velocity >= 0.),
        };
        let exploring = self.learning_active && self.learning_phase == 1;
        let envelope = self.adaptive.envelope(
            &c,
            time,
            position_rad,
            velocity,
            bounds,
            direction,
            input.speed_counts_s * RAD,
            exploring,
        )?;
        let moving_direction = usize::from(velocity >= 0.);
        let measured_envelope = self.adaptive.envelope(
            &c,
            time,
            position_rad,
            velocity,
            bounds,
            moving_direction,
            input.speed_counts_s * RAD,
            false,
        )?;
        let mut input = input;
        let held_jog_limit = match requested_command {
            MotionCommand::Jog(_) => self.tuning.operator_jog_speed_limit_counts_s,
            MotionCommand::Track(..) if self.tuning.track_stopping_envelope == Some(false) => {
                Some(self.tuning.maximum_speed_counts_s)
            }
            _ => None,
        };
        if let Some(limit) = held_jog_limit {
            // The operator holds the key and watches the part: the requested
            // speed applies up to the explicit ceiling. Stopping-distance
            // slowing and the predictive brake belong to automatic motion and
            // learning; the taught pose remains a hard stop for the reference.
            input.speed_counts_s = requested_speed.min(limit).max(0.001);
        } else {
            input.speed_counts_s = (envelope.permitted_speed_rad_s / RAD).max(0.001);
            if !self.fully_taught {
                input.speed_counts_s = input.speed_counts_s.min(c.bootstrap_speed_rad_s / RAD);
            }
        }
        // A held jog away from a pose drives the part away; measured drift or
        // coasting toward the pose does not re-latch the brake against it.
        let measured_brake = measured_envelope.brake_now && held_jog_limit.is_none();
        let needs_braking = held_jog_limit.is_none()
            && (measured_brake || self.context_changed || envelope.permitted_speed_rad_s <= 1e-9);
        self.context_changed = false;
        if needs_braking && self.brake_latch.is_none() {
            self.brake_latch = Some(position_rad);
            self.brake_direction = if measured_brake {
                Some(moving_direction)
            } else if envelope.permitted_speed_rad_s <= 1e-9 {
                Some(direction)
            } else {
                None
            };
        }
        // An operator-held jog is never trapped by the latch. Once the measured
        // motion no longer needs braking, it resumes at the permitted speed in
        // either direction: away from a pose at once, toward it slowed to the
        // stopping-distance limit instead of refused.
        if self.brake_latch.is_some() && held_jog_limit.is_some() {
            self.brake_latch = None;
            self.brake_direction = None;
        }
        if let Some(target) = self.brake_latch {
            self.reference.angle_rad = target;
            self.reference.velocity_rad_s = 0.;
            command = MotionCommand::Hold;
            // Resume only a deliberate inward command, or reverse an automatic traversal after settling.
            // Released once stopped near the latch point; with a hold tolerance a
            // sticky axis may rest anywhere within it (it is not driven closer).
            let near = self.tuning.hold_deadband_counts.unwrap_or(0.).max(3.) * RAD;
            let at_rest = self.tuning.hold_deadband_counts.is_some() && !self.coasting && self.command == MotionCommand::Hold;
            if velocity.abs() < RAD && ((position_rad - target).abs() < near || at_rest) {
                let inward = if position_rad > center { 0 } else { 1 };
                if matches!(
                    requested_command,
                    MotionCommand::Sweep | MotionCommand::Learn
                ) {
                    if requested_command == MotionCommand::Sweep {
                        // Turning back before the pose (braked) still ends a half cycle.
                        let toward_upper = inward == usize::from(!self.reverse);
                        if toward_upper != self.toward_upper {
                            self.half_cycles += 1;
                        }
                        self.toward_upper = toward_upper;
                    } else {
                        self.learning_phase = 0;
                    }
                    self.brake_latch = None;
                } else if requested_command != MotionCommand::Hold && direction == inward {
                    self.brake_latch = None;
                }
            }
        }
        if command != MotionCommand::Hold {
            self.adaptive
                .constrain_observed_speed(input.speed_counts_s * RAD)?;
        }
        let completed_before = self.adaptive.completed_stops;
        if self.fully_taught {
            self.adaptive.observe(
                &c,
                time,
                position_rad,
                velocity,
                command == MotionCommand::Hold,
                input.pwm_limit as f64 / 1000.,
                bounds,
            )?;
        }
        let previous_command = self.command;
        self.command = command;
        match command {
            MotionCommand::Learn => unreachable!("Learning resolves to shared move/hold control"),
            MotionCommand::Sweep => {
                let endpoint = if self.toward_upper {
                    self.upper_pose
                } else {
                    self.lower_pose
                };
                if (self.reference.angle_rad - endpoint).abs() < RAD
                    && (position as f64 * RAD - endpoint).abs() < 4. * RAD
                    && velocity.abs() < 5. * RAD
                {
                    self.toward_upper = !self.toward_upper;
                    self.half_cycles += 1;
                    // Preserve learned gravity-holding effort through reversal.
                }
                let endpoint = if self.toward_upper {
                    self.upper_pose
                } else {
                    self.lower_pose
                };
                let governor = Governor {
                    period_s: dt,
                    maximum_speed_rad_s: input.speed_counts_s * RAD,
                    maximum_acceleration_rad_s2: self.tuning.maximum_acceleration_counts_s2 * RAD,
                    response_rate_per_s: self.tuning.response_rate_per_s,
                };
                self.reference = governor.update(self.reference, endpoint)?;
            }
            MotionCommand::Jog(direction) => {
                if direction != -1 && direction != 1 {
                    return Err("Jog direction must be -1 or 1".into());
                }
                self.toward_upper = direction > 0;
                let sign = direction as f64 * if self.reverse { -1. } else { 1. };
                let endpoint = if sign > 0. {
                    (self.raw_upper as f64 - 4.) * RAD
                } else {
                    (self.raw_lower as f64 + 4.) * RAD
                };
                let lead = held_jog_limit
                    .and(self.tuning.operator_jog_stop_lead_s)
                    .map_or(0., |s| s * self.reference.velocity_rad_s.abs());
                let distance = ((endpoint - self.reference.angle_rad) * sign - lead).max(0.);
                let acceleration = held_jog_limit
                    .and(self.tuning.operator_jog_acceleration_counts_s2)
                    .map_or(self.tuning.maximum_acceleration_counts_s2, |a| a.max(self.tuning.maximum_acceleration_counts_s2))
                    * RAD;
                let stopping = held_jog_limit
                    .and(self.tuning.operator_jog_deceleration_counts_s2)
                    .map_or(acceleration, |d| d * RAD)
                    .max(acceleration);
                let desired =
                    sign * (input.speed_counts_s * RAD).min((2. * stopping * distance).sqrt());
                // Speed up at the ordinary acceleration; slow down (release, pose) at the stopping rate.
                let (down, up) = if sign > 0. { (stopping, acceleration) } else { (acceleration, stopping) };
                self.reference.velocity_rad_s = desired.clamp(
                    self.reference.velocity_rad_s - down * dt,
                    self.reference.velocity_rad_s + up * dt,
                );
                self.reference.angle_rad += self.reference.velocity_rad_s * dt;
                if (endpoint - self.reference.angle_rad) * sign < 0. {
                    self.reference.angle_rad = endpoint;
                    self.reference.velocity_rad_s = 0.;
                }
            }
            MotionCommand::Hold => {
                let acceleration = self.tuning.maximum_acceleration_counts_s2 * RAD;
                if previous_command != MotionCommand::Hold {
                    // Discard any tracking backlog at release; brake from the measured pose.
                    self.reference.angle_rad = position as f64 * RAD;
                    self.reference.velocity_rad_s = 0.;
                    self.coasting = true;
                    self.coast_started = time;
                }
                // After a release the hold follows the part until it comes to
                // rest, then holds where it stopped: it never pulls back to the
                // release point. Only damping acts while it coasts.
                let here = position.clamp(self.raw_lower, self.raw_upper) as f64 * RAD;
                if self.coasting {
                    self.reference.angle_rad = here;
                    self.reference.velocity_rad_s = 0.;
                    // Only damping acts: the integral built up to drive the jog
                    // (e.g. against gravity) would otherwise keep the part
                    // moving at a steady creep that never counts as stopped.
                    self.pid.integral = 0.;
                    // A part that keeps moving is held where it is after a
                    // bounded coast, not followed indefinitely.
                    if (velocity / RAD).abs() < 20. || time - self.coast_started > MAXIMUM_COAST_S {
                        self.coasting = false;
                        self.hold_target_rad = here;
                    }
                } else if self.tuning.hold_deadband_counts.is_some() {
                    // Return to the hold pose by interpolating at a gentle speed,
                    // not by a step: a sticky axis then creeps instead of lurching.
                    let step = HOLD_RETURN_COUNTS_S * RAD * dt;
                    self.reference.angle_rad += (self.hold_target_rad - self.reference.angle_rad).clamp(-step, step);
                    self.reference.velocity_rad_s = 0.;
                } else {
                    let old = self.reference.velocity_rad_s;
                    self.reference.velocity_rad_s =
                        0f64.clamp(old - acceleration * dt, old + acceleration * dt);
                    self.reference.angle_rad += (old + self.reference.velocity_rad_s) * 0.5 * dt;
                }
            }
            MotionCommand::Track(raw, velocity) => {
                if !raw.is_finite() || !velocity.is_finite() || raw < self.raw_lower as f64 + 4. || raw > self.raw_upper as f64 - 4. {
                    return Err("Tracked reference outside taught working range".into());
                }
                self.reference.angle_rad = raw * RAD;
                self.reference.velocity_rad_s = velocity * RAD;
            }
            MotionCommand::Target(raw) => {
                if !raw.is_finite()
                    || raw < self.raw_lower as f64 + 4.
                    || raw > self.raw_upper as f64 - 4.
                {
                    return Err("Dial target outside taught working range".into());
                }
                self.reference = Governor {
                    period_s: dt,
                    maximum_speed_rad_s: input.speed_counts_s * RAD,
                    maximum_acceleration_rad_s2: self.tuning.maximum_acceleration_counts_s2 * RAD,
                    response_rate_per_s: self.tuning.response_rate_per_s,
                }
                .update(self.reference, raw * RAD)?;
            }
        }
        // Reference never wraps or requests a pose beyond the taught mechanical envelope.
        self.reference.angle_rad = self
            .reference
            .angle_rad
            .clamp(self.raw_lower as f64 * RAD, self.raw_upper as f64 * RAD);
        let mut error = self.reference.angle_rad / RAD - position as f64;
        if error.abs() > self.tuning.following_error_counts && command == MotionCommand::Hold {
            // A hold keeps its pose: resynchronising here would move the hold
            // to wherever an oscillation or a push happened to be.
            warnings.push(format!("Holding error above {:.0} counts", self.tuning.following_error_counts));
        } else if error.abs() > self.tuning.following_error_counts && matches!(command, MotionCommand::Track(..)) {
            // An externally timed reference (a gait) keeps its timing: moving it
            // to the motor would zero the drive and the motor would never catch up.
            warnings.push(format!("Tracking error {:.0} counts behind the tracked reference", error));
        } else if error.abs() > self.tuning.following_error_counts {
            // Retrack a move instead of stopping: the target restarts from the measured pose.
            warnings.push(format!("Tracking error {:.0} counts; target resynchronised to the motor", error));
            self.reference.angle_rad = (position.clamp(self.raw_lower, self.raw_upper)) as f64 * RAD;
            self.reference.velocity_rad_s = 0.;
            error = self.reference.angle_rad / RAD - position as f64;
        }
        let mut gains = self.tuning.pid.clone();
        gains.duty_limit = (input.pwm_limit as f64 / 1000.).min(gains.duty_limit);
        // A settled hold drives nothing and forgets its integral: friction holds
        // a stationary axis, and pushing a sticky one at breakaway effort only
        // makes it lurch past the target and hunt. Drift beyond the tolerance
        // is corrected again.
        let from_rest = self.hold_target_rad / RAD - position as f64;
        let was_settled = self.settled;
        self.settled = command == MotionCommand::Hold
            && !self.coasting
            && self.tuning.hold_deadband_counts.is_some_and(|d| {
                // Settle once stopped anywhere within the tolerance of the rest
                // pose; a sticky axis cannot be landed exactly, and chasing the
                // last counts hunts.
                from_rest.abs() <= d && (self.settled || (velocity / RAD).abs() < 20.)
            });
        if was_settled && !self.settled && command == MotionCommand::Hold {
            // Disturbed beyond the tolerance: the return starts from here.
            self.reference.angle_rad = position.clamp(self.raw_lower, self.raw_upper) as f64 * RAD;
        }
        // A stationary move target (a dial target that has arrived, or a
        // tracked reference that is momentarily still) gets the same
        // tolerance as a hold: chasing the last counts through backlash and
        // stiction is what makes a worm or sticky joint hunt.
        let still_target = matches!(command, MotionCommand::Target(_) | MotionCommand::Track(..))
            && (self.reference.velocity_rad_s / RAD).abs() < 1.
            && self.tuning.hold_deadband_counts.is_some_and(|d| (self.reference.angle_rad / RAD - position as f64).abs() <= d && (velocity / RAD).abs() < 20.);
        let settled = self.settled || still_target;
        let action = self.pid.step(
            &gains,
            if settled { position as f64 * RAD } else { self.reference.angle_rad },
            position as f64 * RAD,
            velocity,
            dt,
        )?;
        if settled {
            self.pid = PidState::default();
        }
        // Stick-slip: the integral that built up to break a stuck axis free is
        // far more than the friction once it moves. Drop it at breakaway so the
        // axis does not lurch past its target.
        let moving = (velocity / RAD).abs() >= 20.;
        if moving && !self.was_moving && self.friction_duty > 0. {
            self.pid.integral = 0.;
        }
        self.was_moving = moving;
        // While the reference moves, drive the speed it asks for directly
        // (measured gain plus friction in its direction), as the smooth
        // open-loop tuning steps did; feedback only trims the error. At rest,
        // friction help follows the error, smoothed over a few counts.
        let v_ref = self.reference.velocity_rad_s / RAD;
        let moving_ref = v_ref.abs() > 1.;
        let feed_forward = match self.speed_gain {
            Some(k) if moving_ref && !settled => v_ref / k + self.friction_duty * v_ref.signum(),
            _ => 0.,
        };
        let friction = if settled || (moving_ref && self.speed_gain.is_some()) { 0. } else { self.friction_duty * (error / 3.).tanh() };
        let mut duty = if settled { 0. } else { (action.duty + friction + feed_forward).clamp(-gains.duty_limit, gains.duty_limit) };
        if let Some(slew) = self.tuning.effort_slew_per_s {
            let step = slew * dt;
            duty = duty.clamp(self.last_duty - step, self.last_duty + step);
        }
        self.last_duty = duty;
        let mut pwm = (duty * 1000.).round() as i16;
        if position <= self.raw_lower && pwm < 0 || position >= self.raw_upper && pwm > 0 {
            pwm = 0;
        }
        if duty.abs() >= gains.duty_limit * 0.95 && error.abs() > 4. {
            let since = *self.saturated_since.get_or_insert(time);
            if time - since > self.tuning.stalled_at_limit_s
                && time - self.stationary_since > self.tuning.stalled_at_limit_s
            {
                // Back off and keep control: retrack so the drive does not keep pushing.
                warnings.push("No encoder progress at the PWM ceiling; target resynchronised".into());
                self.reference.angle_rad = (position.clamp(self.raw_lower, self.raw_upper)) as f64 * RAD;
                self.reference.velocity_rad_s = 0.;
                self.saturated_since = None;
                self.stationary_since = time;
            }
        } else {
            self.saturated_since = None;
        }
        self.last_time = time;
        Ok(SweepSample {
            elapsed_ms: (time * 1000.) as u128,
            position_raw: position.rem_euclid(4096) as u16,
            position_continuous: position,
            target_raw: self.reference.angle_rad / RAD,
            target_velocity_counts_s: self.reference.velocity_rad_s / RAD,
            velocity_counts_s: velocity / RAD,
            requested_speed_counts_s: requested_speed,
            pwm,
            toward_upper: self.toward_upper,
            half_cycles: self.half_cycles,
            warnings,
            holding: matches!(command, MotionCommand::Hold | MotionCommand::Target(_))
                && self.reference.velocity_rad_s.abs() < 0.01 * RAD
                && error.abs() < 3.,
            adaptation: AdaptationSample {
                status: if self.brake_latch.is_some() {
                    "Braking before boundary"
                } else if self.learning_active && self.learning_phase == 3 {
                    "Requested speed learned; holding position"
                } else if self.learning_active {
                    "Learning response in the middle of the range"
                } else if input.speed_counts_s + 0.01 < requested_speed {
                    "Speed limited by stopping evidence and clearance"
                } else {
                    "Within learned speed envelope"
                }
                .into(),
                permitted_speed_counts_s: input.speed_counts_s,
                stopping_distance_counts: measured_envelope.stopping_distance_rad / RAD,
                decreasing_stops: self.adaptive.accepted_stops(&c, 0, time),
                increasing_stops: self.adaptive.accepted_stops(&c, 1, time),
                decreasing_speed_counts_s: self.adaptive.demonstrated_speed(&c, 0, time) / RAD,
                increasing_speed_counts_s: self.adaptive.demonstrated_speed(&c, 1, time) / RAD,
                decreasing_acceleration_counts_s2: self.adaptive.directions[0].acceleration_rad_s2
                    / RAD,
                increasing_acceleration_counts_s2: self.adaptive.directions[1].acceleration_rad_s2
                    / RAD,
                braking: command == MotionCommand::Hold,
                learning: self.learning_active,
                learning_complete: self.learning_active && self.learning_phase == 3,
                evidence: if self.adaptive.completed_stops != completed_before {
                    Some(self.adaptive.clone())
                } else {
                    None
                },
            },
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn tuning() -> SweepTuning {
        SweepTuning {
            period_s: 0.02,
            maximum_feedback_interval_s: 0.1,
            maximum_speed_counts_s: 500.,
            maximum_acceleration_counts_s2: 100.,
            response_rate_per_s: 2.,
            following_error_counts: 64.,
            velocity_filter_s: 0.08,
            stalled_at_limit_s: 1.,
            pid: Pid {
                kp: 5.2,
                ki: 0.8,
                kd: 0.04,
                integral_limit: 0.125,
                duty_limit: 1.,
            },
            provenance: "Synthetic test; not a physical fit".into(),
            adaptive: AdaptiveConfig {
                bootstrap_speed_rad_s: 500. * RAD,
                fallback_deceleration_rad_s2: 1000. * RAD,
                reaction_time_s: 0.1,
                position_uncertainty_rad: RAD,
                boundary_margin_rad: 4. * RAD,
                learning_inset_fraction: 0.25,
                minimum_stops: 3,
                braking_safety_factor: 0.5,
                trial_speed_growth: 1.25,
                learning_start_fraction: 0.25,
                maximum_evidence_age_s: 120.,
            },
            operator_jog_speed_limit_counts_s: None,
            operator_jog_deceleration_counts_s2: None,
            operator_jog_stop_lead_s: None,
            hold_deadband_counts: None,
            effort_slew_per_s: None,
            operator_jog_acceleration_counts_s2: None,
            track_stopping_envelope: None,
        }
    }
    fn axis(reverse: bool) -> AxisCalibration {
        AxisCalibration {
            role: "test".into(),
            lower: Some(if reverse { 1300 } else { 1000 }),
            upper: Some(if reverse { 1000 } else { 1300 }),
            reverse,
            ..Default::default()
        }
    }
    #[test]
    fn follows_both_orientations_and_live_speed_changes() {
        for reverse in [false, true] {
            let mut sweep = RangeSweep::new(&axis(reverse), 1150, 40, tuning()).unwrap();
            let mut position: f64 = 1150.;
            let mut previous_target = 1150.;
            let mut max_cycles = 0;
            for i in 1..=6000 {
                let speed = if i < 1500 { 40. } else { 15. };
                let s = sweep
                    .update(
                        i as f64 * 0.02,
                        position.round() as i32,
                        SweepInput {
                            speed_counts_s: speed,
                            pwm_limit: 100,
                        },
                    )
                    .unwrap();
                assert!((1000.0..=1300.0).contains(&s.target_raw));
                assert!(s.pwm.abs() <= 100);
                assert!((s.target_raw - previous_target).abs() <= 40. * 0.02 + 1e-6);
                position = s.target_raw;
                previous_target = s.target_raw;
                max_cycles = max_cycles.max(s.half_cycles);
            }
            assert!(max_cycles >= 4);
        }
    }
    #[test]
    fn refuses_missing_limits_and_warns_on_lag_and_stalled_motor() {
        assert!(RangeSweep::new(&AxisCalibration::default(), 1100, 40, tuning()).is_err());
        let mut s = RangeSweep::new(&axis(false), 1150, 40, tuning()).unwrap();
        let late = s.update(0.3, 1150, SweepInput { speed_counts_s: 5., pwm_limit: 100 }).unwrap();
        assert!(late.warnings.iter().any(|w| w.contains("Feedback gap")), "{:?}", late.warnings);
        let mut s = RangeSweep::new(&axis(false), 1150, 40, tuning()).unwrap();
        // A motor that never moves is warned about and retracked, never stopped.
        let mut warned = Vec::new();
        for i in 1..2000 {
            let sample = s.update(i as f64 * 0.02, 1150, SweepInput { speed_counts_s: 50., pwm_limit: 25 }).unwrap();
            warned.extend(sample.warnings);
            assert!((sample.target_raw - 1150.).abs() <= 64.5, "retracking keeps the target near the motor");
        }
        assert!(warned.iter().any(|w| w.contains("Tracking error") || w.contains("No encoder progress")), "{warned:?}");
    }
    #[test]
    fn held_jog_follows_requested_speed_and_stops_at_taught_pose() {
        // Commissioning-like assumptions: 5 counts/s bootstrap, slow fallback braking.
        let run = |limit: Option<f64>| {
            let mut t = tuning();
            t.adaptive.bootstrap_speed_rad_s = 5. * RAD;
            t.adaptive.fallback_deceleration_rad_s2 = 10. * RAD;
            t.adaptive.reaction_time_s = 0.25;
            t.operator_jog_speed_limit_counts_s = limit;
            t.operator_jog_deceleration_counts_s2 = Some(1000.);
            t.operator_jog_stop_lead_s = Some(0.1);
            let mut wide = axis(false);
            (wide.lower, wide.upper) = (Some(1000), Some(3000));
            let mut c = RangeSweep::teaching(&wide, 1600, t).unwrap();
            let (mut p, mut v, mut middle, mut last) = (1600f64, 0f64, 0f64, 0f64);
            for i in 1..6000 {
                let s = match c.control(i as f64 * 0.02, p.round() as i32, SweepInput { speed_counts_s: 60., pwm_limit: 1000 }, MotionCommand::Jog(1)) { Ok(s) => s, Err(e) => panic!("{e} at p={p:.1} v={v:.1} limit={limit:?} step {i}") };
                v += (2. * s.pwm as f64 - v) * 0.02 / 0.08;
                p += v * 0.02;
                assert!(s.target_raw <= 2996., "reference passed the taught upper pose: {}", s.target_raw);
                assert!(p < 3005., "part far past the taught upper pose: {p}");
                if (2700.0..2950.0).contains(&p) { middle = middle.max(v); }
                last = p;
            }
            (middle, last)
        };
        let (held, end) = run(Some(100.));
        assert!(held > 45., "held jog keeps the requested 60 counts/s close to the pose: {held}");
        assert!(end > 2985., "held jog arrives at the pose: {end}");
        let (gated, _) = run(None);
        assert!(gated < 15., "without the operator limit, jogs stay at the bootstrap crawl: {gated}");
    }
    #[test]
    fn supervised_gait_tracking_can_skip_the_passive_braking_envelope() {
        // Slow passive-braking assumption, as for a tuned motor coasting; the
        // tracked reference swings across most of the taught range at speed.
        let run = |envelope: Option<bool>| {
            let mut t = tuning();
            t.adaptive.fallback_deceleration_rad_s2 = 600. * RAD;
            t.maximum_speed_counts_s = 3000.;
            t.track_stopping_envelope = envelope;
            let mut wide = axis(false);
            (wide.lower, wide.upper) = (Some(1000), Some(3000));
            let mut c = RangeSweep::new(&wide, 2000, 40, t).unwrap();
            let (mut p, mut v, mut fastest, mut brakes) = (2000f64, 0f64, 0f64, 0);
            for i in 1..1500 {
                let time = i as f64 * 0.02;
                let (raw, rate) = (2000. + 800. * (time * 1.5).sin(), 1200. * (time * 1.5).cos());
                let s = c.control(time, p.round() as i32, SweepInput { speed_counts_s: rate.abs() + 60., pwm_limit: 1000 }, MotionCommand::Track(raw, rate)).unwrap();
                v += (2. * s.pwm as f64 - v) * 0.02 / 0.08;
                p += v * 0.02;
                assert!((1000.0..3000.0).contains(&p), "part left the taught poses: {p}");
                brakes += usize::from(s.adaptation.status == "Braking before boundary");
                fastest = fastest.max(v.abs());
            }
            (fastest, brakes)
        };
        let (fast, brakes) = run(Some(false));
        assert_eq!(brakes, 0, "no predictive brake on supervised tracking");
        assert!(fast > 800., "follows the gait's speed: {fast}");
        let (_, gated) = run(None);
        assert!(gated > 0, "by default the passive-braking envelope still brakes tracked motion");
    }
    #[test]
    fn held_jog_moves_away_from_a_new_pose_at_full_requested_speed() {
        // Commissioning-like assumptions and a synthetic gravity load, not a hardware fit.
        let mut t = tuning();
        t.adaptive.bootstrap_speed_rad_s = 5. * RAD;
        t.adaptive.fallback_deceleration_rad_s2 = 10. * RAD;
        t.adaptive.reaction_time_s = 0.25;
        t.operator_jog_speed_limit_counts_s = Some(500.);
        let mut untaught = axis(false);
        (untaught.lower, untaught.upper) = (None, None);
        let mut c = RangeSweep::teaching(&untaught, 2000, t).unwrap();
        // Plant state: position, velocity (counts, counts/s) and time.
        let mut x = [2000f64, 0., 0.];
        fn run(c: &mut RangeSweep, x: &mut [f64; 3], command: MotionCommand, speed: f64, steps: usize) -> f64 {
            let mut fastest = 0f64;
            for _ in 0..steps {
                x[2] += 0.02;
                let s = c.control(x[2], x[0].round() as i32, SweepInput { speed_counts_s: speed, pwm_limit: 1000 }, command).unwrap();
                x[1] += (2. * s.pwm as f64 - 20. - x[1]) * 0.02 / 0.08;
                x[0] += x[1] * 0.02;
                fastest = fastest.max(x[1].abs());
            }
            fastest
        }
        run(&mut c, &mut x, MotionCommand::Jog(1), 100., 150);
        run(&mut c, &mut x, MotionCommand::Hold, 100., 100);
        // Save the upper pose here: a context change latches the brake.
        let mut taught = untaught.clone();
        taught.upper = Some(x[0].round() as i32);
        c.set_taught_bounds(&taught).unwrap();
        let upper = x[0];
        run(&mut c, &mut x, MotionCommand::Hold, 100., 5);
        let away = run(&mut c, &mut x, MotionCommand::Jog(-1), 300., 150);
        assert!(x[0] < upper - 300., "jog away from the new upper pose must move: {upper} -> {}", x[0]);
        assert!(away > 250., "jog away from the pose must reach the requested speed: {away}");
        let low = x[0];
        run(&mut c, &mut x, MotionCommand::Jog(1), 300., 600);
        assert!(x[0] > low + 100. && x[0] < upper, "jog back toward the pose moves and stops short of it: {low} -> {} < {upper}", x[0]);
    }
    #[test]
    fn held_jog_toward_a_pose_is_not_braked_by_the_learning_envelope() {
        // Scripted tracking with one stick-slip lurch toward the only taught
        // pose, faster than the learning envelope could stop. A manual jog is
        // never held by the predictive brake: it keeps going and stops at the pose.
        let mut t = tuning();
        t.adaptive.bootstrap_speed_rad_s = 5. * RAD;
        t.adaptive.fallback_deceleration_rad_s2 = 10. * RAD;
        t.adaptive.reaction_time_s = 0.25;
        t.operator_jog_speed_limit_counts_s = Some(500.);
        t.operator_jog_deceleration_counts_s2 = Some(1000.);
        t.operator_jog_stop_lead_s = Some(0.1);
        let mut upper_only = axis(false);
        (upper_only.lower, upper_only.upper) = (None, Some(3000));
        let mut c = RangeSweep::teaching(&upper_only, 2700, t).unwrap();
        let (mut target, mut braked) = (2700f64, false);
        for i in 1..=300 {
            let time = i as f64 * 0.02;
            let p = target + if (1.0..1.1).contains(&time) { 20. } else { 0. };
            let s = c.control(time, p.round() as i32, SweepInput { speed_counts_s: 100., pwm_limit: 1000 }, MotionCommand::Jog(1)).unwrap();
            braked |= s.adaptation.braking;
            target = s.target_raw;
            assert!(target <= 2996., "reference passed the taught pose: {target}");
        }
        assert!(!braked, "manual jogs are never held by the predictive brake");
        assert!(target > 2950., "holding the key reaches the pose: {target}");
    }
    /// Hold with full effort, 70 ms loop period and a lagging, delayed,
    /// stick-slip synthetic plant (the conditions that made the belt axis hunt).
    fn hold_peak_to_peak(period: f64, deadband: Option<f64>, slew: Option<f64>, tuned: Option<super::super::calibration::MotorTuning>) -> f64 {
        let mut t = tuning();
        t.pid = serde_json::from_value(serde_json::from_str::<serde_json::Value>(include_str!("../../../../examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json")).unwrap()["sweep_tuning"]["pid"].clone()).unwrap();
        t.maximum_feedback_interval_s = 0.12;
        t.response_rate_per_s = 2.;
        t.hold_deadband_counts = deadband;
        t.effort_slew_per_s = slew;
        let mut wide = axis(false);
        (wide.lower, wide.upper) = (Some(1000), Some(3000));
        wide.tuning = tuned;
        let mut c = RangeSweep::teaching(&wide, 2000, t).unwrap();
        let (mut p, mut v, mut delayed) = (2000f64, 500f64, std::collections::VecDeque::from(vec![0f64; 2]));
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        let n = (8.4 / period) as usize;
        for i in 1..=n {
            // Released at speed on the first period, then held; bumped 40 counts at period 30.
            if i == n / 4 { p += 40.; }
            let command = if i == 1 { MotionCommand::Jog(1) } else { MotionCommand::Hold };
            let s = c.control(i as f64 * period, p.round() as i32, SweepInput { speed_counts_s: 100., pwm_limit: 1000 }, command).unwrap();
            delayed.push_back(s.pwm as f64);
            let applied = delayed.pop_front().unwrap();
            // Stick-slip: 20% duty to break away, 6% friction once moving.
            let drive = (applied.abs() - 60.).max(0.) * applied.signum() * 1.8;
            for _ in 0..(period * 100.).round() as usize {
                if v.abs() < 5. && applied.abs() < 200. {
                    v = 0.;
                } else {
                    v += (drive - v) * 0.01 / 0.05;
                }
                p += v * 0.01;
            }
            if i > n / 2 { lo = lo.min(p); hi = hi.max(p); }
        }
        hi - lo
    }
    #[test]
    fn hold_deadband_and_effort_slew_calm_hunting() {
        use super::super::motor_identification::{design, StepFit};
        let before = hold_peak_to_peak(0.07, None, None, None);
        // Gains designed from this plant's step response (1800 counts/s per
        // duty beyond 6% friction, ~50 ms lag) and the 70 ms loop period.
        let fits: Vec<_> = [0.3, -0.3, 0.5, -0.5].iter().map(|d: &f64| StepFit {
            duty: *d, speed_counts_s: (d.abs() - 0.06) * 1800. * d.signum(), time_constant_s: 0.06 }).collect();
        let tuned = design(&fits, [0.2, 0.2], 0.07, 0.08, "synthetic").unwrap();
        let slow = hold_peak_to_peak(0.07, Some(16.), Some(4.), Some(tuned.clone()));
        // The moving axis now runs near a 30 ms period; its gains are designed for that.
        let fits30: Vec<_> = fits.clone();
        let tuned30 = design(&fits30, [0.2, 0.2], 0.03, 0.08, "synthetic").unwrap();
        let fast = hold_peak_to_peak(0.03, Some(16.), Some(4.), Some(tuned30));
        eprintln!("hold peak-to-peak after a 40-count bump: shared gains {before:.1}; tuned at 70 ms {slow:.1}; tuned at 30 ms {fast:.1} counts");
        assert!(before > 100., "shared gains at full effort hunt in this plant: {before}");
        assert!(slow < before / 4., "tuning cuts hunting at the slow period: {slow}");
        assert!(fast <= 40., "at 30 ms the tuned hold settles back within the bump: {fast}");
    }
    /// Held jog at 300 counts/s on a plant fitted to the worm's measured
    /// response (3290 counts/s per duty, 60 ms lag, 6% breakaway, 4.5% moving
    /// friction), 40 ms period, one period of command delay. Returns the
    /// number of drive direction reversals and the speed spread while cruising.
    fn jog_roughness(tuned: bool) -> (usize, f64, f64) {
        use super::super::motor_identification::{design, StepFit};
        let mut t = tuning();
        t.operator_jog_speed_limit_counts_s = Some(500.);
        t.operator_jog_acceleration_counts_s2 = Some(1500.);
        t.operator_jog_deceleration_counts_s2 = Some(1000.);
        t.operator_jog_stop_lead_s = Some(0.1);
        t.effort_slew_per_s = Some(4.);
        t.hold_deadband_counts = Some(16.);
        t.maximum_feedback_interval_s = 0.12;
        let mut wide = axis(false);
        (wide.lower, wide.upper) = (Some(0), Some(4000));
        if tuned {
            let fits: Vec<_> = [0.15, -0.15, 0.3, -0.3, 0.5, -0.5].iter().map(|d: &f64| StepFit {
                duty: *d, speed_counts_s: (d.abs() - 0.045) * 3290. * d.signum(), time_constant_s: 0.06 }).collect();
            wide.tuning = Some(design(&fits, [0.06, 0.06], 0.04, 0.08, "worm-like").unwrap());
        }
        let mut c = RangeSweep::teaching(&wide, 500, t).unwrap();
        let (mut p, mut v, mut pending) = (500f64, 0f64, 0f64);
        let (mut reversals, mut last_sign, mut speeds) = (0, 0f64, Vec::new());
        for i in 1..=100 {
            let s = c.control(i as f64 * 0.04, p.round() as i32, SweepInput { speed_counts_s: 300., pwm_limit: 1000 }, MotionCommand::Jog(1)).unwrap();
            let applied = std::mem::replace(&mut pending, s.pwm as f64 / 1000.);
            if applied != 0. && applied.signum() != last_sign {
                if last_sign != 0. { reversals += 1; }
                last_sign = applied.signum();
            }
            for _ in 0..4 {
                let stuck = v.abs() < 5.;
                let friction = if stuck { 0.06 } else { 0.045 };
                let drive = if stuck && applied.abs() < friction { 0. } else { (applied.abs() - friction).max(0.) * applied.signum() * 3290. };
                v += (drive - v) * 0.01 / 0.06;
                p += v * 0.01;
            }
            if i > 40 { speeds.push(v); }
        }
        let mean = speeds.iter().sum::<f64>() / speeds.len() as f64;
        let spread = (speeds.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / speeds.len() as f64).sqrt();
        (reversals, mean, spread)
    }
    #[test]
    fn tuned_feed_forward_makes_held_jogs_smooth() {
        let (rough_rev, rough_mean, rough_spread) = jog_roughness(false);
        let (rev, mean, spread) = jog_roughness(true);
        eprintln!("jog at 300 counts/s: untuned {rough_rev} reversals, {rough_mean:.0}±{rough_spread:.0}; tuned {rev} reversals, {mean:.0}±{spread:.0} counts/s");
        assert_eq!(rev, 0, "tuned drive never reverses while jogging one way");
        assert!((mean - 300.).abs() < 30., "reaches the requested speed: {mean}");
        assert!(spread < 20., "cruises smoothly: ±{spread}");
        assert!(spread < rough_spread, "smoother than the untuned loop");
    }
    #[test]
    fn tuned_motor_sweeps_its_range_at_speed_inside_the_poses() {
        use super::super::motor_identification::{design, StepFit};
        let mut t = serde_json::from_value::<SweepTuning>(serde_json::from_str::<serde_json::Value>(include_str!("../../../../examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json")).unwrap()["sweep_tuning"].clone()).unwrap();
        t.maximum_feedback_interval_s = 0.12;
        let mut a = axis(false);
        (a.lower, a.upper) = (Some(500), Some(2500));
        let fits: Vec<_> = [0.15, -0.15, 0.3, -0.3, 0.5, -0.5].iter().map(|d: &f64| StepFit {
            duty: *d, speed_counts_s: (d.abs() - 0.045) * 3290. * d.signum(), time_constant_s: 0.06 }).collect();
        a.tuning = Some(design(&fits, [0.066, 0.066], 0.035, 0.08, "worm-like").unwrap());
        let mut c = RangeSweep::teaching(&a, 1500, t).unwrap();
        let (mut p, mut v, mut pending, mut cycles) = (1500f64, 0f64, 0f64, 0);
        for i in 1..=600 {
            let s = c.control(i as f64 * 0.035, p.round() as i32, SweepInput { speed_counts_s: 300., pwm_limit: 1000 }, MotionCommand::Sweep).unwrap();
            let applied = std::mem::replace(&mut pending, s.pwm as f64 / 1000.);
            for _ in 0..35 {
                let stuck = v.abs() < 5.;
                let f = if stuck { 0.066 } else { 0.045 };
                let drive = if stuck && applied.abs() < f { 0. } else { (applied.abs() - f).max(0.) * applied.signum() * 3290. };
                v += (drive - v) * 0.001 / 0.06;
                p += v * 0.001;
            }
            assert!((500.0..=2500.).contains(&p), "left the taught poses: {p}");
            cycles = s.half_cycles;
        }
        assert!(cycles >= 2, "a tuned motor crosses its 2000-count range within 21 s: {cycles} half-cycles");
    }
    #[test]
    fn closed_loop_synthetic_lag_tracks_crawl_and_reversals() {
        // Explicit first-order synthetic actuator, not a model of the printed leg.
        // Tests feedback correction, bounded effort and reversals under plant lag.
        let mut sweep = RangeSweep::new(&axis(false), 1150, 40, tuning()).unwrap();
        let (mut position, mut velocity) = (1150.0f64, 0.0f64);
        let mut cycles = 0;
        for i in 1..=12000 {
            let speed = if i < 4000 {
                5.
            } else if i < 8000 {
                30.
            } else {
                3.
            };
            let s = sweep
                .update(
                    i as f64 * 0.02,
                    position.round() as i32,
                    SweepInput {
                        speed_counts_s: speed,
                        pwm_limit: 200,
                    },
                )
                .unwrap();
            velocity += (2. * s.pwm as f64 - velocity) * 0.02 / 0.08;
            position += velocity * 0.02;
            assert!((1000.0..1300.0).contains(&position));
            assert!((s.target_raw - position).abs() < 10.);
            cycles = s.half_cycles;
        }
        assert!(cycles >= 5);
    }
    #[test]
    fn teaching_release_discards_backlog_and_holds_against_synthetic_gravity() {
        // Constant synthetic load and first-order response, not a hardware fit.
        for reversed in [false, true] {
            let mut c = RangeSweep::teaching(&axis(reversed), 1150, tuning()).unwrap();
            let input = SweepInput {
                speed_counts_s: 20.,
                pwm_limit: 200,
            };
            let (mut p, mut v) = (1150f64, 0f64);
            let mut held = 0.;
            for i in 1..4000 {
                let command = if i < 150 {
                    MotionCommand::Jog(1)
                } else {
                    MotionCommand::Hold
                };
                let sample = c
                    .control(i as f64 * 0.02, p.round() as i32, input, command)
                    .unwrap();
                v += (2. * sample.pwm as f64 - 30. - v) * 0.02 / 0.08;
                p += v * 0.02;
                assert!((1000.0..1300.0).contains(&p));
                if i == 250 {
                    held = sample.target_raw;
                }
                if i > 3500 {
                    assert!(sample.holding);
                    assert!((p - held).abs() < 2., "holding error {}", p - held);
                    assert!(sample.pwm > 5, "must retain effort against load at rest");
                    assert!(
                        (sample.target_raw - held).abs() < 1e-9,
                        "hold target must not drift"
                    );
                }
            }
        }
    }
    #[test]
    fn manual_partial_limits_reverse_direction_and_release_no_backlog() {
        let a = AxisCalibration {
            role: "test".into(),
            lower: Some(1500),
            reverse: true,
            ..Default::default()
        };
        let mut c = RangeSweep::teaching(&a, 1490, tuning()).unwrap();
        let input = SweepInput {
            speed_counts_s: 20.,
            pwm_limit: 100,
        };
        let mut s = c.control(0.02, 1490, input, MotionCommand::Jog(1)).unwrap();
        assert!(s.target_raw < 1490.);
        for i in 2..40 {
            s = c
                .control(i as f64 * 0.02, 1490, input, MotionCommand::Jog(1))
                .unwrap();
        }
        assert!(s.target_raw < 1480.);
        let held = c.control(0.8, 1490, input, MotionCommand::Hold).unwrap();
        assert_eq!(held.target_raw, 1490.); // discard the old requested travel
        assert!(held.holding);
        let outside = c.control(0.82, 1501, input, MotionCommand::Hold).unwrap();
        assert!(outside.warnings.iter().any(|w| w.contains("Outside taught poses")));
        assert!(outside.pwm <= 0, "no drive further outward past the taught pose");
        assert!(RangeSweep::teaching(&a, 2, tuning()).is_ok());
    }
    #[test]
    fn online_learning_and_predictive_braking_under_asymmetric_synthetic_load() {
        let mut config = tuning();
        config.adaptive=serde_json::from_value(serde_json::from_str::<serde_json::Value>(include_str!("../../../../examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json")).unwrap()["sweep_tuning"]["adaptive"].clone()).unwrap();
        let a = AxisCalibration {
            role: "synthetic loaded joint".into(),
            lower: Some(1000),
            upper: Some(3000),
            ..Default::default()
        };
        let mut ctl = RangeSweep::teaching(&a, 2000, config).unwrap();
        ctl.set_taught_bounds(&a).unwrap();
        let (mut position, mut velocity) = (2000.0f64, 0.0f64);
        let mut learned_speed = 0f64;
        let mut stops = 0;
        let mut predictive_brakes = 0;
        for i in 1..=35000 {
            let command = if i <= 22000 {
                MotionCommand::Learn
            } else if i < 28500 {
                MotionCommand::Jog(1)
            } else {
                MotionCommand::Jog(-1)
            };
            let sample = ctl
                .control(
                    i as f64 * 0.02,
                    position.round() as i32,
                    SweepInput {
                        speed_counts_s: 120.,
                        pwm_limit: 300,
                    },
                    command,
                )
                .unwrap();
            // Explicit synthetic plant: direction-dependent damping, fixed gravitational load,
            // weaker response late in the trial, and one encoder count quantization.
            let tau = if i > 26000 {
                0.16
            } else if velocity > 0. {
                0.08
            } else {
                0.12
            };
            velocity += (2. * sample.pwm as f64 - 30. - velocity) * 0.02 / tau;
            position += velocity * 0.02;
            assert!(
                (1000.0..3000.0).contains(&position),
                "hard envelope violated at sample {i}: {position}"
            );
            if i < 22000 {
                assert!(
                    (1500.0..2500.0).contains(&position),
                    "identification must remain in the middle: sample {i} at {position:.0}, {:?} {:?}", sample.adaptation.status, sample.target_raw
                );
            }
            learned_speed = learned_speed.max(
                sample
                    .adaptation
                    .decreasing_speed_counts_s
                    .min(sample.adaptation.increasing_speed_counts_s),
            );
            stops = stops.max(
                sample
                    .adaptation
                    .decreasing_stops
                    .min(sample.adaptation.increasing_stops),
            );
            predictive_brakes += usize::from(sample.adaptation.status == "Braking before boundary");
        }
        assert!(
            stops >= 3,
            "need repeatable evidence in both directions: {stops}"
        );
        // Trials start at a quarter of the requested 120 counts/s instead of
        // the 5 counts/s commissioning crawl and grow from measured stops.
        eprintln!("learned {learned_speed:.0} counts/s, {stops} stops each way");
        assert!(
            learned_speed >= 0.9 * 120.,
            "learning must reach the requested speed: {learned_speed}"
        );
        assert!(
            predictive_brakes > 0,
            "must brake before reaching hard boundaries"
        );
    }
}
