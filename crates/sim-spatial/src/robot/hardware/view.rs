//! The page's `render()` and `renderGait()` (web/viewer/calibration-ui.mjs
//! :82-123, :214-240) as pure functions of the link's snapshot and the
//! panel's form: every text and every enabled flag, rule for rule, with the
//! page's strings. The panel ([`super::panel`]) only copies a [`PanelView`]
//! into widgets; `system_ui` ([`super::panel::controls`]) and REST
//! `hardware_status` ([`status_json`]) read the same view.
//!
//! **Deliberate addition: staleness.** The page never shows an old status as
//! live because its poll errors stop drive. The native link keeps the time
//! of its last read ([`LinkSnapshot::stale`]): when that is older than
//! `link::STALE_AFTER` the status line reads "Status stale — last read N s
//! ago" (before the first read: "Reading the calibration server's status…")
//! and every control that moves or energizes a motor is disabled
//! ([`PanelView::block`]); STOP stays enabled. The panel blocks the same
//! controls while no link exists.
//!
//! **Deliberate addition: link health** ([`LinkSnapshot::health`]). A
//! disconnected link (the server reported its bus lost after it was
//! connected, a pinned virtual bench gone, a request that found the binding
//! gone) prefixes the status line with "DISCONNECTED — why." without
//! blocking anything by itself (a physical bus lost on a STOP readback is
//! reconnected by selecting a motor); a leg gait's clock
//! ([`LinkSnapshot::leg_clock`]) is shown frozen with the reason whenever
//! the leg's data is not live.
//!
//! **Simulated results are labelled.** A leg gait on a virtual bench (the
//! server's gait state says `simulated`, or the link is pinned to a virtual
//! execution) reads "VIRTUAL (simulated) · Leg: …", and a recent leg run the
//! server recorded as simulated is headed "VIRTUAL (simulated) · …"; a run
//! row without the field (an older server) is physical or unknown.
//!
//! Side effects the page's `render()` makes (ending a finished learn, ending
//! a finished leg gait) belong to the link thread, which has the newest state.
mod status;

pub use status::status_json;

use super::Section;
use super::actions::{DriveMode, GaitMode};
use super::link::{Inputs, Intent, LinkHealth, LinkSnapshot};
use serde::Serialize;
use serde_json::Value;
use sim_runtime::hardware::protocol::calibration::{Axis, GaitEntry, GaitStatistics, Status};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

/// The page's form controls (sliders, checkboxes, selects, open sections)
/// and the counters the panel follows. Only the UI writes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    /// Speed, PWM ceiling, hold-others, drive mode, gait speed and effort (sent to the link).
    pub inputs: Inputs,
    /// "Move to a taught pose" (0–100, step 0.1; the page's initial 50).
    pub target_percent: f64,
    /// "The motor is mid-travel with room to move both ways".
    pub tune_ok: bool,
    /// "The leg is suspended with clear space around every joint" (campaign).
    pub campaign_ok: bool,
    /// The gait confirmation ("needed for Leg and Both").
    pub gait_ok: bool,
    /// The gait select's value (index into the snapshot's gait list).
    pub gait_index: usize,
    pub gait_mode: GaitMode,
    /// "Single raw step" (−4095..=4095; the page's initial 1).
    pub step: i32,
    /// Open collapsible sections.
    pub open: BTreeSet<Section>,
    /// Hold-to-move presses accepted and not yet released (keys and the jog
    /// buttons; the page's `keys` set and `pointer`).
    pub held_upper: bool,
    pub held_lower: bool,
    /// The snapshot counters last seen (reset on each new link).
    pub seen_tune_done: u64,
    pub seen_campaign_done: u64,
    pub seen_speed_reset: u64,
}
impl Default for Form {
    fn default() -> Self {
        Self::new(None, None)
    }
}
impl Form {
    /// The page's initial form, with the operator's remembered drive mode and
    /// hold-others choice (the page's `localStorage`, :199-202).
    pub fn new(drive_mode: Option<DriveMode>, hold_others: Option<bool>) -> Self {
        let mut inputs = Inputs::default();
        if let Some(mode) = drive_mode {
            inputs.drive_mode = mode;
        }
        if let Some(on) = hold_others {
            inputs.hold_others = on;
        }
        Form {
            inputs,
            target_percent: 50.0,
            tune_ok: false,
            campaign_ok: false,
            gait_ok: false,
            gait_index: 0,
            gait_mode: GaitMode::Sim,
            step: 1,
            open: Section::ALL.into_iter().filter(|s| s.open_initially()).collect(),
            held_upper: false,
            held_lower: false,
            seen_tune_done: 0,
            seen_campaign_done: 0,
            seen_speed_reset: 0,
        }
    }
}

/// A button's text and whether it is enabled.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Gate {
    pub text: String,
    pub enabled: bool,
}
fn gate(text: impl Into<String>, enabled: bool) -> Gate {
    Gate { text: text.into(), enabled }
}

/// A motor chip (the page's `.motor` buttons).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Chip {
    pub id: u8,
    /// "Knee 1"; a disabled motor is prefixed "⊘ " (the page strikes it through).
    pub label: String,
    pub pressed: bool,
    pub enabled: bool,
    /// The motor is disabled (`.off`).
    pub off: bool,
}

/// Everything the page's `render()` shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PanelView {
    pub chips: Vec<Chip>,
    pub disable: Gate,
    pub sweep_all: Gate,
    pub sequence: String,
    /// `{at} · {text}` lines, newest first.
    pub warnings: String,
    pub status: String,
    /// Q/A and the jog buttons.
    pub jog_enabled: bool,
    pub held_upper: bool,
    pub held_lower: bool,
    pub speed_text: String,
    pub position: String,
    /// (measured, requested) needle fractions; None keeps the last needles (no readback).
    pub needles: Option<(f64, f64)>,
    pub target_enabled: bool,
    /// Where the target slider is put while it is not being dragged (percent).
    pub target_value: Option<f64>,
    pub target_label: String,
    /// Lower, upper, reference: (pose text, button enabled).
    pub poses: [(String, bool); 3],
    pub sweep: Gate,
    pub reset: Gate,
    pub learn: Gate,
    pub tune: Gate,
    pub tune_status: String,
    /// "Run campaign" (its text) and "Resume" share the enabled rule.
    pub campaign: Gate,
    pub campaign_status: String,
    pub learning: String,
    pub capture: String,
    pub telemetry: String,
    /// Advanced: "Swap upper / lower direction", the two resets and "Send raw step".
    pub flip_enabled: bool,
    pub clear_enabled: bool,
    pub raw_step_enabled: bool,
    pub gait: GaitView,
    pub motion: super::motion_view::MotionView,
    /// Why motion controls are disabled regardless of the page's rules (stale, offline).
    pub blocked: Option<String>,
}

/// Everything the page's `renderGait()` shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct GaitView {
    pub speed_text: String,
    pub effort_text: String,
    pub effort_enabled: bool,
    pub play: Gate,
    pub stop_enabled: bool,
    pub modes_enabled: bool,
    pub status: String,
    /// The statistics table's rows (empty: no table).
    pub stats: Vec<[String; 11]>,
    /// Recent leg runs: (heading line, its statistics rows).
    pub runs: Vec<(String, Vec<[String; 11]>)>,
    /// The gait select's option labels.
    pub options: Vec<String>,
}

/// The statistics table's header (`statsTable`).
pub const STATS_HEADER: [&str; 11] = ["Motor", "RMS error", "Peak", "Sim RMS", "Lag", "Effort", "At ceiling", "Governed", "Peak accel", "Min V", "Max °C"];
/// "Recent leg runs" when there are none.
pub const NO_RUNS: &str = "No leg runs yet.";
/// The label of a simulated (virtual bench) leg gait and leg run.
pub const VIRTUAL_SIMULATED: &str = "VIRTUAL (simulated)";
/// The status line's prefix while the link is disconnected ([`mark_disconnected`]).
pub const DISCONNECTED: &str = "DISCONNECTED";

/// The page's motor chips (:13), always these three.
const DEFAULT_MOTORS: [(u8, &str); 3] = [(1, "Knee"), (2, "Worm"), (3, "Belt")];

// ---- the page's helpers ----

/// `Number(x).toFixed(d)`. Both JavaScript and Rust's `format!("{:.d$}")`
/// round the exact binary value of `x`; they differ only on an exact tie
/// (the value lies exactly halfway between two `d`-digit decimals), which
/// JavaScript rounds away from zero and Rust to even. So a tie is handled
/// here only when it is exact: `|x|·10^d` is a half-integer *and* that
/// product was computed without rounding (the fused `mul_add` residual is
/// 0). A product that merely rounds to .5 (1.45 is 1.4499999999999999556
/// in binary, ×10 rounds to 14.5) is not a tie and goes to `format!`,
/// which gives "1.4" as the page does. Non-finite values and negative
/// zero print as JS does.
pub fn fixed(x: f64, d: usize) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    // JavaScript prints negative zero without its sign ((-0).toFixed(1) is
    // "0.0") but keeps the sign of a negative value that rounds to zero
    // ((-0.04).toFixed(1) is "-0.0"), as Rust does.
    let x = if x == 0.0 { 0.0 } else { x };
    let scale = 10f64.powi(d as i32);
    let scaled = x.abs() * scale;
    let exact = x.abs().mul_add(scale, -scaled) == 0.0;
    if scaled < 1e15 && scaled.fract() == 0.5 && exact {
        let rounded = (scaled.floor() + 1.0) / scale;
        let sign = if x < 0.0 { "-" } else { "" };
        return format!("{sign}{rounded:.d$}");
    }
    format!("{x:.d$}")
}

/// A number as JavaScript prints it in a template string: integral values
/// without a fraction, others in their shortest form.
pub fn js_num(x: f64) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x.is_infinite() {
        if x > 0.0 { "Infinity".into() } else { "-Infinity".into() }
    } else if x.fract() == 0.0 && x.abs() < 1e21 {
        format!("{}", x as i128)
    } else {
        format!("{x}")
    }
}

/// `Math.round`: halves round toward +∞.
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// `angle(n)`: encoder counts as motor degrees, one decimal.
pub fn angle(n: f64) -> String {
    format!("{}°", fixed(n * 360.0 / 4096.0, 1))
}

/// `fraction(raw, a)`: between the taught poses (lower 0, upper 1), else the
/// position within one turn (reversed for a reversed axis).
pub fn fraction(raw: f64, a: &Axis) -> f64 {
    match (a.lower, a.upper) {
        (Some(lower), Some(upper)) => (raw - lower as f64) / (upper as f64 - lower as f64),
        _ => {
            let turn = raw.rem_euclid(4096.0) / 4095.0;
            if a.reverse { 1.0 - turn } else { turn }
        }
    }
}

/// `speed()`: the movement speed slider (0–100) as counts/s, 5 to the server's maximum (500 by default).
pub fn speed(maximum_speed_counts_s: Option<f64>, percent: f64) -> f64 {
    5.0 * (maximum_speed_counts_s.unwrap_or(500.0) / 5.0).powf(percent / 100.0)
}

/// `axis()`: the selected motor's calibration (empty when unknown).
pub fn axis(state: &Status, id: Option<u8>) -> Axis {
    id.and_then(|id| state.calibration.as_ref()?.axes.get(&id).cloned()).unwrap_or_default()
}

/// `position(state.samples[id])`.
pub fn position(state: &Status, id: Option<u8>) -> Option<f64> {
    id.and_then(|id| state.samples.get(&id)).map(|t| t.position())
}

/// `outsidePose()`: "reference" when the poses were taught in another
/// encoder session, "upper"/"lower" when the motor is beyond that pose.
pub fn outside_pose(state: &Status, id: Option<u8>) -> Option<&'static str> {
    let a = axis(state, id);
    let p = position(state, id)?;
    if let Some(session) = a.coordinate_session.as_deref().filter(|s| !s.is_empty())
        && Some(session) != state.coordinate_session.as_deref()
    {
        return Some("reference");
    }
    let reverse = match (a.lower, a.upper) {
        (Some(lower), Some(upper)) => upper < lower,
        _ => a.reverse,
    };
    if let Some(upper) = a.upper.map(|u| u as f64)
        && (if reverse { p < upper } else { p > upper })
    {
        return Some("upper");
    }
    if let Some(lower) = a.lower.map(|l| l as f64)
        && (if reverse { p > lower } else { p < lower })
    {
        return Some("lower");
    }
    None
}

/// The status line's precedence (:87-90).
pub fn status_line(s: &LinkSnapshot) -> String {
    let a = axis(&s.state, s.id);
    let mut status = if s.busy {
        "Connecting and checking this motor at zero drive…".to_string()
    } else if s.id.is_none() {
        "Choose the motor you want to calibrate.".to_string()
    } else {
        s.state.message.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| "Ready".into())
    };
    if a.disabled && !s.busy {
        let role = if a.role.is_empty() { "This motor" } else { a.role.as_str() };
        status = format!("{role} is disabled. Enable it to move it.");
    }
    if let Some(outside) = outside_pose(&s.state, s.id)
        && s.ready
        && !s.busy
    {
        status = if outside == "reference" {
            "Saved poses are from an earlier session and are ignored until re-taught; manual moves still work.".into()
        } else {
            format!("Beyond the saved {outside} pose. Move back inward freely; driving further out is blocked.")
        };
    }
    status
}

/// `statsTable`'s rows (`f()`: '—' for a missing or non-finite value).
///
/// **Deliberate difference (honest labels, AGENTS.md).** The page scales
/// before it checks: `f(r.lag_s*1000,0)`, `f(r.mean_effort*100,0)`,
/// `f(r.saturated_fraction*100,0)` and `f(r.governor_limited_fraction*100,0)`
/// receive `null*1000` = 0 for a `null` value (the server writes a
/// non-finite statistic as `null`) and print "0 ms" / "0%", reporting a
/// missing measurement as a measured zero (a missing key gives
/// `undefined*1000` = NaN and prints "— ms"). Here every missing value
/// prints "—" before its unit ("— ms", "—%").
pub fn stats_rows(stats: &BTreeMap<String, GaitStatistics>) -> Vec<[String; 11]> {
    let f = |v: Option<f64>, scale: f64, d: usize| match v.map(|v| v * scale) {
        Some(v) if v.is_finite() => fixed(v, d),
        _ => "—".to_string(),
    };
    stats
        .values()
        .map(|r| {
            [
                r.role.clone(),
                format!("{}°", f(r.tracking_rms_deg, 1.0, 2)),
                format!("{}°", f(r.tracking_peak_deg, 1.0, 1)),
                match r.simulated_tracking_rms_counts {
                    None => "—".to_string(),
                    Some(c) => format!("{}°", f(Some(c), 360.0 / 4096.0, 2)),
                },
                format!("{} ms", f(r.lag_s, 1000.0, 0)),
                format!("{}%", f(r.mean_effort, 100.0, 0)),
                format!("{}%", f(r.saturated_fraction, 100.0, 0)),
                format!("{}%", f(r.governor_limited_fraction, 100.0, 0)),
                f(r.peak_measured_acceleration_counts_s2, 1.0, 0),
                f(r.minimum_voltage_v, 1.0, 1),
                f(r.maximum_temperature_c, 1.0, 0),
            ]
        })
        .collect()
}

/// A gait select option's label (`loadGaits`, :240).
pub fn gait_option_label(g: &GaitEntry) -> String {
    if g.kind.as_deref() == Some("pose_sequence") {
        format!("Poses · {} · {}", g.study, g.trial)
    } else {
        let study = g.study.strip_prefix("gait-search-").unwrap_or(&g.study);
        format!("{}{} m/s · {} · {}", if g.measured_actuators { "★ " } else { "" }, fixed(g.speed_m_s.unwrap_or(0.0), 3), study, g.trial)
    }
}

/// The page's three fixed motor chips (Knee 1, Worm 2, Belt 3, :13): the
/// labels never follow the calibration's roles or axis ids; only the
/// `.off` state is looked up from `state.calibration.axes[id].disabled`
/// (:84).
fn chips(s: &LinkSnapshot) -> Vec<Chip> {
    let axes = s.state.calibration.as_ref().map(|c| &c.axes);
    DEFAULT_MOTORS
        .iter()
        .map(|&(id, role)| {
            let off = axes.and_then(|axes| axes.get(&id)).is_some_and(|a| a.disabled);
            Chip { id, label: format!("{}{role} {id}", if off { "⊘ " } else { "" }), pressed: s.id == Some(id), enabled: !s.busy, off }
        })
        .collect()
}

/// The page's `render()` and `renderGait()` for this snapshot and form at `now`.
pub fn render(s: &LinkSnapshot, form: &Form, now: Instant) -> PanelView {
    let st = &s.state;
    let a = axis(st, s.id);
    let t = s.id.and_then(|id| st.samples.get(&id));
    let latest = st.sweep.as_ref().filter(|sw| s.id.is_some() && sw.motor_id == s.id).and_then(|sw| sw.latest.as_ref());
    let active = s.ready && !s.busy;
    let outside = outside_pose(st, s.id);
    let taught = a.lower.is_some() && a.upper.is_some();
    let session = s.run.is_some();
    let speed_now = speed(st.maximum_speed_counts_s, form.inputs.speed_percent);
    let allowed = latest.and_then(|l| l.adaptation.as_ref()).map(|a| a.permitted_speed_counts_s);
    let mut speed_text = format!("{}°/s motor", fixed(speed_now * 360.0 / 4096.0, 2));
    if let Some(allowed) = allowed
        && session
        && s.intent != Intent::Hold
        && allowed < speed_now * 0.95
    {
        speed_text += &format!(" · limited to {}°/s here", fixed(allowed * 360.0 / 4096.0, 2));
    }
    let pose_enabled = active && !s.sweeping && s.intent == Intent::Hold && !(session && !latest.is_some_and(|l| l.holding));
    let pose = |v: Option<i64>, missing: &str| v.map(|v| format!("{} motor", angle(v as f64))).unwrap_or_else(|| missing.to_string());
    let requested = |t: f64| latest.and_then(|l| l.target_raw).unwrap_or(t);
    let range_enabled = active && taught && outside != Some("reference");
    let tn = st.tuning.as_ref().filter(|tn| s.id.is_some() && tn.motor_id == s.id);
    let tune_status = match (tn, &a.tuning) {
        (Some(tn), _) if tn.running => format!("Tuning: {}", tn.stage),
        (Some(tn), _) if tn.error.as_deref().is_some_and(|e| !e.is_empty()) => format!("Last tuning stopped: {}", tn.error.as_deref().unwrap_or_default()),
        (_, Some(tuned)) => format!(
            "Tuned gains in use: kp {}, ki {}, kd {}, friction {}% · {}",
            fixed(tuned.pid.kp, 2),
            fixed(tuned.pid.ki, 2),
            fixed(tuned.pid.kd, 3),
            fixed(tuned.friction_duty * 100.0, 0),
            tuned.record
        ),
        _ => "Using the shared provisional gains.".into(),
    };
    let campaign_status = match &st.campaign {
        Some(cp) if cp.running => {
            let abort = cp.last.as_ref().and_then(|l| l.abort.as_ref()).filter(|v| truthy(v)).map(|v| v.as_object().and_then(|o| o.keys().next().cloned()).unwrap_or_else(|| "undefined".into()));
            format!("{} · {} stage results saved{}", cp.stage, cp.completed, abort.map(|g| format!(" · last stopped by {g}")).unwrap_or_default())
        }
        Some(cp) if cp.error.as_deref().is_some_and(|e| !e.is_empty()) => format!("Last campaign stopped: {}", cp.error.as_deref().unwrap_or_default()),
        Some(cp) if cp.result.is_some() => {
            let r = cp.result.as_ref().unwrap();
            format!("Finished: {}. {}", r.headline, r.directory)
        }
        _ => "Tune each motor and teach both poses first.".into(),
    };
    let learning = match latest.and_then(|l| l.adaptation.as_ref()) {
        Some(ad) => format!("{}. Stops learned: {} / {}. Allowed now: {}°/s motor.", ad.status, ad.decreasing_stops, ad.increasing_stops, fixed(ad.permitted_speed_counts_s * 360.0 / 4096.0, 2)),
        None => "Teach both poses, set your desired speed, then learn in the middle. Starts at crawl; effort stays within your PWM ceiling.".into(),
    };
    let telemetry = t
        .map(|t| format!("{} V · {} °C · encoder {} · effort {}%", fixed(t.voltage_v, 1), js_num(t.temperature_c), t.position_raw, fixed(latest.map_or(0.0, |l| l.pwm) / 10.0, 1)))
        .unwrap_or_default();
    let mut view = PanelView {
        chips: chips(s),
        disable: gate(if a.disabled { "Enable this motor" } else { "Disable this motor" }, !(s.id.is_none() || s.busy || s.sweep_all)),
        sweep_all: gate(if s.sweep_all { "Stop sweeping all" } else { "Sweep all enabled motors" }, !(s.busy && !s.sweep_all)),
        sequence: s.sequence_text.clone(),
        warnings: s.warnings.iter().map(|(at, text)| format!("{at} · {text}")).collect::<Vec<_>>().join("\n"),
        status: status_line(s),
        jog_enabled: active,
        held_upper: s.intent == Intent::Upper && session,
        held_lower: s.intent == Intent::Lower && session,
        speed_text,
        position: t.map(|t| format!("{} motor", angle(t.position()))).unwrap_or_else(|| "—".into()),
        needles: t.map(|t| (fraction(t.position(), &a), fraction(requested(t.position()), &a))),
        target_enabled: range_enabled,
        target_value: t.filter(|_| taught).map(|t| 100.0 * fraction(requested(t.position()), &a)),
        target_label: (if taught { "lower → upper" } else { "teach both poses first" }).to_string(),
        poses: [(pose(a.lower, "Not taught"), pose_enabled), (pose(a.upper, "Not taught"), pose_enabled), (pose(a.reference, "Not aligned"), pose_enabled)],
        sweep: gate(if s.sweeping || s.intent == Intent::Target { "Pause & hold" } else { "Try saved range" }, range_enabled),
        reset: gate(
            match outside {
                Some("reference") => "Re-teach both poses".to_string(),
                Some(o) => format!("Reset {o} pose"),
                None => "Reset poses".into(),
            },
            active,
        ),
        learn: gate(if s.learning { "Pause learning & hold" } else { "Learn motion in the middle" }, range_enabled),
        tune: gate(if s.tuning { "Tuning…" } else { "Tune this motor" }, s.ready && !s.busy && !s.tuning && !s.sweep_all && form.tune_ok),
        tune_status,
        campaign: gate(if s.campaigning { "Campaign running…" } else { "Run campaign" }, !(s.id.is_none() || s.busy || s.tuning || s.campaigning || s.sweep_all || !form.campaign_ok)),
        campaign_status,
        learning,
        capture: st.capture_message.clone().unwrap_or_default(),
        telemetry,
        // The page leaves these enabled and its handlers return without a motor
        // (or while one is connecting, or for a zero step): shown disabled here.
        flip_enabled: s.id.is_some() && !s.busy,
        clear_enabled: s.id.is_some() && !s.busy,
        raw_step_enabled: s.id.is_some() && form.step != 0 && (-4095..=4095).contains(&form.step),
        gait: render_gait(s, form, now),
        motion: super::motion_view::update(s.id, &a, t, st.sweep.as_ref()),
        blocked: None,
    };
    if s.stale(now) {
        let reason = match s.read_at {
            None => "Reading the calibration server's status…".to_string(),
            Some(at) => format!("Status stale — last read {} s ago", now.duration_since(at).as_secs()),
        };
        view.block(reason);
    }
    mark_disconnected(&mut view, s, now);
    view
}

/// While the link is disconnected ([`LinkHealth::Disconnected`]), the status
/// line starts "DISCONNECTED — {why}." (the line after it unless the reason
/// already says it). Idempotent: the panel calls it again after a block
/// replaced the line. Blocks nothing by itself.
pub fn mark_disconnected(view: &mut PanelView, s: &LinkSnapshot, now: Instant) {
    let LinkHealth::Disconnected { why } = s.health(now) else { return };
    if view.status.starts_with(DISCONNECTED) {
        return;
    }
    let why = why.trim().trim_end_matches('.');
    let rest = view.status.trim().trim_end_matches('.');
    view.status = if rest.is_empty() || why.contains(rest) {
        format!("{DISCONNECTED} — {why}.")
    } else {
        format!("{DISCONNECTED} — {why}. {}", view.status)
    };
}

/// `Math.round(v)` as the page prints it, "—" for a missing (null) value.
fn rounded(v: Option<f64>) -> String {
    v.map_or_else(|| "—".to_string(), |v| js_num(js_round(v)))
}

/// JavaScript truthiness of a JSON value.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

impl PanelView {
    /// Disable every control that energizes or moves a motor (stale status,
    /// no link) and show why on the status line. STOP, "Stop sweeping all",
    /// reads, confirmations and form values stay available.
    pub fn block(&mut self, reason: String) {
        for chip in &mut self.chips {
            chip.enabled = false;
        }
        self.disable.enabled = false;
        if self.sweep_all.text != "Stop sweeping all" {
            self.sweep_all.enabled = false;
        }
        self.jog_enabled = false;
        self.target_enabled = false;
        for pose in &mut self.poses {
            pose.1 = false;
        }
        for g in [&mut self.sweep, &mut self.reset, &mut self.learn, &mut self.tune, &mut self.campaign, &mut self.gait.play] {
            g.enabled = false;
        }
        self.flip_enabled = false;
        self.clear_enabled = false;
        self.raw_step_enabled = false;
        self.status = reason.clone();
        self.blocked = Some(reason);
    }
}

/// `renderGait()` (:214-233) at `now`. A leg gait's time and speed are the
/// one leg clock ([`LinkSnapshot::leg_clock`]: the server's gait time,
/// advanced between reads only while live), the same the mirror samples and
/// `hardware_status` reports; when the leg's data is not live the clock is
/// shown frozen with why ([`LinkHealth::leg_note`]), until a fresh status.
fn render_gait(s: &LinkSnapshot, form: &Form, now: Instant) -> GaitView {
    let st = &s.state;
    let g = st.gait.as_ref();
    let run = s.gait.as_ref();
    let mut text = if s.gaits.is_empty() { "No gaits found yet.".to_string() } else { String::new() };
    if let Some(run) = run {
        let mode = match run.mode {
            GaitMode::Sim => "Sim only",
            GaitMode::Leg => "Leg only",
            GaitMode::Both => "Sim + leg",
        };
        let clock = if run.leg { s.leg_clock(now) } else { None };
        // The line shows the gait time as last read from the server (the
        // clock's base, `GaitRun::t`: it changes once per status read, not
        // every frame, so the label stays steady); the simulated legs sample
        // the same clock interpolated between reads (`Mirror::follow_gait`).
        let (t, scale) = (run.t, clock.as_ref().map_or(run.scale, |c| c.scale));
        let frozen = clock.as_ref().and_then(|c| c.frozen.as_ref());
        text = format!(
            "{mode} · gait time {} s{} of {} s period · {}% speed",
            fixed(t, 2),
            if frozen.is_some() { " (clock frozen)" } else { "" },
            fixed(run.period_s, 2),
            fixed(scale * 100.0, 0)
        );
        if let Some(note) = frozen.and_then(LinkHealth::leg_note) {
            text += &format!("\n{note}");
        }
        // Deliberate difference: the page tests `g?.limits`, and an empty
        // `limits` object is truthy there, so it prints a bare "Limits: "
        // line; here the line is left out when there are no limits. A null
        // limit prints "—": the page's `Math.round(null)` gives 0, which
        // would report a missing governor limit as 0 counts/s.
        if let Some(g) = g.filter(|g| run.leg && !g.limits.is_empty()) {
            let limits: Vec<String> = g.limits.values().map(|l| format!("{} ≤ {} counts/s, {} counts/s²", l.role, rounded(l.governor_speed_counts_s), rounded(l.governor_acceleration_counts_s2))).collect();
            text += &format!("\nLimits: {}", limits.join(" · "));
        }
        if run.leg {
            // Simulated: the server's gait state says so, or the link is pinned to a virtual bench.
            let simulated = g.is_some_and(|g| g.simulated) || s.execution.as_ref().is_some_and(|i| i.is_virtual_calibration());
            let label = if simulated { format!("{VIRTUAL_SIMULATED} · ") } else { String::new() };
            text += &format!("\n{label}Leg: {}", g.and_then(|g| g.phase.clone()).unwrap_or_else(|| "starting".into()));
            if let Some(errors) = g.and_then(|g| g.errors.as_ref()) {
                let role = |k: &str| k.parse::<u8>().ok().and_then(|id| st.calibration.as_ref()?.axes.get(&id)).map(|a| a.role.clone()).filter(|r| !r.is_empty()).unwrap_or_else(|| k.to_string());
                // A null error prints "—" (deliberate: the page's `Math.round(null)`
                // prints 0, a missing tracking error shown as a perfect one).
                let parts: Vec<String> = errors.iter().map(|(k, v)| format!("{} {}", role(k), rounded(*v))).collect();
                text += &format!(" · error {} counts", parts.join(", "));
            }
            if let Some(clamped) = g.and_then(|g| g.clamped).filter(|c| *c > 0) {
                text += &format!(" · {clamped} targets clamped to taught poses");
            }
            if !run.skipped.is_empty() {
                text += &format!("\nNot driven: {}", run.skipped.join(", "));
            }
        }
    } else if let Some(error) = g.and_then(|g| g.error.clone()).filter(|e| !e.is_empty()) {
        text = format!("Last leg gait stopped: {error}");
    }
    // A notice the link published (the gait list could not be read, a play
    // failed). The page writes these and then calls render(), which
    // overwrites them at once; the native panel keeps them until the link clears them.
    if let Some(notice) = &s.gait_notice {
        text = notice.clone();
    }
    let stats = g.filter(|g| !g.statistics.is_empty()).map(|g| stats_rows(&g.statistics)).unwrap_or_default();
    let runs = st
        .gait_runs
        .iter()
        .map(|r| {
            let path = r.gait.clone().unwrap_or_default();
            let parts: Vec<&str> = path.split('/').collect();
            // `(r.gait??'').split('/').slice(-2,-1)[0]`: the gait's folder. For a
            // path without '/' (or no gait) the page's slice is empty and it
            // prints "undefined"; here the whole path (deliberately: the
            // page's text names nothing), and `r.outcome` missing prints ""
            // where the page prints "undefined" or "null".
            let name = if parts.len() >= 2 { parts[parts.len() - 2].to_string() } else { path.clone() };
            // A run the server recorded as simulated (a row without the
            // field, from an older server, is physical or unknown: unlabelled).
            let label = if r.simulated { format!("{VIRTUAL_SIMULATED} · ") } else { String::new() };
            let heading = format!(
                "{label}{name} · effort {}% · speed {}% · {} s · {}",
                js_num(js_round(r.effort.unwrap_or(0.0) * 100.0)),
                js_num(js_round(r.speed_scale.unwrap_or(0.0) * 100.0)),
                fixed(r.gait_time_s.unwrap_or(0.0), 1),
                r.outcome.clone().unwrap_or_default()
            );
            (heading, stats_rows(&r.statistics))
        })
        .collect();
    let gait_run = run.is_some();
    GaitView {
        speed_text: format!("{}% of the gait's timing", js_num(form.inputs.gait_speed_percent)),
        effort_text: format!("{}% of measured motor capability", js_num(form.inputs.gait_effort_percent)),
        effort_enabled: !run.is_some_and(|r| r.leg),
        play: gate(
            match run {
                None => "Play",
                Some(r) if r.playing => "Pause",
                Some(_) => "Resume",
            },
            !(s.gaits.is_empty() || (form.gait_mode != GaitMode::Sim && !gait_run && !form.gait_ok) || s.campaigning || s.tuning),
        ),
        stop_enabled: gait_run,
        modes_enabled: !gait_run,
        status: text,
        stats,
        runs,
        options: s.gaits.iter().map(gait_option_label).collect(),
    }
}

#[cfg(test)]
mod tests;

/// Only release evidence stamped with the current authoritative stop epoch can
/// attest readback; the immediate latch and an ordinary command answer cannot.
pub(crate) fn release_verified(status:&Status)->bool {
    let Some(epoch)=status.stop_epoch else{return false};
    status.stop_latched && status.release.as_ref().is_some_and(|release|
        release["epoch"].as_u64()==Some(epoch) && release["verified"].as_bool()==Some(true))
}
pub(crate) fn release_note(status:&Status)->&'static str {
    if !status.stop_latched {""}
    else if release_verified(status) {" · release readback verified"}
    else {" · STOP latched; release readback pending or uncertain"}
}
