//! The roadmap's end-to-end test, scripted and timed: write a new motor
//! variant as equations (no Rust recompile), get its datasheet, wrap it into
//! a gearmotor, snap that into a winch, compare it with the brushed motor,
//! sweep the magnet strength, and publish the chosen design to a library.
//! Each step's tool time is printed; the whole loop must stay well inside
//! the "idea to first result < 2 min" and "new part to library < 10 min"
//! targets (the remaining time is the person's).
use sim_runtime::{bench, system_builder, system_study};
use sim_system::{library, snap, Command, InstanceKind, InstanceSpec, StudyKind, SystemDocument, Terminal};
use std::time::Instant;

const MOTOR: &str = r#"
part flux_motor "Flux-tuned DC motor"
summary "A brushed motor whose magnet strength is a design parameter."
explain "Torque and back-EMF constants scale with the magnet's remanence relative to a reference magnet."
pairs rotational.inertia rotational.worm_gear rotational.lossy_gear electrical.voltage_source electrical.ground rotational.ground
port p electrical
port n electrical
port shaft rotational
port case rotational
param resistance Ω ~ 2.0 "Winding resistance"
param k_ref N·m/A ~ 0.012 "Motor constant with the reference magnet"
param magnet 1 = 1.0 "Remanence relative to the reference magnet"
param inductance H = 0.0003 "Winding inductance"
realtime inductance = 0
state i Current = 0
let k = k_ref*magnet
let w = shaft.w - case.w
eq inductance*der(i) = p.v - n.v - resistance*i - k*w
flow p = i
flow n = -i
flow shaft = -k*i
flow case = k*i
energy 0.5*inductance*i^2
derive "stall torque per volt" = k_ref*magnet/resistance N·m/V
"#;

#[test]
fn a_new_motor_from_idea_to_library() {
    let started = Instant::now();
    let mut step = Instant::now();
    let mut lap = |what: &str| {
        eprintln!("{what}: {:.2} s", step.elapsed().as_secs_f64());
        step = Instant::now();
    };
    let mut registry = sim_runtime::registry();

    // 1. Write the part (text, no recompile) and load it.
    let id = sim_parts::register(&mut registry, sim_parts::parse("flux_motor.part", MOTOR).unwrap()).unwrap();
    lap("1 write and load the part");

    // 2. Its datasheet: bench, energy audit, step convergence, realtime model.
    let sheet = bench::datasheet(&registry, &id).unwrap();
    assert_eq!(sheet.kind, "motor");
    assert!(sheet.passed(), "{:?}", sheet.checks);
    lap("2 datasheet");

    // 3. A gearmotor built from it (and the library's worm gear) by snapping.
    let mut doc = SystemDocument::new("Flux winch");
    let place = |doc: &mut SystemDocument, name: &str, t: &str, r: &sim_core::BehaviorRegistry| {
        let spec = snap::starter(r, &InstanceKind::Element { component_type: t.into() }, name);
        sim_system::apply(doc, r, &[Command::AddInstance { at: String::new(), name: name.into(), instance: spec }]).unwrap();
    };
    let attach = |doc: &mut SystemDocument, from: &str, port: &str, t: &str, name: &str, r: &sim_core::BehaviorRegistry| {
        let c = snap::suggestions(doc, r, None, "", from).unwrap().into_iter().find(|p| p.port == port).unwrap().candidates.into_iter().find(|c| c.kind == InstanceKind::Element { component_type: t.into() }).unwrap_or_else(|| panic!("{t} not offered on {from}.{port}"));
        let commands = snap::snap(doc, r, "", from, port, &c, name).unwrap();
        sim_system::apply(doc, r, &commands).unwrap();
    };
    place(&mut doc, "motor", &id, &registry);
    attach(&mut doc, "motor", "shaft", "rotational.inertia", "rotor", &registry);
    attach(&mut doc, "motor", "case", "rotational.ground", "frame", &registry);
    attach(&mut doc, "motor", "shaft", "rotational.worm_gear", "worm", &registry);
    sim_system::apply(&mut doc, &registry, &[Command::Group { at: String::new(), instances: vec!["motor".into(), "rotor".into(), "frame".into(), "worm".into()], name: "gearmotor".into(), definition: "flux_gearmotor".into(), label: "Flux gearmotor".into() }]).unwrap();
    // Its outside: supply pins and the output shaft.
    let boundary = |name: &str, inner: (&str, &str)| Command::AddBoundaryPort { at: "gearmotor".into(), name: name.into(), port: Default::default(), connect: Some(Terminal::port(inner.0, inner.1)) };
    sim_system::apply(&mut doc, &registry, &[boundary("p", ("motor", "p")), boundary("n", ("motor", "n")), boundary("output", ("worm", "wheel"))]).unwrap();
    lap("3 gearmotor from the new motor");

    // 4. Snap it into a winch: supply, drum, load.
    sim_system::apply(&mut doc, &registry, &[
        Command::AddInstance { at: String::new(), name: "supply".into(), instance: InstanceSpec::element("electrical.switched_voltage_source").with("voltage", 12.0).with("off_at", 1.2) },
        Command::AddInstance { at: String::new(), name: "gnd".into(), instance: InstanceSpec::element("electrical.ground") },
        Command::AddInstance { at: String::new(), name: "drum".into(), instance: InstanceSpec::element("rotational.inertia").with("inertia", 2.1e-4) },
        Command::AddInstance { at: String::new(), name: "weight".into(), instance: InstanceSpec::element("rotational.load_torque").with("torque", -0.19613) },
    ]).unwrap();
    let port = |name: &str| name.to_string();
    sim_system::apply(&mut doc, &registry, &[
        Command::Connect { at: String::new(), terminals: vec![Terminal::port("supply", "p"), Terminal::port("gearmotor", &port("p"))], label: String::new() },
        Command::Connect { at: String::new(), terminals: vec![Terminal::port("supply", "n"), Terminal::port("gearmotor", &port("n")), Terminal::port("gnd", "pin")], label: String::new() },
        Command::Connect { at: String::new(), terminals: vec![Terminal::port("gearmotor", &port("output")), Terminal::port("drum", "shaft"), Terminal::port("weight", "shaft")], label: String::new() },
    ]).unwrap();
    doc.run = Some(sim_system::RunSettings { integrator: sim_system::IntegratorChoice::BackwardEuler, interval: 5e-4, absolute_tolerance: None, relative_tolerance: None, max_iterations: None, rationale: "as the worm-drive winch".into() });
    let first = system_builder::simulate(&doc, &registry, 1.0, system_builder::config_for(&doc), &["drum.shaft.speed".into()]).unwrap();
    let lift = first[0].values.last().copied().unwrap();
    assert!(lift > 10.0, "the winch lifts: {lift} rad/s");
    lap("4 into the winch, first result");
    let idea_to_result = started.elapsed().as_secs_f64();

    // 5. Compare with the brushed motor (same ports: a swap).
    doc.studies.insert("vs_brushed".into(), sim_system::Study {
        at: "gearmotor".into(),
        instance: "motor".into(),
        kind: StudyKind::Compare { alternatives: vec![InstanceKind::Element { component_type: "bridge.brushed_motor".into() }] },
        duration: 1.0,
        observe: vec!["drum.shaft.speed".into()],
        metrics: vec![sim_system::Metric { label: "lift".into(), observable: "drum.shaft.speed".into(), reduce: sim_system::Reduce::Mean, window: Some([0.8, 1.0]) }],
    });
    let compare = system_study::run(&doc, &registry, "vs_brushed", &doc.studies["vs_brushed"], 2, None, &|_, _| {}).unwrap();
    assert!(compare.variants.iter().all(|v| v.error.is_none()), "{:?}", compare.variants.iter().map(|v| &v.error).collect::<Vec<_>>());
    lap("5 compare with the brushed motor");

    // 6. Sweep the magnet: stronger magnets lift slower but harder (ω ≈ V/k).
    doc.studies.insert("magnet".into(), sim_system::Study {
        at: String::new(),
        instance: "gearmotor".into(),
        kind: StudyKind::Sweep { parameter: "motor/magnet".into(), values: vec![0.8, 1.0, 1.2, 1.4] },
        duration: 1.0,
        observe: vec!["drum.shaft.speed".into(), "gearmotor/motor.p.current".into()],
        metrics: vec![sim_system::Metric { label: "lift".into(), observable: "drum.shaft.speed".into(), reduce: sim_system::Reduce::Mean, window: Some([0.8, 1.0]) }],
    });
    let sweep = system_study::run(&doc, &registry, "magnet", &doc.studies["magnet"], 4, None, &|_, _| {}).unwrap();
    let lifts: Vec<f64> = sweep.variants.iter().map(|v| v.metrics[0].1).collect();
    eprintln!("{}", system_study::table(&sweep));
    assert!(lifts.windows(2).all(|w| w[1] < w[0]), "stronger magnet, slower lift: {lifts:?}");
    lap("6 sweep the magnet");

    // 7. Choose 1.2 and publish the gearmotor to a library.
    sim_system::apply(&mut doc, &registry, &[Command::SetParameter { at: "gearmotor".into(), name: "motor".into(), parameter: "magnet".into(), binding: Some(sim_system::ParameterBinding::value(1.2)) }]).unwrap();
    let lib = std::env::temp_dir().join(format!("sim-final-{}", std::process::id()));
    let published = library::publish(&doc, "flux_gearmotor", &lib).unwrap();
    assert!(published.iter().any(|p| p.id == "flux_gearmotor" && p.changed));
    lap("7 publish");
    let total = started.elapsed().as_secs_f64();
    eprintln!("tool time: idea → first result {idea_to_result:.2} s; whole loop {total:.2} s");
    assert!(idea_to_result < 120.0 && total < 600.0);
    std::fs::remove_dir_all(&lib).ok();
}
