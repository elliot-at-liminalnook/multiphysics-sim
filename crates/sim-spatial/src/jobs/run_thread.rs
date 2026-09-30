//! One long-lived worker: a named OS thread with a command channel in and a
//! shared snapshot out. The worker's loop is its own (pacing, physics calls
//! through the shared runtime, what it publishes); this type owns only the
//! thread, the channel, the snapshot hand-off and shutdown.
use std::sync::{Arc, Condvar, Mutex, MutexGuard, mpsc};
use std::time::{Duration, Instant};

use super::lock;

/// How long dropping a [`RunThread`] waits for its worker to notice the
/// closed channel and exit. Every worker loop checks its channel between
/// ticks (an idle one is blocked on it and wakes at once); past the bound the
/// thread is detached with a warning and exits at its next check, so the UI
/// never waits longer than this.
pub const JOIN_BOUND: Duration = Duration::from_millis(200);

/// A snapshot stamped with the generation of the command it follows.
pub trait Stamped {
    fn generation(&self) -> u64;
}

/// Sending to a worker that has stopped (or never had a thread).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stopped;

type Exit = Arc<(Mutex<bool>, Condvar)>;

/// Set when the worker's closure returns or unwinds.
struct Exited(Exit);
impl Drop for Exited {
    fn drop(&mut self) {
        *lock(&self.0.0) = true;
        self.0.1.notify_all();
    }
}

pub struct RunThread<Cmd, S> {
    name: String,
    commands: Option<mpsc::Sender<Cmd>>,
    shared: Arc<Mutex<S>>,
    thread: Option<(std::thread::JoinHandle<()>, Exit)>,
    bound: Duration,
}

impl<Cmd: Send + 'static, S: Send + 'static> RunThread<Cmd, S> {
    /// Starts `body` on a thread called `name` with the command receiver and
    /// the shared snapshot (initially `initial`). A closed channel
    /// (`recv`/`try_recv` disconnected) is the stop signal: the body returns.
    pub fn spawn(name: &str, initial: S, body: impl FnOnce(mpsc::Receiver<Cmd>, Arc<Mutex<S>>) + Send + 'static) -> Self {
        let (commands, receiver) = mpsc::channel();
        let shared = Arc::new(Mutex::new(initial));
        let out = shared.clone();
        let exit: Exit = Arc::new((Mutex::new(false), Condvar::new()));
        let flag = Exited(exit.clone());
        let handle = std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                let _flag = flag;
                body(receiver, out);
            })
            .unwrap_or_else(|e| panic!("spawn {name} thread: {e}"));
        Self { name: name.to_string(), commands: Some(commands), shared, thread: Some((handle, exit)), bound: JOIN_BOUND }
    }
    /// A snapshot with no thread behind it: sends fail with [`Stopped`].
    pub fn idle(name: &str, initial: S) -> Self {
        Self { name: name.to_string(), commands: None, shared: Arc::new(Mutex::new(initial)), thread: None, bound: JOIN_BOUND }
    }
    /// Waits at most `bound` on drop (instead of [`JOIN_BOUND`]); zero never waits.
    pub fn join_bound(mut self, bound: Duration) -> Self {
        self.bound = bound;
        self
    }
}

impl<Cmd, S> RunThread<Cmd, S> {
    pub fn send(&self, command: Cmd) -> Result<(), Stopped> {
        self.commands.as_ref().ok_or(Stopped)?.send(command).map_err(|_| Stopped)
    }
    /// Another sender for the same worker (the worker stops only once every
    /// sender is gone).
    pub fn sender(&self) -> Option<mpsc::Sender<Cmd>> {
        self.commands.clone()
    }
    pub fn shared(&self) -> &Arc<Mutex<S>> {
        &self.shared
    }
    /// The published snapshot (a poisoned lock still yields the last value).
    pub fn lock(&self) -> MutexGuard<'_, S> {
        lock(&self.shared)
    }
    /// The snapshot, only if it follows command `generation` or later: an
    /// older one is stale and never applied.
    pub fn latest(&self, generation: u64) -> Option<S>
    where
        S: Stamped + Clone,
    {
        let s = self.lock();
        (s.generation() >= generation).then(|| s.clone())
    }
    /// The worker has returned (or there never was one).
    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|(_, exit)| *lock(&exit.0))
    }
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl<Cmd, S> Drop for RunThread<Cmd, S> {
    fn drop(&mut self) {
        // Closing the channel is the stop signal.
        self.commands = None;
        let Some((handle, exit)) = self.thread.take() else { return };
        let started = Instant::now();
        let (done, wake) = &*exit;
        let guard = lock(done);
        let (guard, _) = wake.wait_timeout_while(guard, self.bound, |exited| !*exited).unwrap_or_else(|p| p.into_inner());
        let exited = *guard;
        drop(guard);
        if exited {
            let _ = handle.join();
        } else if !self.bound.is_zero() {
            bevy::log::warn!("{} did not stop within {:?} of its channel closing; detached (it exits at its next command check)", self.name, started.elapsed());
        }
    }
}
