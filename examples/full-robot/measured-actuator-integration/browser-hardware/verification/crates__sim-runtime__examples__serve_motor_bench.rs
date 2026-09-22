//! Loopback browser adapter over the existing finite FPGA acquisition path.
//! No controller or physics implementation lives in this HTTP adapter.
use serde::Deserialize;
use serde_json::{Value, json};
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
fn start(app: &Arc<App>, request: Option<Playback>, client: &str) -> Result<Value, String> {
    let plan = request
        .as_ref()
        .map(|r| app.trace.bind_bench_clip(r, &app.template, &app.config.ids))
        .transpose()?;
    let mut s = app.state.lock().unwrap();
    if s.active {
        return Err("A capture already owns the serial port".into());
    }
    if request.is_some() && s.inspection["completed"] != true {
        return Err("Inspect connected hardware before starting".into());
    }
    if request.is_some()
        && s.result["mode"] == "fpga_device_clock_pd"
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
    let command = plan
        .as_ref()
        .map(|p| serde_json::to_value(p).unwrap())
        .unwrap_or(json!({"control":"inspect"}));
    write_json(dir.join("plan.json"), &command)?;
    write_json(
        dir.join("binding.json"),
        &json!({"request":request,"source":app.trace.source,"source_sha256":app.trace.source_sha256,"trace_blake3":blake3::hash(&serde_json::to_vec(&app.trace).unwrap()).to_hex().to_string(),"note":"Relative to measured home. Final 200 ms returns to home. Reference playback, not live quadruped simulation."}),
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
    s.active = true;
    s.lease = Instant::now();
    s.owner = client.to_owned();
    s.run = Some(dir.clone());
    s.request = serde_json::to_value(&request).unwrap();
    s.plan = command;
    s.result = Value::Null;
    let app = Arc::clone(app);
    let motion = request.is_some();
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
                    let _ = fs::write(dir.join("capture/STOP"), b"child status failure");
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
                let _ = fs::write(dir.join("capture/STOP"), b"Browser lease expired");
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    });
    Ok(json!({"started":true}))
}
fn response(stream: &mut TcpStream, code: u16, kind: &str, body: &[u8]) {
    let reason = if code == 200 { "OK" } else { "Rejected" };
    let _ = write!(
        stream,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: frame-ancestors 'none'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(body);
}
fn handle(mut stream: TcpStream, app: Arc<App>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let result = (|| -> Result<(String, String), String> {
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
                    .replace("__TOKEN__", &app.token),
            ));
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
                json!({"ids":app.config.ids,"coordinates":app.trace.coordinates,"source":app.trace.source,"source_sha256":app.trace.source_sha256})
            }
            ("GET", "/status") => status(&app, client),
            ("POST", "/inspect") => start(&app, None, client)?,
            ("POST", "/preview") => {
                let r: Playback =
                    serde_json::from_slice(&data[end..end + length]).map_err(|e| e.to_string())?;
                json!({"plan":app.trace.bind_bench_clip(&r,&app.template,&app.config.ids)?})
            }
            ("POST", "/run") => start(
                &app,
                Some(serde_json::from_slice(&data[end..end + length]).map_err(|e| e.to_string())?),
                client,
            )?,
            ("POST", "/stop") => {
                let s = app.state.lock().unwrap();
                if s.active {
                    if let Some(p) = &s.run {
                        fs::create_dir_all(p.join("capture")).map_err(|e| e.to_string())?;
                        fs::write(p.join("capture/STOP"), b"Browser STOP")
                            .map_err(|e| e.to_string())?;
                    }
                }
                json!({"stop_requested":true,"physical_stop_verified":false})
            }
            _ => return Err("Unknown endpoint".into()),
        };
        Ok(("application/json".into(), v.to_string()))
    })();
    match result {
        Ok((kind, body)) => response(&mut stream, 200, &kind, body.as_bytes()),
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
