//! Graph dock: live time histories of observables from the background run,
//! drawn under the physical view. Presentation only: the samples are the
//! committed frames of the shared `SystemSession`; nothing is integrated here.
//!
//! What is plotted: pinned observables if any, else the most telling
//! quantities of the one selected instance (speed, current, torque, …), else
//! the system's generated readouts. Traces are rasterised on the CPU into one
//! texture per chart (a few hundred kilobytes, redrawn ten times a second).
use super::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sim_inspect::ObservationLocation;

pub(super) const MAX_CHARTS: usize = 4;
/// Height of the dock in logical pixels.
pub(super) const DOCK: f32 = 196.0;
pub(crate) const RASTER: (u32, u32) = (720, 200);
/// Cap on observables recorded for graphs, beyond the ones pinned.
const RECORD_CAP: usize = 160;

/// Quantities worth plotting, most telling first.
const PREFERRED: [&str; 11] = ["AngularVelocity", "Current", "Torque", "Velocity", "Force", "Temperature", "Voltage", "Angle", "Position", "HeatFlow", "Power"];

#[derive(Default)]
pub(super) struct Graphs {
    pub visible: bool,
    pub pinned: Vec<String>,
    pub images: Vec<Handle<Image>>,
    pub charts: Vec<Chart>,
    refreshed: f64,
    /// Study result already drawn (by source hash + name), to avoid redrawing.
    drawn_study: Option<String>,
}

impl Graphs {
    pub fn force_refresh(&mut self) {
        self.refreshed = f64::NEG_INFINITY;
        self.drawn_study = None;
    }
}

#[derive(Clone, Debug)]
pub(super) struct Chart {
    pub id: String,
    pub title: String,
    pub unit: String,
    pub latest: Option<f64>,
    pub range: (f64, f64),
    pub window: (f64, f64),
    pub color: [u8; 3],
    pub pinned: bool,
    /// Overlaid variants (study results): label and colour per trace.
    pub legend: Vec<(String, [u8; 3])>,
    /// Horizontal axis label ("s" for time; the parameter for sweeps).
    pub x_label: String,
}

const COLORS: [[u8; 3]; 6] = [[77, 212, 191], [222, 148, 84], [140, 170, 255], [235, 110, 160], [200, 200, 90], [170, 120, 230]];

fn quantity_rank(o: &sim_inspect::ObservableDescriptor) -> Option<usize> {
    let name = o.quantity.name.rsplit('.').next().unwrap_or("");
    PREFERRED.iter().position(|q| *q == name)
}

fn component_of<'a>(scene: &'a SpatialScene, o: &'a sim_inspect::ObservableDescriptor) -> Option<&'a str> {
    match &o.location {
        ObservationLocation::Across { port, .. } | ObservationLocation::Through { port, .. } | ObservationLocation::Signal { port } => scene.description.ports.get(port).map(|p| p.component.as_str()),
        ObservationLocation::State { component, .. } => Some(component.as_str()),
        ObservationLocation::Diagnostic { component, .. } => component.as_deref(),
    }
}

/// Plottable observables of the instance at flattened path `path` (itself
/// or anything inside it), best first: (id, title relative to the path).
pub(super) fn candidates(scene: &SpatialScene, path: &str) -> Vec<(String, String)> {
    let mut out: Vec<(usize, String, String)> = scene
        .description
        .observables
        .iter()
        .filter(|(_, o)| o.availability == sim_inspect::Availability::Available)
        .filter_map(|(id, o)| {
            let rank = quantity_rank(o)?;
            let component = component_of(scene, o)?;
            (component == path || component.starts_with(&format!("{path}/"))).then(|| {
                let key = system_builder::observable_key(&scene.description, id);
                let title = key.strip_prefix(&format!("{path}/")).or_else(|| key.strip_prefix(&format!("{path}."))).unwrap_or(&key).to_string();
                (rank, id.clone(), title)
            })
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));
    out.into_iter().map(|(_, id, title)| (id, title)).collect()
}

/// What the run should record so graphs have data: pinned first, then
/// plottable observables up to a cap.
pub(super) fn recordable(scene: &SpatialScene, pinned: &[String]) -> Vec<String> {
    let mut ids: Vec<String> = pinned.to_vec();
    let mut rest: Vec<(usize, &String)> = scene.description.observables.iter().filter(|(_, o)| o.availability == sim_inspect::Availability::Available).filter_map(|(id, o)| quantity_rank(o).map(|r| (r, id))).collect();
    rest.sort();
    ids.extend(rest.into_iter().take(RECORD_CAP).map(|(_, id)| id.clone()));
    ids
}

/// The observables the dock shows now.
fn plotted(builder: &Builder, scene: &SpatialScene) -> Vec<(String, bool)> {
    if !builder.graphs.pinned.is_empty() {
        return builder.graphs.pinned.iter().map(|p| (p.clone(), true)).collect();
    }
    if let Some(name) = builder.only_selected() {
        let found = candidates(scene, &builder.full_path(&name));
        // One chart per quantity kind reads better than three speeds.
        let mut kinds = BTreeSet::new();
        let chosen: Vec<(String, bool)> = found
            .into_iter()
            .filter(|(id, _)| kinds.insert(scene.description.observables[id].quantity.name.clone()))
            .take(3)
            .map(|(id, _)| (id, false))
            .collect();
        if !chosen.is_empty() {
            return chosen;
        }
    }
    let readouts: Vec<(String, bool)> = scene.animation.as_ref().map(|a| a.readouts.iter().take(3).map(|r| (r.observable.clone(), false)).collect()).unwrap_or_default();
    if !readouts.is_empty() {
        return readouts;
    }
    recordable(scene, &[]).into_iter().take(3).map(|id| (id, false)).collect()
}

pub(super) fn update(time: Res<Time>, mut builder: ResMut<Builder>, mut scene: ResMut<SpatialScene>, mut images: ResMut<Assets<Image>>) {
    let dock = if builder.graphs.visible { DOCK } else { 0. };
    if scene.builder_dock != dock {
        scene.builder_dock = dock;
        builder.panel_dirty = true;
    }
    let now = time.elapsed_secs_f64();
    if !builder.graphs.visible || now - builder.graphs.refreshed < 0.1 {
        return;
    }
    builder.graphs.refreshed = now;
    if let Some(result) = builder.study.result.clone() {
        let key = format!("{}#{}", result.source_hash, result.name);
        if builder.graphs.drawn_study.as_deref() != Some(key.as_str()) {
            study_charts(&mut builder, &result, &mut images);
            builder.graphs.drawn_study = Some(key);
            builder.panel_dirty = true;
        }
        return;
    }
    builder.graphs.drawn_study = None;
    let ids = plotted(&builder, &scene);
    let mut charts = Vec::new();
    for (slot, (id, pinned)) in ids.iter().enumerate().take(MAX_CHARTS) {
        let Some(o) = scene.description.observables.get(id) else { continue };
        let unit = sim_inspect::plot::unit(&scene.description, o).to_string();
        let history = builder.history(id);
        let color = COLORS[slot % COLORS.len()];
        ensure_image(&mut builder, slot, &mut images);
        let (pixels, range, window) = rasterize(&[(&history, color)]);
        if let Some(image) = images.get_mut(&builder.graphs.images[slot]) {
            image.data = Some(pixels);
        }
        charts.push(Chart { id: id.clone(), title: system_builder::observable_key(&scene.description, id), unit, latest: history.last().map(|p| p[1]), range, window, color, pinned: *pinned, legend: Vec::new(), x_label: "s".into() });
    }
    let changed = charts.len() != builder.graphs.charts.len() || charts.iter().zip(&builder.graphs.charts).any(|(a, b)| a.id != b.id || a.latest != b.latest);
    builder.graphs.charts = charts;
    if changed {
        builder.panel_dirty = true;
    }
}

fn ensure_image(builder: &mut Builder, slot: usize, images: &mut Assets<Image>) {
    while builder.graphs.images.len() <= slot {
        let image = Image::new_fill(Extent3d { width: RASTER.0, height: RASTER.1, depth_or_array_layers: 1 }, TextureDimension::D2, &[18, 22, 27, 255], TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
        builder.graphs.images.push(images.add(image));
    }
}

/// One chart per observed quantity with a trace per variant; for sweeps, a
/// first chart of the first metric against the swept value.
fn study_charts(builder: &mut Builder, result: &sim_runtime::system_study::StudyResult, images: &mut Assets<Image>) {
    let legend: Vec<(String, [u8; 3])> = result.variants.iter().enumerate().map(|(i, v)| (v.label.clone(), COLORS[i % COLORS.len()])).collect();
    let mut charts = Vec::new();
    let mut slot = 0;
    if let (Some(parameter), Some(first)) = (&result.parameter, result.variants.first().and_then(|v| v.metrics.first()).map(|m| m.0.clone())) {
        let points: Vec<[f64; 2]> = result.variants.iter().filter_map(|v| Some([v.value?, v.metrics.iter().find(|m| m.0 == first)?.1])).filter(|p| p[1].is_finite()).collect();
        ensure_image(builder, slot, images);
        let (pixels, range, window) = rasterize(&[(&points, COLORS[0])]);
        if let Some(image) = images.get_mut(&builder.graphs.images[slot]) {
            image.data = Some(pixels);
        }
        charts.push(Chart { id: format!("{parameter}→{first}"), title: format!("{first} vs {parameter}"), unit: String::new(), latest: points.last().map(|p| p[1]), range, window, color: COLORS[0], pinned: false, legend: Vec::new(), x_label: parameter.clone() });
        slot += 1;
    }
    let observables: Vec<(String, String)> = result.variants.iter().find(|v| !v.series.is_empty()).map(|v| v.series.iter().map(|s| (s.label.clone(), s.unit.clone())).collect()).unwrap_or_default();
    for (label, unit) in observables.into_iter().take(MAX_CHARTS - slot) {
        let traces: Vec<(Vec<[f64; 2]>, [u8; 3])> = result
            .variants
            .iter()
            .enumerate()
            .filter_map(|(i, v)| v.series.iter().find(|s| s.label == label).map(|s| (s.times.iter().zip(&s.values).map(|(t, x)| [*t, *x]).collect(), COLORS[i % COLORS.len()])))
            .collect();
        ensure_image(builder, slot, images);
        let refs: Vec<(&[[f64; 2]], [u8; 3])> = traces.iter().map(|(p, c)| (p.as_slice(), *c)).collect();
        let (pixels, range, window) = rasterize(&refs);
        if let Some(image) = images.get_mut(&builder.graphs.images[slot]) {
            image.data = Some(pixels);
        }
        charts.push(Chart { id: label.clone(), title: label, unit, latest: None, range, window, color: COLORS[0], pinned: false, legend: legend.clone(), x_label: "s".into() });
        slot += 1;
    }
    builder.graphs.charts = charts;
}

/// Draw traces on shared axes; returns RGBA pixels, the y range and the x window.
pub(crate) fn rasterize(traces: &[(&[[f64; 2]], [u8; 3])]) -> (Vec<u8>, (f64, f64), (f64, f64)) {
    rasterize_span(traces, Some(HISTORY_SECONDS))
}

/// Draw traces of [x, y] points; `last` keeps only that much of x before its
/// maximum (a time window), `None` the whole x range (an x–y plot).
pub(crate) fn rasterize_span(traces: &[(&[[f64; 2]], [u8; 3])], last: Option<f64>) -> (Vec<u8>, (f64, f64), (f64, f64)) {
    let (w, h) = (RASTER.0 as i64, RASTER.1 as i64);
    let mut px = vec![0u8; (w * h * 4) as usize];
    let mut put = |x: i64, y: i64, c: [u8; 3], a: f32| {
        if x < 0 || y < 0 || x >= w || y >= h {
            return;
        }
        let i = ((y * w + x) * 4) as usize;
        for k in 0..3 {
            px[i + k] = (px[i + k] as f32 * (1. - a) + c[k] as f32 * a) as u8;
        }
        px[i + 3] = 255;
    };
    for y in 0..h {
        for x in 0..w {
            put(x, y, [18, 22, 27], 1.);
        }
    }
    for k in 1..4 {
        let y = h * k / 4;
        for x in 0..w {
            put(x, y, [40, 47, 56], 1.);
        }
    }
    let all: Vec<&[f64; 2]> = traces.iter().flat_map(|(p, _)| p.iter()).collect();
    if all.len() < 2 {
        return (px, (0., 0.), (0., 0.));
    }
    let t1 = all.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    let t0 = all.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min).max(last.map_or(f64::NEG_INFINITY, |l| t1 - l));
    let (mut lo, mut hi) = all.iter().filter(|p| p[0] >= t0).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    let span = (hi - lo).max(1e-9 * hi.abs().max(lo.abs())).max(1e-12);
    lo -= 0.08 * span;
    hi += 0.08 * span;
    let to_px = |p: &[f64; 2]| -> (i64, i64) {
        let x = if t1 > t0 { (p[0] - t0) / (t1 - t0) } else { 1. };
        let y = (p[1] - lo) / (hi - lo);
        ((x * (w - 1) as f64).round() as i64, ((1. - y) * (h - 1) as f64).round() as i64)
    };
    if lo < 0. && hi > 0. {
        let (_, y0) = to_px(&[t0, 0.]);
        for x in 0..w {
            put(x, y0, [90, 100, 112], 1.);
        }
    }
    for (points, color) in traces {
    let color = *color;
    let Some(first) = points.iter().find(|p| p[0] >= t0) else { continue };
    let mut previous = to_px(first);
    if points.len() == 1 || points.len() <= 12 {
        // Few points (a sweep): mark each one.
        for p in points.iter() {
            let (x, y) = to_px(p);
            for dx in -3..=3 {
                for dy in -3..=3 {
                    put(x + dx, y + dy, color, 1.);
                }
            }
        }
    }
    for p in points.iter().filter(|p| p[0] >= t0).skip(1) {
        let next = to_px(p);
        let (dx, dy) = (next.0 - previous.0, next.1 - previous.1);
        let steps = dx.abs().max(dy.abs()).max(1);
        for s in 0..=steps {
            let x = previous.0 + dx * s / steps;
            let y = previous.1 + dy * s / steps;
            put(x, y, color, 1.);
            put(x, y + 1, color, 0.85);
            put(x, y - 1, color, 0.35);
        }
        previous = next;
    }
    }
    (px, (lo, hi), (t0, t1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasterize_draws_the_trace_inside_the_frame() {
        let points: Vec<[f64; 2]> = (0..200).map(|i| [i as f64 * 0.01, (i as f64 * 0.05).sin()]).collect();
        let (px, range, window) = rasterize(&[(&points, [255, 0, 0])]);
        assert_eq!(px.len(), (RASTER.0 * RASTER.1 * 4) as usize);
        assert!(range.0 < -0.99 && range.1 > 0.99);
        assert!((window.1 - 1.99).abs() < 1e-12);
        let red = px.chunks(4).filter(|c| c[0] == 255 && c[1] == 0).count();
        assert!(red > RASTER.0 as usize, "trace drawn ({red} pixels)");
    }
}
