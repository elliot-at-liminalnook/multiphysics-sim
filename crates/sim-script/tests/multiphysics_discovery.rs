use rhai::Map;
use sim_core::{BehaviorRegistry, ConnectorKind};
use sim_script::{Sources, catalogue, evaluate};

fn registry() -> BehaviorRegistry {
    let mut r = BehaviorRegistry::default();
    sim_domain_hydraulic::register(&mut r).unwrap();
    sim_domain_chemical::register(&mut r).unwrap();
    sim_domain_radiative::register(&mut r).unwrap();
    sim_domain_granular::register(&mut r).unwrap();
    sim_domain_magnetic::register(&mut r).unwrap();
    r
}

#[test]
fn multibody_schemas_expose_physical_units_and_reject_invalid_mechanics() {
    let mut r = BehaviorRegistry::default();
    sim_domain_multibody::elements::register(&mut r).unwrap();
    let catalog = catalogue(&r);
    let components = catalog.as_array().unwrap();
    assert_eq!(components.len(), 6);
    assert!(components.iter().all(|c| c["parameters_complete"] == true));
    for (kind, parameter, unit) in [
        ("multibody.rigid_body", "ixx", "kg·m²"),
        ("multibody.sphere_contact", "regularisation", "m/s"),
        ("multibody.compass_walker", "time_scale", "s"),
        ("multibody.driven_pendulum", "gravity", "m/s²"),
        ("multibody.pendulum_on_cart", "escapement_kick", "rad/s"),
        ("multibody.pitch_plunge_section", "unbalance", "kg·m"),
    ] {
        let component = components.iter().find(|c| c["type"] == kind).unwrap();
        assert!(component["parameters"].as_array().unwrap().iter().any(|p| p["name"] == parameter && p["unit"] == unit));
    }
    let pendulum = components.iter().find(|c| c["type"] == "multibody.driven_pendulum").unwrap();
    assert_eq!(pendulum["ports"][0]["unit"], "m/s²");
    for (kind, params, expected) in [
        ("multibody.rigid_body", "mass:1, ixx:0, iyy:1, izz:1", "ixx"),
        ("multibody.sphere_contact", "radius:0.1, stiffness:1000, regularisation:0", "regularisation"),
        ("multibody.compass_walker", "slope:0.01, time_scale:0", "time_scale"),
        ("multibody.compass_walker", "slope:0.01, elastic:0.2", "elastic"),
        ("multibody.driven_pendulum", "length:0", "length"),
        ("multibody.pendulum_on_cart", "mass:1, length:1, damping:-1", "damping"),
        ("multibody.pitch_plunge_section", "mass:1, pitch_inertia:1, unbalance:1, plunge_stiffness:1, pitch_stiffness:1", "positive kinetic energy"),
    ] {
        let source = format!("\nlet element = part(\"element\", \"{kind}\", #{{{params}}});");
        let error = evaluate(&Sources::single("mechanics.rhai", &source), &r, Map::new()).unwrap_err().to_string();
        assert!(error.contains("mechanics.rhai") && error.contains("line 2") && error.contains(expected), "{error}");
    }
}

#[test]
fn walker_time_scale_preserves_the_dimensionless_trajectory_in_physical_seconds() {
    let mut r = BehaviorRegistry::default();
    sim_domain_multibody::elements::register(&mut r).unwrap();
    let mut endpoints = Vec::new();
    for scale in [1., 2.] {
        let source = format!("let walker = part(\"walker\", \"multibody.compass_walker\", #{{slope:0.01, time_scale:{scale}}});");
        let plan = evaluate(&Sources::single("walker.rhai", &source), &r, Map::new()).unwrap();
        let mut model = sim_core::ModelWorld::default();
        let parts = plan.apply(&mut model, &r, Default::default()).unwrap();
        let mut runtime = sim_compile::Runtime::new(model, &r, sim_dynamics::Integrator::implicit_midpoint()).unwrap();
        runtime.advance(0.2 * scale, 0.001 * scale).unwrap();
        let state: Vec<_> = ["theta", "phi", "theta_dot", "phi_dot"].iter().enumerate().map(|(i, name)| {
            let id = runtime.state_id(parts["walker"].behavior, name);
            let quantity = runtime.model.state.iter().find(|(key, _)| *key == id).unwrap().1.quantity;
            assert_eq!(quantity.unit(), if i < 2 { "rad" } else { "rad/s" });
            runtime.get(id) * if i < 2 { 1. } else { scale }
        }).collect();
        endpoints.push(state);
    }
    for (a, b) in endpoints[0].iter().zip(&endpoints[1]) { assert!((a-b).abs() < 1e-9, "{a} vs {b}"); }
}

#[test]
fn chain_dynamic_members_validate_indices_and_keep_derived_defaults_explicit() {
    let mut r = BehaviorRegistry::default();
    sim_domain_multibody::chain::register(&mut r).unwrap();
    let catalog = catalogue(&r);
    assert_eq!(catalog[0]["parameters_complete"], true);
    assert!(catalog[0]["parameters"].as_array().unwrap().iter().any(|p|
        p["name"] == "link*.inertia" && p["unit"] == "kg·m²" && p["default_label"] == "link mass * length² / 12"));
    let params = "\"joint.elbow\":0, \"link0.mass\":2, \"link0.length\":0.5";
    for addition in ["", ", \"initial.joint.elbow.speed\":2", ", \"link0.com\":0.1, \"link0.inertia\":0.02"] {
        let source = format!("let arm = part(\"arm\", \"multibody.chain\", #{{{params}{addition}}});");
        evaluate(&Sources::single("chain.rhai", &source), &r, Map::new()).unwrap();
    }
    for (params, expected) in [
        (params.replace("elbow\":0", "elbow\":0.5"), "joint.elbow"),
        (params.replace("elbow\":0", "elbow\":1"), "chain order"),
        (format!("{params}, \"joint.wrist\":0"), "chain order"),
        (format!("{params}, \"link1.mass\":1"), "link index"),
        (format!("{params}, \"link00.com\":0.1"), "link index"),
        (format!("{params}, \"initial.joint.missing.angle\":0.1"), "undeclared port"),
        (format!("{params}, \"link0.inertia\":-1"), "link0.inertia"),
    ] {
        let source = format!("\nlet arm = part(\"arm\", \"multibody.chain\", #{{{params}}});");
        let error = evaluate(&Sources::single("chain.rhai", &source), &r, Map::new()).unwrap_err().to_string();
        assert!(error.contains("chain.rhai") && error.contains("line 2") && error.contains(expected), "{error}");
    }
}

#[test]
fn sensor_catalogue_and_rhai_validation_share_channel_units_and_constraints() {
    let mut r = BehaviorRegistry::default();
    sim_domain_sensing::register(&mut r).unwrap();
    let catalog = catalogue(&r);
    let components = catalog.as_array().unwrap();
    assert_eq!(components.len(), 9);
    assert!(components.iter().all(|c| c["parameters_complete"] == true));
    let imu = components.iter().find(|c| c["type"] == "sensor.imu").unwrap();
    assert!(imu["ports"].as_array().unwrap().iter().any(|p| p["name"] == "ax" && p["unit"] == "m/s²"));
    assert!(imu["parameters"].as_array().unwrap().iter().any(|p| p["name"] == "noise.gyro" && p["unit"] == "rad/s"));
    evaluate(&Sources::single("sensors.rhai", r#"
        let reading = part("reading", "sensor.imu", #{period:0.01, "noise.ax":0.1, "quantum.gyro":0.002, seed:17});
    "#), &r, Map::new()).unwrap();
    for (kind, parameters, expected) in [
        ("sensor.imu", "period:0.01, noise:0.1", "noise"),
        ("sensor.imu", "period:0.01, \"quantum.gyro\":-0.2", "quantum.gyro"),
        ("sensor.tachometer", "noise:0.2", "period > 0"),
        ("sensor.encoder", "counts:1024", "period > 0"),
        ("actuator.servo", "bandwidth:50, torque_constant:0", "torque_constant"),
        ("actuator.quantiser", "step:0", "step"),
    ] {
        let source = format!("\nlet reading = part(\"reading\", \"{kind}\", #{{{parameters}}});");
        let error = evaluate(&Sources::single("sensors.rhai", &source), &r, Map::new()).unwrap_err().to_string();
        assert!(error.contains("sensors.rhai") && error.contains("line 2") && error.contains("reading") && error.contains(expected), "{error}");
    }
}

#[test]
fn planar_contact_discovery_checks_units_geometry_and_dynamic_patch_members() {
    let mut r = BehaviorRegistry::default();
    sim_domain_multibody::planar::register(&mut r).unwrap();
    sim_domain_multibody::contact::register(&mut r).unwrap();
    let catalog = catalogue(&r);
    let components = catalog.as_array().unwrap();
    assert_eq!(components.len(), 16);
    assert!(components.iter().all(|c| c["parameters_complete"] == true));
    for (kind, name, unit) in [
        ("planar.bend", "stiffness", "J"), ("planar.rod", "stiffness", "N/m"),
        ("joint.revolute", "stabilisation", "1/s"), ("joint.fixed", "offset", "rad"),
        ("planar.rigid_body", "inertia", "kg·m²"), ("planar.drag", "coefficient", "kg/m"),
        ("contact.point_plane", "friction_scale", "N·s/m"), ("contact.wheel", "initial.spin", "rad/s"),
        ("contact.point_terrain_compliant", "patch*.x0", "m"),
        ("contact.point_plane_compliant", "regularisation", "m/s"),
    ] {
        let c = components.iter().find(|c| c["type"] == kind).unwrap();
        assert!(c["parameters"].as_array().unwrap().iter().any(|p| p["name"] == name && p["unit"] == unit), "{kind}.{name}");
    }
    for (kind, params) in [
        ("contact.point_terrain_compliant", "stiffness:10000, patches:1, \"patch0.x0\":-1, \"patch0.x1\":2"),
        ("joint.prismatic", "ux:0.6, uy:0.8"), ("planar.drag", "coefficient:0.5"),
        ("planar.drag", "density:1.2, cd:0.5, area:0.1"),
    ] {
        let source = format!("let element = part(\"element\", \"{kind}\", #{{{params}}});");
        evaluate(&Sources::single("mechanics.rhai", &source), &r, Map::new()).unwrap();
    }
    for (kind, params, expected) in [
        ("contact.point_terrain_compliant", "stiffness:10000, patches:0, \"patch0.x0\":0", "patch index"),
        ("contact.point_terrain_compliant", "stiffness:10000, patches:1, \"patch0.x0\":2, \"patch0.x1\":1", "patch end"),
        ("contact.point_terrain_compliant", "stiffness:10000, patches:1, \"patch0.x0\":0", "patch0.x1"),
        ("contact.point_terrain_compliant", "stiffness:10000, patches:1, \"patch01.x0\":0", "patch index"),
        ("contact.point_terrain_compliant", "stiffness:10000, patches:1.5", "patches"),
        ("contact.point_plane_compliant", "stiffness:10000, regularisation:0", "regularisation"),
        ("joint.prismatic", "ux:0, uy:0", "unit length"),
        ("planar.drag", "coefficient:0.5, area:1", "not both"),
        ("planar.drag", "cd:0.5", "area"),
        ("planar.point_mass", "mass:0", "mass"),
        ("planar.rigid_body", "mass:1, inertia:0", "inertia"),
    ] {
        let source = format!("\nlet element = part(\"element\", \"{kind}\", #{{{params}}});");
        let error = evaluate(&Sources::single("mechanics.rhai", &source), &r, Map::new()).unwrap_err().to_string();
        assert!(error.contains("mechanics.rhai") && error.contains("line 2") && error.contains(expected), "{error}");
    }
}

#[test]
fn physical_channel_units_survive_registry_discovery() {
    let catalogue = catalogue(&registry());
    let components = catalogue.as_array().unwrap();
    assert_eq!(components.len(), 24);
    assert!(components.iter().all(|c| c["parameters_complete"] == true));
    for (component, across, through) in [
        ("chem.species", "J/mol", "mol/s"),
        ("radiation.surface", "W/m²", "W"),
        ("granular.column", "Pa", "kg/s"),
        ("hydraulic.volume", "Pa", "m³/s"),
        ("magnetic.reluctance", "A", "V"),
    ] {
        let c = components.iter().find(|c| c["type"] == component).unwrap();
        assert_eq!(
            c["ports"][0]["lanes"][0]["across_unit"], across,
            "{component}"
        );
        assert_eq!(
            c["ports"][0]["lanes"][0]["through_unit"], through,
            "{component}"
        );
    }
    // The compiler's lane declarations and legacy scalar schema must agree.
    for kind in [
        ConnectorKind::FluidPh,
        ConnectorKind::Chemical,
        ConnectorKind::Radiative,
        ConnectorKind::Granular,
    ] {
        assert_eq!(kind.schema().across, kind.lanes()[0].across_kind);
        assert_eq!(kind.schema().through, kind.lanes()[0].through_kind);
    }
    assert_eq!(ConnectorKind::FluidPh.lanes()[1].across_kind.unit(), "J/kg");
    let surface = components
        .iter()
        .find(|c| c["type"] == "radiation.surface")
        .unwrap();
    assert!(
        surface["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "band_hi" && p["unit"] == "µm")
    );
}

#[test]
fn script_validation_catches_physical_parameter_mistakes_at_source() {
    let r = registry();
    for (kind, parameters, bad) in [
        (
            "hydraulic.valve",
            "conductance:1.0, closure_time:1.0, inertance:1.0",
            "floor:2.0",
        ),
        ("chem.species", "volume:1.0", "reference:0.0"),
        (
            "radiation.surface",
            "area:1.0, emissivity:0.5",
            "band_hii:13.0",
        ),
        (
            "granular.column",
            "diameter:1.0, density:1000.0",
            "\"initial.mass\":-1.0",
        ),
        (
            "magnetic.reluctance",
            "reluctance:1000.0",
            "initial_flux:0.2",
        ),
    ] {
        let valid = format!("let element = part(\"element\", \"{kind}\", #{{{parameters}}});");
        evaluate(&Sources::single("units.rhai", &valid), &r, Map::new()).unwrap();
        let invalid =
            format!("\nlet element = part(\"element\", \"{kind}\", #{{{parameters}, {bad}}});");
        let error = evaluate(&Sources::single("units.rhai", &invalid), &r, Map::new())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("units.rhai") && error.contains("line 2") && error.contains("element"),
            "{error}"
        );
    }
    // Geometry-derived reluctance is an explicit alternative to a supplied value.
    evaluate(
        &Sources::single(
            "geometry.rhai",
            "let core = part(\"core\", \"magnetic.reluctance\", #{length:0.1, area:0.001});",
        ),
        &r,
        Map::new(),
    )
    .unwrap();
}

#[test]
fn composite_motor_initial_temperature_reaches_the_native_node() {
    let mut r = registry();
    sim_domain_bridges::elements::register(&mut r).unwrap();
    sim_domain_thermal::register(&mut r).unwrap();
    sim_domain_rotational::elements::register(&mut r).unwrap();
    let source = r#"
        let motor = part("motor", "bridge.motor", #{resistance:2.0, torque_constant:0.1,
            "initial.plug.thermal.temperature":310.0});
        let housing = part("housing", "thermal.capacitance", #{heat_capacity:20.0});
        let shaft = part("shaft", "rotational.inertia", #{inertia:1.0});
        connect([motor.port("plug.thermal"), housing.port("node")]);
        connect([motor.port("plug.rotational"), shaft.port("shaft")]);
        connect([motor.port("plug.electrical")]);
    "#;
    let plan = evaluate(&Sources::single("motor.rhai", source), &r, Map::new()).unwrap();
    let mut model = sim_core::ModelWorld::default();
    plan.apply(&mut model, &r, Default::default()).unwrap();
    let mut runtime = sim_compile::Runtime::new(
        model,
        &r,
        sim_dynamics::Integrator::BackwardEuler(Default::default()),
    )
    .unwrap();
    // The supplied value must be consumed, rather than merely allowed by validation.
    assert!(runtime.model.state.iter().any(|(_, state)| state.quantity
        == sim_core::QuantityKind::Temperature
        && (state.committed - 310.0).abs() < 1e-12));
    runtime.advance(0.1, 0.01).unwrap();
    for invalid_name in ["initial.plug.temperature", "initial.temperature"] {
        let invalid = source.replace("initial.plug.thermal.temperature", invalid_name);
        assert!(evaluate(&Sources::single("motor.rhai", &invalid), &r, Map::new()).is_err());
    }
}

#[test]
fn extending_native_nets_preserves_composite_members_and_compiler_checks() {
    let mut r = registry();
    sim_domain_bridges::elements::register(&mut r).unwrap();
    sim_domain_thermal::register(&mut r).unwrap();
    sim_domain_control::elements::register(&mut r).unwrap();
    let mut world = sim_core::ModelWorld::default();
    let motor = world.part(&r, "motor", "bridge.motor", [("resistance", 2.), ("torque_constant", 0.1)]).unwrap();
    world.connect([motor.port("plug")]);
    let original: Vec<_> = world.connections.iter().map(|c| c.ports.clone()).collect();
    let plan = evaluate(&Sources::single("composite.rhai", r#"
        let housing = part("housing", "thermal.capacitance", #{heat_capacity:20.0});
        connect([component("motor").port("plug.thermal"), housing.port("node")]);
    "#), &r, Map::new()).unwrap();
    let parts = sim_script::instances(&world);
    plan.apply(&mut world, &r, parts).unwrap();
    assert_eq!(world.connections.len(), 3);
    for ports in original {
        let connection = world.connections.iter().find(|c| c.ports.contains(&ports[0])).unwrap();
        if ports[0] == motor.port("plug.thermal") { assert_eq!(connection.ports.len(), 2); }
        else { assert_eq!(connection.ports, ports); }
    }
    sim_compile::compile(&world, &r).unwrap();
    for physical in [false, true] {
        let mut world = sim_core::ModelWorld::default();
        let source = world.part(&r, "source", "control.constant", [("value", 1.)]).unwrap();
        world.connect([source.port("value")]);
        let code = if physical {
            "let p = part(\"added\",\"thermal.capacitance\",#{heat_capacity:1.0}); connect([component(\"source\").port(\"value\"),p.port(\"node\")]);"
        } else {
            "let p = part(\"added\",\"control.constant\",#{value:2.0}); connect([component(\"source\").port(\"value\"),p.port(\"value\")]);"
        };
        let plan = evaluate(&Sources::single("invalid.rhai", code), &r, Map::new()).unwrap();
        let parts = sim_script::instances(&world);
        plan.apply(&mut world, &r, parts).unwrap();
        let error = sim_compile::compile(&world, &r).unwrap_err();
        if physical { assert!(matches!(error, sim_compile::CompileError::IncompatibleConnection { .. })); }
        else { assert!(matches!(error, sim_compile::CompileError::SignalOutputCount { .. })); }
    }
}

#[test]
fn fluid_storage_exposes_mass_and_specific_enthalpy() {
    let mut r = BehaviorRegistry::default();
    sim_domain_fluid::twophase::register(&mut r).unwrap();
    let plan = evaluate(
        &Sources::single(
            "fluid.rhai",
            r#"
        let vessel = part("vessel", "fluid.volume_ph", #{volume:0.01});
        connect([vessel.port("node")]);
    "#,
        ),
        &r,
        Map::new(),
    )
    .unwrap();
    let mut model = sim_core::ModelWorld::default();
    let parts = plan.apply(&mut model, &r, Default::default()).unwrap();
    let runtime = sim_compile::Runtime::new(
        model,
        &r,
        sim_dynamics::Integrator::BackwardEuler(Default::default()),
    )
    .unwrap();
    // The water EOS smooths phase boundaries; at 20 °C that changes the
    // liquid-density estimate by less than 0.1 g for this ten-litre vessel.
    for (name, kind, value, tolerance) in [
        ("mass", sim_core::QuantityKind::Mass, 9.92, 1e-4),
        (
            "enthalpy",
            sim_core::QuantityKind::SpecificEnthalpy,
            83720.0,
            1e-8,
        ),
    ] {
        let id = runtime.state_id(parts["vessel"].behavior, name);
        let state = runtime
            .model
            .state
            .iter()
            .find(|(key, _)| *key == id)
            .unwrap()
            .1;
        assert_eq!(state.quantity, kind);
        assert!(
            (state.committed - value).abs() < tolerance,
            "{name}: {}",
            state.committed
        );
    }
}

#[test]
fn dynamic_ports_validate_members_positions_and_integer_settings() {
    let mut r = BehaviorRegistry::default();
    sim_domain_line::register(&mut r).unwrap();
    sim_domain_control::external::register(&mut r).unwrap();
    let source = r#"
        let string = part("string", "line.string", #{length:1.0, tension:10.0,
            mass_per_length:0.01, "tap.middle":0.5, "initial.tap.middle.position":0.01});
        let controller = part("controller", "control.external", #{period:0.01,
            "sense.position":0.0, "act.force":0.0});
    "#;
    let plan = evaluate(&Sources::single("ports.rhai", source), &r, Map::new()).unwrap();
    let mut model = sim_core::ModelWorld::default();
    let parts = plan.apply(&mut model, &r, Default::default()).unwrap();
    assert!(parts["string"].try_port("tap.middle").is_some());
    assert!(parts["controller"].try_port("sense.position").is_some());
    assert!(parts["controller"].try_port("act.force").is_some());
    for invalid in [
        source.replace(
            "initial.tap.middle.position",
            "initial.tap.missing.position",
        ),
        source.replace("\"tap.middle\":0.5", "\"tap.middle\":1.5"),
        source.replace("period:0.01", "period:0.01, input_delay:0.5"),
        source.replace("length:1.0", "length:1.0, cells:0"),
    ] {
        let error = evaluate(&Sources::single("ports.rhai", &invalid), &r, Map::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("ports.rhai"), "{error}");
    }
}

#[test]
fn normalized_acoustics_cannot_silently_join_a_physical_port() {
    let mut r = BehaviorRegistry::default();
    sim_domain_acoustic::register(&mut r).unwrap();
    sim_domain_hydraulic::register(&mut r).unwrap();
    // The same compliance equation can terminate a physical acoustic port.
    let mut physical_descriptor = r.get(&"hydraulic.volume".into()).unwrap().clone();
    physical_descriptor.type_id = "test.physical_acoustic".into();
    physical_descriptor.ports[0].schema = sim_core::PortSchema::Acausal(ConnectorKind::Acoustic);
    r.register(physical_descriptor).unwrap();
    let mut model = sim_core::ModelWorld::default();
    let duct = model
        .part(&r, "duct", "acoustic.duct_modes", [("tap", 0.25)])
        .unwrap();
    let physical = model
        .part(
            &r,
            "physical",
            "test.physical_acoustic",
            [("compliance", 1.0)],
        )
        .unwrap();
    model.connect([duct.port("tap"), physical.port("port")]);
    model.connect([duct.port("velocity")]);
    assert_eq!(
        sim_compile::compile(&model, &r).unwrap_err(),
        sim_compile::CompileError::IncompatibleConnection { connection: 0 }
    );
    let catalogue = catalogue(&r);
    let duct = catalogue
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == "acoustic.duct_modes")
        .unwrap();
    assert_eq!(duct["ports"][0]["lanes"][0]["across_unit"], "1");
    assert_eq!(duct["ports"][0]["lanes"][0]["through_unit"], "1");
}
