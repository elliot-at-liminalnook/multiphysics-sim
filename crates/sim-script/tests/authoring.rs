use rhai::Map;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, Channel, Contract, Coupler, ModelWorld, QuantityKind};
use sim_dynamics::Integrator;
use sim_script::{evaluate, RhaiController, Sources};
use std::collections::BTreeMap;

fn registry() -> BehaviorRegistry {
    let mut registry = BehaviorRegistry::default();
    sim_domain_rotational::elements::register(&mut registry).unwrap();
    registry
}

#[test]
fn parameter_discovery_and_validation_use_native_registry_declarations() {
    let registry = registry();
    let catalogue = sim_script::catalogue(&registry);
    let inertia = catalogue
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["type"] == "rotational.inertia")
        .unwrap();
    assert_eq!(inertia["parameters_complete"], true);
    assert!(inertia["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == "inertia" && p["unit"] == "kg·m²" && p["required"] == true));
    assert_eq!(inertia["ports"][0]["lanes"][0]["across_unit"], "rad");
    for params in [
        "#{inertia:1.0, dampning:0.1}",
        "#{inertia:0.0}",
        "#{inertia:1.0, \"initial.speeed\":3.0}",
    ] {
        let source = format!("let disk = part(\"disk\",\"rotational.inertia\",{params});");
        let error = evaluate(
            &Sources::single("parameters.rhai", &source),
            &registry,
            Map::new(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("disk") && error.contains("line 1"),
            "{error}"
        );
    }
    let mut native = ModelWorld::default();
    let disk = native
        .part(
            &registry,
            "disk",
            "rotational.inertia",
            [("inertia", 1.), ("dampning", 0.1)],
        )
        .unwrap();
    native.connect([disk.port("shaft")]);
    let error = Runtime::new(
        native,
        &registry,
        Integrator::BackwardEuler(Default::default()),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("dampning"), "{error}");
}

#[test]
fn captured_seed_is_available_to_systems_and_fresh_controllers() {
    let sources = Sources::single("system.rhai", "let disk = part(\"disk\", \"rotational.inertia\", #{inertia:1.0, \"initial.speed\": seed()});");
    for seed in [0, 47, (1_u64 << 53) - 1] {
        let plan = sim_script::evaluate_seeded(&sources, &registry(), Map::new(), seed).unwrap();
        assert_eq!(plan.components[0].parameters["initial.speed"], seed as f64);
        let sources = Sources::single(
            "controller.rhai",
            "fn control(t,s,a,state) { a[\"target\"] = seed(); #{commands:a,state:state} }",
        );
        let mut controller = RhaiController::with_seed(sources, Map::new(), seed).unwrap();
        let contract = Contract {
            element: "controller".into(),
            period: 0.01,
            sensors: vec![],
            actuators: vec![Channel {
                name: "target".into(),
                kind: QuantityKind::Dimensionless,
            }],
        };
        controller.open(&contract).unwrap();
        let mut values = [0.];
        controller.sample(0., &[], &mut values).unwrap();
        assert_eq!(values, [seed as f64]);
    }
}

#[test]
fn native_and_scripted_models_have_identical_traces() {
    let registry = registry();
    let sources = Sources::single(
        "system.rhai",
        r#"
        let disk = part("disk", "rotational.inertia", #{inertia: 2.0, damping: 0.5, "initial.speed": 3.0});
        connect([disk.port("shaft")]);
    "#,
    );
    let plan = evaluate(&sources, &registry, Map::new()).unwrap();
    assert_eq!(plan.components[0].location.source, "system.rhai");
    let mut scripted = ModelWorld::default();
    let parts = plan
        .apply(&mut scripted, &registry, BTreeMap::new())
        .unwrap();
    let mut native = ModelWorld::default();
    let disk = native
        .part(
            &registry,
            "disk",
            "rotational.inertia",
            [("inertia", 2.), ("damping", 0.5), ("initial.speed", 3.)],
        )
        .unwrap();
    native.connect([disk.port("shaft")]);
    let mut a = Runtime::new(
        native,
        &registry,
        Integrator::BackwardEuler(Default::default()),
    )
    .unwrap();
    let mut b = Runtime::new(
        scripted,
        &registry,
        Integrator::BackwardEuler(Default::default()),
    )
    .unwrap();
    let sa = a.state_id(disk.behavior, "speed");
    let sb = b.state_id(parts["disk"].behavior, "speed");
    for _ in 0..100 {
        a.advance(0.01, 0.001).unwrap();
        b.advance(0.01, 0.001).unwrap();
        assert!((a.get(sa) - b.get(sb)).abs() < 1e-12);
    }
}

#[test]
fn component_references_connect_forward_declarations_without_duplication() {
    let r = registry();
    let plan = evaluate(&Sources::single("references.rhai", r#"
        let disk = component("graph/disk");
        connect([disk.port("shaft")]);
        let declared = part("graph/disk", "rotational.inertia", #{inertia:2.0});
    "#), &r, Map::new()).unwrap();
    assert_eq!(plan.components.len(), 1);
    let mut world = ModelWorld::default();
    plan.apply(&mut world, &r, BTreeMap::new()).unwrap();
    Runtime::new(world, &r, Integrator::BackwardEuler(Default::default())).unwrap();
    let plan = evaluate(&Sources::single("references.rhai",
        "connect([component(\"missing\").port(\"shaft\")]);"), &r, Map::new()).unwrap();
    let error = plan.apply(&mut ModelWorld::default(), &r, BTreeMap::new()).unwrap_err();
    assert!(error.contains("references.rhai:1:") && error.contains("missing"), "{error}");
}

#[test]
fn scripted_attachment_joins_existing_native_thermal_nets() {
    let mut r = registry();
    sim_domain_thermal::register(&mut r).unwrap();
    let mut world = ModelWorld::default();
    let winding = world.part(&r, "winding", "thermal.capacitance",
        [("heat_capacity", 2.), ("initial.temperature", 300.)]).unwrap();
    let housing = world.part(&r, "housing", "thermal.capacitance",
        [("heat_capacity", 3.), ("initial.temperature", 300.)]).unwrap();
    world.connect([winding.port("node")]);
    world.connect([housing.port("node")]);
    let plan = evaluate(&Sources::single("attachment.rhai", r#"
        let heat = part("heat", "thermal.heat_source", #{power:10.0});
        connect([component("winding").port("node"), heat.port("node")]);
        connect([component("housing").port("node"), heat.port("node")]);
    "#), &r, Map::new()).unwrap();
    let parts = sim_script::instances(&world);
    plan.apply(&mut world, &r, parts).unwrap();
    assert_eq!(world.behaviors.len(), 3);
    assert_eq!(world.connections.len(), 1);
    assert_eq!(world.connections[0].ports.len(), 3);
    let mut runtime = Runtime::new(world, &r, Integrator::BackwardEuler(Default::default())).unwrap();
    let temperature = runtime.across_id(housing.port("node"));
    runtime.advance(1., 0.01).unwrap();
    assert!((runtime.get(temperature) - 302.).abs() < 1e-8);
}

#[test]
fn binding_reuses_native_identity_and_validates_merged_parameters() {
    let r = registry();
    for (kind, params, second, expected) in [
        ("rotational.inertia", "damping:0.5", "", None),
        ("rotational.inertia", "inertia:-1.0", "", Some("inertia")),
        ("rotational.inertia", "dampning:0.5", "", Some("dampning")),
        ("rotational.ground", "", "", Some("expected rotational.ground")),
        ("rotational.inertia", "", "let duplicate = bind_component(\"again\",\"native\",\"rotational.inertia\",#{});", Some("bound more than once")),
    ] {
        let mut world = ModelWorld::default();
        let native = world.part(&r, "native", "rotational.inertia", [("inertia", 2.)]).unwrap();
        world.connect([native.port("shaft")]);
        let source = format!("let disk = bind_component(\"graph/disk\",\"native\",\"{kind}\",#{{{params}}}); {second}");
        let plan = evaluate(&Sources::single("binding.rhai", &source), &r, Map::new()).unwrap();
        let imports = sim_script::instances(&world);
        let result = plan.apply(&mut world, &r, imports);
        if let Some(message) = expected {
            let error = result.unwrap_err();
            assert!(error.contains(message) && error.contains("binding.rhai:1:"), "{error}");
        } else {
            assert_eq!(result.unwrap()["graph/disk"].behavior, native.behavior);
            assert_eq!(world.behaviors.len(), 1);
            assert_eq!(world.behaviors[native.behavior].parameters["inertia"].value_si, 2.);
            assert_eq!(world.behaviors[native.behavior].parameters["damping"].value_si, 0.5);
            Runtime::new(world, &r, Integrator::BackwardEuler(Default::default())).unwrap();
        }
    }
}

#[test]
fn repeated_port_is_a_source_diagnostic() {
    let r = registry();
    let plan = evaluate(&Sources::single("duplicate.rhai", r#"
        let disk = part("disk", "rotational.inertia", #{inertia:1.0});
        connect([disk.port("shaft"), disk.port("shaft")]);
    "#), &r, Map::new()).unwrap();
    let error = plan.apply(&mut ModelWorld::default(), &r, BTreeMap::new()).unwrap_err();
    assert!(error.contains("duplicate.rhai:3:") && error.contains("same port"), "{error}");
}

#[test]
fn captured_module_can_build_a_reusable_subsystem() {
    let sources=Sources {entry:"systems/main.rhai".into(),files:[
        ("systems/main.rhai".into(),"import \"parts\" as parts; let disk = parts::disk(\"disk\"); connect([disk.port(\"shaft\")]);".into()),
        ("systems/parts.rhai".into(),"fn disk(name) { part(name, \"rotational.inertia\", #{inertia: 1.0}) }".into()),
    ].into()};
    let plan = evaluate(&sources, &registry(), Map::new()).unwrap();
    assert_eq!(plan.components[0].name, "disk");
    assert_eq!(plan.components[0].location.source, "systems/parts.rhai");
}

#[test]
fn unknown_components_parameters_and_ports_report_sources() {
    for src in [
        "let p = part(\"bad\",\"missing\", #{});",
        "let p = part(\"bad\",\"rotational.inertia\", #{});",
    ] {
        let err = evaluate(&Sources::single("bad.rhai", src), &registry(), Map::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("line 1"), "{err}");
    }
    let p=evaluate(&Sources::single("port.rhai","let p = part(\"disk\",\"rotational.inertia\",#{inertia:1.0}); connect([p.port(\"wrong\")]);"),&registry(),Map::new()).unwrap();
    let err = p
        .apply(&mut ModelWorld::default(), &registry(), BTreeMap::new())
        .unwrap_err();
    assert!(
        err.contains("port.rhai") && err.contains("disk.wrong"),
        "{err}"
    );
}

fn contract() -> Contract {
    Contract {
        element: "controller".into(),
        period: 0.01,
        sensors: vec![Channel {
            name: "angle".into(),
            kind: QuantityKind::Angle,
        }],
        actuators: vec![Channel {
            name: "target".into(),
            kind: QuantityKind::Angle,
        }],
    }
}
#[test]
fn sampled_controller_has_named_channels_and_fresh_state_per_run() {
    let sources = Sources::single(
        "controller.rhai",
        r#"
        fn control(t, sensors, commands, state) {
            let count = if state.contains("count") {state.count} else {0};
            #{ commands: #{target: -2.0*sensors.angle + count*0.1}, state: #{count: count+1} }
        }
    "#,
    );
    for _ in 0..2 {
        let mut controller = RhaiController::new(sources.clone(), Map::new()).unwrap();
        controller.open(&contract()).unwrap();
        let mut out = [0.];
        controller.sample(0., &[0.2], &mut out).unwrap();
        assert!((out[0] + 0.4).abs() < 1e-12);
        controller.sample(0.01, &[0.2], &mut out).unwrap();
        assert!((out[0] + 0.3).abs() < 1e-12);
    }
}
#[test]
fn controller_rejects_wrong_channels_without_partial_output() {
    let sources = Sources::single(
        "controller.rhai",
        "fn control(t,s,a,state) { #{commands:#{wrong:1.0},state:#{}} }",
    );
    let mut controller = RhaiController::new(sources, Map::new()).unwrap();
    controller.open(&contract()).unwrap();
    let mut output = [42.];
    assert!(controller.sample(0., &[0.], &mut output).is_err());
    assert_eq!(output, [42.]);
}

#[test]
fn controller_keeps_captured_imports_and_constants_across_samples() {
    let sources = Sources { entry: "control/main.rhai".into(), files: [
        ("control/main.rhai".into(), "import \"helper\" as helper; const offset = 0.1; fn control(t,s,a,state) { let n = if state.contains(\"n\") { state.n } else { 0 }; #{commands: #{target: helper::value() + global::offset + n * 0.01}, state: #{n: n+1}} }".into()),
        ("control/helper.rhai".into(), "fn value() { 0.2 }".into())
    ].into() };
    for _ in 0..2 {
        let mut controller = RhaiController::new(sources.clone(), Map::new()).unwrap();
        controller.open(&contract()).unwrap();
        let mut output = [0.];
        for i in 0..10 {
            controller
                .sample(i as f64 * 0.01, &[0.], &mut output)
                .unwrap();
            assert!((output[0] - (0.3 + i as f64 * 0.01)).abs() < 1e-12);
        }
    }
}

#[test]
fn import_cycles_and_conflicting_configuration_are_diagnostics() {
    let sources = Sources {
        entry: "main.rhai".into(),
        files: [
            ("main.rhai".into(), "import \"a\" as a;".into()),
            ("a.rhai".into(), "import \"b\" as b;".into()),
            ("b.rhai".into(), "import \"a\" as a;".into()),
        ]
        .into(),
    };
    let error = evaluate(&sources, &registry(), Map::new())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("import cycle") && error.contains("a.rhai"),
        "{error}"
    );
    let error = evaluate(
        &Sources::single(
            "config.rhai",
            "configure(#{settings:#{step:0.01}}); configure(#{});",
        ),
        &registry(),
        Map::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("already declared") && error.contains("config.rhai"),
        "{error}"
    );
}

#[test]
fn full_robot_controller_commands_all_twelve_servos() {
    let source = include_str!("../../../examples/full-robot/controller.rhai");
    let mut controller = RhaiController::new(Sources::single("controller.rhai", source), Map::new()).unwrap();
    let mut c = contract();
    c.actuators = ["+X", "-Y", "+Y", "-X"].into_iter().flat_map(|leg| {
        ["Hip servo output", "Worm servo output", "Foot servo output"].into_iter().map(move |role| Channel {
            name: format!("{leg} | {role}.target"), kind: QuantityKind::Angle,
        })
    }).collect();
    controller.open(&c).unwrap();
    let mut commands = [1.0; 12];
    controller.sample(0., &[0.], &mut commands).unwrap();
    assert_eq!(commands, [0.; 12]);
}
