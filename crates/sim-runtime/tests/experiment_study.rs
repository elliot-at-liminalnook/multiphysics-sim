use sim_runtime::{experiment_comparison::hx_archive, experiment_study::*};
use std::{path::Path, sync::atomic::AtomicBool};
fn archive() -> hx_archive::Archive {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    hx_archive::load(
        &repo.join("examples/actuators/hx30hm/pwm-identification"),
        &repo,
    )
    .unwrap()
}
#[test]
fn physical_replay_refinement_validation_and_portable_evidence() {
    let mut study = Study::new(archive()).unwrap();
    let trial = study
        .archive
        .trials
        .iter()
        .find(|t| t.drive == 0.1 && t.device == 4)
        .unwrap();
    let id = trial.id.clone();
    study.draft.motor.insert("gear_friction".into(), 0.05);
    let result = evaluate(
        &study.archive,
        &[id.clone()],
        &study.baseline,
        &study.draft,
        None,
        false,
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    assert!(
        result.results[0].errors.is_empty(),
        "{:?}",
        result.results[0].errors
    );
    let baseline = result.results[0].baseline.as_ref().unwrap();
    let candidate = result.results[0].candidate.as_ref().unwrap().clone();
    assert_ne!(baseline.trace, candidate.trace);
    assert_eq!(baseline.trace.samples.len(), trial.measured.samples.len());
    // Timestep sensitivity with the same actual shared physics path.
    let mut finer = study.baseline.clone();
    finer.step_s *= 0.5;
    let fine = simulate(trial, &finer, &AtomicBool::new(false)).unwrap();
    let error = baseline
        .trace
        .samples
        .iter()
        .zip(&fine.samples)
        .map(|(a, b)| (a.value - b.value).abs())
        .fold(0., f64::max);
    assert!(error < 0.0015, "step error {error}");
    study.evaluations.push(result);
    study.view.trial_id = Some(id);
    study.validation_seen = true;
    study.candidate_edited();
    assert!(study.validation_influenced);
    study.notes = "Need a known load before drawing conclusions".into();
    let path = std::env::temp_dir().join(format!("experiment-study-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    study.save_new(&path).unwrap();
    assert!(study.save_new(&path).is_err());
    let restored = Study::load(&path).unwrap();
    assert_eq!(restored.notes, study.notes);
    assert_eq!(restored.draft, study.draft);
    assert_eq!(
        restored.evaluations[0].results[0]
            .candidate
            .as_ref()
            .unwrap()
            .trace,
        candidate.trace
    );
    std::fs::remove_file(path).unwrap();
}
#[test]
fn cancellation_and_invalid_candidates_do_not_produce_passes() {
    let study = Study::new(archive()).unwrap();
    let ids = vec![study.archive.trials[0].id.clone()];
    let cancelled = evaluate(
        &study.archive,
        &ids,
        &study.baseline,
        &study.draft,
        None,
        false,
        &AtomicBool::new(true),
        |_, _| {},
    )
    .unwrap();
    assert!(cancelled.cancelled);
    assert!(cancelled.results[0].candidate.is_none());
    let mut bad = study.draft.clone();
    bad.motor.insert("resistance".into(), -1.);
    assert!(
        evaluate(
            &study.archive,
            &ids,
            &study.baseline,
            &bad,
            None,
            false,
            &AtomicBool::new(false),
            |_, _| {}
        )
        .is_err()
    );
}

#[test]
fn pulse_edges_are_scheduled_and_signed() {
    let registry = sim_runtime::registry();
    for amplitude in [-0.3, 0.3] {
        let mut model = sim_core::ModelWorld::default();
        let p = model
            .part(
                &registry,
                "pulse",
                sim_domain_control::pulse::PULSE,
                [("amplitude", amplitude), ("start", 0.1), ("duration", 0.2)],
            )
            .unwrap();
        model.connect([p.port("value")]);
        let mut rt = sim_compile::Runtime::new(
            model,
            &registry,
            sim_dynamics::Integrator::BackwardEuler(sim_runtime::newton()),
        )
        .unwrap();
        let value = rt.signal_id(p.port("value"));
        rt.advance(0.05, 0.031).unwrap();
        assert_eq!(rt.get(value), 0.);
        rt.advance(0.10, 0.031).unwrap();
        assert_eq!(rt.get(value), amplitude);
        rt.advance(0.10, 0.031).unwrap();
        assert_eq!(rt.get(value), amplitude);
        rt.advance(0.10, 0.031).unwrap();
        assert_eq!(rt.get(value), 0.);
    }
}
#[test]
fn reject_tampered_evidence_and_export_readable_report() {
    let mut s = Study::new(archive()).unwrap();
    s.archive.trials[0].comparison.rmse = 999.;
    assert!(s.validate().is_err());
    s = Study::new(archive()).unwrap();
    let id = s.archive.trials[0].id.clone();
    let e = evaluate(
        &s.archive,
        &[id.clone()],
        &s.baseline,
        &s.draft,
        None,
        false,
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    s.evaluations.push(e);
    s.notes = "<script>not executable</script>".into();
    let path = std::env::temp_dir().join(format!("experiment-export-{}.html", std::process::id()));
    let _ = std::fs::remove_file(&path);
    s.export_html_new(&path).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("<svg"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("<script>"));
    assert!(html.contains(&id));
    s.evaluations[0].results[0]
        .candidate
        .as_mut()
        .unwrap()
        .trace
        .samples[0]
        .value += 1.;
    assert!(s.validate().is_err());
    std::fs::remove_file(path).unwrap();
}
