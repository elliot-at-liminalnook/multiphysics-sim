// Contract: error [rad], enabled [1] -> bias [rad]. Parameters are the
// control.angle_integral registry's five explicit scales. Call at period_s.
fn control(t, sensors, commands, state) {
    let previous = if state.contains("bias") { state.bias } else { 0.0 };
    state.bias = angle_integral_update(previous, sensors.error,
        sensors.enabled >= 1.0, parameters());
    commands.bias = state.bias;
    #{commands: commands, state: state}
}
