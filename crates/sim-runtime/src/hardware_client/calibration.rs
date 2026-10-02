//! `serve_actuator_calibration`: its status (tolerant: every field defaults,
//! a malformed section reads as its default, unknown fields are ignored) and
//! the command bodies the browser's `web/viewer/calibration-ui.mjs` posts to
//! `/calibration/command`, key for key and in the same order
//! (`JSON.stringify` of the object literal the page builds; `undefined`
//! members omitted). The server parses them with serde (`Request`,
//! `deny_unknown_fields`), so a misspelt key is an error there.
//!
//! Numbers are written as JavaScript writes them ([`super::js_number`],
//! ECMAScript `Number::toString`): an integral value without a fraction
//! (`5`, not `5.0`), small magnitudes in plain decimal down to 1e-6
//! (`0.0000032`), exponents as `1e-7` and `1e+21`. The server's integer
//! fields (`id` u8, `sequence` u64, `run_id` u64, `drive_pwm` u16, `delta`
//! i16) are integers here too.
use super::{Body, Json, js_number, lenient, lenient_items};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Execution provenance is strict even though ordinary status telemetry is tolerant.
/// A loopback URL, fixture label or pseudo-terminal name is never provenance.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionIdentity {
    pub schema_version: u32,
    pub kind: String,
    pub server_instance: String,
    pub bench_instance: String,
}
impl ExecutionIdentity {
    pub fn is_virtual_calibration(&self) -> bool {
        self.schema_version == 1 && self.kind == "virtual_calibration"
            && valid_instance(&self.server_instance) && valid_instance(&self.bench_instance)
    }
}
pub(crate) fn valid_instance(v: &str) -> bool {
    v.len() == 36 && v.bytes().enumerate().all(|(i, b)| {
        if matches!(i, 8 | 13 | 18 | 23) { b == b'-' } else { b.is_ascii_hexdigit() }
    })
}
/// The one remote calibration authorization policy. Callers own freshness and
/// connection generation; reconnect must first discard the pinned identity.
pub fn authorize_virtual(identity: Option<&ExecutionIdentity>, generation: u64,
    current_generation: u64, connected: bool, fresh: bool) -> Result<(), String> {
    // Identity first: a physical or unknown server is refused as such, never
    // as a merely expired session that a reconnect could seem to fix.
    if !identity.is_some_and(ExecutionIdentity::is_virtual_calibration) {
        return Err("Remote calibration requires a verified virtual calibration server".into());
    }
    if !connected || !fresh || generation == 0 || generation != current_generation {
        return Err("Virtual calibration authorization expired; reconnect explicitly".into());
    }
    Ok(())
}
/// HTTP status the calibration server answers when a command's execution
/// binding (identity, generation or a lost virtual bench) is refused.
/// Clients revoke their pinned authorization on it; an ordinary business
/// refusal (400, e.g. "Unknown motor ID" or a command outside
/// [`virtual_command_allowed`]) leaves the binding intact.
pub const BINDING_REFUSED_STATUS: u16 = 409;
/// Error text prefix the server uses for [`BINDING_REFUSED_STATUS`] answers.
pub const BINDING_REFUSED: &str = "Calibration execution binding refused";
/// Whether a failed command means the connection or its execution binding is
/// gone (transport, decode or [`BINDING_REFUSED_STATUS`]), not a refusal of
/// the command itself.
pub fn binding_lost(error: &super::ClientError) -> bool {
    use crate::loopback_http::Error as E;
    match error {
        E::NotLoopback(_) | E::Transport(_) | E::Decode(_) => true,
        E::Server { status, .. } => *status == BINDING_REFUSED_STATUS,
    }
}
/// In-scope wire commands on a virtual bench. Gait playback (`gait_start` and
/// its lease heartbeat `gait_update`) is in scope: it runs the same governed,
/// taught-window-clamped session as on the leg, against the bench's motor
/// models, and its records are labelled simulated. Raw jog (`step`/`jog`),
/// direction/flip and lesson motion (`lab_step`) stay out; a generic
/// remote-motion override does not exist. STOP is never gated.
pub fn virtual_command_allowed(action: &str) -> bool {
    matches!(action, "inspect" | "select" | "set_disabled" | "enable" | "clear"
        | "capture" | "capture_hold" | "halt" | "motion_start" | "motion_update"
        | "sweep_start" | "sweep_update" | "sweep_all" | "tune" | "campaign"
        | "gait_start" | "gait_update")
}

/// `GET /calibration/status`.
pub const STATUS: &str = "/calibration/status";
/// `POST /calibration/command`.
pub const COMMAND: &str = "/calibration/command";
/// `GET /calibration/gaits`.
pub const GAITS: &str = "/calibration/gaits";
/// `GET /calibration/export`: the calibration document.
pub const EXPORT: &str = "/calibration/export";

/// `GET /calibration/status`: the server's shared state object.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Status {
    #[serde(default, deserialize_with = "lenient")]
    pub execution: Option<ExecutionIdentity>,
    /// The powered encoder tracking session; poses taught in another are ignored.
    #[serde(default, deserialize_with = "lenient")]
    pub coordinate_session: Option<String>,
    /// The speed slider's top (`sweep_tuning.maximum_speed_counts_s`).
    #[serde(default, deserialize_with = "lenient")]
    pub maximum_speed_counts_s: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub connected: bool,
    /// The motor the server has enabled for this client (None: torque off).
    #[serde(default, deserialize_with = "lenient")]
    pub enabled_id: Option<u8>,
    #[serde(default, deserialize_with = "lenient")]
    pub busy: bool,
    /// Latest readback per motor id.
    #[serde(default, deserialize_with = "lenient")]
    pub samples: BTreeMap<u8, Telemetry>,
    #[serde(default, deserialize_with = "lenient")]
    pub message: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub error: Option<String>,
    /// The server's calibration output directory (exports are written under it).
    #[serde(default, deserialize_with = "lenient")]
    pub output: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub calibration: Option<CalibrationDoc>,
    #[serde(default, deserialize_with = "lenient")]
    pub sweep: Option<Sweep>,
    #[serde(default, deserialize_with = "lenient")]
    pub tuning: Option<Tuning>,
    #[serde(default, deserialize_with = "lenient")]
    pub campaign: Option<Campaign>,
    #[serde(default, deserialize_with = "lenient")]
    pub gait: Option<GaitState>,
    #[serde(default, deserialize_with = "lenient_items")]
    pub gait_runs: Vec<GaitRun>,
    #[serde(default, deserialize_with = "lenient")]
    pub capture_message: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub last_jog: Option<Value>,
    /// A lesson lab step (`{running, role, duty, seconds, result|error}`).
    #[serde(default, deserialize_with = "lenient")]
    pub lab: Option<Value>,
}

/// One motor's readback (`servo_bus::Telemetry` as serialized).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Telemetry {
    #[serde(default, deserialize_with = "lenient")]
    pub position_raw: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub position_continuous: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub position_rad: f64,
    #[serde(default, deserialize_with = "lenient")]
    pub speed_rad_s: f64,
    #[serde(default, deserialize_with = "lenient")]
    pub voltage_v: f64,
    #[serde(default, deserialize_with = "lenient")]
    pub temperature_c: f64,
    /// Uncalibrated (0.001 A per count, as the servo reports it).
    #[serde(default, deserialize_with = "lenient")]
    pub current_a_uncalibrated: f64,
}
impl Telemetry {
    /// `position(t)` in the page: the continuous count, else the raw one.
    pub fn position(&self) -> f64 {
        self.position_continuous.unwrap_or(self.position_raw) as f64
    }
}

/// `state.calibration`: the persisted calibration document.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct CalibrationDoc {
    #[serde(default, deserialize_with = "lenient")]
    pub fixture: String,
    #[serde(default, deserialize_with = "lenient")]
    pub units: String,
    #[serde(default, deserialize_with = "lenient")]
    pub axes: BTreeMap<u8, Axis>,
}

/// One motor's taught poses and tuning (`AxisCalibration` as serialized).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Axis {
    #[serde(default, deserialize_with = "lenient")]
    pub role: String,
    #[serde(default, deserialize_with = "lenient")]
    pub lower: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub upper: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub reference: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub reverse: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub coordinate_session: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub reference_session: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub reference_joint_rad: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub disabled: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub tuning: Option<MotorTuning>,
}

/// Gains fitted to one motor (`calibration::MotorTuning`, the fields shown).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct MotorTuning {
    #[serde(default, deserialize_with = "lenient")]
    pub pid: Pid,
    #[serde(default, deserialize_with = "lenient")]
    pub friction_duty: f64,
    #[serde(default, deserialize_with = "lenient")]
    pub record: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Pid {
    #[serde(default, deserialize_with = "lenient")]
    pub kp: f64,
    #[serde(default, deserialize_with = "lenient")]
    pub ki: f64,
    #[serde(default, deserialize_with = "lenient")]
    pub kd: f64,
}

/// `state.sweep`: the motion session (one motor, or all for Sweep all).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Sweep {
    #[serde(default, deserialize_with = "lenient")]
    pub running: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub run_id: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    pub motor_id: Option<u8>,
    #[serde(default, deserialize_with = "lenient")]
    pub motor_ids: Vec<u8>,
    #[serde(default, deserialize_with = "lenient")]
    pub skipped: Vec<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub all: bool,
    /// Latest sample per motor id (a whole `SweepSample`; the page reads
    /// `half_cycles` and `warnings`).
    #[serde(default, deserialize_with = "lenient")]
    pub axes: BTreeMap<u8, SweepAxis>,
    /// The last ≤300 samples of the selected motor.
    #[serde(default, deserialize_with = "lenient_items")]
    pub samples: Vec<SweepSample>,
    #[serde(default, deserialize_with = "lenient")]
    pub latest: Option<SweepSample>,
    #[serde(default, deserialize_with = "lenient")]
    pub motion_error: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub teaching: bool,
    /// The session's requested speed, PWM ceiling (0..=1000) and drive mode.
    #[serde(default, deserialize_with = "lenient")]
    pub speed_counts_s: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub pwm_limit: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub drive_mode: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SweepAxis {
    #[serde(default, deserialize_with = "lenient")]
    pub half_cycles: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    pub warnings: Vec<String>,
}

/// One control-period sample (`calibration_sweep::SweepSample` as serialized).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SweepSample {
    #[serde(default, deserialize_with = "lenient")]
    pub elapsed_ms: f64,
    #[serde(default, deserialize_with = "lenient")]
    pub position_raw: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub position_continuous: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub target_raw: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub target_velocity_counts_s: f64,
    #[serde(default, deserialize_with = "lenient")]
    pub velocity_counts_s: f64,
    #[serde(default, deserialize_with = "lenient")]
    pub requested_speed_counts_s: f64,
    /// Signed duty, ±1000 full scale.
    #[serde(default, deserialize_with = "lenient")]
    pub pwm: f64,
    #[serde(default, deserialize_with = "lenient")]
    pub toward_upper: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub half_cycles: u64,
    #[serde(default, deserialize_with = "lenient")]
    pub holding: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub adaptation: Option<Adaptation>,
    #[serde(default, deserialize_with = "lenient")]
    pub warnings: Vec<String>,
}
impl SweepSample {
    pub fn position(&self) -> f64 {
        self.position_continuous.unwrap_or(self.position_raw) as f64
    }
}

/// `calibration_sweep::AdaptationSample`, the fields the page shows.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Adaptation {
    #[serde(default, deserialize_with = "lenient")]
    pub status: String,
    #[serde(default, deserialize_with = "lenient")]
    pub permitted_speed_counts_s: f64,
    #[serde(default, deserialize_with = "lenient")]
    pub decreasing_stops: u64,
    #[serde(default, deserialize_with = "lenient")]
    pub increasing_stops: u64,
    #[serde(default, deserialize_with = "lenient")]
    pub braking: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub learning: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub learning_complete: bool,
}

/// `state.tuning`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Tuning {
    #[serde(default, deserialize_with = "lenient")]
    pub running: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub motor_id: Option<u8>,
    #[serde(default, deserialize_with = "lenient")]
    pub stage: String,
    #[serde(default, deserialize_with = "lenient")]
    pub error: Option<String>,
    /// Largest excursion of the identification moves (counts).
    #[serde(default, deserialize_with = "lenient")]
    pub travel_counts: Option<f64>,
}

/// `state.campaign`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Campaign {
    #[serde(default, deserialize_with = "lenient")]
    pub running: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub stage: String,
    #[serde(default, deserialize_with = "lenient")]
    pub completed: u64,
    /// `{stage, axis, completed, abort}` of the last finished stage.
    #[serde(default, deserialize_with = "lenient")]
    pub last: Option<CampaignLast>,
    #[serde(default, deserialize_with = "lenient")]
    pub error: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub result: Option<CampaignResult>,
    /// Where the receipts are written.
    #[serde(default, deserialize_with = "lenient")]
    pub directory: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub skipped: Vec<String>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct CampaignLast {
    #[serde(default, deserialize_with = "lenient")]
    pub stage: String,
    #[serde(default, deserialize_with = "lenient")]
    pub axis: Option<u8>,
    #[serde(default, deserialize_with = "lenient")]
    pub completed: bool,
    /// `characterization::Abort`, externally tagged: `{"Sag": {…}}`.
    #[serde(default, deserialize_with = "lenient")]
    pub abort: Option<Value>,
}
impl CampaignLast {
    /// `Object.keys(cp.last.abort)[0]`: the gate that stopped the last stage.
    pub fn gate(&self) -> Option<&str> {
        self.abort.as_ref()?.as_object()?.keys().next().map(String::as_str)
    }
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct CampaignResult {
    #[serde(default, deserialize_with = "lenient")]
    pub headline: String,
    #[serde(default, deserialize_with = "lenient")]
    pub directory: String,
}

/// `state.gait`: a gait playing on the leg.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct GaitState {
    #[serde(default, deserialize_with = "lenient")]
    pub running: bool,
    /// The gait plays on a virtual bench (`execution` names it in the
    /// status); absent from an older server, which means physical.
    #[serde(default, deserialize_with = "lenient")]
    pub simulated: bool,
    /// The gait's repository path.
    #[serde(default, deserialize_with = "lenient")]
    pub gait: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub t: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub speed_scale: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub effort: Option<f64>,
    /// `approach`, `playing` or `paused`.
    #[serde(default, deserialize_with = "lenient")]
    pub phase: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub motor_ids: Vec<u8>,
    /// Per motor id.
    #[serde(default, deserialize_with = "lenient")]
    pub limits: BTreeMap<String, GaitLimit>,
    /// Goal sent per motor id (counts); `None` where the server wrote a
    /// non-finite value as `null` (so one such value keeps the rest).
    #[serde(default, deserialize_with = "lenient")]
    pub targets: BTreeMap<String, Option<f64>>,
    /// Tracking error per motor id (counts); `None` per motor as in
    /// [`GaitState::targets`].
    #[serde(default, deserialize_with = "lenient")]
    pub errors: Option<BTreeMap<String, Option<f64>>>,
    /// Targets clamped to the taught poses so far.
    #[serde(default, deserialize_with = "lenient")]
    pub clamped: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    pub statistics: BTreeMap<String, GaitStatistics>,
    #[serde(default, deserialize_with = "lenient")]
    pub warnings: Vec<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct GaitLimit {
    #[serde(default, deserialize_with = "lenient")]
    pub role: String,
    /// `None` where the server wrote a non-finite value as `null` (a
    /// lenient plain `f64` would show it as a false 0).
    #[serde(default, deserialize_with = "lenient")]
    pub governor_speed_counts_s: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub governor_acceleration_counts_s2: Option<f64>,
}
/// One row of the gait statistics table (every value may be missing; the
/// server writes a non-finite value as `null`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct GaitStatistics {
    #[serde(default, deserialize_with = "lenient")]
    pub role: String,
    #[serde(default, deserialize_with = "lenient")]
    pub tracking_rms_deg: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub tracking_peak_deg: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub simulated_tracking_rms_counts: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub lag_s: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub mean_effort: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub saturated_fraction: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub governor_limited_fraction: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub peak_measured_acceleration_counts_s2: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub minimum_voltage_v: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub maximum_temperature_c: Option<f64>,
}
/// `state.gait_runs[]`: recent leg runs, newest first.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct GaitRun {
    /// The run record's file name under `<output>/gait-runs/`.
    #[serde(default, deserialize_with = "lenient")]
    pub file: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub gait: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub effort: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub speed_scale: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub gait_time_s: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub outcome: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub statistics: BTreeMap<String, GaitStatistics>,
    /// The run was on a virtual bench. A record written before runs were
    /// labelled carries no such field and is not simulated (false).
    #[serde(default, deserialize_with = "lenient")]
    pub simulated: bool,
    /// The execution (server and bench instances) that wrote the record;
    /// `None` for an older record or an unreadable identity.
    #[serde(default, deserialize_with = "lenient")]
    pub execution: Option<ExecutionIdentity>,
}

/// `GET /calibration/gaits`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Gaits {
    #[serde(default, deserialize_with = "lenient_items")]
    pub gaits: Vec<GaitEntry>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct GaitEntry {
    /// Repository path of its `compiled.json` (what `gait?path=` and `gait_start` take).
    #[serde(default, deserialize_with = "lenient")]
    pub path: String,
    /// `lab_gait` or `pose_sequence` (gait-lab results); absent for search trials.
    #[serde(default, deserialize_with = "lenient")]
    pub kind: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub study: String,
    #[serde(default, deserialize_with = "lenient")]
    pub trial: String,
    #[serde(default, deserialize_with = "lenient")]
    pub speed_m_s: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub measured_actuators: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub summary: Option<String>,
}

/// The speed and effort a motion request carries: the page's `input()`.
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub speed_counts_s: f64,
    /// `Math.round(pwm_percent * 10)`: 0..=1000.
    pub drive_pwm: u16,
    /// `hold`, `upper`, `lower`, `sweep`, `learn` or `target`.
    pub motion: String,
    pub target_raw: f64,
    pub hold_others: bool,
    /// `pwm`, `servo_position` or `servo_speed`.
    pub drive_mode: String,
}
impl Input {
    /// `...input()`, with `motion` replaced in place (`{...input(), motion: 'hold'}`).
    fn members(&self, motion: Option<&str>) -> Vec<(&'static str, Json)> {
        vec![
            ("speed_counts_s", js_number(self.speed_counts_s)),
            ("drive_pwm", Json::from(self.drive_pwm)),
            ("motion", Json::from(motion.unwrap_or(&self.motion))),
            ("target_raw", js_number(self.target_raw)),
            ("hold_others", Json::from(self.hold_others)),
            ("drive_mode", Json::from(self.drive_mode.as_str())),
        ]
    }
}

/// `send(action, extra)`: `{action, id, sequence, ...extra}`
/// (calibration-ui.mjs:67). The page never sends a command before a motor is
/// chosen; `None` omits `id`. STOP accepts a missing/null motor identity
/// and stops every configured axis; other commands still validate motor IDs.
pub fn command(action: &str, id: Option<u8>, sequence: u64, extra: Vec<(&'static str, Json)>) -> Body {
    let mut members = vec![("action", Json::from(action))];
    if let Some(id) = id {
        members.push(("id", Json::from(id)));
    }
    members.push(("sequence", Json::from(sequence)));
    members.extend(extra);
    Body::new(members)
}
/// Inspect all configured motors while stopped; no motor selection or energizing.
pub fn inspect(sequence: u64) -> Body {
    command("inspect", None, sequence, vec![])
}
/// `send('stop')` (:125).
pub fn stop(id: Option<u8>, sequence: u64) -> Body {
    command("stop", id, sequence, vec![])
}
/// `send('select', {hold_others})` (:129).
pub fn select(id: u8, sequence: u64, hold_others: bool) -> Body {
    command("select", Some(id), sequence, vec![("hold_others", Json::from(hold_others))])
}
/// `send('set_disabled', {disabled})` (:161).
pub fn set_disabled(id: u8, sequence: u64, disabled: bool) -> Body {
    command("set_disabled", Some(id), sequence, vec![("disabled", Json::from(disabled))])
}
/// `send('motion_start', input())` (:151).
pub fn motion_start(id: u8, sequence: u64, input: &Input) -> Body {
    command("motion_start", Some(id), sequence, input.members(None))
}
/// `send('motion_update', {run_id, ...input()})` (:135): the hold-to-move heartbeat.
pub fn motion_update(id: u8, sequence: u64, run_id: u64, input: &Input) -> Body {
    let mut extra = vec![("run_id", Json::from(run_id))];
    extra.extend(input.members(None));
    command("motion_update", Some(id), sequence, extra)
}
/// `send('sweep_all', input())` (:180).
pub fn sweep_all(id: u8, sequence: u64, input: &Input) -> Body {
    command("sweep_all", Some(id), sequence, input.members(None))
}
/// `send('tune', {supported: true, drive_pwm})` (:207).
pub fn tune(id: u8, sequence: u64, drive_pwm: u16) -> Body {
    command("tune", Some(id), sequence, vec![("supported", Json::from(true)), ("drive_pwm", Json::from(drive_pwm))])
}
/// `send('campaign', {supported: true, resume})` (:278).
pub fn campaign(id: u8, sequence: u64, resume: bool) -> Body {
    command("campaign", Some(id), sequence, vec![("supported", Json::from(true)), ("resume", Json::from(resume))])
}
/// One motor bound to a CAD joint for gait playback on the leg
/// (`LegMirror.gaitBindings`, calibration-mirror.mjs:123).
#[derive(Clone, Debug, PartialEq)]
pub struct GaitBinding {
    pub id: u8,
    pub joint: String,
    pub polarity: f64,
    pub home_rad: f64,
}
/// `send('gait_start', {supported: true, gait, bindings, speed_scale, effort, drive_pwm, drive_mode})` (:260).
#[allow(clippy::too_many_arguments)]
pub fn gait_start(id: u8, sequence: u64, gait: &str, bindings: &[GaitBinding], speed_scale: f64, effort: f64, drive_pwm: u16, drive_mode: &str) -> Body {
    let bindings: Vec<Json> = bindings
        .iter()
        .map(|b| Json::from(Body::new(vec![("id", Json::from(b.id)), ("joint", Json::from(b.joint.as_str())), ("polarity", js_number(b.polarity)), ("home_rad", js_number(b.home_rad))])))
        .collect();
    command(
        "gait_start",
        Some(id),
        sequence,
        vec![
            ("supported", Json::from(true)),
            ("gait", Json::from(gait)),
            ("bindings", Json::Array(bindings)),
            ("speed_scale", js_number(speed_scale)),
            ("effort", js_number(effort)),
            ("drive_pwm", Json::from(drive_pwm)),
            ("drive_mode", Json::from(drive_mode)),
        ],
    )
}
/// `api('command', {action: 'gait_update', speed_scale, playing})` (:251,
/// :261, :270): the gait lease heartbeat (no id or sequence, as the page
/// sends it).
pub fn gait_update(speed_scale: f64, playing: bool) -> Body {
    Body::new(vec![("action", Json::from("gait_update")), ("speed_scale", js_number(speed_scale)), ("playing", Json::from(playing))])
}
/// Saving a pose while no session runs: `send('capture', {boundary, ...joint})` (:302)
/// (`reference_joint_rad` only for `reference`, and only when the mirror knows it).
pub fn capture(id: u8, sequence: u64, boundary: &str, reference_joint_rad: Option<f64>) -> Body {
    let mut extra = vec![("boundary", Json::from(boundary))];
    if let Some(rad) = reference_joint_rad {
        extra.push(("reference_joint_rad", js_number(rad)));
    }
    command("capture", Some(id), sequence, extra)
}
/// Saving a pose in a held session:
/// `send('capture_hold', {boundary, run_id, ...input(), motion: 'hold', ...joint})` (:302)
/// (`motion` keeps its place from `input()`, with the value `hold`).
pub fn capture_hold(id: u8, sequence: u64, boundary: &str, run_id: u64, input: &Input, reference_joint_rad: Option<f64>) -> Body {
    let mut extra = vec![("boundary", Json::from(boundary)), ("run_id", Json::from(run_id))];
    extra.extend(input.members(Some("hold")));
    if let Some(rad) = reference_joint_rad {
        extra.push(("reference_joint_rad", js_number(rad)));
    }
    command("capture_hold", Some(id), sequence, extra)
}
/// `send('clear', {boundary})` (:305): `lower`, `upper` or `both`.
pub fn clear(id: u8, sequence: u64, boundary: &str) -> Body {
    command("clear", Some(id), sequence, vec![("boundary", Json::from(boundary))])
}
/// `send('flip')` (:307).
pub fn flip(id: u8, sequence: u64) -> Body {
    command("flip", Some(id), sequence, vec![])
}
/// `send('jog', {delta, drive_pwm})` (:320): Send raw step.
pub fn jog(id: u8, sequence: u64, delta: i16, drive_pwm: u16) -> Body {
    command("jog", Some(id), sequence, vec![("delta", Json::from(delta)), ("drive_pwm", Json::from(drive_pwm))])
}

/// `/calibration/gait?path=…`, encoded as `encodeURIComponent` does (:254).
pub fn gait_path(path: &str) -> String {
    format!("/calibration/gait?path={}", super::encode_uri_component(path))
}
