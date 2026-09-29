//! Every trial of a finished gait search converts to a readable gait file and
//! back to exactly the motion and controller values the search evaluated.
use sim_runtime::gait_lab::{self, Study};
use std::path::Path;

const STUDY: &str = "examples/full-robot/measured-actuator-integration/gait-search-measured-2026-09-23";

fn numbers(v: &serde_json::Value, out: &mut Vec<f64>) {
    match v {
        serde_json::Value::Number(n) => out.push(n.as_f64().unwrap()),
        serde_json::Value::Array(a) => a.iter().for_each(|v| numbers(v, out)),
        serde_json::Value::Object(o) => o.values().for_each(|v| numbers(v, out)),
        _ => {}
    }
}

#[test]
fn search_trials_round_trip_through_gait_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = root.join(STUDY);
    if !dir.join("comparison-config.json").is_file() {
        eprintln!("skipped: {STUDY} is not present");
        return;
    }
    let study = Study::load(&dir.join("comparison-config.json"), &root).unwrap();
    let mut checked = 0;
    for entry in std::fs::read_dir(dir.join("comparison")).unwrap() {
        let proposal = entry.unwrap().path().join("proposal.json");
        let Ok(bytes) = std::fs::read(&proposal) else { continue };
        let values = serde_json::from_value(serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["values"].clone()).unwrap();
        let script = study.export(&values).unwrap();
        // Through the file format itself, not just the in-memory structure.
        let text = gait_lab::write_script(&script, "round trip").unwrap();
        let file = std::env::temp_dir().join(format!("gait-lab-roundtrip-{checked}.yaml"));
        std::fs::write(&file, &text).unwrap();
        let back = gait_lab::read_script(&file).unwrap();
        std::fs::remove_file(&file).ok();
        let (recipe, file_values) = study.with_script(&back).unwrap();
        let (mut a, mut b) = (vec![], vec![]);
        numbers(&serde_json::to_value(study.study_motion(&values).unwrap()).unwrap(), &mut a);
        numbers(&serde_json::to_value(recipe.template.materialize(&file_values).unwrap()).unwrap(), &mut b);
        assert_eq!(a.len(), b.len(), "{}", proposal.display());
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() <= 1e-12 * x.abs().max(1.), "{}: {x} vs {y}", proposal.display());
        }
        for (k, v) in &back.controller {
            assert_eq!(values[k], *v, "{k}");
        }
        checked += 1;
    }
    eprintln!("{checked} trials round-tripped");
    assert!(checked >= 50, "only {checked} trials found");
}

fn measured_study() -> Option<(Study, std::path::PathBuf)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = root.join(STUDY).join("comparison-config.json");
    if !config.is_file() {
        eprintln!("skipped: {STUDY} is not present");
        return None;
    }
    Some((Study::load(&config, &root).unwrap(), root))
}

#[test]
fn a_pose_sequence_compiles_checks_and_plays() -> Result<(), String> {
    let Some((study, _)) = measured_study() else { return Ok(()) };
    let script: sim_domain_control::pose_script::PoseScript = serde_norway::from_str(
        "version: 1\nname: crouch\nposes:\n  stand: {}\n  crouch:\n    all: {foot_servo_deg: -90}\nsequence:\n  - {pose: stand, move_s: 1.5, hold_s: 1.0}\n  - {pose: crouch, move_s: 1.5, hold_s: 1.0}\n",
    )
    .unwrap();
    let out = std::env::temp_dir().join(format!("gait-lab-poses-{}", std::process::id()));
    let report = study.compile_poses(&script, Path::new("crouch.yaml"), &out).unwrap();
    assert_eq!(report.status, "ready", "{:?}", report.reasons);
    let compiled: serde_json::Value = serde_json::from_slice(&std::fs::read(Path::new(&report.results_directory).join("compiled.json")).unwrap()).unwrap();
    std::fs::remove_dir_all(&out).ok();
    // The gait player (browser mirror and physical leg) loads it as-is.
    let gait = sim_runtime::gait_playback::Gait::from_compiled(&compiled, "crouch").unwrap();
    assert_eq!(gait.info.period_s, 5.0);
    let foot = gait.index("+X | Foot servo output").unwrap();
    assert!((gait.sample(0.)?[foot] - (-60f64).to_radians()).abs() < 1e-3, "starts standing");
    assert!((gait.sample(3.0)?[foot] - (-90f64).to_radians()).abs() < 1e-12, "crouched after the move");
    assert!((gait.sample(5.0 - 1e-9)?[foot] - gait.sample(0.)?[foot]).abs() < 1e-6, "loops back to standing");
    // A pose past the command range is blocked with a reason.
    let mut bad = script.clone();
    bad.poses.get_mut("crouch").unwrap().all.insert("foot_servo_deg".into(), 10.);
    let blocked = study.compile_poses(&bad, Path::new("bad.yaml"), &out).unwrap();
    std::fs::remove_dir_all(&out).ok();
    assert_eq!(blocked.status, "blocked");
    assert!(blocked.reasons.iter().any(|r| r.contains("command range")), "{:?}", blocked.reasons);
    Ok(())
}

#[test]
fn tune_ranges_become_a_search_config() {
    let Some((study, _)) = measured_study() else { return };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(STUDY).join("comparison");
    let proposal = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path().join("proposal.json")).find(|p| p.is_file()).unwrap();
    let values = serde_json::from_value(serde_json::from_slice::<serde_json::Value>(&std::fs::read(proposal).unwrap()).unwrap()["values"].clone()).unwrap();
    let mut script = study.export(&values).unwrap();
    let travel = script.cycle.travel_m;
    script.tune.insert("cycle.travel_m".into(), [travel * 0.8, travel * 1.2]);
    script.tune.insert("leg_defaults.stance".into(), [0.6, 0.75]);
    let config = study.tune_config(&script, "somewhere/qualification").unwrap();
    let recipe: sim_runtime::contact_exploration::Recipe = serde_json::from_value(config["recipe"].clone()).unwrap();
    let names: Vec<&str> = recipe.template.space.parameters.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["cycle.travel_m", "leg_defaults.stance"], "only tuned numbers are searched");
    assert!(recipe.policy_bindings.is_empty(), "untuned controller values are fixed");
    let governor = &recipe.experiment.scene.controller.as_ref().unwrap().parameters["reference_governor"];
    assert_eq!(governor["maximum_speed_rad_s"].as_f64(), Some(script.controller["governor_speed_rad_s"]));
    assert_eq!(config["baseline"]["cycle.travel_m"].as_f64(), Some(travel));
    assert_eq!(config["qualification_directory"], "somewhere/qualification");
    // Its baseline is the file's motion.
    let motion = recipe.template.materialize(&serde_json::from_value(config["baseline"].clone()).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(&motion).unwrap(), serde_json::to_value(script.motion(&study.legs, study.direction()).unwrap()).unwrap());
    script.tune.clear();
    assert!(study.tune_config(&script, "x").unwrap_err().contains("nothing to search"));
}

const LAB: &str = "examples/full-robot/measured-actuator-integration/gait-lab-2026-09-25";

#[test]
fn a_maneuver_steers_a_gait_through_the_cad_model() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (config, gait) = (root.join(LAB).join("study.json"), root.join(LAB).join("gaits/aws-6216-Bayesian-009.yaml"));
    if !config.is_file() || !gait.is_file() {
        eprintln!("skipped: {LAB} is not present");
        return;
    }
    let study = Study::load(&config, &root).unwrap();
    let gait = gait_lab::read_script(&gait).unwrap();
    let arc: sim_domain_control::maneuver_script::ManeuverScript = serde_norway::from_str(
        "version: 1\nname: short-arc\nlimits: {forward_m_s: 0.3, lateral_m_s: 0.25, yaw_deg_s: 25, forward_m_s2: 0.15, lateral_m_s2: 0.15, yaw_deg_s2: 20}\ncommands:\n  - {at_s: 0}\n  - {at_s: 0.5, forward_m_s: 0.15, yaw_deg_s: 10}\nduration_s: 5\n",
    )
    .unwrap();
    let out = std::env::temp_dir().join(format!("gait-lab-maneuver-{}", std::process::id()));
    let report = study.check_maneuver(&gait, &arc, Path::new("short-arc.yaml"), &out).unwrap();
    assert_eq!(report.status, "ready", "{:?}", report.reasons);
    assert!(report.checked_s == 5. && report.turned_deg > 5. && report.travel_m[0].hypot(report.travel_m[1]) > 0.2, "{report:?}");
    assert!(report.maximum_marker_error_m < 1e-6);
    assert!(Path::new(&report.results_directory).join("trace.json").is_file());
    // Three times the gait's speed: the joints cannot keep up.
    let mut fast = arc.clone();
    fast.limits.forward_m_s = 0.8;
    fast.limits.forward_m_s2 = 2.;
    fast.commands[1] = sim_domain_control::maneuver_script::Command { at_s: 0.5, forward_m_s: 0.8, ..Default::default() };
    let blocked = study.check_maneuver(&gait, &fast, Path::new("fast.yaml"), &out).unwrap();
    std::fs::remove_dir_all(&out).ok();
    assert_eq!(blocked.status, "blocked");
    assert!(blocked.reasons.iter().any(|r| r.contains("motor") || r.contains("IK") || r.contains("limit")), "{:?}", blocked.reasons);
}
