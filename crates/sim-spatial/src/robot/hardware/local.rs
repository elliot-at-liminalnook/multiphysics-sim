//! Viewer lifetime adapter: the shared application supplies a worker; jobs owns its thread.
use serde_json::Value;
pub(crate) use sim_runtime::hardware::local::ServiceKind;
pub(crate) use sim_runtime::hardware::protocol::{
    Body, CONNECT_TIMEOUT, ClientError, STOP_TIMEOUT, bench,
};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct Client {
    endpoint: Endpoint,
    // A driver survives all link, beat, export and sync clones. The endpoint is
    // dropped first, latching STOP/shutdown before the driver handle disappears.
    _driver: Option<Arc<crate::jobs::RunThread<(), ()>>>,
    pub calibration_execution: Option<(
        sim_runtime::hardware::protocol::calibration::ExecutionIdentity,
        u64,
    )>,
    pub timeout: Duration,
}
#[derive(Clone)]
enum Endpoint {
    Local(sim_runtime::hardware::local::LocalClient),
    #[cfg(test)]
    Fixture(Arc<dyn Fn(&str, &Value) -> Result<Value, ClientError> + Send + Sync>),
}
impl Client {
    pub fn open(path: &std::path::Path, kind: ServiceKind) -> Result<Self, String> {
        let (endpoint, worker) = sim_runtime::hardware::local::LocalClient::open(path, kind)?;
        let driver =
            crate::jobs::RunThread::spawn("hardware-acquisition", (), move |_, _| worker.run())
                .join_bound(Duration::ZERO);
        Ok(Self {
            endpoint: Endpoint::Local(endpoint),
            _driver: Some(Arc::new(driver)),
            calibration_execution: None,
            timeout: sim_runtime::hardware::protocol::REQUEST_TIMEOUT,
        })
    }
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout.max(Duration::from_millis(1));
        self
    }
    pub fn with_calibration_execution(
        mut self,
        identity: sim_runtime::hardware::protocol::calibration::ExecutionIdentity,
        generation: u64,
    ) -> Self {
        self.calibration_execution = Some((identity.clone(), generation));
        if let Endpoint::Local(endpoint) = &mut self.endpoint {
            *endpoint = endpoint
                .clone()
                .with_calibration_execution(identity, generation);
        }
        self
    }
    pub fn get(&self, path: &str) -> Result<Value, ClientError> {
        match &self.endpoint {
            Endpoint::Local(e) => e.clone().with_timeout(self.timeout).get(path),
            #[cfg(test)]
            Endpoint::Fixture(f) => f(path, &Value::Null),
        }
    }
    pub fn get_as<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        serde_json::from_value(self.get(path)?).map_err(|e| ClientError::Decode(e.to_string()))
    }
    pub fn post(&self, path: &str, body: &Body) -> Result<Value, ClientError> {
        match &self.endpoint {
            Endpoint::Local(e) => e.clone().with_timeout(self.timeout).post(path, body),
            #[cfg(test)]
            Endpoint::Fixture(f) => f(path, &body.to_value()),
        }
    }
    pub fn calibration_stop_epoch(&self) -> u64 {
        match &self.endpoint {
            Endpoint::Local(e) => e.calibration_stop_epoch(),
            #[cfg(test)]
            Endpoint::Fixture(_) => 0,
        }
    }
    pub fn bench_stop_epoch(&self) -> u64 {
        match &self.endpoint {
            Endpoint::Local(e) => e.bench_stop_epoch(),
            #[cfg(test)]
            Endpoint::Fixture(_) => 0,
        }
    }
    pub fn post_at_epoch(&self, path: &str, body: &Body, epoch: u64) -> Result<Value, ClientError> {
        match &self.endpoint {
            Endpoint::Local(e) => e.post_at_epoch(path, body, epoch),
            #[cfg(test)]
            Endpoint::Fixture(f) => f(path, &body.to_value()),
        }
    }
    /// Immediate atomic STOP, independent of the ordinary worker queue.
    pub fn latch_stop(&self) {
        if let Endpoint::Local(e) = &self.endpoint {
            e.stop_immediate();
        }
    }
    pub fn send_only(&self, path: &str, body: &Body) -> Result<(), ClientError> {
        match &self.endpoint {
            Endpoint::Local(e) => e.send_only(path, body),
            #[cfg(test)]
            Endpoint::Fixture(f) => f(path, &body.to_value()).map(|_| ()),
        }
    }
    #[cfg(test)]
    pub fn fixture(
        handler: Arc<dyn Fn(&str, &Value) -> Result<Value, ClientError> + Send + Sync>,
    ) -> Self {
        Self {
            endpoint: Endpoint::Fixture(handler),
            _driver: None,
            calibration_execution: None,
            timeout: Duration::from_secs(2),
        }
    }
}
