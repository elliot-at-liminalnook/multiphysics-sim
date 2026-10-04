//! A view state composed from what an agent names instead of a camera it
//! cannot see: `fit` (the parts to frame, with everything under them; empty:
//! every shown body), `direction` (front, back, left, right, top, bottom,
//! iso) or `yaw` / `pitch`, a `section` slice (`{axis: x | y | z, offset}`
//! through the framed parts' centre, or `{origin, normal}`), `display_mode`
//! and `orthographic`, over a whole `state` or the window's camera. The
//! result is RoboCAD's view-state schema (`sim_cad::saved_views::validate_state`).
//!
//! Conventions (RoboCAD's): millimetres, Z up; yaw 0 puts the eye on +X,
//! 90 on +Y; pitch is the eye's elevation. A section keeps
//! `dot(n, p − o) ≤ 0`; without a direction the eye looks at the cut face
//! from the removed side (along +n).
use crate::cad::sync::LocalSnapshot;
use serde_json::{Value, json};

/// What `save`, `replace` and `update` take to compose a state.
#[derive(Clone, Debug, Default)]
pub(crate) struct Compose<'a> {
    pub state: Option<&'a Value>,
    pub fit: Option<&'a [String]>,
    pub direction: Option<&'a str>,
    pub yaw: Option<f64>,
    pub pitch: Option<f64>,
    pub section: Option<&'a Value>,
    pub display_mode: Option<&'a str>,
    pub orthographic: Option<bool>,
}
impl Compose<'_> {
    /// Whether anything beyond the window's camera is asked for.
    pub(crate) fn any(&self) -> bool {
        self.state.is_some() || self.fit.is_some() || self.direction.is_some() || self.yaw.is_some() || self.pitch.is_some() || self.section.is_some() || self.display_mode.is_some() || self.orthographic.is_some()
    }
}

/// `ids` and every node under them (all geometry nodes when empty).
fn with_children(local: &LocalSnapshot, ids: &[String]) -> Vec<String> {
    let nodes = &local.tree.nodes;
    if ids.is_empty() {
        return nodes.iter().filter(|n| n.effective_visible).map(|n| n.id.clone()).collect();
    }
    let mut out: Vec<String> = ids.to_vec();
    for n in nodes {
        if n.parent.as_ref().is_some_and(|p| out.contains(p)) && !out.contains(&n.id) {
            out.push(n.id.clone());
        }
    }
    out
}

/// The bounding box (mm) of the parts' exact tessellation.
pub(crate) fn bounds(local: &LocalSnapshot, ids: &[String]) -> Result<([f64; 3], [f64; 3]), String> {
    for id in ids {
        if local.archive.node(id).is_none() {
            return Err(format!("part {id} does not exist"));
        }
    }
    let wanted = with_children(local, ids);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for body in local.geometry.iter().filter(|b| wanted.contains(&b.node_id)) {
        for v in &body.vertices_mm {
            for i in 0..3 {
                lo[i] = lo[i].min(v[i]);
                hi[i] = hi[i].max(v[i]);
            }
        }
    }
    if !lo[0].is_finite() {
        return Err("nothing to frame: those parts have no drawn geometry".into());
    }
    Ok((lo, hi))
}

fn vec3(v: &Value, what: &str) -> Result<[f64; 3], String> {
    let a = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| format!("{what} must be [x, y, z]"))?;
    let mut out = [0.; 3];
    for i in 0..3 {
        out[i] = a[i].as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("{what} must be three finite numbers"))?;
    }
    Ok(out)
}

/// The eye direction's yaw and pitch (degrees).
fn angles(e: [f64; 3]) -> (f64, f64) {
    let l = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
    let e = e.map(|v| v / l);
    let pitch = e[2].clamp(-1., 1.).asin().to_degrees().clamp(-89.5, 89.5);
    let yaw = if e[0].abs() < 1e-9 && e[1].abs() < 1e-9 { -90. } else { e[1].atan2(e[0]).to_degrees() };
    (yaw, pitch)
}

fn preset(name: &str) -> Result<(f64, f64), String> {
    Ok(match name {
        "front" => (-90., 0.),
        "back" => (90., 0.),
        "right" => (0., 0.),
        "left" => (180., 0.),
        "top" => (-90., 89.5),
        "bottom" => (-90., -89.5),
        "iso" => (-35., 28.),
        other => return Err(format!("direction must be front, back, left, right, top, bottom or iso, not {other}")),
    })
}

/// The composed, validated state. `base`: the window's camera as a state
/// (None without a window); RoboCAD's defaults when neither it nor `state`.
pub(crate) fn compose(local: &LocalSnapshot, base: Option<Value>, c: &Compose) -> Result<Value, String> {
    let mut state = match (c.state, base) {
        (Some(s), _) => sim_cad::saved_views::validate_state(s)?,
        (None, Some(b)) => sim_cad::saved_views::validate_state(&b)?,
        (None, None) => sim_cad::saved_views::validate_state(&json!({}))?,
    };
    // The framed region's centre is also where an axis section goes through.
    let mut centre = None;
    if let Some(ids) = c.fit {
        let (lo, hi) = bounds(local, ids)?;
        let mid = [(lo[0] + hi[0]) / 2., (lo[1] + hi[1]) / 2., (lo[2] + hi[2]) / 2.];
        let radius = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt() / 2.;
        let fov = state["fov"].as_f64().unwrap_or(40.).to_radians();
        state["target"] = json!(mid);
        state["distance"] = json!((radius.max(1.) / (fov / 2.).sin() * 1.15 * 1000.).round() / 1000.);
        centre = Some(mid);
    }
    let mut eye_from_section = None;
    if let Some(sec) = c.section.filter(|s| s.get("axis").is_none() && s.get("origin").is_none() && s.get("enabled") == Some(&json!(false))) {
        // `{enabled: false}`: the slice off, its plane kept for turning it back on.
        state["section"]["enabled"] = json!(false);
        let _ = sec;
    } else if let Some(sec) = c.section {
        let plane = if let Some(axis) = sec.get("axis") {
            let normal = match axis.as_str() {
                Some("x") => [1., 0., 0.],
                Some("y") => [0., 1., 0.],
                Some("z") => [0., 0., 1.],
                _ => return Err("section axis must be x, y or z".into()),
            };
            let flip = sec.get("flip").and_then(Value::as_bool).unwrap_or(false);
            let normal = if flip { normal.map(|v: f64| -v) } else { normal };
            let offset = sec.get("offset").map_or(Ok(0.), |o| o.as_f64().filter(|v| v.is_finite()).ok_or("section offset must be a number of millimetres"))?;
            let through = match centre {
                Some(m) => m,
                None => vec3(&state["target"], "target")?,
            };
            let axis_index = normal.iter().position(|v| v.abs() > 0.5).expect("an axis");
            let mut origin = through;
            origin[axis_index] += offset;
            sim_cad::saved_views::section_plane(origin, normal)?
        } else {
            let origin = vec3(sec.get("origin").ok_or("section needs axis, or origin and normal")?, "section origin")?;
            let normal = vec3(sec.get("normal").ok_or("section needs a normal with its origin")?, "section normal")?;
            sim_cad::saved_views::section_plane(origin, normal)?
        };
        eye_from_section = Some(vec3(&plane["normal"], "normal")?);
        state["section"] = json!({"enabled": sec.get("enabled").and_then(Value::as_bool).unwrap_or(true), "plane": plane});
    }
    let (mut yaw, mut pitch) = (state["yaw"].as_f64().unwrap_or(-35.), state["pitch"].as_f64().unwrap_or(28.));
    if let Some(d) = c.direction {
        (yaw, pitch) = preset(d)?;
    } else if let Some(n) = eye_from_section.filter(|_| c.yaw.is_none() && c.pitch.is_none()) {
        (yaw, pitch) = angles(n);
    }
    if let Some(y) = c.yaw {
        yaw = y;
    }
    if let Some(p) = c.pitch {
        pitch = p;
    }
    state["yaw"] = json!(yaw);
    state["pitch"] = json!(pitch);
    if c.direction.is_some() || c.yaw.is_some() || c.pitch.is_some() || eye_from_section.is_some() {
        // Turntable angles: a trackball rotation would override them.
        state["mode"] = json!("turntable");
    }
    if let Some(m) = c.display_mode {
        state["display_mode"] = json!(m);
    }
    if let Some(o) = c.orthographic {
        state["orthographic"] = json!(o);
    }
    sim_cad::saved_views::validate_state(&state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_section_normal_becomes_the_eye_direction() {
        assert_eq!(angles([0., 1., 0.]), (90., 0.));
        let (yaw, pitch) = angles([0., 0., 1.]);
        assert_eq!((yaw, pitch), (-90., 89.5));
        assert!(preset("sideways").is_err());
    }
}
