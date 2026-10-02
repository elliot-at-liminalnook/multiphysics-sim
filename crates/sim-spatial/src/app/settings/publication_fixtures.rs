//! T51 source-only fixtures through shared stages and the actual acknowledgment owner.
use super::*;
use serde_json::json;
use sim_runtime::publication::{Hooks, Stage};
use std::{io, path::Path};
struct Fail(Stage);
impl Hooks for Fail {
    fn before(&self, stage: Stage, _: &Path, _: &Path) -> io::Result<()> {
        if stage == self.0 { Err(io::Error::other("injected settings publication failure")) } else { Ok(()) }
    }
}
fn fixture(name: &str) -> (SettingsOwner, jobs::Paths) {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("settings-t51-{}-{nonce}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let paths = jobs::Paths { unified: Some(dir.join("viewer.json")), recent: None, hardware: dir.join("hardware.json") };
    let mut owner = SettingsOwner::default();
    owner.ready = true;
    owner.paths = Some(paths.clone());
    owner.revision = 1;
    (owner, paths)
}
fn attempt(owner: &mut SettingsOwner, paths: &jobs::Paths, snapshot: &Value, stage: Stage) {
    owner.save_revision = Some(owner.revision);
    let answer = publication::publish_ordered_with(paths, snapshot, owner.revision, &owner.gate, &Fail(stage));
    plugin::land_save(owner, answer);
}
fn finish(mut owner: SettingsOwner, paths: jobs::Paths) {
    owner.ready = false; // No shutdown job is authorized by fixture teardown.
    std::fs::remove_dir_all(paths.unified.unwrap().parent().unwrap()).unwrap();
}
#[test]
fn prepublication_stages_keep_actual_owner_dirty_and_destination_absent() {
    for stage in [Stage::CreateTemp, Stage::Write, Stage::FileSync, Stage::Publish] {
        let (mut owner, paths) = fixture("unpublished");
        attempt(&mut owner, &paths, &json!({"schema":1,"retained":"bytes"}), stage);
        assert!(owner.dirty());
        assert_eq!(owner.saved_revision, 0);
        assert!(!paths.unified.as_ref().unwrap().exists());
        assert!(matches!(owner.drain_status(), DrainStatus::PublicationFailed(_)));
        assert_eq!(owner.gate.lock().unwrap().visible_revision, 0);
        finish(owner, paths);
    }
}
#[test]
fn visible_unconfirmed_retry_synchronizes_before_acknowledging() {
    let (mut owner, paths) = fixture("retry");
    let snapshot = json!({"schema":1,"retained":"exact input"});
    attempt(&mut owner, &paths, &snapshot, Stage::DirectorySync);
    assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(paths.unified.as_ref().unwrap()).unwrap()).unwrap(), snapshot);
    assert_eq!(owner.gate.lock().unwrap().visible_revision, 1);
    assert_eq!(owner.gate.lock().unwrap().revision, 0);
    assert!(owner.dirty());
    owner.retry();
    // This retry fails file-sync in confirm_existing, rather than writing again.
    attempt(&mut owner, &paths, &snapshot, Stage::FileSync);
    assert!(owner.dirty());
    owner.retry();
    owner.save_revision = Some(1);
    let answer = jobs::publish_ordered(&paths, &snapshot, 1, &owner.gate);
    plugin::land_save(&mut owner, answer);
    assert_eq!(owner.saved_revision, 1);
    assert!(owner.drain_ready());
    finish(owner, paths);
}
#[test]
fn observed_external_edit_survives_retry_and_never_acknowledges() {
    let (mut owner, paths) = fixture("external");
    let snapshot = json!({"schema":1,"ours":true});
    attempt(&mut owner, &paths, &snapshot, Stage::DirectorySync);
    let external = b"{\"schema\":1,\"external\":true}";
    std::fs::write(paths.unified.as_ref().unwrap(), external).unwrap();
    owner.retry();
    owner.save_revision = Some(1);
    let answer = jobs::publish_ordered(&paths, &snapshot, 1, &owner.gate);
    plugin::land_save(&mut owner, answer);
    assert_eq!(std::fs::read(paths.unified.as_ref().unwrap()).unwrap(), external);
    assert!(owner.dirty());
    assert!(!owner.drain_ready());
    finish(owner, paths);
}
#[test]
fn visible_newer_revision_excludes_old_writes_and_late_acknowledgments() {
    let (mut owner, paths) = fixture("floor");
    let old = json!({"schema":1,"revision":1});
    let new = json!({"schema":1,"revision":2});
    jobs::publish_ordered(&paths, &old, 1, &owner.gate).unwrap();
    owner.revision = 2;
    attempt(&mut owner, &paths, &new, Stage::DirectorySync);
    assert!(jobs::publish_ordered(&paths, &old, 1, &owner.gate).is_err());
    assert!(jobs::publish_ordered(&paths, &old, 2, &owner.gate).is_err());
    owner.save_revision = Some(1);
    plugin::land_save(&mut owner, Ok(1));
    assert_eq!(owner.saved_revision, 0);
    assert!(!owner.drain_ready());
    assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(paths.unified.as_ref().unwrap()).unwrap()).unwrap(), new);
    finish(owner, paths);
}
#[test]
fn colliding_temporary_is_not_owned_or_removed_by_consumer() {
    use std::sync::Mutex;
    struct Collision(Mutex<Option<std::path::PathBuf>>);
    impl Hooks for Collision {
        fn before(&self, stage: Stage, _: &Path, working: &Path) -> io::Result<()> {
            if stage == Stage::CreateTemp {
                std::fs::write(working, b"somebody else's temporary")?;
                *self.0.lock().unwrap() = Some(working.to_path_buf());
            }
            Ok(())
        }
    }
    let (mut owner, paths) = fixture("collision");
    let hooks = Collision(Mutex::new(None));
    owner.save_revision = Some(1);
    let answer = publication::publish_ordered_with(&paths, &json!({"schema":1}), 1, &owner.gate, &hooks);
    plugin::land_save(&mut owner, answer);
    assert!(owner.dirty());
    assert!(!paths.unified.as_ref().unwrap().exists());
    assert_eq!(std::fs::read(hooks.0.lock().unwrap().as_ref().unwrap()).unwrap(), b"somebody else's temporary");
    finish(owner, paths);
}
#[test]
fn revision_changes_while_draining_require_the_new_confirmed_snapshot() {
    let (mut owner, paths) = fixture("draining");
    let old = json!({"schema":1,"revision":1});
    attempt(&mut owner, &paths, &old, Stage::DirectorySync);
    let stamp = owner.drain_stamp();
    let mut cad = owner.cad.clone();
    cad.clearance += 0.1;
    owner.set_cad(cad).unwrap();
    assert_ne!(owner.drain_stamp(), stamp);
    let answer = jobs::publish_ordered(&paths, &old, 1, &owner.gate);
    owner.save_revision = Some(1);
    plugin::land_save(&mut owner, answer);
    assert_eq!(owner.saved_revision, 1);
    assert!(owner.dirty());
    assert!(!owner.drain_ready());
    let current = json!({"schema":1,"revision":2});
    owner.save_revision = Some(2);
    let answer = jobs::publish_ordered(&paths, &current, 2, &owner.gate);
    plugin::land_save(&mut owner, answer);
    assert!(owner.drain_ready());
    finish(owner, paths);
}
