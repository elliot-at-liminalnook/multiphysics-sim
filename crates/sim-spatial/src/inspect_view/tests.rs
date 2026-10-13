use super::scene::pick_part;
use super::ui::inspector;
use super::*;
use crate::camera::Orbit;
use crate::tests::fixture;
use bevy::picking::{
    backend::HitData,
    pointer::{Location, PointerButton, PointerId},
};
use bevy::camera::RenderTarget;
use bevy::ui_widgets::Button;

#[test]
fn linked_net_and_port_selection_reveal_geometry_without_editing_sources() {
    let mut scene = fixture();
    let physical = serde_json::to_vec(&scene.description).unwrap();
    let geometry = serde_json::to_vec(&scene.spatial).unwrap();
    scene.state.hidden = scene.description.components.keys().cloned().collect();
    let net = scene
        .description
        .nets
        .values()
        .find(|n| n.ports.len() > 2)
        .unwrap()
        .clone();
    scene.set_selection(SelectionTarget::net(&net.id)).unwrap();
    assert!(scene.state.connections);
    assert_eq!(scene.details.nets.len(), 1);
    assert_eq!(scene.details.ports.len(), net.ports.len());
    assert!(
        scene
            .details
            .components
            .iter()
            .all(|id| !scene.state.hidden.contains(id))
    );
    assert!(linked::net_geometry(&scene, &net.id).is_some());
    assert!(inspector(&scene).contains("CONNECTION"));
    scene.apply(SpatialCommand::HideSelected).unwrap();
    assert!(
        scene
            .details
            .components
            .iter()
            .all(|id| scene.state.hidden.contains(id))
    );
    assert!(linked::net_geometry(&scene, &net.id).is_none());
    scene.set_selection(SelectionTarget::None).unwrap();
    scene.set_selection(SelectionTarget::net(&net.id)).unwrap();
    assert!(linked::net_geometry(&scene, &net.id).is_some());
    let port = net.ports[0].clone();
    scene
        .set_selection(SelectionTarget::Ports {
            ids: std::collections::BTreeSet::from([port.clone()]),
        })
        .unwrap();
    assert!(
        scene
            .details
            .components
            .contains(&scene.description.ports[&port].component)
    );
    assert!(inspector(&scene).contains("PORT"));
    assert_eq!(serde_json::to_vec(&scene.description).unwrap(), physical);
    assert_eq!(serde_json::to_vec(&scene.spatial).unwrap(), geometry);
    scene.set_selection(SelectionTarget::None).unwrap();
    assert_eq!(scene.details, SelectionDetails::default());
}
#[test]
fn mesh_click_and_list_button_resolve_the_same_source_identity() {
    let scene = fixture();
    let expected = scene.spatial.parts[1].component.clone();
    let mut app = App::new();
    app.insert_resource(scene)
        .insert_resource(ButtonInput::<KeyCode>::default())
        .init_resource::<app::actions::Replies>();
    app::actions::register::<inspect::InspectAction>(&mut app);
    // A press writes the action; its one handler applies it.
    app.add_systems(Update, (inspect::input, inspect::apply).chain());
    let camera = app
        .world_mut()
        .spawn(Orbit {
            focus: Vec3::ZERO,
            radius: 1.0,
            yaw: 0.0,
            pitch: 0.0,
            home: false,
            ..Default::default()
        })
        .id();
    let part = app
        .world_mut()
        .spawn(Part { index: 1 })
        .observe(pick_part)
        .id();
    let window = app.world_mut().spawn_empty().id();
    let click = bevy::picking::events::PointerClick {
        entity: part,
        pointer: bevy::picking::events::Pointer::new(
            PointerId::Mouse,
            Location {
                target: RenderTarget::Window(bevy::window::WindowRef::Entity(window))
                    .normalize(Some(window))
                    .unwrap(),
                position: Vec2::ZERO,
            },
        ),
        button: PointerButton::Primary,
        hit: HitData::new(camera, 0.1, None, None),
        duration: std::time::Duration::from_millis(50),
        count: 1,
    };
    app.world_mut().trigger(click);
    app.update();
    assert_eq!(
        app.world()
            .resource::<SpatialScene>()
            .state
            .selected
            .as_ref(),
        Some(&expected)
    );
    assert!(inspector(app.world().resource::<SpatialScene>()).contains("resistance: 2"));
    app.world_mut()
        .resource_mut::<SpatialScene>()
        .apply(SpatialCommand::ClearSelection)
        .unwrap();
    let button = app
        .world_mut()
        .spawn((
            Button,
            crate::ui_kit::activation::Activated,
            inspect::InspectAction::Display { action: SpatialCommand::Select { component: expected.clone() } },
        ))
        .id();
    app.update();
    assert_eq!(
        app.world()
            .resource::<SpatialScene>()
            .state
            .selected
            .as_ref(),
        Some(&expected)
    );
    *app.world_mut().get_mut::<inspect::InspectAction>(button).unwrap() = inspect::InspectAction::Toggle(inspect::Toggle::Exploded);
    app.world_mut().entity_mut(button).remove::<crate::ui_kit::activation::Activated>();
    app.update();
    app.world_mut().entity_mut(button).insert(crate::ui_kit::activation::Activated);
    app.update();
    app.world_mut().entity_mut(button).remove::<crate::ui_kit::activation::Activated>();
    assert!(app.world().resource::<SpatialScene>().state.exploded);
    app.update();
    assert!(
        app.world().resource::<SpatialScene>().state.exploded,
        "holding a button must not repeatedly toggle"
    );
}

/// A primary click on `part`, as the picking backend sends it.
fn click(app: &mut App, camera: Entity, part: Entity) {
    let window = app.world_mut().spawn_empty().id();
    let click = bevy::picking::events::PointerClick {
        entity: part,
        pointer: bevy::picking::events::Pointer::new(
            PointerId::Mouse,
            Location { target: RenderTarget::Window(bevy::window::WindowRef::Entity(window)).normalize(Some(window)).unwrap(), position: Vec2::ZERO },
        ),
        button: PointerButton::Primary, hit: HitData::new(camera, 0.1, None, None), duration: std::time::Duration::from_millis(50), count: 1,
    };
    app.world_mut().trigger(click);
}

/// Inspect's click, parts row and REST `select` write the one shared
/// selection (the Inspect document's items) through the same handler, and
/// the view shows it; Escape clears it.
#[test]
fn click_parts_row_and_rest_select_make_the_same_shared_selection() {
    use crate::document::{DocumentKind, DocumentRegistry, Source};
    use crate::selection::{Item, Selection};
    let scene = fixture();
    let id = scene.spatial.parts[1].component.clone();
    let mut registry = DocumentRegistry::default();
    let document = registry.open(ViewerMode::Inspect, DocumentKind::Assembly, Source::path("motor-thermal.description.json")).id;
    let mut app = App::new();
    app.insert_resource(scene).insert_resource(ButtonInput::<KeyCode>::default()).init_resource::<app::actions::Replies>().init_resource::<Selection>().insert_resource(registry);
    app::actions::register::<inspect::InspectAction>(&mut app);
    app.add_systems(Update, (inspect::input, inspect::apply, projection::project_selection).chain());
    let camera = app.world_mut().spawn(Orbit::default()).id();
    let part = app.world_mut().spawn(Part { index: 1 }).observe(pick_part).id();
    let selected = |app: &App| app.world().resource::<Selection>().all().to_vec();
    let shown = |app: &App| app.world().resource::<SpatialScene>().shown.clone();
    let escape = |app: &mut App| {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::Escape);
        keys.clear();
        assert!(app.world().resource::<Selection>().is_empty_for(document));
        assert_eq!(app.world().resource::<SpatialScene>().shown, SelectionTarget::None);
    };

    click(&mut app, camera, part);
    app.update();
    let clicked = selected(&app);
    assert_eq!(clicked.len(), 1);
    assert_eq!((&clicked[0].item, clicked[0].document, clicked[0].revision), (&Item::Component { id: id.clone() }, document, 0));
    assert_eq!(shown(&app), SelectionTarget::component(id.clone()));
    assert_eq!(app.world().resource::<SpatialScene>().state.selected.as_ref(), Some(&id));
    escape(&mut app);

    // REST `select`, parsed as the server parses it, through the same handler.
    let command = sim_api::Command { command: "select".into(), args: serde_json::json!({"target": {"kind": "components", "ids": [id]}}) };
    let rest = <inspect::InspectAction as app::actions::Action>::parse(&command).unwrap();
    app.world_mut().write_message(app::actions::Act::quiet(rest));
    app.update();
    assert_eq!(selected(&app), clicked);
    assert_eq!(shown(&app), SelectionTarget::component(id.clone()));
    escape(&mut app);

    // The parts list's row.
    app.world_mut().spawn((Button, crate::ui_kit::activation::Activated, inspect::InspectAction::Display { action: SpatialCommand::Select { component: id.clone() } }));
    app.update();
    assert_eq!(selected(&app), clicked);
    assert_eq!(shown(&app), SelectionTarget::component(id));
}

#[test]
fn same_look_parts_share_a_material_until_one_is_tinted() {
    use bevy::ecs::system::RunSystemOnce;
    let mut scene = fixture();
    // Force two primitive parts to the same spawn look.
    scene.spatial.parts[1].color_srgb = scene.spatial.parts[0].color_srgb;
    let base = scene.spatial.parts[0].color_srgb;
    let first_component = scene.spatial.parts[0].component.clone();
    let mut app = App::new();
    app.insert_resource(scene)
        .insert_resource(Assets::<Mesh>::default())
        .insert_resource(Assets::<StandardMaterial>::default())
        .init_resource::<super::scene::PartMaterials>()
        .add_systems(Update, super::scene::update_parts);
    app.world_mut()
        .run_system_once(|mut commands: Commands, scene: Res<SpatialScene>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut cache: ResMut<super::scene::PartMaterials>| {
            super::scene::spawn_parts(&mut commands, &scene, &mut meshes, &mut materials, Some(&mut cache), None);
        })
        .unwrap();
    let handles = |app: &mut App| {
        let world = app.world_mut();
        let mut query = world.query::<(&Part, &MeshMaterial3d<StandardMaterial>)>();
        let mut by_index: Vec<(usize, Handle<StandardMaterial>)> = query.iter(world).map(|(part, material)| (part.index, material.0.clone())).collect();
        by_index.sort_by_key(|(index, _)| *index);
        by_index
    };
    let spawned = handles(&mut app);
    let shared = spawned.iter().find(|(index, _)| *index == 0).unwrap().1.clone();
    assert_eq!(spawned.iter().find(|(index, _)| *index == 1).unwrap().1, shared, "same-look parts share one material asset");
    // Let `update_parts` initialize its change tick before the tint.
    app.update();

    // Tint only the first part (selection): it gets a private material,
    // and the shared asset the second part still uses is not recoloured.
    app.world_mut().resource_mut::<SpatialScene>().details.components.insert(first_component);
    app.update();
    let tinted = handles(&mut app);
    let first = tinted.iter().find(|(index, _)| *index == 0).unwrap().1.clone();
    assert_ne!(first, shared);
    assert_eq!(tinted.iter().find(|(index, _)| *index == 1).unwrap().1, shared);
    let materials = app.world().resource::<Assets<StandardMaterial>>();
    assert_eq!(materials.get(&first).unwrap().base_color, Color::srgb(0.96, 0.70, 0.26));
    assert_eq!(materials.get(&shared).unwrap().base_color, Color::srgb(base[0], base[1], base[2]));

    // Clearing the tint swaps the first part back to the shared handle.
    app.world_mut().resource_mut::<SpatialScene>().details.components.clear();
    app.update();
    let restored = handles(&mut app);
    assert_eq!(restored.iter().find(|(index, _)| *index == 0).unwrap().1, shared);
}
