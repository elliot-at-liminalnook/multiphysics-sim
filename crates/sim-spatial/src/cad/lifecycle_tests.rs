//! Local loader lifecycle cases. Source reviewed; not executed for CD1–CD4.
use super::{CadDocument, CadTarget, sync};
use crate::{document::DocumentRegistry, jobs::Job, selection::Selection};
use bevy::prelude::*;
use crate::cad::types::{DocState, NodeSummary};

fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::File("/previous.rcad".into()));
    doc.doc_key = Some((Some("previous".into()), 7));
    doc.doc = Some(DocState { nodes: vec![NodeSummary { id: "kept".into(), ..default() }], ..default() });
    doc
}
fn pending(doc: &mut CadDocument, source_generation: u64) {
    doc.local_load = Some(sync::LocalLoad { sequence: 1, source_generation, source_revision: 7, target: "/replacement.rcad".into(), job: Job::finished(source_generation, Err("bad B-rep".into())) });
}
#[test]
fn cancellation_retains_current_document_and_drops_pending_owner() {
    let mut doc = document();
    let generation = doc.generation;
    pending(&mut doc, generation);
    sync::cancel_load(&mut doc, 1);
    assert!(doc.local_load.is_none());
    assert_eq!(doc.generation, generation);
    assert_eq!(doc.doc.as_ref().unwrap().nodes[0].id, "kept");
    assert!(doc.load_outcomes[&1].as_ref().unwrap_err().contains("cancelled"));
}
fn receive(doc: CadDocument) -> App {
    let mut app = App::new();
    app.insert_resource(doc).insert_resource(DocumentRegistry::default()).insert_resource(Selection::default()).add_systems(Update, sync::receive);
    app.update();
    app
}
#[test]
fn failed_replacement_retains_current_archive_projection() {
    let mut doc = document();
    let generation = doc.generation;
    pending(&mut doc, generation);
    let app = receive(doc);
    let doc = app.world().resource::<CadDocument>();
    assert_eq!(doc.generation, generation);
    assert_eq!(doc.doc.as_ref().unwrap().nodes[0].id, "kept");
    assert!(matches!(&doc.target, CadTarget::File(path) if path.to_str() == Some("/previous.rcad")));
    assert_eq!(doc.load_outcomes[&1].as_ref().unwrap_err(), "bad B-rep");
}
#[test]
fn stale_generation_and_revision_are_refused_before_result_application() {
    for revision_changed in [false, true] {
        let mut doc = document();
        let captured = doc.generation;
        pending(&mut doc, captured);
        if revision_changed { doc.doc_key.as_mut().unwrap().1 = 8; } else { doc.generation += 1; }
        let app = receive(doc);
        let doc = app.world().resource::<CadDocument>();
        assert_eq!(doc.doc.as_ref().unwrap().nodes[0].id, "kept");
        assert!(doc.load_outcomes[&1].as_ref().unwrap_err().contains("stale"));
    }
}
#[test]
fn mode_exit_cancels_local_work() {
    let mut doc = document();
    let generation = doc.generation;
    pending(&mut doc, generation);
    doc.cancel_load();
    assert!(doc.local_load.is_none());
}
