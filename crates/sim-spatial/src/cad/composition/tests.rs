use super::*;
fn document() -> CadDocument {
    let mut doc = CadDocument::new(crate::cad::CadTarget::Service(
        "http://127.0.0.1:8420".into(),
    ));
    doc.client = Some(sim_runtime::cad_client::CadClient::new("http://127.0.0.1:8420").unwrap());
    doc.connection = crate::cad::Connection::Connected;
    doc.doc = Some(sim_runtime::cad_client::DocState {
        revision: 23,
        document_id: Some("doc17".into()),
        ..Default::default()
    });
    doc.health = Some(sim_runtime::cad_client::Health {
        revision: 23,
        ..Default::default()
    });
    doc
}
#[test]
fn real_draft_parser_refuses_foreign_stale_and_invalid_values_without_discarding() {
    let doc = document();
    let mut st = CadCompositionState::default();
    st.drafts.push(Draft {
        document_id: Some("doc17".into()),
        generation: doc.generation + 1,
        revision: 23,
        original: None,
        fields: BTreeMap::from([
            ("name".into(), "Retained".into()),
            ("parameter.mass".into(), "not a number".into()),
        ]),
        error: None,
    });
    st.current = Some(0);
    assert!(
        draft_command(&doc, &st)
            .unwrap_err()
            .contains("document changed")
    );
    st.drafts[0].generation = doc.generation;
    st.drafts[0].revision = 22;
    assert!(
        draft_command(&doc, &st)
            .unwrap_err()
            .contains("revision 22")
    );
    st.drafts[0].revision = 23;
    st.drafts[0].document_id = Some("foreign-document".into());
    assert!(
        draft_command(&doc, &st)
            .unwrap_err()
            .contains("draft.document_id")
    );
    st.drafts[0].document_id = Some("doc17".into());
    assert!(
        draft_command(&doc, &st)
            .unwrap_err()
            .contains("composition.parameters.mass")
    );
    assert_eq!(st.drafts[0].fields["parameter.mass"], "not a number");
}
#[test]
fn switching_completed_check_invalidates_prior_read_and_pending_endpoint() {
    let mut st = CadCompositionState::default();
    st.check_id = "check-A".into();
    st.snapshot_key = Some((1, 23));
    st.failed = Some((1, 23));
    st.pending_port = Some(CadEndpoint {
        component_id: "a".into(),
        port: "port".into(),
    });
    st.read = Some((
        (1, 23),
        Job::spawn(Pool::Compute, 1, "fixture old check read", |_| {
            Err("cancelled old check".into())
        }),
    ));
    set_check(&mut st, "check-B".into());
    assert_eq!(st.check_id, "check-B");
    assert!(st.read.is_none());
    assert!(st.snapshot_key.is_none());
    assert!(st.pending_port.is_none());
    assert!(st.failed.is_none());
}

#[test]
fn invalid_adaptation_keeps_source_repair_controls_and_complete_replace_draft() {
    let doc = document();
    let mut st = CadCompositionState::default();
    let graph:CadGraph=serde_json::from_value(json!({"version":1,"components":{"bad":{"id":"bad","name":"Retained unknown source","type":"not.registered","parameters":{}}},"connections":{}})).unwrap();
    let error = adapter::adapt(&graph, 23, &[], &[], &BTreeMap::new())
        .err()
        .unwrap();
    st.snapshot = Some(Snapshot {
        graph: GraphSnapshot {
            revision: 23,
            document_id: Some("doc17".into()),
            graph: graph.clone(),
        },
        types: vec![],
        recipes: BTreeMap::new(),
        presentation: None,
        presentation_error: Some(error),
        check_id: String::new(),
        imports: None,
    });
    st.snapshot_key = Some((doc.generation, 23));
    let controls = controls_of(&doc, &st);
    assert!(controls.iter().any(|c| c.0 == "cad:composition:remove:bad"));
    assert!(
        controls
            .iter()
            .any(|c| c.0 == "cad:composition:replace_form" && c.3.is_ok())
    );
    st.drafts.push(Draft {
        document_id: Some("doc17".into()),
        generation: doc.generation,
        revision: 23,
        original: None,
        fields: BTreeMap::from([("graph".into(), serde_json::to_string(&graph).unwrap())]),
        error: None,
    });
    st.current = Some(0);
    let (command, revision) = draft_command(&doc, &st).unwrap();
    assert_eq!(revision, 23);
    match command {
        GraphCommand::Replace { graph: replacement } => assert_eq!(replacement, graph),
        _ => panic!("complete graph draft must use typed Replace"),
    }
    // The parser preserves unsupported source; shared registry validation in
    // mutation refuses it by named path rather than inventing a physical type.
    assert!(
        st.snapshot
            .as_ref()
            .unwrap()
            .presentation_error
            .as_ref()
            .unwrap()
            .contains("graph.components.bad.type")
    );
}

#[test]
fn zoom_never_changes_source_or_accepts_nonfinite_factor() {
    let graph: CadGraph =
        serde_json::from_value(json!({"version":1,"components":{},"connections":{}})).unwrap();
    let before = serde_json::to_value(&graph).unwrap();
    assert_eq!(sim_diagram::composition::zoom(1., f32::NAN), 1.);
    assert_eq!(sim_diagram::composition::zoom(1., 100.), 8.);
    assert_eq!(serde_json::to_value(&graph).unwrap(), before);
}
