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
        })
    }
    pub fn update(
        &mut self,
        time: f64,
        position: u16,
        input: SweepInput,
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
        if action.duty.abs() >= gains.duty_limit * 0.95 {
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
}
