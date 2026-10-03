//! A system file that hosts the wheeled robot: `Command::LinkFile` (link,
//! unlink, rename, remove, and its refusals), flattening and compiling with
//! hosted instances, the hosted-only wiring rule, and
//! `system_robot::resolve` against examples/wheeled-robot/baseline (the
//! model, its controller binding and drive profile are read and the script
//! hashed; no Python process is started). Written by reading; not yet executed.
use sim_core::BehaviorRegistry;
use sim_runtime::drive_host::DriveRequest;
use sim_runtime::system_robot;
use sim_system::{Command, InstanceSpec, ParameterBinding, SystemDocument, Terminal, apply};
use std::path::PathBuf;

/// The system file the links resolve against (it need not exist: only its directory is used).
fn system_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/wheeled-robot/baseline/rover.system.json")
}

fn add(name: &str, component_type: &str) -> Command {
    Command::AddInstance { at: String::new(), name: name.into(), instance: InstanceSpec::element(component_type) }
}
fn link(instance: &str, path: Option<&str>) -> Command {
    Command::LinkFile { instance: instance.into(), path: path.map(str::to_string) }
}

/// The commands a script sends to build the rover system (one atomic batch).
fn rover_commands() -> Vec<Command> {
    let mut commands = vec![
        add("rover", "robot.articulated"),
        add("controller", "control.external"),
        add("limiter", "control.drive_limiter"),
        link("rover", Some("robot.simrobot.json")),
        link("controller", Some("robot.controller.json")),
        link("limiter", Some("robot.drive.json")),
    ];
    for axis in ["forward", "yaw"] {
        commands.push(Command::SetParameter { at: String::new(), name: "controller".into(), parameter: format!("sense.command.{axis}"), binding: Some(ParameterBinding::value(1.0)) });
        commands.push(Command::Connect { at: String::new(), terminals: vec![Terminal::port("limiter", &format!("twist.{axis}")), Terminal::port("controller", &format!("sense.command.{axis}"))], label: String::new() });
    }
    commands
}

fn rover(r: &BehaviorRegistry) -> SystemDocument {
    let mut doc = SystemDocument::new("Rover");
    apply(&mut doc, r, &rover_commands()).unwrap();
    doc
}

#[test]
fn hosted_instances_are_linked_without_their_implementation_parameters_and_left_out_of_the_model() {
    let r = sim_runtime::registry();
    let doc = rover(&r);
    assert_eq!(doc.links.len(), 3);
    assert_eq!(doc.link("rover").map(|l| l.path.as_str()), Some("robot.simrobot.json"));
    // The file round-trips with its links.
    let text = serde_json::to_string(&doc).unwrap();
    let back: SystemDocument = serde_json::from_str(&text).unwrap();
    assert_eq!(back, doc);
    let flat = sim_system::flatten(&doc, &r).unwrap();
    assert!(flat.components.is_empty() && flat.model.behaviors.is_empty() && flat.model.connections.is_empty());
    assert_eq!(flat.hosted.keys().map(String::as_str).collect::<Vec<_>>(), ["controller", "limiter", "rover"]);
    assert_eq!(flat.hosted["controller"].parameters.keys().map(String::as_str).collect::<Vec<_>>(), ["sense.command.forward", "sense.command.yaw"]);
    assert_eq!(flat.hosted_nets.len(), 2);
    // Hosted instances are a finding, not missing parameters or loose ports.
    assert!(flat.findings.iter().filter(|f| f.code == "hosted").count() == 3, "{:?}", flat.findings);
    assert!(!flat.findings.iter().any(|f| f.code == "missing_parameter" || f.code == "unconnected_port"), "{:?}", flat.findings);
    // Build mode's describe/schematic compile and the numerical runtime accept it.
    let compiled = sim_runtime::system_builder::compile(&doc, &r, sim_runtime::system_builder::default_config()).unwrap();
    assert!(compiled.spatial.is_none());
    assert!(sim_runtime::system_builder::check(&doc, &r).unwrap().compile_error.is_none());
}

#[test]
fn renaming_or_removing_a_linked_instance_renames_or_drops_its_link_and_unlink_needs_a_link() {
    let r = sim_runtime::registry();
    let mut doc = rover(&r);
    apply(&mut doc, &r, &[Command::RenameInstance { at: String::new(), name: "rover".into(), new_name: "chassis".into() }]).unwrap();
    assert!(doc.link("rover").is_none() && doc.link("chassis").is_some_and(|l| l.path == "robot.simrobot.json"));
    apply(&mut doc, &r, &[Command::RemoveInstance { at: String::new(), name: "limiter".into() }]).unwrap();
    assert!(doc.link("limiter").is_none() && doc.definitions["root"].nets.is_empty());
    let revision = doc.revision;
    apply(&mut doc, &r, &[link("controller", None)]).unwrap();
    assert!(doc.link("controller").is_none() && doc.revision == revision + 1);
    let e = apply(&mut doc, &r, &[link("controller", None)]).unwrap_err().to_string();
    assert!(e.contains("`controller` has no linked file"), "{e}");
    // Relinking: the same command, the shared undo path (a new revision).
    apply(&mut doc, &r, &[link("controller", Some("robot.controller.json"))]).unwrap();
    assert_eq!(doc.revision, revision + 2);
}

#[test]
fn a_link_is_refused_naming_the_instance_and_the_rule() {
    let r = sim_runtime::registry();
    let doc = rover(&r);
    let refused = |commands: Vec<Command>| {
        let mut d = doc.clone();
        let e = apply(&mut d, &r, &commands).unwrap_err().to_string();
        assert_eq!(d, doc, "a refused batch leaves the document unchanged");
        e
    };
    let e = refused(vec![link("rover", Some("robot.drive.json"))]);
    assert!(e.contains("link of `rover`") && e.contains("robot.articulated instance links a `<name>.simrobot.json` file"), "{e}");
    let e = refused(vec![link("rover", Some("/abs/robot.simrobot.json"))]);
    assert!(e.contains("is absolute"), "{e}");
    let e = refused(vec![link("rover", Some(""))]);
    assert!(e.contains("the path is empty"), "{e}");
    let e = refused(vec![link("rover", Some(".simrobot.json"))]);
    assert!(e.contains("is not one"), "{e}");
    let e = refused(vec![link("ghost", Some("robot.simrobot.json"))]);
    assert!(e.contains("has no instance `ghost`"), "{e}");
    let e = refused(vec![add("r1", "electrical.resistor"), link("r1", Some("r1.simrobot.json"))]);
    assert!(e.contains("`r1` is a electrical.resistor element; only robot.articulated, control.external, control.drive_limiter elements can be hosted"), "{e}");
    // A hosted port joins only hosted ports.
    let e = refused(vec![
        add("k", "control.constant"),
        Command::SetParameter { at: String::new(), name: "k".into(), parameter: "value".into(), binding: Some(ParameterBinding::value(0.0)) },
        Command::Connect { at: String::new(), terminals: vec![Terminal::port("k", "value"), Terminal::port("controller", "sense.command.forward")], label: String::new() },
    ]);
    assert!(e.contains("is on a hosted instance but k.value is not"), "{e}");
    // A wired hosted instance is not unlinked behind its nets.
    let e = refused(vec![link("controller", None)]);
    assert!(e.contains("`controller` cannot be unlinked while controller.sense.command.forward, controller.sense.command.yaw are connected") && e.ends_with("disconnect it first"), "{e}");
    // Connect before link_file is an ordinary, type-checked net (m/s into a dimensionless sense port).
    let mut fresh = SystemDocument::new("order");
    let e = apply(&mut fresh, &r, &[add("controller", "control.external"), add("limiter", "control.drive_limiter"),
        Command::SetParameter { at: String::new(), name: "controller".into(), parameter: "sense.command.forward".into(), binding: Some(ParameterBinding::value(1.0)) },
        Command::Connect { at: String::new(), terminals: vec![Terminal::port("limiter", "twist.forward"), Terminal::port("controller", "sense.command.forward")], label: String::new() },
        link("controller", Some("robot.controller.json")), link("limiter", Some("robot.drive.json"))]).unwrap_err().to_string();
    assert!(e.starts_with("command 3: "), "{e}");
    // Grouping a linked instance would leave its link without a root instance.
    let e = refused(vec![Command::Group { at: String::new(), instances: vec!["limiter".into()], name: "drive".into(), definition: "drive_group".into(), label: String::new() }]);
    assert!(e.contains("link of `limiter`") && e.contains("unlink an instance before grouping it"), "{e}");
}

#[test]
fn the_rover_system_resolves_to_its_files_and_wiring() {
    let r = sim_runtime::registry();
    let doc = rover(&r);
    let flat = sim_system::flatten(&doc, &r).unwrap();
    let system = system_robot::resolve(&doc, &system_path(), &flat).unwrap().expect("a hosted robot");
    assert_eq!((system.robot.instance.as_str(), system.controller.instance.as_str()), ("rover", "controller"));
    assert_eq!(system.limiter.as_ref().map(|l| l.instance.as_str()), Some("limiter"));
    assert_eq!(system.wiring, ["limiter.twist.forward → controller.sense.command.forward", "limiter.twist.yaw → controller.sense.command.yaw"]);
    // The scene the drive host runs: the model's control period, the binding's program with the four command channels.
    assert_eq!(system.scene.period_s, 0.02);
    assert_eq!(system.scene.controller.as_ref().unwrap().inputs.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), sim_runtime::controller_binding::COMMAND_CHANNELS);
    assert!(system.controlled.profile_path.ends_with("robot.drive.json"));
    // The same interpretation Robot mode uses.
    let (twist, halt) = DriveRequest::Axes { forward: 1.0, lateral: 0.0, yaw: 0.0 }.interpret(&system.controlled).unwrap();
    assert!(!halt && (twist.forward_m_s - 0.26).abs() < 1e-12);
    let json = system.json();
    assert_eq!(json["elements"]["controller"]["link"], "robot.controller.json");
    assert_eq!(json["limits"][0]["max_speed"]["unit"], "m/s");
    // The same files through another relative path are the same binding (canonical paths).
    let mut other = rover(&r);
    apply(&mut other, &r, &[link("rover", Some("../baseline/robot.simrobot.json")), link("controller", Some("../baseline/robot.controller.json"))]).unwrap();
    assert!(system_robot::resolve(&other, &system_path(), &sim_system::flatten(&other, &r).unwrap()).unwrap().is_some());
    // A plain system hosts nothing.
    let plain = SystemDocument::new("plain");
    assert!(system_robot::resolve(&plain, &system_path(), &sim_system::flatten(&plain, &r).unwrap()).unwrap().is_none());
}

#[test]
fn errors_carrying_the_controller_label_name_the_system_instance() {
    let r = sim_runtime::registry();
    let doc = rover(&r);
    let system = system_robot::resolve(&doc, &system_path(), &sim_system::flatten(&doc, &r).unwrap()).unwrap().unwrap();
    let external = system.scene.controller.as_ref().unwrap().external.as_ref().unwrap();
    let label = external.label("seam");
    let e = system.name_error(&format!("{label}: cannot start python3: not found"));
    assert_eq!(e, format!("`controller` ({label}): cannot start python3: not found"));
    let e = system.name_error(&format!("episode failed: {label}: timed out; reset before continuing"));
    assert_eq!(e, format!("`controller` ({label}): episode failed: timed out; reset before continuing"));
    let e = system.name_error("unsupported scene version");
    assert!(e.starts_with("robot system (`rover` from robot.simrobot.json, `controller` from robot.controller.json): "), "{e}");
}

#[test]
fn a_robot_system_refuses_what_the_drive_host_does_not_run() {
    let r = sim_runtime::registry();
    let resolve = |commands: Vec<Command>| {
        let mut doc = rover(&r);
        apply(&mut doc, &r, &commands).unwrap();
        let flat = sim_system::flatten(&doc, &r).unwrap();
        system_robot::resolve(&doc, &system_path(), &flat).err().expect("refused")
    };
    // A compiled element beside the hosted robot.
    let e = resolve(vec![add("k", "control.constant"), Command::SetParameter { at: String::new(), name: "k".into(), parameter: "value".into(), binding: Some(ParameterBinding::value(1.0)) }]);
    assert!(e.starts_with("a robot system runs its linked robot, controller and drive limiter on the shared drive host; `k` (control.constant) is not hosted"), "{e}");
    // A physical value typed into a hosted instance.
    let e = resolve(vec![Command::SetParameter { at: String::new(), name: "rover".into(), parameter: "gravity".into(), binding: Some(ParameterBinding::value(1.0)) }]);
    assert!(e.contains("`rover`.gravity is set in the system file") && e.contains("the model file"), "{e}");
    let e = resolve(vec![Command::SetParameter { at: String::new(), name: "controller".into(), parameter: "period".into(), binding: Some(ParameterBinding::value(0.02)) }]);
    assert!(e.contains("`controller`.period is set in the system file"), "{e}");
    // Wiring the host does not run: a cross-axis net.
    let e = resolve(vec![
        Command::SetParameter { at: String::new(), name: "controller".into(), parameter: "sense.command.lateral".into(), binding: Some(ParameterBinding::value(1.0)) },
        Command::Connect { at: String::new(), terminals: vec![Terminal::port("limiter", "twist.lateral"), Terminal::port("controller", "sense.command.lateral")], label: String::new() },
        Command::Disconnect { at: String::new(), terminal: Terminal::port("controller", "sense.command.yaw") },
        Command::Connect { at: String::new(), terminals: vec![Terminal::port("limiter", "twist.yaw"), Terminal::port("controller", "sense.command.lateral")], label: String::new() },
    ]);
    assert!(e.contains("is not wiring the drive host runs"), "{e}");
    // Two robots.
    let e = resolve(vec![add("twin", "robot.articulated"), link("twin", Some("robot.simrobot.json"))]);
    assert!(e.contains("exactly one robot.articulated; this one hosts 2 (`rover`, `twin`)"), "{e}");
    // A limiter linked to a profile that does not exist.
    let e = resolve(vec![link("limiter", Some("missing.drive.json"))]);
    assert!(e.contains("`limiter` (control.drive_limiter") && e.contains("the drive profile cannot be found"), "{e}");
}
