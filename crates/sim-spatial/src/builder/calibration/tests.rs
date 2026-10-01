use super::*;

#[test]
fn calibration_review_loads_tracked_archive_and_names_bad_paths() {
    // The workspace root the handler resolves (the repository, from the test's directory).
    let root = crate::workspace::root().unwrap().to_path_buf();
    assert!(root.join(DEFAULT_ARCHIVE).is_dir(), "{}", root.display());
    let dir = root.join(DEFAULT_ARCHIVE);
    let r = review(&dir, &root).unwrap();
    let a = &r.archive;
    assert_eq!(a.trials.len(), 216);
    let n = counts(a);
    let pass = |f: &dyn Fn(&Trial) -> bool| a.trials.iter().filter(|t| f(t) && t.comparison.passes).count();
    assert_eq!(n.held_out.total, a.trials.iter().filter(|t| t.split != "train").count());
    assert_eq!(n.held_out.pass, pass(&|t| t.split != "train"));
    assert_eq!(n.train.pass, pass(&|t| t.split == "train"));
    assert_eq!((n.train.total, n.held_out.total), (54, 162));
    assert_eq!(n.all.pass + n.all.fail, 216);
    // The archive's own comparisons agree with its README ("81/162 held-out trials").
    assert_eq!(n.held_out.pass, 81);
    eprintln!("held-out {}/{} pass, train {}/{} pass; by split {:?}", n.held_out.pass, n.held_out.total, n.train.pass, n.train.total, n.by_split);
    assert!(a.observation_blake3.len() == 64 && a.model_blake3.len() == 64);
    assert_eq!(a.verified_inputs, a.input_blake3.len(), "{:?}", a.integrity_issues);
    assert_eq!(review_json(&r)["trials"].as_array().unwrap().len(), 216);

    let missing = root.join("target/no-such-archive");
    let e = review(&missing, &root).unwrap_err();
    assert!(e.contains(&missing.display().to_string()), "{e}");
    let file = dir.join("README.md");
    let e = review(&file, &root).unwrap_err();
    assert!(e.contains(&file.display().to_string()) && e.contains("not supported"), "{e}");
    let sweep = root.join("examples/actuators/hx30hm/hardware/2026-09-11-nine-servos/pwm-individual-full-range");
    let e = review(&sweep, &root).unwrap_err();
    assert!(e.contains(&sweep.display().to_string()) && e.contains("sweep.csv") && e.contains("not supported"), "{e}");

    // The Builder handler: worker thread, pending refusal, state, filters, last good kept.
    let system = root.join("examples/systems-builder/motor-driver-board/board.system.json");
    let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
    let mut b = Builder::open(system, root.join("library/systems"), registry).unwrap();
    let wait = |b: &mut Builder| {
        for _ in 0..1200 {
            if b.finish_calibration() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("archive load did not finish");
    };
    b.calibration_first_visit();
    assert_eq!(b.calibration.pending(), Some(dir.as_path()));
    assert_eq!(b.calibration_json()["phase"], "loading");
    assert!(b.calibration_request(None).unwrap_err().contains("Still loading"));
    wait(&mut b);
    b.set_calibration_filter(Some(SplitFilter::HeldOut), Some(OutcomeFilter::Pass));
    let state = b.state_json();
    let c = &state["calibration_review"];
    assert_eq!(c["phase"], "loaded");
    assert_eq!(c["path"], dir.display().to_string());
    assert_eq!(c["trial_count"], 216);
    assert_eq!(c["counts"]["held_out"]["pass"], 81);
    assert_eq!(c["visible"].as_array().unwrap().len(), 81);
    assert_eq!(c["page"]["trials"].as_array().unwrap().len(), PAGE_ROWS);
    assert!(c["page"]["trials"].as_array().unwrap().iter().all(|t| t["held_out"] == true && t["comparison"]["passes"] == true));
    b.calibration_first_visit();
    assert!(b.calibration.pending().is_none(), "first visit loads once");
    b.calibration_request(Some(missing.clone())).unwrap();
    wait(&mut b);
    let c = b.calibration_json();
    assert_eq!(c["phase"], "failed");
    assert!(c["error"].as_str().unwrap().contains(&missing.display().to_string()));
    assert_eq!(c["requested"], missing.display().to_string());
    assert_eq!(c["path"], dir.display().to_string(), "the last good archive stays, with its own path");

    // Selection: the archive's own counts and endpoints; unknown ids are named.
    assert!(b.calibration_json()["selected"].is_null());
    for t in [a.trials.iter().find(|t| held_out(t) && t.comparison.passes).unwrap(), a.trials.iter().find(|t| held_out(t) && !t.comparison.passes).unwrap()] {
        b.select_calibration_trial(&t.id).unwrap();
        let s = &b.calibration_json()["selected"];
        assert_eq!(s["id"], t.id.as_str());
        assert_eq!((s["held_out"].as_bool(), s["role"].as_str()), (Some(true), Some("held-out (validation data)")));
        assert_eq!(s["comparison"]["passes"], t.comparison.passes);
        assert_eq!(s["comparison"]["rmse"], t.comparison.rmse);
        assert_eq!(s["limits"]["final_abs_error"], t.limits.final_abs_error);
        for (key, trace) in [("measured", &t.measured), ("predicted", &t.predicted)] {
            assert!(!trace.samples.is_empty());
            assert_eq!(s[key]["count"], trace.samples.len());
            assert_eq!(s[key]["first"]["time_s"], trace.samples[0].time_s);
            assert_eq!(s[key]["last"]["value"], trace.samples.last().unwrap().value);
        }
        eprintln!("{} ({}, passes {}): measured {} samples {} → {}, predicted {} samples {} → {}", t.id, t.split, t.comparison.passes, s["measured"]["count"], s["measured"]["first"], s["measured"]["last"], s["predicted"]["count"], s["predicted"]["first"], s["predicted"]["last"]);
    }
    let kept = b.calibration.selected.clone();
    let e = b.select_calibration_trial("no-such-trial").unwrap_err();
    assert!(e.contains("no-such-trial"), "{e}");
    assert_eq!(b.calibration.selected, kept, "the previous selection stays");
    // REST: trial without a path selects in place, no reload.
    let train = a.trials.iter().find(|t| !held_out(t)).unwrap();
    let mut continuation = serde_json::Value::Null;
    match b.calibration_rest(&serde_json::json!({"trial": train.id}), &mut continuation, false) {
        sim_api::Outcome::Done(Ok(v)) => assert_eq!(v["selected"]["role"], "train (fitting data)"),
        _ => panic!("trial selection should finish at once"),
    }
    assert!(b.calibration.pending().is_none());
    // A reload that no longer contains the selection clears it (drop it from a copy).
    let mut shown = b.calibration.shown.clone().unwrap();
    shown.archive.trials.retain(|t| t.id != train.id);
    b.calibration.job = Some(Job { path: dir.clone(), work: crate::jobs::Job::finished(99, Ok(shown)) });
    assert!(b.finish_calibration());
    assert!(b.calibration.selected.is_none() && b.calibration_json()["selected"].is_null());
}
