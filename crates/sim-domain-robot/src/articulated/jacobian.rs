//! Hybrid linearization: exact inertial rate columns, constraint reactions,
//! transmission rows and kinematic identities. Configuration-dependent dynamics
//! and geometric loop/contact derivatives still use local differences.
//! Keeping the numerical remainder here lets us omit known zero rate columns
//! and replace individual structures without a second set of physics equations.
use super::*;
use sim_core::{Input, LocalJacobian, Output};

struct Sample {
    states: Vec<f64>,
    through: Vec<f64>,
    signals: Vec<f64>,
}

impl Articulated {
    fn sample_jacobian(&self, view: &View, rates: &[f64],
        prepared: Option<&prepared::ContactLinearization<'_>>) -> (Sample, Evaluation) {
        let mut result = Sample {
            states: vec![0.0; self.state_count],
            through: vec![0.0; view.across.len()],
            signals: vec![0.0; self.signal_out_names.len()],
        };
        let mut ctx = Context::new(
            view.time,
            view.states,
            rates,
            view.offsets,
            view.rate_map,
            view.across,
            view.across_rates,
            view.signals_in,
            &mut result.states,
            &mut result.through,
            &mut result.signals,
        );
        let g = self.read_ctx(&ctx);
        let e = match prepared {
            Some(prepared) => prepared.evaluate(&g),
            None => self.evaluate(&g),
        };
        self.write_residual(&mut ctx, &g, &e);
        (result, e)
    }

    pub(super) fn hybrid_jacobian(&self, view: &View, rates: &[f64], out: &mut LocalJacobian) {
        self.structured_jacobian(view,rates,out,true);
    }

    /// Shared exact blocks; rate-only callers skip the numerical state remainder.
    pub(super) fn structured_jacobian(&self, view: &View, rates: &[f64], out: &mut LocalJacobian, state_remainder:bool) {
        // Immutable, scoped preparation is shared by derivative workers. The
        // existing bitwise dependency checks still recompute geometry after
        // pose changes and forces after velocity/bristle changes. Do not create
        // it with contact disabled: that would enable a different force law.
        let prepared = self.contact_on.then(|| prepared::ContactLinearization::new(self, view, rates));
        let (base, e) = self.sample_jacobian(view, rates, prepared.as_ref());
        let mut exact_rows = vec![false; self.state_count];
        let mut exact_columns = vec![false; self.state_count];
        let mut acceleration_columns = Vec::new();
        for b in &self.bases {
            let o = b.state;
            if b.grounded {
                for k in 0..BASE_STATES {
                    exact_rows[o + k] = true;
                    out.state_state(o + k, o + k, 1.0);
                }
            } else {
                for k in 0..3 {
                    exact_rows[o + k] = true;
                    out.state_rate(o + k, o + k, 1.0);
                    out.state_state(o + k, o + 7 + k, -1.0);
                }
                for k in 3..7 {
                    out.state_rate(o + k, o + k, 1.0);
                }
                acceleration_columns.extend(o + 7..o + 13);
            }
        }
        for (_, d) in self.dofs() {
            acceleration_columns.push(d.qd_state);
            let row = d.q_state.unwrap_or(d.qd_state);
            exact_rows[row] = true;
            if let Some(p) = d.port {
                out.state_state(row, d.qd_state, 1.0);
                out.set(Output::State(row), Input::AcrossDerivative(p, 0), -1.0);
            } else {
                out.state_rate(row, row, 1.0);
                out.state_state(row, d.qd_state, -1.0);
            }
        }
        for l in &self.links {
            if let Some(f) = &l.flex {
                for m in 0..f.modes {
                    let row = f.state + m;
                    exact_rows[row] = true;
                    out.state_rate(row, row, 1.0);
                    out.state_state(row, row + f.modes, -1.0);
                    acceleration_columns.push(row + f.modes);
                }
            }
            if !l.grounded {
                for k in 0..3 {
                    out.state_rate(l.bristle_state + k, l.bristle_state + k, 1.0);
                    if !self.contact_on {
                        exact_columns[l.bristle_state + k] = true;
                    }
                }
            }
        }
        for imu in &self.imus {
            for k in 0..16 {
                exact_rows[imu.state + k] = true;
                exact_columns[imu.state + k] = true;
                out.state_rate(imu.state + k, imu.state + k, 1.0);
            }
            for k in 0..6 {
                out.set(
                    Output::Signal(imu.signals[k]),
                    Input::State(imu.state + k),
                    1.0,
                );
            }
        }
        for lp in &self.loops {
            for k in 0..lp.rows {
                let col = lp.lambda_state + k;
                exact_columns[col] = true;
                if k >= 3 && lp.angular_redundant {
                    exact_rows[col] = true;
                    out.state_state(col, col, self.loop_angular_cfm);
                    continue;
                }
                let mut forces = vec![V::zeros(); self.links.len()];
                let mut moments = forces.clone();
                if k < 3 {
                    let mut f = V::zeros();
                    f[k] = 1.0;
                    forces[lp.a] += f;
                    forces[lp.b] -= f;
                    moments[lp.a] += (e.links[lp.a].r * lp.r_a).cross(&f);
                    moments[lp.b] -= (e.links[lp.b].r * lp.r_b).cross(&f);
                } else {
                    let (e1, e2, ax) = lp.axis.as_ref().expect("angular loop row");
                    let torque = (e.links[lp.b].r * ax)
                        .cross(&(e.links[lp.a].r * if k == 3 { e1 } else { e2 }));
                    moments[lp.a] += torque;
                    moments[lp.b] -= torque;
                }
                self.reaction_column(&e, forces, moments, Input::State(col), out);
                out.state_state(
                    col,
                    col,
                    if k < 3 {
                        self.loop_cfm
                    } else {
                        self.loop_angular_cfm
                    },
                );
            }
        }
        let dofs: Vec<_> = self.dofs().map(|(_, d)| d).collect();
        for t in &self.transmissions {
            exact_columns[t.lambda_state] = true;
            exact_rows[t.lambda_state] = true;
            out.state_state(t.lambda_state, t.lambda_state, self.loop_angular_cfm);
            for (index, weight) in [(t.driver, 1.0), (t.driven, -t.ratio)] {
                let d = dofs[index];
                let position = match d.port {
                    Some(p) => Input::Across(p, 0),
                    None => Input::State(d.q_state.unwrap()),
                };
                out.set(
                    Output::State(t.lambda_state),
                    position,
                    self.loop_alpha.powi(2) * weight,
                );
                out.state_state(t.lambda_state, d.qd_state, 2.0 * self.loop_alpha * weight);
                out.state_rate(t.lambda_state, d.qd_state, weight);
                out.set(dof_output(d), Input::State(t.lambda_state), -weight);
            }
        }
        // Inverse dynamics is affine in acceleration. With all velocities,
        // reaction forces and acceleration-independent loads zero, a unit
        // acceleration gives a mass-matrix column directly: no tiny probe
        // and no subtraction of gravity/contact loads.
        let mut inertia = self.read_view(view);
        for b in &self.bases {
            for k in 7..13 {
                inertia.states[b.state + k] = 0.0;
            }
        }
        inertia.qd.fill(0.0);
        for (_, d) in self.dofs() {
            inertia.states[d.qd_state] = 0.0;
        }
        for l in &self.links {
            if let Some(f) = &l.flex {
                for m in 0..f.modes {
                    inertia.states[f.state + f.modes + m] = 0.0;
                }
            }
        }
        for lp in &self.loops {
            for k in 0..lp.rows {
                inertia.states[lp.lambda_state + k] = 0.0;
            }
        }
        for t in &self.transmissions {
            inertia.states[t.lambda_state] = 0.0;
        }
        for col in acceleration_columns {
            inertia.rates[col] = 1.0;
            for (i, d) in dofs.iter().enumerate() {
                inertia.qdd[i] = inertia.rates[d.qd_state];
            }
            let linear = self.evaluate_forces(&inertia, false, true);
            self.inertia_column(&linear, col, out);
            inertia.rates[col] = 0.0;
        }

        if !state_remainder {return;}

        let inputs: Vec<_> = (0..self.state_count)
            .filter(|&i| !exact_columns[i])
            .map(Input::State)
            .chain(
                dofs.iter()
                    .filter_map(|d| d.port.map(|p| Input::Across(p, 0))),
            )
            .chain((0..view.signals_in.len()).map(Input::Signal))
            .collect();
        let differentiate = |inputs: &[Input]| {
            let mut states = view.states.to_vec();
            let mut across = view.across.to_vec();
            let mut signals = view.signals_in.to_vec();
            let mut perturbed_rates = rates.to_vec();
            let mut out = LocalJacobian::default();
            for &input in inputs {
                let value = match input {
                    Input::State(i) => states[i],
                    Input::StateRate(i) => perturbed_rates[i],
                    Input::Across(p, k) => across[view.offsets[p] + k],
                    Input::Signal(i) => signals[i],
                    _ => unreachable!(),
                };
                let requested = 1e-8 * (1.0 + value.abs());
                let perturbed = value + requested;
                let step = perturbed - value;
                let assign = |states: &mut [f64],
                              rates: &mut [f64],
                              across: &mut [f64],
                              signals: &mut [f64],
                              v| {
                    match input {
                        Input::State(i) => states[i] = v,
                        Input::StateRate(i) => rates[i] = v,
                        Input::Across(p, k) => across[view.offsets[p] + k] = v,
                        Input::Signal(i) => signals[i] = v,
                        _ => unreachable!(),
                    }
                };
                assign(
                    &mut states,
                    &mut perturbed_rates,
                    &mut across,
                    &mut signals,
                    perturbed,
                );
                let changed = View {
                    states: &states,
                    across: &across,
                    signals_in: &signals,
                    ..*view
                };
                let (sample, _) = self.sample_jacobian(&changed, &perturbed_rates, prepared.as_ref());
                assign(
                    &mut states,
                    &mut perturbed_rates,
                    &mut across,
                    &mut signals,
                    value,
                );
                let mut emit = |output, a, b| {
                    let derivative = (a - b) / step;
                    if derivative != 0.0 {
                        out.set(output, input, derivative);
                    }
                };
                for i in 0..self.state_count {
                    if !exact_rows[i] {
                        emit(Output::State(i), sample.states[i], base.states[i]);
                    }
                }
                for p in 0..view.offsets.len() - 1 {
                    for k in 0..view.offsets[p + 1] - view.offsets[p] {
                        let i = view.offsets[p] + k;
                        emit(Output::Through(p, k), sample.through[i], base.through[i]);
                    }
                }
                for i in 0..sample.signals.len() {
                    emit(Output::Signal(i), sample.signals[i], base.signals[i]);
                }
            }
            out.entries
        };
        // The hybrid's numerical remainder can be as large as an entire
        // behavior. Small batches balance fresh geometry queries against cached
        // probes. Keep indexed column order and private chunk scratch, just
        // as the compiler's numerical fallback does. WASM remains serial.
        #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
        if inputs.len() >= 32 && rayon::current_num_threads() > 1 {
            use rayon::prelude::*;
            let columns: Vec<_> = inputs.par_chunks(sim_core::linearization_batch_columns(rayon::current_num_threads())).map(differentiate).collect();
            for entries in columns {
                out.entries.extend(entries);
            }
            return;
        }
        out.entries.extend(differentiate(&inputs));
    }

    fn inertia_column(&self, e: &Evaluation, col: usize, out: &mut LocalJacobian) {
        let input = Input::StateRate(col);
        let mut emit = |row, value| {
            if value != 0.0 {
                out.set(row, input, value);
            }
        };
        for (bi, b) in self.bases.iter().enumerate() {
            if !b.grounded {
                for k in 0..6 {
                    emit(Output::State(b.state + 7 + k), e.base_wrench[bi][k]);
                }
            }
        }
        for (ji, j) in self.joints.iter().enumerate() {
            for (k, d) in j.dofs.iter().enumerate() {
                emit(dof_output(d), e.joints[ji].tau_needed[k]);
            }
        }
        for (li, l) in self.links.iter().enumerate() {
            let Some(f) = &l.flex else { continue };
            for m in 0..f.modes {
                let row = f.state + f.modes + m;
                let mass = if row == col { f.mass[m] } else { 0.0 };
                emit(
                    Output::State(row),
                    (mass - e.modal_force[li][m]) / f.stiffness[m].max(1.0),
                );
            }
        }
        for lp in &self.loops {
            let (a, b) = (&e.links[lp.a], &e.links[lp.b]);
            let aa = a.acc + a.alpha.cross(&(a.r * lp.r_a));
            let ab = b.acc + b.alpha.cross(&(b.r * lp.r_b));
            for k in 0..3 {
                emit(Output::State(lp.lambda_state + k), (ab - aa)[k]);
            }
            if let Some((e1, e2, ax)) = lp.axis.as_ref().filter(|_| !lp.angular_redundant) {
                let axis = b.r * ax;
                for (k, local) in [(3, e1), (4, e2)] {
                    let tangent = a.r * local;
                    let derivative =
                        a.alpha.cross(&tangent).dot(&axis) + tangent.dot(&b.alpha.cross(&axis));
                    emit(Output::State(lp.lambda_state + k), derivative);
                }
            }
        }
        // Transmission acceleration rows were already supplied exactly.
    }

    /// Differentiate the backward Newton–Euler force pass at fixed geometry.
    /// Inputs are *required* link wrenches (negative external forces). There
    /// is no differencing or reevaluation of collision geometry in this pass.
    fn reaction_column(
        &self,
        e: &Evaluation,
        mut f: Vec<V>,
        mut n: Vec<V>,
        input: Input,
        out: &mut LocalJacobian,
    ) {
        let mut joint_force = vec![V::zeros(); self.joints.len()];
        let mut joint_moment = joint_force.clone();
        for (ji, j) in self.joints.iter().enumerate().rev() {
            let o = e.joint_points[ji];
            let force = f[j.child];
            let moment = n[j.child] + (e.links[j.child].p - o).cross(&force);
            joint_force[ji] = force;
            joint_moment[ji] = moment;
            f[j.parent] += force;
            n[j.parent] += moment + (o - e.links[j.parent].p).cross(&force);
            for (k, d) in j.dofs.iter().enumerate() {
                let wrench = if d.kind == DofKind::Revolute {
                    moment
                } else {
                    force
                };
                let v = e.joints[ji].axes[k].dot(&wrench);
                if v != 0.0 {
                    out.set(dof_output(d), input, v);
                }
            }
        }
        for b in &self.bases {
            if !b.grounded {
                for k in 0..3 {
                    if f[b.link][k] != 0.0 {
                        out.set(Output::State(b.state + 7 + k), input, f[b.link][k]);
                    }
                    if n[b.link][k] != 0.0 {
                        out.set(Output::State(b.state + 10 + k), input, n[b.link][k]);
                    }
                }
            }
        }
        for (li, l) in self.links.iter().enumerate() {
            let Some(flex) = &l.flex else { continue };
            for m in 0..flex.modes {
                let mut derivative = 0.0;
                for &ji in &l.children {
                    if let Some(boundary) = self.joints[ji].flex_boundary {
                        let shape = flex.shapes[m][boundary];
                        let force = e.links[li].r.transpose() * joint_force[ji];
                        let moment = e.links[li].r.transpose() * joint_moment[ji];
                        derivative += V::new(shape[0], shape[1], shape[2]).dot(&force)
                            + V::new(shape[3], shape[4], shape[5]).dot(&moment);
                    }
                }
                if derivative != 0.0 {
                    out.set(
                        Output::State(flex.state + flex.modes + m),
                        input,
                        derivative / flex.stiffness[m].max(1.0),
                    );
                }
            }
        }
    }
}

fn dof_output(d: &Dof) -> Output {
    match d.port {
        Some(p) => Output::Through(p, 0),
        None => Output::State(d.qd_state),
    }
}
