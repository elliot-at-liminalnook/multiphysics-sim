use super::*;
use super::super::system_actions::UiAction;
use super::super::test_support::test_selection;

/// Three resistors at the top level (no nets), saved to a temporary file.
fn fixture(test: impl FnOnce(Builder, SpatialScene)) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("picked-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut doc = SystemDocument::new("Picked fixture");
    let root = doc.definitions.get_mut(&doc.root).unwrap();
    for (name, x) in [("a", 0.), ("b", 0.2), ("c", 0.4)] {
        root.instances.insert(name.into(), InstanceSpec::element("electrical.resistor").with("resistance", 100.).at([x, 0.02, 0.]));
    }
    sim_system::display::assign_ids(&mut doc);
    let path = dir.join("picked.system.json");
    SystemStore::create(&path, &doc).unwrap();
    let b = Builder::open(path, dir.join("library/systems"), sim_runtime::system_registry()).unwrap();
    let flat = sim_system::flatten(&b.document, &b.registry).unwrap();
    let description = sim_inspect::model::describe(&flat.model, &b.registry, &flat.source_hash, flat.revision, &flat.identities).unwrap().description;
    let spatial = flat.spatial(&description.id, &b.document.title);
    let scene = SpatialScene::for_builder(description, spatial).unwrap();
    test(b, scene);
    let _ = std::fs::remove_dir_all(dir);
}

fn orbit() -> Orbit {
    Orbit { focus: Vec3::ZERO, radius: 0.5, yaw: 0.5, pitch: 0.5, home: false, ..Default::default() }
}

/// The Outline row (`BuildAction::Select`) and `system_select`'s adapter, then
/// a shift-click on a part (`BuildAction::PickPart`) and its `system_ui`
/// twin (`click_part`), leave the same shared selection: the same items, of
/// the same document, at the same revision.
#[test]
fn ui_and_rest_selections_give_the_same_shared_selection() {
    fixture(|mut b, mut scene| {
        let mut orbit = orbit();
        let (mut ui_selection, mut ui_documents) = test_selection(&b);
        let mut ui = Picked::new(&mut ui_selection, &mut ui_documents);
        let (mut rest_selection, mut rest_documents) = test_selection(&b);
        let mut rest = Picked::new(&mut rest_selection, &mut rest_documents);

        dispatch(&mut b, &mut scene, &mut orbit, &mut ui, BuildAction::Select("b".into()));
        let state = b.select(&mut rest, vec!["b".into()]).map(|()| rest.state(&b)).unwrap();
        assert_eq!(state["selected"], serde_json::json!(["b"]));
        ui.sync(&b);
        assert_eq!(ui.selection.all(), rest.selection.all());
        assert_eq!(ui.names(), BTreeSet::from(["b".to_string()]));

        // The schematic box is the Outline's path.
        dispatch(&mut b, &mut scene, &mut orbit, &mut ui, BuildAction::SchematicSelect("a".into()));
        b.select(&mut rest, vec!["a".into()]).unwrap();
        assert_eq!(ui.selection.all(), rest.selection.all());

        // Shift-click adds (a 3D pick in Build, and REST system_ui click_part).
        dispatch(&mut b, &mut scene, &mut orbit, &mut ui, BuildAction::PickPart { index: 0, component: "c".into(), add: true, world: None });
        let state = b.ui_request(UiAction::ClickPart { component: "c".into(), add: true, point_m: None }, None, &mut scene, &mut orbit, &mut rest).unwrap();
        ui.sync(&b);
        assert_eq!(ui.selection.all(), rest.selection.all());
        assert_eq!(state["selected"], serde_json::json!(["a", "c"]));

        // An unknown name is refused by the adapter, naming it; nothing changes.
        let before = rest.selection.all().to_vec();
        let e = b.select(&mut rest, vec!["nope".into()]).unwrap_err();
        assert!(e.contains("nope"), "{e}");
        assert_eq!(rest.selection.all(), before.as_slice());
    });
}

/// An edit that removes a selected instance (here another editor's REST
/// `system` command) advances the document; the re-check drops it by name
/// and keeps the rest, restamped at the new revision.
#[test]
fn an_edit_that_removes_a_selected_instance_drops_it_by_name() {
    fixture(|mut b, _| {
        let (mut selection, mut documents) = test_selection(&b);
        let mut pick = Picked::new(&mut selection, &mut documents);
        b.select(&mut pick, vec!["a".into(), "b".into()]).unwrap();
        pick.sync(&b);
        let before = pick.selection.changed;
        b.apply("Delete a", vec![SystemCommand::RemoveInstance { at: String::new(), name: "a".into() }]).unwrap();
        let dropped = pick.sync(&b);
        assert_eq!(dropped, vec!["component a".to_string()]);
        assert_eq!(pick.selection.dropped, dropped);
        assert!(pick.selection.changed > before);
        assert_eq!(pick.names(), BTreeSet::from(["b".to_string()]));
        let document = pick.document().unwrap();
        assert_eq!(pick.registry.revision(document), Some(b.document.revision));
        assert!(pick.selection.of(document).all(|s| s.revision == b.document.revision));
        // The REST answer lists what is left.
        assert_eq!(pick.state(&b)["selected"], serde_json::json!(["b"]));

        // The builder's own delete clears what it removed.
        b.remove_selected(&mut pick);
        assert!(pick.names().is_empty());
        assert!(!b.level_instances().contains("b"));
    });
}
