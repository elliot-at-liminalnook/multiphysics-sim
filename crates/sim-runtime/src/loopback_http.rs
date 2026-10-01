//! Loopback addresses and one-request HTTP/1.1 exchanges over
//! `std::net::TcpStream`, shared by the clients of the local servers (the
//! hardware servers, [`crate::hardware_client`]; RoboCAD's REST API,
//! [`crate::cad_client`]): connect, read and write timeouts, an overall
//! deadline, a response size cap, and the server's `error` field surfaced on
//! a non-2xx answer.
//!
//! One request per connection (`Connection: close`). The request is
//! `{METHOD} {path} HTTP/1.1`, `Host: 127.0.0.1:PORT`, the caller's headers
//! in order, `Content-Length` when there is a body, `Connection: close`,
//! then the body. Only `127.0.0.1` is ever connected: [`Endpoint::parse`]
//! and [`Endpoint::loopback`] refuse anything else, and the address is
//! checked again before every connection.
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// Connect timeout for every request.
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
/// Largest response body accepted (the hardware status carries ≤300 sweep
/// samples; a compiled gait or a CAD mesh is the largest answer).
const MAX_BODY: usize = 64 << 20;
/// Largest status line plus headers accepted.
const MAX_HEAD: usize = 64 << 10;
/// Why a host is refused: the local servers bind (and the hardware servers
/// check) 127.0.0.1 only.
const ONLY_IPV4_LOOPBACK: &str = "the local servers listen on 127.0.0.1 only (use http://127.0.0.1:PORT or http://localhost:PORT)";

/// A loopback server address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// The host to connect to: always `127.0.0.1` (checked again before
    /// every connection).
    pub ip: IpAddr,
    pub port: u16,
}

/// Why a request failed.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// Not a loopback address (nothing was sent).
    NotLoopback(String),
    /// Connecting, writing or reading failed (includes timeouts).
    Transport(String),
    /// The server answered with an error status: its `error` field, or
    /// "Request failed (HTTP {status})" when the answer has none.
    Server { status: u16, error: String },
    /// The answer was not the JSON expected.
    Decode(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotLoopback(e) | Error::Transport(e) | Error::Decode(e) => f.write_str(e),
            Error::Server { error, .. } => f.write_str(error),
        }
    }
}
impl std::error::Error for Error {}

/// One request: what goes on the wire after the request line's
/// `HTTP/1.1` and the `Host` header, and how a closed connection is
/// explained.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    /// `GET`, `POST`, `PUT`, `PATCH` or `DELETE` (uppercase ASCII letters).
    pub method: &'a str,
    /// Starts with `/`; no bytes ≤ `' '` or `0x7f` (percent-encode them).
    pub path: &'a str,
    /// Written after `Host`, in order. A name or value with a control
    /// character is refused before connecting.
    pub headers: &'a [(&'a str, &'a str)],
    /// The body; `Content-Length` is added when `Some`.
    pub body: Option<&'a str>,
    /// Appended verbatim to "… the server closed the connection without a
    /// usable answer ({e})" (e.g. why the hardware server may have closed).
    pub closed_hint: &'a str,
}

impl Endpoint {
    /// `http://127.0.0.1:PORT` or `http://localhost:PORT` (as 127.0.0.1),
    /// with an optional trailing `/`. The local servers bind 127.0.0.1 only
    /// (the hardware servers also require `Host: 127.0.0.1:PORT`), so
    /// `[::1]`, other 127.x addresses, any other host, scheme, path, query
    /// or a missing port is refused, naming the URL.
    pub fn parse(url: &str) -> Result<Endpoint, Error> {
        let refuse = |why: &str| Error::NotLoopback(format!("{url}: {why}"));
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
    /// An endpoint from an address: only 127.0.0.1 (the address the local
    /// servers bind), with a nonzero port.
    pub fn loopback(ip: IpAddr, port: u16) -> Result<Endpoint, Error> {
        if ip != IpAddr::V4(Ipv4Addr::LOCALHOST) || port == 0 {
            return Err(Error::NotLoopback(format!("{}: {ONLY_IPV4_LOOPBACK}", SocketAddr::new(ip, port))));
        }
        Ok(Endpoint { ip, port })
    }
    /// The `Host` header: `127.0.0.1:4194` (the hardware servers compare it
    /// to their origin without `http://`).
    pub fn host(&self) -> String {
        SocketAddr::new(self.ip, self.port).to_string()
    }
    /// `http://` + [`Endpoint::host`].
    pub fn origin(&self) -> String {
        format!("http://{}", self.host())
    }
}

/// One request on its own connection; the 2xx body, or the server's error
/// ([`Error::Server`] with its JSON `error` field, else "Request failed
/// (HTTP {status})").
///
/// A server may read a request's head and, when it refuses it (the
/// hardware servers on a stale token after a restart, a bad client id or
/// length), answer and close without reading its body. With body bytes left
/// unread the server's kernel resets the connection, so our write or read
/// may fail with `ConnectionReset`/`BrokenPipe`/`ConnectionAborted` after
/// the answer arrived. Then the answer received so far is used when it is
/// whole (`salvage`), so the server's `error` surfaces; else the error
/// says the server closed the connection, followed by
/// [`Request::closed_hint`].
pub fn exchange(endpoint: &Endpoint, timeout: Duration, request: &Request) -> Result<String, Error> {
    let Request { method, path, closed_hint, .. } = *request;
    let origin = endpoint.origin();
    let (mut stream, fail, written) = send(endpoint, timeout, request)?;
    let closed = |e: std::io::Error| Error::Transport(format!("{method} {origin}{path}: the server closed the connection without a usable answer ({e}){closed_hint}"));
    let reset = match written {
        Ok(()) => None,
        Err(e) if is_closed(&e) => Some(e),
        Err(e) => return Err(fail("write", e)),
    };
    let read = read_response(&mut stream, Instant::now() + timeout);
    let (status, text) = match (read, reset) {
        (Ok(answer), _) => answer,
        (Err(ReadError::Io(_, e, data)), _) if is_closed(&e) => salvage(&data).ok_or_else(|| closed(e))?,
        // The write already failed with a reset: that is the cause.
        (Err(ReadError::Io(_, _, data)), Some(e)) => salvage(&data).ok_or_else(|| closed(e))?,
        (Err(ReadError::Io(what, e, _)), None) => return Err(fail(what, e)),
        (Err(ReadError::Malformed(why)), Some(e)) if why == NO_ANSWER => return Err(closed(e)),
        (Err(ReadError::Malformed(why)), _) => return Err(Error::Decode(format!("{method} {origin}{path}: {why}"))),
    };
    if (200..300).contains(&status) {
        return Ok(text);
    }
    let error = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| format!("Request failed (HTTP {status})"));
    Err(Error::Server { status, error })
}

/// Sends `request` without waiting for the answer (a page's `keepalive`
/// fetch on `pagehide`, for when the window is closing). Connects (within
/// [`CONNECT_TIMEOUT`]), writes the whole request, flushes, shuts down the
/// write half and returns; the answer is never read. `Ok` means only that
/// the request was written, not that the server accepted it.
pub fn send_only(endpoint: &Endpoint, timeout: Duration, request: &Request) -> Result<(), Error> {
    let (stream, fail, written) = send(endpoint, timeout, request)?;
    written.map_err(|e| fail("write", e))?;
    // The request is complete (the servers read by `Content-Length`); the
    // FIN tells the server nothing more follows. A server that already
    // answered and closed may make this fail (ENOTCONN on macOS) after
    // the request was delivered, so it is not an error. The stream then
    // drops.
    let _ = stream.shutdown(Shutdown::Write);
    Ok(())
}

/// The answer text as JSON; the error names `path`.
pub fn json(path: &str, text: &str) -> Result<Value, Error> {
    serde_json::from_str(text).map_err(|e| Error::Decode(format!("{path}: the answer is not JSON: {e}")))
}

/// A JSON answer as `T`; the error names `path`.
pub fn decode<T: DeserializeOwned>(path: &str, value: Value) -> Result<T, Error> {
    serde_json::from_value(value).map_err(|e| Error::Decode(format!("{path}: unexpected answer: {e}")))
}

/// Whether `s` holds a byte that may not appear in a request line or header
/// (a control character or DEL).
fn has_control(s: &str) -> bool {
    s.bytes().any(|b| b < b' ' || b == 0x7f)
}

/// Checks the request, connects and writes it in full; the stream, the
/// error builder for this request and how the write went (a server that
/// refuses a request answers and closes without reading its body, which
/// may fail the write after the answer has arrived: see [`exchange`]).
#[allow(clippy::type_complexity)]
fn send(endpoint: &Endpoint, timeout: Duration, request: &Request) -> Result<(TcpStream, impl Fn(&str, std::io::Error) -> Error, std::io::Result<()>), Error> {
    let Request { method, path, headers, body, .. } = *request;
    // The fields are public: check again rather than trust construction.
    if endpoint.ip != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err(Error::NotLoopback(format!("{}: {ONLY_IPV4_LOOPBACK}", endpoint.origin())));
    }
    if !path.starts_with('/') || path.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return Err(Error::Transport(format!("invalid request path {path:?}")));
    }
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(Error::Transport(format!("invalid request method {method:?}")));
    }
    for (name, value) in headers {
        if name.is_empty() || name.bytes().any(|b| b <= b' ' || b == 0x7f || b == b':') {
            return Err(Error::Transport(format!("invalid request header name {name:?}")));
        }
        if has_control(value) {
            return Err(Error::Transport(format!("request header {name} contains a control character")));
        }
    }
    let origin = endpoint.origin();
    let method_owned = method.to_string();
    let path_owned = path.to_string();
    let fail = move |what: &str, e: std::io::Error| {
        let e = if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) { "timed out".to_string() } else { e.to_string() };
        Error::Transport(format!("{method_owned} {origin}{path_owned}: {what}: {e}"))
    };
    let addr = SocketAddr::new(endpoint.ip, endpoint.port);
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| fail("connect", e))?;
    stream.set_write_timeout(Some(timeout)).map_err(|e| fail("set timeout", e))?;
    stream.set_read_timeout(Some(timeout)).map_err(|e| fail("set timeout", e))?;
    let _ = stream.set_nodelay(true);

    let mut text = format!("{method} {path} HTTP/1.1\r\nHost: {}\r\n", endpoint.host());
    for (name, value) in headers {
        text.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        text.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    text.push_str("Connection: close\r\n\r\n");
    if let Some(body) = body {
        text.push_str(body);
    }
    let written = stream.write_all(text.as_bytes()).and_then(|()| stream.flush());
    Ok((stream, fail, written))
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
