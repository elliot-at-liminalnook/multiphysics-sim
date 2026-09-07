// Teacher experiment: ideal body pose and floor-load observations feed a
// shared Rust Jacobian correction. Registered servo dynamics remain active.
fn control(t, sensors, commands, state) {
    let joint_gain = sensors["command.tracking_gain"];
    let body_gain = sensors["command.body_gain"];
    for name in commands.keys() {
        let joint = name.sub_string(0, name.len() - 7);
        let reference = sensors[joint + ".reference"];
        commands[name] = reference
            + joint_gain * (reference - sensors[joint + ".angle"])
            + body_gain * sensors[joint + ".body_correction"];
    }
    #{commands: commands, state: state}
}
