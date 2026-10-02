//! Written, unexecuted fixtures; no user preference paths or hardware.
use super::*;

#[test]
fn readiness_is_visible_without_a_second_disk_backend() {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let pending = discover(ViewerMode::Build, None, None, Recents::default(), false, &cancel);
    assert!(pending.sections[0].empty.contains("still loading"));
    let mut recents = Recents::default();
    recents.record(ViewerMode::Build, &Document::Preset("retained".into()), 42);
    let loaded = discover(ViewerMode::Build, None, None, recents, true, &cancel);
    assert_eq!(loaded.sections[0].choices[0].document, Document::Preset("retained".into()));
}

#[test]
fn late_readiness_supersedes_discovery_without_replacing_picker_edits() {
    let mut picker = Picker::default();
    picker.open = Some(ViewerMode::Cad);
    picker.from = Some(ViewerMode::Inspect);
    picker.draft = TextDraft { text: "/typed/new.rcad".into(), select_all: false };
    picker.edited = true;
    picker.scroll = 81.0;
    picker.listing_asked = Some("retained-listing".into());
    picker.refresh_recents(false, &Recents::default());
    let initial = picker.sources.pending().unwrap();
    let mut recents = Recents::default();
    recents.record(ViewerMode::Cad, &Document::Url("http://isolated.invalid".into()), 2);
    picker.refresh_recents(true, &recents);
    let replacement = picker.sources.pending().unwrap();
    assert!(replacement > initial, "Latest discards the older discovery publication");
    assert_eq!(picker.draft.text, "/typed/new.rcad");
    assert!(picker.edited);
    assert_eq!(picker.scroll, 81.0);
    assert_eq!(picker.listing_asked.as_deref(), Some("retained-listing"));
    picker.refresh_recents(true, &recents);
    assert_eq!(picker.sources.pending(), Some(replacement), "unchanged readiness does not restart the walk");
    picker.close();
    picker.refresh_recents(true, &recents);
    assert!(picker.sources.pending().is_none(), "a late load cannot reopen a closed picker");
    assert!(picker.open.is_none());
}
