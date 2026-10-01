//! Typed wrappers of RoboCAD's robotics `Ops` methods (`commands.py`),
//! each one `POST /ops/{name}` through [`CadClient::op`]: positional
//! arguments in the Python signature's order, keywords only where the
//! Python takes `**fields`. `None` is sent as `null`, which RoboCAD's
//! argument converter passes on as Python's `None` (so `connect_fixed`
//! without `at` uses the child's centroid, a `None` name RoboCAD's
//! default). Each is one RoboCAD undo step (`configure_robot` one
//! composite): call them through a client with [`super::EDIT_TIMEOUT`].
//! RoboCAD answers 422 with the `KernelError` text for a refused edit (an
//! unknown motor, a joint type, a body that is not a body), 500 with the
//! Python error for an unknown node id (`KeyError: '…'`).
use super::{CadClient, CadError, OpResult};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// `Ops.add_joint`'s arguments. Pivot mm; axis a direction; limits rad (mm
/// for prismatic); `motor` a motor body id; `gear_ratio` the extra
/// reduction between the motor output and the joint.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AddJoint {
    /// revolute | continuous | prismatic | fixed | ball | loop_revolute | loop_spherical
    pub kind: String,
    /// The parent body; `None` is the world.
    pub parent: Option<String>,
    pub child: String,
    pub pivot: [f64; 3],
    pub axis: [f64; 3],
    pub lower: Option<f64>,
    pub upper: Option<f64>,
    pub motor: Option<String>,
    pub gear_ratio: f64,
    /// `None`: RoboCAD names it "{type} {child name}".
    pub name: Option<String>,
}

impl Default for AddJoint {
    /// `add_joint`'s defaults: axis +Z, gear ratio 1 (a revolute joint).
    fn default() -> Self {
        AddJoint { kind: "revolute".into(), parent: None, child: String::new(), pivot: [0.0; 3], axis: [0.0, 0.0, 1.0], lower: None, upper: None, motor: None, gear_ratio: 1.0, name: None }
    }
}

/// `Ops.add_motor`'s arguments: a library motor ([`super::MotorSpec`]
/// id) placed with its mounting face at `mount_point` (mm), the shaft
/// along `shaft_dir`; `mount_on` the body it rides on, `cut_mount` cuts
/// its mounting holes into that body.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AddMotor {
    pub spec_id: String,
    pub mount_point: [f64; 3],
    pub shaft_dir: [f64; 3],
    pub rotation_deg: f64,
    pub mount_on: Option<String>,
    pub cut_mount: bool,
    /// `None`: the spec's name.
    pub name: Option<String>,
}

impl Default for AddMotor {
    /// `add_motor`'s defaults: no rotation, not mounted, no holes cut.
    fn default() -> Self {
        AddMotor { spec_id: String::new(), mount_point: [0.0; 3], shaft_dir: [0.0, 0.0, 1.0], rotation_deg: 0.0, mount_on: None, cut_mount: false, name: None }
    }
}

impl CadClient {
    /// `add_joint(type, parent, child, pivot, axis, lower, upper, motor,
    /// gear_ratio, name)`: the new joint node's id in `result`.
    pub fn add_joint(&self, joint: &AddJoint) -> Result<OpResult, CadError> {
        let j = joint;
        self.op("add_joint", &[json!(j.kind), json!(j.parent), json!(j.child), json!(j.pivot), json!(j.axis), json!(j.lower), json!(j.upper), json!(j.motor), json!(j.gear_ratio), json!(j.name)], &Map::new())
    }
    /// `set_joint(joint_id, **fields)`: `fields` replace those keys of the
    /// joint (`Joint.to_json`'s: type, parent, child, pivot, axis, lower,
    /// upper, motor, gear_ratio, damping, friction, home, stroke; any other
    /// key is ignored by RoboCAD). Undo step "Edit joint".
    pub fn set_joint(&self, joint_id: &str, fields: &Map<String, Value>) -> Result<OpResult, CadError> {
        self.op("set_joint", &[json!(joint_id)], fields)
    }
    /// `rename(node_id, name)`. Undo step "Rename".
    pub fn rename(&self, node_id: &str, name: &str) -> Result<OpResult, CadError> {
        self.op("rename", &[json!(node_id), json!(name)], &Map::new())
    }
    /// `connect_fixed(parent, child, at, name)`: a fixed joint (the two
    /// move as one link); `at` mm, `None` the child's centroid.
    pub fn connect_fixed(&self, parent: &str, child: &str, at: Option<[f64; 3]>, name: Option<&str>) -> Result<OpResult, CadError> {
        self.op("connect_fixed", &[json!(parent), json!(child), json!(at), json!(name)], &Map::new())
    }
    /// `add_motor(spec_id, mount_point, shaft_dir, rotation_deg, mount_on,
    /// cut_mount, name)`: the motor body's id. An unknown spec is a 422
    /// "unknown motor {id}; see the library". Undo step "Add motor".
    pub fn add_motor(&self, motor: &AddMotor) -> Result<OpResult, CadError> {
        let m = motor;
        self.op("add_motor", &[json!(m.spec_id), json!(m.mount_point), json!(m.shaft_dir), json!(m.rotation_deg), json!(m.mount_on), json!(m.cut_mount), json!(m.name)], &Map::new())
    }
    /// `mount_motor(motor_id, body_id)`: the body its housing rides on
    /// (`None` unmounts). Undo step "Mount motor".
    pub fn mount_motor(&self, motor_id: &str, body_id: Option<&str>) -> Result<OpResult, CadError> {
        self.op("mount_motor", &[json!(motor_id), json!(body_id)], &Map::new())
    }
    /// `attach_motor(joint_id, motor_id, gear_ratio)`: the motor drives the
    /// joint (`None` detaches it); an unmounted motor is mounted on the
    /// joint's parent. Undo step "Attach motor".
    pub fn attach_motor(&self, joint_id: &str, motor_id: Option<&str>, gear_ratio: f64) -> Result<OpResult, CadError> {
        self.op("attach_motor", &[json!(joint_id), json!(motor_id), json!(gear_ratio)], &Map::new())
    }
    /// `set_ground(body_id, ground)`. Undo step "Ground".
    pub fn set_ground(&self, body_id: &str, ground: bool) -> Result<OpResult, CadError> {
        self.op("set_ground", &[json!(body_id), json!(ground)], &Map::new())
    }
    /// `infer_joints()`: revolute joints at coaxial shaft/bore pairs not
    /// already jointed; the new ids (one undo step "Infer joints", none
    /// when nothing was found).
    pub fn infer_joints(&self) -> Result<OpResult, CadError> {
        self.op("infer_joints", &[], &Map::new())
    }
    /// `set_robot_setting(key, value)`: a document-level robot setting
    /// (battery, control, uncertainty, world, identification, …) replaced
    /// whole; `result` is the whole `robot_settings`. Undo step "Robot setting".
    pub fn set_robot_setting(&self, key: &str, value: &Value) -> Result<OpResult, CadError> {
        self.op("set_robot_setting", &[json!(key), value.clone()], &Map::new())
    }
    /// `configure_robot(expected_revision, updates, joints, groups,
    /// moves)`: validated assembly metadata and connectors as one undo
    /// step; refused with 409 when RoboCAD's revision is no longer
    /// `expected_revision`.
    pub fn configure_robot(&self, expected_revision: u64, updates: Option<&Value>, joints: Option<&Value>, groups: Option<&Value>, moves: Option<&Value>) -> Result<OpResult, CadError> {
        self.op("configure_robot", &[json!(expected_revision), json!(updates), json!(joints), json!(groups), json!(moves)], &Map::new())
    }
}
