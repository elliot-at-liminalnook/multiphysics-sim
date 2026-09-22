use sim_runtime::controller_refinement::fpga::*;
#[test]
fn explicit_physical_scope_includes_every_active_motor() {
    validate_physical_scope(&[10, 11, 12], &[10, 11, 12]).unwrap();
    validate_physical_scope(&[10, 11, 12], &[12]).unwrap();
    validate_physical_scope(&(4..=12).collect::<Vec<_>>(), &[10, 11, 12]).unwrap();
    for scope in [
        vec![],
        vec![10, 10, 12],
        vec![12, 11, 10],
        vec![3, 10, 11, 12],
        vec![10, 11, 12, 13],
        vec![10, 12],
    ] {
        assert!(validate_physical_scope(&scope, &[10, 11, 12]).is_err());
    }
    assert!(validate_physical_scope(&[10, 11, 12], &[]).is_err());
    assert!(validate_physical_scope(&[10, 11, 12], &[12, 12]).is_err());
}
fn recording() -> Recording {
    let mut r = Recording {
        version: 1,
        plan: Plan {
            control: "fpga_pd".into(),
            name: "Synthetic zero-action parity fixture".into(),
            role: "timing".into(),
            ids: vec![4],
            period_s: 0.1,
            gains: sim_domain_control::fixed_pd::Gains {
                kp_q8: 256,
                kd_q8: 128,
                kv_q8: 256,
                limit: 50,
            },
            targets: vec![[0; 9]; 3],
            rms_limit_counts: 3.,
            peak_limit_counts: 10.,
            bitstream_path: "synthetic".into(),
            bitstream_blake3: "0".repeat(64),
        },
        home: [2048; 9],
        frames: vec![],
        completed: true,
        failure: None,
        stop_verified: true,
        stop_request_s: 0.3,
        stop_receipt_s: 0.31,
        transactions_origin_host_s: 0.,
        sources: Default::default(),
        initial: serde_json::json!({"synthetic":true}),
        recovery: serde_json::json!({}),
    };
    for tick in 0..3 {
        let t = tick as f64 * 0.1;
        let mut raw = [0u8; 15];
        raw[1] = 8;
        raw[6] = 120;
        raw[7] = 25;
        r.frames.push(Frame {
            tick,
            observations: vec![Observation {
                id: 4,
                request_s: t + 0.01,
                completion_s: t + 0.02,
                telemetry: sim_runtime::acquisition::servo_bus::Telemetry::decode(&raw).unwrap(),
            }],
            command_request_s: t + 0.03,
            command_receipt_s: t + 0.04,
            pwm_readback: [0; 9],
            torque_readback: Some([1; 9]),
            arithmetic_matches: true,
        });
    }
    r
}
#[test]
fn measured_voltage_replay_preserves_source_and_changes_actual_plant_response() {
    use sim_runtime::controller_refinement::{fpga_review::{Mode, Prediction}, fpga_voltage};
    use std::sync::atomic::AtomicBool;
    let mut r = recording();
    r.plan.gains.kp_q8 = 4096;
    r.plan.gains.kd_q8 = 0;
    r.plan.gains.kv_q8 = 0;
    r.plan.targets[1][0] = 20;
    r.plan.targets[2][0] = 20;
    r.frames[1].pwm_readback[0] = 50;
    r.frames[2].pwm_readback[0] = 50;
    // Equal mean to the fixed 10 V comparison, but a falling measured rail.
    for (f, v) in r.frames.iter_mut().zip([12., 10., 8.]) {
        f.observations[0].telemetry.voltage_v = v;
        f.observations[0].telemetry.voltage_raw = (v * 10.) as u8;
    }
    r.validate().unwrap();
    let model = sim_runtime::experiment_study::baseline().unwrap().0;
    let unchanged = model.clone();
    let fixed = predict(&r, 4, &model, false).unwrap();
    let p = fpga_voltage::predict(&r, 4, &model, Mode::ReplayMeasuredVoltage,
        &AtomicBool::new(false), |_, _| {}).unwrap();
    assert_eq!(model, unchanged);
    assert!(p.voltage_input.as_ref().unwrap().replaced_source.is_none());
    assert!(p.electrical.iter().any(|s| s.supply_voltage_v > 11.));
    assert!(p.electrical.iter().any(|s| s.supply_voltage_v < 9.));
    let fixed_angle = fixed["samples_time_encoder_duty_angle"][2][3].as_f64().unwrap();
    assert!((p.samples_time_encoder_duty_angle[2][3] - fixed_angle).abs() > 1e-5,
        "measured sag must reach the actual plant, not only metadata");
    let encoded = serde_json::to_vec(&p).unwrap();
    let mut damaged: Prediction = serde_json::from_slice(&encoded).unwrap();
    damaged.model.power.as_mut().unwrap().source_parameters.insert("voltage.1".into(), 12.);
    assert!(damaged.validate(&r).is_err());
    let mut damaged: Prediction = serde_json::from_slice(&encoded).unwrap();
    damaged.voltage_input.as_mut().unwrap().motor_id = 5;
    assert!(damaged.validate(&r).is_err());
    let mut damaged: Prediction = serde_json::from_slice(&encoded).unwrap();
    damaged.voltage_input = None;
    assert!(damaged.validate(&r).is_err());
    let mut damaged: Prediction = serde_json::from_slice(&encoded).unwrap();
    damaged.electrical.clear();
    assert!(damaged.validate(&r).is_err());
    let closed = fpga_voltage::predict(&r, 4, &model, Mode::ClosedLoopMeasuredVoltage,
        &AtomicBool::new(false), |_, _| {}).unwrap();
    assert!(closed.samples_time_encoder_duty_angle.iter().any(|s| s[1] != 2048.),
        "closed-loop feedback must come from evolving simulated position");
    assert!(fpga_voltage::predict(&r, 4, &model, Mode::ClosedLoopMeasuredVoltage,
        &AtomicBool::new(true), |_, _| panic!("cancelled run advanced")).is_err());
}

#[test]
fn measured_voltage_family_review_roundtrips_and_does_not_reassign_trial_roles() {
    use sim_runtime::controller_refinement::{fpga_review::*, workspace::Workspace, fpga_voltage};
    use std::sync::atomic::AtomicBool;
    let r = recording();
    let model = sim_runtime::experiment_study::baseline().unwrap().0;
    let review = review(&r, &model, Mode::ClosedLoopMeasuredVoltage,
        &AtomicBool::new(false), |_, _| {}).unwrap();
    assert_eq!(review.predictions.len(), 1, "{:?}", review.failures);
    let mut workspace = Workspace::default();
    workspace.fpga_recordings.push(r.clone());
    workspace.fpga_reviews.push(review);
    let decoded: Workspace = serde_json::from_slice(&serde_json::to_vec(&workspace).unwrap()).unwrap();
    decoded.validate().unwrap();
    use sim_runtime::controller_refinement::calibration_data::CalibrationData;
    assert!(Dataset { recordings: vec![r.clone()], measured_voltage: true }.cases().is_err());
    let mut training = r.clone();
    training.plan.role = "training".into();
    let legacy: Dataset = serde_json::from_value(serde_json::json!({"recordings":[training]})).unwrap();
    assert!(!legacy.measured_voltage);
    assert!(serde_json::to_value(&legacy).unwrap().get("measured_voltage").is_none());
    let mut conditioned = legacy.clone();
    conditioned.measured_voltage = true;
    assert_ne!(legacy.fingerprint(), conditioned.fingerprint());
    assert_eq!(legacy.cases().unwrap()[0].id, conditioned.cases().unwrap()[0].id);
    let trace = conditioned.predict(&conditioned.cases().unwrap()[0].id, &model,
        &AtomicBool::new(false)).unwrap();
    assert_eq!(trace.samples.len(), r.frames.len());
    let mut powered = model;
    powered.power = Some(sim_runtime::controller_refinement::power::Setup {
        source_component: "electrical.voltage_source".into(),
        source_parameters: std::collections::BTreeMap::from([("voltage".into(), 11.)]),
        auxiliary_current_a: 0.02,
        evidence: "synthetic source, not a battery measurement".into(),
        limits: Default::default(),
    });
    let (resolved, conditioning) = fpga_voltage::condition_model(&r, 4, &powered).unwrap();
    assert_eq!(conditioning.replaced_source, powered.power);
    assert_eq!(resolved.power.unwrap().auxiliary_current_a, 0.02);
}
#[test]
fn validates_actual_pwm_instead_of_trusting_audit_flag() {
    let mut r = recording();
    r.validate().unwrap();
    r.frames[1].pwm_readback[0] = 1;
    assert!(r.validate().is_err());
}
#[test]
fn packet_contains_targets_and_gains_not_host_drive() {
    let r = recording();
    let p = r.plan.parameters(0, &r.home).unwrap();
    assert_eq!(p.len(), 46);
    assert_eq!(&p[..4], &[1, 0, 0, 1]);
    assert_eq!(&p[10..14], &[0, 8, 0, 0]);
}
#[test]
fn captured_schedule_runs_same_controller_against_shared_plant() {
    let r = recording();
    let model = sim_runtime::experiment_study::baseline().unwrap().0;
    let p = predict(&r, 4, &model, true).unwrap();
    assert_eq!(
        p["samples_time_encoder_duty_angle"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(p["rms_prediction_counts"], 0.);
    let replay = predict(&r, 4, &model, false).unwrap();
    assert_eq!(
        p["samples_time_encoder_duty_angle"],
        replay["samples_time_encoder_duty_angle"]
    );
}

#[test]
fn fpga_review_cancellation_and_saved_evidence_validation() {
    use sim_runtime::controller_refinement::{fpga_review::*, workspace::Workspace};
    use std::sync::atomic::AtomicBool;
    let r = recording();
    let model = sim_runtime::experiment_study::baseline().unwrap().0;
    let cancelled = review(
        &r,
        &model,
        Mode::ClosedLoop,
        &AtomicBool::new(true),
        |_, _| {},
    )
    .unwrap();
    assert!(cancelled.cancelled);
    assert!(cancelled.predictions.is_empty());
    assert_eq!(cancelled.failures.len(), 1);
    let result = review(
        &r,
        &model,
        Mode::ClosedLoop,
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(result.predictions.len(), 1, "{:?}", result.failures);
    let mut w = Workspace::default();
    w.fpga_recordings.push(r);
    w.fpga_reviews.push(result);
    w.validate().unwrap();
    let encoded = serde_json::to_vec(&w).unwrap();
    let mut decoded: Workspace = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    decoded.fpga_reviews[0].predictions[0].rms_prediction_counts = 10.;
    assert!(decoded.validate().is_err());
    let mut decoded: Workspace = serde_json::from_slice(&encoded).unwrap();
    decoded.fpga_recordings[0].plan.role = "training".into();
    assert!(decoded.validate().is_err());
    let mut decoded: Workspace = serde_json::from_slice(&encoded).unwrap();
    decoded.fpga_reviews[0].predictions.clear();
    assert!(decoded.validate().is_err());
}

#[test]
fn actual_nine_motor_reviews_roundtrip_and_export_without_promoting_accuracy() {
    use sim_runtime::controller_refinement::{fpga_review::*, workspace::Workspace};
    use sim_runtime::{experiment_comparison::hx_archive, experiment_study::Study};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let base = root
        .join("examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control");
    let r: Recording = serde_json::from_slice(
        &std::fs::read(base.join("nine-faster-7p5pct-validation/fpga-recording.json")).unwrap(),
    )
    .unwrap();
    r.validate().unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &std::fs::read(base.join("nine-faster-closed-loop-prediction.json")).unwrap(),
    )
    .unwrap();
    let predictions: Vec<Prediction> = serde_json::from_value(v["predictions"].clone()).unwrap();
    assert_eq!(predictions.len(), 9);
    assert!(predictions.iter().all(|p| !p.prediction_pass));
    let review = Review {
        recording_hash: r.fingerprint(),
        models: predictions
            .iter()
            .map(|p| (p.id, p.model.clone()))
            .collect(),
        mode: Mode::ClosedLoop,
        predictions,
        failures: vec![],
        cancelled: false,
    };
    review.validate(&r).unwrap();
    let archive = hx_archive::load(
        &root.join("examples/actuators/hx30hm/pwm-identification"),
        &root,
    )
    .unwrap();
    let mut study = Study::new(archive).unwrap();
    study.refinement = Workspace::default();
    study.refinement.fpga_recordings.push(r);
    study.refinement.fpga_reviews.push(review);
    let incomplete: Recording =
        serde_json::from_slice(&std::fs::read(base.join("zero-nine/fpga-recording.json")).unwrap())
            .unwrap();
    incomplete.validate_capture().unwrap();
    assert!(incomplete.validate().is_err());
    study.refinement.fpga_recordings.push(incomplete);
    let dir = std::env::temp_dir().join(format!("fpga-study-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("review.json");
    study.save_new(&path).unwrap();
    let loaded = Study::load(&path).unwrap();
    assert_eq!(loaded.refinement.fpga_recordings.len(), 2);
    loaded.export_html_new(&dir.join("review.html")).unwrap();
    let html = std::fs::read_to_string(dir.join("review.html")).unwrap();
    assert!(html.contains("Own-feedback closed loop"));
    assert!(html.contains("Unscored acquisition"));
    assert!(html.contains("Model prediction scores are separate"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn controller_design_is_separate_from_measurement_and_preserves_execution_parity() {
    use sim_runtime::controller_refinement::{
        calibration::Family, fpga_design, workspace::Workspace,
    };
    use std::sync::atomic::AtomicBool;
    let r = recording();
    let hash = r.fingerprint();
    let model = sim_runtime::experiment_study::baseline().unwrap().0;
    let family = Family {
        shared: model.clone(),
        device_deltas: Default::default(),
    };
    let mut e = fpga_design::Experiment {
        timing_recording_hash: hash.clone(),
        plan: r.plan.clone(),
    };
    let parity =
        fpga_design::simulate(&e, &r, &family, &AtomicBool::new(false), |_, _| {}).unwrap();
    let prediction = predict(&r, 4, &model, true).unwrap();
    assert_eq!(
        serde_json::to_value(&parity.axes[0].simulation.samples).unwrap(),
        prediction["samples_time_encoder_duty_angle"]
    );
    e.plan.targets[1][0] = 8;
    e.plan.targets[2][0] = -8;
    let designed =
        fpga_design::simulate(&e, &r, &family, &AtomicBool::new(false), |_, _| {}).unwrap();
    assert_eq!(designed.axes.len(), 1, "{:?}", designed.failures);
    assert!(designed.axes[0].tracking.rms_counts > 0.);
    assert_eq!(r.fingerprint(), hash);
    let mut workspace = Workspace::default();
    workspace.fpga_recordings.push(r.clone());
    workspace.fpga_design_runs.push(designed);
    workspace.fpga_design_drafts.push(e.clone());
    workspace.validate().unwrap();
    let mut bad_draft = workspace.clone();
    bad_draft.fpga_design_drafts[0].plan.ids.clear();
    assert!(bad_draft.validate().is_err());
    let json = serde_json::to_string(&workspace).unwrap();
    let mut loaded: Workspace = serde_json::from_str(&json).unwrap();
    loaded.validate().unwrap();
    loaded.fpga_design_runs[0].axes[0].tracking.rms_counts = 0.;
    assert!(loaded.validate().is_err());
    e.plan.gains.limit += 1;
    assert!(e.validate(&r).is_err());
}

#[test]
fn profile_compiler_rejects_aliasing_and_produces_bounded_rest_to_rest_targets() {
    use sim_runtime::controller_refinement::fpga_design::Profile;
    let mut p = recording().plan;
    p.targets = vec![[0; 9]; 81];
    p.ids = (4..=12).collect();
    let profile = Profile::default();
    let generated = profile.apply(&p).unwrap();
    assert_eq!(generated.targets.first(), Some(&[0; 9]));
    assert_eq!(generated.targets.last(), Some(&[0; 9]));
    assert!(generated.targets.iter().any(|v| v[0] != v[1]));
    let mut invalid = profile.clone();
    invalid.frequency_hz = 5.;
    assert!(invalid.apply(&p).is_err());
    invalid = profile;
    invalid.amplitude_counts = f64::NAN;
    assert!(invalid.apply(&p).is_err());
}
