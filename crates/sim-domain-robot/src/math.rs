//! Small rigid-body helpers over nalgebra: quaternions in the `[w, x, y, z]`
//! order the frame connector uses, rotations and skew products.

use nalgebra::{Matrix3, Quaternion, UnitQuaternion, Vector3};

pub type V = Vector3<f64>;
pub type M = Matrix3<f64>;

pub fn v(a: [f64; 3]) -> V {
    Vector3::new(a[0], a[1], a[2])
}

pub fn quat(w: f64, x: f64, y: f64, z: f64) -> UnitQuaternion<f64> {
    UnitQuaternion::from_quaternion(Quaternion::new(w, x, y, z))
}

/// `[w, x, y, z]` of a unit quaternion.
pub fn quat_parts(q: &UnitQuaternion<f64>) -> [f64; 4] {
    [q.w, q.i, q.j, q.k]
}

/// Rotation about `axis` (unit) by `angle`.
pub fn rot_axis(axis: V, angle: f64) -> M {
    let (s, c) = angle.sin_cos();
    let k = skew(axis);
    M::identity() + k * s + k * k * (1.0 - c)
}

/// Small-angle rotation from a rotation vector (exact Rodrigues).
pub fn rot_vec(theta: V) -> M {
    let a = theta.norm();
    if a < 1e-12 {
        M::identity() + skew(theta)
    } else {
        rot_axis(theta / a, a)
    }
}

pub fn skew(a: V) -> M {
    M::new(0.0, -a.z, a.y, a.z, 0.0, -a.x, -a.y, a.x, 0.0)
}

/// An orthonormal frame whose z axis is `z`.
pub fn frame_from_z(z: V) -> M {
    let z = if z.norm() < 1e-12 { Vector3::z() } else { z.normalize() };
    let helper = if z.x.abs() < 0.9 { Vector3::x() } else { Vector3::y() };
    let x = helper.cross(&z).normalize();
    let y = z.cross(&x);
    M::from_columns(&[x, y, z])
}

/// `q̇` for body-frame angular velocity `w` (quaternion derivative).
pub fn quat_rate(q: &UnitQuaternion<f64>, w_body: V) -> [f64; 4] {
    let (qw, qx, qy, qz) = (q.w, q.i, q.j, q.k);
    let (wx, wy, wz) = (w_body.x, w_body.y, w_body.z);
    [0.5 * (-qx * wx - qy * wy - qz * wz), 0.5 * (qw * wx + qy * wz - qz * wy), 0.5 * (qw * wy - qx * wz + qz * wx), 0.5 * (qw * wz + qx * wy - qy * wx)]
}

pub fn m3(a: [[f64; 3]; 3]) -> M {
    M::new(a[0][0], a[0][1], a[0][2], a[1][0], a[1][1], a[1][2], a[2][0], a[2][1], a[2][2])
}
