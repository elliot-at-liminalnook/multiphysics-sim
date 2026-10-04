//! `robot_guide` (and `GET /v1/robot_guide`): how robot mode works, for an
//! agent starting cold. The concepts (documents, the run thread, sessions,
//! frames and history), the workflows in order, every command with a
//! working example, and the rules. Written from the code it describes; the
//! command list mirrors `GET /v1/capabilities` (where each command's full
//! description, refusals and state fields are).
use serde_json::{Value, json};

/// The guide (`topic` narrows it to one section).
pub(crate) fn guide(topic: Option<&str>) -> Result<Value, String> {
    let all = json!({
        "about": "Robot mode inspects and runs one robot inside the viewer process (Rust; no server). Its document is a physical export (*.simrobot.json, from CAD) or a declared preset from web/viewer/presets.json (embedded, recorded, live or drive), or a tested controller recipe from the leaderboard. Physics runs on its own run thread on simulation time, paced to speed_scale × real time; drawing never steps physics.",
        "how_to_call": {
            "batch": "POST /v1/batch {\"commands\":[{\"command\":NAME,\"args\":{...}}]} → {\"job_id\"}; GET /v1/jobs/{job_id} until status is succeeded | failed | cancelled; results[i].ok, results[i].value or results[i].error. Commands in one batch run in order.",
            "resources": "GET /v1/robot_state (everything shown: model, run, readouts, inputs, graphs, history, overlays, recordings, leaderboard, video, comments), /v1/robot_guide (this), /v1/capabilities (every command with its full description and refusals).",
            "entering": "viewer_mode {mode: robot, path: /abs/x.simrobot.json} or {mode: robot, preset: ID}; once in robot mode, robot_preset {id} opens another preset in place.",
            "ui_parity": "Every button is also a system_ui control: system_ui {action: {operation: controls}} lists them with ui_revision; {operation: activate, id, ui_revision} presses one through the same handler as a click.",
        },
        "concepts": {
            "documents": {
                "robot_file": "A *.simrobot.json export (physical v3+, or planar v2): links with mass/inertia/material provenance, joints, motors, transmissions, battery, actuator profiles, its CAD source (cad_link). Reloaded on change (robot_reload). Runs the shared PhysicalRobot; jog sets servo targets; overlays show contacts, joint frames, deflections and the stress results file.",
                "embedded": "A preset with a scene and config (and optionally a task): runs sim_runtime's EmbeddedSession or EmbeddedEnvironment (with a task: rewards, observations, termination). Typed inputs (robot_inputs), motion requests (robot_input / W A S D), recordings and replays.",
                "live": "A preset whose scene carries its own controller (e.g. pendulum-live): runs sim_runtime::session::Session one held action per controller period, as the browser worker did.",
                "drive": "A preset that opens a drive model with its controller binding (e.g. rover-drive): robot_drive / keyboard / gamepad requests through the drive profile, with the deadman on simulated time.",
                "recorded": "A preset that plays back a captured run (no physics here): robot_recorded seeks and plays.",
                "tested": "tested-<id>: a controller recipe from the leaderboard catalog, its sources checked against their sha256 before it runs or replays.",
            },
            "run": "robot_state.run: phase idle | building | running | paused | ended | failed, time (simulated s), steps, rtf (achieved real-time factor), speed_scale (requested; powers of two 0.125–8), compute_limited, generation (moves on every reset/reload; frames of an older generation are never shown).",
            "frames_and_history": "The run thread publishes frames (poses, inputs, readouts). The window keeps the last 30 s (robot_state.history); robot_history {t} shows a kept frame (pausing a running run) and the graph dock's cursor follows it; {live: true} returns to the live frame.",
            "readouts": "robot_state.readouts: the Run section's numbers (execution, performance, contacts, travel and walking speed, learning progress, motion progress, world load, neural residuals, joint readings, task observations, actuation profile, scheduled pushes). A block the run does not publish is null with the reason; nothing is estimated to fill it.",
            "inputs": "robot_state.inputs: the built session's typed input channels (name, kind, bounds, initial, held). Values are refused outside their bounds, never clamped. The motion heartbeat is the controller's packet counter and is not set by hand.",
            "graphs": "The graph dock (key G): fixed charts plus a Picked chart of up to 8 channels from robot_state.graphs.candidates (robot_graphs {pick}).",
            "sections": "The inspector's sections: run (live readouts, inputs, timeline), link (the selected link's physical properties), joints, drives, source (the export's provenance), comments (the CAD source's comment threads, read and written in the .rcad in process).",
            "selection": "One selection shared by the link list, the 3D view and robot_state.selected (system_ui link:<index>).",
            "units_and_frames": "SI throughout (m, kg, s, rad, N). The 3D view's display frame is Y up; the model frame is the export's. camera {focus, radius, yaw, pitch} sets the orbit; fit frames the robot; robot_view {fit_selected: true} frames the selected link.",
            "fidelity": "Results carry their fidelity: an uncalibrated model or provisional limit says so in robot_state (provenance, uncertainty, actuator profile status). A viewer run is not a hardware result.",
        },
        "workflows": [
            {"goal": "Look before acting", "steps": ["GET /v1/robot_state: format, links, joints, run, readouts, inputs", "system_ui {action: {operation: controls}} for what can be pressed now", "robot_presets to see what can be opened (openable and why not)"]},
            {"goal": "Run a preset and watch it", "steps": ["robot_preset {id: pendulum-embedded} (or viewer_mode {mode: robot, preset: ...})", "robot_run {action: start}; robot_speed {scale: 4} for faster than real time", "poll robot_state.run (time, rtf, compute_limited) and robot_state.readouts", "robot_run {action: pause}; robot_run {action: reset} to rebuild at t = 0"]},
            {"goal": "Command a running preset", "steps": ["robot_state.inputs lists the channels and bounds", "robot_inputs {values: {NAME: VALUE}} sets typed inputs (the Inputs block's sliders)", "robot_input {channels: {...}} or {key: w | a | s | d | stop} for motion requests (W A S D)", "a drive preset or a --robot FILE with a controller binding: robot_drive {forward, lateral, yaw} (repeat faster than robot_state.drive's deadman timeout)"]},
            {"goal": "Measure walking or realtime performance", "steps": ["open the preset, robot_run {action: start} at robot_speed {scale: 1}", "after a few seconds read robot_state.run.rtf and compute_limited, and robot_state.readouts.travel / walking (body speed from the published pose; null when the run publishes no base pose)", "repeat at robot_speed {scale: 8} to see the compute ceiling (compute_limited true means the run could not keep up)"]},
            {"goal": "Review what happened", "steps": ["robot_run {action: pause}", "robot_history {t: SECONDS} (within robot_state.history.span) shows that frame; each trace in robot_state.graphs.charts carries at_cursor, its value there", "robot_graphs {pick: KEY} adds a channel to the Picked chart", "robot_history {live: true} returns to the live frame"]},
            {"goal": "Record and replay", "steps": ["robot_save_recording {note} writes the session's recording under runs/robot-presets/<id>/ (robot_state.recording.last_saved)", "robot_replay {file} replays it through the runtime's own replay and reports the verdict (matches or the first difference)", "robot_video {on: true} … {on: false} writes an MP4 of the 3D view under runs/robot-video/ (drawn frames, not a physics record)"]},
            {"goal": "Compare controllers", "steps": ["robot_leaderboard {op: list} (filters: search, status, profile)", "robot_leaderboard {op: inspect, id} for one evidence card; select + compare for several", "robot_leaderboard {op: run, id} loads the tested recipe and runs it with its evaluated step-0 inputs; {op: replay, id} replays its evaluated input events"]},
            {"goal": "Inspect a robot file from CAD", "steps": ["viewer_mode {mode: robot, path: /abs/robot.simrobot.json}", "robot_state.cad_link says whether the export is current with its .rcad", "system_ui link:<index> then robot_state.selected (mass, com, inertia, material and density provenance)", "robot_overlay {contacts, joints, deflections, stress}", "robot_jog {joint, target} moves one servo target (refused outside the file's limits, never clamped)"]},
            {"goal": "Discuss with the person", "steps": ["robot_threads {op: state} lists the CAD source's comment threads with the link each is on", "robot_threads {op: reply, thread, body} writes a reply into the .rcad (refused if the file changed since it was read)", "robot_threads {op: open_in_cad, thread} switches to CAD mode on that thread"]},
            {"goal": "Preview a gait", "steps": ["robot_state.gait_preview.reports lists tracked gait reports", "robot_gait {report: NAME} then {action: play}; kinematic only: nothing is simulated or sent to hardware"]},
        ],
        "commands": {
            "robot_state": {"example": {}, "does": "Everything robot mode shows (read-only)."},
            "robot_guide": {"example": {"topic": "workflows"}, "does": "This guide; topic narrows it."},
            "system_ui": {"example": {"action": {"operation": "controls"}}, "does": "List or activate the window's controls by id (the same handler as a click)."},
            "robot_presets": {"example": {}, "does": "The declared presets with mode, paths and whether each opens."},
            "robot_preset": {"example": {"id": "pendulum-live"}, "does": "Open a preset in place of the current robot."},
            "robot_run": {"example": {"action": "start"}, "actions": "start, pause, step, reset"},
            "robot_speed": {"example": {"scale": 4}, "does": "Pacing (× real time): 0.125, 0.25, 0.5, 1, 2, 4, 8; or action up | down."},
            "robot_inputs": {"example": {"values": {"position.target": 0.4}}, "does": "Typed session inputs of a preset (bounds enforced)."},
            "robot_input": {"example": {"key": "w"}, "does": "Motion requests (channels or a W A S D key; stop)."},
            "robot_drive": {"example": {"forward": 0.5, "lateral": 0, "yaw": 0}, "does": "Drive requests through a drive profile (or action, or stop: true)."},
            "robot_history": {"example": {"t": 2.5}, "does": "Show a kept frame (or live: true)."},
            "robot_graphs": {"example": {"pick": "learning:reward"}, "does": "Pick or remove a channel on the Picked chart; visible toggles the dock."},
            "robot_view": {"example": {"follow": true, "display_hz": 30}, "does": "fit_selected, follow the robot, cap the display rate (0 = uncapped, 15, 30, 60)."},
            "robot_overlay": {"example": {"contacts": true, "stress": false}, "does": "Overlays: contacts (presets too), joints, deflections, stress (robot files)."},
            "robot_jog": {"example": {"joint": "left axle", "target": 0.5}, "does": "Servo-target jog of a --robot FILE joint (target or delta)."},
            "robot_recorded": {"example": {"action": "seek", "t": 0.8}, "does": "A recorded preset's timeline: play, pause, start, seek, step, speed."},
            "robot_save_recording": {"example": {"note": "after a push"}, "does": "Write the run's recording (a writer thread; robot_state.recording.last_saved)."},
            "robot_replay": {"example": {"file": "20260930T060822.729Z.json"}, "does": "Replay a saved recording (or action cancel | list)."},
            "robot_leaderboard": {"example": {"op": "list"}, "ops": "list, open, close, search, status, profile, select, compare, inspect, run, replay, download"},
            "robot_video": {"example": {"on": true}, "does": "Record the 3D view to an MP4 under runs/robot-video/ (on: false writes it)."},
            "robot_gait": {"example": {"report": "REPORT"}, "does": "Kinematic gait preview: open, play, pause, stop, seek, speed, list."},
            "robot_reload": {"example": {}, "does": "Re-read the --robot FILE now (it is also watched)."},
            "robot_threads": {"example": {"op": "reply", "thread": "THREAD", "body": "Checked in the simulator: …"}, "ops": "state, refresh, open, reply, edit, delete, resolve, open_in_cad"},
            "camera": {"example": {"focus": [0, 0, 0], "radius": 0.5, "yaw": 0.7, "pitch": 0.4}, "does": "Set the orbit camera."},
            "fit": {"example": {}, "does": "Frame the whole robot."},
        },
        "rules": [
            "Read robot_state before acting and after: refusals name their reason (a recorded preset, no built session, a value outside bounds, a replay holding the run); fix the cause, do not retry blindly.",
            "Bounds and limits are refused, never clamped; never edit a preset or export to get past a refusal.",
            "Physics advances on simulated time on the run thread; rtf and compute_limited say whether it kept up. A display cap or video recording never changes the physics.",
            "Label results with their fidelity: an uncalibrated model or a provisional limit is not a hardware result.",
            "Recordings, videos and leaderboard files go under runs/ (ignored by git); keep what reproduces them (the preset, seed and inputs are in the recording).",
            "Robot mode never drives hardware by itself: the Leg calibration panel's motion needs the operator present, and STOP holds from every section.",
            "Comment replies are written straight into the CAD source's .rcad and are not undone here.",
        ],
    });
    match topic {
        None => Ok(all),
        Some(t) => all.get(t).cloned().map(|v| json!({t: v})).ok_or_else(|| format!("no guide topic {t} (about, how_to_call, concepts, workflows, commands, rules)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::actions::Action;

    /// Every robot mode command is in the guide, and every guide command exists.
    #[test]
    fn the_guide_lists_every_robot_command() {
        let all = guide(None).unwrap();
        let listed: std::collections::BTreeSet<String> = all["commands"].as_object().unwrap().keys().cloned().collect();
        let specs: std::collections::BTreeSet<String> = crate::robot::RobotAction::commands().into_iter().map(|s| s.name.to_string()).filter(|n| n != "state").collect();
        assert_eq!(listed, specs);
        assert_eq!(guide(Some("rules")).unwrap()["rules"], all["rules"]);
        assert!(guide(Some("nope")).is_err());
    }
}
