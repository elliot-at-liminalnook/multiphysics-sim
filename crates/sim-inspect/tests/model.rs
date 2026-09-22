use sim_core::{BehaviorRegistry, ModelWorld};
use sim_domain_thermal as thermal;
use sim_inspect::{model::*, *};

fn setup() -> (BehaviorRegistry, ModelWorld, sim_core::Instance) {
    let mut registry = BehaviorRegistry::default();
    thermal::register(&mut registry).unwrap();
    let mut model = ModelWorld::default();
    let a = model
        .part(
            &registry,
            "storage",
            thermal::CAPACITANCE,
            [("heat_capacity", 2.), ("initial.temperature", 313.15)],
        )
        .unwrap();
    let b = model
        .part(
            &registry,
            "conduction",
            thermal::CONDUCTANCE,
            [("conductance", 0.5)],
        )
        .unwrap();
    let c = model
        .part(
            &registry,
            "ambient",
            thermal::AMBIENT,
            [("temperature", 293.15)],
        )
        .unwrap();
    let extra = model
        .part(
            &registry,
            "storage",
            thermal::CAPACITANCE,
            [("heat_capacity", 1.)],
        )
        .unwrap();
    model.connect([a.port("node"), b.port("a"), extra.port("node")]);
    model.connect([b.port("b"), c.port("node")]);
    (registry, model, a)
}

#[test]
fn general_model_exports_branched_topology_and_distinct_duplicate_labels() {
    let (registry, model, _) = setup();
    let inspection = describe(
        &model,
        &registry,
        "source-a",
        1,
        &IdentityBindings::default(),
    )
    .unwrap();
    assert_eq!(inspection.description.components.len(), 4);
    assert!(
        inspection
            .description
            .nets
            .values()
            .any(|n| n.ports.len() == 3)
    );
    assert_eq!(
        inspection
            .description
            .components
            .values()
            .filter(|c| c.label == "storage")
            .count(),
        2
    );
    assert!(
        inspection
            .description
            .components
            .values()
            .all(|c| !c.persistent_identity)
    );
    assert!(
        inspection
            .description
            .observables
            .values()
            .filter(|o| matches!(o.location, ObservationLocation::Through { .. }))
            .all(|o| matches!(o.availability, Availability::Unavailable { .. }))
    );
    let again = describe(
        &model,
        &registry,
        "source-a",
        1,
        &IdentityBindings::default(),
    )
    .unwrap();
    assert_eq!(inspection.description.id, again.description.id);
}

#[test]
fn authoring_ids_survive_rename_and_rebuild_while_layout_binding_changes() {
    let (registry, mut model, a) = setup();
    let mut ids = IdentityBindings::default();
    ids.components.insert(
        a.behavior,
        ComponentIdentity {
            persistent: true,
            id: "cad-uuid-a".into(),
            source: None,
            cad: Some(CadReference {
                artifact_hash: "cad-v1".into(),
                body_id: "body-a".into(),
            }),
            group: None,
        },
    );
    let first = describe(&model, &registry, "source-a", 1, &ids).unwrap();
    let object = model.behaviors[a.behavior].object;
    model.objects[object].name = "Renamed housing".into();
    let second = describe(&model, &registry, "source-b", 2, &ids).unwrap();
    assert_eq!(
        first.components[&a.behavior],
        second.components[&a.behavior]
    );
    assert_eq!(first.ports[&a.port("node")], second.ports[&a.port("node")]);
    assert_eq!(
        second.description.components["cad-uuid-a"].label,
        "Renamed housing"
    );
    assert_ne!(first.description.id, second.description.id);
    assert!(
        DiagramState::new(&first.description)
            .validate(&second.description)
            .is_err()
    );
    let (registry, rebuilt, a2) = setup();
    let rebuilt_ids = IdentityBindings {
        components: [(a2.behavior, ids.components[&a.behavior].clone())].into(),
        ..Default::default()
    };
    let third = describe(&rebuilt, &registry, "source-a", 1, &rebuilt_ids).unwrap();
    assert_eq!(
        first.components[&a.behavior],
        third.components[&a2.behavior]
    );
}

#[test]
fn compiled_model_inspection_does_not_change_committed_state() {
    let (registry, model, _) = setup();
    let mut runtime = sim_compile::Runtime::new(
        model,
        &registry,
        sim_dynamics::Integrator::implicit_midpoint(),
    )
    .unwrap();
    runtime.advance(0.1, 0.001).unwrap();
    let before = serde_json::to_string(&runtime.model).unwrap();
    let inspection = describe(
        &runtime.model,
        &registry,
        "captured",
        1,
        &IdentityBindings::default(),
    )
    .unwrap();
    assert!(
        inspection
            .description
            .observables
            .values()
            .any(|o| matches!(o.location, ObservationLocation::State { .. }))
    );
    assert_eq!(before, serde_json::to_string(&runtime.model).unwrap());
    assert_eq!(runtime.time, 0.1);
}

#[test]
fn unavailable_component_and_duplicate_identity_are_explicit() {
    let (registry, mut model, a) = setup();
    model.behaviors[a.behavior].kind = "missing.component".into();
    let inspection = describe(
        &model,
        &registry,
        "captured",
        1,
        &IdentityBindings::default(),
    )
    .unwrap();
    assert!(
        inspection
            .description
            .diagnostics
            .iter()
            .any(|d| d.code == "unregistered_component")
    );
    let mut ids = IdentityBindings::default();
    for (key, _) in &model.behaviors {
        ids.components.insert(
            key,
            ComponentIdentity {
                persistent: true,
                id: "duplicate".into(),
                source: None,
                cad: None,
                group: None,
            },
        );
    }
    assert!(
        describe(&model, &registry, "captured", 1, &ids)
            .unwrap_err()
            .to_string()
            .contains("duplicate component")
    );
}

#[test]
fn compiled_state_identity_survives_in_place_rename_and_independent_rebuild() {
    let build = |rename: bool| {
        let (registry, mut model, _) = setup();
        let (id, object) = model
            .behaviors
            .iter()
            .find(|(_, b)| b.kind.0 == thermal::AMBIENT)
            .map(|(id, b)| (id, b.object))
            .unwrap();
        if rename {
            model.objects[object].name = "Room boundary".into();
        }
        let runtime = sim_compile::Runtime::new(
            model,
            &registry,
            sim_dynamics::Integrator::implicit_midpoint(),
        )
        .unwrap();
        let mut identities = IdentityBindings::default();
        identities.components.insert(
            id,
            ComponentIdentity {
                id: "cad/ambient".into(),
                persistent: true,
                source: None,
                cad: None,
                group: None,
            },
        );
        (registry, runtime, identities, object)
    };
    let (registry, mut runtime, ids, object) = build(false);
    let state_ids = |d: ModelInspection| {
        d.description.observables.into_values().filter(|o| matches!(&o.location, ObservationLocation::State { component, .. } if component == "cad/ambient")).map(|o| o.id).collect::<Vec<_>>()
    };
    let before = state_ids(describe(&runtime.model, &registry, "capture", 1, &ids).unwrap());
    assert!(!before.is_empty());
    runtime.model.objects[object].name = "Room boundary".into();
    assert_eq!(
        before,
        state_ids(describe(&runtime.model, &registry, "capture2", 2, &ids).unwrap())
    );
    let (registry, rebuilt, ids, _) = build(true);
    assert_eq!(
        before,
        state_ids(describe(&rebuilt.model, &registry, "capture2", 2, &ids).unwrap())
    );
}

#[test]
fn process_local_factory_handles_are_not_physical_parameters_or_identity() {
    let mut registry = BehaviorRegistry::default();
    registry
        .register(sim_core::BehaviorDescriptor {
            type_id: "example.reference".into(),
            display_name: "Referenced model",
            ports: vec![],
            equations: None,
            parameters: Some(vec![
                sim_core::ParameterDeclaration::required("reference", "handle")
                    .implementation_reference(),
                sim_core::ParameterDeclaration::required("mass", "kg"),
            ]),
        })
        .unwrap();
    let mut model = ModelWorld::default();
    let part = model
        .part(
            &registry,
            "body",
            "example.reference",
            [("reference", 2.), ("mass", 3.)],
        )
        .unwrap();
    let first = describe(
        &model,
        &registry,
        "captured",
        1,
        &IdentityBindings::default(),
    )
    .unwrap();
    model.behaviors[part.behavior]
        .parameters
        .get_mut("reference")
        .unwrap()
        .value_si = 999.;
    let second = describe(
        &model,
        &registry,
        "captured",
        1,
        &IdentityBindings::default(),
    )
    .unwrap();
    assert_eq!(first.description.id, second.description.id);
    let component = second.description.components.values().next().unwrap();
    assert_eq!(component.parameters.len(), 1);
    assert_eq!(component.parameters["mass"].value, 3.);
}

#[test]
fn external_quantity_reaches_generic_inspection_without_domain_cases() {
    use sim_core::definitions::{Dimension, QuantityDescriptor, QuantityNature};
    use sim_core::{
        Behavior, BehaviorDescriptor, Context, QuantityKind, StateDeclaration, signal_out,
    };
    const CUSTOM: QuantityKind = QuantityKind::named("extension.concentration", 1, "mol/m³");
    struct Source;
    impl Behavior for Source {
        fn states(&self) -> Vec<StateDeclaration> {
            vec![StateDeclaration::new("amount_density", CUSTOM, 2.0)]
        }
        fn residual(&self, ctx: &mut Context) {
            ctx.set_state_residual(0, ctx.state_rate(0));
            ctx.set_signal(0, ctx.state(0));
        }
    }
    let mut registry = BehaviorRegistry::default();
    registry
        .register_quantity(&QuantityDescriptor {
            id: CUSTOM.definition_id(),
            label: "Concentration".into(),
            dimension: Dimension::si([-3, 0, 0, 0, 0, 1, 0]).into(),
            canonical_unit: CUSTOM.unit().into(),
            nature: QuantityNature::Linear,
            display_units: vec![],
        })
        .unwrap();
    registry
        .register(BehaviorDescriptor::new(
            "extension.source",
            "Source",
            vec![signal_out("value", CUSTOM)],
            |_| Ok(Box::new(Source)),
        ))
        .unwrap();
    let mut model = ModelWorld::default();
    model
        .part(&registry, "sample", "extension.source", [])
        .unwrap();
    let runtime = sim_compile::Runtime::new(
        model,
        &registry,
        sim_dynamics::Integrator::implicit_midpoint(),
    )
    .unwrap();
    let description = describe(
        &runtime.model,
        &registry,
        "extension-example",
        1,
        &IdentityBindings::default(),
    )
    .unwrap()
    .description;
    assert_eq!(
        description
            .observables
            .values()
            .filter(|o| o.quantity == CUSTOM.definition_id())
            .count(),
        2,
        "custom component state and signal, alongside generated diagnostics"
    );
    let decoded: SystemDescription =
        serde_json::from_slice(&serde_json::to_vec(&description).unwrap()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.id, description.id);
}
