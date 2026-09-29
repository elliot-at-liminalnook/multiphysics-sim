//! How the camera moves and what the view emphasises: eased glides between
//! framings (never a cut), slow orbiting, zoom to a part, and the display
//! directives shared with lesson scripts and narration (spotlight, pins,
//! picture-in-picture, X-ray, exploded view). The learner's own input always
//! wins: dragging or zooming stops any scripted move.
use super::*;

/// A camera position around its focus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pose {
    pub focus: Vec3,
    pub radius: f32,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Glide {
    from: Pose,
    to: Pose,
    t: f32,
    duration: f32,
}

/// Default glide time: long enough to follow where the view goes (about a
/// second), short enough not to feel like waiting.
pub(crate) const GLIDE_S: f32 = 1.0;

fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

impl Orbit {
    pub(crate) fn pose(&self) -> Pose {
        Pose { focus: self.focus, radius: self.radius, yaw: self.yaw, pitch: self.pitch }
    }
    fn set_pose(&mut self, p: Pose) {
        self.focus = p.focus;
        self.radius = p.radius;
        self.yaw = p.yaw;
        self.pitch = p.pitch;
    }
    /// Ease to `to` over `seconds`; 0 (or no previous pose) cuts.
    pub(crate) fn glide_to(&mut self, to: Pose, seconds: f32) {
        self.home = false;
        if seconds <= 0. || self.radius <= 0. {
            self.glide = None;
            self.set_pose(to);
            return;
        }
        // Turn the short way round.
        let to = Pose { yaw: self.yaw + wrap(to.yaw - self.yaw), ..to };
        self.glide = Some(Glide { from: self.pose(), to, t: 0., duration: seconds });
    }
    /// The direction the camera is settling on: a glide's destination, else
    /// where it is now. A zoom that starts during a glide keeps that heading.
    pub(crate) fn heading(&self) -> (f32, f32) {
        self.glide.map_or((self.yaw, self.pitch), |g| (g.to.yaw, g.to.pitch))
    }
    /// Advance any glide and spin by `dt` wall seconds.
    pub(crate) fn step(&mut self, dt: f32) {
        if let Some(mut g) = self.glide {
            g.t += dt / g.duration.max(1e-3);
            let s = ease(g.t);
            let lerp = |a: f32, b: f32| a + (b - a) * s;
            // Radius eases in log space so zooms feel even.
            let radius = (g.from.radius.max(1e-6).ln() + (g.to.radius.max(1e-6).ln() - g.from.radius.max(1e-6).ln()) * s).exp();
            self.set_pose(Pose { focus: g.from.focus.lerp(g.to.focus, s), radius, yaw: lerp(g.from.yaw, g.to.yaw), pitch: lerp(g.from.pitch, g.to.pitch) });
            self.glide = (g.t < 1.).then_some(g);
        }
        if self.spin != 0. {
            self.yaw += self.spin * dt;
        }
    }
    /// Jump to where a glide was going (reduced motion: cuts, not glides).
    pub(crate) fn finish_glide(&mut self) {
        if let Some(g) = self.glide.take() {
            self.set_pose(g.to);
        }
    }
    /// The learner took over: stop scripted motion.
    pub(crate) fn interrupt(&mut self) {
        self.glide = None;
        self.spin = 0.;
    }
}

/// The pose framing an instance path (none: everything) at `zoom`, from
/// `yaw`/`pitch`, for a view of the given aspect ratio.
pub(crate) fn frame_pose(scene: &SpatialScene, focus: Option<&str>, zoom: f32, yaw: f32, pitch: f32, aspect: f32) -> Pose {
    let (center, radius) = scene.bounds_of(focus);
    Pose { focus: center, radius: radius.max(1e-3) * 2.9 / aspect.clamp(0.1, 1.0) / zoom.max(1e-3), yaw, pitch }
}

/// The aspect ratio of the 3D view as currently laid out.
pub(crate) fn aspect(scene: &SpatialScene, window: &Window) -> f32 {
    match scene.learn_view {
        Some(v) if v.full.height() > 0.0 => (v.full.width() / v.full.height()).max(0.1),
        _ => ((window.width() - scene.left() - scene.right()) / (window.height() - scene.top() - scene.bottom()).max(1.)).max(0.1),
    }
}

/// Glide to frame one component (or instance path) from the current direction.
pub(crate) fn zoom_to(scene: &SpatialScene, orbit: &mut Orbit, window: &Window, focus: Option<&str>, zoom: f32, seconds: f32) {
    let (yaw, pitch) = orbit.heading();
    let pose = frame_pose(scene, focus, zoom, yaw, pitch, aspect(scene, window));
    orbit.glide_to(pose, seconds);
}

/// The second camera of the picture-in-picture close-up.
#[derive(Component)]
pub(crate) struct InsetCamera;
#[derive(Component)]
pub(crate) struct InsetFrame;

/// Ease the exploded view open or closed.
pub(crate) fn animate(time: Res<Time>, mut scene: ResMut<SpatialScene>) {
    let target = if scene.state.exploded || scene.directives.explode { 1. } else { 0. };
    let t = scene.explode_t;
    if scene.reduced_motion {
        if t != target {
            scene.explode_t = target;
        }
        return;
    }
    if (t - target).abs() > 1e-4 {
        let step = time.delta_secs().min(0.1) / 0.8;
        scene.explode_t = if target > t { (t + step).min(target) } else { (t - step).max(target) };
    }
}

/// Parts of an instance path (itself and anything inside it).
fn inside(component: &str, path: &str) -> bool {
    path.is_empty() || component == path || component.starts_with(&format!("{path}/"))
}

/// See-through alpha for a part under the current directives (None: as usual).
pub(crate) fn emphasis(scene: &SpatialScene, component: &str) -> Option<f32> {
    let d = &scene.directives;
    let focused = d.spotlight.iter().any(|p| inside(component, p)) || scene.details.components.contains(component);
    if !d.spotlight.is_empty() && !focused {
        return Some(if d.xray || scene.state.xray { 0.08 } else { 0.12 });
    }
    if (d.xray || scene.state.xray) && !focused {
        return Some(0.22);
    }
    // A highlighted part: the others step back, softer than a spotlight.
    if !scene.soft_focus.is_empty() && !scene.soft_focus.iter().any(|p| inside(component, p)) {
        return Some(0.35);
    }
    None
}

/// Centre and size of an instance path's parts where they are drawn now.
pub(crate) fn live_bounds(scene: &SpatialScene, path: &str) -> Option<(Vec3, f32)> {
    let idx: Vec<usize> = scene.spatial.parts.iter().enumerate().filter(|(_, p)| inside(&p.component, path)).map(|(i, _)| i).collect();
    if idx.is_empty() {
        return None;
    }
    let centre = idx.iter().map(|&i| crate::animation::part_transform(scene, i).translation).sum::<Vec3>() / idx.len() as f32;
    let (_, radius) = scene.bounds_of(Some(path));
    Some((centre, radius))
}

/// The caption text on the inset and split views (spawned before the UI fonts load).
#[derive(Component)]
pub(crate) struct ViewCaption;

/// Give view captions the interface's semibold face once it has loaded.
pub(crate) fn caption_fonts(fonts: Option<Res<crate::builder::ui::UiFonts>>, mut captions: Query<&mut TextFont, With<ViewCaption>>) {
    let Some(fonts) = fonts else { return };
    for mut f in &mut captions {
        if f.font != fonts.semibold {
            f.font = fonts.semibold.clone();
        }
    }
}

/// 3D arrows with labels, anchored to parts (they follow the part and the camera).
pub(crate) fn draw_pins(scene: Res<SpatialScene>, mut gizmos: Gizmos, mut labels: ResMut<crate::physics_view::Labels>, camera: Single<(&GlobalTransform, &Orbit)>) {
    let (gt, orbit) = *camera;
    let up = gt.up().as_vec3();
    let right = gt.right().as_vec3();
    for (path, label) in &scene.directives.pins {
        let Some((centre, radius)) = live_bounds(&scene, path) else { continue };
        // Sized to the view, not the part: a large part's arrow stays short
        // and on screen.
        let d = (gt.translation() - centre).length().max(1e-3);
        let r = radius.max(0.003).min(d * 0.12);
        // From the side facing the middle of the view (up-left by default),
        // so the tail and its label stay on screen for parts near an edge.
        let to_middle = orbit.focus - centre;
        let vertical = if to_middle.dot(up) < -r { -0.8 } else { 0.8 };
        let horizontal = if to_middle.dot(right) > r { 0.6 } else { -0.6 };
        let dir = (up * vertical + right * horizontal).normalize();
        let head = centre + dir * r * 1.05;
        let tail = head + dir * d * 0.09;
        let color = Color::srgb(1.0, 0.85, 0.35);
        gizmos.arrow(tail, head, color).with_tip_length(d * 0.02);
        gizmos.sphere(Isometry3d::from_translation(tail), d * 0.004, color);
        if !label.is_empty() {
            // Authored emphasis: placed ahead of the physics labels.
            labels.0.insert(0, (tail, label.clone(), color));
        }
    }
}

/// Place the picture-in-picture camera in the lower-right of the main view,
/// looking at the inset's part from the main camera's direction.
#[allow(clippy::type_complexity)]
pub(crate) fn inset(
    scene: Res<SpatialScene>,
    window: Single<&Window>,
    main: Single<(&Camera, &Orbit), Without<InsetCamera>>,
    mut inset: Single<(&mut Camera, &mut Transform), (With<InsetCamera>, Without<Orbit>)>,
    mut frame: Query<(&mut Node, &mut Visibility, &Children), With<InsetFrame>>,
    mut texts: Query<&mut Text>,
) {
    let (main_camera, orbit) = *main;
    let (camera, transform) = &mut *inset;
    let target = scene.directives.inset.clone().and_then(|(path, zoom)| Some((live_bounds(&scene, &path)?, path, zoom)));
    let viewport = main_camera.viewport.clone().filter(|v| v.physical_size.x > 200 && v.physical_size.y > 150);
    let (Some(((centre, radius), path, zoom)), Some(v)) = (target, viewport) else {
        if camera.is_active {
            camera.is_active = false;
        }
        for (_, mut vis, _) in &mut frame {
            *vis = Visibility::Hidden;
        }
        return;
    };
    let size = (v.physical_size.as_vec2() * 0.34).max(Vec2::new(120., 90.)).as_uvec2();
    let margin = UVec2::splat((10. * window.scale_factor()) as u32);
    let position = v.physical_position + v.physical_size - size - margin;
    camera.is_active = true;
    camera.viewport = Some(Viewport { physical_position: position, physical_size: size, ..default() });
    let distance = radius.max(1e-3) * 2.9 / zoom.max(0.1) as f32;
    let horizontal = orbit.pitch.cos() * distance;
    **transform = Transform::from_translation(centre + Vec3::new(orbit.yaw.sin() * horizontal, orbit.pitch.sin() * distance, orbit.yaw.cos() * horizontal)).looking_at(centre, Vec3::Y);
    let scale = window.scale_factor();
    for (mut node, mut vis, children) in &mut frame {
        *vis = Visibility::Inherited;
        node.left = Val::Px(position.x as f32 / scale - 2.);
        node.top = Val::Px(position.y as f32 / scale - 2.);
        node.width = Val::Px(size.x as f32 / scale + 4.);
        node.height = Val::Px(size.y as f32 / scale + 4.);
        for c in children.iter() {
            if let Ok(mut text) = texts.get_mut(c) {
                let label = scene.description.components.get(&path).map(|c| c.label.clone()).unwrap_or_else(|| path.clone());
                let value = format!("Close-up · {label} · ×{}", (zoom * 10.).round() / 10.);
                if text.0 != value {
                    text.0 = value;
                }
            }
        }
    }
}

/// The part under the pointer in the 3D view (its component path).
#[derive(Resource, Default)]
pub(crate) struct PartHover(pub Option<String>);

pub(crate) fn part_over(over: Trigger<Pointer<Over>>, parts: Query<&Part>, scene: Res<SpatialScene>, mut hover: ResMut<PartHover>) {
    if let Some(p) = parts.get(over.target()).ok().and_then(|p| scene.spatial.parts.get(p.index)) {
        hover.0 = Some(p.component.clone());
    }
}
pub(crate) fn part_out(_: Trigger<Pointer<Out>>, mut hover: ResMut<PartHover>) {
    hover.0 = None;
}

/// A second run on the same clock: other parameter values, drawn faint
/// over this one (ghost) or as a copy beside it (split).
#[derive(Clone, Debug)]
pub struct CompanionView {
    pub label: String,
    pub ghost: bool,
    pub frame: Option<sim_inspect::SampleFrame>,
}

/// The copy of a part shown in the split view's right half.
#[derive(Component)]
pub(crate) struct CompanionPart(pub usize);
#[derive(Component)]
pub(crate) struct SplitCamera;
#[derive(Component)]
pub(crate) struct SplitLabel(pub bool);

/// Where the split copy lives: far enough away never to show in the main view.
pub(crate) fn split_offset(scene: &SpatialScene) -> Vec3 {
    Vec3::X * scene.bounds_of(None).1.max(0.05) * 40.
}

/// Ghost: faint outlines of the moving parts where the companion run has them.
pub(crate) fn draw_ghost(scene: Res<SpatialScene>, mut gizmos: Gizmos, mut labels: ResMut<crate::physics_view::Labels>) {
    let Some(c) = scene.companion.as_ref().filter(|c| c.ghost) else { return };
    let Some(a) = &scene.animation else { return };
    let color = Color::srgba(0.72, 0.58, 1.0, 0.8);
    let mut labelled = false;
    for (i, p) in scene.spatial.parts.iter().enumerate() {
        let moving = a.translations.iter().any(|t| t.part == p.id) || a.rotations.iter().any(|r| r.part == p.id);
        if !moving || scene.state.hidden.contains(&p.component) {
            continue;
        }
        let t = crate::animation::part_transform_at(&scene, i, c.frame.as_ref());
        match p.shape {
            SpatialShape::Box { size } => gizmos.cuboid(t.with_scale(Vec3::from_array(size)), color),
            SpatialShape::Cylinder { radius, length } => {
                let axis = t.rotation * Vec3::Y;
                for side in [-0.5, 0.5] {
                    gizmos.circle(Isometry3d::new(t.translation + axis * length * side, Quat::from_rotation_arc(Vec3::Z, axis)), radius, color);
                    gizmos.line(t.translation + axis * length * side, t.transform_point(Vec3::new(radius, length * side, 0.)), color);
                }
            }
            SpatialShape::Sphere { radius } => {
                gizmos.sphere(Isometry3d::from_translation(t.translation), radius, color);
            }
        }
        if !labelled && a.translations.iter().chain(std::iter::empty()).any(|x| x.part == p.id) {
            labels.0.push((t.translation, format!("ghost: {}", c.label), color));
            labelled = true;
        }
    }
}

/// Split: a copy of the assembly posed by the companion run, seen by a
/// second camera in the right half of the view, on the same orbit.
#[allow(clippy::type_complexity)]
pub(crate) fn split(
    mut commands: Commands,
    scene: Res<SpatialScene>,
    window: Single<&Window>,
    originals: Query<(&Part, &Mesh3d, &MeshMaterial3d<StandardMaterial>), Without<CompanionPart>>,
    mut copies: Query<(Entity, &CompanionPart, &mut Transform), Without<SplitCamera>>,
    main: Single<(&mut Camera, &Orbit, &Transform), (Without<SplitCamera>, Without<InsetCamera>, Without<CompanionPart>)>,
    mut split: Single<(&mut Camera, &mut Transform), (With<SplitCamera>, Without<Orbit>, Without<CompanionPart>)>,
    mut labels: Query<(&mut Node, &mut Visibility, &Children, &SplitLabel)>,
    mut texts: Query<&mut Text>,
) {
    let active = scene.companion.as_ref().filter(|c| !c.ghost);
    let (mut main_camera, _orbit, main_transform) = main.into_inner();
    let (camera, transform) = &mut *split;
    let Some(c) = active else {
        for (e, _, _) in &copies {
            commands.entity(e).despawn();
        }
        camera.is_active = false;
        for (_, mut v, _, _) in &mut labels {
            *v = Visibility::Hidden;
        }
        return;
    };
    let offset = split_offset(&scene);
    if copies.is_empty() {
        for (part, mesh, material) in &originals {
            commands.spawn((CompanionPart(part.index), Mesh3d(mesh.0.clone()), MeshMaterial3d(material.0.clone()), Transform::default(), SceneContent));
        }
    }
    for (_, copy, mut t) in &mut copies {
        let mut pose = crate::animation::part_transform_at(&scene, copy.0, c.frame.as_ref());
        pose.translation += offset;
        *t = pose;
    }
    // Halve the main view; the split camera takes the right half.
    let Some(v) = main_camera.viewport.clone() else { return };
    if v.physical_size.x < 80 {
        return;
    }
    let half = UVec2::new(v.physical_size.x / 2, v.physical_size.y);
    let left = Viewport { physical_position: v.physical_position, physical_size: half, ..default() };
    let right = Viewport { physical_position: v.physical_position + UVec2::new(half.x, 0), physical_size: half, ..default() };
    if main_camera.viewport.as_ref().map(|x| x.physical_size) != Some(half) {
        main_camera.viewport = Some(left.clone());
        main_camera.sub_camera_view = None;
    }
    camera.is_active = true;
    camera.viewport = Some(right.clone());
    **transform = Transform::from_translation(main_transform.translation + offset).with_rotation(main_transform.rotation);
    let scale = window.scale_factor();
    for (mut node, mut vis, children, right_side) in &mut labels {
        *vis = Visibility::Inherited;
        let x = if right_side.0 { right.physical_position.x } else { left.physical_position.x };
        node.left = Val::Px(x as f32 / scale + 10.);
        node.top = Val::Px(v.physical_position.y as f32 / scale + 36.);
        for ch in children.iter() {
            if let Ok(mut text) = texts.get_mut(ch) {
                let value = if right_side.0 { c.label.clone() } else { "This run".to_string() };
                if text.0 != value {
                    text.0 = value;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glides_ease_to_the_pose_the_short_way_and_stop_on_input() {
        let mut o = Orbit { focus: Vec3::ZERO, radius: 1., yaw: 3.0, pitch: 0.3, ..Default::default() };
        let to = Pose { focus: Vec3::X, radius: 4., yaw: -3.0, pitch: 0.5 };
        o.glide_to(to, 1.0);
        o.step(0.5);
        // Halfway in time is halfway in position (smoothstep), and the radius eases in log space.
        assert!((o.focus.x - 0.5).abs() < 1e-5 && (o.radius - 2.).abs() < 1e-4);
        // From 3.0 to −3.0 rad the short way round passes π, not zero.
        assert!(o.yaw > 3.0);
        o.step(0.6);
        assert!(o.glide.is_none() && (o.radius - 4.).abs() < 1e-5 && (o.focus.x - 1.).abs() < 1e-6);
        o.spin = 0.2;
        o.glide_to(Pose { radius: 1., ..o.pose() }, 1.);
        o.interrupt();
        assert!(o.glide.is_none() && o.spin == 0.);
        let before = o.pose();
        o.step(1.);
        assert_eq!(o.pose(), before);
        // The first framing is a cut.
        let mut fresh = Orbit::default();
        fresh.glide_to(to, 1.);
        assert_eq!(fresh.pose(), Pose { yaw: -3.0, ..to });
    }
}
