//! Caller-owned sessions, configuration, submission validation and control plane.
use super::*;

/// Transport-neutral calibration application. The caller owns its worker thread.
pub struct Service;
/// Cloneable control plane; STOP and heartbeat never wait behind acquisition.
pub struct Handle {
    pub(super) app: Arc<App>,
}
pub struct Worker {
    app: Arc<App>,
    rx: mpsc::Receiver<Job>,
    cfg: Config,
    bus: Option<CalibrationBus>,
    started: bool,
    _lease: crate::hardware::ownership::DeviceLease,
}
impl Clone for Handle {
    fn clone(&self) -> Self {
        self.app.handles.fetch_add(1, Ordering::SeqCst);
        Self {
            app: self.app.clone(),
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        if self.app.handles.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.app.latch_stop();
            self.app.shutdown.store(true, Ordering::SeqCst);
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if self.started {
            return;
        } // The inner bus guard owns run cleanup.
        let epoch = self.app.latch_stop();
        if let Some(bus) = self.bus.as_mut() {
            for id in self.cfg.roles.keys() {
                let _ = bus.stop(*id);
            }
        }
        self.app.shutdown.store(true, Ordering::SeqCst);
        let mut state = self.app.state.lock().unwrap_or_else(|e| e.into_inner());
        state["connected"] = json!(false);
        state["stop_epoch"] = json!(epoch);
        state["release"] = json!({"epoch":epoch,"verified":false,"pending":false,"error":"Hardware worker dropped before execution; release readback unverified"});
    }
}
impl Worker {
    pub fn run(mut self) {
        self.started = true;
        let bus = self.bus.take();
        worker(
            self.app.clone(),
            std::mem::replace(&mut self.rx, mpsc::channel().1),
            self.cfg.clone(),
            bus,
        );
        // The bus-owning worker already published final release. Do not let
        // the outer Drop create an epoch no worker remains to acknowledge.
        self.app.shutdown.store(true, Ordering::SeqCst);
        let epoch = *self.app.safety.lock().unwrap();
        let mut state = self.app.state.lock().unwrap_or_else(|e| e.into_inner());
        if state["stop_epoch"].as_u64() != Some(epoch) {
            state["connected"] = json!(false);
            state["stop_epoch"] = json!(epoch);
            state["release"] = json!({"epoch":epoch,"verified":false,"pending":false,"error":"Hardware worker ended before release evidence; inspect before rearming"});
        }
    }
}
impl Service {
    pub fn open_config(path: &std::path::Path) -> R<(Handle, Worker)> {
        let cfg: Config = serde_json::from_slice(
            &fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?,
        )
        .map_err(|e| format!("{}: {e}", path.display()))?;
        Self::new(cfg)
    }
    pub fn new(cfg: Config) -> R<(Handle, Worker)> {
        if cfg.roles.keys().copied().collect::<Vec<_>>() != vec![1, 2, 3] {
            return Err("This FPGA profile covers IDs 1,2,3".into());
        }
        cfg.sweep_tuning.validate()?;
        if cfg.virtual_bench != (cfg.serial == "virtual-capability-only") {
            return Err("virtual_bench=true requires serial=virtual-capability-only; physical sessions require a direct serial path".into());
        }
        if cfg.virtual_bench
            && !crate::hardware::protocol::calibration::valid_instance(&cfg.bench_instance)
        {
            return Err("Virtual configuration requires a stable UUID bench_instance and a separate output directory".into());
        }
        let execution = ExecutionIdentity {
            schema_version: 1,
            kind: if cfg.virtual_bench {
                "virtual_calibration"
            } else {
                "physical"
            }
            .into(),
            server_instance: crate::hardware::protocol::new_client_id(),
            bench_instance: if cfg.virtual_bench {
                cfg.bench_instance.clone()
            } else {
                String::new()
            },
        };
        let device = if cfg.virtual_bench {
            format!("virtual:{}", cfg.bench_instance)
        } else {
            cfg.serial.clone()
        };
        let lease = crate::hardware::ownership::DeviceLease::acquire(&device, "calibration")?;
        if cfg.virtual_bench
            && cfg.output.exists()
            && !cfg.output.join("execution.json").exists()
            && fs::read_dir(&cfg.output)
                .map_err(|e| e.to_string())?
                .next()
                .is_some()
        {
            return Err("Virtual output contains unidentified prior artifacts; use a new empty output directory".into());
        }
        fs::create_dir_all(&cfg.output).map_err(|e| e.to_string())?;
        let provenance = cfg.output.join("execution.json");
        if provenance.exists() {
            let previous: ExecutionIdentity =
                serde_json::from_slice(&fs::read(&provenance).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            if previous.kind != execution.kind
                || previous.bench_instance != execution.bench_instance
            {
                return Err("Output belongs to a different physical/virtual bench; use a new output directory".into());
            }
        }
        fs::write(
            cfg.output
                .join(format!("execution-{}.json", execution.server_instance)),
            serde_json::to_vec_pretty(&execution).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::write(
            provenance,
            serde_json::to_vec_pretty(&execution).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let bus = if cfg.virtual_bench {
            use crate::acquisition::virtual_bench::{Bench, MotorModel};
            let knee = MotorModel {
                breakaway_duty: 0.14,
                moving_friction_duty: 0.07,
                ..Default::default()
            };
            let worm = MotorModel {
                speed_gain: 3290.,
                breakaway_duty: 0.066,
                moving_friction_duty: 0.045,
                ..Default::default()
            };
            let belt = MotorModel {
                speed_gain: 3030.,
                breakaway_duty: 0.08,
                moving_friction_duty: 0.06,
                load_duty: -0.02,
                ..Default::default()
            };
            Some(CalibrationBus::in_process(
                Bench::new([3100., 1100., 1500.], [knee, worm, belt]),
                &cfg.output.join("serial.jsonl"),
            )?)
        } else {
            None
        };
        let (tx, rx) = mpsc::sync_channel(1);
        let app = Arc::new(App {
            execution: execution.clone(),
            shutdown: AtomicBool::new(false),
            handles: std::sync::atomic::AtomicUsize::new(1),
            generations: Mutex::new(Default::default()),
            safety: Mutex::new(0),
            state: Mutex::new(
                json!({"execution":execution,"coordinate_session":stamp().to_string(),"maximum_speed_counts_s":cfg.sweep_tuning.maximum_speed_counts_s,"connected":false,"enabled_id":null,"busy":false,"samples":{},"message":"Starting","error":null,"output":cfg.output}),
            ),
            jobs: tx,
            stop: AtomicBool::new(true),
            cancel: AtomicBool::new(true),
            cancel_sequence: AtomicU64::new(0),
            sweep: Mutex::new(None),
            gait: Mutex::new(None),
            sweep_tuning: cfg.sweep_tuning.clone(),
        });
        app.state.lock().unwrap()["gait_runs"] = gait_run_history(&cfg);
        Ok((
            Handle { app: app.clone() },
            Worker {
                app,
                rx,
                cfg,
                bus,
                started: false,
                _lease: lease,
            },
        ))
    }
}
impl Handle {
    pub fn execution(&self) -> ExecutionIdentity {
        self.app.execution.clone()
    }
    pub fn status(&self) -> Value {
        let epoch = *self.app.safety.lock().unwrap();
        let mut state = self.app.state.lock().unwrap().clone();
        state["stop_latched"] = json!(self.app.stop.load(Ordering::SeqCst));
        if state["stop_epoch"].as_u64() != Some(epoch) && self.app.stop.load(Ordering::SeqCst) {
            state["release"] = json!({"epoch":epoch,"verified":false,"pending":true});
        }
        state
    }
    pub fn export(&self) -> Value {
        export_document(&self.app)
    }
    pub fn gaits(&self) -> R<Value> {
        gait_catalog()
    }
    pub fn gait(&self, path: &str) -> R<Value> {
        gait_with_governor(path)
    }
    pub fn stop_immediate(&self) {
        self.app.latch_stop();
    }
    pub fn halt(&self, sequence: u64) {
        self.app
            .cancel_sequence
            .fetch_max(sequence, Ordering::SeqCst);
        self.app.cancel.store(true, Ordering::SeqCst);
    }
    pub fn shutdown(&self) {
        self.app.latch_stop();
        self.app.shutdown.store(true, Ordering::SeqCst);
    }
    pub fn get(&self, path: &str) -> R<Value> {
        match path {
            "/calibration/status" => Ok(self.status()),
            "/calibration/export" => Ok(self.export()),
            "/calibration/gaits" => self.gaits(),
            _ if path.starts_with("/calibration/gait?path=") => self.gait(&percent_decode(
                path.trim_start_matches("/calibration/gait?path="),
            )?),
            _ => Err("Unknown endpoint".into()),
        }
    }
    pub fn command(
        &self,
        client: &str,
        body: Value,
        pin: Option<(ExecutionIdentity, u64)>,
    ) -> R<Value> {
        self.command_inner(client, body, pin, None)
    }
    pub fn stop_epoch(&self) -> u64 {
        *self.app.safety.lock().unwrap()
    }
    /// Bind an authorized submission before crossing a queued/control-plane
    /// boundary. STOP between authorization and dispatch cannot rearm it.
    pub fn command_at_epoch(
        &self,
        client: &str,
        body: Value,
        pin: Option<(ExecutionIdentity, u64)>,
        expected_stop_epoch: u64,
    ) -> R<Value> {
        self.command_inner(client, body, pin, Some(expected_stop_epoch))
    }
    fn command_inner(
        &self,
        client: &str,
        body: Value,
        pin: Option<(ExecutionIdentity, u64)>,
        expected_stop_epoch: Option<u64>,
    ) -> R<Value> {
        if client.len() != 36 || !client.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
            return Err("Invalid tab identity".into());
        }
        let request: Request = serde_json::from_value(body).map_err(|e| e.to_string())?;
        let app = &self.app;
        if app.shutdown.load(Ordering::SeqCst) && request.action != "stop" {
            return Err("Hardware session closed; reconnect explicitly".into());
        }
        let execution = if request.action == "stop" {
            None
        } else if let Some((identity, generation)) = pin {
            // Scope is checked after the binding, in check_execution (400).
            if identity != app.execution || !identity.is_virtual_calibration() || generation == 0 {
                return Err(format!("{BINDING_REFUSED}: identity mismatch"));
            }
            let mut generations = app.generations.lock().unwrap();
            let current = generations.entry(client.into()).or_insert(generation);
            if generation < *current {
                return Err(format!("{BINDING_REFUSED}: stale connection generation"));
            }
            if generation > *current {
                *current = generation;
                app.latch_stop();
            }
            Some((identity, generation))
        } else {
            None
        };
        app.check_execution(&request.action, client, execution.as_ref())?;
        let mut stop_epoch = *app.safety.lock().unwrap();
        if !matches!(request.action.as_str(), "stop" | "halt") {
            if expected_stop_epoch.is_some_and(|expected| expected != stop_epoch) {
                return Err(STOP_INTERRUPTED.into());
            }
        }

        if request.action == "capture_hold" {
            // Answered only once the hold session saved the pose (200 with
            // the full state) or refused it (400), never before.
            let (done, outcome) = mpsc::sync_channel(1);
            let run_id = {
                let mut lock = app.sweep.lock().unwrap();
                let ctl = lock
                    .as_mut()
                    .ok_or("Move then release to hold before saving")?;
                if app.stop.load(Ordering::SeqCst)
                    || app.cancel.load(Ordering::SeqCst)
                    || motion_request(&request)? != MotionCommand::Hold
                    || !ctl.teaching
                    || ctl.motion != MotionCommand::Hold
                    || !matches!(request.boundary.as_str(), "upper" | "lower" | "reference")
                {
                    return Err("Release to hold before saving a pose".into());
                }
                if ctl.capture.is_some() {
                    return Err("A pose save is already pending; wait for its answer".into());
                }
                ctl.update(client, &request, &app.sweep_tuning)?;
                if ctl.motion != MotionCommand::Hold {
                    return Err("Release before saving".into());
                }
                ctl.capture = Some(PendingCapture {
                    boundary: request.boundary.clone(),
                    joint_rad: request.reference_joint_rad,
                    sequence: request.sequence,
                    done,
                });
                ctl.run_id
            }; // The session takes the capture under this lock: release it before waiting.
            let state = await_capture(app, run_id, request.sequence, &outcome)?;
            return Ok(state);
        }
        if request.action == "gait_update" {
            let mut lock = app.gait.lock().unwrap();
            let lease = lock.as_mut().ok_or("No gait is playing on the leg")?;
            if lease.owner != client {
                return Err("Another tab owns the gait session".into());
            }
            if !(request.speed_scale > 0. && request.speed_scale <= 1.) {
                return Err("Speed scale must be in (0, 1]".into());
            }
            if app.stop.load(Ordering::SeqCst) || app.cancel.load(Ordering::SeqCst) {
                return Err("Gait stop is latched".into());
            }
            if lease.last_seen.elapsed() > Duration::from_millis(1500) {
                return Err("Browser heartbeat lost; gait stopped".into());
            }
            lease.speed_scale = request.speed_scale;
            lease.playing = request.playing;
            lease.last_seen = Instant::now();
            return Ok(json!({"ok":true}));
        }
        if request.action == "sweep_update" || request.action == "motion_update" {
            let mut lock = app.sweep.lock().unwrap();
            let control = lock.as_mut().ok_or("No continuous sweep is active")?;
            if app.stop.load(Ordering::SeqCst) || app.cancel.load(Ordering::SeqCst) {
                return Err("Sweep stop is latched".into());
            }
            control.update(client, &request, &app.sweep_tuning)?;
            return Ok(json!({"ok":true}));
        }
        if request.action == "clear" || request.action == "select" {
            // Compare and advance atomically with STOP, never accepting a
            // select merely because it arrived after an earlier STOP.
            let mut safety = app.safety.lock().unwrap();
            if *safety != stop_epoch {
                return Err(STOP_INTERRUPTED.into());
            }
            *safety = safety.wrapping_add(1);
            stop_epoch = *safety;
            app.stop.store(true, Ordering::SeqCst);
            app.cancel.store(true, Ordering::SeqCst);
        }
        if request.action == "stop" {
            app.latch_stop();
            // Independent of the ordinary queue. Worker/active loops observe
            // the latch; response means latched, not stationary readback.
            // The copy shows the latch at once (disabled, idle); the stored
            // state and its records are left to the worker's torque-off.
            let mut state = app.state.lock().unwrap().clone();
            state["enabled_id"] = Value::Null;
            state["busy"] = json!(false);
            state["stop_latched"] = json!(true);
            state["release"] =
                json!({"epoch":*app.safety.lock().unwrap(),"verified":false,"pending":true});
            state["message"] = json!(STOP_LATCHED_MESSAGE);
            return Ok(state);
        }
        if request.action == "halt" {
            // Release, not STOP: cancel the running/queued motion (even a jog
            // queued but not started) without bumping the STOP epoch, so other
            // axes keep their state and the queued halt itself still runs.
            app.cancel_sequence
                .fetch_max(request.sequence, Ordering::SeqCst);
            app.cancel.store(true, Ordering::SeqCst);
        }
        let (tx, rx) = mpsc::channel();
        app.jobs
            .try_send(Job {
                queued: Instant::now(),
                execution,
                stop_epoch,
                request,
                client: client.into(),
                reply: tx,
            })
            .map_err(|_| "Hardware busy; no command queued")?;
        let value = rx
            .recv_timeout(HTTP_WAIT)
            .map_err(|_| "Hardware response timed out")??;
        Ok(value)
    }
}
