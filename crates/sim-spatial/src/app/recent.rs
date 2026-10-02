//! Recent-document value types and legacy path resolution.
//! Persistence, migration and canonicalization jobs belong to `app::settings`.
use super::ViewerMode;
use super::switch::Document;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The file's schema version. Bump it when an entry gains a variant or
/// field an older viewer cannot read. The settings owner refuses newer schemas.
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
    config_dir_from(|k| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from), cfg!(target_os = "macos"))
}

pub(crate) fn config_dir_from(var: impl Fn(&str)->Option<PathBuf>, macos: bool) -> Option<PathBuf> {
    if let Some(dir) = var("SIM_SPATIAL_CONFIG_DIR") {
        return Some(dir);
    }
    if let Some(xdg) = var("XDG_CONFIG_HOME") {
        return Some(xdg.join("sim-spatial"));
    }
    let home = var("HOME")?;
    Some(if macos { home.join("Library/Application Support/sim-spatial") } else { home.join(".config/sim-spatial") })
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
