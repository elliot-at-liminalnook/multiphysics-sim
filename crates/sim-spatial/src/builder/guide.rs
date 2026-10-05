//! `system_guide` (and `GET /v1/system_guide`): how Build mode composes and
//! runs systems, for an agent starting cold. Concepts, workflows in order,
//! the composition commands with working examples, and the rules. Every
//! command's full description, refusals and state fields are in
//! `GET /v1/capabilities`; `system_state` shows everything on screen.
use serde_json::{Value, json};

/// The guide (`topic` narrows it to one section).
pub(crate) fn guide(topic: Option<&str>) -> Result<Value, String> {
    let all = json!({
        "about": "Build mode edits and runs one system file (*.system.json, schema sim.system/2) in the viewer process. A system is one hierarchical composition of elements (registered components with equations), subsystems (definitions placed like linked CAD components), generated assemblies (a robot built from its .simrobot.json) and blocks (executable implementations: FMI 3 FMUs, host controllers). The same file runs in the window (system_run), headless (sim-system run FILE) and in tests. See docs/architecture/composition.md.",
        "how_to_call": {
            "batch": "POST /v1/batch {\"commands\":[{\"command\":NAME,\"args\":{...}}]} → {\"job_id\"}; GET /v1/jobs/{job_id} until it ends; results[i].ok / value / error. Commands in one batch run in order.",
            "resources": "GET /v1/capabilities (every command), /v1/system_guide (this). system_state answers with the file, revision, findings, compile status, live run, composition (blocks, tests, evidence) and history.",
            "entering": "viewer_mode {mode: build, path: /abs/x.system.json}, or system_open {path} from Build mode.",
        },
        "concepts": {
            "physical_vs_signal": "Physical (acausal) ports join conserved quantities and become equations (an electrical net, a shaft, a thermal node). Signal ports carry values one way (one output, many inputs). A block touches the physics only through signals.",
            "blocks": "A block runs code at its clock's ticks, between integrator advances, never inside an equation solve: inputs are sampled from the committed state, outputs hold until its next write. FMI 3 Co-Simulation blocks compute end-of-step outputs (their output at a tick applies at the next tick); host blocks may answer at the same tick (feedthrough). input_delay/output_delay add whole samples. A cycle of same-instant blocks is an algebraic loop and is refused.",
            "units": "A block's ports carry exact quantities from the FMU's units (K → Temperature, W with quantity HeatFlowRate → HeatFlow, rad → Angle…); a connection to another quantity is refused when it is made. No conversion ever happens.",
            "artifacts": "An FMU block records the archive's SHA-256; a run refuses a changed file (re-add it to accept the new artifact). A generated robot records its ports; a run refuses a source that no longer offers them.",
            "robots": "system_add_robot places a robot as a generated assembly with boundary ports: supply_p/supply_n (motor bus), ambient (thermal environment), <joint>.target inputs (servo setpoints) and <joint>.angle / <joint>.speed / imu.* outputs. A battery, a cooling path and controller blocks are ordinary parts wired to them.",
            "tests_and_evidence": "A test is requirements on observables (readable keys such as thermometer.temperature) judged on a run of the system as composed. Its evidence is kept beside the file, bound to the model, every artifact's bytes, the run settings and the test; any change makes it stale and says what changed. A requirement the run cannot judge is not assessed, never a pass.",
            "not_claimed": "An FMU proves the controller code runs against this model. It says nothing about firmware timing on a microcontroller or hardware fidelity: that is a hardware adapter's job.",
        },
        "workflows": [
            {"goal": "Compose a controlled system from scratch", "steps": [
                "system {commands: [add_instance …]} places elements (e.g. thermal.capacitance, thermal.conductance, thermal.ambient, thermal.controlled_heat_source, thermal.temperature_sensor)",
                "system_inspect_fmu {path: fmus/thermostat.fmu} shows what the FMU offers and whether it can be used",
                "system_add_fmu {path, name, period, parameters} adds it as a block",
                "system {commands: [connect {terminals: [{instance, port}, …]} …]} wires sensors to block inputs and block outputs to actuators",
                "system_state: findings list unconnected ports; compile_error is null when it compiles",
                "system_run {action: start} runs it; system_plot {pin: [keys]} graphs observables"]},
            {"goal": "Put a robot in a system", "steps": [
                "copy the .simrobot.json next to the system file",
                "system_add_robot {source: rover.simrobot.json, name: rover}",
                "add a battery (robot.battery) and ground (electrical.ground); connect battery.p to rover.supply_p and battery.n + ground.pin to rover.supply_n",
                "connect rover.ambient to a thermal environment (thermal.ambient, or a capacitance vented through a conductance)",
                "add one controller block per joint (system_add_fmu) and connect rover.<joint>.angle → controller input, controller output → rover.<joint>.target"]},
            {"goal": "Change a block's timing", "steps": ["system {commands: [{command: set_block_timing, name, timing: {clock: {kind: periodic, period, offset}, input_delay, output_delay, deadline_s}}]}"]},
            {"goal": "Prove a requirement", "steps": [
                "system_test {action: set, name, test: {duration_s, requirements: [{id, observable, reduce, window, min, max}]}}",
                "system_test {action: run, name}; poll system_state.composition.running until null",
                "system_state.composition.standing[name] is current {verdict} or stale {changed}; evidence holds each requirement's measured value"]},
        ],
        "commands": {
            "system_guide": {"example": {"topic": "workflows"}, "does": "This guide; topic narrows it."},
            "system_inspect_fmu": {"example": {"path": "fmus/thermostat.fmu"}, "does": "What an FMU offers and why a block could not use it."},
            "system_add_fmu": {"example": {"path": "fmus/thermostat.fmu", "name": "thermostat", "period": 0.5, "parameters": {"setpoint": 294.15}}, "does": "Add an FMU block (interface and SHA-256 from the archive)."},
            "system_add_robot": {"example": {"source": "rover.simrobot.json", "name": "rover"}, "does": "Add a generated robot assembly."},
            "system_test": {"example": {"action": "run", "name": "comfort"}, "does": "Save, delete, run or read acceptance tests and their evidence."},
            "system": {"example": {"commands": [{"command": "connect", "terminals": [{"instance": "thermometer", "port": "temperature"}, {"instance": "thermostat", "port": "temperature"}]}]}, "does": "Every edit (add/remove/rename instances, connect, set_parameter, set_block_timing, set_test, group…), validated and undoable."},
            "system_state": {"example": {}, "does": "Everything shown, including composition (blocks, tests, standing, evidence)."},
            "system_run": {"example": {"action": "start"}, "does": "Run, pause, step or reset the live run (blocks bound to fresh instances every start and reset)."},
            "system_plot": {"example": {"pin": ["thermometer.temperature"]}, "does": "Pin observables to the graph dock."},
        },
        "rules": [
            "Read system_state before and after acting: refusals name their reason (a unit mismatch, an unsupported FMU feature, a changed artifact, an unconnected port); fix the cause, never work around it.",
            "Paths in a system file are relative to its directory; keep FMUs and robot sources next to it.",
            "A host block runs only where its host supplies it; Build mode supplies none, so a system with one runs headless only in a host that binds it.",
            "Evidence is honest: re-run a test after any change it reports; a stale or incomplete verdict is not a pass.",
            "Physics advances on simulation time; blocks run at their ticks. Nothing here drives hardware.",
        ],
    });
    match topic {
        None => Ok(all),
        Some(t) => all.get(t).cloned().map(|v| json!({t: v})).ok_or_else(|| format!("no guide topic {t} (about, how_to_call, concepts, workflows, commands, rules)")),
    }
}

/// `GET /v1/system_guide`.
pub(super) fn publish(rest: Option<bevy::prelude::ResMut<crate::rest::Rest>>) {
    if let Some(mut rest) = rest {
        rest.0.publish_changed("system_guide", "1", || guide(None).unwrap_or_default());
    }
}
