use sim_core::FnCoupler;
use std::sync::{Arc, Mutex};
type Ticks = Arc<Mutex<Vec<[f64; 3]>>>;
use sim_runtime::{
    BuildOptions, PhysicalRobot,
    electrical::{SupplySample, summarize_supply},
};

#[test]
fn signed_supply_accounting_separates_consumption_return_and_true_minimum() {
    let trace = vec![
        SupplySample {
            time_s: 0.,
            voltage_v: 12.,
            current_a: 1.,
            power_w: 12.,
            state_of_charge: Some(0.8),
        },
        SupplySample {
            time_s: 1.,
            voltage_v: 10.,
            current_a: -1.,
            power_w: -10.,
            state_of_charge: Some(0.8),
        },
        SupplySample {
            time_s: 2.,
            voltage_v: 12.,
            current_a: 0.,
            power_w: 0.,
            state_of_charge: Some(0.8),
        },
    ];
    let s = summarize_supply(&trace).unwrap();
    assert_eq!(s.minimum_voltage_v, 10.);
    assert!((s.drawn_energy_j - 36. / 11.).abs() < 1e-12);
    assert!((s.returned_energy_j - (25. / 11. + 5.)).abs() < 1e-12);
    assert_eq!(s.discharged_charge_c, 0.25);
    assert_eq!(s.returned_charge_c, 0.75);
    let mut bad = trace.clone();
    bad[1].power_w = 10.;
    assert!(summarize_supply(&bad).is_err());
    let mut bad = trace;
    bad[1].state_of_charge = Some(-0.1);
    assert!(!summarize_supply(&bad).unwrap().state_of_charge_in_range);
}

fn fixture(capacity_ah: f64, initial_soc: f64) -> (PhysicalRobot, Ticks) {
    fixture_at(capacity_ah, initial_soc, 0.0001)
}
fn fixture_at(capacity_ah: f64, initial_soc: f64, step: f64) -> (PhysicalRobot, Ticks) {
    let scene: sim_runtime::session::Scene = serde_json::from_str(include_str!(
        "../../../examples/interactive/pendulum.scene.json"
    ))
    .unwrap();
    // Explicit synthetic fixture: two identical motor/bridge chains share one
    // source and shaft. Opposite duties cancel motion, exposing electrical loading.
    let mut model = scene.robot;
    model.gravity = [0.; 3];
    model.control.period_s = 0.001;
    for m in &mut model.motors {
        m.gearbox.backlash_rad = 0.;
        m.electrical.no_load_current = 0.;
    }
    let mut other = model.motors[0].clone();
    other.name = "second".into();
    other.id = "synthetic-second".into();
    model.motors.push(other);
    model.battery=Some(serde_json::from_value(serde_json::json!({"cells":2.,"nominal_voltage":5.,"internal_resistance":0.8,"capacity_ah":capacity_ah,"initial_soc":initial_soc,"cutoff_voltage":3.})).unwrap());
    let options = BuildOptions {
        driver_control: true,
        flex: false,
        contact: false,
        step,
        sample: step,
        ..Default::default()
    };
    let mut robot = PhysicalRobot::build(model, &sim_runtime::registry(), &options).unwrap();
    let seam = robot.seam.unwrap();
    let contract = robot.runtime.contract(seam);
    let servo = contract
        .actuators
        .iter()
        .position(|p| p.name == "servo.duty")
        .unwrap();
    let second = contract
        .actuators
        .iter()
        .position(|p| p.name == "second.duty")
        .unwrap();
    let ticks = Arc::new(Mutex::new(vec![]));
    let captured = ticks.clone();
    robot
        .runtime
        .bind_coupler(
            seam,
            Box::new(FnCoupler(move |t: f64, _s: &[f64], a: &mut [f64]| {
                let duty = if t + 1e-12 < 0.01 { 0.3 } else { 0. };
                a[servo] = duty;
                a[second] = -duty;
                captured.lock().unwrap().push([t, duty, -duty]);
            })),
            // A function of time only: checkpoints may include it.
            true,
        )
        .unwrap();
    (robot, ticks)
}
#[test]
fn cad_robot_reports_shared_battery_terminals_instead_of_summed_winding_energy() {
    let (mut robot, ticks) = fixture(0.01, 0.8);
    let motor_current = |name: &str| {
        let (b, _) = robot
            .runtime
            .model
            .behaviors
            .iter()
            .find(|(_, b)| robot.runtime.model.objects[b.object].name == name)
            .unwrap();
        robot.runtime.state_id(b, "current")
    };
    let currents = [motor_current("servo.unit"), motor_current("second.unit")];
    for _ in 0..50 {
        robot.advance(0.0001).unwrap();
        let supply = robot.battery_reading().unwrap();
        let total = 0.3 * robot.runtime.get(currents[0]) - 0.3 * robot.runtime.get(currents[1]);
        assert!(
            (supply.current_a - total).abs() < 1e-7,
            "shared source KCL: {} vs {total}",
            supply.current_a
        );
        let soc = supply.state_of_charge.unwrap();
        let ocv = 5. * (0.9 + 0.2 * soc - 0.15 * (1. - soc).powi(8));
        assert!(
            (supply.voltage_v - (ocv - 0.8 * total)).abs() < 1e-7,
            "load must sag the shared source"
        );
    }
    robot.advance(0.015).unwrap();
    let report = robot.results("synthetic two-motor fixture, no physical validation");
    let battery = &report["battery"];
    assert_eq!(battery["accounting_version"], 2);
    let min = battery["min_voltage"].as_f64().unwrap();
    let final_v = battery["final_voltage_v"].as_f64().unwrap();
    assert!(
        min < final_v - 0.01,
        "minimum is {min}, recovered final is {final_v}"
    );
    let samples: Vec<SupplySample> = serde_json::from_value(battery["samples"].clone()).unwrap();
    assert_eq!(samples[0].time_s, 0.);
    let stopped = samples
        .iter()
        .find(|s| s.time_s > 0.005 && s.current_a.abs() < 1e-10)
        .unwrap();
    let commands = ticks.lock().unwrap().clone();
    let off = commands
        .iter()
        .find(|c| c[0] > 0.005 && c[1] == 0.)
        .unwrap()[0];
    assert!(
        (off - 0.01).abs() < 1e-12,
        "Controller PWM edge must be 10 ms, not one tick late: {off}"
    );
    // A committed time immediately before the exact event may still hold the
    // pre-event current. Require the first post-edge sample within one physics step.
    assert!(
        stopped.time_s >= off - 1e-12 && stopped.time_s <= off + robot.step + 1e-12,
        "Source current must clear within one retained sample of the 10 ms edge: {}",
        stopped.time_s
    );
    assert!((samples.last().unwrap().time_s - robot.time()).abs() < 1e-10);
    let expected = summarize_supply(&samples).unwrap();
    assert!(
        (battery["energy_j"].as_f64().unwrap()
            - (expected.drawn_energy_j - expected.returned_energy_j))
            .abs()
            < 1e-12
    );
    let winding: f64 = report["motors"]
        .as_object()
        .unwrap()
        .values()
        .map(|m| m["energy_j"].as_f64().unwrap())
        .sum();
    assert!(
        (winding - battery["energy_j"].as_f64().unwrap()).abs() > 1e-7,
        "source energy must account for driver loss separately"
    );
    let depleted = (0.8 - samples.last().unwrap().state_of_charge.unwrap()) * 3600. * 0.01;
    let q = expected.discharged_charge_c - expected.returned_charge_c;
    assert!(
        (q - depleted).abs() < q.abs() * 0.015,
        "charge accounting mismatch: {q} vs {depleted}"
    );
    assert!(expected.peak_discharge_current_a > 0.);
    if let Some(path) = std::env::var_os("SIM_ROBOT_POWER_TEST_REPORT") {
        use std::io::Write;
        let evidence = serde_json::json!({"evidence_kind":"simulation_only","source_scene":"examples/interactive/pendulum.scene.json",
            "source_scene_blake3":blake3::hash(include_bytes!("../../../examples/interactive/pendulum.scene.json")).to_hex().to_string(),
            "fixture_code":"crates/sim-runtime/tests/robot_power.rs","fixture_code_blake3":blake3::hash(include_bytes!("robot_power.rs")).to_hex().to_string(),
            "runtime":sim_runtime::physics_context::RuntimeIdentity::current(),
            "experimental_overrides":{"gravity":[0.,0.,0.],"motors":"Duplicate original onto the same shaft as second; both use zero backlash and no-load loss for the analytic fixture", "battery":robot.model.battery,"control":"Opposed signed PWM: servo +0.3, second -0.3 for t<0.01 s, then both zero; shared controller tick 0.001 s","step_s":robot.step,"sample_s":0.0001,"flex":false,"contact":false},
            "checks":"Both motor branches sum to battery-terminal current; voltage equals shared OCV minus source IR drop; source energy is distinct from winding energy; integrated charge agrees with state-of-charge change within 1.5 percent discretization tolerance; minimum voltage precedes recovery",
            "controller_events_time_s_and_signed_duties":commands, "results":report});
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        f.write_all(&serde_json::to_vec_pretty(&evidence).unwrap())
            .unwrap();
        f.sync_all().unwrap();
    }
}

#[test]
fn depleted_and_rewound_robot_runs_cannot_claim_valid_battery_history() {
    let (mut empty, _) = fixture(0.001, 0.0000001);
    assert!(
        empty
            .advance(0.001)
            .unwrap_err()
            .contains("state of charge")
    );
    let report = empty.results("synthetic depleted source");
    assert_eq!(
        report["battery"]["summary"]["state_of_charge_in_range"],
        false
    );
    let (mut robot, _) = fixture(0.01, 0.8);
    let initial = robot.runtime.snapshot().unwrap();
    robot.advance(0.005).unwrap();
    robot.runtime.restore(&initial).unwrap();
    let report = robot.results("synthetic rewind");
    assert!(
        report["battery"]["failure"]
            .as_str()
            .unwrap()
            .contains("rewound")
    );
    assert!(report["battery"].get("energy_j").is_none());
}

#[test]
fn source_accounting_converges_with_step_and_is_independent_of_host_chunking() {
    let run = |step: f64, chunks: &[f64]| {
        let (mut robot, _) = fixture_at(0.01, 0.8, step);
        for dt in chunks {
            robot.advance(*dt).unwrap();
        }
        let report = robot.results("synthetic step/cadence comparison");
        let samples: Vec<SupplySample> =
            serde_json::from_value(report["battery"]["samples"].clone()).unwrap();
        summarize_supply(&samples).unwrap()
    };
    let coarse = run(0.0001, &[0.02]);
    let chunked = run(0.0001, &[0.003, 0.005, 0.002, 0.01]);
    let fine = run(0.00005, &[0.02]);
    assert!((coarse.drawn_energy_j - chunked.drawn_energy_j).abs() < 1e-10);
    assert!((coarse.minimum_voltage_v - chunked.minimum_voltage_v).abs() < 1e-10);
    assert!((coarse.drawn_energy_j - fine.drawn_energy_j).abs() < fine.drawn_energy_j * 0.03);
    assert!((coarse.minimum_voltage_v - fine.minimum_voltage_v).abs() < 0.002);
}
