//! Loopback addresses and one-request HTTP/1.1 exchanges over
//! `std::net::TcpStream`: connect, read and write timeouts, an overall
//! deadline, a response size cap, and the servers' `error` field surfaced on
//! a non-2xx answer.
use super::{Body, CALIBRATION_MAX_BODY, CONNECT_TIMEOUT, Client, ClientError, Endpoint, MOTOR_BENCH_MAX_BODY, REQUEST_TIMEOUT, ServerKind};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// Largest response body accepted (the status carries ≤300 sweep samples;
/// a compiled gait is the largest answer).
const MAX_BODY: usize = 64 << 20;
/// Largest status line plus headers accepted.
const MAX_HEAD: usize = 64 << 10;
/// Why a host is refused: the servers bind and check 127.0.0.1 only.
const ONLY_IPV4_LOOPBACK: &str = "the hardware servers listen on 127.0.0.1 only (use http://127.0.0.1:PORT or http://localhost:PORT)";

impl Endpoint {
    /// `http://127.0.0.1:PORT` or `http://localhost:PORT` (as 127.0.0.1),
    /// with an optional trailing `/`. Both hardware servers bind 127.0.0.1
    /// only and require `Host: 127.0.0.1:PORT`, so `[::1]`, other 127.x
    /// addresses, any other host, scheme, path, query or a missing port is
    /// refused, naming the URL.
    pub fn parse(url: &str) -> Result<Endpoint, ClientError> {
        let refuse = |why: &str| ClientError::NotLoopback(format!("{url}: {why}"));
        let rest = url.trim().strip_prefix("http://").ok_or_else(|| refuse("only http:// to a loopback server is accepted"))?;
        let authority = rest.strip_suffix('/').unwrap_or(rest);
        if authority.contains(|c: char| matches!(c, '/' | '?' | '#' | '@')) {
            return Err(refuse("only the server's root is accepted (no path, query or user)"));
        }
        if authority.starts_with('[') {
            return Err(refuse(ONLY_IPV4_LOOPBACK));
        }
        let (host, port) = authority.rsplit_once(':').ok_or_else(|| refuse("missing port"))?;
        if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
            return Err(refuse("missing or invalid port"));
        }
        let port: u16 = port.parse().map_err(|_| refuse("port out of range"))?;
        if port == 0 {
            return Err(refuse("port 0"));
        }
        if host != "127.0.0.1" && !host.eq_ignore_ascii_case("localhost") {
            return Err(refuse(ONLY_IPV4_LOOPBACK));
        }
        Ok(Endpoint { ip: IpAddr::V4(Ipv4Addr::LOCALHOST), port })
    }
    /// An endpoint from an address: only 127.0.0.1 (the address both
    /// servers bind), with a nonzero port.
    pub fn loopback(ip: IpAddr, port: u16) -> Result<Endpoint, ClientError> {
        if ip != IpAddr::V4(Ipv4Addr::LOCALHOST) || port == 0 {
            return Err(ClientError::NotLoopback(format!("{}: {ONLY_IPV4_LOOPBACK}", SocketAddr::new(ip, port))));
        }
        Ok(Endpoint { ip, port })
    }
    /// The `Host` header: `127.0.0.1:4194` (the servers compare it to their
    /// origin without `http://`).
    pub fn host(&self) -> String {
        SocketAddr::new(self.ip, self.port).to_string()
    }
    /// `http://` + [`Endpoint::host`].
    pub fn origin(&self) -> String {
        format!("http://{}", self.host())
    }
}

impl Client {
    /// A client with the ordinary request timeout ([`REQUEST_TIMEOUT`]) and
    /// the calibration server's body limit ([`CALIBRATION_MAX_BODY`]); use
    /// [`Client::for_kind`] for the motor bench.
    pub fn new(endpoint: Endpoint, token: String, client_id: String) -> Client {
        Client { endpoint, token, client_id, timeout: REQUEST_TIMEOUT, max_body: CALIBRATION_MAX_BODY }
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
    /// Connects (within [`CONNECT_TIMEOUT`]), writes the whole request with
    /// the same headers as [`Client::post`], flushes, shuts down the write
    /// half and returns; the answer is never read. The servers act on a
    /// request once they have parsed it (the calibration server latches its
    /// stop flags then, before queueing the job), so a STOP sent this way
    /// takes effect even though no one reads the reply. `Ok` means only that
    /// the request was written, not that the server accepted it.
    pub fn send_only(&self, path: &str, body: &Body) -> Result<(), ClientError> {
        let text = self.checked_body(body)?;
        let (stream, fail, written) = self.send("POST", path, Some(&text), true)?;
        written.map_err(|e| fail("write", e))?;
        // The request is complete (the servers read by `Content-Length`); the
        // FIN tells the server nothing more follows. A server that already
        // answered and closed may make this fail (ENOTCONN on macOS) after
        // the request was delivered, so it is not an error. The stream then
        // drops.
        let _ = stream.shutdown(Shutdown::Write);
        Ok(())
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

    /// Checks the request, connects and writes it in full; the stream, the
    /// error builder for this request and how the write went (a server that
    /// refuses a request answers and closes without reading its body, which
    /// may fail the write after the answer has arrived: see
    /// [`Client::exchange`]).
    #[allow(clippy::type_complexity)]
    fn send(&self, method: &str, path: &str, body: Option<&str>, control: bool) -> Result<(TcpStream, impl Fn(&str, std::io::Error) -> ClientError, std::io::Result<()>), ClientError> {
        // The fields are public: check again rather than trust construction.
        if self.endpoint.ip != IpAddr::V4(Ipv4Addr::LOCALHOST) {
            return Err(ClientError::NotLoopback(format!("{}: {ONLY_IPV4_LOOPBACK}", self.endpoint.origin())));
        }
        if !path.starts_with('/') || path.bytes().any(|b| b <= b' ' || b == 0x7f) {
            return Err(ClientError::Transport(format!("invalid request path {path:?}")));
        }
        if control && [&self.token, &self.client_id].iter().any(|v| v.bytes().any(|b| b < b' ' || b == 0x7f)) {
            return Err(ClientError::Transport("control token or client id contains a control character".into()));
        }
        let origin = self.endpoint.origin();
        let method_owned = method.to_string();
        let path_owned = path.to_string();
        let fail = move |what: &str, e: std::io::Error| {
            let e = if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) { "timed out".to_string() } else { e.to_string() };
            ClientError::Transport(format!("{method_owned} {origin}{path_owned}: {what}: {e}"))
        };
        let addr = SocketAddr::new(self.endpoint.ip, self.endpoint.port);
        let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| fail("connect", e))?;
        stream.set_write_timeout(Some(self.timeout)).map_err(|e| fail("set timeout", e))?;
        stream.set_read_timeout(Some(self.timeout)).map_err(|e| fail("set timeout", e))?;
        let _ = stream.set_nodelay(true);

        let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {}\r\n", self.endpoint.host());
        if control {
            request.push_str(&format!("X-Control-Token: {}\r\nX-Client-Id: {}\r\nContent-Type: application/json\r\n", self.token, self.client_id));
        }
        if let Some(body) = body {
            request.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        request.push_str("Connection: close\r\n\r\n");
        if let Some(body) = body {
            request.push_str(body);
        }
        let written = stream.write_all(request.as_bytes()).and_then(|()| stream.flush());
        Ok((stream, fail, written))
    }

    /// One request on its own connection; the 2xx body, or the server's error.
    ///
    /// Both servers read a request's head and, when they refuse it (a stale
    /// token after a restart, a bad client id or length), answer and close
    /// without reading its body. With body bytes left unread the server's
    /// kernel resets the connection, so our write or read may fail with
    /// `ConnectionReset`/`BrokenPipe`/`ConnectionAborted` after the answer
    /// arrived. Then the answer received so far is used when it is whole
    /// ([`salvage`]), so the server's `error` (e.g. "Session token required")
    /// surfaces; else the error says the server closed the connection and its
    /// token may have changed.
    fn exchange(&self, method: &str, path: &str, body: Option<&str>, control: bool) -> Result<String, ClientError> {
        let origin = self.endpoint.origin();
        let (mut stream, fail, written) = self.send(method, path, body, control)?;
        let closed = |e: std::io::Error| {
            ClientError::Transport(format!(
                "{method} {origin}{path}: the server closed the connection without a usable answer ({e}); \
                 if it was restarted its control token changed: reconnect to read the new one"
            ))
        };
        let reset = match written {
            Ok(()) => None,
            Err(e) if is_closed(&e) => Some(e),
            Err(e) => return Err(fail("write", e)),
        };
        let read = read_response(&mut stream, Instant::now() + self.timeout);
        let (status, text) = match (read, reset) {
            (Ok(answer), _) => answer,
            (Err(ReadError::Io(_, e, data)), _) if is_closed(&e) => salvage(&data).ok_or_else(|| closed(e))?,
            // The write already failed with a reset: that is the cause.
            (Err(ReadError::Io(_, _, data)), Some(e)) => salvage(&data).ok_or_else(|| closed(e))?,
            (Err(ReadError::Io(what, e, _)), None) => return Err(fail(what, e)),
            (Err(ReadError::Malformed(why)), Some(e)) if why == NO_ANSWER => return Err(closed(e)),
            (Err(ReadError::Malformed(why)), _) => return Err(ClientError::Decode(format!("{method} {origin}{path}: {why}"))),
        };
        if (200..300).contains(&status) {
            return Ok(text);
        }
        let error = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| format!("Request failed (HTTP {status})"));
        Err(ClientError::Server { status, error })
    }
}

enum ReadError {
    /// What failed, why, and the bytes received before it.
    Io(&'static str, std::io::Error, Vec<u8>),
    Malformed(String),
}

/// Why a response could not be read when nothing arrived.
const NO_ANSWER: &str = "the server closed the connection without answering";

/// Whether an I/O error means the peer closed or reset the connection.
fn is_closed(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::ConnectionReset | ErrorKind::BrokenPipe | ErrorKind::ConnectionAborted)
}

/// A response from the bytes received before the connection was reset: a
/// complete head and its `Content-Length` body, or a complete head and
/// whatever body arrived when that parses as JSON (a server error without a
/// length). `None` when nothing usable arrived.
fn salvage(data: &[u8]) -> Option<(u16, String)> {
    let p = data.windows(4).position(|w| w == b"\r\n\r\n")?;
    let (status, length) = parse_head(std::str::from_utf8(&data[..p]).ok()?).ok()?;
    let body = &data[p + 4..];
    let body = match length {
        Some(n) if body.len() >= n => &body[..n],
        _ if serde_json::from_slice::<Value>(body).is_ok() => body,
        _ => return None,
    };
    Some((status, String::from_utf8(body.to_vec()).ok()?))
}

/// Reads one response: the status code and the body as UTF-8. Stops at
/// `Content-Length` when given (the servers send it), else at EOF; never
/// past `deadline` or [`MAX_BODY`].
fn read_response(stream: &mut TcpStream, deadline: Instant) -> Result<(u16, String), ReadError> {
    let malformed = |why: &str| ReadError::Malformed(why.to_string());
    let mut data: Vec<u8> = Vec::new();
    let mut head: Option<(usize, u16, Option<usize>)> = None;
    let mut chunk = [0u8; 16 << 10];
    let mut eof = false;
    loop {
        if head.is_none() {
            if let Some(p) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                let text = std::str::from_utf8(&data[..p]).map_err(|_| malformed("response headers are not UTF-8"))?;
                let (status, length) = parse_head(text).map_err(ReadError::Malformed)?;
                if length.is_some_and(|n| n > MAX_BODY) {
                    return Err(malformed("response body larger than 64 MiB"));
                }
                head = Some((p + 4, status, length));
            } else if data.len() > MAX_HEAD {
                return Err(malformed("response headers too large"));
            }
        }
        if let Some((start, _, Some(length))) = head {
            if data.len() >= start + length {
                data.truncate(start + length);
                break;
            }
        }
        if let Some((start, _, _)) = head {
            if data.len() - start > MAX_BODY {
                return Err(malformed("response body larger than 64 MiB"));
            }
        }
        if eof {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ReadError::Io("read", std::io::Error::from(ErrorKind::TimedOut), data));
        }
        // macOS refuses to set a timeout (EINVAL) on a socket the peer has
        // already reset; the answer it sent may still be buffered, so read
        // on under the previous timeout (at most the client's); the loop
        // still checks the deadline between reads.
        let _ = stream.set_read_timeout(Some(remaining));
        match stream.read(&mut chunk) {
            Ok(0) => eof = true,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(ReadError::Io("read", e, data)),
        }
    }
    let Some((start, status, length)) = head else {
        return Err(malformed(if data.is_empty() { NO_ANSWER } else { "incomplete response headers" }));
    };
    if let Some(length) = length {
        if data.len() < start + length {
            return Err(malformed("response ended before its Content-Length"));
        }
    }
    let text = String::from_utf8(data.split_off(start)).map_err(|_| malformed("response body is not UTF-8"))?;
    Ok((status, text))
}

/// The status code and `Content-Length` of a response head.
fn parse_head(head: &str) -> Result<(u16, Option<usize>), String> {
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.split_whitespace();
    let version = parts.next().unwrap_or("");
    let status = parts.next().and_then(|s| s.parse::<u16>().ok());
    let status = match status {
        Some(s) if version.starts_with("HTTP/1.") && (100..600).contains(&s) => s,
        _ => return Err(format!("not an HTTP response: {status_line:?}")),
    };
    let mut length = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("content-length") {
            let n = value.parse::<usize>().map_err(|_| format!("invalid Content-Length {value:?}"))?;
            if length.is_some_and(|m| m != n) {
                return Err("conflicting Content-Length headers".into());
            }
            length = Some(n);
        } else if name.eq_ignore_ascii_case("transfer-encoding") && !value.eq_ignore_ascii_case("identity") {
            return Err(format!("unsupported Transfer-Encoding {value:?}"));
        }
    }
    Ok((status, length))
}

fn json(path: &str, text: &str) -> Result<Value, ClientError> {
    serde_json::from_str(text).map_err(|e| ClientError::Decode(format!("{path}: the answer is not JSON: {e}")))
}

fn decode<T: DeserializeOwned>(path: &str, value: Value) -> Result<T, ClientError> {
    serde_json::from_value(value).map_err(|e| ClientError::Decode(format!("{path}: unexpected answer: {e}")))
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
