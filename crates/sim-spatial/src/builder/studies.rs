//! Studies and saved runs: run and poll studies, compare alternatives,
//! overlay saved runs and replay them headlessly.
use super::*;

impl ReplayOutcome {
    /// One-line result for the Studies tab.
    pub fn headline(&self) -> String {
        match (self.status, self.max_rel_diff, &self.error) {
            ("running", ..) => format!("Replaying… {:.1} s", self.wall_seconds),
            ("cancelled", ..) => "Cancelled · replay stopped, no result".into(),
            (_, Some(d), _) if d == 0. => format!("Reproduced exactly · max rel diff 0 · {} samples · {}", self.samples.unwrap_or(0), self.fidelity),
            (_, Some(d), _) => format!("Differs · max rel diff {d:.2e} · {} samples · {}", self.samples.unwrap_or(0), self.fidelity),
            (_, _, Some(e)) => format!("Replay failed: {e}"),
            _ => String::new(),
        }
    }
}

impl Builder {
    /// Overlay saved runs in the graph dock and the Studies tab.
    pub fn compare_runs(&mut self, ids: &[String]) -> Result<(), String> {
        let records: Vec<_> = self.runs.iter().filter(|(_, s)| ids.contains(&s.id)).map(|(p, _)| sim_runtime::run_history::load(p)).collect::<Result<_, _>>()?;
        if records.len() < 2 {
            return Err("pick at least two runs to compare".into());
        }
        let metrics: Vec<sim_system::Metric> = records[0].series.iter().take(4).map(|s| sim_system::Metric { label: format!("{} (final)", s.label), observable: s.label.clone(), reduce: sim_system::Reduce::Final, window: None }).collect();
        self.study.result = Some(sim_runtime::run_history::compare(&records, &metrics));
        self.graphs.visible = true;
        self.graphs.force_refresh();
        self.tab = Tab::Studies;
        self.panel_dirty = true;
        Ok(())
    }

    /// Rerun saved run `id` headlessly on a worker thread and compare it with
    /// its record; the outcome arrives in `poll_replay`. Replaces (and stops)
    /// a replay already running.
    pub fn replay_run(&mut self, id: &str) -> Result<(), String> {
        let (path, summary) = self.runs.iter().find(|(_, s)| s.id == id).cloned().ok_or_else(|| format!("no saved run `{id}`"))?;
        let record = sim_runtime::run_history::load(&path)?;
        self.cancel_replay();
        let registry = self.registry.clone();
        let (edited, fidelity) = (record.edited_while_running(), record.fidelity_label());
        let work = crate::jobs::Job::spawn(crate::jobs::Pool::Dedicated, 0, "replay thread", move |ctx| sim_runtime::run_history::replay_with_cancel(&record, &registry, Some(ctx.cancel_flag())));
        self.replay.outcomes.insert(
            id.to_string(),
            ReplayOutcome { id: id.to_string(), status: "running", max_rel_diff: None, samples: None, error: None, wall_seconds: 0., duration: summary.duration, seed: summary.seed, fidelity, edited_while_running: edited },
        );
        self.replay.job = Some(ReplayJob { id: id.to_string(), work, started: std::time::Instant::now() });
        self.tab = Tab::Studies;
        self.status = format!("Replaying run {id} headlessly from t = 0 on the shared runtime (background).");
        self.panel_dirty = true;
        Ok(())
    }

    /// Stop the running replay between simulation steps; it reports no result.
    pub fn cancel_replay(&mut self) -> bool {
        // Dropping the job at the end of this function stops the rerun.
        let Some(job) = self.replay.job.take() else { return false };
        if let Some(o) = self.replay.outcomes.get_mut(&job.id) {
            o.status = "cancelled";
            o.wall_seconds = job.started.elapsed().as_secs_f64();
        }
        self.status = format!("Replay of {} cancelled.", job.id);
        self.panel_dirty = true;
        true
    }

    pub(super) fn poll_replay(&mut self) {
        let Some(job) = &self.replay.job else { return };
        let polled = job.work.poll();
        let wall = job.started.elapsed().as_secs_f64();
        let id = job.id.clone();
        let Some(outcome) = self.replay.outcomes.get_mut(&id) else { return };
        outcome.wall_seconds = wall;
        let Some(result) = polled else { return };
        match result {
            Ok(report) => {
                outcome.status = "done";
                outcome.max_rel_diff = Some(report.max_rel_diff);
                outcome.samples = Some(report.samples);
            }
            Err(e) => {
                outcome.status = "error";
                outcome.error = Some(e);
            }
        }
        self.status = format!("Run {id}: {}", outcome.headline());
        self.replay.job = None;
        self.panel_dirty = true;
    }

    pub fn replay_json(&self) -> serde_json::Value {
        serde_json::json!({
            "running": self.replay.job.as_ref().map(|j| &j.id),
            "method": "headless rerun from t = 0 with the record's document, seed and config on the shared runtime; compared at every recorded sample time",
            "outcomes": self.replay.outcomes.values().map(|o| {
                let mut v = serde_json::json!(o);
                v["headline"] = serde_json::json!(o.headline());
                v
            }).collect::<Vec<_>>(),
        })
    }

    /// Save a study (shared command, undoable) and start running it.
    pub fn save_and_run_study(&mut self, name: &str, study: sim_system::Study) -> Result<(), String> {
        self.apply(&format!("Study {name}"), vec![SystemCommand::SetStudy { name: name.to_string(), study: Some(study) }])?;
        self.run_study(name)
    }

    /// Run a saved study on worker threads; results arrive in `poll_study`.
    pub fn run_study(&mut self, name: &str) -> Result<(), String> {
        let study = self.document.studies.get(name).cloned().ok_or_else(|| format!("no study `{name}`"))?;
        // Replacing the job below cancels a study already running.
        let (document, registry, n) = (self.document.clone(), self.registry.clone(), name.to_string());
        let total = sim_runtime::system_study::variant_count(&study);
        let work = crate::jobs::Job::spawn(crate::jobs::Pool::Dedicated, 0, format!("study {name}"), move |ctx| {
            let threads = std::thread::available_parallelism().map(|n| n.get().saturating_sub(1).max(1)).unwrap_or(2);
            sim_runtime::system_study::run(&document, &registry, &n, &study, threads, Some(ctx.cancel_flag()), &|done, total| ctx.steps(done as u64, total as u64))
        });
        self.study.job = Some(StudyJob { name: name.to_string(), work, total });
        self.study.error = None;
        self.tab = Tab::Studies;
        self.graphs.visible = true;
        self.status = format!("Running study {name}: {total} variants on the shared runtime (background).");
        self.panel_dirty = true;
        Ok(())
    }

    pub fn study_json(&self) -> serde_json::Value {
        serde_json::json!({
            "running": self.study_progress().map(|(n, d, t)| serde_json::json!({"name": n, "done": d, "total": t})),
            "error": self.study.error,
            "result": self.study.result.as_ref().map(|r| serde_json::json!({
                "name": r.name, "kind": r.kind, "parameter": r.parameter, "source_hash": r.source_hash,
                "table": sim_runtime::system_study::table(r),
                "variants": r.variants.iter().map(|v| serde_json::json!({"label": v.label, "value": v.value, "metrics": v.metrics, "derived": v.derived, "error": v.error, "wall_seconds": v.wall_seconds})).collect::<Vec<_>>(),
            })),
        })
    }

    pub(crate) fn study_progress(&self) -> Option<(String, usize, usize)> {
        self.study.job.as_ref().map(|j| (j.name.clone(), j.work.progress().steps.map_or(0, |(done, _)| done as usize), j.total))
    }

    pub(super) fn poll_study(&mut self) {
        let Some(job) = &self.study.job else { return };
        match job.work.poll() {
            None => {}
            Some(result) => {
                self.study.job = None;
                match result {
                    Ok(r) => {
                        self.status = format!("Study {} finished: {} variants", r.name, r.variants.len());
                        self.study.result = Some(r);
                    }
                    Err(e) => {
                        self.status = format!("Study failed: {e}");
                        self.study.error = Some(e);
                    }
                }
                self.panel_dirty = true;
                self.graphs.force_refresh();
            }
        }
    }

    /// A comparison of `name` against the alternatives that fit its ports
    /// (same interface first, at most three), with default metrics.
    pub fn compare_alternatives(&mut self, scene: &SpatialScene, name: &str) -> Result<(), String> {
        let list = library::alternatives(&self.document, &self.registry, Some(&self.library_dir), &self.level, name).map_err(|e| e.to_string())?;
        let chosen: Vec<library::Alternative> = list.into_iter().filter(|a| a.same_interface).take(3).collect();
        if chosen.is_empty() {
            return Err(format!("nothing with the same interface fits {name}; use Show alternatives and Swap instead"));
        }
        let mut imports = Vec::new();
        for a in &chosen {
            if let Some(path) = &a.library_path {
                imports.push(SystemCommand::AddDefinitions { definitions: library::import(std::path::Path::new(path)).map_err(|e| e.to_string())? });
            }
        }
        if !imports.is_empty() {
            self.apply("Import alternatives", imports)?;
        }
        let observe = self.default_observe(scene, name);
        let study = self.default_study(observe, name, sim_system::StudyKind::Compare { alternatives: chosen.into_iter().map(|a| a.kind).collect() });
        self.save_and_run_study(&format!("compare_{name}"), study)
    }

    /// Default observables and metrics: the run's readouts plus what the
    /// selected part itself exposes, meaned over the last quarter.
    pub(super) fn default_observe(&self, scene: &SpatialScene, name: &str) -> Vec<String> {
        let mut observe: Vec<String> = scene.animation.as_ref().map(|a| a.readouts.iter().take(2).map(|r| system_builder::observable_key(&scene.description, &r.observable)).collect()).unwrap_or_default();
        for (id, _) in graphs::candidates(scene, &self.full_path(name)).into_iter().take(3) {
            observe.push(system_builder::observable_key(&scene.description, &id));
        }
        for pinned in &self.graphs.pinned {
            observe.push(system_builder::observable_key(&scene.description, pinned));
        }
        observe.sort();
        observe.dedup();
        observe
    }

    pub(super) fn default_study(&self, observe: Vec<String>, name: &str, kind: sim_system::StudyKind) -> sim_system::Study {
        let duration = self.document.studies.values().map(|s| s.duration).fold(2.0, f64::max);
        let metrics = observe.iter().map(|o| sim_system::Metric { label: format!("{o} (mean, last 25 %)"), observable: o.clone(), reduce: sim_system::Reduce::Mean, window: Some([0.75 * duration, duration]) }).collect();
        sim_system::Study { at: self.level.clone(), instance: name.to_string(), kind, duration, observe, metrics }
    }
}
