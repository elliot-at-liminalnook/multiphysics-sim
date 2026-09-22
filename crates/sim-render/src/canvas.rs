use ab_glyph::{Font, FontArc, PxScale, ScaleFont, point};
use image::{Rgba, RgbaImage};
use std::sync::OnceLock;
pub const INK: [u8; 3] = [34, 48, 64];
pub const MUTED: [u8; 3] = [92, 110, 124];
pub const TEAL: [u8; 3] = [16, 133, 151];
pub const GRID: [u8; 3] = [218, 226, 232];
pub struct Canvas {
    pub image: RgbaImage,
}
impl Canvas {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            image: RgbaImage::from_pixel(w, h, Rgba([247, 250, 252, 255])),
        }
    }
    pub fn blend(&mut self, x: i32, y: i32, color: [u8; 3], alpha: f32) {
        if x < 0 || y < 0 || x >= self.image.width() as i32 || y >= self.image.height() as i32 {
            return;
        }
        let p = self.image.get_pixel_mut(x as u32, y as u32);
        let a = alpha.clamp(0., 1.);
        for i in 0..3 {
            p[i] = (p[i] as f32 * (1. - a) + color[i] as f32 * a).round() as u8;
        }
    }
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [u8; 3]) {
        for yy in (y.max(0.) as i32)..((y + h).min(self.image.height() as f32) as i32) {
            for xx in (x.max(0.) as i32)..((x + w).min(self.image.width() as f32) as i32) {
                self.blend(xx, yy, color, 1.);
            }
        }
    }
    pub fn dot(&mut self, x: f32, y: f32, r: f32, color: [u8; 3]) {
        for yy in (y - r - 1.).floor() as i32..=(y + r + 1.).ceil() as i32 {
            for xx in (x - r - 1.).floor() as i32..=(x + r + 1.).ceil() as i32 {
                let d = ((xx as f32 + 0.5 - x).powi(2) + (yy as f32 + 0.5 - y).powi(2)).sqrt();
                self.blend(xx, yy, color, (r + 0.5 - d).clamp(0., 1.));
            }
        }
    }
    pub fn line(&mut self, a: [f32; 2], b: [f32; 2], width: f32, color: [u8; 3]) {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let n = dx.abs().max(dy.abs()).ceil().min(10000.) as usize;
        for i in 0..=n {
            let t = i as f32 / n.max(1) as f32;
            self.dot(a[0] + dx * t, a[1] + dy * t, width * 0.5, color);
        }
    }
    pub fn text(&mut self, x: f32, y: f32, size: f32, text: &str, color: [u8; 3], max_width: f32) {
        static FONT: OnceLock<FontArc> = OnceLock::new();
        let font = FONT
            .get_or_init(|| FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).unwrap());
        let scale = PxScale::from(size);
        let scaled = font.as_scaled(scale);
        let mut caret = x;
        let baseline = y + scaled.ascent();
        let mut previous = None;
        for ch in text.chars() {
            let id = font.glyph_id(ch);
            if let Some(prev) = previous {
                caret += scaled.kern(prev, id);
            }
            let advance = scaled.h_advance(id);
            if caret + advance > x + max_width {
                break;
            }
            if let Some(g) =
                font.outline_glyph(id.with_scale_and_position(scale, point(caret, baseline)))
            {
                let bounds = g.px_bounds();
                g.draw(|gx, gy, coverage| {
                    self.blend(
                        bounds.min.x as i32 + gx as i32,
                        bounds.min.y as i32 + gy as i32,
                        color,
                        coverage,
                    )
                });
            }
            caret += advance;
            previous = Some(id);
        }
    }
    pub fn header(&mut self, title: &str, subtitle: &str) {
        let w = self.image.width() as f32;
        self.rect(0., 0., w, 84., [255, 255, 255]);
        self.text(28., 17., 26., title, INK, w - 56.);
        self.text(28., 52., 15., subtitle, MUTED, w - 56.);
        self.line([0., 83.], [w, 83.], 1., GRID);
    }
    pub fn png(self) -> Result<Vec<u8>, String> {
        let mut output = std::io::Cursor::new(Vec::new());
        self.image
            .write_to(&mut output, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        Ok(output.into_inner())
    }
}
pub fn number(v: f64) -> String {
    if v != 0. && (v.abs() < 0.001 || v.abs() >= 10000.) {
        format!("{v:.2e}")
    } else {
        format!("{v:.3}")
    }
}
