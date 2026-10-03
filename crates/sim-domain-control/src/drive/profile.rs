//! The drive profile: a robot's `sim.drive/1` file beside its model. It
//! says which body axes the robot can be driven on, how fast and how hard
//! (speed, acceleration and stop deceleration per axis, each with units and
//! provenance), what the named actions do, and the deadman rule. Its
//! numeric ranges are checked through the registry descriptor
//! [`limiter_descriptor`] (`control.drive_limiter`), so the systems editor,
//! CAD inspectors, exports and Rhai see the same parameter declarations.
//!
//! A host resolves a profile against the robot's geometry into a
//! [`ResolvedDrive`] (`sim.drive.resolved/1`), the JSON a controller
//! receives as `--drive-json`.
use super::geometry::{DriveGeometry, Provenance, Valued, WheelJoint};
use super::kinematics::{self, ACCEL_UNITS, AXIS_NAMES, BodyTwist, Deadman, KinematicsError, Limits, OnLoss, SPEED_UNITS};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sim_core::{
    Behavior, BehaviorDescriptor, ComponentNotes, Context, EquationError, ParameterDeclaration, QuantityKind, StateDeclaration, View,
    param, signal_in, signal_out,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

/// The schema this build reads and writes.
pub const SCHEMA: &str = "sim.drive/1";
const SCHEMA_PREFIX: &str = "sim.drive/";
const SCHEMA_VERSION: u64 = 1;
/// The resolved form handed to a controller.
pub const RESOLVED_SCHEMA: &str = "sim.drive.resolved/1";
/// Registry type id of the drive limiter.
pub const LIMITER: &str = "control.drive_limiter";

/// Which wheel joints the kinematic adapter drives, in the model's names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum KinematicsSpec {
    Differential { left: String, right: String },
    Mecanum { front_left: String, front_right: String, rear_left: String, rear_right: String },
}

impl KinematicsSpec {
    /// `"differential"` or `"mecanum"` (the resolved file's `kinematics`).
    pub fn name(&self) -> &'static str {
        match self {
            KinematicsSpec::Differential { .. } => "differential",
            KinematicsSpec::Mecanum { .. } => "mecanum",
        }
    }
    /// Field names of the wheel roles, in mixer order.
    pub fn roles(&self) -> &'static [&'static str] {
        match self {
            KinematicsSpec::Differential { .. } => &["left", "right"],
            KinematicsSpec::Mecanum { .. } => &["front_left", "front_right", "rear_left", "rear_right"],
        }
    }
    /// Wheel joint names in mixer order (`[left, right]` or `[fl, fr, rl, rr]`).
    pub fn joints(&self) -> Vec<&str> {
        match self {
            KinematicsSpec::Differential { left, right } => vec![left.as_str(), right.as_str()],
            KinematicsSpec::Mecanum { front_left, front_right, rear_left, rear_right } => {
                vec![front_left.as_str(), front_right.as_str(), rear_left.as_str(), rear_right.as_str()]
            }
        }
    }
}

/// A declared number and where it comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredValue {
    pub value: f64,
    pub source: String,
}

/// Where the drive geometry comes from: derived from the robot's model
/// (the default, so CAD stays the owner), or declared in the profile with a
/// source for each value (for a robot whose model lacks wheel geometry, or
/// a mecanum drive, whose roller layout the model does not describe).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum GeometrySource {
    /// `{"source": "model"}`. An empty struct variant, not a unit variant:
    /// serde ignores extra keys beside an internally tagged unit variant
    /// even under `deny_unknown_fields` (serde's InternallyTaggedUnitVisitor),
    /// while a struct variant refuses them by name.
    Model {},
    Declared {
        track_width_m: DeclaredValue,
        #[serde(default)]
        wheelbase_m: Option<DeclaredValue>,
        wheel_radius_m: DeclaredValue,
        /// Joint sign per wheel in mixer order (+1 or -1).
        signs: Vec<f64>,
    },
}

/// A value with its unit, written out so a file never leaves units implicit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quantity {
    pub value: f64,
    pub unit: String,
}

/// One supported body axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisSpec {
    /// Largest commanded speed (m/s or rad/s); nonnegative.
    pub max_speed: Quantity,
    /// Largest change of commanded speed per second while a request is live.
    pub max_accel: Quantity,
    /// Deceleration used when the deadman expires with `on_loss: ramp`.
    pub stop_decel: Quantity,
    pub provenance: Provenance,
}

/// What a named action requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionRequest {
    /// Zero twist, approached under each axis's max_accel.
    Stop,
    /// Zero twist at once.
    Halt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedAction {
    pub name: String,
    pub request: ActionRequest,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnLossSpec {
    /// Ramp to zero under each axis's `stop_decel`.
    Ramp,
    /// Zero at once.
    Immediate,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeadmanSpec {
    /// A request older than this (s) is lost.
    pub timeout_s: f64,
    pub on_loss: OnLossSpec,
    pub provenance: Provenance,
}

/// A robot's drive profile (`sim.drive/1`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveProfile {
    pub schema: String,
    #[serde(default)]
    pub description: Option<String>,
    pub kinematics: KinematicsSpec,
    pub geometry: GeometrySource,
    /// Keys `forward`, `lateral`, `yaw`; an absent axis is unsupported.
    /// Intended: a key written twice in the file keeps its last value (JSON
    /// objects are maps; serde_json keeps the last duplicate), as for every
    /// other object in the file.
    pub axes: BTreeMap<String, AxisSpec>,
    pub actions: Vec<NamedAction>,
    pub deadman: DeadmanSpec,
}

/// A refused profile: the file, the field path and why.
#[derive(Clone, Debug, PartialEq)]
pub struct DriveProfileError {
    pub file: PathBuf,
    pub field: String,
    pub message: String,
}

impl DriveProfileError {
    pub fn new(file: &Path, field: impl Into<String>, message: impl Into<String>) -> Self {
        Self { file: file.to_path_buf(), field: field.into(), message: message.into() }
    }
}

impl fmt::Display for DriveProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.file.display(), self.field, self.message)
    }
}

impl std::error::Error for DriveProfileError {}

/// sha256 of `bytes` as lowercase hex (the identity recorded with runs).
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The schema rule, checked before serde so a newer file is named as newer
/// rather than as a pile of unknown fields.
fn check_schema(schema: Option<&Value>, file: &Path) -> Result<(), DriveProfileError> {
    let refuse = |message: String| Err(DriveProfileError::new(file, "schema", message));
    match schema {
        None => refuse(format!("missing; a drive profile starts with \"schema\": \"{SCHEMA}\"")),
        Some(Value::String(s)) if s == SCHEMA => Ok(()),
        Some(Value::String(s)) => match s.strip_prefix(SCHEMA_PREFIX).and_then(|n| n.parse::<u64>().ok()) {
            Some(n) if n > SCHEMA_VERSION => refuse(format!("newer schema {s} than this build reads; this build reads {SCHEMA}. Update the simulator to open it")),
            _ => refuse(format!("unknown schema `{s}`; this build reads {SCHEMA}")),
        },
        Some(other) => refuse(format!("must be the string \"{SCHEMA}\", not {other}")),
    }
}

/// Deserialize one section on its own, so a failure can name its path.
fn probe<T: DeserializeOwned>(value: &Value, path: String) -> Option<(String, String)> {
    T::deserialize(value).err().map(|e| (path, e.to_string()))
}

/// The deepest section of a document that fails to deserialize, with
/// serde_json's message (which names an unknown or missing field). `None`
/// when every present section is fine on its own (an unknown or missing
/// top-level field).
fn failing_section(map: &serde_json::Map<String, Value>) -> Option<(String, String)> {
    if let Some(v) = map.get("description") {
        if let Some(e) = probe::<Option<String>>(v, "description".into()) {
            return Some(e);
        }
    }
    if let Some(v) = map.get("kinematics") {
        if let Some(e) = probe::<KinematicsSpec>(v, "kinematics".into()) {
            return Some(e);
        }
    }
    if let Some(v) = map.get("geometry") {
        if let Some(e) = probe::<GeometrySource>(v, "geometry".into()) {
            return Some(e);
        }
    }
    if let Some(v) = map.get("axes") {
        let Value::Object(axes) = v else {
            return probe::<BTreeMap<String, AxisSpec>>(v, "axes".into());
        };
        for (name, axis) in axes {
            if let Value::Object(fields) = axis {
                for quantity in ["max_speed", "max_accel", "stop_decel"] {
                    if let Some(q) = fields.get(quantity) {
                        if let Some(e) = probe::<Quantity>(q, format!("axes.{name}.{quantity}")) {
                            return Some(e);
                        }
                    }
                }
                if let Some(p) = fields.get("provenance") {
                    if let Some(e) = probe::<Provenance>(p, format!("axes.{name}.provenance")) {
                        return Some(e);
                    }
                }
            }
            if let Some(e) = probe::<AxisSpec>(axis, format!("axes.{name}")) {
                return Some(e);
            }
        }
    }
    if let Some(v) = map.get("actions") {
        let Value::Array(actions) = v else {
            return probe::<Vec<NamedAction>>(v, "actions".into());
        };
        for (i, action) in actions.iter().enumerate() {
            if let Some(e) = probe::<NamedAction>(action, format!("actions[{i}]")) {
                return Some(e);
            }
        }
    }
    if let Some(v) = map.get("deadman") {
        if let Some(e) = probe::<DeadmanSpec>(v, "deadman".into()) {
            return Some(e);
        }
    }
    None
}

fn nonempty(text: &str) -> bool {
    !text.trim().is_empty()
}

fn provenance_ok(provenance: &Provenance) -> bool {
    match provenance {
        Provenance::Derived { from } => nonempty(from),
        Provenance::Declared { source } | Provenance::Estimated { source } | Provenance::Measured { source } => nonempty(source),
    }
}

/// A limiter parameter name to the profile field it came from.
fn parameter_field(name: &str) -> String {
    match name.split_once('.') {
        Some((quantity @ ("max_speed" | "max_accel" | "stop_decel"), axis)) => format!("axes.{axis}.{quantity}.value"),
        _ => match name {
            "deadman_timeout" => "deadman.timeout_s".into(),
            "on_loss_immediate" => "deadman.on_loss".into(),
            other => other.into(),
        },
    }
}

impl DriveProfile {
    /// Parse and validate a profile. The schema is checked first (a newer
    /// `sim.drive/N` is named as newer), then serde (an unknown field keeps
    /// serde_json's message, under the section it is in), then [`validate`](Self::validate).
    pub fn from_json(text: &str, file: &Path) -> Result<Self, DriveProfileError> {
        let value: Value = serde_json::from_str(text).map_err(|e| DriveProfileError::new(file, "(document)", format!("not valid JSON: {e}")))?;
        let Value::Object(map) = &value else {
            return Err(DriveProfileError::new(file, "(document)", "a drive profile is a JSON object"));
        };
        check_schema(map.get("schema"), file)?;
        let profile: DriveProfile = match serde_json::from_str(text) {
            Ok(profile) => profile,
            Err(whole) => {
                let (field, message) = failing_section(map).unwrap_or_else(|| ("(top level)".into(), whole.to_string()));
                return Err(DriveProfileError::new(file, field, message));
            }
        };
        profile.validate(file)?;
        Ok(profile)
    }

    /// Read, parse and validate a profile file; also returns the sha256 of
    /// its bytes (recorded with runs, so a replay can tell the file changed).
    /// Reads the disk: call it off the UI thread.
    pub fn load(path: &Path) -> Result<(Self, String), DriveProfileError> {
        let bytes = std::fs::read(path).map_err(|e| DriveProfileError::new(path, "(file)", format!("cannot read: {e}")))?;
        let sha = sha256_hex(&bytes);
        let text = std::str::from_utf8(&bytes).map_err(|e| DriveProfileError::new(path, "(file)", format!("not UTF-8: {e}")))?;
        Ok((Self::from_json(text, path)?, sha))
    }

    /// Check everything serde cannot: the schema, axis names, units,
    /// numeric ranges (through the registry descriptor), wheel names,
    /// action names, declared geometry and provenance.
    pub fn validate(&self, file: &Path) -> Result<(), DriveProfileError> {
        let refuse = |field: String, message: String| Err(DriveProfileError::new(file, field, message));
        check_schema(Some(&Value::String(self.schema.clone())), file)?;

        // Wheel joints: named and distinct.
        let joints = self.kinematics.joints();
        let mut seen = BTreeSet::new();
        for (role, joint) in self.kinematics.roles().iter().zip(&joints) {
            if !nonempty(joint) {
                return refuse(format!("kinematics.{role}"), "must name a wheel joint of the model".into());
            }
            if !seen.insert(*joint) {
                return refuse(format!("kinematics.{role}"), format!("joint `{joint}` is listed for two wheels"));
            }
        }

        // Axes: known names, the drive can move that way, units, finite values.
        if self.axes.is_empty() {
            return refuse("axes".into(), "no supported axis; list at least one of forward, lateral, yaw".into());
        }
        for (name, axis) in &self.axes {
            let Some(i) = AXIS_NAMES.iter().position(|n| n == name) else {
                return refuse(format!("axes.{name}"), "unknown axis; the axes are forward, lateral and yaw".into());
            };
            if i == kinematics::LATERAL && matches!(self.kinematics, KinematicsSpec::Differential { .. }) {
                return refuse(format!("axes.{name}"), "a differential drive cannot move sideways; remove the lateral axis".into());
            }
            for (quantity, q, unit) in [("max_speed", &axis.max_speed, SPEED_UNITS[i]), ("max_accel", &axis.max_accel, ACCEL_UNITS[i]), ("stop_decel", &axis.stop_decel, ACCEL_UNITS[i])] {
                if q.unit != unit {
                    return refuse(format!("axes.{name}.{quantity}.unit"), format!("is `{}`; this axis's {quantity} is in `{unit}`", q.unit));
                }
                if !q.value.is_finite() {
                    return refuse(format!("axes.{name}.{quantity}.value"), "must be a finite number".into());
                }
            }
            if !provenance_ok(&axis.provenance) {
                return refuse(format!("axes.{name}.provenance"), "must say where the limits come from".into());
            }
        }
        if !self.deadman.timeout_s.is_finite() {
            return refuse("deadman.timeout_s".into(), "must be a finite number".into());
        }
        if !provenance_ok(&self.deadman.provenance) {
            return refuse("deadman.provenance".into(), "must say where the deadman rule comes from".into());
        }

        // Numeric ranges: the registry's declarations, the same ones the
        // systems editor and exports show for control.drive_limiter.
        profile_descriptor()
            .validate_parameters(&self.limiter_parameters())
            .map_err(|e| match e {
                EquationError::InvalidParameter(name, message) => DriveProfileError::new(file, parameter_field(&name), message),
                EquationError::MissingParameter(name) => DriveProfileError::new(file, parameter_field(&name), "missing"),
            })?;

        // Actions: named and unique.
        let mut names = BTreeSet::new();
        for (i, action) in self.actions.iter().enumerate() {
            if !nonempty(&action.name) {
                return refuse(format!("actions[{i}].name"), "must not be empty".into());
            }
            if !names.insert(action.name.as_str()) {
                return refuse(format!("actions[{i}].name"), format!("action `{}` is listed twice", action.name));
            }
        }

        // Declared geometry: complete for the kinematics, with sources.
        if let GeometrySource::Declared { track_width_m, wheelbase_m, wheel_radius_m, signs } = &self.geometry {
            let mecanum = matches!(self.kinematics, KinematicsSpec::Mecanum { .. });
            let mut values = vec![("track_width_m", track_width_m), ("wheel_radius_m", wheel_radius_m)];
            match (mecanum, wheelbase_m) {
                (true, Some(w)) => values.push(("wheelbase_m", w)),
                (true, None) => return refuse("geometry.wheelbase_m".into(), "a mecanum drive needs its wheelbase".into()),
                (false, Some(_)) => return refuse("geometry.wheelbase_m".into(), "a differential drive has no wheelbase; remove it".into()),
                (false, None) => {}
            }
            for (name, v) in values {
                if !(v.value.is_finite() && v.value > 0.0) {
                    return refuse(format!("geometry.{name}.value"), "must be a positive length in metres".into());
                }
                if !nonempty(&v.source) {
                    return refuse(format!("geometry.{name}.source"), "must say where the value comes from".into());
                }
            }
            if signs.len() != joints.len() {
                return refuse("geometry.signs".into(), format!("lists {} signs; this drive has {} wheels ({})", signs.len(), joints.len(), self.kinematics.roles().join(", ")));
            }
            for (i, s) in signs.iter().enumerate() {
                if *s != 1.0 && *s != -1.0 {
                    return refuse(format!("geometry.signs[{i}]"), format!("is {s}; a joint sign is +1 or -1"));
                }
            }
        }
        Ok(())
    }

    /// The profile's numbers under the limiter's parameter names (no
    /// `period`: a profile does not choose the control period).
    pub fn limiter_parameters(&self) -> BTreeMap<String, f64> {
        let mut values = BTreeMap::new();
        for (name, axis) in &self.axes {
            values.insert(format!("max_speed.{name}"), axis.max_speed.value);
            values.insert(format!("max_accel.{name}"), axis.max_accel.value);
            values.insert(format!("stop_decel.{name}"), axis.stop_decel.value);
        }
        values.insert("deadman_timeout".into(), self.deadman.timeout_s);
        values.insert("on_loss_immediate".into(), if self.deadman.on_loss == OnLossSpec::Immediate { 1.0 } else { 0.0 });
        values
    }

    fn axis(&self, i: usize) -> Option<&AxisSpec> {
        self.axes.get(AXIS_NAMES[i])
    }

    /// Plain-number limits in axis order. An unsupported axis has speed 0;
    /// its acceleration and stop deceleration are never used (its speed is
    /// always zero) but the shared checks need them positive, so it takes
    /// the other linear axis's values when that one is supported (the same
    /// units), else 1.
    pub fn resolved_limits(&self) -> ResolvedLimits {
        let axes: [Option<&AxisSpec>; 3] = std::array::from_fn(|i| self.axis(i));
        let pick = |i: usize, f: fn(&AxisSpec) -> f64| -> f64 {
            let sibling = match i {
                0 => Some(1),
                1 => Some(0),
                _ => None,
            };
            axes[i].map(f).or_else(|| sibling.and_then(|j: usize| axes[j].map(f))).unwrap_or(1.0)
        };
        ResolvedLimits {
            supported: std::array::from_fn(|i| axes[i].is_some()),
            max_speed: std::array::from_fn(|i| axes[i].map_or(0.0, |a| a.max_speed.value)),
            max_accel: std::array::from_fn(|i| pick(i, |a| a.max_accel.value)),
            stop_decel: std::array::from_fn(|i| pick(i, |a| a.stop_decel.value)),
        }
    }

    pub fn limits(&self) -> Limits {
        self.resolved_limits().limits()
    }

    pub fn deadman(&self) -> Deadman {
        let decel = self.resolved_limits().stop_decel;
        Deadman {
            timeout_s: self.deadman.timeout_s,
            on_loss: match self.deadman.on_loss {
                OnLossSpec::Ramp => OnLoss::Ramp { decel },
                OnLossSpec::Immediate => OnLoss::Immediate,
            },
        }
    }

    /// A named action, or an error listing the profile's actions.
    pub fn action(&self, name: &str) -> Result<&NamedAction, String> {
        self.actions.iter().find(|a| a.name == name).ok_or_else(|| {
            let names: Vec<&str> = self.actions.iter().map(|a| a.name.as_str()).collect();
            format!("the drive profile has no action `{name}`; it has: {}", if names.is_empty() { "none".into() } else { names.join(", ") })
        })
    }

    /// The declared geometry with `Declared` provenance, `Ok(None)` when
    /// the geometry comes from the model.
    pub fn declared_geometry(&self) -> Result<Option<DriveGeometry>, String> {
        let GeometrySource::Declared { track_width_m, wheelbase_m, wheel_radius_m, signs } = &self.geometry else {
            return Ok(None);
        };
        let joints = self.kinematics.joints();
        if signs.len() != joints.len() {
            return Err(format!("geometry.signs lists {} signs; this drive has {} wheels", signs.len(), joints.len()));
        }
        let declared = |v: &DeclaredValue| Valued { value: v.value, provenance: Provenance::Declared { source: v.source.clone() } };
        Ok(Some(DriveGeometry {
            track_width_m: declared(track_width_m),
            wheelbase_m: wheelbase_m.as_ref().map(declared),
            wheel_radius_m: declared(wheel_radius_m),
            wheels: joints
                .iter()
                .zip(signs)
                .zip(self.kinematics.roles())
                .map(|((joint, sign), role)| WheelJoint {
                    joint: joint.to_string(),
                    sign: *sign,
                    provenance: Provenance::Declared { source: format!("drive profile geometry.signs ({role} wheel)") },
                })
                .collect(),
        }))
    }

    /// The resolved drive a controller receives: this profile's limits and
    /// deadman with `geometry` (derived from the model or declared). The
    /// geometry must list this profile's wheel joints in mixer order and
    /// build its mixer.
    pub fn resolve(&self, file: &Path, profile_sha256: &str, geometry: DriveGeometry) -> Result<ResolvedDrive, DriveProfileError> {
        let expected = self.kinematics.joints();
        if geometry.joints() != expected {
            return Err(DriveProfileError::new(
                file,
                "geometry",
                format!("the geometry's wheels {:?} are not the profile's {} wheels {:?}", geometry.joints(), self.kinematics.name(), expected),
            ));
        }
        let built = match self.kinematics {
            KinematicsSpec::Differential { .. } => geometry.differential().map(|_| ()),
            KinematicsSpec::Mecanum { .. } => geometry.mecanum().map(|_| ()),
        };
        built.map_err(|e| DriveProfileError::new(file, "geometry", e))?;
        Ok(ResolvedDrive {
            schema: RESOLVED_SCHEMA.into(),
            profile: file.display().to_string(),
            profile_sha256: profile_sha256.into(),
            kinematics: self.kinematics.name().into(),
            geometry,
            limits: self.resolved_limits(),
            deadman: ResolvedDeadman {
                timeout_s: self.deadman.timeout_s,
                on_loss: match self.deadman.on_loss {
                    OnLossSpec::Ramp => "ramp".into(),
                    OnLossSpec::Immediate => "immediate".into(),
                },
            },
        })
    }
}

/// Limits in plain numbers, axis order `[forward, lateral, yaw]`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedLimits {
    pub supported: [bool; 3],
    pub max_speed: [f64; 3],
    pub max_accel: [f64; 3],
    pub stop_decel: [f64; 3],
}

impl ResolvedLimits {
    pub fn limits(&self) -> Limits {
        Limits { supported: self.supported, max_speed: self.max_speed, max_accel: self.max_accel }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedDeadman {
    pub timeout_s: f64,
    /// `"ramp"` or `"immediate"`.
    pub on_loss: String,
}

/// What a host hands a robot's controller (`--drive-json '<json>'`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedDrive {
    pub schema: String,
    /// The profile path as the host found it.
    pub profile: String,
    pub profile_sha256: String,
    /// `"differential"` or `"mecanum"`.
    pub kinematics: String,
    pub geometry: DriveGeometry,
    pub limits: ResolvedLimits,
    pub deadman: ResolvedDeadman,
}

impl ResolvedDrive {
    pub fn limits(&self) -> Limits {
        self.limits.limits()
    }
    /// The deadman rule. Anything but `"ramp"` stops at once (a resolved
    /// file is written by [`DriveProfile::resolve`]; [`validate`](Self::validate) names a bad value).
    pub fn deadman(&self) -> Deadman {
        Deadman {
            timeout_s: self.deadman.timeout_s,
            on_loss: if self.deadman.on_loss == "ramp" { OnLoss::Ramp { decel: self.limits.stop_decel } } else { OnLoss::Immediate },
        }
    }
    /// Check a resolved drive read back from JSON: schema, deadman, limits
    /// and the mixer it names.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RESOLVED_SCHEMA {
            return Err(format!("resolved drive schema is `{}`; this build reads {RESOLVED_SCHEMA}", self.schema));
        }
        if self.deadman.on_loss != "ramp" && self.deadman.on_loss != "immediate" {
            return Err(format!("resolved drive deadman.on_loss is `{}`; it is ramp or immediate", self.deadman.on_loss));
        }
        if !(self.deadman.timeout_s.is_finite() && self.deadman.timeout_s > 0.0) {
            return Err("resolved drive deadman.timeout_s must be positive".into());
        }
        if self.kinematics == "differential" && self.limits.supported[kinematics::LATERAL] {
            return Err("resolved drive limits.supported[1] (lateral) is true; a differential drive cannot move sideways".into());
        }
        if self.limits.stop_decel.iter().any(|d| !(d.is_finite() && *d > 0.0)) {
            return Err("resolved drive limits.stop_decel must be positive".into());
        }
        kinematics::check_twist(BodyTwist::ZERO, &self.limits()).map_err(|e| format!("resolved drive limits: {e}"))?;
        match self.kinematics.as_str() {
            "differential" => self.geometry.differential().map(|_| ()),
            "mecanum" => self.geometry.mecanum().map(|_| ()),
            other => Err(format!("resolved drive kinematics is `{other}`; it is differential or mecanum")),
        }
    }
}

// ---------------------------------------------------------------------------
// Registry: control.drive_limiter

static LIMITER_NOTES: ComponentNotes = ComponentNotes {
    category: "Control",
    explanation: "The shared teleoperation rule every host applies to a drive request (the viewer's run thread, a robot's controller and this element). \
        Every `period` it samples the requested body twist and the request sequence (heartbeat). A request whose sequence has advanced is live: it must lie within each axis's max speed \
        (zero on an axis without max_speed) and the output approaches it by at most max_accel·period per sample. When no new sequence has arrived for deadman_timeout, \
        the request is ignored and the output stops: ramped down under stop_decel, or zero at once when on_loss_immediate is 1. A request outside the profile makes the outputs NaN \
        (refused, not clipped). A robot's drive profile (sim.drive/1) is validated against these same declarations.",
    equations: &[
        "age = 0 when sequence advanced, else min(age + period, deadman_timeout)",
        "live (age < deadman_timeout): twist_i ← twist_i + clamp(request_i − twist_i, ±max_accel_i·period)",
        "lost, ramp: twist_i ← twist_i + clamp(−twist_i, ±stop_decel_i·period)",
        "lost, immediate: twist ← 0",
    ],
    tradeoffs: "A rate limit on the commanded twist, not a dynamic model: actuator torque, wheel slip and the robot's own controller decide what the body does. \
        Use control.command_lease alone when only freshness matters.",
    limits: "No jerk limit; the first sample uses dt = 0. The deadman counts samples, so a timeout shorter than one period always expires.",
    parameters: &[
        ("max_speed.forward", "Largest forward speed; omit for an axis the robot cannot move on"),
        ("max_speed.lateral", "Largest sideways speed (left positive); omit for a differential drive"),
        ("max_speed.yaw", "Largest yaw rate (counter-clockwise positive)"),
        ("max_accel.forward", "Largest change of forward speed per second while a request is live"),
        ("max_accel.lateral", "Largest change of lateral speed per second while a request is live"),
        ("max_accel.yaw", "Largest change of yaw rate per second while a request is live"),
        ("stop_decel.forward", "Forward deceleration after the deadman expires (ramp)"),
        ("stop_decel.lateral", "Lateral deceleration after the deadman expires (ramp)"),
        ("stop_decel.yaw", "Yaw deceleration after the deadman expires (ramp)"),
        ("deadman_timeout", "A request older than this is lost"),
        ("on_loss_immediate", "0: ramp to zero under stop_decel; 1: zero at once"),
        ("period", "Sample period (the controller's loop period)"),
    ],
    pairs_with: &["control.command_lease"],
    typical: &[
        ("max_speed.forward", 0.3),
        ("max_accel.forward", 0.6),
        ("stop_decel.forward", 1.2),
        ("max_speed.yaw", 3.0),
        ("max_accel.yaw", 6.0),
        ("stop_decel.yaw", 12.0),
        ("deadman_timeout", 0.5),
        ("on_loss_immediate", 0.0),
        ("period", 0.02),
    ],
    ..ComponentNotes::new("Limits a requested body twist (speed and acceleration per axis) and stops it when requests stop arriving.")
};

/// The registry description of the drive limiter, `control.drive_limiter`.
/// Inputs `request.forward`, `request.lateral` (m/s), `request.yaw` (rad/s)
/// and `sequence` (the request heartbeat); outputs `twist.forward`,
/// `twist.lateral`, `twist.yaw`. Per-axis parameters are optional as a
/// group: an axis with `max_speed.<axis>` needs `max_accel.<axis>` (and
/// `stop_decel.<axis>` when ramping); an axis without it is unsupported.
pub fn limiter_descriptor() -> BehaviorDescriptor {
    use ParameterDeclaration as P;
    let mut parameters = Vec::new();
    for (i, axis) in AXIS_NAMES.iter().enumerate() {
        parameters.push(P::alternative(format!("max_speed.{axis}"), SPEED_UNITS[i]).nonnegative());
        parameters.push(P::alternative(format!("max_accel.{axis}"), ACCEL_UNITS[i]).positive());
        parameters.push(P::alternative(format!("stop_decel.{axis}"), ACCEL_UNITS[i]).positive());
    }
    parameters.push(P::required("deadman_timeout", "s").positive());
    parameters.push(P::required("on_loss_immediate", "1").integer(0.0, 1.0));
    parameters.push(P::required("period", "s").positive());
    BehaviorDescriptor::new(
        LIMITER,
        "Drive limiter (twist rate limit and deadman)",
        vec![
            signal_in("request.forward", QuantityKind::LinearVelocity),
            signal_in("request.lateral", QuantityKind::LinearVelocity),
            signal_in("request.yaw", QuantityKind::AngularVelocity),
            signal_in("sequence", QuantityKind::Dimensionless),
            signal_out("twist.forward", QuantityKind::LinearVelocity),
            signal_out("twist.lateral", QuantityKind::LinearVelocity),
            signal_out("twist.yaw", QuantityKind::AngularVelocity),
        ],
        make_limiter,
    )
    .with_parameters(parameters)
    .with_notes(&LIMITER_NOTES)
}

/// The descriptor a profile is validated against: the limiter's, without
/// `period` (the host's control period supplies it, not the profile).
fn profile_descriptor() -> BehaviorDescriptor {
    let mut descriptor = limiter_descriptor();
    if let Some(parameters) = &mut descriptor.parameters {
        parameters.retain(|p| p.name != "period");
    }
    descriptor
}

/// The drive limiter as a sampled element: the shared [`kinematics::step`]
/// run every `period` on its inputs.
#[derive(Clone, Debug)]
pub struct DriveLimiter {
    pub limits: Limits,
    pub deadman: Deadman,
    pub period_s: f64,
}

impl DriveLimiter {
    /// Build from limiter parameters (see [`limiter_descriptor`]).
    pub fn from_parameters(p: &BTreeMap<String, f64>) -> Result<Self, EquationError> {
        fn invalid(name: &str, message: String) -> EquationError {
            EquationError::InvalidParameter(name.into(), message)
        }
        let immediate = param(p, "on_loss_immediate")? == 1.0;
        let mut supported = [false; 3];
        let mut max_speed = [0.0; 3];
        // Unsupported axes keep these unused placeholders: their speed is
        // always zero, and the shared checks want positive limits.
        let mut max_accel = [1.0; 3];
        let mut stop_decel = [1.0; 3];
        for (i, axis) in AXIS_NAMES.iter().enumerate() {
            let (speed, accel, decel) = (format!("max_speed.{axis}"), format!("max_accel.{axis}"), format!("stop_decel.{axis}"));
            match p.get(&speed) {
                Some(v) => {
                    supported[i] = true;
                    max_speed[i] = *v;
                    max_accel[i] = param(p, &accel)?;
                    if immediate {
                        stop_decel[i] = p.get(&decel).copied().unwrap_or(1.0);
                    } else {
                        stop_decel[i] = param(p, &decel)?;
                    }
                }
                None => {
                    for name in [&accel, &decel] {
                        if p.contains_key(name) {
                            return Err(invalid(name, format!("given without {speed}; an axis is supported only with its max speed")));
                        }
                    }
                }
            }
        }
        // The same placeholder rule as `DriveProfile::resolved_limits`: an
        // unsupported linear axis takes the other linear axis's values.
        for (i, j) in [(0, 1), (1, 0)] {
            if !supported[i] && supported[j] {
                max_accel[i] = max_accel[j];
                stop_decel[i] = stop_decel[j];
            }
        }
        if !supported.contains(&true) {
            return Err(invalid("max_speed.forward", "the drive limiter needs at least one axis: give max_speed.forward, max_speed.lateral or max_speed.yaw".into()));
        }
        let limits = Limits { supported, max_speed, max_accel };
        kinematics::check_twist(BodyTwist::ZERO, &limits).map_err(|e| invalid(LIMITER, e.to_string()))?;
        if let Some(i) = (0..3).find(|i| !(stop_decel[*i].is_finite() && stop_decel[*i] > 0.0)) {
            return Err(invalid(&format!("stop_decel.{}", AXIS_NAMES[i]), "must be positive".into()));
        }
        let period_s = param(p, "period")?;
        let timeout_s = param(p, "deadman_timeout")?;
        if !(period_s.is_finite() && period_s > 0.0) {
            return Err(invalid("period", "must be positive".into()));
        }
        // Strictly longer, as `drive_geometry::check_deadman` requires: a request is live while its age < timeout.
        if !(timeout_s.is_finite() && timeout_s > period_s) {
            return Err(invalid("deadman_timeout", format!("{timeout_s} s is not longer than one period ({period_s} s); every request would be lost")));
        }
        let on_loss = if immediate { OnLoss::Immediate } else { OnLoss::Ramp { decel: stop_decel } };
        Ok(Self { limits, deadman: Deadman { timeout_s, on_loss }, period_s })
    }

    /// One sample: the sequence and age bookkeeping and the shared step.
    /// `first` is the first sample (dt = 0). Returns the commanded twist,
    /// the sequence now held and the request age. A sequence that does not
    /// advance (including NaN) leaves the request ageing, so a broken
    /// heartbeat stops the robot.
    pub fn sample(&self, first: bool, previous: BodyTwist, request: BodyTwist, held_sequence: f64, age_s: f64, sequence: f64) -> (Result<BodyTwist, KinematicsError>, f64, f64) {
        let (held, age) = if sequence > held_sequence {
            (sequence, 0.0)
        } else {
            (held_sequence, (age_s + if first { 0.0 } else { self.period_s }).min(self.deadman.timeout_s))
        };
        let dt = if first { 0.0 } else { self.period_s };
        let twist = kinematics::step(previous, request, dt, age, &self.limits, &self.deadman).map(|c| c.twist);
        (twist, held, age)
    }
}

/// States: 0 samples taken, 1 held sequence, 2 request age, 3..6 commanded
/// twist. Outputs hold between samples (zero-rate states), updated in jumps
/// at scheduled sample times, as in `control.command_lease`.
///
/// Intended behaviour, kept on purpose:
/// - Startup is expired: the age starts at `deadman_timeout` and the held
///   sequence at 0 (the seam's initial heartbeat), so until a sequence above
///   0 arrives the stop rule applies to a zero twist and the output is zero.
/// - A refused request (outside the profile, unsupported axis, NaN) sets the
///   outputs to NaN, and NaN latches: the next sample's previous twist is
///   NaN, which the shared limit refuses, so the outputs stay NaN through
///   live requests and a `ramp` stop. Only an `on_loss_immediate` expiry
///   (zero without reading the previous twist) clears it. A run that sees
///   NaN has been told a request was refused; it is not clipped into motion.
impl Behavior for DriveLimiter {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![
            StateDeclaration::new("samples", QuantityKind::Dimensionless, 0.0),
            // Heartbeat 0 means no request yet (the seam's initial value).
            StateDeclaration::new("sequence", QuantityKind::Dimensionless, 0.0),
            StateDeclaration::new("age", QuantityKind::Time, self.deadman.timeout_s),
            StateDeclaration::new("forward", QuantityKind::LinearVelocity, 0.0),
            StateDeclaration::new("lateral", QuantityKind::LinearVelocity, 0.0),
            StateDeclaration::new("yaw", QuantityKind::AngularVelocity, 0.0),
        ]
    }
    fn residual(&self, ctx: &mut Context) {
        for i in 0..6 {
            ctx.set_state_residual(i, ctx.state_rate(i));
        }
        for i in 0..3 {
            ctx.set_signal(i, ctx.state(3 + i));
        }
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        out.push(view.state(0) * self.period_s - view.time);
    }
    fn scheduled_events(&self, view: &View, out: &mut Vec<(usize, f64)>) {
        out.push((0, view.state(0) * self.period_s));
    }
    fn jump(&mut self, _index: usize, view: &View, states: &mut [f64]) {
        let first = view.state(0) == 0.0;
        let previous = BodyTwist::new(view.state(3), view.state(4), view.state(5));
        let request = BodyTwist::new(view.signal_in(0), view.signal_in(1), view.signal_in(2));
        let (twist, held, age) = self.sample(first, previous, request, view.state(1), view.state(2), view.signal_in(3));
        // A refused request (outside the profile, NaN) is not clipped into
        // a motion: the outputs go NaN so the run reports it.
        let twist = twist.map(BodyTwist::to_array).unwrap_or([f64::NAN; 3]);
        states[1] = held;
        states[2] = age;
        states[3..6].copy_from_slice(&twist);
        states[0] = view.state(0) + 1.0;
    }
}

fn make_limiter(p: &BTreeMap<String, f64>) -> Result<Box<dyn Behavior>, EquationError> {
    Ok(Box::new(DriveLimiter::from_parameters(p)?))
}
