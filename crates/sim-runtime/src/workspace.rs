//! The workspace root: the one directory repository data is read from and
//! written under (library/parts, library/systems, models, web/viewer/presets.json,
//! runs/…), whatever directory a tool was launched from.
//!
//! Rule, first match wins:
//! 1. an explicit override: the caller's flag (e.g. `sim-spatial --workspace DIR`),
//!    else the `SIM_WORKSPACE` environment variable. An override without the
//!    marker is an error naming it; it never falls through to a walk;
//! 2. walking up from the opened file's directory;
//! 3. walking up from the current directory.
//!
//! The marker ([`MARKER`]) is a directory holding a `Cargo.toml` with a
//! `[workspace]` table and a `library/` directory. Roots are canonical
//! (absolute). When nothing matches, the error lists every directory searched;
//! callers report it and never substitute the current directory.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const ENV: &str = "SIM_WORKSPACE";
pub const MARKER: &str = "a Cargo.toml with a [workspace] table next to a library/ directory";
pub const RULE: &str = "--workspace DIR, else $SIM_WORKSPACE, else the nearest ancestor of the opened file, else of the current directory, holding a Cargo.toml with a [workspace] table next to a library/ directory";

/// How the root was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FoundBy {
    /// The caller's explicit flag.
    Override,
    /// `$SIM_WORKSPACE`.
    Env,
    /// Walking up from this opened file.
    OpenedFile(PathBuf),
    /// Walking up from this current directory.
    Cwd(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceRoot {
    pub path: PathBuf,
    pub found_by: FoundBy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceError {
    /// The override that lacks the marker, if one was given.
    pub rejected_override: Option<(String, PathBuf)>,
    /// Every directory checked for the marker, in order.
    pub searched: Vec<PathBuf>,
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.rejected_override {
            Some((by, dir)) => write!(f, "workspace root {} (from {by}) is not a workspace: it lacks {MARKER}", dir.display()),
            None => {
                let dirs: Vec<String> = self.searched.iter().map(|d| d.display().to_string()).collect();
                write!(f, "no workspace root found: none of [{}] holds {MARKER}; pass --workspace DIR or set {ENV}", dirs.join(", "))
            }
        }
    }
}
impl std::error::Error for WorkspaceError {}

type Resolved = Result<WorkspaceRoot, WorkspaceError>;
static PROCESS: std::sync::OnceLock<Resolved> = std::sync::OnceLock::new();

/// Resolve this process's root once, at launch (the first call wins), from
/// the tool's override flag and the file it opened.
pub fn init(explicit: Option<&Path>, opened: Option<&Path>) -> &'static Resolved {
    PROCESS.get_or_init(|| resolve_workspace(explicit, opened))
}

/// This process's root: the one [`init`] resolved, else resolved once from
/// `$SIM_WORKSPACE` and the current directory. Every repository-data default
/// (the parts directory, the lesson sandbox, …) reads it.
pub fn get() -> &'static Resolved {
    init(None, None)
}

/// True when `dir` holds the marker.
pub fn is_root(dir: &Path) -> bool {
    dir.join("library").is_dir() && std::fs::read_to_string(dir.join("Cargo.toml")).is_ok_and(|t| t.lines().any(|l| l.trim() == "[workspace]"))
}

/// The rule above with the process environment and current directory.
pub fn resolve_workspace(explicit: Option<&Path>, opened: Option<&Path>) -> Result<WorkspaceRoot, WorkspaceError> {
    let env = std::env::var_os(ENV).filter(|v| !v.is_empty()).map(PathBuf::from);
    let cwd = std::env::current_dir().ok();
    resolve_from(explicit, env.as_deref(), opened, cwd.as_deref())
}

/// The rule as a pure function of its inputs (relative `opened` and override
/// paths are taken relative to `cwd`).
pub fn resolve_from(explicit: Option<&Path>, env: Option<&Path>, opened: Option<&Path>, cwd: Option<&Path>) -> Result<WorkspaceRoot, WorkspaceError> {
    let absolute = |p: &Path| match cwd {
        Some(c) if p.is_relative() => c.join(p),
        _ => p.to_path_buf(),
    };
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    for (by, found_by, dir) in [("--workspace", FoundBy::Override, explicit), (ENV, FoundBy::Env, env)] {
        if let Some(dir) = dir {
            let dir = canonical(&absolute(dir));
            return if is_root(&dir) { Ok(WorkspaceRoot { path: dir, found_by }) } else { Err(WorkspaceError { rejected_override: Some((by.into(), dir.clone())), searched: vec![dir] }) };
        }
    }
    let mut searched = Vec::new();
    let starts = [opened.map(|f| (canonical(&absolute(f)), true)), cwd.map(|c| (canonical(c), false))];
    for (start, is_file) in starts.into_iter().flatten() {
        let first = if is_file { start.parent().map(Path::to_path_buf) } else { Some(start.clone()) };
        for dir in first.iter().flat_map(|d| d.ancestors()) {
            if searched.iter().any(|s| s == dir) {
                continue;
            }
            if is_root(dir) {
                let found_by = if is_file { FoundBy::OpenedFile(start) } else { FoundBy::Cwd(start) };
                return Ok(WorkspaceRoot { path: dir.to_path_buf(), found_by });
            }
            searched.push(dir.to_path_buf());
        }
    }
    Err(WorkspaceError { rejected_override: None, searched })
}

/// The shared REST shape: `{root, found_by, from, error, rule}` (root null
/// with the error when unresolved).
pub fn to_json(resolved: &Result<WorkspaceRoot, WorkspaceError>) -> Value {
    match resolved {
        Ok(w) => {
            let (by, from) = match &w.found_by {
                FoundBy::Override => ("override", None),
                FoundBy::Env => ("env", None),
                FoundBy::OpenedFile(p) => ("opened_file", Some(p)),
                FoundBy::Cwd(p) => ("cwd", Some(p)),
            };
            json!({"root": w.path, "found_by": by, "from": from, "error": null, "rule": RULE})
        }
        Err(e) => json!({"root": null, "found_by": null, "from": null, "error": e.to_string(), "rule": RULE}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(dir: &Path) {
        std::fs::create_dir_all(dir.join("library")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    }

    #[test]
    fn resolves_by_rule_and_names_what_was_searched() {
        let tmp = std::env::temp_dir().join(format!("sim-workspace-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let (repo, other, bare) = (tmp.join("repo"), tmp.join("other"), tmp.join("bare"));
        marker(&repo);
        marker(&other);
        std::fs::create_dir_all(repo.join("examples/a")).unwrap();
        std::fs::create_dir_all(&bare).unwrap();
        // A Cargo.toml without [workspace] is not the marker.
        std::fs::write(bare.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        std::fs::create_dir_all(bare.join("library")).unwrap();
        let file = repo.join("examples/a/board.system.json");
        std::fs::write(&file, "{}").unwrap();
        let repo = std::fs::canonicalize(&repo).unwrap();
        let other = std::fs::canonicalize(&other).unwrap();
        let bare = std::fs::canonicalize(&bare).unwrap();

        // Nested opened file wins over cwd.
        let w = resolve_from(None, None, Some(&file), Some(&bare)).unwrap();
        assert_eq!(w.path, repo);
        assert_eq!(w.found_by, FoundBy::OpenedFile(std::fs::canonicalize(&file).unwrap()));
        assert_eq!(to_json(&Ok(w))["found_by"], "opened_file");
        // Cwd walk when no file is opened (from a nested directory).
        let w = resolve_from(None, None, None, Some(&repo.join("examples/a"))).unwrap();
        assert_eq!((w.path, w.found_by), (repo.clone(), FoundBy::Cwd(repo.join("examples/a"))));
        // The flag beats the env var, which beats the file.
        assert_eq!(resolve_from(Some(&other), Some(&repo), Some(&file), None).unwrap(), WorkspaceRoot { path: other.clone(), found_by: FoundBy::Override });
        assert_eq!(resolve_from(None, Some(&other), Some(&file), None).unwrap(), WorkspaceRoot { path: other.clone(), found_by: FoundBy::Env });
        // An override without the marker is an error naming it, with no walk.
        let e = resolve_from(Some(&bare), None, Some(&file), None).unwrap_err();
        assert_eq!(e.searched, vec![bare.clone()]);
        assert!(e.to_string().contains(&bare.display().to_string()) && e.to_string().contains("--workspace"), "{e}");
        // No marker anywhere: the error lists every directory searched.
        let e = resolve_from(None, None, Some(&bare.join("x.system.json")), Some(&bare)).unwrap_err();
        assert!(e.rejected_override.is_none());
        assert_eq!(e.searched.first(), Some(&bare));
        assert!(e.searched.contains(&tmp.canonicalize().unwrap()));
        let text = e.to_string();
        assert!(text.contains(&bare.display().to_string()) && text.contains(MARKER) && text.contains(ENV), "{text}");
        assert!(to_json(&Err(e))["root"].is_null());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
