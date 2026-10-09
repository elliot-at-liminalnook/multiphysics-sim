//! The print jobs without a window, in process: a split of a bar longer
//! than the printer runs on its job, publishes its pieces as one edit and
//! says RoboCAD's done text; a job started on an older revision is refused
//! by name; cancel asks first, then cancels; the done texts of each kind.
use super::jobs_tracker::{self, capitalize, done_text, line, publishes};
use crate::app::actions::{Call, Origin, Replies};
use crate::cad::document::{CadDocument, CadTarget};
use serde_json::{Value, json};
use sim_api::Outcome;
use crate::cad::types::PrintJob;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// An archive with one box body `size` mm from the origin: (archive, the body's id).
fn archive_with_box(size: [f64; 3]) -> (Arc<sim_cad::ArchiveDocument>, String) {
    let path = Path::new("/tmp/print-jobs-test.rcad");
    let empty = sim_cad::ArchiveDocument::from_bytes(path, sim_cad::edit::empty_archive(None).unwrap(), &|| false, &|_| {}).unwrap();
    let stamps = sim_cad::annotations::Stamps::default();
    let mut edit = sim_cad::Edit::of(&empty);
    let id = {
        let mut cx = sim_cad::ops::Ctx { doc: &empty, stamps: &stamps, edit: &mut edit, centroid: &|_| None, cancelled: &|| false };
        let brep = sim_cad::kernel::build(&sim_cad::kernel::Shape::Box { corner: [0.0; 3], size }, &|| false).unwrap();
        cx.add_built(sim_cad::kernel::Built { kind: sim_cad::kernel::Kind::Solid, brep }, "Bar", None, None).unwrap()
    };
    (Arc::new(empty.apply(edit).unwrap()), id)
}

/// A local document showing `archive`, its jobs writing under a scratch folder.
fn document(archive: Arc<sim_cad::ArchiveDocument>, runs: &Path) -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::File(PathBuf::from("/tmp/print-jobs-test.rcad")));
    let snapshot = crate::cad::sync::snapshot_of(archive, &|| false, &|_| {}).unwrap();
    crate::cad::local::install(&mut doc, Arc::new(snapshot));
    doc.connection = crate::cad::Connection::Connected;
    doc.print.jobs.runs = Some(runs.to_path_buf());
    doc
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("print-jobs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Tick until `done` (a job's work runs on its own thread).
fn until(doc: &mut CadDocument, what: &str, done: impl Fn(&CadDocument) -> bool) {
    let end = Instant::now() + Duration::from_secs(120);
    while !done(doc) {
        assert!(Instant::now() < end, "timed out waiting for {what}: {}", jobs_tracker::state_json(doc));
        jobs_tracker::tick(doc);
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn state(doc: &CadDocument, id: &str) -> String {
    doc.print.jobs.jobs.iter().find(|j| j.id == id).map(|j| j.state.clone()).unwrap_or_default()
}

#[test]
fn a_split_job_runs_in_process_and_publishes_its_pieces_as_one_edit() {
    // 600 mm: longer than any printer's bed, so it is cut.
    let (archive, bar) = archive_with_box([600.0, 30.0, 20.0]);
    let runs = scratch("split");
    let mut doc = document(archive, &runs);
    let revision = doc.shown_revision();
    let job = jobs_tracker::start(&mut doc, "split", json!({"node": bar, "printer": "bambu-h2c", "joint": "pins"}), revision, "Started split of Bar".into()).unwrap();
    assert_eq!(job.kind, "split");
    assert!(!doc.print.jobs.blockers().is_empty(), "leaving CAD mode is refused while it runs");
    until(&mut doc, "the split to end and publish", |d| state(d, &job.id) != "running" && d.edit.is_some());
    let done = doc.print.jobs.jobs.iter().find(|j| j.id == job.id).unwrap().clone();
    assert_eq!(done.state, "done", "{:?}", done.error);
    assert!(done.result["piece_nodes"].as_array().is_some_and(|p| p.len() >= 2), "{}", done.result);
    assert!(done_text(&done).0.starts_with("split into "));
    assert_eq!(doc.edit_label().as_deref(), Some("Split Bar for printing"));
    assert!(doc.print.jobs.blockers().is_empty());
}

#[test]
fn a_job_read_at_an_older_revision_is_refused_by_name() {
    let (archive, bar) = archive_with_box([20.0, 20.0, 20.0]);
    let mut doc = document(archive, &scratch("stale"));
    let stale = doc.shown_revision() + 5;
    let e = jobs_tracker::start(&mut doc, "split", json!({"node": bar}), stale, String::new()).unwrap_err();
    assert!(e.contains("moved to revision"), "{e}");
    assert!(doc.print.jobs.jobs.is_empty());
    let now = doc.shown_revision();
    let e = jobs_tracker::start(&mut doc, "bake", json!({}), now, String::new()).unwrap_err();
    assert!(e.contains("not a print job"), "{e}");
}

#[test]
fn cancel_asks_first_then_cancels_and_never_publishes() {
    let (archive, bar) = archive_with_box([900.0, 40.0, 30.0]);
    let mut doc = document(archive, &scratch("cancel"));
    let revision = doc.shown_revision();
    let job = jobs_tracker::start(&mut doc, "split", json!({"node": bar}), revision, String::new()).unwrap();
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut c = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    let asked = jobs_tracker::cancel(&mut doc, &mut c, None, None);
    assert!(matches!(&asked, Outcome::Done(Ok(v)) if v["confirming"] == true));
    assert!(doc.print.jobs.confirming);
    assert!(matches!(jobs_tracker::cancel(&mut doc, &mut c, None, Some(true)), Outcome::Done(Ok(_))));
    until(&mut doc, "the cancelled job to end", |d| state(d, &job.id) != "running");
    assert_eq!(state(&doc, &job.id), "cancelled");
    assert!(doc.edit.is_none(), "a cancelled job publishes nothing");
    match jobs_tracker::cancel(&mut doc, &mut c, None, Some(true)) {
        Outcome::Done(Err(e)) => assert_eq!(e, "No print jobs are running"),
        _ => panic!("expected the refusal"),
    }
}

#[test]
fn robocads_texts_for_each_kind() {
    let job = |kind: &str, result: Value| PrintJob { id: "x".into(), kind: kind.into(), state: "done".into(), fraction: 1.0, result, ..PrintJob::default() };
    let split = job("split", json!({"group": "g1", "piece_nodes": ["p1", "p2", "p3"], "hardware": [{"item": "screw", "size": "M3x12", "count": 4}]}));
    assert_eq!(done_text(&split), ("split into 3 pieces; hardware: 4× screw M3x12".to_string(), None));
    let plan = job("plan", json!({"plate_files": ["a.3mf", "b.3mf"], "total_hours": 5.26, "total_filament_g": 123.4, "plates": "/runs/plates"}));
    assert_eq!(done_text(&plan).0, "plan: 2 plate(s), about 5.3 h and 123 g (estimates); 3MF files in /runs/plates");
    assert_eq!(done_text(&job("strength_split", json!({"recommendation": "split", "why": "the joint is weaker"}))).0, "split: the joint is weaker");
    let assembly = job("assembly", json!({"guide": "/runs/a/guide.html", "steps": [1, 2, 3], "exploded": "n9"}));
    assert_eq!(done_text(&assembly), ("assembly: 3 steps; guide /runs/a/guide.html".to_string(), Some("/runs/a/guide.html".to_string())));
    assert!(publishes(&assembly) && publishes(&split) && !publishes(&job("assembly", json!({"guide": "/g.html", "steps": []}))));
    let coupons = job("coupons", json!({"coupons": [1, 2], "plates": ["p.3mf"], "protocol": "/runs/c/PROTOCOL.md"}));
    assert_eq!(done_text(&coupons), ("coupons: 2 on 1 plate(s); break them, fill results.json, then `sim-print promote results.json`".to_string(), Some("/runs/c".to_string())));
    assert_eq!(capitalize("strength_split"), "Strength_split");
    assert_eq!(capitalize("ANALYZE"), "Analyze");
    let mut quiet = job("analyze", Value::Null);
    quiet.state = "running".into();
    quiet.fraction = 0.126;
    assert_eq!(line(&quiet), "analyze x: running 13 % ");
}
