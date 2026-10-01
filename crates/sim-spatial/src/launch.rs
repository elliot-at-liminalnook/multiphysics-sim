//! `sim-spatial FILE`: choose the viewer mode from what FILE is. Detection is
//! by name or directory structure only, never by reading content:
//!
//! 1. a file named `*.system.json` → build mode (as `--system FILE`);
//! 2. a file named `*.simrobot.json` → robot mode (as `--robot FILE`);
//! 3. a directory holding `place.json` → place mode (a `sim-place build`
//!    directory, as `--place DIR`);
//! 4. a directory with at least one `<slug>/lesson.md` child → lessons mode
//!    (as `--lessons DIR`);
//! 5. a file named `*.rcad` → CAD mode (RoboCAD's headless service is
//!    started on it; `--cad-url URL` attaches to a running one instead).
//!
//! Anything else, including a path that does not exist, is an error naming
//! the path and these accepted types.

use std::path::Path;

/// The mode a positional FILE opens in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchKind {
    System,
    Robot,
    Place,
    Lessons,
    Cad,
}

/// The accepted types, as listed in errors and `--help`.
pub const ACCEPTED: &str = "a *.system.json file (build mode), a *.simrobot.json file (robot mode), a directory holding place.json (place mode), a directory with <slug>/lesson.md entries (lessons mode), or a *.rcad file (CAD mode)";

/// Classify `path` by the rule in the module doc.
pub fn classify(path: &Path) -> Result<LaunchKind, String> {
    let reject = |why: &str| Err(format!("{}: {why}; expected {ACCEPTED}", path.display()));
    if path.is_file() {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if name.ends_with(".system.json") {
            return Ok(LaunchKind::System);
        }
        if name.ends_with(".simrobot.json") {
            return Ok(LaunchKind::Robot);
        }
        if name.ends_with(".rcad") {
            return Ok(LaunchKind::Cad);
        }
        return reject("not a recognised file type");
    }
    if path.is_dir() {
        if path.join("place.json").is_file() {
            return Ok(LaunchKind::Place);
        }
        let lessons = std::fs::read_dir(path)
            .map(|entries| entries.flatten().any(|e| e.path().join("lesson.md").is_file()))
            .unwrap_or(false);
        if lessons {
            return Ok(LaunchKind::Lessons);
        }
        return reject("directory holds neither place.json nor <slug>/lesson.md");
    }
    reject("no such file or directory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_by_name_and_structure() {
        let tmp = std::env::temp_dir().join(format!("sim-spatial-launch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("place")).unwrap();
        std::fs::create_dir_all(tmp.join("lessons/a")).unwrap();
        std::fs::create_dir_all(tmp.join("empty")).unwrap();
        for f in ["x.system.json", "x.simrobot.json", "x.rcad", "x.json", "place/place.json", "lessons/a/lesson.md"] {
            std::fs::write(tmp.join(f), "{}").unwrap();
        }
        assert_eq!(classify(&tmp.join("x.system.json")), Ok(LaunchKind::System));
        assert_eq!(classify(&tmp.join("x.simrobot.json")), Ok(LaunchKind::Robot));
        assert_eq!(classify(&tmp.join("x.rcad")), Ok(LaunchKind::Cad));
        assert_eq!(classify(&tmp.join("place")), Ok(LaunchKind::Place));
        assert_eq!(classify(&tmp.join("lessons")), Ok(LaunchKind::Lessons));
        for bad in ["x.json", "missing.system.json", "missing.rcad", "empty"] {
            let e = classify(&tmp.join(bad)).unwrap_err();
            assert!(e.contains(&tmp.join(bad).display().to_string()), "{e}");
            assert!(e.contains(ACCEPTED), "{e}");
        }
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
