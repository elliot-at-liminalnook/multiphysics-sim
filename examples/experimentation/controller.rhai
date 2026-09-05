// Supervisory position targets in radians; Rust servo firmware closes feedback.
import "reference" as reference;

fn control(t, sensors, commands, state) {
    let p = parameters();
    commands["hinge1.target"] = reference::step(t, p.target1);
    if commands.contains("hinge2.target") {
        commands["hinge2.target"] = reference::step(t, p.target2);
    }
    #{commands: commands, state: state}
}
