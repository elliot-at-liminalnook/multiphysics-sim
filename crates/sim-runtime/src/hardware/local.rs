//! In-process application routing. The caller owns and runs every worker.
//! Route names retain browser vocabulary; no socket, token or process is involved.
use super::{bench as application_bench, calibration as application_calibration, protocol};
use protocol::{Body, ClientError, calibration};
pub use protocol::{CONNECT_TIMEOUT, STOP_TIMEOUT, next_connection_generation};
use serde_json::Value;
use std::{
    path::Path,
    sync::{Arc, mpsc},
    time::Duration,
};
#[derive(Clone, Copy, Debug)]
pub enum ServiceKind {
    Calibration,
    MotorBench,
}
#[derive(Clone)]
pub struct LocalClient {
    backend: Arc<Backend>,
    owner: String,
    pub timeout: Duration,
    pub calibration_execution: Option<(calibration::ExecutionIdentity, u64)>,
}
enum Backend {
    Calibration(application_calibration::Handle),
    Bench {
        app: Arc<application_bench::App>,
        work: mpsc::Sender<application_bench::Work>,
    },
}
impl Drop for Backend {
    fn drop(&mut self) {
        match self {
            Self::Calibration(handle) => handle.shutdown(),
            Self::Bench { app, .. } => app.latch_stop(),
        }
    }
}
pub enum Worker {
    Calibration(application_calibration::Worker),
    Bench(mpsc::Receiver<application_bench::Work>),
}
impl Worker {
    pub fn run(self) {
        match self {
            Self::Calibration(worker) => worker.run(),
            Self::Bench(queue) => {
                while let Ok(work) = queue.recv() {
                    let _ = work.run();
                }
            }
        }
    }
}
fn refusal(error: String) -> ClientError {
    ClientError::Server {
        status: if error.starts_with(calibration::BINDING_REFUSED) {
            calibration::BINDING_REFUSED_STATUS
        } else {
            400
        },
        error,
    }
}
impl LocalClient {
    pub fn open(path: &Path, kind: ServiceKind) -> Result<(Self, Worker), String> {
        let (backend, worker) = match kind {
            ServiceKind::Calibration => {
                let (handle, worker) = application_calibration::Service::open_config(path)?;
                (Backend::Calibration(handle), Worker::Calibration(worker))
            }
            ServiceKind::MotorBench => {
                let config = serde_json::from_slice(
                    &std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?,
                )
                .map_err(|e| format!("{}: {e}", path.display()))?;
                let app = application_bench::App::open(config)?;
                let (tx, rx) = mpsc::channel();
                (Backend::Bench { app, work: tx }, Worker::Bench(rx))
            }
        };
        Ok((
            Self {
                backend: Arc::new(backend),
                owner: protocol::process_client_id().into(),
                timeout: protocol::REQUEST_TIMEOUT,
                calibration_execution: None,
            },
            worker,
        ))
    }
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout.max(Duration::from_millis(1));
        self
    }
    pub fn with_calibration_execution(
        mut self,
        identity: calibration::ExecutionIdentity,
        generation: u64,
    ) -> Self {
        self.calibration_execution = Some((identity, generation));
        self
    }
    pub fn stop_immediate(&self) {
        match self.backend.as_ref() {
            Backend::Calibration(handle) => handle.stop_immediate(),
            Backend::Bench { app, .. } => app.latch_stop(),
        }
    }
    pub fn get(&self, path: &str) -> Result<Value, ClientError> {
        match self.backend.as_ref() {
            Backend::Calibration(handle) => handle.get(path).map_err(refusal),
            Backend::Bench { app, .. } => match path {
                protocol::bench::CONFIG => Ok(app.config()),
                protocol::bench::STATUS => Ok(app.status(&self.owner)),
                _ => Err(refusal("Unknown local bench operation".into())),
            },
        }
    }
    pub fn calibration_stop_epoch(&self) -> u64 {
        match self.backend.as_ref() {
            Backend::Calibration(handle) => handle.stop_epoch(),
            _ => 0,
        }
    }
    /// Stamp queued native live opens against the authoritative bench STOP epoch.
    pub fn bench_stop_epoch(&self) -> u64 {
        match self.backend.as_ref() {
            Backend::Bench { app, .. } => app.stop_epoch(),
            _ => 0,
        }
    }
    pub fn post_at_epoch(
        &self,
        path: &str,
        body: &Body,
        expected_stop_epoch: u64,
    ) -> Result<Value, ClientError> {
        self.post_with_epoch(path, body, Some(expected_stop_epoch))
    }
    pub fn post(&self, path: &str, body: &Body) -> Result<Value, ClientError> {
        self.post_with_epoch(path, body, None)
    }
    fn post_with_epoch(
        &self,
        path: &str,
        body: &Body,
        expected_stop_epoch: Option<u64>,
    ) -> Result<Value, ClientError> {
        let limit = match self.backend.as_ref() {
            Backend::Calibration(_) => protocol::CALIBRATION_MAX_BODY,
            Backend::Bench { .. } => protocol::MOTOR_BENCH_MAX_BODY,
        };
        let length = body.text().len();
        if length > limit {
            return Err(ClientError::Transport(format!(
                "request body of {length} bytes exceeds the local application's {limit}-byte limit"
            )));
        }
        match self.backend.as_ref() {
            Backend::Calibration(handle) => {
                if path != calibration::COMMAND {
                    return Err(refusal("Unknown local calibration operation".into()));
                }
                handle
                    .command_at_epoch(
                        &self.owner,
                        body.to_value(),
                        self.calibration_execution.clone(),
                        expected_stop_epoch.unwrap_or_else(|| handle.stop_epoch()),
                    )
                    .map_err(refusal)
            }
            Backend::Bench { app, work } => match path {
                "/inspect" => {
                    if !body.0.is_empty() {return Err(refusal("Inspection takes an empty request body".into()));}
                    let acquisition=app.start_at_epoch(None,&self.owner,None,expected_stop_epoch.unwrap_or_else(||app.stop_epoch())).map_err(refusal)?;
                    work.send(acquisition).map_err(|_|ClientError::Transport("local acquisition worker ended".into()))?;
                    Ok(app.status(&self.owner))
                }
                protocol::bench::STOP => app.stop().map_err(refusal),
                protocol::bench::LIVE_SAMPLE => {
                    let sample = serde_json::from_value(body.to_value())
                        .map_err(|e| refusal(format!("live sample: {e}")))?;
                    app.sample(&self.owner, &sample).map_err(refusal)
                }
                protocol::bench::LIVE_OPEN => {
                    let request = serde_json::from_value(body.to_value())
                        .map_err(|e| refusal(format!("live request: {e}")))?;
                    let acquisition = app
                        .start_at_epoch(
                            None,
                            &self.owner,
                            Some(request),
                            expected_stop_epoch.unwrap_or_else(|| app.stop_epoch()),
                        )
                        .map_err(refusal)?;
                    work.send(acquisition).map_err(|_| {
                        ClientError::Transport("local acquisition worker ended".into())
                    })?;
                    Ok(app.status(&self.owner))
                }
                _ => Err(refusal("Unknown local bench operation".into())),
            },
        }
    }
    /// Exit/handle-drop control only. This latches cancellation without waiting
    /// for acquisition or pretending that stationary readback was observed.
    pub fn send_only(&self, path: &str, body: &Body) -> Result<(), ClientError> {
        match self.backend.as_ref() {
            Backend::Calibration(handle)
                if path == calibration::COMMAND && body.to_value()["action"] == "stop" =>
            {
                handle.stop_immediate();
                Ok(())
            }
            Backend::Bench { app, .. } if path == protocol::bench::STOP => {
                app.latch_stop();
                Ok(())
            }
            _ => Err(refusal(
                "Only immediate STOP is valid on the local exit path".into(),
            )),
        }
    }
}
