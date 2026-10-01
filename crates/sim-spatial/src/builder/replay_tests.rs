use super::*;
use super::live_run::swap_with;

fn wait(b: &mut Builder) -> ReplayOutcome {
    let id = b.replay.job.as_ref().unwrap().id.clone();
    for _ in 0..600 {
        b.poll_replay();
        if b.replay.job.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    b.replay.outcomes[&id].clone()
}

#[test]
fn saved_runs_replay_in_the_background_and_cancel() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("builder-replay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("winch.system.json");
    std::fs::copy(root.join("examples/systems-builder/worm-drive/winch.system.json"), &path).unwrap();
    let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
    let mut b = Builder::open(path.clone(), root.join("library/systems"), registry.clone()).unwrap();
    let runs = sim_runtime::run_history::dir_for(&path);
    let select = vec!["drum.shaft.speed".to_string()];
    let clean = sim_runtime::run_history::record(&b.document, &registry, 0.5, system_builder::config_for(&b.document), &select, "clean").unwrap();
    let mut edited = sim_runtime::run_history::record(&b.document, &registry, 0.5, system_builder::config_for(&b.document), &select, sim_runtime::run_history::EDITED_WHILE_RUNNING).unwrap().with_provenance(Fidelity::Detailed, true);
    edited.id.push_str("-edited");
    let long = sim_runtime::run_history::record(&b.document, &registry, 30., system_builder::config_for(&b.document), &select, "long").unwrap();
    let mut long = long;
    long.id.push_str("-long");
    for r in [&clean, &edited, &long] {
        sim_runtime::run_history::save(&runs, r).unwrap();
    }
    b.runs = sim_runtime::run_history::list(&runs);

    let e = b.replay_run("does-not-exist").unwrap_err();
    assert!(e.contains("does-not-exist"), "{e}");
    assert!(b.replay.job.is_none());

    b.replay_run(&clean.id).unwrap();
    assert_eq!(b.replay.outcomes[&clean.id].status, "running");
    let o = wait(&mut b);
    assert_eq!((o.status, o.max_rel_diff, o.edited_while_running), ("done", Some(0.), false), "{o:?}");
    assert!(o.headline().starts_with("Reproduced exactly"), "{}", o.headline());
    assert!(o.samples.unwrap() > 0);

    b.replay_run(&edited.id).unwrap();
    let o = wait(&mut b);
    assert_eq!((o.status, o.max_rel_diff, o.edited_while_running), ("done", Some(0.), true));

    // Cancel stops the worker between steps; it reports no result.
    b.replay_run(&long.id).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(b.cancel_replay());
    let o = b.replay.outcomes[&long.id].clone();
    assert_eq!((o.status, o.max_rel_diff), ("cancelled", None));
    assert!(!b.cancel_replay(), "nothing left to cancel");
    // The shared replay really stops: a raised flag ends a 30 s rerun at once.
    let started = std::time::Instant::now();
    let stopped = sim_runtime::run_history::replay_with_cancel(&long, &registry, Some(&std::sync::atomic::AtomicBool::new(true)));
    assert_eq!(stopped.unwrap_err(), system_builder::CANCELLED);
    assert!(started.elapsed().as_secs_f64() < 2., "{:?}", started.elapsed());
    let state = b.state_json(&Default::default());
    assert_eq!(state["replay"]["outcomes"].as_array().unwrap().len(), 3);
    std::fs::remove_dir_all(&dir).ok();
}

/// Step and Reset on a live run go through `run_step`/`run_reset` (the
/// buttons' and REST's path) to `Command::Step`/`Command::Reset` on the
/// real run thread: one step is exactly one interval, stepping a running
/// run is refused, reset keeps the old run and clears the history, and a
/// run saved after the reset holds only post-reset samples and replays
/// exactly. Starting/resuming uses the control channel (no Bevy scene).
#[test]
fn live_run_step_and_reset_share_the_session_and_keep_work() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("builder-step-reset-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("winch.system.json");
    std::fs::copy(root.join("examples/systems-builder/worm-drive/winch.system.json"), &path).unwrap();
    let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
    let mut b = Builder::open(path.clone(), root.join("library/systems"), registry.clone()).unwrap();
    let document = b.document.clone();
    let interval = system_builder::config_for(&document).interval;
    let compiled = system_builder::compile(&document, &registry, system_builder::config_for(&document)).unwrap();
    let observed: Vec<String> = compiled.description.observables.keys().filter(|id| system_builder::observable_key(&compiled.description, id).contains("drum.shaft.speed")).cloned().collect();
    let key = observed[0].clone();
    b.last_description = Some(compiled.description.clone());
    assert!(b.run_step().is_err() && b.run_reset().is_err(), "no run: refused");
    assert_eq!(b.state_json(&Default::default())["live_run"], serde_json::Value::Null);
    b.run = Some(LiveRun::spawn(document.clone(), registry.clone(), observed, compiled.description.id.clone(), Fidelity::Detailed));
    let shared = b.run.as_ref().unwrap().worker.shared().clone();
    let status = || shared.lock().unwrap().snapshot.as_ref().and_then(|x| x.status.clone());
    let time = || status().map(|x| x.time).unwrap_or(0.);
    let started = std::time::Instant::now();
    let until = |done: &dyn Fn() -> bool| {
        while !done() && started.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(done(), "timed out");
    };
    // Stepping a running run is refused, never ignored.
    assert!(shared.lock().unwrap().running);
    assert_eq!(b.run_step(), Err("pause the run before stepping".to_string()));
    until(&|| time() >= 0.3);
    b.run_pause();
    until(&|| !shared.lock().unwrap().running && status().is_some_and(|x| x.phase == sim_inspect::live::Phase::Paused));
    let before = status().unwrap();
    b.run_step().unwrap();
    until(&|| status().is_some_and(|x| x.step == before.step + 1));
    let after = status().unwrap();
    assert!((after.time - before.time - interval).abs() < 1e-9, "{} -> {} with dt {interval}", before.time, after.time);
    assert_eq!(b.history(&key).last().map(|p| p[0]), Some(after.time), "the stepped point is on the trace");
    let state = b.state_json(&Default::default());
    assert_eq!((state["realtime"].as_bool(), state["live_run"]["step"].as_u64(), state["live_run"]["phase"].as_str(), state["live_run"]["fidelity"].as_str()), (Some(false), Some(after.step), Some("paused"), Some("detailed")));
    // Reset keeps the run so far, then returns to t = 0 with no history.
    let runs = b.runs.len();
    b.run_reset().unwrap();
    assert_eq!(b.runs.len(), runs + 1, "{}", b.status);
    assert!(b.status.contains(&format!("kept the previous run as {}", b.runs[0].1.id)), "{}", b.status);
    assert!(b.runs[0].1.duration >= 0.3);
    assert!(b.history(&key).is_empty());
    until(&|| !shared.lock().unwrap().reset_pending);
    let reset = status().unwrap();
    assert_eq!((reset.time, reset.step, reset.phase, reset.generation), (0., 0, sim_inspect::live::Phase::Paused, before.generation + 1));
    assert!(b.history(&key).is_empty());
    assert_eq!(b.state_json(&Default::default())["live_run"]["time"].as_f64(), Some(0.));
    // Step once from t = 0, resume briefly, pause and save.
    b.run_step().unwrap();
    until(&|| status().is_some_and(|x| x.step == 1));
    b.run.as_ref().unwrap().worker.send(RunControl::Start).unwrap();
    until(&|| time() >= 0.12);
    b.run_pause();
    until(&|| !shared.lock().unwrap().running);
    let duration = time();
    assert!(duration < 0.3);
    let saved = b.save_run("after reset").unwrap();
    let record = sim_runtime::run_history::load(&saved).unwrap();
    assert_eq!((record.duration, record.fidelity, record.edited_while_running()), (duration, Some(Fidelity::Detailed), false));
    let times = &record.series.iter().find(|s| s.observable == key).unwrap().times;
    assert_eq!(times.first().copied(), Some(interval), "starts at the post-reset step");
    assert!(times.windows(2).all(|w| w[0] < w[1]) && times.iter().all(|t| *t <= duration), "only post-reset samples: {times:?}");
    b.replay_run(&record.id).unwrap();
    let o = wait(&mut b);
    assert_eq!((o.status, o.max_rel_diff), ("done", Some(0.)), "{o:?}");
    assert!(o.headline().starts_with("Reproduced exactly"), "{}", o.headline());
    b.run = None;
    std::fs::remove_dir_all(&dir).ok();
}

/// The Alt-drag grab swaps through `hot_swap` like an edit: a realtime run
/// gets the realtime profile of the pushed document (never the detailed
/// one), `run.document` is what was sent, the file's document is
/// untouched, and a save records the pushed model as edited. Release goes
/// back to the file value and the run stays edited. Pause says so.
#[test]
fn grab_swap_keeps_the_run_fidelity_and_marks_it_edited() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("builder-grab-swap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("winch.system.json");
    std::fs::copy(root.join("examples/systems-builder/worm-drive/winch.system.json"), &path).unwrap();
    let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
    let mut b = Builder::open(path.clone(), root.join("library/systems"), registry.clone()).unwrap();
    let file = b.document.clone();
    let profile = Fidelity::Realtime.document(&file, &registry).unwrap();
    let compiled = system_builder::compile(&profile, &registry, system_builder::config_for(&profile)).unwrap();
    let (parameter, original) = compiled.description.components.values().find_map(|c| match c.component_type.as_str() {
        "rotational.load_torque" => Some((format!("{}.torque", c.id), c.parameters["torque"].value)),
        "translational.load_force" => Some((format!("{}.force", c.id), c.parameters["force"].value)),
        _ => None,
    }).expect("the winch has a load the grab pushes on");
    b.last_description = Some(compiled.description.clone());
    b.run = Some(LiveRun::spawn(profile.clone(), registry.clone(), Vec::new(), compiled.description.id.clone(), Fidelity::Realtime));
    let shared = b.run.as_ref().unwrap().worker.shared().clone();
    let time = || shared.lock().unwrap().snapshot.as_ref().and_then(|x| x.status.as_ref()).map(|x| x.time).unwrap_or(0.);
    let started = std::time::Instant::now();
    while time() < 0.15 && started.elapsed().as_secs() < 60 {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Push: the same edit the gesture makes, on the detailed document.
    let pushed = original * 0.5;
    let mut detailed = file.clone();
    sim_system::apply(&mut detailed, &registry, &[sim_runtime::lesson::set_command(&parameter, pushed).unwrap()]).unwrap();
    let expected = Fidelity::Realtime.document(&detailed, &registry).unwrap();
    assert_ne!(expected.content_hash(), detailed.content_hash());
    assert!(swap_with(&mut b, &parameter, pushed));
    let run = b.run.as_ref().unwrap();
    assert_eq!(run.document.content_hash(), expected.content_hash(), "the realtime profile of the pushed document was sent");
    assert_ne!(run.document.content_hash(), detailed.content_hash(), "never the detailed document");
    assert_eq!((run.edited, run.fidelity, run.description_id.as_str()), (true, Fidelity::Realtime, compiled.description.id.as_str()));
    assert_eq!(b.document.content_hash(), file.content_hash(), "the file's document is untouched");
    // Pause is truthful and a save records the pushed model as edited.
    b.run_pause();
    assert!(b.status.starts_with("Paused"), "{}", b.status);
    while shared.lock().unwrap().running && started.elapsed().as_secs() < 60 {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(b.state_json(&Default::default())["live_run"]["phase"].as_str(), Some("paused"));
    let record = sim_runtime::run_history::load(&b.save_run("grab").unwrap()).unwrap();
    assert_eq!((record.fidelity, record.edited_while_running(), record.content_hash.clone()), (Some(Fidelity::Realtime), true, expected.content_hash()));
    // Release: back to the file value, still edited.
    assert!(swap_with(&mut b, &parameter, original));
    let run = b.run.as_ref().unwrap();
    let mut back = file.clone();
    sim_system::apply(&mut back, &registry, &[sim_runtime::lesson::set_command(&parameter, original).unwrap()]).unwrap();
    assert_eq!((run.document.content_hash(), run.edited), (Fidelity::Realtime.document(&back, &registry).unwrap().content_hash(), true));
    assert_eq!(b.document.content_hash(), file.content_hash());
    b.run = None;
    std::fs::remove_dir_all(&dir).ok();
}

/// A run started with Realtime on is saved with the realtime profile it
/// simulated (document, config, fidelity) and replays exactly. Drives the
/// real live thread and `save_run`; the Bevy system that hot-swaps edits
/// is not exercised here.
#[test]
fn realtime_runs_save_the_profile_they_ran_and_replay_exactly() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("builder-realtime-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("winch.system.json");
    std::fs::copy(root.join("examples/systems-builder/worm-drive/winch.system.json"), &path).unwrap();
    let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
    let mut b = Builder::open(path.clone(), root.join("library/systems"), registry.clone()).unwrap();
    let profile = Fidelity::Realtime.document(&b.document, &registry).unwrap();
    assert_ne!(profile.content_hash(), b.document.content_hash(), "the winch has a realtime profile");
    let compiled = system_builder::compile(&profile, &registry, system_builder::config_for(&profile)).unwrap();
    let observed: Vec<String> = compiled.description.observables.keys().filter(|id| system_builder::observable_key(&compiled.description, id).contains("drum.shaft.speed")).cloned().collect();
    assert!(!observed.is_empty());
    b.last_description = Some(compiled.description.clone());
    // What start_run launches with Realtime on.
    b.realtime = true;
    b.run = Some(LiveRun::spawn(profile.clone(), registry.clone(), observed, compiled.description.id.clone(), Fidelity::Realtime));
    let shared = b.run.as_ref().unwrap().worker.shared().clone();
    let time = || shared.lock().unwrap().snapshot.as_ref().and_then(|x| x.status.as_ref()).map(|x| x.time).unwrap_or(0.);
    let started = std::time::Instant::now();
    while time() < 0.3 && started.elapsed().as_secs() < 60 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    b.run.as_ref().unwrap().worker.send(RunControl::Pause).unwrap();
    while shared.lock().unwrap().running && started.elapsed().as_secs() < 60 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(time() >= 0.3, "run advanced to {}", time());
    // Turning the toggle off mid-run does not relabel the run.
    b.realtime = false;
    let saved = b.save_run("rt").unwrap();
    let record = sim_runtime::run_history::load(&saved).unwrap();
    assert_eq!((record.schema.as_str(), record.fidelity, record.edited_while_running()), (sim_runtime::run_history::SCHEMA, Some(Fidelity::Realtime), false));
    assert_eq!(record.content_hash, profile.content_hash());
    assert_eq!(serde_json::to_value(&record.config).unwrap(), serde_json::to_value(system_builder::config_for(&profile)).unwrap());
    assert_eq!(b.runs[0].1.fidelity, "realtime");
    b.replay_run(&record.id).unwrap();
    let o = wait(&mut b);
    assert_eq!((o.status, o.max_rel_diff, o.fidelity.as_str()), ("done", Some(0.), "realtime"), "{o:?}");
    assert!(o.headline().starts_with("Reproduced exactly") && o.headline().ends_with("realtime"), "{}", o.headline());
    let state = b.state_json(&Default::default());
    assert_eq!(state["replay"]["outcomes"][0]["fidelity"], "realtime");
    // The detailed document (what was recorded before) does not reproduce it.
    let mut detailed = record.clone();
    detailed.config = system_builder::config_for(&b.document);
    detailed.document = b.document.clone();
    let wrong = sim_runtime::run_history::replay(&detailed, &registry);
    assert!(wrong.as_ref().map(|d| *d > 0.).unwrap_or(true), "{wrong:?}");
    b.run = None;
    std::fs::remove_dir_all(&dir).ok();
}
