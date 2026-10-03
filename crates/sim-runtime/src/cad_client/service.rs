//! Starting a headless RoboCAD service for the viewer: the interpreter
//! `cad/run.sh` sets up, a free loopback port, the command line, the
//! child's stderr log, and polling until the API answers.
//!
//! sim-runtime never owns or kills the child: it only builds the
//! [`std::process::Command`] ([`service_command`]) and polls
//! ([`wait_until_live`]). The viewer spawns, reaps and stops it
//! (`jobs::ChildProcess` in sim-spatial), on a job thread: these helpers
//! block.
//!
//! The caller passes the `cad/` directory (sim-spatial resolves
//! `<workspace>/cad`).
use super::{CadClient, Health};
use std::io::{Read, Seek, SeekFrom};
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a service may take to answer after starting: a large `.rcad`
/// loads before the server binds its port.
pub const START_TIMEOUT: Duration = Duration::from_secs(120);
/// Interval between health polls.
const POLL_INTERVAL: Duration = Duration::from_millis(150);
/// Read/write timeout of one health poll (the connect timeout is
/// [`crate::loopback_http::CONNECT_TIMEOUT`]); short, so a cancel or a
/// dead child is noticed promptly.
const POLL_TIMEOUT: Duration = Duration::from_secs(2);

/// The Python of RoboCAD's virtual environment, `cad_dir/.venv/bin/python`
/// (what `cad/run.sh` creates and runs). Never creates it: when it is
/// missing the error names the path and says to run `cad/run.sh` once.
pub fn interpreter(cad_dir: &Path) -> Result<PathBuf, String> {
    #[cfg(windows)]
    let python = cad_dir.join(".venv").join("Scripts").join("python.exe");
    #[cfg(not(windows))]
    let python = cad_dir.join(".venv").join("bin").join("python");
    if python.is_file() {
        Ok(python)
    } else {
        Err(format!("{}: RoboCAD's Python environment is missing; run cad/run.sh once to create the venv", python.display()))
    }
}

/// A port nothing listens on now: binds `127.0.0.1:0`, reads the port the
/// system chose and releases it. Another process could take it before the
/// service binds; the service then fails to start and says so in its log,
/// and the caller must not take that other process's answer for its own
/// (sim-spatial's connect job accepts only a headless RoboCAD serving the
/// file it started, `cad::sync::launch::accept_served`).
pub fn free_port() -> Result<u16, String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| format!("could not find a free port on 127.0.0.1: {e}"))?;
    let port = listener.local_addr().map_err(|e| format!("could not find a free port on 127.0.0.1: {e}"))?.port();
    Ok(port)
}

/// Where the service started on `port` writes its stderr:
/// `<temp dir>/robocad-api-<viewer pid>-<port>.log`.
pub fn log_path(port: u16) -> PathBuf {
    std::env::temp_dir().join(format!("robocad-api-{}-{port}.log", std::process::id()))
}

/// `python -m robocad.api <document> --port N --host 127.0.0.1`, run in
/// `cad_dir` (so `robocad` imports from it), the document as an absolute
/// path (resolved against this process's working directory). stdin and
/// stdout are null; stderr goes to [`log_path`]`(port)`, created (or
/// truncated) here, or is null when that file cannot be created. Read its
/// end with [`log_tail`] when the service fails.
pub fn service_command(cad_dir: &Path, python: &Path, document: &Path, port: u16) -> Command {
    let document = std::path::absolute(document).unwrap_or_else(|_| document.to_path_buf());
    let mut command = Command::new(python);
    command
        .arg("-m")
        .arg("robocad.api")
        .arg(document)
        .arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg("127.0.0.1")
        .current_dir(cad_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    match std::fs::File::create(log_path(port)) {
        Ok(file) => command.stderr(file),
        Err(_) => command.stderr(Stdio::null()),
    };
    command
}

/// The last lines of `path` within `max_bytes` (from the first whole line),
/// trimmed; empty when the file is missing or empty. For error messages
/// from the child's stderr.
pub fn log_tail(path: &Path, max_bytes: usize) -> String {
    let Ok(mut file) = std::fs::File::open(path) else { return String::new() };
    let length = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = length.saturating_sub(max_bytes as u64);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    if file.take(max_bytes as u64).read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0 {
        // Cut mid-line: keep from the first whole line (unless there is only one).
        if let Some(newline) = text.find('\n') {
            if newline + 1 < text.len() {
                text = text[newline + 1..].to_string();
            }
        }
    }
    text.trim().to_string()
}

/// Polls `GET /` every 150 ms until the service answers `ok`, and returns
/// its health. Stops with an error when `alive` errs (the child exited: its
/// message), when `cancelled()` (`"cancelled"`), or at `deadline` (naming
/// the URL, the seconds waited and the last reason). At the deadline
/// `alive` is asked once more and its error preferred: a child that exited
/// during the last probe is reported as such, not as a timeout. Blocks:
/// call it from a job thread.
///
/// `cancelled` and `alive` are checked between probes, so a cancel or an
/// exit is noticed within one probe (at most
/// [`crate::loopback_http::CONNECT_TIMEOUT`] plus the probe's read/write
/// timeout, the client's capped at 2 s) plus one 150 ms pause.
pub fn wait_until_live(client: &CadClient, deadline: Instant, mut alive: impl FnMut() -> Result<(), String>, cancelled: impl Fn() -> bool) -> Result<Health, String> {
    let started = Instant::now();
    let probe = client.clone().with_timeout(client.timeout.min(POLL_TIMEOUT));
    loop {
        if cancelled() {
            return Err("cancelled".into());
        }
        alive()?;
        let last = match probe.health() {
            Ok(health) if health.ok => return Ok(health),
            Ok(_) => String::from("the server answered, but not as RoboCAD (no \"ok\": true)"),
            Err(e) => e.to_string(),
        };
        let now = Instant::now();
        if now >= deadline {
            alive()?;
            return Err(format!("RoboCAD at {} did not answer within {:.0} s ({last})", client.url(), now.duration_since(started).as_secs_f64()));
        }
        std::thread::sleep(POLL_INTERVAL.min(deadline - now));
    }
}
