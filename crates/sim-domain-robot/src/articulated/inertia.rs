//! Rigid mechanical inertia assembled from link motion maps. Geometry is
//! evaluated once; no contact queries or acceleration perturbations are needed.
use super::*;
use nalgebra::DMatrix;

#[derive(Clone)]
pub(super) struct MotionColumn {
    pub(super) index: usize,
    pub(super) linear: V,
    pub(super) angular: V,
}

impl Articulated {
    /// Construct the unconstrained rigid mass matrix at `g` from
    /// sum(Jvᵀ m Jv + Jwᵀ I_world Jw), retaining off-diagonal coupling.
    ///
    /// Coordinate order: world linear acceleration x/y/z and world angular
    /// acceleration x/y/z for each ungrounded base in `bases` order, followed
    /// by every joint DOF in `dofs()` order. The conjugate loads are world
    /// forces/moments at each base COM and joint forces/torques. Thus entries
    /// have mixed kg, kg·m and kg·m² units; they are not quaternion derivatives.
    ///
    /// This is a mechanical building block, not an integrator or a constrained
    /// dynamics solve. Constraints, contact, passive forces and separate motor
    /// states remain external. Modal flexibility is rejected, never discarded.
    /// Compliant fixed joints remain six explicit coordinates. The supported
    /// joint parameterizations have translations before rotations (as emitted
    /// by `Articulated::new`). No symmetry correction or regularization is used.
    pub fn rigid_mass_matrix(&self, g: &Generalized) -> Result<DMatrix<f64>, String> {
        let (links, motion, n) = self.rigid_motion_columns(g)?;
        let mut mass = DMatrix::<f64>::zeros(n, n);
        for (i, link) in self.links.iter().enumerate() {
            let inertia = links[i].r * link.inertia * links[i].r.transpose();
            for b in &motion[i] {
                let force = link.mass * b.linear;
                let torque = inertia * b.angular;
                for a in &motion[i] {
                    mass[(a.index, b.index)] += a.linear.dot(&force) + a.angular.dot(&torque);
                }
            }
        }
        if mass.iter().any(|v| !v.is_finite()) {
            return Err("rigid mass matrix contains nonfinite entries".into());
        }
        Ok(mass)
    }
    // Shared rigid motion map. The same validated geometry/coordinate ordering
    // feeds inertia and original closure derivatives; no force law is involved.
    pub(super) fn rigid_motion_columns(
        &self,
        g: &Generalized,
    ) -> Result<(Vec<LinkKin>, Vec<Vec<MotionColumn>>, usize), String> {
        if self.links.iter().any(|l| l.flex.is_some()) {
            return Err("rigid mass matrix does not discard modal flexibility".into());
        }
        let nd = self.dofs().count();
        if g.states.len() != self.state_count
            || g.rates.len() != self.state_count
            || g.q.len() != nd
            || g.qd.len() != nd
            || g.qdd.len() != nd
        {
            return Err("rigid mass matrix generalized dimensions do not match model".into());
        }
        if g.states
            .iter()
            .chain(&g.rates)
            .chain(&g.q)
            .chain(&g.qd)
            .chain(&g.qdd)
            .any(|v| !v.is_finite())
        {
            return Err("rigid mass matrix requires finite generalized inputs".into());
        }
        for j in &self.joints {
            let mut rotation_seen = false;
            for d in &j.dofs {
                match d.kind {
                    DofKind::Revolute => rotation_seen = true,
                    DofKind::Prismatic if rotation_seen => return Err(
                        "rigid mass matrix requires translations before rotations within a joint"
                            .into(),
                    ),
                    DofKind::Prismatic => (),
                }
            }
        }
        let (links, points, axes) = self.kinematics(g);
        let mut motion: Vec<Vec<MotionColumn>> = vec![Vec::new(); self.links.len()];
        let mut next = 0;
        for b in self.bases.iter().filter(|b| !b.grounded) {
            for k in 0..6 {
                let mut axis = V::zeros();
                axis[k % 3] = 1.0;
                motion[b.link].push(MotionColumn {
                    index: next + k,
                    linear: if k < 3 { axis } else { V::zeros() },
                    angular: if k >= 3 { axis } else { V::zeros() },
                });
            }
            next += 6;
        }
        let n = next + nd;
        for (ji, j) in self.joints.iter().enumerate() {
            let offset = links[j.child].p - links[j.parent].p;
            let mut child: Vec<_> = motion[j.parent]
                .iter()
                .map(|c| MotionColumn {
                    index: c.index,
                    linear: c.linear + c.angular.cross(&offset),
                    angular: c.angular,
                })
                .collect();
            let arm = links[j.child].p - points[ji];
            for (d, axis) in j.dofs.iter().zip(&axes[ji]) {
                child.push(match d.kind {
                    DofKind::Revolute => MotionColumn {
                        index: next,
                        linear: axis.cross(&arm),
                        angular: *axis,
                    },
                    DofKind::Prismatic => MotionColumn {
                        index: next,
                        linear: *axis,
                        angular: V::zeros(),
                    },
                });
                next += 1;
            }
            motion[j.child] = child;
        }
        Ok((links, motion, n))
    }
}
