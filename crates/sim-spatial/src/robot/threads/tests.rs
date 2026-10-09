//! Robot mode's CAD threads without a window: a thread's part maps to the
//! link whose body or members name it, or an ancestor's, never by name; a
//! thread on no link and an evidence thread are labelled; every thread of
//! an export that is not current with its CAD file is labelled for that
//! status; a change is refused by name while the CAD source is not read;
//! a part link to a member selects the containing link; a change is written
//! to the `.rcad` in process and refused when the file changed on disk;
//! Open in CAD reveals in the document the switch creates.
use super::act::{cad_document, reveal_target};
use super::read::{Listed, Reach};
use super::*;
use crate::annotations::{self, ThreadOp};
use crate::app::switch::Document;
use crate::cad::CadTarget;
use crate::cad::types::CadThread;

/// A pin on `m2` (a member of link 1), a pin on `p9` (a plate under group
/// `g1`, whose parent `b0` is link 0's body), a pin on `x1` (on no link)
/// and an evidence thread.
const LIST: &str = r#"[
 {"id": "t1", "anchor": {"node_id": "m2", "point": [1.0, 2.0, 3.0]}, "status": "open", "comments": [{"id": "c1", "author": "You", "body": "Loose: see [the bolt](part:m2)"}, {"id": "c2", "author": "Ana", "body": "Agreed"}], "node_name": "Bolt", "anchor_status": "attached"},
 {"id": "t2", "anchor": {"node_id": "p9", "point": [0.0, 0.0, 0.0]}, "status": "open", "comments": [{"id": "c3", "author": "You", "body": "Thin"}], "node_name": "Plate", "anchor_status": "attached"},
 {"id": "t3", "anchor": {"node_id": "x1", "point": [0.0, 0.0, 0.0]}, "status": "resolved", "comments": [{"id": "c4", "author": "You", "body": "Here"}], "node_name": "Spacer", "anchor_status": "attached"},
 {"id": "e1", "anchor": {"node_id": null, "point": null}, "status": "open", "comments": [{"id": "c5", "author": "You", "body": "Run"}], "evidence": {"run_id": "r1"}, "node_name": "Experiment evidence", "anchor_status": "evidence"}
]"#;

fn threads() -> Vec<CadThread> {
    serde_json::from_str(LIST).expect("thread_detail answers")
}

fn links() -> Vec<LinkKeys> {
    vec![
        LinkKeys { name: "base".into(), id: "b0".into(), members: vec!["b0".into()] },
        LinkKeys { name: "arm".into(), id: "b1".into(), members: vec!["b1".into(), "m2".into()] },
        // A link exported without its body's id.
        LinkKeys { name: "loose".into(), id: String::new(), members: Vec::new() },
    ]
}

fn parents() -> Parents {
    [("b0", None), ("g1", Some("b0")), ("p9", Some("g1")), ("b1", None), ("m2", Some("b1")), ("x1", None)].into_iter().map(|(id, p): (&str, Option<&str>)| (id.to_string(), p.map(str::to_string))).collect()
}

fn base() -> Base {
    Base { simrobot: PathBuf::from("/w/robot.simrobot.json"), cad: PathBuf::from("/w/robot.rcad") }
}

/// The threads read from the CAD source at revision 7.
fn open_state() -> RobotThreads {
    let mut st = RobotThreads::default();
    st.read.listed = Some(Listed { base: base(), revision: 7, document_id: Some("doc-1".into()), identity: "bytes-1".into(), stat: (1, None), threads: threads(), parents: parents() });
    st.read.answered = Some(((base(), st.read.epoch), Reach::Open));
    st
}

fn shown(id: &str) -> Thread<CadAnchor> {
    thread_of(threads().iter().find(|t| t.id == id).expect("listed"))
}

#[test]
fn a_part_maps_to_the_link_whose_body_or_members_name_it() {
    let (links, parents) = (links(), parents());
    // By the link's body id.
    assert_eq!(link_of(&links, &parents, "b1"), Some(1));
    // By a member.
    assert_eq!(link_of(&links, &parents, "m2"), Some(1));
    // By the nearest ancestor that maps (p9 → g1 → b0).
    assert_eq!(link_of(&links, &parents, "p9"), Some(0));
    // On no link; an empty node never matches a link without a body id.
    assert_eq!(link_of(&links, &parents, "x1"), None);
    assert_eq!(link_of(&links, &parents, "unknown"), None);
    assert_eq!(link_of(&links, &parents, ""), None);
}

#[test]
fn the_ancestor_walk_stops_on_a_cycle() {
    let links = links();
    let cyclic: Parents = [("a", Some("b")), ("b", Some("c")), ("c", Some("a"))].into_iter().map(|(id, p): (&str, Option<&str>)| (id.to_string(), p.map(str::to_string))).collect();
    assert_eq!(link_of(&links, &cyclic, "a"), None);
    let selfish: Parents = [("s".to_string(), Some("s".to_string()))].into_iter().collect();
    assert_eq!(link_of(&links, &selfish, "s"), None);
}

#[test]
fn a_thread_on_no_link_or_on_evidence_is_labelled() {
    let (links, parents) = (links(), parents());
    assert_eq!(place(&shown("t1"), &links, &parents, None), Placed { link: Some(1), warning: None });
    assert_eq!(place(&shown("t2"), &links, &parents, None), Placed { link: Some(0), warning: None });
    assert_eq!(place(&shown("t3"), &links, &parents, None), Placed { link: None, warning: Some("Not on any link of this export: Spacer".into()) });
    assert_eq!(place(&shown("e1"), &links, &parents, None), Placed { link: None, warning: Some("Not on any link of this export: experiment evidence".into()) });
    // Without RoboCAD's part name, the node id names it.
    let mut unnamed = shown("t3");
    if let Some(CadAnchor::Surface { node_name, .. }) = unnamed.targets.first_mut() {
        node_name.clear();
    }
    assert_eq!(place(&unnamed, &links, &parents, None).warning.as_deref(), Some("Not on any link of this export: x1"));
}

#[test]
fn every_thread_of_an_export_not_current_with_its_cad_file_is_labelled() {
    let path = PathBuf::from("/w/robot.rcad");
    let cases = [
        (CadLinkStatus::Current { path: path.clone(), sha256: "a".into(), tried: vec![] }, None),
        (CadLinkStatus::Stale { path: path.clone(), recorded_sha256: "a".into(), on_disk_sha256: "b".into(), tried: vec![] }, Some("The export is older than the CAD file: this comment may be on a different part now")),
        (CadLinkStatus::NoRecordedHash { path: path.clone(), on_disk_sha256: "b".into(), tried: vec![] }, Some("The export recorded no CAD hash: this comment may be on a different part now")),
        (CadLinkStatus::Missing { file: "robot.rcad".into(), tried: vec![] }, Some("The export's CAD file was not found: this comment may be on a different part now")),
        (CadLinkStatus::NoSourceFile, Some("The export names no CAD file: this comment may be on a different part now")),
        (CadLinkStatus::Unreadable { path: path.clone(), error: "denied".into(), tried: vec![] }, Some("The CAD file could not be read to compare with the export: this comment may be on a different part now")),
    ];
    let (links, parents) = (links(), parents());
    for (status, note) in cases {
        assert_eq!(link_note(Some(&status)), note, "{status:?}");
        // A mapped thread carries the note alone; an unmapped one both lines.
        assert_eq!(place(&shown("t1"), &links, &parents, link_note(Some(&status))).warning.as_deref(), note);
        let both = place(&shown("t3"), &links, &parents, link_note(Some(&status))).warning;
        match note {
            Some(n) => assert_eq!(both, Some(format!("Not on any link of this export: Spacer\n{n}"))),
            None => assert_eq!(both.as_deref(), Some("Not on any link of this export: Spacer")),
        }
    }
    assert_eq!(cad_path(&cases_path()), Some(Path::new("/w/robot.rcad")));
}

fn cases_path() -> CadLinkStatus {
    CadLinkStatus::Stale { path: PathBuf::from("/w/robot.rcad"), recorded_sha256: "a".into(), on_disk_sha256: "b".into(), tried: vec![] }
}

#[test]
fn a_change_is_refused_by_name_while_the_cad_source_is_not_read() {
    let reply = || ThreadOp::Reply { thread: "t1".into(), body: "Fixed".into(), author: "You".into(), links: vec![] };
    // No CAD file resolved: nothing read.
    let mut st = RobotThreads::default();
    let mut source = RobotCadThreads { st: &mut st, base: None, file: "robot.rcad".into() };
    assert_eq!(annotations::apply(&mut source, "", reply()), Err("the comments of robot.rcad are not read yet (or could not be read): refresh, or open it in CAD mode".to_string()));
    // The file could not be read: not open either.
    let mut st = open_state();
    st.read.answered = Some(((base(), st.read.epoch), Reach::Failed("denied".into())));
    let mut source = RobotCadThreads { st: &mut st, base: Some(base()), file: "robot.rcad".into() };
    assert_eq!(annotations::apply(&mut source, "", reply()), Err(not_open("robot.rcad")));
    assert!(!source.st.in_flight.busy(), "nothing was sent");
    // No undo here, read or not.
    let mut st = open_state();
    let mut source = RobotCadThreads { st: &mut st, base: Some(base()), file: "robot.rcad".into() };
    assert_eq!(annotations::apply(&mut source, "", ThreadOp::Undo), Err(NO_UNDO.to_string()));
    // Open: a new thread is placed in CAD mode, an unknown thread is named; nothing is sent.
    let create = ThreadOp::Create { title: "x".into(), targets: vec![CadAnchor::part("b1", "Arm")], body: "x".into(), author: "You".into(), links: vec![], pin_m: None, view: None };
    assert_eq!(annotations::apply(&mut source, "", create), Err("a new comment thread is placed in CAD mode: Annotate model, then click a surface".to_string()));
    let unknown = ThreadOp::Reply { thread: "zz".into(), body: "x".into(), author: "You".into(), links: vec![] };
    assert_eq!(annotations::apply(&mut source, "", unknown), Err("no comment thread zz in RoboCAD's comments as last read".to_string()));
    // RoboCAD's last-message refusal, before anything is sent.
    let last = ThreadOp::DeleteComment { thread: "t2".into(), comment: "c3".into() };
    assert_eq!(annotations::apply(&mut source, "", last), Err("delete the thread to remove its last comment".to_string()));
    assert!(!source.st.in_flight.busy(), "nothing was sent");
    // The threads it shows are the ones read.
    assert_eq!(source.threads().len(), 4);
    // One change at a time: a second is refused by name while the first is in flight.
    source.st.in_flight.submitted(1, crate::cad::threads::REPLY, Some("t1".into()));
    let reply = ThreadOp::Reply { thread: "t1".into(), body: "Fixed".into(), author: "You".into(), links: vec![] };
    assert_eq!(annotations::apply(&mut source, "", reply), Err(BUSY.to_string()));
}

#[test]
fn a_part_link_to_a_member_selects_the_containing_link() {
    let (links, parents) = (links(), parents());
    // The part link in c1's body, as `thread_of` reads it.
    let t1 = shown("t1");
    let part = t1.comments[0].links.first().expect("a part link");
    assert_eq!(part.node(), Some("m2"));
    assert_eq!(anchor_link(part, &links, &parents), Some(1));
    // A chip on a part under a link's body selects that link; one on no link is not pressable.
    assert_eq!(anchor_link(&CadAnchor::part("p9", "Plate"), &links, &parents), Some(0));
    assert_eq!(anchor_link(&CadAnchor::part("x1", "Spacer"), &links, &parents), None);
    assert_eq!(anchor_link(&CadAnchor::Evidence { evidence: Value::Null }, &links, &parents), None);
}

#[test]
fn open_in_cad_reveals_in_the_document_the_switch_creates() {
    let cad = Path::new("/w/robot.rcad");
    // `app::switch` makes `CadTarget::File(path)` of the file, unchanged.
    let document = cad_document(cad);
    assert_eq!(document, Document::Path(cad.to_path_buf()));
    assert_eq!(reveal_target(&document), Some(CadTarget::File(cad.to_path_buf())));
}

/// A scratch copy of the committed REST-built quadruped (one agent thread).
fn scratch_quadruped(name: &str) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/cad-agent-loop/quadruped.rcad");
    let dir = std::env::temp_dir().join(format!("robot-threads-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("quadruped.rcad");
    std::fs::copy(&source, &path).unwrap();
    path
}

#[test]
fn a_change_is_written_to_the_rcad_in_process_and_refused_when_the_file_moved() {
    let path = scratch_quadruped("write");
    let archive = sim_cad::ArchiveDocument::open(&path).unwrap();
    let (threads, parents) = read::list_archive(&archive, &sim_cad::annotations::pinned_stamps(&archive)).unwrap();
    assert_eq!(threads.len(), 1);
    assert!(parents.len() > 10, "the manifest's nodes give the ancestor walk");
    let thread = threads[0].id.clone();
    let identity = archive.identity().to_string();
    let revision = archive.manifest["revision"].as_u64().unwrap();
    // A reply, as the composer sends it: written, saved, a person's comment.
    let reply = Request::Reply { thread: thread.clone(), body: "Checked from Robot mode".into(), author: "You".into() };
    let answer = commit_job(&path, &identity, reply).unwrap();
    assert_eq!(answer["thread"], json!(thread));
    let after = sim_cad::ArchiveDocument::open(&path).unwrap();
    assert_eq!(after.manifest["revision"].as_u64(), Some(revision + 1));
    let (threads, _) = read::list_archive(&after, &sim_cad::annotations::pinned_stamps(&after)).unwrap();
    let last = threads[0].comments.last().unwrap();
    assert_eq!((last.body.as_str(), last.author.as_str()), ("Checked from Robot mode", "You"));
    // The same change against the bytes first read: refused, nothing written.
    let before = std::fs::read(&path).unwrap();
    let again = Request::Reply { thread, body: "Twice".into(), author: "You".into() };
    assert_eq!(commit_job(&path, &identity, again), Err(changed_on_disk(&path)));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn rest_ops_parse_and_name_their_arguments() {
    assert_eq!(from_rest(None, None, None, None, None, None), Ok(ThreadsAct::State));
    assert_eq!(from_rest(Some("refresh"), None, None, None, None, None), Ok(ThreadsAct::Refresh));
    assert_eq!(from_rest(Some("reply"), Some("t1".into()), None, Some("Fixed".into()), None, None), Ok(ThreadsAct::Reply { thread: Some("t1".into()), body: "Fixed".into(), author: None }));
    assert_eq!(from_rest(Some("resolve"), None, None, None, None, Some(true)), Ok(ThreadsAct::Resolve { thread: None, resolved: Some(true) }));
    assert_eq!(from_rest(Some("open_in_cad"), Some("t1".into()), None, None, None, None), Ok(ThreadsAct::OpenInCad { thread: Some("t1".into()) }));
    assert_eq!(from_rest(Some("open"), None, None, None, None, None), Err("robot_threads op `open` needs thread".to_string()));
    assert_eq!(from_rest(Some("edit"), None, Some("c1".into()), None, None, None), Err("robot_threads op `edit` needs body".to_string()));
    assert_eq!(from_rest(Some("state"), Some("t1".into()), None, None, None, None), Err("robot_threads op `state` takes no thread".to_string()));
    assert!(from_rest(Some("create"), None, None, None, None, None).unwrap_err().starts_with("unknown robot_threads op `create`"));
}

#[test]
fn a_landed_change_reads_again_and_ends_the_draft_it_sent() {
    let mut st = open_state();
    st.compose = "Fixed".into();
    st.current = Some("t1".into());
    st.in_flight.submitted(3, crate::cad::threads::REPLY, Some("t1".into()));
    st.sending = Some((3, "Fixed".into()));
    let epoch = st.read.epoch;
    land(&mut st, 3, Ok(json!({"id": "c9"})));
    assert_eq!(st.read.epoch, epoch + 1);
    assert!(st.compose.is_empty() && st.release && st.sending.is_none());
    assert_eq!(st.answers.get(&3), Some(&Ok(Some("c9".to_string()))));
    // A refusal stays under the composer.
    st.compose = "Again".into();
    st.in_flight.submitted(4, crate::cad::threads::REPLY, Some("t1".into()));
    st.sending = Some((4, "Again".into()));
    land(&mut st, 4, Err(changed_on_disk(Path::new("/w/robot.rcad"))));
    assert_eq!(st.error.as_deref(), Some("/w/robot.rcad changed on disk since its comments were read: nothing was written; they are read again"));
    assert_eq!(st.compose, "Again");
}
