//! Align view (references.py:195-209) and Sketch over this (:216-219).
//!
//! Both use the placement read at the shown revision
//! (`reads::current_placement`): an older read is refused by name.
//!
//! **Align view** is display only. RoboCAD sets `vp.active_plane = p`, a
//! trackball camera with rows `[x_axis, y_axis, normal]` (right, up, back),
//! orthographic, target `p.to_world(w/2, h/2)` and distance
//! `max(h, w / aspect) · 0.6 / tan(fov / 2)` (aspect: the viewport's width
//! over its height). Here the same camera goes to the shared camera as one
//! `CameraAction::Set` (a cut, as RoboCAD's), mapped to the display frame as
//! `views::convert` maps a saved view (model (x, y, z) mm is display (x, z,
//! −y) m; the rotation's columns are the rows mapped), with the turntable
//! heading of the normal for its yaw and pitch. The orthographic view's
//! height is `2 · radius · tan(fov / 2)` as RoboCAD's (`camera::Orbit`), so
//! the formula carries over. The field of view is the camera's (`CadViews`'
//! snapshot), the aspect `CadView`'s 3D view rectangle.
//!
//! **The active plane** becomes the image's plane when the native active
//! plane can name it: one of XY, XZ, YZ, or a plane node, geometrically
//! (the image lies in it; the native active plane is a named plane or a plane
//! node, and the sketch tools take only those). Otherwise it is left as it
//! is and the answer says so; **Sketch over this** is then refused by name
//! rather than sketching on another plane.
use super::reads;
use crate::app::actions::Call;
use crate::cad::actions::{CadAction, Cx};
use crate::cad::sketch::{ActivePlane, BasePlane, CadActivePlane, CadSketches};
use crate::camera::{CameraAction, CameraState, robocad_to_display};
use crate::cad::document::CadDocument;
use bevy::math::{DMat3, DQuat, DVec3};
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{ImagePlacement, PlaneFrame};

/// RoboCAD's camera pitch limit (degrees).
const PITCH_LIMIT: f64 = 89.5;

/// Model direction → display direction: (x, y, z) → (x, z, −y) (`views::convert`).
fn m_to_d(v: DVec3) -> DVec3 {
    DVec3::new(v.x, v.z, -v.y)
}

/// The camera RoboCAD's `align` sets for `p`, on a view of `aspect`
/// (width / height) with vertical field of view `fov` (radians).
pub(crate) fn camera(p: &ImagePlacement, aspect: f64, fov: f64) -> CameraState {
    let n = DVec3::from_array(p.plane.normal).normalize_or(DVec3::Z);
    // `x_axis` as stored, made square to the normal (RoboCAD's are already).
    let x = DVec3::from_array(p.plane.x_axis).reject_from(n).normalize_or(n.any_orthonormal_vector());
    let y = n.cross(x);
    let target = p.plane.to_world(p.width / 2.0, p.height / 2.0);
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 };
    let distance = p.height.max(p.width / aspect) * 0.6 / (fov / 2.0).tan();
    let rotation = DQuat::from_mat3(&DMat3::from_cols(m_to_d(x), m_to_d(y), m_to_d(n))).normalize();
    // The turntable heading of the view's back vector (`views::convert::capture`).
    let yaw = n.y.atan2(n.x).to_degrees();
    let pitch = n.z.clamp(-1.0, 1.0).asin().to_degrees().clamp(-PITCH_LIMIT, PITCH_LIMIT);
    let (yaw, pitch) = robocad_to_display(yaw as f32, pitch as f32);
    CameraState {
        focus: (m_to_d(DVec3::from_array(target)) / 1000.0).as_vec3().to_array(),
        radius: (distance / 1000.0) as f32,
        yaw,
        pitch,
        orthographic: true,
        fov_deg: None,
        trackball: Some(rotation.as_quat().to_array()),
        seconds: None,
    }
}

/// The native active plane naming `p`'s plane: XY, XZ or YZ, else a plane
/// node whose frame (at the shown revision) it lies in.
pub(crate) fn plane_for(p: &ImagePlacement, doc: &CadDocument, sketches: Option<&CadSketches>) -> Option<ActivePlane> {
    let o = DVec3::from_array(p.plane.origin);
    let n = DVec3::from_array(p.plane.normal).normalize_or_zero();
    let same = |f: &PlaneFrame| {
        let m = DVec3::from_array(f.normal).normalize_or_zero();
        let scale = 1.0 + o.abs().max_element();
        m.dot(n).abs() > 1.0 - 1e-9 && (o - DVec3::from_array(f.origin)).dot(m).abs() <= 1e-6 * scale
    };
    if let Some(b) = [BasePlane::Xy, BasePlane::Xz, BasePlane::Yz].into_iter().find(|b| same(&b.frame())) {
        return Some(ActivePlane::Base(b));
    }
    let sketches = sketches?;
    let nodes = doc.doc.as_ref()?.nodes.iter().filter(|n| n.kind == "plane");
    nodes.filter_map(|node| sketches.plane(&node.id).map(|f| (node.id.clone(), f))).find(|(_, f)| same(f)).map(|(id, f)| ActivePlane::Node { id, frame: Some(f) })
}

/// Align view on image `id` (see the module doc).
pub(crate) fn align(cx: &mut Cx, id: &str) -> Result<Value, String> {
    let p = reads::current_placement(cx.doc, id)?.clone();
    let aspect = cx.view.filter(|v| v.valid && v.size.y > 0.0).map_or(1.0, |v| f64::from(v.size.x) / f64::from(v.size.y.max(1.0)));
    let fov = cx.views.as_deref().and_then(|v| v.camera).map_or(std::f64::consts::FRAC_PI_4, |c| f64::from(c.fov));
    let state = camera(&p, aspect, fov);
    cx.camera.push(CameraAction::Set { state });
    let name = cx.doc.node_name(id);
    let plane = plane_for(&p, cx.doc, cx.sketches);
    let note = match &plane {
        Some(active) => {
            set_active(cx.plane, cx.doc, active.clone());
            None
        }
        None => Some(format!("{name} is not on XY, XZ, YZ or a plane node, so the active plane is unchanged")),
    };
    let message = match &note {
        None => format!("Aligned the view to {name}; its plane is the active plane"),
        Some(n) => format!("Aligned the view to {name}; {n}"),
    };
    cx.doc.show(Ok(message.clone()));
    Ok(json!({"aligned": id, "camera": state, "active_plane": cx.plane.label(cx.doc), "note": note, "message": message, "display_only": true}))
}

/// The active plane becomes `plane` (display state, as `sketch::plane::view_act`).
fn set_active(plane: &mut CadActivePlane, doc: &CadDocument, active: ActivePlane) {
    if plane.generation != doc.generation {
        *plane = CadActivePlane { generation: doc.generation, ..Default::default() };
    }
    if plane.plane.as_ref() != Some(&active) {
        plane.plane = Some(active);
    }
}

/// Sketch over this: align, then the Line sketch tool (`sketch.line`) on
/// the image's plane, refused by name when that plane cannot be the active one.
pub(crate) fn sketch(args: &super::ReferencesArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let id = match super::target(cx.doc, args.id.as_ref()) {
        Ok(id) => id,
        Err(e) => return Outcome::Done(Err(e)),
    };
    let p = match reads::current_placement(cx.doc, &id) {
        Ok(p) => p.clone(),
        Err(e) => return Outcome::Done(Err(e)),
    };
    if plane_for(&p, cx.doc, cx.sketches).is_none() {
        return Outcome::Done(Err(format!("Sketch over this: {} is not on XY, XZ, YZ or a plane node, and the sketch tools draw on those only; move it with Apply placement (Front, Side or Top) or add a plane node through it", cx.doc.node_name(&id))));
    }
    if let Err(e) = align(cx, &id) {
        return Outcome::Done(Err(e));
    }
    crate::cad::actions::handle(&CadAction::CadInvoke { id: "sketch.line".into() }, call, cx)
}
