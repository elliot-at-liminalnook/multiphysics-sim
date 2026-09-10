// Generic output adapter. Bounds must come from the runtime actuator contract.
// The wrapped policy is named unbounded_control during explicit preparation.
fn control(t, sensors, commands, state) {
    let response = unbounded_control(t, sensors, commands, state);
    let p = parameters_except(["trajectory", "static_feedforward",
        "dynamic_feedforward", "velocity_feedforward"]);
    let count = 0;
    let requested_commands = #{};
    for name in response.commands.keys() {
        let requested = response.commands[name];
        // Infinity-infinity and NaN arithmetic do not equal zero. Reject them
        // before min/max so saturation cannot hide a nonfinite policy output.
        if requested-requested != 0.0 { throw "nonfinite unbounded command"; }
        let bounds = p.output_bounds[name];
        let applied = requested.max(bounds[0]).min(bounds[1]);
        if applied != requested { count += 1; }
        requested_commands[name] = requested;
        response.commands[name] = applied;
    }
    if !response.state.contains("command_saturation_count") {
        response.state.command_saturation_count = 0;
    }
    response.state.command_saturation_count += count;
    response.state.last_saturated_commands = count;
    response.state.last_requested_commands = requested_commands;
    response
}
