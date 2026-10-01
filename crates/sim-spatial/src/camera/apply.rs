//! The one handler of [`CameraAction`] (`ViewerSet::Actions`), the shared
//! camera's `system_ui` controls ([`controls`]) and their inverse
//! ([`control_action`]).
use super::input::cursor_anchor;
use super::orbit::aspect;
use super::{CameraAction, CameraState, Orbit, OrbitMode, OrbitRules, Pose, ViewArea, ViewPreset, state_json};
use crate::app::actions::{self, Act, InFlight, Replies};
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use serde_json::{Value, json};
use sim_api::Outcome;

/// RoboCAD's field-of-view range, degrees (`ui/app.py` `set_fov`:
/// `QInputDialog.getDouble(..., fov, 5, 120, 1)`).
pub(super) const FOV_DEG: std::ops::RangeInclusive<f32> = 5.0..=120.0;

const NO_ORBIT: &str = "this mode has no orbit camera (Place mode flies: use camera)";

fn finite(name: &str, values: &[f32]) -> Result<(), String> {
    if values.iter().all(|v| v.is_finite()) { Ok(()) } else { Err(format!("{name} must be finite numbers")) }
}

fn fov(degrees: f32, name: &str) -> Result<f32, String> {
    if degrees.is_finite() && FOV_DEG.contains(&degrees) {
        Ok(degrees.to_radians())
    } else {
        Err(format!("{name} must be {}–{} degrees (RoboCAD's field-of-view range); got {degrees}", FOV_DEG.start(), FOV_DEG.end()))
    }
}

/// `camera_set`'s state, checked before anything changes.
fn checked(state: &CameraState) -> Result<(Pose, Option<f32>, Option<Quat>, f32), String> {
    finite("focus", &state.focus)?;
    finite("yaw and pitch", &[state.yaw, state.pitch])?;
    if !(state.radius.is_finite() && state.radius > 0.0) {
        return Err(format!("radius must be a positive distance (metres); got {}", state.radius));
    }
    let fov = state.fov_deg.map(|d| fov(d, "fov_deg")).transpose()?;
    let trackball = match state.trackball {
        None => None,
        Some(q) => {
            finite("trackball", &q)?;
            let q = Quat::from_array(q);
            if q.length() < 1e-6 {
                return Err("trackball must be a rotation quaternion [x, y, z, w]; it has zero length".into());
            }
            Some(q.normalize())
        }
    };
    let seconds = state.seconds.unwrap_or(0.0);
    if !(seconds.is_finite() && seconds >= 0.0) {
        return Err(format!("seconds must be 0 or more; got {seconds}"));
    }
    Ok((Pose { focus: Vec3::from_array(state.focus), radius: state.radius, yaw: state.yaw, pitch: state.pitch }, fov, trackball, seconds))
}

/// One camera action on the mode's orbit camera; the answer is the camera's
/// state after it (`{"camera": …}`).
fn handle(action: &CameraAction, orbit: &mut Mut<Orbit>, rules: &OrbitRules, area: &ViewArea, camera: &Camera) -> Result<Value, String> {
    let mut extra = serde_json::Map::new();
    match action {
        CameraAction::State => return Ok(json!({"camera": state_json(orbit, Some(area))})),
        CameraAction::View { view } => {
            orbit.interrupt();
            // RoboCAD's `set_view` cuts.
            orbit.preset(*view, rules, 0.0);
        }
        CameraAction::Opposite => orbit.opposite(rules),
        CameraAction::Projection { orthographic } => {
            orbit.interrupt();
            orbit.orthographic = orthographic.unwrap_or(!orbit.orthographic);
        }
        CameraAction::Fov { degrees } => {
            let fov = fov(*degrees, "degrees")?;
            orbit.interrupt();
            orbit.fov = fov;
        }
        CameraAction::Fit => {
            orbit.interrupt();
            orbit.frame(rules, aspect(camera), false);
        }
        CameraAction::Home => {
            orbit.interrupt();
            // `place` frames it this frame, after the mode's SimSync work has written the bounds.
            orbit.home = true;
        }
        CameraAction::Pan { dx, dy } => {
            finite("dx and dy", &[*dx, *dy])?;
            orbit.interrupt();
            orbit.pan(Vec2::new(*dx, *dy), rules);
        }
        CameraAction::Orbit { dx, dy, degrees } => {
            finite("dx and dy", &[*dx, *dy])?;
            match degrees {
                Some([yaw, pitch]) => {
                    if *dx != 0.0 || *dy != 0.0 {
                        return Err("give dx, dy (window pixels) or degrees [yaw, pitch], not both".into());
                    }
                    finite("degrees", &[*yaw, *pitch])?;
                    orbit.interrupt();
                    orbit.rotate_by(yaw.to_radians(), pitch.to_radians(), rules);
                }
                None => {
                    orbit.interrupt();
                    orbit.rotate(Vec2::new(*dx, *dy), rules);
                }
            }
        }
        CameraAction::Zoom { factor, at } => {
            if !(factor.is_finite() && *factor > 0.0) {
                return Err(format!("factor must be a positive number (< 1 zooms in); got {factor}"));
            }
            let at = match at {
                Some(p) => {
                    finite("at", p)?;
                    Some(Vec2::from_array(*p))
                }
                None => None,
            };
            let anchor = match at {
                Some(p) => cursor_anchor(orbit, camera, p),
                None => None,
            };
            orbit.interrupt();
            let applied = orbit.zoom(*factor, anchor, rules);
            extra.insert("applied".into(), json!(applied));
            extra.insert("anchor".into(), json!(anchor.map(|a| a.to_array())));
            if at.is_some() && anchor.is_none() {
                extra.insert("note".into(), json!("no point of the view lies under `at` (outside the view, or the view has no size yet): zoomed toward the focus"));
            }
        }
        CameraAction::OrbitMode { mode } => {
            let on = match mode {
                Some(OrbitMode::Trackball) => true,
                Some(OrbitMode::Turntable) => false,
                None => orbit.trackball.is_none(),
            };
            orbit.interrupt();
            orbit.set_trackball(on, rules);
        }
        CameraAction::Spin { rate } => {
            if !rate.is_finite() {
                return Err(format!("rate must be a finite number (rad/s); got {rate}"));
            }
            if *rate != 0.0 && rules.reduced_motion {
                return Err("reduced motion is on: the camera does not spin".into());
            }
            if *rate != 0.0 && orbit.trackball.is_some() {
                return Err("spin circles the turntable: switch to it first (camera_orbit_mode {\"mode\":\"turntable\"})".into());
            }
            orbit.interrupt();
            orbit.spin = *rate;
        }
        CameraAction::Set { state } => {
            let (pose, fov, trackball, seconds) = checked(state)?;
            let pose = Pose { pitch: pose.pitch.clamp(-rules.pitch_limit, rules.pitch_limit), ..pose };
            orbit.interrupt();
            orbit.glide_to(pose, if rules.reduced_motion { 0.0 } else { seconds });
            orbit.trackball = trackball;
            orbit.orthographic = state.orthographic;
            if let Some(fov) = fov {
                orbit.fov = fov;
            }
        }
    }
    extra.insert("camera".into(), state_json(orbit, Some(area)));
    Ok(Value::Object(extra))
}

/// Actions: every camera action, on the mode's orbit camera (the active one
/// when a mode has several). Refusals from a key or click are logged (the
/// camera has no status line of its own); REST gets them as its answer.
pub(super) fn apply(mut messages: ResMut<Messages<Act<CameraAction>>>, mut in_flight: ResMut<InFlight<CameraAction>>, mut replies: ResMut<Replies>, mut cameras: Query<(Entity, &mut Orbit, &OrbitRules, &ViewArea, &Camera)>) {
    if messages.is_empty() && in_flight.is_empty() {
        return;
    }
    let target = cameras.iter().find(|(.., camera)| camera.is_active).or_else(|| cameras.iter().next()).map(|(entity, ..)| entity);
    actions::apply(&mut messages, &mut in_flight, &mut replies, |action, call| {
        let result = match target.and_then(|e| cameras.get_mut(e).ok()) {
            Some((_, mut orbit, rules, area, camera)) => handle(action, &mut orbit, rules, area, camera),
            None => Err(NO_ORBIT.to_string()),
        };
        if let (Err(e), actions::Origin::Ui) = (&result, call.origin) {
            bevy::log::warn!("camera: {e}");
        }
        Outcome::Done(result)
    });
}

/// The shared camera's `system_ui` controls (every orbit mode lists them
/// after its own): each with its REST form as the action.
pub fn controls() -> Vec<Value> {
    let mut out: Vec<(String, String, CameraAction)> = ViewPreset::ALL.iter().map(|v| (format!("camera:view:{}", v.name()), format!("View {}", v.name()), CameraAction::View { view: *v })).collect();
    out.push(("camera:opposite".into(), "Opposite view".into(), CameraAction::Opposite));
    out.push(("camera:projection".into(), "Orthographic / perspective".into(), CameraAction::Projection { orthographic: None }));
    out.push(("camera:fit".into(), "Fit".into(), CameraAction::Fit));
    out.push(("camera:home".into(), "Home view".into(), CameraAction::Home));
    out.push(("camera:orbit_mode".into(), "Orbit: turntable / trackball".into(), CameraAction::OrbitMode { mode: None }));
    out.into_iter()
        .map(|(id, label, action)| {
            let rest = serde_json::to_value(&action).unwrap_or_else(|e| json!({"error": e.to_string()}));
            json!({"id": id, "label": label, "enabled": true, "disabled_reason": null, "action": rest})
        })
        .collect()
}

/// A `camera:*` control id as its action (`CameraAction::parse` for
/// `system_ui` activations).
pub(super) fn control_action(id: &str) -> Option<CameraAction> {
    let rest = id.strip_prefix("camera:")?;
    if let Some(name) = rest.strip_prefix("view:") {
        return ViewPreset::ALL.into_iter().find(|v| v.name() == name).map(|view| CameraAction::View { view });
    }
    Some(match rest {
        "opposite" => CameraAction::Opposite,
        "projection" => CameraAction::Projection { orthographic: None },
        "fit" => CameraAction::Fit,
        "home" => CameraAction::Home,
        "orbit_mode" => CameraAction::OrbitMode { mode: None },
        _ => return None,
    })
}
