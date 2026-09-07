//! Coordinate elimination for a forest of constant transmission relations.
//!
//! This is a linear algebra primitive, not a simulation policy. It represents
//! `x[driver] - ratio * x[driven] = rhs`, including affine acceleration targets
//! from closure stabilization. A reaction-dependent regularization term cannot
//! be discarded when applying this map to an existing dynamics formulation.
use nalgebra::{DMatrix, DVector};

/// Mechanical block `M a - Gᵀ lambda = force`. Transmission rows have zero
/// reaction regularization; remaining bilateral rows satisfy
/// `other_jacobian * a + other_cfm * lambda = other_rhs`.
pub struct AccelerationProblem<'a> {
    pub mass: &'a DMatrix<f64>,
    pub force: &'a [f64],
    pub transmission_rhs: &'a [f64],
    pub other_jacobian: &'a DMatrix<f64>,
    pub other_rhs: &'a [f64],
    pub other_cfm: &'a [f64],
}

#[derive(Debug, serde::Serialize)]
pub struct AccelerationSolution {
    pub accelerations: Vec<f64>,
    pub transmission_reactions: Vec<f64>,
    pub other_reactions: Vec<f64>,
    /// Physical-coordinate residuals, not normalized convergence tests. Mixed
    /// translational/rotational coordinates require application-specific scales.
    pub dynamics_residual: Vec<f64>,
    pub transmission_residual: Vec<f64>,
    pub other_constraint_residual: Vec<f64>,
    pub reaction_root_remainder: Vec<f64>,
    pub linear_system_dimension: usize,
}

#[derive(Clone, Debug)]
struct Edge {
    parent: usize,
    child: usize,
    row: usize,
    parent_coefficient: f64,
    child_coefficient: f64,
}

/// A deterministic basis `x = T z + p` for independent, constant gear/belt rows.
/// Unconnected coordinates remain independent. Cycles are rejected, including
/// consistent redundant cycles; this constructor is not a numerical rank test.
#[derive(Clone, Debug)]
pub struct TransmissionCoordinateMap {
    roots: Vec<usize>,
    component: Vec<usize>,
    scale: Vec<f64>,
    edges: Vec<Edge>,
}

impl TransmissionCoordinateMap {
    /// Relations are `(driver, driven, ratio)`; their input order defines the
    /// order of constraint right-hand sides and returned reaction multipliers.
    pub fn new(n: usize, relations: &[(usize, usize, f64)]) -> Result<Self, String> {
        let mut adjacency = vec![Vec::new(); n];
        for (row, &(driver, driven, ratio)) in relations.iter().enumerate() {
            if driver >= n || driven >= n || driver == driven || !ratio.is_finite() || ratio == 0.0
            {
                return Err(format!("invalid transmission relation {row}"));
            }
            adjacency[driver].push((driven, row, 1.0, -ratio));
            adjacency[driven].push((driver, row, -ratio, 1.0));
        }
        let mut out = Self {
            roots: Vec::new(),
            component: vec![usize::MAX; n],
            scale: vec![0.0; n],
            edges: Vec::with_capacity(relations.len()),
        };
        let mut visited_rows = vec![false; relations.len()];
        for root in 0..n {
            if out.component[root] != usize::MAX {
                continue;
            }
            let group = out.roots.len();
            out.roots.push(root);
            out.component[root] = group;
            out.scale[root] = 1.0;
            let mut queue = vec![root];
            let mut next = 0;
            while next < queue.len() {
                let parent = queue[next];
                next += 1;
                for &(child, row, cp, cc) in &adjacency[parent] {
                    if visited_rows[row] {
                        continue;
                    }
                    visited_rows[row] = true;
                    if out.component[child] != usize::MAX {
                        return Err(format!("transmission cycle at relation {row}"));
                    }
                    let scale = -(cp / cc) * out.scale[parent];
                    if !scale.is_finite() || scale == 0.0 {
                        return Err(format!(
                            "unrepresentable transmission scale at relation {row}"
                        ));
                    }
                    out.component[child] = group;
                    out.scale[child] = scale;
                    out.edges.push(Edge {
                        parent,
                        child,
                        row,
                        parent_coefficient: cp,
                        child_coefficient: cc,
                    });
                    queue.push(child);
                }
            }
        }
        Ok(out)
    }

    pub fn full_dimension(&self) -> usize {
        self.component.len()
    }
    pub fn reduced_dimension(&self) -> usize {
        self.roots.len()
    }
    pub fn constraint_count(&self) -> usize {
        self.edges.len()
    }
    pub fn independent_coordinates(&self) -> &[usize] {
        &self.roots
    }

    /// Dense representation for diagnostics or a small projected dynamics block.
    pub fn matrix(&self) -> DMatrix<f64> {
        DMatrix::from_fn(self.full_dimension(), self.reduced_dimension(), |i, j| {
            if self.component[i] == j {
                self.scale[i]
            } else {
                0.0
            }
        })
    }

    pub fn constraint_matrix(&self) -> DMatrix<f64> {
        let mut g = DMatrix::zeros(self.constraint_count(), self.full_dimension());
        for e in &self.edges {
            g[(e.row, e.parent)] = e.parent_coefficient;
            g[(e.row, e.child)] = e.child_coefficient;
        }
        g
    }

    /// Solve a small rigid-body acceleration block after eliminating the ideal
    /// transmission rows. This does not integrate positions, update contact
    /// modes, or solve electrical/thermal states. It is not a replacement for a
    /// complete coupled timestep. Remaining loop constraints retain their CFM.
    pub fn solve_accelerations(
        &self,
        p: &AccelerationProblem<'_>,
    ) -> Result<AccelerationSolution, String> {
        let n = self.full_dimension();
        let m = p.other_jacobian.nrows();
        let r = self.reduced_dimension();
        self.check(p.force, n)?;
        self.check(p.other_rhs, m)?;
        self.check(p.other_cfm, m)?;
        if p.mass.shape() != (n, n)
            || p.other_jacobian.ncols() != n
            || p.mass
                .iter()
                .chain(p.other_jacobian.iter())
                .any(|v| !v.is_finite())
            || p.other_cfm.iter().any(|v| *v < 0.0)
        {
            return Err("invalid constrained acceleration matrices".into());
        }
        let offset = DVector::from_vec(self.expand(&vec![0.0; r], p.transmission_rhs)?);
        let f = DVector::from_column_slice(p.force);
        let b = DVector::from_column_slice(p.other_rhs);
        // Each full coordinate belongs to exactly one independent coordinate.
        // Scatter through that map instead of multiplying dense basis matrices.
        let mut reduced_mass = DMatrix::zeros(r, r);
        let mut g = DMatrix::zeros(m, r);
        for j in 0..n {
            for i in 0..n {
                reduced_mass[(self.component[i], self.component[j])] +=
                    self.scale[i] * p.mass[(i, j)] * self.scale[j];
            }
            for i in 0..m {
                g[(i, self.component[j])] += p.other_jacobian[(i, j)] * self.scale[j];
            }
        }
        let reduced_force =
            DVector::from_vec(self.project_forces((&f - p.mass * &offset).as_slice())?);
        let constraint_rhs = &b - p.other_jacobian * &offset;
        let mut kkt = DMatrix::zeros(r + m, r + m);
        let mut rhs = DVector::zeros(r + m);
        kkt.view_mut((0, 0), (r, r)).copy_from(&reduced_mass);
        kkt.view_mut((0, r), (r, m)).copy_from(&(-g.transpose()));
        kkt.view_mut((r, 0), (m, r)).copy_from(&g);
        rhs.rows_mut(0, r).copy_from(&reduced_force);
        rhs.rows_mut(r, m).copy_from(&constraint_rhs);
        for i in 0..m {
            kkt[(r + i, r + i)] = p.other_cfm[i];
        }
        let solution = kkt
            .lu()
            .solve(&rhs)
            .ok_or("singular projected acceleration system")?;
        self.check(solution.as_slice(), r + m)?;
        let a = DVector::from_iterator(
            n,
            (0..n).map(|i| self.scale[i] * solution[self.component[i]] + offset[i]),
        );
        let lambda = solution.rows(r, m).into_owned();
        let reaction = p.mass * &a - &f - p.other_jacobian.transpose() * &lambda;
        let (transmission_reactions, reaction_root_remainder) =
            self.recover_reactions(reaction.as_slice())?;
        let gt = self.constraint_matrix();
        let dynamics_residual =
            reaction - gt.transpose() * DVector::from_column_slice(&transmission_reactions);
        let transmission_residual = gt * &a - DVector::from_column_slice(p.transmission_rhs);
        let mut other_constraint_residual = p.other_jacobian * &a - b;
        for i in 0..m {
            other_constraint_residual[i] += p.other_cfm[i] * lambda[i];
        }
        self.check(a.as_slice(), n)?;
        self.check(dynamics_residual.as_slice(), n)?;
        self.check(transmission_residual.as_slice(), self.constraint_count())?;
        self.check(other_constraint_residual.as_slice(), m)?;
        Ok(AccelerationSolution {
            accelerations: a.as_slice().to_vec(),
            transmission_reactions,
            other_reactions: lambda.as_slice().to_vec(),
            dynamics_residual: dynamics_residual.as_slice().to_vec(),
            transmission_residual: transmission_residual.as_slice().to_vec(),
            other_constraint_residual: other_constraint_residual.as_slice().to_vec(),
            reaction_root_remainder,
            linear_system_dimension: r + m,
        })
    }

    /// Reconstruct every coordinate. `rhs` is zero for an ideal displacement or
    /// velocity relation; acceleration-level stabilization can supply nonzero
    /// values. The caller owns the physical meaning and units of these values.
    pub fn expand(&self, reduced: &[f64], rhs: &[f64]) -> Result<Vec<f64>, String> {
        self.check(reduced, self.reduced_dimension())?;
        self.check(rhs, self.constraint_count())?;
        let mut out = vec![0.0; self.full_dimension()];
        for (&root, &value) in self.roots.iter().zip(reduced) {
            out[root] = value;
        }
        for e in &self.edges {
            out[e.child] =
                (rhs[e.row] - e.parent_coefficient * out[e.parent]) / e.child_coefficient;
        }
        self.check(&out, self.full_dimension())?;
        Ok(out)
    }

    /// Generalized force projection `Tᵀ f`, preserving virtual work. This must
    /// also be applied to loads on dependent shafts; dropping those loads would
    /// lose reflected inertia and transmission load transfer.
    pub fn project_forces(&self, full: &[f64]) -> Result<Vec<f64>, String> {
        self.check(full, self.full_dimension())?;
        let mut out = vec![0.0; self.reduced_dimension()];
        for (i, &force) in full.iter().enumerate() {
            out[self.component[i]] += self.scale[i] * force;
        }
        self.check(&out, self.reduced_dimension())?;
        Ok(out)
    }

    /// Recover multipliers from `Gᵀ lambda = reaction` by leaf elimination.
    /// Returns the unbalanced root forces as well: arbitrary supplied forces
    /// need not lie in the range of `Gᵀ`. Callers must check that remainder.
    pub fn recover_reactions(&self, reaction: &[f64]) -> Result<(Vec<f64>, Vec<f64>), String> {
        self.check(reaction, self.full_dimension())?;
        let mut work = reaction.to_vec();
        let mut lambda = vec![0.0; self.constraint_count()];
        for e in self.edges.iter().rev() {
            lambda[e.row] = work[e.child] / e.child_coefficient;
            work[e.parent] -= e.parent_coefficient * lambda[e.row];
        }
        let remainder: Vec<_> = self.roots.iter().map(|&i| work[i]).collect();
        self.check(&lambda, self.constraint_count())?;
        self.check(&remainder, self.reduced_dimension())?;
        Ok((lambda, remainder))
    }

    fn check(&self, values: &[f64], expected: usize) -> Result<(), String> {
        if values.len() != expected || values.iter().any(|v| !v.is_finite()) {
            Err("transmission coordinate dimensions or values are invalid".into())
        } else {
            Ok(())
        }
    }
}
