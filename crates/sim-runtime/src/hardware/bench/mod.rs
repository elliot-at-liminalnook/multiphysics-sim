//! Shared bench application authority; callers own the worker and transport adapter.
use crate::controller_refinement::live_reference::{
    Cursor, Received, Request as LiveRequest, Sample as LiveSample,
};
use crate::controller_refinement::{
    fpga::Plan,
    trajectory_binding::{Playback, ReferenceTrace},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub port: String,
    pub ids: Vec<u8>,
    pub trace: PathBuf,
    pub template: PathBuf,
    #[serde(default)]
    acquisition: Option<PathBuf>,
    pub output: PathBuf,
    pub inspection: PathBuf,
    #[serde(default)]
    pub walking_root: Option<PathBuf>,
    #[serde(default)]
    pub streamed_only: bool,
    #[serde(default)]
    pub virtual_bench: Option<VirtualConfig>,
}
pub struct App {
    config: Config,
    trace: ReferenceTrace,
    template: Plan,
    signals: Mutex<Option<Arc<Signals>>>,
    epoch: Arc<std::sync::atomic::AtomicU64>,
    state: Mutex<State>,
}
struct State {
    active: bool,
    release_uncertain: bool,
    lease: Instant,
    owner: String,
    run: Option<PathBuf>,
    request: Value,
    plan: Value,
    result: Value,
    inspection: Value,
    live_cursor: Option<Cursor>,
    live_request: Option<LiveRequest>,
}
fn value(path: impl AsRef<std::path::Path>) -> Value {
    fs::read(path)
        .ok()
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or(Value::Null)
}
fn lines(path: PathBuf) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}
fn write_json(path: impl AsRef<std::path::Path>, v: &impl serde::Serialize) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn status(app: &App, client: &str) -> Value {
    app.heartbeat(client);
    let mut s = app.state.lock().unwrap();
    if s.owner == client {
        s.lease = Instant::now();
    }
    let live = s
        .run
        .as_ref()
        .map(|p| lines(p.join("capture/live-telemetry.jsonl")))
        .unwrap_or_default();
    json!({"active":s.active,"run":s.run,"request":s.request,"plan":s.plan,"result":s.result,"inspection":s.inspection,"samples":live})
}
fn prepare(
    app: &Arc<App>,
    request: Option<Playback>,
    client: &str,
    live: Option<LiveRequest>,
    epoch: u64,
) -> Result<Work, String> {
    validate_owner(client)?;
    if epoch != app.stop_epoch() {
        return Err("Queued acquisition predates STOP; start a fresh session".into());
    }
    if request.is_some() && app.config.streamed_only {
        return Err(
            "This FPGA profile uses live targets. Open /walking/ to run the motors.".into(),
        );
    }
    let mut plan = request
        .as_ref()
        .map(|r| app.trace.bind_bench_clip(r, &app.template, &app.config.ids))
        .transpose()?;
    if let Some(live) = &live {
        live.validate(&app.config.ids)?;
        if live
            .bindings
            .iter()
            .any(|b| !app.trace.coordinates.contains(&b.coordinate))
        {
            return Err("Unknown CAD coordinate".into());
        }
        let mut p = app.template.clone();
        p.ids = live.bindings.iter().map(|b| b.motor_id).collect();
        p.ids.sort_unstable();
        p.name = format!("Live 100 Hz FPGA WASD references: {}", live.source);
        p.control = "fpga_device_pd".into();
        p.role = "timing".into();
        p.period_s = 0.01;
        p.gains.limit = 100;
        p.targets = vec![[0; 9]; 1200];
        p.validate()?;
        plan = Some(p);
    }
    let mut s = app.state.lock().unwrap();
    if s.active {
        return Err("A capture already owns the serial port".into());
    }
    if (request.is_some() || live.is_some()) && s.inspection["completed"] != true {
        return Err("Inspect connected hardware before starting".into());
    }
    if (request.is_some() || live.is_some()) && s.release_uncertain {
        return Err("Previous physical stop was not verified; inspect before rearming".into());
    }
    if (request.is_some() || live.is_some())
        && s.result["mode"]
            .as_str()
            .is_some_and(|m| m.starts_with("fpga"))
        && s.result["result"]["stop_verified"] != true
    {
        return Err("Previous physical stop was not verified; inspect before rearming".into());
    }
    let lease = crate::hardware::ownership::DeviceLease::acquire(&app.device_key(), client)?;
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = app.config.output.join(format!("session-{id}"));
    fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let mut command = plan
        .as_ref()
        .map(|p| serde_json::to_value(p).unwrap())
        .unwrap_or(json!({"control":"inspect"}));
    if let Some(live) = &live {
        command = json!({"control":"fpga_live_device_reference","request":live,"plan":command});
        write_json(
            dir.join("latest-reference.json"),
            &Received {
                session: dir.file_name().unwrap().to_string_lossy().into(),
                received_unix_s: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs_f64(),
                sample: live.initial.clone(),
            },
        )?;
    }
    write_json(dir.join("plan.json"), &command)?;
    write_json(
        dir.join("binding.json"),
        &if let Some(live) = &live {
            json!({"request":live,"source":live.source,"period_s":0.01,"maximum_session_s":12.0,"note":"Current live Rust/WASM references; relative bench scaling and measured homes. FPGA schedules feedback and control; the host queues bounded target segments. Initial 160 ms hold; live targets buffered approximately 80 to 160 ms. No predetermined gait playback."})
        } else {
            json!({"request":request,"source":app.trace.source,"source_sha256":app.trace.source_sha256,"trace_blake3":blake3::hash(&serde_json::to_vec(&app.trace).unwrap()).to_hex().to_string(),"note":"Relative to measured home. Final 200 ms returns to home. Reference playback, not live quadruped simulation."})
        },
    )?;
    let signals = Arc::new(Signals::new(
        request.is_some() || live.is_some(),
        Arc::clone(&app.epoch),
        epoch,
    ));
    *app.signals.lock().unwrap() = Some(Arc::clone(&signals));
    s.live_cursor = live.as_ref().map(|r| Cursor::new(&r.initial));
    s.live_request = live.clone();
    s.active = true;
    s.lease = Instant::now();
    s.owner = client.to_owned();
    s.run = Some(dir.clone());
    s.request = serde_json::to_value(&request).unwrap();
    s.plan = command;
    s.result = Value::Null;
    drop(s);
    Ok(Work {
        app: Arc::clone(app),
        dir,
        motion: request.is_some() || live.is_some(),
        lease: Some(lease),
        signals,
        executed: false,
        finished: false,
    })
}
fn request_capture_stop(dir: &std::path::Path, reason: &[u8]) -> std::io::Result<()> {
    // The acquisition worker exclusively creates capture/. Never race it by creating that
    // directory from STOP; retain an early cancellation in the session root.
    fs::write(dir.join("STOP"), reason)?;
    if dir.join("capture").is_dir() {
        fs::write(dir.join("capture/STOP"), reason)?;
    }
    Ok(())
}

/// Safety state is independent of the serialized acquisition worker and its channel.
pub(super) struct Signals {
    cancel: std::sync::atomic::AtomicBool,
    heartbeat: Mutex<Instant>,
    motion: bool,
    epoch: Arc<std::sync::atomic::AtomicU64>,
    bound_epoch: u64,
}
impl Signals {
    fn new(motion: bool, epoch: Arc<std::sync::atomic::AtomicU64>, bound_epoch: u64) -> Self {
        Self {
            cancel: std::sync::atomic::AtomicBool::new(false),
            heartbeat: Mutex::new(Instant::now()),
            motion,
            epoch,
            bound_epoch,
        }
    }
    pub(super) fn stop(&self) {
        self.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    pub(super) fn cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::SeqCst)
            || self.epoch.load(std::sync::atomic::Ordering::SeqCst) != self.bound_epoch
    }
    pub(super) fn expired(&self) -> bool {
        self.motion && self.heartbeat.lock().unwrap().elapsed() > Duration::from_millis(900)
    }
    fn alive(&self) -> bool {
        let heartbeat = self.heartbeat.lock().unwrap();
        if self.cancelled() || (self.motion && heartbeat.elapsed() > Duration::from_millis(900)) {
            self.stop();
            false
        } else {
            true
        }
    }
    fn renew(&self) -> bool {
        let mut heartbeat = self.heartbeat.lock().unwrap();
        if self.cancelled() || (self.motion && heartbeat.elapsed() > Duration::from_millis(900)) {
            self.stop();
            return false;
        }
        *heartbeat = Instant::now();
        true
    }
}
/// An exclusive finite capture, executed by a worker owned by the caller.
pub struct Work {
    app: Arc<App>,
    dir: PathBuf,
    motion: bool,
    lease: Option<crate::hardware::ownership::DeviceLease>,
    signals: Arc<Signals>,
    executed: bool,
    finished: bool,
}
impl Work {
    pub fn run(mut self) -> Result<Value, String> {
        self.executed = true;
        let result = if let Some(config) = &self.app.config.virtual_bench {
            virtual_run::run(
                config,
                &self.app.config.ids,
                &self.dir,
                Arc::clone(&self.signals),
            )
        } else {
            acquisition::run(
                &self.app.config.port,
                &self.app.config.ids,
                &self.dir.join("capture"),
                &self.dir.join("plan.json"),
                Arc::clone(&self.signals),
            )
        };
        let mut record = value(self.dir.join("capture/run.json"));
        if record.is_null() {
            record = json!({"completed":false,"error":result.as_ref().err().cloned().unwrap_or_else(||"Acquisition ended without a result".into())});
        }
        record["in_process_acquisition_success"] = json!(result.is_ok());
        if let Err(error) = &result {
            record["completed"] = json!(false);
            if record["error"].is_null() {
                record["error"] = json!(error);
            }
        }
        if let Err(error) = write_json(self.dir.join("application-result.json"), &record) {
            record["completed"] = json!(false);
            record["publication_error"] = json!(error);
        }

        {
            let mut state = self.app.state.lock().unwrap();
            if !self.motion {
                state.inspection = record.clone();
            }
            if self.motion {
                state.release_uncertain = record["result"]["stop_verified"] != true;
            } else if record["completed"] == true {
                let physical_stopped =
                    record["result"]["devices"]
                        .as_object()
                        .is_some_and(|devices| {
                            !devices.is_empty()
                                && devices.values().all(|d| {
                                    d["torque_enable"] == json!([0])
                                        && d["pwm"] == json!([0, 0])
                                        && d["telemetry"]["speed_raw"] == 0
                                })
                        });
                if physical_stopped || self.app.config.virtual_bench.is_some() {
                    state.release_uncertain = false;
                }
            }
            state.result = record.clone();
            state.active = false;
        }
        // Authority remains reserved through final release evidence and publication.
        self.finished = true;
        self.lease.take();
        Ok(record)
    }
}
impl Drop for Work {
    fn drop(&mut self) {
        self.signals.stop();
        if !self.finished {
            let _ = request_capture_stop(&self.dir, b"Dropped queued acquisition");
            let mut state = self.app.state.lock().unwrap();
            state.active = false;
            if self.motion {
                state.release_uncertain = true;
            }
            state.result = json!({"completed":false,"cancelled":true,"stop_verified":false,"error":if self.executed{"Acquisition interrupted; physical release not verified"}else{"Acquisition abandoned before execution; physical release not verified"}});
            let _ = write_json(self.dir.join("application-result.json"), &state.result);
        }
    }
}
impl App {
    pub fn open(config: Config) -> Result<Arc<Self>, String> {
        // Legacy acquisition preferences remain readable. The path is never opened or executed.
        let trace: ReferenceTrace =
            serde_json::from_slice(&fs::read(&config.trace).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        trace.validate()?;
        let template: Plan =
            serde_json::from_slice(&fs::read(&config.template).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        template.validate()?;
        crate::controller_refinement::fpga::validate_physical_scope(&config.ids, &config.ids)?;
        if blake3::hash(&fs::read(&template.bitstream_path).map_err(|e| e.to_string())?)
            .to_hex()
            .as_str()
            != template.bitstream_blake3
        {
            return Err("Bitstream identity mismatch".into());
        }
        fs::create_dir_all(&config.output).map_err(|e| e.to_string())?;
        if let Some(v) = &config.virtual_bench {
            v.validate(&config.ids)?;
        }
        let inspection = value(&config.inspection);
        Ok(Arc::new(Self {
            config,
            trace,
            template,
            signals: Mutex::new(None),
            epoch: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            state: Mutex::new(State {
                active: false,
                release_uncertain: false,
                lease: Instant::now(),
                owner: String::new(),
                run: None,
                request: Value::Null,
                plan: Value::Null,
                result: Value::Null,
                inspection,
                live_cursor: None,
                live_request: None,
            }),
        }))
    }
    pub fn config(&self) -> Value {
        json!({"ids":self.config.ids,"coordinates":self.trace.coordinates,"source":self.trace.source,"source_sha256":self.trace.source_sha256,"streamed_only":self.config.streamed_only,"kind":if self.config.virtual_bench.is_some(){"virtual"}else{"physical"},"fidelity":if self.config.virtual_bench.is_some(){"virtual_host_bench"}else{"physical_fpga"},"in_process":true})
    }
    pub fn status(&self, owner: &str) -> Value {
        status(self, owner)
    }
    pub fn heartbeat(&self, owner: &str) {
        let state = self.state.lock().unwrap();
        if state.owner == owner {
            if let Some(signal) = self.signals.lock().unwrap().as_ref() {
                signal.renew();
            }
        }
    }
    pub fn preview(&self, request: &Playback) -> Result<Value, String> {
        Ok(json!({"plan":self.trace.bind_bench_clip(request,&self.template,&self.config.ids)?}))
    }
    pub fn stop_epoch(&self) -> u64 {
        self.epoch.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub fn start(
        self: &Arc<Self>,
        request: Option<Playback>,
        owner: &str,
        live: Option<LiveRequest>,
    ) -> Result<Work, String> {
        self.start_at_epoch(request, owner, live, self.stop_epoch())
    }
    /// Callers that queue Open stamp this epoch when accepting the intent.
    pub fn start_at_epoch(
        self: &Arc<Self>,
        request: Option<Playback>,
        owner: &str,
        live: Option<LiveRequest>,
        expected_stop_epoch: u64,
    ) -> Result<Work, String> {
        prepare(self, request, owner, live, expected_stop_epoch)
    }
    pub fn sample(&self, owner: &str, sample: &LiveSample) -> Result<Value, String> {
        validate_owner(owner)?;
        let mut s = self.state.lock().unwrap();
        if !s.active || s.owner != owner || s.live_request.is_none() {
            return Err("No live session owned by this tab".into());
        }
        if self
            .signals
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(|signal| !signal.alive())
        {
            return Err("Live session cancelled or owner heartbeat expired".into());
        }
        s.live_request.as_ref().unwrap().validate_sample(sample)?;
        let changed = s.live_cursor.as_mut().unwrap().advance(sample)?;
        if changed {
            let dir = s.run.as_ref().unwrap();
            let r = Received {
                session: dir.file_name().unwrap().to_string_lossy().into(),
                received_unix_s: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_secs_f64(),
                sample: sample.clone(),
            };
            write_json(dir.join("latest-reference.tmp"), &r)?;
            fs::rename(
                dir.join("latest-reference.tmp"),
                dir.join("latest-reference.json"),
            )
            .map_err(|e| e.to_string())?;
            let mut log = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("browser-references.jsonl"))
                .map_err(|e| e.to_string())?;
            writeln!(
                log,
                "{}",
                serde_json::to_string(&r).map_err(|e| e.to_string())?
            )
            .map_err(|e| e.to_string())?;
            s.lease = Instant::now();
            if self
                .signals
                .lock()
                .unwrap()
                .as_ref()
                .is_none_or(|signal| !signal.renew())
            {
                return Err("Live session cancelled or owner heartbeat expired".into());
            }
        }
        Ok(json!({"accepted_new_frame":changed}))
    }
    /// Immediate UI/lifecycle safety signal: no mutex, disk or worker queue.
    pub fn latch_stop(&self) {
        self.epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    /// Acceptance latches cancellation; it never asserts stationary readback.
    pub fn stop(&self) -> Result<Value, String> {
        self.latch_stop();
        if let Some(signal) = self.signals.lock().unwrap().as_ref() {
            signal.stop();
        }
        let s = self.state.lock().unwrap();
        if s.active {
            if let Some(dir) = &s.run {
                request_capture_stop(dir, b"Operator STOP").map_err(|e| e.to_string())?;
            }
        }
        Ok(json!({"stop_requested":true,"physical_stop_verified":false}))
    }
}
pub mod acquisition;

mod virtual_run;
/// Explicit virtual model and taught travel. This is host-loop simulation,
/// not an emulation or qualification of the FPGA device-clock firmware.
#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VirtualConfig {
    pub bench_instance: String,
    pub start: [f64; 3],
    pub models: [crate::acquisition::virtual_bench::MotorModel; 3],
    pub axes: [crate::acquisition::calibration::AxisCalibration; 3],
}
impl VirtualConfig {
    fn validate(&self, ids: &[u8]) -> Result<(), String> {
        if !crate::hardware::protocol::calibration::valid_instance(&self.bench_instance)
            || ids.len() != 3
            || self.start.iter().any(|v| !v.is_finite())
        {
            return Err("Virtual bench needs stable identity and three explicit axes".into());
        }
        for model in &self.models {
            if [
                model.speed_gain,
                model.lag_s,
                model.breakaway_duty,
                model.moving_friction_duty,
                model.load_duty,
                model.gravity_duty,
                model.gravity_zero_counts,
                model.backlash_counts,
                model.compliance_counts_per_duty,
                model.stall_current_a,
                model.winding_resistance_ohm,
                model.heat_capacity_j_k,
                model.thermal_resistance_k_w,
            ]
            .iter()
            .any(|v| !v.is_finite())
                || model.speed_gain <= 0.
                || model.lag_s <= 0.
                || model.heat_capacity_j_k <= 0.
                || model.thermal_resistance_k_w <= 0.
                || model.stall_current_a < 0.
                || model.winding_resistance_ohm < 0.
                || model.backlash_counts < 0.
                || model.compliance_counts_per_duty < 0.
                || !(0.0..=1.0).contains(&model.breakaway_duty)
                || !(0.0..=1.0).contains(&model.moving_friction_duty)
            {
                return Err("Virtual bench model has invalid physical parameters".into());
            }
        }
        for (axis, start) in self.axes.iter().zip(self.start) {
            axis.validate()?;
            let (lo, hi) = axis.encoder_bounds();
            let (lo, hi) = (
                lo.ok_or("Virtual bench lower taught bound missing")?,
                hi.ok_or("Virtual bench upper taught bound missing")?,
            );
            if axis.disabled || start < lo as f64 + 40. || start > hi as f64 - 40. {
                return Err(
                    "Virtual bench starts outside its enabled taught travel clearance".into(),
                );
            }
        }
        Ok(())
    }
}
impl App {
    fn device_key(&self) -> String {
        self.config
            .virtual_bench
            .as_ref()
            .map(|v| format!("virtual:{}", v.bench_instance))
            .unwrap_or_else(|| self.config.port.clone())
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Ok(signals) = self.signals.lock() {
            if let Some(s) = signals.as_ref() {
                s.stop();
            }
        }
    }
}

fn validate_owner(owner: &str) -> Result<(), String> {
    if owner.len() != 36 || !owner.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err("Invalid browser identity".into());
    }
    Ok(())
}
