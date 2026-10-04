//! `project_guide` (and `GET /v1/project_guide`): how a robot goes from a
//! description to a tested design and its parts in this window, for an
//! agent starting cold. Written from the code it describes.
use serde_json::{Value, json};

/// The guide (`topic` narrows it).
pub(crate) fn guide(topic: Option<&str>) -> Result<Value, String> {
    let all = json!({
        "about": "A robot project takes one robot from a description to a tested design and its part files, in order: 1 Design (CAD mode) → 2 Model (the saved design exported in process to a simulation model) → 3 Test (an acceptance test run on the model) → 4 Learn (lessons written for this robot) → 5 Make (part files of the tested design). The bottom strip of every mode shows the open project and its steps; each step switches to its mode on the project's document.",
        "how_to_call": {
            "batch": "POST /v1/batch {\"commands\":[{\"command\":NAME,\"args\":{...}}]} → {\"job_id\"}; GET /v1/jobs/{job_id} until status is succeeded | failed | cancelled.",
            "resources": "GET /v1/project_state (steps, model assumptions, latest test, lessons, chat), /v1/project_guide (this), /v1/cad_guide, /v1/robot_guide, /v1/capabilities.",
            "modes": "project_* commands work in every mode. CAD commands (cad_*) work in CAD mode; project_step design (or project_chat) switches there on the project's file.",
        },
        "concepts": {
            "project_file": "<name>.robot.json beside <name>.rcad (the robot: CAD owns its physical definition), <name>.simrobot.json (exported from it), results/ (test reports), lessons/ (this robot's lessons), make/ (part files). The project holds the description, the acceptance test and the design conversation; the robot itself is the CAD file.",
            "model": "Made in process from the saved CAD file (sim_cad::physical): links (fixed joints and motor mounts merge bodies), exact masses and inertias, joints, library motors, sensors, settings. Every guess is listed in source.assumptions; blocking ones (an unmeasured drive backlash on a motorized joint, a body without a material, a joint with one limit) must be set in CAD before a test counts. Not modelled (stated in the file): flexible links, link-to-link contact, bearing inference from cylinders.",
            "test": "The acceptance test (project_set_test) commands a joint trajectory for a duration and states criteria; the run judges each pass, fail or not assessed. Not assessed is never a pass; the verdict is passed only when every criterion passed, and counts as evidence only when the model has no blocking assumption. Criterion part_strength checks every printed body with the print registry's layer-aware stress check under the run's peak loads (after the run, from the design's meshes). The first run makes a starting test from the driven joints, saved in the project to edit.",
            "freshness": "A model is current when it was exported from the saved CAD file as it is now (source.cad_sha256); a test is current when it ran on the current model. Unsaved CAD edits make step 1 ask for a save; step 2's button saves first.",
            "design_assistant": "project_chat asks an AI that edits the design through CAD's commands (each one undo step) and may make the model and run the test; its answers are kept in the project file.",
        },
        "workflows": [
            {"goal": "Start from a blank window", "steps": ["project_new {name, description}: makes the project and an empty design, opens CAD; with a description the design assistant builds it", "watch project_state.chat and cad_state; ask for changes with project_chat {text}"]},
            {"goal": "Build the robot yourself (CAD commands)", "steps": ["cad_model {op: box | cylinder …} for parts; cad_op {name: set_material, args: [[ids], \"pla\"]}", "cad_op {name: set_ground, args: [BASE]} (a bench robot)", "cad_op {name: add_motor, args: [\"mg996r\", [x,y,z], [dx,dy,dz]], kwargs: {mount_on: BASE}}", "cad_op {name: add_joint, args: [\"revolute\", PARENT, CHILD, [pivot mm], [axis]], kwargs: {lower, upper, motor: MOTOR}}", "cad_op {name: connect_fixed, args: [BODY, PAYLOAD]} for parts that move together", "cad_op {name: set_joint_physics, args: [JOINT], kwargs: {drive_backlash: {width_rad, provenance: \"estimated\", reference}}}"]},
            {"goal": "Prove it", "steps": ["project_step {step: model}: saves the design if needed and makes the model; read project_state.model.assumptions (fix blocking ones in CAD, then again)", "project_set_test {test} to state what it must do (or let the first run make a starting test)", "project_test, then poll project_state.test (running, latest: verdict, outcomes, summary)", "on a failure: change the design (or, if the requirement was wrong, the test) and run it again"]},
            {"goal": "Teach the person", "steps": ["project_lessons {op: suggest}", "project_lessons {op: write, topic}: the AI writes it with this robot's numbers and a render; Lessons mode opens it"]},
            {"goal": "Make it", "steps": ["project_make (after a passed test of the current design): STL files and make/parts.json under make/"]},
        ],
        "commands": {
            "project_state": {"example": {}},
            "project_new": {"example": {"name": "Lift arm", "description": "A one-joint arm that lifts a 20 g weight to 60° in a second"}},
            "project_open": {"example": {"path": "/abs/projects/lift-arm", "step": "test"}},
            "project_close": {"example": {}},
            "project_step": {"example": {"step": "model"}, "steps": "design | model | test | learn | make"},
            "project_export": {"example": {}},
            "project_test": {"example": {}},
            "project_set_test": {"example": {"test": {"name": "lift", "duration_s": 3.0, "trajectory": [{"t": 0.0, "targets": {"Shoulder": 0.0}}, {"t": 1.0, "targets": {"Shoulder": 1.05}}], "criteria": [{"kind": "reaches", "joint": "Shoulder", "target": 1.05, "tolerance": 0.05, "by_s": 1.5}, {"kind": "torque_margin", "min": 0.3}]}}},
            "project_chat": {"example": {"text": "Make the arm 20 mm longer"}},
            "project_lessons": {"example": {"op": "write", "topic": "holding-torque"}},
            "project_make": {"example": {}},
            "project_cancel": {"example": {}},
            "project_panel": {"example": {"open": true, "view": "test"}},
            "project_guide": {"example": {"topic": "workflows"}},
        },
        "rules": [
            "Do the steps in order; a later step's button says what an earlier one still needs.",
            "Never fill in a missing physical value to get past a blocking assumption: set it in CAD with its provenance (estimated, measured) and a reference.",
            "Not assessed is not a pass; say so when reporting a test.",
            "A passed test of a model with estimates is a simulation result, not a hardware result: say which values are estimates.",
            "The design assistant and you edit through the window's commands only; never edit project or repository files directly.",
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

    /// Every project command is in the guide, and every guide command exists.
    #[test]
    fn the_guide_lists_every_project_command() {
        let all = guide(None).unwrap();
        let listed: std::collections::BTreeSet<String> = all["commands"].as_object().unwrap().keys().cloned().collect();
        let specs: std::collections::BTreeSet<String> = super::super::ProjectAction::commands().into_iter().map(|s| s.name.to_string()).collect();
        assert_eq!(listed, specs);
        // Each example parses as the command it documents.
        for (name, c) in all["commands"].as_object().unwrap() {
            let command = sim_api::Command { command: name.clone(), args: c["example"].clone() };
            assert!(super::super::ProjectAction::parse(&command).is_ok(), "{name}: {:?}", super::super::ProjectAction::parse(&command).err());
        }
    }
}
