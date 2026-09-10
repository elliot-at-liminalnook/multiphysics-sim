//! Durable native search host. The shared library owns proposals and physics.
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use serde::{Deserialize, Serialize};
    use sim_runtime::{
        experiment::{Experiment, ExperimentSpec, Journal, Status},
        experiment_search::{self, Settings},
    };
    use std::{
        fs::{self, File, OpenOptions},
        io::Write,
        path::{Path, PathBuf},
    };
    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Stored {
        version: u32,
        revision: u64,
        parent: Option<String>,
        checksum: String,
        journal: Journal,
        settings: Settings,
    }
    impl Stored {
        fn checksum(&self) -> Result<String> {
            let value = serde_json::to_value((
                self.version,
                self.revision,
                &self.parent,
                &self.journal,
                &self.settings,
            ))?;
            Ok(blake3::hash(&serde_json::to_vec(&value)?)
                .to_hex()
                .to_string())
        }
    }
    fn lock(root: &Path) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(root.join("writer.lock"))?;
        file.try_lock()
            .map_err(|e| format!("experiment directory already has a writer: {e}"))?;
        Ok(file)
    }
    fn path(root: &Path, revision: u64) -> PathBuf {
        root.join(format!("state-{revision:020}.json"))
    }
    fn save(root: &Path, state: &mut Stored, advance: bool) -> Result<()> {
        state.journal.validate()?;
        if advance {
            state.parent = Some(state.checksum.clone());
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or("journal revision overflow")?;
        }
        state.checksum = state.checksum()?;
        let destination = path(root, state.revision);
        if destination.exists() {
            return Err("refusing to replace an immutable journal revision".into());
        }
        let temporary = root.join(format!(
            ".state-{}-{}.partial",
            state.revision,
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        serde_json::to_writer(&mut file, state)?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temporary, &destination)?;
        File::open(root)?.sync_all()?;
        Ok(())
    }
    fn load(root: &Path) -> Result<Stored> {
        let revision = fs::read_dir(root)?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let digits = name.strip_prefix("state-")?.strip_suffix(".json")?;
                (digits.len() == 20)
                    .then(|| digits.parse::<u64>().ok())
                    .flatten()
            })
            .max()
            .ok_or("no committed journal revision")?;
        let state: Stored = serde_json::from_reader(File::open(path(root, revision))?)?;
        if state.version != 1 || state.revision != revision || state.checksum != state.checksum()? {
            return Err("journal checksum or revision mismatch".into());
        }
        if revision == 0 {
            if state.parent.is_some() {
                return Err("initial journal has a parent".into());
            }
        } else {
            let previous: Stored = serde_json::from_reader(File::open(path(root, revision - 1))?)?;
            if state.parent.as_ref() != Some(&previous.checksum)
                || previous.checksum != previous.checksum()?
            {
                return Err("journal parent revision mismatch".into());
            }
        }
        state.journal.validate()?;
        Ok(state)
    }
    pub fn main() -> Result<()> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.first().map(String::as_str) == Some("init") && args.len() == 4 {
            let spec: ExperimentSpec = serde_json::from_reader(File::open(&args[1])?)?;
            let settings: Settings = serde_json::from_reader(File::open(&args[2])?)?;
            let experiment = Experiment::bind(spec)?;
            let mut state = Stored {
                version: 1,
                revision: 0,
                parent: None,
                checksum: String::new(),
                journal: Journal::new(experiment),
                settings,
            };
            // Validate selector settings before creating the output directory.
            experiment_search::ask(&state.journal, &state.settings)?;
            let root = Path::new(&args[3]);
            fs::create_dir(root)?;
            let _lock = lock(root)?;
            save(root, &mut state, false)?;
            println!(
                "{}",
                serde_json::json!({"initialized":root,"context_id":state.journal.experiment.context_id})
            );
            return Ok(());
        }
        if args.first().map(String::as_str) != Some("advance") || !(4..=5).contains(&args.len()) {
            return Err("usage: search_motion init spec.json settings.json fresh-directory; or advance directory new-action-budget total-trial-budget [cancel-file]".into());
        }
        let root = Path::new(&args[1]);
        let _lock = lock(root)?;
        let mut state = load(root)?;
        let action_budget: usize = args[2].parse()?;
        let trial_budget: usize = args[3].parse()?;
        if action_budget == 0 || trial_budget == 0 {
            return Err("positive advance budgets required".into());
        }
        let cancelled = || args.get(4).is_some_and(|p| Path::new(p).exists());
        let mut actions = 0;
        let mut replayed = 0;
        while actions < action_budget && !cancelled() {
            if state.journal.pending().is_none() {
                if state.journal.trials.len() >= trial_budget {
                    break;
                }
                let proposal = experiment_search::ask(&state.journal, &state.settings)?;
                state.journal.submit(proposal)?;
                save(root, &mut state, true)?; // Proposal is durable before evaluation.
            }
            let trial = state
                .journal
                .pending()
                .ok_or("missing pending proposal")?
                .clone();
            let resuming = trial.checkpoint.is_some();
            let evaluation = match trial.checkpoint {
                Some(checkpoint) => state.journal.experiment.resume(checkpoint),
                None => state.journal.experiment.start(trial.proposal.clone()),
            };
            let mut evaluation = match evaluation {
                Ok(value) => value,
                Err(error) => {
                    if resuming {
                        return Err(error.into());
                    }
                    state
                        .journal
                        .preparation_failed(&trial.proposal.id, error)?;
                    save(root, &mut state, true)?;
                    continue;
                }
            };
            while matches!(evaluation.status(), Status::Running | Status::Replaying) && !cancelled()
            {
                let replaying = evaluation.status() == Status::Replaying;
                if !replaying && actions >= action_budget {
                    break;
                }
                evaluation.advance(1)?;
                if replaying {
                    replayed += 1;
                } else {
                    actions += 1;
                }
                println!(
                    "{}",
                    serde_json::json!({"trial":trial.proposal.id,"status":evaluation.status(),"new_actions":actions,"replayed_actions":replayed})
                );
            }
            state.journal.record(evaluation.checkpoint()?)?;
            save(root, &mut state, true)?;
        }
        let best = state
            .journal
            .best()?
            .map(|(p, s)| serde_json::json!({"trial":p.id,"values":p.values,"score":s}));
        println!(
            "{}",
            serde_json::json!({"revision":state.revision,"trials":state.journal.trials.len(),"pending":state.journal.pending().map(|t|&t.proposal.id),
            "cancelled":cancelled(),"new_actions":actions,"replayed_actions":replayed,"best":best})
        );
        Ok(())
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::main()
}
#[cfg(target_arch = "wasm32")]
fn main() {
    panic!("native search CLI; use the shared evaluation API in browser workers")
}
