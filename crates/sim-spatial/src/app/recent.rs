//! Recent documents (window-first-usability): what each mode last opened
//! through a mode switch, for the document picker (`app::picker`).
//!
//! - **One file per user**, never in the repository: `recent.json` in the
//!   viewer's config directory ([`file`]: `$SIM_SPATIAL_CONFIG_DIR`, else
//!   `$XDG_CONFIG_HOME/sim-spatial`, else on macOS
//!   `~/Library/Application Support/sim-spatial`, else
//!   `~/.config/sim-spatial`).
//! - **Versioned** ([`VERSION`]): the picker reads a file of another
//!   version, one that does not parse or one it cannot read as empty, so a
//!   newer viewer's file never breaks an older one. A record ([`record_in`])
//!   replaces only a missing file, one that does not parse as JSON, or one of
//!   an older version; a newer version's file, or one that could not be read
//!   (other than missing), is left as it was and the record fails, so an
//!   older viewer never erases a newer viewer's recent documents.
//! - **Written atomically** ([`save`]): a temporary file in the same
//!   directory, flushed, then renamed over the old one; a failed write
//!   leaves the old file whole.
//! - **Off the UI thread**: [`record_job`] (load, record, save) runs on
//!   `Pool::Io`; the picker reads it on its discovery job. One process-wide
//!   lock orders concurrent records.
use super::ViewerMode;
use super::switch::Document;
use crate::jobs::{Job, Pool};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// The file's schema version.
pub const VERSION: u32 = 1;
/// Documents kept per mode, newest first.
pub const KEEP: usize = 10;
/// The file's name in the config directory.
pub const FILE_NAME: &str = "recent.json";

/// One recent document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recent {
    Path(PathBuf),
    Preset(String),
    Url(String),
}
impl Recent {
    pub fn from_document(d: &Document) -> Self {
        match d {
            Document::Path(p) => Recent::Path(p.clone()),
            Document::Preset(id) => Recent::Preset(id.clone()),
            Document::Url(url) => Recent::Url(url.clone()),
        }
    }
    pub fn document(&self) -> Document {
        match self {
            Recent::Path(p) => Document::Path(p.clone()),
            Recent::Preset(id) => Document::Preset(id.clone()),
            Recent::Url(url) => Document::Url(url.clone()),
        }
    }
}

/// One mode's entry: the document and when it was opened (Unix seconds).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub document: Recent,
    pub opened: u64,
}

/// The file's contents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recents {
    pub version: u32,
    /// By mode name (`ViewerMode::name`), newest first, at most [`KEEP`].
    pub modes: BTreeMap<String, Vec<Entry>>,
}
impl Default for Recents {
    fn default() -> Self {
        Recents { version: VERSION, modes: BTreeMap::new() }
    }
}
impl Recents {
    /// `document` opened in `mode` at `now`: moved to the front, once, at most [`KEEP`].
    pub fn record(&mut self, mode: ViewerMode, document: &Document, now: u64) {
        let doc = Recent::from_document(document);
        let list = self.modes.entry(mode.name().to_string()).or_default();
        list.retain(|e| e.document != doc);
        list.insert(0, Entry { document: doc, opened: now });
        list.truncate(KEEP);
    }
    /// `mode`'s recent documents, newest first.
    pub fn list(&self, mode: ViewerMode) -> Vec<Document> {
        self.modes.get(mode.name()).map_or_else(Vec::new, |l| l.iter().map(|e| e.document.document()).collect())
    }
}

/// The viewer's config directory (see the module doc), or None without a home.
pub fn config_dir() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = var("SIM_SPATIAL_CONFIG_DIR") {
        return Some(dir);
    }
    if let Some(xdg) = var("XDG_CONFIG_HOME") {
        return Some(xdg.join("sim-spatial"));
    }
    let home = var("HOME")?;
    Some(if cfg!(target_os = "macos") { home.join("Library/Application Support/sim-spatial") } else { home.join(".config/sim-spatial") })
}

/// `recent.json` in [`config_dir`]. None in the lib tests, so no test reads
/// or writes the user's own file (they call [`record_in`] and [`load`] on a
/// temporary one).
pub fn file() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    config_dir().map(|d| d.join(FILE_NAME))
}

/// What reading the file found.
enum Loaded {
    /// This version's contents.
    Current(Recents),
    /// Missing, not JSON of this schema, or an older version: a record may replace it.
    Replaceable,
    /// A newer version, or a read error other than missing: a record must
    /// leave it alone (the reason).
    Keep(String),
}

fn read(path: &Path) -> Loaded {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded::Replaceable,
        Err(e) => return Loaded::Keep(format!("{}: {e}; not overwriting it", path.display())),
    };
    // The version alone first, so a newer schema whose entries this viewer
    // cannot parse is still recognised as newer.
    #[derive(Deserialize)]
    struct Version {
        version: u32,
    }
    match serde_json::from_slice::<Version>(&bytes) {
        Ok(v) if v.version > VERSION => Loaded::Keep(format!("{}: written by a newer viewer (version {}, this one reads {VERSION}); not overwriting it", path.display(), v.version)),
        Ok(v) if v.version == VERSION => serde_json::from_slice::<Recents>(&bytes).map_or(Loaded::Replaceable, Loaded::Current),
        _ => Loaded::Replaceable,
    }
}

/// Reads `path`: empty when missing, unreadable, not JSON, or of another version.
pub fn load(path: &Path) -> Recents {
    match read(path) {
        Loaded::Current(r) => r,
        Loaded::Replaceable | Loaded::Keep(_) => Recents::default(),
    }
}

/// Temporary-file names, unique in the process.
static TMP: AtomicU64 = AtomicU64::new(0);

/// Writes `recents` to `path` atomically: a temporary file beside it,
/// flushed to disk, renamed over `path`. On an error the temporary file is
/// removed and `path` is left as it was.
pub fn save(path: &Path, recents: &Recents) -> Result<(), String> {
    let dir = path.parent().ok_or_else(|| format!("{}: no parent directory", path.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".{FILE_NAME}.{}.{}.tmp", std::process::id(), TMP.fetch_add(1, Ordering::Relaxed)));
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&serde_json::to_vec_pretty(recents).map_err(std::io::Error::other)?)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    };
    write().map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

/// Orders load-record-save across jobs in this process.
static LOCK: Mutex<()> = Mutex::new(());

/// Records `document` for `mode` in `path` (load, record, save) under the
/// lock. A newer version's file, or one that could not be read (other than
/// missing), is left untouched: Err names why.
pub fn record_in(path: &Path, mode: ViewerMode, document: &Document, now: u64) -> Result<(), String> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut recents = match read(path) {
        Loaded::Current(r) => r,
        Loaded::Replaceable => Recents::default(),
        Loaded::Keep(why) => return Err(why),
    };
    recents.record(mode, document, now);
    save(path, &recents)
}

/// The document as recorded: a path made absolute (relative to the
/// current directory, as the switch opened it), canonical when it exists.
pub fn absolute(document: &Document) -> Document {
    match document {
        Document::Path(p) => {
            let abs = if p.is_absolute() { p.clone() } else { std::env::current_dir().map(|d| d.join(p)).unwrap_or_else(|_| p.clone()) };
            Document::Path(std::fs::canonicalize(&abs).unwrap_or(abs))
        }
        other => other.clone(),
    }
}

/// A successful open through the switch, recorded on `Pool::Io` (runs to
/// the end when its handle is dropped). Without a config directory nothing
/// is recorded, nor in the lib tests ([`file`] is None there).
pub fn record_job(mode: ViewerMode, document: Document) -> Option<Job<()>> {
    let path = file()?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    Some(
        Job::spawn(Pool::Io, 0, "recent documents", move |_| {
            let document = absolute(&document);
            record_in(&path, mode, &document, now).inspect_err(|e| bevy::log::warn!("recent documents: {e}"))
        })
        .complete_on_drop(),
    )
}
