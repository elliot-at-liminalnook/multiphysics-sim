//! The service over an in-memory sidecar, and every source's on-disk format
//! read and written back through its adapter (windowless).
use super::*;
use sim_annotate::ThreadDocument;
use sim_annotate::text::TextAnchor;
use sim_lesson::LessonAnchor;

/// A lesson-style sidecar applied in memory (the store applies the same
/// `ThreadDocument::apply` under its file lock).
struct Memory {
    doc: ThreadDocument<LessonAnchor>,
}
impl ThreadSource for Memory {
    type Anchor = LessonAnchor;
    const THREAD_ID: &'static str = "t";
    const COMMENT_ID: &'static str = "c";
    fn threads(&self) -> BTreeMap<String, Thread<LessonAnchor>> {
        self.doc.threads.clone()
    }
    fn commit(&mut self, _label: &str, command: ThreadCommand<LessonAnchor>) -> Result<Committed, String> {
        self.doc.apply(command, "lesson")?;
        Ok(Committed::Done)
    }
}

fn anchor() -> LessonAnchor {
    let text = sim_annotate::text::normalize("A worm drive trades efficiency for holding torque.");
    LessonAnchor::Text { quote: TextAnchor::capture(&text, 2, 12, "Intro").unwrap() }
}

/// Add, reply and resolve through the service give the document the
/// lessons' hand-built commands gave (same ids and stamps), and undo
/// restores each whole thread as before.
#[test]
fn add_reply_resolve_matches_the_direct_commands() {
    let mut source = Memory { doc: ThreadDocument::new("lesson") };
    let created = apply(&mut source, "New note", ThreadOp::Create { title: first_line("Why a worm?\nmore", "Note"), targets: vec![anchor()], body: "Why a worm?\nmore".into(), author: "Reader".into(), links: vec![], pin_m: None, view: None }).unwrap();
    assert_eq!(created.committed, Committed::Done);
    let id = created.thread.unwrap();
    assert!(id.starts_with("t-"));
    let replied = apply(&mut source, "Reply", ThreadOp::Reply { thread: id.clone(), body: "It holds the load.".into(), author: "Codex".into(), links: vec![] }).unwrap();
    assert_eq!(replied.thread.as_deref(), Some(id.as_str()));
    apply(&mut source, "Resolve", ThreadOp::Resolve { thread: id.clone(), resolved: true }).unwrap();

    // Today's path: the same thread and comment, built by hand.
    let t = &source.doc.threads[&id];
    let (first, second) = (t.comments[0].clone(), t.comments[1].clone());
    assert!(first.id.starts_with("c-") && second.id.starts_with("c-"));
    let mut direct = ThreadDocument::<LessonAnchor>::new("lesson");
    let thread = Thread { id: id.clone(), title: "Why a worm?".into(), resolved: false, targets: vec![anchor()], comments: vec![Comment { id: first.id.clone(), author: "Reader".into(), body: "Why a worm?\nmore".into(), created_at: first.created_at.clone(), edited_at: None, links: vec![] }], pin_m: None, view: None };
    direct.apply(ThreadCommand::PutThread { thread }, "lesson").unwrap();
    direct.apply(ThreadCommand::AddComment { thread: id.clone(), comment: Comment { id: second.id.clone(), author: "Codex".into(), body: "It holds the load.".into(), created_at: second.created_at.clone(), edited_at: None, links: vec![] } }, "lesson").unwrap();
    direct.apply(ThreadCommand::Resolve { thread: id.clone(), resolved: true }, "lesson").unwrap();
    assert_eq!(source.doc, direct);

    apply(&mut source, "Undo", ThreadOp::Undo).unwrap();
    assert!(!source.doc.threads[&id].resolved);
    apply(&mut source, "Redo", ThreadOp::Redo).unwrap();
    assert!(source.doc.threads[&id].resolved);
    // Shared validation refuses before anything is committed.
    let before = source.doc.clone();
    let empty = apply(&mut source, "New note", ThreadOp::Create { title: "x".into(), targets: vec![anchor()], body: " ".into(), author: "Reader".into(), links: vec![], pin_m: None, view: None });
    assert_eq!(empty.unwrap_err(), "comments need unique ID, author, timestamp and body (at most 20000 bytes)");
    assert_eq!(source.doc, before);
    apply(&mut source, "Delete", ThreadOp::Delete { thread: id.clone() }).unwrap();
    assert!(source.doc.threads.is_empty());
}

/// Whole-thread edits (the system discussions' form) match the sidecar's
/// own comment edits, deletes and resolves.
#[test]
fn whole_thread_edits_match_the_sidecar_commands() {
    let mut source = Memory { doc: ThreadDocument::new("lesson") };
    let id = apply(&mut source, "New", ThreadOp::Create { title: "T".into(), targets: vec![anchor()], body: "one".into(), author: "A".into(), links: vec![], pin_m: None, view: None }).unwrap().thread.unwrap();
    apply(&mut source, "Reply", ThreadOp::Reply { thread: id.clone(), body: "two".into(), author: "B".into(), links: vec![] }).unwrap();
    let thread = source.doc.threads[&id].clone();
    let c = thread.comments[1].id.clone();
    for command in [
        ThreadCommand::EditComment { thread: id.clone(), comment: c.clone(), body: "edited".into(), edited_at: "7".into() },
        ThreadCommand::DeleteComment { thread: id.clone(), comment: c.clone() },
        ThreadCommand::Resolve { thread: id.clone(), resolved: true },
    ] {
        let mut doc = source.doc.clone();
        doc.apply(command.clone(), "lesson").unwrap();
        assert_eq!(edited(thread.clone(), command).unwrap(), doc.threads[&id]);
    }
    assert_eq!(edited(thread.clone(), ThreadCommand::DeleteComment { thread: id.clone(), comment: "nope".into() }).unwrap_err(), "unknown comment");
    // Title, pin and link edits are whole-thread puts of the stored thread.
    let (_, put) = lower(&source, ThreadOp::Retitle { thread: id.clone(), title: "Renamed".into() }).unwrap();
    assert!(matches!(put, ThreadCommand::PutThread { ref thread } if thread.title == "Renamed" && thread.comments == source.doc.threads[&id].comments));
    let (_, put) = lower(&source, ThreadOp::Link { thread: id.clone(), targets: vec![anchor(), LessonAnchor::Scene { scene: "s".into(), part: None, time_s: None, missing: false }] }).unwrap();
    assert!(matches!(put, ThreadCommand::PutThread { ref thread } if thread.targets.len() == 2), "an anchor already there is not added again");
    assert_eq!(lower(&source, ThreadOp::Pin { thread: "missing".into(), pin_m: None }).unwrap_err(), "unknown thread");
}

/// A JSON value written back as text and read again (f32/f64 fields print as they were read).
fn rewritten<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::from_str(&serde_json::to_string(value).unwrap()).unwrap()
}
fn repo(path: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(path)
}

/// The lesson sidecar in the repository reads through the store the lesson
/// adapter submits to and writes back unchanged.
#[test]
fn lesson_notes_file_round_trips() {
    let path = repo("lessons/motor-torque-speed/lesson.md.annotations.json");
    let original: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let doc: ThreadDocument<LessonAnchor> = sim_annotate::store::read(&path, &"motor-torque-speed".to_string()).unwrap();
    assert!(!doc.threads.is_empty());
    assert_eq!(rewritten(&doc), original);
}

/// Every committed Inspect sidecar reads, shows each note as a thread,
/// turns each thread back into the same note, and writes back unchanged.
#[test]
fn inspect_notes_files_round_trip_through_the_adapter() {
    let description: sim_inspect::SystemDescription = serde_json::from_slice(&std::fs::read(repo("examples/systems-viewer/spatial/motor-thermal.description.json")).unwrap()).unwrap();
    let base = repo("examples/systems-viewer/evidence/rest-api");
    let mut checked = 0;
    for run in std::fs::read_dir(&base).unwrap().flatten() {
        let path = run.path().join("discussion.annotations.json");
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let doc: sim_inspect::annotations::Document = serde_json::from_value(original.clone()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut back = doc.clone();
        for note in doc.notes.values() {
            let thread = crate::notes::as_thread(note, &description);
            assert_eq!(thread.title, note.label);
            assert_eq!(thread.comments[0].links.len(), note.links.len());
            back.notes.insert(note.id.clone(), crate::notes::as_note(&thread, note.color).unwrap());
        }
        assert_eq!(back, doc, "{}", path.display());
        assert_eq!(rewritten(&back), original, "{}", path.display());
        checked += 1;
    }
    assert!(checked >= 3, "found {checked} sidecars");
}

/// A version-2 note (replies with links and an edit time, resolved) shows
/// as a thread whose comments after the note's text are its replies, turns
/// back into the same note, and writes its version-2 keys only when used.
#[test]
fn inspect_replies_and_resolve_round_trip_through_the_adapter() {
    use sim_inspect::annotations as notes;
    use sim_inspect::selection::SelectionTarget;
    let description: sim_inspect::SystemDescription = serde_json::from_slice(&std::fs::read(repo("examples/systems-viewer/spatial/motor-thermal.description.json")).unwrap()).unwrap();
    let part = description.components.keys().next().unwrap().clone();
    let link = notes::Link { label: "Motor".into(), target: notes::LinkTarget::Selection { target: SelectionTarget::component(part.clone()) } };
    let reply = |id: &str, links: Vec<notes::Link>| Comment { id: id.into(), author: "Ada".into(), body: format!("reply {id}"), created_at: "1760000000".into(), edited_at: None, links };
    let note = notes::Note {
        id: "n".into(),
        label: "Heat path".into(),
        text: "The winding heats the housing.".into(),
        targets: SelectionTarget::component(part),
        links: vec![link.clone()],
        color: [10, 140, 150],
        replies: vec![Comment { edited_at: Some("1760000100".into()), ..reply("r1", vec![link.clone()]) }, reply("r2", vec![])],
        resolved: true,
    };
    let thread = crate::notes::as_thread(&note, &description);
    assert!(thread.resolved);
    assert_eq!(thread.comments.len(), 3);
    assert_eq!((thread.comments[0].id.as_str(), thread.comments[0].author.as_str()), ("n", ""), "the note's text comes first, without author");
    assert_eq!((thread.comments[1].author.as_str(), thread.comments[1].edited_at.as_deref()), ("Ada", Some("1760000100")));
    assert_eq!(thread.comments[0].links[0].reply, None);
    assert_eq!(thread.comments[1].links[0].reply.as_deref(), Some("r1"));
    assert_eq!(crate::notes::as_note(&thread, note.color).unwrap(), note);
    // The whole-thread form of a reply edit, a reply delete and a reopen
    // turns into the note the sidecar's own commands give.
    let edited_thread = edited(thread.clone(), ThreadCommand::EditComment { thread: "n".into(), comment: "r2".into(), body: "changed".into(), edited_at: "7".into() }).unwrap();
    assert_eq!(crate::notes::as_note(&edited_thread, note.color).unwrap().replies[1].body, "changed");
    let deleted = edited(thread.clone(), ThreadCommand::DeleteComment { thread: "n".into(), comment: "r1".into() }).unwrap();
    assert_eq!(crate::notes::as_note(&deleted, note.color).unwrap().replies, vec![reply("r2", vec![])]);
    let reopened = edited(thread, ThreadCommand::Resolve { thread: "n".into(), resolved: false }).unwrap();
    assert!(!crate::notes::as_note(&reopened, note.color).unwrap().resolved);
    // On disk: `replies` and `resolved` only when used.
    let json = rewritten(&note);
    assert_eq!(json["resolved"], serde_json::json!(true));
    assert_eq!(json["replies"][0]["edited_at"], serde_json::json!("1760000100"));
    assert_eq!(serde_json::from_value::<notes::Note>(json).unwrap(), note);
    let plain = notes::Note { replies: vec![], resolved: false, ..note };
    let json = rewritten(&plain);
    assert!(json.get("replies").is_none() && json.get("resolved").is_none());
}

/// No committed system file holds discussions, so a system file from the
/// repository gets the pre-refactor thread JSON (the shape
/// `sim-system/tests/display.rs` pins): the document the builder saves
/// reads it and writes the same discussions back.
#[test]
fn system_discussions_round_trip_in_the_system_document() {
    let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(repo("examples/systems-builder/worm-drive/winch.system.json")).unwrap()).unwrap();
    let target = serde_json::json!({"path": "motor", "label": "motor", "lineage": ["abc"], "missing": false});
    let discussions = serde_json::json!({"threads": {"t1": {
        "id": "t1", "title": "Why 10 Ω?", "resolved": true,
        "targets": [target],
        "comments": [{"id": "c1", "author": "User", "body": "See [motor](part:motor)", "created_at": "1760000000", "edited_at": "1760000100", "links": [target]}],
        "pin_m": [0.0, 0.001, 0.0],
        "view": {"focus": [0.0, 0.0, 0.0], "radius": 0.2, "yaw": 0.3, "pitch": 0.4, "exploded": false, "connections": true, "hidden": ["gearbox"]}
    }}});
    value["discussions"] = discussions.clone();
    let doc: sim_system::SystemDocument = serde_json::from_value(value).unwrap();
    let written = rewritten(&doc);
    assert_eq!(written["discussions"], discussions);
    sim_annotate::validate_thread(&doc.discussions.threads["t1"]).unwrap();
}

/// A remote source's requests wait in `InFlight` until their job's answer
/// lands; an answer that is not this source's is ignored.
#[test]
fn a_remote_commit_lands_through_in_flight() {
    let mut f = InFlight::default();
    assert!(!f.busy());
    f.submitted(7, "Reply", Some("t1".into()));
    f.submitted(8, "New thread", None);
    assert!(f.busy() && f.waits(7) && f.waits(8));
    assert_eq!(f.land(9, Ok(None)), None, "not this source's request");
    let landed = f.land(8, Ok(Some("rc-42".into()))).unwrap();
    assert_eq!((landed.request, landed.label.as_str(), landed.thread, landed.result), (8, "New thread", None, Ok(Some("rc-42".to_string()))));
    assert_eq!(f.land(8, Ok(None)), None, "landed once");
    let failed = f.land(7, Err("RoboCAD: 422".into())).unwrap();
    assert_eq!(failed.thread.as_deref(), Some("t1"));
    assert!(!f.busy());
    f.submitted(10, "Resolve", Some("t1".into()));
    f.clear();
    assert!(!f.waits(10));
}
