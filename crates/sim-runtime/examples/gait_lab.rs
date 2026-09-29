//! Readable gait files for people and language models.
//!
//!   gait_lab export   STUDY_CONFIG TRIAL_DIR OUT.yaml
//!   gait_lab validate STUDY_CONFIG FILE...
//!   gait_lab evaluate STUDY_CONFIG OUT_DIR [--parallel N] [--fidelity fast|detailed] FILE...
//!   gait_lab poses    STUDY_CONFIG OUT_DIR POSE_FILE...
//!   gait_lab steer    STUDY_CONFIG OUT_DIR GAIT_FILE MANEUVER_FILE...
//!   gait_lab tune     STUDY_CONFIG GAIT_FILE NEW_STUDY_DIR [--attempts N] [--seeds S1,S2,...] [--run]
//!
//! STUDY_CONFIG is a compare_gait_search config (robot, screens, qualified
//! reduced model, gates). Evaluation writes OUT_DIR/<gait>-<hash>/report.yaml,
//! compiled.json (playback) and evaluation.json, appends OUT_DIR/journal.jsonl,
//! and reuses the report for an identical file. Create OUT_DIR/STOP to cancel.
use sim_runtime::gait_lab::{self, Fidelity, GaitReport, Study};
use std::path::{Path, PathBuf};

type R<T> = Result<T, String>;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> R<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: gait_lab export STUDY TRIAL_DIR OUT.yaml | validate STUDY FILE... | evaluate STUDY OUT_DIR [--parallel N] [--fidelity fast|detailed] FILE... | poses STUDY OUT_DIR FILE... | steer STUDY OUT_DIR GAIT MANEUVER... | tune STUDY GAIT_FILE NEW_STUDY_DIR [--attempts N] [--seeds S1,S2] [--run]";
    let (command, rest) = args.split_first().ok_or(usage)?;
    let study_path = rest.first().ok_or(usage)?;
    let mut study = Study::load(Path::new(study_path), &repo())?;
    eprintln!("study {} ({}), legs {:?}, controller {:?}", study_path, &study.id[..8], study.legs.iter().map(|l| &l.name).collect::<Vec<_>>(), study.controller_ranges);
    match command.as_str() {
        "export" => {
            let [_, trial, out] = rest else { return Err(usage.into()) };
            let proposal: serde_json::Value = serde_json::from_slice(&std::fs::read(Path::new(trial).join("proposal.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let values = serde_json::from_value(proposal["values"].clone()).map_err(|e| e.to_string())?;
            let mut script = study.export(&values)?;
            script.name = Path::new(trial).file_name().and_then(|s| s.to_str()).unwrap_or("exported").to_string();
            // A study made by `gait_lab tune` fixed the controller values it did
            // not search; put them back so the file evaluates in the lab study.
            let side = Path::new(study_path).with_file_name("gait-lab.json");
            if let Ok(bytes) = std::fs::read(&side) {
                let lab: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                let fixed: std::collections::BTreeMap<String, f64> = serde_json::from_value(lab["fixed_controller"].clone()).map_err(|e| e.to_string())?;
                script.controller.extend(fixed);
                script.notes = format!("Tuned from {} by the search in {}.", lab["gait_file"].as_str().unwrap_or("?"), side.parent().map(|p| p.display().to_string()).unwrap_or_default());
            }
            let speed = std::fs::read(Path::new(trial).join("evaluation.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()).and_then(|e| e["eligible_speed_m_s"].as_f64());
            let header = format!(
                "Exported from {trial}\nSearch result: {}\nEdit and evaluate with: gait_lab evaluate {study_path} OUT_DIR THIS_FILE",
                speed.map_or("not passed".into(), |s| format!("{s:.3} m/s"))
            );
            std::fs::write(out, gait_lab::write_script(&script, &header)?).map_err(|e| e.to_string())?;
            // The file must describe exactly the trial's motion.
            let back = gait_lab::read_script(Path::new(out))?;
            let motion = back.motion(&study.legs, study.direction())?;
            if serde_json::to_value(&motion).map_err(|e| e.to_string())? != serde_json::to_value(study.study_motion(&values)?).map_err(|e| e.to_string())? {
                eprintln!("note: the written file differs from the trial's motion in the last digits (float formatting)");
            }
            println!("wrote {out}");
        }
        "validate" => {
            let mut bad = 0;
            for f in &rest[1..] {
                let result = gait_lab::read_script(Path::new(f)).and_then(|s| study.with_script(&s)).and_then(|(recipe, values)| recipe.schedule_screen(&values));
                match result {
                    Ok(()) => println!("ok       {f}"),
                    Err(e) => {
                        bad += 1;
                        println!("invalid  {f}: {e}");
                    }
                }
            }
            if bad > 0 {
                return Err(format!("{bad} invalid file(s)"));
            }
        }
        "evaluate" => {
            let out = PathBuf::from(rest.get(1).ok_or(usage)?);
            let mut parallel = 8usize;
            let mut fidelity = Fidelity::Fast;
            let mut files = vec![];
            let mut i = 2;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--parallel" => {
                        parallel = rest.get(i + 1).and_then(|v| v.parse().ok()).filter(|n| *n > 0).ok_or("--parallel needs a positive number")?;
                        i += 1;
                    }
                    "--fidelity" => {
                        fidelity = match rest.get(i + 1).map(String::as_str) {
                            Some("fast") => Fidelity::Fast,
                            Some("detailed") => Fidelity::Detailed,
                            _ => return Err("--fidelity is fast or detailed".into()),
                        };
                        i += 1;
                    }
                    f => files.push(PathBuf::from(f)),
                }
                i += 1;
            }
            if files.is_empty() {
                return Err(usage.into());
            }
            if fidelity == Fidelity::Fast {
                eprintln!("reduced model qualified at {:.2}x speedup", study.qualify()?);
            }
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            let study = &study;
            let stop = || out.join("STOP").exists();
            let mut reports: Vec<GaitReport> = vec![];
            for chunk in files.chunks(parallel) {
                let done: Vec<R<GaitReport>> = std::thread::scope(|scope| {
                    let handles: Vec<_> = chunk
                        .iter()
                        .map(|f| {
                            let (out, stop) = (&out, &stop);
                            scope.spawn(move || -> R<GaitReport> {
                                let script = match gait_lab::read_script(f) {
                                    Ok(s) => s,
                                    Err(e) => return Err(e),
                                };
                                study.evaluate(&script, f, out, fidelity, stop)
                            })
                        })
                        .collect();
                    handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("evaluation panicked".into()))).collect()
                });
                for (f, r) in chunk.iter().zip(done) {
                    match r {
                        Ok(report) => {
                            gait_lab::journal(&out, &report)?;
                            println!("{:<13} {:>7}  {}  {}{}", report.status, report.speed_m_s.map_or("-".into(), |s| format!("{s:.3}")), f.display(), report.summary, if report.cached { " (cached)" } else { "" });
                            reports.push(report);
                        }
                        Err(e) => println!("{:<13} {:>7}  {}  {e}", "invalid", "-", f.display()),
                    }
                }
                if stop() {
                    break;
                }
            }
            reports.sort_by(|a, b| b.speed_m_s.partial_cmp(&a.speed_m_s).unwrap_or(std::cmp::Ordering::Equal));
            println!("\nleaderboard (m/s, {} fidelity):", if fidelity == Fidelity::Fast { "fast" } else { "detailed" });
            for r in reports.iter().filter(|r| r.speed_m_s.is_some()) {
                println!("  {:.3}  {}  {}/report.yaml", r.speed_m_s.unwrap(), r.gait, r.results_directory);
            }
        }
        "poses" => {
            let out = PathBuf::from(rest.get(1).ok_or(usage)?);
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            let mut blocked = 0;
            for f in &rest[2..] {
                let report = gait_lab::read_poses(Path::new(f)).and_then(|s| study.compile_poses(&s, Path::new(f), &out));
                match report {
                    Ok(r) => {
                        blocked += usize::from(r.status != "ready");
                        println!("{:<8} {f}  {}\n         {}/report.yaml", r.status, r.summary, r.results_directory);
                    }
                    Err(e) => {
                        blocked += 1;
                        println!("invalid  {f}: {e}");
                    }
                }
            }
            if blocked > 0 {
                return Err(format!("{blocked} pose file(s) not ready"));
            }
        }
        "steer" => {
            let out = PathBuf::from(rest.get(1).ok_or(usage)?);
            let gait = gait_lab::read_script(Path::new(rest.get(2).ok_or(usage)?))?;
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            let mut blocked = 0;
            for f in &rest[3..] {
                match gait_lab::read_maneuver(Path::new(f)).and_then(|m| study.check_maneuver(&gait, &m, Path::new(f), &out)) {
                    Ok(r) => {
                        blocked += usize::from(r.status != "ready");
                        println!("{:<8} {f}  {}\n         {}/report.yaml", r.status, r.summary, r.results_directory);
                    }
                    Err(e) => {
                        blocked += 1;
                        println!("invalid  {f}: {e}");
                    }
                }
            }
            if blocked > 0 {
                return Err(format!("{blocked} maneuver file(s) not ready"));
            }
        }
        "tune" => {
            let (gait, dir) = (rest.get(1).ok_or(usage)?, PathBuf::from(rest.get(2).ok_or(usage)?));
            let (mut attempts, mut seeds, mut launch) = (30usize, vec![4101u64, 4102, 4103, 4104], false);
            let mut i = 3;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--attempts" => {
                        attempts = rest.get(i + 1).and_then(|v| v.parse().ok()).filter(|n| *n >= 2).ok_or("--attempts needs a number >= 2")?;
                        i += 1;
                    }
                    "--seeds" => {
                        seeds = rest.get(i + 1).ok_or("--seeds needs a list")?.split(',').map(|s| s.trim().parse().map_err(|_| format!("bad seed {s}"))).collect::<R<_>>()?;
                        i += 1;
                    }
                    "--run" => launch = true,
                    other => return Err(format!("unknown option {other}")),
                }
                i += 1;
            }
            if dir.exists() {
                return Err(format!("{} exists; choose a new study directory", dir.display()));
            }
            let script = gait_lab::read_script(Path::new(gait))?;
            let rel = |p: &Path| p.display().to_string();
            let mut config = study.tune_config(&script, &rel(&dir.join("qualification")))?;
            config["optimizer_seeds"] = serde_json::json!(seeds);
            config["attempts_per_algorithm_seed"] = serde_json::json!(attempts);
            config["parallel_streams"] = serde_json::json!(2 * seeds.len());
            config["settings"]["seed"] = serde_json::json!(seeds[0]);
            let fixed: std::collections::BTreeMap<&String, f64> = script.controller.iter().filter(|(k, _)| !script.tune.contains_key(&format!("controller.{k}"))).map(|(k, v)| (k, *v)).collect();
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let write = |name: &str, text: String| std::fs::write(dir.join(name), text).map_err(|e| e.to_string());
            write("comparison-config.json", serde_json::to_string(&config).map_err(|e| e.to_string())?)?;
            write("profile.json", serde_json::to_string_pretty(&config["profile"]).map_err(|e| e.to_string())?)?;
            write("gait.yaml", std::fs::read_to_string(gait).map_err(|e| e.to_string())?)?;
            write("gait-lab.json", serde_json::to_string_pretty(&serde_json::json!({"gait_file": gait, "source_study": study_path, "fixed_controller": fixed, "tuned": script.tune})).map_err(|e| e.to_string())?)?;
            let limit = 2 * seeds.len() * attempts;
            let n = rel(&dir);
            write("run.sh", format!(r#"#!/bin/sh
# Search the tune ranges of {gait} (gait_lab tune). Qualify the study's reduced
# model on this gait's baseline, then run the optimizer streams. Cancel: touch STOP here.
cd "{repo}"
N={n}
B=target/release/examples
[ -f $N/baseline.spec.json ] || $B/prepare_gait_candidate $N/comparison-config.json $N/baseline.spec.json > /dev/null || exit 1
if ! grep -q '"qualified":true' $N/qualification/qualification.json 2>/dev/null; then
  [ -d $N/qualification ] || $B/reduced_exploration prepare $N/baseline.spec.json $N/profile.json $N/qualification --fresh || exit 1
  $B/reduced_exploration qualify $N/qualification $N/STOP || {{ echo "the reduced model did not qualify on this baseline"; exit 1; }}
fi
$B/compare_gait_search $N/comparison-config.json $N/comparison {limit} $N/STOP
"#, repo = repo().canonicalize().map_err(|e| e.to_string())?.display()))?;
            println!("wrote {n}: searching {:?} from the file's values ({} streams x {attempts} attempts)", script.tune.keys().collect::<Vec<_>>(), 2 * seeds.len());
            if launch {
                let log = std::fs::File::create(dir.join("run.log")).map_err(|e| e.to_string())?;
                let child = std::process::Command::new("nice").args(["-n", "10", "sh", &format!("{n}/run.sh")]).current_dir(repo()).stdout(log.try_clone().map_err(|e| e.to_string())?).stderr(log).spawn().map_err(|e| e.to_string())?;
                println!("started (pid {}); log {n}/run.log", child.id());
            } else {
                println!("run: sh {n}/run.sh");
            }
        }
        _ => return Err(usage.into()),
    }
    Ok(())
}
