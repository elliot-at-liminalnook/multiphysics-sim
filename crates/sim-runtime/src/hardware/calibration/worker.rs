use super::*;

/// Owns the actual bus through unwinding as well as normal worker shutdown.
/// The outer Worker has moved this bus and cannot release it on panic.
struct BusGuard {
    bus: Option<CalibrationBus>,
    ids: Vec<u8>,
    app: Arc<App>,
}
impl std::ops::Deref for BusGuard {
    type Target = Option<CalibrationBus>;
    fn deref(&self) -> &Self::Target {
        &self.bus
    }
}
impl std::ops::DerefMut for BusGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.bus
    }
}
impl Drop for BusGuard {
    fn drop(&mut self) {
        if let Some(bus) = self.bus.as_mut() {
            for id in &self.ids {
                let _ = bus.stop(*id);
            }
            if std::thread::panicking() {
                self.app.stop.store(true, Ordering::SeqCst);
                self.app.cancel.store(true, Ordering::SeqCst);
                let mut epoch = self.app.safety.lock().unwrap_or_else(|e| e.into_inner());
                *epoch = epoch.wrapping_add(1);
                let stopped_epoch = *epoch;
                drop(epoch);
                self.app.shutdown.store(true, Ordering::SeqCst);
                let mut state = self.app.state.lock().unwrap_or_else(|e| e.into_inner());
                state["enabled_id"] = Value::Null;
                state["busy"] = json!(false);
                state["connected"] = json!(false);
                state["stop_epoch"] = json!(stopped_epoch);
                state["release"] = json!({"epoch":stopped_epoch,"verified":false,"pending":false,"error":"Hardware worker panicked; release readback unverified. Cut motor power."});
            }
        }
    }
}

pub(super) fn worker(
    app: Arc<App>,
    rx: mpsc::Receiver<Job>,
    cfg: Config,
    initial_bus: Option<CalibrationBus>,
) {
    let mut bus = BusGuard {
        bus: initial_bus,
        ids: cfg.roles.keys().copied().collect(),
        app: app.clone(),
    };
    let path = cfg.output.join("calibration.json");
    let mut cal = if path.exists() {
        match fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|v| serde_json::from_slice::<Calibration>(&v).map_err(|e| e.to_string()))
            .and_then(|c| c.validate().map(|_| c))
        {
            Ok(c) => c,
            Err(e) => {
                app.state.lock().unwrap()["error"] = json!(e);
                return;
            }
        }
    } else {
        let mut c = Calibration::default();
        c.fixture = cfg.fixture.clone();
        c.axes = cfg
            .roles
            .iter()
            .map(|(id, role)| {
                (
                    *id,
                    AxisCalibration {
                        role: role.clone(),
                        ..Default::default()
                    },
                )
            })
            .collect();
        c
    };
    if app.execution.is_virtual_calibration() {
        cal.provenance = format!(
            "SIMULATED virtual calibration; server {}; bench {}. Not measured physical hardware. No CAD/registry promotion.",
            app.execution.server_instance, app.execution.bench_instance
        );
    }
    cal.units="Continuous motor encoder counts in the powered tracking session; 4096 counts/revolution; not joint degrees".into();
    let mut idle_polling = false;
    let mut owner = String::new();
    let mut selected = 0;
    let mut last_seq = 0;
    let mut verified = false;
    // Motors whose zero-drive watchdogs were proven for the current selection.
    let mut proven = std::collections::BTreeSet::<u8>::new();
    {
        let mut s = app.state.lock().unwrap();
        s["calibration"] = json!(cal);
        s["message"] = json!("Connect and inspect to read motors. No motion on page load.");
    }
    // STOP epochs already acted on. Every newer epoch torques off all axes,
    // including ones held by hold_others or left after inspect/clear.
    let mut handled_stop_epoch = *app.safety.lock().unwrap();
    loop {
        handled_stop_epoch = observe_stop(
            &app,
            &cfg,
            &cal,
            &mut bus,
            handled_stop_epoch,
            &mut selected,
            &mut verified,
            &mut owner,
        );
        if app.shutdown.load(Ordering::SeqCst) {
            break;
        }
        let job = match rx.recv_timeout(Duration::from_millis(150)) {
            Ok(job) => {
                // A STOP that landed while waiting is acted on before this job.
                handled_stop_epoch = observe_stop(
                    &app,
                    &cfg,
                    &cal,
                    &mut bus,
                    handled_stop_epoch,
                    &mut selected,
                    &mut verified,
                    &mut owner,
                );
                // Refusals here never ran anything, so they skip the error
                // path's stop/disown of a live session. Binding first (409; a
                // generation bump already latched STOP) and virtual scope (400), then expiry, then a
                // job captured before the latest STOP/latch.
                let refusal = app
                    .check_execution(&job.request.action, &job.client, job.execution.as_ref())
                    .err()
                    .or_else(|| {
                        (job.queued.elapsed() + Duration::from_secs(1) >= HTTP_WAIT)
                            .then(|| "Command expired before execution; nothing ran".to_string())
                    })
                    .or_else(|| {
                        (job.stop_epoch != handled_stop_epoch).then(|| STOP_INTERRUPTED.to_string())
                    });
                if let Some(refusal) = refusal {
                    let _ = job.reply.send(Err(refusal));
                    continue;
                }
                job
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let mut idle_lost = false;
                if idle_polling {
                    if let Some(b) = bus.as_mut() {
                        // A disabled motor may be absent: no readback is expected
                        // from it (as in `observe_stop`), so it is not polled.
                        for id in cfg
                            .roles
                            .keys()
                            .filter(|id| !cal.axes.get(*id).is_some_and(|a| a.disabled))
                        {
                            match b.feedback(*id) {
                                Ok(t) => {
                                    app.state.lock().unwrap()["samples"][id.to_string()] = json!(t)
                                }
                                Err(e) => {
                                    idle_polling = false;
                                    verified = false;
                                    owner.clear();
                                    let _ = b.stop(*id);
                                    // Only this axis's turns are uncertain; the others kept reading.
                                    b.reset_turn_tracking_for(*id);
                                    let mut s = app.state.lock().unwrap();
                                    s["enabled_id"] = Value::Null;
                                    s["connected"] = json!(false);
                                    // A virtual execution is revoked only with its bus (lose_bus below).
                                    if multi_turn(&cal.axes[id]) {
                                        s["coordinate_session"] = json!(stamp().to_string());
                                    }
                                    s["message"] = json!(format!(
                                        "Readback lost from {}: {e}. Select a motor to reconnect.",
                                        cfg.roles[id]
                                    ));
                                    idle_lost = app.execution.is_virtual_calibration()
                                        && transport_lost(&e);
                                    break;
                                }
                            }
                        }
                    }
                }
                if idle_lost {
                    // The virtual execution is revoked; its link is not reused.
                    lose_bus(&app, &mut bus);
                    app.state.lock().unwrap()["message"] = json!(
                        "Virtual bench readback lost; bus closed. Restart and reconnect explicitly."
                    );
                }
                continue;
            }
        };
        let virtual_mode = app.execution.is_virtual_calibration();
        // A session failed inside an Ok reply; a bus probe afterwards decides
        // whether the virtual link is gone (its text may be a save failure).
        let mut probe_link = false;
        let result = (|| -> R<Value> {
            let r = &job.request;
            if r.action == "inspect" {
                if bus.is_none() {
                    bus.bus = Some(app.open_bus(&cfg)?)
                }
                let b = bus.as_mut().unwrap();
                idle_polling = true;
                verified = false;
                owner.clear();
                selected = 0;
                // The STOP readback proof comes from an enabled motor: a
                // disabled one may be absent (ID 3 as before if all are disabled).
                let probe = cfg
                    .roles
                    .keys()
                    .copied()
                    .find(|id| !cal.axes.get(id).is_some_and(|a| a.disabled))
                    .unwrap_or(3);
                b.reconnect_stopped(probe)?;
                let mut samples = serde_json::Map::new();
                // Disabled motors may be absent; inspect reads the enabled ones.
                for id in cfg
                    .roles
                    .keys()
                    .filter(|id| !cal.axes.get(*id).is_some_and(|a| a.disabled))
                {
                    samples.insert(id.to_string(), json!(b.feedback(*id)?));
                }
                let mut s = app.state.lock().unwrap();
                s["samples"] = json!(samples);
                s["connected"] = json!(true);
                s["enabled_id"] = Value::Null;
                s["message"] =
                    json!("Readback received. Select an axis and enable supervised teaching.");
                return Ok(s.clone());
            }
            if !cfg.roles.contains_key(&r.id) {
                return Err("Unknown motor ID".into());
            }
            if r.action == "set_disabled" {
                if app.state.lock().unwrap()["enabled_id"] == json!(r.id) {
                    return Err("Stop this motor before disabling it".into());
                }
                let mut next = cal.clone();
                next.axes.get_mut(&r.id).unwrap().disabled = r.disabled;
                save(
                    &cfg.output.join(format!("calibration-{}.json", stamp())),
                    &next,
                )?;
                save(&path, &next)?;
                cal = next;
                let mut s = app.state.lock().unwrap();
                s["calibration"] = json!(cal);
                s["message"] = json!(if r.disabled {
                    "Motor disabled. It cannot be selected or driven until enabled."
                } else {
                    "Motor enabled. Select it to reconnect."
                });
                return Ok(s.clone());
            }
            if cal.axes[&r.id].disabled
                && !matches!(r.action.as_str(), "stop" | "clear" | "flip" | "direction")
            {
                return Err(format!(
                    "{} is disabled; enable it before moving it",
                    cfg.roles[&r.id]
                ));
            }
            if r.action == "select" {
                if bus.is_none() {
                    bus.bus = Some(app.open_bus(&cfg)?);
                }
                let b = bus.as_mut().unwrap();
                idle_polling = true;
                verified = false;
                owner.clear();
                selected = r.id;
                b.reconnect_stopped(r.id)?;
                proven.clear();
                let t = b.prove_watchdogs(r.id)?;
                proven.insert(r.id);
                let mut unproven = Vec::new();
                if r.hold_others {
                    for other in cfg
                        .roles
                        .keys()
                        .copied()
                        .filter(|k| *k != r.id && !cal.axes[k].disabled)
                    {
                        match b.prove_watchdogs(other) {
                            Ok(_) => {
                                proven.insert(other);
                            }
                            Err(e) => unproven.push(format!("{}: {e}", cfg.roles[&other])),
                        }
                    }
                }
                owner = job.client.clone();
                idle_polling = true;
                verified = true;
                last_seq = r.sequence;
                app.cancel_sequence.store(r.sequence, Ordering::SeqCst);
                app.arm(job.stop_epoch, true)?;
                let mut s = app.state.lock().unwrap();
                s["connected"] = json!(true);
                s["samples"][r.id.to_string()] = json!(t);
                s["enabled_id"] = json!(r.id);
                s["capture_message"] = Value::Null;
                s["sweep"] = Value::Null;
                s["message"] = json!(if unproven.is_empty() {
                    "Ready. Hold Q toward upper or A toward lower; release to hold position."
                        .to_string()
                } else {
                    format!(
                        "Ready. Not held (watchdog check failed): {}",
                        unproven.join("; ")
                    )
                });
                return Ok(s.clone());
            }
            let b = bus.as_mut().ok_or("Select a motor first")?;
            if r.action == "enable" {
                if !r.supported {
                    return Err("Confirm the fixture is supported with torque disabled".into());
                }
                // Re-proves the axis this tab just selected; never a rearm
                // after STOP (STOP clears the selection and owner, and leaves
                // the stop flag set until a fresh select).
                if selected != r.id || owner != job.client {
                    return Err("Select this motor in this tab first".into());
                }
                verified = false;
                last_seq = 0;
                app.cancel_sequence.store(0, Ordering::SeqCst);
                app.arm(job.stop_epoch, false)?;
                let t = b.prove_watchdogs(r.id)?;
                app.arm(job.stop_epoch, false)?;
                verified = true;
                let mut s = app.state.lock().unwrap();
                s["samples"][r.id.to_string()] = json!(t);
                s["enabled_id"] = json!(selected);
                s["message"] = json!(
                    "Ready for deliberate jogs. Zero-drive watchdogs verified; mechanical stopping distance remains unqualified."
                );
                return Ok(s.clone());
            }
            if r.action == "clear" {
                // The worker serializes this after any sweep has stopped; immutable
                // snapshots preserve the exact bounds being replaced.
                let t = b.stop(r.id)?;
                let mut next = cal.clone();
                next.axes.get_mut(&r.id).unwrap().clear(&r.boundary)?;
                save(
                    &cfg.output
                        .join(format!("calibration-{}-before-clear.json", stamp())),
                    &cal,
                )?;
                save(
                    &cfg.output.join(format!("calibration-{}.json", stamp())),
                    &next,
                )?;
                save(&path, &next)?;
                cal = next;
                verified = false;
                owner.clear();
                selected = 0;
                app.stop.store(true, Ordering::SeqCst);
                let mut s = app.state.lock().unwrap();
                s["calibration"] = json!(cal);
                s["samples"][r.id.to_string()] = json!(t);
                s["enabled_id"] = Value::Null;
                s["message"] = json!(format!(
                    "{} cleared for this axis; previous values saved in history. Re-enable to teach new poses.",
                    r.boundary
                ));
                return Ok(s.clone());
            }
            if owner != job.client || selected != r.id || !verified {
                return Err("Enable this motor in this tab first".into());
            }
            if r.sequence <= last_seq {
                return Err("Stale or duplicate command rejected".into());
            }
            last_seq = r.sequence;
            let current_session = app.state.lock().unwrap()["coordinate_session"]
                .as_str()
                .map(str::to_string);
            if r.action == "tune" {
                // Operator confirms the axis is mid-travel with room both ways.
                if !r.supported {
                    return Err(
                        "Confirm the motor is mid-travel with room to move both ways".into(),
                    );
                }
                let t = b.stop(r.id)?;
                let position = t.position_continuous.ok_or("Missing encoder coordinate")?;
                let axis = usable(&cal.axes[&r.id], current_session.as_deref());
                let (lo, hi) = axis.encoder_bounds();
                let room = [lo.map(|l| position - l), hi.map(|h| h - position)]
                    .into_iter()
                    .flatten()
                    .min()
                    .unwrap_or(i32::MAX);
                let travel = (room - 60).min(200);
                if travel < 40 {
                    return Err(format!(
                        "Only {room} counts to the nearest saved pose; move toward the middle first (needs 100)"
                    ));
                }
                let duty = (r.drive_pwm as f64 / 1000.).clamp(0.05, 1.);
                // Arm before publishing `running`: a refusal must not leave it set.
                app.arm(job.stop_epoch, false)?;
                {
                    let mut s = app.state.lock().unwrap();
                    s["tuning"] = json!({"execution":app.execution,"simulated":app.execution.is_virtual_calibration(),"running":true,"motor_id":r.id,"stage":"Starting","travel_counts":travel});
                    s["message"] = json!(format!(
                        "Tuning {} — short moves within ±{travel} counts. Z stops.",
                        cfg.roles[&r.id]
                    ));
                    let _ = job.reply.send(Ok(s.clone()));
                }
                let result = b.identify(
                    r.id,
                    travel,
                    duty,
                    cfg.sweep_tuning.period_s,
                    &app.cancel,
                    |stage| {
                        app.state.lock().unwrap()["tuning"]["stage"] = json!(stage);
                    },
                );
                verified = false;
                owner.clear();
                let outcome = result.and_then(|record| {
                    let steps: Vec<crate::acquisition::motor_identification::StepTrace> =
                        serde_json::from_value(record["steps"].clone()).map_err(|e| e.to_string())?;
                    let fits: Vec<_> = steps.iter().filter_map(crate::acquisition::motor_identification::fit_step).collect();
                    let breakaway: [f64; 2] = serde_json::from_value(record["breakaway_duty"].clone()).map_err(|e| e.to_string())?;
                    let name = format!("tune-{}-{}.json", r.id, stamp());
                    let tuning = crate::acquisition::motor_identification::design(
                        &fits, breakaway, record["loop_period_s"].as_f64().unwrap_or(cfg.sweep_tuning.period_s),
                        cfg.sweep_tuning.velocity_filter_s, &name)?;
                    let artifact = json!({"record":record,"fits":fits,"tuning":tuning,"role":cfg.roles[&r.id],
                        "execution":app.execution,"simulated":app.execution.is_virtual_calibration(),"provenance":"Open-loop PWM identification under this fixture's load and supply; commissioning estimate, not a validated joint model"});
                    fs::write(cfg.output.join(&name), serde_json::to_vec_pretty(&artifact).unwrap()).map_err(|e| e.to_string())?;
                    Ok(tuning)
                });
                let mut s = app.state.lock().unwrap();
                s["tuning"]["running"] = json!(false);
                s["enabled_id"] = Value::Null;
                match outcome {
                    Ok(tuning) => {
                        let mut next = cal.clone();
                        next.axes.get_mut(&r.id).unwrap().tuning = Some(tuning.clone());
                        save(
                            &cfg.output.join(format!("calibration-{}.json", stamp())),
                            &next,
                        )?;
                        save(&path, &next)?;
                        cal = next;
                        s["calibration"] = json!(cal);
                        s["tuning"]["result"] = json!(tuning);
                        s["message"] = json!(format!(
                            "Tuned {}: {:.0} counts/s per unit duty, lag {:.0} ms, friction {:.0}%. New gains kp {:.2}, ki {:.2}, kd {:.3}. Select it to use them.",
                            cfg.roles[&r.id],
                            tuning.gain_counts_s_per_duty,
                            tuning.time_constant_s * 1000.,
                            tuning.friction_duty * 100.,
                            tuning.pid.kp,
                            tuning.pid.ki,
                            tuning.pid.kd
                        ));
                    }
                    Err(e) => {
                        probe_link = virtual_mode;
                        s["tuning"]["error"] = json!(e);
                        s["message"] = json!(format!(
                            "Tuning stopped: {e}. Torque off; previous gains kept."
                        ));
                    }
                }
                return Ok(s.clone());
            }
            if r.action == "gait_start" {
                let reply = |s: &Value| {
                    let _ = job.reply.send(Ok(s.clone()));
                };
                let outcome = run_gait(
                    &app,
                    &cfg,
                    b,
                    &cal,
                    &proven,
                    current_session.as_deref(),
                    &job.client,
                    r,
                    job.stop_epoch,
                    reply,
                );
                *app.gait.lock().unwrap() = None;
                app.stop.store(true, Ordering::SeqCst);
                verified = false;
                owner.clear();
                let mut s = app.state.lock().unwrap();
                s["busy"] = json!(false);
                // Labelled even when refused before run_gait published it.
                let gait = s["gait"].take();
                s["gait"] = labelled(&app, gait);
                s["gait"]["running"] = json!(false);
                s["enabled_id"] = Value::Null;
                match outcome {
                    Ok(message) => {
                        s["message"] = json!(format!(
                            "{message}. Torque off and stationary encoder verified."
                        ))
                    }
                    Err(e) => {
                        probe_link = virtual_mode;
                        // As the sweep path: a lost readback may have hidden
                        // encoder turns of every bound axis.
                        if readback_lost(&e) {
                            for g in &r.bindings {
                                b.reset_turn_tracking_for(g.id);
                            }
                            if r.bindings
                                .iter()
                                .any(|g| cal.axes.get(&g.id).is_some_and(multi_turn))
                            {
                                s["coordinate_session"] = json!(stamp().to_string());
                            }
                        }
                        s["gait"]["error"] = json!(e);
                        s["message"] = json!(format!("Gait stopped: {e}. Torque off."));
                    }
                }
                return Ok(s.clone());
            }
            if r.action == "lab_step" {
                let reply = |s: &Value| {
                    let _ = job.reply.send(Ok(s.clone()));
                };
                let outcome = run_lab_step(
                    &app,
                    &cfg,
                    b,
                    &cal,
                    &proven,
                    current_session.as_deref(),
                    r,
                    job.stop_epoch,
                    reply,
                );
                verified = false;
                owner.clear();
                let mut s = app.state.lock().unwrap();
                s["lab"]["running"] = json!(false);
                s["busy"] = json!(false);
                s["enabled_id"] = Value::Null;
                match outcome {
                    Ok(result) => {
                        s["lab"]["result"] = result.clone();
                        s["message"] = json!(format!(
                            "Lab step finished: {}. Torque off.",
                            result["headline"].as_str().unwrap_or("")
                        ));
                    }
                    Err(e) => {
                        probe_link = virtual_mode;
                        s["lab"]["error"] = json!(e);
                        s["message"] = json!(format!("Lab step stopped: {e}. Torque off."));
                    }
                }
                return Ok(s.clone());
            }
            if r.action == "campaign" {
                let reply = |s: &Value| {
                    let _ = job.reply.send(Ok(s.clone()));
                };
                let outcome = run_campaign(
                    &app,
                    &cfg,
                    b,
                    &cal,
                    &proven,
                    current_session.as_deref(),
                    r,
                    job.stop_epoch,
                    reply,
                );
                verified = false;
                owner.clear();
                let mut s = app.state.lock().unwrap();
                s["campaign"]["running"] = json!(false);
                s["busy"] = json!(false);
                s["enabled_id"] = Value::Null;
                match outcome {
                    Ok(summary) => {
                        s["campaign"]["result"] = summary.clone();
                        s["message"] = json!(format!(
                            "Campaign finished: {}. Results in {}. Nothing was promoted to CAD.",
                            summary["headline"].as_str().unwrap_or(""),
                            summary["directory"].as_str().unwrap_or("")
                        ));
                    }
                    Err(e) => {
                        probe_link = virtual_mode;
                        s["campaign"]["error"] = json!(e);
                        s["message"] = json!(format!(
                            "Campaign stopped: {e}. Torque off; completed stages are kept as receipts (Resume continues)."
                        ));
                    }
                }
                return Ok(s.clone());
            }
            if r.action == "sweep_start" || r.action == "motion_start" || r.action == "sweep_all" {
                let all = r.action == "sweep_all";
                let teaching = r.action == "motion_start" || all;
                if app.stop.load(Ordering::SeqCst) {
                    return Err("Stop is latched; enable this axis first".into());
                }
                let input = SweepInput {
                    speed_counts_s: if teaching || all {
                        r.speed_counts_s
                    } else {
                        5.
                    },
                    pwm_limit: r.drive_pwm,
                };
                input.validate(&cfg.sweep_tuning)?;
                let axis = usable(&cal.axes[&r.id], current_session.as_deref());
                axis.validate()?;
                if (!teaching || all) && (axis.lower.is_none() || axis.upper.is_none()) {
                    return Err("Teach both poses before starting a sweep".into());
                }
                // Other motors join the session: held in place while jogging,
                // or swept together. Only proven, enabled, current motors join.
                let mut ids = vec![r.id];
                let mut skipped = Vec::new();
                if all || (teaching && r.hold_others) {
                    for k in cfg.roles.keys().copied().filter(|k| *k != r.id) {
                        let a = usable(&cal.axes[&k], current_session.as_deref());
                        let why = if a.disabled {
                            Some("disabled")
                        } else if all && (a.lower.is_none() || a.upper.is_none()) {
                            Some("poses not taught")
                        } else if !proven.contains(&k) {
                            Some("watchdogs not proven; select with hold enabled")
                        } else {
                            None
                        };
                        match why {
                            Some(w) => skipped.push(format!("{} ({w})", cfg.roles[&k])),
                            None => ids.push(k),
                        }
                    }
                }
                let axes: Vec<AxisCalibration> = ids
                    .iter()
                    .map(|k| usable(&cal.axes[k], current_session.as_deref()))
                    .collect();
                app.arm(job.stop_epoch, false)?;
                if r.sequence <= app.cancel_sequence.load(Ordering::SeqCst)
                    || app.stop.load(Ordering::SeqCst)
                {
                    return Err("Sweep cancelled before starting".into());
                }
                let run_id = stamp() as u64;
                *app.sweep.lock().unwrap() = Some(BrowserSweep {
                    owner: job.client.clone(),
                    id: r.id,
                    run_id,
                    sequence: r.sequence,
                    input,
                    last_seen: Instant::now(),
                    motion: if teaching {
                        motion_request(r)?
                    } else {
                        MotionCommand::Sweep
                    },
                    teaching,
                    capture: None,
                });
                {
                    let mut s = app.state.lock().unwrap();
                    s["busy"] = json!(true);
                    s["sweep"] = json!({"running":true,"run_id":run_id,"motor_id":r.id,"motor_ids":ids,"skipped":skipped,"all":all,"axes":{},"samples":[],"speed_counts_s":input.speed_counts_s,"pwm_limit":r.drive_pwm,"drive_mode":r.drive_mode,"clearance_counts":r.clearance_counts,"teaching":teaching});
                    s["message"] = json!(if all {
                        format!(
                            "Sweeping {} together. Keep this tab active; Stop ends the sweep.",
                            ids.iter()
                                .map(|k| cfg.roles[k].as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    } else {
                        "Starting continuous traversal. Keep this tab active; Stop ends the sweep."
                            .to_string()
                    });
                    let _ = job.reply.send(Ok(s.clone()));
                }
                let mut history = std::collections::VecDeque::new();
                // Per axis: half-cycles at first sample and latest; sweep-all holds an axis after two.
                let progress =
                    std::cell::RefCell::new(std::collections::BTreeMap::<u8, (u64, u64)>::new());
                let done = |k: &u8| progress.borrow().get(k).is_some_and(|(s, l)| *l >= *s + 2);
                let result=b.controlled_motion_multi(&ids,&axes,r.clearance_counts,&cfg.sweep_tuning,&app.cancel,teaching,r.drive_mode,|| {
                    if app.cancel.load(Ordering::SeqCst) || app.stop.load(Ordering::SeqCst) {return Ok(None);}
                    let lock=app.sweep.lock().unwrap();let ctl=lock.as_ref().ok_or("Sweep controls closed")?;
                    if ctl.last_seen.elapsed()>Duration::from_millis(1500) {return Err("Browser heartbeat lost; sweep stopped".into());}
                    let a=usable(&serde_json::from_value(app.state.lock().unwrap()["calibration"]["axes"][r.id.to_string()].clone()).map_err(|e|e.to_string())?,current_session.as_deref());
                    if matches!(ctl.motion,MotionCommand::Target(_)|MotionCommand::Sweep|MotionCommand::Learn) && (a.lower.is_none()||a.upper.is_none()){return Err("Teach both poses before learning, sweeping, or using the angle dial".into());}
                    let mut plan=vec![(ctl.input,ctl.motion,a)];
                    if all {
                        if ids.iter().all(|k|done(k)) {return Ok(None);}
                        if done(&r.id) {plan[0].1=MotionCommand::Hold;}
                    }
                    for k in &ids[1..] {
                        let ak=usable(&serde_json::from_value(app.state.lock().unwrap()["calibration"]["axes"][k.to_string()].clone()).map_err(|e|e.to_string())?,current_session.as_deref());
                        plan.push((ctl.input,if all && !done(k) {MotionCommand::Sweep} else {MotionCommand::Hold},ak));
                    }
                    Ok(Some(plan))
                },|id,t,sample| {
                    {let mut s=app.state.lock().unwrap();s["samples"][id.to_string()]=json!(t);s["sweep"]["axes"][id.to_string()]=json!(sample);}
                    {let mut p=progress.borrow_mut();let e=p.entry(id).or_insert((sample.half_cycles as u64,sample.half_cycles as u64));e.1=sample.half_cycles as u64;}
                    if id!=r.id {return Ok(());}
                    history.push_back(json!(sample));if history.len()>300 {history.pop_front();}
                    // Taking a capture renews the lease: its client is present (it waits on this
                    // capture_hold) but cannot heartbeat until the answer, so the save cannot let it lapse.
                    let capture={let mut lock=app.sweep.lock().unwrap();lock.as_mut().and_then(|ctl|{let c=ctl.capture.take();if c.is_some(){ctl.last_seen=Instant::now();}c})};
                    if let Some(capture)=capture {
                        let stable=history.len()>=6 && history.iter().rev().take(6).all(|s|s["position_continuous"].as_i64().is_some_and(|p|(p-t.position_continuous.unwrap_or(t.position_raw as i32) as i64).abs()<=2));
                        if sample.holding && sample.velocity_counts_s.abs()<2. && (sample.target_raw-t.position_continuous.unwrap_or(t.position_raw as i32) as f64).abs()<3. && stable {
                            let boundary=capture.boundary.as_str();
                            // The waiting request learns every outcome, including a refusal that ends the session.
                            let saved=(||->R<()>{
                                let mut next=cal.clone();let a=next.axes.get_mut(&r.id).unwrap();
                                match boundary{"lower"=>a.lower=Some(t.position_continuous.ok_or("Missing continuous encoder coordinate")?),"upper"=>a.upper=Some(t.position_continuous.ok_or("Missing continuous encoder coordinate")?),"reference"=>a.reference=Some(t.position_continuous.ok_or("Missing continuous encoder coordinate")?),_=>return Err("Unknown pose".into())};
                                if a.lower.into_iter().chain(a.upper).any(|p|!(0..=4095).contains(&p)) {a.coordinate_session=Some(app.state.lock().unwrap()["coordinate_session"].as_str().unwrap().to_string());}
                                if boundary=="reference" {a.reference_session=a.reference.filter(|p|!(0..=4095).contains(p)).map(|_|app.state.lock().unwrap()["coordinate_session"].as_str().unwrap().to_string());a.reference_joint_rad=capture.joint_rad;}
                                next.validate()?;
                                if next.axes[&r.id].reversed()!=cal.axes[&r.id].reversed(){return Err("Pose would reverse upper/lower direction; swap direction explicitly first".into());}
                                save(&cfg.output.join(format!("calibration-{}.json",stamp())),&next)?;save(&path,&next)?;cal=next;
                                // The reading saved is the one the answer shows.
                                let mut s=app.state.lock().unwrap();s["calibration"]=json!(cal);s["capture_message"]=json!(format!("Saved {boundary} pose"));s["samples"][r.id.to_string()]=json!(t);
                                Ok(())
                            })();
                            // Renewed again after the (possibly slow) save, for the same reason.
                            if let Some(ctl)=app.sweep.lock().unwrap().as_mut(){if ctl.run_id==run_id{ctl.last_seen=Instant::now();}}
                            let _=capture.done.try_send(saved.clone());
                            saved?;
                        }else{
                            let settling="Still settling. Release Q/A, wait for Holding, then save the pose.";
                            app.state.lock().unwrap()["capture_message"]=json!(settling);
                            let _=capture.done.try_send(Err(settling.into()));
                        }
                    }
                    let mut s=app.state.lock().unwrap();s["samples"][r.id.to_string()]=json!(t);
                    s["sweep"]["samples"]=json!(history);s["sweep"]["latest"]=json!(sample);
                    if let Some(evidence)=&sample.adaptation.evidence {
                        let artifact=json!({"schema_version":1,"motor_id":r.id,"run_id":run_id,"axis":cal.axes[&r.id],"tuning":cfg.sweep_tuning,"evidence":evidence,"execution":app.execution,"simulated":app.execution.is_virtual_calibration(),"provenance":"Online observations under this session's load, effort ceiling and environment; not a physical safety certification; not auto-loaded next session"});
                        let artifact_path=cfg.output.join(format!("response-{}-{}.json",r.id,run_id));
                        let tmp=artifact_path.with_extension("tmp");fs::write(&tmp,serde_json::to_vec_pretty(&artifact).unwrap()).map_err(|e|e.to_string())?;fs::rename(tmp,artifact_path).map_err(|e|e.to_string())?;
                    }
                    s["message"]=json!(if all {"Sweeping enabled motors together · Z stops drive"} else if teaching {if sample.holding {"Holding position · Q/A to move · Z stops drive"}else{"Moving under feedback control · release to hold"}}else{"Sweeping between taught poses · Q/A takes over · Z stops drive"});
                    Ok(())
                });
                *app.sweep.lock().unwrap() = None;
                app.stop.store(true, Ordering::SeqCst);
                verified = false;
                owner.clear();
                {
                    let mut s = app.state.lock().unwrap();
                    s["busy"] = json!(false);
                    s["sweep"]["running"] = json!(false);
                    s["enabled_id"] = Value::Null;
                }
                let outcome = result?;
                probe_link = virtual_mode && outcome.motion_error.is_some();
                // Every axis kept reading through the session, so turn counts stay valid
                // unless the fault was a lost serial link.
                if outcome.motion_error.as_deref().is_some_and(readback_lost) {
                    for k in &ids {
                        b.reset_turn_tracking_for(*k);
                    }
                    if ids.iter().any(|k| multi_turn(&cal.axes[k])) {
                        app.state.lock().unwrap()["coordinate_session"] =
                            json!(stamp().to_string());
                    }
                }
                let mut s = app.state.lock().unwrap();
                s["samples"][r.id.to_string()] = json!(outcome.telemetry);
                s["sweep"]["motion_error"] = json!(outcome.motion_error);
                s["message"] = json!(format!(
                    "{}. Torque off and stationary encoder verified.",
                    outcome.motion_error.as_deref().unwrap_or("Sweep stopped")
                ));
                return Ok(s.clone());
            }
            if r.action == "halt" {
                let t = b.stop(r.id)?;
                let mut s = app.state.lock().unwrap();
                s["samples"][r.id.to_string()] = json!(t);
                s["message"] = json!("Released. Torque off and stationary encoder verified.");
                return Ok(s.clone());
            }
            if r.action == "flip" {
                let mut next = cal.clone();
                let a = next.axes.get_mut(&r.id).unwrap();
                let reverse = !a.reversed();
                std::mem::swap(&mut a.lower, &mut a.upper);
                a.reverse = reverse;
                save(
                    &cfg.output.join(format!("calibration-{}.json", stamp())),
                    &next,
                )?;
                save(&path, &next)?;
                cal = next;
                let mut s = app.state.lock().unwrap();
                s["calibration"] = json!(cal);
                s["message"] =
                    json!("Upper/lower direction swapped. Physical encoder envelope preserved.");
                return Ok(s.clone());
            }
            if r.action == "direction" {
                if cal.axes[&r.id].lower.is_some() || cal.axes[&r.id].upper.is_some() {
                    return Err("Direction is fixed once a named limit is taught".into());
                }
                let mut next = cal.clone();
                next.axes.get_mut(&r.id).unwrap().reverse = r.reverse;
                save(
                    &cfg.output.join(format!("calibration-{}.json", stamp())),
                    &next,
                )?;
                save(&path, &next)?;
                cal = next;
                let mut s = app.state.lock().unwrap();
                s["calibration"] = json!(cal);
                s["message"] =
                    json!("Part direction saved. Upper/lower refer to the part's poses.");
                return Ok(s.clone());
            }
            if r.action == "jog" {
                app.state.lock().unwrap()["sweep"] = Value::Null;
                if app.stop.load(Ordering::SeqCst) {
                    return Err("Stop is latched; enable teaching again".into());
                }
                app.arm(job.stop_epoch, false)?;
                if r.sequence <= app.cancel_sequence.load(Ordering::SeqCst)
                    || app.stop.load(Ordering::SeqCst)
                {
                    return Ok(app.state.lock().unwrap().clone());
                }
                app.state.lock().unwrap()["busy"] = json!(true);
                let result = b.jog(r.id, &cal.axes[&r.id], r.delta, r.drive_pwm, &app.cancel);
                app.state.lock().unwrap()["busy"] = json!(false);
                let t = result?;
                if t.outside_jog_window || t.motion_error.is_some() {
                    verified = false;
                    owner.clear();
                    app.stop.store(true, Ordering::SeqCst);
                }
                let mut s = app.state.lock().unwrap();
                if t.outside_jog_window || t.motion_error.is_some() {
                    s["enabled_id"] = Value::Null;
                }
                s["samples"][r.id.to_string()] = json!(t.telemetry);
                s["last_jog"] = json!(t);
                s["message"] = json!(format!(
                    "{} Requested {:+} counts; actual {:+} counts at {}% PWM.{}{}",
                    t.reason,
                    t.requested_counts,
                    t.actual_counts,
                    t.drive_pwm as f64 / 10.,
                    if t.stop_reply_recoveries > 0 {
                        " Stop readback reply retried and verified; no motion command repeated."
                    } else {
                        ""
                    },
                    if t.outside_jog_window {
                        " Coast exceeded the jog window. Stopped; inspect clearance before enabling again."
                    } else {
                        ""
                    }
                ));
                return Ok(s.clone());
            }
            if r.action == "capture" {
                let t = b.stop(r.id)?;
                let mut next = cal.clone();
                let a = next.axes.get_mut(&r.id).unwrap();
                match r.boundary.as_str() {
                    "lower" => {
                        a.lower = Some(
                            t.position_continuous
                                .ok_or("Missing continuous encoder coordinate")?,
                        )
                    }
                    "upper" => {
                        a.upper = Some(
                            t.position_continuous
                                .ok_or("Missing continuous encoder coordinate")?,
                        )
                    }
                    "reference" => {
                        a.reference = Some(
                            t.position_continuous
                                .ok_or("Missing continuous encoder coordinate")?,
                        )
                    }
                    _ => return Err("Unknown boundary".into()),
                };
                if r.boundary == "reference" {
                    a.reference_session =
                        a.reference.filter(|p| !(0..=4095).contains(p)).map(|_| {
                            app.state.lock().unwrap()["coordinate_session"]
                                .as_str()
                                .unwrap()
                                .to_string()
                        });
                    a.reference_joint_rad = r.reference_joint_rad;
                }
                if a.lower
                    .into_iter()
                    .chain(a.upper)
                    .any(|p| !(0..=4095).contains(&p))
                {
                    a.coordinate_session = Some(
                        app.state.lock().unwrap()["coordinate_session"]
                            .as_str()
                            .unwrap()
                            .to_string(),
                    );
                }
                next.validate()?;
                // Keep previous measurements as immutable versions before replacing the current record.
                save(
                    &cfg.output.join(format!("calibration-{}.json", stamp())),
                    &next,
                )?;
                save(&path, &next)?;
                cal = next;
                let mut s = app.state.lock().unwrap();
                s["calibration"] = json!(cal);
                s["samples"][r.id.to_string()] = json!(t);
                s["message"] = json!(format!(
                    "{} captured from stationary {} encoder and saved to disk.",
                    r.boundary,
                    if app.execution.is_virtual_calibration() {
                        "simulated"
                    } else {
                        "physical"
                    }
                ));
                return Ok(s.clone());
            }
            Err("Unknown calibration action".into())
        })();
        // Link loss is judged only from a bus call's own error (the stop
        // below, or a readback probe), never from the command's error text,
        // which can be a filesystem failure. A dead socket fails these at
        // once (broken pipe) or within one 150 ms reply timeout.
        if let Err(e) = &result {
            idle_polling = false;
            app.stop.store(true, Ordering::SeqCst);
            verified = false;
            owner.clear();
            let stopped = bus.as_mut().map(|b| {
                b.stop(if selected != 0 {
                    selected
                } else {
                    cfg.roles
                        .keys()
                        .copied()
                        .find(|id| !cal.axes.get(id).is_some_and(|a| a.disabled))
                        .unwrap_or(3)
                })
            });
            let link_lost = virtual_mode
                && matches!(&stopped, Some(Err(stop_error)) if transport_lost(stop_error));
            // The command's text says readback was lost only for a link or
            // frame fault (never device loss: its EIO may be a save failure);
            // the stop's own error may also say the adapter is gone.
            let lost = link_lost
                || link_readback_lost(strip_stop_retry(e))
                || matches!(&stopped, Some(Err(stop_error)) if readback_lost(strip_stop_retry(stop_error)));
            if let Some(b) = bus.as_mut() {
                if lost && selected != 0 {
                    b.reset_turn_tracking_for(selected);
                }
            }
            if link_lost {
                lose_bus(&app, &mut bus);
            }
            let mut s = app.state.lock().unwrap();
            if lost && cal.axes.get(&selected).is_some_and(multi_turn) {
                s["coordinate_session"] = json!(stamp().to_string());
            }
            s["enabled_id"] = Value::Null;
            if lost {
                // A virtual execution is revoked only with its bus (lose_bus).
                s["connected"] = json!(false);
            }
            s["busy"] = json!(false);
            s["error"] = json!(e);
            s["message"] = json!(format!(
                "{e}. {}",
                if link_lost {
                    "Virtual bench link lost; bus closed (the bench torques off on disconnect). Restart and reconnect explicitly"
                } else if stopped.as_ref().is_some_and(|v| v.is_ok()) {
                    "Stopped and readback verified"
                } else {
                    "Physical stop unverified; keep motor power off until resolved"
                }
            ));
        } else if probe_link
            && bus
                .as_mut()
                .and_then(|b| {
                    cfg.roles
                        .keys()
                        .find(|id| !cal.axes.get(*id).is_some_and(|a| a.disabled))
                        .map(|id| b.feedback(*id))
                })
                .is_some_and(|probe| probe.is_err_and(|e| transport_lost(&e)))
        {
            idle_polling = false;
            verified = false;
            owner.clear();
            lose_bus(&app, &mut bus);
            let mut s = app.state.lock().unwrap();
            s["error"] =
                json!("Virtual bench link lost; bus closed. Restart and reconnect explicitly.");
            let message = format!(
                "{} Virtual bench link lost; restart and reconnect explicitly.",
                s["message"].as_str().unwrap_or("")
            );
            s["message"] = json!(message);
        } else {
            app.state.lock().unwrap()["error"] = Value::Null;
        }
        let _ = job.reply.send(result);
    }
    app.latch_stop();
    observe_stop(
        &app,
        &cfg,
        &cal,
        &mut bus,
        handled_stop_epoch,
        &mut selected,
        &mut verified,
        &mut owner,
    );
}
