//! T47 isolated reading-only fixtures. No paths are read, written or removed.
use super::*;

#[test]
fn clean_load_and_session_only_have_distinct_readiness() {
    let mut owner = SettingsOwner::default();
    assert_eq!(owner.drain_status(), DrainStatus::Loading);
    assert!(!owner.drain_ready());
    owner.ready = true;
    assert_eq!(owner.drain_status(), DrainStatus::ConfigUnavailable);
    assert!(!owner.drain_ready());
    let owner = SettingsOwner::fixture_durable();
    assert_eq!(owner.drain_status(), DrainStatus::Ready { revision: 0 });
}
#[test]
fn protected_load_failure_never_becomes_clean_and_changes_scope() {
    let mut owner = SettingsOwner::default();
    let stamp = owner.drain_stamp();
    plugin::land_load(&mut owner, Err("schema newer than supported".into()));
    assert!(matches!(owner.drain_status(), DrainStatus::ProtectedSource(_)));
    assert!(!owner.drain_ready());
    assert_ne!(stamp, owner.drain_stamp());
    owner.retry(); // No configured path: never substitutes defaults or starts a job.
    assert!(!owner.drain_ready());
}
#[test]
fn required_recent_failure_is_retained_until_successful_normalization_and_save() {
    let mut owner = SettingsOwner::fixture_durable();
    owner.record(ViewerMode::Cad, Document::Url("http://fixture".into()));
    let stamp = owner.drain_stamp();
    plugin::land_canonical(&mut owner, Err("normalization denied".into()));
    assert_eq!(owner.records.len(), 1);
    assert!(matches!(owner.drain_status(), DrainStatus::NormalizationFailed(_)));
    owner.retry();
    assert!(matches!(owner.drain_status(), DrainStatus::Normalizing { pending: 1 }));
    plugin::land_canonical(&mut owner, Ok((ViewerMode::Cad, Document::Url("http://fixture".into()), 1)));
    assert!(owner.records.is_empty());
    assert!(!owner.drain_ready());
    // Normalizing the already accepted record does not widen bypass scope.
    assert_eq!(stamp, owner.drain_stamp());
    let revision = owner.revision;
    owner.fixture_acknowledge(revision);
    assert!(owner.drain_ready());
    owner.paths = None; // Prevent Drop fallback from submitting a job.
}
#[test]
fn active_save_new_edit_and_stale_acknowledgment_cannot_authorize_close() {
    let mut owner = SettingsOwner::fixture_durable();
    let mut cad = owner.cad.clone();
    cad.clearance = 0.4;
    owner.set_cad(cad.clone()).unwrap();
    owner.save_revision = Some(1);
    assert_eq!(owner.drain_status(), DrainStatus::Publishing { revision: 1 });
    let stamp = owner.drain_stamp();
    cad.clearance = 0.5;
    owner.set_cad(cad).unwrap();
    plugin::land_save(&mut owner, Ok(1));
    assert_eq!(owner.saved_revision, 1);
    assert!(!owner.drain_ready());
    assert_ne!(stamp, owner.drain_stamp());
    owner.save_revision = Some(2);
    plugin::land_save(&mut owner, Ok(99));
    assert_eq!(owner.saved_revision, 1);
    assert!(matches!(owner.drain_status(), DrainStatus::PublicationFailed(_)));
    owner.retry();
    owner.fixture_acknowledge(2);
    assert!(owner.drain_ready());
}
#[test]
fn failed_publication_retry_and_new_recent_invalidate_previous_readiness() {
    let mut owner = SettingsOwner::fixture_durable();
    owner.revision = 4;
    owner.save_revision = Some(4);
    plugin::land_save(&mut owner, Err("directory durability unconfirmed".into()));
    assert!(matches!(owner.drain_status(), DrainStatus::PublicationFailed(_)));
    assert!(!owner.drain_ready());
    let revision = owner.revision;
    owner.retry();
    assert_eq!(owner.revision, revision);
    owner.fixture_acknowledge(revision);
    assert!(owner.drain_ready());
    let stamp = owner.drain_stamp();
    owner.record(ViewerMode::Cad, Document::Url("http://later".into()));
    assert!(!owner.drain_ready());
    assert_ne!(stamp, owner.drain_stamp());
    owner.paths = None;
}
#[test]
fn snapshot_failure_is_named_and_retry_cannot_claim_saved() {
    let mut owner = SettingsOwner::fixture_durable();
    owner.revision = 1;
    owner.snapshot_error = Some("retained_rows capacity exceeded".into());
    assert!(matches!(owner.drain_status(), DrainStatus::SnapshotFailed(_)));
    owner.retry();
    assert_eq!(owner.drain_status(), DrainStatus::PendingPublication { revision: 1 });
    assert!(!owner.drain_ready());
    owner.paths = None;
}
