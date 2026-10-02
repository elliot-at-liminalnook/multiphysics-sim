//! T51 fixtures use the actual Study writer, job adapter and terminal owner.
//! Source-written only: no job is spawned and no fixture was executed this batch.
use super::{jobs::{self, JobKind, PendingJob, PublicationGate}, state::StudyOwner};
use crate::{document::DocumentRegistry, jobs::Job};
use sim_runtime::{experiment_study::Study, publication::{Hooks, NoHooks, Stage}};
use std::{io, path::{Path, PathBuf}, sync::{Arc, Mutex}};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let nonce=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path=std::env::temp_dir().join(format!("t51-native-study-{}-{nonce}",std::process::id()));
        std::fs::create_dir(&path).unwrap(); Self(path)
    }
}
impl Drop for Directory { fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); } }
struct Fail { destination:PathBuf, stage:Stage }
impl Hooks for Fail {
    fn before(&self,stage:Stage,destination:&Path,_:&Path)->io::Result<()> {
        if stage==self.stage && destination==self.destination {Err(io::Error::other("native publication fixture failure"))} else {Ok(())}
    }
}
fn study()->Study {
    let root=crate::workspace::root().unwrap();
    Study::new(sim_runtime::experiment_comparison::hx_archive::load(&root.join(super::super::DEFAULT_ARCHIVE),root).unwrap()).unwrap()
}
fn terminal(owner:&mut StudyOwner,captured:Study,path:&Path,result:Result<jobs::JobOutput,String>,cancelled:bool,displaced:bool) {
    let stamp=owner.active().unwrap().stamp();
    let gate=Arc::new(Mutex::new(PublicationGate::default()));
    let mut pending=PendingJob {
        id:1,kind:JobKind::Save,stamp:Some(stamp),document:None,source:path.display().to_string(),
        trial_ids:vec![],launch:serde_json::Value::Null,cancel_requested:false,
        job:Job::finished(1,result),selection_epoch:owner.selection_epoch,gate:Some(gate),captured:Some(captured),
    };
    if cancelled {pending.cancel();}
    owner.pending.push(pending);
    if displaced {owner.studies.clear();owner.active=None;}
    jobs::poll_owner(owner,&DocumentRegistry::default());
}
#[test]
fn every_writer_failure_reaches_retained_receipt_without_saved_ack() {
    for stage in [Stage::CreateTemp,Stage::Write,Stage::FileSync,Stage::Publish,Stage::DirectorySync] {
        let dir=Directory::new();let path=dir.0.join("review.json");
        let mut owner=StudyOwner::default();let mut input=study();
        let bytes=b"exact invalid additional source".to_vec();let reference=input.input_contents.capture(bytes.clone());
        owner.retain(input.clone(),"source".into(),None,false,true);
        let result=jobs::publish_artifact(&input,&path,false,&Fail{destination:path.clone(),stage});
        terminal(&mut owner,input,&path,result,false,false);
        assert!(owner.active().unwrap().dirty());assert_eq!(owner.active().unwrap().saved_revision,None);
        assert_eq!(owner.active().unwrap().study.input_contents.resolve(&reference.blake3).unwrap(),bytes);
        let receipt=owner.receipts.last().unwrap();assert!(receipt.error.is_some());
        assert_eq!(receipt.captured.as_ref().unwrap().input_contents.resolve(&reference.blake3).unwrap(),bytes);
        assert_eq!(path.exists(),stage==Stage::DirectorySync);
        if stage==Stage::DirectorySync {assert!(receipt.error.as_ref().unwrap().contains("visible but durability unconfirmed"));}
        let retry=dir.0.join("retry.json");
        owner.active().unwrap().study.save_new(&retry).unwrap();
        assert_eq!(Study::load(&retry).unwrap().input_contents.resolve(&reference.blake3).unwrap(),bytes);
    }
}
#[test]
fn cancelled_and_displaced_failed_capture_retains_bytes_without_any_publication() {
    let dir=Directory::new();let path=dir.0.join("cancelled.json");
    let mut owner=StudyOwner::default();let mut input=study();
    let bytes=b"cancelled malformed source".to_vec();let reference=input.input_contents.capture(bytes.clone());
    owner.retain(input.clone(),"source".into(),None,false,true);
    let mut gate=PublicationGate{started:false,cancelled:true};
    let result=jobs::authorize_publication(&mut gate,false).and_then(|_|jobs::publish_artifact(&input,&path,false,&NoHooks));
    assert!(!gate.started);
    terminal(&mut owner,input,&path,result,true,true);
    let receipt=owner.receipts.last().unwrap();assert!(receipt.cancelled && receipt.displaced && receipt.error.is_some());
    let captured=receipt.captured.as_ref().unwrap();
    assert_eq!(captured.input_contents.resolve(&reference.blake3).unwrap(),bytes);
    assert!(!path.exists());assert!(!dir.0.join(".study-inputs").exists());
    // The orphan capture remains the real recovery source, rather than metadata alone.
    let retry=dir.0.join("recovered.json");captured.save_new(&retry).unwrap();
    assert_eq!(Study::load(&retry).unwrap().input_contents.resolve(&reference.blake3).unwrap(),bytes);
}
#[test]
fn visible_failure_displaced_after_publication_retains_capture_and_destination() {
    let dir=Directory::new();let path=dir.0.join("visible.json");
    let mut owner=StudyOwner::default();let mut input=study();
    let bytes=b"displaced exact source".to_vec();let reference=input.input_contents.capture(bytes.clone());
    owner.retain(input.clone(),"source".into(),None,false,true);
    let result=jobs::publish_artifact(&input,&path,false,&Fail{destination:path.clone(),stage:Stage::DirectorySync});
    terminal(&mut owner,input,&path,result,false,true);
    let receipt=owner.receipts.last().unwrap();assert!(receipt.displaced && receipt.error.is_some());
    assert_eq!(receipt.captured.as_ref().unwrap().input_contents.resolve(&reference.blake3).unwrap(),bytes);
    assert!(path.exists());
    let existing=std::fs::read(&path).unwrap();
    assert!(receipt.captured.as_ref().unwrap().save_new(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(),existing);
}
