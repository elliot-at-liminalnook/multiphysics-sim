//! Copies only hash-declared files into exclusively created workspace roots.
use super::contract::{Manifest, SCHEMA_VERSION, SourceFile};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
pub const MAX_FILE: u64 = 64 * 1024 * 1024;
pub fn relative(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    if p.as_os_str().is_empty() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(format!("unsafe relative path: {path}"));
    }
    Ok(p.to_owned())
}
pub fn absolute_no_symlinks(path: &Path) -> Result<(), String> {
    let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
    let mut at = PathBuf::new();
    for c in absolute.components() {
        at.push(c);
        if fs::symlink_metadata(&at)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err(format!("symlink ancestor: {}", at.display()));
        }
    }
    Ok(())
}
pub fn read_checked(root: &Path, file: &SourceFile) -> Result<Vec<u8>, String> {
    let rel = relative(&file.path)?;
    let bytes = super::owned_path::Directory::open(root)?.read(&rel, MAX_FILE)?;
    if format!("{:x}", Sha256::digest(&bytes)) != file.sha256 {
        return Err(format!("{}: source digest mismatch", file.path));
    }
    Ok(bytes)
}
pub struct Workspace {
    pub root: PathBuf,
    pub model: PathBuf,
    pub directory: super::owned_path::Directory,
}
impl Workspace {
    pub fn create(repository: &Path, manifest: &Manifest, label: &str) -> Result<Self, String> {
        if manifest.schema_version != SCHEMA_VERSION {
            return Err("unsupported manifest schema".into());
        }
        let base = std::env::temp_dir()
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let root = base.join(format!(
            "cad-parity-{}-{}-{label}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let directory = super::owned_path::Directory::open(&base)?.create(
            root.file_name()
                .ok_or("workspace name missing")?
                .to_str()
                .ok_or("workspace name nonUTF8")?,
        )?;
        directory.create("tmp")?;
        directory.create("logs")?;
        let mut seen = std::collections::BTreeSet::new();
        for file in std::iter::once(&manifest.source).chain(manifest.dependencies.iter()) {
            if !seen.insert(file.path.clone()) {
                return Err(format!("duplicate dependency {}", file.path));
            }
            let bytes = read_checked(repository, file)?;
            let mut f = directory.new_file(&relative(&file.path)?)?;
            f.write_all(&bytes).map_err(|e| e.to_string())?;
        }
        let mut marker = directory.new_file(Path::new(".cad-parity-owner"))?;
        serde_json::to_writer(
            &mut marker,
            &serde_json::json!({"source_sha256":manifest.source.sha256}),
        )
        .map_err(|e| e.to_string())?;
        marker.sync_all().map_err(|e| e.to_string())?;
        Ok(Self {
            model: root.join(relative(&manifest.source.path)?),
            root,
            directory,
        })
    }
}
// Retained rather than recursively deleted: receipts and failures remain inspectable,
// and no source/output path can accidentally be passed to a recursive remover.
