use clap::Parser;
use sim_phenomena::ALL;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(about = "Run the emergent-phenomena acceptance suite")]
struct Args {
    /// A scenario name from `surprise-tests.md`, or `all`.
    #[arg(default_value = "all")]
    scenario: String,
    /// Write all reports as JSON.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Render the gallery page with every report's traces embedded.
    #[arg(long)]
    html: Option<PathBuf>,
}

const GALLERY_TEMPLATE: &str = include_str!("../gallery.html");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.scenario == "list" {
        for (name, _) in sim_phenomena::ALL {
            println!("{name}");
        }
        return Ok(());
    }
    let selected = ALL
        .iter()
        .filter(|(name, _)| args.scenario == "all" || *name == args.scenario)
        .collect::<Vec<_>>();
    if selected.is_empty() {
        let names = ALL.iter().map(|(name, _)| *name).collect::<Vec<_>>();
        return Err(format!("unknown scenario `{}`; expected one of {names:?}", args.scenario).into());
    }
    let mut reports = Vec::new();
    let mut all_passed = true;
    for (name, run) in selected {
        let started = std::time::Instant::now();
        let report = run();
        println!("\n{name}  ({:.2}s)", started.elapsed().as_secs_f64());
        for (label, value) in &report.measurements {
            println!("       {label:<40} = {value:.6}");
        }
        for check in &report.checks {
            println!(
                "  {} {:<40} observed={:<14.6} ({})",
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
    if let Some(path) = args.html {
        let data = serde_json::to_string(&reports)?.replace("</script", "<\\/script");
        std::fs::write(path, GALLERY_TEMPLATE.replace("/*__REPORTS__*/null", &data))?;
    }
    if !all_passed {
        return Err("one or more phenomena checks failed".into());
    }
    Ok(())
}
