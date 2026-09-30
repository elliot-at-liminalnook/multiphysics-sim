//! The CPU chart raster shared by build mode's graph dock, lesson plots and
//! robot mode's graph dock: traces of [x, y] points drawn on shared axes into
//! one RGBA texture per chart. Presentation only; the points are whatever the
//! caller recorded.
use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Size of one chart texture in pixels.
pub(crate) const RASTER: (u32, u32) = (720, 200);
/// Trace colours, in order.
pub(crate) const COLORS: [[u8; 3]; 6] = [[77, 212, 191], [222, 148, 84], [140, 170, 255], [235, 110, 160], [200, 200, 90], [170, 120, 230]];

/// An empty chart texture (the raster's background) to draw into.
pub(crate) fn blank_image() -> Image {
    Image::new_fill(Extent3d { width: RASTER.0, height: RASTER.1, depth_or_array_layers: 1 }, TextureDimension::D2, &[18, 22, 27, 255], TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
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
        let (px, range, window) = rasterize_span(&[(&points, [255, 0, 0])], Some(20.0));
        assert_eq!(px.len(), (RASTER.0 * RASTER.1 * 4) as usize);
        assert!(range.0 < -0.99 && range.1 > 0.99);
        assert!((window.1 - 1.99).abs() < 1e-12);
        let red = px.chunks(4).filter(|c| c[0] == 255 && c[1] == 0).count();
        assert!(red > RASTER.0 as usize, "trace drawn ({red} pixels)");
    }

    /// The builder's graph dock used to call its own `graphs::rasterize`,
    /// which was `rasterize_span(traces, Some(HISTORY_SECONDS))`; its callers
    /// now call this one rasterizer directly. The window is the only thing
    /// it adds: traces inside the window draw exactly the whole-range
    /// raster, and longer traces draw exactly the whole-range raster of the
    /// points inside the window (same pixels, range and x window).
    #[test]
    fn a_time_window_draws_the_same_pixels_as_the_points_inside_it() {
        let window = crate::builder::HISTORY_SECONDS;
        let wave = |i: usize, phase: f64| [i as f64 * 0.5, (i as f64 * 0.21 + phase).sin() * 3.0 - 0.4];
        // Inside the window (0 … 15 s): identical to the whole range.
        let a: Vec<[f64; 2]> = (0..=30).map(|i| wave(i, 0.0)).collect();
        let b: Vec<[f64; 2]> = (0..=30).map(|i| wave(i, 1.3)).collect();
        let short = [(a.as_slice(), COLORS[0]), (b.as_slice(), COLORS[1])];
        assert_eq!(rasterize_span(&short, Some(window)), rasterize_span(&short, None));
        // 0 … 50 s against a 20 s window: the points from 30 s on (sample
        // times are exact multiples of 0.5, so 30 s is one of them).
        let a: Vec<[f64; 2]> = (0..=100).map(|i| wave(i, 0.0)).collect();
        let b: Vec<[f64; 2]> = (0..=100).map(|i| wave(i, 1.3)).collect();
        let inside = |p: &Vec<[f64; 2]>| p.iter().copied().filter(|q| q[0] >= 50.0 - window).collect::<Vec<_>>();
        let (a_in, b_in) = (inside(&a), inside(&b));
        assert_eq!(a_in.first().map(|p| p[0]), Some(30.0));
        let windowed = rasterize_span(&[(a.as_slice(), COLORS[0]), (b.as_slice(), COLORS[1])], Some(window));
        let trimmed = rasterize_span(&[(a_in.as_slice(), COLORS[0]), (b_in.as_slice(), COLORS[1])], None);
        assert_eq!(windowed.1, trimmed.1);
        assert_eq!(windowed.2, (30.0, 50.0));
        assert_eq!(windowed.2, trimmed.2);
        assert!(windowed.0 == trimmed.0, "the windowed raster differs from the raster of the points inside the window");
    }
}
