//! Unexecuted regression fixtures: no child processes or project operations.
use super::*;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

fn fixture() -> (Manifest, Scenario, AdapterRun) {
    let m: Manifest =
        serde_json::from_str(include_str!("../../../../examples/cad-parity/wheeled.json")).unwrap();
    let s = m.scenarios[0].clone();
    let mut run = not_run(reference_identity(), &m, &s, "not dispatched");
    let receipt = &mut run.receipts[0];
    receipt.status = ExecutionStatus::Passed;
    receipt.executed_at = Some("2026-10-02T00:00:00+00:00".into());
    receipt.process_document_id = Some(m.document_id.clone());
    receipt.revision = Some(7);
    receipt.message = "completed authoritative operation".into();
    receipt.observations.insert(
        "operation.outcome".into(),
        Observation {
            owner: "robocad".into(),
            unit: None,
            frame: None,
            provenance: None,
            uncertainty: None,
            value: ObservedValue::Present(serde_json::json!("success")),
        },
    );
    (m, s, run)
}
fn assert_closed(s: &Scenario, run: &AdapterRun) {
    let diagnostics = super::super::compare::compare(s, run, run);
    assert!(
        super::super::gates::aggregate(s, run, run, &diagnostics)
            .iter()
            .all(|gate| !gate.passed)
    );
}
#[cfg(unix)]
fn completion(reason: Option<&str>, cleanup: Option<&str>) -> ProcessCompletion {
    ProcessCompletion {
        exit: Some(std::process::ExitStatus::from_raw(0)),
        interruption: reason.map(str::to_owned),
        cleanup_error: cleanup.map(str::to_owned),
    }
}
#[cfg(unix)]
#[test]
fn cancellation_and_timeout_preserve_completed_and_uncertain_receipts() {
    for reason in ["cancelled", "bounded child wait expired"] {
        let (m, s, mut raw) = fixture();
        raw.receipts[1].status = ExecutionStatus::Uncertain;
        raw.receipts[1].executed_at = Some("2026-10-02T00:00:01+00:00".into());
        raw.receipts[1].revision = Some(8);
        raw.receipts[1].process_document_id = Some(m.document_id.clone());
        raw.receipts[1].message = "mutation response unknown; do not retry".into();
        let recovered = recover_reference(
            Ok(serde_json::to_vec(&raw).unwrap()),
            &completion(Some(reason), None),
            &m,
            &s,
        );
        assert_eq!(recovered.receipts, raw.receipts);
        assert_eq!(recovered.identity, raw.identity);
        assert_eq!(recovered.source, raw.source);
        assert!(
            recovered
                .execution_issues
                .iter()
                .any(|issue| issue.contains(reason))
        );
        assert_closed(&s, &recovered);
    }
}
#[cfg(unix)]
#[test]
fn graceful_partial_publication_keeps_actual_receipts_and_identity() {
    let (m, s, mut raw) = fixture();
    raw.receipts.truncate(1);
    raw.identity.version = "unexpected-original-version".into();
    let recovered = recover_reference(
        Ok(serde_json::to_vec(&raw).unwrap()),
        &completion(Some("cancelled"), Some("descendant cleanup unconfirmed")),
        &m,
        &s,
    );
    assert_eq!(recovered.receipts, raw.receipts);
    assert_eq!(recovered.identity, raw.identity);
    assert_eq!(recovered.receipts.len(), 1); // no manufactured missing receipt
    assert!(
        recovered
            .execution_issues
            .iter()
            .any(|issue| issue.contains("identity mismatch"))
    );
    assert!(
        recovered
            .execution_issues
            .iter()
            .any(|issue| issue.contains("shutdown incomplete"))
    );
    assert_closed(&s, &recovered);
}
#[cfg(unix)]
#[test]
fn absent_and_malformed_output_are_explicit_without_execution_timestamps() {
    let (m, s, _) = fixture();
    for bytes in [Err("owned report absent".into()), Ok(b"{broken".to_vec())] {
        let recovered = recover_reference(bytes, &completion(Some("cancelled"), None), &m, &s);
        assert!(
            recovered
                .receipts
                .iter()
                .all(|receipt| receipt.executed_at.is_none())
        );
        assert!(
            recovered
                .receipts
                .iter()
                .all(|receipt| receipt.status != ExecutionStatus::Passed)
        );
        assert!(
            recovered
                .execution_issues
                .iter()
                .any(|issue| issue.contains("evidence unavailable"))
        );
        assert_closed(&s, &recovered);
    }
}
#[cfg(unix)]
#[test]
fn partial_receipt_count_and_nonzero_exit_never_discard_operation_evidence() {
    let (m, s, mut raw) = fixture();
    raw.receipts.truncate(1);
    let mut result = completion(None, None);
    result.exit = Some(std::process::ExitStatus::from_raw(2 << 8));
    let recovered = recover_reference(Ok(serde_json::to_vec(&raw).unwrap()), &result, &m, &s);
    assert_eq!(recovered.receipts, raw.receipts);
    assert!(
        recovered
            .execution_issues
            .iter()
            .any(|issue| issue.contains("receipt set"))
    );
    assert!(
        recovered
            .execution_issues
            .iter()
            .any(|issue| issue.contains("reference process"))
    );
    assert_closed(&s, &recovered);
}

#[cfg(unix)]
#[test]
fn cleanup_error_and_unavailable_exit_keep_even_complete_operation_records() {
    let (m, s, raw) = fixture();
    let mut result = completion(None, Some("leader reap unavailable"));
    result.exit = None;
    let recovered = recover_reference(Ok(serde_json::to_vec(&raw).unwrap()), &result, &m, &s);
    assert_eq!(recovered.receipts, raw.receipts);
    assert!(
        recovered
            .execution_issues
            .iter()
            .any(|issue| issue.contains("exit unavailable"))
    );
    assert!(
        recovered
            .execution_issues
            .iter()
            .any(|issue| issue.contains("leader reap unavailable"))
    );
    assert_closed(&s, &recovered);
}
