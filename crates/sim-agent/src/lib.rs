//! Durable, serial Codex conversations, independent of rendering and physics.
//! The host supplies context and applies replies through its own validated commands.
mod protocol;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Defaults; override with `SIM_CODEX_MODEL` / `SIM_CODEX_EFFORT` (see `Config::from_env`).
/// An unavailable model is an error, never a silent substitute.
pub const MODEL: &str = "gpt-6-astra";
pub const EFFORT: &str = "high";
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn uid() -> String {
    format!(
        "agent-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}

#[derive(Clone)]
pub struct Config {
    pub cwd: PathBuf,
    pub state_path: PathBuf,
    pub executable: PathBuf,
    pub model: String,
    pub effort: String,
}
impl Config {
    /// `SIM_CODEX_EXECUTABLE` (else `~/.local/bin/codex` when present, else `codex`
    /// on PATH), `SIM_CODEX_MODEL` and `SIM_CODEX_EFFORT` (else `MODEL`/`EFFORT`).
    pub fn from_env(cwd: PathBuf, state_path: PathBuf) -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let executable = std::env::var_os("SIM_CODEX_EXECUTABLE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let local = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_default()
                    .join(".local/bin/codex");
                if local.exists() {
                    local
                } else {
                    PathBuf::from("codex")
                }
            });
        Self {
            cwd,
            state_path,
            executable,
            model: var("SIM_CODEX_MODEL").unwrap_or_else(|| MODEL.into()),
            effort: var("SIM_CODEX_EFFORT").unwrap_or_else(|| EFFORT.into()),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Input {
    pub discussion: String,
    /// Stable caller key, e.g. annotation/comment ID plus edited timestamp.
    pub key: String,
    pub revision: u64,
    pub context: Value,
    pub question: String,
    /// Replaces the annotation-answer instructions for a different read-only
    /// task (e.g. drafting a lesson for the host to validate and write).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Replaces the conversation's developer instructions when it starts (a
    /// host whose agent may act through the host's own validated commands,
    /// e.g. CAD edits over the viewer's REST API, says so here). With
    /// `instructions`, the turn is not told to work read-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub developer: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub body: String,
    pub links: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Running,
    Ready,
    Completed,
    Failed,
    Cancelled,
}
impl Status {
    pub fn active(&self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::Ready)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub input: Input,
    pub status: Status,
    pub session: Option<String>,
    pub turn: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub activity: String,
    pub error: Option<String>,
    pub reply: Option<Reply>,
    pub cancel_requested: bool,
    pub unread: bool,
    #[serde(default)]
    pub delivery_ready: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub sequence: u64,
    pub run: String,
    pub at: u64,
    pub kind: String,
    pub message: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct State {
    pub generation: u64,
    pub ready: bool,
    pub error: Option<String>,
    pub auto_answer: bool,
    /// The configured model and effort, reported in status (not persisted).
    #[serde(skip)]
    pub model: String,
    #[serde(skip)]
    pub effort: String,
    pub sessions: BTreeMap<String, String>,
    pub seen: BTreeSet<String>,
    pub runs: Vec<Run>,
    pub events: Vec<Event>,
    pub next_event: u64,
}
impl State {
    fn event(&mut self, run: &str, kind: &str, message: &str) {
        self.next_event += 1;
        self.events.push(Event {
            sequence: self.next_event,
            run: run.into(),
            at: now(),
            kind: kind.into(),
            message: message.chars().take(2000).collect(),
        });
        if self.events.len() > 256 {
            self.events.remove(0);
        }
        self.generation += 1;
    }
    pub fn latest(&self, discussion: &str) -> Option<&Run> {
        self.runs
            .iter()
            .rev()
            .find(|r| r.input.discussion == discussion)
    }
    pub fn public(&self) -> Value {
        json!({"ready":self.ready,"error":self.error,"auto_answer":self.auto_answer,"model":self.model,"effort":self.effort,"generation":self.generation,
            "runs":self.runs.iter().map(|r|json!({"id":r.id,"discussion":r.input.discussion,"source_revision":r.input.revision,"context_summary":r.input.context["context_summary"],"status":r.status,"session":r.session,"turn":r.turn,"created_at":r.created_at,"updated_at":r.updated_at,"activity":r.activity,"error":r.error,"unread":r.unread,"cancel_requested":r.cancel_requested})).collect::<Vec<_>>(),
            "events":self.events,"event_cursor":self.next_event,"oldest_event":self.events.first().map(|e|e.sequence),"semantics":"Agent activity only; not physics inputs. Answer mode reads project files; replies use host validation and undo."})
    }
}

pub struct Supervisor {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
/// Host-supplied, read-only enrichment. Runs on the worker before every turn,
/// including resumed queued work; failures become normal retryable run errors.
pub type PrepareContext = Arc<dyn Fn(&mut Input) -> Result<(), String> + Send + Sync>;
impl Supervisor {
    /// Starts only the bookkeeping worker. Codex starts lazily on the first request.
    pub fn open(config: Config) -> Self {
        Self::open_with_context(config, Arc::new(|_| Ok(())))
    }
    pub fn open_with_context(config: Config, prepare: PrepareContext) -> Self {
        let state = Arc::new(Mutex::new(State {
            model: config.model.clone(),
            effort: config.effort.clone(),
            ..State::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, t) = (state.clone(), stop.clone());
        let worker = thread::spawn(move || worker(config, s, t, prepare));
        Self {
            state,
            stop,
            worker: Some(worker),
        }
    }
    pub fn snapshot(&self) -> State {
        let s = self.state.lock().unwrap();
        State {
            generation: s.generation,
            ready: s.ready,
            error: s.error.clone(),
            auto_answer: s.auto_answer,
            model: s.model.clone(),
            effort: s.effort.clone(),
            sessions: s.sessions.clone(),
            seen: s.seen.clone(),
            events: s.events.clone(),
            next_event: s.next_event,
            runs: s
                .runs
                .iter()
                .map(|r| Run {
                    id: r.id.clone(),
                    input: Input {
                        discussion: r.input.discussion.clone(),
                        key: r.input.key.clone(),
                        revision: r.input.revision,
                        question: String::new(),
                        instructions: None,
                        developer: None,
                        context: if r.status == Status::Ready {
                            r.input.context.clone()
                        } else {
                            json!({"context_summary":r.input.context["context_summary"]})
                        },
                    },
                    status: r.status.clone(),
                    session: r.session.clone(),
                    turn: r.turn.clone(),
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                    activity: r.activity.clone(),
                    error: r.error.clone(),
                    reply: r.reply.clone(),
                    cancel_requested: r.cancel_requested,
                    unread: r.unread,
                    delivery_ready: r.delivery_ready,
                })
                .collect(),
        }
    }
    pub fn set_auto(&self, enabled: bool) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        available(&s)?;
        s.auto_answer = enabled;
        s.event(
            "",
            "settings",
            if enabled {
                "Automatic answers enabled"
            } else {
                "Automatic answers disabled"
            },
        );
        Ok(())
    }
    /// Mark historical comments without generating work; also used while auto is off.
    pub fn observe(&self, inputs: Vec<Input>) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        available(&s)?;
        for input in inputs {
            if !s.seen.contains(&input.key) {
                if s.auto_answer {
                    enqueue(&mut s, input)?;
                } else {
                    s.seen.insert(input.key);
                    s.generation += 1;
                }
            }
        }
        Ok(())
    }
    pub fn ask(&self, input: Input) -> Result<String, String> {
        let mut s = self.state.lock().unwrap();
        available(&s)?;
        enqueue(&mut s, input)
    }
    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        available(&s)?;
        let r = s
            .runs
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or("unknown agent run")?;
        if !r.status.active() {
            return Err("run is already finished".into());
        }
        r.cancel_requested = true;
        if r.status != Status::Running {
            r.status = Status::Cancelled;
            r.reply = None;
            r.activity = "Cancelled".into();
        }
        s.event(id, "cancel_requested", "Cancellation requested");
        Ok(())
    }
    pub fn retry(&self, id: &str) -> Result<String, String> {
        let mut s = self.state.lock().unwrap();
        available(&s)?;
        let r = s
            .runs
            .iter()
            .find(|r| r.id == id)
            .ok_or("unknown agent run")?;
        if !matches!(r.status, Status::Failed | Status::Cancelled) {
            return Err("only failed or cancelled runs can be retried".into());
        }
        let mut input = r.input.clone();
        input.key = uid();
        enqueue(&mut s, input)
    }
    pub fn mark_read(&self, discussion: &str) {
        let mut s = self.state.lock().unwrap();
        let mut changed = false;
        for r in &mut s.runs {
            if r.input.discussion == discussion && r.unread {
                r.unread = false;
                changed = true;
            }
        }
        if changed {
            s.generation += 1;
        }
    }
    /// The host acknowledges only after the deterministic reply ID is in its document.
    pub fn delivered(&self, id: &str, result: Result<(), String>) {
        let mut s = self.state.lock().unwrap();
        if let Some(r) = s
            .runs
            .iter_mut()
            .find(|r| r.id == id && r.status == Status::Ready)
        {
            match result {
                Ok(()) => {
                    r.status = Status::Completed;
                    r.unread = true;
                    r.activity = "Reply ready".into();
                    r.input.context = json!({"system_file":r.input.context["system_file"],"context_summary":r.input.context["context_summary"]});
                }
                Err(e) => {
                    r.status = Status::Failed;
                    r.error = Some(e);
                    r.activity = "Reply could not be attached".into();
                }
            }
            r.updated_at = now();
            s.event(id, "delivery", "Reply delivery finished");
        }
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
fn available(s: &State) -> Result<(), String> {
    if !s.ready {
        Err(s
            .error
            .clone()
            .unwrap_or("agent service is starting".into()))
    } else {
        Ok(())
    }
}
fn enqueue(s: &mut State, input: Input) -> Result<String, String> {
    if input.question.trim().is_empty() {
        return Err("a question is required".into());
    }
    if input.question.len() > 65536
        || serde_json::to_vec(&input.context)
            .map_err(|e| e.to_string())?
            .len()
            > 1_000_000
    {
        return Err("agent context exceeds limit".into());
    }
    if let Some(r) = s.runs.iter().find(|r| r.input.key == input.key) {
        if r.input.discussion != input.discussion || r.input.question != input.question {
            return Err("request_id was already used for another question".into());
        }
        return Ok(r.id.clone());
    }
    if s.seen.contains(&input.key) {
        return Err("this request was already processed; use a new request_id".into());
    }
    while s.runs.len() >= 128 {
        if let Some(i) = s.runs.iter().position(|r| !r.status.active()) {
            s.runs.remove(i);
        } else {
            break;
        }
    }
    if s.runs.iter().filter(|r| r.status.active()).count() >= 32 {
        return Err("agent queue is full (32)".into());
    }
    let id = uid();
    s.seen.insert(input.key.clone());
    s.runs.push(Run {
        id: id.clone(),
        input,
        status: Status::Queued,
        session: None,
        turn: None,
        created_at: now(),
        updated_at: now(),
        activity: "Queued".into(),
        error: None,
        reply: None,
        cancel_requested: false,
        unread: false,
        delivery_ready: false,
    });
    s.event(&id, "queued", "Question queued");
    Ok(id)
}
fn checkpoint(config: &Config, state: &Arc<Mutex<State>>) -> Result<(), String> {
    let mut snapshot = state.lock().unwrap().clone();
    snapshot.ready = false;
    let tmp = config.state_path.with_extension("tmp");
    let bytes = serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?;
    use std::io::Write;
    let mut file = File::create(&tmp).map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(&tmp, &config.state_path).map_err(|e| e.to_string())?;
    if let Some(parent) = config.state_path.parent() {
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn lock_file(config: &Config) -> Result<File, String> {
    if let Some(p) = config.state_path.parent() {
        fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(config.state_path.with_extension("lock"))
        .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("another viewer owns this annotation agent service".into());
        }
    }
    #[cfg(not(unix))]
    return Err("annotation agent locking currently requires Unix".into());
    Ok(lock)
}
fn recover(s: &mut State) {
    s.ready = true;
    s.error = None;
    for r in &mut s.runs {
        if r.status == Status::Ready {
            r.delivery_ready = true;
        }
        if r.status == Status::Running {
            r.status = Status::Failed;
            r.error = Some(
                "Viewer stopped during the turn; retry to continue the saved conversation".into(),
            );
            r.activity = "Interrupted by restart".into();
        }
    }
    s.generation += 1;
}
fn worker(
    config: Config,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    prepare: PrepareContext,
) {
    let result = (|| -> Result<(), String> {
        let _lock = lock_file(&config)?;
        let mut loaded = match fs::read(&config.state_path) {
            Ok(bytes) => serde_json::from_slice::<State>(&bytes)
                .map_err(|e| format!("agent state is invalid; preserved on disk: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(e) => return Err(e.to_string()),
        };
        recover(&mut loaded);
        loaded.model = config.model.clone();
        loaded.effort = config.effort.clone();
        *state.lock().unwrap() = loaded;
        let mut server: Option<protocol::Client> = None;
        let mut persisted = 0;
        while !stop.load(Ordering::Relaxed) {
            let (generation, next) = {
                let s = state.lock().unwrap();
                (
                    s.generation,
                    s.runs.iter().find(|r| r.status == Status::Queued).cloned(),
                )
            };
            if persisted != generation {
                checkpoint(&config, &state)?;
                persisted = generation;
            }
            if let Some(mut run) = next {
                {
                    let mut s = state.lock().unwrap();
                    let current = s.runs.iter_mut().find(|r| r.id == run.id).unwrap();
                    if current.status != Status::Queued || current.cancel_requested {
                        continue;
                    }
                    current.status = Status::Running;
                    current.activity = "Preparing model context".into();
                    s.event(&run.id, "starting", "Preparing model context");
                }
                checkpoint(&config, &state)?;
                let outcome = (|| {
                    prepare(&mut run.input)?;
                    if serde_json::to_vec(&run.input.context)
                        .map_err(|e| e.to_string())?
                        .len()
                        > 1_000_000
                    {
                        return Err(
                            "prepared agent context exceeds 1 MB; narrow the annotation targets"
                                .into(),
                        );
                    }
                    {
                        let mut s = state.lock().unwrap();
                        let current = s.runs.iter_mut().find(|r| r.id == run.id).unwrap();
                        if current.cancel_requested || stop.load(Ordering::Relaxed) {
                            return Err("Cancelled during context preparation".into());
                        }
                        current.input = run.input.clone();
                    }
                    checkpoint(&config, &state)?;
                    if server.is_none() {
                        server = Some(protocol::Client::start(&config)?);
                    }
                    let client = server.as_mut().unwrap();
                    client.run(&config, &run, &state, &stop)
                })();
                match outcome {
                    Ok(reply) => update(&state, &run.id, "ready", "Attaching reply", |r| {
                        if r.cancel_requested {
                            r.status = Status::Cancelled;
                        } else {
                            r.reply = Some(reply);
                            r.status = Status::Ready;
                        }
                    }),
                    Err(error) => {
                        let cancelled = stop.load(Ordering::Relaxed)
                            || state
                                .lock()
                                .unwrap()
                                .runs
                                .iter()
                                .any(|r| r.id == run.id && r.cancel_requested);
                        update(
                            &state,
                            &run.id,
                            if cancelled { "cancelled" } else { "failed" },
                            if cancelled {
                                "Cancelled"
                            } else {
                                "Codex could not finish"
                            },
                            |r| {
                                r.status = if cancelled {
                                    Status::Cancelled
                                } else {
                                    Status::Failed
                                };
                                r.error = if cancelled { None } else { Some(error) };
                            },
                        );
                        server = None;
                    }
                }
                checkpoint(&config, &state)?;
                let mut s = state.lock().unwrap();
                if let Some(r) = s
                    .runs
                    .iter_mut()
                    .find(|r| r.id == run.id && r.status == Status::Ready)
                {
                    r.delivery_ready = true;
                    s.generation += 1;
                }
            } else {
                thread::sleep(Duration::from_millis(50));
            }
        }
        checkpoint(&config, &state)?;
        Ok(())
    })();
    if let Err(e) = result {
        let mut s = state.lock().unwrap();
        s.ready = false;
        s.error = Some(e.clone());
        s.event("", "error", &e);
    }
}
fn update(
    state: &Arc<Mutex<State>>,
    id: &str,
    kind: &str,
    message: &str,
    f: impl FnOnce(&mut Run),
) {
    let mut s = state.lock().unwrap();
    if let Some(r) = s.runs.iter_mut().find(|r| r.id == id) {
        r.activity = message.chars().take(160).collect();
        f(r);
        r.updated_at = now();
        s.event(id, kind, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(key: &str) -> Input {
        Input {
            discussion: "note".into(),
            key: key.into(),
            revision: 7,
            context: json!({}),
            question: "Explain this part".into(),
            instructions: None,
            developer: None,
        }
    }
    fn wait(service: &Supervisor, p: impl Fn(&State) -> bool) -> State {
        let until = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let s = service.snapshot();
            if p(&s) {
                return s;
            }
            assert!(
                std::time::Instant::now() < until,
                "timed out: {}",
                s.public()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
    #[test]
    fn deduplicates_and_bounds_queue() {
        let mut s = State::default();
        let first = enqueue(&mut s, input("one")).unwrap();
        assert_eq!(first, enqueue(&mut s, input("one")).unwrap());
        assert_eq!(s.runs.len(), 1);
        for i in 1..32 {
            enqueue(&mut s, input(&i.to_string())).unwrap();
        }
        assert!(enqueue(&mut s, input("overflow")).is_err());
    }
    #[test]
    fn restart_retains_pending_reply_but_does_not_repeat_active_turn() {
        let mut s = State::default();
        enqueue(&mut s, input("a")).unwrap();
        enqueue(&mut s, input("b")).unwrap();
        s.runs[0].status = Status::Running;
        s.runs[1].status = Status::Ready;
        s.runs[1].reply = Some(Reply {
            body: "answer".into(),
            links: vec![],
        });
        recover(&mut s);
        assert_eq!(s.runs[0].status, Status::Failed);
        assert_eq!(s.runs[1].status, Status::Ready);
        assert!(s.runs[1].delivery_ready);
    }
    #[cfg(unix)]
    fn fixture(stall: bool) -> (Config, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(uid());
        fs::create_dir_all(&dir).unwrap();
        // A real child process speaking deterministic JSON-RPC, no model account required.
        let script = format!(
            r#"#!/bin/sh
n=0
while IFS= read -r line; do
 printf '%s\n' "$line" >> requests.log
 case "$line" in
  *'"id":'*) n=$((n+1));;
  *) continue;;
 esac
 case "$line" in
  *'"method":"initialize"'*) printf '{{"id":%s,"result":{{}}}}\n' "$n";;
  *'"method":"model/list"'*) printf '{{"id":%s,"result":{{"data":[{{"model":"gpt-6-astra","supportedReasoningEfforts":[{{"reasoningEffort":"high"}}]}}],"nextCursor":null}}}}\n' "$n";;
  *'"method":"thread/start"'*|*'"method":"thread/resume"'*) printf '{{"id":%s,"result":{{"thread":{{"id":"saved-session"}}}}}}\n' "$n";;
  *'"method":"turn/start"'*)
   printf '{{"id":%s,"result":{{"turn":{{"id":"turn-1"}}}}}}\n' "$n"
   if [ "{stall}" = "false" ]; then
    printf '%s\n' '{{"method":"item/started","params":{{"threadId":"saved-session","turnId":"turn-1","item":{{"type":"commandExecution","command":"cat AGENTS.md"}}}}}}'
    printf '%s\n' '{{"method":"item/completed","params":{{"threadId":"saved-session","turnId":"turn-1","item":{{"type":"agentMessage","phase":"final_answer","text":"{{\"body\":\"CAD owns the physical definition.\",\"links\":[]}}"}}}}}}'
    printf '%s\n' '{{"method":"turn/completed","params":{{"threadId":"saved-session","turn":{{"id":"turn-1","status":"completed"}}}}}}'
   fi;;
 esac
done
"#
        );
        let executable = dir.join("fixture.sh");
        fs::write(&executable, script).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        (
            Config {
                cwd: dir.clone(),
                state_path: dir.join("state.json"),
                executable,
                model: MODEL.into(),
                effort: EFFORT.into(),
            },
            dir,
        )
    }
    #[cfg(unix)]
    #[test]
    fn protocol_resume_delivery_and_auto_baseline() {
        let (config, dir) = fixture(false);
        let service = Supervisor::open(config.clone());
        wait(&service, |s| s.ready);
        service.observe(vec![input("old")]).unwrap();
        service.set_auto(true).unwrap();
        service.observe(vec![input("old"), input("new")]).unwrap();
        let s = wait(&service, |s| {
            s.runs.first().is_some_and(|r| r.delivery_ready)
        });
        assert_eq!(s.runs.len(), 1);
        assert_eq!(
            s.runs[0].reply.as_ref().unwrap().body,
            "CAD owns the physical definition."
        );
        let id = s.runs[0].id.clone();
        service.delivered(&id, Ok(()));
        assert!(service.snapshot().runs[0].unread);
        service.mark_read("note");
        drop(service);
        let service = Supervisor::open(config);
        wait(&service, |s| s.ready);
        assert_eq!(service.snapshot().runs[0].status, Status::Completed);
        service.ask(input("followup")).unwrap();
        wait(&service, |s| s.runs.len() == 2 && s.runs[1].delivery_ready);
        let log = fs::read_to_string(dir.join("requests.log")).unwrap();
        assert!(log.contains("thread/resume"));
        assert!(log.contains("\"effort\":\"high\""));
        assert!(log.contains("\"sandbox\":\"read-only\""));
        assert!(log.contains("gpt-6-astra"));
        let turn: Value = log
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|v| v["method"] == "turn/start")
            .unwrap();
        assert_eq!(
            turn["params"]["sandboxPolicy"],
            json!({"type":"readOnly","networkAccess":true})
        );
        drop(service);
        fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn cancellation_and_exclusive_owner() {
        let (config, dir) = fixture(true);
        let service = Supervisor::open(config.clone());
        wait(&service, |s| s.ready);
        let other = Supervisor::open(config);
        let s = wait(&other, |s| s.error.is_some());
        assert!(s.error.unwrap().contains("another viewer"));
        drop(other);
        let id = service.ask(input("cancel")).unwrap();
        wait(&service, |s| s.runs[0].turn.is_some());
        service.cancel(&id).unwrap();
        let s = wait(&service, |s| s.runs[0].status == Status::Cancelled);
        assert!(s.runs[0].reply.is_none());
        drop(service);
        fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn prepared_context_reaches_model_and_is_checkpointed() {
        let (config, dir) = fixture(false);
        let service = Supervisor::open_with_context(
            config.clone(),
            Arc::new(|i| {
                i.context["model_context"] = json!({"resolved_resistance_ohm":220});
                Ok(())
            }),
        );
        wait(&service, |s| s.ready);
        service.ask(input("context")).unwrap();
        let s = wait(&service, |s| {
            s.runs.first().is_some_and(|r| r.delivery_ready)
        });
        assert_eq!(
            s.runs[0].input.context["model_context"]["resolved_resistance_ohm"],
            220
        );
        let saved: State = serde_json::from_slice(&fs::read(&config.state_path).unwrap()).unwrap();
        assert_eq!(saved.runs[0].input.context, s.runs[0].input.context);
        assert!(
            fs::read_to_string(dir.join("requests.log"))
                .unwrap()
                .contains("resolved_resistance_ohm")
        );
        drop(service);
        fs::remove_dir_all(dir).unwrap();
    }
}
