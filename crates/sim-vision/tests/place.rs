//! Fusion, layout, maps, path planning and station registration on exact
//! (renderer ground-truth) depth maps, so errors here are the algorithms'.
use sim_vision::camera::CameraModel;
use sim_vision::mvs::DepthMap;
use sim_vision::place::{self, BuildSettings, InputView, MapGrid};
use sim_vision::register::{Target, register};
use sim_vision::render::{RenderSettings, render};
use sim_vision::rig::{Rig, Station};
use sim_vision::scene::Scene;
use sim_vision::{norm, sub, unit};

fn rig() -> Rig {
    Rig { schema: Rig::SCHEMA.into(), axis_point: [0.0; 3], axis: [0.0, 0.0, 1.0], optical_center: [0.0, 0.156, 0.084], optical_axis: [0.0, 1.0, 0.0], up: [0.0, 0.0, 1.0], source: serde_json::Value::Null }
}

fn room() -> Scene {
    serde_json::from_value(serde_json::json!({
        "schema": "sim.vision-scene/1",
        "lighting": {"ambient": 0.5, "sun_direction": [0.3, 0.5, -1.0], "sun": 0.5},
        "objects": [
            {"name": "room", "shape": {"kind": "room", "min": [-2.0, -1.6, -0.8], "max": [2.4, 1.9, 1.7]},
             "texture": {"kind": "noise", "scale": 6.0, "a": [0.2, 0.2, 0.3], "b": [0.9, 0.85, 0.7], "seed": 3}},
            {"name": "crate", "shape": {"kind": "box", "center": [0.9, 0.9, -0.5], "size": [0.6, 0.6, 0.6]},
             "texture": {"kind": "noise", "scale": 20.0, "a": [0.5, 0.2, 0.1], "b": [0.95, 0.7, 0.4], "seed": 9}}
        ]
    }))
    .unwrap()
}

/// Exact depth views from a station (the renderer's depth, not stereo).
fn views(station: Station, n: usize) -> Vec<InputView> {
    let camera = CameraModel::camera_module_3_wide(240);
    let (scene, rig) = (room(), rig());
    (0..n)
        .map(|k| {
            let a = (k as f64 * 360.0 / n as f64).to_radians();
            let world = station.pose(&rig.pose(a));
            let frame = render(&scene, &camera, &|_| world, 0.0, RenderSettings { supersample: 1, time_samples: 1 });
            let depth = frame.depth.iter().map(|d| if d.is_finite() { *d } else { f32::NAN }).collect::<Vec<_>>();
            let n = depth.len();
            InputView { name: format!("{k}"), image: String::new(), pose: rig.pose(a), station: 0, depth: DepthMap { step: 1, columns: camera.width, rows: camera.height, depth, score: vec![0.0; n] }, depth_camera: camera.clone(), depth_rgb8: frame.rgb8() }
        })
        .collect()
}

#[test]
fn fused_room_matches_the_scene_and_gives_layout_maps_and_paths() {
    let v = views(Station::default(), 24);
    let s = BuildSettings::default();
    let bounds = place::observed_bounds(&v, 0.15);
    let tsdf = place::fuse(&v, bounds, &s);
    // Ray casts from the rig agree with the scene.
    let scene = room();
    let mut errors = Vec::new();
    for k in 0..90 {
        let a = (k as f64 * 4.0).to_radians();
        let dir = [a.cos(), a.sin(), -0.15];
        let o = [0.0, 0.0, 0.084];
        if let (Some(h), Some(t)) = (tsdf.raycast(o, dir, 8.0), scene.hit(o, unit(dir))) {
            errors.push((h.distance - t.t).abs());
        }
    }
    errors.sort_by(|a, b| a.total_cmp(b));
    let median = errors[errors.len() / 2];
    assert!(errors.len() > 80, "most rays hit ({})", errors.len());
    assert!(median < 0.5 * s.voxel, "median ray error {median} m");
    // Layout: floor at −0.8 and at least three walls.
    let mesh = tsdf.mesh();
    let planes = place::find_planes(&mesh, &s);
    let floor = planes.iter().find(|p| p.kind == "floor").expect("a floor");
    assert!((floor.centroid[2] + 0.8).abs() < s.voxel, "floor at {}", floor.centroid[2]);
    assert!(planes.iter().filter(|p| p.kind == "wall").count() >= 3, "walls: {:?}", planes.iter().map(|p| &p.kind).collect::<Vec<_>>());
    // Height map: the crate top at −0.2; free space: the crate blocks, the floor around it is free.
    let map = MapGrid { origin: [bounds[0][0], bounds[0][1]], cell: 0.05, dims: [((bounds[1][0] - bounds[0][0]) / 0.05) as usize + 1, ((bounds[1][1] - bounds[0][1]) / 0.05) as usize + 1] };
    let heights = place::height_map(&tsdf, &map, 1.5);
    let (i, j) = map.index_of(0.9, 0.9).unwrap();
    let top = heights[j * map.dims[0] + i];
    assert!((top + 0.2).abs() < 0.05, "crate top {top}");
    let cells = place::free_space(&tsdf, &map, -0.75, -0.3);
    // The crate's inside is never seen: its centre is not free, and its seen side is occupied.
    let (i, j) = map.index_of(0.9, 0.9).unwrap();
    assert_ne!(cells[j * map.dims[0] + i], 0, "crate centre is not free");
    let (i, j) = map.index_of(0.63, 0.9).unwrap();
    assert_eq!(cells[j * map.dims[0] + i], 1, "crate side is occupied");
    // Open floor far enough out that the camera sees down to it is free (nearer, the low band is unseen: unknown).
    let (i, j) = map.index_of(-1.7, 0.0).unwrap();
    assert_eq!(cells[j * map.dims[0] + i], 0, "open floor is free");
    // A path across the room (unknown allowed: the floor near the rig is unseen) avoids the crate.
    let path = place::plan_path(&map, &cells, [-1.6, 1.4], [1.9, -1.2], 0.1, true).expect("a path");
    for w in &path {
        assert!(!(w[0] > 0.55 && w[0] < 1.25 && w[1] > 0.55 && w[1] < 1.25), "waypoint {w:?} crosses the crate");
    }
}

#[test]
fn registration_recovers_a_station_from_a_rough_guess() {
    let truth = Station { position: [0.8, -0.5, 0.0], yaw_deg: 35.0 };
    let (a, b) = (views(Station::default(), 16), views(truth, 16));
    let s = BuildSettings { voxel: 0.04, truncation: 0.12, ..BuildSettings::default() };
    let mesh_a = place::fuse(&a, place::observed_bounds(&a, 0.1), &s).mesh();
    let mesh_b = place::fuse(&b, place::observed_bounds(&b, 0.1), &s).mesh();
    let target = Target::new(mesh_a.vertices, mesh_a.normals, 0.2);
    let guess = Station { position: [0.7, -0.42, 0.0], yaw_deg: 30.0 };
    let reg = register(&target, &mesh_b.vertices, guess, 0.3, 20.0, 0.1);
    let dp = norm(sub(reg.station.position, truth.position));
    eprintln!("registration: {:?}, position error {:.1} mm, yaw error {:.2}°", reg, 1e3 * dp, reg.station.yaw_deg - truth.yaw_deg);
    assert!(dp < 0.02, "position error {dp} m");
    assert!((reg.station.yaw_deg - truth.yaw_deg).abs() < 0.5);
    assert!(reg.overlap > 0.5);
}
