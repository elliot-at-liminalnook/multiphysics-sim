//! Bounded latest-value transport in an existing private selection-session directory.
//! Only background threads serialize, validate, access files or acquire leases.
use super::*;
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
const MAX_BYTES: u64 = 1_048_576;
const POLL: Duration = Duration::from_millis(33);
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    revision: u64,
    snapshot: LiveSnapshot,
}
#[derive(Default)]
struct Outbox {
    latest: Option<(u64, Arc<LiveSnapshot>)>,
    stop: bool,
    error: Option<String>,
}
pub struct Publisher {
    mailbox: Arc<Mutex<Outbox>>,
    revision: u64,
}
impl Publisher {
    pub fn new(directory: PathBuf) -> Self {
        let mailbox = Arc::new(Mutex::new(Outbox::default()));
        let worker = mailbox.clone();
        thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                let lease = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(directory.join("live.lease"))
                    .map_err(|e| e.to_string())?;
                lease
                    .try_lock()
                    .map_err(|e| format!("live publisher already attached: {e}"))?;
                let mut written = 0;
                loop {
                    let pending = {
                        let m = worker.lock().unwrap();
                        if m.stop {
                            break;
                        }
                        m.latest.clone()
                    };
                    if let Some((revision, snapshot)) = pending.filter(|(r, _)| *r != written) {
                        let bytes = serde_json::to_vec(&Envelope {
                            revision,
                            snapshot: (*snapshot).clone(),
                        })
                        .map_err(|e| e.to_string())?;
                        if bytes.len() as u64 > MAX_BYTES {
                            return Err("live snapshot exceeds 1 MiB limit".into());
                        }
                        let mut f = File::create(directory.join("live.pending"))
                            .map_err(|e| e.to_string())?;
                        f.write_all(&bytes).map_err(|e| e.to_string())?;
                        drop(f);
                        fs::rename(directory.join("live.pending"), directory.join("live.json"))
                            .map_err(|e| e.to_string())?;
                        written = revision;
                    }
                    thread::sleep(POLL);
                }
                Ok(())
            })();
            if let Err(e) = result {
                worker.lock().unwrap().error = Some(e);
            }
        });
        Self {
            mailbox,
            revision: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
        }
    }
    /// Coalesces to one snapshot. Call periodically even when paused (heartbeat).
    pub fn publish(&mut self, snapshot: LiveSnapshot) -> Result<(), String> {
        let mut m = self.mailbox.lock().unwrap();
        if let Some(e) = &m.error {
            return Err(e.clone());
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or("live revision exhausted")?;
        m.latest = Some((self.revision, Arc::new(snapshot)));
        Ok(())
    }
}
impl Drop for Publisher {
    fn drop(&mut self) {
        self.mailbox.lock().unwrap().stop = true;
    }
}
#[derive(Clone, Default)]
pub struct Received {
    pub revision: u64,
    pub snapshot: Option<Arc<LiveSnapshot>>,
    pub error: Option<String>,
    pub received_at: Option<Instant>,
}
#[derive(Default)]
struct Inbox {
    value: Received,
    stop: bool,
}
pub struct Subscriber {
    mailbox: Arc<Mutex<Inbox>>,
}
impl Subscriber {
    pub fn new(source: Arc<SystemDescription>, directory: PathBuf) -> Self {
        let mailbox = Arc::new(Mutex::new(Inbox::default()));
        let worker = mailbox.clone();
        thread::spawn(move || {
            let mut gate = LiveGate::default();
            let mut revision = 0;
            loop {
                if worker.lock().unwrap().stop {
                    break;
                }
                let result = (|| -> Result<Option<Envelope>, String> {
                    let lease = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create(true)
                        .truncate(false)
                        .open(directory.join("live.lease"))
                        .map_err(|e| e.to_string())?;
                    match lease.try_lock() {
                        Ok(()) => {
                            return Err("Simulation view disconnected — holding last sample".into());
                        }
                        Err(TryLockError::WouldBlock) => {}
                        Err(e) => return Err(e.to_string()),
                    }
                    let mut bytes = Vec::new();
                    File::open(directory.join("live.json"))
                        .map_err(|e| e.to_string())?
                        .take(MAX_BYTES + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|e| e.to_string())?;
                    if bytes.len() as u64 > MAX_BYTES {
                        return Err("oversize live snapshot".into());
                    }
                    let envelope: Envelope =
                        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                    if envelope.revision < revision {
                        return Err("live transport revision regressed".into());
                    }
                    if envelope.revision == revision {
                        return Ok(None);
                    }
                    gate.accept(&source, &envelope.snapshot)
                        .map_err(|e| e.to_string())?;
                    Ok(Some(envelope))
                })();
                let mut m = worker.lock().unwrap();
                match result {
                    Ok(Some(e)) => {
                        revision = e.revision;
                        m.value = Received {
                            revision,
                            snapshot: Some(Arc::new(e.snapshot)),
                            error: None,
                            received_at: Some(Instant::now()),
                        };
                    }
                    Ok(None) => {}
                    Err(e) => m.value.error = Some(e),
                }
                drop(m);
                thread::sleep(POLL);
            }
        });
        Self { mailbox }
    }
    pub fn latest(&self) -> Received {
        let mut value = self.mailbox.lock().unwrap().value.clone();
        if value
            .received_at
            .is_some_and(|t| t.elapsed() > Duration::from_secs(2))
        {
            value
                .error
                .get_or_insert_with(|| "Live data stale — holding last sample".into());
        }
        value
    }
}
impl Drop for Subscriber {
    fn drop(&mut self) {
        self.mailbox.lock().unwrap().stop = true;
    }
}
