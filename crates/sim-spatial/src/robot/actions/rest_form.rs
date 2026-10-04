//! Robot mode's REST surface besides the commands table: the published
//! resources (`robot_state`, `robot_guide`) and the REST form of
//! [`RobotAction`] (`wire`).
use super::*;
/// Present: `/v1/robot_state` (with `cad_threads`), at most every 100 ms.
#[allow(clippy::too_many_arguments)]
pub(in crate::robot) fn publish(
    rest: Option<ResMut<crate::rest::Rest>>,
    view: Res<RobotView>,
    selection: Res<Selection>,
    registry: Res<DocumentRegistry>,
    threads: Res<crate::robot::threads::RobotThreads>,
    bindings: Option<Res<crate::drive_input::DriveBindings>>,
    drive_input: Option<Res<crate::drive_input::DriveInput>>,
    board: Res<crate::robot::leaderboard::Leaderboard>,
    video: Res<crate::robot::video::VideoRecorder>,
) {
    let Some(mut rest) = rest else { return };
    if rest.0.snapshot_due() {
        let cad_threads = crate::robot::threads::state_json(&view, &threads);
        let mut state = view.state_with_threads(picked::link(&selection, &registry), cad_threads);
        state["leaderboard"] = crate::robot::leaderboard::json(&board, false);
        state["video"] = video.json();
        rest.0.publish("robot_state", view.with_drive_input(state, bindings.as_deref(), drive_input.as_deref()));
    }
    // The mode's guide, for an agent starting cold (`GET /v1/robot_guide`).
    rest.0.publish_changed("robot_guide", "1", || crate::robot::guide::guide(None).unwrap_or_default());
}

/// The REST form of robot mode's commands: `RobotAction` deserializes
/// through it, so each command keeps its JSON shape (fields, tags, unknown
/// fields refused) and its argument errors. It carries no intent of its own.
pub(crate) mod wire {
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
    pub(crate) enum Ui {
        Controls,
        Activate { id: String, ui_revision: u64 },
    }
    #[derive(Deserialize)]
    #[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
    pub(crate) enum Command {
        State,
        RobotState,
        SystemUi { action: Ui },
        Camera { focus: [f32; 3], radius: f32, yaw: f32, pitch: f32 },
        Fit,
        RobotRun { action: String },
        RobotJog { joint: String, target: Option<f64>, delta: Option<f64> },
        RobotPresets,
        RobotGuide { topic: Option<String> },
        RobotPreset { id: String },
        RobotInput { channels: Option<std::collections::BTreeMap<String, f64>>, key: Option<String> },
        RobotInputs { values: std::collections::BTreeMap<String, f64> },
        RobotDrive { forward: Option<f64>, lateral: Option<f64>, yaw: Option<f64>, action: Option<String>, stop: Option<bool> },
        RobotSaveRecording { path: Option<String>, note: Option<String> },
        RobotReplay { file: Option<String>, path: Option<String>, action: Option<String> },
        RobotGait { action: Option<String>, report: Option<String>, path: Option<String>, t: Option<f64>, scale: Option<f64> },
        RobotReload,
        RobotOverlay { contacts: Option<bool>, joints: Option<bool>, deflections: Option<bool>, stress: Option<bool> },
        RobotSpeed { action: Option<String>, scale: Option<f64> },
        RobotView { fit_selected: Option<bool>, follow: Option<bool>, display_hz: Option<u32> },
        RobotVideo { on: Option<bool> },
        RobotLeaderboard { op: String, id: Option<String>, text: Option<String>, status: Option<crate::robot::leaderboard::StatusFilter>, group: Option<String>, on: Option<bool> },
        RobotHistory { t: Option<f64>, live: Option<bool> },
        RobotGraphs { pick: Option<String>, remove: Option<String>, visible: Option<bool> },
        RobotRecorded { action: String, t: Option<f64>, scale: Option<f64>, delta: Option<i64> },
        RobotThreads { op: Option<String>, thread: Option<String>, comment: Option<String>, body: Option<String>, author: Option<String>, resolved: Option<bool> },
    }
}
