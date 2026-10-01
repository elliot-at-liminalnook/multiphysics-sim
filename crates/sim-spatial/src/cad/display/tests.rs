//! Windowless checks of the display state: argument handling, RoboCAD's mode
//! order, section planes, triangle clipping, stale exact sections, the view
//! cube's camera actions and the REST form round trip.
use super::section::{accept, clip, clip_polyline, overhangs, segments};
use super::ui::{cube_action, facing};
use super::*;
use crate::camera::{CameraAction, ViewPreset};
use sim_runtime::cad_client::MeshData;

/// A unit cube (mm) as RoboCAD tessellates it: 8 vertices, 2 triangles per
/// face, faces 0..6 (−Z, +Z, −Y, +Y, −X, +X).
fn cube() -> MeshData {
    let vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]];
    let quads: [[u32; 4]; 6] = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [3, 7, 6, 2], [0, 4, 7, 3], [1, 2, 6, 5]];
    let mut triangles = Vec::new();
    let mut triangle_face = Vec::new();
    for (f, q) in quads.iter().enumerate() {
        triangles.push([q[0], q[1], q[2]]);
        triangles.push([q[0], q[2], q[3]]);
        triangle_face.extend([f as i64, f as i64]);
    }
    MeshData { vertices, triangles, triangle_face, face_count: 6 }
}

fn area(m: &MeshData, t: [u32; 3]) -> f64 {
    let p = t.map(|i| Vec3::from_array(m.vertices[i as usize].map(|x| x as f32)));
    ((p[1] - p[0]).cross(p[2] - p[0]).length() / 2.0) as f64
}

#[test]
fn modes_cycle_in_robocads_order() {
    // ui/viewport.py: MODES = ("shaded", "shaded_edges", "wireframe", "xray", "matcap", "render").
    let names: Vec<&str> = DisplayMode::ALL.iter().map(|m| m.name()).collect();
    assert_eq!(names, ["shaded", "shaded_edges", "wireframe", "xray", "matcap", "render"]);
    let mut d = CadDisplay::default();
    assert_eq!(d.mode, DisplayMode::ShadedEdges);
    let mut seen = Vec::new();
    for _ in 0..6 {
        apply_display(&mut d, &DisplayArgs { next: true, ..default() }).unwrap();
        seen.push(d.mode.name());
    }
    assert_eq!(seen, ["wireframe", "xray", "matcap", "render", "shaded", "shaded_edges"]);
    for m in DisplayMode::ALL {
        assert_eq!(serde_json::to_value(m).unwrap(), serde_json::json!(m.name()));
    }
}

#[test]
fn display_args_set_and_toggle() {
    let mut d = CadDisplay::default();
    assert!(d.grid && !d.build_plate && !d.high_contrast && d.view_cube && d.comment_pins);
    apply_display(&mut d, &DisplayArgs { toggle: Some(DisplaySetting::Grid), ..default() }).unwrap();
    assert!(!d.grid);
    apply_display(&mut d, &DisplayArgs { toggle: Some(DisplaySetting::Grid), ..default() }).unwrap();
    assert!(d.grid);
    apply_display(&mut d, &DisplayArgs { toggle: Some(DisplaySetting::BuildPlate), ..default() }).unwrap();
    apply_display(&mut d, &DisplayArgs { toggle: Some(DisplaySetting::HighContrast), ..default() }).unwrap();
    assert!(d.build_plate && d.high_contrast);
    apply_display(&mut d, &DisplayArgs { mode: Some(DisplayMode::Xray), grid: Some(false), view_cube: Some(false), ..default() }).unwrap();
    assert_eq!((d.mode, d.grid, d.view_cube), (DisplayMode::Xray, false, false));
    // Refusals change nothing.
    let before = d.clone();
    assert!(apply_display(&mut d, &DisplayArgs::default()).is_err());
    assert!(apply_display(&mut d, &DisplayArgs { mode: Some(DisplayMode::Shaded), next: true, ..default() }).is_err());
    assert!(apply_display(&mut d, &DisplayArgs { toggle: Some(DisplaySetting::Grid), grid: Some(true), ..default() }).is_err());
    assert_eq!(d, before);
}

#[test]
fn rest_forms_parse_back_to_the_same_action() {
    let actions = [
        CadAction::CadDisplay(DisplayArgs { toggle: Some(DisplaySetting::BuildPlate), ..default() }),
        CadAction::CadDisplay(DisplayArgs { mode: Some(DisplayMode::Matcap), high_contrast: Some(true), ..default() }),
        CadAction::CadDisplay(DisplayArgs { next: true, ..default() }),
        CadAction::CadSection(SectionArgs::default()),
        CadAction::CadSection(SectionArgs { axis: Some(SectionAxis::Z), offset: Some(5.0), rotate: true, exact: Some("n1".into()), ..default() }),
        CadAction::CadSection(SectionArgs { enabled: Some(true), plane: Some(SectionPlane::on_axis(SectionAxis::Y, 2.0)), ..default() }),
    ];
    for action in actions {
        let form = crate::cad::rest_form::rest_form(&action);
        let back: CadAction = serde_json::from_value(form.clone()).unwrap_or_else(|e| panic!("{form}: {e}"));
        assert_eq!(back, action, "{form}");
    }
    for s in specs() {
        let mut example = s.example.clone();
        example["command"] = serde_json::json!(s.name);
        serde_json::from_value::<CadAction>(example.clone()).unwrap_or_else(|e| panic!("{example}: {e}"));
    }
}

#[test]
fn section_toggles_on_robocads_default_plane_and_moves() {
    let mut d = CadDisplay::default();
    let cx = SectionContext { revision: 3, bounds: Some(([0.0, 10.0, 0.0], [5.0, 30.0, 4.0])), ..default() };
    // Toggle on: Plane.xz through the bounds' centre in Y (RoboCAD's SectionTool.activate).
    apply_section(&mut d, &SectionArgs::default(), &cx).unwrap();
    assert!(d.section.enabled);
    assert_eq!(d.section.plane, Some(SectionPlane { origin: [0.0, 20.0, 0.0], normal: [0.0, -1.0, 0.0], x_axis: [1.0, 0.0, 0.0] }));
    // Toggle off keeps the plane; on again reuses it.
    apply_section(&mut d, &SectionArgs::default(), &cx).unwrap();
    assert!(!d.section.enabled && d.section.plane.is_some());
    // Tab offset: along the normal (−Y here).
    apply_section(&mut d, &SectionArgs { offset: Some(5.0), ..default() }, &cx).unwrap();
    assert!(d.section.enabled);
    assert_eq!(d.section.plane.unwrap().origin, [0.0, 15.0, 0.0]);
    // R: the normal turns 90° about Z (−Y → +X).
    apply_section(&mut d, &SectionArgs { rotate: true, ..default() }, &cx).unwrap();
    let p = d.section.plane.unwrap();
    assert_eq!(p.normal, [1.0, 0.0, 0.0]);
    assert!((p.x_axis[1] - 1.0).abs() < 1e-12, "{p:?}");
    // {axis, offset}; plane and axis together, a bad plane and a non-finite offset are refused.
    apply_section(&mut d, &SectionArgs { axis: Some(SectionAxis::Z), offset: Some(2.5), ..default() }, &cx).unwrap();
    assert_eq!(d.section.plane, Some(SectionPlane { origin: [0.0, 0.0, 2.5], normal: [0.0, 0.0, 1.0], x_axis: [1.0, 0.0, 0.0] }));
    let tilted = SectionPlane { origin: [0.0; 3], normal: [0.0, 0.0, 2.0], x_axis: [1.0, 0.0, 0.1] };
    assert!(apply_section(&mut d, &SectionArgs { plane: Some(tilted), ..default() }, &cx).unwrap_err().contains("perpendicular"));
    assert!(apply_section(&mut d, &SectionArgs { plane: Some(tilted), axis: Some(SectionAxis::X), ..default() }, &cx).is_err());
    assert!(apply_section(&mut d, &SectionArgs { offset: Some(f64::NAN), ..default() }, &cx).is_err());
    let ok = SectionPlane { origin: [1.0, 2.0, 3.0], normal: [0.0, 0.0, 2.0], x_axis: [3.0, 0.0, 0.0] }.validated().unwrap();
    assert_eq!((ok.normal, ok.x_axis), ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
}

#[test]
fn clipping_a_unit_cube_keeps_robocads_side_and_triangle_order() {
    let mesh = cube();
    // z = 0.5, normal +Z: RoboCAD's clip plane keeps dot(n, p − o) ≤ 0, the lower half.
    let plane = SectionPlane::on_axis(SectionAxis::Z, 0.5);
    let cut = clip(&mesh, &plane);
    // Slots keep RoboCAD's order and faces; each of the 8 side triangles is
    // cut, and those leaving four corners append one triangle each.
    assert_eq!(&cut.triangle_face[..12], &mesh.triangle_face[..]);
    assert_eq!(cut.triangles.len(), cut.triangle_face.len());
    assert!(cut.triangles.len() > 12);
    // The top face is gone: collapsed to a point (no area, no ray hit).
    for t in 2..4 {
        let [a, b, c] = cut.triangles[t];
        assert!(a == b && b == c, "{:?}", cut.triangles[t]);
    }
    // Nothing kept lies above the plane; the bottom stays whole.
    for tri in &cut.triangles {
        for i in tri {
            assert!(cut.vertices[*i as usize][2] <= 0.5 + 1e-12);
        }
    }
    assert_eq!(&cut.triangles[..2], &mesh.triangles[..2]);
    // Kept area: the bottom (1) and the lower halves of the four sides (4 × 0.5).
    let total: f64 = cut.triangles.iter().map(|t| area(&cut, *t)).sum();
    assert!((total - 3.0).abs() < 1e-6, "{total}");
    // A cut point on a side shared by two triangles is made once: 8 cube corners + 8 on the vertical edges and side diagonals.
    assert!(cut.vertices.len() <= 8 + 8, "{}", cut.vertices.len());
    // RoboCAD's mesh_segments: the outline is the square at z = 0.5, length 4.
    let outline = segments(&mesh, &plane);
    assert_eq!(outline.len(), 8);
    let length: f32 = outline.iter().map(|[a, b]| a.distance(*b)).sum();
    assert!((length - 4.0).abs() < 1e-5, "{length}");
    assert!(outline.iter().flatten().all(|p| (p.z - 0.5).abs() < 1e-6));
    // The bottom's two triangles face straight down: overhangs; nothing else is.
    let o = overhangs(&mesh, 45.0);
    assert_eq!(o.triangles, mesh.triangles[..2].to_vec());
    assert_eq!(o.triangle_face, vec![0, 0]);
}

#[test]
fn polylines_are_cut_at_the_plane() {
    let plane = SectionPlane::on_axis(SectionAxis::X, 1.0);
    let line = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
    let runs = clip_polyline(&line, &plane);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0], vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)]);
    assert_eq!(runs[1], vec![Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)]);
    assert!(clip_polyline(&line[1..3], &plane).is_empty());
}

fn key(revision: u64, plane: SectionPlane) -> ExactKey {
    ExactKey { node: "n1".into(), revision, plane, query: SectionQuery::Xy }
}

#[test]
fn a_stale_exact_section_is_never_drawn() {
    let xy = SectionPlane::on_axis(SectionAxis::Z, 0.0);
    let section = Section { enabled: true, plane: Some(xy) };
    let curves = SectionCurves { polylines: vec![vec![[0.0; 3], [1.0, 0.0, 0.0]]], dropped: 0 };
    // A read finishing for a superseded request is dropped.
    assert!(accept(Some(&key(5, xy)), Some(key(4, xy)), Ok(curves.clone())).is_none());
    assert!(accept(None, Some(key(4, xy)), Ok(curves.clone())).is_none());
    let answer = accept(Some(&key(5, xy)), Some(key(5, xy)), Ok(curves.clone())).unwrap();
    let exact = ExactSection { request: Some(key(5, xy)), result: Some(answer) };
    assert!(!exact.pending());
    assert_eq!(exact.drawn(&section, 5).map(|s| s.polylines.len()), Some(1));
    // Another revision, another plane, the section off, or a newer request: not drawn.
    assert!(exact.drawn(&section, 6).is_none());
    assert!(exact.drawn(&Section { enabled: true, plane: Some(SectionPlane::on_axis(SectionAxis::Z, 1.0)) }, 5).is_none());
    assert!(exact.drawn(&Section { enabled: false, plane: Some(xy) }, 5).is_none());
    let newer = ExactSection { request: Some(key(6, xy)), ..exact.clone() };
    assert!(newer.pending() && newer.drawn(&section, 6).is_none());
    // An error is kept, not drawn.
    let failed = ExactSection { request: Some(key(5, xy)), result: Some((key(5, xy), Err("422".into()))) };
    assert!(failed.drawn(&section, 5).is_none() && !failed.pending());
}

#[test]
fn exact_sections_need_a_plane_robocads_route_can_name() {
    let mut d = CadDisplay::default();
    let cx = SectionContext { revision: 7, nodes: Some(vec!["n1".into()]), ..default() };
    // An offset plane has no query form; nothing is asked.
    apply_section(&mut d, &SectionArgs { axis: Some(SectionAxis::Z), offset: Some(3.0), ..default() }, &cx).unwrap();
    assert!(apply_section(&mut d, &SectionArgs { exact: Some("n1".into()), ..default() }, &cx).unwrap_err().contains("plane=xy|xz|yz"));
    assert_eq!(d.exact.request, None);
    // Through the origin: the named plane, at RoboCAD's revision.
    assert!(apply_section(&mut d, &SectionArgs { axis: Some(SectionAxis::Y), exact: Some("n1".into()), ..default() }, &cx).unwrap());
    let request = d.exact.request.clone().unwrap();
    assert_eq!((request.node.as_str(), request.revision, request.query.clone()), ("n1", 7, SectionQuery::Xz));
    // The active plane node at the section plane names it.
    let at = SectionPlane::on_axis(SectionAxis::Z, 3.0);
    let with_node = SectionContext { plane_node: Some(("p1".into(), SectionPlane { normal: [0.0, 0.0, -1.0], ..at })), ..cx.clone() };
    assert_eq!(exact_query(&at, &with_node), Ok(SectionQuery::Node("p1".into())));
    // An unknown node, or the section off, is refused.
    assert!(apply_section(&mut d, &SectionArgs { exact: Some("n9".into()), ..default() }, &cx).is_err());
    assert!(apply_section(&mut d, &SectionArgs { enabled: Some(false), exact: Some("n1".into()), ..default() }, &cx).is_err());
    let state = state_json(&d);
    assert_eq!(state["section"]["exact"]["request"]["query"], "xz");
    assert_eq!(state["section"]["exact"]["pending"], true);
    assert_eq!(state["mode"], "shaded_edges");
}

#[test]
fn the_view_cube_writes_robocads_views() {
    // view_cube_hit at the centre: the face the eye looks from.
    assert_eq!(facing(-90.0, 0.0), ViewPreset::Front);
    assert_eq!(facing(90.0, 0.0), ViewPreset::Back);
    assert_eq!(facing(0.0, 0.0), ViewPreset::Right);
    assert_eq!(facing(180.0, 0.0), ViewPreset::Left);
    assert_eq!(facing(-90.0, 89.5), ViewPreset::Top);
    assert_eq!(facing(-90.0, -89.5), ViewPreset::Bottom);
    assert_eq!(facing(-35.0, 28.0), ViewPreset::Right);
    let limit = 89.5f32.to_radians();
    // A face press shows that face …
    assert_eq!(cube_action(ViewPreset::Front, Some(ViewPreset::Iso.yaw_pitch()), limit), CameraAction::View { view: ViewPreset::Front });
    assert_eq!(cube_action(ViewPreset::Top, None, limit), CameraAction::View { view: ViewPreset::Top });
    // … and again, at that face, its opposite (RoboCAD's second click).
    for view in [ViewPreset::Front, ViewPreset::Top, ViewPreset::Right, ViewPreset::Bottom] {
        assert_eq!(cube_action(view, Some(view.yaw_pitch()), limit), CameraAction::Opposite, "{view:?}");
    }
    assert_eq!(cube_action(ViewPreset::Iso, Some(ViewPreset::Iso.yaw_pitch()), limit), CameraAction::View { view: ViewPreset::Iso });
}
