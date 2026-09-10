// Finite-horizon tracking diagnostic. Rust owns trajectories and actuator physics.
fn control(t, sensors, commands, state) {
    let p=parameters_except(["trajectory"]);
    let reference=trajectory_parameter_sample("trajectory",t);
    let interval=(t/p.step_s).floor().to_int().max(0).min(p.torque_offsets.len()-1);
    for name in commands.keys() {
        let j=p.motor_indices[name];
        commands[name]=reference.values[j]+p.velocity_offsets[interval][j]
            +p.feedforward_gain*p.torque_offsets[interval][j];
    }
    #{commands:commands,state:state}
}
