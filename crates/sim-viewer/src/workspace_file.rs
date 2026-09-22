//! Atomic sidecar persistence with optimistic conflict detection between viewers.
use sim_diagram::analysis::Workspace;
use sim_inspect::SystemDescription;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub struct WorkspaceFile {
    pub path: PathBuf,
    expected: Option<blake3::Hash>,
    model: Option<String>,
}
impl WorkspaceFile {
    pub fn open(path: PathBuf, d: &SystemDescription) -> Result<(Self, Workspace), String> {
        match fs::read(&path) {
            Ok(bytes) => {
                let workspace: Workspace = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("Cannot read workspace: {e}"))?;
                workspace.validate(d)?;
                Ok((
                    Self {
                        path,
                        expected: Some(blake3::hash(&bytes)),
                        model: Some(d.id.clone()),
                    },
                    workspace,
                ))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((
                Self {
                    path,
                    expected: None,
                    model: Some(d.id.clone()),
                },
                Workspace::new(d),
            )),
            Err(e) => Err(format!("Cannot open workspace: {e}")),
        }
    }
    pub fn new_path(path: PathBuf) -> Self {
        Self {
            path,
            expected: None,
            model: None,
        }
    }
    pub fn save(&mut self, workspace: &Workspace, d: &SystemDescription) -> Result<(), String> {
        workspace.validate(d)?;
        if self.model.as_ref().is_some_and(|id| id != &d.id) {
            return Err(
                "This file belongs to another model. Choose a different workspace filename.".into(),
            );
        }
        let bytes = serde_json::to_vec_pretty(workspace).map_err(|e| e.to_string())?;
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !parent.is_dir() {
            return Err("Workspace folder does not exist".into());
        }
        let lock_path = PathBuf::from(format!("{}.lock", self.path.display()));
        let _lock = Lock::acquire(lock_path)?;
        let actual = match fs::read(&self.path) {
            Ok(b) => Some(blake3::hash(&b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.to_string()),
        };
        if actual != self.expected {
            return Err(
                "Workspace changed on disk. Choose a new filename to preserve both versions."
                    .into(),
            );
        }
        let name = self
            .path
            .file_name()
            .ok_or("Workspace needs a filename")?
            .to_string_lossy();
        let temporary = parent.join(format!(".{name}.{}.tmp", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| format!("Cannot create workspace temporary file: {e}"))?;
        let result = (|| {
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)?;
            Ok::<_, std::io::Error>(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(|e| format!("Cannot save workspace: {e}"))?;
        let mut retained = bytes;
        retained.push(b'\n');
        self.expected = Some(blake3::hash(&retained));
        self.model = Some(d.id.clone());
        Ok(())
    }
}
struct Lock(PathBuf);
impl Lock {
    fn acquire(path: PathBuf) -> Result<Self, String> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("Workspace is locked or unavailable: {e}"))?;
        Ok(Self(path))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_diagram::analysis::{Annotation, Target};
    #[test]
    fn sidecar_roundtrip_preserves_notes_and_rejects_lost_updates() {
        let d: SystemDescription = serde_json::from_str(include_str!(
            "../../../examples/systems-viewer/full-robot.description.json"
        ))
        .unwrap();
        let root = std::env::temp_dir().join(format!("sim-workspace-test-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let path = root.join("review.json");
        let (mut first, mut workspace) = WorkspaceFile::open(path.clone(), &d).unwrap();
        let (mut second, stale) = WorkspaceFile::open(path.clone(), &d).unwrap();
        let target = Target::Component(d.components.keys().next().unwrap().clone());
        workspace.annotations.insert(
            target.key(),
            Annotation {
                target,
                label: None,
                text: "Review heat path".into(),
            },
        );
        first.save(&workspace, &d).unwrap();
        let retained = fs::read(&path).unwrap();
        assert!(
            second
                .save(&stale, &d)
                .unwrap_err()
                .contains("changed on disk")
        );
        assert_eq!(fs::read(&path).unwrap(), retained);
        let (mut reopened, restored) = WorkspaceFile::open(path.clone(), &d).unwrap();
        assert_eq!(restored, workspace);
        reopened.save(&restored, &d).unwrap();
        let mut other = d.clone();
        other.id = "another-model".into();
        let other_workspace = Workspace::new(&other);
        assert!(reopened.save(&other_workspace, &other).is_err());
        assert_eq!(fs::read(&path).unwrap(), retained);
        assert_eq!(
            fs::read_dir(&root).unwrap().count(),
            1,
            "no temporary or lock files remain"
        );
        fs::remove_file(path).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
