//! Robot FILE mode's source watch (`--robot FILE`): the opened
//! `.simrobot.json` is re-read when it changes on disk, and on a manual
//! Reload. The UI thread only stats the file (length and mtime, every
//! [`POLL`]); reading, hashing and parsing run on a worker through the same
//! loader as the first open ([`crate::robot::load_bytes`]). Presets are not
//! watched.
//!
//! A half-written file is never applied: a model is applied only after the
//! whole file was read, its sha256 differs from the loaded file's and the
//! same bytes parsed. A failed attempt keeps the loaded hash, stat and
//! model; the stat of the failed attempt is remembered, so the next write
//! (a changed stat) retries.
use crate::robot::{Loaded, load_bytes};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// How often the UI thread stats the opened file.
pub const POLL: Duration = Duration::from_millis(500);
pub const RULE: &str = "FILE mode only: the UI thread stats the opened file (length, mtime) every 0.5 s; a changed stat, or a manual Reload, reads and sha256-hashes the whole file on a worker thread. Identical bytes (a touch, an atomic rewrite with the same content) are `unchanged` and nothing is replaced. Different bytes go through the same loader as the first open (PhysicalModel::parse, triangulation, CAD link status); only a successful parse is applied (`loaded`). A read or parse error (`failed`) keeps the last good model, its hash and its run; the next change on disk retries. Presets are not watched.";

/// What started a load of the source file.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// The first load when the window opens.
    Open,
    /// A changed stat seen by the poll.
    Watch,
    /// The Reload button, `system_ui` robot:reload or REST `robot_reload`.
    Manual,
}

/// The cheap change detector: length and modification time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stat {
    len: u64,
    modified: Option<SystemTime>,
}
/// `None` when the file cannot be stat'ed (missing, unreadable directory).
pub fn stat(path: &Path) -> Option<Stat> {
    std::fs::metadata(path).ok().map(|m| Stat { len: m.len(), modified: m.modified().ok() })
}

pub enum Outcome {
    Loaded(Box<Loaded>),
    /// The bytes hash to the loaded file's hash.
    Unchanged,
    /// A read or parse error naming the path.
    Failed(String),
}
impl Outcome {
    pub fn name(&self) -> &'static str {
        match self {
            Outcome::Loaded(_) => "loaded",
            Outcome::Unchanged => "unchanged",
            Outcome::Failed(_) => "failed",
        }
    }
}
/// One worker check of the file.
pub struct Checked {
    /// Taken before the read: a write racing the read changes it again, so the next poll re-checks.
    pub stat: Option<Stat>,
    /// sha256 of the bytes read (None when the read failed).
    pub hash: Option<String>,
    pub outcome: Outcome,
    /// The results file beside the model (`robot_stress`), read by the same worker.
    pub results: Option<crate::robot_stress::StressResults>,
    pub seconds: f64,
}

/// Stat, read, hash, compare with `loaded_hash` and (only when different)
/// parse through the shared loader; then read the results file beside it
/// (`robot_stress::read`). Called on a worker thread.
pub fn check(path: &Path, loaded_hash: Option<&str>) -> Checked {
    let started = Instant::now();
    let stat = stat(path);
    let (hash, outcome) = match std::fs::read(path) {
        Err(e) => (None, Outcome::Failed(format!("{}: {e}", path.display()))),
        Ok(bytes) => {
            let hash = sim_domain_robot::cad_link::sha256_hex(&bytes);
            let outcome = if loaded_hash == Some(hash.as_str()) {
                Outcome::Unchanged
            } else {
                match load_bytes(path, &bytes) {
                    Ok(l) => Outcome::Loaded(Box::new(l)),
                    Err(e) => Outcome::Failed(e),
                }
            };
            (Some(hash), outcome)
        }
    };
    let results = Some(crate::robot_stress::read(path));
    Checked { stat, hash, outcome, results, seconds: started.elapsed().as_secs_f64() }
}

#[derive(Clone, Debug, Serialize)]
pub struct LastReload {
    pub trigger: Trigger,
    /// loaded | unchanged | failed
    pub outcome: &'static str,
    pub error: Option<String>,
    pub at: String,
}

/// The loaded source file's identity and the reload bookkeeping.
pub struct SourceWatch {
    pub path: PathBuf,
    /// sha256 of the bytes the displayed model was parsed from.
    pub hash: Option<String>,
    /// UTC time the displayed model was applied.
    pub loaded_at: Option<String>,
    /// Successful reloads (the first open not counted).
    pub reload_count: u64,
    /// Watch and manual checks that found the loaded bytes.
    pub unchanged_checks: u64,
    pub last: Option<LastReload>,
    /// Whether the last successful reload discarded run or jog state.
    pub run_reset: Option<bool>,
    /// The error of the latest check while the file on disk does not load (the displayed model is the last good one).
    pub failing: Option<String>,
    stat: Option<Stat>,
    next_poll: Instant,
    in_flight: Option<(Trigger, crate::jobs::Job<Checked>)>,
}
impl SourceWatch {
    /// Starts the first load (trigger `open`) on a worker.
    pub fn open(path: PathBuf) -> Self {
        let mut w = Self { path, hash: None, loaded_at: None, reload_count: 0, unchanged_checks: 0, last: None, run_reset: None, failing: None, stat: None, next_poll: Instant::now() + POLL, in_flight: None };
        w.spawn(Trigger::Open);
        w
    }
    pub fn busy(&self) -> Option<Trigger> {
        self.in_flight.as_ref().map(|(t, _)| *t)
    }
    /// Why a manual reload is refused now.
    pub fn check_reload(&self) -> Result<(), String> {
        match self.busy() {
            Some(Trigger::Open) => Err(format!("{} is still opening; reload after it loads", self.path.display())),
            Some(_) => Err(format!("a reload of {} is already in progress", self.path.display())),
            None => Ok(()),
        }
    }
    /// Starts a check on a worker (refused while one is in flight).
    pub fn start(&mut self, trigger: Trigger) -> Result<(), String> {
        self.check_reload()?;
        self.spawn(trigger);
        Ok(())
    }
    fn spawn(&mut self, trigger: Trigger) {
        let (path, hash) = (self.path.clone(), self.hash.clone());
        // Read, hash, parse and triangulate: CPU work.
        let job = crate::jobs::Job::spawn(crate::jobs::Pool::Compute, 0, format!("{}: the reload worker", self.path.display()), move |_| Ok(check(&path, hash.as_deref())));
        self.in_flight = Some((trigger, job));
    }
    /// The UI thread's poll: at most every [`POLL`], a metadata stat.
    /// Returns whether it changed since the last check (the caller then
    /// starts a `watch` check through the one Reload handler).
    pub fn poll(&mut self, now: Instant) -> bool {
        if now < self.next_poll || self.in_flight.is_some() {
            return false;
        }
        self.next_poll = now + POLL;
        stat(&self.path) != self.stat
    }
    /// The finished check, if any (never blocks).
    pub fn take(&mut self) -> Option<(Trigger, Checked)> {
        let (trigger, job) = self.in_flight.as_ref()?;
        let checked = match job.poll()? {
            Ok(c) => c,
            Err(e) => Checked { stat: None, hash: None, outcome: Outcome::Failed(e), results: None, seconds: 0.0 },
        };
        let trigger = *trigger;
        self.in_flight = None;
        Some((trigger, checked))
    }
    /// Records a finished check; returns the model to apply (only `loaded`).
    /// A failure keeps `hash`, so different content later is still a change.
    pub fn settle(&mut self, trigger: Trigger, checked: Checked, now_utc: String) -> Option<Box<Loaded>> {
        self.stat = checked.stat;
        let error = match &checked.outcome {
            Outcome::Failed(e) => Some(e.clone()),
            _ => None,
        };
        if trigger != Trigger::Open {
            self.last = Some(LastReload { trigger, outcome: checked.outcome.name(), error: error.clone(), at: now_utc.clone() });
        }
        match checked.outcome {
            Outcome::Failed(e) => {
                self.failing = Some(e);
                None
            }
            Outcome::Unchanged => {
                // The file on disk is the displayed model again.
                self.failing = None;
                self.unchanged_checks += 1;
                None
            }
            Outcome::Loaded(l) => {
                self.failing = None;
                self.hash = checked.hash;
                self.loaded_at = Some(now_utc);
                if trigger != Trigger::Open {
                    self.reload_count += 1;
                }
                Some(l)
            }
        }
    }
    /// `robot_state.source_file`.
    pub fn json(&self, watching: bool) -> Value {
        json!({"path": self.path, "sha256": self.hash, "loaded_at": self.loaded_at, "reload_count": self.reload_count, "unchanged_checks": self.unchanged_checks,
            "watching": watching, "poll_s": POLL.as_secs_f64(), "in_flight": self.busy(), "last_reload": self.last, "run_reset": self.run_reset,
            "showing_last_good": self.failing.is_some() && self.hash.is_some(), "failing_error": self.failing, "rule": RULE})
    }
}

/// Now as `2026-09-30T12:00:00.000Z`.
pub fn now_utc() -> String {
    crate::robot_recording::iso(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::robot_run::RunController;

    fn finished(w: &mut SourceWatch) -> (Trigger, Checked) {
        let until = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(done) = w.take() {
                return done;
            }
            assert!(Instant::now() < until, "check did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The real worker check and shared loader on a temp copy of the wheeled
    /// robot: a changed mass loads under a later generation, invalid JSON keeps
    /// the last good hash with an error naming the path, and identical bytes
    /// (an atomic tmp + rename rewrite) are `unchanged`.
    #[test]
    fn reload_changed_invalid_and_unchanged() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let original = std::fs::read(root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap();
        let dir = std::env::temp_dir().join(format!("sim-spatial-robot-source-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("robot.simrobot.json");
        std::fs::write(&path, &original).unwrap();

        let mut w = SourceWatch::open(path.clone());
        let (trigger, checked) = finished(&mut w);
        assert_eq!(trigger, Trigger::Open);
        let first = w.settle(trigger, checked, now_utc()).expect("the copy loads");
        let (hash0, mass0) = (w.hash.clone().unwrap(), first.model.links[0].mass);
        assert!(!w.poll(Instant::now() + POLL * 2), "an unchanged file is not a change");
        let (mut run, reset) = RunController::replace(None, first.model.clone());
        assert!(!reset);
        let generation0 = run.generation();

        // A changed mass: the stat changes, the reload applies it, the run context moves on.
        let mut v: Value = serde_json::from_slice(&original).unwrap();
        let mass1 = mass0 * 2.0 + 0.125;
        v["links"][0]["mass"] = json!(mass1);
        let changed = serde_json::to_vec_pretty(&v).unwrap();
        std::fs::write(&path, &changed).unwrap();
        assert!(w.poll(Instant::now() + POLL * 4), "a rewritten file is a changed stat");
        w.start(Trigger::Watch).unwrap();
        assert!(w.start(Trigger::Manual).unwrap_err().contains("already in progress"));
        let (trigger, checked) = finished(&mut w);
        assert_eq!(checked.outcome.name(), "loaded");
        let next = w.settle(trigger, checked, now_utc()).expect("changed mass applies");
        assert_eq!(next.model.links[0].mass, mass1);
        assert_ne!(mass1, mass0);
        let hash1 = w.hash.clone().unwrap();
        assert_ne!(hash1, hash0);
        assert_eq!((w.reload_count, w.last.as_ref().unwrap().trigger, w.last.as_ref().unwrap().outcome), (1, Trigger::Watch, "loaded"));
        (run, _) = RunController::replace(Some(run), next.model.clone());
        assert!(run.generation() > generation0);
        assert_eq!(run.model().links[0].mass, mass1);

        // Invalid JSON: failed, naming the path; the loaded hash (and so the model) stays.
        std::fs::write(&path, b"{\"links\": [").unwrap();
        w.start(Trigger::Watch).unwrap();
        let (trigger, checked) = finished(&mut w);
        assert!(w.settle(trigger, checked, now_utc()).is_none());
        let last = w.last.clone().unwrap();
        assert_eq!(last.outcome, "failed");
        assert!(last.error.as_deref().unwrap().contains(&path.display().to_string()), "{last:?}");
        assert_eq!(w.hash.as_deref(), Some(hash1.as_str()));
        assert_eq!(w.reload_count, 1);
        assert!(w.failing.is_some());
        assert!(!w.poll(Instant::now() + POLL * 6), "the failed content is not retried until it changes");

        // Identical bytes written atomically (tmp + rename): unchanged, nothing applied.
        let tmp = dir.join("robot.simrobot.json.tmp");
        std::fs::write(&tmp, &changed).unwrap();
        std::fs::rename(&tmp, &path).unwrap();
        w.start(Trigger::Manual).unwrap();
        let (trigger, checked) = finished(&mut w);
        assert!(w.settle(trigger, checked, now_utc()).is_none());
        assert_eq!((w.last.as_ref().unwrap().trigger, w.last.as_ref().unwrap().outcome), (Trigger::Manual, "unchanged"));
        assert_eq!((w.hash.as_deref(), w.reload_count, w.failing.as_deref()), (Some(hash1.as_str()), 1, None));

        drop(run);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
