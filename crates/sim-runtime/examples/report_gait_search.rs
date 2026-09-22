//! Read-only report host; optimizer accounting and motion gates stay in Rust APIs.
use serde_json::{Value, json};
use sim_runtime::{
    fidelity::EnvironmentCapture,
    motion_evaluation,
    search_comparison::{self, Algorithm, Trial},
};
use sim_solve::bayesian::Outcome;
use std::{
    collections::BTreeMap,
    fs,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn read<T: serde::de::DeserializeOwned>(p: impl AsRef<Path>) -> Result<T> {
    Ok(serde_json::from_reader(BufReader::new(fs::File::open(p)?))?)
}
fn write(p: impl AsRef<Path>, text: &str) -> Result<()> {
    let f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(p)?;
    let mut w = BufWriter::new(f);
    w.write_all(text.as_bytes())?;
    w.flush()?;
    w.get_ref().sync_all()?;
    Ok(())
}
fn main() -> Result<()> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 2 {
        return Err("usage: report_gait_search comparison-directory fresh-report-directory".into());
    }
    let root = Path::new(&a[0]);
    let config: Value = read(root.join("config.json"))?;
    let identity: Value = read(root.join("identity.json"))?;
    let expected = config["attempts_per_algorithm_seed"]
        .as_u64()
        .ok_or("missing budget")? as usize;
    let seeds: Vec<u64> = serde_json::from_value(config["optimizer_seeds"].clone())?;
    let gates: motion_evaluation::Gates = serde_json::from_value(config["gates"].clone())?;
    let mut rows = vec![];
    for entry in fs::read_dir(root)? {
        let dir = entry?.path();
        let receipt = dir.join("trial.json");
        if receipt.is_file() {
            rows.push((dir, read::<Trial>(receipt)?));
        }
    }
    let mut groups = vec![];
    let mut paths = Vec::<(String, String, Vec<(f64, f64)>)>::new();
    let mut all_complete = true;
    let mut all_bayesian_active = true;
    let mut initial_training_sets = BTreeMap::<String, Vec<u64>>::new();
    let mut verified_captures = 0;
    let mut md = String::from(
        "# Matched gait search results\n\nAll speeds are signed forward speeds of simulated candidates passing the declared gates. Failed attempts retain their budget and wall-time cost. These configured runs are descriptive evidence, not a general algorithm ranking.\n\n| Seed | Method | Attempts | Failed | Best m/s | Gain m/s | Wall hours | Actual / charged sim s |\n| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |\n",
    );
    for seed in seeds {
        for algorithm in [Algorithm::Bayesian, Algorithm::CmaEs] {
            let mut group = rows
                .iter()
                .filter(|(_, t)| t.seed == seed && t.algorithm == algorithm)
                .collect::<Vec<_>>();
            group.sort_by_key(|(_, t)| t.attempt);
            let trials = group.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>();
            let p = search_comparison::progress(&trials)?;
            if group.len() > expected {
                return Err("comparison exceeded declared trial budget".into());
            }
            all_complete &= group.len() == expected;
            let mut methods = BTreeMap::<String, usize>::new();
            let mut reasons = BTreeMap::<String, usize>::new();
            let mut best: Option<(&PathBuf, &Trial)> = None;
            for (dir, t) in &group {
                if t.observation.context_id
                    != identity["context_id"].as_str().ok_or("missing identity")?
                {
                    return Err("trial context mismatch".into());
                }
                if Path::new(&t.observation.evidence).canonicalize()? != dir.canonicalize()? {
                    return Err("trial evidence directory mismatch".into());
                }
                *methods.entry(t.proposal_method.clone()).or_default() += 1;
                let capture = dir.join("capture.json");
                if capture.is_file() {
                    let capture: EnvironmentCapture = read(capture)?;
                    if serde_json::to_value(&capture.recording.runtime_identity)?
                        != identity["runtime"]
                    {
                        return Err("captured runtime differs from comparison identity".into());
                    }
                    let score = motion_evaluation::evaluate(&capture, &gates)?;
                    let observed = match &t.observation.outcome {
                        Outcome::Complete { objective, .. } => Some(-objective),
                        Outcome::Failed { .. } => None,
                    };
                    if score
                        .eligible_speed_m_s
                        .zip(observed)
                        .is_some_and(|(a, b)| (a - b).abs() > 1e-12)
                        || score.eligible_speed_m_s.is_some() != observed.is_some()
                    {
                        return Err("trial score does not match saved capture".into());
                    }
                    verified_captures += 1;
                } else if matches!(t.observation.outcome, Outcome::Complete { .. }) {
                    return Err("successful trial lacks capture".into());
                }
                match &t.observation.outcome {
                    Outcome::Complete { objective, .. } => {
                        if best.is_none_or(|(_, b)| match b.observation.outcome {
                            Outcome::Complete { objective: b, .. } => *objective < b,
                            _ => true,
                        }) {
                            best = Some((dir, t));
                        }
                    }
                    Outcome::Failed { reason } => {
                        *reasons.entry(reason.clone()).or_default() += 1;
                    }
                }
            }
            if algorithm == Algorithm::Bayesian {
                all_bayesian_active &= methods.get("bayesian_log_ei").copied().unwrap_or(0) > 0;
                if let Some((_, first)) = group
                    .iter()
                    .find(|(_, t)| t.proposal_method == "bayesian_log_ei")
                {
                    let training=group.iter().filter(|(_,t)|t.attempt<first.attempt && matches!(t.observation.outcome,Outcome::Complete{..}))
                        .map(|(_,t)|json!({"values":t.observation.values,"outcome":t.observation.outcome})).collect::<Vec<_>>();
                    let id = sim_runtime::physics_context::fingerprint(&json!(training));
                    initial_training_sets.entry(id).or_default().push(seed);
                }
            }
            let curve = (1..=trials.len())
                .map(|n| search_comparison::progress(&trials[..n]))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            paths.push((
                format!("{algorithm:?} seed {seed}"),
                if algorithm == Algorithm::Bayesian {
                    "#2563eb"
                } else {
                    "#d97706"
                }
                .into(),
                curve
                    .iter()
                    .filter_map(|p| p.best_speed_m_s.map(|v| (p.wall_hours, v)))
                    .collect(),
            ));
            md += &format!(
                "| {seed} | {algorithm:?} | {} | {} | {} | {} | {:.4} | {:.1} / {:.1} |\n",
                p.attempts,
                p.failed_attempts,
                p.best_speed_m_s.map_or("—".into(), |v| format!("{v:.6}")),
                p.improvement_m_s.map_or("—".into(), |v| format!("{v:.6}")),
                p.wall_hours,
                p.actual_simulation_s,
                p.charged_simulation_s
            );
            let finalist=best.map(|(dir,t)|json!({"directory":dir,"attempt":t.attempt,"values":t.observation.values,"detailed_spec":dir.join("detailed.spec.json"),"capture":dir.join("capture.json"),"speed_m_s":match t.observation.outcome{Outcome::Complete{objective,..}=>-objective,_=>unreachable!()}}));
            groups.push(json!({"seed":seed,"algorithm":algorithm,"progress":p,"proposal_methods":methods,"failures":reasons,"finalist":finalist,"incumbent_curve":curve}));
        }
    }
    let duplicate_training_sets = initial_training_sets
        .into_values()
        .filter(|seeds| seeds.len() > 1)
        .collect::<Vec<_>>();
    for seeds in &duplicate_training_sets {
        md += &format!(
            "\n**Seed-overlap warning:** Bayesian seeds {seeds:?} use identical successful observations before their first acquisition. Do not count these as independent training histories. All runs and costs remain in this report.\n"
        );
    }
    let summary = json!({"version":1,"identity":identity,"source_config":root.join("config.json"),"all_budgets_complete":all_complete,"bayesian_acquisition_exercised_for_each_seed":all_bayesian_active,"duplicate_bayesian_initial_training_sets":duplicate_training_sets,"verified_captures":verified_captures,"groups":groups,"scope":"Comparison only. Selected candidates still require detailed long-horizon and physical-sensitivity validation. Actual capture scores were recomputed; no missing or failed run is a success. Inspect duplicate initial training warnings before interpreting optimizer seeds as independent evidence."});
    let xmax = paths
        .iter()
        .flat_map(|(_, _, p)| p.iter().map(|p| p.0))
        .fold(0_f64, f64::max)
        .max(0.001);
    let ymin = paths
        .iter()
        .flat_map(|(_, _, p)| p.iter().map(|p| p.1))
        .fold(f64::INFINITY, f64::min);
    let ymax = paths
        .iter()
        .flat_map(|(_, _, p)| p.iter().map(|p| p.1))
        .fold(f64::NEG_INFINITY, f64::max);
    let lo = if ymin.is_finite() { ymin - 0.005 } else { 0. };
    let hi = if ymax.is_finite() { ymax + 0.005 } else { 1. };
    let mut svg = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"600\" viewBox=\"0 0 1000 600\"><rect width=\"1000\" height=\"600\" fill=\"white\"/><g font-family=\"sans-serif\" fill=\"#18212f\"><text x=\"80\" y=\"35\" font-size=\"23\">Best eligible gait versus cumulative search time</text><text x=\"80\" y=\"59\" font-size=\"13\">Simulation results; failed attempts and preparation time included</text>",
    );
    for i in 0..=5 {
        let x = 80. + i as f64 * 164.;
        let y = 480. - i as f64 * 76.;
        svg += &format!(
            "<path d=\"M{x} 100V480 M80 {y}H900\" stroke=\"#e5e7eb\"/><text x=\"{x}\" y=\"502\" font-size=\"12\">{:.2}</text><text x=\"18\" y=\"{}\" font-size=\"12\">{:.3}</text>",
            xmax * i as f64 / 5.,
            y + 4.,
            lo + (hi - lo) * i as f64 / 5.
        );
    }
    for (i, (label, color, p)) in paths.iter().enumerate() {
        // An incumbent changes only when a trial finishes. A sloped segment
        // would imply improvement before the result was actually observed.
        let mut points = String::new();
        let mut previous = None;
        for (x, y) in p {
            let px = 80. + 820. * x / xmax;
            if let Some(old) = previous {
                points += &format!("{px:.2},{:.2} ", 480. - 380. * (old - lo) / (hi - lo));
            }
            points += &format!("{px:.2},{:.2} ", 480. - 380. * (y - lo) / (hi - lo));
            previous = Some(*y);
        }
        svg += &format!(
            "<polyline points=\"{points}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2.5\" stroke-dasharray=\"{}\"/><text x=\"{}\" y=\"{}\" fill=\"{color}\" font-size=\"13\">{label}</text>",
            if i >= 2 { "6 4" } else { "none" },
            80 + (i % 2) * 400,
            553 + (i / 2) * 21
        );
    }
    svg += "<text x=\"390\" y=\"527\" font-size=\"15\">Cumulative wall time (hours)</text><text x=\"80\" y=\"89\" font-size=\"13\">Signed forward speed (m/s)</text></g></svg>";
    md += &format!(
        "\nBudget complete: **{all_complete}**. Actual Bayesian acquisition exercised for every seed: **{all_bayesian_active}**. Recomputed scores for {verified_captures} saved captures.\n\n![Incumbent speed versus wall time](incumbents.svg)\n\n`summary.json` includes proposal-method counts, complete failure reasons, finalist paths, and per-attempt improvement per wall hour. Finalist robustness is not established by this report.\n"
    );
    fs::create_dir(&a[1])?;
    write(
        Path::new(&a[1]).join("summary.json"),
        &serde_json::to_string_pretty(&summary)?,
    )?;
    write(Path::new(&a[1]).join("README.md"), &md)?;
    write(Path::new(&a[1]).join("incumbents.svg"), &svg)?;
    println!(
        "Report written; full budget complete={all_complete}, Bayesian acquisition exercised={all_bayesian_active}"
    );
    Ok(())
}
