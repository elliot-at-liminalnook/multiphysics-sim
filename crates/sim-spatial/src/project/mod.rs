//! Robot projects in the window: one robot taken through the steps in
//! order (Design → Model → Test → Learn → Make), with every mode pointed at
//! the same robot.
//!
//! - **The project** is a `*.robot.json` (`sim_runtime::robot_project`):
//!   the robot's CAD file, its simulation model, the acceptance test, the
//!   design conversation, and its results, lessons and part folders.
//! - **One current project per window** ([`ProjectState`]). Opening one
//!   remembers its documents for CAD, Robot and Lessons (the document
//!   registry), so the mode switcher reopens them; each step's button
//!   switches to its mode on the project's document, using the mode's own
//!   open when it is already showing another (CAD's Open, `robot_open`).
//! - **Steps** ([`status`]) are read from the files on a job and joined
//!   with what the window knows (unsaved CAD edits, an export or a test
//!   running). The bottom strip of every mode shows them; the project
//!   panel shows the current one in detail ([`panel`]).
//! - **Following the modes**: when CAD or Robot mode opens a file that
//!   another project owns (its own Open, the picker), that project becomes
//!   the window's (`actions::follow_documents`).
//! - **Follow-ups** ([`Intent`]): a step that needs another mode first
//!   (Model needs CAD on the project's file; asking the design assistant
//!   needs CAD) switches, then acts once the mode is ready.
//! - **Test**: the acceptance test runs headless on a job
//!   (`sim_runtime::acceptance`); its report is written under the
//!   project's results folder and Robot mode shows the model.
//! - REST: `project_*` in every mode ([`actions`]); `project_guide` explains
//!   the order for an agent starting cold.
mod actions;
mod chat;
mod guide;
mod lessons;
mod make;
mod panel;
pub(crate) mod status;
mod strength;

pub(crate) use actions::ProjectAction;
pub(crate) use panel::ProjectStripRoot;

use crate::app::{ViewerMode, ViewerSet};
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_runtime::robot_project::Project;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How often an open project's files are checked for changes.
const PROBE: Duration = Duration::from_secs(2);

/// The steps, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Design,
    Model,
    Test,
    Learn,
    Make,
}
impl Step {
    pub const ALL: [Step; 5] = [Step::Design, Step::Model, Step::Test, Step::Learn, Step::Make];
    pub fn label(self) -> &'static str {
        match self {
            Step::Design => "Design",
            Step::Model => "Model",
            Step::Test => "Test",
            Step::Learn => "Learn",
            Step::Make => "Make",
        }
    }
    pub fn number(self) -> usize {
        Step::ALL.iter().position(|s| *s == self).unwrap_or(0) + 1
    }
    /// The mode the step works in.
    pub fn mode(self) -> ViewerMode {
        match self {
            Step::Design | Step::Model | Step::Make => ViewerMode::Cad,
            Step::Test => ViewerMode::Robot,
            Step::Learn => ViewerMode::Lessons,
        }
    }
    /// What the step is, for people.
    pub fn about(self) -> &'static str {
        match self {
            Step::Design => "Shape the robot in CAD: parts, materials, joints and the motors that drive them.",
            Step::Model => "Turn the saved design into a physics model: exact masses and inertias, joints, motors and every estimate listed.",
            Step::Test => "Run the model through what the robot must do and judge it against stated criteria.",
            Step::Learn => "Lessons about this robot's parts and results, written for it.",
            Step::Make => "Part files of the tested design, for printing.",
        }
    }
}

/// Something to do once a mode is ready.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Intent {
    /// Save the CAD file if needed, then export the model (CAD on the project's file).
    Export { saving: bool, started: bool },
    /// Ask the design assistant (CAD on the project's file).
    Ask(String),
    /// Run the acceptance test once the model is current.
    Test,
    /// Export the part files (CAD on the project's file, saved).
    Make,
    /// Open the project's lesson `slug` in Lessons mode (just written).
    OpenLessons(String),
}

/// A running acceptance test.
pub(crate) struct TestRun {
    job: Job<Value>,
    started: Instant,
    pub progress: std::sync::Arc<std::sync::Mutex<f64>>,
}

/// The window's current project (see the module doc).
#[derive(Resource, Default)]
pub struct ProjectState {
    pub open: Option<Project>,
    pub files: Option<status::Files>,
    pub steps: Vec<status::StepStatus>,
    /// The status job: the files' key and, when they moved, what they say.
    status_job: Option<Job<(String, Option<status::Files>)>>,
    status_key: Option<String>,
    checked: Option<Instant>,
    pub(crate) test: Option<TestRun>,
    pub(crate) intents: Vec<Intent>,
    /// The last outcome, for the strip and the panel.
    pub message: Option<Result<String, String>>,
    pub(crate) panel: panel::PanelState,
    pub(crate) chat: chat::DesignChat,
    pub(crate) lessons: lessons::LessonWriter,
    pub(crate) make: make::MakeRun,
    /// Bumped on every change shown.
    pub revision: u64,
}
impl ProjectState {
    pub(crate) fn touch(&mut self) {
        self.revision += 1;
    }
    pub(crate) fn say(&mut self, m: Result<String, String>) {
        self.message = Some(m);
        self.touch();
    }
    pub fn step(&self, step: Step) -> Option<&status::StepStatus> {
        self.steps.iter().find(|s| s.step == step)
    }
    /// The project's state as REST shows it.
    pub fn json(&self) -> Value {
        let Some(p) = &self.open else {
            return json!({"open": false, "message": self.message.as_ref().map(msg), "panel": self.panel.json(), "recent": panel::recent_projects(), "rule": "no project is open: project_new {name, description} starts one from an empty design; project_open {path} opens a *.robot.json"});
        };
        let f = self.files.as_ref();
        json!({
            "open": true, "project": p.json(),
            "steps": self.steps, "next": status::next(&self.steps),
            "model": f.map(|f| json!({"exists": f.model_exists, "cad_sha256": f.model_cad_sha256, "cad_revision": f.model_revision, "assumptions": f.assumptions, "blocking": f.blocking, "links": f.model_links, "exporter": f.model_exporter, "error": f.model_error})),
            "design": f.map(|f| json!({"exists": f.cad_exists, "cad_sha256": f.cad_sha256, "revision": f.cad_revision, "bodies": f.bodies, "joints": f.joints, "driven_joints": f.driven_joints, "motors": f.motors, "error": f.cad_error})),
            "test": {"stated": p.file.test, "running": self.test.as_ref().map(|t| json!({"seconds": t.started.elapsed().as_secs_f64(), "progress": *t.progress.lock().unwrap_or_else(|e| e.into_inner())})), "latest": f.and_then(|f| f.report.clone()), "latest_path": f.and_then(|f| f.report_path.clone())},
            "lessons": {"written": f.map(|f| f.lessons.iter().map(|(slug, title)| json!({"slug": slug, "title": title})).collect::<Vec<_>>()), "suggested": lessons::suggestions(self), "writer": self.lessons.json()},
            "make": {"latest": f.and_then(|f| f.make.clone()), "running": self.make.json()},
            "chat": {"turns": p.file.chat, "assistant": self.chat.json()},
            "intents": self.intents.iter().map(|i| format!("{i:?}")).collect::<Vec<_>>(),
            "message": self.message.as_ref().map(msg),
            "panel": self.panel.json(),
        })
    }
}

pub(crate) fn msg(m: &Result<String, String>) -> Value {
    match m {
        Ok(t) => json!({"ok": t}),
        Err(e) => json!({"error": e}),
    }
}

/// Where new projects go by default: `<workspace>/projects/<slug>`.
pub fn default_dir(name: &str) -> Result<PathBuf, String> {
    Ok(crate::workspace::path("projects")?.join(sim_runtime::robot_project::slug(name)))
}

/// JobResults: the status job lands, and a new one starts when the files
/// moved; the test job lands and its report is written.
fn refresh(mut st: ResMut<ProjectState>, cad: Option<Res<crate::cad::CadDocument>>) {
    let now = Instant::now();
    // Read through a shared borrow first: a ResMut deref marks the state changed.
    let landed = st.status_job.as_ref().and_then(|j| j.poll());
    let live = live(&st, cad.as_deref());
    if let Some(result) = landed {
        let st = &mut *st;
        st.status_job = None;
        if let Ok((key, files)) = result {
            st.status_key = Some(key);
            if let Some(files) = files {
                if let Some(p) = &st.open {
                    st.steps = status::steps(p, &files, live);
                }
                st.files = Some(files);
                st.touch();
            }
        }
    }
    // Live changes (unsaved edits, an export or a test starting or ending) without a file change.
    if let (Some(p), Some(f)) = (&st.open, &st.files) {
        let steps = status::steps(p, f, live);
        let changed = steps.len() != st.steps.len() || steps.iter().zip(&st.steps).any(|(a, b)| a.state != b.state || a.line != b.line);
        if changed {
            st.steps = steps;
            st.touch();
        }
    }
    if st.open.is_some() && st.status_job.is_none() && st.checked.is_none_or(|t| now.duration_since(t) >= PROBE) {
        let st = &mut *st;
        st.checked = Some(now);
        let project = st.open.clone().expect("checked");
        let known = st.status_key.clone();
        st.status_job = Some(Job::spawn(Pool::Io, 0, "project status", move |_| {
            let key = status::key(&project);
            let files = (known.as_deref() != Some(key.as_str())).then(|| status::read(&project));
            Ok((key, files))
        }));
    }
    if let Some(result) = st.test.as_ref().and_then(|t| t.job.poll()) {
        test_landed(&mut st, result);
    }
}

/// The window's live facts for the steps.
fn live(st: &ProjectState, cad: Option<&crate::cad::CadDocument>) -> status::Live {
    let on_project = |doc: &crate::cad::CadDocument| st.open.as_ref().is_some_and(|p| crate::cad::for_project::file(doc) == Some(p.cad().as_path()));
    status::Live {
        unsaved: cad.is_some_and(|d| on_project(d) && crate::cad::for_project::unsaved(d)),
        exporting: cad.is_some_and(|d| on_project(d) && crate::cad::for_project::exporting(d)),
        testing: st.test.is_some(),
    }
}

/// Start the acceptance test of the project's model (on a dedicated job).
/// With no test stated yet, a starting test is made from the model's driven
/// joints and saved into the project (labelled as a starting point).
pub(crate) fn start_test(st: &mut ProjectState) -> Result<Value, String> {
    let p = st.open.clone().ok_or("no project is open")?;
    if st.test.is_some() {
        return Err("the test is already running".into());
    }
    let model_path = p.model();
    if !model_path.is_file() {
        return Err("there is no simulation model yet: make it first (step 2, Model)".into());
    }
    if let Some(f) = &st.files
        && f.blocking > 0
    {
        return Err(format!("the model has {} blocking value(s) to set first (see Model); a test of it would not count", f.blocking));
    }
    let mut project = p.clone();
    if project.file.test.is_none() {
        let driven = st.files.as_ref().map(|f| f.driven.clone()).unwrap_or_default();
        let test = sim_runtime::robot_project::starting_test(&driven).ok_or("the design has no driven joint to test")?;
        project.file.test = Some(test);
        project.save()?;
        st.open = Some(project.clone());
    }
    let test = project.file.test.clone().expect("set above");
    let (results, cad) = (project.results(), project.cad());
    let progress = std::sync::Arc::new(std::sync::Mutex::new(0.0));
    let shared = progress.clone();
    let job = Job::spawn(Pool::Dedicated, 0, format!("project test: {}", test.name), move |ctx| {
        let text = std::fs::read_to_string(&model_path).map_err(|e| format!("{}: {e}", model_path.display()))?;
        let model: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", model_path.display()))?;
        let mut report = sim_runtime::acceptance::run(&model, &test, &|| ctx.cancelled(), &|f| *shared.lock().unwrap_or_else(|e| e.into_inner()) = f)?;
        // Printed-part strength: judged here, from the design's part meshes and the run's peak loads.
        for (i, c) in test.criteria.iter().enumerate() {
            if let sim_runtime::acceptance::Criterion::PartStrength { min_safety_factor } = c {
                ctx.message("Checking the printed parts' strength");
                let (status, measured, detail) = strength::assess(&cad, &model, &report, *min_safety_factor, &|| ctx.cancelled());
                let o = &mut report["outcomes"][i];
                o["status"] = json!(status);
                o["measured"] = measured;
                o["detail"] = json!(detail);
            }
        }
        sim_runtime::acceptance::conclude(&mut report);
        let stamp = crate::robot::recording::stamp(crate::robot::recording::now_ms());
        report["ran_at"] = json!(stamp);
        report["model_path"] = json!(model_path);
        std::fs::create_dir_all(&results).map_err(|e| format!("{}: {e}", results.display()))?;
        let path = results.join(format!("{stamp}.test.json"));
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
        report["report_path"] = json!(path);
        Ok(report)
    });
    st.test = Some(TestRun { job, started: Instant::now(), progress });
    st.say(Ok(format!("Running the test “{}” on the model…", project.file.test.as_ref().map_or("", |t| t.name.as_str()))));
    Ok(json!({"started": true, "test": project.file.test}))
}

fn test_landed(st: &mut ProjectState, result: Result<Value, String>) {
    st.test = None;
    // Read the new report now.
    st.checked = None;
    match result {
        Ok(report) => {
            let summary = report["summary"].as_str().unwrap_or("").to_string();
            let ok = report["verdict"] == "passed";
            st.say(if ok { Ok(format!("Test: {summary}")) } else { Err(format!("Test: {summary}")) });
            st.panel.view = Some(Step::Test);
        }
        Err(e) => st.say(Err(format!("The test could not run: {e}"))),
    }
}

/// Present: `/v1/project_state` follows every change.
fn publish(st: Res<ProjectState>, rest: Option<ResMut<crate::rest::Rest>>, mut last: Local<Option<u64>>) {
    let Some(mut rest) = rest else { return };
    // A running test's progress moves without a revision: publish it at the snapshot rate.
    if *last == Some(st.revision) && !(st.test.is_some() && rest.0.snapshot_due()) {
        return;
    }
    *last = Some(st.revision);
    rest.0.publish("project_state", st.json());
    rest.0.publish_changed("project_guide", "1", || guide::guide(None).unwrap_or_default());
}

/// What the launch asked for: a project to open, or the Start card.
#[derive(Resource, Default)]
pub struct LaunchProject {
    pub path: Option<PathBuf>,
    pub start: bool,
}

/// Startup: open the launch's project (CAD already opened its design), or show Start.
fn launch(launch: Option<Res<LaunchProject>>, mut st: ResMut<ProjectState>, mut out: MessageWriter<crate::app::actions::Act<ProjectAction>>) {
    let Some(launch) = launch else { return };
    if let Some(path) = &launch.path {
        out.write(crate::app::actions::Act::ui(ProjectAction::ProjectOpen { path: path.clone(), step: None }));
    } else if launch.start {
        st.panel.open = true;
        st.touch();
    }
}

/// The project plugin (every mode).
pub struct ProjectPlugin;
impl Plugin for ProjectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProjectState>().add_message::<crate::app::actions::Act<ProjectAction>>().init_resource::<crate::app::actions::InFlight<ProjectAction>>();
        actions::build(app);
        panel::build(app);
        chat::build(app);
        app.add_systems(Update, (actions::follow_documents, refresh, actions::follow_up, lessons::tick, make::tick).chain().in_set(ViewerSet::JobResults))
            .add_systems(Update, publish.in_set(ViewerSet::Present))
            .add_systems(Startup, launch);
    }
}
