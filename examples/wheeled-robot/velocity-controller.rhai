// Integrate requested shaft velocity into a position reference. The ordinary
// CAD motor, gearbox and firmware execute it; this does not move physical state.
fn control(t, sensors, commands, state) {
    let p = parameters();
    let left = if state.contains("left") { state.left } else { p.initial_left };
    let right = if state.contains("right") { state.right } else { p.initial_right };
    state.left = left + p.period_s * sensors["command.left_speed"];
    state.right = right + p.period_s * sensors["command.right_speed"];
    commands["left axle.target"] = state.left;
    commands["right axle.target"] = state.right;
    #{commands: commands, state: state}
}
