//! Numerical initial guesses only. This module cannot return a physical step.
//! Its frozen inertia/passive loads never reach the committed exact endpoint.
use super::*;
use nalgebra::{DVector};

impl RigidEmbedding<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn implicit_velocity_prediction(
        &self, seed: &Generalized, auxiliary_seed: &[f64], time_s: f64, step_s: f64,
        config: &ImplicitStepConfig, mechanical_guess: Option<&[f64]>,
        matrix: &mut Option<JacobianCache>,
        coupling: &dyn Fn(f64,f64,&Generalized,&[f64],&[f64])->Result<CoupledForces,String>,
    ) -> Result<Vec<f64>,String> {
        let old=self.reduced_velocity(seed);
        let n=old.len();
        if n==0 { return Err("predictor has no mechanical velocities".into()); }
        let initial=mechanical_guess.unwrap_or(&old);
        let (trial,q)=self.implicit_trial_pose(seed,step_s,initial);
        let reference=self.solve(&trial,&q,initial)?;
        let prepared=self.prepare_dynamics(&reference)?;
        let selected:Vec<_>=(0..self.base_columns)
            .chain(self.independent.iter().map(|i|self.base_columns+i)).collect();
        let mut unknowns=initial.to_vec();
        if config.auxiliary_rate_unknowns {unknowns.resize(n+auxiliary_seed.len(),0.0);}
        else {unknowns.extend_from_slice(auxiliary_seed);}
        let error=RefCell::new(None);
        let residual=|x:&[f64],r:&mut[f64]| {
            let mut evaluate=|| -> Result<(),String> {
                let u=&x[..n];
                let (mut mechanics,q)=self.implicit_trial_pose(seed,step_s,u);
                let delta=DVector::from_iterator(n,u.iter().zip(initial).map(|(a,b)|step_s*(a-b)));
                let full_delta=&reference.tangent*delta;
                for i in 0..mechanics.q.len() { mechanics.q[i]=reference.generalized.q[i]+full_delta[self.base_columns+i]; }
                for (&i,&value) in self.independent.iter().zip(&q) {mechanics.q[i]=value;}
                let velocity=&reference.tangent*DVector::from_column_slice(u);
                self.set_motion(&mut mechanics,velocity.as_slice(),reference.acceleration_bias.as_slice());
                let states:Vec<_>=if config.auxiliary_rate_unknowns {
                    auxiliary_seed.iter().zip(&x[n..]).map(|(old,rate)|old+step_s*rate).collect()
                } else {x[n..].to_vec()};
                let rates:Vec<_>=if config.auxiliary_rate_unknowns {x[n..].to_vec()}
                    else {states.iter().zip(auxiliary_seed).map(|(new,old)|(new-old)/step_s).collect()};
                let components=sim_solve::profile::EMBEDDED_COMPONENTS.time(|| coupling(time_s+step_s,step_s,&mechanics,&states,&rates))?;
                if components.auxiliary_residuals.len()!=auxiliary_seed.len()
                    || components.auxiliary_residuals.iter().any(|v|!v.is_finite()) {
                    return Err("invalid predictor component residual".into());
                }
                let acceleration=prepared.reduced_accelerations(&components.generalized_loads)?;
                for j in 0..n {r[j]=(u[j]-old[j]-step_s*acceleration[j])/self.column_scales[selected[j]];}
                r[n..].copy_from_slice(&components.auxiliary_residuals);
                Ok(())
            };
            if let Err(e)=evaluate() { *error.borrow_mut()=Some(e);r.fill(f64::NAN); }
        };
        let scale=|i:usize,value:f64| {
            if i>=n && config.auxiliary_rate_unknowns {
                (1.0+(auxiliary_seed[i-n]+step_s*value).abs())/step_s
            } else {1.0+value.abs()}
        };
        // Bound work spent on a guess. Failure is harmless: the caller runs
        // the original exact solve from its original guess and workspace.
        let config=NewtonConfig {max_iterations:6,broyden_updates:false,broyden_negligible_updates:false,..config.newton};
        sim_solve::solve_newton_numeric_scaled_cached_audited(&mut unknowns,config,residual,&scale,matrix,None)
            .map_err(|e|format!("frozen-mechanics predictor: {e}; component error: {:?}",error.borrow()))?;
        if unknowns[..n].iter().any(|v|!v.is_finite()) {return Err("nonfinite velocity prediction".into());}
        Ok(unknowns[..n].to_vec())
    }
}
