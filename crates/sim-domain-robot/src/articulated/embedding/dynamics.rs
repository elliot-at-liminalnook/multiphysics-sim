//! Immutable force-to-acceleration solve for one exact mechanical state.
use super::{EmbeddedAcceleration, EmbeddedMotion, RigidEmbedding};
use nalgebra::{DMatrix, DVector, Dyn, linalg::Cholesky};

/// Owns the prepared pose, velocity, contact history, tangent, inertia and
/// passive loads. Only applied generalized forces may change. The borrowing
/// relationship keeps the underlying model immutable, and no caller can
/// replace parts of this state or accidentally use a different mechanism map.
pub struct PreparedEmbeddedDynamics<'m, 'a> {
    map: &'m RigidEmbedding<'a>,
    generalized: super::Generalized,
    tangent: DMatrix<f64>,
    acceleration_bias: DVector<f64>,
    velocity: Vec<f64>,
    required: DVector<f64>,
    reduced: DMatrix<f64>,
    factor: Cholesky<f64, Dyn>,
    bristle_rates: Vec<f64>,
    contacts: Vec<super::super::ContactPoint>,
}

impl<'a> RigidEmbedding<'a> {
    /// Prepare the shared rigid dynamics at exactly this mechanical state.
    /// Contact geometry, friction, passive loads, gravity and velocity bias
    /// are current. A changed pose, velocity, history or temperature requires
    /// a new preparation. Applied component loads remain separate and fresh.
    pub fn prepare_dynamics<'m>(
        &'m self,
        motion: &EmbeddedMotion,
    ) -> Result<PreparedEmbeddedDynamics<'m, 'a>, String> {
        sim_solve::profile::EMBEDDED_DYNAMICS_PREPARE.time(|| self.prepare_dynamics_impl(motion))
    }

    fn prepare_dynamics_impl<'m>(
        &'m self,
        motion: &EmbeddedMotion,
    ) -> Result<PreparedEmbeddedDynamics<'m, 'a>, String> {
        self.validate_seed(&motion.generalized)?;
        if motion.tangent.shape() != (self.full_dimension(), self.reduced_dimension())
            || motion.acceleration_bias.len() != self.full_dimension()
            || motion
                .tangent
                .iter()
                .chain(motion.acceleration_bias.iter())
                .any(|v| !v.is_finite())
        {
            return Err("invalid embedded dynamics input".into());
        }
        let mut g = motion.generalized.clone();
        let velocity: Vec<_> = self
            .art
            .bases
            .iter()
            .filter(|b| !b.grounded)
            .flat_map(|b| g.states[b.state + 7..b.state + 13].iter().copied())
            .chain(g.qd.iter().copied())
            .collect();
        self.set_motion(&mut g, &velocity, motion.acceleration_bias.as_slice());
        let mass = sim_solve::profile::EMBEDDED_INERTIA.time(|| self.art.rigid_mass_matrix(&g))?;
        let reduced = sim_solve::profile::EMBEDDED_PROJECT_INERTIA.time(|| motion.tangent.transpose() * mass * &motion.tangent);
        let evaluation = sim_solve::profile::EMBEDDED_FORCE_EVALUATION.time(|| self.art.evaluate(&g));
        let required = DVector::from_iterator(
            self.full_dimension(),
            self.art
                .bases
                .iter()
                .enumerate()
                .filter(|(_, b)| !b.grounded)
                .flat_map(|(i, _)| evaluation.base_wrench[i])
                .chain(
                    evaluation
                        .joints
                        .iter()
                        .flat_map(|j| j.tau_needed.iter().zip(&j.tau_passive).map(|(a, b)| a - b)),
                ),
        );
        if reduced
            .iter()
            .chain(required.iter())
            .any(|x| !x.is_finite())
        {
            return Err("nonfinite reduced dynamics".into());
        }
        let factor = reduced
            .clone()
            .cholesky()
            .ok_or("reduced inertia is not positive definite")?;
        Ok(PreparedEmbeddedDynamics {
            map: self,
            generalized: g,
            tangent: motion.tangent.clone(),
            acceleration_bias: motion.acceleration_bias.clone(),
            velocity,
            required,
            reduced,
            factor,
            bristle_rates: evaluation.bristle_rates,
            contacts: evaluation.contacts,
        })
    }
}

impl PreparedEmbeddedDynamics<'_, '_> {
    /// Solve with fresh full-coordinate forces/torques. Retains the original
    /// subtraction/projection order and balance check, avoiding a changed
    /// floating-point expression from separately projecting passive loads.
    pub fn accelerations(&self, applied: &[f64]) -> Result<EmbeddedAcceleration, String> {
        sim_solve::profile::EMBEDDED_DYNAMICS_APPLY.time(|| self.accelerations_impl(applied))
    }

    fn accelerations_impl(&self, applied: &[f64]) -> Result<EmbeddedAcceleration, String> {
        if applied.len() != self.map.full_dimension() || applied.iter().any(|v| !v.is_finite()) {
            return Err("invalid embedded dynamics input".into());
        }
        let rhs = self.tangent.transpose() * (DVector::from_column_slice(applied) - &self.required);
        if rhs.iter().any(|x| !x.is_finite()) {
            return Err("nonfinite reduced dynamics".into());
        }
        let reduced_accelerations = self.factor.solve(&rhs);
        let projected_balance_residual = &self.reduced * &reduced_accelerations - &rhs;
        let full_accelerations = &self.tangent * &reduced_accelerations + &self.acceleration_bias;
        if full_accelerations.iter().any(|v| !v.is_finite())
            || projected_balance_residual.amax() > 1e-8 * (1.0 + rhs.amax())
        {
            return Err("reduced dynamics linear solve failed its residual check".into());
        }
        let mut g = self.generalized.clone();
        self.map
            .set_motion(&mut g, &self.velocity, full_accelerations.as_slice());
        for link in self.map.art.links.iter().filter(|l| !l.grounded) {
            let s = link.bristle_state;
            g.rates[s..s + 3].copy_from_slice(&self.bristle_rates[s..s + 3]);
        }
        Ok(EmbeddedAcceleration {
            generalized: g,
            reduced_accelerations,
            full_accelerations,
            projected_balance_residual,
            bristle_rates: self.bristle_rates.clone(),
            contacts: self.contacts.clone(),
        })
    }
}
