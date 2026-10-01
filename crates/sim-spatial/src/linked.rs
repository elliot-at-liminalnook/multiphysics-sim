use super::*;
use crate::document::DocumentRegistry;
use crate::selection::{Selection, SelectionAction, target_items};
use crate::ui_kit::ACCENT;
use sim_inspect::selection::native::SelectionClient;

/// Inspect's selection link with a schematic peer (`--schematic`,
/// `--selection-link`): it exchanges Inspect's shared selection.
#[derive(Resource)]
pub struct SelectionLink(pub SelectionClient);

/// One exchange with the linked peer: Inspect's selection (the shared
/// selection's items of the Inspect document) goes out; a different target
/// from the peer is checked against the assembly (an unknown id is refused,
/// as `set_selection` refused it) and applied as a set, stamped current.
/// Without the Inspect document the view's projection is exchanged, and the
/// peer's target is returned for the caller to show (as before).
pub(crate) fn exchange(link: &mut SelectionLink, scene: &SpatialScene, owner: Option<(&mut Selection, &DocumentRegistry)>) -> Result<Option<SelectionTarget>, String> {
    let document = owner.as_ref().and_then(|(_, registry)| registry.current(ViewerMode::Inspect)).map(|(document, _)| document);
    match (owner, document) {
        (Some((selection, registry)), Some(document)) => {
            let mine = selection.target(document);
            let peer = link.0.exchange(mine.clone()).map_err(|e| e.to_string())?;
            if peer != mine {
                peer.resolve(&scene.description).map_err(|e| e.to_string())?;
                selection.apply(registry, &SelectionAction::set(document, target_items(&peer)))?;
            }
            Ok(None)
        }
        _ => {
            let peer = link.0.exchange(scene.shown.clone()).map_err(|e| e.to_string())?;
            Ok((peer != scene.shown).then_some(peer))
        }
    }
}
#[derive(Component)]
pub(super) struct LinkStatus;
#[derive(Component)]
pub(super) struct NetHub(pub String);

/// SimSync: the link's exchange (Inspect's selection; the projection
/// shows what the peer chose) and its status label.
pub(super) fn sync_link(
    mut scene: ResMut<SpatialScene>,
    link: Option<ResMut<SelectionLink>>,
    selection: Option<ResMut<Selection>>,
    registry: Option<Res<DocumentRegistry>>,
    mut labels: Query<&mut Text, With<LinkStatus>>,
) {
    let Some(mut link) = link else { return };
    let mut selection = selection;
    let owner = selection.as_deref_mut().zip(registry.as_deref());
    match exchange(&mut link, &scene, owner) {
        Ok(Some(target)) => {
            if let Err(e) = scene.set_selection(target) {
                error!("{e}");
            }
        }
        Err(e) => error!("{e}"),
        Ok(None) => {}
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
/// A click on a net hub selects the net: the connection buttons' action
/// (a refusal is logged by `inspect::apply`, as before).
fn pick_net(click: On<Pointer<Click>>, nets: Query<&NetHub>, mut out: MessageWriter<crate::app::actions::Act<crate::inspect::InspectAction>>) {
    if click.button == bevy::picking::pointer::PointerButton::Primary {
        if let Ok(net) = nets.get(click.entity) {
            out.write(crate::app::actions::Act::ui(crate::inspect::InspectAction::Select { target: SelectionTarget::net(net.0.clone()) }));
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
/// The inspector for what is shown (a group, ports or nets).
pub(super) fn selection_inspector(scene: &SpatialScene) -> Option<String> {
    let (title, ids) = match &scene.shown {
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
