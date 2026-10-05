//! List, check, run and edit lessons from the shell. `check` is what CI
//! runs: every system loads, every reference resolves, every script
//! evaluates and every `expect` claim holds on the recorded run. Edits go
//! through the same command layer and undo journal as the viewer.
use sim_lesson::edit::{Edit, LessonStore};
use sim_runtime::lesson::{self, CheckOptions, Severity};
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: sim-lesson <command> …
  list [DIR]                          lessons in DIR (default lessons/), in reading order
  check [PATH…] [--no-run] [--fresh] [--compares] [--json]
                                      validate lessons (a folder of lessons or lesson.md files);
                                      runs scenes and checks their claims unless --no-run;
                                      --fresh ignores cached runs; --compares also runs sim-compare studies
  run LESSON.md SCENE [--fresh] [--json]
                                      record one scene; print its checks and series summary
  blocks LESSON.md                    block IDs, lines and hashes (for edits)
  measured LESSON.md ID [--json]      a sim-measured block: each measured point beside its
                                      simulation, the gaps, and where each parameter came from
  model LESSON.md [--json]            each system's instances, parameters (value, unit, provenance)
                                      and observable keys: what {{param}}, {{value}}, equations and
                                      claims may name
  edit LESSON.md JSON [--label TEXT] [--expect REVISION]
                                      apply one edit ({\"edit\":\"replace_block\",…}; see sim_lesson::edit::Edit)
  undo LESSON.md | redo LESSON.md     shared history with the viewer
  figure FILE.svg OUT.png [--width PX] render a lesson diagram exactly as the viewer draws it
  concepts [DIR] [--progress FILE]    the concept map, each concept's mastery and the next lesson
  report LESSON.md PROGRESS.json…     where learners stall: questions missed, misconceptions,
                                      hints, reveals, time and rewinds per block (progress files
                                      learners chose to share)
  draft SLUG --system FILE --objectives TEXT [--rounds N] [--dir lessons]
                                      Codex drafts lessons/drafts/SLUG/lesson.md from
                                      lessons/AUTHORING_PROMPT.md, revised until check is clean
Cached runs live in runs/lessons/cache (override with SIM_LESSON_CACHE).";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            2
        }
    };
    std::process::exit(code);
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}
fn value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}
fn positional(args: &[String]) -> Vec<&String> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if a == "--label" || a == "--expect" || a == "--width" || a == "--progress" || a == "--system" || a == "--objectives" || a == "--rounds" || a == "--dir" {
            skip = true;
            continue;
        }
        if !a.starts_with("--") {
            out.push(a);
        }
    }
    out
}

fn run(args: &[String]) -> Result<i32, String> {
    let Some(command) = args.first() else { return Err(USAGE.into()) };
    let rest = &args[1..];
    let pos = positional(rest);
    match command.as_str() {
        "list" => {
            let dir = pos.first().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("lessons"));
            for e in sim_lesson::index::scan(&dir) {
                match &e.error {
                    Some(err) => println!("{:<28} BROKEN  {err}", e.slug),
                    None => println!("{:<28} {:>3} scene(s)  {}{}", e.slug, e.scenes, e.title, e.minutes.map(|m| format!("  ({m} min)")).unwrap_or_default()),
                }
            }
            Ok(0)
        }
        "check" => {
            let registry = sim_runtime::system_registry();
            let options = CheckOptions { run: !flag(rest, "--no-run"), use_cache: !flag(rest, "--fresh"), compares: flag(rest, "--compares") };
            let paths: Vec<PathBuf> = if pos.is_empty() { vec![PathBuf::from("lessons")] } else { pos.iter().map(PathBuf::from).collect() };
            let reports: Vec<_> = paths.iter().flat_map(|p| lesson::check_path(p, &registry, options)).collect();
            if flag(rest, "--json") {
                println!("{}", serde_json::to_string_pretty(&reports).map_err(|e| e.to_string())?);
            } else {
                for r in &reports {
                    for f in &r.findings {
                        println!("{f}");
                    }
                    if r.minutes > 0. {
                        println!("{}: about {:.0} min to work through (reading, questions, scenes)", r.lesson, r.minutes);
                    }
                    for (id, s) in &r.scenes {
                        let passed = s.checks.iter().filter(|c| c.passed).count();
                        println!("{}/{id}: {passed}/{} claims hold · {} · {:.2} s to compute · plays in {:.1} s on screen", r.lesson, s.checks.len(), s.fidelity, s.wall_seconds, s.screen_seconds);
                    }
                }
            }
            let errors = reports.iter().flat_map(|r| &r.findings).filter(|f| f.severity == Severity::Error).count();
            let lessons: std::collections::BTreeSet<_> = reports.iter().map(|r| &r.lesson).collect();
            eprintln!("{} lesson(s), {errors} error(s)", lessons.len());
            Ok(if errors == 0 { 0 } else { 1 })
        }
        "run" => {
            let (Some(file), Some(scene_id)) = (pos.first(), pos.get(1)) else { return Err(USAGE.into()) };
            let registry = sim_runtime::system_registry();
            let l = sim_lesson::Lesson::load(Path::new(file.as_str())).map_err(|e| e.to_string())?;
            let scene = &sim_runtime::lesson::lesson_scene(&l, l.scene(scene_id).ok_or_else(|| format!("no scene `{scene_id}` ({})", l.scenes().map(|(_, s)| s.id.clone()).collect::<Vec<_>>().join(", ")))?);
            let doc = lesson::load_system(&l.system_path(&scene.system), &registry)?;
            let doc = lesson::scene_document(&doc, &registry, scene)?;
            let timeline = l.timeline(scene)?;
            let run = lesson::scene_run(&doc, &registry, scene, &timeline, !flag(rest, "--fresh"), None, &|f| eprint!("\r{:>3.0} %", f * 100.0))?;
            eprintln!();
            if flag(rest, "--json") {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({"key":run.key,"fidelity":run.fidelity,"checks":run.checks,"error":run.error,"frames":run.frames.len(),"applied":run.applied})).map_err(|e| e.to_string())?);
            } else {
                println!("{} · {} frames · key {}", run.fidelity, run.frames.len(), run.key);
                for s in &run.series {
                    let (lo, hi) = s.values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
                    println!("  {:<32} final {:>12.6} {}  min {:.6}  max {:.6}", s.label, s.values.last().copied().unwrap_or(f64::NAN), s.unit, lo, hi);
                }
                for c in &run.checks {
                    println!("  [{}] {}", if c.passed { "ok" } else { "FAIL" }, c.message);
                }
                if let Some(e) = &run.error {
                    println!("  run stopped: {e}");
                }
            }
            Ok(if run.passed() { 0 } else { 1 })
        }
        "model" => {
            let file = pos.first().ok_or(USAGE)?;
            let registry = sim_runtime::system_registry();
            let l = sim_lesson::Lesson::load(Path::new(file.as_str())).map_err(|e| e.to_string())?;
            let model = sim_runtime::lesson_model::Model::new(&l, &registry, true);
            let mut all = serde_json::Map::new();
            for name in l.meta.systems.keys() {
                let (doc, d) = model.system(name)?;
                let observables: Vec<String> = d.observables.keys().map(|id| sim_runtime::system_builder::observable_key(&d, id)).collect();
                let origins = sim_runtime::lesson_model::origins(&doc);
                if flag(rest, "--json") {
                    all.insert(name.clone(), serde_json::json!({"components": d.components.iter().map(|(p, c)| (p.clone(), serde_json::json!({"type": c.component_type, "label": c.label, "parameters": c.parameters}))).collect::<serde_json::Map<_, _>>(), "observables": observables, "origins": origins}));
                    continue;
                }
                println!("system `{name}` ({})", l.system_path(name).display());
                for (path, c) in &d.components {
                    println!("  {path}  [{}] {}", c.component_type, c.label);
                    for (k, v) in &c.parameters {
                        let kind = origins.iter().find(|o| o.target == format!("{path}.{k}")).map(|o| format!("  ({}: {})", o.kind, o.detail.chars().take(70).collect::<String>())).unwrap_or_default();
                        println!("      {k} = {} {}{kind}", v.value, v.unit.clone().unwrap_or_default());
                    }
                }
                println!("  observables: {}", observables.join(", "));
            }
            if flag(rest, "--json") {
                println!("{}", serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?);
            }
            Ok(0)
        }
        "measured" => {
            let (Some(file), Some(id)) = (pos.first(), pos.get(1)) else { return Err(USAGE.into()) };
            let registry = sim_runtime::system_registry();
            let l = sim_lesson::Lesson::load(Path::new(file.as_str())).map_err(|e| e.to_string())?;
            let m = l.measured(id).ok_or_else(|| format!("no sim-measured `{id}`"))?;
            let model = sim_runtime::lesson_model::Model::new(&l, &registry, !flag(rest, "--fresh"));
            let r = sim_runtime::lesson_model::measured(&model, m)?;
            if flag(rest, "--json") {
                println!("{}", serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?);
                return Ok(0);
            }
            println!("{} · {} · data {} ({})", m.id, r.fidelity, m.data, r.data.description);
            println!("  {:>10} {:>12} {:>12} {:>10}", m.x.field, "measured", "simulated", "gap");
            for p in &r.points {
                println!("  {:>10.4} {:>12.4} {:>12.4} {:>+10.4}", p.x, p.measured, p.simulated, p.simulated - p.measured);
            }
            println!("  RMS gap {:.4} {} · largest {:.4} {}{}", r.rms, m.y.unit, r.max_gap, m.y.unit, if r.fitted_to_data { " · parameters were fitted to this data" } else { "" });
            for o in &r.origins {
                println!("  {:<34} {:>12.6} {:<8} {:<11} {}", o.target, o.value, o.unit, o.kind, o.detail.chars().take(60).collect::<String>());
            }
            for c in &r.checks {
                println!("  [{}] {}", if c.passed { "ok" } else { "FAIL" }, c.message);
            }
            Ok(0)
        }
        "concepts" => {
            let dir = pos.first().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("lessons"));
            let map = sim_lesson::concepts::load(&dir)?;
            let lessons: Vec<sim_lesson::Lesson> = sim_lesson::index::scan(&dir).iter().filter_map(|e| sim_lesson::Lesson::load(&e.path).ok()).collect();
            let progress = sim_lesson::progress::Progress::load(&value(rest, "--progress").map(PathBuf::from).unwrap_or_else(sim_lesson::progress::path));
            let mastery = sim_lesson::concepts::mastery(&map, &lessons, &progress);
            for m in &mastery {
                println!("{:<18} {:<32} {:>3.0} %  {}/{} solid{}  taught by {}", m.concept, m.title, m.level * 100., m.solid, m.questions, if m.shaky > 0 { format!(", {} shaky", m.shaky) } else { String::new() }, if m.taught_by.is_empty() { "—".into() } else { m.taught_by.join(", ") });
            }
            let done = |l: &sim_lesson::Lesson| l.quizzes().filter(|(_, q)| q.gates()).all(|(_, q)| progress.passed(&l.slug, &q.id));
            if let Some((slug, why)) = sim_lesson::concepts::next_lesson(&lessons, &mastery, done) {
                println!("next: {slug} ({why})");
            }
            Ok(0)
        }
        "report" => {
            let file = pos.first().ok_or(USAGE)?;
            let l = sim_lesson::Lesson::load(Path::new(file.as_str())).map_err(|e| e.to_string())?;
            let files: Vec<PathBuf> = pos.iter().skip(1).map(PathBuf::from).collect();
            if files.is_empty() {
                return Err("give one or more progress.json files".into());
            }
            let progress: Vec<_> = files.iter().map(|f| sim_lesson::progress::Progress::load(f)).collect();
            let r = sim_lesson::report::build(&l, &progress);
            if flag(rest, "--json") {
                println!("{}", serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?);
            } else {
                print!("{}", sim_lesson::report::text(&r));
            }
            Ok(0)
        }
        "draft" => {
            let slug = pos.first().ok_or(USAGE)?.to_string();
            let system = value(rest, "--system").ok_or("--system FILE is required")?;
            let objectives = value(rest, "--objectives").ok_or("--objectives TEXT is required")?;
            let rounds = value(rest, "--rounds").and_then(|r| r.parse().ok()).unwrap_or(3);
            let dir = value(rest, "--dir").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("lessons"));
            let registry = sim_runtime::system_registry();
            let req = sim_runtime::lesson_draft::DraftRequest { slug: slug.clone(), system: system.into(), objectives, lessons: dir, rounds };
            let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
            let agent = sim_agent::Supervisor::open(sim_agent::Config::from_env(cwd, PathBuf::from("runs/lessons/agents/draft").join(&slug).join("state.json")));
            let outcome = sim_runtime::lesson_draft::draft(&req, &registry, |instructions, question, context| {
                let id = agent.ask(sim_agent::Input { discussion: format!("draft/{slug}"), key: format!("draft/{slug}/{}", sim_agent::now()), revision: 0, context: context.clone(), question: question.into(), instructions: Some(instructions.into()), developer: None })?;
                eprintln!("asked Codex (run {id}); waiting…");
                let started = std::time::Instant::now();
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    let state = agent.snapshot();
                    let Some(run) = state.runs.iter().find(|r| r.id == id) else { continue };
                    match run.status {
                        sim_agent::Status::Ready | sim_agent::Status::Completed => {
                            let body = run.reply.as_ref().map(|r| r.body.clone()).ok_or("Codex returned no reply")?;
                            agent.delivered(&id, Ok(()));
                            return Ok(body);
                        }
                        sim_agent::Status::Failed | sim_agent::Status::Cancelled => return Err(run.error.clone().unwrap_or_else(|| "Codex run failed".into())),
                        _ if started.elapsed().as_secs() > 1800 => return Err("Codex took longer than 30 minutes".into()),
                        _ => {}
                    }
                }
            })?;
            println!("{} after {} round(s): {} error(s)", outcome.path.display(), outcome.rounds, outcome.errors);
            for f in &outcome.findings {
                println!("  {f}");
            }
            Ok(if outcome.errors == 0 { 0 } else { 1 })
        }
        "blocks" => {
            let file = pos.first().ok_or(USAGE)?;
            let l = sim_lesson::Lesson::load(Path::new(file.as_str())).map_err(|e| e.to_string())?;
            println!("revision {}", sim_lesson::edit::revision(&l.source));
            for b in &l.blocks {
                let kind = b.kind.name();
                let first: String = b.text(&l.source).lines().next().unwrap_or("").chars().take(60).collect();
                println!("{:<10} {:<9} lines {:>4}-{:<4} {}  {first}", b.id, kind, b.line, b.end_line, b.hash);
            }
            Ok(0)
        }
        "edit" => {
            let (Some(file), Some(json)) = (pos.first(), pos.get(1)) else { return Err(USAGE.into()) };
            let edit: Edit = serde_json::from_str(json).map_err(|e| format!("edit JSON: {e}"))?;
            let store = LessonStore::new(file.as_str());
            let label = value(rest, "--label").unwrap_or_else(|| "CLI edit".into());
            let (_, applied) = store.apply(&label, edit, value(rest, "--expect").as_deref())?;
            println!("{}", serde_json::to_string(&applied).map_err(|e| e.to_string())?);
            Ok(0)
        }
        "figure" => {
            let (Some(input), Some(out)) = (pos.first(), pos.get(1)) else { return Err(USAGE.into()) };
            let width = value(rest, "--width").and_then(|w| w.parse().ok()).unwrap_or(1440);
            let bytes = std::fs::read(input.as_str()).map_err(|e| format!("{input}: {e}"))?;
            std::fs::write(out.as_str(), sim_lesson::figure::svg_png(&bytes, width)?).map_err(|e| e.to_string())?;
            println!("wrote {out}");
            Ok(0)
        }
        "undo" | "redo" => {
            let file = pos.first().ok_or(USAGE)?;
            let store = LessonStore::new(file.as_str());
            let (_, applied) = if command == "undo" { store.undo()? } else { store.redo()? };
            println!("{}", serde_json::to_string(&applied).map_err(|e| e.to_string())?);
            Ok(0)
        }
        _ => Err(USAGE.into()),
    }
}
