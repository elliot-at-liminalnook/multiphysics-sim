//! Loopback browser adapter over the existing finite FPGA acquisition path.
//! No controller or physics implementation lives in this HTTP adapter.
use serde::Deserialize;
use serde_json::{Value, json};
use sim_runtime::controller_refinement::live_reference::{
    Cursor, Received, Request as LiveRequest, Sample as LiveSample,
};
use sim_runtime::controller_refinement::{
    fpga::Plan,
    trajectory_binding::{Playback, ReferenceTrace},
};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
type E = Box<dyn std::error::Error>;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    port: String,
    ids: Vec<u8>,
    trace: PathBuf,
    template: PathBuf,
    acquisition: PathBuf,
    output: PathBuf,
    inspection: PathBuf,
    #[serde(default)]
    walking_root: Option<PathBuf>,
    #[serde(default)]
    streamed_only: bool,
}
struct App {
    config: Config,
    trace: ReferenceTrace,
    template: Plan,
    token: String,
    origin: String,
    state: Mutex<State>,
}
struct State {
    active: bool,
    lease: Instant,
    owner: String,
    run: Option<PathBuf>,
    request: Value,
    plan: Value,
    result: Value,
    inspection: Value,
    live_cursor: Option<Cursor>,
    live_request: Option<LiveRequest>,
}
fn value(path: impl AsRef<std::path::Path>) -> Value {
    fs::read(path)
        .ok()
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or(Value::Null)
}
fn lines(path: PathBuf) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}
fn write_json(path: impl AsRef<std::path::Path>, v: &impl serde::Serialize) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn status(app: &App, client: &str) -> Value {
    let mut s = app.state.lock().unwrap();
    if s.owner == client {
        s.lease = Instant::now();
    }
    let live = s
        .run
        .as_ref()
        .map(|p| lines(p.join("capture/live-telemetry.jsonl")))
        .unwrap_or_default();
    json!({"active":s.active,"run":s.run,"request":s.request,"plan":s.plan,"result":s.result,"inspection":s.inspection,"samples":live})
}
fn start(
    app: &Arc<App>,
    request: Option<Playback>,
    client: &str,
    live: Option<LiveRequest>,
) -> Result<Value, String> {
    if request.is_some() && app.config.streamed_only {
        return Err("This FPGA profile uses live targets. Open /walking/ to run the motors.".into());
    }
    let mut plan = request
        .as_ref()
        .map(|r| app.trace.bind_bench_clip(r, &app.template, &app.config.ids))
        .transpose()?;
    if let Some(live) = &live {
        live.validate(&app.config.ids)?;
        if live
            .bindings
            .iter()
            .any(|b| !app.trace.coordinates.contains(&b.coordinate))
        {
            return Err("Unknown CAD coordinate".into());
        }
        let mut p = app.template.clone();
        p.ids = live.bindings.iter().map(|b| b.motor_id).collect();
        p.ids.sort_unstable();
        p.name = format!("Live 100 Hz FPGA WASD references: {}", live.source);
        p.control = "fpga_device_pd".into();
        p.role = "timing".into();
        p.period_s = 0.01;
        p.gains.limit = 100;
        p.targets = vec![[0; 9]; 1200];
        p.validate()?;
        plan = Some(p);
    }
    let mut s = app.state.lock().unwrap();
    if s.active {
        return Err("A capture already owns the serial port".into());
    }
    if (request.is_some() || live.is_some()) && s.inspection["completed"] != true {
        return Err("Inspect connected hardware before starting".into());
    }
    if (request.is_some() || live.is_some())
        && s.result["mode"]
            .as_str()
            .is_some_and(|m| m.starts_with("fpga"))
        && s.result["result"]["stop_verified"] != true
    {
        return Err("Previous physical stop was not verified; inspect before rearming".into());
    }
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = app.config.output.join(format!("session-{id}"));
    fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let mut command = plan
        .as_ref()
        .map(|p| serde_json::to_value(p).unwrap())
        .unwrap_or(json!({"control":"inspect"}));
    if let Some(live) = &live {
        command = json!({"control":"fpga_live_device_reference","request":live,"plan":command});
        write_json(
            dir.join("latest-reference.json"),
            &Received {
                session: dir.file_name().unwrap().to_string_lossy().into(),
                received_unix_s: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs_f64(),
                sample: live.initial.clone(),
            },
        )?;
    }
    write_json(dir.join("plan.json"), &command)?;
    write_json(
        dir.join("binding.json"),
        &if let Some(live)=&live {json!({"request":live,"source":live.source,"period_s":0.01,"maximum_session_s":12.0,"note":"Current live Rust/WASM references; relative bench scaling and measured homes. FPGA schedules feedback and control; the host queues bounded target segments. Initial 160 ms hold; live targets buffered approximately 80 to 160 ms. No predetermined gait playback."})}else{json!({"request":request,"source":app.trace.source,"source_sha256":app.trace.source_sha256,"trace_blake3":blake3::hash(&serde_json::to_vec(&app.trace).unwrap()).to_hex().to_string(),"note":"Relative to measured home. Final 200 ms returns to home. Reference playback, not live quadruped simulation."})},
    )?;
    let log = fs::File::create(dir.join("acquisition.log")).map_err(|e| e.to_string())?;
    let mut child = Command::new(&app.config.acquisition)
        .env("HX_BAUD", "1000000")
        .arg(&app.config.port)
        .arg("2")
        .arg(
            app.config
                .ids
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(","),
        )
        .arg(dir.join("capture"))
        .arg(dir.join("plan.json"))
        .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|e| e.to_string())?;
    s.live_cursor = live.as_ref().map(|r| Cursor::new(&r.initial));
    s.live_request = live.clone();
    s.active = true;
    s.lease = Instant::now();
    s.owner = client.to_owned();
    s.run = Some(dir.clone());
    s.request = serde_json::to_value(&request).unwrap();
    s.plan = command;
    s.result = Value::Null;
    let app = Arc::clone(app);
    let motion = request.is_some() || live.is_some();
    std::thread::spawn(move || {
        loop {
            match child.try_wait() {
                Ok(Some(exit)) => {
                    let mut s = app.state.lock().unwrap();
                    let mut result = value(dir.join("capture/run.json"));
                    if result.is_null() {
                        result = json!({"completed":false,"error":"Acquisition exited without a result"});
                    }
                    result["process_exit_success"] = json!(exit.success());
                    if !motion {
                        s.inspection = result.clone();
                    }
                    s.result = result;
                    s.active = false;
                    break;
                }
                Err(e) => {
                    let _ = request_capture_stop(&dir, b"child status failure");
                    let _ = child.wait();
                    let mut s = app.state.lock().unwrap();
                    s.result = json!({"completed":false,"error":e.to_string()});
                    s.active = false;
                    break;
                }
                Ok(None) => {}
            }
            let expired = app.state.lock().unwrap().lease.elapsed() > Duration::from_millis(900);
            if motion && expired {
                let _ = request_capture_stop(&dir, b"Browser lease expired");
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    });
    Ok(json!({"started":true}))
}
fn request_capture_stop(dir: &std::path::Path, reason: &[u8]) -> std::io::Result<()> {
    // The child exclusively creates capture/. Never race it by creating that
    // directory from STOP; retain an early cancellation in the session root.
    fs::write(dir.join("STOP"), reason)?;
    if dir.join("capture").is_dir() { fs::write(dir.join("capture/STOP"), reason)?; }
    Ok(())
}
fn response(stream: &mut TcpStream, code: u16, kind: &str, body: &[u8]) {
    let reason = if code == 200 { "OK" } else { "Rejected" };
    let _ = write!(
        stream,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nCross-Origin-Opener-Policy: same-origin\r\nCross-Origin-Embedder-Policy: require-corp\r\nContent-Security-Policy: frame-ancestors 'none'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(body);
}
fn handle(mut stream: TcpStream, app: Arc<App>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let result = (|| -> Result<(String, Vec<u8>), String> {
        let mut data = Vec::new();
        let end = loop {
            let mut b = [0; 1024];
            let n = stream.read(&mut b).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("Incomplete request".into());
            }
            data.extend_from_slice(&b[..n]);
            if data.len() > 16384 {
                return Err("Request too large".into());
            }
            if let Some(p) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                break p + 4;
            }
        };
        let headers = String::from_utf8(data[..end].to_vec()).map_err(|_| "Invalid headers")?;
        let mut first = headers.lines().next().unwrap_or("").split_whitespace();
        let method = first.next().unwrap_or("");
        let path = first.next().unwrap_or("");
        let header = |key: &str| {
            headers
                .lines()
                .skip(1)
                .filter_map(|l| l.split_once(':'))
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.trim())
        };
        let expected_host = app.origin.strip_prefix("http://").unwrap();
        if header("host") != Some(expected_host)
            || header("origin").is_some_and(|o| o != app.origin)
            || header("sec-fetch-site").is_some_and(|s| !["same-origin", "none"].contains(&s))
        {
            return Err("Only same-origin loopback access is accepted".into());
        }
        if method == "GET" && path == "/" {
            return Ok((
                "text/html; charset=utf-8".into(),
                include_str!("../../../web/motor-bench/index.html")
                    .replace("__TOKEN__", &app.token)
                    .into_bytes(),
            ));
        }
        if method == "GET" && path.starts_with("/walking/") {
            let root = app
                .config
                .walking_root
                .as_ref()
                .ok_or("Walking viewer not configured")?
                .canonicalize()
                .map_err(|e| e.to_string())?;
            let relative = path
                .trim_start_matches("/walking/")
                .split('?')
                .next()
                .unwrap_or("");
            let file = root
                .join(if relative.is_empty() {
                    "index.html"
                } else {
                    relative
                })
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if !file.starts_with(&root) {
                return Err("Outside walking bundle".into());
            }
            let ext = file.extension().and_then(|s| s.to_str()).unwrap_or("");
            let kind = match ext {
                "html" => "text/html",
                "js" | "mjs" => "text/javascript",
                "json" => "application/json",
                "css" => "text/css",
                "wasm" => "application/wasm",
                _ => "application/octet-stream",
            };
            let mut bytes = fs::read(&file).map_err(|e| e.to_string())?;
            if ext == "html" {
                bytes = String::from_utf8(bytes)
                    .map_err(|e| e.to_string())?
                    .replace("__CONTROL_TOKEN__", &app.token)
                    .into_bytes();
            }
            return Ok((kind.into(), bytes));
        }
        if header("x-control-token") != Some(app.token.as_str()) {
            return Err("Session token required".into());
        }
        let length = header("content-length")
            .unwrap_or("0")
            .parse::<usize>()
            .map_err(|_| "Invalid length")?;
        if length > 8192 || header("transfer-encoding").is_some() {
            return Err("Invalid request size".into());
        }
        while data.len() < end + length {
            let mut b = [0; 1024];
            let n = stream.read(&mut b).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("Incomplete body".into());
            }
            data.extend_from_slice(&b[..n]);
        }
        let client = header("x-client-id").ok_or("Browser instance identity required")?;
        if client.len() != 36 || !client.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
            return Err("Invalid browser identity".into());
        }
        let v = match (method, path) {
            ("GET", "/config") => {
                json!({"ids":app.config.ids,"coordinates":app.trace.coordinates,"source":app.trace.source,"source_sha256":app.trace.source_sha256,"streamed_only":app.config.streamed_only})
            }
            ("GET", "/status") => status(&app, client),
            ("POST", "/inspect") => start(&app, None, client, None)?,
            ("POST", "/preview") => {
                let r: Playback =
                    serde_json::from_slice(&data[end..end + length]).map_err(|e| e.to_string())?;
                json!({"plan":app.trace.bind_bench_clip(&r,&app.template,&app.config.ids)?})
            }
            ("POST", "/run") => start(
                &app,
                Some(serde_json::from_slice(&data[end..end + length]).map_err(|e| e.to_string())?),
                client,
                None,
            )?,
            ("POST", "/live/open") => start(
                &app,
                None,
                client,
                Some(serde_json::from_slice(&data[end..end + length]).map_err(|e| e.to_string())?),
            )?,
            ("POST", "/live/sample") => {
                let sample: LiveSample =
                    serde_json::from_slice(&data[end..end + length]).map_err(|e| e.to_string())?;
                let mut s = app.state.lock().unwrap();
                if !s.active || s.owner != client || s.live_request.is_none() {
                    return Err("No live session owned by this tab".into());
                }
                s.live_request.as_ref().unwrap().validate_sample(&sample)?;
                let changed = s.live_cursor.as_mut().unwrap().advance(&sample)?;
                if changed {
                    let dir = s.run.as_ref().unwrap();
                    let r = Received {
                        session: dir.file_name().unwrap().to_string_lossy().into(),
                        received_unix_s: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs_f64(),
                        sample,
                    };
                    write_json(dir.join("latest-reference.tmp"), &r)?;
                    fs::rename(
                        dir.join("latest-reference.tmp"),
                        dir.join("latest-reference.json"),
                    )
                    .map_err(|e| e.to_string())?;
                    let mut log = fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(dir.join("browser-references.jsonl"))
                        .map_err(|e| e.to_string())?;
                    writeln!(log, "{}", serde_json::to_string(&r).unwrap())
                        .map_err(|e| e.to_string())?;
                    s.lease = Instant::now();
                }
                json!({"accepted_new_frame":changed})
            }
            ("POST", "/stop") => {
                let s = app.state.lock().unwrap();
                if s.active {
                    if let Some(p) = &s.run {
                        request_capture_stop(p, b"Browser STOP")
                            .map_err(|e| e.to_string())?;
                    }
                }
                json!({"stop_requested":true,"physical_stop_verified":false})
            }
            _ => return Err("Unknown endpoint".into()),
        };
        Ok(("application/json".into(), v.to_string().into_bytes()))
    })();
    match result {
        Ok((kind, body)) => response(&mut stream, 200, &kind, &body),
        Err(e) => response(
            &mut stream,
            400,
            "application/json",
            json!({"error":e}).to_string().as_bytes(),
        ),
    }
}
fn main() -> Result<(), E> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("serve_motor_bench CONFIG_JSON HTTP_PORT".into());
    }
    let config: Config = serde_json::from_slice(&fs::read(&args[1])?)?;
    let trace: ReferenceTrace = serde_json::from_slice(&fs::read(&config.trace)?)?;
    trace.validate()?;
    let template: Plan = serde_json::from_slice(&fs::read(&config.template)?)?;
    template.validate()?;
    sim_runtime::controller_refinement::fpga::validate_physical_scope(&config.ids, &config.ids)?;
    if blake3::hash(&fs::read(&template.bitstream_path)?)
        .to_hex()
        .as_str()
        != template.bitstream_blake3
    {
        return Err("Bitstream identity mismatch".into());
    }
    fs::create_dir_all(&config.output)?;
    // Exclusive owner across UI processes, released on normal shutdown; stale lock is explicit.
    let lock = config.output.join("SERVER.lock");
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&lock)?
        .write_all(std::process::id().to_string().as_bytes())?;
    let mut random = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let token = random
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect::<String>();
    let listener = TcpListener::bind(format!("127.0.0.1:{}", args[2]))?;
    let origin = format!("http://{}", listener.local_addr()?);
    let inspection = value(&config.inspection);
    let app = Arc::new(App {
        config,
        trace,
        template,
        token,
        origin: origin.clone(),
        state: Mutex::new(State {
            active: false,
            lease: Instant::now(),
            owner: String::new(),
            run: None,
            request: Value::Null,
            plan: Value::Null,
            result: Value::Null,
            inspection,
            live_cursor: None,
            live_request: None,
        }),
    });
    println!("Motor bench: {origin}");
    for stream in listener.incoming() {
        let stream = stream?;
        let app = Arc::clone(&app);
        std::thread::spawn(move || handle(stream, app));
    }
    let _ = fs::remove_file(lock);
    Ok(())
}

#[cfg(test)]
mod stop_request_tests {
    use super::*;
    #[test]
    fn early_stop_does_not_create_the_child_capture_directory() {
        let dir=std::env::temp_dir().join(format!("motor-early-stop-{}",std::process::id()));
        fs::create_dir(&dir).unwrap();
        request_capture_stop(&dir,b"early cancel").unwrap();
        assert!(dir.join("STOP").exists());assert!(!dir.join("capture").exists());
        fs::create_dir(dir.join("capture")).unwrap();
        request_capture_stop(&dir,b"cancel after start").unwrap();
        assert!(dir.join("capture/STOP").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
