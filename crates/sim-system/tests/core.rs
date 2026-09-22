//! Editing, hierarchy, flattening, swapping, library and history, checked
//! against an analytic RC charge curve through the compiled runtime.
use sim_core::BehaviorRegistry;
use sim_system::*;
use std::collections::BTreeMap;

fn registry() -> BehaviorRegistry {
    let mut r = BehaviorRegistry::default();
    sim_domain_electrical::elements::register(&mut r).unwrap();
    sim_domain_thermal::register(&mut r).unwrap();
    sim_domain_rotational::elements::register(&mut r).unwrap();
    sim_domain_bridges::elements::register(&mut r).unwrap();
    r
}

fn add(at: &str, name: &str, instance: InstanceSpec) -> Command {
    Command::AddInstance { at: at.into(), name: name.into(), instance }
}
fn connect(at: &str, terminals: &[(&str, &str)]) -> Command {
    Command::Connect { at: at.into(), terminals: terminals.iter().map(|(i, p)| Terminal::port(i, p)).collect(), label: String::new() }
}

/// 1 V step into R = 1 kΩ, C = 1 µF: v_c(t) = 1 − exp(−t/τ), τ = 1 ms.
fn rc_document(r: &BehaviorRegistry) -> SystemDocument {
    let mut doc = SystemDocument::new("RC");
    apply(
        &mut doc,
        r,
        &[
            add("", "vs", InstanceSpec::element("electrical.voltage_source").with("voltage", 1.0)),
            add("", "r", InstanceSpec::element("electrical.resistor").with("resistance", 1000.0).at([0.02, 0., 0.])),
            add("", "c", InstanceSpec::element("electrical.capacitor").with("capacitance", 1e-6).at([0.04, 0., 0.])),
            add("", "gnd", InstanceSpec::element("electrical.ground")),
            connect("", &[("vs", "p"), ("r", "p")]),
            connect("", &[("r", "n"), ("c", "p")]),
            connect("", &[("c", "n"), ("vs", "n"), ("gnd", "pin")]),
        ],
    )
    .unwrap();
    doc
}

fn capacitor_voltage(doc: &SystemDocument, r: &BehaviorRegistry, capacitor: &str, t: f64) -> f64 {
    let flat = flatten(doc, r).unwrap();
    let p = flat.ports[&format!("{capacitor}#p")];
    let n = flat.ports[&format!("{capacitor}#n")];
    let mut runtime = sim_compile::Runtime::new(flat.model, r, sim_dynamics::Integrator::implicit_midpoint()).unwrap();
    runtime.advance(t, 1e-6).unwrap();
    runtime.get(runtime.across_id(p)) - runtime.get(runtime.across_id(n))
}

#[test]
fn rc_charges_analytically_through_the_flattened_model() {
    let r = registry();
    let doc = rc_document(&r);
    assert!(Resolver::new(&doc, &r).findings().is_empty(), "{:?}", Resolver::new(&doc, &r).findings());
    let v = capacitor_voltage(&doc, &r, "c", 1e-3);
    let exact = 1.0 - (-1.0f64).exp();
    assert!((v - exact).abs() < 1e-5, "v = {v}, exact = {exact}");
}

#[test]
fn grouping_preserves_physics_and_creates_typed_boundary_ports() {
    let r = registry();
    let mut doc = rc_document(&r);
    let before = capacitor_voltage(&doc, &r, "c", 1e-3);
    apply(&mut doc, &r, &[Command::Group { at: "".into(), instances: vec!["r".into(), "c".into()], name: "filter".into(), definition: "rc_filter".into(), label: "RC filter".into() }]).unwrap();
    let group = &doc.definitions["rc_filter"];
    assert_eq!(group.instances.len(), 2);
    assert_eq!(group.ports.len(), 2, "one port toward the source, one toward ground: {:?}", group.ports);
    assert!(group.ports.values().all(|p| p.schema.is_some()), "boundary types are proven at grouping");
    // Placement is relative to the group's center.
    assert_eq!(doc.definitions["root"].instances["filter"].placement.position, [0.03, 0., 0.]);
    let after = capacitor_voltage(&doc, &r, "filter/c", 1e-3);
    assert!((before - after).abs() < 1e-12);
    let flat = flatten(&doc, &r).unwrap();
    let description = sim_inspect::model::describe(&flat.model, &r, &flat.source_hash, doc.revision, &flat.identities).unwrap().description;
    assert!(description.components.contains_key("filter/r"));
    assert_eq!(description.components["filter/c"].group.as_deref(), Some("filter"));
    assert!(description.groups.contains_key("filter"));
    // World placement composes the group frame.
    let part = flat.parts.iter().find(|p| p.component == "filter/c").unwrap();
    assert!((part.position[0] - 0.04).abs() < 1e-6);
    // Ungrouping restores an equivalent flat circuit.
    apply(&mut doc, &r, &[Command::Ungroup { at: "".into(), name: "filter".into() }]).unwrap();
    assert!(doc.definitions["root"].instances.contains_key("c"));
    let again = capacitor_voltage(&doc, &r, "c", 1e-3);
    assert!((before - again).abs() < 1e-12);
}

#[test]
fn drill_in_edits_the_definition_and_make_unique_forks_it() {
    let r = registry();
    let mut doc = rc_document(&r);
    apply(&mut doc, &r, &[Command::Group { at: "".into(), instances: vec!["r".into(), "c".into()], name: "filter".into(), definition: "rc_filter".into(), label: String::new() }]).unwrap();
    // A second placement of the same definition shares its contents.
    apply(&mut doc, &r, &[add("", "filter2", InstanceSpec::subsystem("rc_filter").at([0., 0.05, 0.]))]).unwrap();
    let out = apply(&mut doc, &r, &[Command::SetParameter { at: "filter".into(), name: "r".into(), parameter: "resistance".into(), binding: Some(ParameterBinding::value(2000.)) }]).unwrap();
    assert_eq!(out[0].shared_by, 2, "the edit reports every placement it affects");
    apply(&mut doc, &r, &[Command::MakeUnique { at: "".into(), name: "filter2".into(), definition: "rc_filter_b".into() }]).unwrap();
    apply(&mut doc, &r, &[Command::SetParameter { at: "filter2".into(), name: "r".into(), parameter: "resistance".into(), binding: Some(ParameterBinding::value(500.)) }]).unwrap();
    let a = &doc.definitions["rc_filter"].instances["r"].parameters["resistance"];
    let b = &doc.definitions["rc_filter_b"].instances["r"].parameters["resistance"];
    assert_eq!((a, b), (&ParameterBinding::value(2000.), &ParameterBinding::value(500.)));
    // Elements cannot be drilled into.
    let error = apply(&mut doc, &r, &[add("vs", "x", InstanceSpec::element("electrical.ground"))]).unwrap_err();
    assert!(error.to_string().contains("not a subsystem"), "{error}");
}

#[test]
fn parameters_flow_down_the_hierarchy() {
    let r = registry();
    let mut doc = rc_document(&r);
    apply(&mut doc, &r, &[Command::Group { at: "".into(), instances: vec!["r".into(), "c".into()], name: "filter".into(), definition: "rc_filter".into(), label: String::new() }]).unwrap();
    apply(
        &mut doc,
        &r,
        &[
            Command::DeclareParameter { at: "filter".into(), name: "tau_r".into(), declaration: ParameterDecl { unit: "Ω".into(), default: Some(1000.), description: "series resistance".into() } },
            Command::SetParameter { at: "filter".into(), name: "r".into(), parameter: "resistance".into(), binding: Some(ParameterBinding::Parameter { parameter: "tau_r".into() }) },
        ],
    )
    .unwrap();
    let default = capacitor_voltage(&doc, &r, "filter/c", 1e-3);
    assert!((default - (1.0 - (-1.0f64).exp())).abs() < 1e-5);
    apply(&mut doc, &r, &[Command::SetParameter { at: "".into(), name: "filter".into(), parameter: "tau_r".into(), binding: Some(ParameterBinding::value(2000.)) }]).unwrap();
    let doubled = capacitor_voltage(&doc, &r, "filter/c", 1e-3);
    assert!((doubled - (1.0 - (-0.5f64).exp())).abs() < 1e-5, "{doubled}");
    // Units are checked, never converted.
    let wrong = apply(&mut doc, &r, &[Command::SetParameter { at: "".into(), name: "vs".into(), parameter: "voltage".into(), binding: Some(ParameterBinding::Value { value: 1000., unit: Some("mV".into()), provenance: None }) }]);
    assert!(wrong.unwrap_err().to_string().contains("not converted"));
}

#[test]
fn swap_keeps_connections_and_rejects_broken_contracts() {
    let r = registry();
    let mut doc = rc_document(&r);
    // A detailed resistor: two 500 Ω halves behind the same p/n contract.
    let mut pair = Definition::new("Series pair");
    pair.interface = Some("electrical.resistor".into());
    pair.instances.insert("a".into(), InstanceSpec::element("electrical.resistor").with("resistance", 500.));
    pair.instances.insert("b".into(), InstanceSpec::element("electrical.resistor").with("resistance", 500.));
    pair.ports.insert("p".into(), BoundaryPort::default());
    pair.ports.insert("n".into(), BoundaryPort::default());
    pair.nets.push(Net { label: String::new(), terminals: vec![Terminal::boundary("p"), Terminal::port("a", "p")] });
    pair.nets.push(Net { label: String::new(), terminals: vec![Terminal::port("a", "n"), Terminal::port("b", "p")] });
    pair.nets.push(Net { label: String::new(), terminals: vec![Terminal::port("b", "n"), Terminal::boundary("n")] });
    apply(&mut doc, &r, &[Command::AddDefinitions { definitions: BTreeMap::from([("series_pair".to_string(), pair)]) }]).unwrap();
    let alternatives = library::alternatives(&doc, &r, None, "", "r").unwrap();
    let first = &alternatives[0];
    assert_eq!(first.kind, InstanceKind::Subsystem { definition: "series_pair".into() }, "same interface ranks first: {alternatives:?}");
    assert!(!alternatives.iter().any(|a| a.kind == InstanceKind::Element { component_type: "thermal.conductance".into() }));
    let before = capacitor_voltage(&doc, &r, "c", 1e-3);
    apply(&mut doc, &r, &[Command::Swap { at: "".into(), name: "r".into(), kind: InstanceKind::Subsystem { definition: "series_pair".into() }, keep_parameters: true }]).unwrap();
    assert!(doc.definitions["root"].instances["r"].parameters.is_empty(), "undeclared parameters are dropped");
    let after = capacitor_voltage(&doc, &r, "c", 1e-3);
    assert!((before - after).abs() < 1e-9, "{before} vs {after}");
    let broken = apply(&mut doc, &r, &[Command::Swap { at: "".into(), name: "c".into(), kind: InstanceKind::Element { component_type: "thermal.conductance".into() }, keep_parameters: false }]);
    assert!(broken.unwrap_err().to_string().contains("no port"));
}

#[test]
fn incompatible_nets_are_rejected_without_changing_the_document() {
    let r = registry();
    let mut doc = rc_document(&r);
    apply(&mut doc, &r, &[add("", "sink", InstanceSpec::element("thermal.capacitance").with("heat_capacity", 1.))]).unwrap();
    let snapshot = doc.clone();
    let error = apply(&mut doc, &r, &[connect("", &[("c", "p"), ("sink", "node")])]).unwrap_err();
    assert!(error.to_string().contains("Electrical") || error.to_string().contains("electrical"), "{error}");
    assert_eq!(doc, snapshot);
}

#[test]
fn shared_store_history_undo_redo_and_stale_edits() {
    let r = registry();
    let dir = std::env::temp_dir().join(format!("sim-system-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("rc.system.json");
    let store = SystemStore::create(&path, &rc_document(&r)).unwrap();
    let other = SystemStore::new(&path); // a second editor on the same file
    let applied = store.apply(&r, "Change R", &[Command::SetParameter { at: "".into(), name: "r".into(), parameter: "resistance".into(), binding: Some(ParameterBinding::value(10.)) }], None).unwrap();
    assert_eq!(other.load().unwrap().revision, applied.revision);
    assert!(matches!(other.apply(&r, "late", &[Command::SetTitle { title: "x".into() }], Some(applied.revision - 1)), Err(SystemError::Stale { .. })));
    other.undo().unwrap();
    assert_eq!(store.load().unwrap().definitions["root"].instances["r"].parameters["resistance"], ParameterBinding::value(1000.));
    store.redo().unwrap();
    assert_eq!(other.load().unwrap().definitions["root"].instances["r"].parameters["resistance"], ParameterBinding::value(10.));
    assert_eq!(store.history().undo, vec!["Change R".to_string()]);
    // A rejected batch writes nothing.
    let revision = store.load().unwrap().revision;
    assert!(store.apply(&r, "bad", &[Command::RemoveInstance { at: "".into(), name: "missing".into() }], None).is_err());
    assert_eq!(store.load().unwrap().revision, revision);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn library_round_trip_and_reference_images() {
    let r = registry();
    let dir = std::env::temp_dir().join(format!("sim-system-lib-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut doc = rc_document(&r);
    apply(&mut doc, &r, &[Command::Group { at: "".into(), instances: vec!["r".into(), "c".into()], name: "filter".into(), definition: "rc_filter".into(), label: "RC filter".into() }]).unwrap();
    let saved = library::save(&doc, "rc_filter", &dir.join("library")).unwrap();
    let listed = library::list(&dir.join("library"), &r).unwrap();
    assert_eq!(listed[0].id, "rc_filter");
    assert_eq!(listed[0].ports.len(), 2);
    let mut fresh = SystemDocument::new("fresh");
    apply(&mut fresh, &r, &[Command::AddDefinitions { definitions: library::import(&saved).unwrap() }, add("", "f", InstanceSpec::subsystem("rc_filter"))]).unwrap();
    assert!(fresh.definitions["rc_filter"].source.as_ref().unwrap().content_hash.len() == 64);

    // A 3×2 PNG header is enough for the asset record.
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&3u32.to_be_bytes());
    png.extend_from_slice(&2u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0]);
    let image = dir.join("board.png");
    std::fs::write(&image, &png).unwrap();
    let path = dir.join("rc.system.json");
    let store = SystemStore::create(&path, &doc).unwrap();
    store.import_reference(&r, "filter", "photo", &image, ReferenceView::Spatial, [0., 0., 0.], 0.03).unwrap();
    let loaded = store.load().unwrap();
    let reference = &loaded.definitions["rc_filter"].references["photo"];
    let asset = &loaded.assets[&reference.asset];
    assert_eq!((asset.width_px, asset.height_px), (3, 2));
    assert!(assets::resolve(&path, asset).exists());
    assert!((reference.height(asset) - 0.02).abs() < 1e-6);
    // Two points 10 mm apart on the image are really 25 mm apart.
    store.apply(&r, "Calibrate", &[Command::CalibrateReference { at: "filter".into(), id: "photo".into(), first: [0., 0., 0.], second: [0.01, 0., 0.], distance: 0.025 }], None).unwrap();
    let calibrated = &store.load().unwrap().definitions["rc_filter"].references["photo"];
    assert!((calibrated.width - 0.075).abs() < 1e-6);
    // Reference images are presentation only: physics is unchanged.
    assert_eq!(flatten(&store.load().unwrap(), &r).unwrap().model.behaviors.len(), flatten(&doc, &r).unwrap().model.behaviors.len());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn documents_round_trip_through_json() {
    let r = registry();
    let doc = rc_document(&r);
    let text = serde_json::to_string_pretty(&doc).unwrap();
    let back: SystemDocument = serde_json::from_str(&text).unwrap();
    assert_eq!(doc, back);
    assert_eq!(doc.content_hash(), back.content_hash());
    let command: Command = serde_json::from_str(r#"{"command":"connect","terminals":[{"instance":"r","port":"p"},{"boundary":"x"}]}"#).unwrap();
    assert!(matches!(command, Command::Connect { .. }));
}
