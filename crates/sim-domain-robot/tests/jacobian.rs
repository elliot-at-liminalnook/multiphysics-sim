mod common;
use common::*;
use sim_core::{Behavior, Context, Input, LocalJacobian, Output, View};
use sim_domain_robot::model::*;
use sim_domain_robot::{Articulated, Options};
use sim_dynamics::jacobian_check::{check_jacobian, CheckConfig};
use sim_dynamics::{JacobianParts, System};
use std::sync::Arc;

// Independent, uncompiled local input/output adapter. No sparsity or state
// alias assumptions from the compiler are reused by the numerical reference.
struct LocalSystem {
    art: Articulated,
    offsets: Vec<usize>,
    rate_map: Vec<Option<usize>>,
    n: usize,
}

#[test]
#[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
fn hybrid_loaded_derivatives_are_identical_across_worker_counts() {
    let (system, x, rates) = loaded_fixture();
    let evaluate = |workers| {
        rayon::ThreadPoolBuilder::new().num_threads(workers).build().unwrap().install(|| {
            let mut result = JacobianParts::default();
            system.jacobian(0.3, &x, &rates, &mut result);
            (result.d_dx, result.d_drate)
        })
    };
    let serial = evaluate(1);
    for workers in [2, 4, 8, 16] {
        assert_eq!(serial, evaluate(workers), "workers={workers}");
    }
}
impl LocalSystem {
    fn new(model: PhysicalModel, mut options: Options) -> Self {
        options.hybrid_jacobian = true;
        let art = Articulated::new(Arc::new(model), &options).unwrap();
        let mut offsets = vec![0, 13];
        for _ in &art.port_names {
            offsets.push(offsets.last().unwrap() + 2);
        }
        let width = *offsets.last().unwrap();
        let mut rate_map = vec![None; width];
        for &start in &offsets[1..offsets.len() - 1] {
            rate_map[start] = Some(start + 1);
        }
        let n = art.state_count + width + art.signal_in_names.len().max(art.signal_out_names.len());
        Self {
            art,
            offsets,
            rate_map,
            n,
        }
    }
    fn view<'a>(&'a self, t: f64, x: &'a [f64], rates: &'a [f64]) -> View<'a> {
        let ns = self.art.state_count;
        let end = ns + self.offsets.last().unwrap();
        View {
            time: t,
            states: &x[..ns],
            offsets: &self.offsets,
            rate_map: &self.rate_map,
            across: &x[ns..end],
            across_rates: &rates[ns..end],
            signals_in: &x[end..end + self.art.signal_in_names.len()],
        }
    }
    fn point(&self) -> (Vec<f64>, Vec<f64>) {
        let mut x = vec![0.0; self.n];
        for (i, state) in self.art.states().iter().enumerate() {
            x[i] = state.initial;
        }
        // Nonzero, non-unit quaternion; moving base, joints and nonzero rates.
        // The local audit need not satisfy the constraints to test derivatives.
        for b in &self.art.bases {
            for k in 3..13 {
                x[b.state + k] += 0.021 * (k as f64 + 0.3).sin();
            }
        }
        for (i, (_, d)) in self.art.dofs().enumerate() {
            x[d.qd_state] = 0.17 + i as f64 * 0.13;
            match d.port {
                Some(p) => x[self.art.state_count + self.offsets[p]] = 0.23 + 0.11 * i as f64,
                None => x[d.q_state.unwrap()] = 0.02,
            }
        }
        for lp in &self.art.loops {
            for k in 0..lp.rows {
                x[lp.lambda_state + k] = 0.4 + 0.3 * k as f64;
            }
        }
        for t in &self.art.transmissions {
            x[t.lambda_state] = 0.7;
        }
        let end = self.art.state_count + self.offsets.last().unwrap();
        for i in 0..self.art.signal_in_names.len() {
            x[end + i] = 298.15;
        }
        let rates = (0..self.n).map(|i| 0.31 * (i as f64 + 0.4).cos()).collect();
        (x, rates)
    }
}
impl System for LocalSystem {
    fn dimension(&self) -> usize {
        self.n
    }
    fn residual(&self, t: f64, x: &[f64], rates: &[f64], out: &mut [f64]) {
        out.fill(0.0);
        let ns = self.art.state_count;
        let width = *self.offsets.last().unwrap();
        let view = self.view(t, x, rates);
        let (states, rest) = out.split_at_mut(ns);
        let (through, signals) = rest.split_at_mut(width);
        let mut ctx = Context::new(
            t,
            view.states,
            &rates[..ns],
            &self.offsets,
            &self.rate_map,
            view.across,
            view.across_rates,
            view.signals_in,
            states,
            through,
            &mut signals[..self.art.signal_out_names.len()],
        );
        self.art.residual(&mut ctx);
    }
    fn jacobian(&self, t: f64, x: &[f64], rates: &[f64], out: &mut JacobianParts) -> bool {
        out.clear();
        let ns = self.art.state_count;
        let width = *self.offsets.last().unwrap();
        let mut local = LocalJacobian::default();
        assert!(self
            .art
            .jacobian_at(&self.view(t, x, rates), &rates[..ns], &mut local));
        for (output, input, v) in local.entries {
            let row = match output {
                Output::State(i) => i,
                Output::Through(p, k) => ns + self.offsets[p] + k,
                Output::Signal(i) => ns + width + i,
            };
            match input {
                Input::State(i) => out.dx(row, i, v),
                Input::StateRate(i) => out.drate(row, i, v),
                Input::Across(p, k) => out.dx(row, ns + self.offsets[p] + k, v),
                Input::AcrossDerivative(p, k) => out.drate(row, ns + self.offsets[p] + k, v),
                Input::AcrossRate(p, k) => match self.rate_map[self.offsets[p] + k] {
                    Some(i) => out.dx(row, ns + i, v),
                    None => out.drate(row, ns + self.offsets[p] + k, v),
                },
                Input::Signal(i) => out.dx(row, ns + width + i, v),
            }
        }
        true
    }
}

fn mechanism(floating: bool) -> PhysicalModel {
    let mut m = empty_model();
    m.links.push(box_link(
        "base",
        [0.4, 0.05, 0.02],
        1.0,
        [0.15, 0.0, 0.3],
        !floating,
    ));
    m.links.push(box_link(
        "crank",
        [0.02, 0.02, 0.1],
        0.05,
        [0.0, 0.0, 0.35],
        false,
    ));
    m.links.push(box_link(
        "coupler",
        [0.3, 0.02, 0.02],
        0.1,
        [0.15, 0.0, 0.4],
        false,
    ));
    m.links.push(box_link(
        "rocker",
        [0.02, 0.02, 0.1],
        0.05,
        [0.3, 0.0, 0.35],
        false,
    ));
    let y = [0.0, 1.0, 0.0];
    m.joints.push(joint(
        "a",
        "revolute",
        Some("base"),
        "crank",
        [0.0, 0.0, 0.3],
        y,
    ));
    m.joints.push(joint(
        "b",
        "revolute",
        Some("crank"),
        "coupler",
        [0.0, 0.0, 0.4],
        y,
    ));
    m.joints.push(joint(
        "d",
        "revolute",
        Some("base"),
        "rocker",
        [0.3, 0.0, 0.3],
        y,
    ));
    m.joints.push(joint(
        "c",
        "loop_revolute",
        Some("coupler"),
        "rocker",
        [0.3, 0.0, 0.4],
        y,
    ));
    m.transmissions.push(Transmission {
        name: "gear".into(),
        driver_joint: "a".into(),
        driven_joint: "d".into(),
        ratio: 5.0,
    });
    for j in &mut m.joints {
        j.physics.friction = Friction::default();
    }
    m
}

#[test]
fn hybrid_matches_independent_residual_columns_and_directions_while_moving() {
    for floating in [false, true] {
        let system = LocalSystem::new(
            mechanism(floating), // A modest stabilization gain keeps the still-numerical loop position
            // derivatives resolvable at the checker's default tolerances. The
            // exact reaction test below separately uses the production gain 100.
            Options {
                contact: false,
                flex: false,
                loop_alpha: 1.0,
                ..Options::default()
            },
        );
        let (x, rates) = system.point();
        let report = check_jacobian(&system, 0.3, &x, &rates, &CheckConfig::default()).unwrap();
        assert!(report.passed, "floating={floating}: {report:#?}");
    }
}

#[test]
fn rigid_loop_axis_identity_holds_through_moving_configurations() {
    let options = Options { contact: false, flex: false, structural_loop_identities: true, ..Options::default() };
    let exact = LocalSystem::new(mechanism(true), options.clone());
    let mut original = LocalSystem::new(mechanism(true), options);
    assert!(exact.art.loops.iter().all(|lp| lp.angular_redundant));
    for lp in &mut original.art.loops { lp.angular_redundant = false; }
    let (base, base_rates) = exact.point();
    for sample in 0..40 {
        let mut x = base.clone();
        let rates: Vec<_> = base_rates.iter().enumerate()
            .map(|(i, r)| r + 2.0 * (i as f64 + sample as f64 * 0.31).sin()).collect();
        // General floating orientation/twist and independent hinge angles,
        // including configurations that violate the translational closure.
        for k in 3..13 { x[k] += 0.3 * (sample as f64 + k as f64).sin(); }
        for (i, (_, d)) in exact.art.dofs().enumerate() {
            x[d.qd_state] = 3.0 * (sample as f64 * 0.17 + i as f64).cos();
            let p = d.port.unwrap();
            x[exact.art.state_count + exact.offsets[p]] = 2.0 * (sample as f64 * 0.13 + i as f64).sin();
        }
        let mut a = vec![0.0; exact.n];
        let mut b = a.clone();
        exact.residual(0.3, &x, &rates, &mut a);
        original.residual(0.3, &x, &rates, &mut b);
        for lp in &exact.art.loops {
            for k in 3..5 {
                let row = lp.lambda_state + k;
                assert_eq!(a[row], exact.art.loop_angular_cfm * x[row]);
            }
        }
        for row in 0..exact.n {
            assert!((a[row] - b[row]).abs() <= 1e-10 * (1.0 + a[row].abs()),
                "sample {sample}, row {row}: {} vs {}", a[row], b[row]);
        }
    }
}

#[test]
fn loop_axis_reduction_rejects_misalignment_independent_bases_and_flex() {
    let options = Options { contact: false, flex: false, structural_loop_identities: true, ..Options::default() };
    let mut tilted = mechanism(true);
    tilted.joints[0].axis = [1e-9, 1.0, 0.0];
    let tilted = LocalSystem::new(tilted, options.clone());
    assert!(!tilted.art.loops[0].angular_redundant, "no angular tolerance may erase a real DOF");
    let mut independent = mechanism(true);
    independent.joints.retain(|j| j.name != "d");
    independent.transmissions.clear();
    let independent = LocalSystem::new(independent, options);
    assert!(!independent.art.loops[0].angular_redundant);
    let (flexible, _, _) = loaded_fixture();
    assert!(!flexible.art.loops[0].angular_redundant);
}

#[test]
fn exact_reaction_columns_survive_large_constraint_forces() {
    let system = LocalSystem::new(
        mechanism(true),
        Options {
            contact: false,
            flex: false,
            ..Options::default()
        },
    );
    let (mut x, rates) = system.point();
    let mut columns = Vec::new();
    for lp in &system.art.loops {
        columns.extend(lp.lambda_state..lp.lambda_state + lp.rows);
    }
    for t in &system.art.transmissions {
        columns.push(t.lambda_state);
    }
    for &col in &columns {
        x[col] = 1e8;
    }
    let mut analytic = JacobianParts::default();
    system.jacobian(0.3, &x, &rates, &mut analytic);
    // These equations are affine in each reaction. A symmetric unit increment
    // is an independent reference that avoids subtracting tiny perturbations
    // from the large reaction forces used in this regression.
    for col in columns {
        let mut plus = vec![0.0; system.n];
        let mut minus = plus.clone();
        x[col] += 1.0;
        system.residual(0.3, &x, &rates, &mut plus);
        x[col] -= 2.0;
        system.residual(0.3, &x, &rates, &mut minus);
        x[col] += 1.0;
        for row in 0..system.n {
            let exact: f64 = analytic
                .d_dx
                .iter()
                .filter(|(r, c, _)| *r == row && *c == col)
                .map(|(_, _, v)| v)
                .sum();
            let reference = (plus[row] - minus[row]) * 0.5;
            assert!(
                (exact - reference).abs() < 1e-7,
                "row {row}, col {col}: {exact} != {reference}"
            );
        }
    }
}

fn loaded_fixture() -> (LocalSystem, Vec<f64>, Vec<f64>) {
    let mut model = mechanism(true);
    model.links.push(box_link(
        "foot",
        [0.02, 0.02, 0.04],
        0.03,
        [0.15, 0.0, 0.32],
        false,
    ));
    model.joints.push(joint(
        "slide",
        "prismatic",
        Some("coupler"),
        "foot",
        [0.15, 0.0, 0.4],
        [0.3, 0.5, 0.7],
    ));
    model.links[1].flex = Some(Flex {
        normalization: ModalNormalization::Displacement,
        modes: 1,
        frequencies_hz: vec![20.0],
        damping_ratio: 0.03,
        boundary_frames: vec![BoundaryFrame {
            name: "tip".into(),
            point: [0.0, 0.0, 0.05],
            ..Default::default()
        }],
        modal_stiffness: vec![1000.0],
        modal_mass: vec![0.1],
        boundary_shapes: vec![vec![[0.1, 0.0, 0.3, 0.0, 0.2, 0.0]]],
        participation: vec![[0.1, 0.2, 0.3, 0.2, 0.0, 0.1]],
        stress_cells: vec![],
        stress_per_mode: vec![],
        gravity_sag_m: 0.0,
        softening: Softening {
            tg_c: 60.0,
            width_c: 5.0,
            ratio_above: 0.05,
        },
    });
    model
        .sensors
        .push(serde_json::from_str(r#"{"name":"imu","link":"crank"}"#).unwrap());
    model.cables.push(serde_json::from_str(r#"{"name":"loaded cable","from":{"link":"base"},"to":{"link":"coupler"},"length":0.02,"mass":0.03,"stiffness":20.0,"damping":0.1}"#).unwrap());
    let mut system = LocalSystem::new(model, Options { structural_loop_identities: true, ..Options::default() });
    let (mut x, rates) = system.point();
    for l in &system.art.links {
        if let Some(f) = &l.flex {
            x[f.state] = 0.003;
            x[f.state + f.modes] = 0.12;
        }
    }
    // Put the floor into the geometry so the tested residual includes actual
    // nonzero contact loads while the inertial operator remains contact-free.
    system.art.floor_z = 0.3;
    assert!(system.art.joints.iter().any(|j| j.flex_boundary.is_some()));
    let view = system.view(0.3, &x, &rates);
    let angles: Vec<_> = (0..system.offsets.len() - 1)
        .map(|p| view.across(p))
        .collect();
    let generalized = system.art.generalized(
        view.states.to_vec(),
        rates[..system.art.state_count].to_vec(),
        &angles,
        view.signals_in.to_vec(),
    );
    assert!(
        !system.art.evaluate(&generalized).contacts.is_empty(),
        "must test loaded contact"
    );
    (system, x, rates)
}

#[test]
fn exact_rate_columns_include_flex_contact_and_nonzero_velocity_biases() {
    let (system, x, rates) = loaded_fixture();
    check_loaded_rate_and_reaction_columns(system, x, rates);
}

#[test]
fn rigid_rate_columns_include_loaded_contact_loops_and_cable_forces() {
    let (source, _, _) = loaded_fixture();
    let mut system = LocalSystem::new((*source.art.model).clone(), Options {
        flex: false, structural_loop_identities: false, ..Options::default()
    });
    system.art.floor_z = 0.3;
    let (x, rates) = system.point();
    let view = system.view(0.3, &x, &rates);
    let angles: Vec<_> = (0..system.offsets.len()-1).map(|p| view.across(p)).collect();
    let g = system.art.generalized(view.states.to_vec(), rates[..system.art.state_count].to_vec(),
        &angles, view.signals_in.to_vec());
    assert!(system.art.links.iter().all(|l| l.flex.is_none()));
    assert!(!system.art.evaluate(&g).contacts.is_empty());
    check_loaded_rate_and_reaction_columns(system, x, rates);
}

fn check_loaded_rate_and_reaction_columns(system: LocalSystem, mut x: Vec<f64>, rates: Vec<f64>) {
    let mut matrix = JacobianParts::default();
    system.jacobian(0.3, &x, &rates, &mut matrix);
    let mut changed = rates.clone();
    let mut plus = vec![0.0; system.n];
    let mut minus = plus.clone();
    for col in 0..system.n {
        changed[col] = rates[col] + 1.0;
        system.residual(0.3, &x, &changed, &mut plus);
        changed[col] = rates[col] - 1.0;
        system.residual(0.3, &x, &changed, &mut minus);
        changed[col] = rates[col];
        for row in 0..system.n {
            let exact: f64 = matrix
                .d_drate
                .iter()
                .filter(|(r, c, _)| *r == row && *c == col)
                .map(|(_, _, v)| v)
                .sum();
            let reference = 0.5 * (plus[row] - minus[row]);
            assert!(
                (exact - reference).abs() < 1e-9,
                "row {row}, rate {col}: {exact} != {reference}"
            );
        }
    }
    // Also exercise loop-force propagation through the modal boundary.
    for lp in &system.art.loops {
        for col in lp.lambda_state..lp.lambda_state + lp.rows {
            let original = x[col];
            x[col] = original + 1.0;
            system.residual(0.3, &x, &rates, &mut plus);
            x[col] = original - 1.0;
            system.residual(0.3, &x, &rates, &mut minus);
            x[col] = original;
            for row in 0..system.n {
                let exact: f64 = matrix
                    .d_dx
                    .iter()
                    .filter(|(r, c, _)| *r == row && *c == col)
                    .map(|(_, _, v)| v)
                    .sum();
                assert!(
                    (exact - 0.5 * (plus[row] - minus[row])).abs() < 1e-9,
                    "loop reaction row {row}, col {col}"
                );
            }
        }
    }
}

#[test]
fn prepared_contact_residual_matches_uncached_for_every_input_and_contact_changes() {
    let (system, x, rates) = loaded_fixture();
    check_prepared_contact_queries(&system, &x, &rates);
}

#[test]
fn public_prepared_evaluation_preserves_full_loaded_results_and_contact_option() {
    use sim_domain_robot::articulated::friction::FloorFrictionModel;
    let (mut system, x, rates) = loaded_fixture();
    let view = system.view(0.3, &x, &rates);
    let angles: Vec<_> = (0..system.offsets.len()-1).map(|p|view.across(p)).collect();
    let base = system.art.generalized(view.states.to_vec(), rates[..system.art.state_count].to_vec(),
        &angles, view.signals_in.to_vec());
    for (contact, friction) in [
        (false,FloorFrictionModel::Bristle), (true,FloorFrictionModel::Bristle),
        (false,FloorFrictionModel::RegularizedCoulomb {slip_speed_m_s:0.001}),
        (true,FloorFrictionModel::RegularizedCoulomb {slip_speed_m_s:0.001}),
    ] {
        system.art.contact_on = contact;
        system.art.floor_friction = friction;
        let prepared = system.art.prepare_evaluation(base.clone());
        let history = system.art.prepare_contact_history_rates(base.clone());
        let check = |g: &sim_domain_robot::Generalized| {
            // Debug includes every Evaluation field, including signed zero.
            assert_eq!(format!("{:?}", prepared(g)), format!("{:?}", system.art.evaluate(g)));
            assert_eq!(history(g).iter().map(|x|x.to_bits()).collect::<Vec<_>>(),
                system.art.evaluate(g).bristle_rates.iter().map(|x|x.to_bits()).collect::<Vec<_>>());
        };
        check(&base);
        for i in 0..base.states.len() {
            let mut g = base.clone();g.states[i] += 0.0123;check(&g);
            let mut g = base.clone();g.rates[i] -= 0.0456;check(&g);
        }
        for i in 0..base.q.len() {
            let mut g = base.clone();g.q[i] += 0.123;check(&g);
            let mut g = base.clone();g.qd[i] -= 0.234;check(&g);
            let mut g = base.clone();g.qdd[i] += 0.345;check(&g);
        }
        for i in 0..base.temperatures.len() {
            let mut g = base.clone();g.temperatures[i] += 70.0;check(&g);
        }
        check(&base);
    }
}

#[test]
fn prepared_kinematics_preserves_sequential_axes_and_compliant_fixed_joints() {
    let mut model = empty_model();
    model.links = vec![
        box_link("base", [0.04; 3], 0.2, [0.0, 0.0, 0.1], false),
        box_link("ball child", [0.03; 3], 0.1, [0.1, 0.0, 0.1], false),
        box_link("fixed child", [0.02; 3], 0.05, [0.2, 0.0, 0.1], false),
    ];
    model.joints = vec![
        joint("ball", "ball", Some("base"), "ball child", [0.05, 0.0, 0.1], [0.3, 0.5, 0.7]),
        joint("fixed", "fixed", Some("ball child"), "fixed child", [0.15, 0.0, 0.1], [0.7, 0.3, 0.5]),
    ];
    let system = LocalSystem::new(model, Options { flex: false, ..Default::default() });
    assert_eq!(system.art.joints[0].dofs.len(), 3);
    assert_eq!(system.art.joints[1].dofs.len(), 6);
    let (x, rates) = system.point();
    check_prepared_contact_queries(&system, &x, &rates);
}

#[test]
fn prepared_floor_geometry_tracks_heightfield_pose_and_velocity_changes() {
    let mut model = empty_model();
    model.links.push(box_link("foot", [0.2; 3], 1.0, [0.0, 0.0, 0.15], false));
    model.world.terrain = Some(Terrain {
        origin: [-0.5, -0.5], cell: 1.0, dims: [2, 2],
        heights: vec![-0.05, 0.1, 0.2, 0.35],
    });
    let system = LocalSystem::new(model, Options { flex: false, ..Default::default() });
    let (x, rates) = system.point();
    let g = system.art.generalized(x[..system.art.state_count].to_vec(),
        rates[..system.art.state_count].to_vec(), &[], vec![]);
    assert!(system.art.evaluate(&g).contacts.iter().any(|c| c.other.is_none()));
    assert_ne!(system.art.floor_height(-0.1, 0.0), system.art.floor_height(0.1, 0.0));
    // The shared oracle varies every state/rate, crosses contact boundaries,
    // reverses motion, and returns to the original prepared point.
    check_prepared_contact_queries(&system, &x, &rates);
}

#[test]
fn prepared_geometry_keeps_velocity_dependent_pair_forces_and_contact_transitions() {
    let mut model = empty_model();
    model.links.push(box_link("first", [0.2; 3], 1.0, [0.0, 0.0, 0.3], false));
    model.links.push(box_link("second", [0.2; 3], 1.0, [0.15, 0.0, 0.3], false));
    // When one body moves, the other pair must retain its prepared geometry.
    // Reused and refreshed positive hits must keep the original force order.
    model.links.push(box_link("third", [0.2; 3], 1.0, [0.05, 0.1, 0.3], false));
    let mut system = LocalSystem::new(model, Options {flex:false, ..Default::default()});
    // Deliberately overlapping independent bodies; construction-time overlap
    // exclusions would otherwise suppress the loaded SDF pair in this fixture.
    for link in &mut system.art.links { link.excluded.clear(); }
    let (x, rates) = system.point();
    let g = |point:&[f64]| system.art.generalized(point[..system.art.state_count].to_vec(),
        rates[..system.art.state_count].to_vec(), &[], vec![]);
    let initial = system.art.evaluate(&g(&x));
    assert!(initial.contacts.iter().any(|c|c.other.is_some()), "must exercise SDF pair geometry");
    let mut moving = x.clone();
    moving[system.art.bases[0].state+7] += 0.4;
    moving[system.art.bases[0].state+8] -= 0.3;
    let changed = system.art.evaluate(&g(&moving));
    assert!(initial.contacts.iter().zip(&changed.contacts).any(|(a,b)| a.force != b.force),
        "fixed geometry still requires fresh damping/friction forces");
    check_prepared_contact_queries(&system, &x, &rates);
    // Prepare without inter-body contact, then query a touching configuration.
    let mut apart = x.clone();
    for (i, base) in system.art.bases.iter().enumerate() {
        apart[base.state + 2] += i as f64 + 1.0;
    }
    assert!(system.art.evaluate(&g(&apart)).contacts.iter().all(|c|c.other.is_none()));
    check_prepared_contact_queries(&system, &apart, &rates);

}

// Promotion gate, not a cache regression: this case fails identically with
// preparation disabled. The 1e-8 forward remainder does not pass the independent
// central-stencil audit at these stiff, overlapping SDF contact configurations.
#[test]
#[ignore = "experimental hybrid SDF contact derivatives fail the independent audit; see runtime-audit.md"]
fn hybrid_stiff_sdf_pair_derivative_promotion_gate() {
    let mut model = empty_model();
    model.links.push(box_link("first", [0.2; 3], 1.0, [0.0, 0.0, 0.3], false));
    model.links.push(box_link("second", [0.2; 3], 1.0, [0.15, 0.0, 0.3], false));
    let mut system = LocalSystem::new(model, Options {flex:false, ..Default::default()});
    for link in &mut system.art.links { link.excluded.clear(); }
    let (x, rates) = system.point();
    let mut moving = x.clone();
    moving[system.art.bases[0].state+7] += 0.4;
    moving[system.art.bases[0].state+8] -= 0.3;
    let mut apart = x.clone();
    apart[system.art.bases[0].state+2] += 1.0;
    let geometry = system.art.evaluate(&system.art.generalized(
        x[..system.art.state_count].to_vec(), rates[..system.art.state_count].to_vec(), &[], vec![]));
    let mut samples = Vec::new();
    for (i, link) in system.art.links.iter().enumerate() {
        for (sample, p) in link.contact.iter().enumerate() {
            let world = geometry.links[i].p + geometry.links[i].r * p;
            for (j, other) in system.art.links.iter().enumerate() {
                if i == j {continue;}
                let local = geometry.links[j].r.transpose() * (world - geometry.links[j].p);
                let sdf = other.sdf.as_ref().unwrap();
                let (phi, normal) = sdf.sample(local);
                let grid: Vec<_> = (0..3).map(|k|(local[k]-sdf.origin[k])/sdf.cell).collect();
                samples.push(serde_json::json!({"link":i,"sample":sample,"other":j,"phi":phi,
                    "grid":grid,"normal":normal.as_slice()}));
            }
        }
    }
    println!("{}",serde_json::json!({"contact_geometry":samples}));
    // Observe actual active-pair changes, rather than inferring them solely
    // from the distance values or the checker's stencil warnings.
    for shift in [-1e-5, -5e-6, -1e-6, 0.0, 1e-6, 5e-6, 1e-5] {
        let mut probe = x[..system.art.state_count].to_vec();
        probe[system.art.bases[0].state+1] += shift;
        let e = system.art.evaluate(&system.art.generalized(
            probe, rates[..system.art.state_count].to_vec(), &[], vec![]));
        let contacts: Vec<_> = e.contacts.iter().map(|c|serde_json::json!({
            "link":c.link,"other":c.other,"penetration":c.penetration,
            "sample":system.art.links[c.link].contact.iter().position(|p|
                e.links[c.link].p + e.links[c.link].r * p == c.point)})).collect();
        println!("{}",serde_json::json!({"y_shift":shift,"contacts":contacts}));
    }
    let mut defaults_pass = true;
    for (name, point) in [("loaded", &x), ("sliding", &moving), ("apart", &apart)] {
        for step in [1e-3, 1e-4, 1e-5, 1e-6, 1e-7, 1e-8] {
            let report = check_jacobian(&system, 0.3, point, &rates,
                &CheckConfig {step, ..CheckConfig::default()}).unwrap();
            println!("{}",serde_json::json!({"point":name,"step":step,"report":report}));
            if step == CheckConfig::default().step { defaults_pass &= report.passed; }
        }
    }
    assert!(defaults_pass, "independent derivative promotion gate failed; inspect stencil sweep above");
}

fn check_prepared_contact_queries(system:&LocalSystem, x:&Vec<f64>, rates:&Vec<f64>) {
    let view = system.view(0.3, x, rates);
    let generalized = |v:&View, r:&[f64]| {
        let angles:Vec<_>=(0..system.offsets.len()-1).map(|p|v.across(p)).collect();
        system.art.generalized(v.states.to_vec(),r[..system.art.state_count].to_vec(),&angles,v.signals_in.to_vec())
    };
    let history = system.art.prepare_contact_history_rates(generalized(&view,rates));
    let prepared = system
        .art
        .prepare_linearization(&view, &rates[..system.art.state_count])
        .unwrap();
    let check = |point: &[f64], rate: &[f64]| {
        let mut expected = vec![0.0; system.n];
        system.residual(0.3, point, rate, &mut expected);
        let mut actual = vec![0.0; system.n];
        let v = system.view(0.3, point, rate);
        let g=generalized(&v,rate);
        assert_eq!(history(&g).iter().map(|x|x.to_bits()).collect::<Vec<_>>(),
            system.art.evaluate(&g).bristle_rates.iter().map(|x|x.to_bits()).collect::<Vec<_>>(),
            "contact-only history must match full evaluation through motion/contact changes");
        let ns = system.art.state_count;
        let width = *system.offsets.last().unwrap();
        let (states, rest) = actual.split_at_mut(ns);
        let (through, signals) = rest.split_at_mut(width);
        let mut ctx = Context::new(
            0.3,
            v.states,
            &rate[..ns],
            &system.offsets,
            &system.rate_map,
            v.across,
            v.across_rates,
            v.signals_in,
            states,
            through,
            &mut signals[..system.art.signal_out_names.len()],
        );
        prepared.residual(&mut ctx);
        assert_eq!(
            expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            "cache must preserve exact residual values"
        );
    };
    check(&x, &rates);
    // Change several rates that enter the residual equations directly but do
    // not enter the articulated force/acceleration evaluation. Complete force
    // reuse must still write the NEW position/modal/bristle rate residuals.
    let mut direct_rates = rates.clone();
    for base in &system.art.bases {
        for i in base.state..base.state+7 { direct_rates[i] += 0.123; }
    }
    for (_, dof) in system.art.dofs() {
        if let Some(i) = dof.q_state { direct_rates[i] -= 0.234; }
    }
    for link in &system.art.links {
        if !link.grounded {
            for i in link.bristle_state..link.bristle_state+3 { direct_rates[i] += 0.345; }
        }
        if let Some(f) = &link.flex {
            for i in f.state..f.state+f.modes { direct_rates[i] -= 0.456; }
        }
    }
    check(&x, &direct_rates);
    let mut base_residual = vec![0.0; system.n];
    let mut changed_residual = vec![0.0; system.n];
    system.residual(0.3, x, rates, &mut base_residual);
    system.residual(0.3, x, &direct_rates, &mut changed_residual);
    assert_ne!(base_residual, changed_residual, "cached forces must not cache the whole residual");
    check(&x, &rates);
    for col in 0..system.n {
        for sign in [-1.0, 1.0] {
            let mut changed = x.clone();
            changed[col] += sign * 1e-6 * (1.0 + x[col].abs());
            check(&changed, &rates);
            let mut rate = rates.clone();
            rate[col] += sign * 1e-6 * (1.0 + rates[col].abs());
            check(&x, &rate);
        }
    }
    // Signed zeros and large changes exercise invalidation beyond an FD-sized
    // neighbourhood. Return to the prepared point after these probes, as after a
    // rejected trial: no query may contaminate a later cache hit.
    for col in 0..system.n {
        for value in [0.0, -0.0] {
            let mut changed = x.clone();
            changed[col] = value;
            check(&changed, &rates);
            let mut rate = rates.clone();
            rate[col] = value;
            check(&x, &rate);
        }
    }
    check(&x, &rates);
    let mut airborne = x.clone();
    airborne[system.art.bases[0].state + 2] += 1.0;
    check(&airborne, &rates);
    airborne[system.art.bases[0].state + 2] -= 2.0;
    check(&airborne, &rates);
    let mut reversed = x.clone();
    for base in &system.art.bases {
        for k in 7..13 {
            reversed[base.state + k] = -3.0 * x[base.state + k];
        }
    }
    check(&reversed, &rates);
}

#[test]
fn constraint_velocity_map_includes_floating_base_and_modal_boundaries() {
    let (system, x, rates) = loaded_fixture();
    let ns = system.art.state_count;
    let angles: Vec<_> = system.offsets[..system.offsets.len()-1].iter().map(|o| x[ns+o]).collect();
    let g = system.art.generalized(x[..ns].to_vec(), rates[..ns].to_vec(), &angles, vec![]);
    let audit = system.art.audit_constraints(&g, &Default::default()).unwrap();
    let declarations = system.art.states();
    for (row, values) in audit.scaled_velocity_matrix.iter().enumerate() {
        let velocity: f64 = values.iter().enumerate().map(|(col, value)| {
            let state = declarations.iter().position(|s| s.name == audit.coordinates[col]).unwrap();
            value * g.states[state] / audit.column_scales[col] * audit.row_scales[row]
        }).sum();
        assert!((velocity-audit.rows[row].velocity).abs()<1e-12,"row {row}");
    }
    assert!(audit.coordinates.iter().any(|n| n.contains("etad")));
    assert!(audit.coordinates.iter().any(|n| n.contains(".wx")));
}

#[test]
fn constraint_only_evaluation_matches_loaded_dynamics_across_motion_and_modes() {
    let (source, _, _) = loaded_fixture();
    for flex in [false, true] {
        for identities in [false, true] {
            let mut system = LocalSystem::new((*source.art.model).clone(), Options {
                flex, structural_loop_identities: identities, ..Options::default()
            });
            system.art.floor_z = 0.3;
            let (x, rates) = system.point();
            let view = system.view(0.3, &x, &rates);
            let angles: Vec<_> = (0..system.offsets.len()-1).map(|p| view.across(p)).collect();
            let base = system.art.generalized(view.states.to_vec(), rates[..system.art.state_count].to_vec(),
                &angles, view.signals_in.to_vec());
            assert!(!system.art.loops.is_empty() && !system.art.transmissions.is_empty());
            for sample in 0..16 {
                let mut g = base.clone();
                for (i, value) in g.states.iter_mut().enumerate() {
                    *value += 0.003 * ((i + sample) as f64).sin();
                }
                for (i, value) in g.rates.iter_mut().enumerate() {
                    *value += 0.12 * ((i + sample) as f64).cos();
                }
                for i in 0..g.q.len() {
                    g.q[i] += 0.4 * ((i + sample) as f64).sin();
                    g.qd[i] += 0.3 * ((i + sample) as f64).cos();
                    g.qdd[i] += 0.7 * ((i + sample) as f64).sin();
                }
                let rows = system.art.evaluate_constraint_rows(&g);
                for contact in [false, true] {
                    let expected = system.art.evaluate_with(&g, contact).loop_rows;
                    assert_eq!(rows.len(), expected.len());
                    for (row, (a, b)) in rows.iter().zip(&expected).enumerate() {
                        assert!(a.is_finite() && b.is_finite());
                        assert_eq!(a.to_bits(), b.to_bits(),
                            "flex={flex}, identities={identities}, sample={sample}, row={row}");
                    }
                }
            }
        }
    }
}

#[test]
fn constraint_state_rows_match_independent_full_residual_stencils() {
    for flex in [false, true] {
        let (source, _, _) = loaded_fixture();
        let mut system = LocalSystem::new((*source.art.model).clone(), Options {
            flex, structural_loop_identities: false, constraint_state_step: 1e-4,
            ..Options::default()
        });
        system.art.floor_z = 0.3;
        let (x, rates) = system.point();
        let mut local = LocalJacobian::default();
        let rows = system.art.state_row_jacobian_at(&system.view(0.3, &x, &rates),
            &rates[..system.art.state_count], &mut local);
        assert!(!rows.is_empty());
        let mut actual = nalgebra::DMatrix::<f64>::zeros(system.art.state_count, system.n);
        for &(output, input, value) in &local.entries {
            let Output::State(row) = output else { panic!("state row required"); };
            let col = match input {
                Input::State(i) => i,
                Input::Across(p,k) => system.art.state_count + system.offsets[p] + k,
                _ => panic!("unexpected input"),
            };
            actual[(row,col)] += value;
        }
        let mut changed = x.clone();
        for col in 0..system.n {
            let h = 1e-4*(1.0+x[col].abs());
            let mut samples = Vec::new();
            for delta in [h,-h,0.5*h,-0.5*h] {
                changed[col] = x[col]+delta;
                let mut residual = vec![0.0;system.n];
                system.residual(0.3,&changed,&rates,&mut residual);
                samples.push(residual);
            }
            changed[col] = x[col];
            for &row in &rows {
                let coarse = (samples[0][row]-samples[1][row])/(2.0*h);
                let fine = (samples[2][row]-samples[3][row])/h;
                let reference = (4.0*fine-coarse)/3.0;
                assert!((actual[(row,col)]-reference).abs()<1e-8,
                    "flex={flex}, row={row}, col={col}: {} vs {reference}",actual[(row,col)]);
            }
        }
        #[cfg(all(feature="parallel",not(target_arch="wasm32")))]
        for workers in [1,2,8,16] {
            let mut parallel = LocalJacobian::default();
            let other = rayon::ThreadPoolBuilder::new().num_threads(workers).build().unwrap().install(||
                system.art.state_row_jacobian_at(&system.view(0.3,&x,&rates),
                    &rates[..system.art.state_count], &mut parallel));
            assert_eq!(rows,other); assert_eq!(local.entries,parallel.entries);
        }
        system.art.constraint_state_step = 0.0;
        let mut disabled = LocalJacobian::default();
        assert!(system.art.state_row_jacobian_at(&system.view(0.3,&x,&rates),
            &rates[..system.art.state_count], &mut disabled).is_empty());
        assert!(disabled.entries.is_empty());
    }
}
