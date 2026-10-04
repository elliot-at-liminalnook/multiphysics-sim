//! The project's actions ([`ProjectAction`]): REST `project_*` in every mode,
//! the strip's step chips and the project panel's buttons, all through
//! [`apply`]. Moving to a step uses the target mode's own entry points (the
//! mode switch, CAD's Open, `robot_open`), so their refusals (unsaved
//! edits, a recording being written) stay theirs and reach the strip's line.
use super::{Intent, ProjectState, Step};
use crate::app::actions::{self, Act, Call, InFlight, Replies, Spec, spec};
use crate::app::switch::{Document, ModeSwitch, WindowAction};
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::{CadAction, CadDocument, for_project};
use crate::document::{DocumentKind, DocumentRegistry, Source};
use crate::robot::{RobotAction, RobotView};
use bevy::ecs::message::Messages;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::robot_project::{Project, ProjectFile};
use std::path::{Path, PathBuf};

/// Every project intent (REST `project_*`, the strip and the panel).
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum ProjectAction {
    ProjectState,
    ProjectGuide {
        #[serde(default)]
        topic: Option<String>,
    },
    /// Start a robot from an empty design.
    ProjectNew {
        name: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        dir: Option<PathBuf>,
    },
    ProjectOpen {
        path: PathBuf,
        #[serde(default)]
        step: Option<Step>,
    },
    ProjectClose,
    /// Go to a step: its mode on the project's document, and its work.
    ProjectStep { step: Step },
    /// Save the design if needed and make the simulation model.
    ProjectExport,
    /// Run the acceptance test on the current model.
    ProjectTest,
    /// State the acceptance test (a whole `sim_runtime::acceptance::Test`).
    ProjectSetTest { test: Value },
    /// Ask the design assistant.
    ProjectChat { text: String },
    /// Lessons for this robot: `suggest` (the list), `write {topic}`.
    ProjectLessons {
        op: String,
        #[serde(default)]
        topic: Option<String>,
    },
    /// Export the tested design's part files.
    ProjectMake,
    /// Drop the follow-ups waiting for a mode.
    ProjectCancel,
    /// The project panel: open / close / toggle, and which view.
    ProjectPanel {
        #[serde(default)]
        open: Option<bool>,
        #[serde(default)]
        view: Option<Step>,
    },
}

const WORKFLOW: &str = "Steps in order: 1 Design (CAD: parts, materials, joints, motors) → 2 Model (the saved design exported in process to <name>.simrobot.json; every estimate listed, blocking ones must be set in CAD) → 3 Test (the acceptance test run headless on the model: pass / fail / not assessed per criterion; a report under results/) → 4 Learn (lessons for this robot in its lessons/ folder) → 5 Make (part files of the tested design under make/).";

impl actions::Action for ProjectAction {
    fn commands() -> Vec<Spec> {
        let all = actions::ALL;
        vec![
            spec("project_guide", all, json!({}), "Start here for building a robot end to end: the order of operations, every project command with an example, and the rules (also GET /v1/project_guide). topic narrows it (about, concepts, workflows, commands, rules)."),
            spec("project_state", all, json!({}), format!("The window's robot project (also GET /v1/project_state): open, project (path, name, description, cad, model, results, lessons, make, test, chat), steps (each step, state done | ready | attention | waiting, line, hint), next, design (bodies, joints, driven joints, motors, revision), model (exists, cad_sha256, assumptions with blocking ones, links), test (stated, running with progress, latest report), lessons (written, suggested, writer), make, chat (turns, assistant status), intents (follow-ups waiting for a mode), message. With no project: recent projects. {WORKFLOW}")),
            spec("project_new", all, json!({"name": "Lift arm", "description": "A one-joint arm that lifts a 20 g weight to 60° in a second"}), "Start a robot from an empty design: makes <workspace>/projects/<slug>/ (or dir) with <slug>.robot.json, an empty <slug>.rcad (RoboCAD's stock materials), results/, lessons/ and make/; opens it in CAD mode (step 1) and, with a description, hands it to the design assistant, which builds the robot through CAD's own commands (each one undo step). Refused, naming the file, when the project or its CAD file already exists. Answers project_state."),
            spec("project_open", all, json!({"path": "/abs/projects/lift-arm/lift-arm.robot.json", "step": "test"}), "Open a robot project (a *.robot.json, or the folder holding one): CAD, Robot and Lessons remember its files, and the window goes to step (default design) on the project's document. Refused naming the file when it is not a project (unknown fields and paths outside its folder are named)."),
            spec("project_close", all, json!({}), "Close the window's project (the modes keep what they show)."),
            spec("project_step", all, json!({"step": "model"}), "Go to a step (design | model | test | learn | make): its mode on the project's document (the mode switch, or the mode's own open when it shows another document), then its work: model saves the design if it has unsaved edits and makes the simulation model; test opens the model in Robot mode and runs the test when it is not current; learn opens the project's lessons (or the suggestions when there are none); make exports the part files. The same as the strip's step chips."),
            spec("project_export", all, json!({}), "Make the simulation model: CAD on the project's file (switched to when needed), saved if it has unsaved edits (the step's button says so), then exported in process (sim_cad::physical) to the project's model file. Poll project_state (model, steps)."),
            spec("project_test", all, json!({}), format!("Run the acceptance test on the project's model, headless on a job (sim_runtime::acceptance): every criterion pass, fail or not assessed (never a pass); the report is written to results/<UTC stamp>.test.json and shown in project_state.test.latest. With no test stated, a starting test is made from the robot's driven joints (move toward the upper limit in 1 s and hold; 3° tolerance; 30 % torque reserve; 20 °C winding margin; no limit hits; printed-part safety factor 2) and saved into the project as a starting point to edit. Refused while a model is missing, has blocking values, or a test runs. {}", sim_runtime::acceptance::VERDICT_RULE)),
            spec("project_set_test", all, json!({"test": {"name": "lift the payload", "duration_s": 3.0, "trajectory": [{"t": 0.0, "targets": {"Shoulder": 0.0}}, {"t": 1.0, "targets": {"Shoulder": 1.05}}], "criteria": [{"kind": "reaches", "joint": "Shoulder", "target": 1.05, "tolerance": 0.05, "by_s": 1.5}, {"kind": "torque_margin", "min": 0.3}, {"kind": "winding_temperature", "margin_c": 20.0}, {"kind": "no_limit_hits"}, {"kind": "yield_margin", "min": 1.0}]}}), "State the acceptance test (what the robot must do and how well): name, duration_s (≤ 600), trajectory (waypoints {t, targets {joint: rad, or m for a slide}}, linear between them, held after the last), criteria (kind reaches {joint, target, tolerance, by_s} | tracking {joint, max_error, after_s} | torque_margin {min} | winding_temperature {margin_c} | bearing_margin {min} | yield_margin {min} (needs flexible-link stress: rigid models report not assessed) | part_strength {min_safety_factor} (every printed body of the design checked after the run by the print registry's layer-aware stress check under the run's peak loads: a carried body held at its joint and loaded at its tip by the peak motor torque, a base body held underneath and loaded at the motor mount) | no_fall | no_limit_hits). Validated (fields named) and saved in the project file; a test from before no longer counts as current."),
            spec("project_chat", all, json!({"text": "Make the arm 20 mm longer and check it still lifts the weight"}), "Ask the design assistant (Codex through sim_agent, in this window's REST): it reads the design, changes it through CAD's commands (each one undo step), and may make the model and run the test. CAD is switched to the project's file first when needed. The turn and the answer are kept in the project file (chat); poll project_state.chat."),
            spec("project_lessons", all, json!({"op": "write", "topic": "servo-holding-torque"}), "Lessons for this robot: op suggest lists topics drawn from its parts and its latest test (each with why it matters for this robot); op write {topic} (a suggested topic id, or your own words) has the AI write the lesson into the project's lessons/<slug>/lesson.md with this robot's numbers (and a figure rendered from its CAD model), then Lessons mode opens it. Poll project_state.lessons.writer."),
            spec("project_make", all, json!({}), "Export the part files of the tested design: every printed body as an STL under make/ and make/parts.json (the CAD file's sha256, each part's material, mass and print orientation). Refused unless the latest test passed on the current design. Print studies (layer strength, splitting, plates) are not ported to the in-process editor and are said to be."),
            spec("project_cancel", all, json!({}), "Drop the project's follow-ups still waiting for a mode (shown in project_state.intents)."),
            spec("project_panel", all, json!({"open": true, "view": "model"}), "Show or hide the project panel (open; absent toggles) and choose its view (a step). The strip's project name button toggles it."),
        ]
    }
    fn parse(command: &sim_api::Command) -> Result<Self, String> {
        if command.args.is_null() {
            return sim_api::decode::<Self>(&sim_api::Command { command: command.command.clone(), args: json!({}) });
        }
        sim_api::decode::<Self>(command)
    }
    fn controls() -> &'static [&'static str] {
        &["project:step:<step>", "project:panel", "project:test", "project:export", "project:make", "project:cancel"]
    }
}

/// What the handler writes to the other modes.
#[derive(SystemParam)]
pub(crate) struct Out<'w> {
    switch: MessageWriter<'w, Act<WindowAction>>,
    cad: MessageWriter<'w, Act<CadAction>>,
    robot: MessageWriter<'w, Act<RobotAction>>,
    lessons: MessageWriter<'w, Act<crate::lesson::actions::LessonCommand>>,
}

/// What the handler reads of the window.
#[derive(SystemParam)]
pub(crate) struct Window<'w> {
    mode: Res<'w, State<ViewerMode>>,
    registry: ResMut<'w, DocumentRegistry>,
    cad: Option<Res<'w, CadDocument>>,
    robot: Option<Res<'w, RobotView>>,
    rest: Option<Res<'w, crate::rest::Rest>>,
    learn: Option<Res<'w, crate::lesson::Learn>>,
}

/// CAD mode on `path`: CAD's own Open when it shows another file, else the mode switch.
fn to_cad(path: &Path, w: &Window, out: &mut Out) {
    if *w.mode.get() == ViewerMode::Cad {
        if w.cad.as_deref().and_then(for_project::file) != Some(path) {
            out.cad.write(Act::ui(for_project::open(path)));
        }
    } else {
        out.switch.write(Act::ui(WindowAction::Switch(ModeSwitch { mode: ViewerMode::Cad, document: Some(Document::Path(path.to_path_buf())), reveal: None })));
    }
}

/// Robot mode on `path`: `robot_open` when it shows another robot, else the mode switch.
fn to_robot(path: &Path, w: &Window, out: &mut Out) {
    if *w.mode.get() == ViewerMode::Robot {
        if w.robot.as_deref().map(|r| r.path.as_path()) != Some(path) {
            out.robot.write(Act::ui(RobotAction::Open { path: path.to_path_buf() }));
        }
    } else {
        out.switch.write(Act::ui(WindowAction::Switch(ModeSwitch { mode: ViewerMode::Robot, document: Some(Document::Path(path.to_path_buf())), reveal: None })));
    }
}

/// The project's documents remembered for the modes not showing something now.
fn remember(p: &Project, w: &mut Window) {
    let current = *w.mode.get();
    if current != ViewerMode::Cad {
        w.registry.remember(ViewerMode::Cad, DocumentKind::Cad, Source::path(p.cad()));
    }
    if current != ViewerMode::Robot && p.model().is_file() {
        w.registry.remember(ViewerMode::Robot, DocumentKind::Robot, Source::path(p.model()));
    }
    if current != ViewerMode::Lessons && !super::status::sim_lessons(&p.lessons()).is_empty() {
        w.registry.remember(ViewerMode::Lessons, DocumentKind::Lessons, Source::Lessons { dir: p.lessons(), lesson: None });
    }
}

/// Go to `step` (see `project_step`).
fn go(step: Step, st: &mut ProjectState, w: &Window, out: &mut Out) -> Result<String, String> {
    let p = st.open.clone().ok_or("no robot project is open: start one (project_new) or open one (project_open)")?;
    st.panel.view = Some(step);
    st.intents.retain(|i| matches!(i, Intent::Ask(_)));
    match step {
        Step::Design => {
            to_cad(&p.cad(), w, out);
            Ok(format!("Step 1, Design: {} in CAD", p.file.cad))
        }
        Step::Model => {
            to_cad(&p.cad(), w, out);
            st.intents.push(Intent::Export { saving: false, started: false });
            Ok("Step 2, Model: saving the design if needed, then making the simulation model…".into())
        }
        Step::Test => {
            if !p.model().is_file() {
                return Err("Step 3 needs the simulation model: make it first (step 2, Model)".into());
            }
            to_robot(&p.model(), w, out);
            let current = st.step(Step::Test).is_some_and(|s| s.state == super::status::State::Done || s.state == super::status::State::Attention && s.line.starts_with("Failed"));
            if !current {
                st.intents.push(Intent::Test);
            }
            Ok(format!("Step 3, Test: {} in Robot mode{}", p.file.model, if current { "" } else { "; running the test" }))
        }
        Step::Learn => {
            st.panel.open = true;
            if super::status::sim_lessons(&p.lessons()).is_empty() {
                Ok("Step 4, Learn: pick a suggested lesson to have it written for this robot".into())
            } else {
                if *w.mode.get() != ViewerMode::Lessons {
                    out.switch.write(Act::ui(WindowAction::Switch(ModeSwitch { mode: ViewerMode::Lessons, document: Some(Document::Path(p.lessons())), reveal: None })));
                }
                Ok(format!("Step 4, Learn: this robot's lessons ({})", p.file.lessons))
            }
        }
        Step::Make => {
            to_cad(&p.cad(), w, out);
            st.intents.push(Intent::Make);
            Ok("Step 5, Make: exporting the tested design's part files…".into())
        }
    }
}

/// Open `project` as the window's project, then go to `step`.
fn open(project: Project, step: Step, st: &mut ProjectState, w: &mut Window, out: &mut Out) -> Result<Value, String> {
    remember(&project, w);
    let name = project.file.name.clone();
    *st = ProjectState { panel: super::panel::PanelState { open: true, view: Some(step) }, ..ProjectState::default() };
    // The project's agent service: answers finished while the window was closed are delivered.
    if let Err(e) = st.chat.ensure(&project.path) {
        st.chat.error = Some(e);
    }
    st.open = Some(project);
    let line = go(step, st, w, out)?;
    st.say(Ok(format!("{name}: {line}")));
    Ok(json!({"opened": name, "message": line}))
}

/// One action.
fn handle(action: &ProjectAction, st: &mut ProjectState, w: &mut Window, out: &mut Out) -> Result<Value, String> {
    match action {
        ProjectAction::ProjectState => Ok(st.json()),
        ProjectAction::ProjectGuide { topic } => super::guide::guide(topic.as_deref()),
        ProjectAction::ProjectNew { name, description, dir } => {
            let file = ProjectFile::new(name, description.as_deref().unwrap_or(""))?;
            let dir = match dir {
                Some(d) if d.is_absolute() => d.clone(),
                Some(d) => return Err(format!("project_new: dir must be an absolute path, not {}", d.display())),
                None => super::default_dir(name)?,
            };
            let project = Project::create(&dir, file)?;
            let bytes = sim_cad::edit::empty_archive(None)?;
            std::fs::write(project.cad(), bytes).map_err(|e| format!("{}: {e}", project.cad().display()))?;
            let mut answer = open(project, Step::Design, st, w, out)?;
            if let Some(d) = description.as_ref().filter(|d| !d.trim().is_empty()) {
                st.intents.push(Intent::Ask(format!("Design this robot from an empty document: {d}")));
                answer["assistant"] = json!("the description goes to the design assistant once CAD has opened the empty design");
            }
            answer["project"] = st.json()["project"].clone();
            Ok(answer)
        }
        ProjectAction::ProjectOpen { path, step } => {
            let project = Project::open(path)?;
            open(project, step.unwrap_or(Step::Design), st, w, out)
        }
        ProjectAction::ProjectClose => {
            let name = st.open.as_ref().map(|p| p.file.name.clone()).ok_or("no project is open")?;
            *st = ProjectState { revision: st.revision + 1, ..ProjectState::default() };
            st.say(Ok(format!("Closed {name}")));
            Ok(json!({"closed": name}))
        }
        ProjectAction::ProjectStep { step } => {
            let line = go(*step, st, w, out);
            st.say(line.clone());
            line.map(|m| json!({"step": step, "message": m}))
        }
        ProjectAction::ProjectExport => {
            let line = go(Step::Model, st, w, out);
            st.say(line.clone());
            line.map(|m| json!({"message": m}))
        }
        ProjectAction::ProjectTest => super::start_test(st),
        ProjectAction::ProjectSetTest { test } => {
            let p = st.open.as_mut().ok_or("no project is open")?;
            let test: sim_runtime::acceptance::Test = serde_json::from_value(test.clone()).map_err(|e| format!("project_set_test.test: {e}"))?;
            test.validate()?;
            p.file.test = Some(test.clone());
            p.save()?;
            st.say(Ok(format!("Test “{}” saved: {} criteria", test.name, test.criteria.len())));
            Ok(json!({"test": test}))
        }
        ProjectAction::ProjectChat { text } => {
            let p = st.open.clone().ok_or("no project is open")?;
            if text.trim().is_empty() {
                return Err("project_chat needs text".into());
            }
            to_cad(&p.cad(), w, out);
            st.intents.push(Intent::Ask(text.clone()));
            st.panel.view = Some(Step::Design);
            st.say(Ok("Asking the design assistant…".into()));
            Ok(json!({"queued": text}))
        }
        ProjectAction::ProjectLessons { op, topic } => match op.as_str() {
            "suggest" => Ok(json!({"suggested": super::lessons::suggestions(st)})),
            "write" => {
                let topic = topic.clone().ok_or("project_lessons write needs topic (a suggested id, or your own words)")?;
                let endpoint = w.rest.as_ref().map(|r| format!("http://{}", r.0.address));
                super::lessons::write(st, &topic, endpoint)
            }
            other => Err(format!("project_lessons: unknown op `{other}` (suggest | write)")),
        },
        ProjectAction::ProjectMake => {
            let line = go(Step::Make, st, w, out);
            st.say(line.clone());
            line.map(|m| json!({"message": m}))
        }
        ProjectAction::ProjectCancel => {
            let n = st.intents.len();
            st.intents.clear();
            st.say(Ok(format!("{n} follow-up(s) dropped")));
            Ok(json!({"dropped": n}))
        }
        ProjectAction::ProjectPanel { open, view } => {
            st.panel.open = open.unwrap_or(!st.panel.open);
            if view.is_some() {
                st.panel.view = *view;
            }
            st.touch();
            Ok(st.panel.json())
        }
    }
}

/// ViewerSet::Actions: every project action.
fn apply(mut messages: ResMut<Messages<Act<ProjectAction>>>, mut flight: ResMut<InFlight<ProjectAction>>, mut replies: ResMut<Replies>, mut st: ResMut<ProjectState>, mut w: Window, mut out: Out) {
    actions::apply(&mut messages, &mut flight, &mut replies, |action, call: &mut Call| {
        let result = handle(action, &mut st, &mut w, &mut out);
        if let Err(e) = &result
            && !call.rest()
        {
            st.say(Err(e.clone()));
        }
        Outcome::Done(result)
    });
}

/// CAD mode on the project's file, loaded and idle.
fn cad_ready<'a>(st: &ProjectState, w: &'a Window) -> Option<&'a CadDocument> {
    let p = st.open.as_ref()?;
    let doc = w.cad.as_deref()?;
    (*w.mode.get() == ViewerMode::Cad && for_project::file(doc) == Some(p.cad().as_path()) && for_project::loaded(doc) && for_project::busy(doc).is_none()).then_some(doc)
}

/// JobResults: follow-ups whose mode is ready act (see [`Intent`]).
pub(super) fn follow_up(mut st: ResMut<ProjectState>, mut w: Window, mut out: Out) {
    if st.intents.is_empty() {
        return;
    }
    let Some(intent) = st.intents.first().cloned() else { return };
    match intent {
        Intent::Export { saving, started } => {
            let Some(doc) = cad_ready(&st, &w) else { return };
            let model = st.open.as_ref().expect("cad_ready").model();
            if !started && for_project::unsaved(doc) {
                if !saving {
                    out.cad.write(Act::ui(for_project::save()));
                    st.intents[0] = Intent::Export { saving: true, started: false };
                    st.say(Ok("Saving the design before making the model…".into()));
                }
                return;
            }
            if !started {
                out.cad.write(Act::ui(for_project::export(&model)));
                st.intents[0] = Intent::Export { saving, started: true };
                st.say(Ok("Making the simulation model from the saved design…".into()));
                return;
            }
            if for_project::exporting(doc) {
                return;
            }
            st.intents.remove(0);
            match for_project::last_export(doc) {
                Some((path, Ok(m))) if path == model => {
                    st.say(Ok(format!("Model made: {m}")));
                    st.panel.view = Some(Step::Model);
                }
                Some((_, Err(e))) => st.say(Err(format!("The model could not be made: {e}"))),
                _ => {}
            }
        }
        Intent::Ask(text) => {
            let Some(doc) = cad_ready(&st, &w) else { return };
            let endpoint = w.rest.as_ref().map(|r| format!("http://{}", r.0.address));
            let result = super::chat::ask(&mut st, doc, &text, endpoint);
            st.intents.remove(0);
            if let Err(e) = result {
                st.say(Err(format!("The design assistant could not be asked: {e}")));
            }
        }
        Intent::Test => {
            let Some(f) = st.files.as_ref() else { return };
            let current = f.model_exists && f.model_cad_sha256.is_some() && f.model_cad_sha256 == f.cad_sha256;
            let exporting = w.cad.as_deref().is_some_and(for_project::exporting);
            if !current || exporting || st.test.is_some() {
                return;
            }
            st.intents.remove(0);
            if let Err(e) = super::start_test(&mut st) {
                st.say(Err(e));
            }
        }
        Intent::Make => {
            let Some(doc) = cad_ready(&st, &w) else { return };
            st.intents.remove(0);
            if let Err(e) = super::make::start(&mut st, doc) {
                st.say(Err(e));
            }
        }
        Intent::OpenLessons(slug) => {
            st.intents.remove(0);
            let Some(dir) = st.open.as_ref().map(Project::lessons) else { return };
            if *w.mode.get() == ViewerMode::Lessons {
                if w.learn.as_deref().is_some_and(|l| l.dir == dir) {
                    out.lessons.write(Act::ui(crate::lesson::actions::LessonCommand::LessonOpen { slug }));
                } else {
                    st.say(Ok("The lesson is written; Lessons mode shows another folder, so open this robot's lessons from the Learn step after leaving it".into()));
                }
            } else {
                // Remembered with the lesson, so the switch opens it.
                w.registry.remember(ViewerMode::Lessons, DocumentKind::Lessons, Source::Lessons { dir: dir.clone(), lesson: Some(slug) });
                out.switch.write(Act::ui(WindowAction::Switch(ModeSwitch { mode: ViewerMode::Lessons, document: None, reveal: None })));
            }
        }
    }
}

/// The project that owns `file` (a CAD file or a model beside its
/// `*.robot.json`): the project file in the same folder that names it.
pub(crate) fn owner(file: &Path) -> Option<Project> {
    let dir = file.parent()?;
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(sim_runtime::robot_project::SUFFIX)).find_map(|p| {
        let project = Project::open(&p).ok()?;
        (project.cad() == file || project.model() == file).then_some(project)
    })
}

/// JobResults: when CAD mode or Robot mode opens another robot's file (its
/// own Open, the picker, a mode switch), the window follows: the project
/// that owns the file becomes the window's project and the other modes
/// remember its documents. A file no project owns leaves the project open
/// and says so.
pub(super) fn follow_documents(mut st: ResMut<ProjectState>, mut w: Window, mut seen: Local<(Option<PathBuf>, Option<PathBuf>)>) {
    let cad = w.cad.as_deref().and_then(for_project::file).map(Path::to_path_buf);
    let robot = w.robot.as_deref().map(|r| r.path.clone()).filter(|p| p.to_string_lossy().ends_with(".simrobot.json"));
    if (cad.clone(), robot.clone()) == *seen {
        return;
    }
    let changed = if cad != seen.0 { cad.clone() } else if robot != seen.1 { robot.clone() } else { None };
    *seen = (cad, robot);
    let Some(file) = changed else { return };
    let mine = st.open.as_ref().is_some_and(|p| p.cad() == file || p.model() == file);
    if mine {
        return;
    }
    match owner(&file) {
        Some(project) => {
            let name = project.file.name.clone();
            remember(&project, &mut w);
            *st = ProjectState { panel: st.panel.clone(), revision: st.revision + 1, ..ProjectState::default() };
            if let Err(e) = st.chat.ensure(&project.path) {
                st.chat.error = Some(e);
            }
            st.open = Some(project);
            st.say(Ok(format!("Now working on {name}: {} belongs to it", file.file_name().unwrap_or_default().to_string_lossy())));
        }
        None if st.open.is_some() => {
            let name = st.open.as_ref().map(|p| p.file.name.clone()).unwrap_or_default();
            st.say(Ok(format!("{} is not part of {name}: the project's steps still describe {name}", file.display())));
        }
        None => {}
    }
}

pub(super) fn build(app: &mut App) {
    app.add_systems(Update, apply.in_set(ViewerSet::Actions));
}
