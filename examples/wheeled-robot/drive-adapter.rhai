// The wheeled rover's kinematic adapter as an embedded Rhai program: a
// teleoperation twist to wheel position targets. The browser (and any host
// that cannot start a process) runs it in the shared embedded session in
// place of the external clients/python/examples/diff_drive_rover.py; the
// binding names both (baseline/robot.controller.json, `embedded`).
//
// Reads (sensors): `command.forward` (m/s), `command.lateral` (m/s),
// `command.yaw` (rad/s): the twist the host already limited with the shared
// rule (kinematics::step on simulation time, drive_host::TwistState);
// `command.heartbeat`: raised by one for every fresh request.
// Writes (commands): `<wheel joint>.target` (rad) for each wheel of the
// drive geometry, in geometry order: an integrated position reference the
// CAD servo firmware follows.
//
// Parameters (`parameters()`, set by sim_runtime::embedded_drive::build):
// `drive` is the resolved drive (sim.drive.resolved/1): the profile's
// limits and deadman, and the geometry (track width, wheel radius, wheel
// joints and signs) derived from the CAD model with its provenance.
// Nothing here is a literal: the geometry always comes from `drive`.
// `period_s` is the model's control period; `initial_targets` the CAD
// servos' starting targets by joint (0.0, the Python program's start).
//
// Per sample: the controller-side deadman on heartbeat staleness
// (drive_update: kinematics::HeartbeatDeadman, simloop's DriveState), then
// the profile's mixer (drive_differential_mix, or drive_mecanum_mix for a
// mecanum drive), then target += period_s * joint_rate, exactly as the
// Python program and velocity-controller.rhai do. All mixing and deadman
// arithmetic happens in the Rust functions (sim_domain_control::drive),
// none here.
//
// Retries: the output is a function of the sample sequence, not of how
// often it is asked. A sample at a time not later than the last one plus
// half a period (a retried slice: PhysicalRobot::advance re-runs a slice
// that failed to converge on the native seam) rolls the targets and the
// deadman back to their state before that time, from the last 16 samples.
// The embedded session itself never retries controller samples.
//
// Intended differences from diff_drive_rover.py: none in the math. Logging
// is omitted, and so is the measured-twist diagnostic (unmixing the
// wheels' tachometers), which the embedded policy's observations do not
// carry under those names.

fn control(t, sensors, commands, state) {
    let p = parameters();
    let d = p.drive;
    let g = d.geometry;
    let period = p.period_s;
    let wheels = g.wheels;
    let history_limit = 16;

    if !state.contains("targets") {
        let initial = [];
        for wheel in wheels {
            initial.push(p.initial_targets[wheel.joint]);
        }
        state.targets = initial;
        state.deadman = #{};
        state.history = [];
    }

    // A retried slice samples a time again: restore the state from before it.
    let history = state.history;
    if history.len() > 0 && t <= history[history.len() - 1].t + 0.5 * period {
        while history.len() > 0 && t <= history[history.len() - 1].t + 0.5 * period {
            let last = history.pop();
            state.targets = last.targets;
            state.deadman = last.deadman;
        }
    }
    history.push(#{t: t, targets: state.targets, deadman: state.deadman});
    while history.len() > history_limit {
        history.remove(0);
    }
    state.history = history;

    let request = [sensors["command.forward"], sensors["command.lateral"], sensors["command.yaw"]];
    let out = drive_update(state.deadman, t, request, sensors["command.heartbeat"], period, d);
    state.deadman = out.state;

    let signs = [];
    for wheel in wheels {
        signs.push(wheel.sign);
    }
    let rates = if d.kinematics == "mecanum" {
        drive_mecanum_mix(g.track_width_m.value, g.wheelbase_m.value, g.wheel_radius_m.value, signs, out.twist)
    } else {
        drive_differential_mix(g.track_width_m.value, g.wheel_radius_m.value, signs, out.twist)
    };

    let targets = state.targets;
    for i in 0..wheels.len() {
        targets[i] += period * rates[i];
        commands[wheels[i].joint + ".target"] = targets[i];
    }
    state.targets = targets;
    #{commands: commands, state: state}
}
