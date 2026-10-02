//! Reference-shaped, fake-service fixtures. Written, not executed in T42.
use super::components::*;
use super::tests::{ok, serve};
use serde_json::{Value, json};

#[test]
fn descriptor_keeps_nested_variants_and_provenance() {
    let source = json!({"id":"family-a","name":"Link","revision":3,
        "parameters":{"length":{"value":"20 mm","unit":"mm","min":0.01,"provenance":"estimated","description":"Length","uncertainty":{"sigma":0.2}}},
        "ports":{"mount":{"kind":"body","label":"Mount","source_id":"source-body"}},
        "nested":{"child-a":{"definition_id":"leaf","parameter_bindings":{"length":"length / 2"},"overrides":{"width":3}}},
        "variants":{"short":{"definition_id":"leaf","parameter_bindings":{"length":10}}},"default_variant":"short",
        "features":[{"kind":"box","node":"source-body","arguments":{"size":["length",10,20]}}],
        "provenance":{"source":"explicit component family","document_id":"doc-a","revision":7},
        "frame":{"length_unit":"mm","angle_unit":"deg","axes":"right-handed XYZ"},"node_count":2,"dependencies":["leaf"]});
    let definition: ComponentDefinition = serde_json::from_value(source.clone()).unwrap();
    let roundtrip = serde_json::to_value(definition).unwrap();
    for key in [
        "parameters",
        "ports",
        "nested",
        "variants",
        "provenance",
        "frame",
        "features",
    ] {
        assert_eq!(roundtrip[key], source[key], "{key}");
    }
}

#[test]
fn start_carries_document_identity_and_revision_and_cancel_reads_terminal() {
    let status = json!({"id":"job-a","state":"running","stage":"Transferring geometry","done":1,"total":2,"error":null,"log_path":null,"result":null,"document_id":"doc-a","revision":9});
    let mut cancelled = status.clone();
    cancelled["state"] = json!("cancelled");
    let (client, server) = serve(vec![
        ok(&json!({"job":status}).to_string()),
        ok(&cancelled.to_string()),
    ]);
    let operation = ComponentOperation::Place {
        definition_id: "family-a".into(),
        placement: json!({"translation":[0,0,0],"axis":[0,0,1],"angle_deg":0,"scale":1}),
        overrides: Default::default(),
        bindings: [("mount".into(), Some("body-b".into()))].into(),
        name: "Link 2".into(),
        variant: Some("short".into()),
    };
    let stamp = ComponentStamp {
        document_id: "doc-a".into(),
        expected_revision: 9,
    };
    assert_eq!(
        client.start_component(&operation, &stamp).unwrap().job.id,
        "job-a"
    );
    assert!(
        client
            .cancel_component_job("job-a")
            .unwrap()
            .state
            .terminal()
    );
    let seen = server.join().unwrap();
    let body: Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(body["expected_revision"], 9);
    assert_eq!(body["document_id"], "doc-a");
    assert_eq!(body["kwargs"]["bindings"]["mount"], "body-b");
    assert!(seen[0].request_line().contains("/ops/place_component"));
    assert!(
        seen[1]
            .request_line()
            .contains("DELETE /component-jobs/job-a")
    );
}

#[test]
fn stale_start_reports_server_refusal() {
    let (client, server) = serve(vec![super::tests::Answer::Json(
        409,
        json!({"error":"document revision changed"}).to_string(),
    )]);
    let error = client
        .start_component(
            &ComponentOperation::Import {
                path: "/tmp/library.rcomp".into(),
            },
            &ComponentStamp {
                document_id: "doc-a".into(),
                expected_revision: 9,
            },
        )
        .unwrap_err();
    assert_eq!(error.status, Some(409));
    assert!(error.message.contains("revision"));
    server.join().unwrap();
}
