//! One typed client for the two loopback hardware servers,
//! `serve_actuator_calibration` (leg calibration, tuning, campaign, gait
//! playback on the leg) and `serve_motor_bench` (live motor sync), used by
//! the native viewer's hardware panel (docs/hardware-parity.md,
//! docs/architecture/native-viewer.md §8).
//!
//! The servers own the serial bus, leases, watchdogs, the supervisor and
//! STOP; this client only sends what the browser pages send
//! (`web/viewer/calibration-ui.mjs`, `hardware-sync.mjs`), with the same
//! headers, and reads what they answer. Nothing here opens a serial port or
//! decides motion.
//!
//! - **Loopback only.** [`Endpoint::parse`] accepts `http://127.0.0.1:PORT`
//!   and `http://localhost:PORT` (connected as 127.0.0.1) and refuses any
//!   other host, `[::1]` and other 127.x addresses included: both servers
//!   bind 127.0.0.1 only and require `Host: 127.0.0.1:PORT`. Every
//!   connection has a connect, read and write timeout. Plain HTTP/1.1 over
//!   `std::net::TcpStream` (the shared [`crate::loopback_http`]), one request per
//!   connection (`Connection: close`, as both servers answer), so a slow
//!   request never holds another's socket.
//! - **Headers.** `Host` is the server's own origin (`127.0.0.1:PORT`: the
//!   servers refuse any other), `X-Control-Token`, `X-Client-Id` (one random
//!   UUID per viewer process, [`process_client_id`], reused on every connect
//!   and reconnect as the page keeps one per page load; 36 characters of hex
//!   and dashes, as the servers check), `Content-Type:
//!   application/json`, and `Content-Length` for a body. No `Origin` or
//!   `Sec-Fetch-Site` (the servers accept their absence).
//! - **Token hand-off** ([`token`]): the token the server injects into the
//!   page it serves (`<meta name="calibration-token" content="…">` for the
//!   calibration server, `const token='…'` in `/` or the `motor-bridge-token`
//!   meta of `/walking/` for the bench) is read from that page with the same
//!   loopback client, exactly as the browser receives it; or from a file the
//!   operator names. The servers are unchanged.
//! - **Errors** ([`ClientError`]): a non-2xx answer surfaces the server's
//!   `error` field verbatim, as the pages show `v.error`. A request body
//!   larger than the server accepts ([`Client::max_body`]) is refused before
//!   connecting.
//! - **Closing** ([`Client::send_only`]): a request written without reading
//!   its answer, as the page's `keepalive` fetch on `pagehide`, for STOP
//!   while the window closes.
//! - **Bodies** ([`Body`]): written with their members in the page's order
//!   and numbers as JavaScript writes them ([`js_number`], ECMAScript
//!   `Number::toString`), so a body is the same bytes as the page's
//!   `JSON.stringify`.
pub mod bench;
pub mod calibration;
mod http;
pub mod token;
#[cfg(test)]
mod tests;

use serde_json::Value;
use std::time::Duration;

pub use http::{new_client_id, process_client_id};
/// The loopback transport's address and connect timeout
/// ([`crate::loopback_http`], shared with the CAD client).
pub use crate::loopback_http::{CONNECT_TIMEOUT, Endpoint};

/// Why a request failed: the shared transport's error
/// ([`crate::loopback_http::Error`]: `NotLoopback`, `Transport`,
/// `Server { status, error }`, `Decode`).
pub type ClientError = crate::loopback_http::Error;

/// Read/write timeout for an ordinary request: longer than the calibration
/// server's own 8 s wait for its hardware worker, so its answer (or its
/// "Hardware response timed out") arrives rather than a client timeout.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Read/write timeout for STOP. The calibration server latches its stop
/// flags when it parses the request, but answers only after its hardware
/// worker finishes the running job, waiting up to 8 s for it; this outlasts
/// that wait so the answer (or "Hardware response timed out") arrives. STOP
/// rides its own connection on its own thread, so a long wait blocks
/// nothing else.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(12);
/// The calibration server's request body limit (bytes; it refuses more
/// with an error before acting).
pub const CALIBRATION_MAX_BODY: usize = 4096;
/// The motor bench's request body limit (bytes; it refuses a larger body
/// with an error before acting).
pub const MOTOR_BENCH_MAX_BODY: usize = 8192;

/// Which server a client talks to (the token's place in its page differs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerKind {
    Calibration,
    MotorBench,
}

/// A client of one server: its endpoint, token, identity, timeout and
/// request body limit.
/// Cheap to clone; each request opens its own connection.
#[derive(Clone, Debug)]
pub struct Client {
    pub endpoint: Endpoint,
    pub token: String,
    /// The `X-Client-Id`: [`process_client_id`] for every client the viewer
    /// connects (one per process, as the page keeps one per page load).
    pub client_id: String,
    pub timeout: Duration,
    /// Largest request body the server accepts, in bytes
    /// ([`CALIBRATION_MAX_BODY`] or [`MOTOR_BENCH_MAX_BODY`]; see
    /// [`Client::for_kind`]). A larger body is refused before connecting.
    pub max_body: usize,
}

/// A JSON value whose object members keep the order they were written in
/// (serde_json's `Map` sorts keys; the pages' `JSON.stringify` does not).
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// A scalar, or any value whose member order does not matter. Written
    /// by serde_json: use it for strings, booleans and integers; make a
    /// non-integer `f64` with [`js_number`] (`Json::from(f64)` lands here and
    /// would not be written as JavaScript writes it).
    Value(Value),
    /// A number written as JavaScript writes it ([`js_number_text`]); made
    /// by [`js_number`]. serde_json writes some numbers differently
    /// (`3.2e-6` for JavaScript's `0.0000032`, every digit of `2^60`).
    Number(f64),
    Object(Vec<(String, Json)>),
    Array(Vec<Json>),
}
impl Json {
    /// The compact text, members in order (`JSON.stringify` without spacing).
    pub fn write(&self, out: &mut String) {
        match self {
            Json::Value(v) => out.push_str(&v.to_string()),
            Json::Number(x) => out.push_str(&js_number_text(*x)),
            Json::Object(members) => {
                out.push('{');
                for (i, (k, v)) in members.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&Value::from(k.as_str()).to_string());
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
            Json::Array(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
        }
    }
    /// As a `serde_json::Value` (member order lost; for checks and reading).
    pub fn to_value(&self) -> Value {
        match self {
            Json::Value(v) => v.clone(),
            // What a server reading the written text gets (serde_json reads
            // `5` as an integer, `1152921504606847000` as that integer).
            Json::Number(x) => serde_json::from_str(&js_number_text(*x)).unwrap_or(Value::Null),
            Json::Object(members) => Value::Object(members.iter().map(|(k, v)| (k.clone(), v.to_value())).collect()),
            Json::Array(items) => Value::Array(items.iter().map(Json::to_value).collect()),
        }
    }
}
impl<T: Into<Value>> From<T> for Json {
    fn from(v: T) -> Self {
        Json::Value(v.into())
    }
}

/// A request body: an object with its members in the page's order.
#[derive(Clone, Debug, PartialEq)]
pub struct Body(pub Vec<(String, Json)>);
impl Body {
    pub fn new(members: Vec<(&str, Json)>) -> Self {
        Body(members.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }
    /// `{}` (the bench's `/stop`).
    pub fn empty() -> Self {
        Body(Vec::new())
    }
    /// The exact bytes sent.
    pub fn text(&self) -> String {
        let mut out = String::new();
        Json::Object(self.0.clone()).write(&mut out);
        out
    }
    pub fn to_value(&self) -> Value {
        Json::Object(self.0.clone()).to_value()
    }
    /// A member's value, if present.
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}
impl From<Body> for Json {
    fn from(b: Body) -> Self {
        Json::Object(b.0)
    }
}

/// A number as JavaScript's `JSON.stringify` writes it ([`js_number_text`]);
/// non-finite values are `null`, as in JavaScript.
pub fn js_number(x: f64) -> Json {
    if x.is_finite() { Json::Number(x) } else { Json::Value(Value::Null) }
}

/// The text `JSON.stringify` writes for `x`: ECMAScript
/// `Number::toString(x)` (ECMA-262 §6.1.6.1.20) for finite values, with
/// `-0` as `0`; `null` for NaN and the infinities.
///
/// With `k` the shortest round-trip digits `d₁…d_k` and `n` the decimal
/// exponent (the value is `0.d₁…d_k × 10ⁿ`): `k ≤ n ≤ 21` writes the
/// digits then `n − k` zeros; `0 < n ≤ 21` puts the point after `n`
/// digits; `−6 < n ≤ 0` writes `0.`, `−n` zeros and the digits; otherwise
/// `d₁[.d₂…d_k]e±(n−1)`. So `0.0000032`, `1e-7`, `1e+21`, and `2^60` as
/// `1152921504606847000`. The digits are Rust's shortest round-trip digits
/// (`{:e}`), the same set JavaScript requires (the shortest `k`, and among
/// those the closest to `x`).
pub fn js_number_text(x: f64) -> String {
    if !x.is_finite() {
        return "null".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    let sci = format!("{:e}", x.abs());
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let k = digits.len() as i64;
    let n = exponent.parse::<i64>().unwrap_or(0) + 1;
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    if k <= n && n <= 21 {
        out.push_str(digits);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if n - 1 >= 0 { '+' } else { '-' });
        out.push_str(&(n - 1).abs().to_string());
    }
    out
}

/// `deserialize_with` for a status field: a value of the wrong shape reads
/// as the field's default instead of failing the whole answer, so one
/// malformed section (a server change, a partial write) cannot hide the rest.
pub(crate) fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = <Value as serde::Deserialize>::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// `deserialize_with` for a list: the items that parse (a malformed item is
/// dropped, not the list); anything but an array reads as empty.
pub(crate) fn lenient_items<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = <Value as serde::Deserialize>::deserialize(deserializer)?;
    Ok(match value {
        Value::Array(items) => items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect(),
        _ => Vec::new(),
    })
}

/// `encodeURIComponent`: everything but `A–Z a–z 0–9 - _ . ! ~ * ' ( )` is
/// percent-encoded as UTF-8.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
