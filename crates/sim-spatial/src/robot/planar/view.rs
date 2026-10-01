//! Drawing and text for a planar file: gizmos, bounds, the header run line and the inspector.
use super::{FIDELITY, JOG_RULE, PACING, PlanarFrame, PlanarView};
use bevy::prelude::*;

/// The palette of body outlines (as the planar viewer drew them).
const PALETTE: [Color; 6] = [Color::srgb(0.9, 0.55, 0.2), Color::srgb(0.3, 0.7, 0.95), Color::srgb(0.4, 0.85, 0.5), Color::srgb(0.9, 0.35, 0.4), Color::srgb(0.8, 0.8, 0.3), Color::srgb(0.7, 0.5, 0.9)];

/// The plane's point (x right, y up, m) in the display frame. Robot mode's
/// display frame is Y up with the model's Z-up frame rotated by RobotRoot
/// (model (x, y, z) → display (x, z, −y)); the planar working plane is the
/// model's XZ plane (RoboCAD `Plane.xz()`, the export default), so a plane
/// point (u, v) is model (u, 0, v) and display (u, v, 0).
pub fn display(p: [f64; 2]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, 0.0)
}

/// Gizmos for the latest accepted frame: ground grid, outlines (closed, one
/// colour per body; the selected body's in the accent colour), centres of
/// mass, and red chain-tip dots while contacts are on.
pub fn draw(view: &PlanarView, selected: Option<usize>, gizmos: &mut Gizmos) {
    // Ground at display y = 0: the planar build puts the model's lowest point there.
    gizmos.grid(Isometry3d::new(Vec3::ZERO, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), UVec2::splat(40), Vec2::splat(0.05), Color::srgba(0.45, 0.50, 0.58, 0.35));
    let Some(f) = view.run.frame().filter(|f| f.built) else { return };
    for (body, points) in &f.outlines {
        let color = if selected == Some(*body) { crate::ui_kit::ACCENT } else { PALETTE[body % PALETTE.len()] };
        let points: Vec<Vec3> = points.iter().copied().map(display).collect();
        if let (Some(first), Some(last)) = (points.first().copied(), points.last().copied()) {
            gizmos.linestrip(points, color);
            gizmos.line(last, first, color);
        }
    }
    for pose in &f.poses {
        let color = if selected == Some(pose.body) { crate::ui_kit::ACCENT } else { Color::WHITE };
        gizmos.circle(Isometry3d::from_translation(display(pose.com) + Vec3::Z * 0.001), 0.004, color);
    }
    if view.contacts {
        for tip in &f.tips {
            gizmos.circle(Isometry3d::from_translation(display(*tip) + Vec3::Z * 0.001), 0.006, Color::srgb(1.0, 0.3, 0.3));
        }
    }
}

/// Display-frame bounds of a frame's outlines, centres and tips (None when empty).
pub fn bounds(f: &PlanarFrame) -> Option<(Vec3, Vec3)> {
    let points = f.outlines.iter().flat_map(|(_, p)| p.iter().copied()).chain(f.poses.iter().map(|p| p.com)).chain(f.tips.iter().copied());
    let (lo, hi) = points.map(display).fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), p| (lo.min(p), hi.max(p)));
    lo.x.is_finite().then_some((lo, hi))
}

/// The header's run line for a planar file.
pub fn run_line(view: &PlanarView) -> String {
    let r = &view.run;
    let f = r.frame().filter(|f| f.built);
    let time = f.map_or("t —".to_string(), |f| format!("t {:.2} s · {} grid steps", f.time, f.steps));
    let rate = f.and_then(|f| f.achieved_rate).map_or(String::new(), |x| format!(" · {x:.2}× real time achieved"));
    let error = r.frame().and_then(|f| f.error.as_deref()).map_or(String::new(), |e| format!(" · {}", clip(e, 50)));
    format!("{} · {time} · ×{}{rate} · gen {} · planar v2{error}", r.phase().name(), r.speed_scale(), r.generation())
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

/// The inspector text for a planar file (the section names robot mode's tabs: Link → bodies, Joints, Drives → motors, Source).
pub fn inspector_text(view: &PlanarView, section: &str, selected: Option<usize>, watch: &str) -> String {
    let m = &view.loaded.model;
    let f = view.run.frame().filter(|f| f.built);
    let mut t = format!("PLANAR V2 SUMMARY (not the v3 physical model)\n{FIDELITY}\n\n");
    if let Some(e) = view.run.frame().and_then(|f| f.error.as_deref()) {
        t += &format!("RUN FAILED: {e}\n\n");
    }
    for w in f.map(|f| f.warnings.as_slice()).unwrap_or_default() {
        t += &format!("warning (planar build): {w}\n");
    }
    match section {
        "joints" => {
            let names = view.joint_names();
            t += &format!("JOINTS — {} in file, {} simulated (CAD working plane; pivots in mm, angles in rad)\n{JOG_RULE}\nKeys: ←/→ select joint · ↑/↓ move its target (Shift: ×5)\n", m.joints.len(), names.len());
            for j in &m.joints {
                let built = names.iter().position(|n| n == &j.name);
                let marker = if built.is_some() && built == Some(view.selected_joint) { "▸ " } else { "  " };
                t += &format!("\n{marker}{} — {} · {} → {}\n    pivot {:?} mm · axis sign {} · damping {}\n", j.name, j.kind, j.parent.as_deref().unwrap_or("(root)"), j.child, j.pivot2, j.axis_sign, j.damping);
                t += &match &j.limits {
                    Some(l) => format!("    limits (rad, file): {}\n", l.iter().map(|x| x.map_or("none".into(), |v| format!("{v}"))).collect::<Vec<_>>().join(", ")),
                    None => "    limits: none in file\n".into(),
                };
                t += &match (built, f) {
                    (Some(i), Some(f)) => format!("    angle {:+.2}° → target {:+.2}° (relative to the parent link)\n", f.joint_angles.get(i).copied().unwrap_or(f64::NAN).to_degrees(), f.targets.get(i).copied().unwrap_or(f64::NAN).to_degrees()),
                    (None, Some(_)) => "    not simulated (fixed type, or not reachable from the root)\n".into(),
                    _ => "    (building)\n".into(),
                };
            }
        }
        "drives" => {
            t += "MOTORS (per joint, as exported: torque and speed at the joint, gear ratio applied)\n";
            let mut any = false;
            for j in &m.joints {
                if let Some(x) = &j.motor {
                    any = true;
                    t += &format!("• {} on {} — spec {} · stall {} N·m · no-load {} rad/s · rotor inertia {} kg·m²\n", x.name, j.name, if x.spec.is_empty() { "(none)" } else { x.spec.as_str() }, x.stall_torque, x.no_load_speed, x.rotor_inertia);
                }
            }
            if !any {
                t += "none in file (joints without a motor are held by the PD servo without a torque-speed cap)\n";
            }
        }
        "source" => {
            t += watch;
            t += &format!("FILE (as read)\nversion: {} (read as {} by simrobot_version)\nunit: {}\nplane: {}\nsource: {}\n",
                view.loaded.declared_version.map_or("absent".into(), |v| v.to_string()), view.loaded.version, view.loaded.unit.as_deref().unwrap_or("absent (read as mm)"),
                view.loaded.plane, m.source.as_deref().unwrap_or("not recorded"));
            t += &format!("\nRUN\n{PACING}\n");
        }
        _ => match selected.and_then(|i| Some((i, m.bodies.get(i)?))) {
            None => {
                let root = f.and_then(|f| f.root.clone()).map_or("—".into(), |r| format!("{r}{}", if f.is_some_and(|f| f.root_fixed) { " (fixed)" } else { " (free)" }));
                t += &format!("Select a body in the list.\n\n{} bodies · {} joints in file · root {root}\n", m.bodies.len(), m.joints.len());
            }
            Some((i, b)) => {
                t += &format!("{}   (body {} of {})\n\nmass: {} kg\ncom: {:?} mm (working plane)\ninertia about the plane normal: {} kg·m²\nmaterial: {}\nground: {}\noutline: {} loop(s)\n",
                    b.name, i + 1, m.bodies.len(), b.mass_kg, b.com, b.inertia_zz, b.material.as_deref().unwrap_or("none recorded"), if b.ground { "yes (fixed root)" } else { "no" }, b.outline.len());
                if let Some(p) = f.and_then(|f| f.poses.iter().find(|p| p.body == i)) {
                    t += &format!("simulated com ({:.4}, {:.4}) m · angle {:+.2}° from the CAD pose\n", p.com[0], p.com[1], p.angle.to_degrees());
                }
            }
        },
    }
    t
}
