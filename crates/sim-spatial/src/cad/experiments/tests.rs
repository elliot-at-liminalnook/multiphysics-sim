//! Written regression fixtures. This batch does not execute them.
use super::*;
fn document() -> CadDocument {
    let mut d = CadDocument::new(crate::cad::CadTarget::Service("http://127.0.0.1:1".into()));
    d.client = Some(sim_runtime::cad_client::CadClient::new("http://127.0.0.1:1").unwrap());
    d.connection = crate::cad::Connection::Connected;
    d.doc = Some(sim_runtime::cad_client::DocState {
        revision: 7,
        document_id: Some("doc".into()),
        ..Default::default()
    });
    d.doc_key = Some((Some("doc".into()), 7));
    d.health = Some(sim_runtime::cad_client::Health {
        revision: 7,
        ..Default::default()
    });
    d.open_fixture();
    d
}
fn state(d: &CadDocument) -> ExperimentsState {
    let mut st = ExperimentsState::default();
    st.open = true;
    st.drafts.push(Draft::new(Stamp::of(d, 0, 0).unwrap()));
    st.current = Some(0);
    st
}
fn active(s: Stamp) -> lifecycle::Active {
    lifecycle::Active {
        expected_revision: s.revision,
        stamp: s,
        client: sim_runtime::cad_client::CadClient::new("http://127.0.0.1:1").unwrap(),
        label: "unique".into(),
        mutation: false,
        cancel_requested: true,
        uncertain: Some("answer lost".into()),
        status: None,
        start: None,
        poll: None,
        cancel: None,
        discovery: None,
        last: None,
        operation: ExperimentsOp::Run,
    }
}
#[test]
fn refused_draft_values_survive_stale_replaced_and_switched_documents() {
    let mut doc = document();
    let mut st = state(&doc);
    form::set(
        &mut st,
        &ExperimentsArgs {
            name: Some("parameters".into()),
            value: Some("rejected JSON".into()),
            draft_index: Some(0),
            ..ExperimentsArgs::of(ExperimentsOp::Set)
        },
    )
    .unwrap();
    assert!(
        form::request(&doc, &st, false)
            .unwrap_err()
            .contains("experiments.parameters")
    );
    doc.stale = Some("refetch".into());
    assert!(form::guard(&doc, &st).is_err());
    doc.stale = None;
    doc.generation += 1;
    assert!(
        form::guard(&doc, &st)
            .unwrap_err()
            .contains("identity changed")
    );
    assert_eq!(st.drafts[0].get("parameters"), "rejected JSON");
    st.current = None;
    assert!(st.drafts.len() == 1);
    st.current = Some(0);
    assert!(
        form::set(
            &mut st,
            &ExperimentsArgs {
                draft_index: Some(1),
                name: Some("system".into()),
                value: Some("wrong".into()),
                ..ExperimentsArgs::of(ExperimentsOp::Set)
            }
        )
        .is_err()
    );
    assert!(
        form::set(
            &mut st,
            &ExperimentsArgs {
                draft_sequence: Some(0),
                name: Some("system".into()),
                value: Some("wrong".into()),
                ..ExperimentsArgs::of(ExperimentsOp::Set)
            }
        )
        .is_err()
    );
}
#[test]
fn unknown_start_uses_unique_readonly_discovery_and_keeps_cancel() {
    let d = document();
    let st = state(&d);
    let mut a = active(st.drafts[0].stamp.clone());
    assert!(lifecycle::ambiguous("RoboCAD may still apply it"));
    let record = sim_runtime::cad_client::experiments::ExperimentRecord {
        id: "captured".into(),
        label: "unique".into(),
        document_id: Some("doc".into()),
        revision: Some(7),
        state: "running".into(),
        ..Default::default()
    };
    assert!(lifecycle::recover(&mut a, vec![record.clone(), record.clone()]).is_err());
    assert!(a.uncertain.is_some());
    lifecycle::recover(&mut a, vec![record]).unwrap();
    assert!(a.cancel_requested);
    assert!(a.uncertain.is_none());
}
#[test]
fn cancellation_running_ack_keeps_intent_and_terminal_ack_cannot_be_regressed() {
    let d = document();
    let mut a = active(Stamp::of(&d, 0, 0).unwrap());
    let r = sim_runtime::cad_client::experiments::ExperimentRecord {
        id: "run".into(),
        document_id: Some("doc".into()),
        state: "running".into(),
        revision: Some(7),
        updated_at: 2.,
        ..Default::default()
    };
    lifecycle::accept_status(&mut a, r.clone());
    assert!(a.cancel_requested);
    assert!(!a.status.as_ref().unwrap().terminal());
    lifecycle::accept_status(
        &mut a,
        sim_runtime::cad_client::experiments::ExperimentRecord {
            state: "cancelled".into(),
            updated_at: 3.,
            ..r.clone()
        },
    );
    lifecycle::accept_status(&mut a, r);
    assert_eq!(a.status.as_ref().unwrap().state, "cancelled");
}
#[test]
fn actual_drawn_cancel_and_refused_starts_survive_missing_stale_replaced_and_closed_dock() {
    use crate::{
        builder::ui_api::Enabled,
        cad::panel::CadButton,
        ui_kit::{Kit, UiFonts},
    };
    use bevy::ecs::system::RunSystemOnce;
    fn render(mut commands: Commands, doc: Res<CadDocument>, st: Res<ExperimentsState>) {
        let fonts = UiFonts {
            regular: default(),
            italic: default(),
            mono: default(),
            icons: BTreeMap::new(),
            medium: default(),
            semibold: default(),
        };
        commands
            .spawn(Node::default())
            .with_children(|p| ui::draw(p, &Kit::new(&fonts), &doc, &st));
    }
    for case in 0..5 {
        let mut d = document();
        let mut st = state(&d);
        st.active = Some(active(st.drafts[0].stamp.clone()));
        match case {
            1 => {
                d.doc = None;
                d.doc_key = None;
            }
            2 => d.stale = Some("refetch".into()),
            3 => d.generation += 1,
            4 => st.open = false,
            _ => {}
        }
        let mut world = World::new();
        world.insert_resource(d);
        world.insert_resource(st);
        world.run_system_once(render).unwrap();
        let mut q = world.query_filtered::<(&CadButton, &Enabled), With<Button>>();
        let actual = q
            .iter(&world)
            .filter_map(|(b, e)| {
                let CadAction::CadExperiments(a) = &b.0 else {
                    return None;
                };
                Some((a.op, e.0))
            })
            .collect::<Vec<_>>();
        assert!(
            actual.contains(&(ExperimentsOp::Dock, true)),
            "case {case}: actual dock toggle remains available"
        );
        assert!(
            actual.contains(&(ExperimentsOp::Cancel, true)),
            "case {case}: cancellation is actually rendered"
        );
        assert!(
            actual.contains(&(ExperimentsOp::Discover, true)),
            "case {case}: unknown outcome remains inspectable"
        );
        if case != 4 {
            assert!(actual.contains(&(ExperimentsOp::Run, false)));
            assert!(actual.contains(&(ExperimentsOp::Preflight, false)));
            assert!(actual.contains(&(ExperimentsOp::CandidateAccept, false)));
        }
        assert_eq!(world.resource::<ExperimentsState>().drafts.len(), 1);
    }
}
#[test]
fn completed_preflight_control_is_disabled_after_document_revision_changes() {
    let mut d = document();
    let mut st = state(&d);
    st.completed_check = Some((d.generation, "doc".into(), 7, "check".into()));
    assert!(
        controls_of(&d, &st)
            .iter()
            .find(|c| c.0 == "cad:experiments:import_composition")
            .unwrap()
            .3
            .is_ok()
    );
    d.doc.as_mut().unwrap().revision = 8;
    d.health.as_mut().unwrap().revision = 8;
    d.doc_key = Some((Some("doc".into()), 8));
    assert!(
        controls_of(&d, &st)
            .iter()
            .find(|c| c.0 == "cad:experiments:import_composition")
            .unwrap()
            .3
            .is_err()
    );
}
#[test]
fn candidate_unknown_creation_and_refusal_are_discovered_without_retrying() {
    let d = document();
    let mut a = active(Stamp::of(&d, 0, 0).unwrap());
    a.operation = ExperimentsOp::CandidateCreate;
    let record = json!({"id":"candidate","label":"unique","document_id":"doc","base_revision":7,"state":"draft"});
    assert!(lifecycle::recover_candidate(&mut a, json!([record.clone(), record.clone()])).is_err());
    assert!(a.uncertain.is_some());
    assert_eq!(
        lifecycle::recover_candidate(&mut a, json!([record])).unwrap()["id"],
        "candidate"
    );
    assert!(a.cancel_requested);
    a.operation = ExperimentsOp::CandidateDiscard;
    a.label = "candidate".into();
    a.uncertain = Some("lost answer".into());
    assert!(
        lifecycle::recover_candidate(
            &mut a,
            json!({"id":"candidate","document_id":"doc","state":"draft"})
        )
        .is_err()
    );
    assert!(
        lifecycle::recover_candidate(
            &mut a,
            json!({"id":"candidate","document_id":"doc","state":"discarded"})
        )
        .is_ok()
    );
}
#[test]
fn delayed_link_control_cannot_change_edited_draft_and_linked_source_is_readonly() {
    let d = document();
    let mut st = state(&d);
    st.drafts[0].stamp.sequence = 1;
    assert!(
        form::link(
            &mut st,
            &ExperimentsArgs {
                name: Some("system".into()),
                value: Some("/sources/system.rhai".into()),
                draft_index: Some(0),
                draft_sequence: Some(0),
                ..ExperimentsArgs::of(ExperimentsOp::Link)
            }
        )
        .is_err()
    );
    assert!(st.drafts[0].linked.is_empty());
    form::link(
        &mut st,
        &ExperimentsArgs {
            name: Some("system".into()),
            value: Some("/sources/system.rhai".into()),
            draft_index: Some(0),
            draft_sequence: Some(1),
            ..ExperimentsArgs::of(ExperimentsOp::Link)
        },
    )
    .unwrap();
    assert!(
        form::set(
            &mut st,
            &ExperimentsArgs {
                name: Some("system".into()),
                value: Some("overwrite".into()),
                draft_index: Some(0),
                ..ExperimentsArgs::of(ExperimentsOp::Set)
            }
        )
        .unwrap_err()
        .contains("read-only")
    );
    assert!(
        st.drafts[0].restored.get("system").is_none(),
        "Run remains refused until linked bundle capture"
    );
}
fn due_auto(d: &CadDocument) -> ExperimentsState {
    let mut st = state(d);
    st.drafts[0].auto = true;
    st.drafts[0].edited = std::time::Instant::now() - std::time::Duration::from_secs(1);
    st
}
#[test]
fn closing_before_debounce_enqueues_nothing_and_preserves_draft() {
    let d = document();
    let mut st = state(&d);
    st.drafts[0].auto = true;
    automatic::dock(&mut st, false);
    st.drafts[0].edited = std::time::Instant::now() - std::time::Duration::from_secs(1);
    assert!(automatic::enqueue(&mut st, &d).is_none());
    assert_eq!(st.drafts.len(), 1);
    assert!(st.drafts[0].submitted_sequence.is_none());
}
#[test]
fn closing_after_enqueue_refuses_queued_apply_and_releases_only_automatic_marker() {
    let d = document();
    let mut st = due_auto(&d);
    let queued = automatic::enqueue(&mut st, &d).unwrap();
    assert!(automatic::validate(&st, &queued).is_ok());
    automatic::dock(&mut st, false);
    assert!(
        automatic::validate(&st, &queued)
            .unwrap_err()
            .contains("dock closed")
    );
    assert!(st.drafts[0].submitted_sequence.is_none());
    assert!(st.queued_automatic.is_none());
    // Closed native UI/REST intent is explicit and therefore unaffected.
    assert!(automatic::validate(&st, &ExperimentsArgs::of(ExperimentsOp::Run)).is_ok());
    assert!(form::request(&d, &st, false).is_ok());
}
#[test]
fn closed_revision_changes_do_not_rebase_and_reopen_waits_for_fresh_debounce() {
    let mut d = document();
    let mut st = due_auto(&d);
    let old = automatic::enqueue(&mut st, &d).unwrap();
    automatic::dock(&mut st, false);
    d.doc_key = Some((Some("doc".into()), 8));
    d.doc.as_mut().unwrap().revision = 8;
    d.health.as_mut().unwrap().revision = 8;
    automatic::rebase(&mut st, &d);
    assert_eq!(st.drafts.len(), 1);
    assert_eq!(st.drafts[0].stamp.revision, 7);
    automatic::dock(&mut st, true);
    assert!(automatic::validate(&st, &old).is_err());
    assert!(automatic::enqueue(&mut st, &d).is_none());
    automatic::rebase(&mut st, &d);
    assert_eq!(st.drafts.len(), 2);
    assert_eq!(st.drafts[1].stamp.revision, 8);
    assert!(automatic::enqueue(&mut st, &d).is_none());
    st.drafts[1].edited = std::time::Instant::now() - std::time::Duration::from_secs(1);
    let resumed = automatic::enqueue(&mut st, &d).unwrap();
    assert_ne!(resumed.automatic_epoch, old.automatic_epoch);
    assert!(automatic::validate(&st, &resumed).is_ok());
}
#[test]
fn closing_keeps_submitted_automatic_and_explicit_receipts() {
    let d = document();
    let mut st = due_auto(&d);
    let queued = automatic::enqueue(&mut st, &d).unwrap();
    st.active = Some(active(st.drafts[0].stamp.clone()));
    automatic::submitted(&mut st, &queued);
    automatic::dock(&mut st, false);
    assert_eq!(st.drafts[0].submitted_sequence, Some(0));
    assert!(st.active.as_ref().unwrap().cancel_requested);
    let mut st = due_auto(&d);
    let _queued = automatic::enqueue(&mut st, &d).unwrap();
    st.active = Some(active(st.drafts[0].stamp.clone()));
    automatic::submitted(&mut st, &ExperimentsArgs::of(ExperimentsOp::Run));
    automatic::dock(&mut st, false);
    assert_eq!(st.drafts[0].submitted_sequence, Some(0));
    assert!(st.queued_automatic.is_none());
}
#[test]
fn reopening_same_revision_resumes_cancelled_queue_after_debounce() {
    let d = document();
    let mut st = due_auto(&d);
    let old = automatic::enqueue(&mut st, &d).unwrap();
    automatic::dock(&mut st, false);
    automatic::dock(&mut st, true);
    assert!(automatic::enqueue(&mut st, &d).is_none());
    st.drafts[0].edited = std::time::Instant::now() - std::time::Duration::from_secs(1);
    let resumed = automatic::enqueue(&mut st, &d).unwrap();
    assert!(automatic::validate(&st, &old).is_err());
    assert!(automatic::validate(&st, &resumed).is_ok());
}
#[test]
fn changing_auto_settings_invalidates_old_queue_even_if_reenabled_before_apply() {
    let d = document();
    let mut st = due_auto(&d);
    let old = automatic::enqueue(&mut st, &d).unwrap();
    automatic::configure(&mut st, Some(false)).unwrap();
    automatic::configure(&mut st, Some(true)).unwrap();
    assert!(automatic::validate(&st, &old).is_err());
    assert!(automatic::enqueue(&mut st, &d).is_none());
    st.drafts[0].edited = std::time::Instant::now() - std::time::Duration::from_secs(1);
    let renewed = automatic::enqueue(&mut st, &d).unwrap();
    assert!(automatic::validate(&st, &old).is_err());
    assert!(automatic::validate(&st, &renewed).is_ok());
    automatic::configure(&mut st, Some(true)).unwrap();
    assert!(automatic::validate(&st, &renewed).is_err());
    assert!(automatic::enqueue(&mut st, &d).is_none());
}
#[test]
fn candidate_work_with_same_stamp_cannot_claim_a_queued_experiment_receipt() {
    let d = document();
    let mut st = due_auto(&d);
    let _queued = automatic::enqueue(&mut st, &d).unwrap();
    let mut candidate = active(st.drafts[0].stamp.clone());
    candidate.operation = ExperimentsOp::CandidateCreate;
    st.active = Some(candidate);
    automatic::dock(&mut st, false);
    assert!(st.drafts[0].submitted_sequence.is_none());
    assert!(st.active.as_ref().unwrap().cancel_requested);
    st.active = None;
    automatic::dock(&mut st, true);
    st.drafts[0].edited = std::time::Instant::now() - std::time::Duration::from_secs(1);
    assert!(automatic::enqueue(&mut st, &d).is_some());
}
