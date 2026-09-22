//! Reusable, measured encoder-range traversal using shared governor/PID components.
//! No robot topology, inferred joint angles, or plant physics.
use super::calibration::AxisCalibration;
use serde::{Deserialize, Serialize};
use sim_domain_control::{
    pwm_feedback::{EncoderEstimate, Pid, PidState},
    reference_governor::{Config as Governor, State as Reference},
};
const RAD: f64 = std::f64::consts::TAU / 4096.;
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
    pub target_raw: f64,
    pub velocity_counts_s: f64,
    pub requested_speed_counts_s: f64,
    pub pwm: i16,
    pub toward_upper: bool,
    pub half_cycles: u64,
    pub holding: bool,
}
/// Intent in the part coordinate frame. Hold preserves feedback effort against load.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MotionCommand {
    Sweep,
    Jog(i8),
    Hold,
    Target(f64),
}
pub struct RangeSweep {
    tuning: SweepTuning,
    pub raw_lower: u16,
    pub raw_upper: u16,
    lower_pose: f64,
    upper_pose: f64,
    toward_upper: bool,
    reference: Reference,
    estimate: EncoderEstimate,
    pid: PidState,
    last_time: f64,
    stationary_since: f64,
    stationary_position: u16,
    saturated_since: Option<f64>,
    half_cycles: u64,
    command: MotionCommand,
    reverse: bool,
}
impl RangeSweep {
    pub fn new(
        axis: &AxisCalibration,
        position: u16,
        clearance: u16,
        tuning: SweepTuning,
    ) -> Result<Self, String> {
        axis.validate()?;
        tuning.validate()?;
        let (lo, hi) = axis.encoder_bounds();
        let (lo, hi) = (
            lo.ok_or("Teach both poses before sweeping")?,
            hi.ok_or("Teach both poses before sweeping")?,
        );
        if clearance < 4
            || hi - lo <= clearance.saturating_mul(2).saturating_add(8)
            || lo < 8
            || hi > 4087
            || !(lo..=hi).contains(&position)
        {
            return Err("Sweep needs a non-wrapping taught range wider than twice its clearance plus 8 counts, and the motor inside that range".into());
        }
        let (a, b) = ((lo + clearance) as f64 * RAD, (hi - clearance) as f64 * RAD);
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
        })
    }
    pub fn teaching(
        axis: &AxisCalibration,
        position: u16,
        tuning: SweepTuning,
    ) -> Result<Self, String> {
        axis.validate()?;
        let (lo, hi) = axis.encoder_bounds();
        let lo = lo.unwrap_or(8).max(8);
        let hi = hi.unwrap_or(4087).min(4087);
        if hi <= lo + 16 || !(lo..=hi).contains(&position) {
            return Err("Position is outside saved poses or encoder wrap guard; reset the appropriate pose to re-teach".into());
        }
        // These fallback endpoints are encoder wrap guards, not inferred mechanical limits.
        let mut envelope = axis.clone();
        envelope.lower = Some(if axis.reversed() { hi } else { lo });
        envelope.upper = Some(if axis.reversed() { lo } else { hi });
        let mut c = Self::new(&envelope, position, 4, tuning)?;
        c.command = MotionCommand::Hold;
        Ok(c)
    }
    pub fn set_taught_bounds(&mut self, axis: &AxisCalibration) -> Result<(), String> {
        axis.validate()?;
        let (lo, hi) = axis.encoder_bounds();
        let lo = lo.unwrap_or(8).max(8);
        let hi = hi.unwrap_or(4087).min(4087);
        if hi <= lo + 8 {
            return Err("Taught envelope is too narrow".into());
        }
        if axis.reversed() != self.reverse {
            return Err(
                "Saved pose would reverse the taught direction; swap direction explicitly first"
                    .into(),
            );
        }
        self.raw_lower = lo;
        self.raw_upper = hi;
        self.reverse = axis.reversed();
        let margin = ((hi - lo) / 4).min(64).max(4);
        let (a, b) = ((lo + margin) as f64 * RAD, (hi - margin) as f64 * RAD);
        (self.lower_pose, self.upper_pose) = if self.reverse { (b, a) } else { (a, b) };
        Ok(())
    }
    pub fn update(
        &mut self,
        time: f64,
        position: u16,
        input: SweepInput,
    ) -> Result<SweepSample, String> {
        self.control(time, position, input, MotionCommand::Sweep)
    }
    pub fn control(
        &mut self,
        time: f64,
        position: u16,
        input: SweepInput,
        command: MotionCommand,
    ) -> Result<SweepSample, String> {
        input.validate(&self.tuning)?;
        let dt = time - self.last_time;
        if !dt.is_finite() || dt <= 0. || dt > self.tuning.maximum_feedback_interval_s {
            return Err("Sweep feedback deadline exceeded; no catch-up command sent".into());
        }
        if !(self.raw_lower..=self.raw_upper).contains(&position) {
            return Err("Encoder left taught range".into());
        }
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
        let previous_command = self.command;
        self.command = command;
        match command {
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
                    self.pid = PidState::default();
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
                let distance = ((endpoint - self.reference.angle_rad) * sign).max(0.);
                let acceleration = self.tuning.maximum_acceleration_counts_s2 * RAD;
                let desired =
                    sign * (input.speed_counts_s * RAD).min((2. * acceleration * distance).sqrt());
                self.reference.velocity_rad_s = desired.clamp(
                    self.reference.velocity_rad_s - acceleration * dt,
                    self.reference.velocity_rad_s + acceleration * dt,
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
                    self.reference.velocity_rad_s = velocity;
                }
                let old = self.reference.velocity_rad_s;
                self.reference.velocity_rad_s =
                    0f64.clamp(old - acceleration * dt, old + acceleration * dt);
                self.reference.angle_rad += (old + self.reference.velocity_rad_s) * 0.5 * dt;
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
        let error = self.reference.angle_rad / RAD - position as f64;
        if error.abs() > self.tuning.following_error_counts {
            return Err(
                "Sweep following error exceeded; check clearance, load and PWM ceiling".into(),
            );
        }
        let mut gains = self.tuning.pid.clone();
        gains.duty_limit = (input.pwm_limit as f64 / 1000.).min(gains.duty_limit);
        let action = self.pid.step(
            &gains,
            self.reference.angle_rad,
            position as f64 * RAD,
            velocity,
            dt,
        )?;
        let mut pwm = (action.duty * 1000.).round() as i16;
        if position <= self.raw_lower && pwm < 0 || position >= self.raw_upper && pwm > 0 {
            pwm = 0;
        }
        if action.duty.abs() >= gains.duty_limit * 0.95 && error.abs() > 4. {
            let since = *self.saturated_since.get_or_insert(time);
            if time - since > self.tuning.stalled_at_limit_s
                && time - self.stationary_since > self.tuning.stalled_at_limit_s
            {
                return Err("No encoder progress at PWM ceiling; sweep stopped".into());
            }
        } else {
            self.saturated_since = None;
        }
        self.last_time = time;
        Ok(SweepSample {
            elapsed_ms: (time * 1000.) as u128,
            position_raw: position,
            target_raw: self.reference.angle_rad / RAD,
            velocity_counts_s: velocity / RAD,
            requested_speed_counts_s: input.speed_counts_s,
            pwm,
            toward_upper: self.toward_upper,
            half_cycles: self.half_cycles,
            holding: matches!(command, MotionCommand::Hold | MotionCommand::Target(_))
                && self.reference.velocity_rad_s.abs() < 0.01 * RAD
                && error.abs() < 3.,
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
                        position.round() as u16,
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
    fn refuses_missing_limits_lag_and_stalled_motor() {
        assert!(RangeSweep::new(&AxisCalibration::default(), 1100, 40, tuning()).is_err());
        let mut s = RangeSweep::new(&axis(false), 1150, 40, tuning()).unwrap();
        assert!(
            s.update(
                0.3,
                1150,
                SweepInput {
                    speed_counts_s: 5.,
                    pwm_limit: 100
                }
            )
            .is_err()
        );
        let mut s = RangeSweep::new(&axis(false), 1150, 40, tuning()).unwrap();
        let mut fault = None;
        for i in 1..2000 {
            if let Err(e) = s.update(
                i as f64 * 0.02,
                1150,
                SweepInput {
                    speed_counts_s: 50.,
                    pwm_limit: 25,
                },
            ) {
                fault = Some(e);
                break;
            }
        }
        assert!(fault.is_some());
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
                    position.round() as u16,
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
                    .control(i as f64 * 0.02, p.round() as u16, input, command)
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
        assert!(c.control(0.82, 1501, input, MotionCommand::Hold).is_err());
        assert!(RangeSweep::teaching(&a, 2, tuning()).is_err());
    }
}
