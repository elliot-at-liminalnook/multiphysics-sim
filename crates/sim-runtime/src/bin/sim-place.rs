//! Place models from turntable scans, and tools that let an AI understand a place.
//!
//!     sim-place build OUT_DIR SCAN_DIR [SCAN_DIR…] [--voxel M] [--description TEXT]
//!     sim-place tools                         tool definitions (JSON)
//!     sim-place query PLACE_DIR TOOL [ARGS_JSON] [--images DIR]
//!     sim-place mcp PLACE_DIR                 Model Context Protocol server on stdio
//!     sim-place world PLACE_DIR OUT.json      simulator world (floor + terrain heightfield)
//!
//! `build` reads each scan's poses.json and depth maps (written by sim-scan,
//! or by the same format from a real rig), registers extra stations to the
//! first (level 4-DoF ICP from the recorded guess), fuses everything into a
//! signed-distance volume, and writes place.json, the mesh (PLY and OBJ),
//! height map and photos. When the scans come from simulation it also
//! scores the place against the true scene.
//!
//! The tools are the same for the CLI and the MCP server: place_overview,
//! render_view, list_photos, get_photo, photos_of_point, raycast, measure,
//! surfaces, height_at, clearance, free_space_map, plan_path.
use serde_json::{Value, json};
use sim_vision::camera::CameraModel;
use sim_vision::fusion::Tsdf;
use sim_vision::mvs::DepthMap;
use sim_vision::place::{self, BuildSettings, InputView, MapGrid, Place, PlaceFile, PoseRecord, ViewRecord};
use sim_vision::register::{Target, register};
use sim_vision::rig::Station;
use sim_vision::scene::Scene;
use sim_vision::{Pose, V3, add, dot, norm, scale, sub, unit};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

fn main() {
    if let Err(e) = run() {
        eprintln!("sim-place: {e}");
        std::process::exit(1);
    }
}

const USAGE: &str = "usage: sim-place build OUT_DIR SCAN_DIR… [--voxel M] | tools | query PLACE_DIR TOOL [ARGS_JSON] [--images DIR] | mcp PLACE_DIR | world PLACE_DIR OUT.json";

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("build") => build(&args[2..]),
        Some("tools") => {
            println!("{}", serde_json::to_string_pretty(&tools().iter().map(|t| json!({"name": t.name, "description": t.description, "inputSchema": t.schema})).collect::<Vec<_>>()).unwrap());
            Ok(())
        }
        Some("query") => {
            let dir = PathBuf::from(args.get(2).ok_or(USAGE)?);
            let tool = args.get(3).ok_or(USAGE)?;
            let arguments: Value = match args.get(4).filter(|a| !a.starts_with("--")) {
                Some(a) => serde_json::from_str(a).map_err(|e| format!("ARGS_JSON: {e}"))?,
                None => json!({}),
            };
            let place = Place::load(&dir)?;
            let images = PathBuf::from(flag(&args, "--images").unwrap_or_else(|| ".".into()));
            for (i, c) in call(&place, tool, &arguments)?.into_iter().enumerate() {
                match c {
                    Content::Text(t) => println!("{t}"),
                    Content::Image(png) => {
                        let path = images.join(format!("{tool}-{i}.png"));
                        std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))?;
                        println!("[image written to {}]", path.display());
                    }
                }
            }
            Ok(())
        }
        Some("mcp") => mcp(&PathBuf::from(args.get(2).ok_or(USAGE)?)),
        Some("world") => world(&PathBuf::from(args.get(2).ok_or(USAGE)?), &PathBuf::from(args.get(3).ok_or(USAGE)?)),
        _ => Err(USAGE.into()),
    }
}

// ---------------------------------------------------------------- build

struct Scan {
    dir: PathBuf,
    camera: CameraModel,
    depth_camera: CameraModel,
    guess: Station,
    truth: Option<Station>,
    scene: Option<PathBuf>,
    shots: Vec<(String, Pose, DepthMap)>,
}

fn load_scan(dir: &Path) -> Result<Scan, String> {
    let path = dir.join("poses.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| format!("{}: {e}", path.display()))?;
    if v["schema"] != "sim.scan-poses/1" {
        return Err(format!("{}: not a sim.scan-poses/1 file", path.display()));
    }
    let de = |key: &str| -> Result<Value, String> { v.get(key).cloned().ok_or_else(|| format!("{}: missing `{key}`", path.display())) };
    let camera: CameraModel = serde_json::from_value(de("camera")?).map_err(|e| format!("{}: camera: {e}", path.display()))?;
    let depth_camera: CameraModel = serde_json::from_value(de("depth_camera")?).map_err(|e| format!("{}: depth_camera: {e}", path.display()))?;
    let guess: Station = serde_json::from_value(de("station_guess")?).map_err(|e| format!("{}: station_guess: {e}", path.display()))?;
    let truth = v["simulation"]["station_truth"].as_object().map(|_| serde_json::from_value(v["simulation"]["station_truth"].clone())).transpose().map_err(|e| e.to_string())?;
    let scene = v["simulation"]["scene"].as_str().map(PathBuf::from);
    let grid = &v["depth_grid"];
    let (step, columns, rows) = (grid["step"].as_u64().ok_or("depth_grid.step")? as usize, grid["columns"].as_u64().ok_or("depth_grid.columns")? as usize, grid["rows"].as_u64().ok_or("depth_grid.rows")? as usize);
    let mut shots = Vec::new();
    for (k, s) in v["shots"].as_array().ok_or("shots")?.iter().enumerate() {
        let pose: PoseRecord = serde_json::from_value(s["pose"].clone()).map_err(|e| format!("{}: shots[{k}].pose: {e}", path.display()))?;
        let depth_path = dir.join(s["depth"].as_str().ok_or("shots[].depth")?);
        let bytes = std::fs::read(&depth_path).map_err(|e| format!("{}: {e}", depth_path.display()))?;
        let depth: Vec<f32> = bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        if depth.len() != columns * rows {
            return Err(format!("{}: {} values, expected {columns}×{rows}", depth_path.display(), depth.len()));
        }
        let n = depth.len();
        shots.push((s["image"].as_str().ok_or("shots[].image")?.to_string(), pose.pose(), DepthMap { step, columns, rows, depth, score: vec![f32::NAN; n] }));
    }
    Ok(Scan { dir: dir.to_path_buf(), camera, depth_camera, guess, truth, scene, shots })
}

/// The photo resampled onto the depth camera's pixel grid.
fn colours_for(rgb: &(usize, usize, Vec<u8>), cam: &CameraModel) -> Vec<u8> {
    let (w, h, img) = rgb;
    let mut out = vec![0u8; 3 * cam.width * cam.height];
    for y in 0..cam.height {
        for x in 0..cam.width {
            let (sx, sy) = (((x as f64 + 0.5) * *w as f64 / cam.width as f64) as usize, ((y as f64 + 0.5) * *h as f64 / cam.height as f64) as usize);
            let k = 3 * (sy.min(h - 1) * w + sx.min(w - 1));
            out[3 * (y * cam.width + x)..3 * (y * cam.width + x) + 3].copy_from_slice(&img[k..k + 3]);
        }
    }
    out
}

fn surface_points(views: &[InputView], voxel: f64) -> (Vec<V3>, Vec<V3>) {
    let bounds = place::observed_bounds(views, 0.1);
    let tsdf = place::fuse(views, bounds, &BuildSettings { voxel, truncation: 3.0 * voxel, ..BuildSettings::default() });
    let mesh = tsdf.mesh();
    (mesh.vertices, mesh.normals)
}

fn percentile(mut v: Vec<f64>, p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn build(args: &[String]) -> Result<(), String> {
    let out = PathBuf::from(args.first().ok_or(USAGE)?);
    let scan_dirs: Vec<PathBuf> = args[1..].iter().take_while(|a| !a.starts_with("--")).map(PathBuf::from).collect();
    if scan_dirs.is_empty() {
        return Err(USAGE.into());
    }
    let settings = BuildSettings { voxel: flag(args, "--voxel").map(|v| v.parse::<f64>().map_err(|_| "--voxel must be a number")).transpose()?.unwrap_or(0.03), ..BuildSettings::default() };
    let settings = BuildSettings { truncation: 3.0 * settings.voxel, map_cell: (settings.voxel * 5.0 / 3.0).max(0.004), min_plane_area: if settings.voxel < 0.01 { 0.002 } else { 0.15 }, ..settings };
    let t0 = std::time::Instant::now();
    let scans: Vec<Scan> = scan_dirs.iter().map(|d| load_scan(d)).collect::<Result<_, _>>()?;
    std::fs::create_dir_all(out.join("photos")).map_err(|e| format!("{}: {e}", out.display()))?;
    // Per-scan views in each station frame.
    let mut per_scan: Vec<Vec<InputView>> = Vec::new();
    for (s, scan) in scans.iter().enumerate() {
        let mut views = Vec::new();
        // Drop stereo outliers: a depth must agree with two other views.
        let poses: Vec<Pose> = scan.shots.iter().map(|s| s.1).collect();
        let maps: Vec<DepthMap> = scan.shots.iter().map(|s| DepthMap { step: s.2.step, columns: s.2.columns, rows: s.2.rows, depth: s.2.depth.clone(), score: s.2.score.clone() }).collect();
        let filtered = sim_vision::mvs::consistency_filter(&maps, &poses, &scan.depth_camera, 35.0, 0.02, 2);
        for (k, (image, pose, _)) in scan.shots.iter().enumerate() {
            let depth = &sim_vision::mvs::fill_holes(&filtered[k], 3, 4, 0.03);
            let rgb = sim_vision::output::read_png(&scan.dir.join(image))?;
            let name = format!("photos/s{s}_{k:02}.png");
            std::fs::copy(scan.dir.join(image), out.join(&name)).map_err(|e| e.to_string())?;
            views.push(InputView { name: format!("station {s} shot {k}"), image: name, pose: *pose, station: s, depth: DepthMap { step: depth.step, columns: depth.columns, rows: depth.rows, depth: depth.depth.clone(), score: depth.score.clone() }, depth_camera: scan.depth_camera.clone(), depth_rgb8: colours_for(&rgb, &scan.depth_camera) });
        }
        per_scan.push(views);
        eprintln!("loaded scan {s}: {} shots", scan.shots.len());
    }
    // Register stations 1… to station 0.
    // Registration works at the scene's scale: rooms in centimetres, objects in millimetres.
    let feature = (settings.voxel * 10.0 / 3.0).min(0.1);
    let (tp, tn) = surface_points(&per_scan[0], (settings.voxel * 4.0 / 3.0).max(0.002));
    let target = Target::new(tp, tn, 2.0 * feature);
    let mut stations = vec![place::Station { name: scan_dirs[0].display().to_string(), placement: Station::default(), method: "first station: defines the place frame".into(), registration: Value::Null }];
    let mut registration_errors = Vec::new();
    for s in 1..scans.len() {
        let guess = scans[s].guess.then(&scans[0].guess.inverse());
        let (source, _) = surface_points(&per_scan[s], (settings.voxel * 4.0 / 3.0).max(0.002));
        let reg = register(&target, &source, guess, 3.0 * feature, 20.0, feature);
        eprintln!("registered station {s}: overlap {:.2}, rms {:.1} mm, placement {:?}", reg.overlap, 1e3 * reg.rms_m, reg.station);
        let mut record = json!({"guess": guess, "result": reg});
        if let (Some(t0s), Some(ts)) = (scans[0].truth, scans[s].truth) {
            let truth = ts.then(&t0s.inverse());
            let err = json!({"position_mm": 1e3 * norm(sub(reg.station.position, truth.position)), "yaw_deg": reg.station.yaw_deg - truth.yaw_deg, "truth": truth});
            registration_errors.push(err.clone());
            record["error_vs_truth"] = err;
        }
        stations.push(place::Station { name: scan_dirs[s].display().to_string(), placement: reg.station, method: "4-DoF point-to-plane ICP from the recorded guess".into(), registration: record });
    }
    // All views in the place frame.
    let mut views: Vec<InputView> = Vec::new();
    for (s, list) in per_scan.into_iter().enumerate() {
        for mut v in list {
            v.pose = stations[s].placement.pose(&v.pose);
            views.push(v);
        }
    }
    let bounds = place::observed_bounds(&views, 0.15);
    let tsdf = place::fuse(&views, bounds, &settings);
    eprintln!("fused {} views into {:?} voxels of {} m", views.len(), tsdf.grid.dims, settings.voxel);
    let mesh = tsdf.mesh();
    mesh.write_ply(&out.join("mesh.ply"))?;
    mesh.write_obj(&out.join("mesh.obj"), true)?;
    let planes = place::find_planes(&mesh, &settings);
    let floor_z = planes.iter().find(|p| p.kind == "floor").map(|p| p.centroid[2]);
    let ceiling_z = planes.iter().find(|p| p.kind == "ceiling").map(|p| p.centroid[2]);
    let map = MapGrid { origin: [bounds[0][0], bounds[0][1]], cell: settings.map_cell, dims: [((bounds[1][0] - bounds[0][0]) / settings.map_cell).ceil() as usize + 1, ((bounds[1][1] - bounds[0][1]) / settings.map_cell).ceil() as usize + 1] };
    let heights = place::height_map(&tsdf, &map, ceiling_z.map_or(bounds[1][2], |c| c - 0.1));
    let camera = scans[0].camera.clone();
    let view_records: Vec<ViewRecord> = views.iter().map(|v| ViewRecord { name: v.name.clone(), image: v.image.clone(), pose: PoseRecord::from(&v.pose), station: v.station }).collect();
    let quality = evaluate(&scans, &tsdf, &mesh, &views, floor_z);
    let file = PlaceFile {
        schema: place::SCHEMA.into(),
        description: flag(args, "--description").unwrap_or_else(|| format!("Place fused from {} scan station(s)", scans.len())),
        frame: "place frame = the first scan station's frame: turntable base at the origin, metres, +Z up, +X along the rig's zero angle".into(),
        volume: tsdf.grid.clone(),
        volume_file: "volume.bin".into(),
        mesh_file: "mesh.ply".into(),
        mesh_obj_file: "mesh.obj".into(),
        bounds,
        floor_z,
        ceiling_z,
        planes,
        map,
        height_file: "height.bin".into(),
        camera,
        stations,
        views: view_records,
        quality,
        provenance: json!({"scans": scan_dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>(), "settings": settings, "built_by": "sim-place build", "mesh": {"vertices": mesh.vertices.len(), "triangles": mesh.triangles.len()}, "seconds": t0.elapsed().as_secs_f64()}),
    };
    Place::save(&out, &file, &tsdf, &heights)?;
    println!("{}", serde_json::to_string_pretty(&json!({"place": out.display().to_string(), "views": file.views.len(), "mesh_triangles": mesh.triangles.len(), "planes": file.planes.iter().map(|p| format!("{} {} ({:.2} m²)", p.id, p.kind, p.area_m2)).collect::<Vec<_>>(), "floor_z": floor_z, "ceiling_z": ceiling_z, "quality": file.quality, "registration_errors": registration_errors, "seconds": t0.elapsed().as_secs_f64()})).unwrap());
    Ok(())
}

/// Scores against the true scene when the scans came from simulation.
fn evaluate(scans: &[Scan], tsdf: &Tsdf, mesh: &sim_vision::fusion::Mesh, views: &[InputView], floor_z: Option<f64>) -> Value {
    let (Some(truth0), Some(scene_path)) = (scans[0].truth, scans[0].scene.as_ref()) else {
        return json!({"note": "no simulation truth: not evaluated"});
    };
    let Ok(text) = std::fs::read_to_string(scene_path) else { return json!({"note": format!("scene {} unreadable", scene_path.display())}) };
    let Ok(scene) = serde_json::from_str::<Scene>(&text) else { return json!({"note": "scene unreadable"}) };
    let world = |p: V3| truth0.point(p);
    // Mesh accuracy: distance from each vertex to the true surface along its
    // normal (both ways), capped at 0.5 m.
    let stride = (mesh.vertices.len() / 20_000).max(1);
    let mut accuracy = Vec::new();
    let rot = truth0.rotation();
    for (v, n) in mesh.vertices.iter().zip(&mesh.normals).step_by(stride) {
        let (w, nw) = (world(*v), sim_vision::mul(&rot, *n));
        // Start a little out in free space (20 voxels, at most 0.2 m) and come back along the normal.
        let back = (20.0 * tsdf.grid.voxel).min(0.2);
        let d = scene.hit(add(w, scale(nw, back)), scale(nw, -1.0)).map_or(0.5, |h| (h.t - back).abs());
        accuracy.push(d.min(0.5));
    }
    // Rendered-depth agreement: rays from every third photo, truth vs place.
    let cam = &scans[0].camera;
    let (mut agree, mut rays, mut errors) = (0usize, 0usize, Vec::new());
    for v in views.iter().step_by(3) {
        let pw = truth0.pose(&v.pose);
        for y in (8..cam.height).step_by(24) {
            for x in (8..cam.width).step_by(24) {
                let ray = cam.ray(x as f64 + 0.5, y as f64 + 0.5);
                let Some(truth) = scene.hit(pw.center, unit(pw.direction_to_world(ray))) else { continue };
                // Only surfaces inside the scanned volume (a plain backdrop is never reconstructed by design).
                let tp = truth0.inverse().point(truth.point);
                if (0..3).any(|k| tp[k] < tsdf.grid.origin[k] || tp[k] > tsdf.grid.origin[k] + (tsdf.grid.dims[k] - 1) as f64 * tsdf.grid.voxel) {
                    continue;
                }
                rays += 1;
                if let Some(hit) = tsdf.raycast(v.pose.center, v.pose.direction_to_world(ray), 12.0) {
                    let e = (hit.distance - truth.t).abs();
                    errors.push(e);
                    if e < 0.05 {
                        agree += 1;
                    }
                }
            }
        }
    }
    let floor_truth = scene.objects.iter().find_map(|o| match o.shape {
        sim_vision::scene::Shape::Room { min, .. } => Some(min[2] - truth0.position[2]),
        _ => None,
    });
    json!({
        "mesh_accuracy_mm": {"median": 1e3 * percentile(accuracy.clone(), 0.5), "p90": 1e3 * percentile(accuracy, 0.9), "rule": "vertex to the true surface along its normal, from 20 voxels (≤ 0.2 m) in front (capped at 500 mm)"},
        "view_depth_agreement": {"rays": rays, "within_5cm": agree as f64 / rays.max(1) as f64, "median_error_mm": 1e3 * percentile(errors, 0.5)},
        "floor_z": {"found": floor_z, "truth": floor_truth},
        "rule": "simulation only: true scene from the scan's scene file, place frame placed by station 0's true placement",
    })
}

// ---------------------------------------------------------------- tools

struct Tool {
    name: &'static str,
    description: &'static str,
    schema: Value,
}

enum Content {
    Text(String),
    Image(Vec<u8>),
}

fn point_schema(description: &str) -> Value {
    json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": description})
}

const FRAME: &str = "Coordinates are metres in the place frame: +Z up, origin at the first scan station's turntable base (usually a tabletop), +X along the rig's zero angle.";

fn tools() -> Vec<Tool> {
    let band = |d: f64| json!({"type": "number", "default": d});
    vec![
        Tool { name: "place_overview", description: "Start here. Summary of the scanned place: frame, size, floor and ceiling heights, walls and horizontal surfaces (tables, shelves), free floor area, scan stations and photos, plus a top-down map image (+Y up, 1 m grid, unknown areas hatched, photos as red dots, stations as blue crosses).", schema: json!({"type": "object", "properties": {}}) },
        Tool { name: "render_view", description: "See the place from any viewpoint (a virtual walkthrough). mode 'photo' blends the real photos that saw each point (most realistic, near the scan positions); 'shaded' shows the fused surface colours; 'depth' colours by distance. Give position and either look_at or yaw_deg/pitch_deg (yaw 0 = +X, 90 = +Y; pitch up positive).", schema: json!({"type": "object", "required": ["position"], "properties": {"position": point_schema("camera position [x, y, z]"), "look_at": point_schema("point to look at"), "yaw_deg": {"type": "number"}, "pitch_deg": {"type": "number", "default": 0}, "fov_deg": {"type": "number", "default": 90}, "width": {"type": "integer", "default": 640, "maximum": 1600}, "mode": {"type": "string", "enum": ["photo", "shaded", "depth"], "default": "photo"}}}) },
        Tool { name: "list_photos", description: "The real photos the place was built from: index, camera position and viewing direction.", schema: json!({"type": "object", "properties": {}}) },
        Tool { name: "get_photo", description: "One real photo, optionally with a point of the place marked on it (evidence for what is there).", schema: json!({"type": "object", "required": ["index"], "properties": {"index": {"type": "integer"}, "max_width": {"type": "integer", "default": 900}, "mark": point_schema("optional place point to circle")}}) },
        Tool { name: "photos_of_point", description: "Which real photos show a given point (visibility checked against the reconstruction), with the pixel where it appears.", schema: json!({"type": "object", "required": ["point"], "properties": {"point": point_schema("place point")}}) },
        Tool { name: "raycast", description: "First surface along a ray: hit point, distance, surface normal and the layout surface (floor, wall, table…) it belongs to. Give direction or target.", schema: json!({"type": "object", "required": ["origin"], "properties": {"origin": point_schema("ray start"), "direction": point_schema("ray direction"), "target": point_schema("aim at this point"), "max_distance": {"type": "number", "default": 15}}}) },
        Tool { name: "measure", description: "Distance between two points (straight, horizontal and vertical) and whether the straight line between them is clear of surfaces.", schema: json!({"type": "object", "required": ["a", "b"], "properties": {"a": point_schema("first point"), "b": point_schema("second point")}}) },
        Tool { name: "surfaces", description: "The room layout: planar surfaces found in the scan (floor, ceiling, walls, horizontal surfaces such as tables and shelves, other vertical surfaces) with centre, normal, size and area.", schema: json!({"type": "object", "properties": {"kind": {"type": "string", "description": "optional filter, e.g. 'wall' or 'horizontal surface'"}}}) },
        Tool { name: "height_at", description: "What is in the vertical column at (x, y): every surface crossing from top to bottom ('up' = something to stand or put things on, 'down' = an underside), and the top surface height.", schema: json!({"type": "object", "required": ["x", "y"], "properties": {"x": {"type": "number"}, "y": {"type": "number"}}}) },
        Tool { name: "clearance", description: "Distance from a point to the nearest surface, whether the point is inside something, or unknown if it was never seen.", schema: json!({"type": "object", "required": ["point"], "properties": {"point": point_schema("place point")}}) },
        Tool { name: "free_space_map", description: "Top-down map of free, occupied and unknown floor area for a body occupying heights z_min…z_max above the floor (e.g. a robot). Returns areas and an image (green free, red occupied, grey unknown).", schema: json!({"type": "object", "properties": {"z_min": band(0.05), "z_max": band(0.5)}}) },
        Tool { name: "plan_path", description: "Shortest collision-free path across the floor for a round robot of the given radius occupying z_min…z_max above the floor. Unknown areas are avoided unless allow_unknown. Returns waypoints (x, y), length and a map image.", schema: json!({"type": "object", "required": ["start", "goal"], "properties": {"start": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2}, "goal": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2}, "robot_radius": band(0.2), "z_min": band(0.05), "z_max": band(0.5), "allow_unknown": {"type": "boolean", "default": false}}}) },
    ]
}

fn v3(args: &Value, key: &str) -> Result<V3, String> {
    let a = args.get(key).and_then(Value::as_array).ok_or_else(|| format!("`{key}` must be [x, y, z]"))?;
    if a.len() != 3 {
        return Err(format!("`{key}` must have 3 numbers"));
    }
    let v: Vec<f64> = a.iter().map(|x| x.as_f64().ok_or_else(|| format!("`{key}` must be numbers"))).collect::<Result<_, _>>()?;
    Ok([v[0], v[1], v[2]])
}
fn num(args: &Value, key: &str, default: f64) -> Result<f64, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_f64().ok_or_else(|| format!("`{key}` must be a number")),
    }
}
fn r3(p: V3) -> Value {
    json!([(p[0] * 1000.0).round() / 1000.0, (p[1] * 1000.0).round() / 1000.0, (p[2] * 1000.0).round() / 1000.0])
}
fn text(v: Value) -> Content {
    Content::Text(serde_json::to_string_pretty(&v).unwrap())
}

fn nearest_plane(place: &Place, p: V3) -> Option<&place::Plane> {
    place.file.planes.iter().filter(|pl| {
        let d = dot(pl.normal, sub(p, pl.centroid)).abs();
        let (u, v) = (dot(sub(p, pl.centroid), pl.axis_u), dot(sub(p, pl.centroid), pl.axis_v));
        d < 0.05 && u >= pl.extent_u[0] - 0.05 && u <= pl.extent_u[1] + 0.05 && v >= pl.extent_v[0] - 0.05 && v <= pl.extent_v[1] + 0.05
    }).min_by(|a, b| dot(a.normal, sub(p, a.centroid)).abs().total_cmp(&dot(b.normal, sub(p, b.centroid)).abs()))
}

fn plane_json(p: &place::Plane) -> Value {
    json!({"id": p.id, "kind": p.kind, "centre": r3(p.centroid), "normal": r3(p.normal), "size_m": [((p.extent_u[1] - p.extent_u[0]) * 100.0).round() / 100.0, ((p.extent_v[1] - p.extent_v[0]) * 100.0).round() / 100.0], "area_m2": (p.area_m2 * 100.0).round() / 100.0,
        "height_m": if p.normal[2].abs() > 0.95 { json!((p.centroid[2] * 1000.0).round() / 1000.0) } else { Value::Null }})
}

/// Top-down map: 6 px per cell, +Y up. `cells`: optional free-space classes to show instead of heights.
fn map_image(place: &Place, cells: Option<&[u8]>, path: Option<&[[f64; 2]]>) -> (usize, usize, Vec<u8>) {
    let m = &place.file.map;
    let px = 6usize;
    let (w, h) = (m.dims[0] * px, m.dims[1] * px);
    let floor = place.file.floor_z.unwrap_or(place.file.bounds[0][2]);
    let mut img = vec![0u8; 3 * w * h];
    let put = |img: &mut Vec<u8>, x: i64, y: i64, c: [u8; 3]| {
        if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
            let k = 3 * (y as usize * w + x as usize);
            img[k..k + 3].copy_from_slice(&c);
        }
    };
    for j in 0..m.dims[1] {
        for i in 0..m.dims[0] {
            let c = match cells {
                Some(cl) => match cl[j * m.dims[0] + i] {
                    0 => [120, 200, 120],
                    1 => [215, 70, 60],
                    _ => [95, 95, 100],
                },
                None => {
                    let hgt = place.heights[j * m.dims[0] + i];
                    if hgt.is_finite() {
                        let t = ((hgt as f64 - floor) / 2.0).clamp(0.0, 1.0);
                        [(185.0 + 70.0 * t) as u8, (195.0 - 120.0 * t) as u8, (205.0 - 170.0 * t) as u8]
                    } else {
                        [70, 70, 76]
                    }
                }
            };
            for dy in 0..px {
                for dx in 0..px {
                    let hatch = cells.is_none() && !place.heights[j * m.dims[0] + i].is_finite() && (dx + dy) % 4 == 0;
                    put(&mut img, (i * px + dx) as i64, ((m.dims[1] - 1 - j) * px + dy) as i64, if hatch { [110, 110, 118] } else { c });
                }
            }
        }
    }
    let to_px = |x: f64, y: f64| (((x - m.origin[0]) / m.cell + 0.5) * px as f64, ((m.dims[1] as f64 - 0.5) - (y - m.origin[1]) / m.cell) * px as f64);
    // 1 m grid.
    for gx in (m.origin[0].ceil() as i64)..=((m.origin[0] + m.dims[0] as f64 * m.cell) as i64) {
        let (x, _) = to_px(gx as f64, 0.0);
        for y in (0..h).step_by(2) {
            put(&mut img, x as i64, y as i64, [40, 40, 44]);
        }
    }
    for gy in (m.origin[1].ceil() as i64)..=((m.origin[1] + m.dims[1] as f64 * m.cell) as i64) {
        let (_, y) = to_px(0.0, gy as f64);
        for x in (0..w).step_by(2) {
            put(&mut img, x as i64, y as i64, [40, 40, 44]);
        }
    }
    let line = |img: &mut Vec<u8>, a: (f64, f64), b: (f64, f64), c: [u8; 3], t: i64| {
        let n = ((a.0 - b.0).hypot(a.1 - b.1)).ceil().max(1.0) as usize;
        for s in 0..=n {
            let f = s as f64 / n as f64;
            let (x, y) = (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f);
            for dy in -t..=t {
                for dx in -t..=t {
                    put(img, x as i64 + dx, y as i64 + dy, c);
                }
            }
        }
    };
    for p in place.file.planes.iter().filter(|p| p.kind == "wall") {
        let a = add(p.centroid, scale(p.axis_u, p.extent_u[0]));
        let b = add(p.centroid, scale(p.axis_u, p.extent_u[1]));
        line(&mut img, to_px(a[0], a[1]), to_px(b[0], b[1]), [20, 20, 22], 1);
    }
    if let Some(path) = path {
        for w2 in path.windows(2) {
            line(&mut img, to_px(w2[0][0], w2[0][1]), to_px(w2[1][0], w2[1][1]), [30, 90, 230], 1);
        }
    }
    for v in &place.file.views {
        let (x, y) = to_px(v.pose.center[0], v.pose.center[1]);
        line(&mut img, (x - 2.0, y), (x + 2.0, y), [220, 30, 30], 1);
    }
    for s in &place.file.stations {
        let (x, y) = to_px(s.placement.position[0], s.placement.position[1]);
        line(&mut img, (x - 7.0, y), (x + 7.0, y), [30, 60, 220], 1);
        line(&mut img, (x, y - 7.0), (x, y + 7.0), [30, 60, 220], 1);
    }
    (w, h, img)
}

fn map_legend(place: &Place, w: usize, h: usize) -> Value {
    let m = &place.file.map;
    json!({"image_px": [w, h], "metres_per_px": m.cell / 6.0, "top_left_corner_m": [m.origin[0] - 0.5 * m.cell, m.origin[1] + (m.dims[1] as f64 - 0.5) * m.cell], "orientation": "+X right, +Y up", "grid": "dotted lines every 1 m"})
}

fn call(place: &Place, name: &str, args: &Value) -> Result<Vec<Content>, String> {
    let png = |w: usize, h: usize, rgb: &[u8]| sim_vision::output::png_bytes(w, h, rgb).map(Content::Image);
    let floor = place.file.floor_z.unwrap_or(place.file.bounds[0][2]);
    match name {
        "place_overview" => {
            let b = place.file.bounds;
            let cells = place::free_space(&place.tsdf, &place.file.map, floor + 0.05, floor + 0.5);
            let area = |c: u8| cells.iter().filter(|x| **x == c).count() as f64 * place.file.map.cell.powi(2);
            let (w, h, img) = map_image(place, None, None);
            let walls: Vec<Value> = place.file.planes.iter().filter(|p| p.kind == "wall").map(plane_json).collect();
            let surfaces: Vec<Value> = place.file.planes.iter().filter(|p| p.kind == "horizontal surface").map(plane_json).collect();
            Ok(vec![
                text(json!({
                    "description": place.file.description,
                    "frame": FRAME,
                    "bounds_m": [r3(b[0]), r3(b[1])],
                    "extent_m": r3(sub(b[1], b[0])),
                    "floor_z": place.file.floor_z, "ceiling_z": place.file.ceiling_z,
                    "room_height_m": place.file.floor_z.zip(place.file.ceiling_z).map(|(f, c)| c - f),
                    "walls": walls,
                    "horizontal_surfaces_other_than_floor": surfaces,
                    "floor_area_m2_for_a_body_0.05_to_0.5_m_above_floor": {"free": area(0), "occupied": area(1), "unknown": area(2)},
                    "stations": place.file.stations.iter().map(|s| json!({"name": s.name, "position": r3(s.placement.position), "yaw_deg": s.placement.yaw_deg, "method": s.method})).collect::<Vec<_>>(),
                    "photos": place.file.views.len(),
                    "coverage_note": "Seen from the scan stations only: backs of objects and anything hidden from them are unknown, never assumed free.",
                    "map_image": map_legend(place, w, h),
                    "quality": place.file.quality,
                })),
                png(w, h, &img)?,
            ])
        }
        "render_view" => {
            let eye = v3(args, "position")?;
            let target = if args.get("look_at").is_some() {
                v3(args, "look_at")?
            } else {
                let (yaw, pitch) = (num(args, "yaw_deg", 0.0)?.to_radians(), num(args, "pitch_deg", 0.0)?.to_radians());
                add(eye, [pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), pitch.sin()])
            };
            let fov = num(args, "fov_deg", 90.0)?.clamp(10.0, 150.0);
            let width = (num(args, "width", 640.0)? as usize).clamp(64, 1600);
            let height = width * 9 / 16;
            let fx = 0.5 * width as f64 / (0.5 * fov.to_radians()).tan();
            let camera = CameraModel { name: "virtual view".into(), width, height, fx, fy: fx, cx: 0.5 * width as f64, cy: 0.5 * height as f64, readout_s: 0.0, exposure_s: 0.0, provenance: String::new() };
            let pose = place::look_at(eye, target);
            let mode = args.get("mode").and_then(Value::as_str).unwrap_or("photo");
            if !["photo", "shaded", "depth"].contains(&mode) {
                return Err("mode must be photo, shaded or depth".into());
            }
            let (rgb, hit) = place.render(&pose, &camera, mode);
            let forward = unit(sub(target, eye));
            let unseen = place.clearance(eye).is_none();
            Ok(vec![
                text(json!({"position": r3(eye), "forward": r3(forward), "fov_deg": fov, "mode": mode, "centre_of_view_hits": hit.map(|h| json!({"point": r3(h.point), "distance_m": (h.distance * 1000.0).round() / 1000.0, "surface": nearest_plane(place, h.point).map(|p| p.kind.clone())})),
                    "note": if unseen { "the camera position itself was never observed: it may be inside an object" } else { "dark areas were not seen by any scan station" }})),
                png(width, height, &rgb)?,
            ])
        }
        "list_photos" => Ok(vec![text(json!({"photos": place.file.views.iter().enumerate().map(|(i, v)| {
            let f = v.pose.pose().direction_to_world([0.0, 0.0, 1.0]);
            json!({"index": i, "name": v.name, "station": v.station, "position": r3(v.pose.center), "facing_yaw_deg": (f[1].atan2(f[0]).to_degrees() * 10.0).round() / 10.0})
        }).collect::<Vec<_>>(), "camera": {"model": place.file.camera.name, "horizontal_fov_deg": place.file.camera.horizontal_fov_deg()}}))]),
        "get_photo" => {
            let i = num(args, "index", -1.0)? as i64;
            let images = place.images();
            let (w, h, img) = images.get(i as usize).filter(|_| i >= 0).ok_or_else(|| format!("index must be 0…{}", images.len().saturating_sub(1)))?;
            if img.is_empty() {
                return Err("photo file could not be read".into());
            }
            let mut img = img.clone();
            let mut note = json!(null);
            if args.get("mark").is_some() {
                let p = v3(args, "mark")?;
                let cam = &place.file.camera;
                let pose = place.file.views[i as usize].pose.pose();
                if let Some((u, v)) = cam.project(pose.to_camera(p)).filter(|(u, v)| cam.contains(*u, *v)) {
                    let (cx, cy) = (u * *w as f64 / cam.width as f64, v * *h as f64 / cam.height as f64);
                    for a in 0..360 {
                        for r in [14.0, 15.0, 16.0] {
                            let (x, y) = (cx + r * (a as f64).to_radians().cos(), cy + r * (a as f64).to_radians().sin());
                            if x >= 0.0 && y >= 0.0 && (x as usize) < *w && (y as usize) < *h {
                                let k = 3 * (y as usize * w + x as usize);
                                img[k..k + 3].copy_from_slice(&[255, 40, 200]);
                            }
                        }
                    }
                    note = json!({"marked_pixel": [cx.round(), cy.round()]});
                } else {
                    note = json!("the point is outside this photo");
                }
            }
            let max = (num(args, "max_width", 900.0)? as usize).clamp(64, *w);
            let (rw, rh, small) = sim_vision::output::resize(*w, *h, &img, max);
            let v = &place.file.views[i as usize];
            Ok(vec![text(json!({"index": i, "name": v.name, "position": r3(v.pose.center), "mark": note})), png(rw, rh, &small)?])
        }
        "photos_of_point" => {
            let p = v3(args, "point")?;
            let list = place.photos_of(p);
            Ok(vec![text(json!({"point": r3(p), "visible_in": list.iter().map(|(i, u, v, d)| json!({"index": i, "name": place.file.views[*i].name, "pixel": [u.round(), v.round()], "distance_m": (d * 1000.0).round() / 1000.0})).collect::<Vec<_>>(), "note": if list.is_empty() { "no photo sees this point (it may be hidden, behind a surface or outside every view)" } else { "pixel coordinates are in the full-resolution photo" }}))])
        }
        "raycast" => {
            let o = v3(args, "origin")?;
            let d = if args.get("target").is_some() { sub(v3(args, "target")?, o) } else { v3(args, "direction")? };
            if norm(d) < 1e-9 {
                return Err("direction must be nonzero".into());
            }
            let hit = place.tsdf.raycast(o, d, num(args, "max_distance", 15.0)?);
            Ok(vec![text(match hit {
                Some(h) => json!({"hit": true, "point": r3(h.point), "distance_m": (h.distance * 1000.0).round() / 1000.0, "normal": r3(h.normal), "surface": nearest_plane(place, h.point).map(plane_json)}),
                None => json!({"hit": false, "note": "no observed surface along this ray within max_distance (it may leave the scanned area or cross unseen space)"}),
            })])
        }
        "measure" => {
            let (a, b) = (v3(args, "a")?, v3(args, "b")?);
            let d = sub(b, a);
            let blocked = place.tsdf.raycast(a, d, norm(d)).filter(|h| h.distance < norm(d) - 0.02);
            Ok(vec![text(json!({"distance_m": (norm(d) * 1000.0).round() / 1000.0, "horizontal_m": (d[0].hypot(d[1]) * 1000.0).round() / 1000.0, "vertical_m": (d[2] * 1000.0).round() / 1000.0,
                "line_of_sight": match blocked { Some(h) => json!({"clear": false, "first_surface_at": r3(h.point), "after_m": (h.distance * 1000.0).round() / 1000.0}), None => json!({"clear": true}) }}))])
        }
        "surfaces" => {
            let kind = args.get("kind").and_then(Value::as_str);
            Ok(vec![text(json!({"frame": FRAME, "surfaces": place.file.planes.iter().filter(|p| kind.is_none_or(|k| p.kind == k)).map(plane_json).collect::<Vec<_>>()}))])
        }
        "height_at" => {
            let (x, y) = (num(args, "x", f64::NAN)?, num(args, "y", f64::NAN)?);
            if !x.is_finite() || !y.is_finite() {
                return Err("x and y are required".into());
            }
            let col = place.column(x, y);
            Ok(vec![text(json!({"x": x, "y": y, "top_surface_z": place.height_at(x, y), "surfaces_top_to_bottom": col.iter().map(|(z, k)| json!({"z": (z * 1000.0).round() / 1000.0, "faces": k, "above_floor_m": ((z - floor) * 1000.0).round() / 1000.0})).collect::<Vec<_>>(), "floor_z": place.file.floor_z}))])
        }
        "clearance" => {
            let p = v3(args, "point")?;
            Ok(vec![text(match place.clearance(p) {
                None => json!({"point": r3(p), "state": "unknown", "note": "this point was never observed"}),
                Some((d, far)) if far => json!({"point": r3(p), "state": "free", "clearance_m": format!("≥ {:.2}", d)}),
                Some((d, _)) if d < 0.0 => json!({"point": r3(p), "state": "inside a surface", "depth_m": (-d * 1000.0).round() / 1000.0}),
                Some((d, _)) => json!({"point": r3(p), "state": "free", "clearance_m": (d * 1000.0).round() / 1000.0}),
            })])
        }
        "free_space_map" => {
            let (lo, hi) = (num(args, "z_min", 0.05)?, num(args, "z_max", 0.5)?);
            let cells = place::free_space(&place.tsdf, &place.file.map, floor + lo, floor + hi);
            let area = |c: u8| cells.iter().filter(|x| **x == c).count() as f64 * place.file.map.cell.powi(2);
            let (w, h, img) = map_image(place, Some(&cells), None);
            Ok(vec![text(json!({"band_above_floor_m": [lo, hi], "free_m2": area(0), "occupied_m2": area(1), "unknown_m2": area(2), "map_image": map_legend(place, w, h)})), png(w, h, &img)?])
        }
        "plan_path" => {
            let two = |k: &str| -> Result<[f64; 2], String> {
                let a = args.get(k).and_then(Value::as_array).filter(|a| a.len() == 2).ok_or_else(|| format!("`{k}` must be [x, y]"))?;
                Ok([a[0].as_f64().ok_or(format!("`{k}` must be numbers"))?, a[1].as_f64().ok_or(format!("`{k}` must be numbers"))?])
            };
            let (start, goal) = (two("start")?, two("goal")?);
            let (lo, hi) = (num(args, "z_min", 0.05)?, num(args, "z_max", 0.5)?);
            let cells = place::free_space(&place.tsdf, &place.file.map, floor + lo, floor + hi);
            let allow = args.get("allow_unknown").and_then(Value::as_bool).unwrap_or(false);
            match place::plan_path(&place.file.map, &cells, start, goal, num(args, "robot_radius", 0.2)?, allow) {
                Ok(path) => {
                    let length: f64 = path.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
                    let (w, h, img) = map_image(place, Some(&cells), Some(&path));
                    Ok(vec![text(json!({"reachable": true, "length_m": (length * 100.0).round() / 100.0, "waypoints": path.iter().map(|p| json!([(p[0] * 100.0).round() / 100.0, (p[1] * 100.0).round() / 100.0])).collect::<Vec<_>>(), "map_image": map_legend(place, w, h)})), png(w, h, &img)?])
                }
                Err(e) => Ok(vec![text(json!({"reachable": false, "reason": e}))]),
            }
        }
        other => Err(format!("unknown tool `{other}`; see `sim-place tools`")),
    }
}

// ---------------------------------------------------------------- MCP

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

fn mcp(dir: &Path) -> Result<(), String> {
    let place = Place::load(dir)?;
    eprintln!("sim-place mcp: serving {} ({} photos)", dir.display(), place.file.views.len());
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                writeln!(stdout, "{}", json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}})).map_err(|e| e.to_string())?;
                stdout.flush().ok();
                continue;
            }
        };
        let Some(id) = request.get("id").cloned() else { continue }; // notifications need no reply
        let method = request["method"].as_str().unwrap_or("");
        let result: Result<Value, (i64, String)> = match method {
            "initialize" => Ok(json!({
                "protocolVersion": request["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "sim-place", "version": env!("CARGO_PKG_VERSION")},
                "instructions": format!("Tools for understanding one scanned place ({}). Start with place_overview. {FRAME} Unknown space is reported as unknown, never as free. Use render_view to look around, photos_of_point/get_photo for real evidence, and plan_path/free_space_map for movement.", place.file.description),
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools().iter().map(|t| json!({"name": t.name, "description": t.description, "inputSchema": t.schema})).collect::<Vec<_>>()})),
            "tools/call" => {
                let name = request["params"]["name"].as_str().unwrap_or("");
                let args = request["params"].get("arguments").cloned().unwrap_or(json!({}));
                Ok(match call(&place, name, &args) {
                    Ok(content) => json!({"content": content.into_iter().map(|c| match c {
                        Content::Text(t) => json!({"type": "text", "text": t}),
                        Content::Image(png) => json!({"type": "image", "data": base64(&png), "mimeType": "image/png"}),
                    }).collect::<Vec<_>>()}),
                    Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
                })
            }
            other => Err((-32601, format!("method not found: {other}"))),
        };
        let response = match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
        };
        writeln!(stdout, "{response}").map_err(|e| e.to_string())?;
        stdout.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------- simulator world

fn world(dir: &Path, out: &Path) -> Result<(), String> {
    let place = Place::load(dir)?;
    let floor = place.file.floor_z.ok_or("no floor was found in this place: cannot make a walkable world")?;
    let m = &place.file.map;
    let cap = place.file.ceiling_z.unwrap_or(place.file.bounds[1][2]);
    let (nx, ny) = (m.dims[0], m.dims[1]);
    let mut heights = vec![0.0f64; nx * ny];
    let mut unknown = 0usize;
    for i in 0..nx {
        for j in 0..ny {
            let h = place.heights[j * nx + i];
            heights[i * ny + j] = if h.is_finite() {
                h as f64
            } else {
                unknown += 1;
                cap
            };
        }
    }
    let world = json!({
        "schema_note": "simrobot v3 `world` block (cad/PHYSICAL_MODEL.md): heights[ix*ny + iy], absolute z in the place frame",
        "world": {"floor_z": floor, "terrain": {"origin": [m.origin[0], m.origin[1]], "cell": m.cell, "dims": [nx, ny], "heights": heights}},
        "unknown_policy": format!("{unknown} of {} cells were never observed and are raised to the ceiling height ({cap:.2} m): unknown space is treated as an obstacle, not as floor", nx * ny),
        "source": {"place": dir.display().to_string(), "description": place.file.description},
    });
    std::fs::write(out, serde_json::to_string(&world).unwrap()).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("wrote {} ({nx}×{ny} cells of {} m; {unknown} unknown cells raised)", out.display(), m.cell);
    Ok(())
}
