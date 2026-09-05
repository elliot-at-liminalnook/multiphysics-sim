//! Headless runs of a robot exported by the CAD tool.
//!
//!     sim-cad <model.simrobot.json> [seconds]
//!     sim-cad run <model> [--seconds s] [--planar] [--no-flex] [--no-contact] [--modes M] [--step h] [--montecarlo N] [--seed S] [--out results.json] [--verbose] [--report dt]
//!     sim-cad fit <model> <log.csv> [--out fitted.json] [--iterations N]
//!     sim-cad run <model> --controller script.py [--python python3] [--controller-arg=<value>]...
//!
//! `run` writes `<model>.simresult.json` beside the model (or `--out`);
//! `fit` writes an `identification` block for the CAD tool to apply.
use sim_phenomena::scenarios::cad_physical::{fit, read_log, results_path, run_monte_carlo, run_physical_with_controller, summary, BuildOptions};
use sim_phenomena::scenarios::cad_robot::{file_version, run_file};
use sim_phenomena::scenarios::cad_robot::PhysicalModel;

fn usage() -> ! {
    eprintln!("usage: sim-cad <model.simrobot.json> [seconds]\n       sim-cad run <model> [--seconds s] [--planar] [--no-flex] [--no-contact] [--montecarlo N] [--seed S] [--out results.json]\n       sim-cad run <model> --controller script.py [--python python3] [--controller-arg=<value>]...\n       sim-cad fit <model> <log.csv> [--out fitted.json] [--iterations N]");
    std::process::exit(2);
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn controller(args: &[String]) -> Result<Option<std::process::Command>, String> {
    if args.iter().any(|a| a == "--controller") && flag(args, "--controller").is_none_or(|s| s.starts_with("--")) {
        return Err("--controller requires a Python script path".to_owned());
    }
    let Some(script) = flag(args, "--controller") else { return Ok(None) };
    let python = flag(args, "--python").unwrap_or_else(|| "python3".to_owned());
    let mut command = std::process::Command::new(python);
    command.arg("-u").arg(script);
    // Repeat --controller-arg=<value>; no shell interpretation.
    command.args(args.iter().filter_map(|a| a.strip_prefix("--controller-arg=")));
    Ok(Some(command))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(first) = args.first() else { usage() };
    let result = match first.as_str() {
        "run" => {
            let Some(path) = args.get(1) else { usage() };
            let seconds: f64 = flag(&args, "--seconds").and_then(|s| s.parse().ok()).unwrap_or(2.0);
            let opts = BuildOptions { planar: args.iter().any(|a| a == "--planar"), flex: !args.iter().any(|a| a == "--no-flex"), contact: !args.iter().any(|a| a == "--no-contact"), verbose: args.iter().any(|a| a == "--verbose"), report: flag(&args, "--report").and_then(|s| s.parse().ok()).unwrap_or(0.1), step: flag(&args, "--step").and_then(|s| s.parse().ok()).unwrap_or(5.0e-4), flex_modes: flag(&args, "--modes").and_then(|s| s.parse().ok()).unwrap_or(4), ..BuildOptions::default() };
            let out = flag(&args, "--out");
            let external = args.iter().any(|a| a == "--controller");
            if external && args.iter().any(|a| a == "--montecarlo") {
                Err("--controller cannot be combined with --montecarlo; run each controller experiment separately".to_owned())
            } else if external && file_version(path).unwrap_or(0) < 3 {
                Err("--controller requires a readable v3 physical model".to_owned())
            } else if file_version(path).unwrap_or(2) < 3 {
                run_file(path, seconds)
            } else {
                let mut report = controller(&args).and_then(|c| run_physical_with_controller(path, seconds, &opts, out.as_deref(), c));
                if let Some(n) = flag(&args, "--montecarlo").and_then(|s| s.parse::<usize>().ok()) {
                    let seed: u64 = flag(&args, "--seed").and_then(|s| s.parse().ok()).unwrap_or(1);
                    match PhysicalModel::load(path).and_then(|m| run_monte_carlo(&m, n, seed, seconds, &opts)) {
                        Ok(mc) => {
                            let out_path = out.clone().unwrap_or_else(|| results_path(path));
                            if let Ok(text) = std::fs::read_to_string(&out_path) {
                                if let Ok(mut doc) = serde_json::from_str::<serde_json::Value>(&text) {
                                    doc["monte_carlo"] = mc.clone();
                                    let _ = std::fs::write(&out_path, serde_json::to_string_pretty(&doc).unwrap());
                                    report = report.map(|r| format!("{r}\n{}", summary(&serde_json::json!({"monte_carlo": mc, "contacts": {}, "base": {}}))));
                                }
                            }
                        }
                        Err(e) => report = report.map(|r| format!("{r}\nmonte carlo failed: {e}")),
                    }
                }
                report
            }
        }
        "fit" => {
            let (Some(path), Some(log_path)) = (args.get(1), args.get(2)) else { usage() };
            let iterations: usize = flag(&args, "--iterations").and_then(|s| s.parse().ok()).unwrap_or(40);
            let out = flag(&args, "--out").unwrap_or_else(|| path.replace(".simrobot.json", ".identification.json"));
            PhysicalModel::load(path).and_then(|m| read_log(log_path).and_then(|log| fit(&m, &log, log_path, &BuildOptions::default(), iterations))).and_then(|block| {
                std::fs::write(&out, serde_json::to_string_pretty(&block).unwrap()).map_err(|e| format!("{out}: {e}"))?;
                Ok(format!("{}\nidentification written to {out}", serde_json::to_string_pretty(&block).unwrap()))
            })
        }
        _ => {
            let seconds: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2.0);
            run_file(first, seconds)
        }
    };
    match result {
        Ok(report) => println!("{report}"),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
