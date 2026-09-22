#![cfg(feature = "runtime")]
use sim_compile::Runtime;
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, ModelWorld, QuantityKind,
    StateDeclaration, signal_out,
};
use sim_dynamics::Integrator;
use sim_inspect::{
    runtime::{FrameStamp, RuntimeInspection},
    *,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
static CALLS: AtomicUsize = AtomicUsize::new(0);
struct Decay;
impl Behavior for Decay {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("charge", QuantityKind::Voltage, 1.)]
    }
    fn residual(&self, c: &mut Context) {
        CALLS.fetch_add(1, Ordering::Relaxed);
        c.set_state_residual(0, c.state_rate(0) + c.state(0));
        c.set_signal(0, c.state(0));
    }
}
fn decay() -> (Runtime, BehaviorRegistry) {
    let mut registry = BehaviorRegistry::default();
    registry
        .register(BehaviorDescriptor::new(
            "decay",
            "Decay",
            vec![signal_out("voltage", QuantityKind::Voltage)],
            |_| Ok(Box::new(Decay)),
        ))
        .unwrap();
    let mut model = ModelWorld::default();
    model.part(&registry, "Charge", "decay", []).unwrap();
    (
        Runtime::new(model, &registry, Integrator::implicit_midpoint()).unwrap(),
        registry,
    )
}
fn stamp(sequence: u64) -> FrameStamp<'static> {
    FrameStamp {
        run_id: "test-run",
        generation: 1,
        sequence,
        step: sequence,
    }
}
#[test]
fn frame_distinguishes_endpoint_state_from_midpoint_signal_and_reads_no_equations() {
    let (mut runtime, registry) = decay();
    let inspected =
        RuntimeInspection::new(&runtime, &registry, "source", 1, &Default::default()).unwrap();
    assert!(inspected.subscribe(["missing"]).is_err());
    let subscription = inspected
        .subscribe(inspected.description.observables.keys().map(String::as_str))
        .unwrap();
    let initial = subscription.sample(&runtime, stamp(0)).unwrap();
    assert!(
        initial
            .values
            .values()
            .any(|v| matches!(v, SampleValue::Unavailable { .. }))
    );
    assert!(initial.values.values().any(|v| matches!(
        v,
        SampleValue::Committed {
            value: 1.,
            sample_time: 0.
        }
    )));
    runtime.set_observation_capture(true);
    runtime.advance(0.1, 0.1).unwrap();
    let calls = CALLS.load(Ordering::Relaxed);
    let frame = subscription.sample(&runtime, stamp(1)).unwrap();
    assert_eq!(frame.version, 2);
    for (id, sample) in &frame.values {
        match (&inspected.description.observables[id].location, sample) {
            (ObservationLocation::State { .. }, SampleValue::Committed { value, sample_time }) => {
                assert!((value - 0.95 / 1.05).abs() < 1e-10);
                assert_eq!(*sample_time, 0.1);
            }
            (
                ObservationLocation::Signal { .. },
                SampleValue::AcceptedStage {
                    value,
                    sample_time,
                    step_start,
                    step_end,
                },
            ) => {
                assert!((value - 1. / 1.05).abs() < 1e-10);
                assert_eq!((*sample_time, *step_start, *step_end), (0.05, 0., 0.1));
            }
            pair => panic!("unexpected sample {pair:?}"),
        }
    }
    for _ in 0..100 {
        assert_eq!(subscription.sample(&runtime, stamp(1)).unwrap(), frame);
    }
    assert_eq!(CALLS.load(Ordering::Relaxed), calls);
    let bytes = serde_json::to_vec(&frame).unwrap();
    let decoded: SampleFrame = serde_json::from_slice(&bytes).unwrap();
    decoded.validate(&inspected.description).unwrap();
    assert_eq!(decoded, frame);
    let (other, _) = decay();
    assert!(
        subscription
            .sample(&other, stamp(2))
            .unwrap_err()
            .0
            .contains("another runtime")
    );
    let mut gate = FrameGate::new("test-run".into(), 1);
    gate.accept(&inspected.description, &frame).unwrap();
    assert!(gate.accept(&inspected.description, &frame).is_err());
}

#[test]
fn thermal_subscription_exposes_balanced_flows_with_registered_units() {
    let mut registry = BehaviorRegistry::default();
    sim_domain_thermal::register(&mut registry).unwrap();
    let mut model = ModelWorld::default();
    let tank = model
        .part(
            &registry,
            "Storage",
            sim_domain_thermal::CAPACITANCE,
            [("heat_capacity", 2.), ("initial.temperature", 313.15)],
        )
        .unwrap();
    let path = model
        .part(
            &registry,
            "Conduction",
            sim_domain_thermal::CONDUCTANCE,
            [("conductance", 0.5)],
        )
        .unwrap();
    let ambient = model
        .part(
            &registry,
            "Ambient",
            sim_domain_thermal::AMBIENT,
            [("temperature", 293.15)],
        )
        .unwrap();
    model.connect([tank.port("node"), path.port("a")]);
    model.connect([path.port("b"), ambient.port("node")]);
    let mut runtime = Runtime::new(model, &registry, Integrator::implicit_midpoint()).unwrap();
    let inspected = RuntimeInspection::new(
        &runtime,
        &registry,
        "thermal-source",
        1,
        &Default::default(),
    )
    .unwrap();
    let subscribed = inspected
        .subscribe(inspected.description.observables.keys().map(String::as_str))
        .unwrap();
    runtime.set_observation_capture(true);
    runtime.advance(0.1, 0.1).unwrap();
    let frame = subscribed.sample(&runtime, stamp(1)).unwrap();
    for net in inspected.description.nets.values() {
        let mut sum = 0.;
        for port in &net.ports {
            let (id, descriptor) = inspected
                .description
                .observables
                .iter()
                .find(|(_, d)| {
                    matches!(&d.location,
                ObservationLocation::Through { port: p, .. } if p == port)
                })
                .unwrap();
            assert_eq!(descriptor.quantity, QuantityKind::HeatFlow.definition_id());
            let SampleValue::AcceptedStage {
                value, sample_time, ..
            } = frame.values[id]
            else {
                panic!("flow unavailable")
            };
            assert_eq!(sample_time, 0.05);
            sum += value;
        }
        assert!(sum.abs() < 1e-8, "node imbalance {sum}");
    }
}

#[test]
fn failed_runtime_commit_keeps_old_endpoint_and_withholds_new_algebraic_values() {
    struct Failing {
        failed: AtomicBool,
    }
    impl Behavior for Failing {
        fn states(&self) -> Vec<StateDeclaration> {
            vec![StateDeclaration::new("x", QuantityKind::Voltage, 1.)]
        }
        fn residual(&self, c: &mut Context) {
            if c.time > 0.01 {
                self.failed.store(true, Ordering::Relaxed);
            }
            c.set_state_residual(0, c.state_rate(0) + c.state(0));
            c.set_signal(0, c.state(0));
            c.add_through(0, c.across_rate(0));
        }
        fn failure(&self) -> Option<String> {
            self.failed
                .load(Ordering::Relaxed)
                .then(|| "deliberate failure".into())
        }
    }
    let mut registry = BehaviorRegistry::default();
    registry
        .register(BehaviorDescriptor::new(
            "failing",
            "Failing",
            vec![
                signal_out("voltage", QuantityKind::Voltage),
                sim_core::acausal("node", sim_core::ConnectorKind::Thermal),
            ],
            |_| {
                Ok(Box::new(Failing {
                    failed: AtomicBool::new(false),
                }))
            },
        ))
        .unwrap();
    let mut model = ModelWorld::default();
    let failing = model
        .part(
            &registry,
            "failing",
            "failing",
            [("initial.temperature", 300.)],
        )
        .unwrap();
    sim_domain_thermal::register(&mut registry).unwrap();
    let storage = model
        .part(
            &registry,
            "storage",
            sim_domain_thermal::CAPACITANCE,
            [("heat_capacity", 1.), ("initial.temperature", 300.)],
        )
        .unwrap();
    model.connect([failing.port("node"), storage.port("node")]);
    let mut runtime = Runtime::new(model, &registry, Integrator::implicit_midpoint()).unwrap();
    let inspected = RuntimeInspection::new(
        &runtime,
        &registry,
        "failure-source",
        1,
        &Default::default(),
    )
    .unwrap();
    let subscribed = inspected
        .subscribe(inspected.description.observables.keys().map(String::as_str))
        .unwrap();
    runtime.set_observation_capture(true);
    assert!(runtime.advance(0.1, 0.1).is_err());
    let frame = subscribed.sample(&runtime, stamp(1)).unwrap();
    for (id, sample) in &frame.values {
        match inspected.description.observables[id].location {
            ObservationLocation::State { .. } => assert_eq!(
                *sample,
                SampleValue::Committed {
                    value: 1.,
                    sample_time: 0.
                }
            ),
            ObservationLocation::Signal { .. } | ObservationLocation::Through { .. } => {
                assert!(matches!(sample, SampleValue::Unavailable { .. }))
            }
            ObservationLocation::Across { .. } => assert_eq!(
                *sample,
                SampleValue::Committed {
                    value: 300.,
                    sample_time: 0.
                }
            ),
            _ => unreachable!(),
        }
    }
}
