//! The robot definition RoboCAD owns: joints, motors, the actuator library,
//! sensors, cables and the document-level robot settings (battery, control
//! loop, uncertainty, actuator profiles), over `api.py`'s routes.
//!
//! - `GET /robot` (`robotics.robot_summary`, topology-only validation) and
//!   [`CadClient::robot_exact`] (`POST /ops/robot {"exact": true}`: the
//!   geometry checks too, slower).
//! - `GET /motors`: the actuator library (`MotorSpec.to_json`), the one
//!   source of motor values (never hand-copied).
//! - `GET`/`POST /sensors` and `/cables`: the nodes as `node_detail` (201
//!   for a new one; the node's `robot` block is [`SensorMeta`]/[`CableMeta`]).
//! - `GET`/`PUT /battery`, `/control`, `/uncertainty` and `GET`/`POST
//!   /actuator-profiles`: a `GET` answers the setting or `null`; a write
//!   answers the whole `doc.robot_settings` (kept as a `Value`).
//!
//! Reads are tolerant (missing fields take RoboCAD's defaults, unknown
//! ones are ignored, a malformed list or map element is dropped, a `NaN`
//! reads as `None`). Requests omit an optional field that is `None`, so
//! RoboCAD's own default applies. Every write is one RoboCAD undo step: call
//! it through a client with [`super::EDIT_TIMEOUT`].
use super::{CadClient, CadError, NodeDetail};
use crate::hardware::protocol::{lenient, lenient_items};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// `deserialize_with` for a JSON object: the entries whose value parses (a
/// malformed entry is dropped, not the map); anything but an object reads
/// as empty.
pub(super) fn lenient_map<'de, D, T>(deserializer: D) -> Result<BTreeMap<String, T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = <Value as Deserialize>::deserialize(deserializer)?;
    Ok(match value {
        Value::Object(map) => map.into_iter().filter_map(|(k, v)| serde_json::from_value(v).ok().map(|v| (k, v))).collect(),
        _ => BTreeMap::new(),
    })
}

/// An answer that is not the expected list or object.
fn unexpected(method: &'static str, route: &str, expected: &str, got: &Value) -> CadError {
    CadError { method, route: route.to_string(), status: None, message: format!("unexpected answer: expected {expected}, got {got}") }
}

/// `robotics.robot_summary`: the mechanism as RoboCAD sees it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct RobotSummary {
    #[serde(deserialize_with = "lenient_items")]
    pub joints: Vec<RobotJoint>,
    #[serde(deserialize_with = "lenient_items")]
    pub motors: Vec<RobotMotor>,
    /// Bodies (links before merging fixed joints).
    pub links: u64,
    /// Σ joint DoF; `None` with closed loops (their rank must be solved).
    #[serde(deserialize_with = "lenient")]
    pub dof: Option<i64>,
    pub has_closed_loops: bool,
    /// Ground body ids (named "ground" or flagged).
    #[serde(deserialize_with = "lenient_items")]
    pub ground: Vec<String>,
    #[serde(deserialize_with = "lenient_items")]
    pub issues: Vec<RobotIssue>,
    /// "topology only; …" or "geometry and topology" (exact).
    pub validation_scope: String,
}

/// One joint (`Joint.to_json` plus its node and the names RoboCAD looks
/// up). Pivot in mm; limits rad (mm for prismatic); damping N·m·s/rad,
/// friction N·m.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct RobotJoint {
    pub id: String,
    pub name: String,
    /// revolute | continuous | prismatic | fixed | ball | loop_revolute | loop_spherical
    #[serde(rename = "type")]
    pub kind: String,
    /// The parent body id; `None` is the world.
    pub parent: Option<String>,
    pub child: String,
    pub pivot: [f64; 3],
    pub axis: [f64; 3],
    #[serde(deserialize_with = "lenient")]
    pub lower: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub upper: Option<f64>,
    /// The motor body node id.
    pub motor: Option<String>,
    pub gear_ratio: f64,
    pub damping: f64,
    pub friction: f64,
    pub home: f64,
    /// Prismatic travel (mm) when the limits are unset.
    pub stroke: f64,
    pub parent_name: Option<String>,
    pub child_name: Option<String>,
    pub motor_name: Option<String>,
}

impl Default for RobotJoint {
    /// `Joint`'s dataclass defaults: axis +Z, gear ratio 1.
    fn default() -> Self {
        RobotJoint {
            id: String::new(),
            name: String::new(),
            kind: String::new(),
            parent: None,
            child: String::new(),
            pivot: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            lower: None,
            upper: None,
            motor: None,
            gear_ratio: 1.0,
            damping: 0.0,
            friction: 0.0,
            home: 0.0,
            stroke: 0.0,
            parent_name: None,
            child_name: None,
            motor_name: None,
        }
    }
}

/// One placed motor (`motor_body`'s metadata plus its node and spec name).
/// Points in mm.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct RobotMotor {
    pub id: String,
    pub name: String,
    /// The library id ([`MotorSpec::id`]).
    pub spec: Option<String>,
    pub spec_name: Option<String>,
    #[serde(deserialize_with = "lenient")]
    pub mount_point: Option<[f64; 3]>,
    #[serde(deserialize_with = "lenient")]
    pub shaft_axis: Option<[f64; 3]>,
    #[serde(deserialize_with = "lenient")]
    pub shaft_tip: Option<[f64; 3]>,
    pub rotation_deg: f64,
    /// The body the housing rides on.
    pub mounted_on: Option<String>,
    /// The joint it drives.
    pub drives: Option<String>,
}

/// One validation finding (`RobotIssue`): error | warning | info.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct RobotIssue {
    pub severity: String,
    pub message: String,
    pub node: Option<String>,
}

/// One actuator of the library (`MotorSpec.to_json`): sizes mm, mass g,
/// torque N·m and speed rad/s at the output shaft (gear ratio applied),
/// rotor inertia kg·m², mount holes `(x, y, diameter)` on the shaft face.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct MotorSpec {
    pub id: String,
    pub name: String,
    /// stepper | servo | dc_gearmotor | bldc | linear
    pub kind: String,
    /// box | cylinder
    pub shape: String,
    pub size: [f64; 3],
    pub shaft_diameter: f64,
    pub shaft_length: f64,
    pub mass_g: f64,
    pub stall_torque: f64,
    pub no_load_speed: f64,
    pub gear_ratio: f64,
    pub voltage: f64,
    pub rotor_inertia: f64,
    #[serde(deserialize_with = "lenient_items")]
    pub mount_holes: Vec<[f64; 3]>,
    /// `(diameter, height)` of a pilot boss around the shaft.
    #[serde(deserialize_with = "lenient")]
    pub flange: Option<[f64; 2]>,
    /// Linear actuators' travel, mm.
    pub stroke: f64,
    pub color: [f64; 3],
    pub notes: String,
}

impl Default for MotorSpec {
    /// `MotorSpec`'s dataclass defaults.
    fn default() -> Self {
        MotorSpec {
            id: String::new(),
            name: String::new(),
            kind: String::new(),
            shape: String::new(),
            size: [0.0; 3],
            shaft_diameter: 0.0,
            shaft_length: 0.0,
            mass_g: 0.0,
            stall_torque: 0.0,
            no_load_speed: 0.0,
            gear_ratio: 1.0,
            voltage: 5.0,
            rotor_inertia: 0.0,
            mount_holes: Vec::new(),
            flange: None,
            stroke: 0.0,
            color: [0.25, 0.27, 0.31],
            notes: String::new(),
        }
    }
}

/// A sensor node's `robot` block (`Ops.add_sensor`): `point` world mm,
/// `axes` rows the sensor's x, y, z in world.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct SensorMeta {
    /// imu | encoder | current | force
    pub kind: String,
    pub body: String,
    pub point: [f64; 3],
    #[serde(deserialize_with = "lenient")]
    pub axes: Option<Vec<[f64; 3]>>,
    /// The joint an encoder or current sensor reads.
    pub joint: Option<String>,
    pub joint_name: Option<String>,
    #[serde(deserialize_with = "lenient")]
    pub rate_hz: Option<f64>,
}

impl SensorMeta {
    /// A sensor node's metadata; `None` for any other node or a malformed block.
    pub fn of(node: &NodeDetail) -> Option<SensorMeta> {
        if node.summary.kind != "sensor" {
            return None;
        }
        serde_json::from_value(node.robot.clone()?).ok()
    }
}

/// A cable node's `robot` block (`Ops.add_cable`): points world mm,
/// `length` **m** (RoboCAD converts the mm it was given), `mass` kg,
/// `stiffness` N (EA), `damping` N·s.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct CableMeta {
    pub from_body: String,
    pub from_point: [f64; 3],
    pub to_body: String,
    pub to_point: [f64; 3],
    #[serde(deserialize_with = "lenient")]
    pub length: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub mass: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub stiffness: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub damping: Option<f64>,
    pub segments: u32,
}

impl Default for CableMeta {
    /// `add_cable`'s default: four segments.
    fn default() -> Self {
        CableMeta { from_body: String::new(), from_point: [0.0; 3], to_body: String::new(), to_point: [0.0; 3], length: None, mass: None, stiffness: None, damping: None, segments: 4 }
    }
}

impl CableMeta {
    /// A cable node's metadata; `None` for any other node or a malformed block.
    pub fn of(node: &NodeDetail) -> Option<CableMeta> {
        if node.summary.kind != "cable" {
            return None;
        }
        serde_json::from_value(node.robot.clone()?).ok()
    }
}

/// `POST /sensors`' body: `point` world mm. `rate_hz` is the one option
/// override sent; `None` fields are omitted (RoboCAD's defaults apply).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct SensorRequest {
    pub kind: String,
    pub body: String,
    pub point: [f64; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axes: Option<Vec<[f64; 3]>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_hz: Option<f64>,
}

/// `POST /cables`' body: points world mm, `length` **mm** (default 10 %
/// slack), `mass` kg, `stiffness` N, `damping` N·s, `segments` (default 4).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct CableRequest {
    pub from_body: String,
    pub from_point: [f64; 3],
    pub to_body: String,
    pub to_point: [f64; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mass: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stiffness: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damping: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<u32>,
}

/// `robot_settings["battery"]` (`Ops.set_battery`): volts, ohms, amp-hours.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Battery {
    pub cells: u32,
    pub chemistry: String,
    pub nominal_voltage: f64,
    pub internal_resistance: f64,
    pub capacity_ah: f64,
    pub initial_soc: f64,
    pub cutoff_voltage: f64,
}

/// `PUT /battery`'s body (`Ops.set_battery`'s keywords). RoboCAD derives
/// the nominal and cutoff voltages from the chemistry, and the internal
/// resistance too when it is `None` (omitted).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct BatteryRequest {
    pub cells: u32,
    /// lipo | liion | nimh | alkaline | lifepo4
    pub chemistry: String,
    pub capacity_ah: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal_resistance: Option<f64>,
    pub initial_soc: f64,
}

impl Default for BatteryRequest {
    /// `set_battery`'s defaults: 2 cells LiPo, 1 Ah, full.
    fn default() -> Self {
        BatteryRequest { cells: 2, chemistry: "lipo".into(), capacity_ah: 1.0, internal_resistance: None, initial_soc: 1.0 }
    }
}

/// `robot_settings["control"]` (`Ops.set_control`): seconds; targets rad
/// by joint name; trajectory points `{"t": s, "targets": {joint: rad}}`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Control {
    pub period_s: f64,
    pub latency_s: f64,
    #[serde(deserialize_with = "lenient_map")]
    pub targets: BTreeMap<String, f64>,
    /// hold | trajectory
    pub mode: String,
    #[serde(deserialize_with = "lenient_items")]
    pub trajectory: Vec<Value>,
}

/// `PUT /control`'s body (`Ops.set_control`'s keywords). The setting is
/// replaced whole: an omitted `targets`/`trajectory` becomes empty, an
/// omitted `mode` "hold".
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ControlRequest {
    pub period_s: f64,
    pub latency_s: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub targets: Option<BTreeMap<String, f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory: Option<Vec<Value>>,
}

impl Default for ControlRequest {
    /// `set_control`'s defaults: 20 ms period, 4 ms latency.
    fn default() -> Self {
        ControlRequest { period_s: 0.02, latency_s: 0.004, targets: None, mode: None, trajectory: None }
    }
}

/// `POST /actuator-profiles`' body.
#[derive(Serialize)]
struct ProfilesBody<'a> {
    profiles: &'a Value,
}

impl CadClient {
    /// `GET /robot`: joints, motors, DoF, ground and the topology-only
    /// validation issues.
    pub fn robot(&self) -> Result<RobotSummary, CadError> {
        self.get("/robot")
    }
    /// `POST /ops/robot {"args": [], "kwargs": {"exact": true}}`: the
    /// summary with the geometry checks too (pivot distance, shaft
    /// alignment, gravity load against stall torque). Changes nothing.
    pub fn robot_exact(&self) -> Result<RobotSummary, CadError> {
        let mut kwargs = Map::new();
        kwargs.insert("exact".into(), Value::Bool(true));
        let answer = self.op("robot", &[], &kwargs)?;
        serde_json::from_value(answer.result).map_err(|e| CadError { method: "POST", route: "/ops/robot".into(), status: None, message: format!("unexpected answer: {e}") })
    }
    /// `GET /motors`: the actuator library by id. A malformed entry is
    /// dropped, not the library.
    pub fn motors(&self) -> Result<BTreeMap<String, MotorSpec>, CadError> {
        match self.get::<Value>("/motors")? {
            Value::Object(map) => Ok(map.into_iter().filter_map(|(k, v)| serde_json::from_value(v).ok().map(|v| (k, v))).collect()),
            other => Err(unexpected("GET", "/motors", "the motor library", &other)),
        }
    }
    /// `GET /actuator-profiles`: the document's actuator profiles, or
    /// `null` when it has none.
    pub fn actuator_profiles(&self) -> Result<Value, CadError> {
        self.get("/actuator-profiles")
    }
    /// `POST /actuator-profiles {"profiles": …}`: validated by RoboCAD (one
    /// undo step); the profiles afterwards.
    pub fn set_actuator_profiles(&self, profiles: &Value) -> Result<Value, CadError> {
        self.send("POST", "/actuator-profiles", Some(&ProfilesBody { profiles }))
    }
    /// `GET /sensors`: every sensor node (`node_detail`). A malformed node
    /// is dropped, not the list.
    pub fn sensors(&self) -> Result<Vec<NodeDetail>, CadError> {
        self.detail_list("/sensors")
    }
    /// `POST /sensors` (201): the new sensor node (one undo step "Sensor").
    pub fn add_sensor(&self, request: &SensorRequest) -> Result<NodeDetail, CadError> {
        self.send("POST", "/sensors", Some(request))
    }
    /// `GET /cables`: every cable node (`node_detail`), leniently.
    pub fn cables(&self) -> Result<Vec<NodeDetail>, CadError> {
        self.detail_list("/cables")
    }
    /// `POST /cables` (201): the new cable node (one undo step "Cable").
    pub fn add_cable(&self, request: &CableRequest) -> Result<NodeDetail, CadError> {
        self.send("POST", "/cables", Some(request))
    }
    /// `GET /battery`: `None` when the document declares no battery.
    pub fn battery(&self) -> Result<Option<Battery>, CadError> {
        self.get("/battery")
    }
    /// `PUT /battery`: the whole `robot_settings` afterwards.
    pub fn set_battery(&self, request: &BatteryRequest) -> Result<Value, CadError> {
        self.send("PUT", "/battery", Some(request))
    }
    /// `GET /control`: `None` when the document declares no control loop.
    pub fn control(&self) -> Result<Option<Control>, CadError> {
        self.get("/control")
    }
    /// `PUT /control`: the whole `robot_settings` afterwards.
    pub fn set_control(&self, request: &ControlRequest) -> Result<Value, CadError> {
        self.send("PUT", "/control", Some(request))
    }
    /// `GET /uncertainty`: the Monte Carlo sigmas, `None` when unset.
    pub fn uncertainty(&self) -> Result<Option<Map<String, Value>>, CadError> {
        self.get("/uncertainty")
    }
    /// `PUT /uncertainty` with `sigmas` as `Ops.set_uncertainty`'s
    /// keywords (a number is a sigma — absolute for `*_m` keys, a fraction
    /// otherwise; an object is kept as given; `seed` as given), merged into
    /// the current setting: the whole `robot_settings` afterwards.
    pub fn set_uncertainty(&self, sigmas: &Map<String, Value>) -> Result<Value, CadError> {
        self.send("PUT", "/uncertainty", Some(sigmas))
    }

    /// A `GET` answering a list of `node_detail`s; a malformed one is dropped.
    fn detail_list(&self, route: &str) -> Result<Vec<NodeDetail>, CadError> {
        match self.get::<Value>(route)? {
            Value::Array(items) => Ok(items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect()),
            other => Err(unexpected("GET", route, "a list of nodes", &other)),
        }
    }
}
