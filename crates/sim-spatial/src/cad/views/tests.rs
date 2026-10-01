//! Saved views without a window: the native ↔ RoboCAD conversions (frames,
//! units, turntable and trackball), the schema's keys, the display state,
//! and the controls' REST round trip.
use super::convert::{self, ViewCamera, apply_display, camera_of, capture, display_to_model, model_to_display, rot_of, rotation_of};
use super::{CadViews, ViewsArgs, ViewsOp, controls_of, specs};
use crate::app::actions::{Action, control_matches};
use crate::cad::actions::CadAction;
use crate::cad::display::{CadDisplay, DisplayMode, SectionPlane};
use crate::cad::document::{CadDocument, CadTarget, Connection};
use crate::camera::{Orbit, ViewPreset, robocad_to_display};
use bevy::math::DVec3;
use bevy::prelude::*;
use serde_json::Value;
use sim_runtime::cad_client::{Health, SavedView, VIEW_STATE_KEYS, ViewState};
use std::collections::BTreeSet;

fn close(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}

fn close3(a: [f64; 3], b: [f64; 3], eps: f64) -> bool {
    (0..3).all(|i| close(a[i], b[i], eps))
}

/// RoboCAD's `Camera.basis()` for a turntable (yaw, pitch) in degrees:
/// rows right, up, back (ui/viewport.py:57-75).
fn robocad_basis(yaw: f64, pitch: f64) -> [[f64; 3]; 3] {
    let (y, p) = (yaw.to_radians(), pitch.to_radians());
    let back = DVec3::new(p.cos() * y.cos(), p.cos() * y.sin(), p.sin());
    let right = DVec3::Z.cross(back).normalize();
    let up = back.cross(right).normalize();
    [right.to_array(), up.to_array(), back.to_array()]
}

fn turntable(yaw_deg: f32, pitch_deg: f32) -> ViewCamera {
    let (yaw, pitch) = robocad_to_display(yaw_deg, pitch_deg);
    ViewCamera::of(&Orbit { focus: Vec3::new(0.01, 0.02, -0.03), radius: 0.25, yaw, pitch, fov: 40f32.to_radians(), ..Orbit::default() })
}

/// The display frame is the mesh root's: model (x, y, z) mm → display (x, z, −y) / 1000.
#[test]
fn points_map_as_the_mesh_root_does() {
    let root = crate::cad::mesh::root_transform();
    for p in [[10.0, 0.0, 0.0], [0.0, 20.0, 0.0], [0.0, 0.0, 30.0], [12.5, -40.0, 7.0]] {
        let shown = model_to_display(p);
        let want = root.transform_point(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32));
        assert!((shown - want).length() < 1e-6, "{p:?}: {shown} vs the root's {want}");
        assert!(close3(display_to_model(shown), p, 1e-3), "{p:?} back");
    }
    assert_eq!(model_to_display([0.0, 0.0, 1000.0]), Vec3::Y, "model +Z is display up");
}

/// Every RoboCAD named view (turntable) gives RoboCAD's own eye direction
/// and basis both ways: the native camera's rotation, saved as `rot`, is
/// `Camera.basis()`, and the eye sits along RoboCAD's `direction()`.
#[test]
fn a_turntable_view_has_robocads_eye_direction_and_basis_both_ways() {
    for preset in ViewPreset::ALL {
        let (y, p) = preset.robocad_degrees();
        let cam = turntable(y, p);
        let rot = rot_of(cam.rotation);
        let want = robocad_basis(f64::from(y), f64::from(p));
        for (row, (got, want)) in rot.iter().zip(want).enumerate() {
            assert!(close3(*got, want, 1e-5), "{preset:?} row {row}: {got:?} vs RoboCAD's {want:?}");
        }
        // The native eye direction (display) is RoboCAD's back mapped to the display frame.
        let eye = (cam.rotation * Vec3::Z).as_dvec3();
        let back = DVec3::from_array(want[2]);
        assert!((eye - DVec3::new(back.x, back.z, -back.y)).length() < 1e-5, "{preset:?}");
        // And back: RoboCAD's rot restores the same rotation.
        let q = rotation_of(&want);
        assert!(q.dot(cam.rotation).abs() > 1.0 - 1e-6, "{preset:?}: {q} vs {}", cam.rotation);
    }
}

/// A native camera → view state (exactly `validate_state`'s keys) → the
/// same camera.
#[test]
fn the_camera_round_trips_through_the_schema() {
    let display = CadDisplay::default();
    for (y, p, ortho) in [(-35.0, 28.0, false), (120.0, -40.0, true), (-90.0, 89.5, false)] {
        let mut cam = turntable(y, p);
        cam.orthographic = ortho;
        let state = capture(&cam, &display);
        assert_eq!(state.check(), Ok(()));
        let json = serde_json::to_value(&state).unwrap();
        let keys: BTreeSet<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, VIEW_STATE_KEYS.into_iter().collect::<BTreeSet<_>>());
        assert!(close3(state.target, [10.0, 30.0, 20.0], 1e-3), "{:?}", state.target);
        assert!(close(state.distance, 250.0, 1e-3) && close(state.fov, 40.0, 1e-4));
        assert!(close(state.yaw, f64::from(y), 1e-3) && close(state.pitch, f64::from(p), 1e-3), "{} {}", state.yaw, state.pitch);
        assert_eq!((state.mode.as_str(), state.orthographic), ("turntable", ortho));
        let back: ViewState = serde_json::from_value(json).unwrap();
        let (restored, note) = camera_of(&back).unwrap();
        assert_eq!(note, None);
        assert!((Vec3::from_array(restored.focus) - cam.focus).length() < 1e-6);
        assert!((restored.radius - cam.radius).abs() < 1e-6);
        assert!((restored.yaw - cam.yaw).abs() < 1e-5 && (restored.pitch - cam.pitch).abs() < 1e-5);
        assert_eq!((restored.orthographic, restored.trackball, restored.seconds), (ortho, None, None));
        assert!((restored.fov_deg.unwrap() - 40.0).abs() < 1e-4);
    }
}

/// The trackball's rotation is saved as `rot` and restored from it; its
/// yaw and pitch are the nearest turntable heading of its view direction.
#[test]
fn a_trackball_view_restores_its_rotation() {
    let q = (Quat::from_rotation_y(0.7) * Quat::from_rotation_x(-0.4) * Quat::from_rotation_z(0.3)).normalize();
    let cam = ViewCamera::of(&Orbit { focus: Vec3::ZERO, radius: 0.5, trackball: Some(q), fov: 0.6, ..Orbit::default() });
    let state = capture(&cam, &CadDisplay::default());
    assert_eq!(state.mode, "trackball");
    assert_eq!(state.check(), Ok(()), "rot is orthonormal with det 1");
    let back = DVec3::from_array(state.rot[2]);
    let (y, p) = (state.yaw.to_radians(), state.pitch.to_radians());
    assert!((back - DVec3::new(p.cos() * y.cos(), p.cos() * y.sin(), p.sin())).length() < 1e-5, "yaw/pitch point along rot's back");
    let (restored, _) = camera_of(&state).unwrap();
    let r = Quat::from_array(restored.trackball.expect("a trackball rotation"));
    assert!(r.dot(q).abs() > 1.0 - 1e-6, "{r} vs {q}");
}

/// Grid, display mode, comment pins and the section follow the view; a
/// field of view outside the native range is brought into it with a note.
#[test]
fn display_settings_and_fov_are_restored() {
    let mut display = CadDisplay::default();
    display.mode = DisplayMode::Wireframe;
    display.grid = false;
    display.comment_pins = false;
    display.section.enabled = true;
    display.section.plane = Some(SectionPlane { origin: [0.0, 0.0, 5.0], normal: [0.0, 0.0, 1.0], x_axis: [1.0, 0.0, 0.0] });
    let state = capture(&turntable(0.0, 0.0), &display);
    assert_eq!((state.display_mode.as_str(), state.grid, state.comment_pins, state.section.enabled), ("wireframe", false, false, true));
    let mut fresh = CadDisplay::default();
    apply_display(&state, &mut fresh).unwrap();
    assert_eq!((fresh.mode, fresh.grid, fresh.comment_pins), (DisplayMode::Wireframe, false, false));
    assert_eq!((fresh.section.enabled, fresh.section.plane), (true, display.section.plane));
    for mode in ["shaded", "shaded_edges", "wireframe", "xray", "matcap", "render"] {
        assert_eq!(convert::mode_name(convert::mode_of(mode).unwrap_or_else(|| panic!("{mode}"))), mode, "RoboCAD's names");
    }
    // A refused state changes nothing.
    let mut bad = state.clone();
    bad.pitch = 95.0;
    let before = fresh.clone();
    assert!(apply_display(&bad, &mut fresh).is_err() && camera_of(&bad).is_err());
    assert_eq!(fresh, before);
    let wide = ViewState { fov: 150.0, ..ViewState::default() };
    let (camera, note) = camera_of(&wide).unwrap();
    assert_eq!(camera.fov_deg, Some(120.0));
    assert!(note.is_some_and(|n| n.contains("150")));
}

fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    doc.client = Some(sim_runtime::cad_client::CadClient::new("http://127.0.0.1:8420").unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), revision: 4, ..Default::default() });
    doc.doc_key = Some((None, 4));
    doc
}

/// Every control fits `cad:view:<id>` (one segment) and its REST form
/// parses back to the action the panel's button writes.
#[test]
fn controls_fit_the_pattern_and_round_trip_through_rest() {
    let doc = document();
    let mut views = CadViews::default();
    views.listed = Some(((doc.generation, 4), vec![SavedView { id: "a1b2c3d4e5f6".into(), name: "Cutaway".into(), state: ViewState::default() }]));
    views.camera = Some(turntable(-35.0, 28.0));
    views.new_name = "Top detail".into();
    let controls = controls_of(&doc, Some(&views));
    let ids: Vec<&str> = controls.iter().map(|c| c.0.as_str()).collect();
    for want in ["cad:view:panel", "cad:view:list", "cad:view:save", "cad:view:a1b2c3d4e5f6", "cad:view:replace-a1b2c3d4e5f6", "cad:view:delete-a1b2c3d4e5f6"] {
        assert!(ids.contains(&want), "{want}: {ids:?}");
    }
    for (id, _, action, ready) in &controls {
        assert!(control_matches("cad:view:<id>", id), "{id}");
        assert_eq!(ready, &Ok(()), "{id}");
        let Value::Object(mut args) = crate::cad::rest_form::rest_form(action) else { panic!("{id}") };
        let name = args.remove("command").and_then(|v| v.as_str().map(str::to_string)).unwrap();
        assert_eq!(name, "cad_views");
        let parsed = <CadAction as Action>::parse(&sim_api::Command { command: name, args: Value::Object(args) }).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(&parsed, action, "{id}");
    }
    let save = controls.iter().find(|c| c.0 == "cad:view:save").unwrap();
    assert_eq!(save.2, ViewsArgs::of(ViewsOp::Save, None, Some("Top detail")));
    // No name typed, or no camera: save is refused by name.
    views.new_name.clear();
    let save = controls_of(&doc, Some(&views)).into_iter().find(|c| c.0 == "cad:view:save").unwrap();
    assert!(save.3.is_err_and(|e| e.contains("1–120")));
    views.new_name = "x".into();
    views.camera = None;
    let save = controls_of(&doc, Some(&views)).into_iter().find(|c| c.0 == "cad:view:save").unwrap();
    assert!(save.3.is_err_and(|e| e.contains("camera")));
    assert!(controls_of(&doc, None).is_empty(), "no window, no saved-view controls");
}

/// `cad_views` is a capability whose example parses; unknown fields are refused.
#[test]
fn cad_views_example_parses_and_unknown_fields_are_refused() {
    let spec = specs().into_iter().find(|s| s.name == "cad_views").expect("cad_views");
    let parsed = <CadAction as Action>::parse(&sim_api::Command { command: "cad_views".into(), args: spec.example.clone() }).unwrap();
    assert_eq!(parsed, ViewsArgs::of(ViewsOp::Save, None, Some("Worm drive cutaway")));
    let bad = <CadAction as Action>::parse(&sim_api::Command { command: "cad_views".into(), args: serde_json::json!({"op": "restore", "id": "a1", "state": {}}) });
    assert!(bad.is_err());
    let default = <CadAction as Action>::parse(&sim_api::Command { command: "cad_views".into(), args: serde_json::json!({}) }).unwrap();
    assert_eq!(default, CadAction::CadViews(ViewsArgs::default()), "op defaults to list");
}

/// A save keeps the typed name until RoboCAD answers: kept while the edit
/// is in flight and after a refusal, cleared once it succeeded.
#[test]
fn the_typed_name_is_cleared_only_when_the_save_succeeds() {
    use crate::cad::document::{Edit, EditDone};
    let mut doc = document();
    let mut views = CadViews { new_name: "Top detail".into(), ..CadViews::default() };
    let in_flight = |doc: &mut CadDocument| {
        doc.edit_seq += 1;
        doc.edit = Some(Edit { label: "Save view Top detail".into(), job: crate::jobs::Job::finished(0, Ok(EditDone { message: String::new(), result: Value::Null })), started: std::time::Instant::now(), clear_selection: None, activates_plane: false, retarget: None });
        doc.edit_seq
    };
    let seq = in_flight(&mut doc);
    views.saving = Some((doc.generation, seq, "Top detail".into()));
    super::settle_save(&mut views, &doc);
    assert_eq!((views.new_name.as_str(), views.saving.is_some()), ("Top detail", true), "in flight: kept");
    // RoboCAD refused it (finish_edit set the status): the name stays for another try.
    doc.edit = None;
    doc.status = Some(Err("RoboCAD answered 422: name taken".into()));
    super::settle_save(&mut views, &doc);
    assert_eq!((views.new_name.as_str(), views.saving.is_none()), ("Top detail", true));
    assert!(views.feedback.as_deref().is_some_and(|f| f.contains("422")));
    // Saved: cleared.
    let seq = in_flight(&mut doc);
    views.saving = Some((doc.generation, seq, "Top detail".into()));
    doc.edit = None;
    doc.status = Some(Ok("Saved view Top detail".into()));
    super::settle_save(&mut views, &doc);
    assert!(views.new_name.is_empty() && views.saving.is_none());
    // A name retyped meanwhile is the user's newer one and is kept.
    views.new_name = "Side".into();
    let seq = in_flight(&mut doc);
    views.saving = Some((doc.generation, seq, "Top detail".into()));
    doc.edit = None;
    super::settle_save(&mut views, &doc);
    assert_eq!(views.new_name, "Side");
}
