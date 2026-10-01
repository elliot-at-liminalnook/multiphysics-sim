//! Screen overlays placed on narration targets every frame.
use super::*;

/// Screen overlays: text highlight, boxes and arrows, placed every frame
/// on their targets (blocks, charts, scene cards, projected parts).
#[derive(Component)]
pub(crate) struct Overlay;
/// A figure image, by ID, with its own coordinate size.
#[derive(Component)]
pub(crate) struct FigureNode(pub String, pub f32, pub f32);
/// A chart image, by observable key, with its time window.
#[derive(Component)]
pub(crate) struct ChartNode(pub String, pub f64, pub f64);

#[allow(clippy::too_many_arguments)]
pub(in crate::lesson) fn overlay(
    mut commands: Commands,
    learn: Res<Learn>,
    scene: Res<SpatialScene>,
    fonts: Option<Res<UiFonts>>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform), With<Orbit>>,
    blocks: Query<(&super::super::ui::BlockNode, &ComputedNode, &UiGlobalTransform)>,
    charts: Query<(&ChartNode, &ComputedNode, &UiGlobalTransform)>,
    figures: Query<(&FigureNode, &ComputedNode, &UiGlobalTransform)>,
    existing: Query<Entity, With<Overlay>>,
    mut signature: Local<String>,
) {
    let Some(fonts) = fonts else { return };
    let scale = window.scale_factor();
    let rect_of = |node: &ComputedNode, gt: &UiGlobalTransform| Rect::from_center_size(gt.translation / scale, node.size() / scale);
    let block_rect = |id: &str| blocks.iter().find(|(b, n, _)| b.0 == id && n.size().y > 0.).map(|(_, n, g)| rect_of(n, g));
    let page = Rect::new(LEFT_WIDTH, TOPBAR, window.width() - RIGHT_WIDTH, window.height() - STATUSBAR);
    let mut items: Vec<(u8, Rect, String)> = Vec::new(); // 0 highlight, 1 box, 2 arrow
    if let (true, Some(n)) = (learn.active, &learn.narration) {
        let m = &n.marks;
        if let Some(q) = &m.text {
            if let Some(r) = target_block(&learn, &Target::Text { quote: q.clone() }).and_then(|b| block_rect(&b)) {
                items.push((0, r, String::new()));
            }
        }
        if let Some(t) = &m.block {
            if let Some(r) = target_block(&learn, t).and_then(|b| block_rect(&b)) {
                items.push((0, r, String::new()));
            }
        }
        for mark in &m.marks {
            let rect = match &mark.target {
                Target::Part { path } => {
                    let view = scene.learn_view.filter(|v| v.visible.width() > 1.);
                    view.and_then(|v| {
                        let (center, radius) = scene.bounds_of(Some(path));
                        let (cam, gt) = *camera;
                        // Through NDC, which follows the card's partial view.
                        let size = v.visible.size() / scale;
                        let to_screen = |w: Vec3| cam.world_to_ndc(gt, w).map(|n| Vec2::new((n.x + 1.) * 0.5 * size.x, (1. - n.y) * 0.5 * size.y));
                        let c = to_screen(center)?;
                        let edge = to_screen(center + gt.right() * radius.max(0.004))?;
                        let r = (edge - c).length().max(14.);
                        let origin = v.visible.min / scale;
                        let p = origin + c;
                        let inside = Rect::from_corners(v.visible.min / scale, v.visible.max / scale);
                        inside.contains(p).then(|| Rect::from_center_half_size(p, Vec2::splat(r)))
                    })
                }
                Target::Plot { key, window: w } => charts.iter().find(|(c, n, _)| &c.0 == key && n.size().y > 0.).map(|(c, n, g)| {
                    let r = rect_of(n, g);
                    match w {
                        Some([a, b]) => {
                            let span = (c.2 - c.1).max(1e-12);
                            let x0 = r.min.x + r.width() * ((a - c.1) / span).clamp(0., 1.) as f32;
                            let x1 = r.min.x + r.width() * ((b - c.1) / span).clamp(0., 1.) as f32;
                            Rect::new(x0, r.min.y, x1.max(x0 + 4.), r.max.y)
                        }
                        None => r,
                    }
                }),
                Target::Figure { id, region } => figures.iter().find(|(f, n, _)| &f.0 == id && n.size().y > 0.).map(|(f, n, g)| {
                    let r = rect_of(n, g);
                    match region {
                        Some([x, y, w, h]) => {
                            let (sx, sy) = (r.width() / f.1.max(1.), r.height() / f.2.max(1.));
                            Rect::new(r.min.x + x * sx, r.min.y + y * sy, r.min.x + (x + w) * sx, r.min.y + (y + h) * sy)
                        }
                        None => r,
                    }
                }),
                other => target_block(&learn, other).and_then(|b| block_rect(&b)),
            };
            if let Some(r) = rect {
                items.push((if mark.arrow { 2 } else { 1 }, r, mark.label.clone()));
            }
        }
    }
    // Keep only what is on the page (not under the toolbar or panels).
    items.retain(|(_, r, _)| r.max.y > page.min.y && r.min.y < page.max.y);
    let sig = items.iter().map(|(k, r, l)| format!("{k}|{l}|{:.0},{:.0},{:.0},{:.0}", r.min.x, r.min.y, r.max.x, r.max.y)).collect::<Vec<_>>().join(";");
    if *signature == sig {
        return;
    }
    *signature = sig;
    for e in &existing {
        commands.entity(e).despawn();
    }
    let accent = ACCENT;
    let label = |commands: &mut Commands, text: &str, at: Vec2| {
        if text.is_empty() {
            return;
        }
        commands.spawn((
            Overlay,
            Node { border_radius: BorderRadius::all(Val::Px(4.)), position_type: PositionType::Absolute, left: Val::Px(at.x), top: Val::Px(at.y), padding: UiRect::axes(Val::Px(8.), Val::Px(3.)), ..default() },
            BackgroundColor(accent),
            GlobalZIndex(31),
            Pickable::IGNORE,
            children![(Text::new(text), TextFont { font: fonts.semibold.clone().into(), font_size: FontSize::Px(12.), ..default() }, TextColor(ON_ACCENT))],
        ));
    };
    let line = |commands: &mut Commands, a: Vec2, b: Vec2, width: f32| {
        let d = b - a;
        commands.spawn((
            Overlay,
            Node { border_radius: BorderRadius::all(Val::Px(width * 0.5)), position_type: PositionType::Absolute, left: Val::Px((a.x + b.x - d.length()) * 0.5), top: Val::Px((a.y + b.y) * 0.5 - width * 0.5), width: Val::Px(d.length()), height: Val::Px(width), ..default() },
            UiTransform::from_rotation(Rot2::radians(d.y.atan2(d.x))),
            BackgroundColor(accent),
            GlobalZIndex(30),
            Pickable::IGNORE,
        ));
    };
    for (kind, r, text) in &items {
        match kind {
            0 => {
                let r = Rect::new(r.min.x - 6., r.min.y - 3., r.max.x + 6., r.max.y + 3.);
                commands.spawn((Overlay, Node { border_radius: BorderRadius::all(Val::Px(4.)), position_type: PositionType::Absolute, left: Val::Px(r.min.x), top: Val::Px(r.min.y), width: Val::Px(r.width()), height: Val::Px(r.height()), border: UiRect::left(Val::Px(3.)), ..default() }, BackgroundColor(accent.with_alpha(0.10)), BorderColor::all(accent), GlobalZIndex(29), Pickable::IGNORE));
            }
            1 => {
                let r = Rect::new(r.min.x - 5., r.min.y - 5., r.max.x + 5., r.max.y + 5.);
                commands.spawn((Overlay, Node { border_radius: BorderRadius::all(Val::Px(7.)), position_type: PositionType::Absolute, left: Val::Px(r.min.x), top: Val::Px(r.min.y), width: Val::Px(r.width()), height: Val::Px(r.height()), border: UiRect::all(Val::Px(2.5)), ..default() }, BorderColor::all(accent), GlobalZIndex(30), Pickable::IGNORE));
                label(&mut commands, text, Vec2::new(r.min.x, (r.min.y - 24.).max(page.min.y + 4.)));
            }
            _ => {
                // Tail up-left of the target, kept on the page; head on the target's edge.
                let target = r.center();
                let mut tail = Vec2::new(r.min.x - 70., r.min.y - 56.);
                tail = tail.clamp(page.min + Vec2::new(12., 30.), page.max - Vec2::new(12., 12.));
                if tail.distance(target) < 40. {
                    tail = target + Vec2::new(-80., -60.);
                }
                let dir = (target - tail).normalize_or(Vec2::X);
                // Where the ray from the tail enters the target rectangle (padded).
                let pad = Rect::new(r.min.x - 6., r.min.y - 6., r.max.x + 6., r.max.y + 6.);
                let mut head = target;
                for step in 0..400 {
                    let p = tail + dir * step as f32 * 2.;
                    if pad.contains(p) {
                        head = p;
                        break;
                    }
                }
                line(&mut commands, tail, head, 3.);
                let back = -dir * 14.;
                let side = Vec2::new(-dir.y, dir.x) * 7.;
                line(&mut commands, head, head + back + side, 3.);
                line(&mut commands, head, head + back - side, 3.);
                label(&mut commands, text, tail + Vec2::new(-8., -26.));
            }
        }
    }
}
