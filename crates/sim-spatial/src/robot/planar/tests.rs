use super::*;
use crate::robot::run::RunAction;
use std::time::Instant;

/// A ground block and one hanging link: the smallest planar export.
const FILE: &str = r#"{"format":"simrobot","version":2,"unit":"mm","bodies":[
        {"id":"g","name":"ground","mass_kg":1.0,"com":[0,220],"inertia_zz":0.001,"ground":true,"outline":[[[-25,200],[25,200],[25,240],[-25,240]]]},
        {"id":"t","name":"thigh","mass_kg":0.03,"com":[0,140],"inertia_zz":3.6e-5,"outline":[[[-8,80],[8,80],[8,200],[-8,200]]]}],
      "joints":[{"name":"hip","type":"revolute","child":"thigh","parent":"ground","pivot2":[0,200],"limits":[-1.0,1.0]}],"source":"/tmp/robot.rcad"}"#;

fn until(run: &mut PlanarRun, what: &str, done: impl Fn(&PlanarFrame) -> bool) -> PlanarFrame {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        run.poll();
        if let Some(f) = run.frame().filter(|f| done(f)) {
            return f.clone();
        }
        assert!(Instant::now() < deadline, "{what}: no such frame");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn gains_match_the_fidelity_text() {
    assert_eq!((PLANAR_BANDWIDTH_HZ, PLANAR_DAMPING_RATIO), (6.0, 1.0), "update FIDELITY with the gains");
    assert!(FIDELITY.contains("6 Hz") && FIDELITY.contains("damping ratio 1"));
}

/// The robot-mode loader routes a v2 file to the planar summary (never
/// PhysicalModel) and a v3 file to the physical loader; the worker check does the same.
#[test]
fn version_routes_the_file() {
    let path = Path::new("/tmp/planar.simrobot.json");
    match crate::robot::load_file_bytes(path, FILE.as_bytes()).unwrap() {
        crate::robot::source::FileModel::Planar(p) => {
            assert_eq!((p.version, p.declared_version, p.model.bodies.len()), (2, Some(2), 2));
        }
        crate::robot::source::FileModel::Physical(_) => panic!("a v2 file must not load as physical"),
    }
    let unit = FILE.replace(r#""unit":"mm""#, r#""unit":"in""#);
    assert!(crate::robot::load_file_bytes(path, unit.as_bytes()).err().unwrap().contains("unit `in`"));
    // A closed loop is refused by the shared build on the worker (it used to never finish building).
    let looped = FILE.replace(r#""limits":[-1.0,1.0]}]"#, r#""limits":[-1.0,1.0]},{"name":"back","type":"revolute","child":"ground","parent":"thigh","pivot2":[0,80]}]"#);
    assert_ne!(looped, FILE);
    let e = crate::robot::load_file_bytes(path, looped.as_bytes()).err().unwrap();
    assert!(e.contains("does not build") && e.contains("joint back closes a loop"), "{e}");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let v3 = root.join("examples/wheeled-robot/baseline/robot.simrobot.json");
    let bytes = std::fs::read(&v3).unwrap();
    assert!(matches!(crate::robot::load_file_bytes(&v3, &bytes).unwrap(), crate::robot::source::FileModel::Physical(_)));
    let dir = std::env::temp_dir().join(format!("sim-spatial-planar-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("robot.simrobot.json");
    std::fs::write(&file, FILE).unwrap();
    let checked = crate::robot::source::check(&file, None);
    assert_eq!(checked.outcome.name(), "loaded");
    assert!(matches!(checked.outcome, crate::robot::source::Outcome::Loaded(crate::robot::source::FileModel::Planar(_))));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The run thread builds through the shared planar build, steps one grid,
/// moves a target from the current one, runs, and Reset rebuilds at t = 0
/// under the next generation.
#[test]
fn run_thread_builds_steps_moves_and_resets() {
    let loaded = load_bytes(Path::new("/tmp/planar.simrobot.json"), FILE.as_bytes(), &serde_json::from_str(FILE).unwrap()).unwrap();
    let mut run = PlanarRun::spawn(loaded.model, 0, 1.0);
    let built = until(&mut run, "built", |f| f.built);
    assert_eq!(built.joint_names, ["hip"]);
    assert!(!built.outlines.is_empty() && built.tips.len() == 1 && built.root_fixed);
    assert_eq!(built.phase, PlanarPhase::Paused);
    assert!(run.check(RunAction::Pause).is_err());
    run.act(RunAction::Step).unwrap();
    let stepped = until(&mut run, "stepped", |f| f.steps == 1);
    assert!((stepped.time - GRID_S).abs() < 1e-9, "{}", stepped.time);
    let before = stepped.targets[0];
    run.nudge("hip", 0.05).unwrap();
    until(&mut run, "nudged", |f| f.targets.first().is_some_and(|t| (t - (before + 0.05)).abs() < 1e-12));
    assert!(run.nudge("knee", 0.05).unwrap_err().contains("unknown joint `knee`"));
    assert!(run.set_target("hip", f64::NAN).is_err());
    run.act(RunAction::Start).unwrap();
    assert!(run.check(RunAction::Step).is_err());
    until(&mut run, "running", |f| f.steps >= 3);
    run.act(RunAction::Pause).unwrap();
    run.act(RunAction::Reset).unwrap();
    assert!(run.frame().is_none());
    let reset = until(&mut run, "reset", |f| f.built && f.generation == 1);
    assert_eq!((reset.steps, reset.time), (0, 0.0));
    assert!((reset.targets[0] - built.targets[0]).abs() < 1e-12, "Reset returns targets to the CAD pose");
    let started = Instant::now();
    drop(run);
    assert!(started.elapsed() < Duration::from_secs(1), "the run thread stops when its channel closes");
}
