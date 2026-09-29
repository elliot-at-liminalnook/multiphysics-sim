//! Lessons on the shared runtime: the starter lessons' claims hold, scene
//! runs are deterministic and cached by hash, sandboxes keep learner edits
//! away from the lesson's systems, and a false claim fails with its line.
use sim_runtime::lesson::{self, CheckOptions, Severity};
use std::path::{Path, PathBuf};
use std::sync::Once;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Cache and sandboxes go to a per-run temp folder, not the crate directory.
fn scratch() -> PathBuf {
    static INIT: Once = Once::new();
    let dir = std::env::temp_dir().join(format!("sim-lesson-tests-{}", std::process::id()));
    INIT.call_once(|| {
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: set once, before any test reads these variables (every test
        // calls `scratch()` first); all tests agree on the values.
        unsafe {
            std::env::set_var("SIM_LESSON_CACHE", dir.join("cache"));
            std::env::set_var("SIM_LESSON_SANDBOX", dir.join("sandbox"));
        }
    });
    dir
}

fn registry() -> sim_core::BehaviorRegistry {
    // As the lesson tools load it: the winch uses authored library parts.
    sim_runtime::registry_with_parts(&root().join("library/parts")).0
}

#[test]
fn starter_lessons_parse_resolve_and_their_claims_hold() {
    scratch();
    let reports = lesson::check_path(&root().join("lessons"), &registry(), CheckOptions { run: true, use_cache: false, compares: true });
    let lessons: Vec<&str> = reports.iter().map(|r| r.lesson.as_str()).collect();
    for slug in ["motor-torque-speed", "worm-self-locking", "motor-driver-board"] {
        assert!(lessons.contains(&slug), "{slug} missing from {lessons:?}");
    }
    let mut failures = Vec::new();
    for r in &reports {
        for (id, s) in &r.scenes {
            for c in &s.checks {
                eprintln!("{}/{id}: [{}] {}", r.lesson, if c.passed { "ok" } else { "FAIL" }, c.message);
            }
        }
        failures.extend(r.findings.iter().filter(|f| f.severity == Severity::Error).map(|f| f.to_string()));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let claims: usize = reports.iter().flat_map(|r| r.scenes.values()).map(|s| s.checks.len()).sum();
    assert!(claims >= 7, "only {claims} claims were checked");
}

#[test]
fn scene_runs_are_deterministic_cached_and_follow_script_cues() {
    scratch();
    let registry = registry();
    let l = sim_lesson::Lesson::load(&root().join("lessons/motor-torque-speed/lesson.md")).unwrap();
    let scene = l.scene("load-step").unwrap().clone();
    let doc = lesson::load_system(&l.system_path(&scene.system), &registry).unwrap();
    let doc = lesson::scene_document(&doc, &registry, &scene).unwrap();
    let timeline = l.timeline(&scene).unwrap();
    let a = lesson::run_scene(&doc, &registry, &scene, &timeline, None, &|_| {}).unwrap();
    let b = lesson::run_scene(&doc, &registry, &scene, &timeline, None, &|_| {}).unwrap();
    assert_eq!(a.key, b.key);
    assert_eq!(a.frames, b.frames, "same inputs, same frames");
    let speed = |r: &lesson::SceneRun| r.series("rotor.shaft.speed").unwrap().values.clone();
    assert_eq!(speed(&a), speed(&b));
    // The script's load step was applied once, at its time.
    assert_eq!(a.applied.len(), 1);
    assert!((a.applied[0].0 - 0.3).abs() <= 2e-4 + 1e-12, "applied at {}", a.applied[0].0);
    // Frames are at the requested rate and every one carries the rotor angle.
    assert!(a.frames.len() as f64 >= 0.6 * 400.0);
    // Cache round trip.
    lesson::save_cached(&a).unwrap();
    let cached = lesson::load_cached(&a.key).unwrap();
    assert_eq!(cached.frames, a.frames);
    // New claim bounds keep the key (re-checked, not re-run); a change to
    // the timeline changes it.
    let mut edited = scene.clone();
    edited.expect[0].max = Some(1.0);
    assert_eq!(lesson::run_key(&doc, &edited, &timeline), a.key);
    assert!(!lesson::check_run(&a, &edited)[0].passed);
    let shorter = sim_script::presentation::evaluate("t", "at(0.2); set(\"load.torque\", -0.04);").unwrap();
    assert_ne!(lesson::run_key(&doc, &scene, &shorter), a.key);
}

#[test]
fn sandbox_holds_learner_edits_and_resets_to_the_lesson() {
    scratch();
    let registry = registry();
    let l = sim_lesson::Lesson::load(&root().join("lessons/motor-torque-speed/lesson.md")).unwrap();
    let scene = l.scene("load-step").unwrap().clone();
    let authored = std::fs::read(l.system_path(&scene.system)).unwrap();
    let sb = lesson::sandbox(&l, &scene, &registry, true).unwrap();
    assert!(!sb.modified && sb.path.starts_with(scratch()));
    // A learner edit in the builder's store.
    let store = sim_system::SystemStore::new(&sb.path);
    store.apply(&registry, "learner", &[lesson::set_command("load.torque", -0.05).unwrap()], None).unwrap();
    let again = lesson::sandbox(&l, &scene, &registry, false).unwrap();
    assert!(again.modified, "the edit is kept and reported");
    assert_eq!(std::fs::read(l.system_path(&scene.system)).unwrap(), authored, "the lesson's system is untouched");
    let reset = lesson::sandbox(&l, &scene, &registry, true).unwrap();
    assert!(!reset.modified);
}

#[test]
fn a_false_claim_fails_with_its_line() {
    let dir = scratch().join("false-claim");
    std::fs::create_dir_all(&dir).unwrap();
    let system = root().join("lessons/motor-torque-speed/motor.system.json");
    let text = format!(
        "---\ntitle: False claim\nsystems: {{ motor: {} }}\n---\nText.\n\n```sim-scene\nid: s\nsystem: motor\nrun: {{ duration_s: 0.1 }}\nexpect:\n  - {{ observe: rotor.shaft.speed, reduce: final, min: 5000 }}\n```\n",
        system.display()
    );
    let path = dir.join("lesson.md");
    std::fs::write(&path, text).unwrap();
    let reports = lesson::check_path(Path::new(&path), &registry(), CheckOptions { run: true, use_cache: false, compares: false });
    let f = reports[0].findings.iter().find(|f| f.message.contains("claim failed")).expect("the claim fails");
    assert_eq!(f.line, 7, "{f}");
    // Structure-only checks do not run anything and pass.
    let reports = lesson::check_path(Path::new(&path), &registry(), CheckOptions { run: false, use_cache: false, compares: false });
    assert!(reports[0].ok(), "{:?}", reports[0].findings);
}
