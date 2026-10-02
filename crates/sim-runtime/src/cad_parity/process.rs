//! Synchronous process ownership for headless jobs. Unlike viewer jobs' asynchronous
//! reaper this owner polls on the caller's worker, never starts a feature thread.
use std::{
    process::{Child, Command, ExitStatus},
    time::{Duration, Instant},
};
pub struct OwnedProcess {
    child: Child,
    group: u32,
    stopped: bool,
}
#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}
#[cfg(unix)]
fn signal_group(group: u32, signal: i32) {
    unsafe {
        kill(-(group as i32), signal);
    }
}
impl OwnedProcess {
    pub fn spawn(command: &mut Command) -> Result<Self, String> {
        {
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                command.process_group(0);
            }
            command.spawn().map(|child| Self {
                group: child.id(),
                child,
                stopped: false,
            })
        }
        .map_err(|e| e.to_string())
    }
    pub fn alive(&mut self) -> Result<(), String> {
        match self.child.try_wait().map_err(|e| e.to_string())? {
            None => Ok(()),
            Some(s) => Err(format!("owned child exited: {s}")),
        }
    }
    pub fn wait(
        &mut self,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ExitStatus, String> {
        loop {
            if let Some(s) = self.child.try_wait().map_err(|e| e.to_string())? {
                return Ok(s);
            }
            if cancelled() {
                self.stop()?;
                return Err("cancelled".into());
            }
            if Instant::now() >= deadline {
                self.stop()?;
                return Err("bounded child wait expired".into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    pub fn stop(&mut self) -> Result<(), String> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        #[cfg(unix)]
        {
            signal_group(self.group, 15);
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if self.child.try_wait().map_err(|e| e.to_string())?.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            signal_group(self.group, 9);
        }
        if self.child.try_wait().map_err(|e| e.to_string())?.is_none() {
            self.child.kill().map_err(|e| e.to_string())?;
        }
        self.child.wait().map(|_| ()).map_err(|e| e.to_string())
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
