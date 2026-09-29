//! Files: PNG frames, PLY point clouds, panoramas splatted from points, and
//! COLMAP text models (so real scans can go through COLMAP with the servo
//! poses as priors).
use crate::camera::CameraModel;
use crate::mvs::DepthMap;
use crate::{Pose, V3, transpose};
use std::io::Write;
use std::path::Path;

pub fn write_png(path: &Path, width: usize, height: usize, rgb8: &[u8]) -> Result<(), String> {
    image::save_buffer(path, rgb8, width as u32, height as u32, image::ColorType::Rgb8).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn read_png(path: &Path) -> Result<(usize, usize, Vec<u8>), String> {
    let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgb8();
    Ok((img.width() as usize, img.height() as usize, img.into_raw()))
}

#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub position: V3,
    pub color: [u8; 3],
    /// Index of the shot it came from.
    pub shot: usize,
}

/// World points from a depth map, coloured from the reference image.
pub fn points(map: &DepthMap, camera: &CameraModel, pose: &Pose, rgb8: &[u8], shot: usize) -> Vec<Point> {
    let mut out = Vec::new();
    for j in 0..map.rows {
        for i in 0..map.columns {
            let z = map.depth[j * map.columns + i];
            if !z.is_finite() {
                continue;
            }
            let (x, y) = map.pixel(i, j);
            let ray = camera.ray(x as f64 + 0.5, y as f64 + 0.5);
            let position = pose.to_world(crate::scale(ray, z as f64));
            let k = 3 * (y * camera.width + x);
            out.push(Point { position, color: [rgb8[k], rgb8[k + 1], rgb8[k + 2]], shot });
        }
    }
    out
}

pub fn write_ply(path: &Path, points: &[Point]) -> Result<(), String> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?);
    let e = |e: std::io::Error| format!("{}: {e}", path.display());
    write!(f, "ply\nformat ascii 1.0\ncomment sim-vision reconstruction, metres, +Z up\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nend_header\n", points.len()).map_err(e)?;
    for p in points {
        writeln!(f, "{:.5} {:.5} {:.5} {} {} {}", p.position[0], p.position[1], p.position[2], p.color[0], p.color[1], p.color[2]).map_err(e)?;
    }
    Ok(())
}

/// Equirectangular panorama (same layout as [`crate::render::panorama`]) made
/// by splatting points seen from `center`; nearest point wins; holes stay black.
pub fn splat_panorama(points: &[Point], center: V3, width: usize, half_height_deg: f64) -> (usize, Vec<u8>) {
    let height = (width as f64 * (2.0 * half_height_deg) / 360.0).round() as usize;
    let mut depth = vec![f64::INFINITY; width * height];
    let mut rgb = vec![0u8; 3 * width * height];
    for p in points {
        let d = crate::sub(p.position, center);
        let r = crate::norm(d);
        let az = d[1].atan2(d[0]).to_degrees();
        let el = d[2].atan2(d[0].hypot(d[1])).to_degrees();
        let x = ((az + 180.0) / 360.0 * width as f64) as i64;
        let y = ((half_height_deg - el) / (2.0 * half_height_deg) * height as f64) as i64;
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let (xx, yy) = ((x + dx).rem_euclid(width as i64) as usize, y + dy);
            if yy < 0 || yy as usize >= height {
                continue;
            }
            let k = yy as usize * width + xx;
            if r < depth[k] {
                depth[k] = r;
                rgb[3 * k..3 * k + 3].copy_from_slice(&p.color);
            }
        }
    }
    (height, rgb)
}

fn quaternion(m: &crate::M3) -> [f64; 4] {
    let tr = m[0][0] + m[1][1] + m[2][2];
    let (w, x, y, z) = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        (0.25 * s, (m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s)
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        ((m[2][1] - m[1][2]) / s, 0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s)
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        ((m[0][2] - m[2][0]) / s, (m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s)
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        ((m[1][0] - m[0][1]) / s, (m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s)
    };
    [w, x, y, z]
}

/// COLMAP text model (cameras.txt, images.txt, empty points3D.txt): a
/// PINHOLE camera and one image per pose (world → camera, as COLMAP stores it).
pub fn write_colmap(dir: &Path, camera: &CameraModel, images: &[(String, Pose)]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let e = |e: std::io::Error| format!("{}: {e}", dir.display());
    std::fs::write(dir.join("cameras.txt"), format!("# Camera list\n1 PINHOLE {} {} {} {} {} {}\n", camera.width, camera.height, camera.fx, camera.fy, camera.cx, camera.cy)).map_err(e)?;
    let mut text = String::from("# IMAGE_ID QW QX QY QZ TX TY TZ CAMERA_ID NAME, then an empty line of 2D points\n");
    for (i, (name, pose)) in images.iter().enumerate() {
        let r = transpose(&pose.r);
        let t = crate::scale(crate::mul(&r, pose.center), -1.0);
        let q = quaternion(&r);
        text += &format!("{} {:.12} {:.12} {:.12} {:.12} {:.9} {:.9} {:.9} 1 {}\n\n", i + 1, q[0], q[1], q[2], q[3], t[0], t[1], t[2], name);
    }
    std::fs::write(dir.join("images.txt"), text).map_err(e)?;
    std::fs::write(dir.join("points3D.txt"), "# empty: triangulate with colmap point_triangulator\n").map_err(e)
}

/// PNG file bytes for an sRGB image.
pub fn png_bytes(width: usize, height: usize, rgb8: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::write_buffer_with_format(&mut out, rgb8, width as u32, height as u32, image::ColorType::Rgb8, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// Nearest-neighbour resize of an sRGB image.
pub fn resize(width: usize, height: usize, rgb8: &[u8], to_width: usize) -> (usize, usize, Vec<u8>) {
    let to_height = (height * to_width).div_ceil(width).max(1);
    let mut out = Vec::with_capacity(3 * to_width * to_height);
    for y in 0..to_height {
        for x in 0..to_width {
            let (sx, sy) = ((x * width / to_width).min(width - 1), (y * height / to_height).min(height - 1));
            out.extend_from_slice(&rgb8[3 * (sy * width + sx)..3 * (sy * width + sx) + 3]);
        }
    }
    (to_width, to_height, out)
}
