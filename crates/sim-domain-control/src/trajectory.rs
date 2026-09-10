//! Validated, deterministic reference trajectories. Values retain the units and
//! channel order declared by the caller; derivatives use seconds as time units.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    #[default]
    Linear,
    /// Quintic position interpolation with zero velocity/acceleration at knots.
    QuinticRestToRest,
    /// Uniform periodic cubic B-spline through control points (not interpolation
    /// knots). Requires >=4 unique controls, t0=0 and a repeated final control.
    /// Values stay in their convex hull; rates/accelerations are continuous.
    PeriodicCubicBSpline,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    pub time_s: f64,
    pub values: Vec<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrajectoryConfig {
    #[serde(default)]
    pub interpolation: Interpolation,
    pub keyframes: Vec<Keyframe>,
}
#[derive(Clone, Debug)]
pub struct Trajectory {
    config: TrajectoryConfig,
    dimension: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct TrajectorySample {
    pub values: Vec<f64>,
    pub rates: Vec<f64>,
    pub accelerations: Vec<f64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct RateTraversalCell {
    pub reference_start_s: f64,
    pub reference_end_s: f64,
    pub duration_lower_s: f64,
    pub duration_upper_s: f64,
    pub limiting_coordinate: usize,
}
/// Bracket for minimum traversal time with independent constant absolute rate
/// budgets only. Omits acceleration, forces, contact, dwell and phase-rate
/// continuity. The upper construction changes phase rate at cell boundaries;
/// it is a screening calculation, not an acceleration-feasible controller.
#[derive(Clone, Debug, Serialize)]
pub struct RateTraversalBounds {
    pub reference_duration_s: f64,
    pub duration_lower_s: f64,
    pub duration_upper_s: f64,
    pub uniform_duration_s: f64,
    pub cells: Vec<RateTraversalCell>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateRedistributionConfig {
    pub subdivisions_per_segment: usize,
    pub output_controls: usize,
    /// 0 preserves original phase spacing; <1 retains positive cell duration.
    pub blend: f64,
    /// Original control indices which retain their reference times before
    /// output B-spline smoothing; include zero and the repeated final index.
    pub anchor_indices: Vec<usize>,
    /// Optional intervals of original control indices to retime per channel.
    /// Empty channel intervals retain its original phase throughout. Every
    /// interval endpoint must be an anchor so source phase meets identity there.
    #[serde(default)]
    pub coordinate_intervals: Option<Vec<Vec<[usize; 2]>>>,
}
#[derive(Clone, Debug, Serialize)]
pub struct RateRedistribution {
    pub trajectory: TrajectoryConfig,
    /// Redistributed source phase for active channels. Inactive channels use
    /// the new keyframe's original time, as specified by coordinate_intervals.
    pub source_phases_s: Vec<f64>,
}
impl Trajectory {
    pub fn new(config: TrajectoryConfig) -> Result<Self, String> {
        let dimension = config.keyframes.first().map_or(0, |k| k.values.len());
        if dimension == 0
            || config.keyframes.iter().any(|k| {
                !k.time_s.is_finite()
                    || k.time_s < 0.0
                    || k.values.len() != dimension
                    || k.values.iter().any(|v| !v.is_finite())
            })
        {
            return Err(
                "trajectory requires finite, nonempty, consistently sized keyframes".into(),
            );
        }
        for w in config.keyframes.windows(2) {
            let h = w[1].time_s - w[0].time_s;
            if h <= 0.0
                || !h.is_finite()
                || w[0].values.iter().zip(&w[1].values).any(|(a, b)| {
                    let d = b - a;
                    !d.is_finite() || !(d / h * 1.875).is_finite() || !(d / h / h * 6.0).is_finite()
                })
            {
                return Err(
                    "trajectory times must increase and derivatives must remain finite".into(),
                );
            }
        }
        if matches!(config.interpolation, Interpolation::PeriodicCubicBSpline) {
            let ks = &config.keyframes;
            if ks.len() < 5 || ks[0].time_s != 0.0 || ks[0].values != ks.last().unwrap().values {
                return Err("periodic cubic B-spline requires four unique controls, zero start time and a repeated final control".into());
            }
            let h = ks.last().unwrap().time_s / (ks.len() - 1) as f64;
            if ks
                .iter()
                .enumerate()
                .any(|(i, k)| (k.time_s - i as f64 * h).abs() > h * 1e-10)
            {
                return Err("periodic cubic B-spline controls must be uniformly timed".into());
            }
        }
        Ok(Self { config, dimension })
    }
    /// Double a uniform periodic cubic control grid without changing its curve.
    /// This is knot insertion, not sampling the curve as new controls.
    pub fn refined_periodic_config(&self) -> Result<TrajectoryConfig, String> {
        if !matches!(self.config.interpolation, Interpolation::PeriodicCubicBSpline) {
            return Err("periodic cubic refinement requires periodic cubic controls".into());
        }
        let ks = &self.config.keyframes;
        let n = ks.len() - 1;
        let period = ks[n].time_s;
        let mut keyframes = Vec::with_capacity(2 * n + 1);
        for i in 0..n {
            for half in 0..2 {
                let values = (0..self.dimension).map(|j| {
                    let a = ks[(i + n - 1) % n].values[j];
                    let b = ks[i].values[j];
                    let c = ks[(i + 1) % n].values[j];
                    if half == 0 { b + ((a - b) + (c - b)) / 8. }
                    else { b + (c - b) / 2. }
                }).collect();
                keyframes.push(Keyframe { time_s: (2 * i + half) as f64 * period / (2 * n) as f64, values });
            }
        }
        keyframes.push(Keyframe { time_s: period, values: keyframes[0].values.clone() });
        let config = TrajectoryConfig { interpolation: Interpolation::PeriodicCubicBSpline, keyframes };
        Self::new(config.clone())?;
        Ok(config)
    }
    /// Linear/rest-to-rest laws stay between adjacent endpoints; the periodic
    /// B-spline stays in the convex hull of its four neighboring controls.
    /// These bounds therefore cover reference values throughout time,
    /// not the achieved motion, dependent coordinates or collision clearance.
    pub fn validate_value_bounds(
        &self,
        bounds: &[(Option<f64>, Option<f64>)],
    ) -> Result<(), String> {
        if bounds.len() != self.dimension
            || bounds.iter().any(|(lo, hi)| {
                lo.is_some_and(|x| !x.is_finite())
                    || hi.is_some_and(|x| !x.is_finite())
                    || lo.zip(*hi).is_some_and(|(lo, hi)| lo > hi)
            })
        {
            return Err("invalid trajectory value bounds".into());
        }
        for k in &self.config.keyframes {
            for (i, (value, (lo, hi))) in k.values.iter().zip(bounds).enumerate() {
                if lo.is_some_and(|lo| *value < lo) || hi.is_some_and(|hi| *value > hi) {
                    return Err(format!(
                        "trajectory coordinate {i} at {}s = {value} exceeds bounds {lo:?}..{hi:?}",
                        k.time_s
                    ));
                }
            }
        }
        Ok(())
    }
    pub fn dimension(&self) -> usize {
        self.dimension
    }
    /// Affine value transformation about explicit per-channel centers, in the
    /// original channel units. Derivatives scale by the same factors. This
    /// changes the authored reference, never physical actuator limits. Negative
    /// and zero scales are allowed. An identity scale preserves exact values.
    pub fn affine_values(&self, scales: &[f64], centers: &[f64]) -> Result<TrajectoryConfig, String> {
        if scales.len() != self.dimension || centers.len() != self.dimension
            || scales.iter().chain(centers).any(|v| !v.is_finite()) {
            return Err("trajectory affine transform requires finite per-channel scales and centers".into());
        }
        let mut config = self.config.clone();
        for k in &mut config.keyframes {
            for (i, value) in k.values.iter_mut().enumerate() {
                *value = crate::motion_parameters::affine_value(*value, scales[i], centers[i], 0.)?;
            }
        }
        Self::new(config.clone())?;
        Ok(config)
    }
    /// Shift periodic channels by integer control intervals. Positive shifts
    /// advance the authored curve: new(t) = old(t + shift * period / controls).
    /// This is an exact control permutation, not resampling or smoothing. The
    /// caller owns conversion from requested physical-time offsets to integers.
    pub fn shifted_periodic_controls(&self, shifts: &[i64]) -> Result<TrajectoryConfig, String> {
        if !matches!(self.config.interpolation, Interpolation::PeriodicCubicBSpline)
            || shifts.len() != self.dimension {
            return Err("periodic shifts require a periodic cubic curve and one integer per channel".into());
        }
        let n = self.config.keyframes.len() - 1;
        let count = i64::try_from(n).map_err(|_| "periodic control count exceeds integer range")?;
        let mut config = self.config.clone();
        for (j, shift) in shifts.iter().enumerate() {
            let shift = shift.rem_euclid(count) as usize;
            for i in 0..n {
                // Avoid overflow in i+shift, even for very large valid grids.
                let source = if i >= n-shift { i-(n-shift) } else { i+shift };
                config.keyframes[i].values[j] = self.config.keyframes[source].values[j];
            }
            config.keyframes[n].values[j] = config.keyframes[0].values[j];
        }
        Self::new(config.clone())?;
        Ok(config)
    }
    /// Exact per-channel absolute rate maxima of the declared polynomial curve,
    /// including interior extrema. These are reference rates, not actuator rates.
    pub fn maximum_absolute_rates(&self) -> Result<Vec<f64>, String> {
        let mut maxima = vec![0.0_f64; self.dimension];
        for i in 0..self.config.keyframes.len() - 1 {
            for (maximum, peak) in maxima.iter_mut().zip(self.segment_rate_maxima(i, 0., 1.)?) {
                *maximum = maximum.max(peak);
            }
        }
        Ok(maxima)
    }
    fn segment_rate_maxima(&self, i: usize, start: f64, end: f64) -> Result<Vec<f64>, String> {
        let ks = &self.config.keyframes;
        let h = ks[i + 1].time_s - ks[i].time_s;
        (0..self.dimension)
            .map(|j| {
                let peak = match self.config.interpolation {
                    Interpolation::Linear => ((ks[i + 1].values[j] - ks[i].values[j]) / h).abs(),
                    Interpolation::QuinticRestToRest => {
                        let u = 0.5_f64.clamp(start, end);
                        ((ks[i + 1].values[j] - ks[i].values[j]) / h
                            * (30. * u * u * (1. - u) * (1. - u)))
                            .abs()
                    }
                    Interpolation::PeriodicCubicBSpline => {
                        let n = ks.len() - 1;
                        let h = ks[n].time_s / n as f64;
                        let d0 = ks[i].values[j] - ks[(i + n - 1) % n].values[j];
                        let d1 = ks[(i + 1) % n].values[j] - ks[i].values[j];
                        let d2 = ks[(i + 2) % n].values[j] - ks[(i + 1) % n].values[j];
                        let b = (d0 + d1) / 2.;
                        let c = (d1 - d0) / 2.;
                        let d = (d0 - 2. * d1 + d2) / 6.;
                        let rate = |u: f64| ((b + u * (2. * c + u * 3. * d)) / h).abs();
                        let mut peak = rate(start).max(rate(end));
                        if d != 0. {
                            let root = -c / (3. * d);
                            if (start..end).contains(&root) {
                                peak = peak.max(rate(root));
                            }
                        }
                        peak
                    }
                };
                if !peak.is_finite() {
                    return Err("nonfinite trajectory rate bound".into());
                }
                Ok(peak)
            })
            .collect()
    }
    /// Let q(s) be this path and b_i the positive rate budgets (value units/s).
    /// The rate-only optimum is integral max_i |dq_i/ds|/b_i ds. On each cell,
    /// max_i |delta q_i|/b_i bounds it below, and cell_width times the exact
    /// polynomial rate maximum bounds it above. Refinement brackets switching
    /// bottlenecks and reversals without assuming midpoint samples are extrema.
    /// Results use f64 arithmetic, not outward-rounded interval arithmetic.
    /// Initial/final holds and initial time offset are excluded.
    pub fn rate_traversal_bounds(
        &self,
        budgets: &[f64],
        subdivisions_per_segment: usize,
    ) -> Result<RateTraversalBounds, String> {
        let ks = &self.config.keyframes;
        let count = (ks.len() - 1).checked_mul(subdivisions_per_segment);
        if budgets.len() != self.dimension
            || budgets.iter().any(|b| !b.is_finite() || *b <= 0.)
            || subdivisions_per_segment == 0
            || !count.is_some_and(|n| n <= 100_000)
        {
            return Err("positive finite per-coordinate rate budgets, positive subdivision count and at most 100000 traversal cells required".into());
        }
        let duration = ks.last().unwrap().time_s - ks[0].time_s;
        let uniform_rate = self
            .maximum_absolute_rates()?
            .iter()
            .zip(budgets)
            .map(|(r, b)| r / b)
            .fold(0.0_f64, f64::max);
        let mut result = RateTraversalBounds {
            reference_duration_s: duration,
            duration_lower_s: 0.,
            duration_upper_s: 0.,
            uniform_duration_s: duration * uniform_rate,
            cells: Vec::with_capacity(count.unwrap()),
        };
        for i in 0..ks.len() - 1 {
            let h = ks[i + 1].time_s - ks[i].time_s;
            for k in 0..subdivisions_per_segment {
                let start = k as f64 / subdivisions_per_segment as f64;
                let end = (k + 1) as f64 / subdivisions_per_segment as f64;
                let t0 = ks[i].time_s + start * h;
                let t1 = if k + 1 == subdivisions_per_segment {
                    ks[i + 1].time_s
                } else {
                    ks[i].time_s + end * h
                };
                if t1 <= t0 {
                    return Err("traversal cell times lost floating-point resolution".into());
                }
                let a = self.sample(t0)?.values;
                let b = self.sample(t1)?.values;
                let lower = a
                    .iter()
                    .zip(b)
                    .zip(budgets)
                    .map(|((a, b), budget)| (b - a).abs() / budget)
                    .fold(0.0_f64, f64::max);
                let peaks = self.segment_rate_maxima(i, start, end)?;
                let mut limiting = 0;
                let mut maximum = 0.0_f64;
                for (j, (peak, budget)) in peaks.iter().zip(budgets).enumerate() {
                    let rate = peak / budget;
                    if rate > maximum {
                        maximum = rate;
                        limiting = j;
                    }
                }
                let upper = (end - start) * h * maximum;
                if !lower.is_finite() || !upper.is_finite() || lower > upper + 1e-10 * upper.max(1.)
                {
                    return Err("nonfinite or inconsistent traversal bounds".into());
                }
                result.duration_lower_s += lower;
                result.duration_upper_s += upper;
                result.cells.push(RateTraversalCell {
                    reference_start_s: t0,
                    reference_end_s: t1,
                    duration_lower_s: lower,
                    duration_upper_s: upper,
                    limiting_coordinate: limiting,
                });
            }
        }
        if ![
            result.duration_lower_s,
            result.duration_upper_s,
            result.uniform_duration_s,
        ]
        .iter()
        .all(|v| v.is_finite())
        {
            return Err("nonfinite total traversal duration".into());
        }
        Ok(result)
    }
    /// Redistribute phase duration according to the rate-only upper envelope,
    /// separately between explicit anchors. Resample the original curve as
    /// uniform B-spline controls for a C2 periodic result. This smooths (changes)
    /// the path: anchors apply to sampled source phases, not exact output poses.
    /// Collision, lift timing, torque and stability require new validation.
    pub fn redistribute_periodic_rates(
        &self,
        budgets: &[f64],
        config: &RateRedistributionConfig,
    ) -> Result<RateRedistribution, String> {
        let ks = &self.config.keyframes;
        let n = ks.len() - 1;
        if !matches!(
            self.config.interpolation,
            Interpolation::PeriodicCubicBSpline
        ) || !config.blend.is_finite()
            || !(0. ..1.).contains(&config.blend)
            || !(4..=10_000).contains(&config.output_controls)
            || config.anchor_indices.first() != Some(&0)
            || config.anchor_indices.last() != Some(&n)
            || config.anchor_indices.windows(2).any(|w| w[1] <= w[0])
        {
            return Err("periodic source, 0<=blend<1, 4..10000 controls and increasing endpoint-inclusive anchors required".into());
        }
        if let Some(intervals) = &config.coordinate_intervals {
            if intervals.len() != self.dimension
                || intervals.iter().any(|ranges| {
                    ranges.iter().any(|[a, b]| {
                        a >= b
                            || !config.anchor_indices.contains(a)
                            || !config.anchor_indices.contains(b)
                    }) || ranges.windows(2).any(|w| w[0][1] > w[1][0])
                })
            {
                return Err("per-coordinate ordered nonoverlapping intervals must use existing anchor indices".into());
            }
        }
        let bounds = self.rate_traversal_bounds(budgets, config.subdivisions_per_segment)?;
        let mut timed_cells = Vec::with_capacity(bounds.cells.len());
        for anchors in config.anchor_indices.windows(2) {
            let cells = &bounds.cells[anchors[0] * config.subdivisions_per_segment
                ..anchors[1] * config.subdivisions_per_segment];
            let total = cells.iter().map(|c| c.duration_upper_s).sum::<f64>();
            let start = ks[anchors[0]].time_s;
            let end = ks[anchors[1]].time_s;
            let mut time = start;
            for (i, cell) in cells.iter().enumerate() {
                let old_width = cell.reference_end_s - cell.reference_start_s;
                let redistributed = if total > 0. {
                    (end - start) * cell.duration_upper_s / total
                } else {
                    old_width
                };
                let width = (1. - config.blend) * old_width + config.blend * redistributed;
                let next = if i + 1 == cells.len() {
                    end
                } else {
                    time + width
                };
                if !next.is_finite() || next <= time {
                    return Err("redistributed phase duration lost precision".into());
                }
                timed_cells.push((time, next, cell.reference_start_s, cell.reference_end_s));
                time = next;
            }
        }
        let period = ks[n].time_s;
        let mut result = RateRedistribution {
            trajectory: TrajectoryConfig {
                interpolation: Interpolation::PeriodicCubicBSpline,
                keyframes: Vec::new(),
            },
            source_phases_s: Vec::new(),
        };
        for i in 0..config.output_controls {
            let time = i as f64 * period / config.output_controls as f64;
            let index = timed_cells
                .partition_point(|c| c.1 <= time)
                .min(timed_cells.len() - 1);
            let (a, b, old_a, old_b) = timed_cells[index];
            let phase = old_a + (old_b - old_a) * ((time - a) / (b - a)).clamp(0., 1.);
            result.source_phases_s.push(phase);
            let mut values = self.sample(phase)?.values;
            if let Some(intervals) = &config.coordinate_intervals {
                let unchanged = self.sample(time)?.values;
                for (j, ranges) in intervals.iter().enumerate() {
                    if !ranges
                        .iter()
                        .any(|[a, b]| time > ks[*a].time_s && time < ks[*b].time_s)
                    {
                        values[j] = unchanged[j];
                    }
                }
            }
            result.trajectory.keyframes.push(Keyframe {
                time_s: time,
                values,
            });
        }
        result.source_phases_s.push(period);
        result.trajectory.keyframes.push(Keyframe {
            time_s: period,
            values: result.trajectory.keyframes[0].values.clone(),
        });
        Trajectory::new(result.trajectory.clone())?;
        Ok(result)
    }
    /// Before/after the time domain, hold the endpoint with zero derivatives.
    /// Linear-knot derivatives are right-sided; the final knot is held.
    /// Periodic B-splines instead wrap time and preserve cycle derivatives.
    pub fn sample(&self, time_s: f64) -> Result<TrajectorySample, String> {
        if !time_s.is_finite() || time_s < 0.0 {
            return Err("invalid trajectory sample time".into());
        }
        let ks = &self.config.keyframes;
        if matches!(
            self.config.interpolation,
            Interpolation::PeriodicCubicBSpline
        ) {
            let (h, u, indices) = self.periodic_cell(time_s)?;
            let controls = indices.map(|i| &ks[i]);
            let mut result = TrajectorySample {
                values: Vec::new(),
                rates: Vec::new(),
                accelerations: Vec::new(),
            };
            for j in 0..self.dimension {
                // Evaluate around q1 using adjacent differences to avoid large
                // cancelling absolute control values in the derivatives.
                let q1 = controls[1].values[j];
                let d0 = controls[1].values[j] - controls[0].values[j];
                let d1 = controls[2].values[j] - controls[1].values[j];
                let d2 = controls[3].values[j] - controls[2].values[j];
                let a = (-d0 + d1) / 6.;
                let b = (d0 + d1) / 2.;
                let c = (d1 - d0) / 2.;
                let d = (d0 - 2. * d1 + d2) / 6.;
                let value = q1 + a + u * (b + u * (c + u * d));
                let rate = (b + u * (2. * c + u * 3. * d)) / h;
                let acceleration = (2. * c + 6. * u * d) / h / h;
                if ![value, rate, acceleration].iter().all(|v| v.is_finite()) {
                    return Err("nonfinite periodic cubic B-spline sample".into());
                }
                result.values.push(value);
                result.rates.push(rate);
                result.accelerations.push(acceleration);
            }
            return Ok(result);
        }
        let held = if time_s < ks[0].time_s {
            Some(&ks[0])
        } else if time_s >= ks.last().unwrap().time_s {
            ks.last()
        } else {
            None
        };
        if let Some(k) = held {
            return Ok(TrajectorySample {
                values: k.values.clone(),
                rates: vec![0.0; self.dimension],
                accelerations: vec![0.0; self.dimension],
            });
        }
        let end = ks.partition_point(|k| k.time_s <= time_s);
        let (a, b) = (&ks[end - 1], &ks[end]);
        let h = b.time_s - a.time_s;
        let s = (time_s - a.time_s) / h;
        let (position, rate, acceleration) = match self.config.interpolation {
            Interpolation::Linear => (s, 1.0, 0.0),
            Interpolation::QuinticRestToRest => (
                s * s * s * (10.0 + s * (-15.0 + 6.0 * s)),
                30.0 * s * s * (1.0 - s) * (1.0 - s),
                60.0 * s * (1.0 - s) * (1.0 - 2.0 * s),
            ),
            Interpolation::PeriodicCubicBSpline => unreachable!("periodic sampling handled above"),
        };
        Ok(TrajectorySample {
            values: a
                .values
                .iter()
                .zip(&b.values)
                .map(|(a, b)| a + (b - a) * position)
                .collect(),
            rates: a
                .values
                .iter()
                .zip(&b.values)
                .map(|(a, b)| (b - a) / h * rate)
                .collect(),
            accelerations: a
                .values
                .iter()
                .zip(&b.values)
                .map(|(a, b)| (b - a) / h / h * acceleration)
                .collect(),
        })
    }

    /// The four controls that can affect a periodic cubic sample's value,
    /// rate or acceleration. Conservative at exact knots. Valid while time,
    /// knot times and interpolation stay fixed; not a global sparsity claim
    /// when timing is itself optimized. Repeated endpoint aliases control zero.
    pub fn periodic_support_controls(&self, time_s: f64) -> Result<[usize; 4], String> {
        Ok(self.periodic_cell(time_s)?.2)
    }

    fn periodic_cell(&self, time_s: f64) -> Result<(f64, f64, [usize; 4]), String> {
        if !matches!(
            self.config.interpolation,
            Interpolation::PeriodicCubicBSpline
        ) || !time_s.is_finite()
            || time_s < 0.
        {
            return Err("periodic cubic trajectory and finite nonnegative time required".into());
        }
        let ks = &self.config.keyframes;
        let n = ks.len() - 1;
        let h = ks[n].time_s / n as f64;
        let cell = (time_s % ks[n].time_s) / h;
        let i = (cell.floor() as usize).min(n - 1);
        let u = (cell - i as f64).clamp(0., 1.);
        Ok((h, u, [(i + n - 1) % n, i, (i + 1) % n, (i + 2) % n]))
    }
}

#[cfg(test)]
mod support_tests {
    use super::*;
    #[test]
    fn periodic_support_covers_value_and_two_derivatives_across_wraps() {
        for n in [8, 16] {
            let mut config = TrajectoryConfig {
                interpolation: Interpolation::PeriodicCubicBSpline,
                keyframes: (0..=n)
                    .map(|i| Keyframe {
                        time_s: i as f64 / n as f64,
                        values: vec![(i % n) as f64],
                    })
                    .collect(),
            };
            let base = Trajectory::new(config.clone()).unwrap();
            assert_eq!(
                base.periodic_support_controls(0.).unwrap(),
                [n - 1, 0, 1, 2]
            );
            for control in 0..n {
                config.keyframes[control].values[0] += 0.125;
                config.keyframes[n].values = config.keyframes[0].values.clone();
                let changed = Trajectory::new(config.clone()).unwrap();
                for i in 0..=512 {
                    let t = i as f64 / 256.;
                    if !base
                        .periodic_support_controls(t)
                        .unwrap()
                        .contains(&control)
                    {
                        let a = base.sample(t).unwrap();
                        let b = changed.sample(t).unwrap();
                        assert_eq!(a.values, b.values);
                        assert_eq!(a.rates, b.rates);
                        assert_eq!(a.accelerations, b.accelerations);
                    }
                }
                config.keyframes[control].values[0] -= 0.125;
                config.keyframes[n].values = config.keyframes[0].values.clone();
            }
            assert!(base.periodic_support_controls(-1.).is_err());
            assert!(base.periodic_support_controls(f64::NAN).is_err());
        }
    }
}
