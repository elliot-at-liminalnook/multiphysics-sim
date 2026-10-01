//! All background work of the viewer (native-viewer.md §4). Nothing outside
//! this module starts a thread (`tests::threads_are_started_only_in_jobs`
//! scans the source tree for it).
//!
//! - **One-shot jobs** ([`Job`], [`Latest`]): load, scan, read, write,
//!   compute, lay out, generate. A job runs a closure once on a [`Pool`] and
//!   hands back a `Result<T, String>`; it has a generation stamp, a cancel
//!   token the closure can check, optional progress and (for streamed work)
//!   partial updates. Dropping the handle cancels the job, unless it was
//!   marked [`Job::complete_on_drop`] (saves and writes). A panic in the
//!   closure is surfaced as an error naming the job, never lost.
//! - **Long-lived workers** ([`RunThread`]): a simulation run, a gait or
//!   playback clock, the placement validator. One named OS thread each, with
//!   commands in and a shared, generation-stamped snapshot out; dropping it
//!   closes the command channel and joins within a bound.
//! - **Child processes** ([`ChildProcess`]): a process the viewer started
//!   and owns (CAD mode's self-started RoboCAD service). `stop` kills it and
//!   a reaper thread waits for it; dropping it does the same; `detach` leaves
//!   it running. Only self-started processes are ever owned, so an attached
//!   service is never stopped.
//! - **Helpers**: [`reap_child`] waits for a detached child process and
//!   [`drop_off_thread`] drops a large value away from the UI thread.
//!
//! Pool rule (Bevy 0.19.1 `TaskPoolOptions::default()`: `IoTaskPool` and
//! `AsyncComputeTaskPool` each get 25% of the cores, at least 1 and at most
//! 4 threads, so a 2-core machine has one of each; the asset server loads on
//! `IoTaskPool`):
//! - [`Pool::Io`]: file reads, writes and directory scans (short, blocking on
//!   the local disk). That is what `IoTaskPool` is for.
//! - [`Pool::Compute`]: CPU work of up to a few seconds (lay out, compile,
//!   parse and load, rasterise, build a context) on `AsyncComputeTaskPool`.
//! - [`Pool::Dedicated`]: anything that may hold its thread for longer:
//!   network and bench requests, model APIs, studies and replays that fan
//!   out their own workers, scene recordings, a drop that joins other
//!   threads. It gets its own named thread, so it can never occupy one of the
//!   few pool threads the asset server and the other jobs share for minutes.
use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Once};

use bevy::tasks::{AsyncComputeTaskPool, IoTaskPool, Task, TaskPool};

mod child;
mod run_thread;
#[cfg(test)]
mod tests;

pub use child::ChildProcess;
pub use run_thread::{JOIN_BOUND, RunThread, Stamped, Stopped};

/// Where a one-shot job runs (see the module's pool rule).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pool {
    Io,
    Compute,
    Dedicated,
}

/// What a job has reported so far. Every field is optional: a job sets what
/// it knows (a fraction, a count, a line of text).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub fraction: Option<f64>,
    pub steps: Option<(u64, u64)>,
    pub message: String,
}

struct Shared<T, U> {
    cancel: AtomicBool,
    progress: Mutex<Progress>,
    updates: Mutex<Vec<U>>,
    result: Mutex<Option<Result<T, String>>>,
}

fn lock<V>(m: &Mutex<V>) -> MutexGuard<'_, V> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The closure's side of a job: the cancel token, the progress sink and the
/// partial-update sink.
pub struct Ctx<U = ()> {
    shared: Arc<dyn Sink<U>>,
}

trait Sink<U>: Send + Sync {
    fn cancel(&self) -> &AtomicBool;
    fn progress(&self) -> &Mutex<Progress>;
    fn updates(&self) -> &Mutex<Vec<U>>;
}
impl<T: Send, U: Send> Sink<U> for Shared<T, U> {
    fn cancel(&self) -> &AtomicBool {
        &self.cancel
    }
    fn progress(&self) -> &Mutex<Progress> {
        &self.progress
    }
    fn updates(&self) -> &Mutex<Vec<U>> {
        &self.updates
    }
}

impl<U> Ctx<U> {
    /// True once the handle was dropped (cancel-on-drop) or cancelled.
    pub fn cancelled(&self) -> bool {
        self.shared.cancel().load(Ordering::Relaxed)
    }
    /// The token itself, for shared-crate functions that take `&AtomicBool`.
    pub fn cancel_flag(&self) -> &AtomicBool {
        self.shared.cancel()
    }
    pub fn fraction(&self, fraction: f64) {
        lock(self.shared.progress()).fraction = Some(fraction);
    }
    pub fn steps(&self, done: u64, total: u64) {
        lock(self.shared.progress()).steps = Some((done, total));
    }
    pub fn message(&self, message: impl Into<String>) {
        lock(self.shared.progress()).message = message.into();
    }
    /// Hands a partial result to the owner (drained by [`Job::updates`]).
    /// Returns false once the job is cancelled, so a streaming loop can stop.
    pub fn emit(&self, update: U) -> bool {
        if self.cancelled() {
            return false;
        }
        lock(self.shared.updates()).push(update);
        true
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OnDrop {
    Cancel,
    Complete,
}

/// The owner's side of a one-shot job. Keep it in the feature's state; poll
/// it from the feature's one apply system. Dropping it cancels the job
/// (sets the token; a pool job that has not started never runs), unless it
/// is [`Self::complete_on_drop`].
pub struct Job<T, U = ()> {
    generation: u64,
    shared: Arc<Shared<T, U>>,
    /// The pool task (None for `Pool::Dedicated` and finished handles).
    /// Dropping a Bevy `Task` cancels it; `detach` lets it run to the end.
    task: Option<Task<()>>,
    on_drop: OnDrop,
}

/// Bevy's default pools, created with Bevy's own defaults when no `App` has
/// made them yet (tests, `--validate-only`, headless, a job started before
/// `App::run`). `TaskPoolOptions::create_default_pools` uses `get_or_init`
/// for each pool, so whichever of this and `TaskPoolPlugin` runs first
/// creates the same pools and the other reuses them.
fn pools() {
    static INIT: Once = Once::new();
    INIT.call_once(|| bevy::app::TaskPoolOptions::default().create_default_pools());
}
fn pool(pool: Pool) -> &'static TaskPool {
    pools();
    match pool {
        Pool::Io => &**IoTaskPool::get(),
        _ => &**AsyncComputeTaskPool::get(),
    }
}

fn panic_text(payload: &(dyn Any + Send)) -> String {
    payload.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into())
}

impl<T: Send + 'static> Job<T> {
    /// Runs `work` once on `pool`. `generation` is the owner's stamp for this
    /// request (a sequence number it reports, or 0). `name` names the job in
    /// a thread name and in the error if `work` panics:
    /// "{name} ended without a result ({panic})."
    pub fn spawn(pool: Pool, generation: u64, name: impl Into<String>, work: impl FnOnce(&Ctx) -> Result<T, String> + Send + 'static) -> Self {
        Self::streaming(pool, generation, name, work)
    }

    /// A handle that has already finished with `result` (a result computed
    /// elsewhere, handed to the same apply path; also used by tests).
    pub fn finished(generation: u64, result: Result<T, String>) -> Self {
        let shared = Arc::new(Shared { cancel: AtomicBool::new(false), progress: Mutex::new(Progress::default()), updates: Mutex::new(Vec::new()), result: Mutex::new(Some(result)) });
        Self { generation, shared, task: None, on_drop: OnDrop::Cancel }
    }
}

impl<T: Send + 'static, U: Send + 'static> Job<T, U> {
    /// [`Job::spawn`] for work that also hands over partial results as it
    /// goes ([`Ctx::emit`], drained by [`Job::updates`] / [`Job::next_update`]).
    pub fn streaming(pool_kind: Pool, generation: u64, name: impl Into<String>, work: impl FnOnce(&Ctx<U>) -> Result<T, String> + Send + 'static) -> Self {
        let name = name.into();
        let shared = Arc::new(Shared { cancel: AtomicBool::new(false), progress: Mutex::new(Progress::default()), updates: Mutex::new(Vec::new()), result: Mutex::new(None) });
        let worker = shared.clone();
        let label = name.clone();
        let run = move || {
            if worker.cancel.load(Ordering::Relaxed) {
                // Never started: an owner still polling gets an answer.
                *lock(&worker.result) = Some(Err(format!("{label} was cancelled before it started.")));
                return;
            }
            let ctx = Ctx { shared: worker.clone() as Arc<dyn Sink<U>> };
            let result = catch_unwind(AssertUnwindSafe(|| work(&ctx))).unwrap_or_else(|p| Err(format!("{label} ended without a result ({}).", panic_text(p.as_ref()))));
            *lock(&worker.result) = Some(result);
        };
        let task = match pool_kind {
            Pool::Io | Pool::Compute => Some(pool(pool_kind).spawn(async move { run() })),
            Pool::Dedicated => {
                if let Err(e) = std::thread::Builder::new().name(thread_name(&name)).spawn(run) {
                    *lock(&shared.result) = Some(Err(format!("could not start {name}: {e}")));
                }
                None
            }
        };
        Self { generation, shared, task, on_drop: OnDrop::Cancel }
    }

    /// Saves and writes: the job runs to its end even if this handle is
    /// dropped (its result is then discarded). The cancel token is never set
    /// by a drop; [`Self::cancel`] still sets it explicitly.
    pub fn complete_on_drop(mut self) -> Self {
        self.on_drop = OnDrop::Complete;
        self
    }
}

impl<T, U> Job<T, U> {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// The finished result, once (None while running, and after it was taken).
    pub fn poll(&self) -> Option<Result<T, String>> {
        lock(&self.shared.result).take()
    }
    /// Partial updates emitted since the last call, oldest first.
    pub fn updates(&self) -> Vec<U> {
        std::mem::take(&mut *lock(&self.shared.updates))
    }
    /// The oldest partial update not yet taken (one at a time).
    pub fn next_update(&self) -> Option<U> {
        let mut updates = lock(&self.shared.updates);
        (!updates.is_empty()).then(|| updates.remove(0))
    }
    pub fn progress(&self) -> Progress {
        lock(&self.shared.progress).clone()
    }
    /// Asks the closure to stop (it sees [`Ctx::cancelled`]).
    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
    }
}

impl<T, U> Drop for Job<T, U> {
    fn drop(&mut self) {
        match self.on_drop {
            OnDrop::Cancel => {
                self.shared.cancel.store(true, Ordering::Relaxed);
                // Dropping the task cancels it if it has not started.
                drop(self.task.take());
            }
            OnDrop::Complete => {
                if let Some(task) = self.task.take() {
                    task.detach();
                }
            }
        }
    }
}

/// The latest request of one kind: starting a job replaces (and so cancels)
/// the previous one, and only the newest generation's result is ever
/// returned. For features whose requests supersede each other.
pub struct Latest<T, U = ()> {
    job: Option<Job<T, U>>,
    generation: u64,
}
impl<T, U> Default for Latest<T, U> {
    fn default() -> Self {
        Self { job: None, generation: 0 }
    }
}
impl<T: Send + 'static> Latest<T> {
    /// Starts generation `self.generation() + 1`, dropping the previous job.
    pub fn start(&mut self, pool: Pool, name: impl Into<String>, work: impl FnOnce(&Ctx) -> Result<T, String> + Send + 'static) -> u64 {
        self.generation += 1;
        self.job = Some(Job::spawn(pool, self.generation, name, work));
        self.generation
    }
}
impl<T, U> Latest<T, U> {
    /// The newest job's result with its generation, once.
    pub fn poll(&mut self) -> Option<(u64, Result<T, String>)> {
        let job = self.job.as_mut()?;
        let result = job.poll()?;
        let generation = job.generation();
        self.job = None;
        Some((generation, result))
    }
    /// The generation still running, if any.
    pub fn pending(&self) -> Option<u64> {
        self.job.as_ref().map(Job::generation)
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn job(&self) -> Option<&Job<T, U>> {
        self.job.as_ref()
    }
    /// Drops the running job (cancelled; its result is never returned).
    pub fn cancel(&mut self) -> Option<u64> {
        self.job.take().map(|j| j.generation())
    }
}

fn thread_name(name: &str) -> String {
    // Thread names are for debuggers and logs; keep them short.
    name.chars().take(48).collect()
}

/// Waits for a child process we started and do not otherwise wait for, so it
/// never lingers as a zombie; its lifetime is not tied to ours.
pub fn reap_child(mut child: std::process::Child, name: &str) {
    let spawned = std::thread::Builder::new().name(thread_name(&format!("reap {name}"))).spawn(move || {
        let _ = child.wait();
    });
    if let Err(e) = spawned {
        bevy::log::warn!("could not start a thread to reap {name}: {e}; it is reaped when this process exits");
    }
}

/// Drops `value` on its own thread: for values whose drop is slow or joins
/// other threads (a replaced builder and its workers), never on the UI thread.
/// If no thread can be started, it is dropped here.
pub fn drop_off_thread<T: Send + 'static>(value: T, name: &str) {
    let _ = std::thread::Builder::new().name(thread_name(&format!("drop {name}"))).spawn(move || drop(value));
}
