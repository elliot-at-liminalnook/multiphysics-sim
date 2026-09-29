//! Revisioned sidecar files. Every edit takes a file lock, re-reads the file,
//! checks the caller's expected revision, applies one command, validates and
//! writes atomically (temp file + rename). Several processes (both viewers,
//! REST clients, the CLI) can share one file; a stale edit is rejected with a
//! "revision conflict" error instead of overwriting someone else's work.
//!
//! [`Store`] does the file work on a background thread so the UI never
//! blocks on disk; it also re-reads the file while idle to pick up edits made
//! elsewhere.
use serde::{Serialize, de::DeserializeOwned};

/// A document with a monotonically increasing revision and one command type.
pub trait Revisioned: Clone + Serialize + DeserializeOwned + Send + 'static {
    type Command: Send + 'static;
    /// What the document is validated against (a model description, a subject ID).
    type Context: Send + Sync + 'static;
    fn empty(context: &Self::Context) -> Self;
    fn revision(&self) -> u64;
    fn validate(&self, context: &Self::Context) -> Result<(), String>;
    fn apply(&mut self, command: Self::Command, context: &Self::Context) -> Result<(), String>;
}

/// Largest sidecar accepted or written.
pub const MAX_BYTES: usize = 4_194_304;

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::{
        collections::BTreeMap,
        fs::{self, File, OpenOptions, TryLockError},
        io::{Read, Write},
        path::{Path, PathBuf},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    /// Read and validate; a missing file is an empty document.
    pub fn read<D: Revisioned>(path: &Path, context: &D::Context) -> Result<D, String> {
        let mut bytes = Vec::new();
        match File::open(path) {
            Ok(f) => {
                f.take(MAX_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(D::empty(context)),
            Err(e) => return Err(e.to_string()),
        }
        if bytes.len() > MAX_BYTES {
            return Err("annotation file exceeds 4 MiB".into());
        }
        let doc: D = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        doc.validate(context)?;
        Ok(doc)
    }

    /// One locked read-check-apply-write transaction.
    pub fn edit<D: Revisioned>(path: &Path, context: &D::Context, command: D::Command, expected: Option<u64>) -> Result<D, String> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(format!("{}.lock", path.display()))
            .map_err(|e| e.to_string())?;
        let mut acquired = false;
        for _ in 0..20 {
            match lock.try_lock() {
                Ok(()) => {
                    acquired = true;
                    break;
                }
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => return Err(e.to_string()),
            }
        }
        if !acquired {
            return Err("annotation file is busy; retry with a fresh revision".into());
        }
        let mut doc: D = read(path, context)?;
        if expected.is_some_and(|r| r != doc.revision()) {
            return Err("annotation revision conflict; read annotations and merge edits".into());
        }
        doc.apply(command, context)?;
        let bytes = serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_BYTES {
            return Err("annotation file exceeds 4 MiB".into());
        }
        write_atomic(path, &bytes)?;
        Ok(doc)
    }

    /// Temp file in the same directory, fsync, rename.
    pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
        let tmp = PathBuf::from(format!(
            "{}.{}.{}.tmp",
            path.display(),
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()
        ));
        let result = (|| -> std::io::Result<()> {
            let mut f = OpenOptions::new().create_new(true).write(true).open(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
            fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result.map_err(|e| e.to_string())
    }

    struct Request<C> {
        id: u64,
        command: C,
        expected: Option<u64>,
    }
    struct State<D> {
        document: D,
        error: Option<String>,
        results: BTreeMap<u64, Result<D, String>>,
    }

    /// Background-thread file store: `submit` returns a request ID at once;
    /// `result(id)` yields the outcome when the worker finishes.
    pub struct Store<D: Revisioned> {
        state: Arc<Mutex<State<D>>>,
        tx: mpsc::SyncSender<Request<D::Command>>,
        next: u64,
        stop: Arc<AtomicBool>,
        pub path: PathBuf,
    }
    impl<D: Revisioned> Store<D> {
        pub fn new(context: Arc<D::Context>, path: PathBuf) -> Self {
            let state = Arc::new(Mutex::new(State { document: D::empty(&context), error: None, results: BTreeMap::new() }));
            let worker = state.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let stopping = stop.clone();
            let (tx, rx) = mpsc::sync_channel::<Request<D::Command>>(32);
            let file = path.clone();
            std::thread::spawn(move || {
                while !stopping.load(Ordering::Relaxed) {
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(request) => {
                            let id = request.id;
                            let result = edit::<D>(&file, &context, request.command, request.expected);
                            let mut s = worker.lock().unwrap();
                            if let Ok(doc) = &result {
                                s.document = doc.clone();
                                s.error = None;
                            } else {
                                s.error = result.as_ref().err().cloned();
                            }
                            if s.results.len() >= 64 {
                                s.results.pop_first();
                            }
                            s.results.insert(id, result);
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => match read::<D>(&file, &context) {
                            Ok(doc) => {
                                let mut s = worker.lock().unwrap();
                                if s.document.revision() != doc.revision() {
                                    s.error = None;
                                }
                                s.document = doc;
                            }
                            Err(e) => worker.lock().unwrap().error = Some(e),
                        },
                    }
                }
            });
            Self { state, tx, next: 1, stop, path }
        }
        pub fn document(&self) -> D {
            self.state.lock().unwrap().document.clone()
        }
        pub fn revision(&self) -> u64 {
            self.state.lock().unwrap().document.revision()
        }
        pub fn error(&self) -> Option<String> {
            self.state.lock().unwrap().error.clone()
        }
        pub fn submit(&mut self, command: D::Command, expected: Option<u64>) -> Result<u64, String> {
            let id = self.next;
            self.tx.try_send(Request { id, command, expected }).map_err(|e| e.to_string())?;
            self.next += 1;
            Ok(id)
        }
        pub fn result(&mut self, id: u64) -> Option<Result<D, String>> {
            self.state.lock().unwrap().results.remove(&id)
        }
    }
    impl<D: Revisioned> Drop for Store<D> {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }
}
