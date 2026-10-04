//! CAD bodies to a PNG on the CPU (RoboCAD's headless `GET /render`):
//! exact-tessellation triangles drawn with a depth buffer and soft
//! two-light shading, in shaded, x-ray or wireframe mode, from a preset
//! direction or any `[dx, dy, dz]`, optionally cut by an axis section
//! (keeping the side below the value), with highlighted parts, the bodies'
//! feature edges (face boundaries), labels at the parts' centres and a title.
//! Millimetres, Z up. Deterministic: the same input gives the same bytes.
use crate::canvas::{Canvas, INK, MUTED};
use glam::DVec3;

/// One body to draw (world placed, mm).
#[derive(Clone, Debug)]
pub struct Body {
    pub id: String,
    pub name: String,
    pub vertices: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    /// The B-rep face of each triangle (feature edges lie between faces).
    pub triangle_face: Vec<u32>,
    pub color: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct Options {
    pub width: u32,
    pub height: u32,
    /// The eye direction from the target (unit not needed).
    pub view: [f64; 3],
    /// shaded | xray | wireframe.
    pub mode: String,
    /// Keep `axis` (0 x, 1 y, 2 z) ≤ value.
    pub section: Option<(usize, f64)>,
    pub highlight: Vec<String>,
    pub labels: bool,
    pub edges: bool,
    /// Frame these bodies (all when empty).
    pub focus: Vec<String>,
    pub title: Option<String>,
}

/// RoboCAD's render view presets (eye direction from the target).
pub fn preset(view: &str) -> Option<[f64; 3]> {
    Some(match view {
        "iso" => [1., -1., 0.8],
        "iso2" => [-1., -1., 0.8],
        "under" => [1., -1., -0.8],
        "front" => [0., -1., 0.],
        "back" => [0., 1., 0.],
        "right" => [1., 0., 0.],
        "left" => [-1., 0., 0.],
        "top" => [0., 0., 1.],
        "bottom" => [0., 0., -1.],
        _ => return None,
    })
}

fn rgb(c: [f32; 3]) -> [u8; 3] {
    c.map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
}

/// Render `bodies`: the PNG bytes.
pub fn render(bodies: &[Body], o: &Options) -> Result<Vec<u8>, String> {
    if !(16..=8192).contains(&o.width) || !(16..=8192).contains(&o.height) {
        return Err("render size must be 16…8192 px".into());
    }
    let (w, h) = (o.width as f64, o.height as f64);
    let mut c = Canvas::new(o.width, o.height);
    let eye = DVec3::from(o.view).try_normalize().ok_or("the view direction must be nonzero")?;
    // A camera frame with Z up (Y up when looking straight down or up).
    let up = if eye.z.abs() > 0.99 { DVec3::Y } else { DVec3::Z };
    let right = up.cross(eye).normalize();
    let cam_up = eye.cross(right).normalize();
    let inside = |p: DVec3| o.section.is_none_or(|(axis, v)| p[axis] <= v + 1e-9);
    // Bounds of what is framed.
    let framed: Vec<&Body> = if o.focus.is_empty() { bodies.iter().collect() } else { bodies.iter().filter(|b| o.focus.contains(&b.id)).collect() };
    let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
    for b in &framed {
        for v in &b.vertices {
            let p = DVec3::from(*v);
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    if !lo.x.is_finite() {
        return Err("nothing to render: no bodies have geometry".into());
    }
    let center = (lo + hi) * 0.5;
    // Fit the bounding box's corners to the image, orthographically.
    let corners = (0..8).map(|i| DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z }));
    let (mut ex, mut ey) = (1e-9f64, 1e-9f64);
    for p in corners {
        let d = p - center;
        ex = ex.max(d.dot(right).abs());
        ey = ey.max(d.dot(cam_up).abs());
    }
    let top = if o.title.is_some() { 40. } else { 0. };
    let scale = ((w * 0.45) / ex).min(((h - top) * 0.45) / ey);
    let project = |p: DVec3| -> [f32; 3] {
        let d = p - center;
        [(w * 0.5 + d.dot(right) * scale) as f32, ((h + top) * 0.5 - d.dot(cam_up) * scale) as f32, d.dot(eye) as f32]
    };
    let mut z = vec![f32::NEG_INFINITY; (o.width * o.height) as usize];
    let light1 = (eye + cam_up * 0.6 + right * 0.4).normalize();
    let light2 = (eye - right * 0.7).normalize();
    for b in bodies {
        let lit = o.highlight.contains(&b.id);
        let base = if lit { [1.0, 0.62, 0.15] } else { b.color };
        for t in &b.triangles {
            let p: Vec<DVec3> = t.iter().map(|i| DVec3::from(b.vertices[*i as usize])).collect();
            if !p.iter().all(|q| inside(*q)) {
                continue;
            }
            let n = (p[1] - p[0]).cross(p[2] - p[0]).try_normalize().unwrap_or(DVec3::Z);
            // Two-sided: a back face is lit by its flipped normal.
            let n = if n.dot(eye) < 0. { -n } else { n };
            let shade = (0.25 + 0.55 * n.dot(light1).max(0.) + 0.25 * n.dot(light2).max(0.)).min(1.) as f32;
            let color = rgb(base.map(|v| v * shade));
            let q = [project(p[0]), project(p[1]), project(p[2])];
            match o.mode.as_str() {
                "wireframe" => {}
                "xray" => fill(&mut c, None, q, color, 0.18),
                _ => fill(&mut c, Some(&mut z), q, color, 1.0),
            }
        }
    }
    // Lines: wireframe draws every triangle edge; edges draw face boundaries.
    for b in bodies {
        let lit = o.highlight.contains(&b.id);
        let ink = if lit { [200, 90, 10] } else { [40, 52, 66] };
        let mut boundary: std::collections::HashMap<(u32, u32), (u32, usize)> = std::collections::HashMap::new();
        for (k, t) in b.triangles.iter().enumerate() {
            let f = b.triangle_face.get(k).copied().unwrap_or(0);
            for (a, bb) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let key = (a.min(bb), a.max(bb));
                let e = boundary.entry(key).or_insert((f, 0));
                if e.0 != f {
                    e.0 = u32::MAX;
                }
                e.1 += 1;
            }
        }
        for ((a, bb), (face, count)) in boundary {
            let feature = face == u32::MAX || count == 1;
            if !(o.mode == "wireframe" || (o.edges && feature)) {
                continue;
            }
            let (pa, pb) = (DVec3::from(b.vertices[a as usize]), DVec3::from(b.vertices[bb as usize]));
            if !(inside(pa) && inside(pb)) {
                continue;
            }
            let (qa, qb) = (project(pa), project(pb));
            line_depth(&mut c, &z, qa, qb, ink, o.mode == "shaded" || o.mode.is_empty());
        }
    }
    if o.labels {
        for b in &framed {
            let (mut s, mut n) = (DVec3::ZERO, 0.);
            for v in &b.vertices {
                s += DVec3::from(*v);
                n += 1.;
            }
            if n > 0. {
                let p = project(s / n);
                c.dot(p[0], p[1], 3., INK);
                c.text(p[0] + 6., p[1] - 8., 13., &b.name, INK, 220.);
            }
        }
    }
    if let Some(t) = &o.title {
        c.text(16., 10., 18., t, INK, w as f32 - 32.);
        if let Some((axis, v)) = o.section {
            c.text(16., h as f32 - 22., 12., &format!("Section {}:{v} mm (kept below)", ["x", "y", "z"][axis]), MUTED, w as f32 - 32.);
        }
    }
    c.png()
}

fn fill(c: &mut Canvas, mut z: Option<&mut [f32]>, p: [[f32; 3]; 3], color: [u8; 3], alpha: f32) {
    let edge = |a: [f32; 3], b: [f32; 3], x: f32, y: f32| (x - a[0]) * (b[1] - a[1]) - (y - a[1]) * (b[0] - a[0]);
    let area = edge(p[0], p[1], p[2][0], p[2][1]);
    if area.abs() < 1e-7 {
        return;
    }
    let (wi, hi) = (c.image.width() as f32, c.image.height() as f32);
    let xmin = p.iter().map(|v| v[0]).fold(f32::INFINITY, f32::min).floor().max(0.) as u32;
    let xmax = p.iter().map(|v| v[0]).fold(f32::NEG_INFINITY, f32::max).ceil().min(wi - 1.).max(0.) as u32;
    let ymin = p.iter().map(|v| v[1]).fold(f32::INFINITY, f32::min).floor().max(0.) as u32;
    let ymax = p.iter().map(|v| v[1]).fold(f32::NEG_INFINITY, f32::max).ceil().min(hi - 1.).max(0.) as u32;
    let width = c.image.width();
    for y in ymin..=ymax {
        for x in xmin..=xmax {
            let a = edge(p[1], p[2], x as f32 + 0.5, y as f32 + 0.5) / area;
            let b = edge(p[2], p[0], x as f32 + 0.5, y as f32 + 0.5) / area;
            let d = 1. - a - b;
            if a >= -1e-5 && b >= -1e-5 && d >= -1e-5 {
                let depth = p[0][2] * a + p[1][2] * b + p[2][2] * d;
                match z.as_deref_mut() {
                    Some(z) => {
                        let i = (y * width + x) as usize;
                        if depth > z[i] {
                            z[i] = depth;
                            c.blend(x as i32, y as i32, color, alpha);
                        }
                    }
                    None => c.blend(x as i32, y as i32, color, alpha),
                }
            }
        }
    }
}

/// A line drawn where it is not hidden behind a surface (when `occlude`).
fn line_depth(c: &mut Canvas, z: &[f32], a: [f32; 3], b: [f32; 3], color: [u8; 3], occlude: bool) {
    let n = (b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil().min(10000.) as usize;
    let width = c.image.width();
    for i in 0..=n {
        let t = i as f32 / n.max(1) as f32;
        let (x, y, d) = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t);
        if x < 0. || y < 0. || x >= width as f32 || y >= c.image.height() as f32 {
            continue;
        }
        let idx = (y as u32 * width + x as u32) as usize;
        if occlude && z[idx].is_finite() && d < z[idx] - 0.5 {
            continue;
        }
        c.blend(x as i32, y as i32, color, 0.9);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cube_renders_deterministically() {
        let v = vec![[0., 0., 0.], [10., 0., 0.], [10., 10., 0.], [0., 10., 0.], [0., 0., 10.], [10., 0., 10.], [10., 10., 10.], [0., 10., 10.]];
        let t = vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
        let f = vec![0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5];
        let body = Body { id: "c".into(), name: "Cube".into(), vertices: v, triangles: t, triangle_face: f, color: [0.5, 0.6, 0.7] };
        let o = Options { width: 200, height: 150, view: preset("iso").unwrap(), mode: "shaded".into(), section: Some((2, 5.)), highlight: vec![], labels: true, edges: true, focus: vec![], title: Some("Cube".into()) };
        let a = render(std::slice::from_ref(&body), &o).unwrap();
        let b = render(&[body], &o).unwrap();
        assert_eq!(a, b);
        assert_eq!(&a[1..4], b"PNG");
    }
}
