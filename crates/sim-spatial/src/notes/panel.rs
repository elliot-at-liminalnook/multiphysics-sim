//! Inspect's notes panel: the notes drawn as threads with the kit's thread
//! panel (`ui_kit::threads`), its buttons as the view's `annotations`
//! action, hover emphasis, and the guides drawn around noted parts.
use super::*;
use crate::document::DocumentRegistry;
use crate::selection::Selection;
use crate::ui_kit::{DANGER, Kit, Look, UiFonts, size, threads};

#[derive(Component)]
pub(crate) struct NotesPanel;
#[derive(Component, Clone)]
pub(crate) enum NoteAction {
    Select(String),
    Link(String, usize),
    New,
    SaveView,
    Restore(String),
    Undo,
    Redo,
}

/// Inspect notes drawn with the shared thread panel: a note's title selects
/// what it is on, a link chip follows the link.
struct NotesHost<'a> {
    doc: &'a notes::Document,
}
impl threads::Host<NoteAnchor> for NotesHost<'_> {
    type Action = NoteAction;
    fn open(&self, thread: &str) -> NoteAction {
        NoteAction::Select(thread.into())
    }
    fn menu(&self, _comment: &str) -> Option<NoteAction> {
        None
    }
    fn edit(&self, _comment: &str) -> Option<NoteAction> {
        None
    }
    fn delete(&self, _comment: &str) -> Option<NoteAction> {
        None
    }
    fn anchor(&self, a: &NoteAnchor) -> Option<NoteAction> {
        Some(match a.link {
            Some(index) => NoteAction::Link(a.note.clone(), index),
            None => NoteAction::Select(a.note.clone()),
        })
    }
    fn link(&self, _comment: &Comment<NoteAnchor>, _link: &sim_markdown::Link) -> Option<NoteAction> {
        None
    }
    fn color(&self, thread: &str) -> Option<Color> {
        self.doc.notes.get(thread).map(|n| Color::srgb_u8(n.color[0], n.color[1], n.color[2]))
    }
}

/// Input: the notes panel's buttons, as the view's `annotations` action (the
/// same request REST sends); ids and targets are read from the document now,
/// and a new note is on Inspect's shared selection.
pub(crate) fn clicks(scene: Res<SpatialScene>, selection: Option<Res<Selection>>, registry: Option<Res<DocumentRegistry>>, actions: Query<(&Interaction, &NoteAction), Changed<Interaction>>, mut out: MessageWriter<crate::app::actions::Act<crate::inspect::InspectAction>>) {
    if actions.is_empty() {
        return;
    }
    let doc = scene.note_document();
    let selected = match (selection.as_deref(), registry.as_deref().and_then(|r| r.current(crate::app::ViewerMode::Inspect))) {
        (Some(selection), Some((document, _))) => selection.target(document),
        _ => scene.shown.clone(),
    };
    for (interaction, action) in &actions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let request = match action {
            NoteAction::Undo => Some(notes::Request::Edit { change: notes::Command::Undo, expected_revision: Some(doc.revision) }),
            NoteAction::Redo => Some(notes::Request::Edit { change: notes::Command::Redo, expected_revision: Some(doc.revision) }),
            NoteAction::Select(id) => Some(notes::Request::SelectNote { id: id.clone() }),
            NoteAction::Restore(id) => Some(notes::Request::RestoreView { id: id.clone() }),
            NoteAction::Link(note, index) => Some(notes::Request::FollowLink { note: note.clone(), index: *index }),
            NoteAction::SaveView => {
                let id = format!("physical-view-{}", doc.revision + 1);
                Some(notes::Request::SaveView { id, label: "Assembly inspection view".into() })
            }
            NoteAction::New if selected != SelectionTarget::None => {
                let id = format!("note-{}-{}", std::process::id(), doc.revision + 1);
                let components = selected.resolve(&scene.description).map(|d| d.components).unwrap_or_default();
                let links = components.iter().filter_map(|id| scene.description.components.get(id).map(|c| notes::Link { label: c.label.clone(), target: notes::LinkTarget::Selection { target: SelectionTarget::component(id.clone()) } })).collect();
                let note = notes::Note { id, label: "Assembly discussion".into(), text: String::new(), targets: selected.clone(), links, color: NOTE_COLOR };
                Some(notes::Request::Edit { change: notes::Command::PutNote { note }, expected_revision: None })
            }
            NoteAction::New => None,
        };
        if let Some(request) = request {
            out.write(crate::app::actions::Act::ui(crate::inspect::InspectAction::Annotations { action: request }));
        }
    }
}

/// Follow the sidecar's navigation, keep the hover emphasis and redraw the
/// panel when the notes change.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update(
    mut commands: Commands,
    mut scene: ResMut<SpatialScene>,
    mut camera: Single<&mut Orbit>,
    fonts: Option<Res<UiFonts>>,
    panels: Query<Entity, With<NotesPanel>>,
    actions: Query<(Ref<Interaction>, &NoteAction)>,
    (mut selection, registry): (Option<ResMut<Selection>>, Option<Res<DocumentRegistry>>),
    mut revision: Local<Option<String>>,
) {
    let doc = scene.note_document();
    // Only a pending navigation borrows the scene (and the selection)
    // mutably: a write every frame would mark them changed every frame, and
    // the view's systems that skip an unchanged scene would run each frame.
    if doc.navigation.as_ref().is_some_and(|n| n.revision != scene.note_navigation) {
        let mut owner = match (selection.as_deref_mut(), registry.as_deref()) {
            (Some(selection), Some(registry)) => Owner::inspect(selection, registry),
            _ => None,
        };
        super::sync(&mut scene, &mut camera, owner.as_mut());
    }
    let doc = scene.note_document();
    // Hovering a note, view or link emphasises what it points at (its press is `clicks`).
    if actions.iter().any(|(i, _)| i.is_changed()) {
        scene.note_pointer_hover = SelectionTarget::None;
        for (interaction, action) in &actions {
            if *interaction == Interaction::Hovered {
                scene.note_pointer_hover = match action {
                    NoteAction::Select(id) => doc.notes.get(id).map(|n| n.targets.clone()),
                    NoteAction::Restore(id) => doc.views.get(id).map(|v| v.selection.clone()),
                    NoteAction::Link(id, i) => doc.notes.get(id).and_then(|n| n.links.get(*i)).and_then(|l| match &l.target {
                        notes::LinkTarget::Selection { target } => Some(target.clone()),
                        notes::LinkTarget::View { id } => doc.views.get(id).map(|v| v.selection.clone()),
                    }),
                    _ => None,
                }
                .unwrap_or(SelectionTarget::None);
            }
        }
    }
    // Draw once the interface fonts exist (the revision stamp waits for them).
    let Some(fonts) = fonts else { return };
    let k = Kit::new(&fonts);
    let error = scene.note_error.clone().or_else(|| scene.annotations.as_ref().and_then(|s| s.error()));
    let stamp = format!("{}:{:?}", doc.revision, error);
    if revision.as_ref() == Some(&stamp) {
        return;
    }
    *revision = Some(stamp);
    let host = NotesHost { doc: &doc };
    let shown = InspectNotes { scene: &mut scene }.threads();
    for panel in &panels {
        commands.entity(panel).despawn_related::<Children>();
        commands.entity(panel).with_children(|column| {
            column.spawn(k.section("Discussion"));
            if let Some(error) = &error {
                column.spawn(k.text(error, size::ITEM, DANGER, 0));
            }
            column.spawn(k.button("Annotate selected parts", NoteAction::New, Look::Secondary, true));
            column.spawn(k.button("Save this inspection angle", NoteAction::SaveView, Look::Secondary, true));
            for (enabled, label, action) in [(!doc.undo.is_empty(), "Undo discussion", NoteAction::Undo), (!doc.redo.is_empty(), "Redo discussion", NoteAction::Redo)] {
                if enabled {
                    column.spawn(k.button(label, action, Look::Ghost, true));
                }
            }
            for thread in shown.values() {
                threads::card(column, &k, &host, thread);
            }
            if !doc.views.is_empty() {
                column.spawn(k.section("Saved views"));
            }
            for view in doc.views.values() {
                column.spawn(k.button(&view.label, NoteAction::Restore(view.id.clone()), Look::Ghost, true));
            }
        });
    }
}

/// A box around each noted part and around each note's parts together (the
/// hovered or emphasised target in amber).
pub(crate) fn guides(scene: Res<SpatialScene>, mut gizmos: Gizmos) {
    let doc = scene.note_document();
    let hover = if scene.note_pointer_hover != SelectionTarget::None { &scene.note_pointer_hover } else { &scene.note_hover };
    let mut groups: Vec<_> = doc.notes.values().map(|n| (n.targets.clone(), Color::srgb_u8(n.color[0], n.color[1], n.color[2]))).collect();
    if *hover != SelectionTarget::None {
        groups.push((hover.clone(), Color::srgb(1., 0.80, 0.35)));
    }
    for (target, color) in groups {
        let Ok(details) = target.resolve(&scene.description) else {
            continue;
        };
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for (i, p) in scene.spatial.parts.iter().enumerate() {
            if !details.components.contains(&p.component) || scene.state.hidden.contains(&p.component) {
                continue;
            }
            let radius = match p.shape {
                SpatialShape::Box { size } => Vec3::from_array(size).length() * 0.5,
                SpatialShape::Cylinder { radius, length } => radius.hypot(length * 0.5),
                SpatialShape::Sphere { radius } => radius,
            };
            let pose = animation::part_transform(&scene, i);
            let dimensions = match p.shape {
                SpatialShape::Box { size } => Vec3::from_array(size),
                SpatialShape::Cylinder { radius, length } => Vec3::new(radius * 2., length, radius * 2.),
                SpatialShape::Sphere { radius } => Vec3::splat(radius * 2.),
            };
            gizmos.cube(Transform { scale: dimensions * 1.025, ..pose }, color);
            let center = pose.translation;
            lo = lo.min(center - Vec3::splat(radius));
            hi = hi.max(center + Vec3::splat(radius));
        }
        if lo.is_finite() && hi.is_finite() {
            let pad = (hi - lo).length() * 0.035;
            gizmos.cube(Transform::from_translation((lo + hi) * 0.5).with_scale(hi - lo + Vec3::splat(pad)), color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ViewerMode;
    use crate::document::{DocumentKind, Source};
    #[test]
    fn shared_note_builds_native_cards_and_link_hover_click_use_source_selection() {
        let mut scene = crate::tests::fixture();
        let id = "example/motor-thermal/motor".to_string();
        let directory = std::env::temp_dir().join(format!("native-note-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("notes.json");
        let mut doc = notes::Document::new(&scene.description);
        doc.apply(
            notes::Command::PutNote {
                note: notes::Note {
                    id: "n".into(),
                    label: "Drive discussion".into(),
                    text: "shared".into(),
                    targets: SelectionTarget::component(id.clone()),
                    color: [30, 150, 160],
                    links: vec![notes::Link { label: "Motor link".into(), target: notes::LinkTarget::Selection { target: SelectionTarget::component(id.clone()) } }],
                },
            },
            &scene.description,
        )
        .unwrap();
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        scene.connect_annotations(path);
        let start = std::time::Instant::now();
        while scene.note_document().revision != 1 {
            assert!(start.elapsed() < std::time::Duration::from_secs(3));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // Inspect's document is open: a followed link is its shared selection.
        let mut registry = DocumentRegistry::default();
        let document = registry.open(ViewerMode::Inspect, DocumentKind::Assembly, Source::Assembly { description: "motor.description.json".into(), spatial: "motor.spatial.json".into() }).id;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(scene)
            .insert_resource(registry)
            .init_resource::<Selection>()
            // The panel draws with the kit's fonts (placeholder handles here).
            .insert_resource(UiFonts { regular: Handle::default(), italic: Handle::default(), mono: Handle::default(), icons: Default::default(), medium: Handle::default(), semibold: Handle::default() })
            .init_resource::<crate::app::actions::Replies>()
            // A press is an `annotations` action, applied by the view's one handler.
            .add_systems(Update, (update, clicks, crate::inspect::apply).chain());
        crate::app::actions::register::<crate::inspect::InspectAction>(&mut app);
        app.world_mut().spawn(Orbit { focus: Vec3::ZERO, radius: 1., yaw: 0., pitch: 0., home: false, ..Default::default() });
        app.world_mut().spawn((NotesPanel, Node::default()));
        app.update();
        let world = app.world_mut();
        assert!(world.query::<&Text>().iter(world).any(|t| t.0 == "Drive discussion"));
        let entity = world.query::<(Entity, &NoteAction)>().iter(world).find_map(|(e, a)| matches!(a, NoteAction::Link(_, 0)).then_some(e)).unwrap();
        world.entity_mut(entity).insert(Interaction::Hovered);
        app.update();
        assert_eq!(app.world().resource::<SpatialScene>().note_pointer_hover, SelectionTarget::component(id.clone()));
        app.world_mut().entity_mut(entity).insert(Interaction::Pressed);
        app.update();
        assert_eq!(app.world().resource::<Selection>().target(document), SelectionTarget::component(id.clone()));
        assert_eq!(app.world().resource::<SpatialScene>().shown, SelectionTarget::component(id));
        // The note's text is its thread's one message: an edit through the
        // service becomes the sidecar's note command.
        {
            let mut scene = app.world_mut().resource_mut::<SpatialScene>();
            let applied = crate::annotations::apply(&mut InspectNotes { scene: &mut scene }, "Edit", crate::annotations::ThreadOp::EditComment { thread: "n".into(), comment: "n".into(), body: "edited".into(), links: None }).unwrap();
            let crate::annotations::Committed::Pending(request) = applied.committed else { panic!("the sidecar's worker applies note edits") };
            let start = std::time::Instant::now();
            let written = loop {
                if let Some(result) = scene.annotations.as_mut().unwrap().result(request) {
                    break result.unwrap();
                }
                assert!(start.elapsed() < std::time::Duration::from_secs(3));
                std::thread::sleep(std::time::Duration::from_millis(10));
            };
            assert_eq!(written.notes["n"].text, "edited");
            assert_eq!(written.notes["n"].color, [30, 150, 160], "the note keeps its colour");
            let refused = crate::annotations::apply(&mut InspectNotes { scene: &mut scene }, "Reply", crate::annotations::ThreadOp::Reply { thread: "n".into(), body: "x".into(), author: "A".into(), links: vec![] });
            assert!(refused.unwrap_err().contains("no replies"));
        }
        drop(app);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
