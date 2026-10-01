//! The registry's ids and revisions (windowless).
use super::*;

#[test]
fn a_reload_keeps_the_id_and_advances_the_revision() {
    let mut r = DocumentRegistry::default();
    let first = r.open(ViewerMode::Build, DocumentKind::System, Source::path("a.system.json"));
    assert!(!first.reload && first.replaced.is_none());
    assert_eq!(r.current(ViewerMode::Build), Some((first.id, 0)));
    // An edit the mode counts.
    assert!(r.set_revision(first.id, 3));
    assert!(!r.set_revision(first.id, 3), "an unchanged count is not a change");
    // The same file again: a reload.
    let again = r.open(ViewerMode::Build, DocumentKind::System, Source::path("a.system.json"));
    assert!(again.reload);
    assert_eq!(again.id, first.id);
    assert_eq!(r.current(ViewerMode::Build), Some((first.id, 4)));
    // Another file: a new id at revision 0, naming the one it replaced.
    let other = r.open(ViewerMode::Build, DocumentKind::System, Source::path("b.system.json"));
    assert!(!other.reload);
    assert_ne!(other.id, first.id);
    assert_eq!(other.replaced, Some(first.id));
    assert_eq!(r.revision(other.id), Some(0));
    assert!(r.get(first.id).is_none(), "one entry per mode");
}

#[test]
fn closing_remembers_and_a_lesson_choice_keeps_the_folder_id() {
    let mut r = DocumentRegistry::default();
    let id = r.open(ViewerMode::Lessons, DocumentKind::Lessons, Source::Lessons { dir: "lessons".into(), lesson: None }).id;
    r.close(ViewerMode::Lessons);
    assert_eq!(r.current(ViewerMode::Lessons), None, "a closed document is not current");
    assert_eq!(r.entry(ViewerMode::Lessons).unwrap().presence, Presence::Remembered);
    r.set_source(ViewerMode::Lessons, Source::Lessons { dir: "lessons".into(), lesson: Some("gears".into()) });
    let again = r.open(ViewerMode::Lessons, DocumentKind::Lessons, Source::Lessons { dir: "lessons".into(), lesson: Some("gears".into()) });
    assert_eq!(again.id, id);
    assert!(again.reload);
    let remembered = r.remember(ViewerMode::Robot, DocumentKind::Robot, Source::Preset { id: "p".into() });
    assert_eq!(r.entry(ViewerMode::Robot).unwrap().presence, Presence::Remembered);
    assert_eq!(r.remember(ViewerMode::Robot, DocumentKind::Robot, Source::Preset { id: "p".into() }), remembered);
}
