//! CAD-style, constant-size viewport pins. Keyed entities survive panel rebuilds.
use super::*;
use bevy::ui::FocusPolicy;
use crate::ui_kit::UiFonts;

#[derive(Clone, serde::Serialize)]
pub(super) struct MarkerInfo {
    pub thread: String,
    pub target: String,
    pub number: String,
    /// Logical pixels relative to the viewer window (same frame as mouse input).
    pub anchor_px: [f32; 2],
    pub center_px: [f32; 2],
}
#[derive(Component)]
pub(super) struct Marker {
    key: (String, String),
}
#[derive(Component)]
pub(super) struct Leader {
    key: (String, String),
}
#[derive(Component)]
pub(super) struct Caption {
    key: (String, String),
    tooltip: bool,
}

pub(super) fn transform(b: &Builder, scene: &SpatialScene, path: &str) -> Option<Transform> {
    if let Some(i) = scene.spatial.parts.iter().position(|p| p.component == path) {
        if scene.state.hidden.contains(path) {
            return None;
        }
        return Some(placement::part_transform(b, scene, i));
    }
    // Groups remain eligible while at least one rendered descendant is visible.
    if !scene.spatial.parts.iter().any(|p| {
        p.component.starts_with(&format!("{path}/")) && !scene.state.hidden.contains(&p.component)
    }) {
        return None;
    }
    b.subsystems.get(path).map(|f| {
        Transform::from_translation(Vec3::from_array(f.position) + placement::preview_delta(b, path))
            .with_rotation(Quat::from_array(f.rotation_xyzw))
    })
}

fn center(anchor: Vec2, used: &[Vec2], min: Vec2, max: Vec2) -> Vec2 {
    let mut c = (anchor + Vec2::new(24., -24.)).clamp(min, max);
    for _ in 0..24 {
        if used.iter().all(|p| p.distance(c) >= 36.) {
            break;
        }
        c.y += 36.;
        if c.y > max.y {
            c.y = min.y;
            c.x = (c.x + 38.).min(max.x);
        }
    }
    c
}

pub(super) fn sync(
    mut commands: Commands,
    mut b: ResMut<Builder>,
    scene: Res<SpatialScene>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform), With<Orbit>>,
    fonts: Option<Res<UiFonts>>,
    mut markers: Query<
        (
            Entity,
            &Marker,
            &mut Node,
            &mut BackgroundColor,
            &Interaction,
        ),
        Without<Leader>,
    >,
    mut leaders: Query<
        (
            Entity,
            &Leader,
            &mut Node,
            &mut UiTransform,
            &mut BackgroundColor,
        ),
        Without<Marker>,
    >,
    mut captions: Query<(&Caption, &mut Text, &mut Node), (Without<Marker>, Without<Leader>)>,
) {
    let Some(fonts) = fonts else { return };
    let min = Vec2::new(scene.left() + 18., scene.top() + 18.);
    let max = Vec2::new(
        window.width() - scene.right() - 18.,
        window.height() - scene.bottom() - 18.,
    );
    if max.x <= min.x || max.y <= min.y {
        return;
    }
    let mut wanted = BTreeMap::new();
    let mut used = vec![];
    let mut infos = vec![];
    let mut add = |thread: String, path: String, number: String, pin: [f32; 3], title: String| {
        let Some(tr) = transform(&b, &scene, &path) else {
            return;
        };
        let Some(anchor) = camera
            .0
            .world_to_viewport(camera.1, tr.transform_point(Vec3::from_array(pin)))
            .ok()
        else {
            return;
        };
        if anchor.x < min.x - 18.
            || anchor.x > max.x + 18.
            || anchor.y < min.y - 18.
            || anchor.y > max.y + 18.
        {
            return;
        }
        let c = center(anchor, &used, min, max);
        used.push(c);
        let info = MarkerInfo {
            thread: thread.clone(),
            target: path.clone(),
            number,
            anchor_px: anchor.to_array(),
            center_px: c.to_array(),
        };
        wanted.insert((thread, path), (info.clone(), title));
        infos.push(info);
    };
    for (n, t) in b
        .document
        .discussions
        .threads
        .values()
        .enumerate()
        .filter(|(_, t)| !t.resolved)
    {
        let mut seen = BTreeSet::new();
        for target in t
            .targets
            .iter()
            .chain(t.comments.iter().flat_map(|c| &c.links))
            .filter(|r| !r.missing)
        {
            if !seen.insert(target.path.clone()) {
                continue;
            }
            let pin = if t.targets.first().is_some_and(|r| r.path == target.path) {
                t.pin_m.unwrap_or([0.; 3])
            } else {
                [0.; 3]
            };
            add(
                t.id.clone(),
                target.path.clone(),
                format!("{}{}",n+1,if b.agent_badge(&t.id).is_some(){"•"}else{""}),
                pin,
                format!(
                    "{}\n{} · {} comments{}",
                    t.title,
                    target.path,
                    t.comments.len(),
                    b.agent_badge(&t.id).map(|s|format!("\n{s}")).unwrap_or_default()
                ),
            );
        }
    }
    if b.input
        .as_ref()
        .is_some_and(|i| i.purpose == Purpose::Comment)
        && b.discussion.selected.is_none()
    {
        if let Some(path) = b.discussion.draft_targets.first() {
            add(
                "draft".into(),
                path.clone(),
                "+".into(),
                b.discussion.draft_pin.unwrap_or([0.; 3]),
                "New discussion · Enter posts".into(),
            );
        }
    }
    b.discussion.markers = infos;
    let mut existing = BTreeSet::new();
    let mut hovered = BTreeSet::new();
    let selected = b.discussion.selected.as_deref();
    for (entity, marker, mut node, mut bg, interaction) in &mut markers {
        let Some((info, _)) = wanted.get(&marker.key) else {
            commands.entity(entity).despawn();
            continue;
        };
        existing.insert(marker.key.clone());
        let hover = matches!(interaction, Interaction::Hovered | Interaction::Pressed);
        if hover {
            hovered.insert(marker.key.clone());
        }
        node.left = Val::Px(info.center_px[0] - 15.);
        node.top = Val::Px(info.center_px[1] - 15.);
        bg.0 = if hover || selected == Some(info.thread.as_str()) {
            Color::srgb(0.55, 0.34, 0.10)
        } else {
            Color::srgb(0.07, 0.25, 0.32)
        };
    }
    for (entity, line, mut node, mut tr, mut bg) in &mut leaders {
        let Some((info, _)) = wanted.get(&line.key) else {
            commands.entity(entity).despawn();
            continue;
        };
        let a = Vec2::from_array(info.anchor_px);
        let c = Vec2::from_array(info.center_px);
        let delta = c - a;
        node.left = Val::Px((a.x + c.x - delta.length()) * 0.5);
        node.top = Val::Px((a.y + c.y) * 0.5 - 1.);
        node.width = Val::Px(delta.length());
        tr.rotation = Rot2::radians(delta.y.atan2(delta.x));
        bg.0 = if hovered.contains(&line.key) {
            Color::srgb(1., 0.78, 0.3)
        } else {
            Color::srgb(0.45, 0.80, 0.95)
        };
    }
    for (caption, mut text, mut node) in &mut captions {
        if let Some((info, title)) = wanted.get(&caption.key) {
            text.0 = if caption.tooltip {
                title.clone()
            } else {
                info.number.clone()
            };
            if caption.tooltip {
                node.display = if hovered.contains(&caption.key) {
                    Display::Flex
                } else {
                    Display::None
                };
            }
        }
    }
    for (key, (info, title)) in wanted.iter().filter(|(k, _)| !existing.contains(*k)) {
        let marker = commands
            .spawn((
                Marker { key: key.clone() },
                Button, crate::ui_kit::activation::Ordinary,
                Node { border_radius: BorderRadius::MAX,
                    position_type: PositionType::Absolute,
                    left: Val::Px(info.center_px[0] - 15.),
                    top: Val::Px(info.center_px[1] - 15.),
                    width: Val::Px(30.),
                    height: Val::Px(30.),
                    border: UiRect::all(Val::Px(2.)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                GlobalZIndex(20),
                BorderColor::all(Color::srgb(0.45, 0.80, 0.95)),
                BackgroundColor(Color::srgb(0.07, 0.25, 0.32)),
            ))
            .id();
        if info.thread != "draft" {
            commands
                .entity(marker)
                .insert(BuildAction::Discussion(discussion::Action::Open(
                    info.thread.clone(),
                )));
        }
        commands.entity(marker).with_children(|parent| {
            parent.spawn((
                Caption {
                    key: key.clone(),
                    tooltip: false,
                },
                Text::new(&info.number),
                TextFont {
                    font: fonts.semibold.clone().into(),
                    font_size: FontSize::Px(14.),
                    ..default()
                },
                TextColor(Color::WHITE),
                Pickable::IGNORE,
            ));
            parent.spawn((
                Caption {
                    key: key.clone(),
                    tooltip: true,
                },
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(34.),
                    top: Val::Px(0.),
                    width: Val::Px(190.),
                    padding: UiRect::all(Val::Px(7.)),
                    display: Display::None,
                    ..default()
                },
                Text::new(title),
                TextFont {
                    font: fonts.semibold.clone().into(),
                    font_size: FontSize::Px(12.),
                    ..default()
                },
                TextColor(Color::WHITE),
                BackgroundColor(Color::srgba(0.06, 0.09, 0.12, 0.96)),
                Pickable::IGNORE,
                FocusPolicy::Pass,
            ));
        });
        commands.spawn((
            Leader { key: key.clone() },
            Node {
                position_type: PositionType::Absolute,
                height: Val::Px(2.),
                ..default()
            },
            BackgroundColor(Color::srgb(0.45, 0.80, 0.95)),
            GlobalZIndex(19),
            Pickable::IGNORE,
            FocusPolicy::Pass,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_pins_separate_and_stay_in_viewport() {
        let (min, max) = (Vec2::splat(20.), Vec2::splat(400.));
        let a = center(Vec2::new(390., 10.), &[], min, max);
        let b = center(Vec2::new(390., 10.), &[a], min, max);
        assert!(a.distance(b) >= 36.);
        assert!(a.cmpge(min).all() && a.cmple(max).all());
        assert!(b.cmpge(min).all() && b.cmple(max).all());
    }
}
