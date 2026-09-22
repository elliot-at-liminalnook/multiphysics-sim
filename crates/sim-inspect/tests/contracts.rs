use sim_core::{ConnectorKind, QuantityKind, definitions::builtins};
use sim_inspect::*;
use std::collections::BTreeMap;

fn description() -> SystemDescription {
    let mut d = SystemDescription {
        version: SCHEMA_VERSION,
        id: String::new(),
        source_hash: "captured-source-hash".into(),
        model_revision: 3,
        definitions: builtins::registry()
            .unwrap()
            .freeze()
            .unwrap()
            .catalog()
            .clone(),
        components: BTreeMap::new(),
        ports: BTreeMap::new(),
        nets: BTreeMap::new(),
        groups: BTreeMap::new(),
        observables: BTreeMap::new(),
        diagnostics: Vec::new(),
    };
    for id in ["a", "b", "c"] {
        d.components.insert(
            id.into(),
            ComponentDescription {
                id: id.into(),
                label: id.into(),
                component_type: "thermal.capacitance".into(),
                group: None,
                source: None,
                cad: None,
                persistent_identity: true,
                parameters: BTreeMap::new(),
            },
        );
        let p = format!("{id}.node");
        d.ports.insert(
            p.clone(),
            PortDescription {
                id: p,
                name: "node".into(),
                component: id.into(),
                composite_parent: None,
                schema: PortKind::Physical {
                    connector: builtins::connector_id(ConnectorKind::Thermal),
                },
            },
        );
    }
    d.nets.insert(
        "junction".into(),
        NetDescription {
            id: "junction".into(),
            ports: vec!["a.node".into(), "b.node".into(), "c.node".into()],
        },
    );
    d.observables.insert(
        "a.temperature".into(),
        ObservableDescriptor {
            id: "a.temperature".into(),
            label: "Temperature".into(),
            quantity: builtins::quantity_id(QuantityKind::Temperature),
            location: ObservationLocation::Across {
                port: "a.node".into(),
                lane: "temperature".into(),
            },
            sign_convention: None,
            coordinate_frame: None,
            availability: Availability::Available,
        },
    );
    d.seal().unwrap();
    d
}

fn sample(d: &SystemDescription) -> SampleFrame {
    SampleFrame {
        version: SCHEMA_VERSION,
        description_id: d.id.clone(),
        model_revision: d.model_revision,
        run_id: "run-a".into(),
        generation: 2,
        sequence: 0,
        step: 10,
        time: 0.1,
        values: BTreeMap::from([(
            "a.temperature".into(),
            SampleValue::Committed {
                value: 293.15,
                sample_time: 0.1,
            },
        )]),
    }
}

#[test]
fn roundtrip_preserves_branched_nets_and_detects_changed_sources() {
    let d = description();
    let json = serde_json::to_string(&d).unwrap();
    let decoded: SystemDescription = serde_json::from_str(&json).unwrap();
    decoded.validate().unwrap();
    assert_eq!(json, serde_json::to_string(&decoded).unwrap());
    assert_eq!(decoded.nets["junction"].ports.len(), 3);
    let mut edited = decoded.clone();
    edited.components.get_mut("a").unwrap().label = "renamed".into();
    assert!(edited.validate().is_err());
    edited.seal().unwrap();
    assert_ne!(edited.id, decoded.id);
    assert!(sample(&d).validate(&edited).is_err());
}

#[test]
fn stale_worker_frames_do_not_change_the_live_cursor() {
    let d = description();
    let mut f = sample(&d);
    let mut gate = FrameGate::new(f.run_id.clone(), f.generation);
    gate.accept(&d, &f).unwrap();
    assert!(gate.accept(&d, &f).is_err());
    f.sequence = 1;
    f.generation = 1;
    assert!(gate.accept(&d, &f).is_err());
    f.generation = 2;
    f.run_id = "other-run".into();
    assert!(gate.accept(&d, &f).is_err());
    f.run_id = "run-a".into();
    gate.accept(&d, &f).unwrap();
    f.sequence = 2;
    f.step = 0;
    assert!(gate.accept(&d, &f).is_err());
    // Observation playback can move backward by validating a captured frame.
    sample(&d).validate(&d).unwrap();
}

#[test]
fn missing_and_nonfinite_values_are_never_silently_zero() {
    let d = description();
    let mut f = sample(&d);
    f.values.insert(
        "a.temperature".into(),
        SampleValue::Unavailable {
            reason: "not captured".into(),
        },
    );
    f.validate(&d).unwrap();
    assert!(serde_json::to_string(&f).unwrap().contains("not captured"));
    for value in [f64::NAN, f64::INFINITY] {
        f.values.insert(
            "a.temperature".into(),
            SampleValue::Committed {
                value,
                sample_time: 0.1,
            },
        );
        assert!(f.validate(&d).is_err());
    }
    f.values.insert(
        "a.temperature".into(),
        SampleValue::Committed {
            value: 293.15,
            sample_time: 0.2,
        },
    );
    assert!(f.validate(&d).is_err());
    f.values.clear();
    f.values.insert(
        "unknown".into(),
        SampleValue::Unavailable {
            reason: "none".into(),
        },
    );
    assert!(f.validate(&d).is_err());
}

#[test]
fn presentation_changes_do_not_modify_physical_identity() {
    let d = description();
    let before = serde_json::to_string(&d).unwrap();
    let mut layout = DiagramState::new(&d);
    layout
        .positions
        .insert("a".into(), Point { x: 30., y: 40. });
    layout.pinned.insert("a".into());
    layout.plot_observables.insert("a.temperature".into());
    layout.validate(&d).unwrap();
    let roundtrip: DiagramState =
        serde_json::from_str(&serde_json::to_string(&layout).unwrap()).unwrap();
    assert_eq!(layout, roundtrip);
    assert_eq!(before, serde_json::to_string(&d).unwrap());
    layout.zoom = f32::NAN;
    assert!(layout.validate(&d).is_err());
    layout.zoom = 1.;
    layout.pinned.insert("missing".into());
    assert!(layout.validate(&d).is_err());
}

#[test]
fn invalid_authoring_graph_can_be_inspected_but_broken_identity_cannot() {
    let mut d = description();
    d.ports.get_mut("b.node").unwrap().schema = PortKind::Unresolved {
        declared_type: "new-domain.unknown".into(),
    };
    d.diagnostics.push(Diagnostic {
        code: "missing_type".into(),
        message: "Domain package is unavailable".into(),
        subject: Some("b.node".into()),
    });
    d.seal().unwrap();
    d.validate().unwrap();
    d.nets
        .get_mut("junction")
        .unwrap()
        .ports
        .push("missing-port".into());
    assert!(d.seal().is_err());
}

#[test]
fn wrong_observation_units_and_group_cycles_are_rejected() {
    let mut d = description();
    d.observables.get_mut("a.temperature").unwrap().quantity =
        builtins::quantity_id(QuantityKind::Voltage);
    assert!(
        d.seal()
            .unwrap_err()
            .to_string()
            .contains("quantity mismatch")
    );
    let mut d = description();
    d.groups.insert(
        "g".into(),
        GroupDescription {
            id: "g".into(),
            label: "group".into(),
            parent: Some("g".into()),
        },
    );
    assert!(d.seal().unwrap_err().to_string().contains("cyclic"));
}

#[test]
fn stage_frames_are_versioned_and_reject_invalid_sampling_intervals() {
    let d = description();
    let mut f = sample(&d);
    let id = f.values.keys().next().unwrap().clone();
    f.version = SAMPLE_FRAME_VERSION;
    f.values.insert(
        id.clone(),
        SampleValue::AcceptedStage {
            value: 293.15,
            sample_time: 0.05,
            step_start: 0.,
            step_end: 0.1,
        },
    );
    f.validate(&d).unwrap();
    let json = serde_json::to_vec(&f).unwrap();
    let restored: SampleFrame = serde_json::from_slice(&json).unwrap();
    assert_eq!(restored, f);
    f.version = 1;
    assert!(f.validate(&d).is_err());
    f.version = SAMPLE_FRAME_VERSION;
    for (sample_time, step_start, step_end) in [
        (0.05, 0.1, 0.),
        (0.2, 0., 0.1),
        (0.05, 0., 1.),
        (f64::NAN, 0., 0.1),
    ] {
        f.values.insert(
            id.clone(),
            SampleValue::AcceptedStage {
                value: 293.15,
                sample_time,
                step_start,
                step_end,
            },
        );
        assert!(f.validate(&d).is_err());
    }
}
