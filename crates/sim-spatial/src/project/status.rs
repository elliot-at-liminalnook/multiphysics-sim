//! Where a project stands: each step's state, read from its files on a
//! `Pool::Io` job (the CAD archive's manifest, the exported model's
//! source block, the latest test report, the lessons folder, the part
//! files), then joined with what the window knows now (unsaved CAD edits,
//! an export or a test running). The job runs again only when a file's
//! size or modification time moved.
use super::Step;
use serde::Serialize;
use serde_json::{Value, json};
use sim_runtime::robot_project::Project;
use std::path::Path;
use std::time::SystemTime;

/// done | ready (the next thing to do) | attention (something to fix) | waiting (an earlier step first).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Done,
    Ready,
    Attention,
    Waiting,
}
impl State {
    pub fn mark(self) -> &'static str {
        match self {
            State::Done => "✓",
            State::Ready => "●",
            State::Attention => "!",
            State::Waiting => "○",
        }
    }
}

/// One step as shown.
#[derive(Clone, Debug, Serialize)]
pub struct StepStatus {
    pub step: Step,
    pub state: State,
    /// What is true now, in a sentence.
    pub line: String,
    /// What to do next (None when done).
    pub hint: Option<String>,
}

/// What the files say (the job's answer).
#[derive(Clone, Debug, Default, Serialize)]
pub struct Files {
    pub cad_exists: bool,
    pub cad_sha256: Option<String>,
    pub cad_revision: Option<u64>,
    pub bodies: usize,
    pub joints: usize,
    pub driven_joints: usize,
    pub motors: usize,
    /// (name, lower, upper) of each motorized joint (rad; for a starting test).
    pub driven: Vec<(String, Option<f64>, Option<f64>)>,
    /// (motor name, library spec) of each motor.
    pub motor_specs: Vec<(String, String)>,
    /// Material ids the bodies use.
    pub materials: Vec<String>,
    /// Sensor kinds (encoder, imu, current, force).
    pub sensors: Vec<String>,
    pub battery: bool,
    pub cad_error: Option<String>,
    pub model_exists: bool,
    pub model_cad_sha256: Option<String>,
    pub model_revision: Option<u64>,
    pub model_exporter: Option<String>,
    pub assumptions: Vec<Value>,
    pub blocking: usize,
    pub model_links: usize,
    pub model_error: Option<String>,
    pub report: Option<Value>,
    pub report_path: Option<String>,
    /// Whether the latest report still describes what a test would run now
    /// (the system, every file it reads, the settings, the test); None
    /// without a stated test.
    pub standing: Option<sim_runtime::system_evidence::Standing>,
    pub system_exists: bool,
    pub system_error: Option<String>,
    pub lessons: Vec<(String, String)>,
    pub make: Option<Value>,
}

/// The files' identity: sizes and modification times (the job reruns when it moves).
pub fn key(project: &Project) -> String {
    let stat = |p: &Path| std::fs::metadata(p).ok().map(|m| (m.len(), m.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_millis())));
    let dir = |p: &Path| std::fs::read_dir(p).map(|d| d.flatten().filter_map(|e| Some((e.file_name(), e.metadata().ok()?.modified().ok()?))).collect::<Vec<_>>()).ok();
    let judged: Vec<_> = project.file.test.iter().flat_map(|t| judged_paths(project, t)).map(|(_, p)| stat(&p)).collect();
    format!("{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}", stat(&project.path), stat(&project.cad()), stat(&project.model()), stat(&project.system()), system_files(project), judged, dir(&project.results()), dir(&project.lessons()), stat(&project.make().join("parts.json")))
}

/// Size and modification time of every file the project's system reads
/// (FMU archives, generated sources), so a changed controller is noticed.
fn system_files(project: &Project) -> Vec<(String, Option<(u64, Option<u128>)>)> {
    let system = project.system();
    let Ok(document) = sim_system::SystemStore::new(&system).load() else { return Vec::new() };
    let base = system.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut files: Vec<String> = document.definitions.values().flat_map(|d| d.instances.values()).filter_map(|i| match &i.kind {
        sim_system::InstanceKind::Block { implementation: sim_system::BlockSource::Fmu { path, .. }, .. } => Some(path.clone()),
        sim_system::InstanceKind::Generated { source, .. } => Some(source.clone()),
        _ => None,
    }).collect();
    files.sort();
    files.dedup();
    files.into_iter().map(|f| {
        let stat = std::fs::metadata(base.join(&f)).ok().map(|m| (m.len(), m.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_millis())));
        (f, stat)
    }).collect()
}

/// The files a judgement of `test` reads beyond the system's own: the CAD
/// file's part meshes and the print registry, for printed-part strength.
fn judged_paths(project: &Project, test: &sim_runtime::acceptance::Test) -> Vec<(String, std::path::PathBuf)> {
    if !test.criteria.iter().any(|c| matches!(c, sim_runtime::acceptance::Criterion::PartStrength { .. })) {
        return Vec::new();
    }
    let mut out = vec![("the design's part meshes (CAD file)".to_string(), project.cad())];
    if let Ok(registry) = crate::workspace::path("library/printing/registry.json") {
        out.push(("the print registry".to_string(), registry));
    }
    out
}

/// [`judged_paths`] with each file's SHA-256, for the test's fingerprint.
pub(crate) fn judged_files(project: &Project, test: &sim_runtime::acceptance::Test) -> std::collections::BTreeMap<String, String> {
    judged_paths(project, test).into_iter().map(|(name, path)| (name, sim_domain_robot::cad_link::sha256_file(&path).unwrap_or_else(|e| format!("unreadable: {e}")))).collect()
}

/// Read the files (on a job).
pub fn read(project: &Project) -> Files {
    let mut f = Files::default();
    let cad = project.cad();
    f.cad_exists = cad.is_file();
    if f.cad_exists {
        match sim_cad::ArchiveDocument::open(&cad) {
            Ok(a) => {
                f.cad_sha256 = Some(a.identity().trim_start_matches("sha256:").to_string());
                f.cad_revision = a.manifest["revision"].as_u64();
                let nodes: Vec<&Value> = a.manifest["nodes"].as_array().map(|n| n.iter().collect()).unwrap_or_default();
                f.bodies = nodes.iter().filter(|n| n["kind"] == "body" && n["robot"]["kind"] != "motor").count();
                f.motors = nodes.iter().filter(|n| n["robot"]["kind"] == "motor").count();
                f.motor_specs = nodes.iter().filter(|n| n["robot"]["kind"] == "motor").map(|n| (n["name"].as_str().unwrap_or("motor").to_string(), n["robot"]["spec"].as_str().unwrap_or("").to_string())).collect();
                let mut materials: Vec<String> = nodes.iter().filter(|n| n["kind"] == "body" && n["robot"]["kind"] != "motor").filter_map(|n| n["material"].as_str().map(str::to_string)).collect();
                materials.sort();
                materials.dedup();
                f.materials = materials;
                let mut sensors: Vec<String> = nodes.iter().filter(|n| n["kind"] == "sensor").filter_map(|n| n["robot"]["kind"].as_str().map(str::to_string)).collect();
                sensors.sort();
                sensors.dedup();
                f.sensors = sensors;
                f.battery = a.manifest["robot_settings"]["battery"].is_object();
                for n in nodes.iter().filter(|n| n["kind"] == "joint" && n["joint"].is_object()) {
                    let j = &n["joint"];
                    if j["type"] == "fixed" {
                        continue;
                    }
                    f.joints += 1;
                    if j["motor"].is_string() {
                        f.driven_joints += 1;
                        let scale = if j["type"] == "prismatic" { 1e-3 } else { 1.0 };
                        f.driven.push((n["name"].as_str().unwrap_or("joint").to_string(), j["lower"].as_f64().map(|x| x * scale), j["upper"].as_f64().map(|x| x * scale)));
                    }
                }
            }
            Err(e) => f.cad_error = Some(e),
        }
    }
    let model = project.model();
    f.model_exists = model.is_file();
    if f.model_exists {
        match std::fs::read_to_string(&model).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string())) {
            Ok(m) => {
                let s = &m["source"];
                f.model_cad_sha256 = s["cad_sha256"].as_str().map(|h| h.trim_start_matches("sha256:").to_string());
                f.model_revision = s["cad_revision"].as_u64();
                f.model_exporter = s["exporter"].as_str().map(str::to_string);
                f.assumptions = s["assumptions"].as_array().cloned().unwrap_or_default();
                f.blocking = f.assumptions.iter().filter(|a| a["blocking"] == json!(true)).count();
                f.model_links = m["links"].as_array().map_or(0, Vec::len);
            }
            Err(e) => f.model_error = Some(e),
        }
    }
    if let Some((path, report)) = project.latest_report() {
        f.report_path = Some(path.display().to_string());
        f.report = Some(report);
    }
    let system = project.system();
    f.system_exists = system.is_file();
    if let Some(test) = &project.file.test {
        f.standing = Some(if f.report.is_none() {
            sim_runtime::system_evidence::Standing::NotAssessed
        } else {
            match sim_system::SystemStore::new(&system).load() {
                Ok(document) => {
                    let base = system.parent().unwrap_or(Path::new("."));
                    sim_runtime::acceptance::standing(f.report.as_ref(), &sim_runtime::acceptance::fingerprint(&document, base, test, &judged_files(project, test)))
                }
                Err(e) => {
                    f.system_error = Some(format!("{}: {e}", system.display()));
                    sim_runtime::acceptance::stale(f.report.as_ref().expect("checked"), "the project's system file cannot be read")
                }
            }
        });
    }
    f.lessons = sim_lessons(&project.lessons());
    f.make = std::fs::read_to_string(project.make().join("parts.json")).ok().and_then(|t| serde_json::from_str(&t).ok());
    f
}

/// (slug, title) of each `<slug>/lesson.md` in `dir`.
pub fn sim_lessons(dir: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let md = e.path().join("lesson.md");
            let text = std::fs::read_to_string(&md).ok()?;
            let title = text.lines().find_map(|l| l.strip_prefix("title:")).map(|t| t.trim().trim_matches('"').to_string()).unwrap_or_else(|| e.file_name().to_string_lossy().into_owned());
            Some((e.file_name().to_string_lossy().into_owned(), title))
        })
        .collect();
    out.sort();
    out
}

/// What the window knows that the files do not.
#[derive(Clone, Copy, Debug, Default)]
pub struct Live {
    /// CAD mode has the project's file open with unsaved edits.
    pub unsaved: bool,
    pub exporting: bool,
    pub testing: bool,
}

/// The steps from the files and the window.
pub fn steps(project: &Project, f: &Files, live: Live) -> Vec<StepStatus> {
    let mut out = Vec::new();
    // 1 Design.
    let design = if !f.cad_exists {
        (State::Ready, "No CAD file yet".to_string(), Some("Describe the robot to the design assistant, or start modelling in CAD".to_string()))
    } else if let Some(e) = &f.cad_error {
        (State::Attention, format!("The CAD file could not be read: {e}"), Some("Open it in CAD to see what is wrong".into()))
    } else if f.bodies == 0 {
        (State::Ready, "The CAD file is empty".into(), Some("Ask the design assistant to build it, or add parts in CAD".into()))
    } else if f.driven_joints == 0 {
        (State::Ready, format!("{} part(s), {} joint(s), {} motor(s): nothing is driven yet", f.bodies, f.joints, f.motors), Some("Add a joint with a motor on it (CAD: Robot → Add joint, Add motor)".into()))
    } else if live.unsaved {
        (State::Attention, format!("{} part(s), {} driven joint(s); unsaved edits", f.bodies, f.driven_joints), Some("Save the design (Ctrl+S) so the model and the test describe it".into()))
    } else {
        (State::Done, format!("{} part(s), {} joint(s) ({} driven), {} motor(s)", f.bodies, f.joints, f.driven_joints, f.motors), None)
    };
    let designed = design.0 == State::Done || (design.0 == State::Attention && f.driven_joints > 0);
    out.push(StepStatus { step: Step::Design, state: design.0, line: design.1, hint: design.2 });
    // 2 Model.
    let current = f.model_exists && f.model_cad_sha256.is_some() && f.model_cad_sha256 == f.cad_sha256;
    let model = if live.exporting {
        (State::Waiting, "Exporting the simulation model…".to_string(), None)
    } else if !designed {
        (State::Waiting, "Needs a design with a driven joint".into(), Some("Finish step 1 first".into()))
    } else if !f.model_exists {
        (State::Ready, "No simulation model yet".into(), Some("Make the model: the CAD's parts, joints and motors become a physics model".into()))
    } else if let Some(e) = &f.model_error {
        (State::Attention, format!("The model could not be read: {e}"), Some("Make the model again".into()))
    } else if !current {
        (State::Attention, "The model is older than the CAD file".into(), Some("Make the model again from the saved design".into()))
    } else if live.unsaved {
        (State::Attention, "The design has changed since the model was made (unsaved edits)".into(), Some("Make the model again: it saves the design first".into()))
    } else if f.blocking > 0 {
        (State::Attention, format!("{} value(s) must be set before tests count ({} estimate(s) listed)", f.blocking, f.assumptions.len() - f.blocking), Some("Fix the blocking items listed under Model, then make it again".into()))
    } else {
        (State::Done, format!("{} link(s); {} estimate(s) listed", f.model_links, f.assumptions.len()), None)
    };
    let modelled = model.0 == State::Done;
    out.push(StepStatus { step: Step::Model, state: model.0, line: model.1, hint: model.2 });
    // 3 Test.
    use sim_runtime::system_evidence::Standing;
    let test = if live.testing {
        (State::Waiting, "Running the test…".to_string(), None)
    } else if !modelled {
        (State::Waiting, "Needs a current model with nothing blocking".into(), Some("Finish step 2 first".into()))
    } else if project.file.test.is_none() {
        (State::Ready, "No test stated yet".into(), Some("Run the test: a starting test is made from the robot's joints, to edit".into()))
    } else {
        match (&f.standing, &f.report) {
            (Some(Standing::Current { .. }), Some(r)) => match (r["verdict"].as_str(), r["evidence"].as_bool()) {
                (Some("passed"), Some(true)) => (State::Done, r["summary"].as_str().unwrap_or("passed").to_string(), None),
                (Some("failed"), _) => (State::Attention, r["summary"].as_str().unwrap_or("failed").to_string(), Some("Change the design (or the test) and run it again".into())),
                _ => (State::Attention, r["summary"].as_str().unwrap_or("incomplete").to_string(), Some("Some criteria could not be assessed; see Test".into())),
            },
            // A result for something that has since changed is not a result for this.
            (Some(Standing::Stale { changed, .. }), _) => (State::Ready, format!("Not tested since {}", changed.join("; ")), Some("Run the test again: the last result no longer describes this".into())),
            _ => (State::Ready, "Not tested yet".into(), Some("Run the test".into())),
        }
    };
    let tested = test.0 == State::Done;
    out.push(StepStatus { step: Step::Test, state: test.0, line: test.1, hint: test.2 });
    // 4 Learn (never blocks).
    let learn = if f.lessons.is_empty() {
        (State::Ready, "No lessons for this robot yet".to_string(), Some("Pick a suggested lesson and have it written for this robot".to_string()))
    } else {
        (State::Done, format!("{} lesson(s) for this robot", f.lessons.len()), Some("Open them, or write another".to_string()))
    };
    out.push(StepStatus { step: Step::Learn, state: learn.0, line: learn.1, hint: learn.2 });
    // 5 Make.
    let made_current = f.make.as_ref().is_some_and(|m| m["cad_sha256"].as_str() == f.cad_sha256.as_deref() && f.cad_sha256.is_some());
    let make = if !tested {
        (State::Waiting, "Needs a passed test of the current design".to_string(), Some("Finish step 3 first".to_string()))
    } else if !made_current {
        (State::Ready, "No part files for the tested design yet".into(), Some("Export the printable parts of the tested design".into()))
    } else {
        let n = f.make.as_ref().and_then(|m| m["parts"].as_array().map(Vec::len)).unwrap_or(0);
        (State::Done, format!("{n} part file(s) from the tested design"), None)
    };
    out.push(StepStatus { step: Step::Make, state: make.0, line: make.1, hint: make.2 });
    out
}

/// The first step that is not done (the one to do next).
pub fn next(steps: &[StepStatus]) -> Option<Step> {
    steps.iter().find(|s| s.state != State::Done && s.step != Step::Learn).map(|s| s.step)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_runtime::robot_project::{Project, ProjectFile};

    fn project() -> Project {
        Project { path: "/w/arm.robot.json".into(), file: ProjectFile::new("arm", "").unwrap() }
    }

    #[test]
    fn steps_follow_the_files_in_order() {
        let p = project();
        let s = steps(&p, &Files::default(), Live::default());
        assert_eq!(s.iter().map(|s| s.state).collect::<Vec<_>>(), [State::Ready, State::Waiting, State::Waiting, State::Ready, State::Waiting]);
        assert_eq!(next(&s), Some(Step::Design));
        let mut f = Files { cad_exists: true, cad_sha256: Some("aa".into()), bodies: 2, joints: 1, driven_joints: 1, motors: 1, ..Default::default() };
        // (Unsaved edits after a current model make the model stale too: checked below.)
        let s = steps(&p, &f, Live::default());
        assert_eq!((s[0].state, s[1].state), (State::Done, State::Ready));
        // A stale model, then a current one with a blocking value.
        f.model_exists = true;
        f.model_cad_sha256 = Some("bb".into());
        assert_eq!(steps(&p, &f, Live::default())[1].state, State::Attention);
        f.model_cad_sha256 = Some("aa".into());
        f.blocking = 1;
        f.assumptions = vec![json!({"blocking": true})];
        let s = steps(&p, &f, Live::default());
        assert!(s[1].line.contains("must be set"));
        f.blocking = 0;
        f.assumptions.clear();
        let s = steps(&p, &f, Live { unsaved: true, ..Default::default() });
        assert_eq!(s[0].state, State::Attention, "unsaved edits");
        assert_eq!((s[1].state, s[2].state), (State::Attention, State::Waiting), "the model no longer describes the design");
        // A passed report of this model; then Make is next.
        let mut p2 = p.clone();
        p2.file.test = sim_runtime::robot_project::starting_test(&[("j".into(), None, Some(1.0))]);
        f.report = Some(json!({"model": {"cad_sha256": "aa"}, "verdict": "passed", "evidence": true, "summary": "Passed all 4 criteria."}));
        // A report alone proves nothing about what is there now.
        assert_eq!(steps(&p2, &f, Live::default())[2].state, State::Ready);
        use sim_runtime::system_evidence::{Standing, Verdict};
        f.standing = Some(Standing::Current { verdict: Verdict::Passed });
        let s = steps(&p2, &f, Live::default());
        assert_eq!((s[2].state, s[4].state), (State::Done, State::Ready));
        assert_eq!(next(&s), Some(Step::Make));
        // The same passed report after the test (or a controller, or the model) changed: not a pass, and nothing to make.
        f.standing = Some(Standing::Stale { verdict: Verdict::Passed, changed: vec!["the test changed".into()] });
        let s = steps(&p2, &f, Live::default());
        assert_eq!((s[2].state, s[4].state), (State::Ready, State::Waiting));
        assert!(s[2].line.contains("the test changed"), "{}", s[2].line);
        assert_eq!(next(&s), Some(Step::Test));
    }
}
