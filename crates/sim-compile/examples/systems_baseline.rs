//! Capture pre-migration models and numerical results. Run with --check PATH to
//! compare against the retained baseline; timing is evidence, not portable CI.
use serde_json::{Value, json};
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld};
use sim_dynamics::Integrator;
use std::time::Instant;

fn model(case: &str, r: &BehaviorRegistry) -> ModelWorld {
    use sim_domain_bridges::elements as b;
    use sim_domain_electrical::elements as e;
    use sim_domain_multibody::{contact as p, elements as f};
    use sim_domain_rotational::elements as m;
    use sim_domain_thermal as t;
    let mut w = ModelWorld::default();
    match case {
        "rc" => {
            let source = w
                .part(r, "supply", e::VOLTAGE_SOURCE, [("voltage", 1.)])
                .unwrap();
            let ground = w.part(r, "ground", e::GROUND, []).unwrap();
            let resistor = w
                .part(r, "resistor", e::RESISTOR, [("resistance", 2.)])
                .unwrap();
            let capacitor = w
                .part(r, "capacitor", e::CAPACITOR, [("capacitance", 0.5)])
                .unwrap();
            w.connect([source.port("p"), resistor.port("p")]);
            w.connect([resistor.port("n"), capacitor.port("p")]);
            w.connect([source.port("n"), capacitor.port("n"), ground.port("pin")]);
        }
        "thermal" => {
            let storage = w
                .part(
                    r,
                    "storage",
                    t::CAPACITANCE,
                    [("heat_capacity", 2.), ("initial.temperature", 313.15)],
                )
                .unwrap();
            let conduction = w
                .part(r, "conduction", t::CONDUCTANCE, [("conductance", 0.5)])
                .unwrap();
            let ambient = w
                .part(r, "ambient", t::AMBIENT, [("temperature", 293.15)])
                .unwrap();
            w.connect([storage.port("node"), conduction.port("a")]);
            w.connect([ambient.port("node"), conduction.port("b")]);
        }
        "motor_composite" => {
            let motor = w
                .part(
                    r,
                    "motor",
                    b::MOTOR,
                    [("resistance", 2.), ("torque_constant", 0.1)],
                )
                .unwrap();
            let source = w
                .part(r, "supply", e::VOLTAGE_SOURCE, [("voltage", 2.)])
                .unwrap();
            let ground = w.part(r, "ground", e::GROUND, []).unwrap();
            let rotor = w
                .part(
                    r,
                    "rotor",
                    m::INERTIA,
                    [("inertia", 0.1), ("damping", 0.02)],
                )
                .unwrap();
            let storage = w
                .part(
                    r,
                    "case",
                    t::CAPACITANCE,
                    [("heat_capacity", 2.), ("initial.temperature", 293.15)],
                )
                .unwrap();
            let conduction = w
                .part(r, "cooling", t::CONDUCTANCE, [("conductance", 0.5)])
                .unwrap();
            let ambient = w
                .part(r, "ambient", t::AMBIENT, [("temperature", 293.15)])
                .unwrap();
            w.connect([
                motor.port("plug"),
                source.port("p"),
                rotor.port("shaft"),
                storage.port("node"),
                conduction.port("a"),
            ]);
            w.connect([source.port("n"), ground.port("pin")]);
            w.connect([conduction.port("b"), ambient.port("node")]);
        }
        "planar_frame" => {
            let body = w
                .part(
                    r,
                    "body",
                    p::PLANAR_RIGID_BODY,
                    [
                        ("mass", 1.),
                        ("inertia", 0.1),
                        ("gravity", 0.),
                        ("initial.vx", 2.),
                    ],
                )
                .unwrap();
            w.connect([body.port("frame")]);
        }
        "spatial_frame" => {
            let body = w
                .part(
                    r,
                    "body",
                    f::RIGID_BODY,
                    [
                        ("mass", 1.),
                        ("ixx", 0.1),
                        ("iyy", 0.2),
                        ("izz", 0.3),
                        ("gravity", 0.),
                    ],
                )
                .unwrap();
            w.connect([body.port("frame")]);
        }
        _ => panic!("unknown baseline"),
    }
    w
}

fn capture() -> Value {
    let mut r = BehaviorRegistry::default();
    sim_domain_electrical::elements::register(&mut r).unwrap();
    sim_domain_thermal::register(&mut r).unwrap();
    sim_domain_bridges::elements::register(&mut r).unwrap();
    sim_domain_rotational::elements::register(&mut r).unwrap();
    sim_domain_multibody::elements::register(&mut r).unwrap();
    sim_domain_multibody::contact::register(&mut r).unwrap();
    let cases: Vec<_> = ["rc", "thermal", "motor_composite", "planar_frame", "spatial_frame"]
        .into_iter().map(|name| {
            let w = model(name, &r);
            let serialized = serde_json::to_value(&w).unwrap();
            let start = Instant::now();
            let mut rt = Runtime::new(w, &r, Integrator::implicit_midpoint()).unwrap();
            let compile_s = start.elapsed().as_secs_f64();
            let mut channels: Vec<_> = rt.model.state.iter()
                .map(|(id, entry)| (entry.name.clone(), entry.quantity.unit().to_owned(), id)).collect();
            channels.sort_by(|a, b| a.0.cmp(&b.0));
            let mut samples = vec![json!({"time": rt.time, "values": channels.iter().map(|(_,_,id)| rt.get(*id)).collect::<Vec<_>>()})];
            let start = Instant::now();
            for _ in 0..10 {
                rt.advance(0.1, 0.001).unwrap();
                samples.push(json!({"time": rt.time, "values": channels.iter().map(|(_,_,id)| rt.get(*id)).collect::<Vec<_>>() }));
            }
            let advance_s = start.elapsed().as_secs_f64();
            json!({"name": name, "model": serialized, "channels": channels.iter().map(|(n,u,_)| json!({"name":n,"unit":u})).collect::<Vec<_>>(),
                "samples":samples,"compile_s":compile_s,"advance_s":advance_s})
        }).collect();
    json!({"version":1,"step_s":0.001,"sample_s":0.1,"cases":cases})
}

fn main() {
    let result = capture();
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--check") {
        let baseline: Value = serde_json::from_slice(&std::fs::read(&args[1]).unwrap()).unwrap();
        assert_eq!(baseline["version"], result["version"]);
        assert_eq!(baseline["step_s"], result["step_s"]);
        assert_eq!(baseline["sample_s"], result["sample_s"]);
        assert_eq!(
            baseline["cases"].as_array().unwrap().len(),
            result["cases"].as_array().unwrap().len()
        );
        for (old, new) in baseline["cases"]
            .as_array()
            .unwrap()
            .iter()
            .zip(result["cases"].as_array().unwrap())
        {
            assert_eq!(old["name"], new["name"]);
            let decoded: ModelWorld = serde_json::from_value(old["model"].clone()).unwrap();
            assert_eq!(
                serde_json::to_value(decoded).unwrap(),
                new["model"],
                "legacy model changed"
            );
            assert_eq!(
                old["channels"], new["channels"],
                "channel schema changed in {}",
                old["name"]
            );
            assert_eq!(
                old["samples"].as_array().unwrap().len(),
                new["samples"].as_array().unwrap().len()
            );
            for (a, b) in old["samples"]
                .as_array()
                .unwrap()
                .iter()
                .zip(new["samples"].as_array().unwrap())
            {
                assert_eq!(a["time"], b["time"]);
                assert_eq!(
                    a["values"].as_array().unwrap().len(),
                    b["values"].as_array().unwrap().len()
                );
                for (x, y) in a["values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(b["values"].as_array().unwrap())
                {
                    let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
                    assert!(
                        (x - y).abs() <= 1e-10 * (1. + x.abs()),
                        "{}: {x} != {y}",
                        old["name"]
                    );
                }
            }
        }
        eprintln!("All five baseline trajectories match (absolute + relative tolerance 1e-10)");
    }
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
}
