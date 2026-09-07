// Ideal-state experiment, not a hardware sensor/controller claim.
// Correction is outside the registered servo firmware: that firmware and the
// motor equations still determine whether a requested angle can be achieved.
fn control(t, sensors, commands, state) {
    let gain = sensors["command.tracking_gain"];
    for name in commands.keys() {
        let joint = name.sub_string(0, name.len() - 7); // remove ".target"
        let reference = sensors[joint + ".reference"];
        let actual = sensors[joint + ".angle"];
        commands[name] = reference + gain * (reference - actual);
    }
    #{commands: commands, state: state}
}
