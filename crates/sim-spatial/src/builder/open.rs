//! Open another system file in the running build-mode window.
//!
//! `Builder::open_system` is the one entry point: the Systems tab (path
//! field and discovered list), `system_ui` activations of those controls and
//! REST `system_open` all call it. It checks what would be lost, then loads,
//! validates and compiles the file on a worker thread with the same
//! `Builder::open` and compile the launch uses. `finish_open` is the only
//! place a loaded system replaces this one; a failed load leaves the current
//! system untouched. Opening reads files only: it creates no run directory
//! and no annotations file.
use super::*;

/// Launch facts the open path needs (passed in by build mode, not globals).
/// Lesson mode has none, so it cannot switch systems.
#[derive(Clone, Debug)]
pub struct Shell {
    /// The file the window was launched with.
    pub launch: PathBuf,
    /// `--annotations FILE`: it belongs to the launch file only.
    pub annotations: Option<PathBuf>,
    /// `--schematic`: a sim-viewer child showing this file (not retargeted).
    pub schematic: Option<PathBuf>,
    /// Shared display-model catalog directory (the system's `models/` joins it).
    pub models: PathBuf,
}

impl Shell {
    /// The annotations sidecar for `path`: the explicit file for the launch
    /// system, `<path>.annotations.json` for any other.
    pub fn annotations_for(&self, path: &std::path::Path) -> PathBuf {
        match &self.annotations {
            Some(explicit) if same_file(path, &self.launch) => explicit.clone(),
            _ => PathBuf::from(format!("{}.annotations.json", path.display())),
        }
    }
}

pub(crate) fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// A load in progress (its generation is the open's `seq`).
pub(super) struct OpenJob {
    path: PathBuf,
    work: crate::jobs::Job<Box<Opened>>,
    started: std::time::Instant,
}

/// Everything the new system needs, built off the UI thread.
pub(super) struct Opened {
    builder: Builder,
    compiled: CompileResult,
    models: crate::models::ModelLibrary,
    systems: Vec<PathBuf>,
}

/// State kept across the switch: the pending load, the last outcome (for
/// REST callers polling their job) and the discovered system files.
#[derive(Default)]
pub(super) struct OpenState {
    pub shell: Option<Shell>,
    pub job: Option<OpenJob>,
    seq: u64,
    /// (sequence, outcome) of the latest finished open.
    last: Option<(u64, Result<serde_json::Value, String>)>,
    /// `*.system.json` files offered in the Systems tab.
    pub systems: Vec<PathBuf>,
}

impl OpenState {
    pub fn pending(&self) -> Option<&std::path::Path> {
        self.job.as_ref().map(|j| j.path.as_path())
    }
}

/// `*.system.json` under the workspace's examples/systems-builder, the library and the current file's folder.
pub(super) fn discover(current: &std::path::Path, library_dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files = crate::workspace::path("examples/systems-builder").map(|d| library::system_files(&d)).unwrap_or_default();
    files.extend(library::system_files(library_dir));
    if let Some(dir) = current.parent() {
        files.extend(library::system_files(if dir.as_os_str().is_empty() { std::path::Path::new(".") } else { dir }));
    }
    let mut files: Vec<PathBuf> = files.into_iter().map(|p| std::path::absolute(&p).unwrap_or(p)).collect();
    files.sort();
    files.dedup();
    files
}

impl Builder {
    /// Build mode: allow `open_system` with these launch facts.
    pub fn enable_open(&mut self, shell: Shell) {
        self.open.systems = discover(&self.store.path, &self.library_dir);
        self.open.shell = Some(shell);
    }

    /// Opening another system is enabled (build mode's builder; a lesson's
    /// sandbox builder has no shell).
    pub(crate) fn can_open(&self) -> bool {
        self.open.shell.is_some()
    }

    /// What leaving Build/Lessons for another mode (or replacing this
    /// builder) would lose: the `system_open` blockers below, and an open
    /// still loading. The mode switch (`app::switch`) refuses on these.
    pub(crate) fn switch_blockers(&self) -> Vec<String> {
        let mut blockers = self.open_blockers();
        if let Some(pending) = self.open.pending() {
            blockers.push(format!("{} is still opening: wait or cancel it", pending.display()));
        }
        blockers
    }

    /// What replacing this builder with a new lesson's sandbox builder would
    /// lose: the switch blockers, and a live run of the system file (which
    /// `finish_open` would save; a replacement cannot, so it is refused).
    pub(crate) fn replace_blockers(&self) -> Vec<String> {
        let mut blockers = self.switch_blockers();
        if self.run.is_some() && self.can_open() {
            blockers.push(format!("a live run of {} is open: save it (Save run) or reset it first", self.store.path.display()));
        }
        blockers
    }

    /// What would be lost by switching now, if anything. A live run is not
    /// listed: `finish_open` stops it and keeps it through the auto-save.
    pub(super) fn open_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if let Some(i) = &self.input {
            let what = match &i.purpose {
                Purpose::Comment | Purpose::ThreadTitle | Purpose::CommentAuthor => "a discussion draft",
                _ => "a text field draft",
            };
            blockers.push(format!("{what} is open ({}): submit or cancel it", serde_json::to_value(&i.purpose).map(|v| v.to_string()).unwrap_or_default()));
        } else if self.discussion.editing.is_some() {
            blockers.push("a comment is being edited: save or cancel it".into());
        }
        if self.drag.is_some() {
            blockers.push("a placement drag is in progress: release it".into());
        }
        if let Some((name, done, total)) = self.study_progress() {
            blockers.push(format!("study {name} is running ({done} of {total}): wait or cancel it in Studies"));
        }
        if let Some(job) = &self.replay.job {
            blockers.push(format!("run {} is replaying: wait or cancel it in Studies", job.id));
        }
        if self.agent.state.runs.iter().any(|r| r.status.active()) {
            blockers.push("Codex is answering a discussion: wait or cancel it".into());
        }
        blockers
    }

    /// Open another system file in this window (the shared path for the
    /// Systems tab, `system_ui` and REST `system_open`). Refuses with the
    /// blockers named, or starts loading off the UI thread and returns the
    /// job's sequence number; `finish_open` installs it or reports why not.
    pub fn open_system(&mut self, path: PathBuf) -> Result<u64, String> {
        let result = self.start_open(path);
        if let Err(e) = &result {
            self.action_error = Some(e.clone());
            self.status = e.clone();
        }
        self.panel_dirty = true;
        result
    }

    fn start_open(&mut self, path: PathBuf) -> Result<u64, String> {
        if self.open.shell.is_none() {
            return Err("Opening another system is available in build mode (--system FILE) only.".into());
        }
        let path = std::path::absolute(&path).unwrap_or(path);
        if let Some(pending) = self.open.pending() {
            return Err(format!("Cannot open {}: still opening {}.", path.display(), pending.display()));
        }
        if same_file(&path, &self.store.path) {
            return Err(format!("{} is already open.", path.display()));
        }
        if !path.is_file() {
            return Err(format!("Could not open {}: no such file.", path.display()));
        }
        let blockers = self.open_blockers();
        if !blockers.is_empty() {
            return Err(format!("Not opening {}: {}. {} stays open.", path.display(), blockers.join("; "), self.store.path.display()));
        }
        let shell = self.open.shell.clone().expect("checked above");
        let (library_dir, registry) = (self.library_dir.clone(), self.registry.clone());
        let worker_path = path.clone();
        self.open.seq += 1;
        let lost = format!("Could not open {}: the loader", path.display());
        let work = crate::jobs::Job::spawn(crate::jobs::Pool::Compute, self.open.seq, lost, move |_| load(worker_path, library_dir, registry, &shell));
        self.open.job = Some(OpenJob { path: path.clone(), work, started: std::time::Instant::now() });
        self.status = format!("Opening {}: loading and validating in the background…", path.display());
        Ok(self.open.seq)
    }

    /// Stop waiting for a pending open (the worker's result is dropped).
    pub fn cancel_open(&mut self) -> bool {
        let Some(job) = self.open.job.take() else { return false };
        self.status = format!("Open of {} cancelled; {} stays open.", job.path.display(), self.store.path.display());
        self.open.last = Some((job.work.generation(), Err(self.status.clone())));
        self.panel_dirty = true;
        true
    }

    /// Late evidence work may begin after an open was accepted. Keep the old
    /// document and return the same refusal to the retained REST continuation.
    pub(super) fn refuse_pending_open(&mut self, reason: &str) {
        if self.cancel_open() {
            self.status = format!("{} Not replaced: {reason}", self.status);
            if let Some((_, result)) = self.open.last.as_mut() {
                *result = Err(self.status.clone());
            }
        }
    }

    /// Install a finished load: the only place the open system is replaced.
    /// Returns true when a load finished (installed or refused).
    pub(crate) fn finish_open(&mut self, scene: &mut SpatialScene, models: Option<&mut crate::models::ModelLibrary>) -> bool {
        let Some(job) = &self.open.job else { return false };
        let Some(loaded) = job.work.poll() else { return false };
        let job = self.open.job.take().expect("polled above");
        let seq = job.work.generation();
        self.panel_dirty = true;
        let refuse = |b: &mut Builder, e: String| {
            b.action_error = Some(e.clone());
            b.status = e.clone();
            b.open.last = Some((seq, Err(e)));
            true
        };
        let opened = match loaded {
            Ok(o) => o,
            Err(e) => {
                let e = format!("{e} {} stays open.", self.store.path.display());
                return refuse(self, e);
            }
        };
        // Something new may have started while loading.
        let blockers = self.open_blockers();
        if !blockers.is_empty() {
            let e = format!("Not opening {}: {}. {} stays open.", job.path.display(), blockers.join("; "), self.store.path.display());
            return refuse(self, e);
        }
        let Opened { builder: mut next, compiled, models: next_models, systems } = *opened;
        let mut notes = Vec::new();
        // A live run is kept, not dropped: saved to the old file's runs.
        if self.run.is_some() {
            let time = self.run.as_ref().and_then(|r| r.worker.shared().lock().ok().and_then(|s| s.snapshot.as_ref().and_then(|x| x.status.as_ref()).map(|x| x.time))).unwrap_or(0.);
            if self.run.as_ref().is_some_and(|r| r.robot) {
                // `save_run` refuses robot runs (robot_run::NO_RUN_RECORD): stop it without a save.
                notes.push(format!("the robot system's run was stopped at t = {time:.3} s (a robot system's run keeps no run record yet)"));
            } else if time >= 0.1 {
                match self.save_run("stopped to open another system") {
                    Ok(saved) => notes.push(format!("the live run was stopped and saved as {}", saved.display())),
                    Err(e) => {
                        let e = format!("Not opening {}: the live run could not be saved ({e}); stop it first. {} stays open.", job.path.display(), self.store.path.display());
                        return refuse(self, e);
                    }
                }
            } else {
                notes.push(format!("the live run was stopped at t = {time:.3} s (under 0.1 s, not kept)"));
            }
            self.run = None;
        }
        let shell = self.open.shell.clone().expect("a job needs a shell");
        let old = self.store.path.clone();
        if let Some(schematic) = shell.schematic.as_ref().filter(|s| same_file(s, &old)) {
            notes.push(format!("the schematic window still shows {} (not retargeted)", schematic.display()));
        }
        if shell.annotations.is_some() && same_file(&old, &shell.launch) {
            notes.push("--annotations stays with the launch file; this system uses its own sidecar".into());
        }
        // Per-system state all comes from the new builder; the window-level
        // pieces (scene, annotations, models) are retargeted here.
        next.open = OpenState { shell: Some(shell.clone()), job: None, seq: self.open.seq, last: None, systems };
        next.tab = self.tab;
        // The schematic pane stays open; its layout is the new system's (reset).
        next.schematic.visible = self.schematic.visible;
        // The registry is not per-system: its inspector (and any pending load) stays.
        next.actuators = std::mem::take(&mut self.actuators);
        next.realtime = self.realtime && next.document.realtime.is_some();
        if let Ok(c) = &compiled.result {
            scene.replace(c.description.clone(), c.spatial.clone(), c.animation.clone());
        }
        scene.live = Default::default();
        let annotations = shell.annotations_for(&next.store.path);
        scene.retarget_annotations(annotations.clone());
        if let Some(models) = models {
            *models = next_models;
        }
        // The compile already ran with the load; rebuild_scene draws and fits it.
        next.job = Some(crate::jobs::Job::finished(next.document.revision, Ok(compiled)));
        next.scene_dirty = false;
        next.fitted = false;
        let summary = serde_json::json!({
            "path": next.store.path, "title": next.document.title, "revision": next.document.revision,
            "runs": next.runs.len(), "annotations": annotations, "previous": old,
            "notes": notes, "load_seconds": job.started.elapsed().as_secs_f64(),
        });
        next.status = format!("Opened {} ({}, revision {}){}", next.store.path.display(), next.document.title, next.document.revision, if notes.is_empty() { String::new() } else { format!("; {}", notes.join("; ")) });
        next.open.last = Some((seq, Ok(summary)));
        next.panel_dirty = true;
        // Dropping the old builder joins its agent worker: not on the UI thread.
        let previous = std::mem::replace(self, next);
        crate::jobs::drop_off_thread(previous, "the previous system");
        true
    }

    /// REST `system_open`: start (or refuse) on the first call, then report
    /// the outcome once `finish_open` has run.
    pub(crate) fn open_request(&mut self, path: PathBuf, continuation: &mut serde_json::Value, cancelled: bool) -> sim_api::Outcome {
        let Some(seq) = continuation.get("open").and_then(|s| s.as_u64()) else {
            return match self.open_system(path) {
                Ok(seq) => {
                    *continuation = serde_json::json!({"open": seq});
                    sim_api::Outcome::Pending
                }
                Err(e) => sim_api::Outcome::Done(Err(e)),
            };
        };
        match &self.open.last {
            Some((s, result)) if *s == seq => sim_api::Outcome::Done(result.clone()),
            _ if cancelled && self.open.job.as_ref().is_some_and(|j| j.work.generation() == seq) => {
                self.cancel_open();
                sim_api::Outcome::Done(Err("cancelled".into()))
            }
            _ if self.open.job.as_ref().is_none_or(|j| j.work.generation() != seq) => sim_api::Outcome::Done(Err("the open was superseded".into())),
            _ => sim_api::Outcome::Pending,
        }
    }

    pub(super) fn open_json(&self) -> serde_json::Value {
        let shell = self.open.shell.as_ref();
        serde_json::json!({
            "available": shell.is_some(),
            "pending": self.open.pending(),
            "last": self.open.last.as_ref().map(|(seq, r)| serde_json::json!({"seq": seq, "ok": r.is_ok(), "result": r.as_ref().ok(), "error": r.as_ref().err()})),
            "systems": self.open.systems,
            "annotations": shell.map(|s| s.annotations_for(&self.store.path)),
            "schematic": shell.and_then(|s| s.schematic.as_ref()).map(|p| serde_json::json!({"file": p, "shows_this_system": same_file(p, &self.store.path)})),
        })
    }
}

/// A build-mode builder for `path` with Open enabled, its compiled scene
/// (annotations connected to `shell`'s sidecar for it) and the display
/// models (the shared catalog plus the system's own `models/`). The launch's
/// loaders, for a switch to build mode in a window that has no builder yet
/// (called on a worker).
pub(crate) fn open_build(path: PathBuf, library_dir: PathBuf, registry: BehaviorRegistry, shell: Shell) -> Result<(Builder, SpatialScene, crate::models::ModelLibrary), String> {
    let mut builder = Builder::open(path.clone(), library_dir, registry)?;
    let mut scene = compiled_scene(&builder).map_err(|e| format!("{}: does not compile: {e}", path.display()))?;
    scene.connect_annotations(shell.annotations_for(&path));
    let mut models = crate::models::ModelLibrary::open(shell.models.clone());
    models.extend(&path.parent().unwrap_or(std::path::Path::new(".")).join("models"));
    builder.enable_open(shell);
    Ok((builder, scene, models))
}

/// Worker thread: the same load, validation and compile as the launch.
fn load(path: PathBuf, library_dir: PathBuf, registry: BehaviorRegistry, shell: &Shell) -> Result<Box<Opened>, String> {
    let named = |e: String| if e.contains(&path.display().to_string()) { format!("Could not open {}.", e.trim_end_matches('.')) } else { format!("Could not open {}: {}.", path.display(), e.trim_end_matches('.')) };
    let builder = Builder::open(path.clone(), library_dir.clone(), registry).map_err(named)?;
    let compiled = compile_now(builder.document.clone(), builder.registry.clone());
    if let Err(e) = &compiled.result {
        return Err(named(format!("does not compile: {e}")));
    }
    let mut models = crate::models::ModelLibrary::open(shell.models.clone());
    models.extend(&path.parent().unwrap_or(std::path::Path::new(".")).join("models"));
    let systems = discover(&path, &library_dir);
    Ok(Box::new(Opened { builder, compiled, models, systems }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(b: &mut Builder, scene: &mut SpatialScene) {
        for _ in 0..1200 {
            if b.finish_open(scene, None) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("open did not finish");
    }

    #[test]
    fn open_system_switches_or_keeps_the_current_system() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("builder-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let board = dir.join("board.system.json");
        let winch = dir.join("winch.system.json");
        std::fs::copy(root.join("examples/systems-builder/motor-driver-board/board.system.json"), &board).unwrap();
        std::fs::copy(root.join("examples/systems-builder/worm-drive/winch.system.json"), &winch).unwrap();
        let broken = dir.join("broken.system.json");
        std::fs::write(&broken, "{\"schema\": \"sim.system/1\", \"nonsense\": true}").unwrap();
        let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
        let library = root.join("library/systems");
        let mut b = Builder::open(board.clone(), library.clone(), registry.clone()).unwrap();
        let compiled = system_builder::compile(&b.document, &registry, system_builder::config_for(&b.document)).unwrap();
        let mut scene = SpatialScene::for_builder(compiled.description.clone(), compiled.flat.spatial(&compiled.description.id, &b.document.title)).unwrap();
        // A saved run of the board, to see the list change.
        let run = sim_runtime::run_history::record(&b.document, &registry, 0.05, system_builder::config_for(&b.document), &Vec::<String>::new(), "board").unwrap();
        sim_runtime::run_history::save(&sim_runtime::run_history::dir_for(&board), &run).unwrap();
        b.runs = sim_runtime::run_history::list(&sim_runtime::run_history::dir_for(&board));
        assert_eq!(b.runs.len(), 1);

        // Lesson mode (no shell) cannot switch.
        assert!(b.open_system(winch.clone()).unwrap_err().contains("build mode"));
        b.enable_open(Shell { launch: board.clone(), annotations: Some(dir.join("explicit.json")), schematic: None, models: root.join("library/models") });
        assert!(b.open.systems.iter().any(|p| p.ends_with("winch.system.json")), "{:?}", b.open.systems);
        let (title, revision) = (b.document.title.clone(), b.document.revision);

        // A missing file names the path; nothing changes.
        let missing = dir.join("nope.system.json");
        let e = b.open_system(missing.clone()).unwrap_err();
        assert!(e.contains(&missing.display().to_string()), "{e}");
        assert!(b.open.job.is_none());
        // An invalid document fails on the worker, names the path, keeps the board.
        b.open_system(broken.clone()).unwrap();
        wait(&mut b, &mut scene);
        let e = b.open.last.clone().unwrap().1.unwrap_err();
        assert!(e.contains(&broken.display().to_string()) && e.contains("stays open"), "{e}");
        assert_eq!((b.path(), b.document.title.clone(), b.document.revision, b.runs.len()), (board.as_path(), title.clone(), revision, 1));
        assert!(b.status.contains("broken.system.json"), "{}", b.status);

        // Guard: an open draft refuses and names it.
        b.start_input(Purpose::Filter, "motor".into());
        let e = b.open_system(winch.clone()).unwrap_err();
        assert!(e.contains("text field draft") && e.contains("stays open"), "{e}");
        assert!(b.open.job.is_none());
        // A draft started while loading is kept: the install is refused.
        b.input = None;
        b.open_system(winch.clone()).unwrap();
        b.start_input(Purpose::Filter, "gear".into());
        wait(&mut b, &mut scene);
        assert!(b.open.last.clone().unwrap().1.unwrap_err().contains("text field draft"));
        assert_eq!((b.path(), b.input.as_ref().map(|i| i.buffer.as_str())), (board.as_path(), Some("gear")));
        b.input = None;

        // Success: path, document, runs, annotations and scene follow the winch;
        // the board's selection leaves with its document (`picked::follow_open`).
        let (mut selection, mut documents) = super::super::test_support::test_selection(&b);
        let board_document = picked::document(&documents).unwrap();
        Picked::new(&mut selection, &mut documents).set(["mcu".to_string()]).unwrap();
        let seq = b.open_system(winch.clone()).unwrap();
        assert!(b.status.starts_with("Opening"), "{}", b.status);
        wait(&mut b, &mut scene);
        picked::follow_open(&mut selection, &mut documents, b.path());
        let selected = picked::names(&selection, &documents);
        assert_ne!(picked::document(&documents), Some(board_document), "the winch is a new document");
        let summary = b.open.last.clone().unwrap();
        assert_eq!(summary.0, seq);
        let summary = summary.1.unwrap();
        assert_eq!(b.path(), winch.as_path());
        assert_ne!(b.document.title, title);
        assert!(b.runs.is_empty() && selected.is_empty() && selection.all().is_empty() && b.level.is_empty() && b.replay.outcomes.is_empty());
        assert!(b.study.result.is_none() && b.study.job.is_none() && !b.fitted && b.job.is_some());
        assert_eq!(summary["annotations"], serde_json::json!(format!("{}.annotations.json", winch.display())));
        assert_eq!(scene.description.id, b.job.as_ref().unwrap().poll().unwrap().unwrap().result.unwrap().description.id);
        let state = b.state_json(&selected);
        assert_eq!((state["path"].clone(), state["title"].clone()), (serde_json::json!(winch), serde_json::json!(b.document.title)));
        // Opening wrote nothing: no runs directory or annotations file for the winch.
        assert!(!sim_runtime::run_history::dir_for(&winch).exists());
        assert!(!PathBuf::from(format!("{}.annotations.json", winch.display())).exists());
        // Back to the launch file: its explicit --annotations file returns.
        b.open_system(board.clone()).unwrap();
        wait(&mut b, &mut scene);
        let back = b.open.last.clone().unwrap().1.unwrap();
        assert_eq!((back["annotations"].clone(), b.runs.len()), (serde_json::json!(dir.join("explicit.json")), 1));
        std::fs::remove_dir_all(&dir).ok();
    }
}
