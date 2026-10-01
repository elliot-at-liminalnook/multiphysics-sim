use super::*;
use crate::ui_kit::{DANGER, Kit, Look, RAISED, TEXT, Tint, UiFonts, size, wrap};
use serde_json::{Value, json};
use sim_inspect::annotations as notes;
#[derive(Component)]
pub(super) struct NotesPanel;
#[derive(Component, Clone)]
pub(super) enum NoteAction {
    Select(String),
    Link(String, usize),
    New,
    SaveView,
    Restore(String),
    Undo,
    Redo,
}
impl SpatialScene {
    pub fn connect_annotations(&mut self, path: std::path::PathBuf) {
        self.annotations = Some(notes::native::Store::new(
            std::sync::Arc::new(self.description.clone()),
            path,
        ));
    }
    /// Another system was opened: its own sidecar, no leftover emphasis or navigation.
    pub(crate) fn retarget_annotations(&mut self, path: std::path::PathBuf) {
        self.note_navigation = 0;
        self.note_hover = SelectionTarget::None;
        self.note_pointer_hover = SelectionTarget::None;
        self.note_error = None;
        self.connect_annotations(path);
    }
    pub(super) fn note_document(&self) -> notes::Document {
        self.annotations
            .as_ref()
            .map(|s| s.document())
            .unwrap_or_else(|| notes::Document::new(&self.description))
    }
}
pub(super) fn sync(scene: &mut SpatialScene, camera: &mut Orbit) {
    let doc = scene.note_document();
    if let Some(nav) = doc
        .navigation
        .as_ref()
        .filter(|n| n.revision != scene.note_navigation)
    {
        scene.note_navigation = nav.revision;
        if let Some(view) = doc.views.get(&nav.view) {
            if let Some(v) = &view.physical {
                // A cut: also ends a glide (which would carry on from its
                // start) and returns from the trackball.
                // A restored view stands still: a spin would turn away from it.
                camera.interrupt();
                camera.glide_to(crate::camera::Pose { focus: Vec3::from_array(v.focus), radius: v.radius, yaw: v.yaw, pitch: v.pitch }, 0.0);
                scene.state.exploded = v.exploded;
                scene.state.connections = v.connections;
                scene.state.hidden = v.hidden.clone();
            }
            if let Err(e) = scene.set_selection(view.selection.clone()) {
                scene.note_error = Some(e.to_string());
            }
        }
    }
}
pub(super) fn api(
    scene: &mut SpatialScene,
    camera: &mut Orbit,
    action: notes::Request,
    continuation: &mut Value,
) -> sim_api::Outcome {
    let result = (|| -> Result<Option<Value>, String> {
        if let Some(id) = continuation.as_u64() {
            return scene
                .annotations
                .as_mut()
                .ok_or("annotation store not connected")?
                .result(id)
                .map(|r| r.map(|d| Some(json!(d))))
                .unwrap_or(Ok(None));
        }
        let doc = scene.note_document();
        let change = match action {
            notes::Request::Document => return Ok(Some(json!(doc))),
            notes::Request::Emphasize { target } => {
                target
                    .validate(&scene.description)
                    .map_err(|e| e.to_string())?;
                scene.note_hover = target;
                return Ok(Some(json!({"emphasized":scene.note_hover})));
            }
            notes::Request::SelectNote { id } => {
                let note = doc.notes.get(&id).ok_or("unknown annotation")?;
                scene
                    .set_selection(note.targets.clone())
                    .map_err(|e| e.to_string())?;
                return Ok(Some(json!({"selection":scene.selection})));
            }
            notes::Request::SaveView { id, label } => notes::Command::PutView {
                view: notes::SavedView {
                    id,
                    label,
                    selection: scene.selection.clone(),
                    schematic: None,
                    physical: Some(notes::PhysicalView {
                        focus: camera.focus.to_array(),
                        radius: camera.radius,
                        // The heading drawn (the trackball's, when it is on).
                        yaw: camera.turntable().0,
                        pitch: camera.turntable().1,
                        exploded: scene.state.exploded,
                        connections: scene.state.connections,
                        hidden: scene.state.hidden.clone(),
                    }),
                },
            },
            notes::Request::RestoreView { id } => notes::Command::FollowView { id },
            notes::Request::FollowLink { note, index } => {
                let link = doc
                    .notes
                    .get(&note)
                    .and_then(|n| n.links.get(index))
                    .ok_or("unknown annotation link")?;
                match &link.target {
                    notes::LinkTarget::Selection { target } => {
                        scene
                            .set_selection(target.clone())
                            .map_err(|e| e.to_string())?;
                        return Ok(Some(json!({"selection":scene.selection})));
                    }
                    notes::LinkTarget::View { id } => notes::Command::FollowView { id: id.clone() },
                }
            }
            notes::Request::Edit {
                change,
                expected_revision,
            } => {
                let id = scene
                    .annotations
                    .as_mut()
                    .ok_or("annotation store not connected")?
                    .submit(change, expected_revision)?;
                *continuation = json!(id);
                return Ok(None);
            }
        };
        let id = scene
            .annotations
            .as_mut()
            .ok_or("annotation store not connected")?
            .submit(change, None)?;
        *continuation = json!(id);
        Ok(None)
    })();
    match result {
        Ok(Some(value)) => {
            scene.note_error = None;
            sync(scene, camera);
            sim_api::Outcome::Done(Ok(value))
        }
        Ok(None) => sim_api::Outcome::Pending,
        Err(error) => sim_api::Outcome::Done(Err(error)),
    }
}
/// Input: the notes panel's buttons, as the view's `annotations` action (the
/// same request REST sends); ids and targets are read from the document now.
pub(super) fn clicks(scene: Res<SpatialScene>, actions: Query<(&Interaction, &NoteAction), Changed<Interaction>>, mut out: MessageWriter<crate::app::actions::Act<crate::inspect::InspectAction>>) {
    if actions.is_empty() {
        return;
    }
    let doc = scene.note_document();
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
            NoteAction::New => {
                if scene.selection != SelectionTarget::None {
                    let id = format!("note-{}-{}", std::process::id(), doc.revision + 1);
                    Some(notes::Request::Edit {
                        change: notes::Command::PutNote {
                            note: notes::Note {
                                id,
                                label: "Assembly discussion".into(),
                                text: String::new(),
                                targets: scene.selection.clone(),
                                links: scene
                                    .details
                                    .components
                                    .iter()
                                    .map(|id| notes::Link { label: scene.description.components[id].label.clone(), target: notes::LinkTarget::Selection { target: SelectionTarget::component(id.clone()) } })
                                    .collect(),
                                color: [30, 155, 160],
                            },
                        },
                        expected_revision: None,
                    })
                } else {
                    None
                }
            }
        };
        if let Some(request) = request {
            out.write(crate::app::actions::Act::ui(crate::inspect::InspectAction::Annotations { action: request }));
        }
    }
}
pub(super) fn update(
    mut commands: Commands,
    mut scene: ResMut<SpatialScene>,
    mut camera: Single<&mut Orbit>,
    fonts: Option<Res<UiFonts>>,
    panels: Query<Entity, With<NotesPanel>>,
    actions: Query<(Ref<Interaction>, &NoteAction)>,
    mut revision: Local<Option<String>>,
) {
    sync(&mut scene, &mut camera);
    let doc = scene.note_document();
    // Hovering a note, view or link emphasises what it points at (its press is `clicks`).
    if actions.iter().any(|(i, _)| i.is_changed()) {
        scene.note_pointer_hover = SelectionTarget::None;
        for (interaction, action) in &actions {
            if *interaction == Interaction::Hovered {
                scene.note_pointer_hover = match action {
                    NoteAction::Select(id) => doc.notes.get(id).map(|n| n.targets.clone()),
                    NoteAction::Restore(id) => doc.views.get(id).map(|v| v.selection.clone()),
                    NoteAction::Link(id, i) => doc
                        .notes
                        .get(id)
                        .and_then(|n| n.links.get(*i))
                        .and_then(|l| match &l.target {
                            notes::LinkTarget::Selection { target } => Some(target.clone()),
                            notes::LinkTarget::View { id } => {
                                doc.views.get(id).map(|v| v.selection.clone())
                            }
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
    let error = scene
        .note_error
        .clone()
        .or_else(|| scene.annotations.as_ref().and_then(|s| s.error()));
    let stamp = format!("{}:{:?}", doc.revision, error);
    if revision.as_ref() == Some(&stamp) {
        return;
    }
    *revision = Some(stamp);
    for panel in &panels {
        commands.entity(panel).despawn_related::<Children>();
        commands.entity(panel).with_children(|column| {
            column.spawn(k.section("Discussion"));
            if let Some(error) = &error {
                column.spawn(k.text(error, size::ITEM, DANGER, 0));
            }
            column.spawn(k.button("Annotate selected parts", NoteAction::New, Look::Secondary, true));
            column.spawn(k.button("Save this inspection angle", NoteAction::SaveView, Look::Secondary, true));
            for (enabled, label, action) in [
                (!doc.undo.is_empty(), "Undo discussion", NoteAction::Undo),
                (!doc.redo.is_empty(), "Redo discussion", NoteAction::Redo),
            ] {
                if enabled {
                    column.spawn(k.button(label, action, Look::Ghost, true));
                }
            }
            for note in doc.notes.values() {
                let color = Color::srgb_u8(note.color[0], note.color[1], note.color[2]);
                // A card framed in the note's own colour (no kit card widget).
                column
                    .spawn((
                        Node {
                            border_radius: BorderRadius::all(Val::Px(6.)),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(Val::Px(10.)),
                            row_gap: Val::Px(5.),
                            border: UiRect::all(Val::Px(1.)),
                            flex_shrink: 0.,
                            ..default()
                        },
                        BorderColor::all(color),
                        BackgroundColor(RAISED),
                    ))
                    .with_children(|card| {
                        // The title keeps the note's colour, so it is a
                        // tinted row rather than a kit button (whose label
                        // colour comes from its look).
                        card.spawn((
                            Button,
                            NoteAction::Select(note.id.clone()),
                            Tint::CLEAR,
                            bevy::ui::prelude::AccessibleLabel::new(note.label.as_str()),
                            Node { border_radius: BorderRadius::all(Val::Px(4.)), padding: UiRect::axes(Val::Px(4.), Val::Px(2.)), ..default() },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|b| {
                            b.spawn(k.text(&note.label, 15., color, 2));
                        });
                        if !note.text.is_empty() {
                            card.spawn(k.text(&note.text, size::ITEM, TEXT, 0));
                        }
                        if !note.links.is_empty() {
                            card.spawn(wrap()).with_children(|links| {
                                for (index, link) in note.links.iter().enumerate() {
                                    links.spawn(k.chip(&format!("↗ {}", link.label), NoteAction::Link(note.id.clone(), index), false, true));
                                }
                            });
                        }
                    });
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
pub(super) fn guides(scene: Res<SpatialScene>, mut gizmos: Gizmos) {
    let doc = scene.note_document();
    let hover = if scene.note_pointer_hover != SelectionTarget::None {
        &scene.note_pointer_hover
    } else {
        &scene.note_hover
    };
    let mut groups: Vec<_> = doc
        .notes
        .values()
        .map(|n| {
            (
                n.targets.clone(),
                Color::srgb_u8(n.color[0], n.color[1], n.color[2]),
            )
        })
        .collect();
    if *hover != SelectionTarget::None {
        groups.push((hover.clone(), Color::srgb(1., 0.80, 0.35)));
    }
    for (target, color) in groups {
        let Ok(details) = target.resolve(&scene.description) else {
            continue;
        };
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for (i, p) in scene.spatial.parts.iter().enumerate() {
            if !details.components.contains(&p.component)
                || scene.state.hidden.contains(&p.component)
            {
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
                SpatialShape::Cylinder { radius, length } => {
                    Vec3::new(radius * 2., length, radius * 2.)
                }
                SpatialShape::Sphere { radius } => Vec3::splat(radius * 2.),
            };
            gizmos.cube(
                Transform {
                    scale: dimensions * 1.025,
                    ..pose
                },
                color,
            );
            let center = pose.translation;
            lo = lo.min(center - Vec3::splat(radius));
            hi = hi.max(center + Vec3::splat(radius));
        }
        if lo.is_finite() && hi.is_finite() {
            let pad = (hi - lo).length() * 0.035;
            gizmos.cube(
                Transform::from_translation((lo + hi) * 0.5).with_scale(hi - lo + Vec3::splat(pad)),
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_note_builds_native_cards_and_link_hover_click_use_source_selection() {
        let mut scene = crate::tests::fixture();
        let id = "example/motor-thermal/motor".to_string();
        let directory = std::env::temp_dir().join(format!(
            "native-note-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
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
                    links: vec![notes::Link {
                        label: "Motor link".into(),
                        target: notes::LinkTarget::Selection {
                            target: SelectionTarget::component(id.clone()),
                        },
                    }],
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
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(scene)
            // The panel draws with the kit's fonts (placeholder handles here).
            .insert_resource(UiFonts { regular: Handle::default(), italic: Handle::default(), mono: Handle::default(), icons: Default::default(), medium: Handle::default(), semibold: Handle::default() })
            .init_resource::<crate::app::actions::Replies>()
            // A press is an `annotations` action, applied by the view's one handler.
            .add_systems(Update, (update, clicks, crate::inspect::apply).chain());
        crate::app::actions::register::<crate::inspect::InspectAction>(&mut app);
        app.world_mut().spawn(Orbit {
            focus: Vec3::ZERO,
            radius: 1.,
            yaw: 0.,
            pitch: 0.,
            home: false,
            ..Default::default()
        });
        app.world_mut().spawn((NotesPanel, Node::default()));
        app.update();
        let world = app.world_mut();
        assert!(
            world
                .query::<&Text>()
                .iter(world)
                .any(|t| t.0 == "Drive discussion")
        );
        let entity = world
            .query::<(Entity, &NoteAction)>()
            .iter(world)
            .find_map(|(e, a)| matches!(a, NoteAction::Link(_, 0)).then_some(e))
            .unwrap();
        world.entity_mut(entity).insert(Interaction::Hovered);
        app.update();
        assert_eq!(
            app.world().resource::<SpatialScene>().note_pointer_hover,
            SelectionTarget::component(id.clone())
        );
        app.world_mut()
            .entity_mut(entity)
            .insert(Interaction::Pressed);
        app.update();
        assert_eq!(
            app.world().resource::<SpatialScene>().selection,
            SelectionTarget::component(id)
        );
        drop(app);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
