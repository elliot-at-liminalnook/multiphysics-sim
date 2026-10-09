//! Bounded native process transport for the shared systems session.
//! UI hosts own the child process, so cancellation does not depend on a solver.
use crate::system_session::{
    Command, ModelSource, Phase, Reply, SessionStatus, SystemSession,
};
use serde::{Deserialize, Serialize};
use sim_core::BehaviorRegistry;
use sim_inspect::SampleFrame;
use std::{
    io::{self, BufRead, Write},
    time::{Duration, Instant},
};

pub const MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;
const RECORD_BUDGET: usize = 16 * 1024 * 1024;
pub use crate::system_launch::{Launch, SourceBinding};
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Launch {
        launch: Launch,
    },
    Command {
        id: u64,
        generation: u64,
        command: Command,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Building,
    Ready {
        reply: Reply,
    },
    Reply {
        id: u64,
        reply: Reply,
    },
    Frame {
        status: SessionStatus,
        frame: SampleFrame,
    },
    Error {
        id: Option<u64>,
        status: Option<SessionStatus>,
        message: String,
    },
    Exited {
        code: Option<i32>,
    },
}
fn read_message<R: BufRead, T: serde::de::DeserializeOwned>(
    reader: &mut R,
) -> Result<Option<T>, String> {
    let mut bytes = Vec::new();
    let n = std::io::Read::take(&mut *reader, (MAX_MESSAGE_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Ok(None);
    }
    if n > MAX_MESSAGE_BYTES || bytes.last() != Some(&b'\n') {
        return Err("worker message exceeds limit or is incomplete".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| e.to_string())
}
fn write_message<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() + 1 > MAX_MESSAGE_BYTES {
        return Err("worker message exceeds limit".into());
    }
    writer
        .write_all(&bytes)
        .and_then(|_| writer.write_all(b"\n"))
        .and_then(|_| writer.flush())
        .map_err(|e| e.to_string())
}
/// Run on a process's main thread. Hosts may register additional domain crates
/// before calling this function; the transport contains no domain cases.
pub fn serve(registry: BehaviorRegistry) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    std::thread::spawn(move || {
        let mut input = io::BufReader::new(io::stdin());
        loop {
            match read_message::<_, Request>(&mut input) {
                Ok(Some(request)) => {
                    if tx.send(Ok(request)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = tx.send(Err(error));
                    break;
                }
            }
        }
    });
    let mut output = io::BufWriter::new(io::stdout());
    let launch = match rx.recv().map_err(|_| "worker closed before launch")?? {
        Request::Launch { launch } => launch,
        _ => return Err("first worker request must be launch".into()),
    };
    write_message(&mut output, &Event::Building)?;
    if launch.version != 1 {
        return Err("unsupported worker launch version".into());
    }
    if let Err(message) = launch.validate_binding(&registry) {
        write_message(
            &mut output,
            &Event::Error {
                id: None,
                status: None,
                message,
            },
        )?;
        return Ok(());
    }
    let source = ModelSource {
        model: launch.model,
        registry,
        identities: launch.binding.map(|b| b.identities).unwrap_or_default(),
        source_hash: launch.source_hash,
        revision: launch.revision,
        base: launch.base.map(std::path::PathBuf::from),
        drive: None,
    };
    let mut session =
        match SystemSession::new(launch.run_id, launch.config, move |c| source.build(c)) {
            Ok(s) => s,
            Err(message) => {
                write_message(
                    &mut output,
                    &Event::Error {
                        id: None,
                        status: None,
                        message,
                    },
                )?;
                return Ok(());
            }
        };
    let description_size = serde_json::to_vec(session.description())
        .map_err(|e| e.to_string())?
        .len();
    write_message(
        &mut output,
        &Event::Ready {
            reply: session.execute(Command::Describe)?,
        },
    )?;
    let mut run_clock = Instant::now();
    let mut run_time = 0.;
    let mut sent = Instant::now();
    loop {
        let running = session.status().phase == Phase::Running;
        let due = running && session.status().time <= run_time + run_clock.elapsed().as_secs_f64();
        let request = if due {
            rx.try_recv().map(Some).or_else(|e| match e {
                std::sync::mpsc::TryRecvError::Empty => Ok(None),
                std::sync::mpsc::TryRecvError::Disconnected => Err(()),
            })
        } else {
            rx.recv_timeout(Duration::from_millis(5))
                .map(Some)
                .or_else(|e| match e {
                    std::sync::mpsc::RecvTimeoutError::Timeout => Ok(None),
                    std::sync::mpsc::RecvTimeoutError::Disconnected => Err(()),
                })
        };
        match request {
            Err(()) => return Ok(()),
            Ok(Some(Err(message))) => {
                write_message(
                    &mut output,
                    &Event::Error {
                        id: None,
                        status: Some(session.status().clone()),
                        message,
                    },
                )?;
                return Ok(());
            }
            Ok(Some(Ok(Request::Launch { .. }))) => write_message(
                &mut output,
                &Event::Error {
                    id: None,
                    status: Some(session.status().clone()),
                    message: "worker is already initialized".into(),
                },
            )?,
            Ok(Some(Ok(Request::Command {
                id,
                generation,
                command,
            }))) => {
                let validation = if generation != session.status().generation {
                    Err("stale command generation".into())
                } else if let Command::BeginRecording {
                    observables,
                    capacity,
                } = &command
                {
                    // Conservative JSON bound including escaped IDs, numeric/stage
                    // fields and unavailable reasons. Reject before recording starts.
                    let per_frame = observables.iter().try_fold(1024usize, |n, id| {
                        n.checked_add(id.len().checked_mul(6)?.checked_add(512)?)
                    });
                    if per_frame
                        .and_then(|n| n.checked_mul(*capacity))
                        .and_then(|n| n.checked_add(description_size))
                        .is_none_or(|n| n > RECORD_BUDGET)
                    {
                        Err("recording exceeds this transport's 16 MiB budget; choose fewer observables or frames".into())
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                };
                let was_running = session.status().phase == Phase::Running;
                let result = validation.and_then(|_| session.execute(command));
                match result {
                    Ok(reply) => {
                        if reply.status.phase == Phase::Running && !was_running {
                            run_clock = Instant::now();
                            run_time = reply.status.time;
                        }
                        write_message(&mut output, &Event::Reply { id, reply })?;
                    }
                    Err(message) => write_message(
                        &mut output,
                        &Event::Error {
                            id: Some(id),
                            status: Some(session.status().clone()),
                            message,
                        },
                    )?,
                }
            }
            Ok(None) if due => match session.tick() {
                Ok(_) if sent.elapsed() >= Duration::from_millis(33) => {
                    write_message(
                        &mut output,
                        &Event::Frame {
                            status: session.status().clone(),
                            frame: session.latest().clone(),
                        },
                    )?;
                    sent = Instant::now();
                }
                Ok(_) => {}
                Err(message) => write_message(
                    &mut output,
                    &Event::Error {
                        id: None,
                        status: Some(session.status().clone()),
                        message,
                    },
                )?,
            },
            Ok(None) => {}
        }
    }
}

/// Bounded command/reply channels plus one coalesced display frame. Neither pipe
/// reads/writes nor child reaping run on the caller's UI thread.
pub struct Client {
    child: Option<std::process::Child>,
    tx: std::sync::mpsc::SyncSender<Request>,
    rx: std::sync::mpsc::Receiver<Event>,
    latest: std::sync::Arc<std::sync::Mutex<Option<Event>>>,
    next_id: u64,
}
impl Client {
    pub fn spawn(
        executable: &std::path::Path,
        arguments: &[&str],
        launch: Launch,
    ) -> Result<Self, String> {
        use std::{
            process::Stdio,
            sync::{Arc, Mutex, mpsc},
        };
        let mut child = std::process::Command::new(executable)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| e.to_string())?;
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, requests) = mpsc::sync_channel(8);
        let (events, rx) = mpsc::sync_channel(16);
        let latest = Arc::new(Mutex::new(None));
        let display = latest.clone();
        let failures = events.clone();
        std::thread::spawn(move || {
            for request in requests {
                if let Err(message) = write_message(&mut stdin, &request) {
                    let _ = failures.send(Event::Error {
                        id: None,
                        status: None,
                        message,
                    });
                    break;
                }
            }
        });
        std::thread::spawn(move || {
            let mut reader = io::BufReader::new(stdout);
            loop {
                match read_message::<_, Event>(&mut reader) {
                    Ok(Some(event @ Event::Frame { .. })) => *display.lock().unwrap() = Some(event),
                    Ok(Some(event)) => {
                        if events.send(event).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(message) => {
                        let _ = events.send(Event::Error {
                            id: None,
                            status: None,
                            message,
                        });
                        break;
                    }
                }
            }
        });
        tx.try_send(Request::Launch { launch })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            child: Some(child),
            tx,
            rx,
            latest,
            next_id: 1,
        })
    }
    pub fn command(&mut self, generation: u64, command: Command) -> Result<u64, String> {
        let id = self.next_id;
        let next = id.checked_add(1).ok_or("worker request IDs exhausted")?;
        self.tx
            .try_send(Request::Command {
                id,
                generation,
                command,
            })
            .map_err(|e| e.to_string())?;
        self.next_id = next;
        Ok(id)
    }
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events: Vec<_> = self.rx.try_iter().take(16).collect();
        if let Some(frame) = self.latest.lock().unwrap().take() {
            events.push(frame);
        }
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    events.push(Event::Exited {
                        code: status.code(),
                    });
                    self.child = None;
                }
                Ok(None) => {}
                Err(error) => events.push(Event::Error {
                    id: None,
                    status: None,
                    message: error.to_string(),
                }),
            }
        }
        events
    }
    pub fn terminate(&mut self) -> Result<(), String> {
        if let Some(child) = &mut self.child {
            child.kill().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}
