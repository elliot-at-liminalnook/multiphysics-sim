//! Native local selection sessions. Each UI talks to a background worker through
//! one coalesced mailbox. A locked, atomically replaced session record orders
//! changes across processes. No file I/O or waits happen in `exchange`.
//!
//! The directory is private and ephemeral. It is separate from CAD and analysis
//! sidecars, survives either window closing, and has no daemon. Advisory leases
//! allow one assembly and one schematic per session and detect closed peers.
use super::SelectionTarget;
use crate::{InspectionError, SystemDescription, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Write},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: u64 = 1_048_576;
const POLL: Duration = Duration::from_millis(20);
static NEXT_SESSION: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Peer {
    Assembly,
    Schematic,
}
impl Peer {
    fn name(self) -> &'static str {
        match self {
            Self::Assembly => "assembly",
            Self::Schematic => "schematic",
        }
    }
    fn other(self) -> Self {
        match self {
            Self::Assembly => Self::Schematic,
            Self::Schematic => Self::Assembly,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionRecord {
    pub version: u32,
    pub description_id: String,
    pub sequence: u64,
    pub origin: Peer,
    pub target: SelectionTarget,
}
impl SelectionRecord {
    fn validate(&self, d: &SystemDescription) -> Result<(), InspectionError> {
        ensure(self.version == 1, "unsupported selection session version")?;
        ensure(
            self.description_id == d.id,
            "selection session belongs to a different model",
        )?;
        self.target.validate(d)
    }
}

fn io_error(e: impl std::fmt::Display) -> InspectionError {
    InspectionError(format!("selection link: {e}"))
}
fn read_record(path: &Path, d: &SystemDescription) -> Result<SelectionRecord, InspectionError> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io_error)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    ensure(
        bytes.len() as u64 <= MAX_BYTES,
        "selection record exceeds size limit",
    )?;
    let record: SelectionRecord = serde_json::from_slice(&bytes).map_err(io_error)?;
    record.validate(d)?;
    Ok(record)
}
fn write_record(dir: &Path, record: &SelectionRecord) -> Result<(), InspectionError> {
    let bytes = serde_json::to_vec(record).map_err(io_error)?;
    ensure(
        bytes.len() as u64 <= MAX_BYTES,
        "selection record exceeds size limit",
    )?;
    let tmp = dir.join("selection.pending");
    // Writers hold the separate stable-inode write.lock while replacing this file.
    let mut f = File::create(&tmp).map_err(io_error)?;
    f.write_all(&bytes).map_err(io_error)?;
    drop(f);
    fs::rename(tmp, dir.join("selection.json")).map_err(io_error)
}
fn lock_file(path: &Path) -> Result<File, InspectionError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(io_error)
}

pub fn create_session(
    d: &SystemDescription,
    initial: SelectionTarget,
) -> Result<PathBuf, InspectionError> {
    d.validate()?;
    initial.validate(d)?;
    let tick = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io_error)?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "sim-selection-{}-{tick}-{}",
        std::process::id(),
        NEXT_SESSION.fetch_add(1, Ordering::Relaxed)
    ));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(io_error)?;
    write_record(
        &dir,
        &SelectionRecord {
            version: 1,
            description_id: d.id.clone(),
            sequence: 0,
            origin: Peer::Assembly,
            target: initial,
        },
    )?;
    Ok(dir)
}

#[derive(Debug, Clone, Default)]
pub struct LinkSnapshot {
    pub record: Option<SelectionRecord>,
    pub peer_online: bool,
    pub acknowledged_request: u64,
    pub error: Option<String>,
}
#[derive(Default)]
struct Mailbox {
    pending: Option<(u64, SelectionTarget)>,
    snapshot: LinkSnapshot,
    stop: bool,
}

pub struct SelectionClient {
    description: Arc<SystemDescription>,
    mailbox: Arc<Mutex<Mailbox>>,
    last_ui: SelectionTarget,
    request: u64,
    last_sequence: Option<u64>,
    pub directory: PathBuf,
}
impl SelectionClient {
    /// Startup validates only the supplied in-memory description. All transport
    /// reads, lease acquisition, publication and peer detection run off the UI.
    pub fn connect(
        d: Arc<SystemDescription>,
        dir: PathBuf,
        peer: Peer,
        initial_ui: SelectionTarget,
    ) -> Result<Self, InspectionError> {
        d.validate()?;
        initial_ui.validate(&d)?;
        let mailbox = Arc::new(Mutex::new(Mailbox::default()));
        let worker_mailbox = mailbox.clone();
        let worker_dir = dir.clone();
        let worker_description = d.clone();
        thread::Builder::new()
            .name(format!("{}-selection", peer.name()))
            .spawn(move || {
                if let Err(e) = work(&worker_description, &worker_dir, peer, &worker_mailbox) {
                    worker_mailbox.lock().unwrap().snapshot.error = Some(e.to_string());
                }
            })
            .map_err(io_error)?;
        Ok(Self {
            description: d,
            mailbox,
            last_ui: initial_ui,
            request: 0,
            last_sequence: None,
            directory: dir,
        })
    }
    pub fn description_id(&self) -> &str {
        &self.description.id
    }
    pub fn snapshot(&self) -> LinkSnapshot {
        self.mailbox.lock().unwrap().snapshot.clone()
    }
    /// Feed the current local UI selection and apply the returned selection.
    /// Local edits supersede older received frames until their request is
    /// acknowledged. Applying a remote result does not publish an echo.
    pub fn exchange(&mut self, local: SelectionTarget) -> Result<SelectionTarget, InspectionError> {
        local.validate(&self.description)?;
        if local != self.last_ui {
            self.request = self
                .request
                .checked_add(1)
                .ok_or_else(|| io_error("request counter exhausted"))?;
            self.mailbox.lock().unwrap().pending = Some((self.request, local.clone()));
            self.last_ui = local;
            return Ok(self.last_ui.clone());
        }
        let snapshot = self.snapshot();
        if snapshot.acknowledged_request >= self.request {
            if let Some(record) = snapshot.record {
                if self.last_sequence.is_none_or(|seq| record.sequence > seq) {
                    self.last_sequence = Some(record.sequence);
                    self.last_ui = record.target;
                }
            }
        }
        Ok(self.last_ui.clone())
    }
    pub fn status(&self, other: &str) -> String {
        let s = self.snapshot();
        if let Some(error) = s.error {
            format!("Link unavailable: {error}")
        } else if s.peer_online {
            format!("Linked to {other}")
        } else {
            format!("Waiting for {other}")
        }
    }
}
impl Drop for SelectionClient {
    fn drop(&mut self) {
        self.mailbox.lock().unwrap().stop = true;
    }
}

fn work(
    d: &SystemDescription,
    dir: &Path,
    peer: Peer,
    mailbox: &Mutex<Mailbox>,
) -> Result<(), InspectionError> {
    // Reject a foreign/corrupt session before claiming a role or writing anything.
    read_record(&dir.join("selection.json"), d)?;
    let lease = lock_file(&dir.join(format!("{}.lease", peer.name())))?;
    // A peer briefly locks this inode while probing liveness. Allow that
    // transient probe to finish before treating contention as a duplicate role.
    let mut acquired = false;
    for _ in 0..20 {
        match lease.try_lock() {
            Ok(()) => {
                acquired = true;
                break;
            }
            Err(TryLockError::WouldBlock) => thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(io_error(e)),
        }
    }
    ensure(
        acquired,
        &format!(
            "{} window already attached or lease unavailable",
            peer.name()
        ),
    )?;
    let other_lease = lock_file(&dir.join(format!("{}.lease", peer.other().name())))?;
    let writer = lock_file(&dir.join("write.lock"))?;
    let mut pending = None;
    let mut acknowledged = 0;
    let mut last_sequence = 0;
    loop {
        {
            let mut m = mailbox.lock().unwrap();
            if m.stop {
                break;
            }
            if let Some(new) = m.pending.take() {
                pending = Some(new);
            }
        }
        let peer_online = match other_lease.try_lock() {
            Ok(()) => {
                other_lease.unlock().map_err(io_error)?;
                false
            }
            Err(TryLockError::WouldBlock) => true,
            Err(e) => return Err(io_error(e)),
        };
        let result = (|| -> Result<SelectionRecord, InspectionError> {
            let mut record = read_record(&dir.join("selection.json"), d)?;
            ensure(
                record.sequence >= last_sequence,
                "selection session sequence moved backwards",
            )?;
            if let Some((request, target)) = &pending {
                match writer.try_lock() {
                    Ok(()) => {
                        let written = (|| {
                            let mut record = read_record(&dir.join("selection.json"), d)?;
                            ensure(
                                record.sequence >= last_sequence,
                                "selection session sequence moved backwards",
                            )?;
                            if &record.target != target {
                                record.sequence = record
                                    .sequence
                                    .checked_add(1)
                                    .ok_or_else(|| io_error("sequence exhausted"))?;
                                record.origin = peer;
                                record.target = target.clone();
                                write_record(dir, &record)?;
                            }
                            Ok::<_, InspectionError>(record)
                        })();
                        writer.unlock().map_err(io_error)?;
                        record = written?;
                        acknowledged = *request;
                        pending = None;
                    }
                    Err(TryLockError::WouldBlock) => {}
                    Err(e) => return Err(io_error(e)),
                }
            }
            Ok(record)
        })();
        let mut m = mailbox.lock().unwrap();
        m.snapshot.peer_online = peer_online;
        m.snapshot.acknowledged_request = acknowledged;
        match result {
            Ok(record) => {
                last_sequence = record.sequence;
                m.snapshot.record = Some(record);
                m.snapshot.error = None;
            }
            Err(e) => {
                m.snapshot.error = Some(e.to_string());
            }
        }
        drop(m);
        thread::sleep(POLL);
    }
    drop(lease); // OS releases the role even when a whole process terminates.
    Ok(())
}
