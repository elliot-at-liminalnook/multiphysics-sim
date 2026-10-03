//! `serve_motor_bench`: live motor sync (the walking page's
//! `web/viewer/hardware-sync.mjs`). The viewer streams the live simulation's
//! motor targets; the server maps them to bench motors, the FPGA runs the
//! feedback loop and its watchdog, and a session ends by itself after 12 s
//! or when targets stop arriving (lease 900 ms).
//!
//! Bodies are the page's, key for key and in the same order; the server
//! parses them with serde `deny_unknown_fields`
//! (`controller_refinement::live_reference::{Request, Sample}`,
//! `trajectory_binding::Binding`). The status types are tolerant: every
//! field defaults, a malformed field reads as its default, unknown fields are
//! ignored.
use super::calibration::Telemetry;
use super::{Body, Json, js_number, lenient, lenient_items};
use serde::Deserialize;
use serde_json::Value;

/// `GET`: motor ids, CAD coordinates and the trace source.
pub const CONFIG: &str = "/config";
/// `GET`: session state and the live samples (renews the lease of its owner).
pub const STATUS: &str = "/status";
/// `POST` [`open`]: start a live session.
pub const LIVE_OPEN: &str = "/live/open";
/// `POST` [`sample`]: the latest targets.
pub const LIVE_SAMPLE: &str = "/live/sample";
/// `POST` [`stop()`]: request the physical stop (the FPGA verifies it).
pub const STOP: &str = "/stop";

/// `GET /config`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Explicit execution identity; absent means unknown, never virtual authorization.
    pub kind: String,
    pub fidelity: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub ids: Vec<u8>,
    /// `joint.<leg> | <joint>` names of the trace's coordinates.
    #[serde(default, deserialize_with = "lenient")]
    pub coordinates: Vec<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub source: String,
    #[serde(default, deserialize_with = "lenient")]
    pub source_sha256: Option<String>,
    /// The FPGA profile only accepts live targets (no recorded playback).
    #[serde(default, deserialize_with = "lenient")]
    pub streamed_only: bool,
}

/// `GET /status`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Status {
    /// A capture owns the serial port.
    #[serde(default, deserialize_with = "lenient")]
    pub active: bool,
    /// The session directory.
    #[serde(default, deserialize_with = "lenient")]
    pub run: Option<String>,
    /// The last session's `capture/run.json` (null while one runs).
    #[serde(default, deserialize_with = "lenient")]
    pub result: Option<SessionResult>,
    /// `capture/live-telemetry.jsonl` of the current or last session.
    #[serde(default, deserialize_with = "lenient_items")]
    pub samples: Vec<LiveSample>,
}

/// A session's result (`run.json`, or the server's own failure record).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SessionResult {
    #[serde(default, deserialize_with = "lenient")]
    pub completed: Option<bool>,
    #[serde(default, deserialize_with = "lenient")]
    pub error: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub result: Option<SessionOutcome>,
}
impl SessionResult {
    /// The page's closing line (hardware-sync.mjs:27).
    pub fn summary(&self) -> String {
        let verified = self.result.as_ref().and_then(|r| r.stop_verified) == Some(true);
        let why = self
            .result
            .as_ref()
            .and_then(|r| r.failure.clone())
            .or_else(|| self.error.clone())
            .unwrap_or_else(|| {
                if self.completed == Some(true) {
                    "12-second live session complete.".into()
                } else {
                    "Motor session ended without completion.".into()
                }
            });
        format!(
            "{}{why}",
            if verified {
                "Motors stopped and verified. "
            } else {
                "Stop verification incomplete. "
            }
        )
    }
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SessionOutcome {
    #[serde(default, deserialize_with = "lenient")]
    pub stop_verified: Option<bool>,
    #[serde(default, deserialize_with = "lenient")]
    pub failure: Option<String>,
}

/// One feedback read of one motor (a `live-telemetry.jsonl` line, written
/// by the acquisition examples `hx_fpga.rs.inc` / `hx_device.rs.inc`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct LiveSample {
    #[serde(default, deserialize_with = "lenient")]
    pub id: u8,
    #[serde(default, deserialize_with = "lenient")]
    pub telemetry: Telemetry,
    /// Encoder count at the session's start: angles are relative to it.
    #[serde(default, deserialize_with = "lenient")]
    pub home_raw: f64,
    /// The target applied when this feedback was read (counts from home).
    #[serde(default, deserialize_with = "lenient")]
    pub previous_target_counts: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub target_counts: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub pwm: f64,
    #[serde(default, deserialize_with = "lenient")]
    pub pwm_limit: f64,
    #[serde(default, deserialize_with = "lenient")]
    pub frame: f64,
    #[serde(default, deserialize_with = "lenient")]
    pub total_frames: Option<f64>,
    /// `[sequence, time_s, input_age_s]` of the live reference in use.
    #[serde(default, deserialize_with = "lenient")]
    pub live_source: Option<Vec<Value>>,
}
impl LiveSample {
    /// Age of the simulation input behind this target (s): `live_source[2]`.
    pub fn input_age_s(&self) -> Option<f64> {
        self.live_source.as_ref()?.get(2)?.as_f64()
    }
    /// The page's measured angle: `(position_raw - home_raw) · 360/4096` (deg).
    pub fn measured_deg(&self) -> f64 {
        (self.telemetry.position_raw as f64 - self.home_raw) * 360.0 / 4096.0
    }
    /// The page's target angle: `(previous_target_counts ?? target_counts ?? 0) · 360/4096` (deg).
    pub fn target_deg(&self) -> f64 {
        self.previous_target_counts
            .or(self.target_counts)
            .unwrap_or(0.0)
            * 360.0
            / 4096.0
    }
}

/// One CAD coordinate bound to a bench motor (`bindings()`, hardware-sync.mjs:18).
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub coordinate: String,
    pub motor_id: u8,
    /// `1` or `-1`.
    pub polarity: i8,
}

/// The live simulation's motor targets (`currentSample()`, hardware-sync.mjs:17).
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub sequence: u64,
    pub time_s: f64,
    /// In the page's coordinate order (written as the `targets_rad` object).
    pub targets: Vec<(String, f64)>,
}
impl Sample {
    fn members(&self) -> Vec<(&'static str, Json)> {
        let targets = Body(
            self.targets
                .iter()
                .map(|(name, rad)| (name.clone(), js_number(*rad)))
                .collect(),
        );
        vec![
            ("sequence", Json::from(self.sequence)),
            ("time_s", js_number(self.time_s)),
            ("targets_rad", Json::from(targets)),
        ]
    }
}

/// `api('/live/open', {bindings, amplitude, source, initial})` (hardware-sync.mjs:23).
pub fn open(bindings: &[Binding], amplitude: f64, source: &str, initial: &Sample) -> Body {
    let bindings: Vec<Json> = bindings
        .iter()
        .map(|b| {
            Json::from(Body::new(vec![
                ("coordinate", Json::from(b.coordinate.as_str())),
                ("motor_id", Json::from(b.motor_id)),
                ("polarity", Json::from(b.polarity)),
            ]))
        })
        .collect();
    Body::new(vec![
        ("bindings", Json::Array(bindings)),
        ("amplitude", js_number(amplitude)),
        ("source", Json::from(source)),
        ("initial", Json::from(Body::new(initial.members()))),
    ])
}
/// `api('/live/sample', sample)` (hardware-sync.mjs:21).
pub fn sample(s: &Sample) -> Body {
    Body::new(s.members())
}
/// `api('/stop', {})` (hardware-sync.mjs:22).
pub fn stop() -> Body {
    Body::empty()
}
