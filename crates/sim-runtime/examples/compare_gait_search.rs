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
    /// Screened proposals: candidates are schedule-screened and prepared
    /// (in parallel) until one passes; only physics trials use a slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    screening: Option<Screening>,
    /// Optimizer streams (seed × algorithm) run at once. Each stream stays
    /// sequential, so results do not depend on this; only wall time does.
    #[serde(default = "one")]
    parallel_streams: usize,
    /// Stop a trial's simulation once a partial evaluation shows a rejection
    /// that cannot be undone (fall, body contact, orientation, tracking peak);
    /// checked every this many simulated seconds. The outcome is the same
    /// rejection the full run would give.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    early_rejection_check_s: Option<f64>,
    /// Keep per-trial evidence small: specs are recorded by hash (they are
    /// reproducible from the recipe and the trial's values) and the capture
    /// is written only for trials that pass the gates. The compiled gait,
    /// screen, evaluation and trial receipt are always written.
    #[serde(default)]
    minimal_artifacts: bool,
}
fn one() -> usize {
    1
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Screening {
    parallel: usize,
    max_candidates_per_trial: usize,
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
    let mut config: Config = read(&args[0])?;
    // Motor-dependent values come from the accepted actuator registry, not the
    // config file's copies; the synced values enter the study identity.
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let actuators = config.recipe.sync_actuators(&repo)?;
    let root = Path::new(&args[1]);
    let limit: usize = args[2].parse()?;
    if limit == 0 || config.optimizer_seeds.is_empty() || config.attempts_per_algorithm_seed < 2 {
        return Err("nonempty seeds, positive budget and at least two attempts required".into());
    }
    let id = identity(&config)?;
    if !root.exists() {
        fs::create_dir(root)?;
        write(root.join("config.json"), &config)?;
        if !actuators.is_null() {
            write(root.join("actuator-provenance.json"), &actuators)?;
        }
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
    // One stream per (seed, algorithm): its attempts are sequential (each
    // proposal learns from its own history), streams are independent, so
    // they run in parallel without changing any stream's results.
    let added = std::sync::atomic::AtomicUsize::new(0);
    let baseline_trial = std::sync::Mutex::new(trials.iter().find(|t| t.attempt == 0 && !t.proposal_method.contains("shared baseline")).cloned());
    let stop = || args.get(3).is_some_and(|p| Path::new(p).exists());
    let run_attempt = |seed: u64, algorithm: Algorithm, attempt: usize, group: &[Trial]| -> Result<Trial> {
        let dir = root.join(format!("{seed}-{algorithm:?}-{attempt:03}"));
        // An interrupted attempt is retained for explicit recovery. Never
        // overwrite evidence or silently restart a potentially live run.
        fs::create_dir(&dir)?;
        let start = Instant::now();
        let mut settings = config.settings.clone();
        settings.seed = seed;
        let history = group
            .iter()
            .flat_map(|t| t.history().cloned())
            .collect::<Vec<_>>();
        let (values, prepared, preparation_wall_s, screened, proposal_wall_s) = match &config.screening {
            Some(sc) if attempt > 0 => {
                let named_of = |v: &[f64]| parameters.named_values(v);
                let (screened, accepted) = search_comparison::propose_until_prepared(
                    &problem, &baseline, &history, algorithm, &settings,
                    sc.parallel, sc.max_candidates_per_trial,
                    |v| config.recipe.schedule_screen(&named_of(v)?),
                    |v| config.recipe.prepare(&named_of(v)?),
                    |i| dir.join(format!("screened-{i:03}")).display().to_string(),
                )?;
                eprintln!("{seed} {algorithm:?} attempt {attempt}: {} candidates screened", screened.len());
                match accepted {
                    Some((values, prepared, wall)) => {
                        let proposal_wall_s = start.elapsed().as_secs_f64() - wall;
                        (values, Ok(prepared), wall, screened, proposal_wall_s.max(0.))
                    }
                    None => {
                        let values = screened.last().map(|s| s.observation.values.clone()).ok_or("no candidate proposed")?;
                        let reason = format!("no candidate passed preparation within {} screened proposals", sc.max_candidates_per_trial);
                        let mut screened = screened;
                        screened.pop();
                        (values, Err(reason), 0., screened, start.elapsed().as_secs_f64())
                    }
                }
            }
            _ => {
                let values = search_comparison::suggest(&problem, &baseline, &history, algorithm, &settings)?;
                let proposal_wall_s = start.elapsed().as_secs_f64();
                let named = parameters.named_values(&values)?;
                eprintln!("{seed} {algorithm:?} attempt {attempt}: preparing");
                let prep_start = Instant::now();
                let prepared = config.recipe.prepare(&named);
                (values, prepared, prep_start.elapsed().as_secs_f64(), vec![], proposal_wall_s)
            }
        };
        // The accepted candidate was proposed after the screened ones.
        let proposed_after = history.iter().cloned().chain(screened.iter().map(|s| s.observation.clone())).collect::<Vec<_>>();
        let method = search_comparison::proposal_method(&problem, &proposed_after, algorithm, &settings);
        let named = parameters.named_values(&values)?;
        write(
            dir.join("proposal.json"),
            &serde_json::json!({"algorithm":algorithm,"seed":seed,"attempt":attempt,"values":named,"context_id":problem.context_id,"screened_candidates":screened.len()}),
        )?;
        if !screened.is_empty() {
            write(dir.join("screened.json"), &screened)?;
        }
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
                if config.minimal_artifacts {
                    let fingerprint = |v: &sim_runtime::experiment::ExperimentSpec| -> Result<String> {
                        Ok(sim_runtime::physics_context::fingerprint(&serde_json::to_value(v)?))
                    };
                    // The playback governor is what gait playback needs from the spec.
                    write(dir.join("spec-identity.json"), &serde_json::json!({
                        "detailed": fingerprint(&paired.detailed)?, "reduced": fingerprint(&paired.reduced)?,
                        "reference_governor": paired.detailed.scene.controller.as_ref().map(|c| c.parameters["reference_governor"].clone()),
                        "note": "Specs are reproducible: prepare_gait_candidate with this trial's values, then reduced_exploration's recipe preparation."}))?;
                } else {
                    write(dir.join("detailed.spec.json"), &paired.detailed)?;
                    write(dir.join("reduced.spec.json"), &paired.reduced)?;
                }
                let sim_start = Instant::now();
                let captured = (|| -> Result<_> {
                    let mut run = CaptureSession::new(&paired.reduced)?;
                    let mut reported = -1.;
                    let mut checked = 0.;
                    while !run.done() {
                        if args.get(3).is_some_and(|p| Path::new(p).exists()) {
                            break;
                        }
                        run.advance()?;
                        if let Some(every) = config.early_rejection_check_s {
                            if run.time_s() - checked >= every {
                                checked = run.time_s();
                                let partial = run.capture(sim_start.elapsed().as_secs_f64());
                                if let Some(reason) = motion_evaluation::evaluate(&partial, &config.gates).ok().as_ref().and_then(motion_evaluation::decided_rejection) {
                                    eprintln!("{seed} {algorithm:?} {attempt}: stopped at {:.2}s simulated: {reason}", run.time_s());
                                    break;
                                }
                            }
                        }
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
                        let evaluated = motion_evaluation::evaluate(&capture, &config.gates);
                        if !config.minimal_artifacts || evaluated.as_ref().is_ok_and(|r| r.eligible_speed_m_s.is_some()) {
                            write(dir.join("capture.json"), &capture)?;
                        }
                        match evaluated {
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
            proposal_method: method.into(),
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
            screened,
        };
        write(dir.join("trial.json"), &trial)?;
        Ok(trial)
    };
    let run_stream = |seed: u64, algorithm: Algorithm| -> std::result::Result<(), String> {
        let mut group = trials.iter().filter(|t| t.algorithm == algorithm && t.seed == seed).cloned().collect::<Vec<_>>();
        group.sort_by_key(|t| t.attempt);
        for attempt in 0..config.attempts_per_algorithm_seed {
            if group.iter().any(|t| t.attempt == attempt) {
                continue;
            }
            let own_baseline = attempt == 0 && baseline_trial.lock().unwrap().as_ref().is_some_and(|b| b.seed == seed && b.algorithm == algorithm);
            if stop() || (!own_baseline && added.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= limit) {
                return Ok(());
            }
            search_comparison::progress(&group)?;
            if group.len() != attempt {
                return Err("missing earlier trial receipt".into());
            }
            // Attempt 0 is the qualified baseline in every stream: simulate it
            // once and share that trial's outcome and evidence.
            let shared_baseline = if attempt == 0 { baseline_trial.lock().unwrap().clone() } else { None };
            let trial = match shared_baseline {
                // The stream that simulated it keeps its own trial.
                Some(base) if base.seed == seed && base.algorithm == algorithm => base,
                Some(base) => {
                    let dir = root.join(format!("{seed}-{algorithm:?}-{attempt:03}"));
                    fs::create_dir(&dir).map_err(|e| e.to_string())?;
                    let trial = Trial { algorithm, seed, attempt, proposal_method: format!("{} (shared baseline)", base.proposal_method), proposal_wall_s: 0., preparation_wall_s: 0., simulation_wall_s: 0., total_wall_s: 0., ..base };
                    write(dir.join("trial.json"), &trial).map_err(|e| e.to_string())?;
                    trial
                }
                None => {
                    let trial = run_attempt(seed, algorithm, attempt, &group).map_err(|e| e.to_string())?;
                    if attempt == 0 {
                        *baseline_trial.lock().unwrap() = Some(trial.clone());
                    }
                    trial
                }
            };
            group.push(trial);
            let progress = search_comparison::progress(&group)?;
            let dir = root.join(format!("{seed}-{algorithm:?}-{attempt:03}"));
            write(dir.join("progress.json"), &progress).map_err(|e| e.to_string())?;
            eprintln!("{}", serde_json::to_string(&progress).map_err(|e| e.to_string())?);
        }
        Ok(())
    };
    let streams: Vec<(u64, Algorithm)> = config.optimizer_seeds.iter().flat_map(|s| [(*s, Algorithm::Bayesian), (*s, Algorithm::CmaEs)]).collect();
    let parallel = config.parallel_streams.max(1);
    // Simulate the shared baseline (attempt 0) before the streams fan out.
    if let Some((seed, algorithm)) = streams.first() {
        if baseline_trial.lock().unwrap().is_none() {
            let mut group = trials.iter().filter(|t| t.algorithm == *algorithm && t.seed == *seed).cloned().collect::<Vec<_>>();
            group.sort_by_key(|t| t.attempt);
            if group.is_empty() && !stop() && added.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < limit {
                let trial = run_attempt(*seed, *algorithm, 0, &group)?;
                *baseline_trial.lock().unwrap() = Some(trial);
            }
        }
    }
    for chunk in streams.chunks(parallel) {
        let results: Vec<std::result::Result<(), String>> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk.iter().map(|(seed, algorithm)| scope.spawn(|| run_stream(*seed, *algorithm))).collect();
            handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("stream panicked".into()))).collect()
        });
        for r in results {
            r?;
        }
        if stop() {
            return Ok(());
        }
    }
    println!(
        "All declared comparison attempts completed; detailed finalist validation is still separate."
    );
    Ok(())
}
