use sim_system::{display::*, *};

fn fixture() -> (SystemDocument, sim_core::BehaviorRegistry) {
    let mut r = sim_core::BehaviorRegistry::default();
    sim_domain_electrical::elements::register(&mut r).unwrap();
    let mut d = SystemDocument::new("Display editing");
    apply(
        &mut d,
        &r,
        &[
            Command::AddInstance {
                at: "".into(),
                name: "a".into(),
                instance: InstanceSpec::element("electrical.resistor")
                    .with("resistance", 10.)
                    .at([0.013, 0.02, 0.]),
            },
            Command::AddInstance {
                at: "".into(),
                name: "b".into(),
                instance: InstanceSpec::element("electrical.resistor")
                    .with("resistance", 20.)
                    .at([0.027, 0.02, 0.]),
            },
        ],
    )
    .unwrap();
    (d, r)
}
#[test]
fn display_grid_moves_preserve_offsets_and_physics() {
    let (mut d, r) = fixture();
    let before = flatten(&d, &r).unwrap().model;
    let commands = moves(
        &d,
        &r,
        "",
        &["a".into(), "b".into()],
        [0.039, 0.02, -0.016],
        true,
    )
    .unwrap();
    assert_eq!(
        d.definitions["root"].instances["a"].placement.position[0], 0.013,
        "preview never mutates"
    );
    apply(&mut d, &r, &commands).unwrap();
    let a = d.definitions["root"].instances["a"].placement.position;
    let b = d.definitions["root"].instances["b"].placement.position;
    assert!((a[0] - 0.04).abs() < 1e-7 && (a[2] + 0.02).abs() < 1e-7);
    assert!((b[0] - a[0] - 0.014).abs() < 1e-7);
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(flatten(&d, &r).unwrap().model).unwrap(),
        "display moves must not alter the physics model"
    );
    assert!(
        Grid {
            spacing_m: 0.,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(moves(&d, &r, "", &["a".into()], [f32::NAN, 0., 0.], true).is_err());
}
#[test]
fn discussion_follows_rename_group_ungroup_but_not_name_reuse() {
    let (mut d, r) = fixture();
    let target = bind(&d, "a").unwrap();
    let thread = Thread {
        id: "t".into(),
        title: "Motor placement".into(),
        resolved: false,
        targets: vec![target.clone()],
        comments: vec![Comment {
            id: "c".into(),
            author: "User".into(),
            body: "Check placement".into(),
            created_at: "1".into(),
            edited_at: None,
            links: vec![target],
        }],
        pin_m: Some([0.01, 0., 0.]),
        view: None,
    };
    apply(&mut d, &r, &[Command::PutThread { thread }]).unwrap();
    apply(
        &mut d,
        &r,
        &[Command::RenameInstance {
            at: "".into(),
            name: "a".into(),
            new_name: "renamed".into(),
        }],
    )
    .unwrap();
    apply(
        &mut d,
        &r,
        &[Command::Group {
            at: "".into(),
            instances: vec!["renamed".into(), "b".into()],
            name: "g".into(),
            definition: "group".into(),
            label: "Group".into(),
        }],
    )
    .unwrap();
    assert_eq!(d.discussions.threads["t"].targets[0].path, "g/renamed");
    assert_eq!(
        d.discussions.threads["t"].comments[0].links[0].path,
        "g/renamed"
    );
    apply(
        &mut d,
        &r,
        &[Command::Ungroup {
            at: "".into(),
            name: "g".into(),
        }],
    )
    .unwrap();
    assert_eq!(d.discussions.threads["t"].targets[0].path, "renamed");
    apply(
        &mut d,
        &r,
        &[Command::RemoveInstance {
            at: "".into(),
            name: "renamed".into(),
        }],
    )
    .unwrap();
    assert!(d.discussions.threads["t"].targets[0].missing);
    apply(
        &mut d,
        &r,
        &[Command::AddInstance {
            at: "".into(),
            name: "renamed".into(),
            instance: InstanceSpec::element("electrical.resistor").with("resistance", 99.),
        }],
    )
    .unwrap();
    assert!(
        d.discussions.threads["t"].targets[0].missing,
        "a replacement with the same name is not the annotated part"
    );
    let restored: SystemDocument =
        serde_json::from_slice(&serde_json::to_vec(&d).unwrap()).unwrap();
    assert_eq!(restored.discussions, d.discussions);
}
#[test]
fn grid_frames_are_local_and_discussion_edits_are_undoable() {
    let (mut d, r) = fixture();
    apply(
        &mut d,
        &r,
        &[Command::Group {
            at: "".into(),
            instances: vec!["a".into(), "b".into()],
            name: "g".into(),
            definition: "group".into(),
            label: "Group".into(),
        }],
    )
    .unwrap();
    let mut transform = d.definitions["root"].instances["g"].placement.clone();
    transform.rotation_xyzw = [0., 0.70710677, 0., 0.70710677];
    apply(
        &mut d,
        &r,
        &[
            Command::MoveInstance {
                at: "".into(),
                name: "g".into(),
                placement: transform,
            },
            Command::SetDisplayGrid {
                at: "g".into(),
                grid: Grid {
                    origin_m: [0.005, 0., 0.],
                    ..Default::default()
                },
            },
        ],
    )
    .unwrap();
    let command = moves(&d, &r, "g", &["a".into()], [0.016, 0., 0.], true).unwrap();
    apply(&mut d, &r, &command).unwrap();
    assert!((d.definitions["group"].instances["a"].placement.position[0] - 0.015).abs() < 1e-7);
    let directory = std::env::temp_dir().join(format!("display-test-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("test.system.json");
    std::fs::write(&file, serde_json::to_vec(&d).unwrap()).unwrap();
    let store = SystemStore::new(file);
    let t = Thread {
        id: "t".into(),
        title: "Note".into(),
        targets: vec![bind(&d, "g").unwrap()],
        comments: vec![],
        resolved: false,
        pin_m: None,
        view: None,
    };
    store
        .apply(
            &r,
            "Note",
            &[Command::PutThread { thread: t }],
            Some(d.revision),
        )
        .unwrap();
    assert!(store.apply(&r, "Stale", &[], Some(d.revision)).is_err());
    assert_eq!(store.load().unwrap().discussions.threads.len(), 1);
    store.undo().unwrap();
    assert!(store.load().unwrap().discussions.is_empty());
    store.redo().unwrap();
    assert_eq!(store.load().unwrap().discussions.threads.len(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn overlap_preview_and_store_reject_atomically_but_touching_and_repair_are_allowed() {
    let (d, r) = fixture();
    let commands = moves(&d, &r, "", &["a".into()], [0.027, 0.02, 0.], false).unwrap();
    let report = sim_system::display_overlap::preview(&d, &r, &commands).unwrap();
    assert!(!report.allowed);
    assert_eq!(report.conflicts[0].first.path, "a");
    assert_eq!(report.conflicts[0].second.path, "b");
    let dir = std::env::temp_dir().join(format!("display-overlap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("model.json");
    let _ = std::fs::remove_file(&path);
    let store = sim_system::store::SystemStore::create(&path, &d).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let history = serde_json::to_value(store.history()).unwrap();
    assert!(
        store
            .apply(&r, "bad move", &commands, Some(d.revision))
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(serde_json::to_value(store.history()).unwrap(), history);
    let touch = moves(&d, &r, "", &["a".into()], [0.014, 0.02, 0.], false).unwrap();
    assert!(
        sim_system::display_overlap::preview(&d, &r, &touch)
            .unwrap()
            .allowed
    );
    let mut existing = d.clone();
    apply(&mut existing, &r, &commands).unwrap();
    let away = moves(&existing, &r, "", &["a".into()], [0.026, 0.02, 0.], false).unwrap();
    assert!(
        sim_system::display_overlap::preview(&existing, &r, &away)
            .unwrap()
            .allowed
    );
    let rigid = moves(
        &existing,
        &r,
        "",
        &["a".into(), "b".into()],
        [0.5, 0.02, 0.],
        false,
    )
    .unwrap();
    assert!(
        sim_system::display_overlap::preview(&existing, &r, &rigid)
            .unwrap()
            .allowed
    );
    // A wire does not establish mechanical permission to intersect.
    let mut wired = d.clone();
    apply(
        &mut wired,
        &r,
        &[Command::Connect {
            at: "".into(),
            terminals: vec![Terminal::port("a", "p"), Terminal::port("b", "p")],
            label: "wire".into(),
        }],
    )
    .unwrap();
    assert!(
        !sim_system::display_overlap::preview(&wired, &r, &commands)
            .unwrap()
            .allowed
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn overlap_checks_mechanical_mates_and_rotated_nested_parts() {
    let mut r = sim_core::BehaviorRegistry::default();
    sim_domain_rotational::elements::register(&mut r).unwrap();
    let mut d = SystemDocument::new("mechanical");
    for (name, x) in [("a", 0.), ("b", 0.1)] {
        apply(
            &mut d,
            &r,
            &[Command::AddInstance {
                at: "".into(),
                name: name.into(),
                instance: InstanceSpec::element("rotational.inertia")
                    .with("inertia", 0.01)
                    .at([x, 0., 0.]),
            }],
        )
        .unwrap();
    }
    let commands = moves(&d, &r, "", &["b".into()], [0., 0., 0.], false).unwrap();
    assert!(
        !sim_system::display_overlap::preview(&d, &r, &commands)
            .unwrap()
            .allowed
    );
    let mut attach = commands.clone();
    attach.push(Command::Connect {
        at: "".into(),
        terminals: vec![Terminal::port("a", "shaft"), Terminal::port("b", "shaft")],
        label: "explicit shaft mate".into(),
    });
    assert!(
        sim_system::display_overlap::preview(&d, &r, &attach)
            .unwrap()
            .allowed
    );
    apply(
        &mut d,
        &r,
        &[Command::Group {
            at: "".into(),
            instances: vec!["a".into()],
            name: "group".into(),
            definition: "group_def".into(),
            label: "group".into(),
        }],
    )
    .unwrap();
    let mut placement = d.definitions["root"].instances["group"].placement.clone();
    placement.rotation_xyzw = [
        0.,
        0.,
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ];
    placement.position = [0.1, 0., 0.];
    let report = sim_system::display_overlap::preview(
        &d,
        &r,
        &[Command::MoveInstance {
            at: "".into(),
            name: "group".into(),
            placement,
        }],
    )
    .unwrap();
    assert!(!report.allowed);
    assert!(
        report
            .conflicts
            .iter()
            .any(|c| c.first.path == "group/a" || c.second.path == "group/a")
    );
}

#[test]
fn library_internals_are_preserved_but_every_shared_occurrence_is_checked() {
    let (mut d, r) = fixture();
    let mut sub = Definition::new("authored pack");
    for name in ["one", "two"] {
        sub.instances.insert(
            name.into(),
            InstanceSpec::element("electrical.resistor").with("resistance", 1.),
        );
    }
    apply(
        &mut d,
        &r,
        &[Command::AddDefinitions {
            definitions: std::collections::BTreeMap::from([("pack".into(), sub)]),
        }],
    )
    .unwrap();
    let add = Command::AddInstance {
        at: "".into(),
        name: "pack1".into(),
        instance: InstanceSpec::subsystem("pack").at([1., 0., 0.]),
    };
    assert!(
        sim_system::display_overlap::preview(&d, &r, &[add.clone()])
            .unwrap()
            .allowed
    );
    apply(
        &mut d,
        &r,
        &[
            add,
            Command::AddInstance {
                at: "".into(),
                name: "pack2".into(),
                instance: InstanceSpec::subsystem("pack").at([0.027, 0.02, 0.]),
            },
        ],
    )
    .unwrap();
    // Start clear. Editing the definition affects both pack occurrences.
    let mut initial = moves(
        &d,
        &r,
        "pack1",
        &["one".into(), "two".into()],
        [0.1, 0., 0.],
        false,
    )
    .unwrap();
    apply(&mut d, &r, &initial).unwrap();
    initial = moves(
        &d,
        &r,
        "pack1",
        &["one".into(), "two".into()],
        [0., 0., 0.],
        false,
    )
    .unwrap();
    let report = sim_system::display_overlap::preview(&d, &r, &initial).unwrap();
    assert!(!report.allowed);
    assert!(
        report
            .conflicts
            .iter()
            .any(|c| c.first.path.starts_with("pack2/") || c.second.path.starts_with("pack2/"))
    );
}

/// System discussions now use the shared `sim-annotate` types; a thread in
/// the pre-refactor JSON reads, validates and writes back unchanged, and
/// still follows its part through a rename.
#[test]
fn discussion_format_is_unchanged_and_follows_renames() {
    let (mut d, r) = fixture();
    let a = bind(&d, "a").unwrap();
    let json = serde_json::json!({
        "id": "t1", "title": "Why 10 Ω?", "resolved": false,
        "targets": [{"path": "a", "label": "a", "lineage": a.lineage, "missing": false}],
        "comments": [{"id": "c1", "author": "User", "body": "See [a](part:a)", "created_at": "1760000000", "edited_at": null,
                      "links": [{"path": "a", "label": "a", "lineage": a.lineage, "missing": false}]}],
        "pin_m": [0.0, 0.001, 0.0],
        "view": {"focus": [0.0, 0.0, 0.0], "radius": 0.2, "yaw": 0.3, "pitch": 0.4, "exploded": false, "connections": true, "hidden": []}
    });
    let thread: Thread = serde_json::from_value(json.clone()).unwrap();
    let written: serde_json::Value = serde_json::from_str(&serde_json::to_string(&thread).unwrap()).unwrap();
    assert_eq!(written, json);
    validate_thread(&thread).unwrap();
    apply(&mut d, &r, &[Command::PutThread { thread }]).unwrap();
    apply(&mut d, &r, &[Command::RenameInstance { at: "".into(), name: "a".into(), new_name: "r_top".into() }]).unwrap();
    let t = &d.discussions.threads["t1"];
    assert_eq!(t.targets[0].path, "r_top");
    assert_eq!(t.comments[0].links[0].path, "r_top");
    assert!(!t.targets[0].missing);
    // Shared validation messages are unchanged.
    let mut bad = t.clone();
    bad.targets[0].path.clear();
    assert_eq!(validate_thread(&bad).unwrap_err().to_string().contains("invalid discussion reference"), true);
}
