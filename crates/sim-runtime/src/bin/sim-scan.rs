//! Virtual-camera scan of a turntable system (`sim.scan/1`).
//!
//!     sim-scan examples/camera-turntable/scan.json [--out DIR] [--shots N] [--width PX]
//!
//! 1. Simulates the system file on the shared runtime (same path as
//!    `sim-system run` and the viewers) to get the true disc angle and the
//!    servo encoder angle over time.
//! 2. Renders each shot from the true angle trajectory. Each image row has its
//!    own time (rolling shutter) and the exposure is averaged, so motion shows.
//! 3. Records the angle the servo would report at the shot: encoder counts ÷
//!    belt ratio, both read from the system file, plus optional labelled test errors.
//! 4. Refines those angles from the images (one angle per shot), then
//!    reconstructs depth by plane-sweep stereo with true, reported and
//!    refined poses, fuses a point cloud, and compares everything against the
//!    renderer's ground truth.
//!
//! Outputs go to DIR (default `runs/<spec stem>/scan-<unix time>`): shots/*.png,
//! contact-sheet.png, panorama.png (ground truth / reconstruction /
//! direction-only stitch), cloud.ply, colmap/ (refined poses), report.json.
use serde::Deserialize;
use serde_json::json;
use sim_runtime::system_builder as builder;
use sim_vision::camera::CameraModel;
use sim_vision::mvs::{MvsSettings, View, depth_map};
use sim_vision::output::{Point, points, splat_panorama, write_colmap, write_png, write_ply};
use sim_vision::refine::{RefineSettings, refine_angles};
use sim_vision::render::{RenderSettings, gray_from_rgb8, panorama, render};
use sim_vision::rig::{Rig, Station};
use sim_vision::scene::Scene;
use sim_vision::{Gray, Pose, dot, norm, sub};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    schema: String,
    #[serde(default)]
    description: String,
    system: String,
    rig: String,
    scene: String,
    camera: CameraSpec,
    true_angle: String,
    reported: Reported,
    schedule_from: String,
    shots: Shots,
    render: RenderSettings,
    reconstruct: Reconstruct,
    test_errors: Option<TestErrors>,
    /// Where the rig really stands in the scene (simulation truth).
    #[serde(default)]
    station: Station,
    /// Where a person measured it to be (tape measure): the starting guess
    /// for registering this scan to others. Defaults to the truth.
    #[serde(default)]
    station_guess: Option<Station>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CameraSpec {
    preset: String,
    width: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reported {
    observable: String,
    ratio_from: Ratio,
    quantum_from: ParameterRef,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ratio {
    instance: String,
    numerator: String,
    denominator: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParameterRef {
    instance: String,
    parameter: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Shots {
    count: usize,
    /// `dwell`: shoot `delay_after_move_s` after each move ends; `moving`: mid-move.
    mode: String,
    delay_after_move_s: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reconstruct {
    image_width: usize,
    neighbors_deg: f64,
    /// Plane-sweep settings; the library defaults when absent.
    #[serde(default)]
    mvs: Option<MvsSettings>,
}
/// Deliberate errors added to the reported angles, to test refinement. Labelled in the report.
#[derive(Deserialize, serde::Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct TestErrors {
    random_sigma_deg: f64,
    #[serde(default)]
    sinusoid_amplitude_deg: f64,
    #[serde(default)]
    seed: u64,
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// A parameter of a top-level instance: its bound value, else the registry default.
fn parameter(doc: &sim_system::SystemDocument, registry: &sim_core::BehaviorRegistry, instance: &str, name: &str) -> Result<f64, String> {
    let root = doc.definitions.get(&doc.root).ok_or("system has no root definition")?;
    let spec = root.instances.get(instance).ok_or_else(|| format!("system has no instance `{instance}`"))?;
    if let Some(sim_system::ParameterBinding::Value { value, .. }) = spec.parameters.get(name) {
        return Ok(*value);
    }
    let sim_system::InstanceKind::Element { component_type } = &spec.kind else { return Err(format!("`{instance}.{name}` is not bound and `{instance}` is a subsystem")) };
    let descriptor = registry.get(&component_type.as_str().into()).map_err(|e| e.to_string())?;
    descriptor
        .parameters
        .as_ref()
        .and_then(|ps| ps.iter().find(|p| p.name == name))
        .and_then(|p| p.default)
        .ok_or_else(|| format!("`{instance}.{name}` is neither bound nor defaulted"))
}

struct Track {
    times: Vec<f64>,
    values: Vec<f64>,
}
impl Track {
    fn at(&self, t: f64) -> f64 {
        let i = self.times.partition_point(|x| *x < t);
        if i == 0 {
            return self.values[0];
        }
        if i >= self.times.len() {
            return *self.values.last().unwrap();
        }
        let (t0, t1) = (self.times[i - 1], self.times[i]);
        let k = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
        self.values[i - 1] + k * (self.values[i] - self.values[i - 1])
    }
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn wrap(a: f64) -> f64 {
    (a + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI) - std::f64::consts::PI
}

/// Deterministic standard normal samples (Box–Muller on a 64-bit LCG).
fn normals(seed: u64, n: usize) -> Vec<f64> {
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let mut uniform = || {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((s >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    };
    (0..n).map(|_| (-2.0 * uniform().ln()).sqrt() * (2.0 * std::f64::consts::PI * uniform()).cos()).collect()
}

fn main() {
    if let Err(e) = run() {
        eprintln!("sim-scan: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let spec_path = PathBuf::from(args.get(1).ok_or("usage: sim-scan SPEC.json [--out DIR] [--shots N] [--width PX]")?);
    let spec_text = std::fs::read_to_string(&spec_path).map_err(|e| format!("{}: {e}", spec_path.display()))?;
    let spec: Spec = serde_json::from_str(&spec_text).map_err(|e| format!("{}: {e}", spec_path.display()))?;
    if spec.schema != "sim.scan/1" {
        return Err(format!("{}: schema `{}` is not sim.scan/1", spec_path.display(), spec.schema));
    }
    let base = spec_path.parent().unwrap_or(Path::new("."));
    let read = |rel: &str| std::fs::read_to_string(base.join(rel)).map_err(|e| format!("{}: {e}", base.join(rel).display()));
    let hash = |text: &str| blake3::hash(text.as_bytes()).to_hex().to_string();
    let rig_text = read(&spec.rig)?;
    let rig: Rig = serde_json::from_str(&rig_text).map_err(|e| format!("{}: {e}", spec.rig))?;
    rig.validate().map_err(|e| format!("{}: {e}", spec.rig))?;
    let scene_text = read(&spec.scene)?;
    let scene: Scene = serde_json::from_str(&scene_text).map_err(|e| format!("{}: {e}", spec.scene))?;
    scene.validate().map_err(|e| format!("{}: {e}", spec.scene))?;
    let width = flag(&args, "--width").map(|w| w.parse::<usize>().map_err(|_| "--width must be a whole number")).transpose()?.unwrap_or(spec.camera.width);
    let camera = match spec.camera.preset.as_str() {
        "camera_module_3_wide" => CameraModel::camera_module_3_wide(width),
        other => return Err(format!("unknown camera preset `{other}` (camera_module_3_wide)")),
    };
    let count = flag(&args, "--shots").map(|n| n.parse::<usize>().map_err(|_| "--shots must be a whole number")).transpose()?.unwrap_or(spec.shots.count);

    // --- the mechanism ---------------------------------------------------------
    let registry = sim_runtime::system_registry();
    let doc = sim_system::SystemStore::new(base.join(&spec.system)).load().map_err(|e| e.to_string())?;
    let p = |i: &str, n: &str| parameter(&doc, &registry, i, n);
    let ratio = p(&spec.reported.ratio_from.instance, &spec.reported.ratio_from.numerator)? / p(&spec.reported.ratio_from.instance, &spec.reported.ratio_from.denominator)?;
    let quantum = p(&spec.reported.quantum_from.instance, &spec.reported.quantum_from.parameter)?;
    let s = &spec.schedule_from;
    let (period, move_s, begin, step) = (p(s, "period")?, p(s, "move")?, p(s, "begin")?, p(s, "step")?);
    let frame_time = camera.readout_s + camera.exposure_s;
    let shot_start: Vec<f64> = (0..count)
        .map(|k| match spec.shots.mode.as_str() {
            "dwell" => Ok(begin + k as f64 * period + move_s + spec.shots.delay_after_move_s),
            "moving" => Ok(begin + k as f64 * period + 0.5 * move_s - 0.5 * frame_time),
            other => Err(format!("shots.mode `{other}` is not dwell or moving")),
        })
        .collect::<Result<_, _>>()?;
    if spec.shots.mode == "dwell" && move_s + spec.shots.delay_after_move_s + frame_time > period {
        return Err(format!("shots.delay_after_move_s leaves no time: move {move_s} s + delay + frame {frame_time:.3} s exceeds the {period} s period"));
    }
    let duration = shot_start.last().copied().unwrap_or(0.0) + frame_time + 0.05;
    let t0 = Instant::now();
    let config = builder::config_for(&doc);
    let series = builder::simulate(&doc, &registry, duration, config, &[spec.true_angle.clone(), spec.reported.observable.clone()])?;
    let track = |name: &str| -> Result<Track, String> {
        let s = series.iter().find(|s| s.label == name).ok_or_else(|| format!("the run has no series `{name}` (have: {})", series.iter().map(|s| s.label.as_str()).collect::<Vec<_>>().join(", ")))?;
        Ok(Track { times: s.times.clone(), values: s.values.clone() })
    };
    let (truth, servo) = (track(&spec.true_angle)?, track(&spec.reported.observable)?);
    let sim_seconds = t0.elapsed().as_secs_f64();

    // --- shots ------------------------------------------------------------------
    let out = match flag(&args, "--out") {
        Some(o) => PathBuf::from(o),
        None => {
            let stem = spec_path.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "scan".into());
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            PathBuf::from("runs").join(stem).join(format!("scan-{now}"))
        }
    };
    std::fs::create_dir_all(out.join("shots")).map_err(|e| format!("{}: {e}", out.display()))?;
    let t1 = Instant::now();
    let station = spec.station;
    let pose_at = |t: f64| station.pose(&rig.pose(truth.at(t)));
    let mut rgb = Vec::with_capacity(count);
    let mut gt_depth = Vec::with_capacity(count);
    let mut true_angle = Vec::with_capacity(count);
    let mut reported = Vec::with_capacity(count);
    let mut motion = Vec::with_capacity(count);
    for (k, &start) in shot_start.iter().enumerate() {
        let frame = render(&scene, &camera, &pose_at, start, spec.render);
        let rgb8 = frame.rgb8();
        write_png(&out.join("shots").join(format!("shot_{k:02}.png")), camera.width, camera.height, &rgb8)?;
        rgb.push(rgb8);
        gt_depth.push(frame.depth);
        // The host reads the encoder at mid-frame; the servo reports whole counts.
        let t_read = start + 0.5 * frame_time;
        true_angle.push(truth.at(t_read));
        reported.push((servo.at(t_read) / quantum).round() * quantum / ratio);
        motion.push((truth.at(start + frame_time) - truth.at(start)).abs());
        eprint!("\rrendered {}/{count}", k + 1);
    }
    eprintln!();
    let injected: Vec<f64> = match &spec.test_errors {
        Some(t) => {
            let z = normals(t.seed, count);
            (0..count).map(|k| (t.random_sigma_deg * z[k] + t.sinusoid_amplitude_deg * (true_angle[k]).sin()).to_radians()).collect()
        }
        None => vec![0.0; count],
    };
    let reported: Vec<f64> = reported.iter().zip(&injected).map(|(r, e)| r + e).collect();
    let render_seconds = t1.elapsed().as_secs_f64();

    // --- refinement ----------------------------------------------------------------
    let t2 = Instant::now();
    let mut grays: Vec<Gray> = rgb.iter().map(|c| gray_from_rgb8(camera.width, camera.height, c)).collect();
    while grays[0].width >= 2 * spec.reconstruct.image_width {
        grays = grays.iter().map(Gray::half).collect();
    }
    let rcam = camera.scaled(grays[0].width);
    let refined = refine_angles(&grays, &reported, &rcam, &rig, RefineSettings { neighbor_deg: spec.reconstruct.neighbors_deg, ..RefineSettings::default() });
    let refined_angle: Vec<f64> = reported.iter().zip(&refined.corrections).map(|(a, c)| a + c).collect();
    let refine_seconds = t2.elapsed().as_secs_f64();

    // Angle errors relative to truth, with the unobservable common offset removed for the refined set.
    let deg = |v: f64| v.to_degrees();
    let offset = |angles: &[f64]| angles.iter().zip(&true_angle).map(|(a, t)| wrap(a - t)).sum::<f64>() / count as f64;
    let (off_rep, off_ref) = (offset(&reported), offset(&refined_angle));
    let err_rep: Vec<f64> = reported.iter().zip(&true_angle).map(|(a, t)| deg(wrap(a - t))).collect();
    let err_ref: Vec<f64> = refined_angle.iter().zip(&true_angle).map(|(a, t)| deg(wrap(a - t - off_ref))).collect();
    let rms = |v: &[f64]| (v.iter().map(|x| x * x).sum::<f64>() / v.len().max(1) as f64).sqrt();
    let step_err = |v: &[f64]| (0..v.len()).map(|i| (v[(i + 1) % v.len()] - v[i]).abs()).fold(0.0f64, f64::max);

    // --- reconstruction ---------------------------------------------------------------
    let t3 = Instant::now();
    let mvs = spec.reconstruct.mvs.unwrap_or_default();
    let neighbors = |angles: &[f64], i: usize| -> Vec<usize> { (0..count).filter(|&j| j != i && wrap(angles[j] - angles[i]).abs().to_degrees() <= spec.reconstruct.neighbors_deg).collect() };
    let scale_to_full = camera.width as f64 / rcam.width as f64;
    let gt_at = |i: usize, x: usize, y: usize| -> f64 {
        let (fx, fy) = (((x as f64 + 0.5) * scale_to_full) as usize, ((y as f64 + 0.5) * scale_to_full) as usize);
        gt_depth[i][fy.min(camera.height - 1) * camera.width + fx.min(camera.width - 1)] as f64
    };
    let mut variants = serde_json::Map::new();
    let mut refined_maps = Vec::new();
    let mut reported_maps = Vec::new();
    for (name, angles) in [("true poses (oracle)", &true_angle), ("reported (servo)", &reported), ("refined", &refined_angle)] {
        if name == "refined" && !refined.accepted {
            // Refinement kept the servo angles: the maps are the reported ones.
            variants.insert(name.into(), variants["reported (servo)"].clone());
            refined_maps = std::mem::take(&mut reported_maps);
            continue;
        }
        let poses: Vec<Pose> = angles.iter().map(|a| rig.pose(*a)).collect();
        let mut rel = Vec::new();
        let (mut accepted, mut samples) = (0usize, 0usize);
        let mut maps = Vec::new();
        for i in 0..count {
            let reference = View { image: &grays[i], camera: &rcam, pose: poses[i] };
            let views: Vec<View> = neighbors(angles, i).iter().map(|&j| View { image: &grays[j], camera: &rcam, pose: poses[j] }).collect();
            let map = depth_map(&reference, &views, mvs);
            samples += map.rows * map.columns;
            for jj in 0..map.rows {
                for ii in 0..map.columns {
                    let z = map.depth[jj * map.columns + ii];
                    if z.is_finite() {
                        accepted += 1;
                        let (x, y) = map.pixel(ii, jj);
                        let g = gt_at(i, x, y);
                        if g.is_finite() {
                            rel.push((z as f64 - g).abs() / g);
                        }
                    }
                }
            }
            maps.push(map);
            eprint!("\rdepth maps ({name}) {}/{count}", i + 1);
        }
        eprintln!();
        let within = rel.iter().filter(|e| **e < 0.05).count() as f64 / rel.len().max(1) as f64;
        variants.insert(name.into(), json!({
            "completeness": accepted as f64 / samples.max(1) as f64,
            "median_relative_depth_error": percentile(&mut rel.clone(), 0.5),
            "p90_relative_depth_error": percentile(&mut rel, 0.9),
            "share_within_5_percent": within,
        }));
        if name == "refined" {
            refined_maps = maps;
        } else if name == "reported (servo)" {
            reported_maps = maps;
        }
    }
    // Fuse: keep a point only if another shot's depth map agrees within 3 %.
    let poses: Vec<Pose> = refined_angle.iter().map(|a| rig.pose(*a)).collect();
    let mut cloud: Vec<Point> = Vec::new();
    let mut point_error = Vec::new();
    for i in 0..count {
        let mut rgb_small = vec![0u8; 3 * rcam.width * rcam.height];
        for y in 0..rcam.height {
            for x in 0..rcam.width {
                let (fx, fy) = (((x as f64 + 0.5) * scale_to_full) as usize, ((y as f64 + 0.5) * scale_to_full) as usize);
                let k = 3 * (fy.min(camera.height - 1) * camera.width + fx.min(camera.width - 1));
                rgb_small[3 * (y * rcam.width + x)..3 * (y * rcam.width + x) + 3].copy_from_slice(&rgb[i][k..k + 3]);
            }
        }
        for pt in points(&refined_maps[i], &rcam, &poses[i], &rgb_small, i) {
            let agrees = neighbors(&refined_angle, i).iter().any(|&j| {
                let q = poses[j].to_camera(pt.position);
                let Some((u, v)) = rcam.project(q) else { return false };
                let map = &refined_maps[j];
                let (ii, jj) = ((u / map.step as f64) as usize, (v / map.step as f64) as usize);
                ii < map.columns && jj < map.rows && {
                    let z = map.depth[jj * map.columns + ii] as f64;
                    z.is_finite() && (z - q[2]).abs() / q[2] < 0.03
                }
            });
            if agrees {
                // Distance to the true surface along the viewing ray.
                let c = station.point(poses[i].center);
                let d = sub(station.point(pt.position), c);
                let range = norm(d);
                let dir = sim_vision::scale(d, 1.0 / range);
                if let Some(hit) = scene.hit(c, dir) {
                    point_error.push((range - hit.t).abs());
                }
                cloud.push(pt);
            }
        }
    }
    write_ply(&out.join("cloud.ply"), &cloud)?;
    // What the place builder needs: final poses (station frame) and depth maps.
    std::fs::create_dir_all(out.join("depth")).map_err(|e| e.to_string())?;
    let mut shots_out = Vec::new();
    for (k, map) in refined_maps.iter().enumerate() {
        let name = format!("depth/shot_{k:02}.f32");
        std::fs::write(out.join(&name), map.depth.iter().flat_map(|d| d.to_le_bytes()).collect::<Vec<u8>>()).map_err(|e| e.to_string())?;
        shots_out.push(json!({"image": format!("shots/shot_{k:02}.png"), "depth": name, "angle": refined_angle[k], "pose": {"center": poses[k].center, "rotation": poses[k].r}}));
    }
    let first = &refined_maps[0];
    std::fs::write(out.join("poses.json"), serde_json::to_string_pretty(&json!({
        "schema": "sim.scan-poses/1",
        "frame": "station: turntable base at the origin, metres, +Z up",
        "camera": camera,
        "depth_camera": rcam,
        "depth_grid": {"step": first.step, "columns": first.columns, "rows": first.rows, "format": "f32 little-endian, row-major, NaN = no depth (camera-frame z, m)"},
        "rig": rig,
        "station_guess": spec.station_guess.unwrap_or(station),
        "simulation": {"station_truth": station, "scene": base.join(&spec.scene).display().to_string()},
        "shots": shots_out,
    })).unwrap()).map_err(|e| e.to_string())?;
    let recon_seconds = t3.elapsed().as_secs_f64();

    // --- panoramas: truth, reconstruction, direction-only stitch ------------------------------
    let center = [rig.axis_point[0], rig.axis_point[1], rig.optical_center[2]];
    let pw = 1440;
    let half_h = 34.0;
    // Truth from the same spot, oriented like the station frame.
    let gt = panorama(&scene, station.point(center), pw, half_h, station.yaw_deg);
    let (ph, recon) = splat_panorama(&cloud, center, pw, half_h);
    let mut naive = vec![0u8; 3 * pw * ph];
    for y in 0..ph {
        let el = (half_h - (y as f64 + 0.5) / ph as f64 * 2.0 * half_h).to_radians();
        for x in 0..pw {
            let az = (-180.0 + (x as f64 + 0.5) / pw as f64 * 360.0).to_radians();
            let d = [el.cos() * az.cos(), el.cos() * az.sin(), el.sin()];
            // The shot looking most nearly along d; treat d as a direction at infinity.
            let i = (0..count).max_by(|a, b| dot(poses[*a].direction_to_world([0.0, 0.0, 1.0]), d).total_cmp(&dot(poses[*b].direction_to_world([0.0, 0.0, 1.0]), d))).unwrap();
            let q = sim_vision::mul(&sim_vision::transpose(&poses[i].r), d);
            if let Some((u, v)) = camera.project(q).filter(|(u, v)| camera.contains(*u, *v)) {
                let k = 3 * (v as usize * camera.width + u as usize);
                naive[3 * (y * pw + x)..3 * (y * pw + x) + 3].copy_from_slice(&rgb[i][k..k + 3]);
            }
        }
    }
    let mut stacked = gt.rgb8();
    stacked.extend(std::iter::repeat_n(40u8, 3 * pw * 6));
    stacked.extend(&recon);
    stacked.extend(std::iter::repeat_n(40u8, 3 * pw * 6));
    stacked.extend(&naive);
    write_png(&out.join("panorama.png"), pw, gt.height + ph * 2 + 12, &stacked)?;

    // Contact sheet: 6 columns of thumbnails.
    let (tw, th) = (192usize, (192.0 * camera.height as f64 / camera.width as f64).round() as usize);
    let cols = 6usize;
    let rows = count.div_ceil(cols);
    let mut sheet = vec![20u8; 3 * cols * tw * rows * th];
    for (i, img) in rgb.iter().enumerate() {
        let (ox, oy) = ((i % cols) * tw, (i / cols) * th);
        for y in 0..th {
            for x in 0..tw {
                let (sx, sy) = (x * camera.width / tw, y * camera.height / th);
                let k = 3 * (sy * camera.width + sx);
                let o = 3 * ((oy + y) * cols * tw + ox + x);
                sheet[o..o + 3].copy_from_slice(&img[k..k + 3]);
            }
        }
    }
    write_png(&out.join("contact-sheet.png"), cols * tw, rows * th, &sheet)?;
    write_colmap(&out.join("colmap"), &camera, &(0..count).map(|k| (format!("shot_{k:02}.png"), poses[k])).collect::<Vec<_>>())?;

    // --- report ------------------------------------------------------------------------
    let pixel_deg = camera.horizontal_fov_deg() / camera.width as f64;
    let report = json!({
        "schema": "sim.scan-report/1",
        "fidelity": "virtual camera: ideal pinhole (undistorted), Lambertian procedural scene, fixed light; mechanism: turntable.system.json with its labelled estimates and the multi-turn FPGA assumption. Uncalibrated: not evidence of real-camera accuracy.",
        "inputs": {
            "spec": {"path": spec_path.display().to_string(), "blake3": hash(&spec_text), "description": spec.description},
            "system": {"path": spec.system, "revision": doc.revision, "content_hash": doc.content_hash()},
            "rig": {"path": spec.rig, "blake3": hash(&rig_text), "source": rig.source},
            "scene": {"path": spec.scene, "blake3": hash(&scene_text)},
            "camera": camera,
            "render": spec.render,
            "shots": {"count": count, "mode": spec.shots.mode, "delay_after_move_s": spec.shots.delay_after_move_s, "step_deg": step.to_degrees(), "period_s": period, "move_s": move_s},
            "mvs": mvs, "reported_angle": {"belt_ratio": ratio, "encoder_quantum_rad": quantum, "rule": "round(servo angle / quantum) · quantum / ratio at mid-frame"},
            "test_errors": spec.test_errors,
        },
        "timing_s": {"simulate": sim_seconds, "render": render_seconds, "refine": refine_seconds, "reconstruct": recon_seconds},
        "angles": {
            "pixel_deg": pixel_deg,
            "reported_minus_true_deg": {"mean": off_rep.to_degrees(), "rms": rms(&err_rep), "max_abs": err_rep.iter().fold(0.0f64, |m, x| m.max(x.abs())), "max_neighbor_step": step_err(&err_rep)},
            "refined_minus_true_deg (common offset removed)": {"rms": rms(&err_ref), "max_abs": err_ref.iter().fold(0.0f64, |m, x| m.max(x.abs())), "max_neighbor_step": step_err(&err_ref)},
            "refinement": {"accepted": refined.accepted, "score_before": refined.score_before, "score_after": refined.score_after, "weights": refined.weights, "sweep_change_deg": refined.sweep_change.iter().map(|c| c.to_degrees()).collect::<Vec<_>>()},
            "per_shot": (0..count).map(|k| json!({"shot": k, "start_s": shot_start[k], "true_deg": deg(true_angle[k]), "reported_deg": deg(reported[k]), "refined_deg": deg(refined_angle[k]), "injected_test_error_deg": deg(injected[k]), "motion_during_frame_deg": deg(motion[k])})).collect::<Vec<_>>(),
        },
        "depth": variants,
        "cloud": {"points": cloud.len(), "median_error_mm": 1e3 * percentile(&mut point_error.clone(), 0.5), "p90_error_mm": 1e3 * percentile(&mut point_error, 0.9), "filter": "kept when another shot's depth map agrees within 3 %"},
        "outputs": ["shots/*.png", "contact-sheet.png", "panorama.png (truth / reconstruction / direction-only stitch)", "cloud.ply", "colmap/"],
    });
    std::fs::write(out.join("report.json"), serde_json::to_string_pretty(&report).unwrap()).map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string_pretty(&json!({"out": out.display().to_string(), "angles": report["angles"].as_object().map(|a| a.iter().filter(|(k, _)| *k != "per_shot" && *k != "refinement").map(|(k, v)| (k.clone(), v.clone())).collect::<serde_json::Map<_, _>>()), "depth": report["depth"], "cloud": report["cloud"], "timing_s": report["timing_s"]})).unwrap());
    Ok(())
}
