use serde_json::json;
use sim_runtime::geometry_evaluation::*;
fn request() -> Request {
    let hash = "a".repeat(64);
    Request {
        expected_capture_blake3: hash.clone(),
        expected_duration_s: 2.,
        expected_frame_times_s: vec![0., 0.5, 1., 1.5, 2.],
        gates: Gates {
            feet: vec!["foot".into()],
            after_startup_s: 0.,
            minimum_clearance_m: 0.003,
            minimum_excursions: 2,
            maximum_inter_link_penetration_m: 0.001,
        },
        audit: json!({"capture_blake3":hash,"capture_completed":true,"capture_error":null,"frames":[0.,0.004,0.,0.006,0.].iter().enumerate().map(|(i,gap)|json!({"time_s":i as f64*0.5,"maximum_inter_link_penetration_m":0.,"floor_clearances":[{"link":"foot","surface_samples":8,"minimum_clearance_m":gap}]})).collect::<Vec<_>>()}),
    }
}
#[test]
fn repeated_lift_gate_counts_crossings_and_rejects_dragging() {
    let r = request();
    let a = evaluate(&r).unwrap();
    assert!(a.passed);
    assert_eq!(a.feet["foot"].excursions, 2);
    let mut r = r.clone();
    r.audit["frames"][3]["floor_clearances"][0]["minimum_clearance_m"] = json!(0.002);
    assert!(!evaluate(&r).unwrap().passed);
    r.audit["frames"][0]["floor_clearances"][0]["minimum_clearance_m"] = json!(0.004);
    assert_eq!(evaluate(&r).unwrap().feet["foot"].excursions, 0);
}
#[test]
fn fails_closed_on_partial_mismatched_missing_and_penetrating_audits() {
    let mut r = request();
    r.audit["capture_completed"] = json!(false);
    assert!(!evaluate(&r).unwrap().passed);
    let mut r = request();
    r.expected_duration_s = 3.;
    assert!(!evaluate(&r).unwrap().passed);
    let mut r = request();
    r.audit["capture_blake3"] = json!("b".repeat(64));
    assert!(evaluate(&r).is_err());
    let mut r = request();
    r.audit["frames"][1]["floor_clearances"] = json!([]);
    assert!(evaluate(&r).is_err());
    let mut r = request();
    r.audit["frames"][0]["maximum_inter_link_penetration_m"] = json!(0.002);
    assert!(!evaluate(&r).unwrap().passed);
    let mut r = request();
    r.audit["frames"].as_array_mut().unwrap().remove(2);
    assert!(evaluate(&r).is_err());
    let mut r = request();
    r.audit["frames"][2]["time_s"] = json!(1.1);
    assert!(evaluate(&r).is_err());
}
