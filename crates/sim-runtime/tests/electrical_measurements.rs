use sim_runtime::{
    controller_refinement::{control::*, electrical_measurements as em, power, recording::*},
    experiment_study,
};
use std::{collections::BTreeMap, sync::atomic::AtomicBool};
fn recording() -> Recording {
    let mut e = Experiment::default();
    e.duration_s = 0.2;
    e.timing.observation_delay_ticks = 0;
    e.timing.command_delay_ticks = 0;
    e.trajectory = vec![
        Knot {
            time_s: 0.,
            position_rad: 0.,
        },
        Knot {
            time_s: 0.2,
            position_rad: 0.,
        },
    ];
    e.electrical = Some(power::Controller {
        sensing: power::Sensing {
            voltage_quantum_v: 0.1,
            supply_current_quantum_a: None,
            winding_current_quantum_a: None,
            evidence: "Synthetic voltage register fixture".into(),
        },
        nominal_voltage_for_compensation_v: Some(12.),
        limits: Default::default(),
    });
    let mut controller = ControllerSession::new(e.clone()).unwrap();
    let frames = (0..8)
        .map(|i| {
            let t = 0.01 + i as f64 * 0.02;
            let f = controller
                .tick(
                    t + 0.002,
                    Feedback {
                        encoder_rad: 0.,
                        observed_s: t,
                        request_s: t - 0.001,
                        completion_s: t + 0.001,
                        received_s: t + 0.001,
                        electrical: Some(power::Observation {
                            supply_voltage_v: 12.,
                            supply_current_a: None,
                            winding_current_a: None,
                        }),
                    },
                )
                .unwrap();
            MeasuredFrame {
                control: f,
                command_request_s: t + 0.003,
                command_receipt_s: t + 0.004,
                drive_counts: 0,
                voltage_v: 12.,
                temperature_c: 25.,
                current_raw_uncalibrated: 999,
            }
        })
        .collect();
    Recording {
        version: 1,
        experiment: e,
        runtime: sim_runtime::physics_context::RuntimeIdentity::current(),
        frames,
        stop_request_s: 0.18,
        stop_receipt_s: 0.19,
        completed: true,
        failure: None,
        stop_verified: true,
        initial_registers: serde_json::json!({"synthetic":true}),
        transactions_origin_host_s: 0.,
        timing_evidence: "Synthetic shared-clock measurement windows".into(),
        source_hashes: BTreeMap::from([(
            "synthetic test fixture".into(),
            blake3::hash(b"electrical test fixture")
                .to_hex()
                .to_string(),
        )]),
    }
}
fn prediction(r: &Recording, purpose: Purpose) -> Prediction {
    let mut model = experiment_study::baseline().unwrap().0;
    model.power = Some(power::Setup {
        source_component: "electrical.voltage_source".into(),
        source_parameters: BTreeMap::from([("voltage".into(), 12.03)]),
        auxiliary_current_a: 0.234,
        evidence: "Synthetic constant supply and load for analytic test".into(),
        limits: Default::default(),
    });
    predict(
        r,
        &model,
        purpose,
        &sim_runtime::experiment_comparison::Limits {
            rmse: 0.001,
            final_abs_error: 0.001,
        },
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap()
}
#[test]
fn electrical_prediction_keeps_feedback_quantized_and_raw_current_unconverted() {
    let r = recording();
    r.validate().unwrap();
    for purpose in [
        Purpose::RecordedCommandReplay,
        Purpose::ClosedLoopPrediction,
    ] {
        let p = prediction(&r, purpose);
        p.validate(&r).unwrap();
        assert!(
            (p.electrical
                .as_ref()
                .unwrap()
                .samples
                .last()
                .unwrap()
                .supply_voltage_v
                - 12.03)
                .abs()
                < 1e-10
        );
        for f in &p.simulated_frames {
            let o = f.observation.electrical.as_ref().unwrap();
            assert_eq!(o.supply_voltage_v, 12.);
            assert!(o.supply_current_a.is_none());
        }
        let m = em::Measurements::servo_voltage(&r).unwrap();
        let e = em::evaluate(&m, &p).unwrap();
        e.validate(&p).unwrap();
        assert_eq!(e.channels.len(), 1);
        assert!(e.supply_energy.is_none());
        assert!(e.channels[0].passes.is_none());
        assert!((e.channels[0].rmse - 0.03).abs() < 1e-10);
    }
}
#[test]
fn calibrated_synchronized_measurements_compare_amps_watts_and_energy_and_reject_tampering() {
    let r = recording();
    let p = prediction(&r, Purpose::RecordedCommandReplay);
    let mut m = em::Measurements::servo_voltage(&r).unwrap();
    let v = &mut m.channels[0];
    v.calibration.gain = 0.01;
    v.calibration.sensor = "Synthetic external voltage sensor".into();
    for s in &mut v.raw_samples {
        s.value = 1203.;
    }
    let mut i = v.clone();
    i.name = "supply_current".into();
    i.calibration.gain = 0.001;
    i.calibration.sensor = "Synthetic supply shunt ADC".into();
    for s in &mut i.raw_samples {
        s.value = 234.;
    }
    m.channels.push(i);
    for name in ["supply_voltage", "supply_current", "supply_power"] {
        m.limits.insert(
            name.into(),
            sim_runtime::experiment_comparison::Limits {
                rmse: 1e-8,
                final_abs_error: 1e-8,
            },
        );
    }
    m.validate_recording(&r).unwrap();
    let evaluation = em::evaluate(&m, &p).unwrap();
    evaluation.validate(&p).unwrap();
    assert_eq!(evaluation.channels.len(), 3);
    assert!(evaluation.channels.iter().all(|c| c.passes == Some(true)));
    let e = evaluation.supply_energy.as_ref().unwrap();
    assert!((e.measured_drawn_j - 12.03 * 0.234 * 0.14).abs() < 1e-10);
    assert!((e.predicted_drawn_j - e.measured_drawn_j).abs() < 1e-8);
    assert_eq!(e.measured_returned_j, 0.);
    let mut changed = evaluation.clone();
    changed.measurements.channels[1].calibration.gain *= 2.;
    assert!(changed.validate(&p).is_err());
    let mut shifted = m.clone();
    shifted.channels[1].raw_samples[0].time_s += 0.00001;
    assert!(shifted.validate().is_err());
    let mut extended = m.clone();
    for c in &mut extended.channels {
        let t = c.raw_samples.last().unwrap().clone();
        c.raw_samples
            .push(sim_runtime::experiment_comparison::Observation {
                time_s: 0.185,
                request_s: 0.184,
                completion_s: 0.186,
                ..t
            });
    }
    assert!(
        em::evaluate(&extended, &p)
            .unwrap_err()
            .contains("extrapolate")
    );
    let mut missing = m.clone();
    missing.channels[1].calibration.uncertainty.clear();
    assert!(missing.validate().is_err());
    let mut swapped = m.clone();
    swapped.channels[1].name = "winding_current".into();
    assert!(
        swapped.validate().is_err(),
        "supply-current/power limits cannot refer to a winding channel"
    );
}

#[test]
fn imposed_voltage_is_not_independent_validation() {
    let r=recording();let model=experiment_study::baseline().unwrap().0;
    let mut measured=em::Measurements::servo_voltage(&r).unwrap();
    measured.limits.insert("supply_voltage".into(),sim_runtime::experiment_comparison::Limits{rmse:0.01,final_abs_error:0.01});
    let predicted=predict_with_voltage(&r,&model,Purpose::RecordedCommandReplay,&sim_runtime::experiment_comparison::Limits{rmse:0.01,final_abs_error:0.01},&measured,&AtomicBool::new(false),|_,_|{}).unwrap();
    predicted.validate(&r).unwrap();
    let comparison=em::evaluate(&measured,&predicted).unwrap();
    assert_eq!(comparison.channels.iter().find(|c|c.name=="supply_voltage").unwrap().passes,None);
    assert!(comparison.method.contains("imposed waveform input"));
}
