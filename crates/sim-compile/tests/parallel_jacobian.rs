#![cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
use sim_compile::Runtime;
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, ModelWorld, QuantityKind,
    StateDeclaration,
};
use sim_dynamics::{Integrator, JacobianParts, System};

struct CoupledStates;
impl Behavior for CoupledStates {
    fn states(&self) -> Vec<StateDeclaration> {
        (0..64)
            .map(|i| StateDeclaration::new(format!("x{i}"), QuantityKind::Dimensionless, 0.0))
            .collect()
    }
    fn residual(&self, ctx: &mut Context) {
        for i in 0..64 {
            let x = ctx.state(i);
            let neighbour = ctx.state((i + 1) % 64);
            ctx.set_state_residual(
                i,
                (1.0 + neighbour * neighbour) * ctx.state_rate(i) + x * x * x - neighbour.sin(),
            );
        }
    }
}

#[test]
fn large_component_columns_preserve_triplet_order_and_values_across_worker_counts() {
    let mut registry = BehaviorRegistry::default();
    registry
        .register(BehaviorDescriptor::new(
            "test.coupled",
            "Coupled states",
            vec![],
            |_| Ok(Box::new(CoupledStates)),
        ))
        .unwrap();
    let mut model = ModelWorld::default();
    model
        .part(&registry, "coupled", "test.coupled", [])
        .unwrap();
    let runtime = Runtime::new(model, &registry, Integrator::implicit_midpoint()).unwrap();
    let system = &runtime.islands[0].system;
    assert_eq!(system.dimension(), 64);
    let x: Vec<_> = (0..64).map(|i| 0.03 * i as f64 - 0.6).collect();
    let rate: Vec<_> = (0..64).map(|i| (i as f64 * 0.31).cos() * 0.2).collect();
    let evaluate = |workers| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap()
            .install(|| {
                assert_eq!(sim_compile::derivative_worker_capacity(), workers);
                let mut result = JacobianParts::default();
                assert!(system.jacobian(0.2, &x, &rate, &mut result));
                result
            })
    };
    let serial = evaluate(1);
    for workers in [2, 4, 8, 16] {
        let parallel = evaluate(workers);
        assert_eq!(
            serial.d_dx, parallel.d_dx,
            "state columns with {workers} workers"
        );
        assert_eq!(
            serial.d_drate, parallel.d_drate,
            "rate columns with {workers} workers"
        );
    }
    let check =
        sim_dynamics::jacobian_check::check_jacobian(system, 0.2, &x, &rate, &Default::default())
            .unwrap();
    assert!(check.passed, "{check:#?}");
}
