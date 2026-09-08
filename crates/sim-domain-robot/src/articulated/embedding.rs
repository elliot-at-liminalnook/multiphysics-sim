//! Local independent-coordinate chart for an ideal rigid closed mechanism.
//! This explicitly replaces stabilized/CFM closure with geometric closure for
//! an experimental reduced model. It never changes the detailed evaluator.
//! Every original row is checked, including redundant rows. No contact, limits,
//! actuator dynamics, compliance or branch-global reachability is inferred.
use super::{Articulated, DofKind, Generalized};
use nalgebra::{
    DMatrix, DVector, Dyn,
    linalg::{PermutationSequence, SVD},
};
mod dynamics;
mod factor_blocks;
mod slider_crank;
pub use dynamics::PreparedEmbeddedDynamics;
pub use slider_crank::{AnalyticSliderCrank, SliderCrankAudit, SliderCrankCoordinates};
mod step;
pub use step::EmbeddedStep;
mod placement;
pub use placement::{
    CoordinateInterval, EmbeddedPoint, PlanePlacement, PlanePlacementConfig, PointPlacement,
    PointPlaneTarget, PointTarget,
};
mod implicit;
mod mechanical_advance;
pub use mechanical_advance::{EmbeddedMechanicalAdvance, MechanicalSegment};
pub use implicit::{
    CoupledForces, EmbeddedImplicitStep, ImplicitSolverWorkspace, ImplicitStepConfig,
    ImplicitStepDiagnostics, ImplicitEndpointAudit,
};
mod motors;
pub use motors::{EmbeddedMotorBank, EmbeddedMotorConfig, EmbeddedMotorReading, MotorBoundary};
mod control;
pub use control::SampledMotorControl;
mod drivers;
pub use drivers::{
    DriverBoundary, EmbeddedDriverBank, EmbeddedDriverConfig, EmbeddedDriverReading,
};
mod servos;
pub use servos::{EmbeddedServoBank, EmbeddedServoConfig, EmbeddedServoControl, ServoBoundary};
mod motor_step;
pub use motor_step::{
    EmbeddedContactSample, EmbeddedContactStep, EmbeddedControlledAdvance, EmbeddedMotorAdvance,
    MotorSolveStatistics,
};

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependentSolve {
    #[default]
    Svd,
    /// Experimental independent least-squares path. SVD still diagnoses rank.
    PivotedQr,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EmbeddingConfig {
    /// Opt into the shared rigid motion map instead of independent unit-velocity
    /// probes when assembling the original closure Jacobian. All rows/rank
    /// checks remain; this is not an approximate or lagged Jacobian.
    pub direct_closure_jacobian: bool,
    pub dependent_solve: DependentSolve,
    /// Factor exact independent blocks of each current dependent matrix. No
    /// numerical threshold is used to discard couplings; global rank checks remain.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub block_dependent_factorization: bool,
    /// Experimental direct positions for completely covered, structurally
    /// certified slider-cranks and independent-to-dependent transmissions.
    /// Rank, tangent, curvature and every original closure check remain numeric.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub analytic_mechanism_positions: bool,
    pub length_scale_m: f64,
    pub angle_scale_rad: f64,
    pub scaled_closure_tolerance: f64,
    /// Tighter nonlinear position solve leaves room for position errors to be
    /// amplified in velocity/acceleration closure at moving configurations.
    pub scaled_position_tolerance: f64,
    pub relative_rank_tolerance: f64,
    pub absolute_rank_tolerance: f64,
    pub max_iterations: usize,
    pub max_scaled_correction: f64,
}
impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            direct_closure_jacobian: false,
            dependent_solve: DependentSolve::Svd,
            block_dependent_factorization: false,
            analytic_mechanism_positions: false,
            length_scale_m: 0.1,
            angle_scale_rad: 1.0,
            scaled_closure_tolerance: 1e-8,
            scaled_position_tolerance: 1e-11,
            relative_rank_tolerance: 1e-10,
            absolute_rank_tolerance: 1e-12,
            max_iterations: 30,
            max_scaled_correction: 0.25,
        }
    }
}

/// Velocity order matches `rigid_mass_matrix`: free-base world linear/angular
/// velocity, then all joint DOFs. Reduced order is free base then the supplied
/// independent joint names. Positions here are joint coordinates only; the
/// caller supplies the base pose and a nearby branch seed in `Generalized`.
pub struct RigidEmbedding<'a> {
    art: &'a Articulated,
    independent: Vec<usize>,
    dependent: Vec<usize>,
    column_scales: Vec<f64>,
    row_scales: Vec<f64>,
    base_columns: usize,
    config: EmbeddingConfig,
    analytic_positions: Option<slider_crank::AnalyticPositions>,
}

// Local to one unchanged closure Jacobian. Velocity mapping and acceleration
// curvature have different right-hand sides but exactly the same coefficients.
// Never reuse this across poses, Newton position iterations, or contact trials.
enum DependentFactor {
    Empty,
    Blocks {
        columns: usize,
        blocks: Vec<(Vec<usize>, Vec<usize>, DependentFactor)>,
    },
    Svd {
        decomposition: SVD<f64, Dyn, Dyn>,
        cutoff: f64,
    },
    Qr {
        q: DMatrix<f64>,
        r: DMatrix<f64>,
        permutation: PermutationSequence<Dyn>,
    },
}
impl DependentFactor {
    fn solve(&self, rhs: &DMatrix<f64>) -> Result<DMatrix<f64>, String> {
        if rhs.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite dependent-coordinate right-hand side".into());
        }
        match self {
            Self::Empty => Ok(DMatrix::zeros(0, rhs.ncols())),
            Self::Blocks { columns, blocks } => {
                let mut result = DMatrix::zeros(*columns, rhs.ncols());
                for (rows, cols, factor) in blocks {
                    let local = DMatrix::from_fn(rows.len(), rhs.ncols(), |i, j| rhs[(rows[i], j)]);
                    let x = factor.solve(&local)?;
                    for (i, &col) in cols.iter().enumerate() {
                        result.row_mut(col).copy_from(&x.row(i));
                    }
                }
                Ok(result)
            }
            Self::Svd {
                decomposition,
                cutoff,
            } => decomposition.solve(rhs, *cutoff).map_err(|e| e.to_string()),
            Self::Qr { q, r, permutation } => {
                let mut x = r
                    .solve_upper_triangular(&(q.transpose() * rhs))
                    .ok_or("dependent QR solve failed")?;
                permutation.inv_permute_rows(&mut x);
                Ok(x)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct EmbeddedMotion {
    pub generalized: Generalized,
    /// v_full = tangent * v_reduced, in physical coordinate units.
    pub tangent: DMatrix<f64>,
    /// a_full = tangent * a_reduced + acceleration_bias.
    pub acceleration_bias: DVector<f64>,
    pub position_iterations: usize,
    pub minimum_scaled_singular_value: f64,
    pub maximum_scaled_position_error: f64,
    pub maximum_scaled_velocity_error: f64,
    pub maximum_scaled_acceleration_error: f64,
}

#[derive(Debug)]
pub struct EmbeddedAcceleration {
    pub generalized: Generalized,
    pub reduced_accelerations: DVector<f64>,
    pub full_accelerations: DVector<f64>,
    /// Roundoff diagnostic in the reduced force/torque coordinates.
    pub projected_balance_residual: DVector<f64>,
    /// Shared contact-law history derivatives at this position/velocity.
    pub bristle_rates: Vec<f64>,
    pub contacts: Vec<super::ContactPoint>,
}

impl<'a> RigidEmbedding<'a> {
    /// Solve instantaneous ideal constrained rigid dynamics. Applied loads are
    /// full-coordinate forces/torques, in `rigid_mass_matrix` order. The shared
    /// evaluator supplies gravity, velocity bias, contact and passive loads at
    /// the given state. This does not advance time, integrate contact history,
    /// simulate motor electronics or invent an actuator torque command.
    pub fn accelerations(
        &self,
        motion: &EmbeddedMotion,
        applied: &[f64],
    ) -> Result<EmbeddedAcceleration, String> {
        if applied.len() != self.full_dimension() || applied.iter().any(|v| !v.is_finite()) {
            return Err("invalid embedded dynamics input".into());
        }
        self.prepare_dynamics(motion)?.accelerations(applied)
    }

    pub fn new(
        art: &'a Articulated,
        independent: &[String],
        config: EmbeddingConfig,
    ) -> Result<Self, String> {
        if art.bases.len() != 1 || art.links.iter().any(|l| l.flex.is_some()) {
            return Err("rigid embedding requires one connected rigid base; modal flexibility is not discarded".into());
        }
        if [
            config.length_scale_m,
            config.angle_scale_rad,
            config.scaled_closure_tolerance,
            config.scaled_position_tolerance,
            config.relative_rank_tolerance,
            config.absolute_rank_tolerance,
            config.max_scaled_correction,
        ]
        .iter()
        .any(|x| !x.is_finite() || *x <= 0.0)
            || config.relative_rank_tolerance >= 1.0
            || config.max_iterations == 0
            || config.max_iterations > 1000
        {
            return Err("invalid embedding configuration".into());
        }
        let dofs: Vec<_> = art.dofs().map(|(_, d)| d).collect();
        let mut selected = Vec::new();
        for name in independent {
            let found: Vec<_> = dofs
                .iter()
                .enumerate()
                .filter(|(_, d)| &d.name == name)
                .map(|(i, _)| i)
                .collect();
            if found.len() != 1 || selected.contains(&found[0]) {
                return Err(format!(
                    "missing, ambiguous or duplicate independent coordinate {name}"
                ));
            }
            selected.push(found[0]);
        }
        let base_columns = if art.bases[0].grounded { 0 } else { 6 };
        let dependent: Vec<_> = (0..dofs.len()).filter(|i| !selected.contains(i)).collect();
        let analytic_positions = if config.analytic_mechanism_positions {
            Some(slider_crank::AnalyticPositions::new(
                art, &selected, &dependent,
            )?)
        } else {
            None
        };
        let column_scales = (0..base_columns)
            .map(|i| {
                if i < 3 {
                    config.length_scale_m
                } else {
                    config.angle_scale_rad
                }
            })
            .chain(dofs.iter().map(|d| match d.kind {
                DofKind::Prismatic => config.length_scale_m,
                DofKind::Revolute => config.angle_scale_rad,
            }))
            .collect();
        let row_scales = art
            .original_closure_units()
            .iter()
            .map(|unit| match *unit {
                "m" => config.length_scale_m,
                "rad" => config.angle_scale_rad,
                _ => 1.0,
            })
            .collect();
        Ok(Self {
            art,
            independent: selected,
            dependent,
            column_scales,
            row_scales,
            base_columns,
            config,
            analytic_positions,
        })
    }

    pub fn independent_joint_indices(&self) -> &[usize] {
        &self.independent
    }
    pub fn full_dimension(&self) -> usize {
        self.column_scales.len()
    }
    pub fn reduced_dimension(&self) -> usize {
        self.base_columns + self.independent.len()
    }

    fn row_scale(&self, unit: &str) -> f64 {
        match unit {
            "m" => self.config.length_scale_m,
            "rad" => self.config.angle_scale_rad,
            _ => 1.0,
        }
    }
    fn validate_seed(&self, g: &Generalized) -> Result<(), String> {
        let n = self.art.dofs().count();
        if g.states.len() != self.art.state_count
            || g.rates.len() != self.art.state_count
            || g.q.len() != n
            || g.qd.len() != n
            || g.qdd.len() != n
            || g.states
                .iter()
                .chain(&g.rates)
                .chain(&g.q)
                .chain(&g.qd)
                .chain(&g.qdd)
                .any(|x| !x.is_finite())
        {
            return Err("invalid generalized embedding seed".into());
        }
        for base in &self.art.bases {
            let norm = g.states[base.state + 3..base.state + 7]
                .iter()
                .fold(0.0_f64, |n, v| n.hypot(*v));
            if !norm.is_finite() || norm < 1e-10 {
                return Err("invalid base quaternion".into());
            }
        }
        Ok(())
    }
    fn set_motion(&self, g: &mut Generalized, velocity: &[f64], acceleration: &[f64]) {
        g.rates.fill(0.0);
        for b in &self.art.bases {
            g.states[b.state + 7..b.state + 13].fill(0.0);
            if !b.grounded {
                g.states[b.state + 7..b.state + 13].copy_from_slice(&velocity[..6]);
                g.rates[b.state..b.state + 3].copy_from_slice(&velocity[..3]);
                let p = &g.states[b.state + 3..b.state + 7];
                let q = crate::math::quat(p[0], p[1], p[2], p[3]);
                let body_w = q.inverse_transform_vector(&crate::math::V::new(
                    velocity[3],
                    velocity[4],
                    velocity[5],
                ));
                g.rates[b.state + 3..b.state + 7]
                    .copy_from_slice(&crate::math::quat_rate(&q, body_w));
                g.rates[b.state + 7..b.state + 13].copy_from_slice(&acceleration[..6]);
            }
        }
        for (i, (_, d)) in self.art.dofs().enumerate() {
            g.qd[i] = velocity[self.base_columns + i];
            g.qdd[i] = acceleration[self.base_columns + i];
            g.states[d.qd_state] = g.qd[i];
            g.rates[d.qd_state] = g.qdd[i];
            if let Some(s) = d.q_state {
                g.states[s] = g.q[i];
                g.rates[s] = g.qd[i];
            }
        }
        for lp in &self.art.loops {
            g.states[lp.lambda_state..lp.lambda_state + lp.rows].fill(0.0);
        }
        for t in &self.art.transmissions {
            g.states[t.lambda_state] = 0.0;
        }
    }
    fn position(&self, g: &Generalized) -> DVector<f64> {
        DVector::from_iterator(
            self.art.loops.iter().map(|l| l.rows).sum::<usize>() + self.art.transmissions.len(),
            self.art
                .original_closure_values(g)
                .iter()
                .map(|r| r.position / self.row_scale(&r.unit)),
        )
    }
    /// Unit-velocity probes use exact linearity of rigid kinematics, not tiny
    /// position finite differences. All original closure rows participate.
    fn jacobian(&self, g: &Generalized) -> Result<DMatrix<f64>, String> {
        sim_solve::profile::EMBEDDED_CLOSURE_JACOBIAN.time(|| self.jacobian_impl(g))
    }
    fn jacobian_impl(&self, g: &Generalized) -> Result<DMatrix<f64>, String> {
        if self.config.direct_closure_jacobian {
            let raw = self.art.rigid_closure_velocity_jacobian(g)?;
            if raw.nrows() != self.row_scales.len() {
                return Err("closure Jacobian row metadata mismatch".into());
            }
            return Ok(DMatrix::from_fn(raw.nrows(), raw.ncols(), |i, j| {
                raw[(i, j)] * self.column_scales[j] / self.row_scales[i]
            }));
        }
        let n = self.full_dimension();
        let mut zero = g.clone();
        let mut v = vec![0.0; n];
        let a = vec![0.0; n];
        self.set_motion(&mut zero, &v, &a);
        let rows = self.row_scales.len();
        let mut matrix = DMatrix::zeros(rows, n);
        for col in 0..n {
            v[col] = self.column_scales[col];
            self.set_motion(&mut zero, &v, &a);
            for (row, r) in self.art.original_closure_values(&zero).iter().enumerate() {
                matrix[(row, col)] = r.velocity / self.row_scale(&r.unit);
            }
            v[col] = 0.0;
        }
        Ok(matrix)
    }
    fn factor_dependent(&self, jac: &DMatrix<f64>) -> Result<(DependentFactor, f64), String> {
        sim_solve::profile::EMBEDDED_CLOSURE_FACTOR.time(|| self.factor_dependent_impl(jac))
    }
    fn factor_dependent_impl(&self, jac: &DMatrix<f64>) -> Result<(DependentFactor, f64), String> {
        if self.dependent.is_empty() {
            return Ok((DependentFactor::Empty, 1.0));
        }
        let a = DMatrix::from_fn(jac.nrows(), self.dependent.len(), |i, j| {
            jac[(i, self.base_columns + self.dependent[j])]
        });
        if a.iter().any(|v| !v.is_finite()) || a.nrows() < a.ncols() {
            return Err("nonfinite or underconstrained embedding".into());
        }
        if self.config.block_dependent_factorization {
            let groups = factor_blocks::independent_blocks(&a);
            if groups.len() > 1 {
                let mut factors = Vec::with_capacity(groups.len());
                let mut global_min = f64::INFINITY;
                let mut global_max = 0.0_f64;
                for (rows, cols) in groups {
                    let local =
                        DMatrix::from_fn(rows.len(), cols.len(), |i, j| a[(rows[i], cols[j])]);
                    let (factor, min, max) = self.factor_matrix(local)?;
                    global_min = global_min.min(min);
                    global_max = global_max.max(max);
                    factors.push((rows, cols, factor));
                }
                self.check_rank(global_min, global_max)?;
                return Ok((
                    DependentFactor::Blocks {
                        columns: a.ncols(),
                        blocks: factors,
                    },
                    global_min,
                ));
            }
        }
        let (factor, min, _) = self.factor_matrix(a)?;
        Ok((factor, min))
    }
    fn check_rank(&self, min: f64, max: f64) -> Result<f64, String> {
        let cutoff = self
            .config
            .absolute_rank_tolerance
            .max(self.config.relative_rank_tolerance * max);
        if min <= cutoff {
            return Err(format!(
                "singular dependent-coordinate chart: minimum scaled singular value {min}, cutoff {cutoff}"
            ));
        }
        Ok(cutoff)
    }
    fn factor_matrix(&self, a: DMatrix<f64>) -> Result<(DependentFactor, f64, f64), String> {
        if a.nrows() < a.ncols() {
            return Err("singular dependent-coordinate chart: underconstrained block".into());
        }
        // QR needs the singular VALUES for the unchanged rank diagnostic,
        // but never uses SVD's left/right vectors. Avoid constructing them.
        let vectors = matches!(self.config.dependent_solve, DependentSolve::Svd);
        let svd = sim_solve::profile::EMBEDDED_CLOSURE_SVD.time(|| a.clone().svd(vectors, vectors));
        let max = svd.singular_values.iter().copied().fold(0.0, f64::max);
        let min = svd
            .singular_values
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        let cutoff = self.check_rank(min, max)?;
        let factor = match self.config.dependent_solve {
            DependentSolve::Svd => DependentFactor::Svd {
                decomposition: svd,
                cutoff,
            },
            DependentSolve::PivotedQr => {
                let (q, r, p) = a.col_piv_qr().unpack();
                DependentFactor::Qr {
                    q,
                    r,
                    permutation: p,
                }
            }
        };
        Ok((factor, min, max))
    }

    /// Local continuation on the assembly branch near `seed`. The caller must
    /// bound command increments and check collision/actuator feasibility.
    /// This uses ideal constraints, not the detailed model's CFM compliance.
    pub fn solve(
        &self,
        seed: &Generalized,
        positions: &[f64],
        velocities: &[f64],
    ) -> Result<EmbeddedMotion, String> {
        sim_solve::profile::EMBEDDED_MAPPING.time(|| self.solve_impl(seed, positions, velocities))
    }

    fn solve_impl(
        &self,
        seed: &Generalized,
        positions: &[f64],
        velocities: &[f64],
    ) -> Result<EmbeddedMotion, String> {
        self.validate_seed(seed)?;
        if positions.len() != self.independent.len()
            || velocities.len() != self.reduced_dimension()
            || positions.iter().chain(velocities).any(|v| !v.is_finite())
        {
            return Err("invalid independent positions or velocities".into());
        }
        let mut g = seed.clone();
        for (&i, &q) in self.independent.iter().zip(positions) {
            g.q[i] = q;
        }
        if let Some(analytic) = &self.analytic_positions {
            analytic.apply(self.art, seed, &mut g)?;
        }
        let zeros = vec![0.0; self.full_dimension()];
        self.set_motion(&mut g, &zeros, &zeros);
        let mut iterations = 0;
        loop {
            let p = self.position(&g);
            if p.iter().any(|v| !v.is_finite()) {
                return Err("nonfinite geometric closure".into());
            }
            if p.amax()
                <= self
                    .config
                    .scaled_position_tolerance
                    .min(self.config.scaled_closure_tolerance)
            {
                break;
            }
            if self.analytic_positions.is_some() {
                return Err(format!(
                    "analytic positions failed original closure: {}",
                    p.amax()
                ));
            }
            if iterations >= self.config.max_iterations {
                return Err("geometric closure iteration limit".into());
            }
            let jac = self.jacobian(&g)?;
            let rhs = DMatrix::from_column_slice(p.len(), 1, (-&p).as_slice());
            let (factor, _) = self.factor_dependent(&jac)?;
            let step = factor.solve(&rhs)?;
            let factor =
                (self.config.max_scaled_correction / step.amax().max(f64::MIN_POSITIVE)).min(1.0);
            let mut accepted = None;
            for backtrack in 0..16 {
                let mut trial = g.clone();
                let alpha = factor * 0.5_f64.powi(backtrack);
                for (j, &i) in self.dependent.iter().enumerate() {
                    trial.q[i] += alpha * step[(j, 0)] * self.column_scales[self.base_columns + i];
                }
                let r = self.position(&trial);
                if r.iter().all(|v| v.is_finite()) && r.norm() < p.norm() {
                    accepted = Some(trial);
                    break;
                }
            }
            g = accepted.ok_or(
                "geometric closure did not improve; inconsistent constraints or branch seed",
            )?;
            iterations += 1;
        }
        let jac = self.jacobian(&g)?;
        let selected: Vec<_> = (0..self.base_columns)
            .chain(self.independent.iter().map(|i| self.base_columns + i))
            .collect();
        let rhs = DMatrix::from_fn(jac.nrows(), selected.len(), |i, j| -jac[(i, selected[j])]);
        let (factor, min_singular) = self.factor_dependent(&jac)?;
        let dependent = factor.solve(&rhs)?;
        let mut scaled = DMatrix::zeros(self.full_dimension(), self.reduced_dimension());
        for (j, &i) in selected.iter().enumerate() {
            scaled[(i, j)] = 1.0;
        }
        for (i, &d) in self.dependent.iter().enumerate() {
            for j in 0..selected.len() {
                scaled[(self.base_columns + d, j)] = dependent[(i, j)];
            }
        }
        let tangent_error = (&jac * &scaled).amax();
        if tangent_error > self.config.scaled_closure_tolerance {
            return Err(format!(
                "independent coordinates violate original closure velocity directions: maximum scaled error {tangent_error:e}, minimum dependent singular value {min_singular:e}"
            ));
        }
        let tangent = DMatrix::from_fn(scaled.nrows(), scaled.ncols(), |i, j| {
            scaled[(i, j)] * self.column_scales[i] / self.column_scales[selected[j]]
        });
        let full_v = &tangent * DVector::from_column_slice(velocities);
        self.set_motion(&mut g, full_v.as_slice(), &zeros);
        let rows = self.art.original_closure_values(&g);
        let rhs = DMatrix::from_fn(rows.len(), 1, |i, _| {
            -rows[i].acceleration / self.row_scale(&rows[i].unit)
        });
        let bias = factor.solve(&rhs)?;
        let mut acceleration_bias = DVector::zeros(self.full_dimension());
        for (i, &d) in self.dependent.iter().enumerate() {
            acceleration_bias[self.base_columns + d] =
                bias[(i, 0)] * self.column_scales[self.base_columns + d];
        }
        self.set_motion(&mut g, full_v.as_slice(), acceleration_bias.as_slice());
        let rows = self.art.original_closure_values(&g);
        let error = |get: fn(&super::constraints::ClosureValues) -> f64| {
            rows.iter()
                .map(|r| get(r).abs() / self.row_scale(&r.unit))
                .fold(0.0, f64::max)
        };
        let pe = error(|r| r.position);
        let ve = error(|r| r.velocity);
        let ae = error(|r| r.acceleration);
        if [pe, ve, ae]
            .iter()
            .any(|x| !x.is_finite() || *x > self.config.scaled_closure_tolerance)
        {
            return Err(format!(
                "original closure motion check failed: position {pe}, velocity {ve}, acceleration {ae}"
            ));
        }
        Ok(EmbeddedMotion {
            generalized: g,
            tangent,
            acceleration_bias,
            position_iterations: iterations,
            minimum_scaled_singular_value: min_singular,
            maximum_scaled_position_error: pe,
            maximum_scaled_velocity_error: ve,
            maximum_scaled_acceleration_error: ae,
        })
    }
}
