//! The launch's workspace root ([`sim_runtime::workspace`]), resolved once in
//! `main` from `--workspace`, `$SIM_WORKSPACE`, the opened file and the
//! current directory. Every repository-data default reads it: the part
//! registry, library, models, presets and their inputs, runs/ outputs and
//! source links. Explicit command-line paths stay relative to the current
//! directory.

use sim_runtime::workspace::{WorkspaceError, WorkspaceRoot};
use std::path::{Path, PathBuf};

/// Resolve for this launch (first call wins) and report the result on stderr.
pub fn init(explicit: Option<&Path>, opened: Option<&Path>) -> &'static Result<WorkspaceRoot, WorkspaceError> {
    let resolved = sim_runtime::workspace::init(explicit, opened);
    match resolved {
        Ok(w) => eprintln!("Workspace root: {} (found by {})", w.path.display(), json()["found_by"].as_str().unwrap_or_default()),
        Err(e) => eprintln!("Warning: {e}. Repository data (authored parts, library, presets, runs/) has no default location."),
    }
    resolved
}

/// The launch's resolution (resolved from the current directory if `init` was not called).
pub fn get() -> &'static Result<WorkspaceRoot, WorkspaceError> {
    sim_runtime::workspace::get()
}

/// The root, or the error naming what was searched.
pub fn root() -> Result<&'static Path, String> {
    get().as_ref().map(|w| w.path.as_path()).map_err(|e| e.to_string())
}

/// `<root>/rel`, or the error naming what was searched.
pub fn path(rel: impl AsRef<Path>) -> Result<PathBuf, String> {
    root().map(|r| r.join(rel))
}

/// The shared REST shape (`sim_runtime::workspace::to_json`).
pub fn json() -> serde_json::Value {
    sim_runtime::workspace::to_json(get())
}

/// The working directory for Codex annotation agents: the workspace root.
/// With no root (already reported at launch) the git checkout holding the
/// current directory, else the current directory, as before: the agent needs
/// some directory to run in, and it writes no repository data there itself.
pub fn agent_dir() -> PathBuf {
    root().map(Path::to_path_buf).unwrap_or_else(|_| {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        cwd.ancestors().find(|p| p.join(".git").exists()).unwrap_or(&cwd).to_path_buf()
    })
}
