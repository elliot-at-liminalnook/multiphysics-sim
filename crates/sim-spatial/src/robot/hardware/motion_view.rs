//! "Command vs real motion" (web/viewer/actuator-motion-view.mjs): the
//! read-only comparison of the requested and measured encoder motion, as
//! pure functions ([`comparison`] is `motionComparison`, [`update`] is
//! `ActuatorMotionView.update`). The panel draws [`MotionView::chart`] with
//! `chart::rasterize_fixed` over [`MotionChart::x_range`] and
//! [`MotionChart::y_range`] (an x–y plot over elapsed ms).
//!
//! The page calls `update({id, axis, telemetry, sweep})` (:120) with no
//! `jog` or `preview`, so its per-motor receipts are never set: a record
//! exists only for a continuous session of the selected motor. That is kept.
//!
//! The axes are the page's (:29-31): x over `begin..end` (at least 1 ms,
//! so one sample or two at the same time stamp still have a width), y over
//! `lo − margin .. lo + span + margin` where `lo`/`hi` span 0, the last
//! request and every measured and requested value, `span = max(1, hi − lo)`
//! and `margin = 0.15·span`; the labels are `lo`/`hi` to one decimal. A
//! single sample therefore draws and labels as the page does.
//!
//! Differences in drawing only: samples without a requested target are
//! left out of the requested trace and out of the y range, deliberately:
//! the page maps every sample (`y(p.target_raw-origin)`), so a `null`
//! target (`null-origin` = −origin) draws a false point at minus the start
//! position, and a missing key (`undefined-origin` = NaN) writes NaN
//! coordinates; a `null` last request makes the page's `Math.min` NaN.
//! The measured points are marked only while there are few of them (the
//! shared raster's rule; the page marks each one).
use super::view::{fixed, js_num};
use serde::Serialize;
use sim_runtime::hardware::protocol::calibration::{Axis, Sweep, SweepSample, Telemetry};

/// The heading and the note under the chart (:12).
pub const HEADING: &str = "Command vs real motion";
pub const NOTE: &str = "Blue: requested · green: encoder readback. Updates during motion and holding; gaps between samples are not measured. Part direction uses your mounting setting. The 3D robot is not yet bound to these encoder readings.";
/// The chart's title label (:36).
pub const CHART_TITLE: &str = "Motor encoder change (counts)";

/// One step's comparison (`motionComparison`).
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    /// +1: toward upper is encoder +.
    pub sign: f64,
    pub requested: Option<f64>,
    pub actual: Option<f64>,
    pub agreement: String,
}

/// A step or a session to compare (`record`).
#[derive(Clone, Debug, PartialEq)]
pub struct Record<'a> {
    pub samples: &'a [SweepSample],
    pub start: f64,
    pub requested_counts: f64,
    pub actual_counts: Option<f64>,
}

/// `Math.sign`: 0 for 0, NaN for NaN.
fn js_sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

/// `motionComparison(axis, jog)`.
pub fn comparison(axis: &Axis, record: Option<&Record>) -> Comparison {
    let sign = match (axis.lower, axis.upper) {
        (Some(lower), Some(upper)) => {
            if upper < lower {
                -1.0
            } else {
                1.0
            }
        }
        _ => {
            if axis.reverse {
                -1.0
            } else {
                1.0
            }
        }
    };
    let Some(r) = record else {
        return Comparison { sign, requested: None, actual: None, agreement: "No measured step yet".into() };
    };
    let agreement = match r.actual_counts {
        None => "Awaiting measured reply",
        Some(a) if a == 0.0 => "No measured movement",
        Some(a) if js_sign(a) == js_sign(r.requested_counts) => "Measured direction matches command",
        Some(_) => "DIRECTION MISMATCH — check mapping",
    };
    Comparison { sign, requested: Some(r.requested_counts * sign), actual: r.actual_counts.map(|a| a * sign), agreement: agreement.into() }
}

/// The traces to draw: [elapsed ms, counts from the start] points.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MotionChart {
    #[serde(skip)]
    pub requested: Vec<[f64; 2]>,
    #[serde(skip)]
    pub measured: Vec<[f64; 2]>,
    /// The first and last sample time (ms), as the page labels them;
    /// `end ≥ begin + 1`.
    pub begin: f64,
    pub end: f64,
    /// The smallest and largest value plotted, 0 included (the page's
    /// `lo`/`hi`, labelled to one decimal).
    pub lo: f64,
    pub hi: f64,
}

impl MotionChart {
    /// The x axis: `begin..end` (ms).
    pub fn x_range(&self) -> (f64, f64) {
        (self.begin, self.end)
    }

    /// The y axis as the page scales it (:29, :31): `span = max(1, hi − lo)`
    /// and a 15 % margin either side, so the bottom is `lo − margin` and the
    /// top `lo + span + margin` (always wider than zero).
    pub fn y_range(&self) -> (f64, f64) {
        let span = (self.hi - self.lo).max(1.0);
        let margin = span * 0.15;
        (self.lo - margin, self.lo + span + margin)
    }

    /// The chart's labels: `hi`, `lo` and the time span (:36).
    pub fn labels(&self) -> [String; 3] {
        [fixed(self.hi, 1), fixed(self.lo, 1), format!("{} ms – {} ms", js_num(self.begin), js_num(self.end))]
    }
}

/// What the view shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MotionView {
    /// "Toward upper ↑ = encoder + · toward lower ↓ = encoder −".
    pub direction: String,
    pub status: String,
    /// The status is a direction mismatch (drawn in the danger colour, else OK).
    pub mismatch: bool,
    pub chart: Option<MotionChart>,
    /// The two lines shown instead of a chart.
    pub placeholder: Option<[String; 2]>,
}

/// `update({id, axis, telemetry, sweep})`.
pub fn update(id: Option<u8>, axis: &Axis, telemetry: Option<&Telemetry>, sweep: Option<&Sweep>) -> MotionView {
    let continuous = sweep.filter(|sw| id.is_some() && sw.motor_id == id && !sw.samples.is_empty());
    let record = continuous.map(|sw| {
        let start = sw.samples[0].position();
        let live = sw.samples.last().expect("samples checked non-empty");
        Record { samples: &sw.samples, start, requested_counts: live.target_raw.map_or(f64::NAN, |t| t - start), actual_counts: Some(live.position() - start) }
    });
    let c = comparison(axis, record.as_ref());
    let direction = format!("Toward upper ↑ = encoder {} · toward lower ↓ = encoder {}", if c.sign > 0.0 { "+" } else { "−" }, if c.sign > 0.0 { "−" } else { "+" });
    let mut status = c.agreement.clone();
    if record.is_some() {
        let requested = c.requested.unwrap_or(f64::NAN);
        status += &format!(" · requested {} {} counts", if requested > 0.0 { "↑" } else { "↓" }, js_num(requested.abs()));
        if let Some(actual) = c.actual {
            status += &format!(" · measured {} {} counts", if actual > 0.0 { "↑" } else if actual < 0.0 { "↓" } else { "—" }, js_num(actual.abs()));
        }
    }
    let live = continuous.and_then(|sw| sw.samples.last());
    if let (Some(sw), Some(live)) = (continuous, live) {
        status = format!(
            "{} · toward {} · measured {} part counts/s · tracking error {} counts",
            if sw.running {
                if live.holding { "Holding" } else { "Moving" }
            } else {
                "Stopped"
            },
            if live.toward_upper { "upper ↑" } else { "lower ↓" },
            fixed(live.velocity_counts_s * c.sign, 1),
            fixed(live.target_raw.map_or(f64::NAN, |t| t - live.position()), 1)
        );
    }
    let mismatch = continuous.is_none() && c.agreement.starts_with("DIRECTION");
    let Some(r) = record else {
        let first = telemetry.map(|t| format!("Encoder: {} counts", t.position_raw)).unwrap_or_else(|| "Waiting for encoder readback".into());
        return MotionView { direction, status, mismatch, chart: None, placeholder: Some([first, "Hold Q/A to compare command and motion.".into()]) };
    };
    let origin = r.start;
    let begin = if continuous.is_some() { r.samples.iter().map(|p| p.elapsed_ms).fold(f64::INFINITY, f64::min) } else { 0.0 };
    let begin = if begin.is_finite() { begin } else { 0.0 };
    let end = r.samples.iter().map(|p| p.elapsed_ms).fold(begin + 1.0, f64::max);
    let measured: Vec<[f64; 2]> = r.samples.iter().map(|p| [p.elapsed_ms, p.position() - origin]).collect();
    let requested: Vec<[f64; 2]> = r.samples.iter().filter_map(|p| p.target_raw.map(|t| [p.elapsed_ms, t - origin])).collect();
    let values = [0.0, r.requested_counts].into_iter().chain(measured.iter().chain(&requested).map(|p| p[1])).filter(|v| v.is_finite());
    let (lo, hi) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
    MotionView { direction, status, mismatch, chart: Some(MotionChart { requested, measured, begin, end, lo, hi }), placeholder: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ms: f64, position: i64, target: Option<f64>) -> SweepSample {
        SweepSample { elapsed_ms: ms, position_raw: position, target_raw: target, velocity_counts_s: 12.34, toward_upper: true, ..Default::default() }
    }

    #[test]
    fn comparison_names_direction_agreement() {
        let a = Axis { lower: Some(3000), upper: Some(1000), ..Default::default() };
        assert_eq!(comparison(&a, None).sign, -1.0);
        let r = |req: f64, act: Option<f64>| Record { samples: &[], start: 0.0, requested_counts: req, actual_counts: act };
        let plain = Axis::default();
        assert_eq!(comparison(&plain, Some(&r(10.0, None))).agreement, "Awaiting measured reply");
        assert_eq!(comparison(&plain, Some(&r(10.0, Some(0.0)))).agreement, "No measured movement");
        assert_eq!(comparison(&plain, Some(&r(10.0, Some(4.0)))).agreement, "Measured direction matches command");
        assert_eq!(comparison(&plain, Some(&r(10.0, Some(-4.0)))).agreement, "DIRECTION MISMATCH — check mapping");
        let c = comparison(&a, Some(&r(10.0, Some(4.0))));
        assert_eq!((c.requested, c.actual), (Some(-10.0), Some(-4.0)));
    }

    #[test]
    fn without_a_session_the_view_waits_for_readback() {
        let v = update(Some(1), &Axis::default(), None, None);
        assert_eq!(v.direction, "Toward upper ↑ = encoder + · toward lower ↓ = encoder −");
        assert_eq!(v.status, "No measured step yet");
        assert_eq!(v.placeholder.as_ref().unwrap()[0], "Waiting for encoder readback");
        let t = Telemetry { position_raw: 1234, ..Default::default() };
        let v = update(Some(1), &Axis::default(), Some(&t), None);
        assert_eq!(v.placeholder.unwrap(), ["Encoder: 1234 counts".to_string(), "Hold Q/A to compare command and motion.".to_string()]);
    }

    #[test]
    fn a_session_draws_requested_and_measured_traces() {
        let sweep = Sweep { running: true, motor_id: Some(2), samples: vec![sample(100.0, 1000, Some(1000.0)), sample(200.0, 1010, None), sample(300.0, 1030, Some(1050.0))], ..Default::default() };
        let v = update(Some(2), &Axis::default(), None, Some(&sweep));
        assert_eq!(v.status, "Moving · toward upper ↑ · measured 12.3 part counts/s · tracking error 20.0 counts");
        assert!(!v.mismatch);
        let chart = v.chart.unwrap();
        assert_eq!((chart.begin, chart.end), (100.0, 300.0));
        assert_eq!(chart.measured, vec![[100.0, 0.0], [200.0, 10.0], [300.0, 30.0]]);
        assert_eq!(chart.requested, vec![[100.0, 0.0], [300.0, 50.0]]);
        // The page's y range: 0, the last request and every value; a 15 %
        // margin of max(1, hi − lo) either side.
        assert_eq!((chart.lo, chart.hi), (0.0, 50.0));
        assert_eq!(chart.y_range(), (-7.5, 57.5));
        assert_eq!(chart.labels(), ["50.0".to_string(), "0.0".to_string(), "100 ms – 300 ms".to_string()]);
        // Another motor's session is not this motor's record.
        assert!(update(Some(1), &Axis::default(), None, Some(&sweep)).chart.is_none());
    }

    #[test]
    fn one_sample_draws_and_labels_like_the_page() {
        // One sample, at rest: values are all 0, so the span is 1 and the
        // x axis is 1 ms wide.
        let mut at_rest = sample(40.0, 500, Some(500.0));
        at_rest.velocity_counts_s = 0.0;
        let sweep = Sweep { running: true, motor_id: Some(1), samples: vec![at_rest], ..Default::default() };
        // Taught in reverse: the velocity is multiplied by −1 (−0 at rest).
        let a = Axis { lower: Some(3000), upper: Some(1000), ..Default::default() };
        let v = update(Some(1), &a, None, Some(&sweep));
        assert_eq!(v.status, "Moving · toward upper ↑ · measured 0.0 part counts/s · tracking error 0.0 counts");
        let chart = v.chart.unwrap();
        assert_eq!(chart.x_range(), (40.0, 41.0));
        assert_eq!((chart.lo, chart.hi), (0.0, 0.0));
        assert_eq!(chart.y_range(), (-0.15, 1.15));
        assert_eq!(chart.labels(), ["0.0".to_string(), "0.0".to_string(), "40 ms – 41 ms".to_string()]);
        // A request without a target leaves the range to the values there are.
        let sweep = Sweep { running: true, motor_id: Some(1), samples: vec![sample(0.0, 500, None), sample(0.0, 490, None)], ..Default::default() };
        let chart = update(Some(1), &Axis::default(), None, Some(&sweep)).chart.unwrap();
        assert_eq!((chart.lo, chart.hi, chart.x_range()), (-10.0, 0.0, (0.0, 1.0)));
    }
}
