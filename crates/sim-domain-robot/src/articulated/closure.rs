//! Constraint residuals shared by full dynamics and inexpensive derivative probes.
use super::*;

impl Articulated {
    /// Evaluate the exact loop and transmission residuals without contact queries
    /// or the backward force pass. Row order matches `Evaluation::loop_rows`:
    /// compiled loops first, then transmissions.
    ///
    /// This includes acceleration, velocity/position stabilization and configured
    /// CFM terms, including topology-certified identities when enabled. Modal
    /// deflections use the same forward kinematics as the complete evaluator.
    /// Inputs have the same dimensions and conventions as `evaluate`.
    pub fn evaluate_constraint_rows(&self, g: &Generalized) -> Vec<f64> {
        let (links, _, _) = self.kinematics(g);
        self.constraint_rows_with_reactions(g, &links, None)
    }

    pub(super) fn constraint_rows_with_reactions(
        &self,
        g: &Generalized,
        links: &[LinkKin],
        mut reactions: Option<(&mut [V], &mut [V], &mut [f64])>,
    ) -> Vec<f64> {
        let mut loop_rows = Vec::new();
        for lp in &self.loops {
            let (ka, kb) = (&links[lp.a], &links[lp.b]);
            let ra = ka.r * lp.r_a;
            let rb = kb.r * lp.r_b;
            let lam = &g.states[lp.lambda_state..lp.lambda_state + lp.rows];
            let f = V::new(lam[0], lam[1], lam[2]);
            if let Some((f_ext, t_ext, _)) = reactions.as_mut() {
                f_ext[lp.b] += f;
                t_ext[lp.b] += rb.cross(&f);
                f_ext[lp.a] -= f;
                t_ext[lp.a] -= ra.cross(&f);
            }
            let pa = ka.p + ra;
            let pb = kb.p + rb;
            let va = ka.vel + ka.w.cross(&ra);
            let vb = kb.vel + kb.w.cross(&rb);
            let aa = ka.acc + ka.alpha.cross(&ra) + ka.w.cross(&ka.w.cross(&ra));
            let ab = kb.acc + kb.alpha.cross(&rb) + kb.w.cross(&kb.w.cross(&rb));
            let al = self.loop_alpha;
            let phi = pb - pa;
            let dphi = vb - va;
            let ddphi = ab - aa;
            for k in 0..3 {
                loop_rows.push(
                    ddphi[k] + 2.0 * al * dphi[k] + al * al * phi[k] + self.loop_cfm * lam[k],
                );
            }
            if let Some((e1, e2, ax)) = &lp.axis {
                if lp.angular_redundant {
                    // phi, dphi and ddphi are structural zeros, not small
                    // residuals being discarded. Their generalized reaction
                    // forces are also zero. Preserve CFM and state ordering.
                    loop_rows.extend([
                        self.loop_angular_cfm * lam[3],
                        self.loop_angular_cfm * lam[4],
                    ]);
                    continue;
                }
                let a_w = kb.r * ax;
                let da = kb.w.cross(&a_w);
                let dda = kb.alpha.cross(&a_w) + kb.w.cross(&kb.w.cross(&a_w));
                for (row, e_l) in [(3usize, e1), (4, e2)] {
                    let e_w = ka.r * e_l;
                    let de = ka.w.cross(&e_w);
                    let dde = ka.alpha.cross(&e_w) + ka.w.cross(&ka.w.cross(&e_w));
                    let phi = e_w.dot(&a_w);
                    let dphi = de.dot(&a_w) + e_w.dot(&da);
                    let ddphi = dde.dot(&a_w) + 2.0 * de.dot(&da) + e_w.dot(&dda);
                    loop_rows.push(
                        ddphi + 2.0 * al * dphi + al * al * phi + self.loop_angular_cfm * lam[row],
                    );
                    let torque = a_w.cross(&e_w) * lam[row];
                    if let Some((_, t_ext, _)) = reactions.as_mut() {
                        t_ext[lp.b] += torque;
                        t_ext[lp.a] -= torque;
                    }
                }
            }
        }
        for t in &self.transmissions {
            let lambda = g.states[t.lambda_state];
            let phi = g.q[t.driver] - t.ratio * g.q[t.driven];
            let velocity = g.qd[t.driver] - t.ratio * g.qd[t.driven];
            let acceleration = g.qdd[t.driver] - t.ratio * g.qdd[t.driven];
            loop_rows.push(
                acceleration
                    + 2.0 * self.loop_alpha * velocity
                    + self.loop_alpha.powi(2) * phi
                    + self.loop_angular_cfm * lambda,
            );
            if let Some((_, _, transmission_torque)) = reactions.as_mut() {
                transmission_torque[t.driver] += lambda;
                transmission_torque[t.driven] -= t.ratio * lambda;
            }
        }
        loop_rows
    }
}

impl Articulated {
    pub(super) fn constraint_state_jacobian(
        &self,
        view: &sim_core::View,
        rates: &[f64],
        out: &mut sim_core::LocalJacobian,
    ) -> Vec<usize> {
        use sim_core::{Input, Output};
        if self.constraint_state_step == 0.0 {
            return Vec::new();
        }
        let mut rows = Vec::new();
        let mut moving_rows = Vec::new();
        let mut offset = 0;
        for lp in &self.loops {
            for k in 3..lp.rows {
                let row = lp.lambda_state + k;
                rows.push(row);
                out.state_state(row, row, self.loop_angular_cfm);
                if !lp.angular_redundant {
                    moving_rows.push((row, offset + k));
                }
            }
            offset += lp.rows;
        }
        if moving_rows.is_empty() {
            return rows;
        }
        // Only kinematic inputs can affect closure. Reactions contribute the
        // explicit CFM diagonal above; temperatures, IMUs and bristles do not.
        let mut states = Vec::new();
        for b in &self.bases {
            states.extend(b.state..b.state + BASE_STATES);
        }
        for (_, d) in self.dofs() {
            states.push(d.qd_state);
            if let Some(q) = d.q_state {
                states.push(q);
            }
        }
        for l in &self.links {
            if let Some(f) = &l.flex {
                states.extend(f.state..f.state + 2 * f.modes);
            }
        }
        states.sort_unstable();
        states.dedup();
        let inputs: Vec<_> = states
            .into_iter()
            .map(Input::State)
            .chain(
                self.dofs()
                    .filter_map(|(_, d)| d.port.map(|p| Input::Across(p, 0))),
            )
            .collect();
        let evaluate = |view: &sim_core::View| {
            let mut g = self.read_view(view);
            // Match read_ctx: algebraic grounded-base rates are not read.
            for b in self.bases.iter().filter(|b| !b.grounded) {
                g.rates[b.state..b.state + BASE_STATES]
                    .copy_from_slice(&rates[b.state..b.state + BASE_STATES]);
            }
            for (i, (_, d)) in self.dofs().enumerate() {
                g.qdd[i] = rates[d.qd_state];
            }
            for l in &self.links {
                if let Some(f) = &l.flex {
                    g.rates[f.state..f.state + 2 * f.modes]
                        .copy_from_slice(&rates[f.state..f.state + 2 * f.modes]);
                }
            }
            self.evaluate_constraint_rows(&g)
        };
        let differentiate = |inputs: &[Input]| {
            let mut states = view.states.to_vec();
            let mut across = view.across.to_vec();
            let mut entries = Vec::new();
            for &input in inputs {
                let (values, index) = match input {
                    Input::State(i) => (&mut states, i),
                    Input::Across(p, k) => (&mut across, view.offsets[p] + k),
                    _ => unreachable!(),
                };
                let value = values[index];
                let h = self.constraint_state_step * (1.0 + value.abs());
                let mut samples = Vec::with_capacity(4);
                for delta in [h, -h, 0.5 * h, -0.5 * h] {
                    match input {
                        Input::State(i) => states[i] = value + delta,
                        Input::Across(p, k) => across[view.offsets[p] + k] = value + delta,
                        _ => unreachable!(),
                    }
                    samples.push(evaluate(&sim_core::View {
                        states: &states,
                        across: &across,
                        ..*view
                    }));
                }
                match input {
                    Input::State(i) => states[i] = value,
                    Input::Across(p, k) => across[view.offsets[p] + k] = value,
                    _ => unreachable!(),
                }
                for &(row, index) in &moving_rows {
                    let coarse = (samples[0][index] - samples[1][index]) / (2.0 * h);
                    let fine = (samples[2][index] - samples[3][index]) / h;
                    let derivative = (4.0 * fine - coarse) / 3.0;
                    if derivative != 0.0 {
                        entries.push((Output::State(row), input, derivative));
                    }
                }
            }
            entries
        };
        #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
        if inputs.len() >= 32 && rayon::current_num_threads() > 1 {
            use rayon::prelude::*;
            let parts: Vec<_> = inputs
                .par_chunks(sim_core::linearization_batch_columns(
                    rayon::current_num_threads(),
                ))
                .map(differentiate)
                .collect();
            for part in parts {
                out.entries.extend(part);
            }
            return rows;
        }
        out.entries.extend(differentiate(&inputs));
        rows
    }
}
