use super::*;
fn document() -> CadDocument {
    let mut doc = CadDocument::new(crate::cad::CadTarget::Service(
        "http://127.0.0.1:8420".into(),
    ));
    doc.connection = crate::cad::Connection::Connected;
    doc.doc_key = Some((Some("doc17".into()), 23));
    doc.doc = Some(crate::cad::types::DocState {
        revision: 23,
        document_id: Some("doc17".into()),
        ..Default::default()
    });
    doc.health = Some(crate::cad::types::Health {
        revision: 23,
        ..Default::default()
    });
    doc.open_fixture();
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
    st.pending_port = Some(ports::PendingPort {
        endpoint: CadEndpoint {
            component_id: "a".into(),
            port: "port".into(),
        },
        generation: 1,
        document_id: "doc17".into(),
        revision: 23,
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

fn pending_fixture() -> (CadDocument, CadCompositionState) {
    let doc = document();
    let mut st = CadCompositionState::default();
    let graph = serde_json::from_value(json!({"version":1,"components":{"a":{"id":"a","name":"A","type":"thermal.capacitance","parameters":{"heat_capacity":2}}},"connections":{}})).unwrap();
    let types = serde_json::from_value(json!([{"type":"thermal.capacitance","name":"Capacity","parameters_complete":true,"parameters":[{"name":"heat_capacity","unit":"J/K","required":true,"default":null,"default_label":null,"minimum":0,"maximum":null,"exclusive_minimum":true,"integer":false}],"ports":[{"name":"port","schema":{"Acausal":"Thermal"}}]}])).unwrap();
    st.snapshot = Some(Snapshot {
        graph: GraphSnapshot {
            revision: 23,
            document_id: Some("doc17".into()),
            graph,
        },
        types,
        recipes: BTreeMap::new(),
        presentation: None,
        presentation_error: None,
        check_id: String::new(),
        imports: None,
    });
    st.snapshot_key = Some((doc.generation, 23));
    st.pending_port = Some(ports::PendingPort {
        endpoint: CadEndpoint {
            component_id: "a".into(),
            port: "port".into(),
        },
        generation: doc.generation,
        document_id: "doc17".into(),
        revision: 23,
    });
    (doc, st)
}
#[test]
fn pending_pick_refuses_new_revision_and_same_revision_replacement() {
    let (mut doc, mut st) = pending_fixture();
    assert!(ports::validate_pending(&doc, &st, Some(23)).is_ok());
    assert!(
        ports::validate_pending(&doc, &st, Some(22))
            .unwrap_err()
            .contains("stamp")
    );
    doc.doc.as_mut().unwrap().revision = 24;
    doc.health.as_mut().unwrap().revision = 24;
    st.snapshot.as_mut().unwrap().graph.revision = 24;
    st.snapshot_key = Some((doc.generation, 24));
    assert!(
        ports::validate_pending(&doc, &st, Some(24))
            .unwrap_err()
            .contains("stamp")
    );
    let (mut doc, mut st) = pending_fixture();
    doc.doc.as_mut().unwrap().document_id = Some("replacement".into());
    st.snapshot.as_mut().unwrap().graph.document_id = Some("replacement".into());
    assert!(
        ports::validate_pending(&doc, &st, Some(23))
            .unwrap_err()
            .contains("stamp")
    );
    assert!(st.pending_port.is_some());
}
#[test]
fn singleton_open_is_typed_source_net_and_connected_port_is_refused() {
    let (doc, mut st) = pending_fixture();
    let before = st.snapshot.as_ref().unwrap().graph.graph.clone();
    let (GraphCommand::Connect { ports }, revision) =
        ports::open_command(&doc, &st, Some(23)).unwrap()
    else {
        panic!("typed connect required")
    };
    assert_eq!(revision, 23);
    assert_eq!(ports.len(), 1);
    let mut physical = before.clone();
    physical.connections.insert(
        "explicit-open".into(),
        CadConnection {
            id: "explicit-open".into(),
            ports: ports.clone(),
        },
    );
    let snapshot = st.snapshot.as_ref().unwrap();
    let adapted = adapter::adapt(&physical, 23, &snapshot.types, &[], &snapshot.recipes).unwrap();
    let mut signals = adapted.description;
    for p in signals.ports.values_mut() {
        p.schema = sim_inspect::PortKind::SignalOutput {
            signal_type: sim_core::definitions::SignalType::Any,
        };
    }
    sim_system::composition::validate_connections(&signals).unwrap();
    for p in signals.ports.values_mut() {
        p.schema = sim_inspect::PortKind::SignalInput {
            signal_type: sim_core::definitions::SignalType::Any,
        };
    }
    assert!(
        sim_system::composition::validate_connections(&signals)
            .unwrap_err()
            .contains("signal output")
    );
    st.snapshot.as_mut().unwrap().graph.graph = physical;
    assert!(
        ports::open_command(&doc, &st, Some(23))
            .unwrap_err()
            .contains("already connected")
    );
    let controls = controls_of(&doc, &st);
    assert!(
        controls
            .iter()
            .any(|c| c.0 == "cad:composition:cancel_port")
    );
    assert!(controls.iter().any(|c| c.0 == "cad:composition:leave_open"));
    assert_eq!(
        serde_json::to_value(CadCompositionArgs::of(CompositionOp::CancelPort)).unwrap()["op"],
        "cancel_port"
    );
    assert_eq!(
        serde_json::to_value(CadCompositionArgs::of(CompositionOp::LeaveOpen)).unwrap()["op"],
        "leave_open"
    );
}

#[test]
fn first_pick_rejects_stale_display_and_cancel_keeps_source() {
    let (doc, mut st) = pending_fixture();
    assert!(
        ports::snapshot_at(&doc, &st, 22)
            .err()
            .unwrap()
            .contains("stamp")
    );
    st.snapshot.as_mut().unwrap().graph.document_id = Some("foreign".into());
    assert!(
        ports::snapshot_at(&doc, &st, 23)
            .err()
            .unwrap()
            .contains("stamp")
    );
    let before = st.snapshot.as_ref().unwrap().graph.graph.clone();
    assert_eq!(ports::cancel(&mut st)["source_changed"], false);
    assert!(st.pending_port.is_none());
    assert_eq!(st.snapshot.as_ref().unwrap().graph.graph, before);
    assert!(st.error.as_ref().unwrap().contains("cancelled"));
}

#[test]
fn port_dispatch_waits_for_matching_ack_and_preserves_refused_intent() {
    let (doc, mut st) = pending_fixture();
    let pending = st.pending_port.clone().unwrap();
    st.submitted_port = Some(ports::SubmittedPort {
        generation: doc.generation,
        seq: 9,
        pending: pending.clone(),
    });
    port_edit_answered(&mut st, doc.generation, 8, Ok(()));
    assert!(st.submitted_port.is_some());
    port_edit_answered(
        &mut st,
        doc.generation,
        9,
        Err("HTTP 409 stale source".into()),
    );
    assert!(st.submitted_port.is_none());
    assert_eq!(st.pending_port.as_ref(), Some(&pending));
    assert!(st.error.as_ref().unwrap().contains("409"));
    st.submitted_port = Some(ports::SubmittedPort {
        generation: doc.generation,
        seq: 10,
        pending,
    });
    port_edit_answered(&mut st, doc.generation, 10, Ok(()));
    assert!(st.pending_port.is_none());
}
#[test]
fn cancelling_submitted_port_keeps_ack_marker_without_resurrecting_intent() {
    let (doc, mut st) = pending_fixture();
    st.submitted_port = Some(ports::SubmittedPort {
        generation: doc.generation,
        seq: 9,
        pending: st.pending_port.clone().unwrap(),
    });
    assert_eq!(ports::cancel(&mut st)["submitted_edit_continues"], true);
    assert!(st.pending_port.is_none());
    assert!(st.submitted_port.is_some());
    assert!(st.error.as_ref().unwrap().contains("continues"));
    port_edit_answered(&mut st, doc.generation, 9, Err("409".into()));
    assert!(st.pending_port.is_none());
    assert!(st.submitted_port.is_none());
}

#[test]
fn drawn_pending_buttons_keep_shared_actions_without_a_usable_snapshot() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::{builder::ui_api::Enabled, cad::panel::CadButton, ui_kit::{Kit, UiFonts}};

    fn render(mut commands: Commands, doc: Res<CadDocument>, st: Res<CadCompositionState>) {
        let fonts = UiFonts {
            regular: default(), italic: default(), mono: default(), icons: BTreeMap::new(),
            medium: default(), semibold: default(),
        };
        commands.spawn(Node::default()).with_children(|p| ui::draw(p, &Kit::new(&fonts), &doc, &st));
    }
    // Inspect actual Button entities, not the control catalogue. Missing,
    // revised and replaced snapshots must all retain the cancellation widget.
    for case in 0..5 {
        let (mut doc, mut st) = pending_fixture();
        st.open = case != 4;
        match case {
            1 | 4 => { st.snapshot = None; st.snapshot_key = None; }
            2 => {
                doc.doc.as_mut().unwrap().revision = 24;
                doc.health.as_mut().unwrap().revision = 24;
                doc.doc_key = Some((Some("doc17".into()), 24));
            }
            3 => {
                doc.doc.as_mut().unwrap().document_id = Some("replacement".into());
                doc.doc_key = Some((Some("replacement".into()), 23));
            }
            _ => {}
        }
        let pending = st.pending_port.clone();
        let mut world = World::new();
        world.insert_resource(doc);
        world.insert_resource(st);
        world.run_system_once(render).unwrap();
        let mut query = world.query_filtered::<(&CadButton, &Enabled), With<Button>>();
        let actions: Vec<_> = query.iter(&world).filter_map(|(button, enabled)| {
            let CadAction::CadComposition(args) = &button.0 else { return None; };
            match &args.op {
                CompositionOp::LeaveOpen | CompositionOp::CancelPort => Some((args.op.clone(), args.revision, enabled.0)),
                _ => None,
            }
        }).collect();
        assert_eq!(actions.len(), 2, "case {case}: both actual widgets exist");
        assert!(actions.contains(&(CompositionOp::LeaveOpen, Some(23), case == 0)));
        assert!(actions.contains(&(CompositionOp::CancelPort, None, true)));
        assert_eq!(world.resource::<CadCompositionState>().pending_port, pending,
            "drawing never cancels or acknowledges source edits");
    }
}
