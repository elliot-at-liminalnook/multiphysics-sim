//! Empirical constant-twist prediction from timed planar poses.
//!
//! This is a kinematic surrogate for screening long-horizon displacement, not
//! a contact or actuator model. Fit only past observations and validate future
//! endpoints separately. Unobserved turns greater than pi between samples
//! cannot be recovered; callers must supply sufficiently frequent observations.
use crate::stepping::advance_planar;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimedPlanarPose {
    pub time_s: f64,
    /// World x/y in metres, heading in radians.
    pub pose: [f64; 3],
}

/// A forecast window bound to a vector of measured or sampled responses.
/// Reusing an index preserves known identity between windows (for example,
/// their shared last observed position) when propagating model uncertainty.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanarResponseWindow {
    /// Relative/world x and y in metres, followed by heading in radians.
    pub pose: [usize; 3],
    /// Body-frame vx/vy in m/s, followed by yaw rate in rad/s.
    pub twist: [usize; 3],
}

impl PlanarResponseWindow {
    pub fn predict(&self, responses: &[f64], elapsed_s: f64) -> Result<[f64; 3], String> {
        if !elapsed_s.is_finite() || elapsed_s < 0. {
            return Err("finite nonnegative response forecast interval required".into());
        }
        let select = |indices: [usize; 3]| -> Result<[f64; 3], String> {
            let mut values = [0.; 3];
            for (value, index) in values.iter_mut().zip(indices) {
                *value = *responses
                    .get(index)
                    .ok_or("response index outside supplied vector")?;
                if !value.is_finite() {
                    return Err("nonfinite planar response".into());
                }
            }
            Ok(values)
        };
        let pose = advance_planar(select(self.pose)?, select(self.twist)?, elapsed_s);
        if pose.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite response forecast".into());
        }
        Ok(pose)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PlanarPrediction {
    /// Body-frame x/y velocity (m/s) and heading rate (rad/s).
    pub twist: [f64; 3],
    pub duration_s: f64,
    pub intervals: usize,
    pub observed_path_length_m: f64,
    pub observed_yaw_change_rad: f64,
}

/// Mean motion with a least-squares heading trend. Repeated yaw motion and
/// observation noise can otherwise bias the endpoint heading difference and
/// the orientation used for long-horizon extrapolation. This is still an
/// empirical constant-motion approximation, not a guarantee of periodicity.
#[derive(Clone, Debug, Serialize)]
pub struct PlanarTrendPrediction {
    pub motion: PlanarPrediction,
    /// Last observed position, with heading replaced by its fitted trend.
    pub anchor: TimedPlanarPose,
    pub heading_residual_rms_rad: f64,
}

impl PlanarTrendPrediction {
    pub fn fit(samples: &[TimedPlanarPose]) -> Result<Self, String> {
        let mut motion = PlanarPrediction::fit(samples)?;
        let mut headings = Vec::with_capacity(samples.len());
        let mut heading = samples[0].pose[2];
        headings.push(heading);
        for pair in samples.windows(2) {
            let delta = pair[1].pose[2] - pair[0].pose[2];
            heading += delta.sin().atan2(delta.cos());
            headings.push(heading);
        }
        let origin_time = samples[0].time_s;
        // Centered online covariance avoids subtracting large world times.
        let (mut mean_t, mut mean_heading, mut time_square, mut cross) = (0., 0., 0., 0.);
        for (i, (sample, heading)) in samples.iter().zip(&headings).enumerate() {
            let t = sample.time_s - origin_time;
            let dt = t - mean_t;
            let dh = heading - mean_heading;
            mean_t += dt / (i + 1) as f64;
            mean_heading += dh / (i + 1) as f64;
            time_square += dt * (t - mean_t);
            cross += dt * (heading - mean_heading);
        }
        let rate = cross / time_square;
        let mut anchor = *samples.last().unwrap();
        anchor.pose[2] = mean_heading + rate * (anchor.time_s - origin_time - mean_t);
        let residual = samples
            .iter()
            .zip(&headings)
            .map(|(sample, heading)| {
                let fitted = mean_heading + rate * (sample.time_s - origin_time - mean_t);
                (heading - fitted).powi(2)
            })
            .sum::<f64>();
        let heading_residual_rms_rad = (residual / samples.len() as f64).sqrt();
        if !rate.is_finite() || !anchor.pose[2].is_finite() || !heading_residual_rms_rad.is_finite()
        {
            return Err("nonfinite heading trend".into());
        }
        motion.twist[2] = rate;
        Ok(Self {
            motion,
            anchor,
            heading_residual_rms_rad,
        })
    }

    /// Extrapolate from the end of the fitting window, using the fitted mean
    /// heading. The raw pose remains available in the caller's observations.
    pub fn predict(&self, time_s: f64) -> Result<[f64; 3], String> {
        self.motion.predict(self.anchor, time_s)
    }
}

impl PlanarPrediction {
    /// Average interval SE(2) logarithms, weighted by elapsed time. Rotation
    /// and translation are coupled rather than fitting world-axis velocities.
    pub fn fit(samples: &[TimedPlanarPose]) -> Result<Self, String> {
        if samples.len() < 2
            || samples
                .iter()
                .any(|s| !s.time_s.is_finite() || s.pose.iter().any(|x| !x.is_finite()))
        {
            return Err("at least two finite timed poses required".into());
        }
        let mut integrated = [0.; 3];
        let mut path_length = 0.;
        for pair in samples.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            let dt = b.time_s - a.time_s;
            if !dt.is_finite() || dt <= 0. {
                return Err("strictly increasing finite sample times required".into());
            }
            let raw_angle = b.pose[2] - a.pose[2];
            let angle = raw_angle.sin().atan2(raw_angle.cos());
            let (s, c) = a.pose[2].sin_cos();
            let dx = b.pose[0] - a.pose[0];
            let dy = b.pose[1] - a.pose[1];
            let local = [c * dx + s * dy, -s * dx + c * dy];
            // Inverse left Jacobian: V(theta)^-1 = [[k,h],[-h,k]].
            let h = angle / 2.;
            let k = if angle.abs() < 1e-5 {
                1. - angle * angle / 12. - angle.powi(4) / 720.
            } else {
                h / h.tan()
            };
            integrated[0] += k * local[0] + h * local[1];
            integrated[1] += -h * local[0] + k * local[1];
            integrated[2] += angle;
            path_length += dx.hypot(dy);
        }
        let duration_s = samples.last().unwrap().time_s - samples[0].time_s;
        let twist = integrated.map(|x| x / duration_s);
        if !duration_s.is_finite()
            || !path_length.is_finite()
            || twist.iter().any(|x| !x.is_finite())
        {
            return Err("nonfinite planar fit".into());
        }
        Ok(Self {
            twist,
            duration_s,
            intervals: samples.len() - 1,
            observed_path_length_m: path_length,
            observed_yaw_change_rad: integrated[2],
        })
    }

    pub fn predict(&self, anchor: TimedPlanarPose, time_s: f64) -> Result<[f64; 3], String> {
        let dt = time_s - anchor.time_s;
        if !anchor.time_s.is_finite()
            || !time_s.is_finite()
            || !dt.is_finite()
            || dt < 0.
            || anchor
                .pose
                .iter()
                .chain(self.twist.iter())
                .any(|x| !x.is_finite())
        {
            return Err("finite pose and nonnegative prediction horizon required".into());
        }
        let pose = advance_planar(anchor.pose, self.twist, dt);
        if pose.iter().any(|x| !x.is_finite()) {
            return Err("nonfinite planar prediction".into());
        }
        Ok(pose)
    }
}
