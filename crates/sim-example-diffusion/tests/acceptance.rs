use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld};
use sim_dynamics::Integrator;
use sim_example_diffusion::*;
fn registry() -> BehaviorRegistry {
    let mut r = BehaviorRegistry::default();
    register(&mut r).unwrap();
    r
}
fn relaxation_error(h: f64) -> f64 {
    let r = registry();
    let model = relaxation(&r).unwrap();
    let port = model
        .ports
        .iter()
        .find(|(_, p)| p.name == "species" && model.behaviors[p.owner].kind.0 == STORAGE)
        .unwrap()
        .0;
    let mut runtime = Runtime::new(model, &r, Integrator::implicit_midpoint()).unwrap();
    runtime.advance(4., h).unwrap();
    let actual = runtime.get(runtime.across_lane_id(port, 0));
    let exact = 1. + 2. * (-1.0_f64).exp();
    (actual - exact).abs()
}
#[test]
fn analytic_relaxation_and_second_order_refinement() {
    let coarse = relaxation_error(0.2);
    let fine = relaxation_error(0.1);
    eprintln!("diffusion relaxation errors h=0.2: {coarse:e}; h=0.1: {fine:e}");
    assert!(coarse < 2e-4, "{coarse}");
    assert!(
        fine < coarse * 0.27 && fine > coarse * 0.23,
        "expected second-order refinement"
    );
    assert!(relaxation_error(0.01) < 4e-7);
}
#[test]
fn closed_system_conserves_amount_throughout_relaxation() {
    let r = registry();
    let mut model = ModelWorld::default();
    let a = model
        .part(
            &r,
            "A",
            STORAGE,
            [("volume", 2.), ("initial.concentration", 3.)],
        )
        .unwrap();
    let b = model
        .part(
            &r,
            "B",
            STORAGE,
            [("volume", 5.), ("initial.concentration", 0.)],
        )
        .unwrap();
    let path = model
        .part(&r, "path", CONDUCTANCE, [("conductance", 0.5)])
        .unwrap();
    model.connect([a.port("species"), path.port("a")]);
    model.connect([b.port("species"), path.port("b")]);
    let mut runtime = Runtime::new(model, &r, Integrator::implicit_midpoint()).unwrap();
    let a = runtime.across_lane_id(a.port("species"), 0);
    let b = runtime.across_lane_id(b.port("species"), 0);
    for i in 1..=100 {
        runtime.advance(0.1, 0.01).unwrap();
        let (ca, cb) = (runtime.get(a), runtime.get(b));
        assert!((2. * ca + 5. * cb - 6.).abs() < 1e-9);
        let exact_difference = 3. * (-0.35 * i as f64 * 0.1).exp();
        assert!((ca - cb - exact_difference).abs() < 2e-6);
        assert!(ca >= cb && cb >= 0.);
    }
}
#[test]
fn unknown_domain_roundtrips_and_is_fully_inspectable_by_generic_consumers() {
    let r = registry();
    let model = relaxation(&r).unwrap();
    let bytes = serde_json::to_vec(&model).unwrap();
    let decoded: ModelWorld = serde_json::from_slice(&bytes).unwrap();
    let compiled = sim_compile::compile(&decoded, &r).unwrap();
    assert_eq!(
        compiled
            .definitions
            .connector_by_id(&SPECIES.definition_id())
            .unwrap()
            .energy,
        sim_core::definitions::PortEnergy::Unavailable
    );
    let capture =
        sim_inspect::model::describe(&decoded, &r, "diffusion acceptance", 1, &Default::default())
            .unwrap();
    capture.description.validate().unwrap();
    assert_eq!(capture.description.components.len(), 3);
    assert_eq!(capture.description.nets.len(), 2);
    assert!(
        capture
            .description
            .definitions
            .connectors
            .iter()
            .any(|d| d.id == SPECIES.definition_id())
    );
    let catalogue = sim_script::catalogue(&r);
    let storage = catalogue
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["type"] == STORAGE)
        .unwrap();
    assert_eq!(storage["ports"][0]["lanes"][0]["across_unit"], "mol/m³");
    assert!(
        storage["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "initial.concentration" && p["unit"] == "mol/m³")
    );
    let layout = sim_diagram::route(
        &capture.description,
        &sim_diagram::initial_state(&capture.description),
    );
    assert!(layout.unrouted.is_empty());
    assert_eq!(layout.nets.len(), 2);
    assert_eq!(layout.ports.len(), 4);
}
#[test]
fn semantic_connector_identity_is_required_even_for_identical_lane_shapes() {
    use sim_core::definitions::{ConnectorDefinition, DefinitionId};
    use sim_core::{BehaviorDescriptor, ConnectorKind, acausal};
    let mut r = registry();
    let mut other = Species.descriptor();
    other.id = DefinitionId::new("example.other.species", 1);
    r.register_connector(&other).unwrap();
    r.register(BehaviorDescriptor::new(
        "other",
        "Other species",
        vec![acausal("species", ConnectorKind::from_descriptor(&other))],
        |_| Ok(Box::new(Storage { volume: 1. })),
    ))
    .unwrap();
    let mut model = ModelWorld::default();
    let a = model.part(&r, "A", STORAGE, [("volume", 1.)]).unwrap();
    let b = model.part(&r, "B", "other", []).unwrap();
    model.connect([a.port("species"), b.port("species")]);
    assert!(matches!(
        sim_compile::compile(&model, &r),
        Err(sim_compile::CompileError::IncompatibleConnection { .. })
    ));
}

#[test]
fn external_domain_flow_bindings_retain_units_and_conservation() {
    let r = registry();
    let model = relaxation(&r).unwrap();
    let ports = model
        .ports
        .iter()
        .filter(|(_, p)| model.behaviors[p.owner].kind.0 == CONDUCTANCE)
        .map(|(id, p)| (p.name.clone(), id))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut runtime = Runtime::new(model, &r, Integrator::implicit_midpoint()).unwrap();
    let a = runtime.bind_through(ports["a"], 0).unwrap();
    let b = runtime.bind_through(ports["b"], 0).unwrap();
    assert_eq!(a.unit(), "mol/s");
    assert_eq!(
        a.quantity(),
        &sim_core::QuantityKind::MolarFlow.definition_id()
    );
    runtime.set_observation_capture(true);
    runtime.advance(0.2, 0.2).unwrap();
    let a = runtime.read_flow(&a).unwrap();
    let b = runtime.read_flow(&b).unwrap();
    assert_eq!(a.evaluation_time, 0.1);
    assert_eq!(a.step_end, 0.2);
    assert!((a.value - 1. / 1.025).abs() < 1e-9);
    assert!((a.value + b.value).abs() < 1e-12);
}

#[test]
fn external_domain_subscription_preserves_concentration_and_flow_semantics() {
    use sim_inspect::{
        ObservationLocation, SampleValue,
        runtime::{FrameStamp, RuntimeInspection},
    };
    let registry = registry();
    let mut runtime = Runtime::new(
        relaxation(&registry).unwrap(),
        &registry,
        Integrator::implicit_midpoint(),
    )
    .unwrap();
    let inspection = RuntimeInspection::new(
        &runtime,
        &registry,
        "diffusion subscription",
        1,
        &Default::default(),
    )
    .unwrap();
    let subscribed = inspection
        .subscribe(
            inspection
                .description
                .observables
                .keys()
                .map(String::as_str),
        )
        .unwrap();
    runtime.set_observation_capture(true);
    runtime.advance(0.2, 0.2).unwrap();
    let frame = subscribed
        .sample(
            &runtime,
            FrameStamp {
                run_id: "diffusion",
                generation: 0,
                sequence: 1,
                step: 1,
            },
        )
        .unwrap();
    let mut across_count = 0;
    let mut flow_count = 0;
    for (id, descriptor) in &inspection.description.observables {
        match &descriptor.location {
            ObservationLocation::Across { port, .. } => {
                across_count += 1;
                assert_eq!(descriptor.quantity, CONCENTRATION.definition_id());
                let component = &inspection.description.components
                    [&inspection.description.ports[port].component];
                if component.component_type == STORAGE {
                    let SampleValue::Committed { value, sample_time } = frame.values[id] else {
                        panic!("missing endpoint concentration")
                    };
                    assert_eq!(sample_time, 0.2);
                    assert!((value - (1. + 2. * 0.975 / 1.025)).abs() < 1e-9);
                }
            }
            ObservationLocation::Through { .. } => {
                flow_count += 1;
                assert_eq!(
                    descriptor.quantity,
                    sim_core::QuantityKind::MolarFlow.definition_id()
                );
                let SampleValue::AcceptedStage {
                    value,
                    sample_time,
                    step_start,
                    step_end,
                } = frame.values[id]
                else {
                    panic!("missing stage flow")
                };
                assert_eq!((sample_time, step_start, step_end), (0.1, 0., 0.2));
                assert!((value.abs() - 1. / 1.025).abs() < 1e-9);
            }
            _ => {}
        }
    }
    assert_eq!((across_count, flow_count), (4, 4));
    for net in inspection.description.nets.values() {
        let sum: f64 = inspection
            .description
            .observables
            .iter()
            .filter_map(|(id, descriptor)| {
                let ObservationLocation::Through { port, .. } = &descriptor.location else {
                    return None;
                };
                if !net.ports.contains(port) {
                    return None;
                }
                let SampleValue::AcceptedStage { value, .. } = frame.values[id] else {
                    unreachable!()
                };
                Some(value)
            })
            .sum();
        assert!(sum.abs() < 1e-9);
    }
}
