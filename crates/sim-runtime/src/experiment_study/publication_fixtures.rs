//! Unexecuted fixtures through the actual Study adapters and shared writer.
use super::{input_content::Store, input_content_fixtures::{directory, envelope, study}};
use crate::publication::{Hooks, Stage};
use std::{io, path::{Path, PathBuf}};

struct Fail { target: PathBuf, stage: Stage }
impl Hooks for Fail {
    fn before(&self, stage: Stage, destination: &Path, _: &Path) -> io::Result<()> {
        if destination == self.target && stage == self.stage {
            Err(io::Error::other("Study fixture injected failure"))
        } else { Ok(()) }
    }
}

#[test]
fn manifest_failures_never_acknowledge_or_consume_retained_input() {
    for stage in [Stage::Write, Stage::FileSync, Stage::Publish, Stage::DirectorySync] {
        let dir = directory();
        let path = dir.join("manifest.json");
        let mut captured = study();
        let bytes = b"exact rejected input".to_vec();
        let reference = envelope(&mut captured, bytes.clone());
        let result = captured.save_new_with(&path, &Fail { target: path.clone(), stage });
        assert!(result.is_err(), "unconfirmed work must not acknowledge success");
        assert_eq!(captured.input_contents.resolve(&reference.blake3).unwrap(), bytes);
        assert_eq!(std::fs::read(Store::artifact_path(&path, &reference.blake3)).unwrap(), bytes);
        assert_eq!(path.exists(), stage == Stage::DirectorySync);
        // Immutable artifacts require a fresh manifest name on retry, even if visible.
        let retry = dir.join("retry.json");
        captured.save_new(&retry).unwrap();
        assert_eq!(super::Study::load(&retry).unwrap().input_contents.resolve(&reference.blake3).unwrap(), bytes);
    }
}

#[test]
fn unconfirmed_companion_is_error_then_verified_synchronized_retry() {
    let dir = directory(); let path = dir.join("manifest.json"); let mut captured = study();
    let reference = envelope(&mut captured, b"retained exact bytes".to_vec());
    let companion = Store::artifact_path(&path, &reference.blake3);
    assert!(captured.save_new_with(&path, &Fail { target: companion.clone(), stage: Stage::DirectorySync }).is_err());
    assert!(companion.exists()); assert!(!path.exists());
    // Existing matching bytes do not swallow a second confirmation failure.
    assert!(captured.save_new_with(&path, &Fail { target: companion.clone(), stage: Stage::FileSync }).is_err());
    assert!(!path.exists());
    captured.save_new(&path).unwrap();
    assert_eq!(super::Study::load(&path).unwrap().input_contents, captured.input_contents);
}

struct ConcurrentCompanion { target: PathBuf, bytes: Vec<u8> }
impl Hooks for ConcurrentCompanion {
    fn before(&self, stage: Stage, destination: &Path, _: &Path) -> io::Result<()> {
        if stage == Stage::Publish && destination == self.target {
            std::fs::write(destination, &self.bytes)?;
        }
        Ok(())
    }
}
#[test]
fn concurrent_companion_identity_is_verified_before_reuse() {
    for corrupt in [false, true] {
        let dir = directory(); let path = dir.join("manifest.json"); let mut captured = study();
        let bytes = b"captured content".to_vec();
        let reference = envelope(&mut captured, bytes.clone());
        let companion = Store::artifact_path(&path, &reference.blake3);
        let winner = if corrupt { b"corrupt".to_vec() } else { bytes };
        let result = captured.save_new_with(&path, &ConcurrentCompanion { target: companion.clone(), bytes: winner.clone() });
        assert_eq!(result.is_ok(), !corrupt);
        assert_eq!(std::fs::read(companion).unwrap(), winner);
        assert_eq!(path.exists(), !corrupt);
        if corrupt { assert!(result.unwrap_err().contains("corrupt artifact")); }
    }
}

struct Collision;
impl Hooks for Collision {
    fn before(&self, stage: Stage, _: &Path, working: &Path) -> io::Result<()> {
        if stage == Stage::CreateTemp { std::fs::write(working, b"somebody else's temporary file")?; }
        Ok(())
    }
}
#[test]
fn temp_collision_preserves_unowned_file_and_report_refuses_existing_destination() {
    let dir = directory(); let path = dir.join("manifest.json"); let captured = study();
    assert!(captured.save_new_with(&path, &Collision).is_err());
    assert!(!path.exists());
    let files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|f| f.unwrap().path()).collect();
    assert_eq!(files.len(), 1);
    assert_eq!(std::fs::read(&files[0]).unwrap(), b"somebody else's temporary file");
    let report = dir.join("report.html"); std::fs::write(&report, b"existing report evidence").unwrap();
    assert!(captured.export_html_new(&report).is_err());
    assert_eq!(std::fs::read(&report).unwrap(), b"existing report evidence");
    let new_report = dir.join("new.html");
    assert!(captured.export_html_new_with(&new_report, &Fail { target: new_report.clone(), stage: Stage::DirectorySync }).is_err());
    assert!(new_report.exists());
}
