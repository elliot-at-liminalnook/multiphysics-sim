//! `inspect_guide` (and `GET /v1/inspect_guide`): how Inspect mode works,
//! for an agent starting cold. Concepts, workflows in order, every command
//! of the spatial view with a working example, and the rules. Every
//! command's full description and refusals are in `GET /v1/capabilities`.
use serde_json::{Value, json};

/// The guide (`topic` narrows it to one section).
pub(crate) fn guide(topic: Option<&str>) -> Result<Value, String> {
    let all = json!({
        "about": "Inspect mode shows one source-bound assembly: a system description (*.description.json, from sim-inspect) and its spatial presentation (the *.spatial.json beside it), optionally with a recorded or live capture bound to it. It reads; it never advances physics and never edits the system. The same commands drive the 3D view that Build and Lessons draw into, so what is learned here applies there.",
        "how_to_call": {
            "batch": "POST /v1/batch {\"commands\":[{\"command\":NAME,\"args\":{...}}]} → {\"job_id\"}; GET /v1/jobs/{job_id} until it ends; results[i].ok / value / error. Commands in one batch run in order.",
            "resources": "GET /v1/inspect_guide (this), /v1/capabilities (every command), and the published resources /v1/state, /v1/description, /v1/spatial, /v1/measurements and /v1/annotations, which change as the window does.",
            "entering": "viewer_mode {mode: inspect, path: /abs/x.description.json} (its *.spatial.json beside it), or launch sim-spatial --description FILE --spatial FILE. Without a path the mode reopens what it showed, else the built-in example assembly.",
            "headless": "sim-spatial --headless --description FILE --spatial FILE serves these commands without a window (inspect mode only; render works, screenshot and viewer_mode do not).",
            "seeing": "render draws the assembly off-screen to a PNG (any view, size or section, without moving the window's camera); screenshot {path} saves the window exactly as drawn. camera_* commands are the shared orbit camera's.",
        },
        "concepts": {
            "description_vs_spatial": "The description is physical identity: components, typed ports, nets, observables with units and provenance, validation findings. The spatial file is presentation only: parts, shapes, dimensions, positions and the source identity each part stands for. Changing the view never changes the description.",
            "frames": "A frame is one set of committed observations at a simulation time (measurements). Inspect only shows frames a recording or a live session produced; a value that was not measured is absent, never guessed.",
            "selection": "One selection shared with every mode: none, components, ports or nets, by their source IDs. select sets it exactly; display {kind: select} picks one component the way the parts list does.",
            "display": "Exploded view, connection lines, X-ray, strobe, hidden parts and overlay layers (power, forces, current, heat, trails) are display state: they change what is drawn, not what is measured.",
            "annotations": "Notes, replies, links and saved views live in a sidecar file shared with the schematic window (--annotations FILE). They are presentation; they never feed physics.",
        },
        "workflows": [
            {"goal": "Understand an assembly", "steps": [
                "description: list components, ports, nets and observables (units and provenance included)",
                "spatial: which parts draw which components",
                "render {options: {view: isometric}} for a picture; state for what the window shows now"]},
            {"goal": "Look at one part closely", "steps": [
                "select {target: {kind: components, ids: [ID]}}",
                "display {action: {kind: hide_selected}} or {kind: set_exploded, enabled: true} to clear the view",
                "render {options: {parts: [ID], view: front}} or with section {axis: x, offset: 0.0} for a cut",
                "display {action: {kind: show_all}} when done"]},
            {"goal": "Read measured values", "steps": [
                "animation: which observables are bound to which parts",
                "measurements: the last frame (time and values); state.live_status says whether a capture is running",
                "display {action: {kind: set_overlay, layer: power, enabled: true}} to draw a measured quantity on the parts"]},
            {"goal": "Leave a note for a person", "steps": [
                "annotations {action: {operation: document}} to read existing notes and saved views",
                "annotations {action: {operation: edit, …}} to add or change one (targets are selections)",
                "annotations {action: {operation: save_view, …}} to keep a camera, selection and display for later"]},
        ],
        "commands": {
            "inspect_guide": {"example": {"topic": "workflows"}, "does": "This guide; topic narrows it."},
            "state": {"example": {}, "does": "Display, selection, camera, annotation revision and live status."},
            "description": {"example": {}, "does": "The typed components, ports, nets, observables and validation."},
            "spatial": {"example": {}, "does": "Display parts, dimensions, provenance and source identities."},
            "animation": {"example": {}, "does": "Which observables are bound to which parts."},
            "measurements": {"example": {}, "does": "The last measured frame."},
            "render": {"example": {"options": {"view": "isometric", "size": {"width": 1280, "height": 900}, "section": null}}, "does": "Off-screen PNG (an image artifact); views current, isometric, front, top, right; optional parts filter and x/y/z section in metres."},
            "select": {"example": {"target": {"kind": "components", "ids": ["source-id"]}}, "does": "Set the shared selection exactly."},
            "display": {"example": {"action": {"kind": "set_exploded", "enabled": true}}, "does": "kind: select {component}, clear_selection, set_exploded, set_connections, set_overlay {layer, enabled}, set_xray, set_strobe ({enabled}), hide_selected, show_all."},
            "camera": {"example": {"focus": [0, 0, 0], "radius": 0.5, "yaw": 0.7, "pitch": 0.4}, "does": "Absolute orbit pose (metres, radians; pitch within ±1.5)."},
            "fit": {"example": {}, "does": "Fit everything shown."},
            "panels": {"example": {"parts": true, "compact": false}, "does": "Show the parts panel; compact inspector."},
            "annotations": {"example": {"action": {"operation": "document"}}, "does": "Read and change notes, replies, links and saved views (shared with the schematic)."},
        },
        "rules": [
            "Inspect never advances physics or edits the system: to change the system use Build mode (system_guide); to run it, Build's system_run or a recording.",
            "Address things by their source IDs from description; refusals name an unknown ID, an out-of-range pitch or a missing part.",
            "A missing value is missing: do not infer a measurement the frame does not hold.",
            "Prefer render over screenshot when the window may be hidden; render never moves the window's camera.",
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
    use crate::inspect::InspectAction;

    /// Every spatial-view command is in the guide, every guide command
    /// exists, and every example parses as the command it documents.
    #[test]
    fn the_guide_lists_every_inspect_command() {
        let all = guide(None).unwrap();
        let listed: std::collections::BTreeSet<String> = all["commands"].as_object().unwrap().keys().cloned().collect();
        let specs: std::collections::BTreeSet<String> = InspectAction::commands().into_iter().map(|s| s.name.to_string()).collect();
        assert_eq!(listed, specs);
        for (name, c) in all["commands"].as_object().unwrap() {
            let command = sim_api::Command { command: name.clone(), args: c["example"].clone() };
            assert!(InspectAction::parse(&command).is_ok(), "{name}: {:?}", InspectAction::parse(&command).err());
        }
        assert_eq!(guide(Some("rules")).unwrap()["rules"], all["rules"]);
        assert!(guide(Some("nope")).is_err());
    }
}
