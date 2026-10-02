//! Filesystem mechanics only: callers own serialization, identity checks, authorization,
//! cancellation and revision ordering. No cross-process compare-and-swap is provided.
//! Confirmation means the OS accepted file and directory sync, not a hardware guarantee.
//! All parent ancestors are synchronized, including directory entries created here.
use std::{fmt, fs::{self, File, OpenOptions}, io::{self, Write}, path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy { ImmutableNew, Replace }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage { PrepareParent, CreateTemp, Write, FileSync, Publish, DirectorySync, Cleanup }

/// A deterministic seam in the real writer, also usable by downstream consumer fixtures.
/// `destination` scopes faults to an artifact; `working_path` names the current operation.
/// Implementations must not mutate production destinations. No alternate writer exists.
pub trait Hooks {
    fn before(&self, _stage: Stage, _destination: &Path, _working_path: &Path) -> io::Result<()> { Ok(()) }
}
pub struct NoHooks;
impl Hooks for NoHooks {}

#[derive(Debug)]
pub struct Failure {
    pub stage: Stage,
    pub kind: io::ErrorKind,
    pub message: String,
    pub cleanup_error: Option<String>,
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.stage, self.message)?;
        if let Some(error) = &self.cleanup_error { write!(f, "; owned temporary cleanup failed: {error}")?; }
        Ok(())
    }
}
#[derive(Debug)]
pub enum Outcome {
    /// The destination and directory ancestry were synchronized. A cleanup warning
    /// is separate from durability; conservative consumers may still refuse acknowledgment.
    Confirmed { cleanup_error: Option<String> },
    /// This invocation did not publish. An independently existing destination is untouched.
    Unpublished(Failure),
    /// Publication succeeded, but required synchronization did not. Never delete the destination.
    VisibleUnconfirmed(Failure),
}
impl Outcome {
    /// Conservative adapter for existing Result acknowledgment owners. Cleanup errors
    /// remain observable even when synchronization succeeded; retained inputs allow retry.
    pub fn into_result(self) -> Result<(), String> {
        match self {
            Self::Confirmed { cleanup_error: None } => Ok(()),
            Self::Confirmed { cleanup_error: Some(error) } => Err(format!("destination durability confirmed; owned temporary cleanup failed: {error}")),
            Self::Unpublished(error) => Err(format!("destination not published: {error}")),
            Self::VisibleUnconfirmed(error) => Err(format!("destination visible but durability unconfirmed: {error}")),
        }
    }
}
fn failure(stage: Stage, path: &Path, error: io::Error) -> Failure {
    Failure { stage, kind: error.kind(), message: format!("{}: {error}", path.display()), cleanup_error: None }
}
fn step(hooks: &dyn Hooks, stage: Stage, destination: &Path, path: &Path, operation: impl FnOnce() -> io::Result<()>) -> Result<(), Failure> {
    hooks.before(stage, destination, path).and_then(|_| operation()).map_err(|error| failure(stage, path, error))
}
fn parent(path: &Path) -> &Path {
    path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."))
}
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
fn temporary(dir: &Path) -> PathBuf {
    // create_new, not the name's entropy, establishes ownership. A collision fails
    // without retrying or touching the colliding file (including PID reuse).
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_nanos());
    dir.join(format!(".sim-publication-{}-{stamp}-{}.tmp", std::process::id(), NEXT_TEMP.fetch_add(1, Ordering::Relaxed)))
}
fn sync_ancestry(destination: &Path, hooks: &dyn Hooks) -> Result<(), Failure> {
    // Resolve relative '.' and '..' so every actual ancestor, through root, is
    // synchronized. This is not protection against concurrent path/symlink changes.
    let dir = fs::canonicalize(parent(destination)).map_err(|error| failure(Stage::DirectorySync, parent(destination), error))?;
    for ancestor in dir.ancestors() {
        step(hooks, Stage::DirectorySync, destination, ancestor, || {
            #[cfg(unix)]
            { File::open(ancestor)?.sync_all() }
            #[cfg(not(unix))]
            { Err(io::Error::new(io::ErrorKind::Unsupported, "directory synchronization is unsupported by this implementation")) }
        })?;
    }
    Ok(())
}
pub fn publish(path: &Path, bytes: &[u8], policy: Policy) -> Outcome {
    publish_with(path, bytes, policy, &NoHooks)
}
pub fn publish_with(path: &Path, bytes: &[u8], policy: Policy, hooks: &dyn Hooks) -> Outcome {
    if let Err(error) = step(hooks, Stage::PrepareParent, path, parent(path), || fs::create_dir_all(parent(path))) {
        return Outcome::Unpublished(error);
    }
    let temp = temporary(parent(path));
    let file = hooks.before(Stage::CreateTemp, path, &temp)
        .and_then(|_| OpenOptions::new().write(true).create_new(true).open(&temp));
    let mut file = match file {
        Ok(file) => file,
        // No ownership was obtained: cleanup must not touch the colliding file.
        Err(error) => return Outcome::Unpublished(failure(Stage::CreateTemp, &temp, error)),
    };
    let prepared = step(hooks, Stage::Write, path, &temp, || file.write_all(bytes))
        .and_then(|_| step(hooks, Stage::FileSync, path, &temp, || file.sync_all()));
    drop(file);
    let published = prepared.and_then(|_| step(hooks, Stage::Publish, path, path, || match policy {
        Policy::ImmutableNew => fs::hard_link(&temp, path),
        // Same-directory rename follows the platform's replacement semantics.
        // Unsupported replacement fails rather than deleting an existing destination.
        Policy::Replace => fs::rename(&temp, path),
    }));
    // Rename consumed the temporary name; immutable publication leaves it linked.
    // On failure only this successfully-created temporary belongs to us.
    let needs_cleanup = policy == Policy::ImmutableNew || published.is_err();
    let cleanup_error = if needs_cleanup {
        step(hooks, Stage::Cleanup, path, &temp, || fs::remove_file(&temp)).err().map(|e| e.to_string())
    } else { None };
    if let Err(mut error) = published {
        error.cleanup_error = cleanup_error;
        return Outcome::Unpublished(error);
    }
    match sync_ancestry(path, hooks) {
        Ok(()) => Outcome::Confirmed { cleanup_error },
        Err(mut error) => { error.cleanup_error = cleanup_error; Outcome::VisibleUnconfirmed(error) }
    }
}
/// Synchronize an existing artifact after the consumer verifies its identity or
/// expected snapshot. Readability alone never proves durability. The verification
/// to synchronization race remains the caller's documented cross-process limit.
pub fn confirm_existing(path: &Path) -> Outcome { confirm_existing_with(path, &NoHooks) }
pub fn confirm_existing_with(path: &Path, hooks: &dyn Hooks) -> Outcome {
    let synced = step(hooks, Stage::FileSync, path, path, || File::open(path)?.sync_all())
        .and_then(|_| sync_ancestry(path, hooks));
    match synced {
        Ok(()) => Outcome::Confirmed { cleanup_error: None },
        Err(error) => Outcome::VisibleUnconfirmed(error),
    }
}

#[cfg(test)]
mod fixtures;
