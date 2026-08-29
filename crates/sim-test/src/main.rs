use clap::Parser;
use sim_test::{ScenarioName, run};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(about = "Run deterministic actuator acceptance scenarios")]
struct Args {
    /// extend, reverse, load, brownout, obstruction, release, coast, or all
    #[arg(default_value = "all")]
    scenario: String,
    /// Write the report (including trace) as JSON.
    #[arg(long)]
    output: Option<PathBuf>,
}

fn parse_scenario(value: &str) -> Option<ScenarioName> {
    ScenarioName::ALL
        .into_iter()
        .find(|scenario| scenario.as_str() == value)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let scenarios = if args.scenario == "all" {
        ScenarioName::ALL.to_vec()
    } else {
        vec![
            parse_scenario(&args.scenario)
                .ok_or_else(|| format!("unknown scenario `{}`", args.scenario))?,
        ]
    };

    let mut reports = Vec::new();
    let mut all_passed = true;
    for scenario in scenarios {
        let report = run(scenario)?;
        println!("\n{}", scenario.as_str());
        for check in &report.checks {
            println!(
                "  {} {:<28} observed={:.6} ({})",
                if check.passed { "PASS" } else { "FAIL" },
                check.name,
                check.observed,
                check.expectation
            );
        }
        all_passed &= report.passed();
        reports.push(report);
    }

    if let Some(path) = args.output {
        std::fs::write(path, serde_json::to_vec_pretty(&reports)?)?;
    }
    if !all_passed {
        return Err("one or more acceptance checks failed".into());
    }
    Ok(())
}
