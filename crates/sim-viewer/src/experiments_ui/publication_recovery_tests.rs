//! Production poll/recovery adapters; fixtures written, deliberately UNEXECUTED.
use super::*;
use sim_runtime::publication::{Hooks,Stage};
use std::{io,path::Path};
struct Fault(Stage);
impl Hooks for Fault {
    fn before(&self, stage:Stage, _: &Path, _: &Path)->io::Result<()> {
        if stage==self.0 {Err(io::Error::other("injected publication failure"))} else {Ok(())}
    }
}
fn captured(p:&ExperimentsPanel,path:PathBuf)->PublicationCapture {
    PublicationCapture{index:0,revision:p.revisions[0],kind:PublicationKind::Portable,destination:path,study:p.studies[0].clone()}
}
fn deliver(p:&mut ExperimentsPanel,capture:PublicationCapture,result:Result<String,String>,cancelled:bool) {
    let (tx,rx)=mpsc::channel();
    let pending=capture.clone();
    tx.send(ResultMessage::Saved(capture,result)).unwrap();
    p.job=Some(Job{rx,cancel:Arc::new(AtomicBool::new(cancelled)),progress:Arc::new(AtomicUsize::new(0)),total:0,label:"captured fixture".into(),publication:Some(pending)});
    p.poll(&egui::Context::default());
}
#[test]
fn failed_cancelled_and_stale_poll_keeps_exact_capture_without_overwriting_edits() {
    for (result,cancelled) in [(Err("failed immutable destination".into()),false),(Err("cancelled before publication".into()),true),(Ok("completed after cancellation".into()),true),(Ok("completed stale capture".into()),false)] {
        let mut p=tests::panel();p.revisions[0]=1;
        p.studies[0].notes="captured intent".into();
        let exact=vec![255,0,42,0];let reference=p.studies[0].input_contents.capture(exact.clone());
        let capture=captured(&p,"original.simstudy".into());
        p.studies[0].notes="newer edited intent".into();p.revisions[0]+=1;
        deliver(&mut p,capture,result,cancelled);
        assert_eq!(p.studies[0].notes,"newer edited intent");assert_eq!(p.saved_revisions[0],0);
        let recovery=&p.publication_recoveries[0];
        assert_eq!(recovery.capture.destination,PathBuf::from("original.simstudy"));
        assert_eq!(recovery.capture.revision,1);assert!(matches!(recovery.capture.kind,PublicationKind::Portable));
        assert_eq!(recovery.capture.study.input_contents.resolve(&reference.blake3).unwrap(),exact);
        p.open_publication_recovery(0);
        assert_eq!(p.studies[0].notes,"newer edited intent");assert_eq!(p.studies[1].notes,"captured intent");
        assert_ne!(p.revisions[1],p.saved_revisions[1]);
        let bytes=p.studies[1].portable_bytes().unwrap();
        let reopened=Study::load_bytes(Path::new("relocated/recovered.simstudy"),&bytes).unwrap();
        assert_eq!(reopened.input_contents.resolve(&reference.blake3).unwrap(),exact);
    }
}
#[test]
fn real_publication_faults_keep_capture_and_visible_unconfirmed_destination_untouched() {
    for stage in [Stage::Write,Stage::DirectorySync] {
        let root=std::env::temp_dir().join(format!("legacy-capture-{}-{:?}",std::process::id(),stage));
        std::fs::create_dir_all(&root).unwrap();
        let mut p=tests::panel();p.revisions[0]=1;p.studies[0].notes="exact captured notes".into();
        let capture=captured(&p,root.join("evidence.simstudy"));
        let result=capture.study.save_portable_new_with(&capture.destination,&Fault(stage)).map(|_|"published".into());
        assert!(result.is_err());
        let before=std::fs::read(&capture.destination).ok();
        assert_eq!(before.is_some(),stage==Stage::DirectorySync);
        if let Some(bytes)=&before {assert_eq!(Study::load_bytes(&capture.destination,bytes).unwrap().notes,"exact captured notes");}
        p.studies[0].notes="newer notes".into();p.revisions[0]+=1;
        deliver(&mut p,capture.clone(),result,false);p.open_publication_recovery(0);
        assert_eq!(std::fs::read(&capture.destination).ok(),before);
        assert_eq!(p.saved_revisions[0],0);assert_eq!(p.studies[0].notes,"newer notes");
        if before.is_some() {
            assert!(capture.kind.publish(&capture.study,&capture.destination).is_err());
            assert_eq!(std::fs::read(&capture.destination).ok(),before);
        }
        // Only fixture-created temporary files; no retained project evidence is removed.
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn disconnected_worker_retains_pending_snapshot() {
    let mut p=tests::panel();let capture=captured(&p,"possibly-visible.simstudy".into());
    let (tx,rx)=mpsc::channel();drop(tx);
    p.job=Some(Job{rx,cancel:Arc::new(AtomicBool::new(false)),progress:Arc::new(AtomicUsize::new(0)),total:0,label:"disconnected fixture".into(),publication:Some(capture)});
    p.studies[0].notes="later edit".into();p.revisions[0]+=1;
    p.poll(&egui::Context::default());
    assert_eq!(p.publication_recoveries.len(),1);assert_eq!(p.studies[0].notes,"later edit");
}
