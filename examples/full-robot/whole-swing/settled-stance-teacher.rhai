// Robot policy only: Rust supplies the same bounded body/point suggestions
// and actuator dynamics. History guards stronger feedback after a transfer.
fn control(t, sensors, commands, state) {
    let p = parameters();
    let moving_reference = !state.contains("last_reference");
    let references = #{};
    for name in commands.keys() {
        let joint = name.sub_string(0, name.len() - 7);
        let reference = sensors[joint + ".reference"];
        if state.contains("last_reference") &&
            (reference - state.last_reference[name]).abs() > p.reference_motion_tolerance_rad {
            moving_reference = true;
        }
        references[name] = reference;
    }
    state.last_reference = references;
    if moving_reference || !state.contains("reference_stable_since") {
        state.reference_stable_since = t;
    }
    let support = 1.0;
    for name in p.support_force_channels {
        support = support.min((sensors[name] / p.full_support_force_n).max(0.0).min(1.0));
    }
    let stopped = true;
    for name in p.motion_command_channels { if sensors[name] != 0.0 { stopped = false; } }
    if !stopped { state.reference_stable_since = t; }
    let settled = stopped && t - state.reference_stable_since >= p.standing_settle_s;
    let increment = p.standing_gain_increment + if settled { p.settled_gain_increment } else { 0.0 };
    let extra_gain = if stopped { increment * support } else { 0.0 };
    let body_gain = sensors["command.body_gain"] + extra_gain;
    if p.settled_point_gain_scale < 0.0 || p.settled_point_gain_scale > 1.0 {
        throw "settled point gain scale must be in [0, 1]";
    }
    let point_gain = if settled {
        sensors["command.point_gain"] * p.settled_point_gain_scale
    } else { sensors["command.point_gain"] };
    for name in commands.keys() {
        let joint = name.sub_string(0, name.len() - 7);
        let reference = sensors[joint + ".reference"];
        commands[name] = reference
            + sensors["command.tracking_gain"] * (reference - sensors[joint + ".angle"])
            + body_gain * sensors[joint + ".body_correction"]
            + point_gain * sensors[joint + ".point_correction"];
        let residual = sensors[p.residual_input_by_target[name]];
        if residual != 0.0 { commands[name] += residual; }
    }
    #{commands: commands, state: state}
}
