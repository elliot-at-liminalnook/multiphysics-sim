//! The hardware servers' requests over the shared loopback transport
//! ([`crate::loopback_http`]): the control headers (`X-Control-Token`,
//! `X-Client-Id`, `Content-Type`), the servers' body limits, and the
//! process's client identity.
use super::{Body, CALIBRATION_MAX_BODY, Client, ClientError, MOTOR_BENCH_MAX_BODY, REQUEST_TIMEOUT, ServerKind};
use crate::loopback_http::{self, Request, decode, json};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::io::Read;
use std::time::Duration;

/// Why a hardware server may close a connection without a usable answer:
/// appended to the transport's "closed the connection" error.
const CLOSED_HINT: &str = "; if it was restarted its control token changed: reconnect to read the new one";

impl Client {
    /// A client with the ordinary request timeout ([`REQUEST_TIMEOUT`]) and
    /// the calibration server's body limit ([`CALIBRATION_MAX_BODY`]); use
    /// [`Client::for_kind`] for the motor bench.
    pub fn new(endpoint: super::Endpoint, token: String, client_id: String) -> Client {
        Client { endpoint, token, client_id, timeout: REQUEST_TIMEOUT, max_body: CALIBRATION_MAX_BODY, calibration_execution: None }
    }
    /// The same client with the request body limit of `kind`'s server
    /// ([`CALIBRATION_MAX_BODY`] or [`MOTOR_BENCH_MAX_BODY`]).
    pub fn for_kind(mut self, kind: ServerKind) -> Self {
        self.max_body = match kind {
            ServerKind::Calibration => CALIBRATION_MAX_BODY,
            ServerKind::MotorBench => MOTOR_BENCH_MAX_BODY,
        };
        self
    }
    pub fn calibration_execution(&self) -> Option<(&super::calibration::ExecutionIdentity, u64)> {
        self.calibration_execution.as_ref().map(|(identity, generation)| (identity, *generation))
    }
    /// Pin this connection's verified virtual execution; never reuse on reconnect.
    pub fn with_calibration_execution(mut self, identity: super::calibration::ExecutionIdentity, generation: u64) -> Self {
        self.calibration_execution = Some((identity, generation));
        self
    }
    /// The same client with another read/write timeout (e.g.
    /// [`super::STOP_TIMEOUT`]), at least 1 ms (a zero timeout is an error
    /// for `set_read_timeout` and would make every request fail).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout.max(Duration::from_millis(1));
        self
    }
    /// `GET path` with the control headers; the answer as JSON.
    pub fn get(&self, path: &str) -> Result<Value, ClientError> {
        let body = self.exchange("GET", path, None, true)?;
        json(path, &body)
    }
    /// `POST path` with `body`; the answer as JSON. A body larger than the
    /// server's limit ([`Client::max_body`]) is refused before connecting.
    pub fn post(&self, path: &str, body: &Body) -> Result<Value, ClientError> {
        let text = self.checked_body(body)?;
        let answer = self.exchange("POST", path, Some(&text), true)?;
        json(path, &answer)
    }
    /// [`Client::get`], decoded as `T`.
    pub fn get_as<T: DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        decode(path, self.get(path)?)
    }
    /// [`Client::post`], decoded as `T`.
    pub fn post_as<T: DeserializeOwned>(&self, path: &str, body: &Body) -> Result<T, ClientError> {
        decode(path, self.post(path, body)?)
    }
    /// `GET path` as a browser loads a page (no token or client id): the
    /// body text. Used to read the token the server injects into its page.
    pub fn page(&self, path: &str) -> Result<String, ClientError> {
        self.exchange("GET", path, None, false)
    }
    /// `POST path` with `body` without waiting for the answer: the page's
    /// `keepalive` fetch on `pagehide`, for when the window is closing.
    /// Connects (within [`super::CONNECT_TIMEOUT`]), writes the whole
    /// request with the same headers as [`Client::post`], flushes, shuts
    /// down the write half and returns; the answer is never read
    /// ([`loopback_http::send_only`]). The servers act on a request once
    /// they have parsed it (the calibration server latches its stop flags
    /// then, before queueing the job), so a STOP sent this way takes effect
    /// even though no one reads the reply. `Ok` means only that the request
    /// was written, not that the server accepted it.
    pub fn send_only(&self, path: &str, body: &Body) -> Result<(), ClientError> {
        let text = self.checked_body(body)?;
        let generation = self.calibration_execution.as_ref().map(|(_, g)| g.to_string());
        let headers = self.headers(true, path, Some(&text), generation.as_deref())?;
        loopback_http::send_only(&self.endpoint, self.timeout, &Request { method: "POST", path, headers: &headers, body: Some(&text), closed_hint: CLOSED_HINT })
    }

    /// The body's bytes, or a refusal when they exceed [`Client::max_body`]
    /// (the server would refuse it without acting).
    fn checked_body(&self, body: &Body) -> Result<String, ClientError> {
        let text = body.text();
        if text.len() > self.max_body {
            return Err(ClientError::Transport(format!("request body of {} bytes exceeds the server's {}-byte limit", text.len(), self.max_body)));
        }
        Ok(text)
    }

    /// The headers after `Host`: for a control request the token, client id
    /// and `Content-Type` (in the pages' order), checked for control
    /// characters before connecting; none for a page load.
    fn headers<'a>(&'a self, control: bool, path: &str, body: Option<&str>, generation: Option<&'a str>) -> Result<Vec<(&'static str, &str)>, ClientError> {
        if !control {
            return Ok(Vec::new());
        }
        if [&self.token, &self.client_id].iter().any(|v| v.bytes().any(|b| b < b' ' || b == 0x7f)) {
            return Err(ClientError::Transport("control token or client id contains a control character".into()));
        }
        let mut headers = vec![("X-Control-Token", self.token.as_str()), ("X-Client-Id", self.client_id.as_str()), ("Content-Type", "application/json")];
        let stop = body.and_then(|b| serde_json::from_str::<Value>(b).ok())
            .is_some_and(|b| b["action"] == "stop");
        if path == super::calibration::COMMAND && !stop {
            if let Some((identity, _)) = &self.calibration_execution {
                if !identity.is_virtual_calibration() || generation == Some("0") {
                    return Err(ClientError::Transport("Invalid virtual execution identity or generation".into()));
                }
                headers.push(("X-Calibration-Server", &identity.server_instance));
                headers.push(("X-Calibration-Bench", &identity.bench_instance));
                headers.push(("X-Calibration-Generation", generation.unwrap_or("0")));
            }
        }
        Ok(headers)
    }

    /// One request on its own connection ([`loopback_http::exchange`]); the
    /// 2xx body, or the server's error. A server that refuses a request
    /// (a stale token after a restart, a bad client id or length) answers
    /// and closes without reading its body; the transport salvages that
    /// answer so its `error` (e.g. "Session token required") surfaces, else
    /// says the server closed the connection and its token may have changed.
    fn exchange(&self, method: &str, path: &str, body: Option<&str>, control: bool) -> Result<String, ClientError> {
        let generation = self.calibration_execution.as_ref().map(|(_, g)| g.to_string());
        let headers = self.headers(control, path, body, generation.as_deref())?;
        loopback_http::exchange(&self.endpoint, self.timeout, &Request { method, path, headers: &headers, body, closed_hint: CLOSED_HINT })
    }
}

/// This process's `X-Client-Id`: one [`new_client_id`], made on first use
/// and returned on every later call, so every connect and reconnect of the
/// viewer presents the same identity, as the browser page keeps one
/// `crypto.randomUUID()` per page load (the servers tie leases and command
/// ordering to it).
pub fn process_client_id() -> String {
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ID.get_or_init(new_client_id).clone()
}

/// A random (version 4) UUID for `X-Client-Id`, as `crypto.randomUUID()`:
/// 36 characters, lowercase hex and dashes (what both servers accept). From
/// `/dev/urandom`; where that is unavailable, from a per-process random hash
/// of the time, process id and a counter (unique, not secret; the id only
/// tells one panel's leases from another's). Never panics.
pub fn new_client_id() -> String {
    let mut bytes = [0u8; 16];
    if !urandom(&mut bytes) {
        fallback_bytes(&mut bytes);
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

fn urandom(bytes: &mut [u8; 16]) -> bool {
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(bytes)).is_ok()
}

fn fallback_bytes(bytes: &mut [u8; 16]) {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let state = std::collections::hash_map::RandomState::new();
    for (half, out) in bytes.chunks_mut(8).enumerate() {
        let mut h = state.build_hasher();
        h.write_u64(count);
        h.write_usize(half);
        h.write_usize(out.as_ptr() as usize);
        // Clocks and process ids are unsupported (and panic) on wasm32.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            h.write_u128(nanos);
            h.write_u32(std::process::id());
        }
        out.copy_from_slice(&h.finish().to_le_bytes());
    }
}
