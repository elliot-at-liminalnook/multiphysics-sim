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

/// Nothing outside `src/jobs/` starts a thread (native-viewer.md §4).
#[test]
fn threads_are_started_only_in_jobs() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let jobs = src.join("jobs");
    // Built so this file never contains the needles literally.
    let needles = [["thread", "::spawn"].concat(), ["thread", "::Builder"].concat()];
    let mut offenders = Vec::new();
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
                for (i, line) in text.lines().enumerate() {
                    if needles.iter().any(|n| line.contains(n.as_str())) {
                        offenders.push(format!("{}:{}: {}", path.strip_prefix(&src).unwrap_or(&path).display(), i + 1, line.trim()));
                    }
                }
            }
        }
    }
    assert!(offenders.is_empty(), "start threads through crate::jobs (Job, RunThread, reap_child, drop_off_thread), not directly:\n{}", offenders.join("\n"));
}
