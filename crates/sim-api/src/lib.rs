//! Bounded, loopback-only HTTP control for native and headless Rust hosts.
//! Transport threads never touch application state. The owner polls one ordered
//! command at a time; pending commands retain private continuation state.
pub mod client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub type Result = std::result::Result<Value, String>;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub command: String,
    #[serde(default = "empty")]
    pub args: Value,
}
fn empty() -> Value {
    json!({})
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub commands: Vec<Command>,
    #[serde(default = "yes")]
    pub stop_on_error: bool,
}
fn yes() -> bool {
    true
}
/// Catalog entries carry typed JSON argument examples and completion semantics.
pub fn capability(name: &str, args: Value, description: &str) -> Value {
    json!({"command":name,"args":args,"description":description})
}
pub enum Outcome {
    Pending,
    Done(Result),
    Image(Artifact),
}
pub struct Artifact {
    pub png: Vec<u8>,
    pub metadata: Value,
}
pub struct ImageTask {
    rx: Mutex<std::sync::mpsc::Receiver<std::result::Result<Artifact, String>>>,
}
impl ImageTask {
    pub fn spawn(
        render: impl FnOnce() -> std::result::Result<Artifact, String> + Send + 'static,
    ) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        thread::spawn(move || {
            let _ = tx.send(render());
        });
        Self { rx: Mutex::new(rx) }
    }
    pub fn poll(&mut self, cancelled: bool) -> Outcome {
        match self.rx.lock().unwrap().try_recv() {
            Ok(result) => {
                if cancelled {
                    Outcome::Done(Err("image request cancelled".into()))
                } else {
                    match result {
                        Ok(image) => Outcome::Image(image),
                        Err(e) => Outcome::Done(Err(e)),
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => Outcome::Pending,
            Err(_) => Outcome::Done(Err("image worker stopped".into())),
        }
    }
}
#[derive(Clone)]
struct StoredImage {
    png: Arc<Vec<u8>>,
    receipt: Value,
}

impl From<Result> for Outcome {
    fn from(value: Result) -> Self {
        Self::Done(value)
    }
}
#[derive(Clone, Serialize)]
struct Job {
    id: u64,
    status: &'static str,
    completed: usize,
    total: usize,
    results: Vec<Value>,
    cancel_requested: bool,
}
struct Work {
    id: u64,
    batch: Batch,
    index: usize,
    continuation: Value,
    started: Instant,
}
struct Shared {
    resources: BTreeMap<String, Value>,
    queue: VecDeque<Work>,
    jobs: BTreeMap<u64, Job>,
    next_id: u64,
    images: BTreeMap<u64, StoredImage>,
    next_image: u64,
    /// Called when a command is queued, so a host that sleeps between
    /// frames (a background window) can wake to run it.
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
}
/// Drop stops listening; no process-global runtime or renderer dependency.
pub struct Server {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    active: Option<Work>,
    pub address: SocketAddr,
    published: BTreeMap<String, String>,
    last_snapshot: Option<Instant>,
}
impl Server {
    pub fn bind(port: u16, name: &str, capabilities: Vec<Value>) -> std::io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let shared = Arc::new(Mutex::new(Shared {
            resources: BTreeMap::from([
                (
                    "/".into(),
                    json!({"service":name,"version":1,"address":address.to_string(),"pid":std::process::id(),"batch":"/v1/batch","jobs":"/v1/jobs/{id}"}),
                ),
                (
                    "/v1/capabilities".into(),
                    json!({"version":1,"commands":capabilities,"batch":{"ordered":true,"atomic":false,"max_commands":128,"stop_on_error_default":true},"limits":{"body_bytes":1048576,"queued_batches":64,"retained_jobs":256},"completion":"202 is acceptance only; poll job until succeeded, failed or cancelled; results are per command"}),
                ),
            ]),
            queue: VecDeque::new(),
            jobs: BTreeMap::new(),
            next_id: 1,
            images: BTreeMap::new(),
            next_image: 1,
            waker: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let s = shared.clone();
        let stopping = stop.clone();
        let handle = thread::spawn(move || {
            // A small fixed worker pool bounds connections and isolates slow clients.
            let (tx, rx) = std::sync::mpsc::sync_channel::<TcpStream>(8);
            let rx = Arc::new(Mutex::new(rx));
            let streams = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let workers: Vec<_> = (0..4).map(|_| {
                let rx = rx.clone(); let s = s.clone();
                let streams = streams.clone(); let stopping = stopping.clone();
                thread::spawn(move || loop {
                    let stream = rx.lock().unwrap().recv();
                    let Ok(mut stream) = stream else { break };
                    // Accepted sockets inherit O_NONBLOCK on some platforms (macOS).
                    // The fixed worker pool uses bounded blocking I/O, including large writes.
                    if stream.set_nonblocking(false).is_err() {continue;}
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                    let (status, value) = match read_request(&mut stream, address) {
                        Ok((method, path, body)) => {
                            if method=="GET" && path.starts_with("/v1/events/") {
                                stream_resource(&mut stream,&s,&path,&stopping,&streams);
                                continue;
                            }
                            let png=if method=="GET"{path.strip_prefix("/v1/images/").and_then(|v|v.strip_suffix(".png")).and_then(|v|v.parse::<u64>().ok()).and_then(|id|s.lock().unwrap().images.get(&id).map(|i|i.png.clone()))}else{None};
                            if let Some(png)=png{
                                let _=write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\r\n",png.len());
                                let _=stream.write_all(&png);continue;
                            }
                            route(&s,&method,&path,body)
                        },
                        Err(e) => (400, json!({"error":e})),
                    };
                    let body = serde_json::to_vec(&value).unwrap();
                    let _ = write!(stream, "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\r\n", match status {200=>"OK",202=>"Accepted",404=>"Not Found",429=>"Too Many Requests",_=>"Bad Request"},body.len());
                    let _ = stream.write_all(&body);
                })
            }).collect();
            while !stopping.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = tx.try_send(stream);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
            drop(tx);
            for worker in workers {
                let _ = worker.join();
            }
        });
        Ok(Self {
            shared,
            stop,
            thread: Some(handle),
            active: None,
            address,
            published: BTreeMap::new(),
            last_snapshot: None,
        })
    }
    /// Static descriptions are serialized only when their source identity changes.
    /// Run `wake` whenever a command is queued (for hosts that idle between frames).
    pub fn set_waker(&self, wake: impl Fn() + Send + Sync + 'static) {
        self.shared.lock().unwrap().waker = Some(Arc::new(wake));
    }

    /// True while a job is running or waiting: the host should keep stepping.
    pub fn busy(&self) -> bool {
        self.active.is_some() || !self.shared.lock().unwrap().queue.is_empty()
    }

    pub fn publish_changed(
        &mut self,
        resource: &str,
        revision: &str,
        build: impl FnOnce() -> Value,
    ) {
        if self.published.get(resource).is_none_or(|r| r != revision) {
            self.publish(resource, build());
            self.published.insert(resource.into(), revision.into());
        }
    }
    /// Bound snapshot overhead independently of native rendering cadence.
    pub fn snapshot_due(&mut self) -> bool {
        if self
            .last_snapshot
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(100))
        {
            self.last_snapshot = Some(Instant::now());
            true
        } else {
            false
        }
    }
    pub fn publish(&self, resource: &str, value: Value) {
        self.shared
            .lock()
            .unwrap()
            .resources
            .insert(format!("/v1/{resource}"), value);
    }
    /// At most one command per tick. A pending command must not re-submit work;
    /// its continuation can retain the underlying worker request ID.
    pub fn poll(&mut self, mut execute: impl FnMut(&Command, &mut Value, bool) -> Outcome) {
        if self.active.is_none() {
            self.active = self.shared.lock().unwrap().queue.pop_front();
        }
        let Some(work) = &mut self.active else { return };
        {
            let mut s = self.shared.lock().unwrap();
            let job = s.jobs.get_mut(&work.id).unwrap();
            // Unstarted commands are skipped. A pending operation receives a
            // cooperative cancellation request and must settle before completion.
            if job.cancel_requested && work.continuation.is_null() {
                job.status = "cancelled";
                self.active = None;
                return;
            }
            job.status = "running";
        }
        let cancelled = self.shared.lock().unwrap().jobs[&work.id].cancel_requested;
        let outcome = execute(
            &work.batch.commands[work.index],
            &mut work.continuation,
            cancelled,
        );
        let result = match outcome {
            Outcome::Pending => return,
            Outcome::Done(result) => result,
            Outcome::Image(image) => {
                if image.png.len() > 32 * 1024 * 1024
                    || !image.png.starts_with(b"\x89PNG\r\n\x1a\n")
                {
                    Err("invalid or oversized PNG artifact".into())
                } else {
                    let mut s = self.shared.lock().unwrap();
                    while s.images.len() >= 16
                        || s.images.values().map(|i| i.png.len()).sum::<usize>() + image.png.len()
                            > 64 * 1024 * 1024
                    {
                        s.images.pop_first();
                    }
                    let id = s.next_image;
                    s.next_image += 1;
                    let receipt = json!({"image_id":id,"url":format!("/v1/images/{id}.png"),"metadata_url":format!("/v1/images/{id}"),"content_type":"image/png","bytes":image.png.len(),"metadata":image.metadata});
                    s.images.insert(
                        id,
                        StoredImage {
                            png: Arc::new(image.png),
                            receipt: receipt.clone(),
                        },
                    );
                    Ok(receipt)
                }
            }
        };
        let failed = result.is_err();
        let mut s = self.shared.lock().unwrap();
        let job = s.jobs.get_mut(&work.id).unwrap();
        job.results.push(match result {
            Ok(value) => json!({"index":work.index,"command":work.batch.commands[work.index].command,"ok":true,"value":value}),
            Err(error) => json!({"index":work.index,"command":work.batch.commands[work.index].command,"ok":false,"error":error}),
        });
        work.index += 1;
        job.completed = work.index;
        work.continuation = Value::Null;
        if job.cancel_requested
            || (failed && work.batch.stop_on_error)
            || work.index == work.batch.commands.len()
        {
            job.status = if job.cancel_requested {
                "cancelled"
            } else if job.results.iter().any(|r| r["ok"] == false) {
                "failed"
            } else {
                "succeeded"
            };
            self.active = None;
        } else {
            work.started = Instant::now();
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
/// SSE snapshots for any published resource. Two bounded stream slots leave
/// command workers available. Reconnect after 20 seconds; each connection starts
/// with the complete snapshot, including the resource's retained event cursor.
fn stream_resource(stream:&mut TcpStream,shared:&Mutex<Shared>,path:&str,stop:&AtomicBool,count:&std::sync::atomic::AtomicUsize){
    if count.fetch_update(Ordering::Relaxed,Ordering::Relaxed,|n|if n<2{Some(n+1)}else{None}).is_err(){
        let _=stream.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");return;
    }
    struct Slot<'a>(&'a std::sync::atomic::AtomicUsize);
    impl Drop for Slot<'_>{fn drop(&mut self){self.0.fetch_sub(1,Ordering::Relaxed);}}
    let _slot=Slot(count);
    let resource=format!("/v1/{}",path.trim_start_matches("/v1/events/"));
    if !shared.lock().unwrap().resources.contains_key(&resource){let _=stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");return;}
    if stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\r\nretry: 500\n\n").is_err(){return;}
    let started=Instant::now();let mut last=Value::Null;let mut heartbeat=Instant::now();
    while !stop.load(Ordering::Relaxed)&&started.elapsed()<Duration::from_secs(20){
        let value=shared.lock().unwrap().resources.get(&resource).cloned().unwrap_or(Value::Null);
        if value!=last{
            if writeln!(stream,"event: snapshot\ndata: {}\n",value).is_err(){break;}last=value;heartbeat=Instant::now();
        }else if heartbeat.elapsed()>Duration::from_secs(5){if stream.write_all(b": heartbeat\n\n").is_err(){break;}heartbeat=Instant::now();}
        thread::sleep(Duration::from_millis(100));
    }
}
fn route(shared: &Mutex<Shared>, method: &str, path: &str, body: Value) -> (u16, Value) {
    let mut s = shared.lock().unwrap();
    if method == "GET" {
        if path == "/v1/images" {
            return (
                200,
                json!(s.images.values().map(|i| &i.receipt).collect::<Vec<_>>()),
            );
        }
        if let Some(id) = path
            .strip_prefix("/v1/images/")
            .and_then(|v| v.parse::<u64>().ok())
        {
            return s
                .images
                .get(&id)
                .map(|i| (200, i.receipt.clone()))
                .unwrap_or((404, json!({"error":"unknown or evicted image"})));
        }
        if let Some(v) = s.resources.get(path) {
            return (200, v.clone());
        }
        if path == "/v1/jobs" {
            return (200, json!(s.jobs.values().collect::<Vec<_>>()));
        }
    }
    if let Some(id) = path
        .strip_prefix("/v1/jobs/")
        .and_then(|v| v.parse::<u64>().ok())
    {
        if let Some(job) = s.jobs.get_mut(&id) {
            if method == "DELETE" {
                job.cancel_requested = true;
            }
            if method == "GET" || method == "DELETE" {
                return (200, json!(job));
            }
        }
        return (
            404,
            json!({"error":"unknown job (completed jobs have bounded retention)"}),
        );
    }
    if method == "POST" && (path == "/v1/batch" || path == "/v1/commands") {
        let batch = if path == "/v1/batch" {
            serde_json::from_value::<Batch>(body)
        } else {
            serde_json::from_value::<Command>(body).map(|command| Batch {
                commands: vec![command],
                stop_on_error: true,
            })
        };
        let batch = match batch {
            Ok(b) if !b.commands.is_empty() && b.commands.len() <= 128 => b,
            Ok(_) => return (400, json!({"error":"batch requires 1 to 128 commands"})),
            Err(e) => return (400, json!({"error":e.to_string()})),
        };
        if s.queue.len() >= 64 {
            return (429, json!({"error":"command queue is full"}));
        }
        if s.jobs.len() >= 256 {
            if let Some(id) = s
                .jobs
                .iter()
                .find(|(_, j)| matches!(j.status, "succeeded" | "failed" | "cancelled"))
                .map(|(id, _)| *id)
            {
                s.jobs.remove(&id);
            } else {
                return (429, json!({"error":"job capacity reached"}));
            }
        }
        let id = s.next_id;
        s.next_id += 1;
        s.jobs.insert(
            id,
            Job {
                id,
                status: "queued",
                completed: 0,
                total: batch.commands.len(),
                results: vec![],
                cancel_requested: false,
            },
        );
        s.queue.push_back(Work {
            id,
            batch,
            index: 0,
            continuation: Value::Null,
            started: Instant::now(),
        });
        if let Some(wake) = &s.waker {
            wake();
        }
        return (202, json!({"job_id":id,"url":format!("/v1/jobs/{id}")}));
    }
    (
        404,
        json!({"error":"unknown endpoint","discovery":"/v1/capabilities"}),
    )
}
fn read_request(
    stream: &mut TcpStream,
    address: SocketAddr,
) -> std::result::Result<(String, String, Value), String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() >= 16384 || Instant::now() > deadline {
            return Err("header limit exceeded".into());
        }
        stream.read_exact(&mut byte).map_err(|e| e.to_string())?;
        bytes.push(byte[0]);
    }
    let header = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let mut lines = header.split("\r\n");
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let method = first.next().ok_or("missing method")?.to_owned();
    let path = first.next().ok_or("missing path")?.to_owned();
    if first.next() != Some("HTTP/1.1") || first.next().is_some() {
        return Err("HTTP/1.1 required".into());
    }
    let mut headers = BTreeMap::new();
    for line in lines.filter(|l| !l.is_empty()) {
        let (key, value) = line.split_once(':').ok_or("invalid header")?;
        if headers
            .insert(key.to_ascii_lowercase(), value.trim().to_owned())
            .is_some()
        {
            return Err("duplicate header".into());
        }
    }
    let hosts = [address.to_string(), format!("localhost:{}", address.port())];
    if !headers.get("host").is_some_and(|h| hosts.contains(h)) {
        return Err("loopback Host required".into());
    }
    if headers.contains_key("origin") || headers.contains_key("transfer-encoding") {
        return Err("browser-origin and chunked requests are not supported".into());
    }
    let length = headers
        .get("content-length")
        .map(|v| v.parse::<usize>().map_err(|_| "invalid content length"))
        .transpose()?
        .unwrap_or(0);
    if length > 1048576 {
        return Err("body exceeds 1 MiB".into());
    }
    if method == "POST"
        && !headers
            .get("content-type")
            .is_some_and(|v| v.split(';').next() == Some("application/json"))
    {
        return Err("application/json required".into());
    }
    let mut body = vec![0; length];
    let mut n = 0;
    while n < length {
        if Instant::now() > deadline {
            return Err("request timeout".into());
        }
        let count = stream.read(&mut body[n..]).map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("incomplete body".into());
        }
        n += count;
    }
    let value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).map_err(|e| e.to_string())?
    };
    Ok((method, path, value))
}
/// Deserialize arguments strictly by using deny_unknown_fields on command enums.
pub fn decode<T: serde::de::DeserializeOwned>(command: &Command) -> std::result::Result<T, String> {
    let mut value = command
        .args
        .as_object()
        .cloned()
        .ok_or("args must be an object")?;
    if value
        .insert("command".into(), json!(command.command))
        .is_some()
    {
        return Err("args cannot override command".into());
    }
    serde_json::from_value(Value::Object(value)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn submit(server: &Server, commands: Value, stop: bool) -> u64 {
        let (status, v) = route(
            &server.shared,
            "POST",
            "/v1/batch",
            json!({"commands":commands,"stop_on_error":stop}),
        );
        assert_eq!(status, 202);
        v["job_id"].as_u64().unwrap()
    }
    fn job(server: &Server, id: u64) -> Value {
        route(
            &server.shared,
            "GET",
            &format!("/v1/jobs/{id}"),
            Value::Null,
        )
        .1
    }
    #[test]
    fn real_http_transfers_large_json_and_binary_artifacts_without_truncation() {
        let mut server = Server::bind(0, "large-payload", vec![]).unwrap();
        let url = format!("http://{}", server.address);
        let value = json!({"samples":"x".repeat(4*1024*1024)});
        server.publish("large", value.clone());
        assert!(
            client::request(&url, "GET", "/v1/large", None).unwrap() == value,
            "large JSON changed in transit"
        );
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(vec![91; 2 * 1024 * 1024]);
        let expected = png.clone();
        let id = submit(&server, json!([{"command":"render"}]), true);
        server.poll(|_, _, _| {
            Outcome::Image(Artifact {
                png: png.clone(),
                metadata: json!({"source":"test"}),
            })
        });
        let receipt = job(&server, id)["results"][0]["value"].clone();
        assert!(
            client::image(&url, receipt["url"].as_str().unwrap()).unwrap() == expected,
            "binary response changed in transit"
        );
        assert_eq!(
            client::request(&url, "GET", receipt["metadata_url"].as_str().unwrap(), None).unwrap()
                ["metadata"]["source"],
            "test"
        );
    }
    #[test]
    fn batches_wait_for_completion_and_retain_order_and_partial_errors() {
        let mut server = Server::bind(0, "test", vec![]).unwrap();
        let id = submit(
            &server,
            json!([{"command":"increment"},{"command":"read"},{"command":"fail"},{"command":"read"}]),
            false,
        );
        let mut count = 0;
        server.poll(|_, continuation, _| {
            *continuation = json!(true);
            Outcome::Pending
        });
        assert_eq!(job(&server, id)["completed"], 0);
        for _ in 0..4 {
            server.poll(|command, _, _| match command.command.as_str() {
                "increment" => {
                    count += 1;
                    Ok(json!(count)).into()
                }
                "read" => Ok(json!(count)).into(),
                _ => Err("test failure".into()).into(),
            });
        }
        let j = job(&server, id);
        assert_eq!(j["status"], "failed");
        assert_eq!(j["completed"], 4);
        assert_eq!(j["results"][1]["value"], 1);
        assert_eq!(j["results"][3]["value"], 1);
    }
    #[test]
    fn stop_on_error_and_queued_cancellation_never_apply_later_commands() {
        let mut server = Server::bind(0, "test", vec![]).unwrap();
        let id = submit(
            &server,
            json!([{"command":"fail"},{"command":"unreachable"}]),
            true,
        );
        server.poll(|_, _, _| Err("failed".into()).into());
        assert_eq!(job(&server, id)["completed"], 1);
        let id = submit(&server, json!([{"command":"cancelled"}]), true);
        route(
            &server.shared,
            "DELETE",
            &format!("/v1/jobs/{id}"),
            Value::Null,
        );
        server.poll(|_, _, _| panic!("cancelled command executed"));
        assert_eq!(job(&server, id)["status"], "cancelled");
    }
    /// A host that idles between frames is woken when work arrives, and
    /// reports itself busy until the job has run.
    #[test]
    fn queued_commands_wake_the_host_and_keep_it_busy_until_run() {
        let mut server = Server::bind(0, "test", vec![]).unwrap();
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w = woken.clone();
        server.set_waker(move || {
            w.fetch_add(1, Ordering::SeqCst);
        });
        assert!(!server.busy());
        let id = submit(&server, json!([{"command":"x"}]), true);
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        assert!(server.busy());
        server.poll(|_, _, _| Ok(json!(1)).into());
        assert_eq!(job(&server, id)["status"], "succeeded");
        assert!(!server.busy());
    }
    #[test]
    fn empty_oversized_batches_and_reserved_command_injection_are_rejected() {
        let server = Server::bind(0, "test", vec![]).unwrap();
        for commands in [
            json!([]),
            json!((0..129).map(|_| json!({"command":"x"})).collect::<Vec<_>>()),
        ] {
            assert_eq!(
                route(
                    &server.shared,
                    "POST",
                    "/v1/batch",
                    json!({"commands":commands})
                )
                .0,
                400
            );
        }
        assert!(
            decode::<Value>(&Command {
                command: "x".into(),
                args: json!({"command":"y"})
            })
            .is_err()
        );
    }
    fn request(server: &Server, extra: &str, host: Option<&str>) -> String {
        let mut stream = TcpStream::connect(server.address).unwrap();
        let host = host
            .map(str::to_owned)
            .unwrap_or_else(|| server.address.to_string());
        write!(
            stream,
            "GET /v1/state HTTP/1.1\r\nHost: {host}\r\n{extra}\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }
    #[test]
    fn sse_delivers_changes_while_rest_remains_available() {
        use std::io::{BufRead,BufReader};
        let server=Server::bind(0,"sse-test",vec![]).unwrap();
        server.publish("agent",json!({"generation":1,"status":"queued"}));
        server.publish("state",json!({"revision":7}));
        let mut socket=TcpStream::connect(server.address).unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        write!(socket,"GET /v1/events/agent HTTP/1.1\r\nHost: {}\r\n\r\n",server.address).unwrap();
        let mut reader=BufReader::new(socket);
        let mut line=String::new();
        loop{line.clear();reader.read_line(&mut line).unwrap();if line.starts_with("data:"){break;}}
        assert!(line.contains("queued"));
        assert!(request(&server,"",None).ends_with("{\"revision\":7}"));
        server.publish("agent",json!({"generation":2,"status":"running"}));
        loop{line.clear();reader.read_line(&mut line).unwrap();if line.starts_with("data:"){break;}}
        assert!(line.contains("running"));
        drop(reader);drop(server);
    }
    #[test]
    fn actual_http_reads_published_state_and_rejects_browser_and_rebinding() {
        let server = Server::bind(0, "test", vec![]).unwrap();
        server.publish("state", json!({"revision":7}));
        let response = request(&server, "", None);
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.ends_with("{\"revision\":7}"));
        assert!(
            request(&server, "Origin: https://example.org\r\n", None).starts_with("HTTP/1.1 400")
        );
        assert!(request(&server, "", Some("example.org")).starts_with("HTTP/1.1 400"));
        assert!(request(&server, "Content-Length: 1048577\r\n", None).starts_with("HTTP/1.1 400"));
    }
}
