//! A child process the viewer started and owns (RoboCAD's headless REST
//! service, started by CAD mode). Only a process the viewer itself spawned
//! ever becomes a [`ChildProcess`], so only self-started services are ever
//! stopped: a RoboCAD the viewer attached to has no `ChildProcess` and is
//! never touched.
//!
//! Nothing here blocks the caller: `stop` sends the kill and leaves the wait
//! to a reaper thread ([`super::reap_child`]), `detach` leaves the process
//! running with a reaper waiting for it, and `exited` is a non-blocking
//! `try_wait`.
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
    pub fn spawn(name: &str, mut command: Command) -> Result<ChildProcess, String> {
        let child = command.spawn().map_err(|e| format!("could not start {name}: {e}"))?;
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
    /// A failed check is reported too ("could not check whether {name} is
    /// running: {e}"), and repeated the same way.
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
            Err(e) => {
                self.exit = Some(format!("could not check whether {} is running: {e}", self.name));
                self.exit.clone()
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
