//! Meaningful source-written fixtures; not executed in T51.
use super::*;
use std::sync::Mutex;
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("t51-publication-{}-{}",std::process::id(),NEXT_TEMP.fetch_add(1,Ordering::Relaxed)));
        fs::create_dir(&path).unwrap(); Self(path)
    }
}
impl Drop for Dir { fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); } }
struct Fail(Stage);
impl Hooks for Fail {
    fn before(&self, stage:Stage, _: &Path, _: &Path)->io::Result<()> {
        if stage==self.0 {Err(io::Error::other("injected publication failure"))} else {Ok(())}
    }
}
#[test]
fn every_prepublication_stage_preserves_existing_destination() {
    for stage in [Stage::PrepareParent,Stage::CreateTemp,Stage::Write,Stage::FileSync,Stage::Publish] {
        let dir=Dir::new(); let path=dir.0.join("evidence");fs::write(&path,b"prior").unwrap();
        let result=publish_with(&path,b"next",Policy::Replace,&Fail(stage));
        assert!(matches!(result,Outcome::Unpublished(Failure{stage:s,..}) if s==stage));
        assert_eq!(fs::read(path).unwrap(),b"prior");
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(),1);
    }
}
#[test]
fn immutable_conflict_and_unconfirmed_visibility_are_distinct() {
    let dir=Dir::new();let path=dir.0.join("evidence");
    let result=publish_with(&path,b"first",Policy::ImmutableNew,&Fail(Stage::DirectorySync));
    assert!(matches!(result,Outcome::VisibleUnconfirmed(Failure{stage:Stage::DirectorySync,..})));
    assert_eq!(fs::read(&path).unwrap(),b"first");
    assert!(matches!(publish(&path,b"second",Policy::ImmutableNew),Outcome::Unpublished(Failure{stage:Stage::Publish,kind:io::ErrorKind::AlreadyExists,..})));
    assert_eq!(fs::read(&path).unwrap(),b"first");
    confirm_existing(&path).into_result().unwrap();
}
#[test]
fn temporary_collision_does_not_authorize_cleanup() {
    struct Collision(Mutex<Option<PathBuf>>);
    impl Hooks for Collision {
        fn before(&self,stage:Stage,_:&Path,working:&Path)->io::Result<()> {
            if stage==Stage::CreateTemp {fs::write(working,b"someone else's temporary")?;*self.0.lock().unwrap()=Some(working.to_path_buf());} Ok(())
        }
    }
    let dir=Dir::new();let path=dir.0.join("evidence");let hooks=Collision(Mutex::new(None));
    assert!(matches!(publish_with(&path,b"next",Policy::ImmutableNew,&hooks),Outcome::Unpublished(Failure{stage:Stage::CreateTemp,kind:io::ErrorKind::AlreadyExists,..})));
    assert!(!path.exists());
    let temp=hooks.0.lock().unwrap().clone().unwrap();
    assert_eq!(fs::read(temp).unwrap(),b"someone else's temporary");
}
#[test]
fn cleanup_diagnostic_does_not_hide_confirmed_or_unpublished_status() {
    let dir=Dir::new();let path=dir.0.join("evidence");
    let result=publish_with(&path,b"first",Policy::ImmutableNew,&Fail(Stage::Cleanup));
    assert!(matches!(&result,Outcome::Confirmed{cleanup_error:Some(_)}));
    assert!(result.into_result().is_err());
    assert_eq!(fs::read(&path).unwrap(),b"first");
    struct Both;
    impl Hooks for Both {
        fn before(&self,stage:Stage,_:&Path,_:&Path)->io::Result<()> {
            if matches!(stage,Stage::Write|Stage::Cleanup) {Err(io::Error::other("injected"))} else {Ok(())}
        }
    }
    assert!(matches!(publish_with(&dir.0.join("other"),b"next",Policy::ImmutableNew,&Both),Outcome::Unpublished(Failure{stage:Stage::Write,cleanup_error:Some(_),..})));
}
#[test]
fn new_directory_ancestry_is_confirmed_through_root() {
    struct Observe(Mutex<Vec<PathBuf>>);
    impl Hooks for Observe {
        fn before(&self,stage:Stage,_:&Path,working:&Path)->io::Result<()> {
            if stage==Stage::DirectorySync {self.0.lock().unwrap().push(working.to_path_buf());} Ok(())
        }
    }
    let dir=Dir::new();let path=dir.0.join("new/nested/evidence");let hooks=Observe(Mutex::new(vec![]));
    publish_with(&path,b"bytes",Policy::ImmutableNew,&hooks).into_result().unwrap();
    let parent=fs::canonicalize(parent(&path)).unwrap();
    assert_eq!(*hooks.0.lock().unwrap(),parent.ancestors().map(Path::to_path_buf).collect::<Vec<_>>());
}
