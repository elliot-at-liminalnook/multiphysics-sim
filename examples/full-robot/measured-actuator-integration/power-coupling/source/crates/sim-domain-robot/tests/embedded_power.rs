mod common;
use common::*;
use sim_core::Behavior;
use sim_domain_robot::articulated::embedding::*;
use sim_domain_robot::{Articulated, Generalized, Options};
use std::sync::Arc;

fn fixture() -> (
    Articulated,
    Generalized,
    EmbeddedMotorBank,
    EmbeddedDriverBank,
    EmbeddedServoBank,
    EmbeddedPowerConfig,
) {
    let mut model = empty_model();
    model.gravity = [0.0; 3];
    model
        .links
        .push(box_link("ground", [0.1; 3], 1.0, [0.0, 0.0, -1.0], true));
    for (name, x) in [("a", 0.0), ("b", 0.3)] {
        model
            .links
            .push(box_link(name, [0.1; 3], 1.0, [x, 0.0, 0.0], false));
        model.joints.push(joint(
            name,
            "revolute",
            Some("ground"),
            name,
            [x, 0.0, 0.0],
            [0.0, 0.0, 1.0],
        ));
    }
    let art = Articulated::new(
        Arc::new(model),
        &Options {
            contact: false,
            flex: false,
            ..Default::default()
        },
    )
    .unwrap();
    let g = art.generalized(
        art.states().iter().map(|s| s.initial).collect(),
        vec![0.0; art.state_count],
        &vec![0.0; art.port_names.len() + 1],
        vec![],
    );
    let configs: Vec<_> = ["joint.a", "joint.b"]
        .iter()
        .map(|name| EmbeddedMotorConfig {
            dof: (*name).into(),
            residual_scales: [1.0; 3],
            parameters: [
                ("resistance", 2.0),
                ("inductance", 0.003),
                ("torque_constant", 0.08),
                ("back_emf_constant", 0.08),
                ("ratio", 5.0),
                ("efficiency", 1.0),
                ("rotor_inertia", 0.0001),
                ("gear_inertia", 0.0003),
                ("gear_stiffness", 20.0),
                ("gear_damping", 0.04),
                ("temp_coeff", 0.0),
                ("derating", 0.0),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect(),
        })
        .collect();
    let motors = EmbeddedMotorBank::new_with_events(&art, &configs).unwrap();
    let names = motors.dof_names();
    let drivers = EmbeddedDriverBank::new(
        &motors,
        &names
            .iter()
            .map(|n| EmbeddedDriverConfig {
                dof: n.clone(),
                parameters: [
                    ("on_resistance".into(), 0.1),
                    ("current_limit".into(), 10.0),
                ]
                .into(),
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let servos = EmbeddedServoBank::new(
        &motors,
        &names
            .iter()
            .map(|n| EmbeddedServoConfig {
                dof: n.clone(),
                parameters: [
                    ("rate".into(), 100.0),
                    ("offset".into(), 1.0),
                    ("kp".into(), 1.0),
                ]
                .into(),
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let config = EmbeddedPowerConfig {
        battery: [
            ("cells".into(), 3.0),
            ("nominal_voltage".into(), 10.0),
            ("internal_resistance".into(), 0.4),
            ("capacity_ah".into(), 0.01),
            ("initial_soc".into(), 0.8),
        ]
        .into(),
        branches: vec![
            PowerBranchConfig {
                id: "trunk".into(),
                parent: None,
                resistance_ohm: 0.2,
                motors: vec![],
            },
            PowerBranchConfig {
                id: "a".into(),
                parent: Some("trunk".into()),
                resistance_ohm: 0.3,
                motors: vec![names[0].clone()],
            },
            PowerBranchConfig {
                id: "b".into(),
                parent: Some("trunk".into()),
                resistance_ohm: 0.1,
                motors: vec![names[1].clone()],
            },
        ],
        residual_scales: [1.0; 4],
    };
    (art, g, motors, drivers, servos, config)
}

#[test]
fn shared_supply_matches_kcl_voltage_drops_charge_and_terminal_energy_through_reversal() {
    let (art, mut g, mut motors, drivers, mut servos, config) = fixture();
    let power = EmbeddedPowerBank::new(&motors, &config).unwrap();
    let map = RigidEmbedding::new(&art, &motors.dof_names(), Default::default()).unwrap();
    let inputs = [ServoBoundary {
        target_rad: 0.0,
        supply_voltage_v: 999.0,
        winding_temperature_k: 293.15,
    }; 2];
    let mut held = PoweredServoState {
        servo: servos.initial_states(),
        power: power.initial_states(),
    };
    let servo_layout = servos.state_layout();
    let mut x = motors.initial_states();
    let h = 0.0002;
    let mut regenerated = false;
    for tick in 0..80 {
        let duty = if tick < 40 { 0.6 } else { -0.6 };
        for (start, _, _) in &servo_layout {
            held.servo[*start] = duty;
        }
        let old = held.power.clone();
        let mut control = servos
            .connect(&motors, &drivers, &inputs)
            .unwrap()
            .with_power(&power)
            .unwrap();
        assert!(!control.independent_motor_boundaries());
        let result = motors
            .advance_with_control(
                &map,
                &g,
                &x,
                &held,
                tick as f64 * h,
                h,
                &ImplicitStepConfig {
                    condense_auxiliary: true,
                    color_auxiliary_jacobian: true,
                    auxiliary_rate_unknowns: true,
                    ..Default::default()
                },
                &Default::default(),
                &mut control,
                |_, _| Ok(vec![0.0; 2]),
            )
            .unwrap();
        assert_eq!(
            result
                .motor
                .solves
                .successful_trial_colored_auxiliary_solves,
            0
        );
        held = result.control_state;
        x = result.motor.motor_states;
        g = result.motor.endpoint.generalized;
        let rates: Vec<_> = held
            .power
            .iter()
            .zip(&old)
            .map(|(a, b)| (a - b) / h)
            .collect();
        let (_, driver, residuals, reading) = power
            .evaluate(
                (tick + 1) as f64 * h,
                &drivers,
                &x,
                &[DriverBoundary {
                    supply_voltage_v: 0.0,
                    duty,
                    winding_temperature_k: 293.15,
                }; 2],
                &held.power,
                &rates,
            )
            .unwrap();
        assert!(residuals.iter().all(|r| r.abs() < 1e-7), "{residuals:?}");
        let current = duty * (x[0] + x[3]);
        let soc = old[0] - h * current / 36.0;
        let emf = 10.0 * (0.9 + 0.2 * soc - 0.15 * (1.0 - soc).powi(8));
        assert!((reading.state_of_charge - soc).abs() < 1e-11);
        assert!((reading.voltage_v - (emf - 0.4 * current)).abs() < 1e-9);
        assert!((reading.branches[0].voltage_v - (reading.voltage_v - 0.2 * current)).abs() < 1e-9);
        for i in 0..2 {
            let branch = &reading.branches[i + 1];
            let branch_i = duty * x[i * 3];
            assert!((branch.current_a - branch_i).abs() < 1e-12);
            assert!(
                (branch.voltage_v - (reading.branches[0].voltage_v - [0.3, 0.1][i] * branch_i))
                    .abs()
                    < 1e-9
            );
            assert!(branch.wiring_loss_w >= -1e-12);
        }
        let wiring: f64 = reading.branches.iter().map(|b| b.wiring_loss_w).sum();
        let bridge_input: f64 = driver
            .iter()
            .enumerate()
            .map(|(i, d)| reading.branches[i + 1].voltage_v * d.supply_current_a)
            .sum();
        assert!((reading.power_w - wiring - bridge_input).abs() < 1e-9);
        assert!(
            (reading.terminal_energy_j - old.last().unwrap() - h * reading.power_w).abs() < 1e-10
        );
        regenerated |= current < 0.0;
    }
    assert!(
        regenerated,
        "reversal must exercise signed charge/energy accounting"
    );
    assert!(g.q.iter().all(|q| q.abs() > 1e-4));
}

#[test]
fn topology_requires_exact_coverage_acyclic_feeds_and_explicit_battery_parameters() {
    let (_, _, motors, _, _, config) = fixture();
    assert!(EmbeddedPowerBank::new(&motors, &config).is_ok());
    for key in config.battery.keys() {
        let mut bad = config.clone();
        bad.battery.remove(key);
        assert!(
            EmbeddedPowerBank::new(&motors, &bad).is_err(),
            "missing {key}"
        );
    }
    let mut cases = vec![];
    let mut bad = config.clone();
    bad.branches[0].parent = Some("a".into());
    cases.push(bad);
    let mut bad = config.clone();
    bad.branches[0].parent = Some("missing".into());
    cases.push(bad);
    let mut bad = config.clone();
    bad.branches[2].id = "a".into();
    cases.push(bad);
    let mut bad = config.clone();
    bad.branches[2].motors.clear();
    cases.push(bad);
    let mut bad = config.clone();
    bad.branches[2].motors = vec!["joint.a".into()];
    cases.push(bad);
    let mut bad = config.clone();
    bad.branches[2].motors = vec!["missing".into()];
    cases.push(bad);
    let mut bad = config.clone();
    bad.branches[2].resistance_ohm = -0.1;
    cases.push(bad);
    for bad in cases {
        assert!(EmbeddedPowerBank::new(&motors, &bad).is_err());
    }
    let mut ideal = config;
    ideal.branches[0].resistance_ohm = 0.0;
    assert!(EmbeddedPowerBank::new(&motors, &ideal).is_ok());
}

#[test]
fn simultaneous_motor_load_reduces_other_motor_speed_and_separate_feeds_reduce_sag() {
    let run = |second_duty, trunk_resistance| {
        let (art, mut g, mut motors, drivers, mut servos, mut config) = fixture();
        config.branches[0].resistance_ohm = trunk_resistance;
        let power = EmbeddedPowerBank::new(&motors, &config).unwrap();
        let map = RigidEmbedding::new(&art, &motors.dof_names(), Default::default()).unwrap();
        let inputs = [ServoBoundary {
            target_rad: 0.0,
            supply_voltage_v: 0.0,
            winding_temperature_k: 293.15,
        }; 2];
        let mut state = PoweredServoState {
            servo: servos.initial_states(),
            power: power.initial_states(),
        };
        let layout = servos.state_layout();
        state.servo[layout[0].0] = 0.6;
        state.servo[layout[1].0] = second_duty;
        let mut x = motors.initial_states();
        let mut control = servos
            .connect(&motors, &drivers, &inputs)
            .unwrap()
            .with_power(&power)
            .unwrap();
        for tick in 0..20 {
            let result = motors
                .advance_with_control(
                    &map,
                    &g,
                    &x,
                    &state,
                    tick as f64 * 0.0002,
                    0.0002,
                    &Default::default(),
                    &Default::default(),
                    &mut control,
                    |_, _| Ok(vec![0.0; 2]),
                )
                .unwrap();
            state = result.control_state;
            x = result.motor.motor_states;
            g = result.motor.endpoint.generalized;
        }
        (state.power[3], g.qd[0], x[0])
    };
    let alone = run(0.0, 0.2);
    let simultaneous = run(0.6, 0.2);
    let separate_feeds = run(0.6, 0.0);
    assert!(
        simultaneous.0 < alone.0 && simultaneous.1 < alone.1 && simultaneous.2 < alone.2,
        "shared load must reduce voltage, speed and current: {simultaneous:?} vs {alone:?}"
    );
    assert!(
        separate_feeds.0 > simultaneous.0 && separate_feeds.1 > simultaneous.1,
        "removing shared wiring resistance must improve motor response"
    );
}

#[test]
fn fpga_controller_preserves_subperiod_pwm_delay_with_shared_power() {
    let (art, g, mut motors, drivers, _, config) = fixture();
    let names = motors.dof_names();
    let power = EmbeddedPowerBank::new(&motors, &config).unwrap();
    let map = RigidEmbedding::new(&art, &names, Default::default()).unwrap();
    let configs: Vec<_> = names
        .iter()
        .map(|name| EmbeddedServoConfig {
            dof: name.clone(),
            parameters: [
                ("kp_q8", 256.0),
                ("kd_q8", 0.0),
                ("kv_q8", 0.0),
                ("limit", 600.0),
                ("period", 0.001),
                ("latency", 0.00025),
                ("offset", 0.0),
                ("encoder_quantum", std::f64::consts::TAU / 4096.0),
                ("encoder_zero", 2048.0),
                ("encoder_direction", 1.0),
                ("initial_target", 0.0),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect(),
        })
        .collect();
    let mut servos =
        EmbeddedServoBank::new_with_kinds(&motors, &configs, &["control.sampled_fixed_pd"; 2])
            .unwrap();
    let layout = servos.state_layout();
    let held = PoweredServoState {
        servo: servos.initial_states(),
        power: power.initial_states(),
    };
    let inputs = [ServoBoundary {
        target_rad: 0.1,
        supply_voltage_v: 0.0,
        winding_temperature_k: 293.15,
    }; 2];
    let initial = motors.initial_states();
    let mut control = servos
        .connect(&motors, &drivers, &inputs)
        .unwrap()
        .with_power(&power)
        .unwrap();
    let at_delivery = motors
        .advance_with_control(
            &map,
            &g,
            &initial,
            &held,
            0.0,
            0.00025,
            &Default::default(),
            &Default::default(),
            &mut control,
            |_, _| Ok(vec![0.0; 2]),
        )
        .unwrap();
    // The integer command arrives at the exact deadline. There was no winding
    // current, motion or terminal energy during the preceding delay interval.
    assert!(
        at_delivery
            .motor
            .motor_states
            .iter()
            .all(|v| v.abs() < 1e-12)
    );
    assert!(at_delivery.control_state.power.last().unwrap().abs() < 1e-12);
    assert!(
        layout
            .iter()
            .all(|(i, _, _)| at_delivery.control_state.servo[*i] > 0.0)
    );
    let after = motors
        .advance_with_control(
            &map,
            &at_delivery.motor.endpoint.generalized,
            &at_delivery.motor.motor_states,
            &at_delivery.control_state,
            0.00025,
            0.00025,
            &Default::default(),
            &Default::default(),
            &mut control,
            |_, _| Ok(vec![0.0; 2]),
        )
        .unwrap();
    assert!(
        motors
            .state_layout()
            .iter()
            .all(|(i, _, _)| after.motor.motor_states[*i] > 0.0)
    );
    assert!(after.control_state.power[0] < held.power[0]);
    assert!(after.control_state.power[1] < held.power[1]);
    assert!(*after.control_state.power.last().unwrap() > 0.0);
}

#[test]
fn power_checkpoints_replay_and_failed_intervals_do_not_commit_charge_or_energy() {
    let (art, g, mut motors, drivers, _, config) = fixture();
    let power = EmbeddedPowerBank::new(&motors, &config).unwrap();
    let map = RigidEmbedding::new(&art, &motors.dof_names(), Default::default()).unwrap();
    let mut servos = EmbeddedServoBank::new(
        &motors,
        &motors
            .dof_names()
            .iter()
            .map(|name| EmbeddedServoConfig {
                dof: name.clone(),
                parameters: [
                    ("rate".into(), 1000.0),
                    ("offset".into(), 0.001),
                    ("kp".into(), 1.0),
                ]
                .into(),
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let inputs = [ServoBoundary {
        target_rad: 0.6,
        supply_voltage_v: 0.0,
        winding_temperature_k: 293.15,
    }; 2];
    let mut held = PoweredServoState {
        servo: servos.initial_states(),
        power: power.initial_states(),
    };
    for (start, _, _) in servos.state_layout() {
        held.servo[start] = 0.6;
    }
    let initial = motors.initial_states();
    let checkpoint = serde_json::to_string(&held).unwrap();
    let mut workspace = ImplicitSolverWorkspace::default();
    let implicit = ImplicitStepConfig::default();
    let hybrid = sim_dynamics::hybrid::HybridConfig::default();
    let mut control = servos
        .connect(&motors, &drivers, &inputs)
        .unwrap()
        .with_power(&power)
        .unwrap();
    let failure = motors.advance_with_control_cached(
        &map,
        &g,
        &initial,
        &held,
        0.0,
        0.004,
        &implicit,
        &hybrid,
        &mut control,
        &mut workspace,
        |t, _| {
            if t >= 0.002 {
                Err("injected failure".into())
            } else {
                Ok(vec![0.0; 2])
            }
        },
    );
    assert!(failure.is_err());
    assert_eq!(serde_json::to_string(&held).unwrap(), checkpoint);
    let full = motors
        .advance_with_control_cached(
            &map,
            &g,
            &initial,
            &held,
            0.0,
            0.004,
            &implicit,
            &hybrid,
            &mut control,
            &mut workspace,
            |_, _| Ok(vec![0.0; 2]),
        )
        .unwrap();
    let first = motors
        .advance_with_control(
            &map,
            &g,
            &initial,
            &serde_json::from_str(&checkpoint).unwrap(),
            0.0,
            0.002,
            &implicit,
            &hybrid,
            &mut control,
            |_, _| Ok(vec![0.0; 2]),
        )
        .unwrap();
    let resumed =
        serde_json::from_str(&serde_json::to_string(&first.control_state).unwrap()).unwrap();
    let second = motors
        .advance_with_control(
            &map,
            &first.motor.endpoint.generalized,
            &first.motor.motor_states,
            &resumed,
            0.002,
            0.002,
            &implicit,
            &hybrid,
            &mut control,
            |_, _| Ok(vec![0.0; 2]),
        )
        .unwrap();
    for (a, b) in full
        .motor
        .motor_states
        .iter()
        .zip(&second.motor.motor_states)
    {
        assert!((a - b).abs() < 1e-10);
    }
    for (a, b) in full
        .control_state
        .power
        .iter()
        .zip(&second.control_state.power)
    {
        assert!((a - b).abs() < 1e-10);
    }
    drop(control);
    // The final sample changes duty instantaneously. Its reported terminal and
    // branch voltages must already satisfy KCL, without waiting one more step.
    let commands = servos
        .commands(
            0.004,
            &full.motor.endpoint.generalized,
            &full.control_state.servo,
            &inputs,
        )
        .unwrap();
    let driver_inputs: Vec<_> = commands
        .into_iter()
        .map(|duty| DriverBoundary {
            duty,
            supply_voltage_v: 0.0,
            winding_temperature_k: 293.15,
        })
        .collect();
    let states = &full.control_state.power;
    let reading = power
        .evaluate(
            0.004,
            &drivers,
            &full.motor.motor_states,
            &driver_inputs,
            states,
            &vec![0.0; states.len()],
        )
        .unwrap();
    assert!(
        reading.2[1..states.len() - 1]
            .iter()
            .all(|r| r.abs() < 1e-9)
    );
    let mut projected = states.clone();
    projected[1] += 0.1;
    power
        .reconcile(
            0.004,
            &drivers,
            &full.motor.motor_states,
            &driver_inputs,
            &mut projected,
        )
        .unwrap();
    assert_eq!(projected[0], states[0]);
    assert_eq!(projected.last(), states.last());
}
