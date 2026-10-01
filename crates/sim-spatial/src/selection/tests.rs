//! The one selection (windowless): ops, stale-revision refusals, re-checks
//! after a document advances, and the apply system.
use super::*;
use crate::document::{DocumentKind, Source};

fn registry() -> (DocumentRegistry, DocumentId) {
    let mut r = DocumentRegistry::default();
    let id = r.open(ViewerMode::Build, DocumentKind::System, Source::path("a.system.json")).id;
    (r, id)
}
fn component(id: &str) -> Item {
    Item::Component { id: id.into() }
}

#[test]
fn set_add_toggle_remove_and_clear_combine_as_the_modes_did() {
    let (r, d) = registry();
    let mut s = Selection::default();
    assert!(s.apply(&r, &SelectionAction::set(d, [component("a"), component("b"), component("a")])).unwrap());
    assert_eq!(s.components(d), ["a", "b"], "duplicates dropped, order kept");
    s.apply(&r, &SelectionAction::new(Op::Add, d, [component("b"), component("c")])).unwrap();
    assert_eq!(s.components(d), ["a", "b", "c"]);
    s.apply(&r, &SelectionAction::new(Op::Toggle, d, [component("a"), component("d")])).unwrap();
    assert_eq!(s.components(d), ["b", "c", "d"]);
    s.apply(&r, &SelectionAction::new(Op::Remove, d, [component("c")])).unwrap();
    assert_eq!(s.components(d), ["b", "d"]);
    let before = s.changed;
    assert!(!s.apply(&r, &SelectionAction::set(d, [component("b"), component("d")])).unwrap(), "the same selection is no change");
    assert_eq!(s.changed, before);
    s.apply(&r, &SelectionAction::clear(d)).unwrap();
    assert!(s.is_empty_for(d));
}

#[test]
fn an_item_picked_at_an_older_revision_is_refused_by_name() {
    let (mut r, d) = registry();
    let mut s = Selection::default();
    r.set_revision(d, 5);
    let err = s.apply(&r, &SelectionAction::picked(Op::Set, d, 4, [component("motor")])).unwrap_err();
    assert!(err.contains("component motor") && err.contains("revision 4") && err.contains("revision 5"), "{err}");
    assert!(s.is_empty_for(d), "nothing applied");
    s.apply(&r, &SelectionAction::picked(Op::Set, d, 5, [component("motor")])).unwrap();
    assert_eq!(s.of(d).next().unwrap().revision, 5);
    let err = s.apply(&r, &SelectionAction::set(DocumentId(99), [component("x")])).unwrap_err();
    assert!(err.contains("99"), "{err}");
}

#[test]
fn a_document_that_advanced_restamps_keeps_or_drops_its_items_by_name() {
    let (mut r, d) = registry();
    let mut s = Selection::default();
    s.apply(&r, &SelectionAction::set(d, [component("kept"), component("gone"), Item::Cad(SelectionItem("n1".into(), "face".into(), 3))])).unwrap();
    r.set_revision(d, 1);
    let dropped = s.revalidate(d, 1, |item| match item {
        Item::Component { id } if id == "gone" => Recheck::Drop,
        Item::Cad(_) => Recheck::Keep,
        _ => Recheck::Restamp,
    });
    assert_eq!(dropped, ["component gone"]);
    assert_eq!(s.dropped, ["component gone"]);
    let stamps: Vec<(Item, u64)> = s.of(d).map(|x| (x.item.clone(), x.revision)).collect();
    assert_eq!(stamps, [(component("kept"), 1), (Item::Cad(SelectionItem("n1".into(), "face".into(), 3)), 0)], "a CAD face stays at the revision it was picked at");
    // A link is re-found by name across a reload.
    let mut s = Selection::default();
    s.apply(&r, &SelectionAction::set(d, [Item::Link { index: 2, name: "shin".into() }])).unwrap();
    r.set_revision(d, 2);
    s.revalidate(d, 2, |item| {
        if let Item::Link { index, .. } = item {
            *index = 0;
        }
        Recheck::Restamp
    });
    assert_eq!(s.link(d), Some(0));
    s.forget(d);
    assert!(s.all().is_empty());
}

#[test]
fn the_apply_system_applies_picks_and_answers_rest() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin)).insert_state(ViewerMode::Build).configure_sets(Update, (ViewerSet::Input, ViewerSet::Actions).chain());
    build(&mut app);
    let d = app.world_mut().resource_mut::<DocumentRegistry>().open(ViewerMode::Build, DocumentKind::System, Source::path("a.system.json")).id;
    app.world_mut().write_message(Act::ui(SelectionAction::set(d, [component("motor")])));
    let reply = app.world_mut().resource_mut::<Replies>().open();
    app.world_mut().write_message(Act { action: SelectionAction::picked(Op::Add, d, 7, [component("drum")]), origin: actions::Origin::Rest(reply) });
    app.update();
    assert_eq!(app.world().resource::<Selection>().components(d), ["motor"]);
    match app.world_mut().resource_mut::<Replies>().take(reply) {
        Some(Outcome::Done(Err(e))) => assert!(e.contains("component drum"), "{e}"),
        _ => panic!("expected a refusal naming the item"),
    }
}
