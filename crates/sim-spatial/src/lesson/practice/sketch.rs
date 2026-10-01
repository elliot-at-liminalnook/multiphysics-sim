//! Sketch questions: the drawing canvas, its input and dots, and the
//! drawn curve over the simulated one.
use super::*;

/// Columns in a sketch.
pub(crate) const SKETCH_COLUMNS: usize = 96;

/// Where the pointer draws a sketch.
#[derive(Component)]
pub(crate) struct SketchCanvas(pub String);
/// One column's dot of a sketch.
#[derive(Component)]
pub(crate) struct SketchDot(pub String, pub usize);

pub(super) fn sketch_canvas(c: &mut ChildSpawnerCommands, k: &Kit, l: &Learn, q: &Quiz, locked: bool) {
    let [t0, t1] = l.sketch_window(q);
    let [lo, hi] = q.range.unwrap_or([0., 1.]);
    let label = q.observe.clone().unwrap_or_default();
    c.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|r| {
        r.spawn(k.text(format!("{label} {}", if q.unit.is_empty() { String::new() } else { format!("({})", q.unit) }), 11.5, TEXT, 1));
        r.spawn(k.text(format!("{} … {}", crate::builder::ui::num(lo), crate::builder::ui::num(hi)), 10.5, FAINT, 0));
    });
    // The sketch's drawing surface (a canvas coloured like the chart it predicts).
    let mut canvas = c.spawn((Node { border_radius: BorderRadius::all(Val::Px(4.)), width: Val::Percent(100.), aspect_ratio: Some(720. / 200.), flex_shrink: 0., border: UiRect::all(Val::Px(1.)), ..default() }, BackgroundColor(Color::srgb(0.07, 0.086, 0.106)), BorderColor::all(BORDER)));
    if !locked {
        canvas.insert((k.pointer_surface("Sketch canvas", true), SketchCanvas(q.id.clone())));
    }
    canvas.with_children(|cv| {
        for g in 1..4 {
            cv.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.), right: Val::Px(0.), top: Val::Percent(25. * g as f32), height: Val::Px(1.), ..default() }, BackgroundColor(Color::srgb(0.16, 0.19, 0.23)), Pickable::IGNORE));
        }
        for i in 0..SKETCH_COLUMNS {
            cv.spawn((Node { border_radius: BorderRadius::all(Val::Px(2.5)), position_type: PositionType::Absolute, width: Val::Px(5.), height: Val::Px(5.), margin: UiRect { left: Val::Px(-2.5), top: Val::Px(-2.5), ..default() }, display: Display::None, ..default() }, BackgroundColor(Color::srgb(0.70, 0.58, 0.96)), SketchDot(q.id.clone(), i), Pickable::IGNORE));
        }
    });
    c.spawn(k.text(format!("time {} … {} s", crate::builder::ui::num(t0), crate::builder::ui::num(t1)), 10.5, FAINT, 0));
}

/// Drawing: dragging over a canvas sets each column the pointer crosses.
pub(in crate::lesson) fn sketch_input(canvases: Query<(&Interaction, &bevy::ui::RelativeCursorPosition, &SketchCanvas)>, mut learn: ResMut<Learn>, mut last: Local<Option<(String, usize, f32)>>) {
    let mut drawing = false;
    for (interaction, cursor, canvas) in &canvases {
        let (Interaction::Pressed, Some(p)) = (interaction, surface_point(cursor)) else { continue };
        drawing = true;
        let n = SKETCH_COLUMNS;
        let col = ((p.x.clamp(0., 0.9999)) * n as f32) as usize;
        let y = (1. - p.y).clamp(0., 1.);
        let columns = learn.sketches.entry(canvas.0.clone()).or_insert_with(|| vec![None; n]);
        // Fill the columns skipped since the last sample, so fast strokes stay continuous.
        match &*last {
            Some((id, c0, y0)) if id == &canvas.0 && *c0 != col => {
                let (a, b) = (*c0.min(&col), *c0.max(&col));
                for c in a..=b {
                    let f = (c as f32 - *c0 as f32) / (col as f32 - *c0 as f32);
                    columns[c] = Some(y0 + (y - y0) * f);
                }
            }
            _ => columns[col] = Some(y),
        }
        *last = Some((canvas.0.clone(), col, y));
    }
    if !drawing {
        *last = None;
    }
}

/// Place the dots of every sketch where its columns are.
pub(in crate::lesson) fn sketch_dots(learn: Res<Learn>, mut dots: Query<(&mut Node, &SketchDot)>) {
    if !learn.is_changed() {
        return;
    }
    let mut columns = std::collections::BTreeMap::new();
    for (mut node, dot) in &mut dots {
        let y = columns.entry(dot.0.clone()).or_insert_with(|| learn.sketch_columns(&dot.0)).as_ref().and_then(|c| c.get(dot.1).copied().flatten());
        let display = if y.is_some() { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        if let Some(y) = y {
            let left = Val::Percent((dot.1 as f32 + 0.5) / SKETCH_COLUMNS as f32 * 100.);
            let top = Val::Percent((1. - y) * 100.);
            if node.left != left || node.top != top {
                node.left = left;
                node.top = top;
            }
        }
    }
}

/// The comparison image and gap for a committed sketch, once its scene has a run.
pub(in crate::lesson) fn sketch_result(run: &sim_runtime::lesson::SceneRun, q: &Quiz, prediction: &str, window: [f64; 2]) -> Option<(Image, f64)> {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let sketch: Vec<[f64; 2]> = serde_json::from_str(prediction).ok()?;
    let series = run.series(q.observe.as_deref()?)?;
    let range = q.range?;
    let gap = sim_lesson::quiz::sketch_error(&sketch, &series.times, &series.values, range)?;
    let simulated: Vec<[f64; 2]> = series.times.iter().zip(&series.values).filter(|(t, _)| **t >= window[0] && **t <= window[1]).map(|(t, v)| [*t, *v]).collect();
    // Pin the axes to the question's so both curves sit where the reader drew.
    let frame = [[window[0], range[0]], [window[1], range[1]]];
    let (pixels, _, _) = crate::chart::rasterize_span(&[(&frame, [18, 22, 27]), (&simulated, [77, 212, 191]), (&sketch, [178, 148, 245])], None);
    let (w, h) = crate::chart::RASTER;
    Some((Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, pixels, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default()), gap))
}
