//! Small native automation client. No renderer, shell invocation or HTTP library.
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::{Duration, Instant},
};
/// Only the local services are supported; redirects and remote hosts are rejected.
fn request_bytes(
    base: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Vec<u8>, String> {
    let authority = base
        .strip_prefix("http://")
        .ok_or("expected http://127.0.0.1:PORT")?
        .trim_end_matches('/');
    let address: SocketAddr = authority
        .parse()
        .map_err(|_| "expected numeric loopback address and port")?;
    if !address.ip().is_loopback() {
        return Err("only loopback services are supported".into());
    }
    if !["GET", "POST", "DELETE"].contains(&method)
        || !path.starts_with('/')
        || path.contains(['\r', '\n', ' '])
    {
        return Err("invalid method or path".into());
    }
    let mut stream =
        TcpStream::connect_timeout(&address, Duration::from_secs(3)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let bytes = body
        .map(serde_json::to_vec)
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    write!(stream,"{method} {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len()).map_err(|e|e.to_string())?;
    stream.write_all(&bytes).map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    stream
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut response)
        .map_err(|e| e.to_string())?;
    if response.len() > 64 * 1024 * 1024 {
        return Err("response exceeds 64 MiB client limit".into());
    }
    let end = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("missing HTTP response header")?;
    let header = std::str::from_utf8(&response[..end]).map_err(|e| e.to_string())?;
    let status = header
        .split_whitespace()
        .nth(1)
        .ok_or("missing HTTP status")?;
    let length = header
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                .map(|(_, v)| v.trim().parse::<usize>())
        })
        .ok_or("missing content length")?
        .map_err(|e| e.to_string())?;
    if response.len() - end - 4 != length {
        return Err(format!(
            "truncated HTTP response for {path}: expected {length} bytes, received {}",
            response.len() - end - 4
        ));
    }
    if status != "200" && status != "202" {
        return Err(format!(
            "HTTP {status}: {}",
            String::from_utf8_lossy(&response[end + 4..])
        ));
    }
    Ok(response[end + 4..].to_vec())
}
pub fn request(
    base: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, String> {
    serde_json::from_slice(&request_bytes(base, method, path, body)?).map_err(|e| e.to_string())
}
/// Retrieve an image from a returned artifact URL without opening a desktop window.
pub fn image(base: &str, path: &str) -> Result<Vec<u8>, String> {
    let bytes = request_bytes(base, "GET", path, None)?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("response is not a PNG".into());
    }
    Ok(bytes)
}
/// A failed batch is returned intact so callers can inspect every applied step.
pub fn batch(base: &str, batch: &Value, timeout: Duration) -> Result<Value, String> {
    let accepted = request(base, "POST", "/v1/batch", Some(batch))?;
    let path = accepted["url"].as_str().ok_or("missing job URL")?;
    let start = Instant::now();
    loop {
        let job = request(base, "GET", path, None)?;
        match job["status"].as_str() {
            Some("succeeded" | "failed" | "cancelled") => return Ok(job),
            _ => {}
        }
        if start.elapsed() > timeout {
            return Err(format!(
                "Timed out waiting; job remains inspectable at {base}{path}. It was not resubmitted or silently cancelled."
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
/// Execute batches across viewers in a single tool invocation. Ordering is
/// explicit; no cross-process atomicity or implicit retry of mutating requests.
pub fn plan(plan: &Value, timeout: Duration) -> Result<Value, String> {
    let entries = plan["batches"]
        .as_array()
        .ok_or("plan needs a batches array")?;
    if entries.is_empty() || entries.len() > 128 {
        return Err("plan requires 1 to 128 batches".into());
    }
    let mut results = Vec::new();
    for entry in entries {
        let url = entry["url"].as_str().ok_or("each batch needs a url")?;
        let result = batch(
            url,
            &json!({"commands":entry["commands"],"stop_on_error":entry.get("stop_on_error").cloned().unwrap_or(json!(true))}),
            timeout,
        );
        let failed = result.as_ref().map_or(true, |j| j["status"] != "succeeded");
        results.push(match result {
            Ok(job) => json!({"url":url,"job":job}),
            Err(error) => json!({"url":url,"error":error}),
        });
        if failed
            && plan
                .get("stop_on_error")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        {
            break;
        }
    }
    Ok(json!({"results":results}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    #[test]
    fn one_client_plan_executes_ordered_batches_and_returns_failures() {
        let mut server = crate::Server::bind(0, "client-test", vec![]).unwrap();
        let url = format!("http://{}", server.address);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = std::thread::spawn(move || {
            let mut count = 0;
            while !stopping.load(Ordering::Relaxed) {
                server.poll(|command, _, _| match command.command.as_str() {
                    "increment" => {
                        count += 1;
                        Ok(json!(count)).into()
                    }
                    "read" => Ok(json!(count)).into(),
                    _ => Err("unknown command".into()).into(),
                });
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        let result=plan(&json!({"batches":[{"url":url,"commands":[{"command":"increment"},{"command":"read"}]},{"url":url,"commands":[{"command":"fail"},{"command":"increment"}]}]}),Duration::from_secs(3)).unwrap();
        stop.store(true, Ordering::Relaxed);
        worker.join().unwrap();
        assert_eq!(result["results"][0]["job"]["results"][1]["value"], 1);
        assert_eq!(result["results"][1]["job"]["status"], "failed");
        assert_eq!(result["results"][1]["job"]["completed"], 1);
    }
}
