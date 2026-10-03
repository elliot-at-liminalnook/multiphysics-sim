//! Browser compatibility transport over the shared in-process calibration application.
use serde_json::{Value, json};
use sim_runtime::hardware::calibration::{Config, Handle, Service};
use sim_runtime::hardware::protocol::calibration::{
    BINDING_REFUSED, BINDING_REFUSED_STATUS, ExecutionIdentity,
};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
type R<T> = Result<T, String>;
struct Adapter {
    handle: Handle,
    execution: ExecutionIdentity,
    origin: String,
    token: String,
    viewer: PathBuf,
}
fn percent_decode(s: &str) -> R<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                out.push(
                    u8::from_str_radix(
                        std::str::from_utf8(&bytes[i + 1..i + 3]).map_err(|e| e.to_string())?,
                        16,
                    )
                    .map_err(|e| e.to_string())?,
                );
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|e| e.to_string())
}
fn reply(s: &mut TcpStream, status: u16, kind: &str, body: &[u8]) {
    let _ = write!(
        s,
        "HTTP/1.1 {status} Response\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nCross-Origin-Opener-Policy: same-origin\r\nCross-Origin-Embedder-Policy: require-corp\r\nContent-Security-Policy: frame-ancestors 'none'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(body);
}
fn handle(mut stream: TcpStream, app: Arc<Adapter>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let result = (|| -> R<(String, Vec<u8>)> {
        let mut data = Vec::new();
        let end = loop {
            let mut b = [0; 1024];
            let n = stream.read(&mut b).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("Incomplete request".into());
            }
            data.extend(&b[..n]);
            if data.len() > 16384 {
                return Err("Request too large".into());
            }
            if let Some(p) = data.windows(4).position(|v| v == b"\r\n\r\n") {
                break p + 4;
            }
        };
        let headers = String::from_utf8(data[..end].to_vec()).map_err(|e| e.to_string())?;
        let first = headers
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>();
        if first.len() != 3 {
            return Err("Bad request".into());
        }
        let (method, path) = (first[0], first[1].split('?').next().unwrap());
        let h = |key: &str| {
            headers
                .lines()
                .skip(1)
                .filter_map(|l| l.split_once(':'))
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.trim())
        };
        if h("host") != app.origin.strip_prefix("http://")
            || h("origin").is_some_and(|v| v != app.origin)
            || h("sec-fetch-site").is_some_and(|v| v != "same-origin" && v != "none")
        {
            return Err("Same-origin localhost access required".into());
        }
        if method == "GET" && !path.starts_with("/calibration/") {
            if path == "/actuator-motion-view.mjs" {
                return Ok((
                    "text/javascript".into(),
                    include_bytes!("../../../web/viewer/actuator-motion-view.mjs").to_vec(),
                ));
            }
            if path == "/calibration-mirror.mjs" {
                return Ok((
                    "text/javascript".into(),
                    include_bytes!("../../../web/viewer/calibration-mirror.mjs").to_vec(),
                ));
            }
            if path == "/calibration-ui.mjs" {
                return Ok(("text/javascript".into(), {
                    let source = include_str!("../../../web/viewer/calibration-ui.mjs");
                    if app.execution.is_virtual_calibration() {
                        let extra = format!(
                            "'X-Calibration-Server':{},'X-Calibration-Bench':{},'X-Calibration-Generation':'1',",
                            json!(app.execution.server_instance),
                            json!(app.execution.bench_instance)
                        );
                        source
                            .replace(
                                "headers:{'X-Control-Token'",
                                &format!("headers:{{{extra}'X-Control-Token'"),
                            )
                            .into_bytes()
                    } else {
                        source.as_bytes().to_vec()
                    }
                }));
            }
            let root = app.viewer.canonicalize().map_err(|e| e.to_string())?;
            let file = root
                .join(if path == "/" {
                    "index.html"
                } else {
                    path.trim_start_matches('/')
                })
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if !file.starts_with(&root) {
                return Err("Outside viewer".into());
            }
            let kind = match file.extension().and_then(|s| s.to_str()).unwrap_or("") {
                "html" => "text/html",
                "mjs" | "js" => "text/javascript",
                "wasm" => "application/wasm",
                "json" => "application/json",
                "css" => "text/css",
                _ => "application/octet-stream",
            };
            let mut bytes = fs::read(&file).map_err(|e| e.to_string())?;
            if path == "/" {
                bytes=String::from_utf8(bytes).map_err(|e|e.to_string())?.replace("</body>",&format!("<meta name=\"calibration-token\" content=\"{}\"><script type=\"module\" src=\"/calibration-ui.mjs\"></script></body>",app.token)).into_bytes()
            }
            return Ok((kind.into(), bytes));
        }
        if h("x-control-token") != Some(app.token.as_str()) {
            return Err("Session token required".into());
        }
        if method == "GET" && path == "/calibration/status" {
            return Ok((
                "application/json".into(),
                app.handle.status().to_string().into_bytes(),
            ));
        }
        if method == "GET" && path == "/calibration/gaits" {
            return Ok((
                "application/json".into(),
                serde_json::to_vec(&app.handle.gaits()?).unwrap(),
            ));
        }
        if method == "GET" && path == "/calibration/gait" {
            let query = first[1].split_once('?').map(|q| q.1).unwrap_or("");
            let rel = query.strip_prefix("path=").ok_or("gait path required")?;
            let rel = percent_decode(rel)?;
            return Ok((
                "application/json".into(),
                serde_json::to_vec(&app.handle.gait(&rel)?).unwrap(),
            ));
        }
        if method == "GET" && path == "/calibration/export" {
            return Ok((
                "application/json".into(),
                serde_json::to_vec_pretty(&app.handle.export()).unwrap(),
            ));
        }
        if method != "POST" || path != "/calibration/command" {
            return Err("Unknown endpoint".into());
        }
        let client = h("x-client-id").ok_or("Tab identity required")?;
        if client.len() != 36 || !client.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
            return Err("Invalid tab identity".into());
        }
        let length = h("content-length")
            .unwrap_or("0")
            .parse::<usize>()
            .map_err(|e| e.to_string())?;
        if length > 4096 || h("transfer-encoding").is_some() {
            return Err("Invalid body length".into());
        }
        while data.len() < end + length {
            let mut b = [0; 1024];
            let n = stream.read(&mut b).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("Incomplete body".into());
            }
            data.extend(&b[..n]);
        }
        let request: Value =
            serde_json::from_slice(&data[end..end + length]).map_err(|e| e.to_string())?;
        let execution = if request["action"] == "stop" {
            None
        } else if h("x-calibration-server").is_some()
            || h("x-calibration-bench").is_some()
            || h("x-calibration-generation").is_some()
        {
            let identity = ExecutionIdentity {
                schema_version: 1,
                kind: "virtual_calibration".into(),
                server_instance: h("x-calibration-server")
                    .ok_or(format!("{BINDING_REFUSED}: server identity required"))?
                    .into(),
                bench_instance: h("x-calibration-bench")
                    .ok_or(format!("{BINDING_REFUSED}: bench identity required"))?
                    .into(),
            };
            let generation = h("x-calibration-generation")
                .ok_or(format!("{BINDING_REFUSED}: connection generation required"))?
                .parse::<u64>()
                .map_err(|_| format!("{BINDING_REFUSED}: invalid connection generation"))?;
            Some((identity, generation))
        } else {
            None
        };
        let value = app.handle.command(client, request, execution)?;
        Ok(("application/json".into(), value.to_string().into_bytes()))
    })();
    match result {
        Ok((kind, bytes)) => reply(&mut stream, 200, &kind, &bytes),
        // Execution-binding refusals (identity, generation, lost virtual bench)
        // are 409 so clients revoke their pin; business refusals, including an
        // out-of-scope virtual command, stay 400.
        Err(e) => reply(
            &mut stream,
            if e.starts_with(BINDING_REFUSED) {
                BINDING_REFUSED_STATUS
            } else {
                400
            },
            "application/json",
            json!({"error":e}).to_string().as_bytes(),
        ),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("serve_actuator_calibration CONFIG HTTP_PORT; configure virtual_bench locally (obsolete --virtual-bench socket argument is refused)".into());
    }
    let cfg: Config = serde_json::from_slice(&fs::read(&args[1])?)?;
    let viewer = cfg.viewer.clone();
    let (control, worker) = Service::new(cfg)?;
    let execution = control.execution();
    let listener = TcpListener::bind(format!("127.0.0.1:{}", args[2]))?;
    let origin = format!("http://{}", listener.local_addr()?);
    let mut bytes = [0; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let token = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let app = Arc::new(Adapter {
        handle: control,
        execution,
        origin: origin.clone(),
        token,
        viewer,
    });
    std::thread::spawn(move || worker.run());
    println!("Calibration and robot viewer: {origin}");
    for stream in listener.incoming() {
        let app = app.clone();
        let stream = stream?;
        std::thread::spawn(move || handle(stream, app));
    }
    Ok(())
}
