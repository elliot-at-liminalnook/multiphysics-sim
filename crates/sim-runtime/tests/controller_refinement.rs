use sim_domain_control::pwm_feedback::{EncoderEstimate, Pid, PidState};
use sim_runtime::controller_refinement::control::*;
use std::sync::atomic::AtomicBool;

fn observation(t: f64, angle: f64) -> Feedback {
    Feedback {
        electrical: None,
        observed_s: t,
        received_s: t,
        encoder_rad: angle,
        request_s: t,
        completion_s: t,
    }
}

#[test]
fn encoder_unwrap_and_held_observations_do_not_invent_velocity() {
    let mut e = EncoderEstimate::default();
    let wrap = std::f64::consts::TAU;
    e.observe(0., wrap - 0.01, wrap, 0.).unwrap();
    let (p, v) = e.observe(0.02, 0.01, wrap, 0.).unwrap();
    assert!((p - wrap - 0.01).abs() < 1e-12);
    assert!((v - 1.).abs() < 1e-12);
    assert_eq!(e.observe(0.02, 0.01, wrap, 0.).unwrap(), (p, v));
    assert!(e.observe(0.01, 0., wrap, 0.).is_err());
}

#[test]
fn saturated_pid_does_not_wind_up_and_can_reverse() {
    let p = Pid {
        kp: 1.,
        ki: 1.,
        kd: 0.,
        integral_limit: 10.,
        duty_limit: 0.1,
    };
    let mut state = PidState::default();
    for _ in 0..1000 {
        assert_eq!(state.step(&p, 1., 0., 0., 0.02).unwrap().duty, 0.1);
    }
    assert_eq!(state.integral, 0.);
    assert_eq!(state.step(&p, -1., 0., 0., 0.02).unwrap().duty, -0.1);
}

#[test]
fn controller_rejects_stale_feedback_and_records_command_delay() {
    let mut e = Experiment::default();
    e.trajectory[0].position_rad = 1.;
    e.trajectory[1].position_rad = 1.;
    let mut c = ControllerSession::new(e).unwrap();
    let first = c.tick(0., observation(0., 0.)).unwrap();
    assert!(first.requested_duty > 0.);
    assert_eq!(first.applied_duty, 0.);
    assert_eq!(
        c.tick(0.02, observation(0.02, 0.)).unwrap().applied_duty,
        first.requested_duty
    );
    assert!(c.tick(0.2, observation(0.02, 0.)).is_err());
}

#[test]
fn captured_controller_runs_in_shared_physics_and_reproduces() {
    let mut e = Experiment::default();
    e.duration_s = 0.2;
    e.trajectory = vec![
        Knot {
            time_s: 0.,
            position_rad: 0.,
        },
        Knot {
            time_s: 0.02,
            position_rad: 0.02,
        },
    ];
    let model = sim_runtime::experiment_study::baseline().unwrap().0;
    let run = || simulate(&e, &model, &AtomicBool::new(false), |_, _| {}).unwrap();
    let a = run();
    let b = run();
    assert!(a.failure.is_none(), "{:?}", a.failure);
    assert!(a.frames.len() >= 9, "{} frames", a.frames.len());
    assert!(a.truth.last().unwrap()[1] > 0., "{:?}", a.truth);
    a.validate().unwrap();
    assert_eq!(a.truth, b.truth);
    assert_eq!(
        serde_json::to_value(&a.frames).unwrap(),
        serde_json::to_value(&b.frames).unwrap()
    );
    let cancelled = simulate(&e, &model, &AtomicBool::new(true), |_, _| {}).unwrap();
    assert!(cancelled.cancelled && cancelled.score.is_none());
    cancelled.validate().unwrap();
}

#[test]
fn rust_and_rhai_share_observations_and_bounded_commands() {
    let mut rust = Experiment::default();
    rust.controller = Policy::RustPid {
        parameters: Pid {
            kp: 0.8,
            ki: 0.,
            kd: 0.03,
            integral_limit: 0.,
            duty_limit: 0.2,
        },
    };
    let mut rhai = rust.clone();
    rhai.controller = Policy::Rhai { source: "fn control(t,s,a,state) { #{commands: #{duty: 0.8*(s.target-s.position)-0.03*s.velocity}, state: state} }".into(),parameters:serde_json::json!({}),duty_limit:0.2 };
    let mut a = ControllerSession::new(rust).unwrap();
    let mut b = ControllerSession::new(rhai).unwrap();
    for i in 0..40 {
        let t = i as f64 * 0.02;
        let observation = observation(t, 0.002 * i as f64);
        let ra = a.tick(t, observation.clone()).unwrap();
        let rb = b.tick(t, observation).unwrap();
        assert_eq!(ra.applied_duty, rb.applied_duty);
        assert_eq!(ra.estimated_velocity_rad_s, rb.estimated_velocity_rad_s);
    }
}

fn synthetic_archive() -> (
    sim_runtime::experiment_comparison::hx_archive::Archive,
    sim_runtime::experiment_study::ModelSettings,
) {
    use sim_runtime::{
        experiment_comparison::{compare, hx_archive},
        experiment_study,
    };
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut archive = hx_archive::load(
        &repo.join("examples/actuators/hx30hm/pwm-identification"),
        &repo,
    )
    .unwrap();
    let truth = experiment_study::baseline().unwrap().0;
    let train = archive
        .trials
        .iter()
        .find(|t| t.device == 4 && t.drive > 0. && t.split == "train")
        .unwrap()
        .clone();
    let held = archive
        .trials
        .iter()
        .find(|t| t.device == 4 && t.drive > 0. && t.split != "train")
        .unwrap()
        .clone();
    archive.trials = vec![train, held];
    for t in &mut archive.trials {
        t.measured = experiment_study::simulate(t, &truth, &AtomicBool::new(false)).unwrap();
        t.predicted = t.measured.clone();
        t.comparison = compare(&t.measured, &t.predicted, &t.limits).unwrap();
    }
    (archive, truth)
}

#[test]
fn bounded_fit_recovers_synthetic_parameter_without_using_held_out_trials() {
    use sim_runtime::controller_refinement::calibration::*;
    let (a, truth) = synthetic_archive();
    let value = truth.motor["resistance"];
    let mut initial = truth.clone();
    initial.motor.insert("resistance".into(), value * 1.2);
    let family = Family {
        shared: initial,
        device_deltas: Default::default(),
    };
    let coords = vec![Coordinate {
        path: "motor.resistance".into(),
        device: None,
        lower: value * 0.7,
        upper: value * 1.4,
    }];
    let train = vec![a.trials[0].id.clone()];
    let held = vec![a.trials[1].id.clone()];
    let cancel = AtomicBool::new(false);
    assert!(
        fit(
            &a,
            &family,
            &held,
            &train,
            &coords,
            40,
            false,
            &cancel,
            |_, _| {}
        )
        .is_err()
    );
    let report = fit(
        &a,
        &family,
        &train,
        &held,
        &coords,
        40,
        false,
        &cancel,
        |_, _| {},
    )
    .unwrap();
    let recovered = report.candidate.shared.motor["resistance"];
    assert!(
        (recovered / value - 1.).abs() < 0.02,
        "{recovered} vs {value}"
    );
    assert!(
        report
            .scores
            .iter()
            .all(|s| s.candidate.as_ref().unwrap().rmse < s.baseline.as_ref().unwrap().rmse)
    );
    let sensitivity = sensitivity(&a, &family, &train, &coords, &cancel, |_, _| {}).unwrap();
    assert_eq!(sensitivity.rank, 1);
    assert!(
        fit(
            &a,
            &family,
            &train,
            &held,
            &coords,
            40,
            false,
            &AtomicBool::new(true),
            |_, _| {}
        )
        .is_err()
    );
}

#[test]
fn recorded_commands_and_closed_loop_prediction_are_distinct_on_captured_timing() {
    use sim_runtime::controller_refinement::recording::*;
    let mut e = Experiment::default();
    e.timing.observation_delay_ticks = 0;
    e.timing.command_delay_ticks = 0;
    e.trajectory = vec![
        Knot {
            time_s: 0.,
            position_rad: 0.,
        },
        Knot {
            time_s: 0.4,
            position_rad: 0.,
        },
    ];
    e.duration_s = 0.5;
    let mut controller = ControllerSession::new(e.clone()).unwrap();
    let mut frames = vec![];
    for i in 0..8 {
        let t = 0.01 + i as f64 * 0.05;
        let feedback = Feedback {
            electrical: None,
            observed_s: t,
            request_s: t - 0.001,
            completion_s: t + 0.002,
            received_s: t + 0.002,
            encoder_rad: 0.1,
        };
        let f = controller.tick(t + 0.005, feedback).unwrap();
        let counts = (f.applied_duty * 1000.).round() as i16;
        frames.push(MeasuredFrame {
            control: f,
            command_request_s: t + 0.006,
            command_receipt_s: t + 0.01,
            drive_counts: counts,
            voltage_v: 12.,
            temperature_c: 25.,
            current_raw_uncalibrated: 0,
        });
    }
    let r = Recording {
        version: 1,
        experiment: e,
        runtime: sim_runtime::physics_context::RuntimeIdentity::current(),
        frames,
        stop_request_s: 0.45,
        stop_receipt_s: 0.46,
        completed: true,
        failure: None,
        stop_verified: true,
        initial_registers: serde_json::json!({"synthetic":true}),
        transactions_origin_host_s: 0.,
        timing_evidence:
            "Synthetic schedule; deliberately inconsistent feedback tests prediction separation"
                .into(),
        source_hashes: std::collections::BTreeMap::from([("fixture".into(), "synthetic".into())]),
    };
    r.validate().unwrap();
    let model = sim_runtime::experiment_study::baseline().unwrap().0;
    let limits = sim_runtime::experiment_comparison::Limits {
        rmse: 0.001,
        final_abs_error: 0.001,
    };
    let replay = predict(
        &r,
        &model,
        Purpose::RecordedCommandReplay,
        &limits,
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    let closed = predict(
        &r,
        &model,
        Purpose::ClosedLoopPrediction,
        &limits,
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(closed.predicted.samples.len(), r.frames.len());
    assert_ne!(closed.predicted, replay.predicted);
    assert!(closed.simulated_frames.iter().all(|f| f.applied_duty == 0.));
    assert!(!closed.model_error.passes);
    let mut tampered = r.clone();
    tampered.frames[0].drive_counts = 0;
    assert!(tampered.validate().is_err());
}

#[test]
fn cad_proposal_preserves_source_requires_review_and_feeds_shared_motor_mapping() {
    use sim_runtime::controller_refinement::cad;
    let mut motor: serde_json::Value = serde_json::from_str::<serde_json::Value>(include_str!(
        "../../../examples/actuators/hx30hm/plan.json"
    ))
    .unwrap()["motor"]
        .clone();
    motor["id"] = serde_json::json!("bench-motor");
    motor["electrical"]["back_emf_constant"] = motor["electrical"]["torque_constant"].clone();
    let source = serde_json::json!({"version":4,"motors":[motor],"source":{"artifact":"retained CAD"},"future_cad_metadata":{"preserve":true}});
    let original = source.clone();
    let baseline = sim_runtime::experiment_study::baseline().unwrap().0;
    let mut candidate = baseline.clone();
    candidate
        .motor
        .insert("resistance".into(), baseline.motor["resistance"] * 1.1);
    candidate.motor.insert(
        "no_load_current".into(),
        baseline.motor["no_load_current"] * 0.8,
    );
    let proposal = cad::propose(
        &source,
        "bench-motor",
        4,
        &baseline,
        &candidate,
        vec!["synthetic test evidence; not measured".into()],
        "Synthetic unloaded test",
    )
    .unwrap();
    // Version-1 JSON stores numeric previous values; the new optional-field
    // representation must still deserialize and accept those reviews.
    let saved = serde_json::to_value(&proposal).unwrap();
    assert_eq!(saved["version"], 1);
    assert!(saved["changes"][0]["previous"].is_number());
    let proposal: cad::Proposal = serde_json::from_value(saved).unwrap();
    assert!(proposal.accept(&source, "").is_err());
    let accepted = proposal
        .accept(
            &source,
            "Accept estimated resistance for this synthetic test",
        )
        .unwrap();
    assert_eq!(source, original);
    assert_eq!(
        accepted["future_cad_metadata"],
        source["future_cad_metadata"]
    );
    let model = sim_domain_robot::model::PhysicalModel::parse(&accepted.to_string()).unwrap();
    let parameters = sim_domain_robot::motor::cad_motor_unit_parameters(
        &model.motors[0],
        0.,
        298.15,
        true,
        false,
    );
    assert_eq!(parameters["resistance"], candidate.motor["resistance"]);
    assert_eq!(
        parameters["no_load_current"],
        candidate.motor["no_load_current"]
    );
    let mut stale = source.clone();
    stale["motors"][0]["name"] = serde_json::json!("changed CAD");
    assert!(proposal.accept(&stale, "accept").is_err());
}

#[test]
fn fresh_sweep_import_preserves_samples_device_identity_and_reserved_repetitions() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement");
    let s = sim_runtime::controller_refinement::sweep_review::load(
        &root.join("nine-motor-onset"),
        &root.join("split-policy.json"),
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(s.archive.trials.len(), 108);
    assert_eq!(
        s.archive
            .trials
            .iter()
            .filter(|t| t.split == "train")
            .count(),
        72
    );
    assert_eq!(
        s.archive
            .trials
            .iter()
            .filter(|t| t.split == "held_out_repetition")
            .count(),
        36
    );
    assert_eq!(
        s.archive
            .trials
            .iter()
            .map(|t| t.measured.samples.len())
            .sum::<usize>(),
        std::fs::read_to_string(root.join("nine-motor-onset/sweep.csv"))
            .unwrap()
            .lines()
            .count()
            - 1
    );
    s.validate().unwrap();
}

#[test]
fn transient_metrics_bracket_crossings_and_leave_unresolved_motion_unscored() {
    use sim_runtime::{
        controller_refinement::transients,
        experiment_comparison::{Observation, Trace},
    };
    let trace = Trace {
        quantity: sim_core::QuantityKind::Angle.definition_id(),
        unit: "rad".into(),
        samples: (0..=20)
            .map(|i| {
                let t = i as f64 / 10.;
                Observation {
                    time_s: t,
                    value: t.min(1.),
                    request_s: t,
                    completion_s: t,
                }
            })
            .collect(),
    };
    let r = transients::response(&trace, [0., 0.], 0., 2., 0., 1., 0.001, 0.01, 0.3).unwrap();
    let rise = r.rise_10_to_90.interval_s.unwrap();
    assert!((rise[0] - 0.7).abs() < 1e-12 && (rise[1] - 0.9).abs() < 1e-12);
    assert_eq!(r.settling_after_hold.interval_s, Some([0.9, 1.]));
    assert_eq!(r.sampled_overshoot_rad, 0.);
    let mut no_motion = trace.clone();
    for s in &mut no_motion.samples {
        s.value = 0.;
    }
    let r = transients::response(&no_motion, [0., 0.], 0., 2., 0., 1., 0.001, 0.01, 0.3).unwrap();
    assert!(r.rise_10_to_90.interval_s.is_none() && r.settling_after_hold.interval_s.is_none());
    let reversed = Trace {
        samples: trace
            .samples
            .iter()
            .map(|s| Observation {
                value: -s.value,
                ..s.clone()
            })
            .collect(),
        ..trace
    };
    assert_eq!(
        transients::response(&reversed, [0., 0.], 0., 2., 0., -1., 0.001, 0.01, 0.3)
            .unwrap()
            .rise_10_to_90
            .interval_s,
        Some(rise)
    );
}

#[test]
fn fitting_attempts_retain_rejected_inputs_and_validate_saved_prediction_traces() {
    use sim_runtime::controller_refinement::calibration::*;
    let (archive, model) = synthetic_archive();
    let request = FitRequest {
        model: Family {
            shared: model.clone(),
            device_deltas: Default::default(),
        },
        training_ids: vec![archive.trials[1].id.clone()],
        validation_ids: vec![archive.trials[0].id.clone()],
        coordinates: vec![Coordinate {
            path: "motor.resistance".into(),
            device: None,
            lower: 1.,
            upper: 8.,
        }],
        maximum_evaluations: 4,
        validation_influenced: false,
    };
    let failed = attempt(&archive, &request, &AtomicBool::new(false), |_, _| {});
    assert!(failed.outcome.is_none());
    assert!(failed.failure.unwrap().contains("Held-out"));
    assert_eq!(failed.request.training_ids, request.training_ids);
    let mut valid = request;
    valid.training_ids = vec![archive.trials[0].id.clone()];
    valid.validation_ids = vec![archive.trials[1].id.clone()];
    let result = attempt(&archive, &valid, &AtomicBool::new(false), |_, _| {});
    assert!(!result.evaluations.is_empty());
    let mut fit = result.outcome.unwrap();
    fit.validate(&archive).unwrap();
    assert!(fit.has_verified_traces());
    fit.scores[0].candidate_prediction.as_mut().unwrap().samples[0].value += 0.01;
    assert!(fit.validate(&archive).is_err());
}

#[test]
fn failed_or_cancelled_robustness_preserves_scenarios_without_a_misleading_envelope() {
    use sim_runtime::controller_refinement::calibration::*;
    let model = sim_runtime::experiment_study::baseline().unwrap().0;
    let mut e = Experiment::default();
    e.duration_s = 0.1;
    e.trajectory = vec![
        Knot {
            time_s: 0.,
            position_rad: 0.,
        },
        Knot {
            time_s: 0.1,
            position_rad: 0.,
        },
    ];
    let a = Variant {
        label: "valid".into(),
        model: model.clone(),
        timing: e.timing.clone(),
        evidence: "test hypothesis".into(),
    };
    let mut b = a.clone();
    b.label = "invalid".into();
    b.model.step_s = -1.;
    let result = robustness(&e, &[a.clone(), b], &AtomicBool::new(false), |_, _| {}).unwrap();
    result.validate().unwrap();
    assert_eq!(result.variants.len(), 2);
    assert_eq!(result.failures.len(), 1);
    assert!(result.prediction_envelope.is_empty());
    let cancelled = robustness(&e, &[a], &AtomicBool::new(true), |_, _| {}).unwrap();
    cancelled.validate().unwrap();
    assert!(
        cancelled.cancelled
            && cancelled.runs.is_empty()
            && cancelled.prediction_envelope.is_empty()
    );
    assert_eq!(cancelled.variants.len(), 1);
}

#[test]
fn persisted_evidence_rejects_changed_commands_traces_and_scenario_inputs() {
    use sim_runtime::controller_refinement::{calibration::*, recording::*, workspace::Workspace};
    let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/controller-id4/recording.json");
    let r: Recording = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let model = sim_runtime::experiment_study::baseline().unwrap().0;
    let mut p = predict(
        &r,
        &model,
        Purpose::ClosedLoopPrediction,
        &sim_runtime::experiment_comparison::Limits {
            rmse: 0.01,
            final_abs_error: 0.02,
        },
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    p.validate(&r).unwrap();
    p.simulated_frames[2].requested_duty += 0.001;
    assert!(p.validate(&r).is_err());
    let mut early = r.clone();
    early.frames.clear();
    early.completed = false;
    early.failure = Some("Preflight rejected the trial".into());
    early.validate().unwrap();
    let mut w = Workspace::default();
    w.recordings.push(early);
    w.validate().unwrap();
    let mut e = Experiment::default();
    e.duration_s = 0.1;
    e.trajectory = vec![
        Knot {
            time_s: 0.,
            position_rad: 0.,
        },
        Knot {
            time_s: 0.1,
            position_rad: 0.,
        },
    ];
    let v = Variant {
        label: "test".into(),
        model,
        timing: e.timing.clone(),
        evidence: "synthetic hypothesis".into(),
    };
    let mut report = robustness(&e, &[v], &AtomicBool::new(false), |_, _| {}).unwrap();
    report.validate().unwrap();
    report.variants[0].model.conditions.load_inertia *= 2.;
    assert!(report.validate().is_err());
}

#[test]
fn setup_context_keeps_unknowns_and_coverage_marks_new_trajectories() {
    use sim_runtime::controller_refinement::{
        context::*, evidence, recording::Recording, workspace::Workspace,
    };
    let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/controller-id4/recording.json");
    let r: Recording = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut context = CaptureContext::unknown(&r);
    context.validate().unwrap();
    assert!(context.properties.iter().all(|p| p.value.is_none()));
    context.properties[0].value = Some(0.1);
    assert!(context.validate().is_err());
    context.properties[0].origin = Origin::Measured;
    context.properties[0].source = "scale reading".into();
    context.properties[0].uncertainty_bounds = Some([0.09, 0.11]);
    context.validate().unwrap();
    let model = sim_runtime::experiment_study::baseline().unwrap().0;
    let mut w = Workspace::default();
    w.experiment = r.experiment.clone();
    w.recordings.push(r);
    w.capture_contexts.push(context);
    w.validate().unwrap();
    let original = evidence::controller_coverage(&w, &model);
    assert_eq!(
        original[0].draft_extrapolation_reasons,
        vec!["Current model revision has no predictions for this recording"]
    );
    w.experiment.trajectory[2].position_rad *= 1.5;
    assert!(
        evidence::controller_coverage(&w, &model)[0]
            .draft_extrapolation_reasons
            .iter()
            .any(|s| s.contains("Untested trajectory"))
    );
    w.capture_contexts[0].recording_hash = "a".repeat(64);
    assert!(w.validate().is_err());
}

#[test]
fn recorded_command_fitting_uses_whole_runs_and_never_tunes_on_reserved_inputs() {
    use sim_runtime::{
        controller_refinement::{
            calibration::*,
            calibration_data::*,
            recording::{self, Purpose, Recording},
            workspace::Workspace,
        },
        experiment_comparison::{Limits, Trace},
        experiment_study,
    };
    use std::cell::RefCell;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement");
    let recordings = [
        "controller-id4/recording.json",
        "reserved-id4-reversal-0/recording.json",
    ]
    .iter()
    .map(|p| serde_json::from_slice::<Recording>(&std::fs::read(root.join(p)).unwrap()).unwrap())
    .collect::<Vec<_>>();
    let limits = Limits {
        rmse: 3. * std::f64::consts::TAU / 4096.,
        final_abs_error: 5. * std::f64::consts::TAU / 4096.,
    };
    let assignments = recordings
        .iter()
        .enumerate()
        .map(|(i, r)| Assignment {
            recording_hash: r.fingerprint(),
            role: if i == 0 { Role::Train } else { Role::HeldOut },
            limits: limits.clone(),
            rationale: "Test: already inspected measurements; no fresh validation claim".into(),
        })
        .collect::<Vec<_>>();
    let dataset = RecordingDataset::capture(&recordings, &assignments).unwrap();
    let model = experiment_study::baseline().unwrap().0;
    let expected = recording::predict(
        &recordings[0],
        &model,
        Purpose::RecordedCommandReplay,
        &limits,
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(
        dataset
            .predict(
                &assignments[0].recording_hash,
                &model,
                &AtomicBool::new(false)
            )
            .unwrap(),
        expected.predicted
    );
    struct Tracked {
        data: RecordingDataset,
        calls: RefCell<Vec<String>>,
    }
    impl CalibrationData for Tracked {
        fn cases(&self) -> Result<Vec<Case>, String> {
            self.data.cases()
        }
        fn fingerprint(&self) -> String {
            self.data.fingerprint()
        }
        fn predict(
            &self,
            id: &str,
            m: &experiment_study::ModelSettings,
            c: &AtomicBool,
        ) -> Result<Trace, String> {
            self.calls.borrow_mut().push(id.into());
            self.data.predict(id, m, c)
        }
    }
    let tracked = Tracked {
        data: dataset.clone(),
        calls: RefCell::new(vec![]),
    };
    let mut request = FitRequest {
        model: Family {
            shared: model,
            device_deltas: Default::default(),
        },
        coordinates: vec![Coordinate {
            path: "motor.no_load_current".into(),
            device: Some(4),
            lower: -0.05,
            upper: 0.05,
        }],
        training_ids: vec![assignments[0].recording_hash.clone()],
        validation_ids: vec![assignments[1].recording_hash.clone()],
        maximum_evaluations: 3,
        validation_influenced: true,
    };
    let result = attempt(&tracked, &request, &AtomicBool::new(false), |_, _| {});
    result.validate(&dataset).unwrap();
    assert!(result.outcome.is_some(), "{:?}", result.failure);
    assert_eq!(
        tracked
            .calls
            .borrow()
            .iter()
            .filter(|id| *id == &assignments[1].recording_hash)
            .count(),
        2,
        "Reserved data may only be predicted for final baseline/candidate scoring"
    );
    assert_eq!(result.outcome.as_ref().unwrap().scores.len(), 2);
    let mut w = Workspace::default();
    w.recordings = recordings;
    w.recording_assignments = assignments.clone();
    w.recording_fits.push(RecordingFitAttempt {
        dataset,
        attempt: result,
    });
    w.validate().unwrap();
    w.recording_assignments[1].role = Role::Train;
    assert!(w.validate().is_err());
    request.training_ids = request.validation_ids.clone();
    let invalid = attempt(&tracked, &request, &AtomicBool::new(false), |_, _| {});
    assert!(invalid.failure.unwrap().contains("Held-out"));
}

#[test]
fn torque_off_release_coasts_instead_of_shorting_the_motor() {
    use sim_runtime::actuator_bench::{self, Drive, DriverRelease};
    let mut model = sim_runtime::experiment_study::baseline().unwrap().0;
    model.motor.insert("no_load_current".into(), 0.);
    model.motor.insert("gear_friction".into(), 0.);
    model.motor.insert("efficiency".into(), 1.);
    let run = |release, step| {
        let mut bench = actuator_bench::prepare(
            &model,
            12.,
            25.,
            Drive::ReleasedPulse {
                duty: 0.05,
                start_s: 0.,
                duration_s: 0.1,
                release,
            },
        )
        .unwrap();
        bench.runtime.advance(0.099, step).unwrap();
        let before = bench.runtime.get(bench.angle);
        bench.runtime.advance(0.021, step).unwrap();
        let a = bench.runtime.get(bench.angle);
        bench.runtime.advance(0.08, step).unwrap();
        let b = bench.runtime.get(bench.angle);
        (before, a, b)
    };
    let brake = run(DriverRelease::ElectricalBrake, 0.00025);
    let coast = || DriverRelease::TorqueOff {
        off_conductance_s: 1e-8,
        diode_drop_v: 0.7,
        diode_resistance_ohm: 0.1,
    };
    let coarse = run(coast(), 0.00025);
    let fine = run(coast(), 0.000125);
    assert!(
        (brake.0 - coarse.0).abs() < 1e-8,
        "Enabled bridge must share the existing driven equations"
    );
    assert!(
        coarse.2 - coarse.1 > 5. * (brake.2 - brake.1),
        "Coasting must preserve motion beyond the electrical braking transient: {coarse:?} vs {brake:?}"
    );
    assert!(
        (coarse.2 - fine.2).abs() < 0.02 * fine.2.abs(),
        "Release prediction must converge with timestep"
    );
}

#[test]
fn release_import_preserves_modes_splits_and_reports_only_resolved_plateaus() {
    use sim_runtime::{
        actuator_bench::DriverRelease,
        controller_refinement::{sweep_review, transients},
        experiment_comparison::{Observation, Trace},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement");
    let study = sweep_review::load_release(
        &root.join("release-id4"),
        &root.join("release-id4-import-policy.json"),
        DriverRelease::TorqueOff {
            off_conductance_s: 1e-8,
            diode_drop_v: 0.7,
            diode_resistance_ohm: 0.1,
        },
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    study.validate().unwrap();
    assert_eq!(study.archive.trials.len(), 12);
    assert!(study.validation_influenced);
    assert_eq!(
        study
            .archive
            .trials
            .iter()
            .filter(|t| matches!(t.release, Some(DriverRelease::TorqueOff { .. })))
            .count(),
        6
    );
    assert_eq!(
        study
            .archive
            .trials
            .iter()
            .filter(|t| t.split == "train")
            .count(),
        8
    );
    assert_eq!(
        study
            .archive
            .trials
            .iter()
            .map(|t| t.measured.samples.len())
            .sum::<usize>(),
        std::fs::read_to_string(root.join("release-id4/sweep.csv"))
            .unwrap()
            .lines()
            .count()
            - 1
    );
    let mut trace = Trace {
        quantity: sim_core::QuantityKind::Angle.definition_id(),
        unit: "rad".into(),
        samples: (0..=50)
            .map(|i| {
                let t = i as f64 * 0.01;
                Observation {
                    time_s: t,
                    request_s: t,
                    completion_s: t,
                    value: t.min(0.2),
                }
            })
            .collect(),
    };
    let response = transients::release(&trace, [0.2, 0.2], 0.001, 0.15).unwrap();
    assert_eq!(
        response.stationary_tail_start_delay.interval_s,
        Some([0., 0.])
    );
    for s in &mut trace.samples {
        s.value = 0.;
    }
    assert!(
        transients::release(&trace, [0.2, 0.2], 0.001, 0.15)
            .unwrap()
            .stationary_tail_start_delay
            .interval_s
            .is_none()
    );
}

#[test]
fn combined_fit_preserves_sources_and_excludes_each_reserved_behavior_from_objectives() {
    use sim_runtime::{
        controller_refinement::{
            calibration::*, calibration_data::*, recording::Recording, workspace::Workspace,
        },
        experiment_comparison::{Limits, Trace},
        experiment_study::{self, Study},
    };
    use std::cell::RefCell;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement");
    let (pulse, model) = synthetic_archive();
    let mut release = Study::load(&root.join("release-id4-review.json"))
        .unwrap()
        .archive;
    let train = release
        .trials
        .iter()
        .find(|t| t.split == "train")
        .unwrap()
        .clone();
    let held = release
        .trials
        .iter()
        .find(|t| t.split != "train")
        .unwrap()
        .clone();
    release.trials = vec![train, held];
    let recordings = [
        "controller-id4/recording.json",
        "reserved-id4-reversal-0/recording.json",
    ]
    .map(|p| serde_json::from_slice::<Recording>(&std::fs::read(root.join(p)).unwrap()).unwrap())
    .to_vec();
    let assignments = recordings
        .iter()
        .enumerate()
        .map(|(i, r)| Assignment {
            recording_hash: r.fingerprint(),
            role: if i == 0 { Role::Train } else { Role::HeldOut },
            limits: Limits {
                rmse: 3. * r.experiment.timing.encoder_quantum_rad,
                final_abs_error: 5. * r.experiment.timing.encoder_quantum_rad,
            },
            rationale: "Regression test using inspected data; never fresh validation".into(),
        })
        .collect::<Vec<_>>();
    let data = CombinedDataset {
        archives: vec![pulse.clone(), release],
        recordings: Some(RecordingDataset::capture(&recordings, &assignments).unwrap()),
    };
    data.validate().unwrap();
    // A joined dataset routes to the original sources and has no name-based fallback.
    let joined = data.joined().unwrap();
    assert_eq!(joined.cases().unwrap().len(), 6);
    assert_eq!(
        joined
            .predict(&pulse.trials[0].id, &model, &AtomicBool::new(false))
            .unwrap(),
        pulse
            .predict(&pulse.trials[0].id, &model, &AtomicBool::new(false))
            .unwrap()
    );
    assert!(
        joined
            .predict("missing", &model, &AtomicBool::new(false))
            .is_err()
    );
    let mut duplicate = data.clone();
    duplicate.archives.push(pulse.clone());
    assert!(
        duplicate
            .validate()
            .unwrap_err()
            .contains("Duplicate trial")
    );
    struct Tracked<'a> {
        data: &'a CombinedDataset,
        calls: RefCell<Vec<String>>,
    }
    impl CalibrationData for Tracked<'_> {
        fn cases(&self) -> Result<Vec<Case>, String> {
            self.data.cases()
        }
        fn fingerprint(&self) -> String {
            self.data.fingerprint()
        }
        fn predict(
            &self,
            id: &str,
            m: &experiment_study::ModelSettings,
            c: &AtomicBool,
        ) -> Result<Trace, String> {
            self.calls.borrow_mut().push(id.into());
            self.data.predict(id, m, c)
        }
    }
    let cases = data.cases().unwrap();
    let request = FitRequest {
        model: Family {
            shared: model,
            device_deltas: Default::default(),
        },
        coordinates: vec![Coordinate {
            path: "motor.no_load_current".into(),
            device: Some(4),
            lower: -0.05,
            upper: 0.05,
        }],
        training_ids: cases
            .iter()
            .filter(|c| c.split == "train")
            .map(|c| c.id.clone())
            .collect(),
        validation_ids: cases
            .iter()
            .filter(|c| c.split != "train")
            .map(|c| c.id.clone())
            .collect(),
        maximum_evaluations: 3,
        validation_influenced: true,
    };
    let tracked = Tracked {
        data: &data,
        calls: RefCell::new(vec![]),
    };
    let result = attempt(&tracked, &request, &AtomicBool::new(false), |_, _| {});
    assert!(result.outcome.is_some(), "{:?}", result.failure);
    result.validate(&data).unwrap();
    for id in &request.validation_ids {
        assert_eq!(
            tracked.calls.borrow().iter().filter(|x| *x == id).count(),
            2,
            "Each reserved behavior is scored only after optimization: {id}"
        );
    }
    for id in &request.training_ids {
        assert!(tracked.calls.borrow().iter().filter(|x| *x == id).count() > 2);
    }
    let mut workspace = Workspace::default();
    workspace.recordings = recordings;
    workspace.recording_assignments = assignments;
    workspace.combined_fits.push(CombinedFitAttempt {
        dataset: data,
        attempt: result,
    });
    workspace.validate().unwrap();
    let restored: Workspace =
        serde_json::from_slice(&serde_json::to_vec(&workspace).unwrap()).unwrap();
    restored.validate().unwrap();
    let mut tampered = restored.clone();
    tampered.combined_fits[0].dataset.archives[0].trials[1].split = "train".into();
    assert!(tampered.validate().is_err());
    let mut tampered = restored.clone();
    tampered.combined_fits[0]
        .dataset
        .recordings
        .as_mut()
        .unwrap()
        .assignments[1]
        .limits
        .rmse *= 2.;
    assert!(tampered.validate().is_err());
    workspace.recording_assignments[1].role = Role::Train;
    assert!(workspace.validate().is_err());
}

#[test]
fn explicit_loss_transition_matches_low_drive_equilibrium_and_converges_in_time() {
    use sim_runtime::actuator_bench::{self, Drive};
    let model = sim_runtime::experiment_study::baseline().unwrap().0;
    let measure = |scale: Option<f64>, duty: f64, step: f64| {
        let mut m = model.clone();
        if let Some(scale) = scale {
            m.motor.insert("loss_speed_scale".into(), scale);
        }
        let mut b = actuator_bench::prepare(
            &m,
            12.,
            25.,
            Drive::Pulse {
                duty,
                start_s: 0.,
                duration_s: 3.,
            },
        )
        .unwrap();
        b.runtime.advance(1.5, step).unwrap();
        let before = b.runtime.get(b.angle);
        b.runtime.advance(0.5, step).unwrap();
        let after = b.runtime.get(b.angle);
        (after, (after - before) / 0.5)
    };
    let default = measure(None, 0.015, 0.00025);
    assert_eq!(
        default,
        measure(Some(5.), 0.015, 0.00025),
        "Legacy omitted coefficient has identical behavior"
    );
    for duty in [-0.015, 0.015] {
        let (angle, speed) = measure(Some(0.05), duty, 0.00025);
        let (fine_angle, fine_speed) = measure(Some(0.05), duty, 0.000125);
        let mut lo: f64 = 0.;
        let mut hi: f64 = 10.;
        // Independent steady torque/current balance at zero external load:
        // V = (R + Rbridge) I_loss tanh(w / scale) + Ke w.
        for _ in 0..80 {
            let w = (lo + hi) / 2.;
            let v = (model.motor["resistance"] + model.bridge["on_resistance"])
                * model.motor["no_load_current"]
                * (w / 0.05).tanh()
                + model.motor["back_emf_constant"] * w;
            if v < 12. * duty.abs() { lo = w } else { hi = w }
        }
        let expected = duty.signum() * (lo + hi) / 2. / model.motor["ratio"];
        assert!(
            (speed - expected).abs() < expected.abs() * 0.01,
            "{speed} vs steady {expected}"
        );
        assert!((speed - fine_speed).abs() < expected.abs() * 0.001);
        assert!((angle - fine_angle).abs() < 1e-5);
        assert!(
            angle.abs() < std::f64::consts::TAU / 4096.,
            "Creep remains below an encoder count over this interval"
        );
        assert!(speed.abs() < default.1.abs() / 50.);
        assert!(
            speed * duty > 0.,
            "Smooth loss still allows creep, not an exact static hold"
        );
    }
}

#[test]
fn loss_regularization_proposal_introduces_only_the_reviewed_field_and_reaches_robot_runtime() {
    use sim_runtime::controller_refinement::cad;
    let mut motor: serde_json::Value = serde_json::from_str::<serde_json::Value>(include_str!(
        "../../../examples/actuators/hx30hm/plan.json"
    ))
    .unwrap()["motor"]
        .clone();
    motor["id"] = "bench-motor".into();
    motor["electrical"]["back_emf_constant"] = motor["electrical"]["torque_constant"].clone();
    let source =
        serde_json::json!({"version":4,"motors":[motor],"future_cad_metadata":{"preserve":true}});
    let baseline = sim_runtime::experiment_study::baseline().unwrap().0;
    let mut candidate = baseline.clone();
    candidate.motor.insert("loss_speed_scale".into(), 5.);
    let propose = |source: &serde_json::Value,
                   candidate: &sim_runtime::experiment_study::ModelSettings| {
        cad::propose(
            source,
            "bench-motor",
            4,
            &baseline,
            candidate,
            vec!["Synthetic model-regulation evidence; not a hardware calibration".into()],
            "Synthetic unloaded test only",
        )
    };
    assert!(
        propose(&source, &candidate)
            .unwrap_err()
            .contains("No changed"),
        "Explicit registry default does not change physics"
    );
    candidate.motor.insert("loss_speed_scale".into(), 0.05);
    let proposal = propose(&source, &candidate).unwrap();
    assert_eq!(proposal.version, 2);
    assert_eq!(proposal.changes.len(), 1);
    assert_eq!(proposal.changes[0].previous, None);
    let restored: cad::Proposal =
        serde_json::from_str(&serde_json::to_string(&proposal).unwrap()).unwrap();
    let accepted = restored
        .accept(
            &source,
            "Review the explicit numerical-loss hypothesis; no hardware accuracy claim",
        )
        .unwrap();
    assert!(
        source
            .pointer("/motors/0/electrical/loss_speed_scale")
            .is_none()
    );
    assert_eq!(
        accepted["future_cad_metadata"],
        source["future_cad_metadata"]
    );
    let model = sim_domain_robot::model::PhysicalModel::parse(&accepted.to_string()).unwrap();
    let params = sim_domain_robot::motor::cad_motor_unit_parameters(
        &model.motors[0],
        0.,
        298.15,
        true,
        false,
    );
    assert_eq!(params["loss_speed_scale"], 0.05);
    let mut wrong = proposal.clone();
    wrong.changes[0].proposed = 0.;
    assert!(wrong.accept(&source, "bad coefficient").is_err());
    let mut wrong = proposal.clone();
    wrong.version = 1;
    assert!(
        wrong
            .accept(&source, "old format cannot introduce missing fields")
            .is_err()
    );
    let mut wrong = proposal.clone();
    wrong.changes[0].pointer = "/future_cad_metadata/preserve".into();
    assert!(wrong.accept(&source, "wrong field").is_err());
    let mut missing = source.clone();
    missing["motors"][0]["electrical"]
        .as_object_mut()
        .unwrap()
        .remove("resistance");
    candidate
        .motor
        .insert("resistance".into(), baseline.motor["resistance"] * 1.1);
    assert!(
        propose(&missing, &candidate).is_err(),
        "The new optional field must not enable silently inventing missing physical properties"
    );
}

#[test]
fn battery_supply_sag_charge_accounting_and_pwm_current_are_in_the_shared_circuit() {
    use sim_runtime::{
        actuator_bench::{self, Drive},
        controller_refinement::power,
    };
    let mut model = sim_runtime::experiment_study::baseline().unwrap().0;
    model.power = Some(power::Setup {
        source_component: sim_domain_robot::motor::BATTERY.into(),
        source_parameters: [
            ("nominal_voltage", 11.1),
            ("internal_resistance", 0.5),
            ("capacity_ah", 0.01),
            ("initial_soc", 0.8),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect(),
        auxiliary_current_a: 0.2,
        evidence: "Synthetic circuit test; not measured battery parameters".into(),
        limits: power::Limits::default(),
    });
    let mut b = actuator_bench::prepare(
        &model,
        12.,
        25.,
        Drive::Pulse {
            duty: 0.2,
            start_s: 0.,
            duration_s: 1.,
        },
    )
    .unwrap();
    let mut samples = vec![];
    for i in 1..=400 {
        b.runtime.advance(0.00025, 0.00025).unwrap();
        samples.push(b.electrical_sample(i as f64 * 0.00025).unwrap());
    }
    let last = samples.last().unwrap();
    let soc = last.state_of_charge.unwrap();
    let emf = 11.1 * (0.9 + 0.2 * soc - 0.15 * (1. - soc).powi(8));
    assert!((last.supply_voltage_v - (emf - 0.5 * last.supply_current_a)).abs() < 1e-7);
    assert!(
        (last.supply_current_a - (0.2 * last.winding_current_a + 0.2)).abs() < 1e-7,
        "PWM averaged supply current includes auxiliary current, not full winding current"
    );
    assert!(soc < 0.8);
    assert!(last.supply_current_a > 0.);
    let charge = samples
        .iter()
        .map(|s| s.supply_current_a * 0.00025)
        .sum::<f64>();
    assert!((0.8 - soc - charge / 36.).abs() < 1e-7);
    let limits = power::Limits {
        maximum_discharge_current_a: Some(0.01),
        ..Default::default()
    };
    let trace = power::Trace::new(samples, limits).unwrap();
    trace.validate().unwrap();
    assert_eq!(trace.summary.passes, Some(false));
    assert!(trace.summary.drawn_energy_j > 0.);
    let mut edited = trace.clone();
    edited.samples[2].supply_power_w += 1.;
    assert!(edited.validate().is_err());
}

#[test]
fn signed_power_integrates_consumption_and_return_without_swapping_current_locations() {
    use sim_runtime::controller_refinement::power;
    let make = |time, power| power::Sample {
        time_s: time,
        supply_voltage_v: 10.,
        supply_current_a: power / 10.,
        winding_voltage_v: 2.,
        winding_current_a: 1.,
        supply_power_w: power,
        winding_power_w: 2.,
        state_of_charge: None,
    };
    let trace = power::Trace::new(
        vec![make(0., 10.), make(2., -10.)],
        power::Limits::default(),
    )
    .unwrap();
    assert_eq!(trace.summary.drawn_energy_j, 5.);
    assert_eq!(trace.summary.returned_energy_j, 5.);
    assert_eq!(trace.summary.passes, None);
    let mut e = Experiment::default();
    e.duration_s = 0.1;
    e.trajectory = vec![
        Knot {
            time_s: 0.,
            position_rad: 0.,
        },
        Knot {
            time_s: 0.1,
            position_rad: 0.,
        },
    ];
    let mut m = sim_runtime::experiment_study::baseline().unwrap().0;
    m.power = Some(power::Setup {
        source_component: "electrical.voltage_source".into(),
        source_parameters: [("voltage".into(), 12.)].into_iter().collect(),
        auxiliary_current_a: 0.2,
        evidence: "Synthetic fixed supply; explicitly assumed electronics consumption".into(),
        limits: power::Limits::default(),
    });
    let run = simulate(&e, &m, &AtomicBool::new(false), |_, _| {}).unwrap();
    run.validate().unwrap();
    let trace = run.electrical.unwrap();
    assert!((trace.summary.drawn_energy_j - 0.24).abs() < 0.001);
    assert!(
        trace
            .samples
            .iter()
            .skip(1)
            .all(|s| (s.supply_current_a - 0.2).abs() < 1e-7 && s.winding_current_a.abs() < 1e-7)
    );
}

#[test]
fn electrical_controller_uses_declared_voltage_and_limits_without_inventing_current() {
    use sim_runtime::controller_refinement::power;
    let mut e = Experiment::default();
    e.timing.observation_delay_ticks = 0;
    e.timing.command_delay_ticks = 0;
    e.controller = Policy::RustPid {
        parameters: Pid {
            kp: 1.,
            ki: 0.,
            kd: 0.,
            integral_limit: 0.,
            duty_limit: 0.2,
        },
    };
    for k in &mut e.trajectory {
        k.position_rad = 0.04;
    }
    e.electrical = Some(power::Controller {
        sensing: power::Sensing {
            voltage_quantum_v: 0.1,
            supply_current_quantum_a: Some(0.01),
            winding_current_quantum_a: None,
            evidence: "Synthetic calibrated supply sensor; aligned with the encoder observation"
                .into(),
        },
        nominal_voltage_for_compensation_v: Some(12.),
        limits: power::Limits {
            maximum_discharge_current_a: Some(0.5),
            ..Default::default()
        },
    });
    let mut c = ControllerSession::new(e.clone()).unwrap();
    let mut o = observation(0., 0.);
    o.electrical = Some(power::Observation {
        supply_voltage_v: 6.,
        supply_current_a: Some(0.2),
        winding_current_a: None,
    });
    let f = c.tick(0., o.clone()).unwrap();
    assert!(
        (f.requested_duty - 0.08).abs() < 1e-10,
        "Half voltage requires twice nominal duty within configured bounds"
    );
    o.observed_s = 0.02;
    o.received_s = 0.02;
    o.request_s = 0.02;
    o.completion_s = 0.02;
    o.electrical.as_mut().unwrap().supply_current_a = Some(0.6);
    let stopped = c.tick(0.02, o.clone()).unwrap();
    assert_eq!(stopped.applied_duty, 0.);
    assert_eq!(stopped.electrical_limit_reasons, vec!["Discharge current"]);
    validate_frames(&e, &[f, stopped.clone()]).unwrap();
    let mut altered = stopped;
    altered.electrical_limit_reasons.clear();
    assert!(validate_frames(&e, &[altered]).is_err());
    let mut missing = e.clone();
    missing
        .electrical
        .as_mut()
        .unwrap()
        .sensing
        .supply_current_quantum_a = None;
    assert!(
        missing.validate().is_err(),
        "A current/power limit cannot use an absent sensor"
    );
    let mut no_current = o;
    o_current_remove(&mut no_current);
    assert!(
        ControllerSession::new(e)
            .unwrap()
            .tick(0.02, no_current)
            .is_err()
    );
    fn o_current_remove(o: &mut Feedback) {
        o.electrical.as_mut().unwrap().supply_current_a = None;
    }
}

#[test]
fn rhai_and_simulation_share_quantized_electrical_observations() {
    use sim_runtime::controller_refinement::power;
    let mut e = Experiment::default();
    e.duration_s = 0.1;
    e.trajectory = vec![
        Knot {
            time_s: 0.,
            position_rad: 0.,
        },
        Knot {
            time_s: 0.1,
            position_rad: 0.,
        },
    ];
    e.timing.observation_delay_ticks = 0;
    e.timing.command_delay_ticks = 0;
    e.electrical = Some(power::Controller {
        sensing: power::Sensing {
            voltage_quantum_v: 0.1,
            supply_current_quantum_a: Some(0.01),
            winding_current_quantum_a: None,
            evidence: "Synthetic sensing with declared quantization".into(),
        },
        nominal_voltage_for_compensation_v: None,
        limits: Default::default(),
    });
    e.controller=Policy::Rhai{source:"fn control(t,s,a,state) { #{commands: #{duty: 0.01 * s.supply_power / s.supply_voltage}, state: state} }".into(),parameters:serde_json::json!({}),duty_limit:0.2};
    let mut m = sim_runtime::experiment_study::baseline().unwrap().0;
    m.power = Some(power::Setup {
        source_component: "electrical.voltage_source".into(),
        source_parameters: [("voltage".into(), 12.03)].into_iter().collect(),
        auxiliary_current_a: 0.234,
        evidence: "Synthetic power-source/circuit test".into(),
        limits: Default::default(),
    });
    let run = simulate(&e, &m, &AtomicBool::new(false), |_, _| {}).unwrap();
    assert!(run.failure.is_none(), "{:?}", run.failure);
    run.validate().unwrap();
    for frame in &run.frames {
        let o = frame.observation.electrical.as_ref().unwrap();
        assert!((o.supply_voltage_v - 12.).abs() < 1e-8);
        assert!(o.winding_current_a.is_none());
        assert!((frame.requested_duty - 0.01 * o.supply_current_a.unwrap()).abs() < 1e-10);
    }
    assert!(
        run.electrical
            .unwrap()
            .samples
            .iter()
            .all(|s| (s.supply_voltage_v - 12.03).abs() < 1e-7),
        "Physical trace stays separate from quantized controller feedback"
    );
}

#[test]
fn voltage_compensation_preserves_full_duty_range_above_nominal_voltage() {
    use sim_runtime::controller_refinement::power;
    let mut e = Experiment::default();
    e.timing.command_delay_ticks = 0;
    e.trajectory = vec![
        Knot {
            time_s: 0.,
            position_rad: 3.,
        },
        Knot {
            time_s: 1.,
            position_rad: 3.,
        },
    ];
    e.controller = Policy::RustPid {
        parameters: Pid {
            kp: 1.,
            ki: 0.,
            kd: 0.,
            integral_limit: 1.,
            duty_limit: 1.,
        },
    };
    e.electrical = Some(power::Controller {
        sensing: power::Sensing {
            voltage_quantum_v: 0.1,
            supply_current_quantum_a: None,
            winding_current_quantum_a: None,
            evidence: "Synthetic voltage above nominal".into(),
        },
        nominal_voltage_for_compensation_v: Some(12.),
        limits: Default::default(),
    });
    let mut o = observation(0., 0.);
    o.electrical = Some(power::Observation {
        supply_voltage_v: 24.,
        supply_current_a: None,
        winding_current_a: None,
    });
    let f = ControllerSession::new(e).unwrap().tick(0., o).unwrap();
    assert_eq!(f.requested_duty, 1.);
    assert!(f.saturated);
}
