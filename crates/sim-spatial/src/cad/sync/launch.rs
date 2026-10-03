//! Starting RoboCAD's headless service for a `.rcad` file (the connect
//! job's `self_start`) and the checks on its first answer: the port-race
//! refusal (`serves`, `accept_served`) and the log tail quoted in errors.
use super::LOG_TAIL;
use crate::cad::document::{ChildSlot, Connected};
use crate::jobs::Ctx;
use sim_runtime::cad_client::{CadClient, Health, service};
use std::path::Path;
use std::time::Instant;

/// Whether `health` is RoboCAD serving `document` (compared canonically,
/// so /tmp and /private/tmp agree).
pub(in crate::cad) fn serves(health: &Health, document: &Path) -> bool {
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    health.app == "robocad" && health.path.as_deref().is_some_and(|p| canonical(Path::new(p)) == canonical(document))
}

/// The port-race check: `health` (the first answer at `url`) is accepted
/// only when it is a headless RoboCAD serving `document` (`serves`, and
/// `gui` false: the child `service_command` starts is `robocad.api`, which
/// has no window). A desktop RoboCAD on the same file that took the port is
/// another process (its edits are its own; this window only sent `GET /`),
/// so it is refused like any other service; the refusal names what answered.
pub(in crate::cad) fn accept_served(url: &str, health: Health, document: &Path) -> Result<Health, String> {
    if serves(&health, document) && !health.gui {
        return Ok(health);
    }
    let what = match (health.app.is_empty(), health.gui) {
        (true, _) => "an unknown service",
        (false, true) => "RoboCAD's desktop window",
        (false, false) => health.app.as_str(),
    };
    Err(format!(
        "{url} answered as {what} serving {}, not the headless RoboCAD service started for {}: another service took the port",
        health.path.as_deref().unwrap_or("a new document"),
        document.display()
    ))
}

/// What a connect job says when the document closed its child slot.
const SLOT_CLOSED: &str = "CAD mode closed this document while RoboCAD's service was starting; the service was stopped";

/// The connect job of a `.rcad` file: the service on a free loopback port,
/// waited for until it answers (a large document loads before it binds).
/// The process goes into `slot` as soon as it is spawned (the document
/// holds the same slot); a failed, cancelled or closed start stops it here.
pub(super) fn self_start(path: &Path, slot: &ChildSlot, ctx: &Ctx) -> Result<Connected, String> {
    let cad_dir = crate::workspace::path("cad")?;
    let python = service::interpreter(&cad_dir)?;
    let document = std::path::absolute(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !document.is_file() {
        return Err(format!("{}: no such file", document.display()));
    }
    let port = service::free_port()?;
    let url = format!("http://127.0.0.1:{port}");
    let client = CadClient::new(&url).map_err(|e| e.to_string())?;
    let log = service::log_path(port);
    let child = crate::jobs::ChildProcess::spawn("RoboCAD service", service::service_command(&cad_dir, &python, &document, port))?;
    let pid = child.id();
    if let Err(child) = slot.put(child) {
        child.stop();
        return Err(SLOT_CLOSED.into());
    }
    ctx.message(format!("started RoboCAD's headless service (pid {pid}) at {url}; waiting for it to load {}", document.display()));
    let deadline = Instant::now() + service::START_TIMEOUT;
    let live = service::wait_until_live(
        &client,
        deadline,
        || match slot.exited() {
            Some(exit) => Err(exit),
            None if slot.closed() => Err(SLOT_CLOSED.into()),
            None => Ok(()),
        },
        || ctx.cancelled() || slot.closed(),
    );
    let stopped = ctx.cancelled() || slot.closed();
    // The port was free when chosen, but another service could have bound it
    // before this child did: only RoboCAD serving this document is accepted.
    let live = live.and_then(|health| accept_served(&url, health, &document));
    match live {
        Ok(health) if !stopped => Ok(Connected { client, health, self_started: true }),
        result => {
            // A service that never answered (or whose start was cancelled)
            // is stopped, if the document has not already taken it.
            if let Some(child) = slot.take() {
                child.stop();
            }
            match result {
                Ok(_) => Err(SLOT_CLOSED.into()),
                Err(e) if stopped => Err(e),
                Err(e) => Err(format!("{e}; its log {} ends: {}", log.display(), log_tail(&log))),
            }
        }
    }
}

pub(super) fn log_tail(log: &Path) -> String {
    let tail = service::log_tail(log, LOG_TAIL);
    let tail = tail.trim();
    if tail.is_empty() { "(empty)".into() } else { tail.to_string() }
}
