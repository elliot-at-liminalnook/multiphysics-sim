//! The page's dial (web/viewer/calibration-ui.mjs :48 and `needle()` :74):
//! a half-ellipse track over a 300×95 view, the measured needle (solid,
//! round caps) and the requested needle (dashed 5 on, 4 off), rasterized on
//! the CPU into a small RGBA image (transparent background, like
//! `chart.rs`). The "Lower"/"Upper" captions are kit text under the image.
//! Colours are drawing colours, not UI tokens: `chart::COLORS[0]` measured,
//! `chart::COLORS[2]` requested, a grey track (the page's #42616b).
use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// The page's view box.
pub const VIEW: (f64, f64) = (300.0, 95.0);
/// Pixels per view unit.
pub const SCALE: f64 = 2.0;
/// The raster's size in pixels.
pub const SIZE: (u32, u32) = ((VIEW.0 * SCALE) as u32, (VIEW.1 * SCALE) as u32);
const TRACK: [u8; 3] = [66, 97, 107];
/// The needles' pivot (view units).
const PIVOT: (f64, f64) = (150.0, 85.0);

/// `needle(el, f)`: the needle's end for a fraction (clamped to 0..=1; 0 is
/// the lower end, left).
pub fn needle_end(f: f64) -> (f64, f64) {
    let f = if f.is_nan() { 0.5 } else { f.clamp(0.0, 1.0) };
    let a = std::f64::consts::PI * (1.0 - f);
    (150.0 + 85.0 * a.cos(), 85.0 - 65.0 * a.sin())
}

/// A transparent image of the raster's size, to draw into.
pub fn blank_image() -> Image {
    Image::new_fill(Extent3d { width: SIZE.0, height: SIZE.1, depth_or_array_layers: 1 }, TextureDimension::D2, &[0, 0, 0, 0], TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
}

/// One stroke's coverage: segments in view units, `width` in view units,
/// round caps and joins (`round`) or butt ends, an optional dash (on, off).
fn stroke(coverage: &mut [f32], segments: &[((f64, f64), (f64, f64))], width: f64, round: bool, dash: Option<(f64, f64)>) {
    let (w, h) = (SIZE.0 as i64, SIZE.1 as i64);
    let half = width / 2.0 * SCALE;
    let mut along = 0.0;
    for &((ax, ay), (bx, by)) in segments {
        let (ax, ay, bx, by) = (ax * SCALE, ay * SCALE, bx * SCALE, by * SCALE);
        let (dx, dy) = (bx - ax, by - ay);
        let length = (dx * dx + dy * dy).sqrt();
        let (x0, x1) = ((ax.min(bx) - half - 1.0).floor().max(0.0) as i64, ((ax.max(bx) + half + 1.0).ceil() as i64).min(w - 1));
        let (y0, y1) = ((ay.min(by) - half - 1.0).floor().max(0.0) as i64, ((ay.max(by) + half + 1.0).ceil() as i64).min(h - 1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f64 + 0.5 - ax, y as f64 + 0.5 - ay);
                let t = if length > 0.0 { (px * dx + py * dy) / length } else { 0.0 };
                if !round && (t < 0.0 || t > length) {
                    continue;
                }
                if let Some((on, off)) = dash
                    && ((along + t.clamp(0.0, length)) / SCALE).rem_euclid(on + off) >= on
                {
                    continue;
                }
                let tc = t.clamp(0.0, length);
                let (cx, cy) = if length > 0.0 { (dx * tc / length, dy * tc / length) } else { (0.0, 0.0) };
                let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
                let c = (half + 0.5 - d).clamp(0.0, 1.0) as f32;
                let i = (y * w + x) as usize;
                if c > coverage[i] {
                    coverage[i] = c;
                }
            }
        }
        along += length;
    }
}

/// Composite one layer's coverage in `color` over the pixels (straight alpha).
fn over(px: &mut [u8], coverage: &[f32], color: [u8; 3]) {
    for (i, &c) in coverage.iter().enumerate() {
        if c <= 0.0 {
            continue;
        }
        let p = &mut px[i * 4..i * 4 + 4];
        let dst_a = p[3] as f32 / 255.0;
        let out_a = c + dst_a * (1.0 - c);
        for k in 0..3 {
            let v = (color[k] as f32 * c + p[k] as f32 * dst_a * (1.0 - c)) / out_a.max(1e-6);
            p[k] = v.round().clamp(0.0, 255.0) as u8;
        }
        p[3] = (out_a * 255.0).round() as u8;
    }
}

/// The dial for a measured and a requested fraction, as RGBA pixels of [`SIZE`].
pub fn rasterize(measured: f64, requested: f64) -> Vec<u8> {
    let n = (SIZE.0 * SIZE.1) as usize;
    let mut px = vec![0u8; n * 4];
    // The track: `M 60 85 A 90 70 0 0 1 240 85`, the upper half of an
    // ellipse centred on the pivot.
    let arc: Vec<(f64, f64)> = (0..=48).map(|i| std::f64::consts::PI * (1.0 - i as f64 / 48.0)).map(|a| (150.0 + 90.0 * a.cos(), 85.0 - 70.0 * a.sin())).collect();
    let segments: Vec<_> = arc.windows(2).map(|p| (p[0], p[1])).collect();
    let mut layer = vec![0f32; n];
    stroke(&mut layer, &segments, 8.0, true, None);
    over(&mut px, &layer, TRACK);
    layer.fill(0.0);
    stroke(&mut layer, &[(PIVOT, needle_end(requested))], 3.0, false, Some((5.0, 4.0)));
    over(&mut px, &layer, crate::chart::COLORS[2]);
    layer.fill(0.0);
    stroke(&mut layer, &[(PIVOT, needle_end(measured))], 5.0, true, None);
    over(&mut px, &layer, crate::chart::COLORS[0]);
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(px: &[u8], x: f64, y: f64) -> [u8; 4] {
        let i = (((y * SCALE) as usize) * SIZE.0 as usize + (x * SCALE) as usize) * 4;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }

    #[test]
    fn needles_sweep_from_lower_to_upper() {
        let close = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
        assert!(close(needle_end(0.0), (65.0, 85.0)));
        assert!(close(needle_end(1.0), (235.0, 85.0)));
        assert!(close(needle_end(0.5), (150.0, 20.0)));
        assert!(close(needle_end(-3.0), needle_end(0.0)));
        assert!(close(needle_end(7.0), needle_end(1.0)));
    }

    #[test]
    fn the_measured_needle_is_drawn_over_the_requested_one() {
        let px = rasterize(0.5, 1.0);
        assert_eq!(px.len(), (SIZE.0 * SIZE.1 * 4) as usize);
        // Halfway up the measured needle: its colour, opaque.
        let [r, g, b, a] = pixel(&px, 150.0, 50.0);
        assert_eq!([r, g, b], crate::chart::COLORS[0]);
        assert_eq!(a, 255);
        // The requested needle lies along the baseline to the right; its first dash is drawn.
        let [r, g, b, _] = pixel(&px, 190.0, 85.0);
        assert_eq!([r, g, b], crate::chart::COLORS[2]);
        // The track's top, and an empty corner.
        let a = ((100.0 - 150.0) / 90.0f64).acos();
        assert_eq!(&pixel(&px, 100.0, 85.0 - 70.0 * a.sin())[..3], &TRACK[..]);
        assert_eq!(pixel(&px, 2.0, 2.0)[3], 0);
    }
}
