//! Child processes the viewer starts. Nothing outside `jobs` spawns a
//! process (`tests::processes_are_started_only_in_jobs` scans for it).
//!
//! - [`ChildProcess`]: a process the viewer started and owns (RoboCAD's
//!   headless REST service, started by CAD mode). Only a process the viewer
//!   itself spawned ever becomes a `ChildProcess`, so only self-started
//!   services are ever stopped: a RoboCAD the viewer attached to has no
//!   `ChildProcess` and is never touched.
//! - [`spawn_detached`]: a process whose lifetime is not tied to ours (the
//!   linked `sim-viewer` schematic window), reaped when it exits.
//! - [`open_in_browser`]: an http(s) link opened with the system opener
//!   (`open` on macOS, `xdg-open` elsewhere), detached and reaped.
//! - [`open_local`]: an existing local file or folder (an absolute path)
//!   opened with the same opener, detached and reaped (cad-print's assembly
//!   guide and coupon protocol folder).
//!
//! Nothing here blocks the caller: `stop` sends the kill and leaves the wait
//! to a reaper thread ([`super::reap_child`]), `detach` leaves the process
//! running with a reaper waiting for it, and `exited` is a non-blocking
//! `try_wait`.
use std::path::Path;
use std::process::{Child, Command};

use super::reap_child;

/// An owned child process. Dropping it stops it, like [`Self::stop`].
pub struct ChildProcess {
    name: String,
    id: u32,
    /// Taken by `stop`/`detach` (and so by the drop), so it is handled once.
    child: Option<Child>,
    /// The process has exited and its status was collected (reaped).
    reaped: bool,
    /// What `exited` reported, repeated on later calls.
    exit: Option<String>,
}

impl ChildProcess {
    /// Starts `command`. `name` names the process in errors, exit reports
    /// and the reaper thread: "could not start {name}: {e}".
    pub fn spawn(name: &str, command: Command) -> Result<ChildProcess, String> {
        Self::start(name, command).map_err(|e| format!("could not start {name}: {e}"))
    }

    /// [`Self::spawn`] with the OS error as it is, for callers that word it.
    fn start(name: &str, mut command: Command) -> std::io::Result<ChildProcess> {
        let child = command.spawn()?;
        Ok(ChildProcess { name: name.to_string(), id: child.id(), child: Some(child), reaped: false, exit: None })
    }

    /// The OS process id.
    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Non-blocking: None while the process runs; once it has exited (it is
    /// then reaped), "{name} exited ({status})" on this and every later call.
    /// A failed check is logged and answers None (possibly running), so a
    /// service that may hold unsaved edits is never taken for exited.
    pub fn exited(&mut self) -> Option<String> {
        if self.exit.is_some() {
            return self.exit.clone();
        }
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => {
                self.reaped = true;
                self.exit = Some(format!("{} exited ({status})", self.name));
                self.exit.clone()
            }
            // Unknown is treated as running, and not remembered: a process
            // that may hold unsaved work must not be taken for exited.
            Err(e) => {
                bevy::log::warn!("could not check whether {} (pid {}) is running: {e}", self.name, self.id);
                None
            }
        }
    }

    /// Kills the process (`Child::kill`, SIGKILL on unix) and waits for it on
    /// a reaper thread, so the caller never blocks. A process that already
    /// exited is only reaped.
    pub fn stop(mut self) {
        self.stop_now();
    }

    /// Leaves the process running; a reaper thread waits for it so it never
    /// lingers as a zombie. Its lifetime is no longer tied to the viewer's.
    pub fn detach(mut self) {
        if let Some(child) = self.child.take()
            && !self.reaped
        {
            reap_child(child, &self.name);
        }
    }

    fn stop_now(&mut self) {
        let Some(mut child) = self.child.take() else { return };
        if self.reaped {
            return;
        }
        // Exited already (status collected now): nothing to kill or wait for.
        if let Ok(Some(_)) = child.try_wait() {
            self.reaped = true;
            return;
        }
        if let Err(e) = child.kill() {
            bevy::log::warn!("could not stop {} (pid {}): {e}", self.name, self.id);
        }
        // reap_child warns and leaves the zombie to the OS if it cannot start a thread.
        reap_child(child, &self.name);
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        self.stop_now();
    }
}

/// Starts `command` and leaves it running: its lifetime is not tied to the
/// viewer's, and a reaper thread waits for it so it never lingers as a
/// zombie ([`ChildProcess::detach`]). Returns its OS process id. `name`
/// names it in the error, "could not start {name}: {e}", and the reaper.
pub fn spawn_detached(name: &str, command: Command) -> Result<u32, String> {
    let child = ChildProcess::spawn(name, command)?;
    let id = child.id();
    child.detach();
    Ok(id)
}

/// Opens an http(s) `url` in the user's browser with the system opener
/// (`open` on macOS, `xdg-open` elsewhere), detached and reaped like
/// [`spawn_detached`]. Anything else is refused with "not a web link: {url}"
/// (local paths are not handed to the opener). A failed start answers the
/// OS error as it is.
pub fn open_in_browser(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("not a web link: {url}"));
    }
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let mut command = Command::new(opener);
    command.arg(url);
    ChildProcess::start(opener, command).map_err(|e| e.to_string())?.detach();
    Ok(())
}

/// Opens an existing local file or folder (an absolute `path`) with the
/// system opener (`open` on macOS, `xdg-open` elsewhere), detached and
/// reaped like [`open_in_browser`]. A relative path is refused with "not
/// an absolute path: {path}" and one that is neither a file nor a folder
/// with "no such file or folder: {path}", before any opener is started (a
/// link is [`open_in_browser`]'s). It checks the disk, so callers run it on
/// a `Pool::Io` job. A failed start answers the OS error as it is.
pub fn open_local(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!("not an absolute path: {}", path.display()));
    }
    if !(path.is_file() || path.is_dir()) {
        return Err(format!("no such file or folder: {}", path.display()));
    }
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let mut command = Command::new(opener);
    command.arg(path);
    ChildProcess::start(opener, command).map_err(|e| e.to_string())?.detach();
    Ok(())
}
