//! A robot project: one robot taken from a description to a tested design
//! and its parts (`<dir>/<name>.robot.json`, schema [`SCHEMA`]).
//!
//! The project names its files (relative to its folder) and holds what is
//! not the robot's physical definition: the description the person gave,
//! the acceptance test (what the robot must do and how well), the design
//! conversation, and where results, lessons and part files go. The robot
//! itself is the CAD file; the simulation model is exported from it.
//!
//! Files are written atomically; unknown fields are refused, so a misspelt
//! field fails naming the path.
use crate::acceptance::Test;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "sim.robot-project/1";
/// The project file's name ending.
pub const SUFFIX: &str = ".robot.json";

/// One turn of the design conversation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ChatTurn {
    /// `person` or `assistant`.
    pub role: String,
    pub text: String,
    /// UTC stamp.
    pub at: String,
    /// The assistant run that answered (assistant turns).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

/// The project file.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectFile {
    pub schema: String,
    pub name: String,
    /// What the person asked for, in their words.
    #[serde(default)]
    pub description: String,
    /// The CAD file (relative to the project folder).
    pub cad: String,
    /// The simulation model the CAD exports to.
    pub model: String,
    /// Test reports go here.
    #[serde(default = "default_results")]
    pub results: String,
    /// This robot's lessons (`<slug>/lesson.md`).
    #[serde(default = "default_lessons")]
    pub lessons: String,
    /// Part files for making the robot.
    #[serde(default = "default_make")]
    pub make: String,
    /// The acceptance test; None until stated.
    #[serde(default)]
    pub test: Option<Test>,
    #[serde(default)]
    pub chat: Vec<ChatTurn>,
}
fn default_results() -> String {
    "results".into()
}
fn default_lessons() -> String {
    "lessons".into()
}
fn default_make() -> String {
    "make".into()
}

/// A file name from a robot name: lower case, words joined by `-`.
pub fn slug(name: &str) -> String {
    let s: String = name.trim().to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

impl ProjectFile {
    /// A new project named `name` (files `<slug>.rcad`, `<slug>.simrobot.json`).
    pub fn new(name: &str, description: &str) -> Result<Self, String> {
        let s = slug(name);
        if s.is_empty() {
            return Err("a robot needs a name with at least one letter or digit".into());
        }
        Ok(ProjectFile { schema: SCHEMA.into(), name: name.trim().into(), description: description.trim().into(), cad: format!("{s}.rcad"), model: format!("{s}.simrobot.json"), results: default_results(), lessons: default_lessons(), make: default_make(), test: None, chat: Vec::new() })
    }
    pub fn parse(text: &str) -> Result<Self, String> {
        let p: ProjectFile = serde_json::from_str(text).map_err(|e| format!("not a robot project ({SCHEMA}): {e}"))?;
        p.validate()?;
        Ok(p)
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!("schema is {:?}, not {SCHEMA}", self.schema));
        }
        if self.name.trim().is_empty() {
            return Err("name is empty".into());
        }
        for (field, value) in [("cad", &self.cad), ("model", &self.model), ("results", &self.results), ("lessons", &self.lessons), ("make", &self.make)] {
            let p = Path::new(value);
            if value.is_empty() || p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                return Err(format!("{field} must be a path inside the project folder, not {value:?}"));
            }
        }
        if !self.cad.ends_with(".rcad") {
            return Err(format!("cad must be a .rcad file, not {:?}", self.cad));
        }
        if !self.model.ends_with(".simrobot.json") {
            return Err(format!("model must be a .simrobot.json file, not {:?}", self.model));
        }
        if let Some(t) = &self.test {
            t.validate()?;
        }
        Ok(())
    }
    /// Write to `path` atomically (a temporary file beside it, then a rename).
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: {e}", path.display())
        })
    }
}

/// An open project: its file and where it lives.
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub path: PathBuf,
    pub file: ProjectFile,
}
impl Project {
    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }
    pub fn cad(&self) -> PathBuf {
        self.dir().join(&self.file.cad)
    }
    pub fn model(&self) -> PathBuf {
        self.dir().join(&self.file.model)
    }
    pub fn results(&self) -> PathBuf {
        self.dir().join(&self.file.results)
    }
    pub fn lessons(&self) -> PathBuf {
        self.dir().join(&self.file.lessons)
    }
    pub fn make(&self) -> PathBuf {
        self.dir().join(&self.file.make)
    }
    /// Create the project in `dir` (made if missing): the file, its results,
    /// lessons and make folders. Refused when the project file or its CAD
    /// file already exists (nothing is overwritten).
    pub fn create(dir: &Path, file: ProjectFile) -> Result<Self, String> {
        file.validate()?;
        let path = dir.join(format!("{}{SUFFIX}", slug(&file.name)));
        if path.exists() {
            return Err(format!("{} already exists: open it, or choose another name", path.display()));
        }
        let p = Project { path, file };
        if p.cad().exists() {
            return Err(format!("{} already exists: choose another name", p.cad().display()));
        }
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for d in [p.results(), p.lessons(), p.make()] {
            std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        p.file.save(&p.path)?;
        Ok(p)
    }
    pub fn open(path: &Path) -> Result<Self, String> {
        let path = if path.is_dir() {
            let mut found: Vec<PathBuf> = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(SUFFIX)).collect();
            found.sort();
            match found.len() {
                1 => found.remove(0),
                0 => return Err(format!("{} has no *{SUFFIX} project file", path.display())),
                _ => return Err(format!("{} has several project files; name one", path.display())),
            }
        } else {
            path.to_path_buf()
        };
        if !path.to_string_lossy().ends_with(SUFFIX) {
            return Err(format!("{}: a robot project file ends in {SUFFIX}", path.display()));
        }
        Ok(Project { file: ProjectFile::load(&path)?, path })
    }
    pub fn save(&self) -> Result<(), String> {
        self.file.save(&self.path)
    }
    /// The latest test report in the results folder (by file name: reports
    /// are named by UTC stamp), with its path.
    pub fn latest_report(&self) -> Option<(PathBuf, Value)> {
        let mut reports: Vec<PathBuf> = std::fs::read_dir(self.results()).ok()?.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".test.json")).collect();
        reports.sort();
        let path = reports.pop()?;
        let value = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok())?;
        Some((path, value))
    }
    pub fn json(&self) -> Value {
        json!({"path": self.path, "dir": self.dir(), "name": self.file.name, "description": self.file.description, "cad": self.cad(), "model": self.model(), "results": self.results(), "lessons": self.lessons(), "make": self.make(), "test": self.file.test, "chat": self.file.chat})
    }
}

/// A starting acceptance test for a robot with these driven joints (name,
/// lower, upper; rad): each joint moves from its assembly pose to 60 % of
/// the way to its upper limit (or 1 rad) in 1 s and holds; it must arrive
/// within 3° by 1.5 s, keep 30 % of its stall torque in reserve, stay 20 °C
/// below its rated winding temperature, keep within its limits, and leave
/// every printed part a safety factor of 2 under the run's peak loads. A
/// starting point to edit, not a requirement anyone stated.
pub fn starting_test(joints: &[(String, Option<f64>, Option<f64>)]) -> Option<Test> {
    if joints.is_empty() {
        return None;
    }
    let mut criteria = Vec::new();
    let mut start = std::collections::BTreeMap::new();
    let mut end = std::collections::BTreeMap::new();
    for (name, lower, upper) in joints {
        let goal = match (lower, upper) {
            (_, Some(u)) if *u > 0.0 => 0.6 * u,
            (Some(l), _) if *l < 0.0 => 0.6 * l,
            _ => 1.0,
        };
        start.insert(name.clone(), 0.0);
        end.insert(name.clone(), goal);
        criteria.push(crate::acceptance::Criterion::Reaches { joint: name.clone(), target: goal, tolerance: 3f64.to_radians(), by_s: 1.5 });
    }
    criteria.extend([crate::acceptance::Criterion::TorqueMargin { min: 0.3 }, crate::acceptance::Criterion::WindingTemperature { margin_c: 20.0 }, crate::acceptance::Criterion::NoLimitHits, crate::acceptance::Criterion::PartStrength { min_safety_factor: 2.0 }]);
    Some(Test { name: "move and hold".into(), duration_s: 3.0, trajectory: vec![crate::acceptance::Waypoint { t: 0.0, targets: start }, crate::acceptance::Waypoint { t: 1.0, targets: end }], criteria })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_is_created_opened_and_refuses_to_overwrite() {
        let dir = std::env::temp_dir().join(format!("robot-project-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = Project::create(&dir, ProjectFile::new("Lift Arm", "lift a 20 g payload").unwrap()).unwrap();
        assert_eq!(p.path.file_name().unwrap(), "lift-arm.robot.json");
        assert_eq!(p.cad(), dir.join("lift-arm.rcad"));
        assert!(p.lessons().is_dir() && p.results().is_dir());
        let again = Project::open(&dir).unwrap();
        assert_eq!(again.file, p.file);
        assert!(Project::create(&dir, ProjectFile::new("lift arm", "").unwrap()).unwrap_err().contains("already exists"));
        // Unknown fields and paths outside the folder are refused by name.
        assert!(ProjectFile::parse(r#"{"schema":"sim.robot-project/1","name":"x","cad":"x.rcad","model":"x.simrobot.json","colour":1}"#).unwrap_err().contains("colour"));
        assert!(ProjectFile::parse(r#"{"schema":"sim.robot-project/1","name":"x","cad":"../x.rcad","model":"x.simrobot.json"}"#).unwrap_err().contains("inside the project folder"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_starting_test_moves_each_joint_toward_its_limit() {
        let t = starting_test(&[("shoulder".into(), Some(-0.5), Some(2.0))]).unwrap();
        t.validate().unwrap();
        assert!((t.targets_at(2.0)["shoulder"] - 1.2).abs() < 1e-12);
        assert_eq!(t.criteria.len(), 5);
        assert!(starting_test(&[]).is_none());
        assert_eq!(slug("  My  Robot #2 "), "my-robot-2");
    }
}
