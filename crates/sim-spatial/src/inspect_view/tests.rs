use super::scene::pick_part;
use super::ui::inspector;
use super::*;
use crate::tests::fixture;
use bevy::picking::{
    backend::HitData,
    pointer::{Location, PointerButton, PointerId},
};
use bevy::camera::RenderTarget;

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
    let click = Pointer::new(
        PointerId::Mouse,
        Location {
            target: RenderTarget::Window(bevy::window::WindowRef::Entity(window))
                .normalize(Some(window))
                .unwrap(),
            position: Vec2::ZERO,
        },
        Click {
            button: PointerButton::Primary,
            hit: HitData::new(camera, 0.1, None, None),
            duration: std::time::Duration::from_millis(50),
            count: 1,
        },
        part,
    );
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
            Interaction::Pressed,
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
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None;
    app.update();
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
    app.update();
    assert!(app.world().resource::<SpatialScene>().state.exploded);
    app.update();
    assert!(
        app.world().resource::<SpatialScene>().state.exploded,
        "holding a button must not repeatedly toggle"
    );
}
