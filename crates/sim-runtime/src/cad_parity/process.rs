//! Synchronous Unix child ownership. WNOWAIT retains the leader's PID (and PGID)
//! even after exit until ALL group signals are finished. No feature thread.
//! Requires exclusive wait ownership and a non-auto-reaping SIGCHLD disposition.
//! ECHILD/observation errors abandon identity before any further signal. Final
//! KILL delivery precedes reaping; escaped process groups are not contained.
use std::{
    process::{Child, Command, ExitStatus},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct ProcessCompletion {
    pub exit: Option<ExitStatus>,
    pub interruption: Option<String>,
    pub cleanup_error: Option<String>,
}
const GROUP_TERM: i32 = 15; // Unix SIGTERM
const GROUP_KILL: i32 = 9; // Unix SIGKILL

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Owned,
    SignalsFinished,
    Released,
    Lost,
}
trait Backend {
    /// Observe without consuming the waitable leader. ECHILD means ownership lost.
    fn observe(&mut self) -> Result<bool, String>;
    fn signal(&mut self, signal: i32) -> Result<(), String>;
    fn reap(&mut self) -> Result<Option<ExitStatus>, String>;
    fn now(&self) -> Duration;
    fn pause(&mut self);
}
struct Lifecycle<B> {
    backend: B,
    phase: Phase,
    exit: Option<ExitStatus>,
    error: Option<String>,
}
impl<B: Backend> Lifecycle<B> {
    fn record_error(&mut self, error: String) {
        self.error = Some(match self.error.take() {
            Some(previous) => format!("{previous}; {error}"),
            None => error,
        });
    }
    fn observe(&mut self) -> Result<bool, String> {
        if self.phase != Phase::Owned {
            return Err("child identity no longer signal-owned".into());
        }
        match self.backend.observe() {
            Ok(exited) => Ok(exited),
            Err(error) => {
                // Unknown wait ownership is never permission to signal a cached number.
                self.phase = Phase::Lost;
                self.record_error(error);
                Err(self.error.clone().expect("observation error was recorded"))
            }
        }
    }
    fn completion(
        &mut self,
        deadline: Duration,
        cancelled: &dyn Fn() -> bool,
    ) -> ProcessCompletion {
        let mut interruption = None;
        if self.phase == Phase::Owned {
            loop {
                if cancelled() {
                    interruption = Some("cancelled".into());
                    break;
                }
                if self.backend.now() >= deadline {
                    interruption = Some("bounded child wait expired".into());
                    break;
                }
                match self.observe() {
                    Ok(true) => break,
                    Ok(false) => self.backend.pause(),
                    Err(error) => {
                        interruption = Some(error);
                        break;
                    }
                }
            }
        }
        let cleanup_error = self.cleanup().err();
        ProcessCompletion {
            exit: self.exit,
            interruption,
            cleanup_error,
        }
    }
    fn cleanup(&mut self) -> Result<(), String> {
        if self.phase == Phase::Owned {
            self.observe()?;
            if let Err(error) = self.backend.signal(GROUP_TERM) {
                self.record_error(error);
            }
            // Keep the zombie leader waitable throughout the grace interval. An
            // exited leader is not evidence that its descendants have stopped.
            let end = self.backend.now() + Duration::from_secs(2);
            while self.backend.now() < end {
                self.observe()?;
                self.backend.pause();
            }
            if let Err(error) = self.backend.signal(GROUP_KILL) {
                self.record_error(error);
            }
            // Never signal again, even if bounded reaping fails or Drop repeats.
            self.phase = Phase::SignalsFinished;
        }
        if self.phase == Phase::SignalsFinished {
            let end = self.backend.now() + Duration::from_secs(2);
            loop {
                match self.backend.reap() {
                    Ok(Some(exit)) => {
                        self.exit = Some(exit);
                        self.phase = Phase::Released;
                        break;
                    }
                    Ok(None) if self.backend.now() < end => self.backend.pause(),
                    Ok(None) => {
                        self.record_error(
                            "bounded reaping expired; identity retained, no further signals".into(),
                        );
                        break;
                    }
                    Err(error) => {
                        self.phase = Phase::Lost;
                        self.record_error(error);
                        break;
                    }
                }
            }
        }
        self.error.clone().map_or(Ok(()), Err)
    }
}
struct ChildBackend {
    // Monotonic capability loss also covers observations inside platform
    // signal verification, which do not pass through Lifecycle::observe.
    signal_owned: bool,
    child: Child,
    group: u32,
    epoch: Instant,
}
#[cfg(target_os = "macos")]
impl ChildBackend {
    /// XNU killpg skips zombies and returns EPERM if no live target exists.
    /// A retained leader alone is insufficient: inaccessible live descendants
    /// must still fail cleanup. libproc enumerates allproc AND zombproc.
    fn zombie_only_group(&mut self) -> Result<bool, String> {
        if !self.observe()? { return Ok(false); }
        let first = self.group_members()?;
        if !first.contains(&(self.group as libc::pid_t)) {
            return Err("owned group enumeration omitted retained leader".into());
        }
        for &pid in &first {
            let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
            // arg=1 is required to look up zombies (XNU proc_pidinfo).
            let got = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 1,
                (&mut info as *mut libc::proc_bsdinfo).cast(), size) };
            if got != size || info.pbi_pid != pid as u32 || info.pbi_pgid != self.group {
                return Err(format!("owned group member {pid} could not be verified: {}", std::io::Error::last_os_error()));
            }
            if info.pbi_status != libc::SZOMB { return Ok(false); }
        }
        // An original member could fork between the first enumeration and its
        // transition to zombie. Require the complete membership to stay equal
        // after every original member has been verified unable to fork.
        if self.group_members()? != first {
            return Err("owned group membership changed during zombie verification".into());
        }
        if !self.observe()? { return Err("retained leader exit could not be reconfirmed".into()); }
        Ok(true)
    }
    fn group_members(&self) -> Result<Vec<libc::pid_t>, String> {
        const MAX_MEMBERS: usize = 4096;
        // One sentinel entry distinguishes a full/truncated list from a
        // verified bounded list. Never allocate from an OS-reported count.
        let mut pids = vec![0 as libc::pid_t; MAX_MEMBERS + 1];
        let capacity = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
        // PROC_PGRP_ONLY=2: pinned native SDK sys/proc_info.h.
        let got = unsafe { libc::proc_listpids(2, self.group, pids.as_mut_ptr().cast(), capacity) };
        if got <= 0 || got >= capacity || got as usize % std::mem::size_of::<libc::pid_t>() != 0 {
            return Err(format!("owned group enumeration failed or exceeded bound: {}", std::io::Error::last_os_error()));
        }
        pids.truncate(got as usize / std::mem::size_of::<libc::pid_t>());
        if pids.iter().any(|pid| *pid <= 0) { return Err("owned group enumeration contained invalid PID".into()); }
        pids.sort_unstable();
        if pids.windows(2).any(|pair| pair[0] == pair[1]) { return Err("owned group enumeration contained duplicate PID".into()); }
        Ok(pids)
    }
}
impl Backend for ChildBackend {
    fn observe(&mut self) -> Result<bool, String> {
        if !self.signal_owned { return Err("child wait ownership permanently lost".into()); }
        #[cfg(unix)]
        {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // Apple SDK sys/wait.h: WNOWAIT leaves process returned waitable.
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.group as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == 0 {
                return Ok(unsafe { info.si_pid() } != 0);
            }
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EINTR) {
                return Ok(false);
            }
            self.signal_owned = false;
            Err(format!(
                "non-reaping child observation failed; ownership abandoned: {error}"
            ))
        }
        #[cfg(not(unix))]
        {
            Err("retained process-group ownership requires Unix".into())
        }
    }
    fn signal(&mut self, signal: i32) -> Result<(), String> {
        if !self.signal_owned { return Err("child signal ownership permanently lost".into()); }
        #[cfg(unix)]
        {
            let result = unsafe { libc::kill(-(self.group as libc::pid_t), signal) };
            if result == 0 {
                return Ok(());
            }
            let error = std::io::Error::last_os_error();
            // ESRCH is an empty group while the waitable leader still reserves PID.
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            #[cfg(target_os = "macos")]
            if error.raw_os_error() == Some(libc::EPERM) {
                match self.zombie_only_group() {
                    Ok(true) => return Ok(()),
                    Ok(false) => {},
                    Err(reason) => return Err(format!("owned group signal failed: {error}; zombie verification: {reason}")),
                }
            }
            Err(format!("owned group signal failed: {error}"))
        }
        #[cfg(not(unix))]
        {
            let _ = signal;
            Err("process-group cleanup requires Unix".into())
        }
    }
    fn reap(&mut self) -> Result<Option<ExitStatus>, String> {
        self.child
            .try_wait()
            .map_err(|error| format!("child reap failed: {error}"))
    }
    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }
    fn pause(&mut self) {
        std::thread::sleep(Duration::from_millis(25));
    }
}
pub struct OwnedProcess {
    lifecycle: Lifecycle<ChildBackend>,
    completion: Option<ProcessCompletion>,
}
impl OwnedProcess {
    pub fn spawn(command: &mut Command) -> Result<Self, String> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
            let child = command.spawn().map_err(|error| error.to_string())?;
            Ok(Self {
                completion: None,
                lifecycle: Lifecycle {
                    backend: ChildBackend {
                        signal_owned: true,
                        group: child.id(),
                        child,
                        epoch: Instant::now(),
                    },
                    phase: Phase::Owned,
                    exit: None,
                    error: None,
                },
            })
        }
        #[cfg(not(unix))]
        {
            let _ = command;
            Err("safe retained child identity requires Unix; process not started".into())
        }
    }
    pub fn alive(&mut self) -> Result<(), String> {
        if self.lifecycle.observe()? {
            self.stop()?;
            Err("owned child exited (descendant cleanup completed)".into())
        } else {
            Ok(())
        }
    }
    pub fn wait_completion(
        &mut self,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> ProcessCompletion {
        if let Some(completion) = &self.completion {
            return completion.clone();
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let deadline = self.lifecycle.backend.now() + remaining;
        let completion = self.lifecycle.completion(deadline, cancelled);
        self.completion = Some(completion.clone());
        completion
    }
    pub fn wait(
        &mut self,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ExitStatus, String> {
        let completion = self.wait_completion(deadline, cancelled);
        if completion.interruption.is_some() || completion.cleanup_error.is_some() {
            return Err(format!(
                "{}{}",
                completion.interruption.unwrap_or_default(),
                completion
                    .cleanup_error
                    .map(|error| format!("; cleanup: {error}"))
                    .unwrap_or_default()
            ));
        }
        completion
            .exit
            .ok_or_else(|| "child completion has no exit evidence".into())
    }
    pub fn stop(&mut self) -> Result<(), String> {
        self.lifecycle.cleanup()
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
#[cfg(test)]
#[path = "process_fixtures.rs"]
mod fixtures;
