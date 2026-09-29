//! Snapping: typed suggestions per port, curated ranking, structural
//! conflicts, and a working drive assembled only by snapping.
use sim_core::BehaviorRegistry;
use sim_system::snap::{self, Candidate};
use sim_system::*;

fn registry() -> BehaviorRegistry {
    let mut r = BehaviorRegistry::default();
    sim_domain_electrical::elements::register(&mut r).unwrap();
    sim_domain_thermal::register(&mut r).unwrap();
    sim_domain_rotational::elements::register(&mut r).unwrap();
    sim_domain_bridges::elements::register(&mut r).unwrap();
    r
}

fn place(doc: &mut SystemDocument, r: &BehaviorRegistry, name: &str, component_type: &str) {
    let spec = snap::starter(r, &InstanceKind::Element { component_type: component_type.into() }, name);
    apply(doc, r, &[Command::AddInstance { at: String::new(), name: name.into(), instance: spec }]).unwrap();
}

fn candidate(doc: &SystemDocument, r: &BehaviorRegistry, instance: &str, port: &str, component_type: &str) -> Candidate {
    let all = snap::suggestions(doc, r, None, "", instance).unwrap();
    let p = all.iter().find(|p| p.port == port).unwrap_or_else(|| panic!("{instance} has no port {port}"));
    p.candidates.iter().find(|c| c.kind == InstanceKind::Element { component_type: component_type.into() }).unwrap_or_else(|| panic!("{component_type} not offered on {instance}.{port}")).clone()
}

fn attach(doc: &mut SystemDocument, r: &BehaviorRegistry, instance: &str, port: &str, component_type: &str, name: &str) {
    let c = candidate(doc, r, instance, port, component_type);
    let commands = snap::snap(doc, r, "", instance, port, &c, name).unwrap();
    apply(doc, r, &commands).unwrap();
}

#[test]
fn suggestions_are_typed_and_curated() {
    let r = registry();
    let mut doc = SystemDocument::new("motor");
    place(&mut doc, &r, "motor", "bridge.brushed_motor");
    let all = snap::suggestions(&doc, &r, None, "", "motor").unwrap();
    let shaft = all.iter().find(|p| p.port == "shaft").unwrap();
    // Only rotational parts attach to a shaft; curated companions lead.
    for c in &shaft.candidates {
        if let InstanceKind::Element { component_type } = &c.kind {
            let ports = snap::element_port_types(&r, component_type);
            assert!(ports[&c.port].contains("rotational"), "{component_type}.{} offered on a shaft", c.port);
        }
    }
    let first: Vec<&str> = shaft.candidates.iter().take(3).map(|c| c.label.as_str()).collect();
    assert_eq!(first, ["Rotational inertia", "Worm gear", "Ideal gear"], "curated order from the motor notes");
    assert!(shaft.candidates.iter().any(|c| c.label == "Lead screw" && c.port == "screw"));
    let pin = all.iter().find(|p| p.port == "p").unwrap();
    assert!(pin.candidates.iter().any(|c| c.label == "Switched voltage supply" && c.recommended));
    assert!(pin.candidates.iter().all(|c| c.label != "Worm gear"));

    // A saved worm gearbox is recommended on a motor shaft because it
    // contains a worm gear, a companion the motor's notes name.
    let mut gearbox = Definition::new("Worm gearbox");
    gearbox.ports.insert("input".into(), BoundaryPort::default());
    gearbox.instances.insert("mesh".into(), InstanceSpec::element("rotational.worm_gear"));
    gearbox.nets.push(Net { label: String::new(), terminals: vec![Terminal::boundary("input"), Terminal::port("mesh", "worm")] });
    doc.definitions.insert("worm_box".into(), gearbox);
    let mut sink = Definition::new("Resistor pack");
    sink.ports.insert("a".into(), BoundaryPort::default());
    sink.instances.insert("r".into(), InstanceSpec::element("electrical.resistor").with("resistance", 1.0));
    sink.nets.push(Net { label: String::new(), terminals: vec![Terminal::boundary("a"), Terminal::port("r", "p")] });
    doc.definitions.insert("pack".into(), sink);
    let all = snap::suggestions(&doc, &r, None, "", "motor").unwrap();
    let find = |port: &str, label: &str| all.iter().find(|p| p.port == port).unwrap().candidates.iter().find(|c| c.label == label).cloned();
    assert!(find("shaft", "Worm gearbox").is_some_and(|c| c.recommended));
    // It fits the motor's pin, but nothing in it is a named companion.
    assert!(find("p", "Resistor pack").is_some_and(|c| !c.recommended));
}

#[test]
fn a_second_rigid_body_on_a_shaft_is_flagged_with_the_fix() {
    let r = registry();
    let mut doc = SystemDocument::new("motor");
    place(&mut doc, &r, "motor", "bridge.brushed_motor");
    attach(&mut doc, &r, "motor", "shaft", "rotational.inertia", "rotor");
    let again = candidate(&doc, &r, "motor", "shaft", "rotational.inertia");
    assert!(again.conflict.as_deref().is_some_and(|c| c.contains("rotor") && c.contains("coupling")), "{:?}", again.conflict);
    assert!(candidate(&doc, &r, "motor", "shaft", "rotational.worm_gear").conflict.is_none());
    // Conflicting candidates sink to the bottom.
    let shaft = snap::suggestions(&doc, &r, None, "", "motor").unwrap().into_iter().find(|p| p.port == "shaft").unwrap();
    let first_conflict = shaft.candidates.iter().position(|c| c.conflict.is_some()).unwrap();
    assert!(shaft.candidates[first_conflict..].iter().all(|c| c.conflict.is_some()));
}

#[test]
fn a_winch_built_only_by_snapping_compiles_and_runs() {
    let r = registry();
    let mut doc = SystemDocument::new("snapped winch");
    place(&mut doc, &r, "motor", "bridge.brushed_motor");
    attach(&mut doc, &r, "motor", "shaft", "rotational.inertia", "rotor");
    attach(&mut doc, &r, "motor", "case", "rotational.ground", "frame");
    attach(&mut doc, &r, "motor", "p", "electrical.switched_voltage_source", "supply");
    attach(&mut doc, &r, "motor", "n", "electrical.ground", "gnd");
    // The supply's other pin joins the grounded net.
    apply(&mut doc, &r, &[Command::Connect { at: String::new(), terminals: vec![Terminal::port("supply", "n"), Terminal::port("gnd", "pin")], label: String::new() }]).unwrap();
    attach(&mut doc, &r, "motor", "shaft", "rotational.worm_gear", "worm");
    attach(&mut doc, &r, "worm", "wheel", "rotational.inertia", "drum");
    attach(&mut doc, &r, "drum", "shaft", "rotational.load_torque", "load");

    // Typical values were applied and recorded as estimates.
    let motor = &doc.definitions["root"].instances["motor"];
    assert!(matches!(&motor.parameters["resistance"], ParameterBinding::Value { provenance: Some(sim_inspect::Provenance::Estimated { .. }), .. }));
    // Shaft parts continue coaxially along the motor's axis, not on top of it.
    let (m, w) = (doc.definitions["root"].instances["motor"].placement.position, doc.definitions["root"].instances["worm"].placement.position);
    assert!(m != w);

    assert!(Resolver::new(&doc, &r).findings().is_empty(), "{:?}", Resolver::new(&doc, &r).findings());
    let flat = flatten(&doc, &r).unwrap();
    let mut runtime = sim_compile::Runtime::new(flat.model, &r, sim_dynamics::Integrator::BackwardEuler(Default::default())).unwrap();
    runtime.advance(0.2, 1e-3).unwrap();
}
