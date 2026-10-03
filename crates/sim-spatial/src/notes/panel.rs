//! Inspect's notes panel: the notes drawn as threads with the kit's thread
//! panel (`ui_kit::threads`), its buttons as the view's `annotations`
//! action, hover emphasis, and the guides drawn around noted parts.
//!
//! The Open / Resolved / All filter lists notes as thread cards (the note's
//! text, then its replies with author and time). The open note (the last
//! one pressed) is drawn with Resolve / Reopen, Edit text, each reply's
//! "···" menu (Edit, Delete) and the reply composer; its state and typing
//! are `notes/compose.rs` ([`NotesUi`]).
use super::compose::{NoteField, NotesUi};
use super::*;
use crate::document::DocumentRegistry;
use crate::selection::Selection;
use crate::ui_kit::{DANGER, FAINT, Kit, Look, OK, SUBTLE, UiFonts, size, threads, wrap};

#[derive(Component)]
pub(crate) struct NotesPanel;
#[derive(Component, Clone, Debug)]
pub(crate) enum NoteAction {
    Select(String),
    /// Link `index` of a note, or of its reply (the middle field).
    Link(String, Option<String>, usize),
    New,
    SaveView,
    Restore(String),
    Undo,
    Redo,
    /// The Open / Resolved / All filter.
    Filter(threads::Shown),
    /// Resolve (true) or reopen a note.
    Resolve(String, bool),
    /// A comment's "···" menu in the open note (pressed again: closed).
    Menu(String),
    /// Edit a comment of a note (the note's id: its text) in the composer.
    Edit(String, String),
    /// Delete a reply of a note.
    Delete(String, String),
    /// The composer's area, Post and Cancel for a note; the author's field.
    Compose(String),
    Post(String),
    Cancel(String),
    Author,
}

/// Inspect notes drawn with the shared thread panel: a note's title opens
/// it and selects what it is on, a link chip follows the link. Only the
/// open note's comments have menus.
struct NotesHost<'a> {
    doc: &'a notes::Document,
    description: &'a sim_inspect::SystemDescription,
    /// The open note, when drawing it.
    open: Option<&'a str>,
}
impl threads::Host<NoteAnchor> for NotesHost<'_> {
    type Action = NoteAction;
    fn open(&self, thread: &str) -> NoteAction {
        NoteAction::Select(thread.into())
    }
    fn menu(&self, comment: &str) -> Option<NoteAction> {
        self.open.map(|_| NoteAction::Menu(comment.into()))
    }
    fn edit(&self, comment: &str) -> Option<NoteAction> {
        self.open.map(|note| NoteAction::Edit(note.into(), comment.into()))
    }
    /// Replies only: the note's text goes with the note.
    fn delete(&self, comment: &str) -> Option<NoteAction> {
        self.open.filter(|note| *note != comment).map(|note| NoteAction::Delete(note.into(), comment.into()))
    }
    fn anchor(&self, a: &NoteAnchor) -> Option<NoteAction> {
        Some(match a.link {
            Some(index) => NoteAction::Link(a.note.clone(), a.reply.clone(), index),
            None => NoteAction::Select(a.note.clone()),
        })
    }
    fn link(&self, _comment: &Comment<NoteAnchor>, _link: &sim_markdown::Link) -> Option<NoteAction> {
        None
    }
    fn warning(&self, thread: &str) -> Option<String> {
        let note = self.doc.notes.get(thread)?;
        note.targets.validate(self.description).is_err().then(|| "The parts this note was on are gone from the description".to_string())
    }
    fn selected(&self, thread: &str) -> bool {
        self.open == Some(thread)
    }
    fn color(&self, thread: &str) -> Option<Color> {
        self.doc.notes.get(thread).map(|n| Color::srgb_u8(n.color[0], n.color[1], n.color[2]))
    }
}

/// Input: the notes panel's buttons, as the view's `annotations` action (the
/// same request REST sends); ids and targets are read from the document now,
/// and a new note is on Inspect's shared selection. The filter, the open
/// note and its menu are the panel's state; the composer's presses are
/// `compose::compose`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn clicks(scene: Res<SpatialScene>, mut ui: ResMut<NotesUi>, selection: Option<Res<Selection>>, registry: Option<Res<DocumentRegistry>>, actions: Query<&NoteAction, With<crate::ui_kit::activation::Activated>>, mut out: MessageWriter<crate::app::actions::Act<crate::inspect::InspectAction>>) {
    if actions.is_empty() {
        return;
    }
    let doc = scene.note_document();
    let selected = match (selection.as_deref(), registry.as_deref().and_then(|r| r.current(crate::app::ViewerMode::Inspect))) {
        (Some(selection), Some((document, _))) => selection.target(document),
        _ => scene.shown.clone(),
    };
    let mut next = ui.clone();
    for action in &actions {
        let request = match action {
            NoteAction::Undo => Some(notes::Request::Edit { change: notes::Command::Undo, expected_revision: Some(doc.revision) }),
            NoteAction::Redo => Some(notes::Request::Edit { change: notes::Command::Redo, expected_revision: Some(doc.revision) }),
            NoteAction::Select(id) => {
                if next.open.as_deref() != Some(id.as_str()) {
                    next.menu = None;
                }
                next.open = Some(id.clone());
                Some(notes::Request::SelectNote { id: id.clone() })
            }
            NoteAction::Restore(id) => Some(notes::Request::RestoreView { id: id.clone() }),
            NoteAction::Link(note, reply, index) => Some(notes::Request::FollowLink { note: note.clone(), index: *index, reply: reply.clone() }),
            NoteAction::SaveView => {
                let id = format!("physical-view-{}", doc.revision + 1);
                Some(notes::Request::SaveView { id, label: "Assembly inspection view".into() })
            }
            NoteAction::New if selected != SelectionTarget::None => {
                let id = format!("note-{}-{}", std::process::id(), doc.revision + 1);
                let components = selected.resolve(&scene.description).map(|d| d.components).unwrap_or_default();
                let links = components.iter().filter_map(|id| scene.description.components.get(id).map(|c| notes::Link { label: c.label.clone(), target: notes::LinkTarget::Selection { target: SelectionTarget::component(id.clone()) } })).collect();
                let note = notes::Note { id, label: "Assembly discussion".into(), text: String::new(), targets: selected.clone(), links, color: NOTE_COLOR, replies: vec![], resolved: false };
                Some(notes::Request::Edit { change: notes::Command::PutNote { note }, expected_revision: None })
            }
            NoteAction::New => None,
            NoteAction::Filter(shown) => {
                next.shown = *shown;
                None
            }
            NoteAction::Resolve(note, resolved) => Some(notes::Request::Resolve { note: note.clone(), resolved: *resolved }),
            NoteAction::Menu(comment) => {
                next.menu = if next.menu.as_deref() == Some(comment.as_str()) { None } else { Some(comment.clone()) };
                None
            }
            NoteAction::Delete(note, comment) => {
                next.menu = None;
                Some(notes::Request::DeleteComment { note: note.clone(), comment: comment.clone() })
            }
            NoteAction::Edit(..) | NoteAction::Compose(_) | NoteAction::Post(_) | NoteAction::Cancel(_) | NoteAction::Author => None,
        };
        if let Some(request) = request {
            out.write(crate::app::actions::Act::ui(crate::inspect::InspectAction::Annotations { action: request }));
        }
    }
    ui.set_if_neq(next);
}

/// The open note: its card (title, messages with the open menu), Resolve /
/// Reopen and Edit text, the author and the reply composer.
fn open_card(body: &mut ChildSpawnerCommands, k: &Kit, host: &NotesHost, thread: &Thread<NoteAnchor>, ui: &NotesUi) {
    let id = thread.id.as_str();
    threads::card_with(body, k, host, thread, ui.menu.as_deref(), |card| {
        if thread.resolved {
            card.spawn(k.text("Resolved", size::DETAIL, OK, 1));
        }
        card.spawn(wrap()).with_children(|r| {
            r.spawn(k.button(if thread.resolved { "Reopen" } else { "Resolve" }, NoteAction::Resolve(id.to_string(), !thread.resolved), Look::Ghost, true));
            r.spawn(k.button("Edit text", NoteAction::Edit(id.to_string(), id.to_string()), Look::Ghost, true));
        });
        let draft = ui.drafts.get(id);
        let editing = draft.and_then(|d| d.edit.as_ref()).map(|e| e.comment.as_str());
        let focused = ui.composing.as_deref() == Some(id) && ui.focus == Some(NoteField::Compose);
        let drafting = focused || draft.is_some_and(|d| !d.text().is_empty() || d.edit.is_some());
        // The author's own field while it has the keyboard; otherwise the
        // composer's author button (shown while no draft is open).
        let typing_author = ui.focus == Some(NoteField::Author);
        if typing_author {
            card.spawn(k.caption("Author"));
            card.spawn(k.input(&ui.author, "Your display name", NoteAction::Author, true));
        }
        let (label, submit) = match editing {
            Some(comment) if comment == id => ("Edit the note's text", "Save"),
            Some(_) => ("Edit reply", "Save"),
            None => ("Reply", "Post reply"),
        };
        threads::composer(
            card,
            k,
            threads::Composer {
                identity: format!("inspect-composer:{id}:{editing:?}"),
                label,
                draft: drafting.then(|| draft.map_or("", |d| d.text())),
                placeholder: "Write a reply…",
                min_height: 64.,
                focus: NoteAction::Compose(id.to_string()),
                submit: NoteAction::Post(id.to_string()),
                submit_label: submit,
                cancel: NoteAction::Cancel(id.to_string()),
                author: (!typing_author).then(|| (ui.author.as_str(), NoteAction::Author)),
                error: draft.and_then(|d| d.error.as_deref()),
            },
        );
        if draft.is_some_and(|d| d.waiting.is_some()) {
            card.spawn(k.text("Saving to the notes file…", size::DETAIL, SUBTLE, 0));
        }
    });
}

/// Follow the sidecar's navigation, keep the hover emphasis and redraw the
/// panel when the notes or the panel's state change.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update(
    mut commands: Commands,
    mut scene: ResMut<SpatialScene>,
    mut camera: Single<&mut Orbit>,
    fonts: Option<Res<UiFonts>>,
    ui: Res<NotesUi>,
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
                    NoteAction::Link(id, reply, i) => doc
                        .notes
                        .get(id)
                        .and_then(|n| match reply {
                            None => n.links.get(*i),
                            Some(reply) => n.replies.iter().find(|r| &r.id == reply).and_then(|r| r.links.get(*i)),
                        })
                        .and_then(|l| match &l.target {
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
    // Redraw on a new revision or error, or when the panel's own state
    // changed (`set_if_neq` in `clicks` and `compose`: no false changes).
    let stamp = format!("{}:{:?}", doc.revision, error);
    if revision.as_ref() == Some(&stamp) && !ui.is_changed() {
        return;
    }
    *revision = Some(stamp);
    let description = &scene.description;
    let shown: Vec<Thread<NoteAnchor>> = doc.notes.values().filter(|n| ui.shown.keeps(n.resolved)).map(|n| as_thread(n, description)).collect();
    let host = NotesHost { doc: &doc, description, open: None };
    let open_host = NotesHost { doc: &doc, description, open: ui.open.as_deref() };
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
            if !doc.notes.is_empty() {
                threads::filter_row(column, &k, ui.shown, NoteAction::Filter);
            }
            for thread in &shown {
                if ui.open.as_deref() == Some(thread.id.as_str()) {
                    open_card(column, &k, &open_host, thread, &ui);
                } else if thread.resolved {
                    // A listed resolved note says so in its title (as the kit's list headings do).
                    let mut marked = thread.clone();
                    marked.title = format!("{}  · resolved", thread.title);
                    threads::card(column, &k, &host, &marked);
                } else {
                    threads::card(column, &k, &host, thread);
                }
            }
            if shown.is_empty() && !doc.notes.is_empty() {
                let empty = match ui.shown {
                    threads::Shown::Open => "No open notes",
                    threads::Shown::Resolved => "No resolved notes",
                    threads::Shown::All => "No notes",
                };
                column.spawn(k.text(empty, size::ITEM, FAINT, 0));
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
                    replies: vec![],
                    resolved: false,
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
            .init_resource::<NotesUi>()
            // A press is an `annotations` action, applied by the view's one handler.
            .add_systems(Update, (update, clicks, crate::inspect::apply).chain());
        crate::app::actions::register::<crate::inspect::InspectAction>(&mut app);
        app.world_mut().spawn(Orbit { focus: Vec3::ZERO, radius: 1., yaw: 0., pitch: 0., home: false, ..Default::default() });
        app.world_mut().spawn((NotesPanel, Node::default()));
        app.update();
        let world = app.world_mut();
        assert!(world.query::<&Text>().iter(world).any(|t| t.0 == "Drive discussion"));
        let entity = world.query::<(Entity, &NoteAction)>().iter(world).find_map(|(e, a)| matches!(a, NoteAction::Link(_, None, 0)).then_some(e)).unwrap();
        world.entity_mut(entity).insert(Interaction::Hovered);
        app.update();
        assert_eq!(app.world().resource::<SpatialScene>().note_pointer_hover, SelectionTarget::component(id.clone()));
        app.world_mut().entity_mut(entity).insert(crate::ui_kit::activation::Activated);
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
            // A reply is the sidecar's reply command (version 2), written
            // by the Store's worker.
            let replied = crate::annotations::apply(&mut InspectNotes { scene: &mut scene }, "Reply", crate::annotations::ThreadOp::Reply { thread: "n".into(), body: "x".into(), author: "A".into(), links: vec![] }).unwrap();
            let crate::annotations::Committed::Pending(request) = replied.committed else { panic!("the sidecar's worker applies replies") };
            let start = std::time::Instant::now();
            let written = loop {
                if let Some(result) = scene.annotations.as_mut().unwrap().result(request) {
                    break result.unwrap();
                }
                assert!(start.elapsed() < std::time::Duration::from_secs(3));
                std::thread::sleep(std::time::Duration::from_millis(10));
            };
            assert_eq!(written.version, 2);
            let reply = &written.notes["n"].replies[0];
            assert_eq!((reply.author.as_str(), reply.body.as_str()), ("A", "x"));
            assert_eq!(written.notes["n"].text, "edited", "the note's text is kept");
            // The thread shows the text, then the reply.
            let thread = InspectNotes { scene: &mut scene }.thread("n").unwrap();
            assert_eq!(thread.comments.len(), 2);
            assert_eq!(thread.comments[1].id, reply.id);
            // The note's text is not deleted as a comment.
            let refused = crate::annotations::apply(&mut InspectNotes { scene: &mut scene }, "Delete comment", crate::annotations::ThreadOp::DeleteComment { thread: "n".into(), comment: "n".into() });
            assert_eq!(refused.unwrap_err(), "delete the note to remove its text");
        }
        // The panel's Resolve press is the view's `annotations` action,
        // applied by the one handler and written by the Store. (No
        // activation `clear` system runs here: each press is taken off
        // after its frame.)
        fn find(app: &mut App, wanted: fn(&NoteAction) -> bool) -> Option<Entity> {
            let world = app.world_mut();
            world.query::<(Entity, &NoteAction)>().iter(world).find_map(|(e, a)| wanted(a).then_some(e))
        }
        fn press(app: &mut App, entity: Entity) {
            app.world_mut().entity_mut(entity).insert(crate::ui_kit::activation::Activated);
            app.update();
            if let Ok(mut pressed) = app.world_mut().get_entity_mut(entity) {
                pressed.remove::<crate::ui_kit::activation::Activated>();
            }
        }
        // The reply changed the sidecar: the panel redraws.
        app.update();
        assert!(find(&mut app, |a| matches!(a, NoteAction::Resolve(..))).is_none(), "Resolve is drawn for the open note only");
        let select = find(&mut app, |a| matches!(a, NoteAction::Select(id) if id == "n")).unwrap();
        press(&mut app, select);
        assert_eq!(app.world().resource::<NotesUi>().open.as_deref(), Some("n"));
        app.update();
        assert!(find(&mut app, |a| matches!(a, NoteAction::Menu(_))).is_some(), "the open note's reply has its menu");
        let resolve = find(&mut app, |a| matches!(a, NoteAction::Resolve(id, true) if id == "n")).expect("the open note has Resolve");
        press(&mut app, resolve);
        let start = std::time::Instant::now();
        while !app.world().resource::<SpatialScene>().note_document().notes["n"].resolved {
            assert!(start.elapsed() < std::time::Duration::from_secs(3));
            std::thread::sleep(std::time::Duration::from_millis(10));
            app.update();
        }
        // Open (the default filter) no longer lists it; Resolved does.
        app.update();
        assert!(find(&mut app, |a| matches!(a, NoteAction::Select(id) if id == "n")).is_none());
        let resolved = find(&mut app, |a| matches!(a, NoteAction::Filter(crate::ui_kit::threads::Shown::Resolved))).unwrap();
        press(&mut app, resolved);
        app.update();
        assert!(find(&mut app, |a| matches!(a, NoteAction::Resolve(id, false) if id == "n")).is_some(), "a resolved note is reopened from the Resolved list");
        drop(app);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
