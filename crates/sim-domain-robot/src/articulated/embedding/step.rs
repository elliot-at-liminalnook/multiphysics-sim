//! Experimental second-order explicit midpoint advancement on a local rigid
//! mechanism chart. Quaternion rotations use world-frame exponential updates.
use super::{EmbeddedAcceleration, Generalized, RigidEmbedding};
use crate::math::{V, quat, quat_parts};
use nalgebra::UnitQuaternion;

#[derive(Debug)]
pub struct EmbeddedStep {
    pub time_s: f64,
    /// Endpoint mechanical state, physical accelerations and contact history.
    pub endpoint: EmbeddedAcceleration,
}

impl RigidEmbedding<'_> {
    /// Read velocity in this chart's order: floating-base world twist, then
    /// selected independent joint rates. Does not assume joints are actuated.
    pub fn reduced_velocity(&self, g: &Generalized) -> Vec<f64> {
        self.art
            .bases
            .iter()
            .filter(|b| !b.grounded)
            .flat_map(|b| g.states[b.state + 7..b.state + 13].iter().copied())
            .chain(self.independent.iter().map(|i| g.qd[*i]))
            .collect()
    }
    pub(super) fn trial_state(
        &self,
        original: &Generalized,
        h: f64,
        stage: &Generalized,
        acceleration: &EmbeddedAcceleration,
    ) -> Result<(Generalized, Vec<f64>, Vec<f64>), String> {
        let mut trial = original.clone();
        let velocity = self.reduced_velocity(stage);
        let original_velocity = self.reduced_velocity(original);
        for b in self.art.bases.iter().filter(|b| !b.grounded) {
            let s = b.state;
            for k in 0..3 {
                trial.states[s + k] = original.states[s + k] + h * velocity[k];
            }
            let p = &original.states[s + 3..s + 7];
            if p.iter().map(|v| v * v).sum::<f64>() < 1e-20 {
                return Err("invalid base quaternion".into());
            }
            let orientation = quat(p[0], p[1], p[2], p[3]);
            let rotation =
                UnitQuaternion::from_scaled_axis(V::new(velocity[3], velocity[4], velocity[5]) * h);
            // World angular velocity acts on the left of local-to-world q.
            trial.states[s + 3..s + 7].copy_from_slice(&quat_parts(&(rotation * orientation)));
        }
        let positions: Vec<_> = self
            .independent
            .iter()
            .enumerate()
            .map(|(j, i)| original.q[*i] + h * velocity[self.base_columns + j])
            .collect();
        let velocities: Vec<_> = original_velocity
            .iter()
            .zip(acceleration.reduced_accelerations.iter())
            .map(|(v, a)| v + h * a)
            .collect();
        for link in self.art.links.iter().filter(|l| !l.grounded) {
            for s in link.bristle_state..link.bristle_state + 3 {
                trial.states[s] = original.states[s] + h * acceleration.bristle_rates[s];
            }
        }
        Ok((trial, positions, velocities))
    }

    /// Advance one explicit midpoint step. The load function is evaluated at
    /// start, midpoint and endpoint and MUST be a pure load law of its inputs.
    /// Sampled controllers must hold their command outside this function and
    /// split steps at control/event deadlines. No controller is ticked here.
    ///
    /// The input is never mutated; errors return no partially advanced state.
    /// Contact bristle history uses the same midpoint stages as mechanics.
    /// This is not an adaptive/error-controlled or impact-event integrator:
    /// caller-supplied timesteps require refinement/stability validation.
    /// Authored sampled IMUs are rejected until their schedule is integrated.
    pub fn step_midpoint<F>(
        &self,
        seed: &Generalized,
        time_s: f64,
        step_s: f64,
        loads: F,
    ) -> Result<EmbeddedStep, String>
    where
        F: Fn(f64, &Generalized) -> Result<Vec<f64>, String>,
    {
        self.validate_seed(seed)?;
        if !time_s.is_finite()
            || time_s < 0.0
            || !step_s.is_finite()
            || step_s <= 0.0
            || !(time_s + step_s).is_finite()
            || time_s + step_s <= time_s
        {
            return Err("invalid mechanical step time or duration".into());
        }
        if !self.art.imus.is_empty() {
            return Err("embedded midpoint does not yet advance authored IMU schedules".into());
        }
        let q: Vec<_> = self.independent.iter().map(|i| seed.q[*i]).collect();
        let u = self.reduced_velocity(seed);
        let start = self.solve(seed, &q, &u)?;
        let a = self.accelerations(&start, &loads(time_s, &start.generalized)?)?;
        let (trial, q, u) =
            self.trial_state(&start.generalized, 0.5 * step_s, &start.generalized, &a)?;
        let midpoint = self.solve(&trial, &q, &u)?;
        let b = self.accelerations(
            &midpoint,
            &loads(time_s + 0.5 * step_s, &midpoint.generalized)?,
        )?;
        let (trial, q, u) =
            self.trial_state(&start.generalized, step_s, &midpoint.generalized, &b)?;
        let end = self.solve(&trial, &q, &u)?;
        let endpoint = self.accelerations(&end, &loads(time_s + step_s, &end.generalized)?)?;
        Ok(EmbeddedStep {
            time_s: time_s + step_s,
            endpoint,
        })
    }
}
