//! Written regression fixtures; not executed in T43.
use super::source::{CadAnchor, CadThreadSource, Request};
use crate::annotations::{self, ThreadOp};
use crate::app::actions::{Call, Origin, Replies};
use crate::cad::{CadDocument, CadTarget};
use serde_json::{Value, json};

fn evidence() -> Value {
    json!({"run_id":"capture-1","time_range":[0.25,0.25],
        "physical_hash":"captured-sha256","signal":"joint.theta",
        "source":{"path":"controller.rhai","line":8,"column":2},
        "node_ids":["captured-node"]})
}
#[test]
fn captured_evidence_cannot_send_into_an_unconnected_document() {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut call = Call {
        origin: Origin::Ui,
        continuation: &mut continuation,
        cancelled: false,
        replies: &mut replies,
    };
    let mut source = CadThreadSource::new(&mut doc, &mut call, Some(4));
    let result = annotations::apply(
        &mut source,
        "Add annotation",
        ThreadOp::Create {
            title: "Experiment evidence".into(),
            targets: vec![CadAnchor::Evidence {
                evidence: evidence(),
            }],
            body: "Captured current overshoot at this sample".into(),
            author: "You".into(),
            links: vec![],
            pin_m: None,
            view: None,
        },
    );
    assert!(result.is_err());
    // Missing authoritative identity refuses before any request is recorded.
    assert!(source.sent.is_none());
}
#[test]
fn malformed_evidence_is_refused_by_the_shared_annotation_adapter() {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut call = Call {
        origin: Origin::Ui,
        continuation: &mut continuation,
        cancelled: false,
        replies: &mut replies,
    };
    let mut source = CadThreadSource::new(&mut doc, &mut call, Some(4));
    let mut bad = evidence();
    bad["time_range"] = json!([0.3, 0.2]);
    let result = annotations::apply(
        &mut source,
        "Add annotation",
        ThreadOp::Create {
            title: "Experiment evidence".into(),
            targets: vec![CadAnchor::Evidence { evidence: bad }],
            body: "Original retained draft".into(),
            author: "You".into(),
            links: vec![],
            pin_m: None,
            view: None,
        },
    );
    assert!(result.unwrap_err().contains("time_range"));
    assert!(!matches!(source.sent, Some(Request::Evidence(_))));
}
#[test]
fn evidence_wire_preserves_capture_identity_and_omits_surface_coordinates() {
    use crate::cad::types::threads::{ExperimentEvidence, NewEvidenceThread};
    let input = NewEvidenceThread {
        body: "Sample note".into(),
        author: "You".into(),
        evidence: serde_json::from_value::<ExperimentEvidence>(evidence()).unwrap(),
        document_id: "live-document".into(),
        expected_revision: 4,
    };
    let wire = serde_json::to_value(input).unwrap();
    assert_eq!(wire["evidence"]["run_id"], "capture-1");
    assert_eq!(wire["evidence"]["source"]["line"], 8);
    assert_eq!(wire["document_id"], "live-document");
    assert_eq!(wire["expected_revision"], 4);
    assert!(wire.get("point").is_none());
    assert!(wire.get("node_id").is_none());
}
