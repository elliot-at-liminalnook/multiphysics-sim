//! Written source-review fixtures; not executed in T44.
use super::*;
use serde_json::json;
fn inputs() -> (Scenario, AdapterRun) {
    let scenario: Scenario = serde_json::from_value(json!({"id":"review-only", "operations":[
        {"id":"read","operation":{"kind":"observe"},"expected":"success"}],
        "comparisons":[{"step_id":"read","path":"identity","family":"identity","unit":null,"frame":null,
            "require_provenance":true,"metric":{"kind":"exact"},"deliberate_difference":null}], "required_coverage":["identity"]})).unwrap();
    let run = serde_json::from_value(json!({"identity":{"name":"reference","version":"1","implementation":"Ops",
        "kernel":"Python/OCCT","derivation":"Python physical","independent":false},
        "source":{"path":"real.rcad","sha256":"a".repeat(64)},"receipts":[{"step_id":"read","status":"passed",
        "expected":"success","message":"fixture input, not execution receipt","process_document_id":"doc","revision":0,
        "observations":{"operation.outcome":{"owner":"robocad.command","unit":null,"frame":null,"provenance":"command",
            "uncertainty":null,"value":{"state":"present","value":"success"}},"identity":{"owner":"document","unit":null,"frame":null,"provenance":"source",
            "uncertainty":{"state":"unknown"},"value":{"state":"present","value":"doc"}}},"executed_at":"2000-01-01T00:00:00Z"}]})).unwrap();
    (scenario, run)
}
fn gates(s: &Scenario, a: &AdapterRun, b: &AdapterRun) -> Vec<Gate> {
    aggregate(s, a, b, &crate::cad_parity::compare::compare(s, a, b))
}
#[test]
fn shared_authority_cannot_qualify_independent_replacements() {
    let (s, a) = inputs();
    let mut b = a.clone();
    b.identity.implementation = "Rust typed client".into();
    let g = gates(&s, &a, &b);
    assert!(g[0].passed);
    assert!(!g[1].passed);
    assert!(!g[2].passed);
    b.identity.independent = true;
    assert!(!gates(&s, &a, &b)[2].passed);
}
#[test]
fn absent_execution_identity_and_coverage_fail_closed() {
    let (mut s, a) = inputs();
    let mut b = a.clone();
    b.receipts[0].executed_at = None;
    assert!(!gates(&s, &a, &b)[0].passed);
    b = a.clone();
    b.source.sha256 = "b".repeat(64);
    assert!(!gates(&s, &a, &b)[0].passed);
    b = a.clone();
    b.receipts[0].process_document_id = Some("replacement".into());
    assert!(!gates(&s, &a, &b)[0].passed);
    s.required_coverage.push("topology".into());
    assert!(!gates(&s, &a, &a)[0].passed);
}
#[test]
fn not_run_uncertain_and_declared_differences_are_not_passes() {
    let (mut s, a) = inputs();
    for status in [
        ExecutionStatus::NotRun,
        ExecutionStatus::Uncertain,
        ExecutionStatus::Incomplete,
        ExecutionStatus::Cancelled,
        ExecutionStatus::Unsupported,
        ExecutionStatus::DeliberateDifference,
        ExecutionStatus::Failed,
    ] {
        let mut b = a.clone();
        b.receipts[0].status = status;
        assert!(!gates(&s, &a, &b)[0].passed);
    }
    s.comparisons[0].deliberate_difference = Some("undo cadence retained from ledger".into());
    assert!(!gates(&s, &a, &a)[0].passed);
}
#[test]
fn empty_duplicate_and_fabricated_evidence_never_passes() {
    let (mut s, a) = inputs();
    assert!(!aggregate(&s, &a, &a, &[])[0].passed);
    let mut b = a.clone();
    b.receipts[0].observations.clear();
    let fabricated = crate::cad_parity::compare::compare(&s, &a, &a);
    assert!(!aggregate(&s, &a, &b, &fabricated)[0].passed);
    s.operations.push(s.operations[0].clone());
    assert!(!gates(&s, &a, &a)[0].passed);
    s.operations.clear();
    s.comparisons.clear();
    s.required_coverage.clear();
    let mut empty = a;
    empty.receipts.clear();
    assert!(!gates(&s, &empty, &empty)[0].passed);
}
#[test]
fn rejection_receipt_requires_the_declared_outcome() {
    let (mut s, mut a) = inputs();
    s.operations[0].expected = ExpectedOutcome::Rejected;
    assert!(!gates(&s, &a, &a)[0].passed);
    a.receipts[0].expected = ExpectedOutcome::Rejected;
    a.receipts[0]
        .observations
        .get_mut("operation.outcome")
        .unwrap()
        .value = ObservedValue::Present(json!("rejected"));
    assert!(gates(&s, &a, &a)[0].passed);
}

#[test]
fn shutdown_issue_blocks_even_complete_passing_receipts_without_rewriting_them() {
    let (s, a) = inputs();
    let mut interrupted = a.clone();
    interrupted
        .execution_issues
        .push("timeout; descendant cleanup uncertain".into());
    assert_eq!(interrupted.receipts, a.receipts);
    let results = gates(&s, &interrupted, &a);
    assert!(results.iter().all(|g| !g.passed));
    assert!(
        results[0]
            .reasons
            .iter()
            .any(|r| r.contains("cleanup uncertain"))
    );
    let bytes = serde_json::to_vec(&interrupted).unwrap();
    let decoded: AdapterRun = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded, interrupted);
}
