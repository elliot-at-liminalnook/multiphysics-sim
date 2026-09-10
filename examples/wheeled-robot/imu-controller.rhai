// Small sensor-conditioned requests for API acceptance, not a locomotion policy.
fn control(t, sensors, commands, state) {
    let sensed = sensors["imu.body imu.available"] * sensors["imu.body imu.gx"];
    commands["left axle.target"] = sensors["command.left"] + 0.001 * sensed;
    commands["right axle.target"] = sensors["command.right"] - 0.001 * sensed;
    #{commands: commands, state: state}
}
