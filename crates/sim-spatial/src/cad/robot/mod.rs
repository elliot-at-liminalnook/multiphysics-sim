//! The robot side of CAD mode (cad-physical-inspect; native-viewer.md "CAD
//! physical properties"): RoboCAD's Robot dock, its robot tools and
//! dialogs, and the joint, motor, sensor and cable glyphs, over RoboCAD's
//! `GET /robot` and the reads beside it.
//!
//! - [`data`]: what RoboCAD says about the robot (the summary with joints,
//!   motors and issues; per-node results and margins; sensors; cables;
//!   battery, control loop and uncertainty; actuator profiles; the motor
//!   library), read on one Dedicated job per (document generation, shown
//!   revision) and kept on the document ([`RobotState::data`]). Every part
//!   reads it; nothing here is computed from geometry or physics.
//! - [`panel`]: the Robot dock as a section of the right dock: the summary
//!   line, the Links / Joints / Motors / Sensors & cables tree with Detail
//!   and Margin columns, the issues and RoboCAD's panel buttons. A row click
//!   is `CadSelect` (the tree's own path into the shared selection); a
//!   double-click on a joint opens the Edit joint form (`ops.set_joint`).
//! - [`tools`]: the motor and joint click tools, "Robot: validate" and the
//!   motor library. The dialogs are op-catalogue forms
//!   (`ops::catalogue::robot`).
//! - [`glyphs`]: joint glyphs, motor shaft axes, sensor triads and sagging
//!   cable arcs, drawn in Present from [`data`], display only.
//!
//! Every edit is one RoboCAD call through `actions::edit_at` (refused by
//! name while an edit is in flight or when the values were read at another
//! revision), so RoboCAD's undo and provenance stay its own.
pub(crate) mod data;
mod glyphs;
pub(crate) mod panel;
mod tools;

use crate::app::actions::{Call, Spec, spec};
use crate::app::{ViewerMode, ViewerSet};
use super::actions::{CAD, CadAction, Cx};
use super::document::CadDocument;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::SelectionItem;

/// The robot parts' state on the document (reset with it).
#[derive(Default)]
pub struct RobotState {
    /// RoboCAD's robot description and the reads beside it (`data`).
    pub data: data::RobotData,
    /// The Robot dock's display state (`panel`).
    pub panel: panel::PanelState,
    /// The click tools' picks, the motor library and validation (`tools`).
    pub tools: tools::ToolsState,
}

/// What `cad_robot` does.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RobotOp {
    /// Show (`open: true`), hide (`false`) or toggle the Robot dock's content.
    #[default]
    Panel,
    /// Read RoboCAD's robot description again now (`data`).
    Refresh,
    /// RoboCAD's "Robot: validate": the description at the current revision,
    /// "robot valid: …" or the issues (`tools`).
    Validate,
    /// RoboCAD's "Robot: motor library…": show, hide or toggle the list
    /// (`GET /motors`, `tools`).
    Library,
    /// One pick of the active click tool (`robot.add_motor`'s face,
    /// `robot.add_joint`'s parent, child and axis face): `item` as the 3D
    /// pick made it, `world` for the joint tool's Ctrl-click (the world as
    /// the parent), `picked_at` the shown revision it was made at.
    Pick,
}

/// `cad_robot`'s arguments.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct RobotArgs {
    #[serde(default)]
    pub op: RobotOp,
    /// Shown or hidden (panel, library; absent toggles).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
    /// The picked item (pick).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<SelectionItem>,
    /// The joint tool's Ctrl-click: the world is the parent (pick).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub world: bool,
    /// The shown revision the pick was made at (pick; required from REST
    /// for a face item, as `cad_run`'s `revision`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picked_at: Option<u64>,
}

impl RobotArgs {
    /// `cad_robot` with `op` and `open`.
    pub(crate) fn of(op: RobotOp, open: Option<bool>) -> CadAction {
        CadAction::CadRobot(RobotArgs { op, open, ..RobotArgs::default() })
    }
}

/// The action a RoboCAD command id stands for when it is not a catalogue
/// operation (`surfaces::registry`'s `Do::Physical`): validate and the
/// motor library here; results, identification, the stress overlay,
/// physical export and the live link in `results`.
pub(crate) fn command_action(id: &str) -> Option<CadAction> {
    match id {
        "robot.validate" => Some(RobotArgs::of(RobotOp::Validate, None)),
        // Shown, never hidden, as RoboCAD's dialog opens.
        "robot.motors" => Some(RobotArgs::of(RobotOp::Library, Some(true))),
        _ => super::results::command_action(id),
    }
}

/// `CadRobot`, from any entry point.
pub(in crate::cad) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    let CadAction::CadRobot(args) = action else { return Outcome::Done(Err("not a robot action".into())) };
    match args.op {
        RobotOp::Panel => panel::handle(args, call, cx),
        RobotOp::Refresh => {
            cx.doc.robot.data.invalidate();
            cx.doc.touch();
            Outcome::Done(Ok(json!({"message": "Reading RoboCAD's robot description again (GET /robot and the reads beside it); it shows in cad_state.robot."})))
        }
        RobotOp::Validate | RobotOp::Library | RobotOp::Pick => tools::handle(args, call, cx),
    }
}

/// `cad_state.robot`.
pub(in crate::cad) fn state_json(doc: &CadDocument) -> Value {
    let mut out = data::state_json(doc);
    out["panel"] = panel::state_json(doc);
    out["tools"] = tools::state_json(doc);
    out
}

/// The robot parts' `system_ui` controls: the panel's (`cad:robot:*` rows,
/// buttons) and the tools' (validate, the motor library).
pub(in crate::cad) fn controls(cx: &Cx) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let selection = cx.shared.items();
    let mut out = panel::controls(cx.doc, &selection);
    out.extend(tools::controls(cx.doc));
    out
}

/// The robot parts' REST commands.
pub(in crate::cad) fn specs() -> Vec<Spec> {
    vec![spec(
        "cad_robot",
        CAD,
        json!({"op": "validate"}),
        "CAD mode: RoboCAD's robot description (GET /robot: joints, motors, links, DoF, ground, issues; with GET /results/nodes margins, GET /sensors, GET /cables, GET /battery, GET /control, GET /uncertainty, GET /actuator-profiles and GET /motors, read on a job at each revision; cad_state.robot). op: panel (open true | false, absent toggles the Robot dock's content), refresh (read it again now), validate (RoboCAD's \"Robot: validate\": \"robot valid: …\" or the issues), library (open true | false: RoboCAD's motor library from GET /motors), pick (the active robot click tool's pick: item [node, kind, index] as a 3D pick makes it, world true for the joint tool's Ctrl-click, picked_at the revision a face item was read at). The dialogs are catalogue forms: cad_invoke robot.add_motor, robot.add_joint, robot.joint_dialog, robot.infer, robot.assign_motor, robot.fixed, robot.ground, robot.add_sensor, robot.add_cable, robot.power, ops.set_joint (Edit joint). Every edit is one RoboCAD call; refused by name while one is in flight. system_ui lists cad:robot:<id>.",
    )]
}

/// CadCorePlugin's windowless robot systems: the description's reads.
pub(crate) struct CoreParts;
impl Plugin for CoreParts {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, data::sync.after(crate::cad::CadSet::Results).in_set(ViewerSet::JobResults).run_if(in_state(ViewerMode::Cad)));
    }
}

/// CadPlugin: the panel's input, the click tools and the glyphs.
pub(in crate::cad) fn build(app: &mut App) {
    panel::build(app);
    tools::build(app);
    glyphs::build(app);
}
