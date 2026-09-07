//! Structural identities and independent diagnostics for closed mechanisms.
//! Only exact tree certification changes evaluation; pose-local rank results
//! never select or remove equations in the simulator.
use super::*;

/// True only when both loop axes are preserved along the rigid tree paths to
/// their common ancestor. Translation cannot change an axis; rotation about
/// that same axis cannot change it either. A shared ancestor may move freely.
pub(super) fn axis_alignment_is_implied(
    a: usize,
    b: usize,
    axis: V,
    links: &[LinkC],
    joints: &[JointC],
) -> bool {
    let mut ancestors = vec![a];
    let mut link = a;
    while let Some(j) = links[link].parent_joint {
        link = joints[j].parent;
        ancestors.push(link);
    }
    let mut common = b;
    while !ancestors.contains(&common) {
        let Some(j) = links[common].parent_joint else {
            return false;
        };
        common = joints[j].parent;
    }
    let transport = |mut link: usize, mut direction: V| -> Option<V> {
        while link != common {
            let j = &joints[links[link].parent_joint?];
            // A modal boundary can rotate relative to the tree. Do not infer
            // redundancy from its undeformed export pose.
            if j.flex_boundary.is_some() {
                return None;
            }
            direction = j.r_jc_rot * direction;
            for d in &j.dofs {
                if matches!(d.kind, DofKind::Revolute) {
                    let mut joint_axis = V::zeros();
                    joint_axis[d.axis] = 1.0;
                    if joint_axis.cross(&direction) != V::zeros() {
                        return None;
                    }
                }
            }
            direction = j.r_j * direction;
            link = j.parent;
        }
        Some(direction)
    };
    match (transport(a, axis), transport(b, axis)) {
        (Some(a), Some(b)) => {
            a.norm_squared() > 0.0 && b.norm_squared() > 0.0 && a.cross(&b) == V::zeros()
        }
        _ => false,
    }
}

/// Original geometric closure, independently recomputed even for certified
/// identities. Angular alignment uses dot products (dimensionless), not angles.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ClosureRow {
    pub name: String,
    pub unit: String,
    pub position: f64,
    pub velocity: f64,
    pub acceleration: f64,
    /// Acceleration + Baumgarte terms + CFM at the supplied state/rate point.
    pub stabilized: f64,
    pub certified_identity: bool,
}

/// Original equation values without allocating diagnostic names or units.
/// Units and row order match `original_closure` exactly. Identity rows are
/// still evaluated at the actual configuration, never replaced with zeros.
#[derive(Clone, Copy, Debug)]
pub struct ClosureValues {
    pub unit: &'static str,
    pub position: f64,
    pub velocity: f64,
    pub acceleration: f64,
    pub stabilized: f64,
    pub certified_identity: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RankConfig {
    /// Characteristic distance and angle used for both row and column scaling.
    pub length_scale_m: f64,
    pub angle_scale_rad: f64,
    /// A threshold for this diagnostic, never permission to delete equations.
    pub relative_tolerance: f64,
    pub absolute_tolerance: f64,
}
impl Default for RankConfig {
    fn default() -> Self {
        Self { length_scale_m: 1.0, angle_scale_rad: 1.0, relative_tolerance: 1e-10, absolute_tolerance: 1e-12 }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct ConstraintAudit {
    pub rows: Vec<ClosureRow>,
    pub coordinates: Vec<String>,
    pub velocity_units: Vec<String>,
    pub column_scales: Vec<f64>,
    pub row_scales: Vec<f64>,
    /// G maps physical generalized velocities to original closure velocities.
    /// Scaling is diag(1/row_scales) * G * diag(column_scales).
    pub scaled_velocity_matrix: Vec<Vec<f64>>,
    /// G^T lambda in the named physical velocity coordinates at this point.
    /// Internal loop/transmission reactions only; contact loads are separate.
    pub generalized_reactions: Vec<f64>,
    pub reaction_units: Vec<String>,
    pub singular_values: Vec<f64>,
    pub qr_diagonal: Vec<f64>,
    pub svd_rank: usize,
    pub svd_cutoff: f64,
    pub qr_cutoff: f64,
    pub qr_rank: usize,
    /// Pivoted-QR selection at this pose only. Never used by the simulator.
    pub independent_rows: Vec<usize>,
    pub config: RankConfig,
}

impl Articulated {
    /// Direct rigid velocity Jacobian of every ORIGINAL closure equation.
    /// Rows match `original_closure`; columns are free-base world linear/angular
    /// velocity, then joint DOFs, as in `rigid_mass_matrix`. Entries use physical
    /// units, not row/column normalization. This is not the complete integrator
    /// Jacobian and includes no contact derivative, stabilization or CFM term.
    ///
    /// Reuses the inertia kernel's rigid motion maps after one geometry pass.
    /// Modal flexibility and unsupported mixed joint parameterizations fail
    /// explicitly. Identity rows remain evaluated, never dropped or zeroed from
    /// pose-local rank. Works away from closure as well as at a closed pose.
    pub fn rigid_closure_velocity_jacobian(&self, g: &Generalized) -> Result<nalgebra::DMatrix<f64>, String> {
        let (links, motion, n) = self.rigid_motion_columns(g)?;
        let nr = self.loops.iter().map(|lp| lp.rows).sum::<usize>() + self.transmissions.len();
        let mut matrix = nalgebra::DMatrix::<f64>::zeros(nr, n);
        let mut row = 0;
        for lp in &self.loops {
            let (ka, kb) = (&links[lp.a], &links[lp.b]);
            let (ra, rb) = (ka.r * lp.r_a, kb.r * lp.r_b);
            for c in &motion[lp.a] {
                let v = c.linear + c.angular.cross(&ra);
                for k in 0..3 { matrix[(row + k, c.index)] -= v[k]; }
            }
            for c in &motion[lp.b] {
                let v = c.linear + c.angular.cross(&rb);
                for k in 0..3 { matrix[(row + k, c.index)] += v[k]; }
            }
            if let Some((e1, e2, axis)) = &lp.axis {
                let ax = kb.r * axis;
                for (k, e) in [e1, e2].into_iter().enumerate() {
                    let e = ka.r * e;
                    for c in &motion[lp.a] {
                        matrix[(row + 3 + k, c.index)] += c.angular.cross(&e).dot(&ax);
                    }
                    for c in &motion[lp.b] {
                        matrix[(row + 3 + k, c.index)] += e.dot(&c.angular.cross(&ax));
                    }
                }
            }
            row += lp.rows;
        }
        let nb = 6 * self.bases.iter().filter(|b| !b.grounded).count();
        for t in &self.transmissions {
            matrix[(row, nb + t.driver)] += 1.0;
            matrix[(row, nb + t.driven)] -= t.ratio;
            row += 1;
        }
        if matrix.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite rigid closure velocity Jacobian".into());
        }
        Ok(matrix)
    }

    /// Recompute all original closure equations, including identity rows that
    /// the experimental evaluator avoids. Acceleration and stabilized values
    /// are meaningful only at the caller's supplied configuration/rate point.
    pub fn original_closure(&self, g: &Generalized) -> Vec<ClosureRow> {
        let mut rows = Vec::new();
        self.visit_original_closure(g, |name, component, v| {
            let name = match component {
                Some((kind, index)) => format!("{name}.{kind}.{index}"),
                None => name.to_owned(),
            };
            rows.push(ClosureRow { name, unit:v.unit.into(), position:v.position,
                velocity:v.velocity, acceleration:v.acceleration, stabilized:v.stabilized,
                certified_identity:v.certified_identity });
        });
        rows
    }

    /// Numeric original rows for solver hot paths. Shares the entire equation
    /// traversal with the named diagnostic API, including stabilization/CFM.
    pub fn original_closure_values(&self, g: &Generalized) -> Vec<ClosureValues> {
        let mut rows = Vec::new();
        self.visit_original_closure(g, |_, _, v| rows.push(v));
        rows
    }

    /// Immutable model metadata; no pose, kinematics or force evaluation.
    /// This order also defines the rows of the original velocity Jacobian.
    pub fn original_closure_units(&self) -> Vec<&'static str> {
        let mut units = Vec::new();
        for lp in &self.loops {
            units.extend(["m";3]);
            if lp.axis.is_some() { units.extend(["1";2]); }
        }
        let dofs:Vec<_> = self.dofs().map(|(_,d)|d).collect();
        for t in &self.transmissions {
            units.push(match dofs[t.driver].kind {DofKind::Revolute=>"rad",DofKind::Prismatic=>"m"});
        }
        units
    }

    fn visit_original_closure(
        &self, g:&Generalized,
        mut emit:impl FnMut(&str, Option<(&'static str,usize)>, ClosureValues),
    ) {
        let (links, _, _) = self.kinematics(g);
        let mut push = |name:&str, component, unit:&'static str, p:f64, v:f64, a:f64, cfm:f64, lambda:f64, identity| {
            emit(name, component, ClosureValues { unit, position: p, velocity: v,
                acceleration: a, stabilized: a + 2.0 * self.loop_alpha * v
                    + self.loop_alpha.powi(2) * p + cfm * lambda, certified_identity: identity });
        };
        for lp in &self.loops {
            let (ka, kb) = (&links[lp.a], &links[lp.b]);
            let (ra, rb) = (ka.r * lp.r_a, kb.r * lp.r_b);
            let p = kb.p + rb - (ka.p + ra);
            let v = kb.vel + kb.w.cross(&rb) - (ka.vel + ka.w.cross(&ra));
            let a = kb.acc + kb.alpha.cross(&rb) + kb.w.cross(&kb.w.cross(&rb))
                - (ka.acc + ka.alpha.cross(&ra) + ka.w.cross(&ka.w.cross(&ra)));
            for k in 0..3 {
                push(&lp.name, Some(("position",k)), "m", p[k], v[k], a[k],
                    self.loop_cfm, g.states[lp.lambda_state + k], false);
            }
            if let Some((e1, e2, axis)) = &lp.axis {
                let ax = kb.r * axis;
                let da = kb.w.cross(&ax);
                let dda = kb.alpha.cross(&ax) + kb.w.cross(&kb.w.cross(&ax));
                for (k, e) in [e1, e2].into_iter().enumerate() {
                    let e = ka.r * e;
                    let de = ka.w.cross(&e);
                    let dde = ka.alpha.cross(&e) + ka.w.cross(&ka.w.cross(&e));
                    push(&lp.name, Some(("alignment",k)), "1", e.dot(&ax),
                        de.dot(&ax) + e.dot(&da), dde.dot(&ax) + 2.0 * de.dot(&da) + e.dot(&dda),
                        self.loop_angular_cfm, g.states[lp.lambda_state + 3 + k], lp.angular_redundant);
                }
            }
        }
        let dofs: Vec<_> = self.dofs().map(|(_, d)| d).collect();
        for t in &self.transmissions {
            let unit = match dofs[t.driver].kind { DofKind::Revolute => "rad", DofKind::Prismatic => "m" };
            push(&t.name, None, unit, g.q[t.driver] - t.ratio * g.q[t.driven],
                g.qd[t.driver] - t.ratio * g.qd[t.driven],
                g.qdd[t.driver] - t.ratio * g.qdd[t.driven],
                self.loop_angular_cfm, g.states[t.lambda_state], false);
        }
    }

    /// Pose-local rank of the ORIGINAL bilateral closure velocity map, with
    /// pivoted QR and an independent SVD diagnostic. Unit-velocity basis probes
    /// exploit the exact linearity of kinematic velocities; no tiny differences,
    /// contact query or dynamics solve is involved. This is not a rank test of
    /// the complete Newton residual and never changes retained equations.
    pub fn audit_constraints(&self, g: &Generalized, config: &RankConfig) -> Result<ConstraintAudit, String> {
        if [config.length_scale_m, config.angle_scale_rad, config.relative_tolerance, config.absolute_tolerance]
            .iter().any(|v| !v.is_finite() || *v <= 0.0) || config.relative_tolerance >= 1.0 {
            return Err("constraint rank scales must be positive and finite; tolerance must be below one".into());
        }
        let rows = self.original_closure(g);
        let declarations = self.states();
        // (owned velocity state, optional joint generalized-velocity index).
        let mut columns = Vec::new();
        for b in self.bases.iter().filter(|b| !b.grounded) {
            columns.extend((b.state + 7..b.state + 13).map(|s| (s, None)));
        }
        columns.extend(self.dofs().enumerate().map(|(i, (_, d))| (d.qd_state, Some(i))));
        for l in &self.links {
            if let Some(f) = &l.flex {
                columns.extend((f.state + f.modes..f.state + 2 * f.modes).map(|s| (s, None)));
            }
        }
        let coordinates = columns.iter().map(|(s, _)| declarations[*s].name.clone()).collect();
        let velocity_units: Vec<String> = columns.iter().map(|(s, _)| declarations[*s].kind.unit().into()).collect();
        let column_scales: Vec<f64> = velocity_units.iter().map(|unit| match unit.as_str() {
            "m/s" => config.length_scale_m, "rad/s" => config.angle_scale_rad,
            // Modal normalization is declared by the model: one in its own unit.
            _ => 1.0,
        }).collect();
        let row_scales: Vec<_> = rows.iter().map(|row| match row.unit.as_str() {
            "m" => config.length_scale_m, "rad" => config.angle_scale_rad, _ => 1.0,
        }).collect();
        let mut zero = g.clone();
        zero.qd.fill(0.0);
        zero.qdd.fill(0.0);
        zero.rates.fill(0.0);
        // Grounded bases must not contribute a spurious velocity offset.
        for b in &self.bases { zero.states[b.state + 7..b.state + 13].fill(0.0); }
        for (s, _) in &columns { zero.states[*s] = 0.0; }
        let mut matrix = nalgebra::DMatrix::zeros(rows.len(), columns.len());
        for (j, &(s, q)) in columns.iter().enumerate() {
            zero.states[s] = column_scales[j];
            if let Some(q) = q { zero.qd[q] = column_scales[j]; }
            for (i, value) in self.original_closure(&zero).iter().enumerate() {
                matrix[(i, j)] = value.velocity / row_scales[i];
            }
            zero.states[s] = 0.0;
            if let Some(q) = q { zero.qd[q] = 0.0; }
        }
        if matrix.iter().any(|v| !v.is_finite()) || rows.iter().any(|r|
            [r.position, r.velocity, r.acceleration, r.stabilized].iter().any(|v| !v.is_finite())) {
            return Err("nonfinite constraint audit point".into());
        }
        let lambdas: Vec<f64> = self.loops.iter().flat_map(|lp|
            g.states[lp.lambda_state..lp.lambda_state + lp.rows].iter().copied())
            .chain(self.transmissions.iter().map(|t| g.states[t.lambda_state])).collect();
        let generalized_reactions = (0..columns.len()).map(|j|
            (0..rows.len()).map(|i| matrix[(i,j)] * row_scales[i] / column_scales[j] * lambdas[i]).sum()).collect();
        let reaction_units = velocity_units.iter().map(|unit| match unit.as_str() {
            "m/s" => "N".into(), "rad/s" => "N·m".into(), _ => format!("W/({unit})"),
        }).collect();
        let mut singular_values = Vec::new();
        let mut qr_diagonal = Vec::new();
        let mut independent_rows = Vec::new();
        let mut svd_rank = 0;
        let mut svd_cutoff = config.absolute_tolerance;
        let mut qr_cutoff = config.absolute_tolerance;
        let mut qr_rank = 0;
        if !rows.is_empty() && !columns.is_empty() {
            singular_values = matrix.clone().svd(false, false).singular_values.as_slice().to_vec();
            singular_values.sort_by(|a,b| b.total_cmp(a));
            svd_cutoff = config.absolute_tolerance.max(config.relative_tolerance * singular_values[0]);
            svd_rank = singular_values.iter().filter(|s| **s > svd_cutoff).count();
            let qr = matrix.transpose().col_piv_qr();
            let r = qr.r();
            qr_diagonal = (0..r.nrows().min(r.ncols())).map(|i| r[(i,i)].abs()).collect();
            qr_cutoff = config.absolute_tolerance.max(config.relative_tolerance * qr_diagonal.iter().copied().fold(0.0, f64::max));
            qr_rank = qr_diagonal.iter().take_while(|s| **s > qr_cutoff).count();
            let mut indices = nalgebra::DMatrix::from_fn(1, rows.len(), |_, j| j);
            qr.p().permute_columns(&mut indices);
            independent_rows = indices.iter().take(qr_rank).copied().collect();
        }
        Ok(ConstraintAudit { rows, coordinates, velocity_units, column_scales, row_scales,
            scaled_velocity_matrix: (0..matrix.nrows()).map(|i| matrix.row(i).iter().copied().collect()).collect(),
            generalized_reactions, reaction_units, singular_values, qr_diagonal, svd_rank, qr_rank, svd_cutoff, qr_cutoff, independent_rows, config: config.clone() })
    }
}
