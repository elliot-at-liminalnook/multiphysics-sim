//! Per-motor feedback gains from a measured open-loop PWM response.
//!
//! The axis is modelled as an integrating plant with a first-order lag and a
//! loop delay, `angle' = K·(duty − friction)` reached with time constant τ.
//! Gains follow the SIMC rule for that plant with a deliberately slow
//! closed-loop time (twice the delay), favouring smooth, well-damped motion
//! over stiffness. The fit is a commissioning estimate from a few short steps,
//! not a validated model of the loaded joint.
use super::calibration::MotorTuning;
use serde::{Deserialize, Serialize};
use sim_domain_control::pwm_feedback::Pid;

const RAD: f64 = std::f64::consts::TAU / 4096.;

/// One constant-duty step: (seconds since the step began, continuous counts).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepTrace {
    pub duty: f64,
    pub samples: Vec<(f64, i32)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StepFit {
    pub duty: f64,
    /// Signed steady speed reached during the step.
    pub speed_counts_s: f64,
    /// Time to 63% of that speed, including loop delay.
    pub time_constant_s: f64,
}

/// Steady speed from the second half of the step; time constant from the
/// first crossing of 63% of it. None if the axis barely moved.
pub fn fit_step(trace: &StepTrace) -> Option<StepFit> {
    fit_step_with(trace, false)
}
/// [`fit_step`], or with `first_order` a least-squares first-order fit with
/// dead time over the whole trace, so a step that ends before steady speed
/// still gives its final speed (used by the characterization campaign).
/// Steps shorter than 2.5 time constants are rejected there rather than
/// extrapolated. The Tune path keeps the simpler second-half mean.
pub fn fit_step_with(trace: &StepTrace, first_order: bool) -> Option<StepFit> {
    let s = &trace.samples;
    if s.len() < 6 {
        return None;
    }
    let (t0, p0) = s[0];
    let (t_end, p_end) = *s.last()?;
    let mid = s.iter().position(|(t, _)| *t >= t0 + 0.5 * (t_end - t0))?;
    let (t_mid, p_mid) = s[mid];
    if t_end - t_mid < 0.05 || (p_end - p0).abs() < 12 {
        return None;
    }
    let mean = (p_end - p_mid) as f64 / (t_end - t_mid);
    if mean.abs() < 5. || mean.signum() != trace.duty.signum() {
        return None;
    }
    if first_order {
        // A short step ends before steady speed. Fit the whole trace to a
        // first-order rise after a dead time d:
        //   p(t) = p0 + v∞·(t' − τ(1 − e^(−t'/τ))),  t' = max(0, t − d),
        // v∞ by least squares for each (τ, d) on a grid, (τ, d) by residual.
        let mut best = (f64::INFINITY, mean, 0.05);
        for ti in 0..100 {
            let tau = 0.01 * 1.04f64.powi(ti);
            for di in 0..=20 {
                let dead = di as f64 * 0.005;
                let (mut bb, mut by) = (0., 0.);
                let basis: Vec<(f64, f64)> = s.iter().map(|(t, p)| {
                    let tp = (t - t0 - dead).max(0.);
                    (tp - tau * (1. - (-tp / tau).exp()), (p - p0) as f64)
                }).collect();
                for (b, y) in &basis { bb += b * b; by += b * y; }
                if bb <= 0. { continue; }
                let v = by / bb;
                let err: f64 = basis.iter().map(|(b, y)| (y - v * b).powi(2)).sum();
                if err < best.0 { best = (err, v, tau); }
            }
        }
        // Too short to identify: the fit extrapolates beyond what it saw.
        if t_end - t0 < 2.5 * best.2 {
            return None;
        }
        if best.1.signum() == trace.duty.signum() && best.0.is_finite() {
            return Some(StepFit { duty: trace.duty, speed_counts_s: best.1, time_constant_s: best.2.clamp(0.01, 1.0) });
        }
    }
    let speed = mean;
    let mut tau = t_end - t0;
    for w in s.windows(3) {
        let v = (w[2].1 - w[0].1) as f64 / (w[2].0 - w[0].0);
        if v * speed.signum() >= 0.63 * speed.abs() {
            tau = w[1].0 - t0;
            break;
        }
    }
    Some(StepFit { duty: trace.duty, speed_counts_s: speed, time_constant_s: tau.clamp(0.01, 1.0) })
}

/// Median of a sample set (None if empty).
pub fn median_of(v: &[f64]) -> Option<f64> {
    median(v.to_vec())
}
fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    Some(v[v.len() / 2])
}

/// Design gains from step fits. `breakaway` is the duty at which the axis first
/// moved, [toward decreasing, toward increasing] counts. `loop_delay_s` is the
/// measured control period and `velocity_filter_s` the damping filter.
pub fn design(
    fits: &[StepFit],
    breakaway: [f64; 2],
    loop_delay_s: f64,
    velocity_filter_s: f64,
    record: &str,
) -> Result<MotorTuning, String> {
    // Per direction, speed = K·(|duty| − kinetic friction). With two step sizes
    // the slope gives K and the intercept the friction once moving, which is
    // lower than breakaway (stick-slip). With one size, breakaway is used.
    let mut gains = Vec::new();
    let mut kinetic = Vec::new();
    for positive in [false, true] {
        let mut d: Vec<&StepFit> = fits.iter().filter(|f| (f.duty > 0.) == positive).collect();
        d.sort_by(|a, b| a.duty.abs().total_cmp(&b.duty.abs()));
        let (Some(low), Some(high)) = (d.first(), d.last()) else { continue };
        let span = high.duty.abs() - low.duty.abs();
        let slope = (high.speed_counts_s.abs() - low.speed_counts_s.abs()) / span;
        if span > 0.05 && slope > 0. {
            gains.push(slope);
            kinetic.push((high.duty.abs() - high.speed_counts_s.abs() / slope).clamp(0., breakaway[usize::from(positive)]));
        } else {
            for f in d {
                let effective = f.duty.abs() - breakaway[usize::from(positive)];
                if effective > 0.02 {
                    gains.push(f.speed_counts_s.abs() / effective);
                    kinetic.push(breakaway[usize::from(positive)]);
                }
            }
        }
    }
    let k = median(gains).ok_or("No step moved clearly beyond friction; raise the PWM ceiling or check for binding")?;
    let kinetic = median(kinetic).unwrap_or(0.);
    let tau = median(fits.iter().map(|f| f.time_constant_s).collect()).unwrap_or(0.05);
    if !loop_delay_s.is_finite() || loop_delay_s <= 0. || !k.is_finite() {
        return Err("Invalid identification timing".into());
    }
    // Delay seen by the loop: one control period, half a period of hold, and
    // half the velocity filter used for damping.
    let theta = 1.5 * loop_delay_s + 0.5 * velocity_filter_s;
    let closed_loop = 2. * theta;
    let k_rad = k * RAD;
    let kp = (1. / (k_rad * (closed_loop + theta))).clamp(0.2, 40.);
    let integral_time = 4. * (closed_loop + theta);
    let ki = (kp / integral_time).clamp(0., 20.);
    let kd = (kp * tau).clamp(0., 5.);
    Ok(MotorTuning {
        pid: Pid {
            kp,
            ki,
            kd,
            // At most 25% duty from the integral (gravity hold).
            integral_limit: if ki > 0. { 0.25 / ki } else { 0.1 },
            duty_limit: 1.,
        },
        // Feed-forward only the friction present while moving: pushing at
        // breakaway level would lurch past the target once the axis frees.
        friction_duty: kinetic.clamp(0., 0.3),
        gain_counts_s_per_duty: k,
        time_constant_s: tau,
        loop_delay_s,
        breakaway_duty: breakaway,
        record: record.into(),
        method: "SIMC PID for an integrating plant with first-order lag; closed-loop time 2× delay; friction feed-forward from the kinetic (moving) friction".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// First-order speed response with friction, sampled every 25 ms.
    fn synthetic(duty: f64, k: f64, tau: f64, friction: f64) -> StepTrace {
        let (mut p, mut v, mut samples) = (0f64, 0f64, Vec::new());
        for i in 0..24 {
            let t = i as f64 * 0.025;
            samples.push((t, p.round() as i32));
            let drive = (duty.abs() - friction).max(0.) * duty.signum() * k;
            v += (drive - v) * 0.025 / tau;
            p += v * 0.025;
        }
        StepTrace { duty, samples }
    }

    #[test]
    fn recovers_gain_and_lag_then_designs_damped_gains() {
        let fits: Vec<_> = [0.3, -0.3, 0.5, -0.5]
            .iter()
            .map(|d| fit_step(&synthetic(*d, 1200., 0.06, 0.08)).unwrap())
            .collect();
        for f in &fits {
            assert!(f.speed_counts_s.signum() == f.duty.signum());
            assert!((0.03..0.15).contains(&f.time_constant_s), "{f:?}");
        }
        let t = design(&fits, [0.08, 0.08], 0.03, 0.08, "test.json").unwrap();
        assert!((t.gain_counts_s_per_duty - 1200.).abs() < 150., "{}", t.gain_counts_s_per_duty);
        // A faster motor gets a smaller proportional gain; damping follows the lag.
        let fast = design(&fits.iter().map(|f| StepFit { speed_counts_s: 2. * f.speed_counts_s, ..f.clone() }).collect::<Vec<_>>(), [0.08, 0.08], 0.03, 0.08, "x").unwrap();
        assert!(fast.pid.kp < t.pid.kp);
        assert!(t.pid.kd > 0. && t.pid.ki > 0.);
        assert!((t.friction_duty - 0.08).abs() < 0.02, "kinetic friction {}", t.friction_duty);
    }

    #[test]
    fn refuses_steps_that_never_beat_friction() {
        let stuck: Vec<_> = [0.1, -0.1].iter().filter_map(|d| fit_step(&synthetic(*d, 1200., 0.06, 0.2))).collect();
        assert!(stuck.is_empty());
        assert!(design(&stuck, [0.2, 0.2], 0.03, 0.08, "x").is_err());
    }
}
