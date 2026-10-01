//! The catalogue's entries: RoboCAD's Robot menu tools and dialogs
//! (cad-physical-inspect; ui/app.py:408-419 and :1519-1674, the dialogs in
//! ui/widgets.py:1076-1523, the tools in ui/tools.py:1244-1361), then the
//! REST-only robot `Ops` methods. Labels, defaults, ranges, hints and
//! messages are RoboCAD's; a Pick field is RoboCAD's combo box over the
//! document (`robot_form::picks`), preset from the selection as its
//! handler presets it (`robot_form::seed`). The arguments are built by
//! `robot_args` (`Shape::Robot`). Not here: `set_joint_physics`,
//! `set_material`, `set_material_props` and `set_color` (the inspector's
//! and the materials panel's).
use super::super::kinds::*;
use super::super::robot_args::RobotCall;
use super::super::*;
use crate::ui_kit::form::Unit;

/// The joint dialog's fields (`JointDialog`, ui/widgets.py:1132-1231), in
/// its order. Limits are typed in degrees (millimetres for a prismatic
/// joint) and sent in radians (metres as typed); pivot and axis as typed.
const JOINT: &[Param] = &[
    p("type", "Type", pick("joint_types"), "revolute"),
    p("parent", "Parent", pick("bodies_or_world"), ""),
    p("child", "Child", pick("bodies"), ""),
    p("pivot", "Pivot (mm)", POINT, "0, 0, 0"),
    p("axis", "Axis", FieldKind::Vector { unit: Unit::Plain }, "0, 0, 1"),
    p("lower", "Lower limit (° or mm)", PLAIN, ""),
    p("upper", "Upper limit (° or mm)", PLAIN, ""),
    p("motor", "Motor", pick("motors_placed_or_none"), ""),
    p("gear_ratio", "Extra gear ratio", number(Unit::Plain, 0.01, 10000.0, 2), "1"),
    p("damping", "Damping (N·m·s/rad)", number(Unit::Plain, 0.0, 1000.0, 4), "0"),
    p("name", "Name", TEXT, ""),
];

/// A number with no unit and no range (RoboCAD's `evaluate(text)` of a line edit).
const PLAIN: FieldKind = FieldKind::Number { unit: Unit::Plain, min: None, max: None, decimals: 6 };

pub(in crate::cad::ops) const ENTRIES: &[OpEntry] = &[
    OpEntry {
        id: "robot.add_motor",
        label: "Robot: add motor from library…",
        category: "Robot",
        keys: &["Ctrl+Shift+M"],
        params: &[
            p("spec", "Motor", pick("motors"), ""),
            p("rotation", "Rotation about shaft (°)", number(Unit::Angle, -360.0, 360.0, 2), "0"),
            p("mount_on", "Mount on", pick("bodies_or_pick"), ""),
            p("cut", "Cut mounting holes and pilot into the mounted body", CHECK, "true"),
            p("name", "Name", TEXT, ""),
            p("point", "Mount point (mm; a face click sets it)", POINT, ""),
            p("shaft_dir", "Shaft direction (into the body; a face click sets it)", FieldKind::Vector { unit: Unit::Plain }, ""),
        ],
        flow: Flow::RobotPick(RobotTool::Motor),
        route: "add_motor",
        shape: Shape::Robot(RobotCall::AddMotor),
        hint: "Click a face to mount the motor there (housing outside, shaft into the body) • Esc to finish",
        source: "ui/app.py:408, ui/app.py:1526-1531 (MotorDialog, then MotorTool; the dialog's last motor, rotation and cut are remembered), ui/widgets.py:1076-1129 (MotorDialog: Motor from the library, Rotation about shaft -360..360°, Mount on \"(pick by clicking a face)\" or a body, the cut checkbox on, Name, the notes line), ui/tools.py:1244-1291 (MotorTool: one Ops.add_motor per face click, the tool stays active), commands.py:966; the point and shaft direction fields are ours (a click fills them)",
        ..BASE
    },
    OpEntry {
        id: "robot.add_joint",
        label: "Robot: add joint (click parent, child, axis face)",
        category: "Robot",
        keys: &["Ctrl+Shift+J"],
        flow: Flow::RobotPick(RobotTool::Joint),
        route: "add_joint",
        shape: Shape::Robot(RobotCall::JointTool),
        hint: "Click the parent body (Ctrl-click = world), then the child, then a cylindrical face or a flat face for the axis",
        source: "ui/app.py:409, ui/app.py:1533-1538 (JointTool, then the joint dialog preset with its picks), ui/tools.py:1294-1361; Ctrl+Shift+J is bound here (RoboCAD lists it, keymap.json never binds it: docs/cad-parity.md:1086)",
        ..BASE
    },
    OpEntry {
        id: "robot.joint_dialog",
        label: "Robot: joint from the two selected bodies…",
        category: "Robot",
        params: JOINT,
        flow: Flow::Form,
        route: "add_joint",
        shape: Shape::Robot(RobotCall::AddJoint),
        source: "ui/app.py:410, ui/app.py:1540-1563 (preset: the first two selected bodies as parent and child, one as the child; the active plane's origin and normal as pivot and axis; \"a joint needs a child body\"; Ops.add_joint, then Ops.set_joint(jid, damping=…) when the damping is not zero), ui/widgets.py:1132-1231 (JointDialog), commands.py:935 and :949",
        ..BASE
    },
    OpEntry {
        id: "robot.infer",
        label: "Robot: infer joints from coaxial holes and pins",
        category: "Robot",
        route: "infer_joints",
        shape: Shape::Robot(RobotCall::Infer),
        source: "ui/app.py:411, ui/app.py:1577-1581 (its two status texts), commands.py:1023",
        ..BASE
    },
    OpEntry {
        id: "robot.assign_motor",
        label: "Robot: assign selected motor to a joint…",
        category: "Robot",
        params: &[p("motor", "Motor", pick("motors_placed"), ""), p("joint", "Joint", pick("joints"), ""), p("gear_ratio", "Extra gear ratio", number(Unit::Plain, 0.01, 10000.0, 2), "1")],
        flow: Flow::Form,
        route: "attach_motor",
        shape: Shape::Robot(RobotCall::AssignMotor),
        source: "ui/app.py:412, ui/app.py:1583-1615 (\"Assign motor to joint\": Motor and Joint preset from the selection, Extra gear ratio 0.01..10000, 1; \"add a motor and a joint first\"), commands.py:992",
        ..BASE
    },
    OpEntry {
        id: "robot.fixed",
        label: "Robot: fix selected bodies together (first is the parent)",
        category: "Robot",
        needs: nodes(2, None, &["body"]),
        route: "connect_fixed",
        shape: Shape::Robot(RobotCall::Fixed),
        refusal: "select the parent body first, then the bodies to fix to it",
        source: "ui/app.py:413, ui/app.py:1617-1624 (the selected bodies; one Ops.connect_fixed per child, each its own undo step), commands.py:960",
        ..BASE
    },
    OpEntry {
        id: "robot.ground",
        label: "Robot: toggle ground on selected bodies",
        category: "Robot",
        needs: nodes(1, None, &["body"]),
        route: "set_ground",
        shape: Shape::Robot(RobotCall::Ground),
        refusal: "select the body that is fixed to the world",
        source: "ui/app.py:414, ui/app.py:1626-1633 (per selected body Ops.set_ground(id, not its robot.ground flag), each its own undo step), commands.py:1016",
        ..BASE
    },
    OpEntry {
        id: "robot.add_sensor",
        label: "Robot: add sensor (IMU, encoder, current, force)…",
        category: "Robot",
        params: &[
            p("kind", "Kind", pick("sensor_kinds"), "imu"),
            p("body", "On body", pick("bodies"), ""),
            p("point", "Point (mm)", POINT, "0, 0, 0"),
            p("joint", "Reads joint", pick("joints_or_none"), ""),
            p("rate_hz", "Rate (Hz)", number(Unit::Plain, 1.0, 100000.0, 2), "200"),
            p("name", "Name", TEXT, ""),
        ],
        flow: Flow::Form,
        route: "add_sensor",
        shape: Shape::Robot(RobotCall::Sensor),
        source: "ui/app.py:417, ui/app.py:1649-1657 (the body preset from the first selected body; Ops.add_sensor(kind, body, point, None, name, joint, rate_hz=…)), ui/widgets.py:1369-1407 (SensorDialog), commands.py:1047",
        ..BASE
    },
    OpEntry {
        id: "robot.add_cable",
        label: "Robot: add cable between bodies…",
        category: "Robot",
        params: &[
            p("from_body", "From body", pick("bodies"), ""),
            p("from_point", "From point (mm)", POINT, "0, 0, 0"),
            p("to_body", "To body", pick("bodies"), ""),
            p("to_point", "To point (mm)", POINT, "0, 0, 0"),
            p("length", "Length (mm; empty: auto: 10 % slack)", LENGTH, ""),
            p("mass", "Mass (g; empty: auto: 4 g per 100 mm)", PLAIN, ""),
            p("name", "Name", TEXT, ""),
        ],
        flow: Flow::Form,
        route: "add_cable",
        shape: Shape::Robot(RobotCall::Cable),
        source: "ui/app.py:418, ui/app.py:1659-1667 (from and to preset from the first two selected bodies; Ops.add_cable(from, from_point, to, to_point, length mm, mass kg, None, name)), ui/widgets.py:1410-1457 (CableDialog: mass typed in g, sent in kg), commands.py:1062",
        ..BASE
    },
    OpEntry {
        id: "robot.power",
        label: "Robot: battery, control loop and uncertainty…",
        category: "Robot",
        params: &[
            p("cells", "Battery cells (series; 0: none (motor supply voltage))", number(Unit::Count, 0.0, 24.0, 0), "0"),
            p("chemistry", "Chemistry", FieldKind::Choice { options: &["lipo", "liion", "lifepo4", "nimh", "alkaline"] }, "lipo"),
            p("capacity_ah", "Capacity (Ah)", number(Unit::Plain, 0.01, 100.0, 2), "1"),
            p("period_s", "Control period (s)", number(Unit::Plain, 0.0001, 1.0, 4), "0.02"),
            p("latency_s", "Control latency (s)", number(Unit::Plain, 0.0, 1.0, 4), "0.004"),
            p("targets", "Target per joint (°): {\"joint name\": degrees}", JSON, "{}"),
            p("dimension", "Dimension σ (mm)", number(Unit::Length, 0.0, 2.0, 3), "0.15"),
            p("friction", "Friction σ (fraction)", number(Unit::Plain, 0.0, 1.0, 2), "0.2"),
        ],
        flow: Flow::Form,
        route: "set_battery",
        shape: Shape::Robot(RobotCall::Power),
        source: "ui/app.py:419, ui/app.py:1669-1674, ui/widgets.py:1460-1523 (PowerDialog: defaults from the document's settings over physical.py:686 default_settings; apply: Ops.set_battery(cells, chemistry, capacity_ah) or Ops.set_robot_setting(\"battery\", None) with no cells, Ops.set_control(period_s, latency_s, targets in rad by joint name), Ops.set_uncertainty(dimension_m, friction), each its own undo step), commands.py:1090, :1145, :1151, :1162; the targets as one JSON field of the revolute, continuous and prismatic joints is ours (RoboCAD shows one line per joint)",
        ..BASE
    },
    // ---- REST-only robot Ops methods (no RoboCAD command) ------------------------
    OpEntry {
        id: "ops.set_joint",
        label: "Edit joint",
        category: "Robot",
        needs: nodes(1, Some(1), &["joint"]),
        params: JOINT,
        flow: Flow::Form,
        route: "set_joint",
        shape: Shape::Robot(RobotCall::EditJoint),
        refusal: "Select the joint to edit",
        source: "ui/app.py:1565-1575 (robot_edit_joint: JointDialog titled \"Edit joint\", preset from the joint; Ops.set_joint(jid, **values), then Ops.rename(jid, name) when the name changed), opened by the Robot panel's double-click; commands.py:949 and :333; refusal ours",
        ..BASE
    },
    OpEntry {
        id: "ops.mount_motor",
        label: "Mount motor on a body",
        category: "Robot",
        params: &[p("motor", "Motor", pick("motors_placed"), ""), p("body", "Body", pick("bodies_or_none"), "")],
        flow: Flow::Form,
        route: "mount_motor",
        shape: Shape::Robot(RobotCall::MountMotor),
        source: "commands.py:983 (mount_motor(motor_id, body_id or None)); labels ours",
        ..BASE
    },
    OpEntry {
        id: "ops.configure_robot",
        label: "Configure robot (assembly metadata and connectors)",
        category: "Robot",
        params: &[p("updates", "updates", JSON, ""), p("joints", "joints", JSON, ""), p("groups", "groups", JSON, ""), p("moves", "moves", JSON, "")],
        flow: Flow::Form,
        route: "configure_robot",
        shape: Shape::Robot(RobotCall::ConfigureRobot),
        args: &[Arg::Revision],
        source: "commands.py:257 (configure_robot(expected_revision, updates, joints, groups, moves): one undo step; expected_revision is the revision the caller read the document at: a form sends the one it opened at), assembly.py; labels ours",
        ..BASE
    },
    OpEntry {
        id: "ops.set_robot_setting",
        label: "Set robot setting",
        category: "Robot",
        params: &[p("key", "key (battery, control, uncertainty, world, identification)", TEXT, ""), p("value", "value (JSON; null removes it)", JSON, "")],
        flow: Flow::Form,
        route: "set_robot_setting",
        args: &[Arg::Param("key"), Arg::Param("value")],
        source: "commands.py:1090 (set_robot_setting(key, value): one undo step \"Robot setting\"); labels ours",
        ..BASE
    },
];
