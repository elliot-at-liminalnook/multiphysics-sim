use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

/// Original bytes and every decompressed entry remain owned and unmodified.
/// Unknown JSON and opaque entries are deliberately not projected into a lossy schema.
#[derive(Clone, Debug)]
pub struct ArchiveDocument {
    pub path: PathBuf,
    pub manifest: Value,
    pub entries: Vec<(String, Vec<u8>)>,
    pub original_bytes: Vec<u8>,
    pub model_identity: String,
    pub effective_nodes: HashMap<String, Value>,
    pub(crate) recipes: HashMap<String, crate::component::Recipe>,
}
impl ArchiveDocument {
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::open_with(path, &|| false, &|_| {})
    }
    pub fn open_with(
        path: &Path,
        cancelled: &dyn Fn() -> bool,
        progress: &dyn Fn(&str),
    ) -> Result<Self, String> {
        progress("Reading archive");
        let bytes = std::fs::read(path).map_err(|e| format!("{}: archive: {e}", path.display()))?;
        Self::from_bytes(path, bytes, cancelled, progress)
    }
    /// An archive from its bytes, as if read from `path` (edits and undo
    /// rebuild documents this way without touching the file).
    pub fn from_bytes(
        path: &Path,
        bytes: Vec<u8>,
        cancelled: &dyn Fn() -> bool,
        progress: &dyn Fn(&str),
    ) -> Result<Self, String> {
        let model_identity = format!("sha256:{:x}", Sha256::digest(&bytes));
        let mut z = zip::ZipArchive::new(Cursor::new(&bytes))
            .map_err(|e| format!("{}: zip: {e}", path.display()))?;
        let mut entries = Vec::new();
        let mut names = HashSet::new();
        for i in 0..z.len() {
            if cancelled() {
                return Err(format!("{}: archive loading cancelled", path.display()));
            }
            let mut f = z
                .by_index(i)
                .map_err(|e| format!("{}: archive entry {i}: {e}", path.display()))?;
            let name = f.name().to_owned();
            if !names.insert(name.clone()) {
                return Err(format!(
                    "{}: duplicate archive entry {name}",
                    path.display()
                ));
            }
            let mut data = Vec::new();
            f.read_to_end(&mut data)
                .map_err(|e| format!("{}: {name}: {e}", path.display()))?;
            entries.push((name, data));
        }
        let raw = entries
            .iter()
            .find(|(n, _)| n == "manifest.json")
            .ok_or_else(|| format!("{}: missing manifest.json", path.display()))?;
        let manifest: Value = serde_json::from_slice(&raw.1)
            .map_err(|e| format!("{}: manifest.json: {e}", path.display()))?;
        if manifest["format"] != "robocad" || manifest["version"] != 1 {
            return Err(format!(
                "{}: manifest.json: unsupported RoboCAD format/version",
                path.display()
            ));
        }
        let nodes = manifest["nodes"]
            .as_array()
            .ok_or_else(|| format!("{}: manifest.json nodes must be an array", path.display()))?;
        let mut effective_nodes = HashMap::new();
        for n in nodes {
            let id = n["id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("{}: manifest.json node has missing id", path.display()))?;
            if effective_nodes.insert(id.to_owned(), n.clone()).is_some() {
                return Err(format!("{}: node {id}: duplicate id", path.display()));
            }
        }
        drop(z);
        let mut doc = Self {
            path: path.into(),
            manifest,
            entries,
            original_bytes: bytes,
            model_identity,
            effective_nodes,
            recipes: HashMap::new(),
        };
        progress("Resolving embedded component definitions");
        crate::component::restore(&mut doc, cancelled, progress)
            .map_err(|e| format!("{}: manifest.json: {e}", path.display()))?;
        if cancelled() {
            return Err(format!(
                "{}: component resolution cancelled",
                path.display()
            ));
        }
        Ok(doc)
    }
    pub fn node(&self, id: &str) -> Option<&Value> {
        self.effective_nodes.get(id)
    }
    pub fn entry(&self, name: &str) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_slice())
    }
    pub fn identity(&self) -> &str {
        &self.model_identity
    }
    pub fn visible(&self, id: &str) -> bool {
        let mut next = Some(id);
        let mut seen = HashSet::new();
        while let Some(id) = next {
            if !seen.insert(id) {
                return false;
            }
            let Some(n) = self.node(id) else {
                return false;
            };
            if n["visible"] == false || n["disabled"] == true {
                return false;
            }
            next = n["parent"].as_str();
        }
        true
    }
}
