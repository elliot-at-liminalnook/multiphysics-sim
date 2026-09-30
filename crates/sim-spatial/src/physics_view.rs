//! Physics made visible, from the same measurements the plots use:
//! moving pieces inside housings (armatures, coupling halves, gear trains),
//! ropes to sliding loads, and switchable layers — power in and out of each
//! part with the system's balance, torque and force arrows, current dots
//! along wires, heat glow and heat flow, and trails of earlier positions.
//! Every quantity comes from the current sample frame (live or recorded);
//! nothing here integrates physics. Scales are relative to the largest value
//! on screen and are stated in the labels.
use super::*;
use crate::animation::part_transform;
use sim_inspect::animation::{FlowBinding, FlowDomain, InternalElement, scalar};
use sim_inspect::spatial::Overlay;
use std::collections::{HashMap, VecDeque};
use std::f32::consts::TAU;

#[derive(Component)]
pub(crate) struct InternalPiece(pub usize);
/// The split view's copy of an internal piece, posed by the companion run.
#[derive(Component)]
pub(crate) struct CompanionPiece;
#[derive(Component)]
pub(crate) struct Rope(pub usize);
#[derive(Component)]
pub(crate) struct PhysicsLabel;
#[derive(Component)]
pub(crate) struct OverlayBar;
#[derive(Component, Clone, Copy)]
pub(crate) struct OverlayToggle(pub Overlay);
/// View switches beside the layers.
#[derive(Component, Clone, Copy, PartialEq)]
pub(crate) enum ViewToggle {
    Xray,
    Explode,
    Strobe,
}

const DRIVE: Color = Color::srgb(0.98, 0.62, 0.22);
const ABSORB: Color = Color::srgb(0.30, 0.78, 0.95);
const CURRENT: Color = Color::srgb(1.0, 0.86, 0.30);
const HEAT: Color = Color::srgb(1.0, 0.42, 0.18);

fn base_rotation(axis: [f32; 3]) -> Quat {
    Quat::from_rotation_arc(Vec3::Y, Vec3::from_array(axis).normalize_or(Vec3::Y))
}

/// Meshes of the moving pieces bound in the animation description.
pub(crate) fn spawn_internals(commands: &mut Commands, scene: &SpatialScene, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    let Some(a) = &scene.animation else { return };
    for (i, b) in a.internals.iter().enumerate() {
        let Some(part) = scene.spatial.parts.iter().find(|p| p.id == b.part) else { continue };
        let [r, g, bl] = b.color_srgb;
        let metal = materials.add(StandardMaterial { base_color: Color::srgb(r, g, bl), metallic: 0.7, perceptual_roughness: 0.35, ..default() });
        let steel = materials.add(StandardMaterial { base_color: Color::srgb(0.46, 0.49, 0.53), metallic: 0.8, perceptual_roughness: 0.3, ..default() });
        let (rad, len) = (b.radius, b.length);
        let mut pieces: Vec<(Handle<Mesh>, Transform, Handle<StandardMaterial>)> = vec![];
        match b.element {
            InternalElement::Shaft => {
                pieces.push((meshes.add(Cylinder::new(rad, len)), Transform::IDENTITY, metal.clone()));
                pieces.push((meshes.add(Cuboid::new(rad * 0.5, len * 0.9, rad * 0.4)), Transform::from_xyz(rad * 0.85, 0., 0.), steel.clone()));
            }
            InternalElement::Armature { poles } => {
                pieces.push((meshes.add(Cylinder::new(rad * 0.45, len * 1.25)), Transform::IDENTITY, steel.clone()));
                let pole = meshes.add(Cuboid::new(rad * 0.62, len, rad * 0.62));
                for k in 0..poles.max(2) {
                    let a = TAU * k as f32 / poles.max(2) as f32;
                    let q = Quat::from_rotation_y(a);
                    pieces.push((pole.clone(), Transform::from_translation(q * Vec3::X * rad * 0.62).with_rotation(q), metal.clone()));
                }
            }
            InternalElement::CouplingHalf { lugs } => {
                // The disc sits away from the coupling's centre; its jaws reach in.
                let inward = (Vec3::from_array(part.position) - Vec3::from_array(b.center)).dot(Vec3::from_array(b.axis)).signum();
                pieces.push((meshes.add(Cylinder::new(rad, len * 0.5)), Transform::from_xyz(0., -inward * len * 0.25, 0.), metal.clone()));
                let lug = meshes.add(Cuboid::new(rad * 0.55, len * 0.5, rad * 0.5));
                for k in 0..lugs.max(1) {
                    let a = TAU * k as f32 / lugs.max(1) as f32;
                    let q = Quat::from_rotation_y(a);
                    pieces.push((lug.clone(), Transform::from_translation(q * Vec3::X * rad * 0.6 + Vec3::Y * inward * len * 0.25).with_rotation(q), metal.clone()));
                }
            }
            InternalElement::Gear { teeth } => {
                pieces.push((meshes.add(Cylinder::new(rad * 0.9, len)), Transform::IDENTITY, metal.clone()));
                let n = teeth.clamp(6, 90);
                let tooth = meshes.add(Cuboid::new(rad * 0.16, len, TAU * rad / n as f32 * 0.5));
                for k in 0..n {
                    let a = TAU * k as f32 / n as f32;
                    let q = Quat::from_rotation_y(a);
                    pieces.push((tooth.clone(), Transform::from_translation(q * Vec3::X * rad * 0.96).with_rotation(q), metal.clone()));
                }
                pieces.push((meshes.add(Cuboid::new(rad * 0.7, len * 1.05, rad * 0.12)), Transform::IDENTITY, steel.clone()));
            }
            InternalElement::Arm => {
                // Hangs straight down (world −Y, square to the joint axis) at angle zero.
                let axis = Vec3::from_array(b.axis).normalize_or(Vec3::Y);
                let down = (Vec3::NEG_Y - axis * Vec3::NEG_Y.dot(axis)).normalize_or(Vec3::X);
                let local = base_rotation(b.axis).inverse() * down;
                let rod = (len * 0.045).max(rad * 0.25);
                pieces.push((meshes.add(Cylinder::new(rod * 1.8, rod * 3.0)), Transform::IDENTITY, steel.clone()));
                pieces.push((meshes.add(Cuboid::new(rod * 2.0, len, rod * 1.2)), Transform::from_translation(local * len * 0.5).with_rotation(Quat::from_rotation_arc(Vec3::Y, local)), metal.clone()));
                pieces.push((meshes.add(Sphere::new(rad)), Transform::from_translation(local * len), metal.clone()));
            }
        }
        commands
            .spawn((InternalPiece(i), Transform::from_translation(Vec3::from_array(b.center)).with_rotation(base_rotation(b.axis)), Visibility::Inherited, SceneContent))
            .with_children(|p| {
                for (mesh, t, m) in &pieces {
                    p.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(m.clone()), *t));
                }
            });
        // Its copy for the split view (hidden unless a split companion is shown).
        commands
            .spawn((InternalPiece(i), CompanionPiece, Transform::default(), Visibility::Hidden, SceneContent))
            .with_children(|p| {
                for (mesh, t, m) in pieces {
                    p.spawn((Mesh3d(mesh), MeshMaterial3d(m), t));
                }
            });
    }
    let rope = meshes.add(Cylinder::new(1., 1.));
    let fibre = materials.add(StandardMaterial { base_color: Color::srgb(0.86, 0.80, 0.64), perceptual_roughness: 0.9, ..default() });
    for (i, _) in a.tethers.iter().enumerate() {
        commands.spawn((Rope(i), Mesh3d(rope.clone()), MeshMaterial3d(fibre.clone()), Transform::default(), Visibility::Inherited, SceneContent));
    }
}

/// Turn each piece by its port's angle; stretch each rope to its load.
pub(crate) fn update_internals(scene: Res<SpatialScene>, mut pieces: Query<(&InternalPiece, Option<&CompanionPiece>, &mut Transform, &mut Visibility), Without<Rope>>, mut ropes: Query<(&Rope, &mut Transform, &mut Visibility), Without<InternalPiece>>) {
    if !scene.is_changed() {
        return;
    }
    let Some(a) = &scene.animation else { return };
    let split = scene.companion.as_ref().filter(|c| !c.ghost);
    let split_offset = crate::view::split_offset(&scene);
    for (piece, companion, mut t, mut v) in &mut pieces {
        let Some(b) = a.internals.get(piece.0) else { continue };
        let part = scene.spatial.parts.iter().find(|p| p.id == b.part);
        let hidden = part.is_some_and(|p| scene.state.hidden.contains(&p.component));
        // The split copy follows the companion run, beside the original.
        let (frame, shift) = match (companion.is_some(), split) {
            (false, _) => (scene.frame(), Vec3::ZERO),
            (true, Some(c)) => (c.frame.as_ref(), split_offset),
            (true, None) => {
                *v = Visibility::Hidden;
                continue;
            }
        };
        *v = if hidden { Visibility::Hidden } else { Visibility::Inherited };
        let angle = scalar(frame, &b.observable).map(|s| (s.value * b.gain).rem_euclid(std::f64::consts::TAU) as f32).unwrap_or(0.);
        let mut offset = scene.spatial.parts.iter().position(|p| p.id == b.part).filter(|_| scene.explode_t > 0.).map(|i| crate::animation::explode_offset(&scene, i) * scene.explode_t).unwrap_or(Vec3::ZERO);
        // Pieces inside a sliding part (a motor riding a rover) slide with it.
        if let Some(slide) = a.translations.iter().find(|t| t.part == b.part) {
            if let Some(v) = scalar(frame, &slide.observable) {
                offset += Vec3::from_array(slide.axis) * (v.value - slide.reference_m) as f32 * scene.motion_scale;
            }
        }
        t.translation = Vec3::from_array(b.center) + offset + shift;
        t.rotation = Quat::from_axis_angle(Vec3::from_array(b.axis).normalize_or(Vec3::Y), angle) * base_rotation(b.axis);
    }
    let thickness = scene.bounds().1 * 0.004;
    for (rope, mut t, mut v) in &mut ropes {
        let Some(tether) = a.tethers.get(rope.0) else { continue };
        let Some(index) = scene.spatial.parts.iter().position(|p| p.id == tether.part) else { continue };
        let from = Vec3::from_array(tether.anchor);
        let to = part_transform(&scene, index).transform_point(Vec3::from_array(tether.attach));
        let d = to - from;
        *v = if d.length() < 1e-6 { Visibility::Hidden } else { Visibility::Inherited };
        *t = Transform::from_translation(from + d * 0.5).with_rotation(Quat::from_rotation_arc(Vec3::Y, d.normalize_or(Vec3::Y))).with_scale(Vec3::new(thickness, d.length(), thickness));
    }
}

/// Parts whose housing is drawn see-through to show the pieces inside.
pub(crate) fn has_internals(scene: &SpatialScene, part: &str) -> bool {
    scene.animation.as_ref().is_some_and(|a| a.internals.iter().any(|b| b.part == part))
}

fn value(frame: Option<&sim_inspect::SampleFrame>, id: Option<&String>) -> Option<f64> {
    scalar(frame, id?).map(|s| s.value)
}

/// Where to draw a port's torque: the turning piece or part on its net.
fn shaft_at(scene: &SpatialScene, f: &FlowBinding) -> Option<(Vec3, Vec3, f32)> {
    let a = scene.animation.as_ref()?;
    let net = &scene.description.nets.get(&f.net)?.ports;
    let on_net = |observable: &str| match &scene.description.observables.get(observable)?.location {
        sim_inspect::ObservationLocation::Across { port, .. } => Some(net.contains(port)),
        _ => None,
    };
    if let Some(b) = a.internals.iter().find(|b| scene.spatial.parts.iter().any(|p| p.id == b.part && p.component == f.component) && on_net(&b.observable) == Some(true)) {
        return Some((Vec3::from_array(b.center), Vec3::from_array(b.axis), b.radius));
    }
    let r = a.rotations.iter().find(|r| on_net(&r.observable) == Some(true))?;
    let index = scene.spatial.parts.iter().position(|p| p.id == r.part)?;
    let t = part_transform(scene, index);
    let radius = match scene.spatial.parts[index].shape {
        SpatialShape::Cylinder { radius, .. } => radius,
        SpatialShape::Box { size } => size[0].max(size[2]) * 0.5,
        SpatialShape::Sphere { radius } => radius,
    };
    Some((t.translation, Vec3::from_array(r.axis), radius))
}

fn label_of(scene: &SpatialScene, component: &str) -> String {
    scene.description.components.get(component).map(|c| c.label.clone()).unwrap_or_else(|| component.to_string())
}

/// Reference components (grounds) whose flow is a balance, not a quantity to read.
fn is_reference(scene: &SpatialScene, component: &str) -> bool {
    scene.description.components.get(component).is_some_and(|c| c.component_type.ends_with(".ground"))
}

/// Three significant figures, switching prefix only far from 1 (below 0.01
/// or from 1000), so a label's width stays steady while its value changes.
fn si(v: f64, unit: &str) -> String {
    let a = v.abs();
    let (scaled, prefix) = if a >= 1e3 { (v / 1e3, "k") } else if a >= 0.01 || a == 0. { (v, "") } else if a >= 1e-5 { (v * 1e3, "m") } else { (v * 1e6, "µ") };
    let s = scaled.abs();
    let decimals = if s == 0. { 1 } else { (2 - s.log10().floor() as i32).clamp(0, 4) as usize };
    format!("{scaled:.decimals$} {prefix}{unit}")
}

/// A world point and its text, placed on screen by [`labels`].
#[derive(Default, Resource)]
pub(crate) struct Labels(pub Vec<(Vec3, String, Color)>);

#[derive(Default)]
pub(crate) struct Motion {
    /// Per flow binding: dot phase along its wire (fraction of a spacing).
    phase: HashMap<usize, f32>,
    last_time: Option<f64>,
    /// Per part: (sim time, transform) of earlier positions.
    trails: HashMap<usize, VecDeque<(f64, Transform)>>,
    last_trail: f64,
}

/// Draw the enabled layers with gizmos and queue their labels.
pub(crate) fn draw(scene: Res<SpatialScene>, time: Res<Time>, mut gizmos: Gizmos, mut labels: ResMut<Labels>, mut motion: Local<Motion>) {
    labels.0.clear();
    let Some(a) = &scene.animation else { return };
    let frame = scene.frame();
    let on = |o: Overlay| scene.state.overlays.contains(&o);
    let (_, extent) = scene.bounds();
    let reach = extent.min(0.25);
    let sim_time = frame.map(|f| f.time);
    let advanced = match (sim_time, motion.last_time) {
        (Some(t), Some(l)) => t > l,
        _ => false,
    };
    if let (Some(t), Some(l)) = (sim_time, motion.last_time) {
        if t < l {
            motion.trails.clear();
            motion.last_trail = f64::NEG_INFINITY;
        }
    }
    motion.last_time = sim_time;
    let hidden = |component: &str| scene.state.hidden.contains(component);
    let part_index = |id: &str| scene.spatial.parts.iter().position(|p| p.id == id);

    if on(Overlay::Values) {
        // One readout per moving or measured part: what the eye should read off it.
        let at_port = |observable: &str, quantity: &str| -> Option<(String, f64)> {
            let port = match &scene.description.observables.get(observable)?.location {
                sim_inspect::ObservationLocation::Across { port, .. } | sim_inspect::ObservationLocation::Through { port, .. } => port.clone(),
                _ => return None,
            };
            scene.description.observables.iter().find(|(_, o)| o.quantity.name == quantity && matches!(&o.location, sim_inspect::ObservationLocation::Across { port: p, .. } if *p == port)).and_then(|(id, _)| Some((id.clone(), scalar(frame, id)?.value)))
        };
        for (i, p) in scene.spatial.parts.iter().enumerate() {
            if hidden(&p.component) {
                continue;
            }
            let mut text = vec![];
            if let Some(r) = a.rotations.iter().find(|r| r.part == p.id) {
                if let Some((_, w)) = at_port(&r.observable, "sim.quantity.AngularVelocity") {
                    text.push(format!("{} rad/s ({:.0} rpm)", si(w, "").trim(), w * 60. / std::f64::consts::TAU));
                }
            }
            if let Some(t) = a.translations.iter().find(|t| t.part == p.id) {
                if let Some(x) = scalar(frame, &t.observable) {
                    text.push(format!("{} from start", si(x.value - t.reference_m, "m")));
                }
            }
            if let Some(c) = a.colors.iter().find(|c| c.part == p.id) {
                if let Some(k) = scalar(frame, &c.observable) {
                    text.push(format!("{:.1} °C", k.value - 273.15));
                }
            }
            // A ground's "current" is the node's balance, zero by construction.
            if let Some(f) = a.flows.iter().find(|f| f.part == p.id && f.domain == FlowDomain::Electrical && !is_reference(&scene, &f.component)) {
                if let Some(iv) = value(frame, f.flow.as_ref()) {
                    text.push(si(iv.abs(), "A"));
                }
            }
            if !text.is_empty() {
                let top = part_transform(&scene, i).translation + Vec3::Y * reach * 0.05;
                labels.0.push((top, format!("{} · {}", p.label, text.join(" · ")), crate::ui_kit::TEXT));
            }
        }
    }

    if on(Overlay::Power) {
        // Power into each part: + absorbs (loads, heat), − delivers (sources).
        let mut by: BTreeMap<&str, (f64, &str)> = BTreeMap::new();
        for f in &a.flows {
            if let Some(p) = f.power(frame) {
                let e = by.entry(f.component.as_str()).or_insert((0., f.part.as_str()));
                e.0 += p;
            }
        }
        let max = by.values().map(|(p, _)| p.abs()).fold(0., f64::max);
        let balance: f64 = by.values().map(|(p, _)| p).sum();
        if max > 1e-9 {
            for (component, (p, part)) in &by {
                if p.abs() < 0.02 * max || hidden(component) {
                    continue;
                }
                let Some(i) = part_index(part) else { continue };
                let base = part_transform(&scene, i).translation + Vec3::Y * reach * 0.12;
                let h = (p.abs() / max) as f32 * reach * 0.35;
                let color = if *p < 0. { DRIVE } else { ABSORB };
                for k in 0..4 {
                    let o = Vec3::new(k as f32 * reach * 0.004, 0., 0.);
                    gizmos.line(base + o, base + o + Vec3::Y * h, color);
                }
                labels.0.push((base + Vec3::Y * h, format!("{} {} {}", label_of(&scene, component), if *p < 0. { "gives" } else { "takes" }, si(p.abs(), "W")), color));
            }
            let (center, _) = scene.bounds();
            labels.0.push((center + Vec3::Y * extent * 0.9, format!("Sum over all parts {} · what one part gives, the others take", si(balance, "W")), Color::WHITE));
        }
    }

    if on(Overlay::Forces) {
        // Torque each part applies to its shaft; force each applies to a slide.
        let torques: Vec<(&FlowBinding, f64)> = a.flows.iter().filter(|f| f.domain == FlowDomain::Rotational && !hidden(&f.component)).filter_map(|f| Some((f, value(frame, f.flow.as_ref())?))).filter(|(f, _)| {
            let kind = scene.description.components.get(&f.component).map(|c| c.component_type.as_str()).unwrap_or("");
            kind != "rotational.inertia" && !kind.ends_with(".ground")
        }).collect();
        let max = torques.iter().map(|(_, t)| t.abs()).fold(0., f64::max);
        let mut slot: HashMap<String, usize> = HashMap::new();
        for (f, tau) in &torques {
            if max < 1e-12 || tau.abs() < 0.03 * max {
                continue;
            }
            let Some((center, axis, radius)) = shaft_at(&scene, f) else { continue };
            let k = slot.entry(f.net.clone()).or_default();
            let ring = radius.max(reach * 0.02) * (1.5 + 0.45 * *k as f32);
            *k += 1;
            let axis = axis.normalize_or(Vec3::Y);
            let u = axis.any_orthonormal_vector();
            let w = axis.cross(u);
            // Applied by the part: opposite to the torque flowing into it.
            let dir = -(tau.signum() as f32);
            let sweep = (tau.abs() / max) as f32 * 4.4 + 0.4;
            let point = |s: f32| center + (u * s.cos() + w * s.sin()) * ring;
            let n = 28;
            let pts: Vec<Vec3> = (0..=n).map(|i| point(dir * sweep * i as f32 / n as f32)).collect();
            let speed = scalar(frame, &scene.description.observables.keys().find(|id| matches!(&scene.description.observables[*id].location, sim_inspect::ObservationLocation::Across { port, lane } if lane == "speed" && scene.description.nets[&f.net].ports.contains(port))).cloned().unwrap_or_default()).map(|s| s.value).unwrap_or(0.);
            let drives = -tau * speed > 0.;
            let color = if drives { DRIVE } else { ABSORB };
            gizmos.linestrip(pts.iter().copied(), color);
            let end = *pts.last().unwrap();
            let tangent = (pts[n] - pts[n - 1]).normalize_or(u);
            let side = axis.cross(tangent) * ring * 0.18;
            gizmos.line(end, end - tangent * ring * 0.25 + side, color);
            gizmos.line(end, end - tangent * ring * 0.25 - side, color);
            labels.0.push((end, format!("{} {}", label_of(&scene, &f.component), si(tau.abs(), "N·m")), color));
        }
        let forces: Vec<(&FlowBinding, f64)> = a.flows.iter().filter(|f| f.domain == FlowDomain::Translational && !hidden(&f.component)).filter_map(|f| Some((f, value(frame, f.flow.as_ref())?))).collect();
        let max = forces.iter().map(|(_, v)| v.abs()).fold(0., f64::max);
        let mut slot: HashMap<String, usize> = HashMap::new();
        for (f, force) in &forces {
            if max < 1e-12 || force.abs() < 0.03 * max {
                continue;
            }
            let kind = scene.description.components.get(&f.component).map(|c| c.component_type.as_str()).unwrap_or("");
            if kind == "translational.mass" {
                continue;
            }
            // Drawn on the sliding body the force acts on.
            let Some(t) = a.translations.iter().find(|t| scene.description.observables.get(&t.observable).is_some_and(|o| matches!(&o.location, sim_inspect::ObservationLocation::Across { port, .. } if scene.description.nets[&f.net].ports.contains(port)))).filter(|t| scene.spatial.parts.iter().any(|p| p.id == t.part && scene.description.components.get(&p.component).is_some_and(|c| c.component_type == "translational.mass"))).or_else(|| a.translations.iter().find(|t| scene.description.observables.get(&t.observable).is_some_and(|o| matches!(&o.location, sim_inspect::ObservationLocation::Across { port, .. } if scene.description.nets[&f.net].ports.contains(port))))) else { continue };
            let Some(i) = part_index(&t.part) else { continue };
            let k = slot.entry(f.net.clone()).or_default();
            let axis = Vec3::from_array(t.axis).normalize_or(Vec3::Y);
            let side = axis.any_orthonormal_vector() * reach * 0.05 * (*k as f32 - 1.);
            *k += 1;
            let from = part_transform(&scene, i).translation + side;
            let length = (force.abs() / max) as f32 * reach * 0.35;
            let dir = -(force.signum() as f32) * axis;
            gizmos.arrow(from, from + dir * length, DRIVE);
            labels.0.push((from + dir * length, format!("{} {}", label_of(&scene, &f.component), si(force.abs(), "N")), DRIVE));
        }
    }

    // Moving dots along wires (current) and chevrons along thermal links (heat).
    let dt = time.delta_secs().min(0.1);
    for (layer, domain, color) in [(Overlay::Current, FlowDomain::Electrical, CURRENT), (Overlay::Heat, FlowDomain::Thermal, HEAT)] {
        if !on(layer) {
            continue;
        }
        let flows: Vec<(usize, &FlowBinding, f64)> = a.flows.iter().enumerate().filter(|(_, f)| f.domain == domain).filter_map(|(i, f)| Some((i, f, value(frame, f.flow.as_ref())?))).collect();
        let max = flows.iter().map(|(_, _, v)| v.abs()).fold(0., f64::max);
        if max < 1e-12 {
            continue;
        }
        // One label per part: a part's current is the same on each of its wires.
        let mut labelled = std::collections::BTreeSet::new();
        for (i, f, v) in &flows {
            let Some((hub, ends)) = crate::linked::net_geometry(&scene, &f.net) else { continue };
            let net = &scene.description.nets[&f.net];
            let Some(k) = net.ports.iter().position(|p| scene.description.ports.get(p).is_some_and(|q| q.component == f.component && q.name == f.port)) else { continue };
            let end = ends[k];
            let rate = (*v / max) as f32;
            let phase = motion.phase.entry(*i).or_default();
            if advanced {
                *phase = (*phase + rate * dt * 1.6).rem_euclid(1.);
            }
            let length = end.distance(hub);
            let spacing = (reach * 0.06).max(1e-4);
            let count = (length / spacing).floor() as usize;
            // Positive flow is into the part: from the hub toward it.
            for d in 0..count {
                let s = (d as f32 + *phase) * spacing / length.max(1e-9);
                if s > 1. || rate.abs() < 0.02 {
                    continue;
                }
                let p = hub.lerp(end, s);
                let size = reach * 0.006 * (0.6 + rate.abs());
                if domain == FlowDomain::Thermal {
                    let dir = (end - hub).normalize_or(Vec3::X) * rate.signum();
                    let side = dir.any_orthonormal_vector() * size * 1.6;
                    gizmos.line(p, p - dir * size * 2. + side, color);
                    gizmos.line(p, p - dir * size * 2. - side, color);
                } else {
                    gizmos.sphere(Isometry3d::from_translation(p), size, color);
                }
            }
            // With Values on, a part's current is already in its readout.
            let shown = domain == FlowDomain::Electrical && on(Overlay::Values);
            // Labelled from the first few percent, so a rising current is named as it rises.
            if rate.abs() > 0.05 && !shown && labelled.insert(f.component.as_str()) {
                labels.0.push((hub.lerp(end, 0.5), format!("{} {}", label_of(&scene, &f.component), si(*v, if domain == FlowDomain::Thermal { "W" } else { "A" })), color));
            }
        }
    }

    if on(Overlay::Trails) {
        // Every 0.12 s of wall time while the simulation moves, keep a pose.
        let now = time.elapsed_secs_f64();
        let moving: Vec<usize> = scene.spatial.parts.iter().enumerate().filter(|(_, p)| a.translations.iter().any(|t| t.part == p.id) || a.rotations.iter().any(|r| r.part == p.id && r.marker_radius.is_some())).map(|(i, _)| i).collect();
        if advanced && now - motion.last_trail > 0.12 {
            motion.last_trail = now;
            for &i in &moving {
                let t = part_transform(&scene, i);
                let trail = motion.trails.entry(i).or_default();
                trail.push_back((sim_time.unwrap_or(0.), t));
                while trail.len() > 12 {
                    trail.pop_front();
                }
            }
        }
        for &i in &moving {
            let Some(trail) = motion.trails.get(&i) else { continue };
            let p = &scene.spatial.parts[i];
            if hidden(&p.component) {
                continue;
            }
            let n = trail.len();
            for (k, (_, t)) in trail.iter().enumerate() {
                let alpha = 0.08 + 0.5 * (k + 1) as f32 / n.max(1) as f32;
                let c = Color::srgba(0.85, 0.90, 1.0, alpha);
                match p.shape {
                    SpatialShape::Box { size } => gizmos.cube(t.with_scale(Vec3::from_array(size)), c),
                    SpatialShape::Cylinder { radius, length } => {
                        let axis = t.rotation * Vec3::Y;
                        let iso = Isometry3d::new(t.translation + axis * length * 0.5, Quat::from_rotation_arc(Vec3::Z, axis));
                        gizmos.circle(iso, radius, c);
                        gizmos.line(t.translation + axis * length * 0.5, t.transform_point(Vec3::new(radius, length * 0.5, 0.)), c);
                    }
                    SpatialShape::Sphere { radius } => {
                        gizmos.sphere(Isometry3d::from_translation(t.translation), radius, c);
                    }
                }
            }
        }
    } else {
        motion.trails.clear();
    }
}

/// Place the queued labels over the 3D view.
/// What a label showed last: kept so labels stay still while values and
/// parts move (text refreshes a few times a second; position eases; the
/// chosen slot sticks while it still fits).
#[derive(Default)]
pub(crate) struct Steady {
    by_key: HashMap<String, SteadyLabel>,
    /// The simulation time shown last frame: a seek or a pause shows the
    /// exact value at once; only continuous playback is throttled.
    sim_time: Option<f64>,
}
struct SteadyLabel {
    text: String,
    changed: f64,
    at: Vec2,
    dy: Option<f32>,
    seen: f64,
}
/// A label's identity: its text up to the first number (the part's name).
fn label_key(text: &str) -> String {
    let cut = text.char_indices().find(|(i, c)| (c.is_ascii_digit() || *c == '−' || *c == '-') && text[*i..].chars().any(|d| d.is_ascii_digit())).map_or(text.len(), |(i, _)| i);
    let key = text[..cut].trim();
    if key.is_empty() { text.to_string() } else { key.to_string() }
}
const TEXT_REFRESH_S: f64 = 0.25;
const EASE_S: f32 = 0.18;
const SNAP_PX: f32 = 90.;

pub(crate) fn labels(mut commands: Commands, labels: Res<Labels>, scene: Res<SpatialScene>, time: Res<Time>, window: Single<&Window>, camera: Single<(&Camera, &GlobalTransform), With<Orbit>>, inset: Query<&Camera, (With<crate::view::InsetCamera>, Without<Orbit>)>, fonts: Option<Res<crate::ui_kit::UiFonts>>, existing: Query<Entity, With<PhysicsLabel>>, mut steady: Local<Steady>) {
    for e in &existing {
        commands.entity(e).despawn();
    }
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs();
    steady.by_key.retain(|_, l| now - l.seen < 1.0);
    let sim_time = scene.live.snapshot.as_ref().and_then(|s| s.frame.as_ref()).map(|f| f.time);
    let playing = matches!((steady.sim_time, sim_time), (Some(a), Some(b)) if b > a && b - a < 0.25);
    steady.sim_time = sim_time;
    let (cam, gt) = *camera;
    let scale = window.scale_factor();
    let Some(viewport) = cam.viewport.as_ref() else { return };
    if viewport.physical_size.x < 40 || viewport.physical_size.y < 40 {
        return;
    }
    let origin = viewport.physical_position.as_vec2() / scale;
    let size = viewport.physical_size.as_vec2() / scale;
    let Some(fonts) = fonts else { return };
    let k = crate::ui_kit::Kit::new(&fonts);
    // Keep clear of the hint line (and the builder's layer chips) along the top.
    let mut placed: Vec<Rect> = vec![Rect::new(0., 0., size.x, if scene.learn_view.is_some() { 22. } else { 60. })];
    // ...and the picture-in-picture close-up.
    for c in inset.iter().filter(|c| c.is_active) {
        if let Some(v) = &c.viewport {
            let min = v.physical_position.as_vec2() / scale - origin;
            placed.push(Rect::from_corners(min - 4., min + v.physical_size.as_vec2() / scale + 4.));
        }
    }
    let mut shown = std::collections::BTreeSet::new();
    // Same text twice (a coupling's torque on both of its ends) reads once.
    for (p, text, color) in labels.0.iter().filter(|(_, t, _)| shown.insert(t.as_str())).take(24) {
        // Through NDC: correct also when a lesson card shows only part of the view.
        let Some(ndc) = cam.world_to_ndc(gt, *p).filter(|n| n.z > 0. && n.z < 1.) else { continue };
        let target = Vec2::new((ndc.x + 1.) * 0.5 * size.x, (1. - ndc.y) * 0.5 * size.y);
        // Steady: the text changes a few times a second, the anchor eases
        // towards the part (snapping only after a real camera move).
        // Keyed by name and colour: an emphasis arrow's name is not the
        // force label that shares it.
        let key = format!("{}|{:?}", label_key(text), color.to_srgba());
        let entry = steady.by_key.entry(key).or_insert_with(|| SteadyLabel { text: text.clone(), changed: now, at: target, dy: None, seen: now });
        if entry.text != *text && (!playing || now - entry.changed >= TEXT_REFRESH_S) {
            entry.text = text.clone();
            entry.changed = now;
        }
        if entry.at.distance(target) > SNAP_PX || scene.reduced_motion {
            entry.at = target;
        } else {
            entry.at += (target - entry.at) * (dt / EASE_S).min(1.);
        }
        entry.seen = now;
        let (at, text) = (entry.at, entry.text.clone());
        if at.x < 0. || at.y < 0. || at.x > size.x - 20. || at.y > size.y - 10. {
            continue;
        }
        // Width from the name plus a fixed allowance for the value, so a
        // changing number never moves the box.
        let chars = label_key(&text).chars().count() + if label_key(&text).len() < text.len() { 12 } else { 0 };
        let box_size = Vec2::new(chars as f32 * 6.3 + 12., 17.);
        let rect = |dy: f32| {
            // Right of the anchor, pulled back inside the view's right edge.
            let min = at + Vec2::new(6. - (at.x + 6. + box_size.x - size.x).max(0.), dy - 9.);
            Rect::from_corners(min, min + box_size)
        };
        let fits = |r: &Rect| r.min.y >= 0. && r.max.y <= size.y && placed.iter().all(|p: &Rect| p.intersect(*r).is_empty());
        // The slot it had last time, while it still fits; else the nearest free one.
        let dy = entry.dy.filter(|dy| fits(&rect(*dy))).or_else(|| [0., 18., -18., 36., -36., 54., -54.].into_iter().find(|dy| fits(&rect(*dy))));
        let Some(dy) = dy else { continue };
        entry.dy = Some(dy);
        let r = rect(dy);
        placed.push(r);
        // A label floats at its anchor over the 3D view, the scene showing through.
        commands.spawn((
            PhysicsLabel,
            Node { border_radius: BorderRadius::all(Val::Px(3.)), position_type: PositionType::Absolute, left: Val::Px(origin.x + r.min.x), top: Val::Px(origin.y + r.min.y), padding: UiRect::axes(Val::Px(5.), Val::Px(1.)), ..default() },
            BackgroundColor(crate::view::BACKDROP.with_alpha(0.72)),
            GlobalZIndex(24),
            Pickable::IGNORE,
            children![(k.text(text, 11., *color, 2), Pickable::IGNORE)],
        ));
    }
}

/// A node whose children are the layer switches. The builder floats one over
/// the top-right of the 3D view; a lesson card puts one under its controls so
/// the switches never cover the scene.
#[derive(Component)]
pub(crate) struct LayerChips;

/// Keep every [`LayerChips`] slot filled with switches that match the view state.
pub(crate) fn overlay_bar(mut commands: Commands, scene: Res<SpatialScene>, camera: Single<&Camera, With<Orbit>>, window: Single<&Window>, fonts: Option<Res<crate::ui_kit::UiFonts>>, floating: Query<Entity, With<OverlayBar>>, slots: Query<Entity, With<LayerChips>>, added: Query<(), Added<LayerChips>>, mut last: Local<String>) {
    // The fonts are loaded while the app is built (`app::CorePlugin`).
    let Some(fonts) = fonts else { return };
    let scale = window.scale_factor();
    // The floating bar is for the builder only; lesson cards carry their own slot.
    let place = camera.viewport.as_ref().filter(|v| v.physical_size.x >= 320 && v.physical_size.y >= 120 && scene.animation.is_some() && scene.learn_view.is_none()).map(|v| {
        let right = (v.physical_position.x + v.physical_size.x) as f32 / scale;
        (right, v.physical_position.y as f32 / scale)
    });
    let signature = format!("{place:?}{:?}{}{}{}", scene.state.overlays, scene.state.xray, scene.state.exploded, scene.state.strobe);
    if *last == signature && added.is_empty() {
        return;
    }
    let moved = !last.starts_with(&format!("{place:?}"));
    *last = signature;
    if moved {
        for e in &floating {
            commands.entity(e).despawn();
        }
        if let Some((right, top)) = place {
            // Floats over the top-right of the 3D view (layout only; the chips are kit chips).
            commands.spawn((OverlayBar, LayerChips, Node { position_type: PositionType::Absolute, left: Val::Px(right - 560.), top: Val::Px(top + 30.), width: Val::Px(552.), justify_content: JustifyContent::FlexEnd, flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(4.), row_gap: Val::Px(4.), ..default() }, GlobalZIndex(25)));
            // Filled on the next frame, when the new slot counts as added.
            return;
        }
    }
    let k = crate::ui_kit::Kit::new(&fonts);
    for slot in &slots {
        commands.entity(slot).despawn_related::<Children>().with_children(|bar| {
            for (toggle, label, on) in [(ViewToggle::Xray, "X-ray", scene.state.xray), (ViewToggle::Explode, "Explode", scene.state.exploded), (ViewToggle::Strobe, "Strobe", scene.state.strobe)] {
                bar.spawn(k.chip(label, toggle, on, true));
            }
            for layer in Overlay::ALL {
                bar.spawn(k.chip(layer.label(), OverlayToggle(layer), scene.state.overlays.contains(&layer), true));
            }
        });
    }
}

/// Input: the overlay bar's toggles, as the view's actions (the display state
/// only, no refit): each press carries the flip of the value shown now, which
/// `inspect::apply` sets later in the same frame. Quiet: a refused toggle is
/// ignored, as before, not logged.
pub(crate) fn overlay_clicks(toggles: Query<(&Interaction, &OverlayToggle), Changed<Interaction>>, views: Query<(&Interaction, &ViewToggle), Changed<Interaction>>, scene: Res<SpatialScene>, mut out: MessageWriter<crate::app::actions::Act<crate::inspect::InspectAction>>) {
    use crate::app::actions::Act;
    use crate::inspect::InspectAction;
    use sim_inspect::spatial::SpatialCommand as C;
    for (interaction, toggle) in &views {
        if *interaction == Interaction::Pressed {
            let command = match toggle {
                ViewToggle::Xray => C::SetXray { enabled: !scene.state.xray },
                ViewToggle::Explode => C::SetExploded { enabled: !scene.state.exploded },
                ViewToggle::Strobe => C::SetStrobe { enabled: !scene.state.strobe },
            };
            out.write(Act::quiet(InspectAction::View(command)));
        }
    }
    for (interaction, toggle) in &toggles {
        if *interaction == Interaction::Pressed {
            let enabled = !scene.state.overlays.contains(&toggle.0);
            out.write(Act::quiet(InspectAction::View(C::SetOverlay { layer: toggle.0, enabled })));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_inspect::{SampleFrame, SampleValue};
    use sim_runtime::system_builder;

    /// The winch as the builder shows it, with a hand-made sample frame.
    fn winch(values: &[(&str, f64)]) -> SpatialScene {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
        let doc = sim_runtime::lesson::load_system(&root.join("examples/systems-builder/worm-drive/winch.system.json"), &registry).unwrap();
        let compiled = system_builder::compile(&doc, &registry, system_builder::config_for(&doc)).unwrap();
        let mut scene = SpatialScene::for_builder(compiled.description.clone(), compiled.spatial.clone().unwrap()).unwrap();
        scene.animation = compiled.animation.clone();
        let key = |k: &str| sim_runtime::lesson::resolve_observable(&compiled.description, k).unwrap();
        let frame = SampleFrame {
            version: sim_inspect::SAMPLE_FRAME_VERSION,
            description_id: scene.description.id.clone(),
            model_revision: scene.description.model_revision,
            run_id: "test".into(),
            generation: 0,
            sequence: 1,
            step: 1,
            time: 0.5,
            values: values.iter().map(|(k, v)| (key(k), SampleValue::Committed { value: *v, sample_time: 0.5 })).collect(),
        };
        scene.live.snapshot = Some(std::sync::Arc::new(sim_inspect::live::LiveSnapshot { version: 1, source_description_id: scene.description.id.clone(), description: None, status: None, frame: Some(frame), error: None }));
        scene
    }

    #[test]
    fn internal_pieces_turn_with_their_ports_and_the_rope_follows_the_load() {
        let scene = winch(&[("motor.shaft.angle", 1.0), ("gearbox/coupling.a.angle", 1.0), ("gearbox/coupling.b.angle", 0.7), ("load.axis.position", 0.2)]);
        let a = scene.animation.clone().unwrap();
        let mut app = App::new();
        app.insert_resource(scene);
        app.add_systems(Update, update_internals);
        let mut spawned = vec![];
        for (i, b) in a.internals.iter().enumerate() {
            spawned.push((app.world_mut().spawn((InternalPiece(i), Transform::default(), Visibility::Inherited)).id(), b.clone()));
        }
        let rope = app.world_mut().spawn((Rope(0), Transform::default(), Visibility::Inherited)).id();
        app.update();
        for (e, b) in &spawned {
            let t = app.world().get::<Transform>(*e).unwrap();
            let axis = Vec3::from_array(b.axis).normalize();
            // Relative to its rest orientation, each piece turned by its own port's angle about its axis.
            let turned = t.rotation * base_rotation(b.axis).inverse();
            let (turn_axis, angle) = turned.to_axis_angle();
            let expected = if b.observable.contains(&"gearbox/coupling".to_string()) && b.center[0] > -0.016 { 0.7 } else { 1.0 };
            let signed = angle * turn_axis.dot(axis).signum();
            assert!((signed - expected).abs() < 1e-4, "{} turned {signed} rad, expected {expected}", b.part);
            assert!(t.translation.abs_diff_eq(Vec3::from_array(b.center), 1e-6));
        }
        // The rope runs from the drum's rim to the top of the risen load.
        let s = app.world().resource::<SpatialScene>();
        let load = s.spatial.parts.iter().position(|p| p.id == "part/load").unwrap();
        let top = part_transform(s, load).transform_point(Vec3::Y * 0.008);
        assert!((part_transform(s, load).translation.y - (s.spatial.parts[load].position[1] + 0.2)).abs() < 1e-6, "the load rose 0.2 m");
        let t = app.world().get::<Transform>(rope).unwrap();
        let from = Vec3::from_array(a.tethers[0].anchor);
        assert!((t.scale.y - from.distance(top)).abs() < 1e-5);
        assert!(t.translation.abs_diff_eq((from + top) * 0.5, 1e-5));
        // Housings with pieces inside are the ones drawn see-through.
        assert!(has_internals(s, "part/motor") && has_internals(s, "part/gearbox/coupling") && !has_internals(s, "part/drum"));
    }
}
