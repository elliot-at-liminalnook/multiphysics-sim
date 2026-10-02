//! Written wire regressions; no fixtures executed in T43.
use super::{experiments::*, motion::*, components::ComponentStamp};
use super::tests::{ok, serve};
use serde_json::{Value,json};
#[test]
fn start_cancel_acknowledgement_and_discovery_preserve_capture_identity() {
    let run=json!({"id":"run-a","document_id":"doc-a","revision":4,"state":"running","label":"draft-9","provenance":{"physical_hash":"hash-a"}});
    let (client,server)=serve(vec![ok(&run.to_string()),ok(&run.to_string()),ok(&json!([run]).to_string())]);
    let request=ExperimentRequest{document_id:"doc-a".into(),expected_revision:4,system:json!({"entry":"main.rhai","files":{"main.rhai":"system()"}}),profile:"quick_check".into(),label:"draft-9".into(),..Default::default()};
    assert_eq!(client.start_experiment(&request).unwrap().revision,Some(4));
    assert!(!client.cancel_experiment("run-a").unwrap().terminal(),"cancel acknowledgment may remain running");
    assert_eq!(client.experiments().unwrap()[0].label,"draft-9");
    let seen=server.join().unwrap();
    let body:Value=serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(body["document_id"],"doc-a");assert_eq!(body["expected_revision"],4);
    assert!(seen[1].head.starts_with("POST /experiments/run-a/cancel "));
    assert!(seen[2].head.starts_with("GET /experiments "));
}
#[test]
fn missing_captured_geometry_is_explicit_and_never_live_geometry() {
    let wire=json!({"identity":{"document_id":"old-doc","revision":2,"source_kind":"experiment","source_id":"run-a","physical_hash":"captured","archive_hash":"archive"},"units":"mm","nodes":[],"missing_reason":"Captured run has no CAD geometry","provenance":{}});
    let (client,server)=serve(vec![ok(&wire.to_string())]);
    let geometry=client.experiment_geometry("run-a").unwrap();
    assert_eq!(geometry.identity.document_id.as_deref(),Some("old-doc"));
    assert!(geometry.nodes.is_empty());assert!(geometry.missing_reason.is_some());
    assert!(server.join().unwrap()[0].head.starts_with("GET /experiments/run-a/geometry "));
}
#[test]
fn pose_save_uses_units_and_authoritative_guard() {
    let program=json!({"name":"sweep","duration":1,"loop":true,"tracks":[{"joint":"hinge","unit":"deg","keys":[[0,0],[0.5,15],[1,0]]}]});
    let (client,server)=serve(vec![ok(&program.to_string())]);
    client.save_motion(&program,&ComponentStamp{document_id:"doc-a".into(),expected_revision:8}).unwrap();
    let seen=server.join().unwrap();let body:Value=serde_json::from_str(&seen[0].body).unwrap();
    assert!(seen[0].head.starts_with("POST /motion/programs/guarded "));
    assert_eq!(body["document_id"],"doc-a");assert_eq!(body["expected_revision"],8);assert_eq!(body["program"]["tracks"][0]["unit"],"deg");
}
#[test]
fn physical_limits_and_fallback_ranges_are_separate() {
    let joint:PoseJoint=serde_json::from_value(json!({"id":"slider","name":"Slider","unit":"mm","home":0,"lower":null,"upper":null,"display_lower":-100,"display_upper":100,"driver":true,"pivot":[0,0,0],"axis":[1,0,0],"child":"body","parent":null})).unwrap();
    assert!(joint.lower.is_none());assert!(joint.upper.is_none());assert_eq!(joint.display_upper,100.);
}
#[test]
fn candidate_accept_has_both_guards_and_discard_keeps_record_visible() {
    let wire=json!({"id":"candidate-a","document_id":"doc-a","base_revision":7,"revision":9,"state":"draft","label":"retained"});
    let mut refused=wire.clone();refused["state"]=json!("discarded");
    let (client,server)=serve(vec![ok(&wire.to_string()),ok(&refused.to_string())]);
    client.accept_candidate("candidate-a","doc-a",7).unwrap();
    assert_eq!(client.discard_candidate("candidate-a").unwrap().state,"discarded");
    let seen=server.join().unwrap();let body:Value=serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(body["document_id"],"doc-a");assert_eq!(body["expected_revision"],7);
    assert!(seen[1].head.starts_with("DELETE /candidates/candidate-a "));
}
#[test]
fn mutating_server_failure_is_unknown_but_read_failure_is_not_a_mutation() {
    let error = super::CadError::from_transport("POST", "/candidates/c/accept", crate::loopback_http::Error::Server { status:500, error:"receipt failed after source publication".into() });
    assert_eq!(error.status, Some(500));
    assert!(error.message.contains("may still apply"));
    let read = super::CadError::from_transport("GET", "/candidates/c", crate::loopback_http::Error::Server { status:500, error:"read failed".into() });
    assert_eq!(read.message,"read failed");
}
