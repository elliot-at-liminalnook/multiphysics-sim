//! Written wire regressions; no server, solver or capture is executed here.
use super::motion::{PoseRequest, PoseSample};
use serde_json::json;
fn reply() -> PoseSample {
    serde_json::from_value(json!({"identity":{"document_id":"model-a","revision":17,"source_kind":"live_kinematic","source_id":"model-a","physical_hash":null,"archive_hash":null},"positions":{"driver":1.2,"passive-rod":-0.47,"coupled":-2.4},"matrices":{},"closure_error_mm":0.004,"time":0.75,"program":null,"prior_applied":true})).unwrap()
}
#[test]
fn continuation_wire_preserves_full_resolved_branch_and_source_identity() {
    let sample = reply();
    let request = PoseRequest {
        document_id: "model-a".into(),
        expected_revision: 17,
        positions: [("driver".into(), 1.4)].into(),
        program: None,
        time: 0.8,
        prior: Some(sample.continuation()),
    };
    let wire = serde_json::to_value(&request).unwrap();
    assert_eq!(wire["prior"]["identity"]["revision"], 17);
    assert_eq!(wire["prior"]["identity"]["source_kind"], "live_kinematic");
    assert_eq!(wire["prior"]["positions"]["passive-rod"], -0.47);
    assert_eq!(wire["prior"]["positions"]["coupled"], -2.4);
    assert_eq!(wire["positions"], json!({"driver":1.4}));
    assert_eq!(
        serde_json::from_value::<PoseRequest>(wire).unwrap(),
        request
    );
}
#[test]
fn exported_continuation_is_a_copy_and_cannot_change_preview_receipt() {
    let preview = reply();
    let mut export = preview.continuation();
    export.positions.insert("passive-rod".into(), 0.91);
    assert_eq!(preview.positions["passive-rod"], -0.47);
    assert_eq!(preview.continuation().positions["passive-rod"], -0.47);
}
#[test]
fn fresh_enter_omits_prior_and_malformed_wire_is_refused() {
    let wire = serde_json::to_value(PoseRequest::default()).unwrap();
    assert!(wire.get("prior").is_none());
    let malformed = json!({"document_id":"model-a","expected_revision":17,"positions":{},"program":null,"time":0.,"prior":{"identity":{},"positions":{"passive-rod":"guessed"}}});
    assert!(serde_json::from_value::<PoseRequest>(malformed).is_err());
}

#[test]
fn pose_sample_transport_sends_branch_receipt_with_guard_and_seek_time() {
    use super::tests::{ok, serve};
    let response = reply();
    let (client, server) = serve(vec![ok(&serde_json::to_string(&response).unwrap())]);
    let request = PoseRequest {
        document_id: "model-a".into(),
        expected_revision: 17,
        positions: [("driver".into(), 1.2)].into(),
        program: Some(json!({"name":"chosen program","duration":2.})),
        time: 0.75,
        prior: Some(response.continuation()),
    };
    let actual = client.sample_pose(&request).unwrap();
    assert_eq!(actual, response);
    let seen = server.join().unwrap();
    assert!(seen[0].head.starts_with("POST /motion/sample "));
    let body: serde_json::Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(body["expected_revision"], 17);
    assert_eq!(body["time"], 0.75);
    assert_eq!(body["prior"]["identity"]["source_id"], "model-a");
    assert_eq!(body["prior"]["positions"]["passive-rod"], -0.47);
    assert_eq!(body["program"]["name"], "chosen program");
}

#[test]
fn older_service_cannot_silently_ignore_continuation() {
    use super::tests::{ok, serve};
    let mut response = reply();
    response.prior_applied = false;
    let mut wire = serde_json::to_value(&response).unwrap();
    for omit_ack in [false, true] {
        if omit_ack {
            wire.as_object_mut().unwrap().remove("prior_applied");
        }
        let (client, server) = serve(vec![ok(&wire.to_string())]);
        let request = PoseRequest {
            document_id: "model-a".into(),
            expected_revision: 17,
            time: 0.75,
            prior: Some(response.continuation()),
            ..Default::default()
        };
        assert!(
            client
                .sample_pose(&request)
                .unwrap_err()
                .message
                .contains("did not confirm validated continuation")
        );
        assert_eq!(
            server.join().unwrap().len(),
            1,
            "unknown continuation capability never triggers retry"
        );
    }
}
