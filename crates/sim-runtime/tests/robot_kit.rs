//! The robot kit's starter robots run and behave physically: the rover
//! cruises near its no-load prediction, the drone climbs to and holds its
//! altitude at the predicted hover current, the series-elastic arm reaches
//! its target against gravity, the stepper axis moves its commanded travel
//! (and loses steps when driven too fast), and the self-locking gripper
//! keeps its grip after the power is cut. Every preset and part the kit
//! uses has notes.
use sim_runtime::{system_builder, system_study};
use sim_system::SystemDocument;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn registry() -> sim_core::BehaviorRegistry {
    let (r, loaded) = sim_runtime::registry_with_parts(&root().join("library/parts"));
    assert!(loaded.iter().all(|l| l.error.is_none()), "{loaded:?}");
    r
}
fn robot(name: &str) -> SystemDocument {
    serde_json::from_slice(&std::fs::read(root().join(format!("examples/robot-kit/{name}.system.json"))).unwrap()).unwrap()
}
fn run(r: &sim_core::BehaviorRegistry, doc: &SystemDocument, duration: f64, keys: &[&str]) -> Vec<system_builder::Series> {
    system_builder::simulate(doc, r, duration, system_builder::config_for(doc), &keys.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
}
fn at(s: &[system_builder::Series], key: &str, t: f64) -> f64 {
    let s = s.iter().find(|s| s.label == key).unwrap_or_else(|| panic!("no {key}"));
    s.values[s.times.iter().position(|x| *x >= t - 1e-9).unwrap()]
}

#[test]
fn starter_robots_behave() {
    let r = registry();

    // Rover: 80 % of a 3S pack through two 37 mm 30:1 gearmotors on 40 mm wheels.
    // No-load wheel speed ≈ 0.8·12.4 V / k / N; cruise is just below it.
    let rover = run(&r, &robot("rover"), 4.0, &["chassis.axis.velocity"]);
    let no_load = 0.8 * 12.4 / 0.0115 / 30.0 * 0.04;
    let cruise = at(&rover, "chassis.axis.velocity", 3.0);
    eprintln!("rover cruise {cruise:.3} m/s (no-load bound {no_load:.3})");
    assert!(cruise > 0.8 * no_load && cruise < no_load, "cruise {cruise}");
    assert!(at(&rover, "chassis.axis.velocity", 4.0).abs() < 0.05, "stops after the throttle pulse");

    // Drone: 1.2 kg climbs to 1 m and holds; each 10" prop needs ≈ 2.9 N,
    // which the momentum-free prop model gives at ≈ 455 rad/s and ≈ 4.4 A.
    let drone = run(&r, &robot("drone"), 6.0, &["airframe.axis.position", "arm1/motor.p.current"]);
    let (z, i) = (at(&drone, "airframe.axis.position", 6.0), at(&drone, "arm1/motor.p.current", 6.0));
    eprintln!("drone altitude {z:.3} m, hover current {i:.2} A per motor");
    assert!((z - 1.0).abs() < 0.03 && (i - 4.4).abs() < 0.3);

    // Series-elastic arm reaches 1.2 rad against gravity.
    let arm = run(&r, &robot("sea_arm"), 3.0, &["link.shaft.angle"]);
    let angle = at(&arm, "link.shaft.angle", 3.0);
    eprintln!("arm angle {angle:.3} rad (target 1.2)");
    assert!((angle - 1.2).abs() < 0.02);

    // Stepper axis: 200 mm in 1 s, far switch trips.
    let axis = run(&r, &robot("belt_axis"), 1.5, &["carriage.axis.position", "home.pressed"]);
    assert!((at(&axis, "carriage.axis.position", 1.5) - 0.2).abs() < 0.002);
    assert!(at(&axis, "home.pressed", 1.5) > 0.99);

    // Gripper: grips, and the self-locking Tr8×2 holds after the power is cut.
    let grip = run(&r, &robot("gripper"), 3.0, &["load_cell.force"]);
    let (on, off) = (at(&grip, "load_cell.force", 1.5), at(&grip, "load_cell.force", 3.0));
    eprintln!("grip {on:.1} N powered, {off:.1} N after power-off");
    assert!(on > 20.0 && (off - on).abs() / on < 0.1);
}

#[test]
fn saved_studies_show_the_trade_offs() {
    let r = registry();
    let value = |res: &system_study::StudyResult, i: usize, k: &str| res.variants[i].metrics.iter().find(|m| m.0.contains(k)).unwrap().1;
    // Driving a stepper too fast loses steps.
    let doc = robot("belt_axis");
    let steps = system_study::run(&doc, &r, "move_speed", &doc.studies["move_speed"], 4, None, &|_, _| {}).unwrap();
    assert!((value(&steps, 0, "final position") - 0.2).abs() < 0.002 && value(&steps, 3, "final position") < 0.1);
    // Bigger props hover on less battery current.
    let doc = robot("drone");
    let props = system_study::run(&doc, &r, "prop_size", &doc.studies["prop_size"], 4, None, &|_, _| {}).unwrap();
    let currents: Vec<f64> = (0..4).map(|i| value(&props, i, "battery current").abs()).collect();
    assert!(currents.windows(2).all(|w| w[1] < w[0]), "{currents:?}");
    // Screws: efficient screws grip harder; only the self-locking one holds exactly.
    let doc = robot("gripper");
    let screws = system_study::run(&doc, &r, "screws", &doc.studies["screws"], 3, None, &|_, _| {}).unwrap();
    eprintln!("{}", system_study::table(&screws));
    let hold = |i: usize| value(&screws, i, "after power-off") / value(&screws, i, "while powered");
    assert!(value(&screws, 2, "while powered") > value(&screws, 0, "while powered"), "ball screw grips harder");
    assert!(hold(0) > 0.95 && hold(2) < 0.9, "self-locking holds; ball screw relaxes");
}

#[test]
fn everything_in_the_kit_is_annotated() {
    let r = registry();
    for name in ["rover", "drone", "sea_arm", "belt_axis", "gripper"] {
        let doc = robot(name);
        for d in doc.definitions.values() {
            for (inst, spec) in &d.instances {
                if let sim_system::InstanceKind::Element { component_type } = &spec.kind {
                    let notes = r.get(&component_type.as_str().into()).unwrap().notes;
                    assert!(notes.is_some_and(|n| !n.summary.is_empty()), "{name}: {inst} ({component_type}) has no notes");
                }
            }
        }
    }
}
