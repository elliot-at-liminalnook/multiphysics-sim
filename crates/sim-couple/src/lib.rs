//! Couplers that carry the seam across a process or socket boundary, so a
//! controller can be written in any language.
//!
//! # The frame protocol
//!
//! Newline-delimited JSON, lockstep. The simulation speaks first:
//!
//! ```text
//! → {"type":"hello","element":"controller","period":0.001,
//!    "sensors":[{"name":"angle","unit":"rad"}],
//!    "actuators":[{"name":"voltage","unit":"V"}]}
//! ← {"type":"ready"}
//! → {"type":"sample","seq":0,"t":0.0,"sensors":[0.1]}
//! ← {"type":"act","seq":0,"actuators":[2.5]}
//! → {"type":"sample","seq":1,"t":0.001,"sensors":[0.09]}
//! ← {"type":"act","seq":1,"actuators":[2.4]}
//! …
//! → {"type":"close"}
//! ```
//!
//! Every `sample` carries the simulation time; the controller never sees a
//! wall clock. A reply whose `seq` does not match, a malformed line, a
//! closed pipe or a reply later than the timeout is a [`CouplerError`],
//! which the runtime reports as an error naming the element.

pub mod environment;
pub use environment::{Environment, Frame, Spaces, serve};

use serde::{Deserialize, Serialize};
use sim_core::{Contract, Coupler, CouplerError};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Named {
    pub name: String,
    pub unit: String,
}

/// Frames the simulation sends.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Outbound {
    Hello { element: String, period: f64, sensors: Vec<Named>, actuators: Vec<Named> },
    Sample { seq: u64, t: f64, sensors: Vec<f64> },
    Close,
}

/// Frames the controller sends back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Inbound {
    Ready,
    Act { seq: u64, actuators: Vec<f64> },
}

impl Outbound {
    pub fn hello(contract: &Contract) -> Self {
        let named = |channels: &[sim_core::Channel]| channels.iter().map(|c| Named { name: c.name.clone(), unit: c.unit().to_owned() }).collect();
        Self::Hello { element: contract.element.clone(), period: contract.period, sensors: named(&contract.sensors), actuators: named(&contract.actuators) }
    }
}

/// A coupler over any byte stream: a writer for outbound frames and a
/// reader thread delivering inbound lines, so a wait can time out.
pub struct FrameCoupler {
    writer: Box<dyn Write + Send>,
    lines: Receiver<std::io::Result<String>>,
    /// How long lockstep waits for a reply before giving up.
    pub timeout: Duration,
    child: Option<Child>,
    seq: u64,
    open: bool,
}

impl FrameCoupler {
    pub fn new(reader: impl Read + Send + 'static, writer: impl Write + Send + 'static) -> Self {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self { writer: Box::new(writer), lines: rx, timeout: Duration::from_secs(10), child: None, seq: 0, open: false }
    }

    /// Spawn `program args…` and speak the protocol over its stdin/stdout;
    /// its stderr is inherited so the controller's own output is visible.
    pub fn spawn(program: &str, args: &[&str]) -> std::io::Result<Self> {
        let mut command = Command::new(program);
        command.args(args);
        Self::spawn_command(command)
    }

    /// Spawn a prepared command (environment, working directory) the same way.
    pub fn spawn_command(mut command: Command) -> std::io::Result<Self> {
        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let mut coupler = Self::new(stdout, stdin);
        coupler.child = Some(child);
        Ok(coupler)
    }

    /// Connect to a controller listening on a TCP address.
    pub fn connect(address: impl ToSocketAddrs) -> std::io::Result<Self> {
        let stream = TcpStream::connect(address)?;
        stream.set_nodelay(true)?;
        Ok(Self::new(stream.try_clone()?, stream))
    }

    /// Listen on a TCP address and accept one controller that connects
    /// (the mirror of [`Self::connect`], for controllers that dial in).
    pub fn accept(address: impl ToSocketAddrs) -> std::io::Result<Self> {
        let listener = std::net::TcpListener::bind(address)?;
        let (stream, _) = listener.accept()?;
        stream.set_nodelay(true)?;
        Ok(Self::new(stream.try_clone()?, stream))
    }

    /// Connect to a controller listening on a Unix socket.
    pub fn connect_unix(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        Ok(Self::new(stream.try_clone()?, stream))
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn send(&mut self, frame: &Outbound) -> Result<(), CouplerError> {
        let mut line = serde_json::to_string(frame).map_err(|e| CouplerError::Other(e.to_string()))?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).and_then(|_| self.writer.flush()).map_err(|e| self.exited(e.to_string()))
    }

    fn receive(&mut self) -> Result<Inbound, CouplerError> {
        match self.lines.recv_timeout(self.timeout) {
            Ok(Ok(line)) => serde_json::from_str(&line).map_err(|e| CouplerError::Malformed(format!("{e} in {line:?}"))),
            Ok(Err(e)) => Err(self.exited(e.to_string())),
            Err(RecvTimeoutError::Timeout) => Err(CouplerError::Timeout(self.timeout.as_secs_f64())),
            Err(RecvTimeoutError::Disconnected) => Err(self.exited("stream closed".to_owned())),
        }
    }

    fn exited(&mut self, detail: String) -> CouplerError {
        let status = self.child.as_mut().and_then(|c| c.try_wait().ok().flatten()).map(|s| format!(" ({s})")).unwrap_or_default();
        CouplerError::Exited(format!("{detail}{status}"))
    }
}

impl Coupler for FrameCoupler {
    fn open(&mut self, contract: &Contract) -> Result<(), CouplerError> {
        self.send(&Outbound::hello(contract))?;
        match self.receive()? {
            Inbound::Ready => {
                self.open = true;
                Ok(())
            }
            other => Err(CouplerError::Malformed(format!("expected ready, got {other:?}"))),
        }
    }

    fn sample(&mut self, t: f64, sensors: &[f64], actuators: &mut [f64]) -> Result<(), CouplerError> {
        let seq = self.seq;
        self.send(&Outbound::Sample { seq, t, sensors: sensors.to_vec() })?;
        match self.receive()? {
            Inbound::Act { seq: got, actuators: values } if got == seq && values.len() == actuators.len() => {
                actuators.copy_from_slice(&values);
                self.seq += 1;
                Ok(())
            }
            Inbound::Act { seq: got, actuators: values } => Err(CouplerError::Malformed(format!("expected seq {seq} with {} actuators, got seq {got} with {}", actuators.len(), values.len()))),
            other => Err(CouplerError::Malformed(format!("expected act, got {other:?}"))),
        }
    }

    fn close(&mut self) {
        if self.open {
            let _ = self.send(&Outbound::Close);
            self.open = false;
        }
        // A controller may have been spawned before plant compilation failed,
        // so no hello/close handshake ever happened. Close stdin before waiting
        // so its first read sees EOF instead of deadlocking this destructor.
        drop(std::mem::replace(&mut self.writer, Box::new(std::io::sink())));
        if let Some(child) = self.child.as_mut() {
            // Also reap controllers that ignore EOF/close. Teardown must not
            // hide the original compilation/protocol failure indefinitely.
            let deadline = Instant::now() + Duration::from_millis(250);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
                    _ => break,
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for FrameCoupler {
    fn drop(&mut self) {
        self.close();
    }
}

/// Spawn a Python controller over the seam: `python3 -u script args…`
/// with `clients/python` (relative to `clients_root`) on `PYTHONPATH`, so
/// `from simloop import Loop` works. Negative-valued flags should be given
/// as `--flag=value`.
pub fn python(clients_root: impl AsRef<std::path::Path>, script: impl AsRef<std::path::Path>, args: &[&str]) -> std::io::Result<FrameCoupler> {
    let dir = clients_root.as_ref().join("python");
    let path = match std::env::var_os("PYTHONPATH") {
        Some(p) => format!("{}:{}", dir.display(), p.to_string_lossy()),
        None => dir.display().to_string(),
    };
    let mut command = Command::new("python3");
    command.arg("-u").arg(script.as_ref()).args(args).env("PYTHONPATH", path);
    FrameCoupler::spawn_command(command)
}

/// Compile (once, when stale) and spawn a C controller written against
/// `clients/c/simloop.h`: `cc -O2 -std=c99 -I clients/c source -lm`.
pub fn c(clients_root: impl AsRef<std::path::Path>, source: impl AsRef<std::path::Path>, binary: impl AsRef<std::path::Path>, args: &[&str]) -> std::io::Result<FrameCoupler> {
    let (source, binary) = (source.as_ref(), binary.as_ref());
    let stale = match (std::fs::metadata(source), std::fs::metadata(binary)) {
        (Ok(s), Ok(b)) => s.modified().ok() > b.modified().ok(),
        _ => true,
    };
    if stale {
        if let Some(parent) = binary.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let include = clients_root.as_ref().join("c");
        let status = Command::new("cc").args(["-O2", "-std=c99", "-I"]).arg(&include).arg("-o").arg(binary).arg(source).arg("-lm").status()?;
        if !status.success() {
            return Err(std::io::Error::other(format!("cc failed to build {}", source.display())));
        }
    }
    FrameCoupler::spawn(binary.to_str().unwrap_or_default(), args)
}

/// A controller loaded as a shared library and called in-process: no
/// pipe, no frames, the same cost as a Rust closure. The library exports
///
/// ```c
/// int simloop_open(const char *element, double period, int n_sensors, int n_actuators); // optional
/// void simloop_sample(double t, const double *sensors, int n_sensors, double *actuators, int n_actuators);
/// void simloop_close(void); // optional
/// ```
///
/// Any language with a C ABI qualifies: C, C++, Zig, Rust, Go.
pub struct DynamicCoupler {
    library: libloading::Library,
}

type SampleFn = unsafe extern "C" fn(f64, *const f64, i32, *mut f64, i32);
type OpenFn = unsafe extern "C" fn(*const std::os::raw::c_char, f64, i32, i32) -> i32;
type CloseFn = unsafe extern "C" fn();

impl DynamicCoupler {
    pub fn load(path: impl AsRef<std::ffi::OsStr>) -> Result<Self, CouplerError> {
        // SAFETY: loading a controller library the user named; its
        // initialisers run with the trust the user gave the path.
        let library = unsafe { libloading::Library::new(path.as_ref()) }.map_err(|e| CouplerError::Other(e.to_string()))?;
        unsafe { library.get::<SampleFn>(b"simloop_sample\0") }.map_err(|e| CouplerError::Other(format!("no simloop_sample: {e}")))?;
        Ok(Self { library })
    }

    /// Compile a C source into a shared library (once, when stale) and load it.
    pub fn compile(source: impl AsRef<std::path::Path>, library: impl AsRef<std::path::Path>) -> Result<Self, CouplerError> {
        let (source, library) = (source.as_ref(), library.as_ref());
        let stale = match (std::fs::metadata(source), std::fs::metadata(library)) {
            (Ok(s), Ok(b)) => s.modified().ok() > b.modified().ok(),
            _ => true,
        };
        if stale {
            if let Some(parent) = library.parent() {
                std::fs::create_dir_all(parent).map_err(|e| CouplerError::Other(e.to_string()))?;
            }
            let status = Command::new("cc").args(["-O2", "-std=c99", "-shared", "-fPIC", "-o"]).arg(library).arg(source).arg("-lm").status().map_err(|e| CouplerError::Other(e.to_string()))?;
            if !status.success() {
                return Err(CouplerError::Other(format!("cc failed to build {}", source.display())));
            }
        }
        Self::load(library)
    }
}

impl DynamicCoupler {
    /// Call an exported `simloop_configure(f64 × 7)` if the library has one.
    pub fn configure(&mut self, stride: f64, period: f64, lift: f64, height: f64, kp: f64, kd: f64, start: f64) -> Result<(), CouplerError> {
        type ConfigureFn = unsafe extern "C" fn(f64, f64, f64, f64, f64, f64, f64);
        let configure = unsafe { self.library.get::<ConfigureFn>(b"simloop_configure\0") }.map_err(|e| CouplerError::Other(e.to_string()))?;
        unsafe { configure(stride, period, lift, height, kp, kd, start) };
        Ok(())
    }
}

impl Coupler for DynamicCoupler {
    fn open(&mut self, contract: &Contract) -> Result<(), CouplerError> {
        if let Ok(open) = unsafe { self.library.get::<OpenFn>(b"simloop_open\0") } {
            let element = std::ffi::CString::new(contract.element.clone()).unwrap_or_default();
            let code = unsafe { open(element.as_ptr(), contract.period, contract.sensors.len() as i32, contract.actuators.len() as i32) };
            if code != 0 {
                return Err(CouplerError::Other(format!("simloop_open returned {code}")));
            }
        }
        Ok(())
    }
    fn sample(&mut self, t: f64, sensors: &[f64], actuators: &mut [f64]) -> Result<(), CouplerError> {
        let sample = unsafe { self.library.get::<SampleFn>(b"simloop_sample\0") }.map_err(|e| CouplerError::Other(e.to_string()))?;
        unsafe { sample(t, sensors.as_ptr(), sensors.len() as i32, actuators.as_mut_ptr(), actuators.len() as i32) };
        Ok(())
    }
    fn close(&mut self) {
        if let Ok(close) = unsafe { self.library.get::<CloseFn>(b"simloop_close\0") } {
            unsafe { close() };
        }
    }
}

/// Wall-clock pacing around any coupler: the controller answers on its own
/// thread, and a sample waits at most `deadline` for the reply. A late
/// controller leaves the previous command held and counts a missed
/// deadline; its answer is applied when it finally arrives, at the next
/// sample. This is real-time mode — for hardware-in-the-loop and for
/// driving a live view from outside — and it is deliberately not
/// deterministic: missed deadlines become physics.
pub struct RealTime {
    requests: Sender<Request>,
    replies: Receiver<Result<Vec<f64>, CouplerError>>,
    pub deadline: Duration,
    missed: Arc<AtomicU64>,
    in_flight: usize,
    worker: Option<std::thread::JoinHandle<()>>,
}

enum Request {
    Open(Contract),
    Sample(f64, Vec<f64>, Vec<f64>),
    Close,
}

impl RealTime {
    pub fn new(mut inner: Box<dyn Coupler>, deadline: Duration) -> Self {
        let (requests, inbox) = channel::<Request>();
        let (outbox, replies) = channel();
        let worker = std::thread::spawn(move || {
            for request in inbox {
                match request {
                    Request::Open(contract) => {
                        let _ = outbox.send(inner.open(&contract).map(|_| Vec::new()));
                    }
                    Request::Sample(t, sensors, mut actuators) => {
                        let result = inner.sample(t, &sensors, &mut actuators).map(|_| actuators);
                        if outbox.send(result).is_err() {
                            break;
                        }
                    }
                    Request::Close => break,
                }
            }
            inner.close();
        });
        Self { requests, replies, deadline, missed: Arc::new(AtomicU64::new(0)), in_flight: 0, worker: Some(worker) }
    }

    /// A handle on the missed-deadline counter, for a viewer or a report.
    pub fn missed(&self) -> Arc<AtomicU64> {
        self.missed.clone()
    }

    fn take(&mut self, reply: Result<Vec<f64>, CouplerError>, actuators: &mut [f64]) -> Result<(), CouplerError> {
        self.in_flight -= 1;
        let values = reply?;
        if values.len() == actuators.len() {
            actuators.copy_from_slice(&values);
        }
        Ok(())
    }
}

impl Coupler for RealTime {
    fn open(&mut self, contract: &Contract) -> Result<(), CouplerError> {
        self.requests.send(Request::Open(contract.clone())).map_err(|_| CouplerError::Exited("controller thread gone".into()))?;
        match self.replies.recv() {
            Ok(result) => result.map(|_| ()),
            Err(_) => Err(CouplerError::Exited("controller thread gone".into())),
        }
    }

    fn sample(&mut self, t: f64, sensors: &[f64], actuators: &mut [f64]) -> Result<(), CouplerError> {
        // Late answers land first: the newest becomes the held command.
        while self.in_flight > 0 {
            match self.replies.try_recv() {
                Ok(reply) => self.take(reply, actuators)?,
                Err(_) => break,
            }
        }
        self.requests.send(Request::Sample(t, sensors.to_vec(), actuators.to_vec())).map_err(|_| CouplerError::Exited("controller thread gone".into()))?;
        self.in_flight += 1;
        // Only a fresh answer counts; an older one arriving now was already late.
        let started = std::time::Instant::now();
        loop {
            let remaining = self.deadline.saturating_sub(started.elapsed());
            match self.replies.recv_timeout(remaining) {
                Ok(reply) => {
                    let fresh = self.in_flight == 1;
                    self.take(reply, actuators)?;
                    if fresh {
                        return Ok(());
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.missed.fetch_add(1, Ordering::Relaxed);
                    return Ok(());
                }
                Err(RecvTimeoutError::Disconnected) => return Err(CouplerError::Exited("controller thread gone".into())),
            }
        }
    }

    fn close(&mut self) {
        let _ = self.requests.send(Request::Close);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for RealTime {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shared_library_controller_is_called_in_process() {
        let dir = std::env::temp_dir().join("simloop-dl-test");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("p.c");
        std::fs::write(&source, "static double gain = 2.0;\nint simloop_open(const char *e, double period, int ns, int na) { (void)e; (void)ns; (void)na; gain = period > 0 ? 2.0 : 0.0; return 0; }\nvoid simloop_sample(double t, const double *s, int ns, double *a, int na) { (void)t; (void)ns; for (int i = 0; i < na; i++) a[i] = -gain * s[i]; }\n").unwrap();
        let mut coupler = DynamicCoupler::compile(&source, dir.join("libp.dylib")).unwrap();
        let contract = Contract { element: "c".into(), period: 0.01, sensors: vec![sim_core::Channel { name: "x".into(), kind: sim_core::QuantityKind::Length }], actuators: vec![sim_core::Channel { name: "f".into(), kind: sim_core::QuantityKind::Force }] };
        coupler.open(&contract).unwrap();
        let mut act = [0.0];
        coupler.sample(0.0, &[0.5], &mut act).unwrap();
        assert_eq!(act, [-1.0]);
    }

    #[test]
    fn a_slow_controller_misses_deadlines_and_holds() {
        let slow = sim_core::FnCoupler(|_t: f64, s: &[f64], a: &mut [f64]| {
            std::thread::sleep(Duration::from_millis(30));
            a[0] = s[0];
        });
        let mut rt = RealTime::new(Box::new(slow), Duration::from_millis(5));
        let missed = rt.missed();
        let mut act = [0.0];
        rt.sample(0.0, &[1.0], &mut act).unwrap();
        assert_eq!(act, [0.0], "held while the controller is late");
        assert_eq!(missed.load(Ordering::Relaxed), 1);
        std::thread::sleep(Duration::from_millis(40));
        rt.sample(0.01, &[2.0], &mut act).unwrap();
        assert_eq!(act, [1.0], "the late answer lands at the next sample");
        assert_eq!(missed.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_fast_controller_never_misses() {
        let fast = sim_core::FnCoupler(|_t: f64, s: &[f64], a: &mut [f64]| a[0] = 2.0 * s[0]);
        let mut rt = RealTime::new(Box::new(fast), Duration::from_millis(50));
        let mut act = [0.0];
        for k in 0..20 {
            rt.sample(k as f64, &[k as f64], &mut act).unwrap();
            assert_eq!(act, [2.0 * k as f64]);
        }
        assert_eq!(rt.missed().load(Ordering::Relaxed), 0);
    }

    #[test]
    fn frames_round_trip() {
        let frame = Outbound::Sample { seq: 3, t: 0.25, sensors: vec![1.0, -2.5] };
        let text = serde_json::to_string(&frame).unwrap();
        assert_eq!(text, r#"{"type":"sample","seq":3,"t":0.25,"sensors":[1.0,-2.5]}"#);
        let back: Inbound = serde_json::from_str(r#"{"type":"act","seq":3,"actuators":[0.5]}"#).unwrap();
        assert_eq!(back, Inbound::Act { seq: 3, actuators: vec![0.5] });
    }

    #[test]
    fn a_process_controller_answers_in_lockstep() {
        // A proportional law written in the shell's Python, one line per frame.
        let script = r#"
import json, sys
hello = json.loads(sys.stdin.readline()); print(json.dumps({"type": "ready"}), flush=True)
for line in sys.stdin:
    f = json.loads(line)
    if f["type"] == "close": break
    print(json.dumps({"type": "act", "seq": f["seq"], "actuators": [-2.0 * f["sensors"][0]]}), flush=True)
"#;
        let mut coupler = FrameCoupler::spawn("python3", &["-c", script]).unwrap();
        let contract = Contract { element: "c".into(), period: 0.01, sensors: vec![sim_core::Channel { name: "x".into(), kind: sim_core::QuantityKind::Length }], actuators: vec![sim_core::Channel { name: "f".into(), kind: sim_core::QuantityKind::Force }] };
        coupler.open(&contract).unwrap();
        let mut act = [0.0];
        coupler.sample(0.0, &[0.5], &mut act).unwrap();
        assert_eq!(act, [-1.0]);
        coupler.sample(0.01, &[-0.25], &mut act).unwrap();
        assert_eq!(act, [0.5]);
        coupler.close();
    }

    #[test]
    fn a_dead_controller_is_an_error_not_a_hold() {
        let mut coupler = FrameCoupler::spawn("python3", &["-c", "import sys; sys.stdin.readline(); sys.exit(3)"]).unwrap();
        let contract = Contract { element: "c".into(), period: 0.01, sensors: vec![], actuators: vec![] };
        let err = coupler.open(&contract).unwrap_err();
        assert!(matches!(err, CouplerError::Exited(_)), "{err}");
    }

    #[test]
    fn an_unopened_controller_gets_eof_and_is_reaped() {
        let mut coupler = FrameCoupler::spawn("python3", &["-c", "import sys; assert sys.stdin.read() == ''"]).unwrap();
        let start = Instant::now();
        coupler.close();
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(coupler.child.as_mut().unwrap().try_wait().unwrap().unwrap().success());
        coupler.close(); // Explicit cleanup followed by Drop is harmless.
    }

    #[test]
    fn a_controller_ignoring_close_is_terminated_and_reaped() {
        let script = "import sys, time; sys.stdin.readline(); print('{\"type\":\"ready\"}', flush=True); time.sleep(60)";
        let mut coupler = FrameCoupler::spawn("python3", &["-c", script]).unwrap();
        coupler.open(&Contract { element: "c".into(), period: 0.01, sensors: vec![], actuators: vec![] }).unwrap();
        let start = Instant::now();
        coupler.close();
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(!coupler.child.as_mut().unwrap().try_wait().unwrap().unwrap().success());
    }
}
