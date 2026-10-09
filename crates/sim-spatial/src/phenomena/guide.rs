//! `phenomena_guide` (and `GET /v1/phenomena_guide`): how Phenomena mode
//! works, for an agent starting cold. Concepts, workflows in order, every
//! command with a working example, and the rules. Every command's full
//! description and refusals are in `GET /v1/capabilities`.
use serde_json::{Value, json};

/// The guide (`topic` narrows it to one section).
pub(crate) fn guide(topic: Option<&str>) -> Result<Value, String> {
    let all = json!({
        "about": "Phenomena mode is a gallery of live exhibits (sim_phenomena::exhibits, 35 of them): physical effects that emerge from the simulator's ordinary elements, such as Huygens' clocks synchronising, the tippe top inverting, a passive walker walking, a Rijke tube singing or a sampled controller hunting. Each exhibit runs one model on a run thread on its own clock, with one knob to change it and a verdict that says which regime it is in. The same models are the acceptance scenarios of the sim-phenomena suite, where they must reproduce a published number.",
        "how_to_call": {
            "batch": "POST /v1/batch {\"commands\":[{\"command\":NAME,\"args\":{...}}]} → {\"job_id\"}; GET /v1/jobs/{job_id} until it ends; results[i].ok / value / error. Commands in one batch run in order.",
            "resources": "GET /v1/phenomena_guide (this), /v1/capabilities (every command), /v1/phenomena_state (the gallery as shown, refreshed as frames arrive).",
            "entering": "viewer_mode {mode: phenomena} (no document: it opens on the exhibit it last showed, else the first), or launch sim-spatial --phenomena [--exhibit N|TITLE].",
            "seeing": "phenomena_state holds the numbers (readouts, signal, its chart, verdict, time); screenshot {path} saves the window as drawn. camera_* commands are the shared orbit camera's.",
            "ui_parity": "Every button is also a system_ui control: system_ui {action: {operation: controls}} lists them; {operation: activate, id} presses one through the same handler as a click.",
        },
        "concepts": {
            "exhibit": "An exhibit has a title, a one-sentence summary of what to watch for, a knob (label, unit, min, max, step, value), readouts (labelled values with units), a signal (one value charted over time) and a verdict (a sentence naming the regime the current knob puts it in, e.g. whether the upright Kapitza pendulum is stable). The verdict describes the model; it is not a pass or fail.",
            "time": "Each exhibit runs on its own clock (time, time_unit) at time_scale × speed simulated seconds per real second; real time per tick is clamped to 0.05 s, so a slow frame never jumps the model. speed is clamped to [1/64, 64].",
            "changing_commands": "select, next, previous, knob, reset, pause and speed go to the run thread. Over REST each answers once the frame shown includes it, with phenomena_state as it is then; generation counts the changes, applied names the last command a frame includes, and older frames are never shown.",
            "knob": "Changing the knob rebuilds the exhibit's model at the new value and restarts the chart; a value is clamped to [min, max] and rounded to the step.",
            "errors": "If the model fails (a solver that does not converge, say), error holds the message verbatim and the exhibit stops until reset or another exhibit is chosen. Two suite scenarios, levitron and geyser, are known to fail at tight tolerances.",
            "ready": "While the exhibits are being built, ready is false and changing commands are refused with the message.",
        },
        "workflows": [
            {"goal": "Survey the gallery", "steps": [
                "phenomena_state: exhibits (number, title) and the current one's summary",
                "phenomena_select {exhibit: N} or {exhibit: \"tippe\"} (a title fragment)",
                "read readouts, signal and verdict; screenshot {path} for a picture"]},
            {"goal": "Find where the regime changes", "steps": [
                "phenomena_state.knob gives the range and step",
                "phenomena_knob {value} at several values (each rebuilds the model and restarts the chart)",
                "after each, let it run (phenomena_pause {paused: false}) and read verdict and the signal's chart",
                "phenomena_speed {steps: 1} doubles the speed when the effect is slow"]},
            {"goal": "Recover from an error", "steps": [
                "phenomena_state.error names the failure",
                "phenomena_reset rebuilds at the current knob, or phenomena_knob to a value that converges"]},
        ],
        "commands": {
            "phenomena_guide": {"example": {"topic": "concepts"}, "does": "This guide; topic narrows it."},
            "state": {"example": {}, "does": "phenomena_state plus viewer_mode."},
            "phenomena_state": {"example": {}, "does": "Exhibits, current exhibit, knob, readouts, verdict, signal and chart, time, speed, paused, error, generation."},
            "phenomena_select": {"example": {"exhibit": 1}, "does": "Open an exhibit by 1-based number or title fragment."},
            "phenomena_next": {"example": {}, "does": "The next exhibit (wraps)."},
            "phenomena_previous": {"example": {}, "does": "The previous exhibit (wraps)."},
            "phenomena_knob": {"example": {"value": 1.0}, "does": "Set the knob (value) or nudge it (steps); exactly one."},
            "phenomena_reset": {"example": {}, "does": "Rebuild the model at the current knob."},
            "phenomena_pause": {"example": {"paused": true}, "does": "Pause or run; without paused it toggles."},
            "phenomena_speed": {"example": {"steps": 1}, "does": "Set the speed, or double (steps 1) or halve (steps -1) it; exactly one."},
            "system_ui": {"example": {"action": {"operation": "controls"}}, "does": "The mode's buttons as controls; activate presses one."},
        },
        "rules": [
            "Read phenomena_state after each change: the answer is the frame that includes it.",
            "Treat the verdict as a description of the regime, and the readouts as the evidence; quote units.",
            "An exhibit is a teaching view of a model, not a calibration: published numbers are checked by the sim-phenomena suite (cargo run -p sim-phenomena -- NAME), not here.",
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
    use crate::phenomena::PhenomenaAction;

    /// Every phenomena command is in the guide, every guide command exists,
    /// every example parses as the command it documents, and the exhibit
    /// count in `about` is the library's.
    #[test]
    fn the_guide_lists_every_phenomena_command() {
        let all = guide(None).unwrap();
        let listed: std::collections::BTreeSet<String> = all["commands"].as_object().unwrap().keys().cloned().collect();
        let specs: std::collections::BTreeSet<String> = PhenomenaAction::commands().into_iter().map(|s| s.name.to_string()).collect();
        assert_eq!(listed, specs);
        for (name, c) in all["commands"].as_object().unwrap() {
            let command = sim_api::Command { command: name.clone(), args: c["example"].clone() };
            assert!(PhenomenaAction::parse(&command).is_ok(), "{name}: {:?}", PhenomenaAction::parse(&command).err());
        }
        let count = sim_phenomena::exhibits::all().len();
        assert!(all["about"].as_str().unwrap().contains(&format!("{count} of them")), "the guide's exhibit count is not {count}");
        assert_eq!(guide(Some("rules")).unwrap()["rules"], all["rules"]);
        assert!(guide(Some("nope")).is_err());
    }
}
