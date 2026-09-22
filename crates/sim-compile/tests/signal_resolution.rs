//! Legacy wildcard terminals must not erase or bypass concrete signal types.
use sim_compile::{CompileError, Runtime};
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, ModelWorld, QuantityKind,
    StateDeclaration, signal_in, signal_out,
};
use sim_dynamics::Integrator;
struct Source;
impl Behavior for Source {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![]
    }
    fn residual(&self, c: &mut Context) {
        c.set_signal(0, 0.25);
    }
}
struct Sink;
impl Behavior for Sink {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![]
    }
    fn residual(&self, _: &mut Context) {}
}
fn registry() -> BehaviorRegistry {
    let mut r = BehaviorRegistry::default();
    r.register(BehaviorDescriptor::new(
        "source",
        "Source",
        vec![signal_out("out", QuantityKind::Dimensionless)],
        |_| Ok(Box::new(Source)),
    ))
    .unwrap();
    r.register(BehaviorDescriptor::new(
        "angle",
        "Angle",
        vec![signal_in("in", QuantityKind::Angle)],
        |_| Ok(Box::new(Sink)),
    ))
    .unwrap();
    r.register(BehaviorDescriptor::new(
        "voltage",
        "Voltage",
        vec![signal_in("in", QuantityKind::Voltage)],
        |_| Ok(Box::new(Sink)),
    ))
    .unwrap();
    r
}
#[test]
fn connected_concrete_type_wins_regardless_of_terminal_order() {
    let r = registry();
    for reverse in [false, true] {
        let mut model = ModelWorld::default();
        let source = model.part(&r, "source", "source", []).unwrap();
        let sink = model.part(&r, "angle", "angle", []).unwrap();
        let mut ports = [source.port("out"), sink.port("in")];
        if reverse {
            ports.reverse();
        }
        model.connect(ports);
        let runtime = Runtime::new(model, &r, Integrator::implicit_midpoint()).unwrap();
        for port in ports {
            let binding = runtime.bind_signal(port).unwrap();
            assert_eq!(binding.quantity(), &QuantityKind::Angle.definition_id());
            assert_eq!(binding.unit(), "rad");
        }
    }
}
#[test]
fn wildcard_cannot_join_incompatible_typed_consumers_in_any_order() {
    let r = registry();
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut model = ModelWorld::default();
        let source = model.part(&r, "source", "source", []).unwrap();
        let angle = model.part(&r, "angle", "angle", []).unwrap();
        let voltage = model.part(&r, "voltage", "voltage", []).unwrap();
        let ports = [source.port("out"), angle.port("in"), voltage.port("in")];
        model.connect(order.map(|i| ports[i]));
        assert!(matches!(
            sim_compile::compile(&model, &r),
            Err(CompileError::IncompatibleConnection { .. })
        ));
    }
}
