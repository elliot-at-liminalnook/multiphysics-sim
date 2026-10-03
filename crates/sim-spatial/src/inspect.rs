//! The spatial assembly view's actions: Inspect mode's commands, and the
//! scene commands of Build and Lessons (the view the builder and the lesson
//! pages draw into). REST, the toolbar and parts/connection buttons, the
//! notes panel, the overlay bar and the keys all write [`InspectAction`];
//! [`apply`] (`ViewerSet::Actions`) is its one handler, which the headless
//! server calls directly ([`serve_headless`]).
use super::*;
use crate::app::actions::{self, Act, Call, InFlight, Replies, Spec, spec};
use crate::document::{DocumentId, DocumentRegistry};
use crate::selection::{Selection, SelectionAction, target_items};
use bevy::ecs::message::Messages;
use serde::Deserialize;
use serde_json::{Value, json};
use sim_api::Outcome;

/// Every intent of the spatial view. The REST variants keep each command's
/// JSON shape (`{"command": …, …}`); the skipped ones are the toolbar's
/// toggles and keys, which read the current state when applied.
#[derive(Component, Deserialize, Clone)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum InspectAction {
    Annotations {
        action: sim_inspect::annotations::Request,
    },
    Render {
        #[serde(default)]
        options: sim_render::physical::Options,
    },
    State,
    Description,
    Spatial,
    Animation,
    Measurements,
    /// The connection buttons (a net), Clear and Escape (none) too.
    Select {
        target: SelectionTarget,
    },
    /// The parts list and keys 1–9 (`select` a component), Hide selected and Show all too.
    Display {
        action: SpatialCommand,
    },
    Camera {
        focus: [f32; 3],
        radius: f32,
        yaw: f32,
        pitch: f32,
    },
    /// Fit view and key H too.
    Fit,
    Panels {
        parts: bool,
        compact: bool,
    },
    /// Parts, Explode (key E) and Connections (key C): flip the current value.
    #[serde(skip)]
    Toggle(Toggle),
    /// The overlay bar's display toggles: the display state only (no refit).
    #[serde(skip)]
    View(SpatialCommand),
    /// Key F: fly to the selected part (the whole system when nothing is selected).
    #[serde(skip)]
    FlyTo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    Parts,
    Exploded,
    Connections,
}

impl actions::Action for InspectAction {
    fn commands() -> Vec<Spec> {
        use actions::SPATIAL as S;
        vec![
            spec("annotations", S, json!({"action":{"operation":"document"}}), "Shared multi-part notes, links, saved views and hover emphasis; same sidecar as schematic. Operations: document, edit, save_view, restore_view, follow_link (reply for a reply's link), select_note, emphasize, reply {note, body, author}, edit_comment {note, comment (the note's id: its text), body}, delete_comment {note, comment}, resolve {note, resolved}"),
            spec("render", S, json!({"options":{"view":"isometric","size":{"width":1280,"height":900},"section":null}}), "Off-screen PNG of captured geometry; x/y/z sections use meters and do not change the viewport"),
            spec("state", S, json!({}), "Display, selection, camera and live status"),
            spec("description", S, json!({}), "Shared typed components, ports, nets, observables and validation"),
            spec("spatial", S, json!({}), "Display parts, dimensions, provenance and source identities"),
            spec("animation", S, json!({}), "Source-bound observation bindings"),
            spec("measurements", S, json!({}), "Last measured frame; this viewer never advances physics"),
            spec("select", S, json!({"target":{"kind":"components","ids":["source-id"]}}), "Exact shared selection: none, components, ports or nets"),
            spec("display", S, json!({"action":{"kind":"set_exploded","enabled":true}}), "SpatialCommand: select, clear_selection, set_exploded, set_connections, set_overlay {layer: power|forces|current|heat|trails, enabled}, hide_selected, show_all"),
            spec("camera", S, json!({"focus":[0,0,0],"radius":0.5,"yaw":0.7,"pitch":0.4}), "Absolute orbit; SI meters and radians; shared by pan, orbit and zoom"),
            spec("fit", S, json!({}), "Fit all display geometry"),
            spec("panels", S, json!({"parts":true,"compact":false}), "Parts panel and compact inspector presentation"),
        ]
    }
}

/// Inspect mode's selection as its handler writes it: the shared
/// [`Selection`]'s items of the Inspect document. Without one (Build and
/// Lessons, whose spatial view shows the builder's or the lesson's
/// selection), `select` and `display` set only what the view shows, as before.
pub(crate) struct Owner<'a> {
    pub selection: &'a mut Selection,
    pub registry: &'a DocumentRegistry,
    pub document: DocumentId,
}
impl<'a> Owner<'a> {
    /// Inspect's document, when it is open.
    pub fn inspect(selection: &'a mut Selection, registry: &'a DocumentRegistry) -> Option<Self> {
        registry.current(ViewerMode::Inspect).map(|(document, _)| Owner { selection, registry, document })
    }
}

/// What is selected, as `state`, `render` metadata and the link report it:
/// the Inspect document's items, else (Build, Lessons) what the view shows.
fn selected(scene: &SpatialScene, owner: Option<&Owner>) -> SelectionTarget {
    owner.map_or_else(|| scene.shown.clone(), |o| o.selection.target(o.document))
}

/// Inspect's selection adapter (REST `select`, the connection buttons,
/// Clear, Escape, Shift-click): `target` is checked against the assembly as
/// `set_selection` resolves it (so a refusal reads as before), applied to the
/// shared selection as a set, and projected to the view.
/// Also the notes' adapter (a note's targets, a link, a saved view).
pub(crate) fn select(scene: &mut SpatialScene, owner: Option<&mut Owner>, target: &SelectionTarget) -> Result<(), String> {
    let Some(owner) = owner else {
        return scene.set_selection(target.clone()).map_err(|e| e.to_string());
    };
    target.resolve(&scene.description).map_err(|e| e.to_string())?;
    owner.selection.apply(owner.registry, &SelectionAction::set(owner.document, target_items(target)))?;
    scene.set_selection(owner.selection.target(owner.document)).map_err(|e| e.to_string())
}

pub(crate) fn state(scene: &SpatialScene, camera: &Orbit, selection: &SelectionTarget) -> Value {
    json!({"description_id":scene.description.id,"selection":selection,"display":scene.state,
        "camera":camera_json(camera),
        "annotations_revision":scene.note_document().revision,"annotation_emphasis":scene.note_hover,"annotation_error":scene.note_error,"parts_visible":scene.parts_visible,"compact":scene.compact,"live_status":scene.live_status(),"live_error":scene.live.error})
}

/// The shared camera's state (`camera_state`'s fields), with `fit_pending`
/// kept for existing callers.
fn camera_json(camera: &Orbit) -> Value {
    let mut out = crate::camera::state_json(camera, None);
    out["fit_pending"] = json!(camera.home);
    out
}

/// The one handler of the spatial view's actions. `window` is None
/// headless; `task` is the scene `render` in flight; `owner` is Inspect's
/// selection (None in Build and Lessons).
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle(scene: &mut SpatialScene, camera: &mut Orbit, task: &mut Option<sim_api::ImageTask>, window: Option<&Window>, owner: Option<&mut Owner>, action: &InspectAction, call: &mut Call) -> Outcome {
    match action {
        InspectAction::Annotations { action } => notes::api(scene, camera, owner, action.clone(), &mut *call.continuation),
        InspectAction::Render { options } => {
            // Its render was dropped with the scene it was capturing (the scope was left).
            if !call.continuation.is_null() && task.is_none() {
                return Outcome::Done(Err("render dropped: the scene it was capturing was left".into()));
            }
            if task.is_none() {
                let snapshot = match capture(scene, camera, &selected(scene, owner.as_deref()), options) {
                    Ok(s) => s,
                    Err(e) => return Outcome::Done(Err(e)),
                };
                let options = options.clone();
                *task = Some(sim_api::ImageTask::spawn(move || sim_render::physical::render(&snapshot, &options).map(|r| sim_api::Artifact { png: r.png, metadata: r.metadata })));
                *call.continuation = json!(true);
            }
            let result = task.as_mut().expect("started above").poll(call.cancelled);
            if !matches!(result, Outcome::Pending) {
                *task = None;
            }
            result
        }
        InspectAction::FlyTo => {
            if let Some(window) = window {
                let focus = scene.details.components.iter().next().cloned().or_else(|| scene.state.selected.clone());
                view::zoom_to(scene, camera, window, focus.as_deref(), 1.0, crate::camera::GLIDE_S);
            }
            Outcome::Done(Ok(Value::Null))
        }
        other => Outcome::Done(execute(scene, camera, owner, other)),
    }
}

/// The spatial view's synchronous actions; REST gets `state` back.
fn execute(scene: &mut SpatialScene, camera: &mut Orbit, mut owner: Option<&mut Owner>, action: &InspectAction) -> sim_api::Result {
    match action {
        InspectAction::Annotations { .. } | InspectAction::Render { .. } | InspectAction::FlyTo => {
            return Err("render requires the asynchronous image dispatcher".into());
        }
        InspectAction::State => return Ok(state(scene, camera, &selected(scene, owner.as_deref()))),
        InspectAction::Description => return Ok(json!(scene.description)),
        InspectAction::Spatial => return Ok(json!(scene.spatial)),
        InspectAction::Animation => return Ok(json!(scene.animation)),
        InspectAction::Measurements => return Ok(json!(scene.frame())),
        InspectAction::Select { target } => select(scene, owner.as_deref_mut(), target)?,
        InspectAction::Display { action } => {
            let fit = matches!(action, SpatialCommand::SetExploded { .. });
            match (action, owner.as_deref_mut()) {
                // The parts list, keys 1-9 and a click: the view's own check
                // (a part to show), then the shared selection.
                (SpatialCommand::Select { component }, Some(owner)) => {
                    scene.state.apply(&scene.spatial, action.clone()).map_err(|e| e.to_string())?;
                    select(scene, Some(owner), &SelectionTarget::component(component.clone()))?;
                }
                (SpatialCommand::ClearSelection, Some(owner)) => {
                    scene.state.apply(&scene.spatial, action.clone()).map_err(|e| e.to_string())?;
                    select(scene, Some(owner), &SelectionTarget::None)?;
                }
                _ => scene.apply(action.clone()).map_err(|e| e.to_string())?,
            }
            camera.home |= fit;
        }
        InspectAction::Camera { focus, radius, yaw, pitch } => {
            let (radius, yaw, pitch) = (*radius, *yaw, *pitch);
            if !focus.iter().chain([radius, yaw, pitch].iter()).all(|x| x.is_finite()) || radius <= 0. || pitch.abs() > 1.5 {
                return Err("finite camera required; radius > 0 and pitch within ±1.5 radians".into());
            }
            // A cut: also ends any glide (which would overwrite it) and the trackball.
            // A restored view stands still: a spin would turn away from it.
            camera.interrupt();
            camera.glide_to(crate::camera::Pose { focus: Vec3::from_array(*focus), radius, yaw, pitch }, 0.0);
        }
        InspectAction::Fit => camera.home = true,
        InspectAction::Panels { parts, compact } => {
            scene.parts_visible = *parts;
            scene.compact = *compact;
            camera.home = true;
        }
        InspectAction::Toggle(Toggle::Parts) => {
            scene.parts_visible = !scene.parts_visible;
            camera.home = true;
        }
        InspectAction::Toggle(Toggle::Exploded) => {
            camera.home = true;
            let enabled = !scene.state.exploded;
            scene.apply(SpatialCommand::SetExploded { enabled }).map_err(|e| e.to_string())?;
        }
        InspectAction::Toggle(Toggle::Connections) => {
            let enabled = !scene.state.connections;
            scene.apply(SpatialCommand::SetConnections { enabled }).map_err(|e| e.to_string())?;
        }
        InspectAction::View(command) => {
            let spatial = scene.spatial.clone();
            scene.state.apply(&spatial, command.clone()).map_err(|e| e.to_string())?;
        }
    }
    Ok(state(scene, camera, &selected(scene, owner.as_deref())))
}

/// Actions: the spatial view's one apply system (Inspect, Build, Lessons).
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply(
    mut messages: ResMut<Messages<Act<InspectAction>>>,
    mut in_flight: ResMut<InFlight<InspectAction>>,
    mut replies: ResMut<Replies>,
    scene: Option<ResMut<SpatialScene>>,
    camera: Option<Single<&mut Orbit>>,
    rest: Option<ResMut<crate::rest::Rest>>,
    window: Option<Single<&Window>>,
    (selection, registry, mode): (Option<ResMut<Selection>>, Option<Res<DocumentRegistry>>, Option<Res<State<ViewerMode>>>),
) {
    let (Some(mut scene), Some(mut camera)) = (scene, camera) else {
        // No scene yet: answer rather than leave a caller waiting.
        actions::apply(&mut messages, &mut in_flight, &mut replies, |_, _| Outcome::Done(Err("the spatial view has no scene".into())));
        return;
    };
    if messages.is_empty() && in_flight.is_empty() {
        return;
    }
    // Inspect's selection is the shared one (without states, as in tests, a
    // registry with an open Inspect document means inspect mode).
    let inspect = mode.is_none_or(|m| *m.get() == ViewerMode::Inspect);
    let mut selection = selection;
    let mut owner = match (selection.as_deref_mut(), registry.as_deref()) {
        (Some(selection), Some(registry)) if inspect => Owner::inspect(selection, registry),
        _ => None,
    };
    let mut no_task = None;
    let mut rest = rest;
    let task = match rest.as_mut() {
        Some(rest) => &mut rest.1,
        None => &mut no_task,
    };
    let window = window.as_deref().copied();
    actions::apply(&mut messages, &mut in_flight, &mut replies, |action, call| {
        let outcome = handle(&mut scene, &mut camera, &mut *task, window, owner.as_mut(), action, call);
        if call.origin == actions::Origin::Ui {
            match (action, &outcome) {
                // The notes panel shows its own error.
                (InspectAction::Annotations { .. }, Outcome::Done(Err(e))) => scene.note_error = Some(e.clone()),
                (_, Outcome::Done(Err(e))) => error!("{e}"),
                _ => {}
            }
        }
        outcome
    });
}

/// Input: the toolbar, parts and connection buttons and the keys, as
/// actions. The lesson screen has its own keys; a kit field with the
/// keyboard (a builder draft, a document field) takes them.
pub(crate) fn input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Query<&InspectAction, (With<crate::ui_kit::activation::Activated>, With<Button>)>,
    scene: Res<SpatialScene>,
    typing: crate::ui_kit::text::Typing,
    mode: Option<Res<State<ViewerMode>>>,
    mut out: MessageWriter<Act<InspectAction>>,
) {
    if mode.is_some_and(|m| *m.get() == ViewerMode::Lessons) {
        return;
    }
    // Ordinary activation remains valid while an editor retains its draft.
    for action in &buttons {
        out.write(Act::ui(action.clone()));
    }
    if typing.get() {
        return;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        out.write(Act::ui(InspectAction::FlyTo));
    }
    for (key, action) in [
        (KeyCode::Escape, InspectAction::Select { target: SelectionTarget::None }),
        (KeyCode::KeyH, InspectAction::Fit),
        (KeyCode::KeyE, InspectAction::Toggle(Toggle::Exploded)),
        (KeyCode::KeyC, InspectAction::Toggle(Toggle::Connections)),
    ] {
        if keys.just_pressed(key) {
            out.write(Act::ui(action));
        }
    }
    let ids = scene.representatives();
    let digits = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9];
    for (i, key) in digits.iter().enumerate() {
        if keys.just_pressed(*key) {
            if let Some((id, _)) = ids.get(i) {
                out.write(Act::ui(InspectAction::Display { action: SpatialCommand::Select { component: id.clone() } }));
            }
        }
    }
}

/// Present: the spatial view's REST snapshots (and, in build and lessons,
/// the builder's agent and the lesson's state), at most every 100 ms.
pub(crate) fn publish(
    rest: Option<ResMut<crate::rest::Rest>>,
    scene: Res<SpatialScene>,
    camera: Single<&Orbit>,
    builder: Option<Res<builder::Builder>>,
    learn: Option<Res<crate::lesson::Learn>>,
    mode: Res<State<ViewerMode>>,
    (selection, registry): (Option<Res<Selection>>, Option<Res<DocumentRegistry>>),
) {
    let Some(mut rest) = rest else { return };
    let mode = *mode.get();
    // The builder stays in the window in inspect mode, but inspect does not serve it.
    let family = mode.builder_family();
    // Inspect's selection is the shared one; Build and Lessons show theirs.
    let document = registry.as_deref().filter(|_| mode == ViewerMode::Inspect).and_then(|r| r.current(ViewerMode::Inspect));
    let selected = match (selection.as_deref(), document) {
        (Some(selection), Some((document, _))) => selection.target(document),
        _ => scene.shown.clone(),
    };
    publish_json(&mut rest.0, &scene, &camera, &selected, builder.as_deref().filter(|_| family), learn.as_deref().filter(|_| family), mode);
}

#[allow(clippy::too_many_arguments)]
fn publish_json(server: &mut sim_api::Server, scene: &SpatialScene, camera: &Orbit, selected: &SelectionTarget, builder: Option<&builder::Builder>, learn: Option<&crate::lesson::Learn>, mode: ViewerMode) {
    if !server.snapshot_due() {
        return;
    }
    if let Some(b) = builder {
        server.publish("agent", b.agent_json());
    }
    if let Some(l) = learn {
        server.publish("lesson", crate::lesson::actions::state(l));
    }
    let mut shown = state(scene, camera, selected);
    shown["viewer_mode"] = json!(mode.name());
    server.publish("state", shown);
    server.publish("annotations", json!(scene.note_document()));
    server.publish_changed("description", &scene.description.id, || json!(scene.description));
    server.publish_changed("spatial", &scene.description.id, || json!(scene.spatial));
    server.publish("measurements", json!(scene.frame()));
}

/// The headless server's poll (`--headless`: inspect mode, no window): the
/// same dispatch and the same handler, answered synchronously. The
/// dispatch refuses what needs a window (`viewer_mode`, the shared
/// camera's `camera_*`), and `system_ui` controls list no `camera:*`.
/// `owner` is the headless server's own selection and Inspect document.
pub(crate) fn serve_headless(server: &mut sim_api::Server, scene: &mut SpatialScene, camera: &mut Orbit, task: &mut Option<sim_api::ImageTask>, mut owner: Option<Owner>) {
    let mode = ViewerMode::Inspect;
    server.poll(|command, continuation, cancelled| {
        let outcome = match crate::app::route::route(mode, false, command) {
            Err(e) => Outcome::Done(Err(e)),
            Ok(feature) if feature.name == "inspect" => match <InspectAction as actions::Action>::parse(command) {
                Ok(action) => {
                    let mut replies = Replies::default();
                    handle(scene, camera, task, None, owner.as_mut(), &action, &mut Call { origin: actions::Origin::Rest(actions::Reply::HEADLESS), continuation, cancelled, replies: &mut replies })
                }
                Err(e) => Outcome::Done(Err(e)),
            },
            // `screenshot` and the switcher's `system_ui` (WindowAction).
            Ok(feature) if feature.name == "window" || feature.name == "switcher" => match <crate::app::switch::WindowAction as actions::Action>::parse(command) {
                Ok(action) => Outcome::Done(action.headless()),
                Err(e) => Outcome::Done(Err(e)),
            },
            Ok(feature) => Outcome::Done(Err(format!("`{}` ({} commands) is not served by the headless server (inspect mode only)", command.command, feature.name))),
        };
        crate::app::route::annotate_for(mode, false, command, outcome)
    });
    let selected = selected(scene, owner.as_ref());
    publish_json(server, scene, camera, &selected, None, None, mode);
}

/// `selected` is what the metadata reports as the selection.
pub(crate) fn capture(scene: &SpatialScene, camera: &Orbit, selected: &SelectionTarget, options: &sim_render::physical::Options) -> Result<sim_render::physical::Snapshot, String> {
    options.size.validate()?;
    if options.parts.iter().any(|id| !scene.spatial.parts.iter().any(|p| &p.id == id || &p.component == id)) {
        return Err("unknown component or part in render filter".into());
    }
    let hover = if scene.note_pointer_hover != SelectionTarget::None { &scene.note_pointer_hover } else { &scene.note_hover };
    let emphasized = hover.resolve(&scene.description).map_err(|e| e.to_string())?.components;
    let mut parts = Vec::new();
    for (i, p) in scene.spatial.parts.iter().enumerate() {
        if !options.include_hidden && scene.state.hidden.contains(&p.component) {
            continue;
        }
        if !options.parts.is_empty() && !options.parts.contains(&p.id) && !options.parts.contains(&p.component) {
            continue;
        }
        let mut transform = animation::part_transform(scene, i);
        if let Some(exploded) = options.exploded {
            if exploded != scene.state.exploded {
                transform.translation += Vec3::from_array(p.exploded_offset) * if exploded { 1. } else { -1. };
            }
        }
        parts.push(sim_render::physical::Part {
            id: p.id.clone(),
            component: p.component.clone(),
            label: scene.description.components[&p.component].label.clone(),
            shape: p.shape.clone(),
            position: transform.translation.to_array(),
            rotation: transform.rotation.to_array(),
            color: animation::part_color(scene, i).unwrap_or(p.color_srgb),
            selected: emphasized.contains(&p.component) || scene.details.components.contains(&p.component),
        });
    }
    let mut connections = Vec::new();
    if options.connections.unwrap_or(scene.state.connections) {
        let positions: BTreeMap<_, _> = parts.iter().map(|p| (p.component.clone(), Vec3::from_array(p.position))).collect();
        for net in scene.description.nets.values() {
            let components: std::collections::BTreeSet<_> = net.ports.iter().filter_map(|id| scene.description.ports.get(id).map(|p| &p.component)).collect();
            let points: Vec<_> = components.iter().filter_map(|id| positions.get(*id)).copied().collect();
            if points.len() > 1 {
                let hub = points.iter().copied().sum::<Vec3>() / points.len() as f32;
                connections.extend(points.into_iter().map(|p| (p.to_array(), hub.to_array())));
            }
        }
    }
    Ok(sim_render::physical::Snapshot {
        parts,
        regions: scene
            .note_document()
            .notes
            .values()
            .filter_map(|n| Some(sim_render::Region { label: n.label.clone(), color: n.color, components: n.targets.resolve(&scene.description).ok()?.components }))
            .collect(),
        connections,
        // The view's heading now (in the trackball its stored yaw/pitch are stale).
        yaw: camera.turntable().0,
        pitch: camera.turntable().1,
        metadata: json!({"annotations":scene.note_document(),"source_description_id":scene.description.id,"selection":selected,"frame":scene.frame().map(|f|json!({"run_id":f.run_id,"generation":f.generation,"sequence":f.sequence,"step":f.step,"time":f.time})),"live_status":scene.live_status(),"geometry":"illustrative display primitives; not source CAD solids"}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scene() -> SpatialScene {
        SpatialScene::new(
            serde_json::from_str(include_str!("../../../examples/systems-viewer/spatial/motor-thermal.description.json")).unwrap(),
            serde_json::from_str(include_str!("../../../examples/systems-viewer/spatial/motor-thermal.spatial.json")).unwrap(),
        )
        .unwrap()
    }
    fn parse(name: &str, args: Value) -> InspectAction {
        <InspectAction as actions::Action>::parse(&sim_api::Command { command: name.into(), args }).unwrap()
    }
    #[test]
    fn rest_selection_display_and_camera_preserve_source() {
        let mut scene = scene();
        let before = json!(scene.description);
        let mut camera = Orbit { focus: Vec3::ZERO, radius: 1., yaw: 0., pitch: 0., home: false, ..Default::default() };
        let id = scene.spatial.parts[0].component.clone();
        let (mut selection, mut registry) = (Selection::default(), DocumentRegistry::default());
        let document = registry.open(ViewerMode::Inspect, crate::document::DocumentKind::Assembly, crate::document::Source::path("motor-thermal.description.json")).id;
        for (name, args) in [("select", json!({"target":{"kind":"components","ids":[id]}})), ("display", json!({"action":{"kind":"hide_selected"}}))] {
            execute(&mut scene, &mut camera, Owner::inspect(&mut selection, &registry).as_mut(), &parse(name, args)).unwrap();
        }
        assert!(scene.state.hidden.contains(&id));
        // The shared selection owns it; the view shows it; `state` reports it.
        assert_eq!(selection.components(document), vec![id.clone()]);
        assert_eq!(scene.shown, SelectionTarget::component(id.clone()));
        let shown = execute(&mut scene, &mut camera, Owner::inspect(&mut selection, &registry).as_mut(), &parse("state", json!({}))).unwrap();
        assert_eq!(shown["selection"], json!({"kind":"components","ids":[id]}));
        assert!(execute(&mut scene, &mut camera, None, &parse("camera", json!({"focus":[0,0,0],"radius":-1,"yaw":0,"pitch":0}))).is_err());
        assert_eq!(camera.radius, 1.);
        assert_eq!(json!(scene.description), before);
    }

    /// An unknown id is refused with `set_selection`'s words and leaves the
    /// shared selection as it was; `display` clear_selection clears it.
    #[test]
    fn rest_select_refuses_unknown_ids_and_display_clears_the_shared_selection() {
        let expected = scene().set_selection(SelectionTarget::component("no-such-component")).unwrap_err().to_string();
        let mut scene = scene();
        let mut camera = Orbit::default();
        let (mut selection, mut registry) = (Selection::default(), DocumentRegistry::default());
        let document = registry.open(ViewerMode::Inspect, crate::document::DocumentKind::Assembly, crate::document::Source::path("a.description.json")).id;
        let id = scene.spatial.parts[0].component.clone();
        execute(&mut scene, &mut camera, Owner::inspect(&mut selection, &registry).as_mut(), &parse("display", json!({"action":{"kind":"select","component":id}}))).unwrap();
        assert_eq!(selection.components(document), vec![id.clone()]);
        let refused = execute(&mut scene, &mut camera, Owner::inspect(&mut selection, &registry).as_mut(), &parse("select", json!({"target":{"kind":"components","ids":["no-such-component"]}}))).unwrap_err();
        assert_eq!(refused, expected);
        assert_eq!(selection.components(document), vec![id]);
        execute(&mut scene, &mut camera, Owner::inspect(&mut selection, &registry).as_mut(), &parse("display", json!({"action":{"kind":"clear_selection"}}))).unwrap();
        assert!(selection.is_empty_for(document));
        assert_eq!(scene.shown, SelectionTarget::None);
        assert!(scene.details.components.is_empty());
    }

    /// A reload whose assembly lost a selected component drops that item by
    /// name and keeps the others, restamped; the view shows what is left.
    #[test]
    fn a_reload_drops_items_the_assembly_no_longer_has() {
        let mut scene = scene();
        let mut camera = Orbit::default();
        let (mut selection, mut registry) = (Selection::default(), DocumentRegistry::default());
        let source = crate::document::Source::path("a.description.json");
        let document = registry.open(ViewerMode::Inspect, crate::document::DocumentKind::Assembly, source.clone()).id;
        let ids: Vec<String> = scene.description.components.keys().take(2).cloned().collect();
        assert_eq!(ids.len(), 2, "the fixture has two components");
        execute(&mut scene, &mut camera, Owner::inspect(&mut selection, &registry).as_mut(), &parse("select", json!({"target":{"kind":"components","ids":ids}}))).unwrap();
        let mut seen = None;
        crate::inspect_view::projection::project(&mut scene, &mut selection, &registry, &mut seen);
        // The same assembly again, without the first component.
        let opened = registry.open(ViewerMode::Inspect, crate::document::DocumentKind::Assembly, source);
        assert!(opened.reload && opened.id == document);
        let mut reloaded = scene;
        reloaded.description.components.remove(&ids[0]);
        let dropped = crate::inspect_view::projection::project(&mut reloaded, &mut selection, &registry, &mut seen);
        assert_eq!(dropped, vec![format!("component {}", ids[0])]);
        assert_eq!(selection.dropped, dropped);
        assert_eq!(selection.components(document), vec![ids[1].clone()]);
        assert!(selection.of(document).all(|s| s.revision == 1));
        assert_eq!(reloaded.shown, SelectionTarget::component(ids[1].clone()));
    }
}
