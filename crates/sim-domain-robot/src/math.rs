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

/// Spatial angular velocity/acceleration for R=exp(skew(phi))*R_initial.
/// Rotation-vector rates are coordinate derivatives, not angular velocity.
pub fn rotation_vector_motion(phi: V, rate: V, acceleration: V) -> Result<(V,V),String> {
    if phi.iter().chain(rate.iter()).chain(acceleration.iter()).any(|v|!v.is_finite()) {
        return Err("finite rotation vector and derivatives required".into());
    }
    let theta=phi.norm();let dot=phi.dot(&rate);
    let (a,b,ad,bd)=if theta<1e-3 {
        let s=theta*theta;
        (0.5-s/24.0+s*s/720.0,1.0/6.0-s/120.0+s*s/5040.0,
         (-1.0/12.0+s/180.0-s*s/6720.0)*dot,(-1.0/60.0+s/1260.0-s*s/60480.0)*dot)
    }else{
        let (sin,cos)=theta.sin_cos();
        ((1.0-cos)/(theta*theta),(theta-sin)/theta.powi(3),
         (theta*sin-2.0*(1.0-cos))/theta.powi(4)*dot,
         (theta*(1.0-cos)-3.0*(theta-sin))/theta.powi(5)*dot)
    };
    let omega=rate+a*phi.cross(&rate)+b*phi.cross(&phi.cross(&rate));
    let alpha=acceleration+a*phi.cross(&acceleration)+b*phi.cross(&phi.cross(&acceleration))
        +ad*phi.cross(&rate)+bd*phi.cross(&phi.cross(&rate))+b*rate.cross(&phi.cross(&rate));
    if omega.iter().chain(alpha.iter()).any(|v|!v.is_finite()) {return Err("nonfinite angular motion".into());}
    Ok((omega,alpha))
}

#[cfg(test)]
mod rotation_motion_tests {
    use super::*;
    #[test]
    fn angular_rates_match_matrix_derivatives_at_zero_and_finite_rotations() {
        for origin in [V::zeros(),V::new(1e-5,2e-5,-1e-5),V::new(0.4,-0.3,0.2)] {
            let v=V::new(0.7,0.2,-0.4);let a=V::new(-0.1,0.3,0.5);let h=1e-5;
            let path=|t:f64|origin+v*t+a*(0.5*t*t);
            let (omega,alpha)=rotation_vector_motion(origin,v,a).unwrap();
            let r=rot_vec(origin);let dot=(rot_vec(path(h))-rot_vec(path(-h)))/(2.0*h)*r.transpose();
            assert!((omega-V::new(dot[(2,1)],dot[(0,2)],dot[(1,0)])).norm()<1e-8);
            let wp=rotation_vector_motion(path(h),v+a*h,a).unwrap().0;
            let wm=rotation_vector_motion(path(-h),v-a*h,a).unwrap().0;
            assert!((alpha-(wp-wm)/(2.0*h)).norm()<1e-8);
        }
    }
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
