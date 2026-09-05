// Same twelve actuator coordinates as controller.py; commands are in radians.
// This default holds the imported pose. Hardware bus IDs/neutral/polarity are
// deliberately absent: this is a simulation controller, not a hardware driver.
fn control(t, sensors, commands, state) {
    for leg in ["+X", "-Y", "+Y", "-X"] {
        for role in ["Hip servo output", "Worm servo output", "Foot servo output"] {
            let channel = leg + " | " + role + ".target";
            commands[channel] = 0.0;
        }
    }
    #{commands: commands, state: state}
}
