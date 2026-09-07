use sim_compile::Runtime;
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, LocalJacobian, ModelWorld,
    QuantityKind, StateDeclaration, View,
};
use sim_dynamics::{Integrator, JacobianParts, System};

struct Partial {
    rows: bool,
    rates: bool,
}
impl Behavior for Partial {
    fn states(&self) -> Vec<StateDeclaration> {
        (0..64)
            .map(|i| StateDeclaration::new(format!("x{i}"), QuantityKind::Dimensionless, 0.0))
            .collect()
    }
    fn residual(&self, ctx: &mut Context) {
        for i in 0..64 {
            let x = ctx.state(i);
            let next = ctx.state((i + 1) % 64);
            let value = if i == 2 {
                ctx.state_rate(i)
            } else {
                (1.0 + next * next) * ctx.state_rate(i) + x * x * x - next.sin()
            };
            ctx.set_state_residual(i, value);
        }
    }
    fn rate_jacobian_at(&self, view: &View, _: &[f64], out: &mut LocalJacobian) -> bool {
        if !self.rates {
            return false;
        }
        for i in 0..64 {
            out.state_rate(
                i,
                i,
                if i == 2 {
                    1.0
                } else {
                    1.0 + view.state((i + 1) % 64).powi(2)
                },
            );
        }
        true
    }
    fn state_row_jacobian_at(
        &self,
        view: &View,
        rates: &[f64],
        out: &mut LocalJacobian,
    ) -> Vec<usize> {
        if !self.rows {
            return Vec::new();
        }
        out.state_state(0, 0, 3.0 * view.state(0).powi(2));
        out.state_state(0, 1, 2.0 * view.state(1) * rates[0] - view.state(1).cos());
        // Claimed zero row has no entries. Rate entries must not replace the
        // separately chosen rate path, even if a shared hook emits them.
        out.state_rate(0, 0, 99999.0);
        vec![0, 2]
    }
}
fn runtime(rows: bool, rates: bool) -> Runtime {
    let mut registry = BehaviorRegistry::default();
    registry
        .register(BehaviorDescriptor::new(
            "test.partial",
            "Partial",
            vec![],
            |p| {
                Ok(Box::new(Partial {
                    rows: p.get("rows").copied().unwrap_or(0.0) > 0.5,
                    rates: p.get("rates").copied().unwrap_or(0.0) > 0.5,
                }))
            },
        ))
        .unwrap();
    let mut model = ModelWorld::default();
    model
        .part(
            &registry,
            "test",
            "test.partial",
            [
                ("rows", if rows { 1.0 } else { 0.0 }),
                ("rates", if rates { 1.0 } else { 0.0 }),
            ],
        )
        .unwrap();
    Runtime::new(model, &registry, Integrator::implicit_midpoint()).unwrap()
}
#[test]
fn selected_rows_preserve_other_rows_and_both_rate_paths() {
    let x: Vec<_> = (0..64).map(|i| 0.03 * i as f64 - 0.6).collect();
    let rates: Vec<_> = (0..64).map(|i| (i as f64 * 0.31).cos() * 0.2).collect();
    let eval = |runtime: &Runtime| {
        let mut j = JacobianParts::default();
        assert!(runtime.islands[0].system.jacobian(0.2, &x, &rates, &mut j));
        j
    };
    for exact_rates in [false, true] {
        let reference = eval(&runtime(false, exact_rates));
        let system = runtime(true, exact_rates);
        let candidate = eval(&system);
        assert_eq!(candidate.d_drate, reference.d_drate);
        let other = |j: &JacobianParts| {
            j.d_dx
                .iter()
                .copied()
                .filter(|(row, _, _)| *row != 0 && *row != 2)
                .collect::<Vec<_>>()
        };
        assert_eq!(other(&candidate), other(&reference));
        let (state, _) = candidate.dense(64);
        for col in 0..64 {
            assert_eq!(state[(2, col)], 0.0);
            let expected = match col {
                0 => 3.0 * x[0].powi(2),
                1 => 2.0 * x[1] * rates[0] - x[1].cos(),
                _ => 0.0,
            };
            assert_eq!(state[(0, col)], expected);
        }
        #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
        for workers in [1, 2, 8, 16] {
            let other = rayon::ThreadPoolBuilder::new()
                .num_threads(workers)
                .build()
                .unwrap()
                .install(|| eval(&system));
            assert_eq!(candidate.d_dx, other.d_dx);
            assert_eq!(candidate.d_drate, other.d_drate);
        }
        let check = sim_dynamics::jacobian_check::check_jacobian(
            &system.islands[0].system,
            0.2,
            &x,
            &rates,
            &Default::default(),
        )
        .unwrap();
        assert!(check.passed, "{check:?}");
    }
}

struct Rotor;
impl Behavior for Rotor {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new(
            "speed",
            QuantityKind::Dimensionless,
            0.0,
        )]
    }
    fn provides(&self) -> Vec<sim_core::Provision> {
        vec![sim_core::Provision {
            port: 0,
            lane: 1,
            state: 0,
        }]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.state(0) - ctx.across_derivative(0, 0));
        ctx.add_through(0, ctx.state_rate(0) + 0.4 * ctx.state(0));
    }
}
struct Observer {
    partial: bool,
}
impl Behavior for Observer {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new(
            "filter",
            QuantityKind::Dimensionless,
            0.0,
        )]
    }
    fn residual(&self, ctx: &mut Context) {
        let value = ctx.state_rate(0) + ctx.state(0).powi(2) + ctx.across(0) * ctx.across_rate(0);
        ctx.set_state_residual(0, value);
        ctx.add_through(0, 0.2 * ctx.state(0));
    }
    fn state_row_jacobian_at(&self, view: &View, _: &[f64], out: &mut LocalJacobian) -> Vec<usize> {
        if !self.partial {
            return Vec::new();
        }
        out.state_state(0, 0, 2.0 * view.state(0));
        out.set(
            sim_core::Output::State(0),
            sim_core::Input::Across(0, 0),
            view.across_rate(0),
        );
        out.set(
            sim_core::Output::State(0),
            sim_core::Input::AcrossRate(0, 0),
            view.across(0),
        );
        vec![0]
    }
}
#[test]
fn supplied_rows_follow_rate_providers_and_preserve_shared_torque_contributions() {
    let build = |partial| {
        let mut registry = BehaviorRegistry::default();
        registry
            .register(BehaviorDescriptor::new(
                "test.rotor",
                "Rotor",
                vec![sim_core::acausal(
                    "pin",
                    sim_core::ConnectorKind::Rotational,
                )],
                |_| Ok(Box::new(Rotor)),
            ))
            .unwrap();
        registry
            .register(BehaviorDescriptor::new(
                "test.observer",
                "Observer",
                vec![sim_core::acausal(
                    "pin",
                    sim_core::ConnectorKind::Rotational,
                )],
                |p| {
                    Ok(Box::new(Observer {
                        partial: p.get("partial").copied().unwrap_or(0.0) > 0.5,
                    }))
                },
            ))
            .unwrap();
        let mut model = ModelWorld::default();
        let a = model.part(&registry, "rotor", "test.rotor", []).unwrap();
        let b = model
            .part(
                &registry,
                "observer",
                "test.observer",
                [("partial", if partial { 1.0 } else { 0.0 })],
            )
            .unwrap();
        model.connect([a.port("pin"), b.port("pin")]);
        (
            Runtime::new(model, &registry, Integrator::implicit_midpoint()).unwrap(),
            b.behavior,
        )
    };
    let (reference, _) = build(false);
    let (candidate, observer) = build(true);
    let system = &candidate.islands[0].system;
    let n = system.dimension();
    let x: Vec<_> = (0..n).map(|i| 0.3 + 0.1 * i as f64).collect();
    let rates: Vec<_> = (0..n).map(|i| 0.2 - 0.03 * i as f64).collect();
    let mut a = JacobianParts::default();
    let mut b = JacobianParts::default();
    reference.islands[0]
        .system
        .jacobian(0.0, &x, &rates, &mut a);
    system.jacobian(0.0, &x, &rates, &mut b);
    assert_eq!(a.d_drate, b.d_drate);
    let row = system.reduced_of[system.state_index(observer, "filter").unwrap()].unwrap();
    assert_eq!(
        a.d_dx
            .iter()
            .filter(|(r, _, _)| *r != row)
            .collect::<Vec<_>>(),
        b.d_dx
            .iter()
            .filter(|(r, _, _)| *r != row)
            .collect::<Vec<_>>()
    );
    let check =
        sim_dynamics::jacobian_check::check_jacobian(system, 0.0, &x, &rates, &Default::default())
            .unwrap();
    assert!(check.passed, "{check:?}");
}

struct Complete;
impl Behavior for Complete {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("x", QuantityKind::Dimensionless, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.state(0) - ctx.state_rate(0));
    }
    fn jacobian_at(&self, _: &View, _: &[f64], out: &mut LocalJacobian) -> bool {
        out.state_state(0, 0, 1.0);
        out.state_rate(0, 0, -1.0);
        true
    }
    fn state_row_jacobian_at(&self, _: &View, _: &[f64], _: &mut LocalJacobian) -> Vec<usize> {
        panic!("complete Jacobian must take precedence")
    }
    fn rate_jacobian_at(&self, _: &View, _: &[f64], _: &mut LocalJacobian) -> bool {
        panic!("complete Jacobian must take precedence")
    }
}
#[test]
fn complete_jacobian_takes_precedence_over_partial_hooks() {
    let mut registry = BehaviorRegistry::default();
    registry
        .register(BehaviorDescriptor::new(
            "test.complete",
            "Complete",
            vec![],
            |_| Ok(Box::new(Complete)),
        ))
        .unwrap();
    let mut model = ModelWorld::default();
    model
        .part(&registry, "complete", "test.complete", [])
        .unwrap();
    let runtime = Runtime::new(model, &registry, Integrator::implicit_midpoint()).unwrap();
    let mut j = JacobianParts::default();
    runtime.islands[0]
        .system
        .jacobian(0.0, &[0.2], &[0.3], &mut j);
    assert_eq!(j.d_dx, vec![(0, 0, 1.0)]);
    assert_eq!(j.d_drate, vec![(0, 0, -1.0)]);
}
