use super::*;
use sim_system::{Definition, SystemDocument};
fn fixture(test: impl FnOnce(Builder, SpatialScene)) {
    // A counter as well: parallel fixtures can share a clock tick.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "drag-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let mut doc = SystemDocument::new("Drag fixture");
    let mut group = Definition::new("Assembly");
    group.instances.insert(
        "left".into(),
        InstanceSpec::element("electrical.resistor").with("resistance", 10.),
    );
    group.instances.insert(
        "right".into(),
        InstanceSpec::element("electrical.resistor")
            .with("resistance", 20.)
            .at([0.08, 0., 0.]),
    );
    doc.definitions.insert("assembly_def".into(), group);
    let root = doc.definitions.get_mut(&doc.root).unwrap();
    root.instances.insert(
        "a".into(),
        InstanceSpec::element("electrical.resistor")
            .with("resistance", 100.)
            .at([0., 0.02, 0.]),
    );
    root.instances.insert(
        "b".into(),
        InstanceSpec::element("electrical.resistor")
            .with("resistance", 200.)
            .at([0.5, 0.02, 0.]),
    );
    root.instances.insert(
        "assembly".into(),
        InstanceSpec::subsystem("assembly_def").at([0.2, 0.02, 0.]),
    );
    sim_system::display::assign_ids(&mut doc);
    let path = dir.join("test.system.json");
    SystemStore::create(&path, &doc).unwrap();
    let mut b = Builder::open(
        path,
        dir.join("library/systems"),
        sim_runtime::system_registry(),
    )
    .unwrap();
    let scene = scene(&mut b);
    test(b, scene);
    std::fs::remove_dir_all(dir).unwrap();
}
fn scene(b: &mut Builder) -> SpatialScene {
    let flat = sim_system::flatten(&b.document, &b.registry).unwrap();
    let d = sim_inspect::model::describe(
        &flat.model,
        &b.registry,
        &flat.source_hash,
        flat.revision,
        &flat.identities,
    )
    .unwrap()
    .description;
    let spatial = flat.spatial(&d.id, &b.document.title);
    b.subsystems = flat.subsystems;
    SpatialScene::for_builder(d, spatial).unwrap()
}
fn drag(b: &Builder, names: &[&str]) -> DragState {
    let start = Vec3::from_array(b.spec(names[0]).unwrap().placement.position);
    DragState::new(
        b,
        names.iter().map(|s| s.to_string()).collect(),
        start,
        start,
        None,
        None,
    )
}
#[test]
fn plane_drag_keeps_grab_offset_and_normal_coordinate_on_all_grid_planes() {
    fixture(|b, _| {
        for plane in [
            sim_system::display::Plane::Xy,
            sim_system::display::Plane::Xz,
            sim_system::display::Plane::Yz,
        ] {
            let mut d = drag(&b, &["a"]);
            let g = Grid {
                plane,
                snap: false,
                ..Default::default()
            };
            let (a, c, n) = plane.axes();
            d.hit = d.start + Vec3::splat(0.003);
            let mut origin = d.hit;
            origin[a] += 0.013;
            origin[c] -= 0.017;
            origin[n] += 1.;
            let mut direction = Vec3::ZERO;
            direction[n] = -1.;
            d.project(origin, direction, &g, false);
            assert!((d.target[a] - d.start[a] - 0.013).abs() < 1e-6);
            assert!((d.target[c] - d.start[c] + 0.017).abs() < 1e-6);
            assert_eq!(d.target[n], d.start[n]);
        }
    });
}
#[test]
fn snapping_and_alt_bypass_use_identical_grid_math() {
    fixture(|b, _| {
        let mut d = drag(&b, &["a"]);
        let g = Grid::default();
        d.project(Vec3::new(0.013, 1., 0.027), Vec3::NEG_Y, &g, true);
        assert!(d.target.abs_diff_eq(Vec3::new(0.01, 0.02, 0.03), 1e-6));
        d.project(Vec3::new(0.013, 1., 0.027), Vec3::NEG_Y, &g, false);
        assert!(d.target.abs_diff_eq(Vec3::new(0.013, 0.02, 0.027), 1e-6));
    });
}
#[test]
fn axis_handles_do_not_jump_on_grab_and_lock_the_other_coordinates() {
    fixture(|b, _| {
        for axis in 0..3 {
            let mut d = drag(&b, &["a"]);
            d.axis = Some(axis);
            let normal = (axis + 1) % 3;
            let mut direction = Vec3::ZERO;
            direction[normal] = -1.;
            let mut origin = d.start;
            origin[axis] += 0.04;
            origin[normal] += 1.;
            d.project(origin, direction, &Grid::default(), false);
            assert!(d.target.abs_diff_eq(d.start, 1e-6));
            origin[axis] += 0.023;
            d.project(origin, direction, &Grid::default(), false);
            assert!((d.target[axis] - d.start[axis] - 0.023).abs() < 1e-6);
            for k in 0..3 {
                if k != axis {
                    assert_eq!(d.target[k], d.start[k]);
                }
            }
        }
    });
}
#[test]
fn parallel_or_behind_camera_rays_preserve_last_finite_position() {
    fixture(|b, _| {
        let mut d = drag(&b, &["a"]);
        let expected = d.target;
        d.project(Vec3::Y, Vec3::X, &Grid::default(), false);
        d.project(Vec3::Y, Vec3::Y, &Grid::default(), false);
        d.axis = Some(0);
        d.project(Vec3::Y, Vec3::X, &Grid::default(), false);
        assert_eq!(d.target, expected);
        assert!(d.target.is_finite());
    });
}
#[test]
fn nested_rotated_frame_projects_in_local_space_and_moves_in_world_space() {
    fixture(|mut b, _| {
        b.level = "assembly".into();
        let mut d = drag(&b, &["left"]);
        d.frame.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let local_origin = Vec3::new(0.03, 1., 0.02);
        d.project(
            d.frame.transform_point(local_origin),
            d.frame.rotation * Vec3::NEG_Y,
            &Grid::default(),
            false,
        );
        assert!(d.target.abs_diff_eq(Vec3::new(0.03, 0., 0.02), 1e-6));
        assert!(
            d.delta("assembly/left")
                .abs_diff_eq(Vec3::new(0., 0.03, 0.02), 1e-6)
        );
        assert_eq!(d.delta("assembly/right"), Vec3::ZERO);
    });
}
#[test]
fn group_and_multiple_selection_move_rigidly_without_matching_similar_prefixes() {
    fixture(|b, _| {
        let mut d = drag(&b, &["a", "assembly"]);
        d.target += Vec3::X * 0.031;
        for path in ["a", "assembly", "assembly/left", "assembly/right"] {
            assert_eq!(d.delta(path), Vec3::X * 0.031);
        }
        for path in ["ab", "assembly2/left", "b"] {
            assert_eq!(d.delta(path), Vec3::ZERO);
        }
        let commands = d.work.commands(d.target).unwrap();
        let positions: Vec<_> = commands
            .into_iter()
            .map(|c| {
                if let SystemCommand::MoveInstance { placement, .. } = c {
                    placement.position
                } else {
                    panic!()
                }
            })
            .collect();
        assert!((positions[1][0] - positions[0][0] - 0.2).abs() < 1e-6);
    });
}
#[test]
fn bevy_mesh_and_annotation_anchor_follow_every_frame_without_accumulation() {
    fixture(|mut b, s| {
        let original = b.document.clone();
        let spatial = serde_json::to_value(&s.spatial).unwrap();
        let index = s
            .spatial
            .parts
            .iter()
            .position(|p| p.component == "a")
            .unwrap();
        let base = animation::part_transform(&s, index);
        let pin = Vec3::new(0.003, 0.004, 0.005);
        b.drag = Some(drag(&b, &["a"]));
        let mut app = App::new();
        app.insert_resource(b)
            .insert_resource(s)
            .add_systems(Update, apply_preview);
        let entity = app
            .world_mut()
            .spawn((Part { index }, Transform::default()))
            .id();
        for n in 0..240 {
            let delta = Vec3::new(n as f32 * 0.0001, 0., (n as f32 * 0.1).sin() * 0.01);
            let mut b = app.world_mut().resource_mut::<Builder>();
            let d = b.drag.as_mut().unwrap();
            d.target = d.start + delta;
            app.update();
            let mesh = *app.world().get::<Transform>(entity).unwrap();
            let b = app.world().resource::<Builder>();
            let s = app.world().resource::<SpatialScene>();
            let anchor = super::super::markers::transform(b, s, "a").unwrap();
            assert!(mesh.translation.abs_diff_eq(base.translation + delta, 1e-6));
            assert_eq!(mesh.transform_point(pin), anchor.transform_point(pin));
        }
        let b = app.world().resource::<Builder>();
        assert_eq!(b.document, original);
        assert!(b.store.history().undo.is_empty());
        assert_eq!(
            serde_json::to_value(&app.world().resource::<SpatialScene>().spatial).unwrap(),
            spatial
        );
        app.world_mut().resource_mut::<Builder>().drag = None;
        app.update();
        assert_eq!(*app.world().get::<Transform>(entity).unwrap(), base);
    });
}
#[test]
fn group_annotations_and_descendant_pins_share_the_preview_delta() {
    fixture(|mut b, s| {
        let base = super::super::markers::transform(&b, &s, "assembly").unwrap();
        let child = super::super::markers::transform(&b, &s, "assembly/left").unwrap();
        let mut d = drag(&b, &["assembly"]);
        d.target += Vec3::Z * 0.11;
        b.drag = Some(d);
        for (path, old) in [("assembly", base), ("assembly/left", child)] {
            assert!(
                (super::super::markers::transform(&b, &s, path)
                    .unwrap()
                    .translation
                    - old.translation)
                    .abs_diff_eq(Vec3::Z * 0.11, 1e-6)
            );
        }
    });
}
#[test]
fn release_holds_preview_until_scene_replacement_then_has_no_double_delta() {
    fixture(|mut b, s| {
        let mut d = drag(&b, &["a"]);
        d.target += Vec3::Z * 0.1;
        d.released = true;
        let expected = Vec3::from_array(
            s.spatial
                .parts
                .iter()
                .find(|p| p.component == "a")
                .unwrap()
                .position,
        ) + Vec3::Z * 0.1;
        let (release, gate) = mpsc::channel::<()>();
        let (work, target) = (d.work.clone(), d.target);
        d.committing = Some(crate::jobs::Job::spawn(crate::jobs::Pool::Dedicated, 0, "test commit", move |_| {
            let _ = gate.recv();
            work.commit(target)
        }));
        assert_eq!(poll_drop(&mut b, &s, &mut d), Some(true));
        assert!(!d.awaiting_scene());
        release.send(()).unwrap();
        let started = std::time::Instant::now();
        while !d.awaiting_scene() && started.elapsed() < std::time::Duration::from_secs(10) {
            assert_eq!(poll_drop(&mut b, &s, &mut d), Some(true));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(d.awaiting_scene());
        assert_eq!(poll_drop(&mut b, &s, &mut d), Some(true));
        b.drag = Some(d);
        let before = super::super::markers::transform(&b, &s, "a").unwrap();
        assert!(before.translation.abs_diff_eq(expected, 1e-6));
        b.reload();
        let new = scene(&mut b);
        let mut d = b.drag.take().unwrap();
        assert_eq!(poll_drop(&mut b, &new, &mut d), Some(false));
        assert!(
            super::super::markers::transform(&b, &new, "a")
                .unwrap()
                .translation
                .abs_diff_eq(expected, 1e-6)
        );
        assert_eq!(b.store.history().undo.len(), 1);
    });
}
#[test]
fn zero_move_release_saves_nothing_and_a_drop_without_rebuild_releases_the_preview() {
    fixture(|mut b, s| {
        let mut d = drag(&b, &["a"]);
        assert!(d.unchanged(), "a click that snaps back must not commit");
        d.target += Vec3::X * 0.01;
        assert!(!d.unchanged());
        // A saved revision that schedules no new scene (or whose rebuild
        // failed) must not hold the drag forever.
        d.committed_revision = Some(b.document.revision + 1);
        b.scene_dirty = true;
        assert_eq!(poll_drop(&mut b, &s, &mut d), Some(true));
        b.scene_dirty = false;
        assert_eq!(poll_drop(&mut b, &s, &mut d), Some(false));
    });
}
#[test]
fn comments_during_a_drag_neither_cancel_it_nor_reject_the_drop() {
    fixture(|mut b, _| {
        let mut d = drag(&b, &["a"]);
        let thread = sim_system::display::Thread {
            id: "note".into(),
            title: "Resistor".into(),
            resolved: false,
            targets: vec![sim_system::display::bind(&b.document, "a").unwrap()],
            comments: vec![],
            pin_m: None,
            view: None,
        };
        b.apply("note", vec![SystemCommand::PutThread { thread }])
            .unwrap();
        assert!(d.same_scene(&b.document));
        assert_eq!(d.revision, b.document.revision);
        d.work.commit(Vec3::new(0., 0.02, 0.1)).unwrap();
        b.reload();
        assert_eq!(b.spec("a").unwrap().placement.position, [0., 0.02, 0.1]);
        assert!(b.document.discussions.threads.contains_key("note"));
        // An assembly edit still cancels the drag and rejects its drop.
        let mut d = drag(&b, &["a"]);
        b.display_move(vec!["b".into()], [0.6, 0.02, 0.], false, false, None)
            .unwrap();
        assert!(!d.same_scene(&b.document));
        let saved = std::fs::read(&b.store.path).unwrap();
        assert!(d.work.commit(Vec3::new(0., 0.02, 0.2)).is_err());
        assert_eq!(std::fs::read(&b.store.path).unwrap(), saved);
    });
}
#[test]
fn authoritative_drop_rejects_overlap_and_stale_edits_without_overwriting_sources() {
    fixture(|b, _| {
        let d = drag(&b, &["a"]);
        let before = std::fs::read(&b.store.path).unwrap();
        assert!(!d.work.validate(Vec3::new(0.5, 0.02, 0.)).unwrap().allowed);
        assert!(d.work.commit(Vec3::new(0.5, 0.02, 0.)).is_err());
        assert_eq!(std::fs::read(&b.store.path).unwrap(), before);
        d.work.commit(Vec3::new(0., 0.02, 0.1)).unwrap();
        let saved = std::fs::read(&b.store.path).unwrap();
        assert!(d.work.commit(Vec3::new(0., 0.02, 0.2)).is_err());
        assert_eq!(std::fs::read(&b.store.path).unwrap(), saved);
        assert_eq!(b.store.history().undo.len(), 1);
        b.store.undo().unwrap();
        assert_eq!(b.store.load().unwrap().definitions, b.document.definitions);
    });
}
#[test]
fn drag_preview_matches_rest_preview_policy() {
    fixture(|mut b, _| {
        let d = drag(&b, &["a"]);
        for target in [Vec3::new(0., 0.02, 0.1), Vec3::new(0.5, 0.02, 0.)] {
            let report = d.work.validate(target).unwrap();
            let rest = b
                .display_move(vec!["a".into()], target.to_array(), false, true, None)
                .unwrap();
            assert_eq!(serde_json::to_value(report).unwrap(), rest["overlap"]);
        }
    });
}
#[test]
fn thousand_part_bevy_preview_stays_inside_60hz_cpu_budget_while_validation_is_blocked() {
    fixture(|mut b, mut s| {
        use std::time::{Duration, Instant};
        let template = s.spatial.parts[0].clone();
        s.spatial.parts = (0..1024)
            .map(|i| {
                let mut p = template.clone();
                p.component = format!("assembly/part-{i}");
                p.id = format!("part/{i}");
                p
            })
            .collect();
        let mut d = drag(&b, &["assembly"]);
        let (entered, enter) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        d.validator = super::super::placement_worker::Latest::new(move |_: &Vec3| {
            entered.send(()).unwrap();
            gate.recv().unwrap();
            Err("controlled slow validator".into())
        });
        d.validator.submit(d.start);
        enter.recv_timeout(Duration::from_secs(2)).unwrap();
        b.drag = Some(d);
        let mut app = App::new();
        app.insert_resource(b)
            .insert_resource(s)
            .add_systems(Update, apply_preview);
        for index in 0..1024 {
            app.world_mut()
                .spawn((Part { index }, Transform::default()));
        }
        let mut timings = vec![];
        for n in 0..250 {
            let begin = Instant::now();
            let mut b = app.world_mut().resource_mut::<Builder>();
            let d = b.drag.as_mut().unwrap();
            d.project(
                Vec3::new(n as f32 * 0.001, 1., 0.1),
                Vec3::NEG_Y,
                &Grid::default(),
                false,
            );
            app.update();
            let b = app.world().resource::<Builder>();
            let s = app.world().resource::<SpatialScene>();
            // Include annotation-anchor work in the hot-path measurement.
            for i in 0..100 {
                assert!(
                    super::super::markers::transform(b, s, &format!("assembly/part-{i}"))
                        .is_some()
                );
            }
            if n >= 10 {
                timings.push(begin.elapsed().as_secs_f64() * 1000.);
            }
        }
        release.send(()).unwrap();
        timings.sort_by(f64::total_cmp);
        let p99 = timings[timings.len() * 99 / 100];
        println!(
            "drag CPU benchmark: 1024 Bevy mesh transforms + 100 annotation anchors; median {:.3} ms, p99 {:.3} ms; budget 16.667 ms (GPU excluded)",
            timings[timings.len() / 2],
            p99
        );
        assert!(
            p99 < 1000. / 60.,
            "drag preview p99 exceeded 60 Hz CPU budget: {p99:.3} ms"
        );
    });
}
#[test]
fn stale_clear_result_cannot_approve_a_new_overlapping_position() {
    fixture(|b, _| {
        let mut d = drag(&b, &["a"]);
        d.target = Vec3::new(0., 0.02, 0.1);
        d.validation = Some((d.target, d.work.validate(d.target)));
        assert!(d.current_validation().unwrap().1.as_ref().unwrap().allowed);
        d.target = Vec3::new(0.5, 0.02, 0.);
        assert!(d.current_validation().is_none());
        assert!(d.work.commit(d.target).is_err());
    });
}
#[test]
fn failed_drop_clears_pending_preview_without_changing_the_document() {
    fixture(|mut b, s| {
        let before = b.document.clone();
        let mut d = drag(&b, &["a"]);
        d.committing = Some(crate::jobs::Job::finished(0, d.work.commit(Vec3::new(0.5, 0.02, 0.))));
        assert_eq!(poll_drop(&mut b, &s, &mut d), Some(false));
        assert_eq!(b.document, before);
        assert!(b.action_error.as_ref().unwrap().contains("overlap"));
    });
}
#[test]
fn actual_bevy_drag_start_and_end_observers_keep_selected_group_identity() {
    fixture(|mut b, s| {
        use bevy::picking::{
            backend::HitData,
            pointer::{Location, PointerId},
        };
        use bevy::camera::{ManualTextureViewHandle, NormalizedRenderTarget};
        let (mut selection, mut documents) = super::super::test_support::test_selection(&b);
        Picked::new(&mut selection, &mut documents).set(["a".to_string(), "assembly".to_string()]).unwrap();
        let index = s
            .spatial
            .parts
            .iter()
            .position(|p| p.component == "assembly/left")
            .unwrap();
        let mut app = App::new();
        app.insert_resource(b)
            .insert_resource(s)
            .insert_resource(selection)
            .insert_resource(documents)
            .add_observer(start_part)
            .add_observer(end_drag);
        let entity = app.world_mut().spawn(Part { index }).id();
        let location = Location {
            target: NormalizedRenderTarget::TextureView(ManualTextureViewHandle(0)),
            position: Vec2::ZERO,
        };
        app.world_mut().trigger(
            Pointer::new(
                PointerId::Mouse,
                location.clone(),
                DragStart {
                    button: PointerButton::Primary,
                    hit: HitData::new(
                        Entity::PLACEHOLDER,
                        1.,
                        Some(Vec3::new(0.2, 0.02, 0.)),
                        None,
                    ),
                },
                entity,
            ),
        );
        assert_eq!(
            app.world()
                .resource::<Builder>()
                .drag
                .as_ref()
                .unwrap()
                .names,
            vec!["a", "assembly"]
        );
        app.world_mut().trigger(
            Pointer::new(
                PointerId::Mouse,
                location,
                DragEnd {
                    button: PointerButton::Primary,
                    distance: Vec2::X * 10.,
                },
                entity,
            ),
        );
        assert!(
            app.world()
                .resource::<Builder>()
                .drag
                .as_ref()
                .unwrap()
                .released
        );
    });
}
