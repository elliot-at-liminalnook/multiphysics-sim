//! Fail-closed evidence aggregation. This is not an acceptance signature or a
//! claim of physical accuracy; current Python/OCCT adapters cannot replace it.
use super::contract::*;
use std::collections::BTreeSet;

fn expected_matches(expected: &ExpectedOutcome, actual: &ExecutionStatus) -> bool {
    match expected {
        // Passed denotes an executed operation satisfying its declared outcome,
        // including a verified rejection, never a transport error assumed refused.
        ExpectedOutcome::Success | ExpectedOutcome::Rejected => {
            matches!(actual, ExecutionStatus::Passed)
        }
        ExpectedOutcome::Cancelled => matches!(actual, ExecutionStatus::Cancelled),
        ExpectedOutcome::Unsupported => matches!(actual, ExecutionStatus::Unsupported),
        ExpectedOutcome::DeliberateDifference => {
            matches!(actual, ExecutionStatus::DeliberateDifference)
        }
    }
}
fn usable(identity: &AdapterIdentity) -> bool {
    [
        &identity.name,
        &identity.version,
        &identity.implementation,
        &identity.kernel,
        &identity.derivation,
    ]
    .iter()
    .all(|s| !s.trim().is_empty())
}
fn digest(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit())
}
fn python_occt(identity: &str) -> bool {
    let id = identity.to_ascii_lowercase();
    id.contains("python")
        || id.contains("occt")
        || id.contains("opencascade")
        || id.contains("robocad")
}
/// Required coverage is evidenced by executed policies, not adapter marketing or
/// fixture files. Every scenario step and comparison must have one paired receipt.
/// The runner additionally validates code/schema/model identity against Manifest.
pub fn aggregate(
    scenario: &Scenario,
    reference: &AdapterRun,
    native: &AdapterRun,
    diagnostics: &[Diagnostic],
) -> Vec<Gate> {
    let mut reasons = Vec::new();
    if super::compare::compare(scenario, reference, native) != diagnostics {
        reasons.push("diagnostics do not equal recomputed observation comparison".into());
    }
    if scenario.id.trim().is_empty() {
        reasons.push("scenario identity absent".into());
    }
    if scenario.operations.is_empty() {
        reasons.push("no operations: vacuous evidence".into());
    }
    if scenario.comparisons.is_empty() {
        reasons.push("no comparison policies: vacuous evidence".into());
    }
    if scenario.required_coverage.is_empty() {
        reasons.push("no required coverage declared".into());
    }
    if reference.source != native.source
        || !digest(&reference.source.sha256)
        || reference.source.path.trim().is_empty()
    {
        reasons.push("durable source identity missing/invalid or mismatched; process identities are not normalized".into());
    }
    let mut steps = BTreeSet::new();
    for step in &scenario.operations {
        if step.id.trim().is_empty() || !steps.insert(step.id.as_str()) {
            reasons.push(format!("duplicate/empty operation ID {}", step.id));
        }
    }
    for (side, run) in [("reference", reference), ("native", native)] {
        for issue in &run.execution_issues {
            reasons.push(format!("{side}: execution incomplete: {issue}"));
        }
        if !usable(&run.identity) {
            reasons.push(format!("{side}: adapter identity incomplete"));
        }
        if run.receipts.len() != scenario.operations.len() {
            reasons.push(format!(
                "{side}: receipt count does not match operation count"
            ));
        }
        if run
            .receipts
            .iter()
            .map(|r| &r.step_id)
            .ne(scenario.operations.iter().map(|s| &s.id))
        {
            reasons.push(format!(
                "{side}: operation receipt order differs from scenario order"
            ));
        }
        let mut received = BTreeSet::new();
        for r in &run.receipts {
            if !received.insert(r.step_id.as_str()) {
                reasons.push(format!("{side}: duplicate receipt {}", r.step_id));
            }
            let Some(step) = scenario.operations.iter().find(|s| s.id == r.step_id) else {
                reasons.push(format!("{side}: unknown receipt {}", r.step_id));
                continue;
            };
            let expected_value = match step.expected {
                ExpectedOutcome::Success => "success",
                ExpectedOutcome::Rejected => "rejected",
                ExpectedOutcome::Cancelled => "cancelled",
                ExpectedOutcome::Unsupported => "unsupported",
                ExpectedOutcome::DeliberateDifference => "deliberate_difference",
            };
            let observed_outcome =
                r.observations
                    .get("operation.outcome")
                    .and_then(|o| match &o.value {
                        ObservedValue::Present(value) => value.as_str(),
                        _ => None,
                    });
            if observed_outcome != Some(expected_value) {
                reasons.push(format!(
                    "{side}/{}: actual operation.outcome absent or differs from declared outcome",
                    r.step_id
                ));
            }
            if r.expected != step.expected || !expected_matches(&step.expected, &r.status) {
                reasons.push(format!(
                    "{side}/{}: expected outcome inconsistent with receipt",
                    r.step_id
                ));
            }
            if !matches!(r.status, ExecutionStatus::Passed) {
                reasons.push(format!(
                    "{side}/{}: non-passing execution {:?}",
                    r.step_id, r.status
                ));
            }
            if r.executed_at.as_deref().is_none_or(|s| s.trim().is_empty()) {
                reasons.push(format!("{side}/{}: executed timestamp absent", r.step_id));
            }
            if r.process_document_id
                .as_deref()
                .is_none_or(|s| s.trim().is_empty())
                || r.revision.is_none()
            {
                reasons.push(format!(
                    "{side}/{}: process document identity/revision absent",
                    r.step_id
                ));
            }
        }
        for step in &scenario.operations {
            if !received.contains(step.id.as_str()) {
                reasons.push(format!("{side}/{}: execution absent", step.id));
            }
        }
    }
    for step in &scenario.operations {
        let a = reference.receipts.iter().find(|r| r.step_id == step.id);
        let b = native.receipts.iter().find(|r| r.step_id == step.id);
        if let (Some(a), Some(b)) = (a, b) {
            if a.process_document_id != b.process_document_id {
                reasons.push(format!(
                    "{}: paired process document identities differ",
                    step.id
                ));
            }
        }
    }
    let mut policies = BTreeSet::new();
    for p in &scenario.comparisons {
        if !steps.contains(p.step_id.as_str())
            || p.path.trim().is_empty()
            || p.family.trim().is_empty()
            || !policies.insert((p.step_id.as_str(), p.path.as_str()))
        {
            reasons.push(format!(
                "{}/{}: unknown step, empty field/family, or duplicate policy",
                p.step_id, p.path
            ));
        }
        if p.deliberate_difference.is_some() {
            reasons.push(format!(
                "{}/{}: unresolved declared deliberate difference",
                p.step_id, p.path
            ));
        }
        let ds: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.step_id == p.step_id
                    && d.family == p.family
                    && (d.path == p.path || d.path.starts_with(&format!("{}/", p.path)))
            })
            .collect();
        if ds.is_empty() {
            reasons.push(format!(
                "{}/{}: comparison evidence absent",
                p.step_id, p.path
            ));
        }
        for d in ds {
            if !matches!(d.status, ExecutionStatus::Passed) {
                reasons.push(format!(
                    "{}/{}: {:?}: {}",
                    d.step_id, d.path, d.status, d.message
                ));
            }
        }
        if matches!(p.metric, Metric::PointSet { .. })
            && !scenario.comparisons.iter().any(|topology| {
                topology.step_id == p.step_id
                    && topology.family == "topology"
                    && matches!(topology.metric, Metric::Exact)
            })
        {
            reasons.push(format!(
                "{}/{}: point-set evidence requires separate exact topology policy",
                p.step_id, p.path
            ));
        }
    }
    for d in diagnostics {
        if !matches!(d.status, ExecutionStatus::Passed)
            && !reasons
                .iter()
                .any(|r| r.contains(&format!("{}/{}:", d.step_id, d.path)))
        {
            reasons.push(format!(
                "{}/{}: non-passing diagnostic {:?}",
                d.step_id, d.path, d.status
            ));
        }
        if !scenario.comparisons.iter().any(|p| {
            d.step_id == p.step_id
                && d.family == p.family
                && (d.path == p.path || d.path.starts_with(&format!("{}/", p.path)))
        }) {
            reasons.push(format!(
                "{}/{}: diagnostic has no comparison policy",
                d.step_id, d.path
            ));
        }
    }
    for family in &scenario.required_coverage {
        if family.trim().is_empty() || !scenario.comparisons.iter().any(|p| p.family == *family) {
            reasons.push(format!("required family {family}: policy/evidence absent"));
        }
    }
    let service = Gate {
        kind: GateKind::ServiceContract,
        passed: reasons.is_empty(),
        reasons: reasons.clone(),
    };
    let mut derivation = reasons.clone();
    if !native.identity.independent
        || native.identity.implementation == reference.identity.implementation
        || native.identity.derivation == reference.identity.derivation
        || python_occt(&native.identity.derivation)
    {
        derivation.push("independent numerical derivation gate refused: shared or undeclared reference implementation/derivation authority".into());
    }
    let mut kernel = reasons;
    if !native.identity.independent
        || native.identity.implementation == reference.identity.implementation
        || native.identity.kernel == reference.identity.kernel
        || python_occt(&native.identity.kernel)
    {
        kernel.push(
            "independent kernel gate refused: shared or undeclared Python/OCCT authority".into(),
        );
    }
    vec![
        service,
        Gate {
            kind: GateKind::IndependentDerivation,
            passed: derivation.is_empty(),
            reasons: derivation,
        },
        Gate {
            kind: GateKind::IndependentKernel,
            passed: kernel.is_empty(),
            reasons: kernel,
        },
    ]
}
#[cfg(test)]
#[path = "gate_fixtures.rs"]
mod fixtures;
