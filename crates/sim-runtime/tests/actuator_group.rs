use sim_core::FnCoupler;
use sim_runtime::{
    actuator_bench::{self, AxisSetup, GroupSetup},
    controller_refinement::{calibration::Family, power},
};
use std::collections::BTreeMap;

fn setup(n: usize, resistance: f64) -> GroupSetup {
    let family:Family=serde_json::from_str(include_str!("../../../examples/actuators/hx30hm/hardware/2026-09-14-unloaded-refinement/baseline-family.json")).unwrap();
    GroupSetup {
        source: power::Setup {
            source_component: sim_domain_electrical::elements::VOLTAGE_SOURCE.into(),
            source_parameters: BTreeMap::from([("voltage".into(), 12.)]),
            auxiliary_current_a: 0.2,
            evidence: "Synthetic circuit test; not measured supply parameters".into(),
            limits: Default::default(),
        },
        shared_resistance_ohm: resistance,
        axes: (0..n)
            .map(|i| AxisSetup {
                key: i.to_string(),
                model: family.shared.clone(),
                temperature_c: 25.,
                command_and_sample_times_s: vec![0., 0.01],
                branch_resistance_ohm: 0.1,
            })
            .collect(),
        evidence: "Synthetic independent shafts with shared supply".into(),
        seed: 0,
    }
}
fn run(n: usize, resistance: f64, step: f64) -> (f64, f64, f64) {
    let setup = setup(n, resistance);
    let mut bench = actuator_bench::prepare_group(&setup).unwrap();
    for axis in bench.axes.values() {
        bench
            .runtime
            .bind_coupler(
                axis.controller,
                Box::new(FnCoupler(|_t: f64, _s: &[f64], a: &mut [f64]| a[0] = 0.1)), false,
            )
            .unwrap();
    }
    let mut time = 0.;
    while time < 0.02 - 1e-12 {
        bench.runtime.advance(step, step).unwrap();
        time += step;
        let [v, i, p] = bench.supply();
        let branches = bench
            .axes
            .keys()
            .map(|key| bench.electrical_sample(key, time).unwrap())
            .collect::<Vec<_>>();
        assert!(
            (i - branches.iter().map(|s| s.supply_current_a).sum::<f64>() - 0.2).abs() < 1e-7,
            "Shared source must satisfy KCL"
        );
        assert!(
            (v - (12. - resistance * i)).abs() < 1e-7,
            "Shared feed must satisfy Ohm's law"
        );
        assert!((p - v * i).abs() < 1e-10);
        for s in branches {
            s.validate().unwrap();
            assert!((s.supply_voltage_v - (v - 0.1 * s.supply_current_a)).abs() < 1e-7);
        }
    }
    let [v, i, _] = bench.supply();
    (v, i, bench.runtime.get(bench.axes["0"].angle))
}
#[test]
fn all_axes_load_the_same_supply_with_current_and_power_accounting() {
    let one = run(1, 0.8, 0.00025);
    let nine = run(9, 0.8, 0.00025);
    assert!(nine.0 < one.0);
    assert!(nine.1 > one.1);
    assert!(nine.2 < one.2);
    let fine = run(9, 0.8, 0.000125);
    assert!((nine.0 - fine.0).abs() < 0.002);
    assert!((nine.2 - fine.2).abs() < 0.0001);
}
#[test]
fn ideal_supply_keeps_independent_axes_identical() {
    let one = run(1, 0., 0.00025);
    let nine = run(9, 0., 0.00025);
    assert!((one.2 - nine.2).abs() < 1e-9);
    assert!((nine.1 - 0.2 - 9. * (one.1 - 0.2)).abs() < 1e-8);
}
#[test]
fn zero_resistance_group_matches_existing_single_motor_fixture() {
    let mut setup = setup(1, 0.);
    setup.axes[0].branch_resistance_ohm = 0.;
    setup.source.auxiliary_current_a = 0.;
    let mut group = actuator_bench::prepare_group(&setup).unwrap();
    let mut single = actuator_bench::prepare(
        &setup.axes[0].model,
        12.,
        25.,
        actuator_bench::Drive::Scheduled {
            times: vec![0., 0.01],
        },
    )
    .unwrap();
    for (runtime, controller) in [
        (&mut group.runtime, group.axes["0"].controller),
        (&mut single.runtime, single.controller.unwrap()),
    ] {
        runtime
            .bind_coupler(
                controller,
                Box::new(FnCoupler(|t: f64, _s: &[f64], a: &mut [f64]| {
                    a[0] = if t < 0.009 { 0.1 } else { -0.1 }
                })), false,
            )
            .unwrap();
    }
    for _ in 0..80 {
        group.runtime.advance(0.00025, 0.00025).unwrap();
        single.runtime.advance(0.00025, 0.00025).unwrap();
        assert!(
            (group.runtime.get(group.axes["0"].angle) - single.runtime.get(single.angle)).abs()
                < 1e-8
        );
    }
}
#[test]
fn conflicting_conditions_and_ambiguous_axis_keys_are_rejected() {
    let mut s = setup(2, 0.);
    s.axes[1].key = s.axes[0].key.clone();
    assert!(actuator_bench::prepare_group(&s).is_err());
    let mut s = setup(1, 0.);
    s.axes[0].model.conditions.voltage_v = Some(12.);
    assert!(actuator_bench::prepare_group(&s).is_err());
    let mut s = setup(1, 0.);
    s.shared_resistance_ohm = -1.;
    assert!(actuator_bench::prepare_group(&s).is_err());
}
