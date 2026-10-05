//! The Rhai drive functions against the shared golden vectors (the file the
//! Rust kinematics and simloop's Python check themselves against), their
//! refusals by function and argument, and the controller-side deadman.
//! Written by reading; not yet executed.
use crate::{Sources, engine, parameter_map};
use rhai::{Array, Dynamic, Map, Scope};
use serde_json::{Value, json};

const GOLDEN: &str = include_str!("../../../sim-domain-control/tests/fixtures/drive_golden.json");

fn array(values: &Value) -> Dynamic {
    Dynamic::from_array(values.as_array().unwrap().iter().map(|v| Dynamic::from_float(v.as_f64().unwrap())).collect())
}
fn floats(result: Array) -> Vec<f64> {
    result.into_iter().map(|v| v.as_float().unwrap()).collect()
}
/// Evaluate `script` with the named Dynamic variables in scope.
fn eval<T: Clone + Send + Sync + 'static>(script: &str, vars: Vec<(&str, Dynamic)>) -> Result<T, String> {
    let engine = engine(Sources::default(), Map::new(), 0);
    let mut scope = Scope::new();
    for (name, value) in vars {
        scope.push_dynamic(name.to_string(), value);
    }
    engine.eval_with_scope::<T>(&mut scope, script).map_err(|e| e.to_string())
}

#[test]
fn rhai_mixers_match_the_golden_vectors() {
    let golden: Value = serde_json::from_str(GOLDEN).unwrap();
    let tol = golden["tolerance"].as_f64().unwrap();
    let mut count = 0;
    for case in golden["differential"].as_array().unwrap() {
        let vars = vec![
            ("w", Dynamic::from_float(case["track_width_m"].as_f64().unwrap())),
            ("r", Dynamic::from_float(case["wheel_radius_m"].as_f64().unwrap())),
            ("s", array(&case["signs"])),
            ("tw", array(&case["twist"])),
        ];
        let got = eval::<Array>("drive_differential_mix(w, r, s, tw)", vars.clone());
        match (got, case.get("wheels")) {
            (Ok(rates), Some(want)) => {
                let (rates, want) = (floats(rates), want.as_array().unwrap());
                assert_eq!(rates.len(), 2, "{}", case["name"]);
                for i in 0..2 {
                    assert!((rates[i] - want[i].as_f64().unwrap()).abs() <= tol, "{}: {rates:?} != {want:?}", case["name"]);
                }
                // Unmixing the rates gives the twist back (a differential drive has no lateral).
                let back = floats(eval::<Array>("drive_differential_unmix(w, r, s, drive_differential_mix(w, r, s, tw))", vars).unwrap());
                let twist: Vec<f64> = case["twist"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
                for i in 0..3 {
                    assert!((back[i] - twist[i]).abs() < 1e-12, "{}: unmix {back:?} != {twist:?}", case["name"]);
                }
            }
            (Err(e), None) => assert!(e.contains("drive_differential_mix: "), "{}: {e}", case["name"]),
            (got, want) => panic!("{}: got {got:?}, golden {want:?}", case["name"]),
        }
        count += 1;
    }
    for case in golden["mecanum"].as_array().unwrap() {
        let vars = vec![
            ("w", Dynamic::from_float(case["track_width_m"].as_f64().unwrap())),
            ("b", Dynamic::from_float(case["wheelbase_m"].as_f64().unwrap())),
            ("r", Dynamic::from_float(case["wheel_radius_m"].as_f64().unwrap())),
            ("s", array(&case["signs"])),
            ("tw", array(&case["twist"])),
        ];
        let rates = floats(eval::<Array>("drive_mecanum_mix(w, b, r, s, tw)", vars.clone()).unwrap());
        let want = case["wheels"].as_array().unwrap();
        for i in 0..4 {
            assert!((rates[i] - want[i].as_f64().unwrap()).abs() <= tol, "{}: {rates:?} != {want:?}", case["name"]);
        }
        let back = floats(eval::<Array>("drive_mecanum_unmix(w, b, r, s, drive_mecanum_mix(w, b, r, s, tw))", vars).unwrap());
        let twist: Vec<f64> = case["twist"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        for i in 0..3 {
            assert!((back[i] - twist[i]).abs() < 1e-12, "{}: unmix {back:?} != {twist:?}", case["name"]);
        }
        count += 1;
    }
    assert!(count >= 10, "only {count} golden mixer cases");
}

#[test]
fn integer_literals_are_numbers() {
    let rates = floats(eval::<Array>("drive_differential_mix(0.12, 0.03, [1, 1], [0.3, 0, 0])", vec![]).unwrap());
    assert!((rates[0] - 10.0).abs() < 1e-12 && (rates[1] - 10.0).abs() < 1e-12, "{rates:?}");
    let rates = floats(eval::<Array>("drive_differential_mix(1, 1, [1, -1], [1, 0, 0])", vec![]).unwrap());
    assert_eq!(rates, vec![1.0, -1.0]);
}

#[test]
fn refusals_name_the_function_and_the_argument() {
    let e = eval::<Array>("drive_differential_mix(0.12, 0.03, [1, 1, 1], [0.3, 0, 0])", vec![]).unwrap_err();
    assert!(e.contains("drive_differential_mix: signs must be 2 numbers (+1 or -1), got 3"), "{e}");
    let e = eval::<Array>("drive_differential_mix(0.12, 0.03, [1, 1], [0.3, 0.2, 0])", vec![]).unwrap_err();
    assert!(e.contains("drive_differential_mix: twist: axis `lateral` = 0.2 is not supported by this drive"), "{e}");
    let e = eval::<Array>("drive_differential_mix(\"wide\", 0.03, [1, 1], [0.3, 0, 0])", vec![]).unwrap_err();
    assert!(e.contains("drive_differential_mix: track_width_m must be a number"), "{e}");
    let e = eval::<Array>("drive_differential_mix(0.12, 0.03, [1, 0.5], [0.3, 0, 0])", vec![]).unwrap_err();
    assert!(e.contains("drive_differential_mix: signs[1]"), "{e}");
    let e = eval::<Array>("drive_differential_mix(0.12, -0.03, [1, 1], [0.3, 0, 0])", vec![]).unwrap_err();
    assert!(e.contains("drive_differential_mix: wheel_radius_m"), "{e}");
    let e = eval::<Array>("drive_differential_unmix(0.12, 0.03, [1, 1], [1])", vec![]).unwrap_err();
    assert!(e.contains("drive_differential_unmix: rates must be 2 numbers"), "{e}");
    let e = eval::<Array>("drive_mecanum_mix(0.2, 0.15, 0.03, [1, 1], [0.3, 0, 0])", vec![]).unwrap_err();
    assert!(e.contains("drive_mecanum_mix: signs must be 4 numbers (+1 or -1), got 2"), "{e}");
    let e = eval::<Array>("drive_mecanum_mix(0.2, 0.15, 0.03, [1, 1, 1, 1], [0.3, 0])", vec![]).unwrap_err();
    assert!(e.contains("drive_mecanum_mix: twist must be 3 numbers"), "{e}");
}

fn resolved_drive() -> Dynamic {
    let declared = json!({"kind": "declared", "source": "test"});
    let drive = json!({
        "schema": "sim.drive.resolved/1", "profile": "test.drive.json", "profile_sha256": "0",
        "kinematics": "differential",
        "geometry": {
            "track_width_m": {"value": 0.12, "provenance": declared},
            "wheelbase_m": null,
            "wheel_radius_m": {"value": 0.03, "provenance": declared},
            "wheels": [{"joint": "left axle", "sign": 1, "provenance": declared}, {"joint": "right axle", "sign": 1, "provenance": declared}]
        },
        "limits": {"supported": [true, false, true], "max_speed": [0.3, 0, 3], "max_accel": [0.6, 0.6, 6], "stop_decel": [1.2, 1.2, 12]},
        "deadman": {"timeout_s": 0.5, "on_loss": "ramp"}
    });
    let parameters = parameter_map(&json!({"drive": drive})).unwrap();
    parameters["drive"].clone()
}

#[test]
fn drive_update_is_the_shared_heartbeat_deadman() {
    let script = r#"
        let a = drive_update(#{}, 0, [0.3, 0, 1], 1, 0.02, drive);
        let b = drive_update(a.state, 0.3, [0.3, 0, 1], 1, 0.02, drive);
        let c = drive_update(b.state, 0.5, [0.3, 0, 1], 1, 0.02, drive);
        let d = drive_update(c.state, 0.52, [0.2, 0, 0], 2, 0.02, drive);
        [a, b, c, d]
    "#;
    let out = eval::<Array>(script, vec![("drive", resolved_drive())]).unwrap();
    let out: Value = rhai::serde::from_dynamic(&Dynamic::from_array(out)).unwrap();
    // Live: the request as sent; the state carries the heartbeat and its time.
    assert_eq!(out[0]["twist"], json!([0.3, 0.0, 1.0]));
    assert_eq!(out[0]["expired"], json!(false));
    assert_eq!(out[0]["state"]["heartbeat"], json!(1.0));
    assert_eq!(out[0]["state"]["changed_t"], json!(0.0));
    assert_eq!(out[1]["expired"], json!(false));
    // 0.5 s without a rise: expired, ramping from the last output at stop_decel.
    assert_eq!(out[2]["expired"], json!(true));
    let t = &out[2]["twist"];
    assert!((t[0].as_f64().unwrap() - (0.3 - 1.2 * 0.02)).abs() < 1e-12 && (t[2].as_f64().unwrap() - (1.0 - 12.0 * 0.02)).abs() < 1e-12, "{t}");
    // A rise revives it.
    assert_eq!(out[3]["twist"], json!([0.2, 0.0, 0.0]));
    assert_eq!(out[3]["expired"], json!(false));
    // Before any request (heartbeat 0) the twist is zero.
    let zero = eval::<Map>("drive_update((), 0.0, [0.3, 0, 1], 0, 0.02, drive)", vec![("drive", resolved_drive())]).unwrap();
    let zero: Value = rhai::serde::from_dynamic(&Dynamic::from_map(zero)).unwrap();
    assert_eq!(zero["twist"], json!([0.0, 0.0, 0.0]));
    assert_eq!(zero["state"]["heartbeat"], json!(0.0));
}

#[test]
fn drive_update_refusals_name_the_argument() {
    let e = eval::<Map>("drive_update(#{}, 0, [0, 0.1, 0], 1, 0.02, drive)", vec![("drive", resolved_drive())]).unwrap_err();
    assert!(e.contains("drive_update: request: axis `lateral`"), "{e}");
    let e = eval::<Map>("drive_update(#{}, 0, [0, 0, 0], 1, 0.02, #{kinematics: \"differential\"})", vec![]).unwrap_err();
    assert!(e.contains("drive_update: drive: "), "{e}");
    let e = eval::<Map>("drive_update(#{twist: [0, 0, 0]}, 0, [0, 0, 0], 1, 0.02, drive)", vec![("drive", resolved_drive())]).unwrap_err();
    assert!(e.contains("drive_update: state.heartbeat: missing"), "{e}");
    let e = eval::<Map>("drive_update(#{}, 0, [0, 0], 1, 0.02, drive)", vec![("drive", resolved_drive())]).unwrap_err();
    assert!(e.contains("drive_update: request must be 3 numbers"), "{e}");
}
