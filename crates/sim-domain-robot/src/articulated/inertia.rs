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

/// Pose-local columns in one allocation instead of one growing vector per link.
/// Ranges follow the compiled forest; within each link, inherited columns still
/// precede joint columns in their original order. No zero/weak column is removed.
pub(super) struct MotionColumns {
    columns: Vec<MotionColumn>,
    ranges: Vec<std::ops::Range<usize>>,
}
impl std::ops::Index<usize> for MotionColumns {
    type Output = [MotionColumn];
    fn index(&self, link: usize) -> &Self::Output {
        &self.columns[self.ranges[link].clone()]
    }
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
        self.validate_rigid_motion(g)?;
        self.rigid_mass_with_kinematics(&self.kinematics(g))
    }

    /// The caller supplies kinematics from the same validated immutable state.
    /// Keep the original assembly order so sharing geometry does not change
    /// the inertia arithmetic or the force evaluator's kinematic values.
    pub(super) fn rigid_mass_with_kinematics(
        &self,
        kinematics: &prepared::Kinematics,
    ) -> Result<DMatrix<f64>, String> {
        let (links, _, _) = kinematics;
        let (motion, n) = self.rigid_motion_columns_with_kinematics(kinematics);
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

    /// Construct Tᵀ M T directly from the rigid link motion maps. `tangent`
    /// maps caller coordinates to the full velocities documented above. It
    /// may be dense, rectangular or rank deficient; this operation does not
    /// infer constraints or regularize inertia. The caller owns closure and
    /// positive-definiteness checks. No physical state is eliminated here.
    pub fn rigid_projected_mass_matrix(
        &self,
        g: &Generalized,
        tangent: &DMatrix<f64>,
    ) -> Result<DMatrix<f64>, String> {
        self.validate_rigid_motion(g)?;
        self.rigid_projected_mass_with_kinematics(tangent, &self.kinematics(g))
    }

    // Kinematics must come from the same validated generalized state/model.
    pub(super) fn rigid_projected_mass_with_kinematics(
        &self,
        tangent: &DMatrix<f64>,
        kinematics: &prepared::Kinematics,
    ) -> Result<DMatrix<f64>, String> {
        let (links, points, axes) = kinematics;
        let nb = 6 * self.bases.iter().filter(|b| !b.grounded).count();
        if tangent.nrows() != nb + self.dofs().count() || tangent.iter().any(|v| !v.is_finite()) {
            return Err("invalid rigid inertia projection".into());
        }
        let nr = tangent.ncols();
        let mut motion = vec![vec![(V::zeros(), V::zeros()); nr]; self.links.len()];
        let mut next = 0;
        for base in self.bases.iter().filter(|b| !b.grounded) {
            for col in 0..nr {
                motion[base.link][col] = (
                    V::new(
                        tangent[(next, col)],
                        tangent[(next + 1, col)],
                        tangent[(next + 2, col)],
                    ),
                    V::new(
                        tangent[(next + 3, col)],
                        tangent[(next + 4, col)],
                        tangent[(next + 5, col)],
                    ),
                );
            }
            next += 6;
        }
        for (ji, joint) in self.joints.iter().enumerate() {
            let offset = links[joint.child].p - links[joint.parent].p;
            for col in 0..nr {
                let (v, w) = motion[joint.parent][col];
                motion[joint.child][col] = (v + w.cross(&offset), w);
            }
            let arm = links[joint.child].p - points[ji];
            for (dof, axis) in joint.dofs.iter().zip(&axes[ji]) {
                let (v, w) = match dof.kind {
                    DofKind::Revolute => (axis.cross(&arm), *axis),
                    DofKind::Prismatic => (*axis, V::zeros()),
                };
                for col in 0..nr {
                    let coefficient = tangent[(next, col)];
                    if coefficient != 0.0 {
                        motion[joint.child][col].0 += coefficient * v;
                        motion[joint.child][col].1 += coefficient * w;
                    }
                }
                next += 1;
            }
        }
        let mut mass = DMatrix::<f64>::zeros(nr, nr);
        for (i, link) in self.links.iter().enumerate() {
            let inertia = links[i].r * link.inertia * links[i].r.transpose();
            // Exact zero columns only; never threshold weak couplings.
            let active: Vec<_> = (0..nr)
                .filter(|&j| motion[i][j].0 != V::zeros() || motion[i][j].1 != V::zeros())
                .collect();
            for &b in &active {
                let force = link.mass * motion[i][b].0;
                let torque = inertia * motion[i][b].1;
                for &a in &active {
                    mass[(a, b)] += motion[i][a].0.dot(&force) + motion[i][a].1.dot(&torque);
                }
            }
        }
        if mass.iter().any(|v| !v.is_finite()) {
            return Err("projected rigid inertia contains nonfinite entries".into());
        }
        Ok(mass)
    }
    // Shared rigid motion map. The same validated geometry/coordinate ordering
    // feeds inertia and original closure derivatives; no force law is involved.
    pub(super) fn rigid_motion_columns(
        &self,
        g: &Generalized,
    ) -> Result<(Vec<LinkKin>, MotionColumns, usize), String> {
        self.validate_rigid_motion(g)?;
        let kinematics = self.kinematics(g);
        let (motion, n) = self.rigid_motion_columns_with_kinematics(&kinematics);
        Ok((kinematics.0, motion, n))
    }

    fn rigid_motion_columns_with_kinematics(
        &self,
        kinematics: &prepared::Kinematics,
    ) -> (MotionColumns, usize) {
        let (links, points, axes) = kinematics;
        let nd = self.dofs().count();
        let mut ranges = vec![0..0; self.links.len()];
        let mut total = 0;
        for b in self.bases.iter().filter(|b| !b.grounded) {
            ranges[b.link] = total..total + 6;
            total += 6;
        }
        for j in &self.joints {
            let count = ranges[j.parent].len() + j.dofs.len();
            ranges[j.child] = total..total + count;
            total += count;
        }
        let mut columns = Vec::with_capacity(total);
        let mut next = 0;
        for b in self.bases.iter().filter(|b| !b.grounded) {
            debug_assert_eq!(columns.len(), ranges[b.link].start);
            for k in 0..6 {
                let mut axis = V::zeros();
                axis[k % 3] = 1.0;
                columns.push(MotionColumn {
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
            debug_assert_eq!(columns.len(), ranges[j.child].start);
            for parent_column in ranges[j.parent].clone() {
                let c = &columns[parent_column];
                let inherited = MotionColumn {
                    index: c.index,
                    linear: c.linear + c.angular.cross(&offset),
                    angular: c.angular,
                };
                columns.push(inherited);
            }
            let arm = links[j.child].p - points[ji];
            for (d, axis) in j.dofs.iter().zip(&axes[ji]) {
                columns.push(match d.kind {
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
        }
        debug_assert_eq!(columns.len(), total);
        (MotionColumns { columns, ranges }, n)
    }

    pub(super) fn validate_rigid_motion(&self, g: &Generalized) -> Result<(), String> {
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
        Ok(())
    }
}
