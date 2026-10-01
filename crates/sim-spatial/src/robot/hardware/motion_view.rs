//! "Command vs real motion" (web/viewer/actuator-motion-view.mjs): the
//! read-only comparison of the requested and measured encoder motion, as
//! pure functions ([`comparison`] is `motionComparison`, [`update`] is
//! `ActuatorMotionView.update`). The panel draws [`MotionView::chart`] with
//! `chart::rasterize_span` (an x–y plot over elapsed ms).
//!
//! The page calls `update({id, axis, telemetry, sweep})` (:120) with no
//! `jog` or `preview`, so its per-motor receipts are never set: a record
//! exists only for a continuous session of the selected motor. That is kept.
//!
//! Differences in drawing only: the plot's vertical range is the traces'
//! own (with the raster's margin), not widened to include 0 and the last
//! request. Samples without a requested target are left out of the
//! requested trace, deliberately: the page maps every sample
//! (`y(p.target_raw-origin)`), so a `null` target (`null-origin` = −origin)
//! draws a false point at minus the start position, and a missing key
//! (`undefined-origin` = NaN) writes NaN coordinates.
use super::view::{fixed, js_num};
use serde::Serialize;
use sim_runtime::hardware_client::calibration::{Axis, Sweep, SweepSample, Telemetry};

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
    /// The first and last sample time (ms), as the page labels them.
    pub begin: f64,
    pub end: f64,
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
    let measured = r.samples.iter().map(|p| [p.elapsed_ms, p.position() - origin]).collect();
    let requested = r.samples.iter().filter_map(|p| p.target_raw.map(|t| [p.elapsed_ms, t - origin])).collect();
    MotionView { direction, status, mismatch, chart: Some(MotionChart { requested, measured, begin, end }), placeholder: None }
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
        // Another motor's session is not this motor's record.
        assert!(update(Some(1), &Axis::default(), None, Some(&sweep)).chart.is_none());
    }
}
