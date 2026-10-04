//! Lessons for the project's robot: suggestions drawn from its parts and its
//! latest test (each saying why it matters for this robot, and the general
//! lesson in the repository when there is one), and a writer that has the
//! AI write one into the project's `lessons/<slug>/lesson.md`.
//!
//! Writing: a job renders the robot from its CAD file (`robot.png` beside
//! the lesson) and gathers its numbers (masses, motors, joints, the latest
//! test); the agent (read-only, through `sim_agent`) returns the complete
//! lesson as its answer; the window validates it with the lesson parser
//! (`sim_lesson::Lesson::parse`) and only then writes it, and Lessons mode
//! opens it.
use super::{Intent, ProjectState};
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
use serde_json::{Value, json};
use sim_agent::{Input, Status};
use std::path::PathBuf;

pub(crate) const LESSON_DEVELOPER: &str = "You write one lesson about a specific robot that a person designed in this repository's viewer. The person may be new to engineering. Read lessons/AUTHORING_PROMPT.md, lessons/README.md, lessons/categories.yaml and lessons/concepts.yaml in the repository before writing, and follow them, except: this lesson has no live builder system (no `systems` in the front matter, no sim-scene, sim-lab or other blocks that need a system); teach with the robot's real numbers from the context, worked examples, at most one equation per part, the figure robot.png (a render of the robot) and sim-quiz questions (each with feedback for every option). Use only numbers given in the context and say which are estimates (the model lists them) and what the test did not assess. Front matter: title, summary, minutes, category (an id from categories.yaml), teaches and needs (ids from concepts.yaml), authors: [Design assistant]. Return JSON: body = the complete lesson.md text (front matter first, nothing before it, no code fence around it), links = [].";

/// A lesson being written.
pub(crate) struct Writing {
    pub slug: String,
    pub title: String,
    /// Gathering the robot's facts and rendering its figure.
    prep: Option<Job<Value>>,
    /// The agent's run, once asked.
    run: Option<String>,
}

#[derive(Default)]
pub(crate) struct LessonWriter {
    pub writing: Option<Writing>,
    pub last: Option<Result<String, String>>,
}
impl LessonWriter {
    pub(crate) fn json(&self) -> Value {
        json!({"writing": self.writing.as_ref().map(|w| json!({"slug": w.slug, "title": w.title, "stage": if w.prep.is_some() { "gathering the robot's numbers and rendering its figure" } else { "the AI is writing" }})), "last": self.last.as_ref().map(super::msg)})
    }
}

/// Topics for this robot (see the module doc), most specific first.
pub(crate) fn suggestions(st: &ProjectState) -> Vec<Value> {
    let Some(f) = &st.files else { return Vec::new() };
    let written: Vec<&str> = f.lessons.iter().map(|(s, _)| s.as_str()).collect();
    let general_dir = crate::workspace::path("lessons").ok();
    let general = |slug: &str| general_dir.as_ref().is_some_and(|d| d.join(slug).join("lesson.md").is_file()).then(|| slug.to_string());
    let mut out: Vec<(String, String, String, Option<String>)> = Vec::new();
    let library = sim_cad::robotics::library();
    let joint = f.driven.first().map(|d| d.0.clone()).unwrap_or_else(|| "joint".into());
    if let Some((name, spec)) = f.motor_specs.first() {
        let spec_name = library.iter().find(|m| m.id == *spec).map_or(spec.clone(), |m| m.name.clone());
        out.push(("holding-torque".into(), format!("How the {spec_name} holds the {joint}: torque, stall and reserve"), format!("{name} must carry the weight beyond the {joint}; the test asks it to keep a share of its stall torque spare. Where that number comes from, with this robot's masses and lever arms."), general("gear-ratio")));
        out.push(("motor-heating".into(), format!("Why the {spec_name} warms up holding still"), "Holding a load draws current, and current heats the winding; the test checks the winding stays below its rating. How hot, how fast, and what changes it.".into(), general("current-control")));
    }
    if !f.driven.is_empty() {
        out.push(("joint-play".into(), format!("Limits and play in the {joint}"), format!("The model gives the {joint} travel limits and an estimated drive backlash. What backlash does to where the arm ends up, and how to measure it."), general("backlash")));
    }
    if f.materials.iter().any(|m| matches!(m.as_str(), "pla" | "petg" | "abs" | "asa" | "nylon" | "tpu")) {
        out.push(("printed-strength".into(), "How strong is a printed part?".into(), "This robot's parts are printed, and the test could not assess their strength (the model's links are rigid). What decides a printed part's strength, and how to check it before printing.".into(), general("dovetails")));
    }
    if f.sensors.iter().any(|s| s == "encoder") {
        out.push(("encoder".into(), "How the encoder knows where the joint is".into(), "The robot measures its joint angle with an encoder; its resolution limits how precisely the controller can hold a position.".into(), general("encoder-resolution")));
    }
    if f.battery {
        out.push(("battery".into(), "The battery under load".into(), "The robot runs from a battery whose voltage sags under load, which changes what the motors can do.".into(), general("battery-sag")));
    }
    if let Some(r) = &f.report {
        let failed: Vec<String> = r["outcomes"].as_array().into_iter().flatten().filter(|o| o["status"] == "fail").filter_map(|o| o["description"].as_str().map(str::to_string)).collect();
        if !failed.is_empty() {
            out.insert(0, ("why-it-failed".into(), "Reading the test: why it failed".into(), format!("The latest test failed: {}. What the numbers say and what to change.", failed.join("; ")), None));
        }
    }
    out.push(("what-the-simulation-proves".into(), "What the simulation proves, and what it does not".into(), "The model lists its estimates and the test lists what it could not assess. How much to trust a passed test before building.".into(), None));
    out.into_iter().map(|(id, title, why, general)| json!({"id": id, "title": title, "why": why, "written": written.contains(&id.as_str()), "general_lesson": general})).collect()
}

/// Start writing `topic` (a suggestion id or the person's own words).
pub(crate) fn write(st: &mut ProjectState, topic: &str, endpoint: Option<String>) -> Result<Value, String> {
    let project = st.open.clone().ok_or("no project is open")?;
    if let Some(w) = &st.lessons.writing {
        return Err(format!("“{}” is still being written", w.title));
    }
    endpoint.ok_or("this window has no command server for the AI")?;
    let found = suggestions(st).into_iter().find(|s| s["id"] == topic);
    let (slug, title, why) = match found {
        Some(s) => (s["id"].as_str().unwrap_or("").to_string(), s["title"].as_str().unwrap_or("").to_string(), s["why"].as_str().unwrap_or("").to_string()),
        None => {
            let slug = sim_runtime::robot_project::slug(&topic.split_whitespace().take(6).collect::<Vec<_>>().join(" "));
            if slug.is_empty() {
                return Err("project_lessons write needs a topic with words in it".into());
            }
            (slug, topic.trim().to_string(), "The person asked for this lesson.".to_string())
        }
    };
    let dir = project.lessons().join(&slug);
    if dir.join("lesson.md").exists() {
        return Err(format!("{} is already written: open it in Lessons", dir.join("lesson.md").display()));
    }
    let (cad, model, report) = (project.cad(), project.model(), st.files.as_ref().and_then(|f| f.report.clone()));
    let (t, w) = (title.clone(), why.clone());
    let prep = Job::spawn(Pool::Dedicated, 0, format!("lesson facts: {slug}"), move |ctx| {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        // The figure: the robot as CAD draws it.
        let archive = sim_cad::ArchiveDocument::open(&cad)?;
        let geometry = sim_cad::geometry::load_geometry(&archive, &|| ctx.cancelled(), &|_| {})?;
        let bodies: Vec<sim_render::cad::Body> = geometry.iter().map(|g| {
            let n = archive.node(&g.node_id);
            let color = n.and_then(|n| sim_cad::ops::v3(&n["color"])).or_else(|| n.and_then(|n| n["material"].as_str()).and_then(|m| sim_cad::edit::material(&archive.manifest, m).and_then(|x| sim_cad::ops::v3(&x["color"])))).map_or([0.66, 0.70, 0.76], |c| c.map(|v| v as f32));
            sim_render::cad::Body { id: g.node_id.clone(), name: n.and_then(|n| n["name"].as_str()).unwrap_or("").to_string(), vertices: g.vertices_mm.clone(), triangles: g.triangles.clone(), triangle_face: g.triangle_faces.clone(), color }
        }).collect();
        let png = sim_render::cad::render(&bodies, &sim_render::cad::Options { width: 1000, height: 750, view: [1., -1., 0.8], mode: "shaded".into(), section: None, highlight: Vec::new(), labels: true, edges: true, focus: Vec::new(), title: None })?;
        std::fs::write(dir.join("robot.png"), png).map_err(|e| format!("robot.png: {e}"))?;
        // The robot's numbers, from its model (what the test ran).
        let m: Value = std::fs::read_to_string(&model).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        let facts = json!({
            "links": m["links"].as_array().into_iter().flatten().map(|l| json!({"name": l["name"], "mass_kg": l["mass"], "com_m": l["com"], "material": l["material"], "members": l["member_names"]})).collect::<Vec<_>>(),
            "joints": m["joints"].as_array().into_iter().flatten().map(|j| json!({"name": j["name"], "type": j["type"], "limits_rad": j["limits"], "axis": j["axis"], "parent": j["parent"], "child": j["child"], "motor": j["motor"], "drive_backlash": j["physics"]["drive_backlash"], "friction": j["physics"]["friction"]})).collect::<Vec<_>>(),
            "motors": m["motors"].as_array().into_iter().flatten().map(|mo| json!({"name": mo["name"], "spec": mo["spec"], "stall_torque_nm": mo["stall_torque"], "no_load_speed_rad_s": mo["no_load_speed"], "mass_kg": mo["mass"], "electrical": mo["electrical"], "thermal": mo["thermal"], "notes": mo["notes"]})).collect::<Vec<_>>(),
            "materials": m["materials"],
            "assumptions": m["source"]["assumptions"], "not_modelled": m["source"]["not_modelled"],
            "latest_test": report.map(|r| json!({"test": r["test"], "verdict": r["verdict"], "summary": r["summary"], "outcomes": r["outcomes"], "motors": r["results"]["motors"], "joints": r["results"]["joints"]})),
        });
        Ok(json!({"title": t, "why": w, "figure": "robot.png", "robot": facts}))
    });
    st.lessons.writing = Some(Writing { slug: slug.clone(), title: title.clone(), prep: Some(prep), run: None });
    st.lessons.last = None;
    st.say(Ok(format!("Writing the lesson “{title}”…")));
    Ok(json!({"writing": slug, "title": title}))
}

/// JobResults: the facts land and the agent is asked; its lesson is
/// validated and written; Lessons mode then opens it.
pub(super) fn tick(mut st: ResMut<ProjectState>, rest: Option<Res<crate::rest::Rest>>) {
    // Lessons the AI finished (this visit's, or one left by an earlier window): validated, written, delivered.
    let ready: Vec<sim_agent::Run> = st.chat.state.runs.iter().filter(|r| r.input.discussion.starts_with("project/lesson/") && r.status == Status::Ready && r.delivery_ready).cloned().collect();
    if !ready.is_empty() {
        let st = &mut *st;
        for run in ready {
            let slug = run.input.discussion.trim_start_matches("project/lesson/").to_string();
            if st.lessons.writing.as_ref().is_some_and(|w| w.slug == slug) {
                st.lessons.writing = None;
            }
            let outcome = run.reply.as_ref().ok_or_else(|| "the AI's answer is missing".to_string()).and_then(|r| save(st, &slug, &r.body));
            st.chat.delivered(&run.id, outcome.as_ref().map(|_| ()).map_err(String::clone));
            match &outcome {
                Ok(path) => {
                    st.say(Ok(format!("Lesson written: {}", path.display())));
                    st.intents.push(Intent::OpenLessons(slug));
                }
                Err(e) => st.say(Err(format!("The lesson was not saved: {e}"))),
            }
            st.lessons.last = Some(outcome.map(|p| p.display().to_string()));
        }
        return;
    }
    // Read through a shared borrow first: a ResMut deref marks the state changed.
    let (slug, title, prepared, run_id) = {
        let Some(w) = &st.lessons.writing else { return };
        (w.slug.clone(), w.title.clone(), w.prep.as_ref().map(|p| p.poll()), w.run.clone())
    };
    let fail = |st: &mut ProjectState, e: String| {
        st.lessons.writing = None;
        st.lessons.last = Some(Err(e.clone()));
        st.say(Err(e));
    };
    match prepared {
        // Stage 1: gathering the facts.
        Some(None) => {}
        Some(Some(result)) => {
            let st = &mut *st;
            let facts = match result {
                Ok(f) => f,
                Err(e) => return fail(st, format!("The lesson could not be prepared: {e}")),
            };
            let Some(project) = st.open.clone() else { return fail(st, "the project was closed".into()) };
            let Some(rest) = rest.map(|r| format!("http://{}", r.0.address)) else { return fail(st, "this window has no command server for the AI".into()) };
            let input = Input {
                discussion: format!("project/lesson/{slug}"),
                key: format!("lesson/{slug}/{}", crate::robot::recording::now_ms()),
                revision: 0,
                question: format!("Write the lesson “{title}” for the robot “{}” ({}). Why it matters here: {}", project.file.name, project.file.description, facts["why"].as_str().unwrap_or("")),
                context: json!({"lesson": {"slug": slug, "title": title, "figure": "robot.png"}, "project": {"name": project.file.name, "description": project.file.description}, "robot": facts["robot"], "viewer_rest": rest}),
                instructions: Some(format!("Write the lesson described below. Repository: read the files named in your developer instructions. The robot's facts are in the context (from its simulation model and latest test); you may also GET {rest}/v1/project_state. Return the complete lesson.md as body.")),
                developer: Some(LESSON_DEVELOPER.into()),
            };
            match st.chat.ensure(&project.path).and_then(|s| s.ask(input)) {
                Ok(run) => {
                    if let Some(w) = st.lessons.writing.as_mut() {
                        w.prep = None;
                        w.run = Some(run);
                    }
                    st.touch();
                }
                Err(e) => fail(st, format!("The AI could not be asked: {e}")),
            }
        }
        // Stage 2: waiting for the agent; a run that ended without a lesson is reported.
        None => {
            let Some(run_id) = run_id else { return };
            let Some(run) = st.chat.state.runs.iter().find(|r| r.id == run_id).cloned() else { return };
            if matches!(run.status, Status::Failed | Status::Cancelled) {
                fail(&mut st, format!("The AI did not write the lesson: {}", run.error.clone().unwrap_or_else(|| format!("{:?}", run.status))));
            }
        }
    }
}

/// Validate the agent's text as a lesson and write it (nothing is written when invalid).
fn save(st: &ProjectState, slug: &str, body: &str) -> Result<PathBuf, String> {
    let project = st.open.as_ref().ok_or("the project was closed")?;
    let text = body.trim().trim_start_matches("```markdown").trim_start_matches("```md").trim_start_matches("```").trim_end_matches("```").trim().to_string() + "\n";
    if !text.starts_with("---") {
        return Err("the answer is not a lesson (it has no front matter)".into());
    }
    let path = project.lessons().join(slug).join("lesson.md");
    sim_lesson::Lesson::parse(&path, &text).map_err(|e| format!("{e:?}"))?;
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, &text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}
