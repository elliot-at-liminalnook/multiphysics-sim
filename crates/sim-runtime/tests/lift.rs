use sim_runtime::lift::*;
use std::collections::BTreeMap;

fn fixture() -> (LiftRequirements, Vec<LiftSample>) {
    let r = LiftRequirements {
        support_check: Default::default(),
        start_s: 0.0,
        end_s: 0.4,
        maximum_sample_gap_s: 0.1,
        qualifying_duration_s: 0.2,
        swing_link: "swing".into(),
        minimum_clearance_m: 0.001,
        maximum_swing_force_n: 0.1,
        minimum_support_forces_n: BTreeMap::from([("support".into(), 1.0)]),
    };
    let samples = (0..5)
        .map(|i| LiftSample {
            time_s: i as f64 * 0.1,
            swing_clearance_m: 0.002,
            floor_forces_n: BTreeMap::from([("swing".into(), 0.0), ("support".into(), 2.0)]),
        })
        .collect();
    (r, samples)
}

#[test]
fn clearance_only_scope_is_explicit_and_cannot_claim_support() {
    let (mut r,mut samples)=fixture();
    r.minimum_support_forces_n.clear();
    assert!(evaluate_lift(&samples,&r).is_err());
    r.support_check=SupportCheck::ClearanceOnly;
    for s in &mut samples {s.floor_forces_n.remove("support");}
    let result=evaluate_lift(&samples,&r).unwrap();
    assert!(result.passed);
    assert!(result.scope.contains("Support and body balance are unassessed"));
    r.minimum_support_forces_n.insert("support".into(),1.0);
    assert!(evaluate_lift(&samples,&r).is_err());
}

#[test]
fn lift_requires_simultaneous_conditions_and_consecutive_duration() {
    let (r, mut samples) = fixture();
    let result = evaluate_lift(&samples, &r).unwrap();
    assert!(result.passed);
    assert!((result.longest_qualifying_span_s - 0.4).abs() < 1e-12);
    samples[2].floor_forces_n.insert("support".into(), 0.5);
    let result = evaluate_lift(&samples, &r).unwrap();
    assert!(!result.passed);
    assert!((result.longest_qualifying_span_s - 0.1).abs() < 1e-12);
    assert_eq!(result.failed_support_samples, 1);
    // All three conditions pass somewhere, but never simultaneously.
    for (i, s) in samples.iter_mut().enumerate() {
        s.swing_clearance_m = if i % 2 == 0 { 0.002 } else { 0.0 };
        s.floor_forces_n
            .insert("support".into(), if i % 2 == 0 { 0.5 } else { 2.0 });
    }
    assert_eq!(
        evaluate_lift(&samples, &r)
            .unwrap()
            .longest_qualifying_span_s,
        0.0
    );
}

#[test]
fn incomplete_or_invalid_evidence_cannot_pass_lift_checks() {
    let (r, samples) = fixture();
    for bad in [
        samples[1..].to_vec(),
        samples[..4].to_vec(),
        vec![samples[0].clone(), samples[4].clone()],
    ] {
        assert!(evaluate_lift(&bad, &r).is_err());
    }
    let mut bad = samples.clone();
    bad[2].floor_forces_n.remove("support");
    assert!(evaluate_lift(&bad, &r).is_err());
    let mut bad = samples.clone();
    bad[2].time_s = bad[1].time_s;
    assert!(evaluate_lift(&bad, &r).is_err());
    let mut bad = samples.clone();
    bad[2].swing_clearance_m = f64::NAN;
    assert!(evaluate_lift(&bad, &r).is_err());
    let mut bad = r.clone();
    bad.qualifying_duration_s = 0.0;
    assert!(evaluate_lift(&samples, &bad).is_err());
    let mut tiny = r.clone();
    tiny.qualifying_duration_s = 1e-12;
    let mut blocked = samples.clone();
    for s in &mut blocked {
        s.swing_clearance_m = 0.0;
    }
    assert!(!evaluate_lift(&blocked, &tiny).unwrap().passed);
    let mut bad = r.clone();
    bad.minimum_support_forces_n.insert("swing".into(), 1.0);
    assert!(evaluate_lift(&samples, &bad).is_err());
    let mut bad = samples;
    bad[2].floor_forces_n.insert("swing".into(), -10.0);
    assert!(!evaluate_lift(&bad, &r).unwrap().passed);
}
