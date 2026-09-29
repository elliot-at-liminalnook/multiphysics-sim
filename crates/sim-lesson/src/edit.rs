//! Editing lesson files through one command layer. The viewer, REST clients
//! and the CLI apply the same [`Edit`]s with the same checks: the caller's
//! expected revision (content hash) must match the file, the edited lesson
//! must still parse, and the write is atomic. Undo and redo live in a
//! journal next to the lesson (`.lesson.md.history.json`), so every editor
//! shares one history; an undo is refused if the file changed outside it.
use crate::{Lesson, LessonError, hash};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "edit", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    /// Replace one block (text or embed, fences included) with new Markdown.
    ReplaceBlock { block: String, hash: String, text: String },
    /// Insert a new block after `block` (at the top of the body when absent).
    InsertAfter {
        #[serde(default)]
        block: Option<String>,
        text: String,
    },
    DeleteBlock { block: String, hash: String },
    /// Replace the whole file (front matter included).
    ReplaceSource { source: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Journal {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    label: String,
    before: String,
    after: String,
}
const JOURNAL_LIMIT: usize = 50;

#[derive(Debug, Clone, Serialize)]
pub struct Applied {
    pub label: String,
    /// Content hash of the file after the edit.
    pub revision: String,
    pub undo: usize,
    pub redo: usize,
}

pub struct LessonStore {
    pub path: PathBuf,
}

/// Content hash used as the lesson's revision.
pub fn revision(source: &str) -> String {
    hash(source)
}

fn located(e: LessonError) -> String {
    e.to_string()
}

impl LessonStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    fn journal_path(&self) -> PathBuf {
        let name = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "lesson.md".into());
        self.path.with_file_name(format!(".{name}.history.json"))
    }
    fn journal(&self) -> Journal {
        std::fs::read(self.journal_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
    fn save_journal(&self, j: &Journal) -> Result<(), String> {
        let bytes = serde_json::to_vec(j).map_err(|e| e.to_string())?;
        sim_annotate::store::write_atomic(&self.journal_path(), &bytes)
    }
    fn locked<T>(&self, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(format!("{}.lock", self.path.display())).map_err(|e| e.to_string())?;
        for _ in 0..40 {
            match lock.try_lock() {
                Ok(()) => return f(),
                Err(std::fs::TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("lesson file is busy; retry".into())
    }
    pub fn read(&self) -> Result<Lesson, String> {
        Lesson::load(&self.path).map_err(located)
    }
    pub fn history(&self) -> (Vec<String>, Vec<String>) {
        let j = self.journal();
        (j.undo.iter().map(|e| e.label.clone()).collect(), j.redo.iter().map(|e| e.label.clone()).collect())
    }

    /// Apply one edit. `expected` is the revision the caller saw; `None`
    /// skips the check (CLI batch use).
    pub fn apply(&self, label: &str, edit: Edit, expected: Option<&str>) -> Result<(Lesson, Applied), String> {
        self.locked(|| {
            let source = std::fs::read_to_string(&self.path).map_err(|e| e.to_string())?;
            if expected.is_some_and(|r| r != revision(&source)) {
                return Err("the lesson changed since you read it; reload and try again".into());
            }
            let lesson = Lesson::parse(&self.path, &source).map_err(located)?;
            let next = edited(&lesson, &edit)?;
            if next == source {
                return Err("the edit changes nothing".into());
            }
            let parsed = Lesson::parse(&self.path, &next).map_err(|e| format!("edit rejected: {e}"))?;
            sim_annotate::store::write_atomic(&self.path, next.as_bytes())?;
            let mut j = self.journal();
            j.undo.push(Entry { label: label.into(), before: source, after: next.clone() });
            if j.undo.len() > JOURNAL_LIMIT {
                j.undo.remove(0);
            }
            j.redo.clear();
            self.save_journal(&j)?;
            Ok((parsed, Applied { label: label.into(), revision: revision(&next), undo: j.undo.len(), redo: 0 }))
        })
    }

    pub fn undo(&self) -> Result<(Lesson, Applied), String> {
        self.step(true)
    }
    pub fn redo(&self) -> Result<(Lesson, Applied), String> {
        self.step(false)
    }
    fn step(&self, undo: bool) -> Result<(Lesson, Applied), String> {
        self.locked(|| {
            let source = std::fs::read_to_string(&self.path).map_err(|e| e.to_string())?;
            let mut j = self.journal();
            let entry = if undo { j.undo.pop() } else { j.redo.pop() }.ok_or(if undo { "nothing to undo" } else { "nothing to redo" })?;
            let (expect, write) = if undo { (entry.after.clone(), entry.before.clone()) } else { (entry.before.clone(), entry.after.clone()) };
            if revision(&source) != revision(&expect) {
                return Err("the lesson changed outside the editor since that edit; undo history no longer applies".into());
            }
            let parsed = Lesson::parse(&self.path, &write).map_err(located)?;
            sim_annotate::store::write_atomic(&self.path, write.as_bytes())?;
            let label = entry.label.clone();
            if undo { j.redo.push(entry) } else { j.undo.push(entry) }
            self.save_journal(&j)?;
            Ok((parsed, Applied { label, revision: revision(&write), undo: j.undo.len(), redo: j.redo.len() }))
        })
    }
}

/// The source after an edit (not yet validated).
pub fn edited(lesson: &Lesson, edit: &Edit) -> Result<String, String> {
    let src = &lesson.source;
    let find = |id: &str, hash: &str| {
        let b = lesson.block(id).ok_or_else(|| format!("no block `{id}`"))?;
        if b.hash != hash {
            return Err(format!("block `{id}` changed since you read it; reload and try again"));
        }
        Ok(b)
    };
    let clean = |text: &str| text.trim_matches('\n').to_string();
    Ok(match edit {
        Edit::ReplaceBlock { block, hash, text } => {
            let b = find(block, hash)?;
            if clean(text).trim().is_empty() {
                return Err("a block cannot be empty; delete it instead".into());
            }
            format!("{}{}{}", &src[..b.start], clean(text), &src[b.end..])
        }
        Edit::InsertAfter { block, text } => {
            if clean(text).trim().is_empty() {
                return Err("nothing to insert".into());
            }
            let at = match block {
                Some(id) => lesson.block(id).ok_or_else(|| format!("no block `{id}`"))?.end,
                None => lesson.body,
            };
            let (before, after) = src.split_at(at);
            if block.is_some() {
                format!("{before}\n\n{}{after}", clean(text))
            } else {
                format!("{before}{}\n\n{}", clean(text), after.trim_start_matches('\n'))
            }
        }
        Edit::DeleteBlock { block, hash } => {
            let b = find(block, hash)?;
            let tail = &src[b.end..];
            let tail = tail.strip_prefix('\n').unwrap_or(tail);
            let tail = tail.strip_prefix('\n').unwrap_or(tail);
            format!("{}{}", &src[..b.start], tail)
        }
        Edit::ReplaceSource { source } => source.clone(),
    })
}

/// Open the lesson in the user's editor (`$VISUAL`/`$EDITOR`, else the
/// platform opener). Returns immediately; the viewer reloads on save.
pub fn open_in_editor(path: &Path) -> Result<(), String> {
    let editor = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).ok().filter(|e| !e.trim().is_empty() && !["vi", "vim", "nano", "emacs -nw"].contains(&e.trim()));
    let mut command = match editor {
        Some(e) => {
            let mut parts = e.split_whitespace();
            let mut c = std::process::Command::new(parts.next().unwrap());
            c.args(parts);
            c
        }
        None if cfg!(target_os = "macos") => {
            let mut c = std::process::Command::new("open");
            c.arg("-t");
            c
        }
        None if cfg!(windows) => {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", "start", ""]);
            c
        }
        None => std::process::Command::new("xdg-open"),
    };
    let mut child = command.arg(path).spawn().map_err(|e| format!("cannot open an editor: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_undo_redo_and_refuse_stale_or_invalid_changes() {
        let dir = std::env::temp_dir().join(format!("lesson-edit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("demo")).unwrap();
        let path = dir.join("demo/lesson.md");
        std::fs::write(&path, "---\ntitle: Demo\n---\n# One\n\nFirst.\n\nSecond.\n").unwrap();
        let store = LessonStore::new(&path);
        let lesson = store.read().unwrap();
        let rev = revision(&lesson.source);
        let b2 = lesson.block("b2").unwrap().clone();
        let (l, applied) = store.apply("Edit first", Edit::ReplaceBlock { block: "b2".into(), hash: b2.hash.clone(), text: "First, edited.".into() }, Some(&rev)).unwrap();
        assert!(l.source.contains("First, edited.\n\nSecond."));
        // The old revision and the old block hash are both stale now.
        assert!(store.apply("again", Edit::DeleteBlock { block: "b2".into(), hash: b2.hash.clone() }, Some(&rev)).is_err());
        assert!(store.apply("again", Edit::DeleteBlock { block: "b2".into(), hash: b2.hash }, Some(&applied.revision)).is_err());
        // An edit that breaks the file is rejected with its line.
        let e = store.apply("bad", Edit::InsertAfter { block: Some("b3".into()), text: "```sim-scene\nid: x\nsystem: nowhere\n```".into() }, None).unwrap_err();
        assert!(e.contains("lesson.md:"), "{e}");
        let (l, _) = store.apply("Add", Edit::InsertAfter { block: None, text: "Opening.".into() }, None).unwrap();
        assert!(l.source.contains("---\nOpening.\n\n# One"));
        store.undo().unwrap();
        let (l, _) = store.undo().unwrap();
        assert_eq!(l.source, "---\ntitle: Demo\n---\n# One\n\nFirst.\n\nSecond.\n");
        store.redo().unwrap();
        std::fs::write(&path, "---\ntitle: Demo\n---\nchanged elsewhere\n").unwrap();
        assert!(store.undo().unwrap_err().contains("outside the editor"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
