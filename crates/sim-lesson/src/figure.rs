//! Figures in lessons: Markdown images (`![caption](file.svg "title")`)
//! next to `lesson.md`. SVG is rasterized with `resvg`, with the app's IBM
//! Plex fonts loaded so diagram text matches the UI (`font-family="IBM Plex
//! Sans"`); PNG and JPEG are shown as they are. A figure's ID is its file
//! stem; narration can box or point at a region of it in the SVG's own
//! units (`figure:torque-speed@60,40,120,80`).
use crate::{BlockKind, Lesson, LessonError};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    /// File stem: `torque-speed` for `torque-speed.svg`.
    pub id: String,
    /// The text block holding the image.
    pub block: String,
    pub src: String,
    pub caption: String,
    pub line: usize,
}

pub const EXTENSIONS: [&str; 4] = ["svg", "png", "jpg", "jpeg"];
/// Largest rasterized width.
pub const MAX_WIDTH: u32 = 1800;

/// Pixels, straight (not premultiplied) sRGB RGBA, plus the figure's own
/// coordinate size (SVG viewBox or image pixels) for region targets.
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub units: (f32, f32),
}

impl Lesson {
    /// Every image in the lesson's text blocks, in order.
    pub fn figures(&self) -> Vec<Figure> {
        let mut out = Vec::new();
        for b in &self.blocks {
            let BlockKind::Markdown { text } = &b.kind else { continue };
            for (i, block) in sim_markdown::parse(text).blocks.iter().enumerate() {
                if let Some(img) = &block.image {
                    let _ = i;
                    let id = Path::new(&img.src).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    let line = b.line + text.lines().position(|l| l.contains(&format!("]({}", img.src))).unwrap_or(0);
                    out.push(Figure { id, block: b.id.clone(), src: img.src.clone(), caption: if img.title.is_empty() { img.alt.clone() } else { img.title.clone() }, line });
                }
            }
        }
        out
    }
    pub fn figure_path(&self, src: &str) -> PathBuf {
        self.dir().join(src)
    }
}

/// Shape checks done while parsing (no file access).
pub fn validate_src(src: &str) -> Result<(), String> {
    let p = Path::new(src);
    if src.starts_with("http://") || src.starts_with("https://") {
        return Err(format!("image `{src}`: lessons use local files (put it next to lesson.md)"));
    }
    if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("image `{src}` must be a path inside the lesson folder"));
    }
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("image `{src}`: use .svg, .png or .jpg"));
    }
    Ok(())
}

fn options() -> &'static resvg::usvg::Options<'static> {
    static OPTIONS: OnceLock<resvg::usvg::Options<'static>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        let mut o = resvg::usvg::Options::default();
        let db = o.fontdb_mut();
        for font in [
            &include_bytes!("../../sim-spatial/assets/fonts/IBMPlexSans-Regular.ttf")[..],
            &include_bytes!("../../sim-spatial/assets/fonts/IBMPlexSans-Medium.ttf")[..],
            &include_bytes!("../../sim-spatial/assets/fonts/IBMPlexSans-SemiBold.ttf")[..],
            &include_bytes!("../../sim-spatial/assets/fonts/IBMPlexSans-Italic.ttf")[..],
            &include_bytes!("../../sim-spatial/assets/fonts/IBMPlexMono-Regular.ttf")[..],
        ] {
            db.load_font_data(font.to_vec());
        }
        db.set_sans_serif_family("IBM Plex Sans");
        db.set_monospace_family("IBM Plex Mono");
        o.font_family = "IBM Plex Sans".into();
        o
    })
}

/// Parse an SVG and report its coordinate size.
pub fn svg_size(bytes: &[u8]) -> Result<(f32, f32), String> {
    let tree = resvg::usvg::Tree::from_data(bytes, options()).map_err(|e| format!("SVG: {e}"))?;
    Ok((tree.size().width(), tree.size().height()))
}

/// Rasterize an SVG at `width` pixels (keeping its aspect ratio).
pub fn rasterize_svg(bytes: &[u8], width: u32) -> Result<Raster, String> {
    let tree = resvg::usvg::Tree::from_data(bytes, options()).map_err(|e| format!("SVG: {e}"))?;
    let size = tree.size();
    let width = width.clamp(16, MAX_WIDTH);
    let scale = width as f32 / size.width().max(1.0);
    let height = ((size.height() * scale).round() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or("SVG too large to draw")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let rgba = pixmap.pixels().iter().flat_map(|p| {
        let c = p.demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }).collect();
    Ok(Raster { width, height, rgba, units: (size.width(), size.height()) })
}

/// Render an SVG to PNG bytes with the same fonts and rasterizer the viewer uses.
pub fn svg_png(bytes: &[u8], width: u32) -> Result<Vec<u8>, String> {
    let tree = resvg::usvg::Tree::from_data(bytes, options()).map_err(|e| format!("SVG: {e}"))?;
    let size = tree.size();
    let scale = width.clamp(16, MAX_WIDTH) as f32 / size.width().max(1.0);
    let mut pixmap = resvg::tiny_skia::Pixmap::new((size.width() * scale).round() as u32, ((size.height() * scale).round() as u32).max(1)).ok_or("SVG too large to draw")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| e.to_string())
}

/// Files exist and decode (SVG parses; PNG/JPEG have valid headers).
pub fn check(lesson: &Lesson) -> Vec<LessonError> {
    let mut out = Vec::new();
    let mut ids = std::collections::BTreeMap::new();
    for f in lesson.figures() {
        let err = |m: String| LessonError { path: lesson.path.clone(), line: f.line, message: m };
        if let Some(first) = ids.insert(f.id.clone(), f.line) {
            out.push(err(format!("figure id `{}` is used twice (lines {first} and {}); rename one file", f.id, f.line)));
        }
        let path = lesson.figure_path(&f.src);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                out.push(err(format!("image `{}`: {e}", f.src)));
                continue;
            }
        };
        let result = if f.src.to_lowercase().ends_with(".svg") { svg_size(&bytes).map(|_| ()) } else { sim_system::assets::image_size(&bytes).map(|_| ()).map_err(|e| e.to_string()) };
        if let Err(e) = result {
            out.push(err(format!("image `{}`: {e}", f.src)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"><rect width="200" height="100" fill="#ff0000"/><text x="10" y="50" font-family="IBM Plex Sans" font-size="20" fill="#ffffff">τ = k·i</text></svg>"##;
    #[test]
    fn svg_rasterizes_with_bundled_fonts_and_keeps_its_units() {
        let r = rasterize_svg(SVG.as_bytes(), 400).unwrap();
        assert_eq!((r.width, r.height, r.units), (400, 200, (200.0, 100.0)));
        // Straight alpha: an opaque red corner, and some white text pixels.
        assert_eq!(&r.rgba[..4], &[255, 0, 0, 255]);
        assert!(r.rgba.chunks(4).any(|p| p[0] > 200 && p[1] > 200 && p[2] > 200), "text was drawn");
        assert!(svg_png(SVG.as_bytes(), 100).unwrap().starts_with(b"\x89PNG"));
        assert!(svg_size(b"<svg").is_err());
    }
    #[test]
    fn image_paths_stay_inside_the_lesson_and_are_checked() {
        assert!(validate_src("torque-speed.svg").is_ok() && validate_src("img/a.png").is_ok());
        for bad in ["../x.svg", "/abs.svg", "https://e.com/a.png", "notes.txt"] {
            assert!(validate_src(bad).is_err(), "{bad}");
        }
        let dir = std::env::temp_dir().join(format!("fig-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("demo")).unwrap();
        std::fs::write(dir.join("demo/ok.svg"), SVG).unwrap();
        std::fs::write(dir.join("demo/lesson.md"), "---\ntitle: T\n---\nText.\n\n![Diagram](ok.svg \"Caption\")\n\n![Missing](gone.svg)\n").unwrap();
        let l = Lesson::load(&dir.join("demo/lesson.md")).unwrap();
        let figs = l.figures();
        assert_eq!(figs.iter().map(|f| (f.id.as_str(), f.caption.as_str(), f.line)).collect::<Vec<_>>(), vec![("ok", "Caption", 6), ("gone", "Missing", 8)]);
        let problems = check(&l);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].line, 8);
        let e = Lesson::parse(&dir.join("demo/lesson.md"), "---\ntitle: T\n---\n![x](../escape.svg)\n").unwrap_err();
        assert_eq!(e.line, 4);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
