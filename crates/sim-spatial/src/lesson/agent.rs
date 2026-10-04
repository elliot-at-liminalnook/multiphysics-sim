//! Codex answers on lesson notes, through the same `sim-agent` supervisor as
//! build-mode discussions (read-only answer mode, durable queue). Lessons
//! ask only when the reader presses Ask — there is no automatic answering,
//! so nothing is spent unasked. Replies are posted to the note as ordinary,
//! undoable comments; part links are kept only when they name an instance
//! of the note's scene.
use super::*;
use serde_json::{Value, json};
use sim_agent::{Input, Status, Supervisor};

#[derive(Default)]
pub(crate) struct LessonAgent {
    service: Option<Supervisor>,
    state: sim_agent::State,
    checked: f64,
}

impl LessonAgent {
    pub fn open(slug: &str, registry: &sim_core::BehaviorRegistry) -> Self {
        let cwd = crate::workspace::agent_dir();
        let state_path = cwd.join("runs/lessons/agents").join(slug).join("state.json");
        let registry = Arc::new(registry.clone());
        let service = Supervisor::open_with_context(
            sim_agent::Config::from_env(cwd, state_path),
            Arc::new(move |input| {
                // Engineering context of the scene's system, when the note is on a scene.
                if let Ok(doc) = serde_json::from_value::<sim_system::SystemDocument>(input.context["document"].clone()) {
                    let targets = input.context["part"].as_str().map(|p| vec![p.to_string()]).unwrap_or_default();
                    input.context["model_context"] = sim_model_context::build(&doc, &registry, &sim_model_context::Request { discussion: None, targets })?;
                }
                Ok(())
            }),
        );
        Self { service: Some(service), state: Default::default(), checked: 0. }
    }
    pub fn ready(&self) -> bool {
        self.service.is_some() && self.state.ready
    }
    pub fn error(&self) -> Option<&str> {
        self.state.error.as_deref()
    }
    pub fn latest(&self, thread: &str) -> Option<&sim_agent::Run> {
        self.state.latest(thread)
    }
    pub fn ask(&mut self, input: Input) -> Result<String, String> {
        let s = self.service.as_ref().ok_or("Codex is not available")?;
        let id = s.ask(input)?;
        self.state = s.snapshot();
        Ok(id)
    }
    pub fn cancel(&mut self, run: &str) -> Result<(), String> {
        let s = self.service.as_ref().ok_or("Codex is not available")?;
        s.cancel(run)?;
        self.state = s.snapshot();
        Ok(())
    }
    pub fn mark_read(&self, thread: &str) {
        if let Some(s) = &self.service {
            s.mark_read(thread);
        }
    }
    pub fn badge(&self, thread: &str) -> Option<String> {
        self.state.latest(thread).and_then(|r| match r.status {
            Status::Queued => Some("Codex queued".into()),
            Status::Running => Some("Codex working".into()),
            Status::Ready => Some("Attaching reply".into()),
            Status::Failed => Some("Codex needs attention".into()),
            Status::Completed if r.unread => Some("New Codex reply".into()),
            _ => None,
        })
    }
    pub fn public(&self) -> Value {
        self.state.public()
    }
}

impl Learn {
    /// What Codex sees for a lesson note: the lesson, the passage or scene
    /// the note is on, and (for scenes) the scene's system document.
    pub(crate) fn agent_input(&self, thread: &str) -> Result<Input, String> {
        let lesson = self.lesson.as_ref().ok_or("no lesson is open")?;
        let t = self.threads().get(thread).cloned().ok_or("unknown note")?;
        if t.resolved {
            return Err("reopen the note before asking Codex".into());
        }
        let question = t.comments.iter().rev().find(|c| !c.is_agent()).map(|c| c.body.clone()).ok_or("the note has no question yet")?;
        let mut context = json!({
            "lesson": {"slug": lesson.slug, "title": lesson.meta.title, "file": lesson.path, "summary": lesson.meta.summary},
            "note": t,
            "semantics": "Lesson notes are presentation only. Scenes are recorded runs of the lesson's system files on the shared runtime; their claims are checked by sim-lesson check.",
        });
        match t.targets.first() {
            Some(LessonAnchor::Text { quote }) => {
                let block = self.index.as_ref().and_then(|i| LessonAnchor::Text { quote: quote.clone() }.block(i).map(String::from));
                let text = block.as_ref().and_then(|b| lesson.block(b)).map(|b| b.text(&lesson.source).to_string());
                context["passage"] = json!({"quote": quote.exact, "section": quote.section, "block": text});
            }
            Some(LessonAnchor::Scene { scene, part, time_s, .. }) => {
                if let Some(s) = lesson.scene(scene) {
                    context["scene"] = json!(s);
                    let path = self.scene.as_ref().filter(|a| &a.id == scene).and_then(|a| a.sandbox.as_ref().map(|sb| sb.path.clone())).unwrap_or_else(|| lesson.system_path(&s.system));
                    context["system_file"] = json!(path);
                    if let Ok(doc) = runtime::load_system(&path, &self.registry) {
                        context["document"] = json!(doc);
                    }
                    if let Some(run) = self.scene.as_ref().filter(|a| &a.id == scene).and_then(|a| a.run.as_ref()) {
                        context["run"] = json!({"fidelity": run.fidelity, "checks": run.checks, "error": run.error, "applied": run.applied, "at_time_s": time_s});
                        // The recorded values at the moment the question is about.
                        if let Some(t) = time_s {
                            let values: serde_json::Map<String, Value> = run.series.iter().filter_map(|s| {
                                let i = s.times.partition_point(|x| *x <= *t).checked_sub(1)?;
                                Some((s.label.clone(), json!({"value": s.values.get(i)?, "unit": s.unit, "sampled_at_s": s.times.get(i)?})))
                            }).collect();
                            context["values_at_time"] = Value::Object(values);
                            context["answer_guidance"] = json!(format!("The reader is asking about t = {t} s of this recorded run. Ground the answer in values_at_time: quote the relevant values with their units and the time, and say which part of the model produces them. Say so if the question needs a quantity that was not recorded."));
                        }
                    }
                }
                context["part"] = json!(part);
            }
            None => {}
        }
        let key = format!("manual/{thread}/{}", sim_annotate::uid("ask"));
        Ok(Input { discussion: thread.into(), key, revision: self.notes_doc.revision, context, question, instructions: None, developer: None })
    }
}

/// Poll the supervisor; post finished replies to their notes.
pub(super) fn tick(learn: &mut Learn, now: f64) {
    if now - learn.agent.checked < 0.25 {
        return;
    }
    learn.agent.checked = now;
    let Some(service) = &learn.agent.service else { return };
    let state = service.snapshot();
    if state.generation != learn.agent.state.generation || state.ready != learn.agent.state.ready {
        learn.dirty = true;
    }
    learn.agent.state = state;
    let ready: Vec<sim_agent::Run> = learn.agent.state.runs.iter().filter(|r| r.status == Status::Ready && r.delivery_ready).cloned().collect();
    for run in ready {
        let result = attach(learn, &run);
        if let Some(s) = &learn.agent.service {
            s.delivered(&run.id, result);
        }
        learn.dirty = true;
    }
}

fn attach(learn: &mut Learn, run: &sim_agent::Run) -> Result<(), String> {
    let t = learn.notes_doc.threads.get(&run.input.discussion).cloned().ok_or("the note was deleted while Codex worked")?;
    if t.comments.iter().any(|c| c.id == run.id) {
        return Ok(());
    }
    let reply = run.reply.as_ref().ok_or("agent reply missing")?;
    let scene = t.targets.iter().find_map(|a| a.scene().map(String::from));
    let known: BTreeSet<String> = serde_json::from_value::<sim_system::SystemDocument>(run.input.context["document"].clone()).map(|d| runtime::instance_paths(&d)).unwrap_or_default();
    let links = match &scene {
        Some(scene) => reply.links.iter().filter(|p| known.contains(*p)).map(|p| LessonAnchor::Scene { scene: scene.clone(), part: Some(p.clone()), time_s: None, missing: false }).collect(),
        None => vec![],
    };
    // Keyed by its run, so a reply is posted once.
    let comment = Comment { id: run.id.clone(), author: "Codex".into(), body: reply.body.clone(), created_at: sim_agent::now().to_string(), edited_at: None, links };
    learn.thread_op("Codex reply", crate::annotations::ThreadOp::Post { thread: t.id.clone(), comment }).map(|_| ())
}
