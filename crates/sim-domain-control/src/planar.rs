//! Planar (SE(2)) body poses and twists shared by every walking reference:
//! pose = world x, y (m) and heading (rad); twist = body-frame forward and
//! lateral speed (m/s) and yaw rate (rad/s). Command-reference geometry only,
//! not the robot's dynamics.

/// Rotate a horizontal vector by `yaw` about +Z.
pub fn rotate(yaw: f64, v: [f64; 2]) -> [f64; 2] {
    let (s, c) = yaw.sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1]]
}

/// Integrate a constant body-frame planar twist exactly, including zero turn.
/// A negative `dt` integrates backwards along the same arc.
pub fn advance_planar(pose: [f64; 3], twist: [f64; 3], dt: f64) -> [f64; 3] {
    let angle = twist[2] * dt;
    let (a, b) = if angle.abs() < 1e-6 {
        (
            dt * (1. - angle * angle / 6.),
            dt * (angle / 2. - angle * angle * angle / 24.),
        )
    } else {
        (angle.sin() / twist[2], (1. - angle.cos()) / twist[2])
    };
    let delta = rotate(
        pose[2],
        [a * twist[0] - b * twist[1], b * twist[0] + a * twist[1]],
    );
    [pose[0] + delta[0], pose[1] + delta[1], pose[2] + angle]
}

/// A point given in a pose's frame (x, y rotated by heading; z unchanged),
/// expressed in world axes.
pub fn to_world(pose: [f64; 3], local: [f64; 3]) -> [f64; 3] {
    let xy = rotate(pose[2], [local[0], local[1]]);
    [pose[0] + xy[0], pose[1] + xy[1], local[2]]
}

/// World rotation vector of `rotation_vector` (world axes at zero heading)
/// after turning the whole body by `yaw` about +Z: Rz(yaw) * exp(r).
pub fn yaw_rotation_vector(yaw: f64, r: [f64; 3]) -> [f64; 3] {
    let angle = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
    let (s, c) = (0.5 * angle).sin_cos();
    let k = if angle < 1e-12 { 0.5 } else { s / angle };
    let q = [c, k * r[0], k * r[1], k * r[2]];
    let (zs, zc) = (0.5 * yaw).sin_cos();
    // [zc, 0, 0, zs] ⊗ q
    let p = [
        zc * q[0] - zs * q[3],
        zc * q[1] - zs * q[2],
        zc * q[2] + zs * q[1],
        zc * q[3] + zs * q[0],
    ];
    let (w, v) = if p[0] < 0. { (-p[0], [-p[1], -p[2], -p[3]]) } else { (p[0], [p[1], p[2], p[3]]) };
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let scale = if n < 1e-12 { 2. / w } else { 2. * n.atan2(w) / n };
    v.map(|x| x * scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arcs_small_angles_and_reversal() {
        let r = advance_planar([0.; 3], [1., 0., 1.], std::f64::consts::FRAC_PI_2);
        assert!((r[0] - 1.).abs() < 1e-12 && (r[1] - 1.).abs() < 1e-12);
        let r = advance_planar([0., 0., std::f64::consts::FRAC_PI_2], [1., 0., 0.], 2.);
        assert!(r[0].abs() < 1e-12 && (r[1] - 2.).abs() < 1e-12);
        let r = advance_planar([0.; 3], [1., 0., 1e-10], 2.);
        assert!((r[0] - 2.).abs() < 1e-12 && (r[1] - 2e-10).abs() < 1e-20);
        let there = advance_planar([0.1, -0.2, 0.3], [0.2, 0.05, 0.4], 1.7);
        let back = advance_planar(there, [0.2, 0.05, 0.4], -1.7);
        for i in 0..3 {
            assert!((back[i] - [0.1, -0.2, 0.3][i]).abs() < 1e-14);
        }
        assert_eq!(to_world([1., 2., std::f64::consts::FRAC_PI_2], [1., 0., 0.3])[2], 0.3);
    }
    #[test]
    fn yaw_composes_with_rotation_vectors() {
        assert_eq!(yaw_rotation_vector(0., [0.; 3]), [0.; 3]);
        let r = yaw_rotation_vector(0.3, [0., 0., 0.2]);
        assert!((r[2] - 0.5).abs() < 1e-15 && r[0] == 0. && r[1] == 0.);
        // Pitch about body y becomes pitch about the turned axis.
        let r = yaw_rotation_vector(std::f64::consts::FRAC_PI_2, [0., 1e-3, 0.]);
        let q = yaw_rotation_vector(0., [0., 1e-3, 0.]);
        assert!((q[1] - 1e-3).abs() < 1e-18);
        assert!(r[2] > 1.57 && r[2] < 1.571);
    }
}
