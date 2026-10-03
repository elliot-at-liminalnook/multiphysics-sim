//! Browser compatibility client for the two loopback hardware adapters,
//! `serve_actuator_calibration` (leg calibration, tuning, campaign, gait
//! playback on the leg) and `serve_motor_bench` (live motor sync). Native
//! workflows use `crate::hardware::local` in process (docs/hardware-parity.md,
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

use std::time::Duration;

pub use http::{new_client_id, process_client_id};
/// The loopback transport's address and connect timeout
/// ([`crate::loopback_http`], shared with the CAD client).
pub use crate::loopback_http::{CONNECT_TIMEOUT, Endpoint};

/// A process-wide generation: mode teardown/recreation must not reuse a
/// previous connection's authorization while retaining the same client id.
pub fn next_connection_generation() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static GENERATION: AtomicU64 = AtomicU64::new(1);
    GENERATION.fetch_add(1, Ordering::Relaxed)
}

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
    /// Pinned virtual execution identity and native connection generation.
    pub calibration_execution: Option<(calibration::ExecutionIdentity, u64)>,
}

pub use crate::hardware::protocol::{Body, Json, js_number, js_number_text, encode_uri_component};
