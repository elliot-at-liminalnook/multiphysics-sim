//! Drive layers: the golden vectors shared with Python, analytic cases for
//! the limiter, deadman and mixers, profile refusals, and the registered
//! `control.drive_limiter` element.
//!
//! The golden file is generated from src/drive/kinematics.rs; regenerate it
//! from the repository root with
//!   rustc -O --edition 2024 crates/sim-domain-control/tests/fixtures/gen_drive_golden.rs -o /tmp/gen_drive_golden && /tmp/gen_drive_golden > crates/sim-domain-control/tests/fixtures/drive_golden.json
use serde_json::Value;
use sim_domain_control::drive::kinematics::{self, Axes, BodyTwist, Deadman, DifferentialDrive, KinematicsError, Limits, Mecanum, OnLoss};
use sim_domain_control::drive::profile::{self, DriveLimiter, DriveProfile, ResolvedDrive};
use std::path::Path;

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}
fn nums<const N: usize>(v: &Value) -> [f64; N] {
    let a = v.as_array().unwrap_or_else(|| panic!("not an array: {v}"));
    assert_eq!(a.len(), N, "{v}");
    std::array::from_fn(|i| num(&a[i]))
}
fn twist(v: &Value) -> BodyTwist {
    BodyTwist::from_array(nums(v))
}
fn limits(v: &Value) -> Limits {
    let s = v["supported"].as_array().unwrap();
    Limits { supported: std::array::from_fn(|i| s[i].as_bool().unwrap()), max_speed: nums(&v["max_speed"]), max_accel: nums(&v["max_accel"]) }
}
fn deadman(v: &Value) -> Deadman {
    let on_loss = match v["on_loss"].as_str().unwrap() {
        "immediate" => OnLoss::Immediate,
        "ramp" => OnLoss::Ramp { decel: nums(&v["decel"]) },
        other => panic!("on_loss {other}"),
    };
    Deadman { timeout_s: num(&v["timeout_s"]), on_loss }
}
/// Compare a computed result to a golden case: values within `tol`, or the
/// same refusal kind.
fn check<const N: usize>(section: &str, case: &Value, key: &str, got: Result<[f64; N], KinematicsError>, tol: f64) {
    let name = format!("{section}: {}", case["name"]);
    match (got, case.get("error")) {
        (Ok(values), None) => {
            let want: [f64; N] = nums(&case[key]);
            for i in 0..N {
                assert!((values[i] - want[i]).abs() <= tol, "{name}: {key} {values:?} != golden {want:?}");
            }
        }
        (Err(e), Some(kind)) => assert_eq!(e.kind(), kind.as_str().unwrap(), "{name}: {e}"),
        (Ok(values), Some(kind)) => panic!("{name}: computed {values:?}, golden refuses with {kind}"),
        (Err(e), None) => panic!("{name}: refused ({e}), golden has a value"),
    }
}

#[test]
fn golden_vectors_match_the_shared_kinematics() {
    let golden: Value = serde_json::from_str(include_str!("fixtures/drive_golden.json")).unwrap();
    assert_eq!(golden["schema"], "sim.drive.golden/1");
    let tol = num(&golden["tolerance"]);
    let cases = |section: &str| golden[section].as_array().unwrap_or_else(|| panic!("section {section}")).clone();
    let mut count = 0;
    for case in cases("scale") {
        let got = kinematics::scale(Axes::from_array(nums(&case["axes"])), &limits(&case["limits"])).map(BodyTwist::to_array);
        check("scale", &case, "twist", got, tol);
        count += 1;
    }
    for case in cases("limit") {
        let got = kinematics::limit(twist(&case["previous"]), twist(&case["request"]), num(&case["dt_s"]), nums(&case["max_accel"])).map(BodyTwist::to_array);
        check("limit", &case, "twist", got, tol);
        count += 1;
    }
    for case in cases("step") {
        let got = kinematics::step(twist(&case["previous"]), twist(&case["request"]), num(&case["dt_s"]), num(&case["age_s"]), &limits(&case["limits"]), &deadman(&case["deadman"]));
        if let (Ok(c), Some(expired)) = (&got, case.get("expired")) {
            assert_eq!(c.expired, expired.as_bool().unwrap(), "step: {}", case["name"]);
        }
        check("step", &case, "twist", got.map(|c| c.twist.to_array()), tol);
        count += 1;
    }
    for case in cases("differential") {
        // A bad geometry (a sign of 0.5) is refused when the mixer is built.
        let got = DifferentialDrive::new(num(&case["track_width_m"]), num(&case["wheel_radius_m"]), nums(&case["signs"])).and_then(|m| m.mix(twist(&case["twist"])));
        check("differential", &case, "wheels", got, tol);
        count += 1;
    }
    for case in cases("mecanum") {
        let got = Mecanum::new(num(&case["track_width_m"]), num(&case["wheelbase_m"]), num(&case["wheel_radius_m"]), nums(&case["signs"])).and_then(|m| m.mix(twist(&case["twist"])));
        check("mecanum", &case, "wheels", got, tol);
        count += 1;
    }
    assert!(count >= 30, "only {count} golden cases");
    // Every section the generator gives refusal cases has a bad_parameter one.
    for section in ["limit", "step", "differential"] {
        assert!(cases(section).iter().any(|c| c["error"] == "bad_parameter"), "section {section} has no bad_parameter case");
    }
}

const W: f64 = 0.12;
const R: f64 = 0.03;

#[test]
fn differential_straight_spin_and_arc_by_hand() {
    let mixer = DifferentialDrive::new(W, R, [1.0, 1.0]).unwrap();
    let close = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12;
    // Straight: both wheels v / R.
    assert!(close(mixer.mix(BodyTwist::new(0.3, 0.0, 0.0)).unwrap(), [10.0, 10.0]));
    // Spin: the wheels move ±ω·W/2, left backwards for counter-clockwise.
    assert!(close(mixer.mix(BodyTwist::new(0.0, 0.0, 1.0)).unwrap(), [-0.06 / R, 0.06 / R]));
    // Arc: v = 0.2, ω = 1 → left (0.2 − 0.06)/0.03, right (0.2 + 0.06)/0.03.
    assert!(close(mixer.mix(BodyTwist::new(0.2, 0.0, 1.0)).unwrap(), [0.14 / R, 0.26 / R]));
    // Lateral is refused by name.
    let e = mixer.mix(BodyTwist::new(0.0, 0.1, 0.0)).unwrap_err();
    assert_eq!(e.kind(), "unsupported");
    assert!(e.to_string().contains("lateral"), "{e}");
}

#[test]
fn mecanum_pure_strafe() {
    let mixer = Mecanum::new(0.2, 0.15, R, [1.0; 4]).unwrap();
    // Left strafe: front-left and rear-right roll back, the others forward.
    let w = mixer.mix(BodyTwist::new(0.0, 0.3, 0.0)).unwrap();
    for (got, want) in w.iter().zip([-10.0, 10.0, 10.0, -10.0]) {
        assert!((got - want).abs() < 1e-12, "{w:?}");
    }
}

#[test]
fn mixers_round_trip() {
    let diff = DifferentialDrive::new(W, R, [-1.0, 1.0]).unwrap();
    let mec = Mecanum::new(0.2, 0.15, R, [1.0, -1.0, -1.0, 1.0]).unwrap();
    for t in [BodyTwist::new(0.3, 0.0, 0.0), BodyTwist::new(-0.1, 0.0, 2.5), BodyTwist::new(0.0, 0.0, -3.0)] {
        let back = diff.unmix(diff.mix(t).unwrap());
        assert!(back.to_array().iter().zip(t.to_array()).all(|(a, b)| (a - b).abs() < 1e-12), "{t:?} -> {back:?}");
    }
    for t in [BodyTwist::new(0.3, -0.2, 0.0), BodyTwist::new(-0.1, 0.4, 1.5), BodyTwist::new(0.0, 0.0, -2.0)] {
        let back = mec.unmix(mec.mix(t).unwrap());
        assert!(back.to_array().iter().zip(t.to_array()).all(|(a, b)| (a - b).abs() < 1e-12), "{t:?} -> {back:?}");
    }
}

fn wheeled_limits() -> Limits {
    Limits { supported: [true, false, true], max_speed: [0.3, 0.0, 3.0], max_accel: [0.6, 0.6, 6.0] }
}

#[test]
fn acceleration_ramp_reaches_the_request_after_ceil_v_over_a_dt_steps() {
    let (v, a, dt) = (0.3, 0.6, 0.02);
    let n = (v / (a * dt)).ceil() as usize; // 25
    let request = BodyTwist::new(v, 0.0, 0.0);
    let mut t = BodyTwist::ZERO;
    for _ in 0..n - 1 {
        t = kinematics::limit(t, request, dt, [a, 0.6, 6.0]).unwrap();
    }
    assert!(t.forward_m_s < v - 0.5 * a * dt, "not there before step {n}: {t:?}");
    t = kinematics::limit(t, request, dt, [a, 0.6, 6.0]).unwrap();
    assert!((t.forward_m_s - v).abs() < 1e-12, "there at step {n}: {t:?}");
}

#[test]
fn deadman_stops_immediately_or_ramps() {
    let limits = wheeled_limits();
    let dt = 0.02;
    let request = BodyTwist::new(0.3, 0.0, 0.0);
    for on_loss in [OnLoss::Immediate, OnLoss::Ramp { decel: [1.2, 1.2, 12.0] }] {
        let deadman = Deadman { timeout_s: 0.5, on_loss };
        // One request at t = 0, then silence; reach full speed first.
        let mut t = BodyTwist::new(0.3, 0.0, 0.0);
        let mut k = 0;
        let expired_at = loop {
            let age = k as f64 * dt;
            let c = kinematics::step(t, request, dt, age, &limits, &deadman).unwrap();
            t = c.twist;
            if c.expired {
                break age;
            }
            assert_eq!(t, request, "live until the timeout");
            k += 1;
        };
        assert!(expired_at >= 0.5 && expired_at < 0.5 + dt, "expired at {expired_at}");
        match on_loss {
            OnLoss::Immediate => assert_eq!(t, BodyTwist::ZERO),
            OnLoss::Ramp { .. } => {
                assert!((t.forward_m_s - (0.3 - 1.2 * dt)).abs() < 1e-12, "{t:?}");
                // ceil(0.3 / (1.2 · 0.02)) = 13 steps to zero in all.
                for _ in 0..12 {
                    t = kinematics::step(t, request, dt, 1.0, &limits, &deadman).unwrap().twist;
                }
                assert!(t.forward_m_s.abs() < 1e-12, "{t:?}");
            }
        }
    }
    // A NaN or negative age is a broken clock: lost.
    let deadman = Deadman { timeout_s: 0.5, on_loss: OnLoss::Immediate };
    assert!(kinematics::deadman_expired(f64::NAN, &deadman) && kinematics::deadman_expired(-0.1, &deadman));
}

#[test]
fn scale_refuses_nan_and_out_of_range_axes() {
    let limits = wheeled_limits();
    let e = kinematics::scale(Axes { forward: f64::NAN, lateral: 0.0, yaw: 0.0 }, &limits).unwrap_err();
    assert_eq!(e.kind(), "not_finite");
    assert!(e.to_string().contains("axes.forward"), "{e}");
    assert_eq!(kinematics::scale(Axes { forward: 0.0, lateral: 0.0, yaw: f64::INFINITY }, &limits).unwrap_err().kind(), "not_finite");
    assert_eq!(kinematics::scale(Axes { forward: 1.5, lateral: 0.0, yaw: 0.0 }, &limits).unwrap_err().kind(), "out_of_range");
    assert_eq!(kinematics::check_twist(BodyTwist::new(f64::NAN, 0.0, 0.0), &limits).unwrap_err().kind(), "not_finite");
}

// ---------------------------------------------------------------------------
// Profiles

fn profile_value() -> Value {
    serde_json::json!({
        "schema": "sim.drive/1",
        "description": "test profile",
        "kinematics": {"type": "differential", "left": "left axle", "right": "right axle"},
        "geometry": {"source": "model"},
        "axes": {
            "forward": {"max_speed": {"value": 0.3, "unit": "m/s"}, "max_accel": {"value": 0.6, "unit": "m/s^2"}, "stop_decel": {"value": 1.2, "unit": "m/s^2"}, "provenance": {"kind": "estimated", "source": "test"}},
            "yaw": {"max_speed": {"value": 3.0, "unit": "rad/s"}, "max_accel": {"value": 6.0, "unit": "rad/s^2"}, "stop_decel": {"value": 12.0, "unit": "rad/s^2"}, "provenance": {"kind": "estimated", "source": "test"}}
        },
        "actions": [{"name": "stop", "request": "stop"}, {"name": "halt", "request": "halt", "description": "zero at once"}],
        "deadman": {"timeout_s": 0.5, "on_loss": "ramp", "provenance": {"kind": "estimated", "source": "test"}}
    })
}
fn parse(value: &Value) -> Result<DriveProfile, profile::DriveProfileError> {
    DriveProfile::from_json(&value.to_string(), Path::new("robot.drive.json"))
}
fn refused(value: &Value) -> profile::DriveProfileError {
    parse(value).expect_err("the profile should be refused")
}

#[test]
fn a_valid_profile_gives_the_shared_limits_and_deadman() {
    let p = parse(&profile_value()).unwrap();
    let l = p.limits();
    assert_eq!(l.supported, [true, false, true]);
    assert_eq!(l.max_speed, [0.3, 0.0, 3.0]);
    // Unsupported lateral borrows forward's (unused) acceleration.
    assert_eq!(l.max_accel, [0.6, 0.6, 6.0]);
    assert_eq!(p.deadman(), Deadman { timeout_s: 0.5, on_loss: OnLoss::Ramp { decel: [1.2, 1.2, 12.0] } });
    assert_eq!(p.action("halt").unwrap().request, profile::ActionRequest::Halt);
    let e = p.action("dance").unwrap_err();
    assert!(e.contains("dance") && e.contains("stop, halt"), "{e}");
    // Serialize and parse back unchanged.
    let again = DriveProfile::from_json(&serde_json::to_string(&p).unwrap(), Path::new("robot.drive.json")).unwrap();
    assert_eq!(again, p);
}

#[test]
fn malformed_profiles_are_refused_with_file_and_field() {
    // Unknown field inside an axis: serde's message, under the axis.
    let mut v = profile_value();
    v["axes"]["forward"]["max_sped"] = serde_json::json!(1.0);
    let e = refused(&v);
    assert_eq!(e.field, "axes.forward");
    assert!(e.message.contains("unknown field `max_sped`"), "{e}");
    assert!(e.to_string().starts_with("robot.drive.json: axes.forward: "), "{e}");

    // Unknown field inside a quantity.
    let mut v = profile_value();
    v["axes"]["yaw"]["max_accel"]["units"] = serde_json::json!("rad/s^2");
    assert_eq!(refused(&v).field, "axes.yaw.max_accel");

    // Unknown top-level field.
    let mut v = profile_value();
    v["extra"] = serde_json::json!(true);
    let e = refused(&v);
    assert_eq!(e.field, "(top level)");
    assert!(e.message.contains("unknown field `extra`"), "{e}");

    // Not JSON.
    assert_eq!(DriveProfile::from_json("{", Path::new("robot.drive.json")).unwrap_err().field, "(document)");

    // Extra keys beside `"source": "model"` are refused, not ignored.
    let mut v = profile_value();
    v["geometry"]["track_width_m"] = serde_json::json!({"value": 0.12, "source": "caliper"});
    let e = refused(&v);
    assert_eq!(e.field, "geometry");
    assert!(e.message.contains("unknown field `track_width_m`"), "{e}");
}

#[test]
fn a_newer_or_unknown_schema_is_refused_before_serde() {
    let mut v = profile_value();
    v["schema"] = serde_json::json!("sim.drive/2");
    // Even with fields this build does not know, the schema is named first.
    v["axes"]["forward"]["jerk"] = serde_json::json!(1.0);
    let e = refused(&v);
    assert_eq!(e.field, "schema");
    assert!(e.message.contains("newer schema sim.drive/2") && e.message.contains("sim.drive/1"), "{e}");
    v["schema"] = serde_json::json!("sim.drivee/1");
    let e = refused(&v);
    assert!(e.field == "schema" && e.message.contains("unknown schema"), "{e}");
    v.as_object_mut().unwrap().remove("schema");
    assert_eq!(refused(&v).field, "schema");
}

#[test]
fn units_axes_ranges_and_names_are_validated() {
    let mut v = profile_value();
    v["axes"]["forward"]["max_accel"]["unit"] = serde_json::json!("m/s");
    let e = refused(&v);
    assert_eq!(e.field, "axes.forward.max_accel.unit");
    assert!(e.message.contains("m/s^2"), "{e}");

    let mut v = profile_value();
    v["axes"]["lateral"] = v["axes"]["forward"].clone();
    let e = refused(&v);
    assert_eq!(e.field, "axes.lateral");
    assert!(e.message.contains("cannot move sideways"), "{e}");

    let mut v = profile_value();
    v["axes"]["sideways"] = v["axes"]["forward"].clone();
    assert_eq!(refused(&v).field, "axes.sideways");

    // Ranges come from the registry descriptor.
    let mut v = profile_value();
    v["axes"]["yaw"]["max_accel"]["value"] = serde_json::json!(0.0);
    let e = refused(&v);
    assert_eq!(e.field, "axes.yaw.max_accel.value");
    assert!(e.message.contains("rad/s^2"), "{e}");
    let mut v = profile_value();
    v["deadman"]["timeout_s"] = serde_json::json!(-1.0);
    assert_eq!(refused(&v).field, "deadman.timeout_s");

    let mut v = profile_value();
    v["actions"][1]["name"] = serde_json::json!("stop");
    assert_eq!(refused(&v).field, "actions[1].name");

    let mut v = profile_value();
    v["kinematics"]["right"] = serde_json::json!("left axle");
    assert_eq!(refused(&v).field, "kinematics.right");

    let mut v = profile_value();
    v["axes"] = serde_json::json!({});
    assert_eq!(refused(&v).field, "axes");

    // Declared geometry needs one sign per wheel.
    let mut v = profile_value();
    v["geometry"] = serde_json::json!({"source": "declared", "track_width_m": {"value": 0.12, "source": "caliper"}, "wheel_radius_m": {"value": 0.03, "source": "caliper"}, "signs": [1]});
    assert_eq!(refused(&v).field, "geometry.signs");
}

#[test]
fn resolved_drive_has_the_documented_shape() {
    use sim_domain_control::drive::geometry::{DriveGeometry, Provenance, Valued, WheelJoint};
    let p = parse(&profile_value()).unwrap();
    let derived = |from: &str| Provenance::Derived { from: from.into() };
    let geometry = DriveGeometry {
        track_width_m: Valued { value: W, provenance: derived("test") },
        wheelbase_m: None,
        wheel_radius_m: Valued { value: R, provenance: derived("test") },
        wheels: ["left axle", "right axle"].map(|j| WheelJoint { joint: j.into(), sign: 1.0, provenance: derived("test") }).to_vec(),
    };
    let resolved = p.resolve(Path::new("robot.drive.json"), "abc123", geometry.clone()).unwrap();
    resolved.validate().unwrap();
    let json = serde_json::to_value(&resolved).unwrap();
    assert_eq!(json["schema"], profile::RESOLVED_SCHEMA);
    assert_eq!(json["kinematics"], "differential");
    assert_eq!(json["geometry"]["wheelbase_m"], Value::Null);
    assert_eq!(json["geometry"]["track_width_m"]["provenance"]["kind"], "derived");
    assert_eq!(json["limits"]["stop_decel"], serde_json::json!([1.2, 1.2, 12.0]));
    assert_eq!(json["deadman"], serde_json::json!({"timeout_s": 0.5, "on_loss": "ramp"}));
    let back: ResolvedDrive = serde_json::from_value(json).unwrap();
    assert_eq!(back.limits(), p.limits());
    assert_eq!(back.deadman(), p.deadman());

    // A differential drive with lateral marked supported is refused.
    let mut sideways = resolved.clone();
    sideways.limits.supported[1] = true;
    let e = sideways.validate().unwrap_err();
    assert!(e.contains("supported[1]") && e.contains("differential"), "{e}");

    // Wheels in the wrong order are refused.
    let mut swapped = geometry;
    swapped.wheels.reverse();
    assert_eq!(p.resolve(Path::new("robot.drive.json"), "abc123", swapped).unwrap_err().field, "geometry");
}

#[test]
fn the_wheeled_robot_example_profile_validates() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/wheeled-robot/baseline/robot.drive.json");
    assert!(path.exists(), "{} is missing: the wheeled robot's drive profile is the proof instance", path.display());
    let (p, sha) = DriveProfile::load(&path).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(sha.len(), 64);
    assert_eq!(p.kinematics, profile::KinematicsSpec::Differential { left: "left axle".into(), right: "right axle".into() });
    assert_eq!(p.geometry, profile::GeometrySource::Model {});
    assert_eq!(p.axes.keys().map(String::as_str).collect::<Vec<_>>(), ["forward", "yaw"]);
    assert_eq!(p.action("stop").unwrap().request, profile::ActionRequest::Stop);
    assert_eq!(p.action("halt").unwrap().request, profile::ActionRequest::Halt);
    assert_eq!(p.deadman.timeout_s, 0.5);
    assert_eq!(p.deadman.on_loss, profile::OnLossSpec::Ramp);
}

// ---------------------------------------------------------------------------
// The registered element

#[test]
fn the_limiter_is_registered_and_steps_the_shared_rule() {
    use sim_core::{BehaviorRegistry, Context, View};
    let mut registry = BehaviorRegistry::default();
    sim_domain_control::elements::register(&mut registry).unwrap();
    let d = registry.get(&profile::LIMITER.into()).unwrap();
    assert!(d.notes.is_some_and(|n| n.category == "Control"));
    let parameters: std::collections::BTreeMap<String, f64> = [
        ("max_speed.forward", 0.3),
        ("max_accel.forward", 0.6),
        ("stop_decel.forward", 1.2),
        ("max_speed.yaw", 3.0),
        ("max_accel.yaw", 6.0),
        ("stop_decel.yaw", 12.0),
        // Off the sample grid, so accumulated and subtracted ages agree on expiry.
        ("deadman_timeout", 0.11),
        ("on_loss_immediate", 0.0),
        ("period", 0.02),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    d.validate_parameters(&parameters).unwrap();
    let limiter = DriveLimiter::from_parameters(&parameters).unwrap();
    assert_eq!(limiter.limits, Limits { supported: [true, false, true], max_speed: [0.3, 0.0, 3.0], max_accel: [0.6, 0.6, 6.0] });
    let mut b = d.equations.unwrap()(&parameters).unwrap();
    let mut states: Vec<f64> = b.states().iter().map(|s| s.initial).collect();
    assert_eq!(states.len(), 6);
    let deadman = limiter.deadman;
    let mut expected = BodyTwist::ZERO;
    let mut last_fresh = None::<f64>;
    for i in 0..30 {
        let t = i as f64 * 0.02;
        // Requests (sequence 1..10) for the first 10 samples, then silence.
        let sequence = if i < 10 { (i + 1) as f64 } else { 10.0 };
        let input = [0.3, 0.0, 3.0, sequence];
        let old = states.clone();
        let v = View { time: t, states: &old, offsets: &[0], rate_map: &[], across: &[], across_rates: &[], signals_in: &input };
        let mut events = vec![];
        b.scheduled_events(&v, &mut events);
        assert_eq!(events, vec![(0, t)]);
        b.jump(0, &v, &mut states);
        if i < 10 {
            last_fresh = Some(t);
        }
        let age = t - last_fresh.unwrap();
        let dt = if i == 0 { 0.0 } else { 0.02 };
        expected = kinematics::step(expected, BodyTwist::new(0.3, 0.0, 3.0), dt, age, &limiter.limits, &deadman).unwrap().twist;
        let (mut residual, mut signals) = ([0.0; 6], [0.0; 3]);
        b.residual(&mut Context::new(t, &states, &[0.0; 6], &[0], &[], &[], &[], &input, &mut residual, &mut [], &mut signals));
        for k in 0..3 {
            assert!((signals[k] - expected.to_array()[k]).abs() < 1e-9, "sample {i}: {signals:?} vs {expected:?}");
        }
    }
    // Silence for longer than the timeout ramps the twist down to zero.
    assert_eq!(expected, BodyTwist::ZERO);

    // An axis's acceleration without its max speed is refused by name.
    let mut bad = parameters_without(&parameters, "max_speed.yaw");
    assert!(DriveLimiter::from_parameters(&bad).unwrap_err().to_string().contains("max_accel.yaw"));
    bad.remove("max_accel.yaw");
    bad.remove("stop_decel.yaw");
    DriveLimiter::from_parameters(&bad).unwrap();
    // A timeout shorter than one period would lose every request.
    let mut short = parameters.clone();
    short.insert("deadman_timeout".into(), 0.01);
    assert!(DriveLimiter::from_parameters(&short).unwrap_err().to_string().contains("deadman_timeout"));
}

fn parameters_without(p: &std::collections::BTreeMap<String, f64>, name: &str) -> std::collections::BTreeMap<String, f64> {
    let mut out = p.clone();
    out.remove(name);
    out
}

/// The controller-side deadman (`HeartbeatDeadman`, simloop's `DriveState`
/// with `limit_live=False`): heartbeat 0 is no request; a rising heartbeat
/// keeps a live request, passed through unchanged; a stale one expires to
/// the stop rule from the last output; NaN or a decrease never refreshes.
#[test]
fn heartbeat_deadman_follows_the_python_drive_state() {
    let limits = wheeled_limits();
    let deadman = Deadman { timeout_s: 0.5, on_loss: OnLoss::Ramp { decel: [1.2, 1.2, 12.0] } };
    let dt = 0.02;
    let request = BodyTwist::new(0.3, 0.0, 1.0);
    let mut d = kinematics::HeartbeatDeadman::default();
    // First sample with heartbeat 0: no request yet, the twist is zero whatever is sent.
    let c = d.update(0.0, request, 0.0, dt, &limits, &deadman).unwrap();
    assert_eq!((c.twist, c.expired, d.heartbeat, d.changed_t), (BodyTwist::ZERO, false, Some(0.0), 0.0));
    // A fresh request passes through unchanged (the host limited it already).
    let c = d.update(0.1, request, 1.0, dt, &limits, &deadman).unwrap();
    assert_eq!((c.twist, c.expired, d.changed_t), (request, false, 0.1));
    // The same heartbeat, a NaN and a decrease do not refresh it.
    d.update(0.2, request, 1.0, dt, &limits, &deadman).unwrap();
    d.update(0.3, request, f64::NAN, dt, &limits, &deadman).unwrap();
    d.update(0.4, request, 0.5, dt, &limits, &deadman).unwrap();
    assert_eq!((d.heartbeat, d.changed_t), (Some(1.0), 0.1));
    // Age 0.5 s since the last rise: expired, ramping from the last output at stop_decel.
    let c = d.update(0.6, request, 1.0, dt, &limits, &deadman).unwrap();
    assert!(c.expired && d.expired);
    assert!((c.twist.forward_m_s - (0.3 - 1.2 * dt)).abs() < 1e-12 && (c.twist.yaw_rad_s - (1.0 - 12.0 * dt)).abs() < 1e-12, "{c:?}");
    // A rise revives it.
    let c = d.update(0.62, request, 2.0, dt, &limits, &deadman).unwrap();
    assert_eq!((c.twist, c.expired), (request, false));
    // A live request outside the profile is refused by name and leaves the state as it was.
    let before = d;
    let e = d.update(0.64, BodyTwist::new(0.0, 0.1, 0.0), 3.0, dt, &limits, &deadman).unwrap_err();
    assert!(e.to_string().contains("lateral"), "{e}");
    assert_eq!(d, before);
    // A non-finite first heartbeat is recorded as 0 (no request yet).
    let mut d = kinematics::HeartbeatDeadman::default();
    let c = d.update(1.0, request, f64::NAN, dt, &limits, &deadman).unwrap();
    assert_eq!((c.twist, d.heartbeat, d.changed_t), (BodyTwist::ZERO, Some(0.0), 1.0));
}
