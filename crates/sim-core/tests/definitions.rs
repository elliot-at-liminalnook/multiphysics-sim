use sim_core::definitions::{builtins, *};
use sim_core::{ConnectorKind, QuantityKind};

fn concentration() -> QuantityDescriptor {
    QuantityDescriptor {
        id: DefinitionId::new("diffusion.concentration", 1),
        label: "Concentration".into(),
        dimension: Dimension::si([-3, 0, 0, 0, 0, 1, 0]).into(),
        canonical_unit: "mol/m³".into(),
        nature: QuantityNature::Linear,
        display_units: vec![],
    }
}

struct Diffusion;
impl ConnectorDefinition for Diffusion {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            id: DefinitionId::new("diffusion.port", 1),
            label: "Diffusion".into(),
            lanes: vec![LaneDescriptor {
                across: VariableDescriptor {
                    name: "concentration".into(),
                    quantity: concentration().id,
                },
                through: Some(VariableDescriptor {
                    name: "molar_flow".into(),
                    quantity: builtins::quantity_id(QuantityKind::MolarFlow),
                }),
                derivative_of: None,
            }],
            rule: ConnectionRule::Balanced,
            energy: PortEnergy::Unavailable,
        }
    }
}

#[test]
fn external_traits_register_without_domain_enums() {
    let mut registry = builtins::registry().unwrap();
    registry.register_quantity(&concentration()).unwrap();
    let id = registry.register_connector(&Diffusion).unwrap();
    let frozen = registry.freeze().unwrap();
    let layout = frozen
        .layout(frozen.connector_handle(&id).unwrap())
        .unwrap();
    assert_eq!(layout.lanes[0].across.quantity, concentration().id);
    assert_eq!(
        frozen
            .quantity(frozen.quantity_handle(&concentration().id).unwrap())
            .unwrap()
            .canonical_unit,
        "mol/m³"
    );
}

#[test]
fn builtins_preserve_lane_order_derivatives_and_owner_rows() {
    let frozen = builtins::registry().unwrap().freeze().unwrap();
    for connector in builtins::CONNECTORS {
        let handle = frozen
            .connector_handle(&connector.definition_id())
            .unwrap();
        let layout = frozen.layout(handle).unwrap();
        let old_lanes = connector.lanes();
        assert_eq!(layout.lanes.len(), old_lanes.len());
        for (lane, old) in layout.lanes.iter().zip(old_lanes) {
            assert_eq!(lane.across.quantity, builtins::quantity_id(&old.across_kind));
            assert_eq!(lane.derivative_of, old.derivative_of);
            assert_eq!(
                lane.through.as_ref().map(|t| &t.quantity),
                (old.through != "-")
                    .then(|| builtins::quantity_id(&old.through_kind))
                    .as_ref()
            );
        }
        if connector.is_owned() {
            let ConnectionRule::Owned {
                contribution_rows, ..
            } = &frozen.connector(handle).unwrap().rule
            else {
                panic!("lost frame ownership")
            };
            assert_eq!(
                contribution_rows,
                &(connector.owned_wrench_offset()..connector.across_width()).collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn registration_order_and_catalog_roundtrip_preserve_fingerprint() {
    let first = builtins::registry().unwrap().freeze().unwrap();
    let mut catalog: DefinitionCatalog =
        serde_json::from_str(&serde_json::to_string(first.catalog()).unwrap()).unwrap();
    catalog.quantities.reverse();
    catalog.connectors.reverse();
    let second = DefinitionRegistry::from_catalog(catalog)
        .unwrap()
        .freeze()
        .unwrap();
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(
        serde_json::to_string(first.catalog()).unwrap(),
        serde_json::to_string(second.catalog()).unwrap()
    );
}

#[test]
fn duplicate_registration_is_atomic_and_stale_handles_are_rejected() {
    let mut registry = builtins::registry().unwrap();
    let before = registry.freeze().unwrap();
    let id = builtins::quantity_id(QuantityKind::Voltage);
    let mut duplicate = builtins::quantity(QuantityKind::Voltage);
    duplicate.label = "Corrupt replacement".into();
    assert_eq!(
        registry.register_quantity(&duplicate),
        Err(DefinitionError::Duplicate(id.clone()))
    );
    assert_eq!(
        before.fingerprint(),
        registry.freeze().unwrap().fingerprint()
    );
    registry.register_quantity(&concentration()).unwrap();
    let after = registry.freeze().unwrap();
    assert!(
        after
            .quantity(before.quantity_handle(&id).unwrap())
            .is_none()
    );
    let c = builtins::connector_id(ConnectorKind::Electrical);
    assert!(after.layout(before.connector_handle(&c).unwrap()).is_none());
}

#[test]
fn identities_and_versions_are_validated_without_silent_upgrade() {
    let mut registry = DefinitionRegistry::default();
    let mut q = concentration();
    q.id.version = 0;
    assert!(registry.register_quantity(&q).is_err());
    q.id = DefinitionId::new("not a namespace", 1);
    assert!(registry.register_quantity(&q).is_err());
    q = concentration();
    registry.register_quantity(&q).unwrap();
    q.id.version = 2;
    registry.register_quantity(&q).unwrap();
    let frozen = registry.freeze().unwrap();
    assert_ne!(
        frozen.quantity_handle(&concentration().id).unwrap(),
        frozen.quantity_handle(&q.id).unwrap()
    );
    assert!(
        frozen
            .quantity_handle(&DefinitionId::new("diffusion.concentration", 3))
            .is_err()
    );
}

#[test]
fn missing_references_and_composition_cycles_fail_before_compilation() {
    let mut r = builtins::registry().unwrap();
    r.register_connector(&Diffusion).unwrap();
    assert!(matches!(r.freeze(), Err(DefinitionError::Missing(_))));
    r.register_quantity(&concentration()).unwrap();
    r.freeze().unwrap();
    let mut a = Diffusion.descriptor();
    a.id = DefinitionId::new("test.a", 1);
    a.lanes.clear();
    let mut b = a.clone();
    b.id = DefinitionId::new("test.b", 1);
    a.rule = ConnectionRule::Composite {
        members: vec![ConnectorMember {
            name: "child".into(),
            connector: b.id.clone(),
        }],
    };
    b.rule = ConnectionRule::Composite {
        members: vec![ConnectorMember {
            name: "child".into(),
            connector: a.id.clone(),
        }],
    };
    r.register_connector(&a).unwrap();
    r.register_connector(&b).unwrap();
    assert!(matches!(r.freeze(), Err(DefinitionError::Cycle(_))));
}

#[test]
fn derivative_units_and_owner_mappings_are_checked() {
    let mut r = builtins::registry().unwrap();
    let mut connector = builtins::connector(ConnectorKind::Rotational);
    connector.id = DefinitionId::new("invalid.rate", 1);
    connector.lanes[1].across.quantity = builtins::quantity_id(QuantityKind::Voltage);
    r.register_connector(&connector).unwrap();
    assert!(
        r.freeze()
            .unwrap_err()
            .to_string()
            .contains("derivative dimensions")
    );
    let mut r = builtins::registry().unwrap();
    let mut connector = builtins::connector(ConnectorKind::Frame);
    connector.id = DefinitionId::new("invalid.owner", 1);
    connector.rule = ConnectionRule::Owned {
        contribution_rows: vec![7; 6],
        unit_quaternions: vec![],
    };
    r.register_connector(&connector).unwrap();
    assert!(
        r.freeze()
            .unwrap_err()
            .to_string()
            .contains("owner contribution")
    );
}

#[test]
fn composite_derivatives_rebase_and_member_names_are_unique() {
    let frozen = builtins::registry().unwrap().freeze().unwrap();
    let c = frozen
        .layout(
            frozen
                .connector_handle(&builtins::connector_id(ConnectorKind::MOTOR))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(c.member_offsets, vec![0, 1, 3]);
    assert_eq!(c.lanes[2].derivative_of, Some(1));
    let mut registry = builtins::registry().unwrap();
    let mut dup = builtins::connector(ConnectorKind::MOTOR);
    dup.id = DefinitionId::new("invalid.members", 1);
    let ConnectionRule::Composite { members } = &mut dup.rule else {
        unreachable!()
    };
    members[1].name = members[0].name.clone();
    registry.register_connector(&dup).unwrap();
    assert!(
        registry
            .freeze()
            .unwrap_err()
            .to_string()
            .contains("duplicate composite")
    );
}

#[test]
fn temperature_offsets_and_fractional_dimensions_are_explicit() {
    let mut q = concentration();
    q.display_units.push(DisplayUnit {
        symbol: "bad".into(),
        scale: 1.,
        offset: 1.,
    });
    assert!(DefinitionRegistry::default().register_quantity(&q).is_err());
    let frozen = builtins::registry().unwrap().freeze().unwrap();
    let t = frozen
        .quantity(
            frozen
                .quantity_handle(&builtins::quantity_id(QuantityKind::Temperature))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(t.display_units[0].offset, 273.15);
    let delta = frozen
        .quantity(
            frozen
                .quantity_handle(&DefinitionId::new("sim.quantity.TemperatureDifference", 1))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(delta.nature, QuantityNature::TemperatureDifference);
    assert_eq!(delta.display_units[0].offset, 0.);
    let position = builtins::quantity(QuantityKind::MassNormalizedDisplacement);
    let rate = builtins::quantity(QuantityKind::MassNormalizedVelocity);
    let QuantityDimension::Si { dimension } = position.dimension else {
        panic!()
    };
    assert_eq!(
        QuantityDimension::from(dimension.rate().unwrap()),
        rate.dimension
    );
    assert!(Dimension::rational([1; 7], 0).is_err());
}

#[test]
fn power_is_explicit_and_dimensionally_validated() {
    let frozen = builtins::registry().unwrap().freeze().unwrap();
    for kind in [ConnectorKind::Granular, ConnectorKind::NormalizedAcoustic] {
        assert_eq!(
            frozen
                .connector(
                    frozen
                        .connector_handle(&builtins::connector_id(kind))
                        .unwrap()
                )
                .unwrap()
                .energy,
            PortEnergy::Unavailable
        );
    }
    let mut r = builtins::registry().unwrap();
    r.register_quantity(&concentration()).unwrap();
    let mut invalid = Diffusion.descriptor();
    invalid.energy = PortEnergy::Products {
        terms: vec![PowerTerm {
            across_lane: 0,
            across_rate: false,
            through_lane: 0,
        }],
    };
    r.register_connector(&invalid).unwrap();
    assert!(r.freeze().unwrap_err().to_string().contains("not power"));
}

#[test]
fn semantic_identity_is_not_dimensional_equivalence() {
    let energy = builtins::quantity(QuantityKind::Energy);
    let torque = builtins::quantity(QuantityKind::Torque);
    assert_eq!(energy.dimension, torque.dimension);
    assert_ne!(energy.id, torque.id);
    assert_ne!(
        SignalType::Any,
        SignalType::Quantity(builtins::quantity_id(QuantityKind::Dimensionless))
    );
}

#[test]
fn component_registry_shares_definitions_and_duplicate_failure_is_atomic() {
    use sim_core::{BehaviorDescriptor, BehaviorRegistry, BehaviorTypeId, acausal};
    fn never_build(
        _: &std::collections::BTreeMap<String, f64>,
    ) -> Result<Box<dyn sim_core::Behavior>, sim_core::EquationError> {
        unreachable!("this test inspects declarations without executing equations")
    }
    let mut r = BehaviorRegistry::default();
    r.register_quantity(&concentration()).unwrap();
    r.register_connector(&Diffusion).unwrap();
    let descriptor = BehaviorDescriptor::new(
        "test.component",
        "Original",
        vec![acausal("plug", ConnectorKind::MOTOR)],
        never_build,
    );
    r.register_definition(&descriptor).unwrap();
    let fingerprint = r.definitions().unwrap().fingerprint().to_owned();
    static EMPTY: [ConnectorKind; 0] = [];
    let invalid = BehaviorDescriptor::new("invalid.component", "Invalid empty composite",
        vec![acausal("empty", ConnectorKind::Composite(&EMPTY))], never_build);
    assert!(r.register(invalid).is_err());
    assert_eq!(r.definitions().unwrap().fingerprint(), fingerprint);
    assert!(!r.contains(&BehaviorTypeId::from("invalid.component")));
    let mut replacement = descriptor.clone();
    replacement.display_name = "Incorrect replacement";
    assert!(r.register_definition(&replacement).is_err());
    assert_eq!(
        r.get(&BehaviorTypeId::from("test.component"))
            .unwrap()
            .display_name,
        "Original"
    );
    assert!(
        r.definitions()
            .unwrap()
            .connector_handle(&Diffusion.descriptor().id)
            .is_ok()
    );
}

#[test]
fn rational_dimension_overflow_is_reported_and_reducible_products_work() {
    let q = Dimension::rational([1, 0, 0, 0, 0, 0, 0], 60_000).unwrap();
    assert_eq!(
        q.product(q).unwrap(),
        Dimension::rational([1, 0, 0, 0, 0, 0, 0], 30_000).unwrap()
    );
    let large = Dimension::si([i16::MAX; 7]);
    assert!(large.product(large).is_err());
    assert!(
        Dimension {
            powers: [0; 7],
            denominator: 0
        }
        .rate()
        .is_err()
    );
}

#[test]
fn open_quantity_reference_roundtrips_and_validates_registered_units() {
    let descriptor = concentration();
    let quantity = QuantityKind::from_descriptor(&descriptor);
    let mut registry = builtins::registry().unwrap();
    assert!(quantity.validate(&registry.freeze().unwrap()).is_err());
    registry.register_quantity(&descriptor).unwrap();
    let definitions = registry.freeze().unwrap();
    quantity.validate(&definitions).unwrap();
    assert_eq!(quantity.definition_id(), descriptor.id);
    assert_eq!(quantity.unit(), "mol/m³");
    let json = serde_json::to_string(&quantity).unwrap();
    let restored: QuantityKind = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, quantity);
    restored.validate(&definitions).unwrap();
    assert!(QuantityKind::named("diffusion.concentration", 1, "kg/m³")
        .validate(&definitions).is_err());
    assert!(QuantityKind::named("diffusion.concentration", 2, "mol/m³")
        .validate(&definitions).is_err());
}

#[test]
fn legacy_quantity_json_preserves_all_builtin_spellings() {
    let definitions = builtins::registry().unwrap().freeze().unwrap();
    for quantity in builtins::QUANTITIES {
        let legacy = serde_json::to_string(quantity).unwrap();
        assert!(legacy.starts_with('"'));
        let decoded: QuantityKind = serde_json::from_str(&legacy).unwrap();
        assert_eq!(&decoded, quantity);
        decoded.validate(&definitions).unwrap();
    }
    assert_eq!(serde_json::to_string(&QuantityKind::Voltage).unwrap(), "\"Voltage\"");
}
