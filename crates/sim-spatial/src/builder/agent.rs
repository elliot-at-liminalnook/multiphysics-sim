//! Physical builder adapter; the reusable worker never writes the system document.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_agent::{Input, Status, Supervisor};

pub(super) struct Agent {
    pub service: Supervisor,
    pub state: sim_agent::State,
    pub expanded: bool,
    checked: f64,
    revision: Option<u64>,
    pub endpoint: Option<String>,
    registry: Arc<Mutex<BehaviorRegistry>>,
    context_task: Option<crate::jobs::Job<Value>>,
}
impl Agent {
    pub fn open(path: &std::path::Path, registry: &BehaviorRegistry) -> Self {
        let cwd = crate::workspace::agent_dir();
        let state_path = path.with_file_name(format!(
            ".{}.agents/state.json",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        let registry = Arc::new(Mutex::new(registry.clone()));
        let worker_registry = registry.clone();
        Self {
            service: Supervisor::open_with_context(
                sim_agent::Config::from_env(cwd, state_path),
                Arc::new(move |input| {
                    let doc: sim_system::SystemDocument =
                        serde_json::from_value(input.context["document"].clone())
                            .map_err(|e| e.to_string())?;
                    let registry = worker_registry.lock().unwrap().clone();
                    input.context["model_context"] = sim_model_context::build(
                        &doc,
                        &registry,
                        &sim_model_context::Request {
                            discussion: Some(input.discussion.clone()),
                            targets: vec![],
                        },
                    )?;
                    let context = &input.context["model_context"];
                    input.context["context_summary"] = json!({
                        "source":context["source"],"scope":context["scope"],
                        "resolved":!context["resolved_model"].is_null(),
                        "resolution_error":context["resolution_error"]
                    });
                    Ok(())
                }),
            ),
            registry,
            context_task: None,
            state: Default::default(),
            expanded: false,
            checked: 0.,
            revision: None,
            endpoint: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Request {
    Status,
    Configure {
        auto_answer: bool,
    },
    Ask {
        discussion: String,
        #[serde(default)]
        question: Option<String>,
        #[serde(default)]
        request_id: Option<String>,
    },
    Cancel {
        run: String,
    },
    Retry {
        run: String,
    },
    MarkRead {
        discussion: String,
    },
    Activity,
}
impl Builder {
    pub(crate) fn agent_endpoint(&mut self, url: String) {
        self.agent.endpoint = Some(url);
    }
    pub(crate) fn agent_json(&self) -> Value {
        self.agent.state.public()
    }

    fn agent_input(
        &self,
        id: &str,
        key: String,
        question: Option<String>,
    ) -> Result<Input, String> {
        let t = self
            .document
            .discussions
            .threads
            .get(id)
            .ok_or("unknown discussion")?;
        if t.resolved {
            return Err("reopen the discussion before asking Codex".into());
        }
        let question = question
            .or_else(|| {
                t.comments
                    .iter()
                    .rev()
                    .find(|c| !is_agent(c))
                    .map(|c| c.body.clone())
            })
            .ok_or("discussion has no human question")?;
        *self.agent.registry.lock().unwrap() = self.registry.clone();
        Ok(Input {
            discussion: id.into(),
            key,
            revision: self.document.revision,
            question,
            context: json!({"system_file":self.store.path.canonicalize().unwrap_or(self.store.path.clone()),"discussion":t,"document":self.document,"viewer_rest":self.agent.endpoint,
                "viewer_status":self.context_status(),"part_library":self.parts_json(),
                "rest_inspection":{"command":"system_context","args":{"discussion":id},"read_only":true},
                "rest_read_resources":["/v1/description","/v1/spatial","/v1/measurements"],"display_semantics":sim_system::display::SEMANTICS}),
            instructions: None,
        })
    }
    fn context_status(&self) -> Value {
        json!({"source_revision":self.document.revision,"compiling":self.job.is_some(),
            "compile_error":self.compile_error,"findings":self.findings,
            "running":self.run.is_some(),"compiled_description":self.last_description.as_ref().map(|d| json!({"id":d.id,"source_hash":d.source_hash,"model_revision":d.model_revision})),
            "freshness":"Viewer status captured when requested. A compiled description can predate current edits; do not treat it or its samples as current-model evidence without checking."})
    }
    /// Same context builder as automatic/manual agent turns. Expensive model
    /// resolution is off the UI thread; REST polls this task without clicks.
    pub(crate) fn context_request(
        &mut self,
        request: sim_model_context::Request,
        continuation: &mut Value,
        cancelled: bool,
    ) -> sim_api::Outcome {
        if cancelled {
            self.agent.context_task = None;
            return sim_api::Outcome::Done(Err("context inspection cancelled".into()));
        }
        if continuation.is_null() {
            let doc = self.document.clone();
            let registry = self.registry.clone();
            let status = self.context_status();
            let parts = self.parts_json();
            let file = self
                .store
                .path
                .canonicalize()
                .unwrap_or(self.store.path.clone());
            self.agent.context_task = Some(crate::jobs::Job::spawn(crate::jobs::Pool::Compute, 0, "model context worker", move |_| {
                sim_model_context::build(&doc, &registry, &request).map(|mut v| {
                    v["viewer_status"] = status;
                    v["part_library"] = parts;
                    v["system_file"] = json!(file);
                    v
                })
            }));
            *continuation = json!(true);
        }
        let Some(task) = &self.agent.context_task else {
            return sim_api::Outcome::Done(Err("model context worker stopped".into()));
        };
        match task.poll() {
            Some(result) => {
                self.agent.context_task = None;
                sim_api::Outcome::Done(result)
            }
            None => sim_api::Outcome::Pending,
        }
    }
    pub(crate) fn agent_request(&mut self, request: Request) -> Result<Value, String> {
        let result = match request {
            Request::Status => return Ok(self.agent.service.snapshot().public()),
            Request::Configure { auto_answer } => {
                // Baseline already-posted comments before enabling; no surprise backlog.
                self.observe_agent_comments()?;
                self.agent.service.set_auto(auto_answer)?;
                json!({"auto_answer":auto_answer})
            }
            Request::Ask {
                discussion,
                question,
                request_id,
            } => {
                let key = format!(
                    "manual/{discussion}/{}",
                    request_id.unwrap_or_else(|| format!(
                        "{}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos()
                    ))
                );
                let input = self.agent_input(&discussion, key, question)?;
                json!({"run":self.agent.service.ask(input)?})
            }
            Request::Cancel { run } => {
                self.agent.service.cancel(&run)?;
                json!({"run":run})
            }
            Request::Retry { run } => {
                json!({"run":self.agent.service.retry(&run)?})
            }
            Request::MarkRead { discussion } => {
                self.agent.service.mark_read(&discussion);
                json!({"discussion":discussion})
            }
            Request::Activity => {
                self.agent.expanded = !self.agent.expanded;
                json!({"expanded":self.agent.expanded})
            }
        };
        self.agent.state = self.agent.service.snapshot();
        self.panel_dirty = true;
        Ok(result)
    }
    fn observe_agent_comments(&self) -> Result<(), String> {
        let mut inputs = vec![];
        for t in self
            .document
            .discussions
            .threads
            .values()
            .filter(|t| !t.resolved)
        {
            // New posted human comments trigger turns; edits do not silently spend another turn.
            for c in t.comments.iter().filter(|c| !is_agent(c)) {
                let key = format!("comment/{}/{}", t.id, c.id);
                if !self.agent.state.seen.contains(&key) {
                    inputs.push(self.agent_input(&t.id, key, Some(c.body.clone()))?);
                }
            }
        }
        self.agent.service.observe(inputs)
    }
    fn attach_agent_reply(&mut self, run: &sim_agent::Run) -> Result<(), String> {
        let t = self
            .document
            .discussions
            .threads
            .get(&run.input.discussion)
            .ok_or("Discussion was deleted while Codex worked")?;
        if t.comments.iter().any(|c| c.id == run.id) {
            return Ok(());
        }
        let reply = run.reply.as_ref().ok_or("agent reply missing")?;
        // Bind to identities from the original document, then refresh against today's document.
        let original: sim_system::SystemDocument =
            serde_json::from_value(run.input.context["document"].clone())
                .map_err(|e| e.to_string())?;
        let links = reply
            .links
            .iter()
            .map(|p| sim_system::display::bind(&original, p).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let comment = sim_system::display::Comment {
            id: run.id.clone(),
            author: "Codex".into(),
            body: reply.body.clone(),
            created_at: sim_agent::now().to_string(),
            edited_at: None,
            links,
        };
        self.apply(
            "Codex reply",
            vec![SystemCommand::AddComment {
                thread: run.input.discussion.clone(),
                comment,
            }],
        )?;
        Ok(())
    }
    pub(super) fn agent_badge(&self, id: &str) -> Option<String> {
        self.agent.state.latest(id).and_then(|r| match r.status {
            Status::Queued => Some("Codex queued".into()),
            Status::Running => Some("Codex working".into()),
            Status::Ready => Some("Attaching reply".into()),
            Status::Failed => Some("Codex needs attention".into()),
            Status::Completed if r.unread => Some("New Codex reply".into()),
            _ => None,
        })
    }
}
fn is_agent(c: &sim_system::display::Comment) -> bool {
    c.id.starts_with("agent-") || c.author.eq_ignore_ascii_case("codex")
}
pub(super) fn tick(time: Res<Time>, mut b: ResMut<Builder>) {
    if time.elapsed_secs_f64() - b.agent.checked < 0.2 {
        return;
    }
    if b.agent.checked.floor() != time.elapsed_secs_f64().floor()
        && b.agent.state.runs.iter().any(|r| r.status.active())
    {
        b.panel_dirty = true;
    }
    b.agent.checked = time.elapsed_secs_f64();
    let state = b.agent.service.snapshot();
    if state.generation != b.agent.state.generation || state.ready != b.agent.state.ready {
        b.panel_dirty = true;
    }
    b.agent.state = state;
    if !b.agent.state.ready {
        return;
    }
    if b.agent.revision != Some(b.document.revision) {
        match b.observe_agent_comments() {
            Ok(()) => b.agent.revision = Some(b.document.revision),
            Err(e) => {
                b.discussion.error = Some(e);
                b.panel_dirty = true;
            }
        }
    }
    let ready = b
        .agent
        .state
        .runs
        .iter()
        .filter(|r| r.status == Status::Ready && r.delivery_ready)
        .cloned()
        .collect::<Vec<_>>();
    for run in ready {
        let result = b.attach_agent_reply(&run);
        b.agent.service.delivered(&run.id, result);
        b.panel_dirty = true;
    }
    // Mark read only when the discussion is actually visible; no camera changes.
    if b.tab == Tab::Discussions {
        if let Some(id) = &b.discussion.selected {
            b.agent.service.mark_read(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reply_preserves_draft_tracks_renames_and_delivers_once_with_undo() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("builder-agent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.system.json");
        std::fs::copy(
            root.join("examples/systems-builder/worm-drive/winch.system.json"),
            &path,
        )
        .unwrap();
        let mut b = Builder::open(
            path,
            root.join("library/systems"),
            sim_runtime::system_registry(),
        )
        .unwrap();
        let thread = sim_system::display::Thread {
            id: "note".into(),
            title: "Motor".into(),
            resolved: false,
            targets: vec![sim_system::display::bind(&b.document, "motor").unwrap()],
            comments: vec![],
            pin_m: None,
            view: None,
        };
        b.apply("note", vec![SystemCommand::PutThread { thread }])
            .unwrap();
        let input = b
            .agent_input("note", "test".into(), Some("Explain motor".into()))
            .unwrap();
        let run = sim_agent::Run {
            id: "agent-test".into(),
            input,
            status: Status::Ready,
            session: Some("session".into()),
            turn: Some("turn".into()),
            created_at: 1,
            updated_at: 1,
            activity: "done".into(),
            error: None,
            reply: Some(sim_agent::Reply {
                body: "Motor reference".into(),
                links: vec!["motor".into()],
            }),
            cancel_requested: false,
            unread: false,
            delivery_ready: true,
        };
        b.apply(
            "rename",
            vec![SystemCommand::RenameInstance {
                at: "".into(),
                name: "motor".into(),
                new_name: "drive_motor".into(),
            }],
        )
        .unwrap();
        b.input = Some(TextInput {
            purpose: Purpose::Comment,
            buffer: "unfinished user reply".into(),
        });
        b.attach_agent_reply(&run).unwrap();
        let revision = b.document.revision;
        assert_eq!(
            b.document.discussions.threads["note"].comments[0].links[0].path,
            "drive_motor"
        );
        assert_eq!(b.input.as_ref().unwrap().buffer, "unfinished user reply");
        b.attach_agent_reply(&run).unwrap();
        assert_eq!(b.document.revision, revision);
        assert_eq!(b.document.discussions.threads["note"].comments.len(), 1);
        b.undo().unwrap();
        assert!(b.document.discussions.threads["note"].comments.is_empty());
        let mut invalid = run.clone();
        invalid.reply.as_mut().unwrap().links = vec!["invented-part".into()];
        assert!(b.attach_agent_reply(&invalid).is_err());
        assert!(b.document.discussions.threads["note"].comments.is_empty());
        drop(b);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
