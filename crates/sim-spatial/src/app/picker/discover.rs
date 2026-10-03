//! The document picker's discovery (on its `Pool::Io` job, never the UI
//! thread): the mode's recent documents, robot presets, the RoboCAD
//! service at the default URL, and the workspace's example documents.
//! The walk stops when the job is cancelled (the picker closed), and a
//! section cut by the walk's budget or [`CAP`] says so in its title.
use super::{Choice, Section, Sources};
use crate::app::ViewerMode;
use crate::app::recent::Recents;
use crate::app::switch::Document;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Directories the example walk never enters (besides dot-directories).
const SKIP: &[&str] = &["runs", "target", "node_modules", ".venv"];
/// How deep the example walk goes below its start.
const DEPTH: usize = 6;
/// Entries per section.
pub(crate) const CAP: usize = 40;
/// Directory entries one walk reads at most (a bound on its time).
const VISITED: usize = 50_000;

type Tree = BTreeMap<PathBuf, Vec<(String, bool)>>;

fn skipped(name: &str) -> bool {
    name.starts_with('.') || SKIP.contains(&name)
}

/// A walk's directories and whether it stopped early (the [`VISITED`]
/// budget ran out, or the job was cancelled).
struct Walk {
    tree: Tree,
    cut: bool,
}

/// Every directory under `start` (itself included) to `depth`, with its
/// entries (name, is a directory; symlinks are not followed), skipping
/// dot-directories and [`SKIP`]. Reads at most [`VISITED`] entries, and
/// stops when `cancel` is set.
fn tree(start: &Path, depth: usize, cancel: &AtomicBool) -> Walk {
    let mut tree = Tree::new();
    let mut stack = vec![(start.to_path_buf(), 0usize)];
    let mut visited = 0;
    let mut cut = false;
    while let Some((dir, d)) = stack.pop() {
        if cut || cancel.load(Ordering::Relaxed) {
            cut = true;
            break;
        }
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        let mut entries = Vec::new();
        for e in read.filter_map(Result::ok) {
            visited += 1;
            // The whole walk stops (not just this directory).
            if visited > VISITED || cancel.load(Ordering::Relaxed) {
                cut = true;
                break;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
            if is_dir && d < depth && !skipped(&name) {
                stack.push((dir.join(&name), d + 1));
            }
            entries.push((name, is_dir));
        }
        tree.insert(dir, entries);
    }
    Walk { tree, cut }
}

/// Files named `*<suffix>` in `tree`.
fn files(tree: &Tree, suffix: &str) -> Vec<PathBuf> {
    tree.iter().flat_map(|(dir, entries)| entries.iter().filter(|(n, d)| !d && !n.starts_with('.') && n.ends_with(suffix)).map(move |(n, _)| dir.join(n))).collect()
}

/// Directories in `tree` holding a file `name`.
fn dirs_holding(tree: &Tree, name: &str) -> Vec<PathBuf> {
    tree.iter().filter(|(_, entries)| entries.iter().any(|(n, d)| !d && n == name)).map(|(dir, _)| dir.clone()).collect()
}

/// Lessons folders in `tree`: directories with a `<slug>/lesson.md` child.
fn lesson_dirs(tree: &Tree) -> Vec<PathBuf> {
    let has_lesson = |dir: &Path| tree.get(dir).is_some_and(|entries| entries.iter().any(|(n, d)| !d && n == "lesson.md"));
    tree.iter().filter(|(dir, entries)| entries.iter().any(|(n, d)| *d && !skipped(n) && has_lesson(&dir.join(n)))).map(|(dir, _)| dir.clone()).collect()
}

/// `*.description.json` files with their `*.spatial.json` beside them.
fn inspect_pairs(tree: &Tree) -> Vec<PathBuf> {
    tree.iter()
        .flat_map(|(dir, entries)| {
            entries.iter().filter_map(move |(n, d)| {
                let stem = n.strip_suffix(".description.json").filter(|_| !d && !n.starts_with('.'))?;
                entries.iter().any(|(m, e)| !e && *m == format!("{stem}.spatial.json")).then(|| dir.join(n))
            })
        })
        .collect()
}

/// `path` relative to `root` when under it.
fn relative(root: Option<&Path>, path: &Path) -> String {
    root.and_then(|r| path.strip_prefix(r).ok()).unwrap_or(path).display().to_string()
}

/// The mode's recent documents (a path that no longer exists is disabled, "missing").
fn recent_section(mode: ViewerMode, root: Option<&Path>, recents: &Recents, ready: bool) -> Section {
    let choices = recents.list(mode)
        .into_iter()
        .map(|document| {
            let (label, detail, enabled) = match &document {
                Document::Path(p) if !p.exists() => (relative(root, p), "missing".to_string(), false),
                Document::Path(p) => (relative(root, p), String::new(), true),
                Document::Preset(id) => (id.clone(), "preset".to_string(), true),
                Document::Url(url) => (url.clone(), "RoboCAD service".to_string(), true),
            };
            Choice { label, detail, enabled, document }
        })
        .collect();
    Section { title: "Recent".into(), empty: if ready { format!("Nothing opened in {} mode yet.", mode.label()) } else { "Preferences are still loading; recent documents will appear here.".into() }, choices }
}

/// The robot presets in `presets` (the browser's list), each with its id
/// and readiness; one that cannot run here is disabled, naming why.
fn preset_section(root: Option<&Path>, presets: Option<&Path>) -> Section {
    let title = "Presets".to_string();
    let Some(file) = presets else {
        return Section { title, empty: "No preset list found (no workspace root).".into(), choices: Vec::new() };
    };
    match crate::robot::preset::list(file) {
        Err(e) => Section { title, empty: e, choices: Vec::new() },
        Ok(list) => {
            let choices = list
                .into_iter()
                .map(|p| {
                    let openable = root.map(|r| p.discovery(r));
                    let enabled = openable.as_ref().is_none_or(|d| d["openable"].as_bool().unwrap_or(false));
                    let mut detail = match p.readiness() {
                        Some(r) => format!("{}, {r}", p.id),
                        None => p.id.clone(),
                    };
                    if let Some(why) = openable.as_ref().and_then(|d| d["not_openable_reason"].as_str()) {
                        detail = format!("{detail}; not available here: {why}");
                    }
                    Choice { label: p.label.clone(), detail, enabled, document: Document::Preset(p.id) }
                })
                .collect();
            Section { title, empty: format!("{} lists no presets.", file.display()), choices }
        }
    }
}

/// The workspace's example documents for `mode`, sorted by path, at most [`CAP`].
fn example_section(mode: ViewerMode, root: Option<&Path>, cancel: &AtomicBool) -> Section {
    let title = if mode == ViewerMode::Lessons { "Lesson folders" } else { "Examples" };
    let Some(root) = root else {
        return Section { title: title.into(), empty: "No workspace root was found, so no examples are listed.".into(), choices: Vec::new() };
    };
    // Whether any walk stopped early (its budget, or the job cancelled).
    let cut = std::cell::Cell::new(false);
    let walk = |dir: &str, depth: usize| {
        let w = tree(&root.join(dir), depth, cancel);
        cut.set(cut.get() || w.cut);
        w.tree
    };
    let examples = || walk("examples", DEPTH);
    let (mut found, what) = match mode {
        ViewerMode::Robot => (files(&examples(), ".simrobot.json"), "*.simrobot.json files under examples/"),
        ViewerMode::Build => (files(&examples(), ".system.json"), "*.system.json files under examples/"),
        ViewerMode::Cad => {
            let mut found = files(&examples(), ".rcad");
            found.extend(files(&walk("cad", DEPTH), ".rcad"));
            (found, "*.rcad files under examples/ or cad/")
        }
        ViewerMode::Place => (dirs_holding(&examples(), "place.json"), "places (folders holding place.json) under examples/"),
        ViewerMode::Lessons => {
            let mut found = lesson_dirs(&walk("lessons", 1));
            found.extend(lesson_dirs(&examples()));
            (found, "lessons folders in lessons/ or under examples/")
        }
        ViewerMode::Inspect => (inspect_pairs(&examples()), "*.description.json files with a *.spatial.json beside them under examples/"),
        ViewerMode::Phenomena => (Vec::new(), "documents (phenomena mode takes none)"),
    };
    let cut = cut.get();
    found.sort();
    found.dedup();
    let total = found.len();
    found.truncate(CAP);
    // A cut section says so: past CAP, or a walk that stopped before the end.
    let title = match (total > CAP, cut) {
        (true, false) => format!("{title} (first {CAP} of {total})"),
        (true, true) => format!("{title} (first {CAP}; the search stopped early)"),
        (false, true) => format!("{title} (the search stopped early)"),
        (false, false) => title.to_string(),
    };
    let empty = if cut { format!("No {what} in the first {VISITED} entries of {}.", root.display()) } else { format!("No {what} in {}.", root.display()) };
    let choices = found.into_iter().map(|p| Choice { label: relative(Some(root), &p), detail: String::new(), enabled: true, document: Document::Path(p) }).collect();
    Section { title, empty, choices }
}

/// What the picker offers for `mode`: recent documents, then (robot)
/// presets, then local CAD archives and the
/// workspace's examples. Reads files: call it on `Pool::Io`, with the job's
/// cancel flag (`jobs::Ctx::cancel_flag`): the walk stops once it is set.
pub(crate) fn discover(mode: ViewerMode, root: Option<PathBuf>, presets: Option<PathBuf>, recents: Recents, ready: bool, cancel: &AtomicBool) -> Sources {
    let root = root.as_deref();
    let mut sections = vec![recent_section(mode, root, &recents, ready)];
    match mode {
        ViewerMode::Robot => {
            sections.push(preset_section(root, presets.as_deref()));
            sections.push(example_section(mode, root, cancel));
        }
        ViewerMode::Cad => {
            sections.push(example_section(mode, root, cancel));
        }
        ViewerMode::Phenomena => {}
        _ => sections.push(example_section(mode, root, cancel)),
    }
    let start_dir = root.map(|r| {
        let examples = r.join("examples");
        let dir = if examples.is_dir() { examples } else { r.to_path_buf() };
        format!("{}/", dir.display().to_string().trim_end_matches('/'))
    });
    Sources { sections, start_dir }
}
