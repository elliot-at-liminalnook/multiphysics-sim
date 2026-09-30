use super::*;
use sim_inspect::selection::native::SelectionClient;

#[derive(Resource)]
pub struct SelectionLink(pub SelectionClient);
#[derive(Component)]
pub(super) struct LinkStatus;
#[derive(Component)]
pub(super) struct NetHub(pub String);

pub(super) fn sync_link(
    mut scene: ResMut<SpatialScene>,
    link: Option<ResMut<SelectionLink>>,
    mut labels: Query<&mut Text, With<LinkStatus>>,
) {
    let Some(mut link) = link else { return };
    match link.0.exchange(scene.selection.clone()) {
        Ok(target) if target != scene.selection => {
            if let Err(e) = scene.set_selection(target) {
                error!("{e}");
            }
        }
        Err(e) => error!("{e}"),
        _ => {}
    }
    let status = link.0.status("schematic");
    for mut label in &mut labels {
        if label.0 != status {
            label.0.clone_from(&status);
        }
    }
}

pub(super) fn net_geometry(scene: &SpatialScene, id: &str) -> Option<(Vec3, Vec<Vec3>)> {
    let positions = scene.positions();
    let net = &scene.description.nets[id];
    let mut endpoints = Vec::new();
    for id in &net.ports {
        let p = &scene.description.ports[id];
        if scene.state.hidden.contains(&p.component) {
            return None;
        }
        endpoints.push(*positions.get(&p.component)?);
    }
    if endpoints.len() < 2 {
        return None;
    }
    let (_, extent) = scene.bounds();
    let hub =
        endpoints.iter().copied().sum::<Vec3>() / endpoints.len() as f32 + Vec3::Y * extent * 0.22;
    Some((hub, endpoints))
}
pub(super) fn spawn_nets(
    commands: &mut Commands,
    scene: &SpatialScene,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mesh = meshes.add(Sphere::new(scene.bounds().1 * 0.022));
    for id in scene.description.nets.keys() {
        commands
            .spawn((
                NetHub(id.clone()),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: ACCENT,
                    unlit: true,
                    ..default()
                })),
                Transform::default(),
                Visibility::Hidden,
                Pickable::default(),
                SceneContent,
            ))
            .observe(pick_net);
    }
}
fn pick_net(click: On<Pointer<Click>>, nets: Query<&NetHub>, mut scene: ResMut<SpatialScene>) {
    if click.button == bevy::picking::pointer::PointerButton::Primary {
        if let Ok(net) = nets.get(click.entity) {
            if let Err(e) = scene.set_selection(SelectionTarget::net(net.0.clone())) {
                error!("{e}");
            }
        }
    }
}
pub(super) fn update_nets(
    scene: Res<SpatialScene>,
    mut nets: Query<(
        &NetHub,
        &mut Transform,
        &mut Visibility,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !scene.is_changed() {
        return;
    }
    for (net, mut transform, mut visibility, material) in &mut nets {
        if let Some((hub, _)) = net_geometry(&scene, &net.0).filter(|_| scene.state.connections) {
            transform.translation = hub;
            *visibility = Visibility::Inherited;
        } else {
            *visibility = Visibility::Hidden;
        }
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.base_color = if scene.details.nets.contains(&net.0) {
                Color::srgb(1.0, 0.76, 0.32)
            } else {
                ACCENT
            };
        }
    }
}
pub(super) fn selection_inspector(scene: &SpatialScene) -> Option<String> {
    let (title, ids) = match &scene.selection {
        SelectionTarget::Nets { ids } => ("CONNECTION", ids),
        SelectionTarget::Ports { ids } => ("PORT", ids),
        SelectionTarget::Components { ids } if ids.len() > 1 => ("COMPONENT GROUP", ids),
        _ => return None,
    };
    let mut s = format!("{title}\n{} selected\n", ids.len());
    s.push_str("\nTERMINALS\n");
    for id in &scene.details.ports {
        let p = &scene.description.ports[id];
        s.push_str(&format!(
            "{}.{}\n",
            scene.description.components[&p.component].label, p.name
        ));
    }
    let represented: std::collections::BTreeSet<_> =
        scene.spatial.parts.iter().map(|p| &p.component).collect();
    for id in &scene.details.components {
        if !represented.contains(id) {
            s.push_str(&format!(
                "\nNo geometry: {}\n",
                scene.description.components[id].label
            ));
        }
    }
    s.push_str("\nGold marks selected components and connections. Guides show topology, not physical wire routes.\n\nSOURCE IDS\n");
    for id in ids {
        s.push_str(&format!("{id}\n"));
    }
    s.push_str(&format!("\n{}", scene.live_status()));
    Some(s)
}
