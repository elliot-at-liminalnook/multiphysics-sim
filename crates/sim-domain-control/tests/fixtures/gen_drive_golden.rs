// Golden vectors for the shared drive kinematics (src/drive/kinematics.rs).
// Python's controller (clients/python) and the Rust tests (tests/drive.rs)
// both check themselves against the file this prints, so the two
// implementations of the limiter, deadman and mixers cannot drift apart.
//
// Regenerate from the repository root (compiles kinematics.rs alone, no cargo):
//   rustc -O --edition 2024 crates/sim-domain-control/tests/fixtures/gen_drive_golden.rs -o /tmp/gen_drive_golden && /tmp/gen_drive_golden > crates/sim-domain-control/tests/fixtures/drive_golden.json
//
// JSON is printed by hand (no serde) with floats in `{:?}` form, which
// round-trips exactly. JSON has no NaN or infinity, so refusals of
// non-finite input are tested in Rust only (tests/drive.rs).
#![allow(dead_code)]

#[path = "../../src/drive/kinematics.rs"]
mod kinematics;

use kinematics::*;

const COMMAND: &str = "rustc -O --edition 2024 crates/sim-domain-control/tests/fixtures/gen_drive_golden.rs -o /tmp/gen_drive_golden && /tmp/gen_drive_golden > crates/sim-domain-control/tests/fixtures/drive_golden.json";

fn num(x: f64) -> String {
    assert!(x.is_finite(), "JSON cannot carry {x}");
    format!("{x:?}")
}
fn arr(xs: &[f64]) -> String {
    format!("[{}]", xs.iter().map(|x| num(*x)).collect::<Vec<_>>().join(", "))
}
fn bools(xs: &[bool]) -> String {
    format!("[{}]", xs.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", "))
}
fn limits_json(l: &Limits) -> String {
    format!("{{\"supported\": {}, \"max_speed\": {}, \"max_accel\": {}}}", bools(&l.supported), arr(&l.max_speed), arr(&l.max_accel))
}
fn deadman_json(d: &Deadman) -> String {
    match d.on_loss {
        OnLoss::Immediate => format!("{{\"timeout_s\": {}, \"on_loss\": \"immediate\"}}", num(d.timeout_s)),
        OnLoss::Ramp { decel } => format!("{{\"timeout_s\": {}, \"on_loss\": \"ramp\", \"decel\": {}}}", num(d.timeout_s), arr(&decel)),
    }
}
fn result<T>(r: Result<T, KinematicsError>, ok: impl FnOnce(T) -> String) -> String {
    match r {
        Ok(v) => ok(v),
        Err(e) => format!("\"error\": \"{}\"", e.kind()),
    }
}
fn section(name: &str, cases: Vec<String>) -> String {
    format!("  \"{name}\": [\n    {}\n  ]", cases.join(",\n    "))
}

/// Limits of a forward + yaw robot (lateral unsupported). Round test values,
/// the same as the resolved-drive example in the design and tests/drive.rs;
/// not the wheeled robot's example profile, whose limits are forward 0.26 m/s,
/// 0.5 m/s^2 and yaw 4.3 rad/s, 8.0 rad/s^2.
fn differential_limits() -> Limits {
    Limits { supported: [true, false, true], max_speed: [0.3, 0.0, 3.0], max_accel: [0.6, 0.6, 6.0] }
}
/// Limits of an omnidirectional robot.
fn mecanum_limits() -> Limits {
    Limits { supported: [true, true, true], max_speed: [0.5, 0.4, 2.0], max_accel: [1.0, 1.0, 4.0] }
}

fn main() {
    let d = differential_limits();
    let m = mecanum_limits();

    let scale_cases: Vec<(&str, [f64; 3], Limits)> = vec![
        ("straight (forward 1)", [1.0, 0.0, 0.0], d),
        ("spin (yaw 1)", [0.0, 0.0, 1.0], d),
        ("arc (forward 0.5, yaw 0.5)", [0.5, 0.0, 0.5], d),
        ("reverse quarter", [-0.25, 0.0, 0.0], d),
        ("negative zero lateral on an unsupported axis", [0.5, -0.0, 0.0], d),
        ("unsupported lateral nonzero", [0.0, 0.5, 0.0], d),
        ("out of range forward 1.5", [1.5, 0.0, 0.0], d),
        ("out of range yaw -1.01", [0.0, 0.0, -1.01], d),
        ("mecanum strafe right (lateral -1)", [0.0, -1.0, 0.0], m),
        ("mecanum combined", [0.5, 0.25, -0.75], m),
    ];
    let scale: Vec<String> = scale_cases
        .into_iter()
        .map(|(name, axes, limits)| {
            let out = result(scale(Axes::from_array(axes), &limits), |t| format!("\"twist\": {}", arr(&t.to_array())));
            format!("{{\"name\": \"{name}\", \"axes\": {}, \"limits\": {}, {out}}}", arr(&axes), limits_json(&limits))
        })
        .collect();

    let limit_cases: Vec<(&str, [f64; 3], [f64; 3], f64, [f64; 3])> = vec![
        ("ramp up from zero over one dt", [0.0; 3], [0.3, 0.0, 3.0], 0.02, [0.6, 0.6, 6.0]),
        ("ramp that reaches the request", [0.29, 0.0, 2.95], [0.3, 0.0, 3.0], 0.02, [0.6, 0.6, 6.0]),
        ("deceleration toward zero", [0.3, 0.0, -3.0], [0.0; 3], 0.02, [1.2, 1.2, 12.0]),
        ("reverse through zero", [0.005, 0.0, 0.0], [-0.3, 0.0, 0.0], 0.02, [0.6, 0.6, 6.0]),
        ("zero dt holds the previous twist", [0.1, 0.2, 0.3], [0.5, -0.4, 2.0], 0.0, [1.0, 1.0, 4.0]),
        ("all three axes at once", [0.0, 0.0, 0.0], [0.5, -0.4, 2.0], 0.1, [1.0, 1.0, 4.0]),
        ("negative dt_s refused", [0.0; 3], [0.3, 0.0, 3.0], -0.02, [0.6, 0.6, 6.0]),
        ("zero max_accel refused", [0.0; 3], [0.3, 0.0, 3.0], 0.02, [0.0, 0.6, 6.0]),
    ];
    let limit: Vec<String> = limit_cases
        .into_iter()
        .map(|(name, previous, request, dt, accel)| {
            let out = result(limit(BodyTwist::from_array(previous), BodyTwist::from_array(request), dt, accel), |t| format!("\"twist\": {}", arr(&t.to_array())));
            format!("{{\"name\": \"{name}\", \"previous\": {}, \"request\": {}, \"dt_s\": {}, \"max_accel\": {}, {out}}}", arr(&previous), arr(&request), num(dt), arr(&accel))
        })
        .collect();

    let ramp = Deadman { timeout_s: 0.5, on_loss: OnLoss::Ramp { decel: [1.2, 1.2, 12.0] } };
    let immediate = Deadman { timeout_s: 0.5, on_loss: OnLoss::Immediate };
    let no_timeout = Deadman { timeout_s: 0.0, on_loss: OnLoss::Immediate };
    let step_cases: Vec<(&str, [f64; 3], [f64; 3], f64, f64, Limits, Deadman)> = vec![
        ("live request ramps up", [0.0; 3], [0.3, 0.0, 3.0], 0.02, 0.1, d, ramp),
        ("live request, age zero", [0.1, 0.0, 1.0], [0.3, 0.0, -3.0], 0.02, 0.0, d, immediate),
        ("deadman timeout, immediate stop", [0.2, 0.0, 1.0], [0.3, 0.0, 3.0], 0.02, 0.6, d, immediate),
        ("deadman timeout, ramp stop", [0.2, 0.0, 1.0], [0.3, 0.0, 3.0], 0.02, 0.6, d, ramp),
        ("age exactly the timeout is expired", [0.2, 0.0, -1.0], [0.3, 0.0, 3.0], 0.02, 0.5, d, ramp),
        ("age just under the timeout is live", [0.2, 0.0, -1.0], [0.3, 0.0, 3.0], 0.02, 0.49, d, ramp),
        ("expired ignores an out-of-profile request", [0.1, 0.0, 0.0], [9.0, 0.0, 0.0], 0.02, 1.0, d, immediate),
        ("out of profile forward 0.4", [0.0; 3], [0.4, 0.0, 0.0], 0.02, 0.0, d, ramp),
        ("unsupported lateral request", [0.0; 3], [0.0, 0.1, 0.0], 0.02, 0.0, d, ramp),
        ("mecanum live request, all axes", [0.0; 3], [0.5, -0.4, 2.0], 0.02, 0.0, m, ramp),
        ("zero deadman timeout refused", [0.0; 3], [0.3, 0.0, 0.0], 0.02, 0.0, d, no_timeout),
    ];
    let step: Vec<String> = step_cases
        .into_iter()
        .map(|(name, previous, request, dt, age, limits, deadman)| {
            let out = result(step(BodyTwist::from_array(previous), BodyTwist::from_array(request), dt, age, &limits, &deadman), |c| {
                format!("\"twist\": {}, \"expired\": {}", arr(&c.twist.to_array()), c.expired)
            });
            format!(
                "{{\"name\": \"{name}\", \"previous\": {}, \"request\": {}, \"dt_s\": {}, \"age_s\": {}, \"limits\": {}, \"deadman\": {}, {out}}}",
                arr(&previous),
                arr(&request),
                num(dt),
                num(age),
                limits_json(&limits),
                deadman_json(&deadman)
            )
        })
        .collect();

    // The wheeled robot's derived geometry (examples/wheeled-robot/baseline):
    // track 0.12 m, wheel radius 0.03 m, both axles along +y (signs [1, 1]).
    let differential_cases: Vec<(&str, f64, f64, [f64; 2], [f64; 3])> = vec![
        ("wheeled robot: straight 0.3 m/s", 0.12, 0.03, [1.0, 1.0], [0.3, 0.0, 0.0]),
        ("wheeled robot: spin 3 rad/s", 0.12, 0.03, [1.0, 1.0], [0.0, 0.0, 3.0]),
        ("wheeled robot: arc 0.15 m/s, 1.5 rad/s", 0.12, 0.03, [1.0, 1.0], [0.15, 0.0, 1.5]),
        ("wheeled robot: reverse arc", 0.12, 0.03, [1.0, 1.0], [-0.1, 0.0, -0.5]),
        ("signs [-1, 1]: straight", 0.12, 0.03, [-1.0, 1.0], [0.3, 0.0, 0.0]),
        ("signs [-1, 1]: spin", 0.12, 0.03, [-1.0, 1.0], [0.0, 0.0, 3.0]),
        ("lateral refused", 0.12, 0.03, [1.0, 1.0], [0.0, 0.1, 0.0]),
        ("wider track", 0.3, 0.05, [1.0, 1.0], [0.2, 0.0, 1.0]),
        ("sign 0.5 refused when the mixer is built", 0.12, 0.03, [0.5, 1.0], [0.3, 0.0, 0.0]),
    ];
    let differential: Vec<String> = differential_cases
        .into_iter()
        .map(|(name, track, radius, signs, twist)| {
            // Construction refusals (a bad sign) are cases too: readers build
            // the mixer and mix in one step.
            let out = result(DifferentialDrive::new(track, radius, signs).and_then(|m| m.mix(BodyTwist::from_array(twist))), |w| format!("\"wheels\": {}", arr(&w)));
            format!(
                "{{\"name\": \"{name}\", \"track_width_m\": {}, \"wheel_radius_m\": {}, \"signs\": {}, \"twist\": {}, {out}}}",
                num(track),
                num(radius),
                arr(&signs),
                arr(&twist)
            )
        })
        .collect();

    let mecanum_cases: Vec<(&str, f64, f64, f64, [f64; 4], [f64; 3])> = vec![
        ("forward", 0.2, 0.15, 0.03, [1.0; 4], [0.3, 0.0, 0.0]),
        ("pure strafe left", 0.2, 0.15, 0.03, [1.0; 4], [0.0, 0.3, 0.0]),
        ("yaw", 0.2, 0.15, 0.03, [1.0; 4], [0.0, 0.0, 1.0]),
        ("combined, mixed signs", 0.2, 0.15, 0.03, [1.0, -1.0, 1.0, -1.0], [0.2, -0.1, 0.5]),
    ];
    let mecanum: Vec<String> = mecanum_cases
        .into_iter()
        .map(|(name, track, base, radius, signs, twist)| {
            let mixer = Mecanum::new(track, base, radius, signs).expect("valid mecanum geometry");
            let out = result(mixer.mix(BodyTwist::from_array(twist)), |w| format!("\"wheels\": {}", arr(&w)));
            format!(
                "{{\"name\": \"{name}\", \"track_width_m\": {}, \"wheelbase_m\": {}, \"wheel_radius_m\": {}, \"signs\": {}, \"twist\": {}, {out}}}",
                num(track),
                num(base),
                num(radius),
                arr(&signs),
                arr(&twist)
            )
        })
        .collect();

    println!("{{");
    println!("  \"schema\": \"sim.drive.golden/1\",");
    println!("  \"generated_by\": \"{}\",", COMMAND.replace('"', "\\\""));
    println!("  \"tolerance\": 1e-12,");
    let sections = [
        section("scale", scale),
        section("limit", limit),
        section("step", step),
        section("differential", differential),
        section("mecanum", mecanum),
    ];
    println!("{}", sections.join(",\n"));
    println!("}}");
}
