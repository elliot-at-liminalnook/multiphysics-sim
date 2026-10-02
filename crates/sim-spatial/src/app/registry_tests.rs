//! Document registry transition cases.
use super::*;

/// The window's switch on MinimalPlugins with what `run` inserts before the
/// plugins (the models and, when given, the launch's registry and scene).
fn registry_app(initial: ViewerMode, registry: Option<DocumentRegistry>, scene: Option<SpatialScene>) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin)).insert_resource(crate::models::ModelLibrary::default());
    if let Some(registry) = registry {
        app.insert_resource(registry);
    }
    if let Some(scene) = scene {
        app.insert_resource(scene);
    }
    app.add_plugins(ModesPlugin { initial });
    app.update();
    app
}

/// `viewer_mode {}` as a REST caller gets it.
fn status(app: &mut App) -> Value {
    let reply = app.world_mut().resource_mut::<Replies>().open();
    app.world_mut().write_message(Act { action: WindowAction::ViewerMode(serde_json::Map::new()), origin: Origin::Rest(reply) });
    settle(app, reply).unwrap()
}

/// The document registry across switches (native-viewer.md §7): a mode
/// reopening its document without a new one is a reload (the id kept, the
/// revision + 1, the selection's items kept); another document gets a new
/// id and the replaced one's items leave the selection; `viewer_mode {}`
/// reports `documents` with its keys and shapes, from the registry.
#[test]
fn a_reopened_document_keeps_its_id_and_a_new_one_gets_a_new_id() {
    use crate::selection::{Item, Op, Selection, SelectionAction};
    let (u1, u2) = ("http://127.0.0.1:1", "http://127.0.0.1:2");
    let mut app = registry_app(ViewerMode::Inspect, None, None);
    let seq = submit(&mut app, ViewerMode::Cad, Some(Document::Url(u1.into())));
    settle(&mut app, seq).unwrap();
    assert_eq!(mode(&app), ViewerMode::Cad);
    let (c1, revision) = app.world().resource::<DocumentRegistry>().current(ViewerMode::Cad).expect("cad's document is open");
    assert_eq!(revision, 0);
    // An item of c1 (the selection is shared by every mode).
    app.world_mut().resource_scope(|world, mut selection: Mut<Selection>| {
        let action = SelectionAction { op: Op::Set, document: c1, items: vec![(Item::Component { id: "body".into() }, None)] };
        assert_eq!(selection.apply(world.resource::<DocumentRegistry>(), &action), Ok(true));
    });

    // Away: the entry stays, remembered, with what CAD reopens.
    let seq = submit(&mut app, ViewerMode::Phenomena, None);
    settle(&mut app, seq).unwrap();
    let registry = app.world().resource::<DocumentRegistry>();
    let entry = registry.entry(ViewerMode::Cad).unwrap();
    assert_eq!((entry.id, entry.presence), (c1, crate::document::Presence::Remembered));
    assert_eq!(registry.current(ViewerMode::Phenomena).map(|(_, r)| r), Some(0), "phenomena's exhibits are its open document");

    // Back with no document: the same one again is a reload.
    let seq = submit(&mut app, ViewerMode::Cad, None);
    settle(&mut app, seq).unwrap();
    assert_eq!(app.world().resource::<DocumentRegistry>().current(ViewerMode::Cad), Some((c1, 1)), "same id, revision + 1");
    assert_eq!(app.world().resource::<Selection>().of(c1).count(), 1, "a reload keeps the document's items");

    // viewer_mode {}: the same keys and shapes as before, plus the registry.
    let documents = status(&mut app)["documents"].clone();
    let keys: BTreeSet<&str> = documents.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, BTreeSet::from(["builder_scene_parked", "cad", "inspect", "inspect_parked", "lessons", "library", "models", "phenomena", "place", "presets", "registry", "robot"]));
    assert_eq!(documents["cad"], json!({"url": u1}));
    assert_eq!(documents["phenomena"], json!({"exhibit": null}));
    assert_eq!(documents["robot"], Value::Null);
    assert_eq!(documents["inspect_parked"], false);
    let cad = documents["registry"].as_array().unwrap().iter().find(|e| e["mode"] == "cad").expect("cad's entry").clone();
    assert_eq!((cad["id"].clone(), cad["kind"].clone(), cad["revision"].clone(), cad["presence"].clone()), (json!(c1.0), json!("cad"), json!(1), json!("open")));
    assert_eq!(cad["source"], json!({"kind": "url", "url": u1}));

    // Phenomena again (its reload), then another document: a new id, c1's items gone.
    let seq = submit(&mut app, ViewerMode::Phenomena, None);
    settle(&mut app, seq).unwrap();
    assert_eq!(app.world().resource::<DocumentRegistry>().current(ViewerMode::Phenomena).map(|(_, r)| r), Some(1));
    app.world_mut().resource_mut::<DocumentRegistry>().remember(ViewerMode::Robot, DocumentKind::Robot, Source::Preset { id: "p".into() });
    assert_eq!(status(&mut app)["documents"]["robot"], json!({"preset": "p"}), "robot's document reads as before");
    let seq = submit(&mut app, ViewerMode::Cad, Some(Document::Url(u2.into())));
    settle(&mut app, seq).unwrap();
    let (c2, revision) = app.world().resource::<DocumentRegistry>().current(ViewerMode::Cad).unwrap();
    assert!(c2 != c1 && revision == 0, "another document: a new id ({c2:?} after {c1:?}) at revision 0");
    assert_eq!(app.world().resource::<Selection>().of(c1).count(), 0, "the replaced document's items are forgotten");
    assert_eq!(status(&mut app)["documents"]["cad"], json!({"url": u2}));
}

/// Inspect's scene and link are parked on its registry entry while another
/// mode is shown and come back from it as they were (no new revision); a
/// load of the same assembly is a reload that replaces the parked scene.
/// The launch's registry (inserted before the plugins, as `run` does) is kept.
#[test]
fn a_parked_inspect_scene_comes_back_through_the_registry() {
    let (description, spatial) = crate::default_inspect_paths();
    let scene = crate::load_inspect(&description, &spatial).unwrap();
    let mut registry = DocumentRegistry::default();
    let launched = registry.open(ViewerMode::Inspect, DocumentKind::Assembly, Source::Assembly { description: description.clone(), spatial: spatial.clone() }).id;
    let mut app = registry_app(ViewerMode::Inspect, Some(registry), Some(scene));
    assert_eq!(app.world().resource::<DocumentRegistry>().current(ViewerMode::Inspect), Some((launched, 0)), "the launch's registry is kept");

    let seq = submit(&mut app, ViewerMode::Phenomena, None);
    settle(&mut app, seq).unwrap();
    assert!(!app.world().contains_resource::<SpatialScene>());
    let registry = app.world().resource::<DocumentRegistry>();
    assert!(registry.is_parked(ViewerMode::Inspect) && registry.current(ViewerMode::Inspect).is_none(), "parked, not open");
    let documents = status(&mut app)["documents"].clone();
    assert_eq!(documents["inspect_parked"], true);
    assert_eq!(documents["inspect"], json!({"description": description, "spatial": spatial}));

    let seq = submit(&mut app, ViewerMode::Inspect, None);
    settle(&mut app, seq).unwrap();
    assert!(app.world().contains_resource::<SpatialScene>(), "the parked scene is back");
    let registry = app.world().resource::<DocumentRegistry>();
    assert!(!registry.is_parked(ViewerMode::Inspect));
    assert_eq!(registry.current(ViewerMode::Inspect), Some((launched, 0)), "unparked, not reloaded");

    // The same assembly loaded again: a reload; the parked scene is replaced.
    let seq = submit(&mut app, ViewerMode::Phenomena, None);
    settle(&mut app, seq).unwrap();
    let seq = submit(&mut app, ViewerMode::Inspect, Some(Document::Path(description.clone())));
    settle(&mut app, seq).unwrap();
    assert!(app.world().contains_resource::<SpatialScene>());
    let registry = app.world().resource::<DocumentRegistry>();
    assert!(!registry.is_parked(ViewerMode::Inspect));
    assert_eq!(registry.current(ViewerMode::Inspect), Some((launched, 1)), "a reload keeps the id");
}
