//! Written source-review fixtures; not executed in T44.
use super::*;
use serde_json::json;
use std::collections::BTreeMap;
fn policy(metric: Metric) -> ComparisonPolicy {
    ComparisonPolicy {
        owners: None,
        step_id: "observe".into(),
        path: "mass".into(),
        family: "mass_inertia".into(),
        unit: Some("kg".into()),
        frame: Some("model".into()),
        require_provenance: true,
        metric,
        deliberate_difference: None,
    }
}
fn tolerance(absolute: f64, relative: f64) -> Tolerance {
    Tolerance {
        absolute,
        relative,
        justification:
            "fixture exercises exact binary boundary, not corpus numerical qualification".into(),
    }
}
fn sample(value: Value, metric: Metric) -> (Scenario, AdapterRun) {
    let scenario = Scenario {
        id: "fixture".into(),
        operations: vec![Step {
            id: "observe".into(),
            operation: Operation::Observe,
            expected: ExpectedOutcome::Success,
        }],
        comparisons: vec![policy(metric)],
        required_coverage: vec!["mass_inertia".into()],
    };
    let observation = Observation {
        owner: "Python/OCCT".into(),
        unit: Some("kg".into()),
        frame: Some("model".into()),
        provenance: Some("derived: OCCT volume*density".into()),
        uncertainty: Some(json!({"state":"unknown"})),
        value: ObservedValue::Present(value),
    };
    let run = AdapterRun {
        identity: AdapterIdentity {
            name: "reference".into(),
            version: "1".into(),
            implementation: "Ops".into(),
            kernel: "Python/OCCT".into(),
            derivation: "Python physical".into(),
            independent: false,
        },
        source: SourceFile {
            path: "model.rcad".into(),
            sha256: "a".repeat(64),
        },
        receipts: vec![Receipt {
            step_id: "observe".into(),
            status: ExecutionStatus::Passed,
            expected: ExpectedOutcome::Success,
            message: "executed fixture input".into(),
            process_document_id: Some("durable-doc".into()),
            revision: Some(0),
            observations: BTreeMap::from([("mass".into(), observation)]),
            executed_at: Some("2000-01-01T00:00:00Z".into()),
        }],
    };
    (scenario, run)
}
fn observation(run: &mut AdapterRun) -> &mut Observation {
    run.receipts[0].observations.get_mut("mass").unwrap()
}
#[test]
fn zero_and_inclusive_binary_boundary() {
    let t = tolerance(0.125, 0.25);
    assert!(numeric(0.0, 0.125, &t).0);
    assert!(!numeric(0.0, f64::from_bits(0.125f64.to_bits() + 1), &t).0);
    assert_eq!(numeric(0.0, 0.0, &t).2, Some(0.0));
    assert_eq!(numeric(0.0, 0.125, &t).2, None);
    assert!(numeric(1.0, 1.375, &t).0);
    assert!(!numeric(1.0, f64::from_bits(1.375f64.to_bits() + 1), &t).0);
}
#[test]
fn finite_extreme_subtraction_does_not_manufacture_zero() {
    assert!(!numeric(-f64::MAX, f64::MAX, &tolerance(0.0, 1.0)).0);
    assert!(numeric(-f64::MAX, f64::MAX, &tolerance(0.0, 2.0)).0);
    assert_eq!(numeric(-f64::MAX, f64::MAX, &tolerance(0.0, 2.0)).1, None);
}
#[test]
fn metadata_mismatch_and_missing_provenance_name_paths() {
    let (scenario, reference) = sample(json!(2), Metric::Exact);
    let mut native = reference.clone();
    observation(&mut native).unit = Some("g".into());
    observation(&mut native).provenance = None;
    let ds = compare(&scenario, &reference, &native);
    assert!(
        ds.iter()
            .any(|d| d.path == "mass/@unit" && d.status == ExecutionStatus::Failed)
    );
    assert!(
        ds.iter()
            .any(|d| d.path == "mass/@provenance" && d.status == ExecutionStatus::Failed)
    );
}
#[test]
fn missing_unsupported_and_invalid_are_distinct() {
    let (scenario, reference) = sample(json!(2), Metric::Exact);
    for (value, status) in [
        (
            ObservedValue::Missing("density".into()),
            ExecutionStatus::Incomplete,
        ),
        (
            ObservedValue::Unsupported("B-rep owner unavailable".into()),
            ExecutionStatus::Unsupported,
        ),
        (
            ObservedValue::Invalid("NaN inertia".into()),
            ExecutionStatus::Failed,
        ),
    ] {
        let mut native = reference.clone();
        observation(&mut native).value = value;
        assert_eq!(compare(&scenario, &reference, &native)[0].status, status);
    }
    // JSON cannot represent non-finite numbers: they must cross the wire as Invalid.
    assert!(serde_json::from_str::<Value>("NaN").is_err());
}
#[test]
fn exact_large_integers_do_not_round_to_same_f64() {
    let (scenario, reference) = sample(json!(9007199254740992u64), Metric::Exact);
    let mut native = reference.clone();
    observation(&mut native).value = ObservedValue::Present(json!(9007199254740993u64));
    assert_eq!(
        compare(&scenario, &reference, &native)[0].status,
        ExecutionStatus::Failed
    );
}
#[test]
fn recursive_numeric_errors_name_array_and_escaped_object_field() {
    let (scenario, reference) = sample(
        json!({"a/b":[1.0,2.0]}),
        Metric::Numeric {
            tolerance: tolerance(0.0, 0.0),
        },
    );
    let mut native = reference.clone();
    observation(&mut native).value = ObservedValue::Present(json!({"a/b":[1.0,3.0]}));
    let d = compare(&scenario, &reference, &native)
        .into_iter()
        .find(|d| d.status == ExecutionStatus::Failed)
        .unwrap();
    assert_eq!(d.path, "mass/a~1b/1");
    assert_eq!(d.absolute_error, Some(1.0));
    assert_eq!(d.unit.as_deref(), Some("kg"));
}
#[test]
fn point_sets_ignore_order_but_never_claim_topology_or_empty_agreement() {
    let metric = Metric::PointSet {
        tolerance: tolerance(0.0, 0.0),
        max_points: 10,
    };
    let (scenario, reference) = sample(json!([[0, 0, 0], [1, 0, 0]]), metric);
    let mut native = reference.clone();
    observation(&mut native).value = ObservedValue::Present(json!([[1, 0, 0], [0, 0, 0]]));
    assert_eq!(
        compare(&scenario, &reference, &native)[0].status,
        ExecutionStatus::Passed
    );
    observation(&mut native).value = ObservedValue::Present(json!([]));
    assert_eq!(
        compare(&scenario, &reference, &native)[0].status,
        ExecutionStatus::Failed
    );
}

#[test]
fn numeric_large_integer_zero_tolerance_never_rounds_to_pass() {
    let (scenario, reference) = sample(
        json!(9007199254740992u64),
        Metric::Numeric {
            tolerance: tolerance(0.0, 0.0),
        },
    );
    let mut native = reference.clone();
    observation(&mut native).value = ObservedValue::Present(json!(9007199254740993u64));
    assert_eq!(
        compare(&scenario, &reference, &native)[0].status,
        ExecutionStatus::Failed
    );
}

#[test]
fn point_coordinates_never_round_distinct_large_integers_to_zero_distance() {
    let (scenario, reference) = sample(
        json!([[9007199254740992u64, 0, 0]]),
        Metric::PointSet {
            tolerance: tolerance(0.0, 0.0),
            max_points: 10,
        },
    );
    let mut native = reference.clone();
    observation(&mut native).value = ObservedValue::Present(json!([[9007199254740993u64, 0, 0]]));
    assert_eq!(
        compare(&scenario, &reference, &native)[0].status,
        ExecutionStatus::Failed
    );
}

#[test]
fn independent_observation_owners_require_explicit_policy_without_normalization() {
    let (mut scenario, reference) = sample(json!(2), Metric::Exact);
    let mut native = reference.clone();
    observation(&mut native).owner = "rust.physical".into();
    assert!(
        compare(&scenario, &reference, &native)
            .iter()
            .any(|d| d.status == ExecutionStatus::Failed)
    );
    scenario.comparisons[0].owners = Some(ObservationOwners {
        reference: "Python/OCCT".into(),
        native: "rust.physical".into(),
    });
    assert_eq!(
        compare(&scenario, &reference, &native)[0].status,
        ExecutionStatus::Passed
    );
    assert_eq!(observation(&mut native).owner, "rust.physical");
}
