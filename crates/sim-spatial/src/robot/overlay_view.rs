//! Drawn overlays: the stress paint and label, the graph dock, and the
//! gizmo overlays (floor grid, centre of mass, run-thread contacts, joint
//! frames and deflections).
use super::*;

/// The stress overlay's paint: when the overlay revision changed (toggle, new
/// results, new meshes), each link mesh gets per-vertex colours through the
/// shared rule (`robot_stress`, from already-parsed results; bounded by
/// vertices × ≤200 hotspot cells per link, timed in `paint_seconds`) or
/// loses them. Links without cells take the normal link colour.
pub(super) fn stress_paint(mut view: ResMut<RobotView>, links: Query<(&LinkMesh, &Mesh3d)>, mut meshes: ResMut<Assets<Mesh>>, mut redraw: MessageWriter<bevy::window::RequestRedraw>) {
    view.stress.take();
    if view.stress.busy() {
        // Keep polling the results-only read in the reactive window.
        redraw.write(bevy::window::RequestRedraw);
    }
    if view.stress.painted.is_some_and(|(r, _)| r == view.stress.revision) {
        return;
    }
    let started = std::time::Instant::now();
    let paint = view.stress.painting();
    let plain = LINK_COLOUR.to_linear().to_f32_array();
    for (link, mesh) in &links {
        let Some(mut mesh) = meshes.get_mut(&mesh.0) else { continue };
        if !paint {
            mesh.remove_attribute(Mesh::ATTRIBUTE_COLOR);
            continue;
        }
        let Some(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|a| a.as_float3()).map(<[[f32; 3]]>::to_vec) else { continue };
        let colours = match (view.stress.results.as_ref(), view.model.as_ref()) {
            (Some(r), Some(m)) => r.colours(m, link.0, &positions),
            _ => None,
        };
        let colours = colours.unwrap_or_else(|| vec![plain; positions.len()]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    }
    let revision = view.stress.revision;
    view.stress.painted = Some((revision, started.elapsed().as_secs_f64()));
}

/// A stress peak for the label: three significant figures in Pa, kPa or MPa,
/// chosen after rounding so 999.96 kPa reads "1.00 MPa". Display only; the
/// stored `peak_stress_pa` is never rewritten. Rounding keeps the label steady.
pub(super) fn stress_label(pa: f64) -> String {
    if !pa.is_finite() {
        return "—".into();
    }
    if pa == 0.0 {
        return "0 Pa".into();
    }
    let sig3 = |v: f64| match v.abs().log10().floor() as i32 - 2 {
        k if k >= 0 => (v / 10f64.powi(k)).round() * 10f64.powi(k),
        k => (v * 10f64.powi(-k)).round() / 10f64.powi(-k),
    };
    let r = sig3(pa);
    let (v, unit) = match r.abs() {
        a if a >= 1e6 => (r / 1e6, "MPa"),
        a if a >= 1e3 => (r / 1e3, "kPa"),
        _ => (r, "Pa"),
    };
    let decimals = (2 - v.abs().log10().floor() as i32).max(0) as usize;
    format!("{v:.decimals$} {unit}")
}

/// The stress label under the overlay buttons (`--robot FILE`): path, mtime,
/// status against the loaded model, peak per link and the colour scale.
pub(super) fn stress_panel(view: Res<RobotView>, mut line: Single<&mut Text, With<StressText>>) {
    let t = match (&view.source, view.model.as_ref()) {
        (Some(_), None) if view.planar.is_some() => "Stress: needs a v3 physical export (`sim-cad run` writes a .simresult.json only for v3 files)".to_string(),
        (None, _) if view.run.is_some() => "Stress: not available for presets (no .simresult.json)".to_string(),
        (None, _) | (_, None) => String::new(),
        (Some(_), Some(m)) if view.stress.enabled => match view.stress.results.as_ref() {
            None => "Stress: reading results…".into(),
            Some(r) => {
                let path = r.path.display();
                match &r.contents {
                    stress::Contents::Missing => format!("Stress: no results file ({path}); run `sim-cad run <model>` to write one"),
                    stress::Contents::Invalid(e) => format!("Stress: results file not usable: {e}"),
                    stress::Contents::Parsed(v) => {
                        let mtime = r.mtime_unix_s.map_or("mtime unknown".into(), |t| format!("mtime {}", recording::iso((t * 1e3) as u128)));
                        let peaks: Vec<String> = sim_domain_robot::stress_results::peaks(v).into_iter().map(|(k, p)| format!("{k} {}", p.map_or("—".into(), stress_label))).collect();
                        format!("Stress · {} · {path} · {mtime}\npeak: {}\n{}", r.status(m), peaks.join(" · "), sim_domain_robot::stress_results::SCALE)
                    }
                }
            }
        },
        (Some(_), Some(_)) => "Stress off (H): colours links from the model's .simresult.json".into(),
    };
    if line.0 != t {
        line.0 = t;
    }
}

/// The graph dock: the fixed chart set (`graphs::charts`) drawn with the
/// shared `crate::chart` raster, redrawn at most ten times a second and only
/// when the sampled history, selection, mode or visibility changed.
pub(super) fn graph_dock(
    mut commands: Commands,
    view: Res<RobotView>,
    fonts: Res<UiFonts>,
    time: Res<Time>,
    mut images: ResMut<Assets<Image>>,
    dock: Single<(Entity, &mut Node), With<GraphDock>>,
    mut handles: Local<Vec<Handle<Image>>>,
    mut drawn: Local<Option<(String, f64)>>,
    mut redraw: MessageWriter<bevy::window::RequestRedraw>,
) {
    let (entity, mut node) = dock.into_inner();
    let display = if view.graphs_visible { Display::Flex } else { Display::None };
    if node.display != display {
        node.display = display;
    }
    let Some(run) = view.run.as_ref().filter(|_| view.graphs_visible) else {
        *drawn = None;
        return;
    };
    let h = run.graphs();
    let gait = run.gait_preview().is_some_and(|g| g.loaded().is_some());
    let stamp = format!("{:?}|{}|{}|{}|{:?}|{:?}|{gait}", view.selected, h.generation(), h.frames(), run.graphs_mode(), h.window(), run.frame().map(|f| (f.time, f.steps)));
    let now = time.elapsed_secs_f64();
    match drawn.as_ref() {
        Some((s, _)) if *s == stamp => return,
        // Bounded refresh while frames stream in: come back once the interval has passed.
        Some((_, at)) if now - at < 0.1 && run.active() => {
            redraw.write(bevy::window::RequestRedraw);
            return;
        }
        _ => {}
    }
    *drawn = Some((stamp, now));
    let charts = run.graph_charts(view.selected);
    let mode = run.graphs_mode();
    commands.entity(entity).despawn_related::<Children>();
    let k = Kit { f: &fonts };
    if gait {
        // The charts are physics frames only; preview samples are never plotted as traces.
        let caption = commands.spawn((Node { width: Val::Px(150.0), flex_shrink: 0.0, ..default() }, children![k.text("Gait preview is kinematic and not charted: these charts are the physics run's frames only.", size::DETAIL, WARN, 0)])).id();
        commands.entity(entity).add_child(caption);
    }
    let num = |x: f64| if x == 0.0 || (x.abs() >= 1e-3 && x.abs() < 1e4) { format!("{x:.4}") } else { format!("{x:.3e}") };
    for (slot, c) in charts.iter().enumerate() {
        while handles.len() <= slot {
            handles.push(images.add(crate::chart::blank_image()));
        }
        let traces: Vec<(&[[f64; 2]], [u8; 3])> = c.traces.iter().enumerate().map(|(i, t)| (t.points.as_slice(), crate::chart::COLORS[i % crate::chart::COLORS.len()])).collect();
        let (pixels, range, window) = crate::chart::rasterize_span(&traces, Some(crate::robot::graphs::WINDOW_S));
        let drawable = c.traces.iter().map(|t| t.points.len()).sum::<usize>() >= 2;
        if let Some(mut image) = images.get_mut(&handles[slot]) {
            image.data = Some(pixels);
        }
        let units: std::collections::BTreeSet<&str> = c.traces.iter().map(|t| t.unit.as_str()).collect();
        let unit = if units.len() == 1 { units.into_iter().next().unwrap_or_default().to_string() } else { String::new() };
        let card = commands.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(0.0), row_gap: Val::Px(3.0), ..default() }).id();
        let head = commands.spawn(Node { flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(6.0), flex_shrink: 0.0, ..default() }).id();
        let title = commands.spawn(k.text(c.title.as_str(), size::CAPTION, TEXT, 1)).id();
        let badge = commands.spawn(k.text(format!("{} · gen {}", mode.to_uppercase(), h.generation()), size::DETAIL, if mode == "replay" { WARN } else { ACCENT }, 0)).id();
        commands.entity(head).add_children(&[title, badge]);
        commands.entity(card).add_child(head);
        if let Some(why) = &c.absent_reason {
            let t = commands.spawn(k.text(why.as_str(), size::DETAIL, SUBTLE, 0)).id();
            commands.entity(card).add_child(t);
        }
        if !c.traces.is_empty() {
            let plot = commands.spawn(k.chart_image(handles[slot].clone(), Node { flex_grow: 1.0, min_height: Val::Px(60.0), ..default() }, true)).id();
            if drawable {
                let with_unit = |v: f64| if unit.is_empty() { num(v) } else { format!("{} {unit}", num(v)) };
                let top = commands.spawn(k.chart_label(with_unit(range.1), Corner::TopLeft)).id();
                let bottom = commands.spawn(k.chart_label(with_unit(range.0), Corner::BottomLeft)).id();
                let x = commands.spawn(k.chart_label(format!("{:.2} – {:.2} s sim time", window.0, window.1), Corner::BottomRight)).id();
                commands.entity(plot).add_children(&[top, bottom, x]);
            }
            commands.entity(card).add_child(plot);
            for (i, t) in c.traces.iter().enumerate() {
                let [r, g, b] = crate::chart::COLORS[i % crate::chart::COLORS.len()];
                let value = match (t.points.last(), &t.absent_reason) {
                    (Some(p), _) => format!("{} {}", num(p[1]), t.unit),
                    (None, Some(why)) => why.clone(),
                    (None, None) => "–".into(),
                };
                let source = if t.source.starts_with("request") { "request (held input in frame)" } else if t.source.starts_with(crate::robot::graphs::WORLD_FRAME) { crate::robot::graphs::WORLD_FRAME } else { t.source.split(" (").next().unwrap_or(&t.source) };
                let color = Color::srgb_u8(r, g, b);
                let swatch = commands.spawn((Node { border_radius: BorderRadius::all(Val::Px(2.0)), width: Val::Px(9.0), height: Val::Px(9.0), flex_shrink: 0.0, ..default() }, BackgroundColor(color))).id();
                let line = commands.spawn(k.text(format!("{}: {value}  ·  {source}", t.name), size::SECTION, color, 0)).id();
                let row = commands.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(5.0), align_items: AlignItems::Center, ..default() }).add_children(&[swatch, line]).id();
                commands.entity(card).add_child(row);
            }
        }
        commands.entity(entity).add_child(card);
    }
}

/// The run-thread overlays' gizmo group: drawn over the link meshes (contacts sit under the
/// wheels and joint axes inside the links, so depth-tested lines were hidden), with wider lines.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub(super) struct OverlayGizmos;

pub(super) fn overlay_gizmo_config() -> GizmoConfig {
    GizmoConfig { depth_bias: -1.0, line: GizmoLineConfig { width: 3.0, ..default() }, ..default() }
}

/// Floor grid and the selected link's centre of mass.
pub(super) fn draw(view: Res<RobotView>, root: Single<&GlobalTransform, With<RobotRoot>>, mut gizmos: Gizmos, mut overlay: Gizmos<OverlayGizmos>) {
    if let Some(p) = &view.planar {
        // A planar file: outlines, centres of mass and chain tips from the planar run's latest frame.
        planar::draw(p, view.selected, &mut gizmos);
        return;
    }
    let Some(model) = &view.model else { return };
    // Run-thread overlays (`--robot FILE`), only from an accepted frame of the current
    // generation, mapped model → display through RobotRoot like the link meshes.
    if let Some(run) = view.run.as_ref() {
        let flags = run.overlays();
        if let Some(f) = run.frame().filter(|f| run::accept(run.generation(), f)) {
            let affine = root.affine();
            let point = |p: &[f64; 3]| affine.transform_point3(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32));
            let vector = |v: &[f64; 3], k: f64| affine.transform_vector3(Vec3::new((v[0] * k) as f32, (v[1] * k) as f32, (v[2] * k) as f32));
            if let Some(joints) = f.overlays.joints.as_ref().filter(|_| flags.joints) {
                const AXES: [Color; 3] = [Color::srgb(1.0, 0.95, 0.3), Color::srgb(0.3, 1.0, 0.95), Color::srgb(1.0, 0.5, 1.0)];
                for j in joints {
                    let p = point(&j.point);
                    for (k, a) in j.axes.iter().enumerate() {
                        let d = vector(a, run::JOINT_AXIS_HALF_M);
                        overlay.line(p - d, p + d, AXES[k % 3]);
                    }
                    overlay.sphere(Isometry3d::from_translation(p), 0.003, Color::WHITE);
                }
            }
            if let Some(contacts) = f.overlays.contacts.as_ref().filter(|_| flags.contacts) {
                for c in contacts {
                    let p = point(&c.point);
                    let color = if c.other == "ground" { Color::srgb(1.0, 0.2, 0.2) } else { Color::srgb(1.0, 0.4, 0.2) };
                    overlay.sphere(Isometry3d::from_translation(p), 0.003, color);
                    overlay.line(p, p + vector(&c.force, run::FORCE_SCALE_M_PER_N), color);
                }
            }
            if let Some(deflections) = f.overlays.deflections.as_ref().filter(|_| flags.deflections) {
                for d in deflections {
                    let p = point(&d.point);
                    overlay.line(p, p + vector(&d.displacement, run::DEFLECTION_MAGNIFICATION), Color::srgb(0.6, 1.0, 0.6));
                }
            }
        }
    }
    let floor = model.world.floor_z as f32;
    gizmos.grid(Isometry3d::new(Vec3::new(0.0, floor, 0.0), Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), UVec2::splat(20), Vec2::splat(0.05), Color::srgba(0.45, 0.50, 0.58, 0.35));
    if let Some(l) = view.selected.and_then(|i| model.links.get(i)) {
        let com = Vec3::new(l.com[0] as f32, l.com[2] as f32, -l.com[1] as f32);
        gizmos.sphere(Isometry3d::from_translation(com), 0.006, ACCENT);
    }
}
