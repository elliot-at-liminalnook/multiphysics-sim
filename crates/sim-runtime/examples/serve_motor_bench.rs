//! Browser compatibility transport over shared in-process bench application behavior.
use serde_json::json;
use sim_runtime::{
    controller_refinement::{
        live_reference::{Request as LiveRequest, Sample as LiveSample},
        trajectory_binding::Playback,
    },
    hardware::bench::{App as Driver, Config},
};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    time::Duration,
};
type E = Box<dyn std::error::Error>;
struct App {
    config: Config,
    driver: Arc<Driver>,
    token: String,
    origin: String,
}
fn launch(
    app: &Arc<App>,
    request: Option<Playback>,
    owner: &str,
    live: Option<LiveRequest>,
) -> Result<serde_json::Value, String> {
    let work = app.driver.start(request, owner, live)?;
    std::thread::Builder::new()
        .name("browser bench acquisition".into())
        .spawn(move || {
            let _ = work.run();
        })
        .map_err(|e| e.to_string())?;
    Ok(json!({"started":true}))
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
                json!({"ids":app.driver.config()["ids"],"coordinates":app.driver.config()["coordinates"],"source":app.driver.config()["source"],"source_sha256":app.driver.config()["source_sha256"],"streamed_only":app.driver.config()["streamed_only"]})
            }
            ("GET", "/status") => app.driver.status(client),
            ("POST", "/inspect") => launch(&app, None, client, None)?,
            ("POST", "/preview") => {
                let r: Playback =
                    serde_json::from_slice(&data[end..end + length]).map_err(|e| e.to_string())?;
                app.driver.preview(&r)?
            }
            ("POST", "/run") => launch(
                &app,
                Some(serde_json::from_slice(&data[end..end + length]).map_err(|e| e.to_string())?),
                client,
                None,
            )?,
            ("POST", "/live/open") => launch(
                &app,
                None,
                client,
                Some(serde_json::from_slice(&data[end..end + length]).map_err(|e| e.to_string())?),
            )?,
            ("POST", "/live/sample") => {
                let sample: LiveSample =
                    serde_json::from_slice(&data[end..end + length]).map_err(|e| e.to_string())?;
                app.driver.sample(client, &sample)?
            }
            ("POST", "/stop") => app.driver.stop()?,
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
    let driver = Driver::open(config.clone())?;
    let mut random = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let token = random
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect::<String>();
    let listener = TcpListener::bind(format!("127.0.0.1:{}", args[2]))?;
    let origin = format!("http://{}", listener.local_addr()?);
    let app = Arc::new(App {
        config,
        driver,
        token,
        origin: origin.clone(),
    });
    println!("Motor bench: {origin}");
    for stream in listener.incoming() {
        let stream = stream?;
        let app = Arc::clone(&app);
        std::thread::spawn(move || handle(stream, app));
    }
    app.driver.stop()?;
    Ok(())
}
