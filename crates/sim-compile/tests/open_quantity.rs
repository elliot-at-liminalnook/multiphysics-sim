//! A quantity defined outside sim-core reaches compiled states and signal lanes.
use sim_compile::Runtime;
use sim_core::definitions::{DefinitionId, Dimension, QuantityDescriptor, QuantityNature};
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, ModelWorld, QuantityKind,
    StateDeclaration, signal_out,
};
use sim_dynamics::Integrator;

const CONCENTRATION: QuantityKind =
    QuantityKind::named("test.diffusion.concentration", 1, "mol/m³");
struct Decay;
impl Behavior for Decay {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("concentration", CONCENTRATION, 1.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.state_rate(0) + ctx.state(0));
        ctx.set_signal(0, ctx.state(0));
    }
}
fn registry(register_quantity: bool) -> BehaviorRegistry {
    let mut registry = BehaviorRegistry::default();
    if register_quantity {
        registry
            .register_quantity(&QuantityDescriptor {
                id: CONCENTRATION.definition_id(),
                label: "Concentration".into(),
                dimension: Dimension::si([-3, 0, 0, 0, 0, 1, 0]).into(),
                canonical_unit: "mol/m³".into(),
                nature: QuantityNature::Linear,
                display_units: vec![],
            })
            .unwrap();
    }
    registry
        .register(BehaviorDescriptor::new(
            "test.decay",
            "Decay",
            vec![signal_out("value", CONCENTRATION)],
            |_| Ok(Box::new(Decay)),
        ))
        .unwrap();
    registry
}
fn model(registry: &BehaviorRegistry) -> ModelWorld {
    let mut model = ModelWorld::default();
    model.part(registry, "sample", "test.decay", []).unwrap();
    model
}
#[test]
fn registered_external_quantity_integrates_and_preserves_channel_metadata() {
    let registry = registry(true);
    let mut runtime =
        Runtime::new(model(&registry), &registry, Integrator::implicit_midpoint()).unwrap();
    runtime.advance(1.0, 0.001).unwrap();
    let behavior = runtime.model.behaviors.keys().next().unwrap();
    let port = runtime.model.ports.keys().next().unwrap();
    let state = runtime.state_id(behavior, "concentration");
    let signal = runtime.signal_id(port);
    for id in [state, signal] {
        let entry = runtime
            .model
            .state
            .iter()
            .find(|(key, _)| *key == id)
            .unwrap()
            .1;
        assert_eq!(
            entry.quantity.definition_id(),
            DefinitionId::new("test.diffusion.concentration", 1)
        );
        assert_eq!(entry.quantity.unit(), "mol/m³");
        // Differential states are endpoint values. The existing midpoint
        // integrator evaluates algebraic signals against the midpoint state;
        // accepted-state observation capture is a separate migration step.
        let expected = if entry.declaration_name.as_deref() == Some("concentration") {
            (-1.0f64).exp()
        } else {
            (-1.0f64).exp() / (1.0 - 0.001 / 2.0)
        };
        assert!(
            (runtime.get(id) - expected).abs() < 1e-6,
            "{} = {}, expected {}",
            entry.name,
            runtime.get(id),
            expected
        );
    }
}
#[test]
fn unregistered_external_signal_quantity_fails_compilation() {
    let registry = registry(false);
    let error = Runtime::new(model(&registry), &registry, Integrator::implicit_midpoint())
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("test.diffusion.concentration"), "{error}");
}
