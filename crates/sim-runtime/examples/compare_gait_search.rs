//! Durable host for matched optimizer budgets. All physics/preparation/scoring
//! are shared library calls. Run sequentially to retain meaningful host timings.
use serde::{Deserialize, Serialize};
use sim_runtime::{
    contact_exploration,
    exploration::{self, CaptureSession},
    motion_evaluation,
    physics_context::RuntimeIdentity,
    search_comparison::{self, Algorithm, Settings, Trial},
};
use sim_solve::bayesian::{Observation, Outcome, Parameter, Problem};
use std::{
    fs::{self, OpenOptions},
    io::{BufReader, BufWriter, Write},
    path::Path,
    time::Instant,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    recipe: contact_exploration::Recipe,
    profile: exploration::Profile,
    baseline: sim_domain_control::motion_parameters::Values,
    gates: motion_evaluation::Gates,
    optimizer_seeds: Vec<u64>,
    attempts_per_algorithm_seed: usize,
    settings: Settings,
    qualification_directory: String,
}
fn read<T: serde::de::DeserializeOwned>(p: impl AsRef<Path>) -> Result<T> {
    Ok(serde_json::from_reader(BufReader::new(fs::File::open(p)?))?)
}
fn write(p: impl AsRef<Path>, v: &impl Serialize) -> Result<()> {
    let f = OpenOptions::new().create_new(true).write(true).open(p)?;
    let mut out = BufWriter::new(f);
    serde_json::to_writer(&mut out, v)?;
    out.flush()?;
    out.get_ref().sync_all()?;
    Ok(())
}
fn identity(c: &Config) -> Result<String> {
    Ok(blake3::hash(
        sim_runtime::physics_context::fingerprint(&serde_json::to_value(&(
            c,
            RuntimeIdentity::current(),
        ))?)
        .as_bytes(),
    )
    .to_hex()
    .to_string())
}
fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 3 || args.len() > 4 {
        return Err("usage: compare_gait_search config.json output-directory max-new-attempts [cancel-file]".into());
    }
    let config: Config = read(&args[0])?;
    let root = Path::new(&args[1]);
    let limit: usize = args[2].parse()?;
    if limit == 0 || config.optimizer_seeds.is_empty() || config.attempts_per_algorithm_seed < 2 {
        return Err("nonempty seeds, positive budget and at least two attempts required".into());
    }
    let id = identity(&config)?;
    if !root.exists() {
        fs::create_dir(root)?;
        write(root.join("config.json"), &config)?;
        write(
            root.join("identity.json"),
            &serde_json::json!({"context_id":id,"runtime":RuntimeIdentity::current()}),
        )?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("run.lock"))?;
    lock.try_lock()?;
    let stored: serde_json::Value = read(root.join("identity.json"))?;
    if stored["context_id"] != id {
        return Err("comparison config or current library identity changed; use a fresh comparison directory".into());
    }
    let qroot = Path::new(&config.qualification_directory);
    let recipe: exploration::Recipe = read(qroot.join("recipe.json"))?;
    let receipt: exploration::Qualification = read(qroot.join("qualification.json"))?;
    if serde_json::to_value(&recipe.profile)? != serde_json::to_value(&config.profile)? {
        return Err("qualified profile differs from comparison profile".into());
    }
    let q = exploration::qualify(
        &recipe,
        &read(qroot.join("detailed.capture.json"))?,
        &read(qroot.join("reduced.capture.json"))?,
        receipt.report.plan.reference,
        receipt.report.plan.candidate,
    )?;
    if !q.qualified {
        return Err("profile is not qualified against current runtime and saved captures".into());
    }
    let mut trials = Vec::<Trial>::new();
    for entry in fs::read_dir(root)? {
        let p = entry?.path().join("trial.json");
        if p.is_file() {
            trials.push(read(p)?);
        }
    }
    let parameters = &config.recipe.template.space;
    parameters.validate(&config.baseline)?;
    let baseline = parameters
        .parameters
        .iter()
        .map(|p| config.baseline[&p.name])
        .collect::<Vec<_>>();
    let problem = Problem {
        context_id: id,
        parameters: parameters
            .parameters
            .iter()
            .map(|p| Parameter {
                name: p.name.clone(),
                unit: p.kind.unit().into(),
                bounds: p.bounds,
            })
            .collect(),
        objective_name: "negative_eligible_forward_speed".into(),
        objective_unit: "m/s".into(),
        constraints: vec![],
    };
    let mut added = 0;
    for &seed in &config.optimizer_seeds {
        for attempt in 0..config.attempts_per_algorithm_seed {
            for algorithm in [Algorithm::Bayesian, Algorithm::CmaEs] {
                if trials
                    .iter()
                    .any(|t| t.algorithm == algorithm && t.seed == seed && t.attempt == attempt)
                {
                    continue;
                }
                if added >= limit || args.get(3).is_some_and(|p| Path::new(p).exists()) {
                    return Ok(());
                }
                let mut group = trials
                    .iter()
                    .filter(|t| t.algorithm == algorithm && t.seed == seed)
                    .cloned()
                    .collect::<Vec<_>>();
                group.sort_by_key(|t| t.attempt);
                search_comparison::progress(&group)?;
                if group.len() != attempt {
                    return Err("missing earlier trial receipt".into());
                }
                let dir = root.join(format!("{seed}-{algorithm:?}-{attempt:03}"));
                // An interrupted attempt is retained for explicit recovery. Never
                // overwrite evidence or silently restart a potentially live run.
                fs::create_dir(&dir)?;
                let start = Instant::now();
                let mut settings = config.settings.clone();
                settings.seed = seed;
                let history = group
                    .iter()
                    .map(|t| t.observation.clone())
                    .collect::<Vec<_>>();
                let values = search_comparison::suggest(
                    &problem, &baseline, &history, algorithm, &settings,
                )?;
                let proposal_wall_s = start.elapsed().as_secs_f64();
                let named = parameters.named_values(&values)?;
                write(
                    dir.join("proposal.json"),
                    &serde_json::json!({"algorithm":algorithm,"seed":seed,"attempt":attempt,"values":named,"context_id":problem.context_id}),
                )?;
                eprintln!("{seed} {algorithm:?} attempt {attempt}: preparing");
                let prep_start = Instant::now();
                let prepared = config.recipe.prepare(&named);
                let preparation_wall_s = prep_start.elapsed().as_secs_f64();
                let charged_simulation_s = config.recipe.experiment.config.steps as f64
                    * config.recipe.experiment.config.step_s;
                let mut simulation_wall_s = 0.;
                let mut actual_simulation_s = 0.;
                let outcome = match prepared {
                    Err(reason) => {
                        write(
                            dir.join("screen-rejection.json"),
                            &serde_json::json!({"reason":reason}),
                        )?;
                        Outcome::Failed { reason }
                    }
                    Ok(prepared) => {
                        if attempt == 0
                            && sim_runtime::physics_context::fingerprint(&serde_json::to_value(
                                &prepared.spec,
                            )?) != sim_runtime::physics_context::fingerprint(
                                &serde_json::to_value(&recipe.detailed)?,
                            )
                        {
                            return Err(
                                "comparison baseline does not match qualified baseline".into()
                            );
                        }
                        write(dir.join("compiled.json"), &prepared.compiled)?;
                        write(dir.join("screen.json"), &prepared.screen)?;
                        let paired = exploration::Recipe {
                            version: 1,
                            detailed: prepared.spec,
                            profile: config.profile.clone(),
                        }
                        .prepare()?;
                        write(dir.join("detailed.spec.json"), &paired.detailed)?;
                        write(dir.join("reduced.spec.json"), &paired.reduced)?;
                        let sim_start = Instant::now();
                        let captured = (|| -> Result<_> {
                            let mut run = CaptureSession::new(&paired.reduced)?;
                            let mut reported = -1.;
                            while !run.done() {
                                if args.get(3).is_some_and(|p| Path::new(p).exists()) {
                                    break;
                                }
                                run.advance()?;
                                if run.time_s() - reported >= 1. {
                                    reported = run.time_s();
                                    eprintln!(
                                        "{seed} {algorithm:?} {attempt}: {:.2}s simulated",
                                        reported
                                    );
                                }
                            }
                            Ok(run.capture(sim_start.elapsed().as_secs_f64()))
                        })();
                        simulation_wall_s = sim_start.elapsed().as_secs_f64();
                        match captured {
                            Err(e) => Outcome::Failed {
                                reason: format!("runtime preparation: {e}"),
                            },
                            Ok(capture) => {
                                actual_simulation_s = capture.recording.completed_steps as f64
                                    * capture.recording.config.step_s;
                                write(dir.join("capture.json"), &capture)?;
                                match motion_evaluation::evaluate(&capture, &config.gates) {
                                    Err(e) => Outcome::Failed {
                                        reason: format!("capture validation: {e}"),
                                    },
                                    Ok(report) => {
                                        write(dir.join("evaluation.json"), &report)?;
                                        match report.eligible_speed_m_s {
                                            Some(speed) => Outcome::Complete {
                                                objective: -speed,
                                                residuals: vec![],
                                            },
                                            None => Outcome::Failed {
                                                reason: report.rejection_reasons.join("; "),
                                            },
                                        }
                                    }
                                }
                            }
                        }
                    }
                };
                let trial = Trial {
                    algorithm,
                    seed,
                    attempt,
                    proposal_method: search_comparison::proposal_method(
                        &problem, &history, algorithm, &settings,
                    )
                    .into(),
                    observation: Observation {
                        context_id: problem.context_id.clone(),
                        values,
                        outcome,
                        evidence: dir.display().to_string(),
                    },
                    proposal_wall_s,
                    preparation_wall_s,
                    simulation_wall_s,
                    total_wall_s: start.elapsed().as_secs_f64(),
                    charged_simulation_s,
                    actual_simulation_s,
                };
                write(dir.join("trial.json"), &trial)?;
                group.push(trial.clone());
                let progress = search_comparison::progress(&group)?;
                write(dir.join("progress.json"), &progress)?;
                eprintln!("{}", serde_json::to_string(&progress)?);
                trials.push(trial);
                added += 1;
            }
        }
    }
    println!(
        "All declared comparison attempts completed; detailed finalist validation is still separate."
    );
    Ok(())
}
