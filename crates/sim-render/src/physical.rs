use crate::{Rendered, Size, canvas::*};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sim_inspect::spatial::SpatialShape;
use std::collections::BTreeSet;
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    #[default]
    Current,
    Isometric,
    Front,
    Top,
    Right,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Z,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub axis: Axis,
    pub offset: f32,
    #[serde(default)]
    pub keep_positive: bool,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub size: Size,
    pub view: View,
    pub section: Option<Section>,
    pub parts: BTreeSet<String>,
    pub include_hidden: bool,
    pub exploded: Option<bool>,
    pub connections: Option<bool>,
}
#[derive(Clone)]
pub struct Part {
    pub id: String,
    pub component: String,
    pub label: String,
    pub shape: SpatialShape,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub color: [f32; 3],
    pub selected: bool,
}
#[derive(Clone)]
pub struct Snapshot {
    pub parts: Vec<Part>,
    pub regions: Vec<crate::Region>,
    pub connections: Vec<([f32; 3], [f32; 3])>,
    pub yaw: f32,
    pub pitch: f32,
    pub metadata: serde_json::Value,
}
fn mesh(shape: &SpatialShape) -> Vec<[Vec3; 3]> {
    let mut triangles = Vec::new();
    match shape {
        SpatialShape::Box { size } => {
            let h = Vec3::from_array(*size) * 0.5;
            let v: Vec<_> = (0..8)
                .map(|i| {
                    Vec3::new(
                        if i & 1 == 0 { -h.x } else { h.x },
                        if i & 2 == 0 { -h.y } else { h.y },
                        if i & 4 == 0 { -h.z } else { h.z },
                    )
                })
                .collect();
            for [a, b, c, d] in [
                [0, 1, 3, 2],
                [4, 6, 7, 5],
                [0, 4, 5, 1],
                [2, 3, 7, 6],
                [0, 2, 6, 4],
                [1, 5, 7, 3],
            ] {
                triangles.push([v[a], v[b], v[c]]);
                triangles.push([v[a], v[c], v[d]]);
            }
        }
        SpatialShape::Cylinder { radius, length } => {
            for i in 0..40 {
                let a = i as f32 / 40. * std::f32::consts::TAU;
                let b = (i + 1) as f32 / 40. * std::f32::consts::TAU;
                let p = Vec3::new(a.cos() * radius, -length * 0.5, a.sin() * radius);
                let q = Vec3::new(b.cos() * radius, -length * 0.5, b.sin() * radius);
                let r = q + Vec3::Y * *length;
                let s = p + Vec3::Y * *length;
                triangles.extend([
                    [p, q, r],
                    [p, r, s],
                    [Vec3::new(0., -length * 0.5, 0.), q, p],
                    [Vec3::new(0., length * 0.5, 0.), s, r],
                ]);
            }
        }
        SpatialShape::Sphere { radius } => {
            let at = |i: usize, j: usize| {
                let theta = i as f32 / 32. * std::f32::consts::TAU;
                let phi = j as f32 / 16. * std::f32::consts::PI;
                Vec3::new(theta.cos() * phi.sin(), phi.cos(), theta.sin() * phi.sin()) * *radius
            };
            for i in 0..32 {
                for j in 0..16 {
                    let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                    triangles.extend([[a, b, c], [a, c, d]]);
                }
            }
        }
    }
    triangles
}
fn axis(section: Section) -> Vec3 {
    match section.axis {
        Axis::X => Vec3::X,
        Axis::Y => Vec3::Y,
        Axis::Z => Vec3::Z,
    }
}
/// Clip convex triangles and retain true plane intersections for a section cap.
fn clip(triangle: [Vec3; 3], section: Section, intersections: &mut Vec<Vec3>) -> Vec<Vec3> {
    let n = axis(section) * if section.keep_positive { 1. } else { -1. };
    let offset = section.offset * if section.keep_positive { 1. } else { -1. };
    let mut polygon = Vec::new();
    for i in 0..3 {
        let a = triangle[i];
        let b = triangle[(i + 1) % 3];
        let da = n.dot(a) - offset;
        let db = n.dot(b) - offset;
        if da >= 0. {
            polygon.push(a);
        }
        if (da >= 0.) != (db >= 0.) {
            let p = a + (b - a) * (da / (da - db));
            polygon.push(p);
            if !intersections.iter().any(|q| q.distance_squared(p) < 1e-14) {
                intersections.push(p);
            }
        }
    }
    polygon
}
fn raster(c: &mut Canvas, z: &mut [f32], p: [[f32; 3]; 3], color: [u8; 3]) {
    let edge = |a: [f32; 3], b: [f32; 3], x: f32, y: f32| {
        (x - a[0]) * (b[1] - a[1]) - (y - a[1]) * (b[0] - a[0])
    };
    let area = edge(p[0], p[1], p[2][0], p[2][1]);
    if area.abs() < 1e-7 {
        return;
    }
    let xmin = p
        .iter()
        .map(|v| v[0])
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.) as u32;
    let xmax = p
        .iter()
        .map(|v| v[0])
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(c.image.width() as f32 - 1.) as u32;
    let ymin = p
        .iter()
        .map(|v| v[1])
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(85.) as u32;
    let ymax = p
        .iter()
        .map(|v| v[1])
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(c.image.height() as f32 - 32.) as u32;
    for y in ymin..=ymax {
        for x in xmin..=xmax {
            let a = edge(p[1], p[2], x as f32 + 0.5, y as f32 + 0.5) / area;
            let b = edge(p[2], p[0], x as f32 + 0.5, y as f32 + 0.5) / area;
            let d = 1. - a - b;
            if a >= -1e-5 && b >= -1e-5 && d >= -1e-5 {
                let depth = p[0][2] * a + p[1][2] * b + p[2][2] * d;
                let i = (y * c.image.width() + x) as usize;
                if depth > z[i] {
                    z[i] = depth;
                    c.blend(x as i32, y as i32, color, 1.);
                }
            }
        }
    }
}
pub fn render(snapshot: &Snapshot, options: &Options) -> Result<Rendered, String> {
    let size = options.size.validate()?;
    if snapshot.parts.is_empty() {
        return Err("no visible parts in image selection".into());
    }
    if options.section.is_some_and(|s| !s.offset.is_finite()) {
        return Err("section offset must be finite meters".into());
    }
    let (yaw, pitch) = match options.view {
        View::Current => (snapshot.yaw, snapshot.pitch),
        View::Isometric => (0.72, 0.5),
        View::Front => (0., 0.),
        View::Top => (0., std::f32::consts::FRAC_PI_2),
        View::Right => (std::f32::consts::FRAC_PI_2, 0.),
    };
    let toward = Vec3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    );
    let right = Vec3::new(yaw.cos(), 0., -yaw.sin());
    let up = toward.cross(right).normalize();
    let project = |v: Vec3| Vec3::new(v.dot(right), v.dot(up), v.dot(toward));
    let mut all = Vec::new();
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    let mut caps = 0;
    for part in &snapshot.parts {
        let q = Quat::from_array(part.rotation);
        let t = Vec3::from_array(part.position);
        let mut intersections = Vec::new();
        for triangle in mesh(&part.shape) {
            let world = triangle.map(|v| t + q * v);
            for v in world {
                let p = project(v);
                lo = lo.min(p);
                hi = hi.max(p);
            }
            let polygon = if let Some(section) = options.section {
                clip(world, section, &mut intersections)
            } else {
                world.to_vec()
            };
            for i in 1..polygon.len().saturating_sub(1) {
                all.push((
                    [polygon[0], polygon[i], polygon[i + 1]],
                    part.color,
                    part.selected,
                ));
            }
        }
        if let Some(section) = options.section.filter(|_| intersections.len() >= 3) {
            let center = intersections.iter().copied().sum::<Vec3>() / intersections.len() as f32;
            let n = axis(section);
            let u = if n.x.abs() < 0.9 {
                n.cross(Vec3::X).normalize()
            } else {
                n.cross(Vec3::Y).normalize()
            };
            let v = n.cross(u);
            intersections.sort_by(|a, b| {
                let a = *a - center;
                let b = *b - center;
                a.dot(v)
                    .atan2(a.dot(u))
                    .total_cmp(&b.dot(v).atan2(b.dot(u)))
            });
            for i in 0..intersections.len() {
                all.push((
                    [
                        center,
                        intersections[i],
                        intersections[(i + 1) % intersections.len()],
                    ],
                    [0.94, 0.57, 0.24],
                    true,
                ));
            }
            caps += 1;
        }
    }
    let legend = if size.width >= 900 { 250. } else { 0. };
    let available = size.width as f32 - legend - 64.;
    let scale = (available / (hi.x - lo.x).max(1e-5))
        .min((size.height as f32 - 180.) / (hi.y - lo.y).max(1e-5))
        * 0.9;
    let center = (lo + hi) * 0.5;
    let xy = |v: Vec3| {
        let p = project(v);
        [
            (size.width as f32 - legend) * 0.5 + (p.x - center.x) * scale,
            94. + (size.height as f32 - 140.) * 0.5 - (p.y - center.y) * scale,
            p.z,
        ]
    };
    let mut c = Canvas::new(size.width, size.height);
    let mut z = vec![f32::NEG_INFINITY; (size.width * size.height) as usize];
    let light = Vec3::new(-0.3, 0.7, 0.6).normalize();
    for (triangle, color, selected) in all {
        let n = (triangle[1] - triangle[0])
            .cross(triangle[2] - triangle[0])
            .normalize_or_zero();
        let shade = 0.5 + 0.5 * n.dot(light).abs();
        let rgb = color.map(|v| (v * shade * 255.).clamp(0., 255.) as u8);
        raster(&mut c, &mut z, triangle.map(xy), rgb);
        let _ = selected;
    }
    for (a, b) in &snapshot.connections {
        let a = xy(Vec3::from_array(*a));
        let b = xy(Vec3::from_array(*b));
        c.line([a[0], a[1]], [b[0], b[1]], 1.6, TEAL);
    }
    // Project each group from the same world poses used to render its parts.
    let mut regions = snapshot.regions.clone();
    let selected = snapshot
        .parts
        .iter()
        .filter(|p| p.selected)
        .map(|p| p.component.clone())
        .collect::<BTreeSet<_>>();
    if !selected.is_empty() && !regions.iter().any(|r| r.components == selected) {
        regions.push(crate::Region {
            label: "Selection / emphasis".into(),
            color: [185, 122, 37],
            components: selected,
        });
    }
    for (index, region) in regions.iter().enumerate() {
        let mut a = [f32::INFINITY; 2];
        let mut b = [f32::NEG_INFINITY; 2];
        for part in snapshot
            .parts
            .iter()
            .filter(|p| region.components.contains(&p.component))
        {
            let rotation = Quat::from_array(part.rotation);
            let position = Vec3::from_array(part.position);
            for p in mesh(&part.shape).into_iter().flatten() {
                let q = xy(rotation * p + position);
                for i in 0..2 {
                    a[i] = a[i].min(q[i] - 8.);
                    b[i] = b[i].max(q[i] + 8.);
                }
            }
        }
        if a[0].is_finite() {
            for (p, q) in [
                ([a[0], a[1]], [b[0], a[1]]),
                ([b[0], a[1]], [b[0], b[1]]),
                ([b[0], b[1]], [a[0], b[1]]),
                ([a[0], b[1]], [a[0], a[1]]),
            ] {
                c.line(p, q, 1.6, region.color);
            }
            let y = (a[1] - 23. - index as f32 * 19.).max(94.);
            c.text(
                a[0] + 5.,
                y,
                16.,
                &region.label,
                region.color,
                (b[0] - a[0]).max(150.),
            );
        }
    }
    let section = options.section.map(|s| {
        format!(
            "{} = {:.4} m; keep {}",
            match s.axis {
                Axis::X => "x",
                Axis::Y => "y",
                Axis::Z => "z",
            },
            s.offset,
            if s.keep_positive {
                "positive"
            } else {
                "negative"
            }
        )
    });
    c.header(
        "Physical assembly",
        &section
            .as_ref()
            .map(|s| format!("Section {s} · orange surfaces mark cut display geometry"))
            .unwrap_or(
                "Source-bound display geometry · current measured pose and temperature colors"
                    .into(),
            ),
    );
    if legend > 0. {
        let x = size.width as f32 - legend;
        c.rect(x, 84., legend, size.height as f32 - 84., [255, 255, 255]);
        c.text(x + 18., 104., 18., "Visible components", INK, legend - 32.);
        let mut seen = BTreeSet::new();
        let mut row = 0;
        for part in &snapshot.parts {
            if seen.insert(&part.component) {
                let y = 138. + row as f32 * 43.;
                if y > size.height as f32 - 90. {
                    break;
                }
                c.rect(
                    x + 18.,
                    y + 3.,
                    12.,
                    12.,
                    part.color.map(|v| (v * 255.) as u8),
                );
                c.text(
                    x + 40.,
                    y,
                    15.,
                    &part.label,
                    if part.selected { TEAL } else { INK },
                    legend - 54.,
                );
                row += 1;
            }
        }
        c.text(
            x + 18.,
            size.height as f32 - 79.,
            13.,
            "Illustrative geometry",
            MUTED,
            legend - 32.,
        );
        c.text(
            x + 18.,
            size.height as f32 - 57.,
            13.,
            "Not a CAD solid section",
            MUTED,
            legend - 32.,
        );
    }
    let origin = [50., size.height as f32 - 64.];
    for (v, label, color) in [
        (Vec3::X, "X", [196, 62, 65]),
        (Vec3::Y, "Y", [50, 145, 77]),
        (Vec3::Z, "Z", [57, 109, 200]),
    ] {
        let p = project(v);
        let end = [origin[0] + p.x * 30., origin[1] - p.y * 30.];
        c.line(origin, end, 2., color);
        c.text(end[0] + 3., end[1] - 6., 13., label, color, 20.);
    }
    c.text(
        104.,
        size.height as f32 - 23.,
        12.,
        "Orthographic inspection · units: meters · section and camera do not modify the model",
        MUTED,
        size.width as f32 - 125.,
    );
    let mut metadata = snapshot.metadata.clone();
    metadata["kind"] = json!("physical");
    metadata["options"] = json!(options);
    metadata["section_caps"] = json!(caps);
    metadata["visible_parts"] = json!(snapshot.parts.iter().map(|p| &p.id).collect::<Vec<_>>());
    metadata["projection"] = json!("orthographic");
    Ok(Rendered {
        png: c.png()?,
        metadata,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn section_clips_geometry_and_keeps_vertices_on_requested_halfspace() {
        let mut points = vec![];
        let section = Section {
            axis: Axis::X,
            offset: 0.,
            keep_positive: false,
        };
        let clipped = clip(
            [
                Vec3::new(-1., 0., 0.),
                Vec3::new(1., 0., 0.),
                Vec3::new(-1., 1., 0.),
            ],
            section,
            &mut points,
        );
        assert_eq!(clipped.len(), 4);
        assert!(clipped.iter().all(|p| p.x <= 0.));
        assert_eq!(points.len(), 2);
        assert!(points.iter().all(|p| p.x == 0.));
    }
    #[test]
    fn box_section_produces_a_real_png_and_cap_metadata() {
        let snap = Snapshot {
            parts: vec![Part {
                id: "box".into(),
                component: "c".into(),
                label: "Box".into(),
                shape: SpatialShape::Box { size: [1.; 3] },
                position: [0.; 3],
                rotation: [0., 0., 0., 1.],
                color: [0.3, 0.6, 0.7],
                selected: false,
            }],
            regions: vec![],
            connections: vec![],
            yaw: 0.7,
            pitch: 0.4,
            metadata: json!({}),
        };
        let options = Options {
            section: Some(Section {
                axis: Axis::X,
                offset: 0.,
                keep_positive: false,
            }),
            size: Size {
                width: 640,
                height: 480,
            },
            ..Default::default()
        };
        let result = render(&snap, &options).unwrap();
        assert_eq!(&result.png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(result.metadata["section_caps"], 1);
        assert_eq!(image::load_from_memory(&result.png).unwrap().width(), 640);
    }
}
