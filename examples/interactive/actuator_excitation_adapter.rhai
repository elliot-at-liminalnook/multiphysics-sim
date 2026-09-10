// Explicit system-identification adapter. Preparation renames the original
// control function to excitation_baseline_control. A fixed authored command
// trajectory is applied while a live motion request is present; baseline state
// continues normally. No future sensor value is read to select these commands.
fn control(t, sensors, commands, state) {
    let response = excitation_baseline_control(t, sensors, commands, state);
    let p = parameters_except(["trajectory", "static_feedforward",
        "dynamic_feedforward", "velocity_feedforward", "excitation_targets"]);
    let sequence = if response.state.contains("excitation_sequence") {
        response.state.excitation_sequence
    } else { -1.0 };
    let age = if response.state.contains("excitation_age_s") {
        response.state.excitation_age_s
    } else { p.command_lease.timeout_s };
    let lease = command_lease_update(sequence, age,
        sensors[p.excitation_sequence_input], p.command_lease);
    response.state.excitation_sequence = lease.sequence;
    response.state.excitation_age_s = lease.age_s;
    if !lease.fresh || (sensors[p.excitation_forward_input] == 0.0
        && sensors[p.excitation_lateral_input] == 0.0) {
        return response;
    }
    let sample = trajectory_parameter_sample("excitation_targets", t);
    if sample.values.len() != p.excitation_actuator_names.len() {
        throw "excitation actuator dimension mismatch";
    }
    for j in 0..p.excitation_actuator_names.len() {
        let name = p.excitation_actuator_names[j];
        if !response.commands.contains(name) { throw "unknown excitation actuator"; }
        response.commands[name] = sample.values[j];
    }
    response
}
