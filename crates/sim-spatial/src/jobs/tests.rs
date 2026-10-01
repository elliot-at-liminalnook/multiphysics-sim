//! The jobs module without a window. The pools come from `pools()`
//! (`TaskPoolOptions::default().create_default_pools()`, Bevy's own
//! `get_or_init` path), so no `App` is needed. Jobs that block use
//! `Pool::Dedicated`: a default pool may have a single thread on a small
//! machine, and a test must not wait on itself.
use super::*;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn wait<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        if let Some(v) = f() {
            return v;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn both_pools_run_jobs_without_an_app() {
    let io = Job::<u32>::spawn(Pool::Io, 1, "io job", |_| Ok(1));
    let compute = Job::<u32>::spawn(Pool::Compute, 2, "compute job", |_| Ok(2));
    assert_eq!(wait("io", || io.poll()), Ok(1));
    assert_eq!(wait("compute", || compute.poll()), Ok(2));
    assert_eq!((io.generation(), compute.generation()), (1, 2));
    // Taken once.
    assert_eq!(io.poll(), None);
}

#[test]
fn stale_generation_is_dropped() {
    let (release, gate) = mpsc::channel::<()>();
    let mut latest = Latest::<u32>::default();
    let first = latest.start(Pool::Dedicated, "first", move |_| {
        let _ = gate.recv_timeout(Duration::from_secs(5));
        Ok(1)
    });
    let second = latest.start(Pool::Dedicated, "second", |_| Ok(2));
    assert_eq!((first, second), (1, 2));
    assert_eq!(wait("second", || latest.poll()), (2, Ok(2)));
    let _ = release.send(());
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(latest.poll(), None, "the superseded generation's result is never returned");
    assert_eq!(latest.pending(), None);
}

#[test]
fn cancel_is_observed_explicitly_and_on_drop() {
    let spin = |saw: Arc<AtomicBool>, started: mpsc::Sender<()>| {
        move |ctx: &Ctx| {
            let _ = started.send(());
            while !ctx.cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            saw.store(true, Ordering::SeqCst);
            Err::<(), _>("cancelled".to_string())
        }
    };
    let (started, running) = mpsc::channel();
    let saw = Arc::new(AtomicBool::new(false));
    let job = Job::spawn(Pool::Dedicated, 0, "explicit", spin(saw.clone(), started.clone()));
    running.recv_timeout(Duration::from_secs(5)).unwrap();
    job.cancel();
    assert_eq!(wait("explicit cancel", || job.poll()), Err("cancelled".into()));
    assert!(saw.load(Ordering::SeqCst));

    let saw = Arc::new(AtomicBool::new(false));
    let job = Job::spawn(Pool::Dedicated, 0, "dropped", spin(saw.clone(), started));
    running.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(job);
    wait("cancel on drop", || saw.load(Ordering::SeqCst).then_some(()));

    // Cancelled before it started: the closure never runs, and the owner still gets an answer.
    // Every AsyncComputeTaskPool thread (at most 4) is held until `release` is dropped.
    let (release, gate) = mpsc::channel::<()>();
    let gate = Arc::new(Mutex::new(gate));
    let _busy: Vec<Job<()>> = (0..8)
        .map(|_| {
            let gate = gate.clone();
            Job::spawn(Pool::Compute, 0, "busy", move |_| {
                let _ = gate.lock().unwrap_or_else(|p| p.into_inner()).recv_timeout(Duration::from_secs(5));
                Ok(())
            })
        })
        .collect();
    let ran = Arc::new(AtomicBool::new(false));
    let flag = ran.clone();
    let queued = Job::<()>::spawn(Pool::Compute, 0, "queued", move |_| {
        flag.store(true, Ordering::SeqCst);
        Ok(())
    });
    queued.cancel();
    drop(release);
    assert_eq!(wait("queued", || queued.poll()), Err("queued was cancelled before it started.".into()));
    assert!(!ran.load(Ordering::SeqCst), "a job cancelled before it started never runs");
}

#[test]
fn errors_and_panics_are_surfaced() {
    let failed = Job::<()>::spawn(Pool::Io, 0, "reader", |_| Err("could not read /no/such/file: not found".into()));
    assert_eq!(wait("error", || failed.poll()), Err("could not read /no/such/file: not found".into()));
    let panicked = Job::<()>::spawn(Pool::Compute, 0, "the layout", |_| panic!("index out of range"));
    let e = wait("panic", || panicked.poll()).unwrap_err();
    assert_eq!(e, "the layout ended without a result (index out of range).");
}

#[test]
fn progress_and_updates_are_visible_while_running() {
    let (release, gate) = mpsc::channel::<()>();
    let job = Job::<&str, u32>::streaming(Pool::Dedicated, 3, "progress", move |ctx| {
        ctx.fraction(0.5);
        ctx.steps(1, 2);
        ctx.emit(7);
        ctx.message("half way");
        let _ = gate.recv_timeout(Duration::from_secs(5));
        Ok("done")
    });
    let p = wait("progress", || Some(job.progress()).filter(|p| p.message == "half way"));
    assert_eq!((p.fraction, p.steps), (Some(0.5), Some((1, 2))));
    assert_eq!(job.updates(), vec![7]);
    assert!(job.updates().is_empty(), "updates are drained");
    assert_eq!(job.poll(), None, "still running");
    release.send(()).unwrap();
    assert_eq!(wait("result", || job.poll()), Ok("done"));
}

#[test]
fn a_detached_write_completes_after_its_handle_is_dropped() {
    let path = std::env::temp_dir().join(format!("sim-spatial-jobs-{}-{}.txt", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
    let (release, gate) = mpsc::channel::<()>();
    let target = path.clone();
    let job = Job::<()>::spawn(Pool::Io, 0, "writer", move |ctx| {
        let _ = gate.recv_timeout(Duration::from_secs(5));
        std::fs::write(&target, if ctx.cancelled() { "cancelled" } else { "written" }).map_err(|e| e.to_string())
    })
    .complete_on_drop();
    drop(job);
    release.send(()).unwrap();
    let text = wait("the write", || std::fs::read_to_string(&path).ok().filter(|t| !t.is_empty()));
    let _ = std::fs::remove_file(&path);
    assert_eq!(text, "written", "a drop never cancels a save");
}

#[test]
fn finished_handles_hand_over_a_result() {
    let job = Job::<u8>::finished(9, Ok(4));
    assert_eq!((job.generation(), job.poll()), (9, Some(Ok(4))));
}

#[derive(Clone, Debug, PartialEq)]
struct Snap {
    generation: u64,
    value: u64,
}
impl Stamped for Snap {
    fn generation(&self) -> u64 {
        self.generation
    }
}

#[test]
fn run_thread_publishes_stamped_snapshots_and_joins_on_drop() {
    let stopped = Arc::new(AtomicBool::new(false));
    let flag = stopped.clone();
    let worker = RunThread::spawn("jobs-test-run", Snap { generation: 0, value: 0 }, move |rx: mpsc::Receiver<(u64, u64)>, out| {
        while let Ok((generation, value)) = rx.recv() {
            *out.lock().unwrap() = Snap { generation, value };
        }
        flag.store(true, Ordering::SeqCst);
    });
    worker.send((1, 10)).unwrap();
    assert_eq!(wait("generation 1", || worker.latest(1)), Snap { generation: 1, value: 10 });
    assert_eq!(worker.latest(2), None, "a snapshot older than the expected generation is stale");
    assert!(!worker.finished());
    let started = Instant::now();
    drop(worker);
    assert!(stopped.load(Ordering::SeqCst), "joined: the worker had returned when drop returned");
    assert!(started.elapsed() < JOIN_BOUND, "{:?}", started.elapsed());
}

#[test]
fn run_thread_drop_is_bounded_when_the_worker_is_busy() {
    let worker = RunThread::spawn("jobs-test-busy", (), |_rx: mpsc::Receiver<()>, _| std::thread::sleep(Duration::from_secs(2)));
    let started = Instant::now();
    drop(worker);
    let waited = started.elapsed();
    assert!(waited >= JOIN_BOUND && waited < JOIN_BOUND + Duration::from_millis(500), "{waited:?}");
    let idle = RunThread::<(), ()>::idle("jobs-test-idle", ());
    assert_eq!(idle.send(()), Err(Stopped));
    let never = RunThread::spawn("jobs-test-zero", (), |_rx: mpsc::Receiver<()>, _| std::thread::sleep(Duration::from_secs(2))).join_bound(Duration::ZERO);
    let started = Instant::now();
    drop(never);
    assert!(started.elapsed() < Duration::from_millis(100));
}

/// True while `pid` exists. A zombie still answers `kill -0` until it is
/// reaped, so "gone" also proves a reaper waited for it.
#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Polls `f` until it is true or `bound` passes; returns whether it became true.
#[cfg(unix)]
fn within(bound: Duration, mut f: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < bound {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    f()
}

#[cfg(unix)]
fn command(program: &str, args: &[&str]) -> std::process::Command {
    let mut c = std::process::Command::new(program);
    c.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    c
}

#[cfg(unix)]
#[test]
fn child_stop_kills_and_reaps_without_blocking() {
    let child = ChildProcess::spawn("test sleeper", command("sleep", &["30"])).unwrap();
    let pid = child.id();
    assert_eq!(child.name(), "test sleeper");
    assert!(pid_alive(pid));
    let started = Instant::now();
    child.stop();
    assert!(started.elapsed() < Duration::from_millis(500), "stop never waits on the caller: {:?}", started.elapsed());
    assert!(within(Duration::from_secs(2), || !pid_alive(pid)), "pid {pid} was killed and reaped");
}

#[cfg(unix)]
#[test]
fn dropping_a_child_process_stops_it() {
    let child = ChildProcess::spawn("dropped sleeper", command("sleep", &["30"])).unwrap();
    let pid = child.id();
    assert!(pid_alive(pid));
    drop(child);
    assert!(within(Duration::from_secs(2), || !pid_alive(pid)), "pid {pid} was killed and reaped on drop");
}

#[cfg(unix)]
#[test]
fn a_detached_child_keeps_running_and_is_reaped_when_it_ends() {
    let child = ChildProcess::spawn("detached sleeper", command("sleep", &["0.3"])).unwrap();
    let pid = child.id();
    child.detach();
    assert!(pid_alive(pid), "detach does not kill");
    assert!(within(Duration::from_millis(1500), || !pid_alive(pid)), "pid {pid} ended and was reaped");
}

#[cfg(unix)]
#[test]
fn spawning_a_missing_program_names_it() {
    let missing = "/no/such/dir/sim-spatial-missing-program";
    let e = ChildProcess::spawn("the missing tool", command(missing, &[])).err().expect("spawn of a missing program fails");
    assert!(e.starts_with("could not start the missing tool: "), "{e}");
}

#[cfg(unix)]
#[test]
fn exited_reports_a_finished_child_and_none_while_running() {
    let mut running = ChildProcess::spawn("running sleeper", command("sleep", &["30"])).unwrap();
    assert_eq!(running.exited(), None);
    let pid = running.id();
    running.stop();
    assert!(within(Duration::from_secs(2), || !pid_alive(pid)));

    let mut done = ChildProcess::spawn("true", command("true", &[])).unwrap();
    let mut report = None;
    assert!(within(Duration::from_secs(2), || {
        report = done.exited();
        report.is_some()
    }));
    let report = report.unwrap();
    assert!(report.starts_with("true exited ("), "{report}");
    assert_eq!(done.exited(), Some(report), "repeated calls report the same exit");
    // Already reaped: stopping it neither kills nor waits again.
    done.stop();
}

#[cfg(unix)]
#[test]
fn spawn_detached_keeps_running_reaps_and_names_a_failure() {
    let pid = spawn_detached("detached helper sleeper", command("sleep", &["0.3"])).unwrap();
    assert!(pid_alive(pid), "a detached process is not killed");
    assert!(within(Duration::from_millis(1500), || !pid_alive(pid)), "pid {pid} ended and was reaped");
    let e = spawn_detached("the missing companion", command("/no/such/dir/sim-spatial-missing-program", &[])).unwrap_err();
    assert!(e.starts_with("could not start the missing companion: "), "{e}");
}

#[test]
fn open_in_browser_refuses_anything_but_a_web_link() {
    // Refused before any opener is started.
    for url in ["file:///etc/hosts", "/tmp/notes.md", "ftp://example.com", ""] {
        assert_eq!(open_in_browser(url), Err(format!("not a web link: {url}")));
    }
}

#[test]
fn open_local_refuses_relative_and_missing_paths_by_name() {
    // Refused before any opener is started.
    for path in ["notes.md", "runs/cad-print/guide.html", ""] {
        assert_eq!(open_local(std::path::Path::new(path)), Err(format!("not an absolute path: {path}")));
    }
    let missing = std::env::temp_dir().join("sim-spatial-open-local-missing").join("guide.html");
    assert_eq!(open_local(&missing), Err(format!("no such file or folder: {}", missing.display())));
}

/// Every non-comment line of the `.rs` files under `src/` outside
/// `src/jobs/` for which `hit` is true: (path relative to `src/`,
/// "path:line: text"), sorted by path.
fn scan_outside_jobs(hit: impl Fn(&str) -> bool) -> Vec<(std::path::PathBuf, String)> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let jobs = src.join("jobs");
    let mut hits = Vec::new();
    let mut dirs = vec![src.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path != jobs {
                    dirs.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                let relative = path.strip_prefix(&src).unwrap_or(&path).to_path_buf();
                for (i, line) in text.lines().enumerate() {
                    if !line.trim_start().starts_with("//") && hit(line) {
                        hits.push((relative.clone(), format!("{}:{}: {}", relative.display(), i + 1, line.trim())));
                    }
                }
            }
        }
    }
    hits.sort();
    hits
}

/// Nothing outside `src/jobs/` starts a thread (native-viewer.md §4).
#[test]
fn threads_are_started_only_in_jobs() {
    // Built so this file never contains the needles literally.
    let needles = [["thread", "::spawn"].concat(), ["thread", "::Builder"].concat()];
    let offenders: Vec<String> = scan_outside_jobs(|line| needles.iter().any(|n| line.contains(n.as_str()))).into_iter().map(|(_, hit)| hit).collect();
    assert!(offenders.is_empty(), "start threads through crate::jobs (Job, RunThread, drop_off_thread), not directly:\n{}", offenders.join("\n"));
}

/// True if `line` names `needle` as a whole path segment (`Command::new(`
/// but not `SpatialCommand::new(`).
fn has_segment(line: &str, needle: &str) -> bool {
    line.match_indices(needle).any(|(at, _)| !line[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_'))
}

/// Nothing outside `src/jobs/` starts a process (native-viewer.md §4): no
/// `.spawn()`/`.output()` (std's `Command` runners; Bevy's and the jobs
/// module's `spawn` take arguments), and a `Command::new(` only where the
/// command is built and handed to `jobs` (`ChildProcess::spawn`,
/// `spawn_detached`), listed in `BUILDS` with the number of sites and why.
/// A command built elsewhere (sim-runtime's `service_command`, which CAD
/// mode hands to `ChildProcess::spawn`) does not appear here at all.
#[test]
fn processes_are_started_only_in_jobs() {
    const BUILDS: &[(&str, usize, &str)] = &[
        ("main.rs", 2, "the linked sim-viewer window's command (build and inspect modes), started with jobs::spawn_detached"),
        ("cad/lifecycle_tests.rs", 1, "test-only stand-in services and kill probes, handed to jobs::ChildProcess::spawn"),
    ];
    // Built so this file never contains the needles literally.
    let runners = [[".spawn", "()"].concat(), [".output", "()"].concat()];
    let new = ["Command", "::new("].concat();
    let mut offenders: Vec<String> = scan_outside_jobs(|line| runners.iter().any(|n| line.contains(n.as_str()))).into_iter().map(|(_, hit)| hit).collect();
    let builds = scan_outside_jobs(|line| has_segment(line, &new));
    let mut files: Vec<&std::path::PathBuf> = builds.iter().map(|(file, _)| file).collect();
    files.dedup();
    for file in files {
        let sites: Vec<&String> = builds.iter().filter(|(f, _)| f == file).map(|(_, hit)| hit).collect();
        let allowed = BUILDS.iter().find(|(path, _, _)| file.as_path() == std::path::Path::new(path)).map_or(0, |(_, count, _)| *count);
        if sites.len() != allowed {
            offenders.push(format!("{} builds {} Command(s), {allowed} allowed:\n  {}", file.display(), sites.len(), sites.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")));
        }
    }
    for (path, count, _) in BUILDS {
        if *count > 0 && !builds.iter().any(|(f, _)| f.as_path() == std::path::Path::new(path)) {
            offenders.push(format!("{path}: allowed {count} Command build(s) but has none; update BUILDS"));
        }
    }
    assert!(offenders.is_empty(), "start processes through crate::jobs (ChildProcess, spawn_detached, open_in_browser), not directly:\n{}", offenders.join("\n"));
}
