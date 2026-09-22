use sim_inspect::{SystemDescription, spatial::*};

fn fixture() -> (SystemDescription, SpatialDescription) {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/systems-viewer/spatial");
    (
        serde_json::from_slice(
            &std::fs::read(root.join("motor-thermal.description.json")).unwrap(),
        )
        .unwrap(),
        serde_json::from_slice(&std::fs::read(root.join("motor-thermal.spatial.json")).unwrap())
            .unwrap(),
    )
}

#[test]
fn fixture_covers_real_components_and_roundtrips() {
    let (d, s) = fixture();
    s.validate(&d).unwrap();
    let covered: std::collections::BTreeSet<_> =
        s.parts.iter().map(|p| p.component.as_str()).collect();
    assert_eq!(covered, d.components.keys().map(String::as_str).collect());
    let copy: SpatialDescription =
        serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
    copy.validate(&d).unwrap();
    assert_eq!(
        serde_json::to_value(&copy).unwrap(),
        serde_json::to_value(&s).unwrap()
    );
}

#[test]
fn reject_foreign_sources_invalid_geometry_and_ambiguous_ids() {
    let (d, s) = fixture();
    let mut bad = s.clone();
    bad.description_id = "another-model".into();
    assert!(bad.validate(&d).unwrap_err().0.contains("foreign"));
    let mut bad = s.clone();
    bad.parts[0].component = "missing".into();
    assert!(bad.validate(&d).unwrap_err().0.contains("unknown"));
    let mut bad = s.clone();
    bad.parts.push(bad.parts[0].clone());
    assert!(bad.validate(&d).unwrap_err().0.contains("duplicate"));
    for value in [f32::NAN, f32::INFINITY] {
        let mut bad = s.clone();
        bad.parts[0].position[0] = value;
        assert!(bad.validate(&d).is_err());
    }
    for radius in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let mut bad = s.clone();
        bad.parts[0].shape = SpatialShape::Sphere { radius };
        assert!(bad.validate(&d).is_err());
    }
    let mut bad = s.clone();
    bad.parts[0].rotation_xyzw = [0.0; 4];
    assert!(bad.validate(&d).unwrap_err().0.contains("normalized"));
    let mut bad = s.clone();
    bad.length_unit = "mm".into();
    assert!(bad.validate(&d).is_err());
    let mut bad = s.clone();
    bad.coordinate_frame = "z_up".into();
    assert!(bad.validate(&d).is_err());
    let mut bad = s.clone();
    bad.version += 1;
    assert!(bad.validate(&d).is_err());
    let mut bad = s;
    bad.parts[0].provenance = GeometryProvenance::Illustrative {
        explanation: "".into(),
    };
    assert!(bad.validate(&d).is_err());
}

#[test]
fn presentation_commands_preserve_source_and_validate_selection() {
    let (d, s) = fixture();
    let before = (
        serde_json::to_vec(&d).unwrap(),
        serde_json::to_vec(&s).unwrap(),
    );
    let mut state = SpatialViewState::default();
    let id = s.parts[0].component.clone();
    state
        .apply(
            &s,
            SpatialCommand::Select {
                component: id.clone(),
            },
        )
        .unwrap();
    state.apply(&s, SpatialCommand::HideSelected).unwrap();
    assert!(state.hidden.contains(&id));
    state
        .apply(&s, SpatialCommand::SetExploded { enabled: true })
        .unwrap();
    state
        .apply(&s, SpatialCommand::SetConnections { enabled: true })
        .unwrap();
    let last = state.clone();
    assert!(
        state
            .apply(
                &s,
                SpatialCommand::Select {
                    component: "unknown".into()
                }
            )
            .is_err()
    );
    assert_eq!(state, last, "invalid commands must be atomic");
    state
        .apply(
            &s,
            SpatialCommand::Select {
                component: id.clone(),
            },
        )
        .unwrap();
    assert!(state.hidden.is_empty(), "selection reveals a hidden part");
    state.apply(&s, SpatialCommand::HideSelected).unwrap();
    state.apply(&s, SpatialCommand::ShowAll).unwrap();
    state.apply(&s, SpatialCommand::ClearSelection).unwrap();
    assert!(state.hidden.is_empty());
    assert!(state.selected.is_none());
    assert_eq!(
        before,
        (
            serde_json::to_vec(&d).unwrap(),
            serde_json::to_vec(&s).unwrap()
        )
    );
}
