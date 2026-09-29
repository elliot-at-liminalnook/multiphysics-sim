//! Generate and inspect lesson narration. Spends money only in `generate`,
//! and never more than `--budget` (default $1) per run.
use sim_lesson::narration::Explainer;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: sim-narrate <command> LESSON …
  LESSON is a lesson folder, its lesson.md or its explainer.md.
  status LESSON                    sections, audio state, timing kind and estimated cost
  check LESSON                     parse the explainer and check its cues against the lesson (no network)
  generate LESSON [--section ID]… [--force] [--budget USD] [--dry-run]
                                   make audio for stale/missing sections (or the named ones);
                                   --force with no --section regenerates every section
Key: OPENROUTER_API_KEY or ~/OPENROUTER_API_KEY. Audio: <lesson>/narration/.";

fn explainer_path(arg: &str) -> PathBuf {
    let p = Path::new(arg);
    if p.is_dir() {
        p.join("explainer.md")
    } else if p.file_name().is_some_and(|n| n == "lesson.md") {
        p.with_file_name("explainer.md")
    } else {
        p.to_path_buf()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
}

fn run(args: &[String]) -> Result<i32, String> {
    let (Some(command), Some(target)) = (args.first(), args.get(1)) else { return Err(USAGE.into()) };
    let path = explainer_path(target);
    let explainer = Explainer::load(&path).map_err(|e| e.to_string())?;
    let rest = &args[2..];
    match command.as_str() {
        "status" => {
            let manifest = explainer.manifest();
            let mut total = 0.0;
            for (item, s) in sim_voice::plan(&explainer).iter().zip(&explainer.sections) {
                let timing = explainer.timing(&manifest, s);
                total += if item.status == sim_voice::Status::Current { 0.0 } else { item.estimated_usd };
                println!("{:<18} {:<8} {:>4} words  {:>6.1} s  {:<9}  ~${:.4}  {}", item.id, format!("{:?}", item.status).to_lowercase(), item.words, timing.duration_s, format!("{:?}", timing.kind).to_lowercase(), item.estimated_usd, s.title);
            }
            println!("to generate stale/missing: ~${total:.3} (estimate; Gemini 3.8 Flash TTS + whisper-1 alignment)");
            Ok(0)
        }
        "check" => {
            let lesson = sim_lesson::Lesson::load(&path.with_file_name("lesson.md")).map_err(|e| e.to_string())?;
            let problems = sim_lesson::narration::check(&explainer, &lesson);
            for p in &problems {
                println!("{p}");
            }
            eprintln!("{} section(s), {} cue(s), {} problem(s)", explainer.sections.len(), explainer.sections.iter().map(|s| s.cues.len()).sum::<usize>(), problems.len());
            Ok(if problems.is_empty() { 0 } else { 1 })
        }
        "generate" => {
            let mut sections = Vec::new();
            let mut budget = sim_voice::DEFAULT_BUDGET_USD;
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--section" => {
                        sections.push(rest.get(i + 1).ok_or("--section needs an ID")?.clone());
                        i += 1;
                    }
                    "--budget" => {
                        budget = rest.get(i + 1).and_then(|v| v.parse().ok()).ok_or("--budget needs dollars")?;
                        i += 1;
                    }
                    "--force" | "--dry-run" => {}
                    other => return Err(format!("unknown option {other}\n{USAGE}")),
                }
                i += 1;
            }
            let force = rest.iter().any(|a| a == "--force");
            if rest.iter().any(|a| a == "--dry-run") {
                for item in sim_voice::plan(&explainer).iter().filter(|p| if sections.is_empty() { force || p.status != sim_voice::Status::Current } else { sections.contains(&p.id) }) {
                    println!("would generate {} ({} words, ~{:.1} s, ~${:.4})", item.id, item.words, item.estimated_seconds, item.estimated_usd);
                }
                return Ok(0);
            }
            let voice = sim_voice::Voice::new(&sim_voice::api_key()?)?;
            let report = sim_voice::generate(&explainer, &voice, &sim_voice::Request { sections: &sections, force, budget_usd: budget }, &|m| eprintln!("{m}"))?;
            for w in &report.warnings {
                eprintln!("warning: {w}");
            }
            println!("generated {} section(s), {:.1} s of audio; estimated ${:.4}{}", report.generated.len(), report.audio_seconds, report.estimated_usd, report.measured_usd.map(|m| format!(", measured ${m:.4}")).unwrap_or_default());
            Ok(0)
        }
        _ => Err(USAGE.into()),
    }
}
