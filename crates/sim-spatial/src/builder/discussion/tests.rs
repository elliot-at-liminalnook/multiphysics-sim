use super::*;
use crate::document::{DocumentKind, Source};
#[test]
fn rest_comments_preserve_the_draft_and_its_target_and_reject_conflicting_edits() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("builder-discussion-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("review.system.json");
    std::fs::copy(
        root.join("examples/systems-builder/worm-drive/winch.system.json"),
        &path,
    )
    .unwrap();
    let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
    let mut b = Builder::open(path.clone(), root.join("library/systems"), registry.clone()).unwrap();
    let compiled = system_builder::compile(
        &b.document,
        &registry,
        system_builder::config_for(&b.document),
    )
    .unwrap();
    let spatial = compiled
        .spatial
        .clone()
        .unwrap_or_else(|| compiled.flat.spatial(&compiled.description.id, "Review"));
    let mut scene = SpatialScene::for_builder(compiled.description, spatial).unwrap();
    let mut orbit = Orbit {
        focus: Vec3::ZERO,
        radius: 0.5,
        yaw: 0.5,
        pitch: 0.5,
        home: false,
        ..Default::default()
    };
    b.scene_dirty = false;
    // The builder's document is open: what a discussion inspects is its shared selection.
    let (mut shared, mut docs) = (Selection::default(), DocumentRegistry::default());
    docs.open(ViewerMode::Build, DocumentKind::System, Source::path(&path));
    let mut pick = Picked::new(&mut shared, &mut docs);
    b.discussion.draft_targets = vec!["motor".into()];
    b.start_input(Purpose::Comment, "My unsent motor note".into());
    b.discussion_request(
        Request::Create {
            title: "Other discussion".into(),
            targets: vec!["gearbox".into()],
            body: "REST comment".into(),
            author: "Codex".into(),
            pin_m: None,
        },
        None,
        &mut scene,
        &mut orbit,
        &mut pick,
    )
    .unwrap();
    assert_eq!(b.input.as_ref().unwrap().buffer, "My unsent motor note");
    assert_eq!(
        b.discussion.selected, None,
        "REST must not retarget the draft"
    );
    assert!(
        !b.scene_dirty,
        "a comment must not rebuild the running model"
    );
    submit(&mut b, &mut scene, &mut orbit, &mut pick);
    assert!(b.input.is_none());
    let id = b.discussion.selected.clone().unwrap();
    let t = &b.document.discussions.threads[&id];
    assert_eq!(t.targets[0].path, "motor");
    let c = t.comments[0].id.clone();
    act(&mut b, &mut scene, &mut orbit, &mut pick, Action::Edit(c.clone()));
    b.input.as_mut().unwrap().buffer = "My local edit".into();
    b.discussion_request(
        Request::EditComment {
            id: id.clone(),
            comment: c,
            body: "External edit".into(),
        },
        None,
        &mut scene,
        &mut orbit,
        &mut pick,
    )
    .unwrap();
    submit(&mut b, &mut scene, &mut orbit, &mut pick);
    assert_eq!(b.input.as_ref().unwrap().buffer, "My local edit");
    assert_eq!(
        b.document.discussions.threads[&id].comments[0].body,
        "External edit"
    );
    assert!(b.status.contains("draft is retained"));
    b.input = None;
    let part = scene
        .spatial
        .parts
        .iter()
        .position(|p| p.component == "motor")
        .unwrap();
    let local = Vec3::new(0.003, 0.002, 0.001);
    let world = animation::part_transform(&scene, part).transform_point(local);
    begin_surface(&mut b, &scene, part, world);
    assert_eq!(b.tab, Tab::Discussions);
    assert_eq!(b.discussion.draft_targets, vec!["motor"]);
    assert!((Vec3::from_array(b.discussion.draft_pin.unwrap()) - local).length() < 1e-6);
    b.input.as_mut().unwrap().buffer = "Surface-specific comment".into();
    act(&mut b, &mut scene, &mut orbit, &mut pick, Action::Open(id.clone()));
    assert!(
        b.discussion.selected.is_none(),
        "a pin click cannot discard or retarget a draft"
    );
    submit(&mut b, &mut scene, &mut orbit, &mut pick);
    let surface_id = b.discussion.selected.clone().unwrap();
    assert!(
        (Vec3::from_array(b.document.discussions.threads[&surface_id].pin_m.unwrap()) - local)
            .length()
            < 1e-6
    );
    b.tab = Tab::Library;
    act(&mut b, &mut scene, &mut orbit, &mut pick, Action::Open(id.clone()));
    assert_eq!(b.tab, Tab::Discussions);
    assert_eq!(b.discussion.selected, Some(id));
    // A target chip selects the instance holding it and shows its parts; Back restores both.
    act(&mut b, &mut scene, &mut orbit, &mut pick, Action::Target("motor".into()));
    assert_eq!(pick.names(), BTreeSet::from(["motor".to_string()]));
    assert_eq!(scene.shown, selection(&scene, &["motor".to_string()]));
    act(&mut b, &mut scene, &mut orbit, &mut pick, Action::Back);
    assert!(pick.names().is_empty());
    assert!(b.discussion.prior.is_none());
    std::fs::remove_dir_all(dir).unwrap();
}
