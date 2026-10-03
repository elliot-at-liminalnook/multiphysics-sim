//! The CAD service's lifecycle without a window (native-viewer.md "CAD mode
//! (2026-09-30)", Decisions: unsaved edits; the review note on
//! `sync::serves`):
//!
//! - closing the window detaches a self-started service that may hold
//!   unsaved edits (left running, its URL returned) and stops a clean one,
//!   both called directly (`CadDocument::release_child`) and through the
//!   real `Last` system (`sync::on_exit` reading an `AppExit` message);
//! - a self-started service is accepted only when it is RoboCAD serving the
//!   opened file (`sync::serves`), so another service that took the port in
//!   the `free_port` race is refused.
//!
//! The "service" is a harmless `sleep`, started (like every process) through
//! `crate::jobs::ChildProcess`. Liveness is probed with `kill -0 PID`, also
//! started through `ChildProcess`; a detached sleeper is killed by the test
//! once its liveness was asserted, and sleeps only 20 s if that ever fails.
use super::sync;
use sim_runtime::cad_client::Health;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use {
    super::document::{CadDocument, CadTarget, Connection},
    bevy::prelude::*,
    sim_runtime::cad_client::CadClient,
    std::time::{Duration, Instant},
};

#[cfg(unix)]
const URL: &str = "http://127.0.0.1:18431";
/// How long a sleeper standing in for the service sleeps (bounded, so one
/// left behind by a failed test ends on its own).
#[cfg(unix)]
const SLEEP_S: &str = "20";

/// A command handed to `jobs::ChildProcess` (nothing here starts it), with
/// no inherited stdio.
#[cfg(unix)]
fn command(program: &str, args: &[&str]) -> std::process::Command {
    let mut c = std::process::Command::new(program);
    c.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    c
}

/// Runs `program args` through `jobs::ChildProcess` and waits (at most 2 s)
/// for its exit report, "{program} exited ({status})".
#[cfg(unix)]
fn run(program: &str, args: &[&str]) -> String {
    let mut child = crate::jobs::ChildProcess::spawn(program, command(program, args)).unwrap_or_else(|e| panic!("{e}"));
    let started = Instant::now();
    loop {
        if let Some(report) = child.exited() {
            return report;
        }
        assert!(started.elapsed() < Duration::from_secs(2), "{program} {args:?} did not finish within 2 s");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// True while `pid` exists (`kill -0` exits 0). A zombie still answers until
/// it is reaped, so "gone" also proves a reaper waited for it. Every test
/// asserts a live pid answers true before relying on a false answer.
#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    run("kill", &["-0", &pid.to_string()]).ends_with("(exit status: 0)")
}

/// Polls `f` until it is true or `bound` passes; returns whether it became true.
#[cfg(unix)]
fn within(bound: Duration, mut f: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < bound {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    f()
}

/// `pid` stays alive for all of `span`: a killed process would be reaped
/// (by `ChildProcess::stop`'s reaper) within it and fail a probe.
#[cfg(unix)]
fn stays_alive(pid: u32, span: Duration) -> bool {
    !within(span, || !pid_alive(pid))
}

/// Ends a sleeper the document detached (only called while it is known to
/// be alive, so its pid cannot have been reused) and waits until the
/// detach's reaper has collected it.
#[cfg(unix)]
fn kill_detached(pid: u32) {
    run("kill", &[&pid.to_string()]);
    assert!(within(Duration::from_secs(2), || !pid_alive(pid)), "the detached test sleeper (pid {pid}) was killed and reaped");
}

/// A connected document whose service this window started (a sleeper in
/// its child slot), with RoboCAD's last `GET /` saying `dirty`.
#[cfg(unix)]
fn self_started(dirty: bool) -> (CadDocument, u32) {
    let file = "/tmp/sim-spatial-lifecycle-test.rcad";
    let mut doc = CadDocument::new(CadTarget::File(PathBuf::from(file)));
    doc.client = Some(CadClient::new(URL).unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), path: Some(file.into()), dirty, revision: 7, ..Default::default() });
    doc.url = Some(URL.into());
    let child = crate::jobs::ChildProcess::spawn("test RoboCAD service", command("sleep", &[SLEEP_S])).unwrap_or_else(|e| panic!("{e}"));
    let pid = child.id();
    if doc.child.put(child).is_err() {
        panic!("a new document's child slot is open");
    }
    assert!(doc.child.running(), "the sleeper runs");
    assert!(pid_alive(pid), "the probe sees the running sleeper (pid {pid})");
    (doc, pid)
}

/// Written only: ordinary close reaches the existing release owner after
/// acknowledged preferences and actual Window destruction, preserving CAD.
#[cfg(unix)]
#[test]
fn ordinary_acknowledged_close_detaches_unsaved_service() {
    use crate::app::{close::{CloseAction, ClosePlugin}, actions::Act, settings::SettingsOwner};
    let (doc, pid) = self_started(true);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<crate::app::actions::Replies>()
        .add_message::<bevy::window::WindowCloseRequested>()
        .insert_resource(SettingsOwner::fixture_durable())
        .insert_resource(crate::ui_kit::UiFonts {
            regular:default(), italic:default(), mono:default(), medium:default(), semibold:default(),
            icons:std::collections::BTreeMap::new(),
        })
        .insert_resource(doc);
    crate::app::configure_sets(&mut app);
    app.add_plugins(ClosePlugin)
        .add_systems(Last, bevy::window::exit_on_all_closed.in_set(bevy::window::ExitSystems))
        .add_systems(Last, sync::on_exit.after(bevy::window::ExitSystems));
    let window = app.world_mut().spawn(Window::default()).id();
    app.world_mut().write_message(Act::ui(CloseAction::CloseRequest));
    app.update();
    assert!(app.world().get::<Window>(window).is_some());
    assert!(app.world().resource::<CadDocument>().child.is_some());
    app.update();
    assert!(app.world().get::<Window>(window).is_none());
    let doc = app.world().resource::<CadDocument>();
    assert!(doc.child.closed() && !doc.child.is_some());
    let alive = stays_alive(pid, Duration::from_millis(400));
    if alive { kill_detached(pid); }
    assert!(alive, "ordinary close must detach unsaved CAD, never kill it");
}

#[cfg(unix)]
#[test]
fn closing_the_window_detaches_a_dirty_self_started_service() {
    let (mut doc, pid) = self_started(true);
    assert_eq!(doc.unsaved(), Some(true));
    let left = doc.release_child("the window closed");
    let alive = stays_alive(pid, Duration::from_millis(400));
    if alive {
        kill_detached(pid);
    }
    assert_eq!(left.as_deref(), Some(URL), "the URL of the service left running is returned");
    assert!(alive, "the dirty service (pid {pid}) was detached, not stopped");
    assert!(doc.child.closed() && !doc.child.is_some(), "the slot is closed and empty");
}

#[cfg(unix)]
#[test]
fn closing_the_window_detaches_a_self_started_service_whose_state_is_unconfirmed() {
    // The last GET / said clean, but the connection is lost: RoboCAD may hold
    // edits made since, so unsaved() is None and the service is kept.
    let (mut doc, pid) = self_started(false);
    doc.connection = Connection::Lost { error: "GET / timed out".into(), since: std::time::Instant::now() };
    assert_eq!(doc.unsaved(), None);
    let left = doc.release_child("the window closed");
    let alive = stays_alive(pid, Duration::from_millis(400));
    if alive {
        kill_detached(pid);
    }
    assert_eq!(left.as_deref(), Some(URL));
    assert!(alive, "the unconfirmed service (pid {pid}) was detached, not stopped");
    assert!(doc.child.closed() && !doc.child.is_some());
}

#[cfg(unix)]
#[test]
fn app_exit_detaches_a_dirty_self_started_service_through_on_exit() {
    let (doc, pid) = self_started(true);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).insert_resource(doc).add_systems(Last, sync::on_exit.after(bevy::window::ExitSystems));
    app.update();
    {
        let doc = app.world().resource::<CadDocument>();
        assert!(doc.child.is_some() && !doc.child.closed(), "nothing is released without an AppExit");
    }
    app.world_mut().write_message(AppExit::Success);
    app.update();
    let (released, closed) = {
        let doc = app.world().resource::<CadDocument>();
        (!doc.child.is_some(), doc.child.closed())
    };
    let alive = stays_alive(pid, Duration::from_millis(400));
    if alive {
        kill_detached(pid);
    }
    assert!(released && closed, "on_exit took the service out of the slot and closed it");
    assert!(alive, "on_exit detached the dirty service (pid {pid}) instead of stopping it");
    // A second frame after the exit does nothing more (on_exit runs once).
    app.update();
}

#[cfg(unix)]
#[test]
fn closing_the_window_stops_a_clean_self_started_service() {
    let (mut doc, pid) = self_started(false);
    assert_eq!(doc.unsaved(), Some(false));
    let started = Instant::now();
    assert_eq!(doc.release_child("the window closed"), None, "nothing is left running");
    assert!(started.elapsed() < Duration::from_millis(500), "release_child does not wait for the stop: {:?}", started.elapsed());
    assert!(doc.child.closed() && !doc.child.is_some(), "the slot is closed and empty");
    let gone = within(Duration::from_secs(2), || !pid_alive(pid));
    if !gone {
        // Not stopped (the failure under test): still alive, so its pid is ours to kill.
        run("kill", &[&pid.to_string()]);
    }
    assert!(gone, "the clean service (pid {pid}) was stopped and reaped");
}

/// A directory under the temp dir, removed on drop.
struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("sim-spatial-cad-lifecycle-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        TempDir(dir)
    }
    fn file(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, "{}").unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        path
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn health(app: &str, path: Option<&Path>) -> Health {
    Health { ok: true, app: app.into(), path: path.map(|p| p.display().to_string()), ..Default::default() }
}

#[test]
fn serves_accepts_robocad_serving_the_same_file_however_spelled() {
    let dir = TempDir::new("same");
    let document = dir.file("bracket.rcad");
    assert!(sync::serves(&health("robocad", Some(&document)), &document));
    // A `.` component, and the canonical spelling (on macOS the temp dir is
    // under /var/folders, canonically /private/var/folders), both ways round.
    let dotted = dir.0.join(".").join("bracket.rcad");
    let canonical = std::fs::canonicalize(&document).unwrap();
    assert!(sync::serves(&health("robocad", Some(&dotted)), &document));
    assert!(sync::serves(&health("robocad", Some(&document)), &dotted));
    assert!(sync::serves(&health("robocad", Some(&canonical)), &document));
    assert!(sync::serves(&health("robocad", Some(&document)), &canonical));
    #[cfg(unix)]
    {
        let link = dir.0.join("link.rcad");
        std::os::unix::fs::symlink(&document, &link).unwrap();
        assert!(sync::serves(&health("robocad", Some(&link)), &document), "a symlink to the opened file is the same file");
    }
}

#[test]
fn serves_refuses_another_app_another_file_and_a_new_document() {
    let dir = TempDir::new("refuse");
    let document = dir.file("bracket.rcad");
    let other = dir.file("other.rcad");
    assert!(!sync::serves(&health("other-service", Some(&document)), &document), "another app serving the same path");
    assert!(!sync::serves(&health("", Some(&document)), &document), "a service that does not name itself");
    assert!(!sync::serves(&health("robocad", Some(&other)), &document), "RoboCAD serving another file");
    assert!(!sync::serves(&health("robocad", None), &document), "RoboCAD with a new (unsaved) document");
}

/// The connect job's port-race check (`sync::accept_served`, called by
/// `self_start` on the first answer): another service on the chosen port is
/// refused by name, and RoboCAD serving the opened file is accepted.
#[test]
fn a_service_that_took_the_port_is_refused_by_name() {
    let dir = TempDir::new("race");
    let document = dir.file("bracket.rcad");
    let other = dir.file("other.rcad");
    let url = "http://127.0.0.1:8431";
    let accepted = sync::accept_served(url, health("robocad", Some(&document)), &document);
    assert_eq!(accepted.map(|h| h.app), Ok("robocad".to_string()));
    let refused = sync::accept_served(url, health("other-service", Some(&document)), &document).unwrap_err();
    assert!(refused.contains("another service took the port") && refused.contains("other-service") && refused.contains(url), "{refused}");
    let refused = sync::accept_served(url, health("robocad", Some(&other)), &document).unwrap_err();
    assert!(refused.contains("another service took the port") && refused.contains("other.rcad"), "{refused}");
    let refused = sync::accept_served(url, health("", None), &document).unwrap_err();
    assert!(refused.contains("an unknown service serving a new document"), "{refused}");
    // A desktop RoboCAD on the same file is not the headless child this window started.
    let desktop = Health { gui: true, ..health("robocad", Some(&document)) };
    let refused = sync::accept_served(url, desktop, &document).unwrap_err();
    assert!(refused.contains("another service took the port") && refused.contains("RoboCAD's desktop window"), "{refused}");
}

/// /tmp is a symlink to /private/tmp on macOS: either spelling of the
/// opened file is the same document.
#[cfg(target_os = "macos")]
#[test]
fn serves_agrees_on_tmp_and_private_tmp() {
    let name = format!("sim-spatial-cad-lifecycle-tmp-{}.rcad", std::process::id());
    let short = Path::new("/tmp").join(&name);
    let long = Path::new("/private/tmp").join(&name);
    std::fs::write(&short, "{}").unwrap_or_else(|e| panic!("{}: {e}", short.display()));
    let both = sync::serves(&health("robocad", Some(&long)), &short) && sync::serves(&health("robocad", Some(&short)), &long);
    let _ = std::fs::remove_file(&short);
    assert!(both, "{} and {} are the same file", short.display(), long.display());
}
