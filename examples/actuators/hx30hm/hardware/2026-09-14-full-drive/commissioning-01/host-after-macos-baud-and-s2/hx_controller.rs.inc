// Uses the existing serial owner, supervised bridge and shared controller state machine.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerBenchPlan {
    control: String,
    experiment: sim_runtime::controller_refinement::control::Experiment,
    /// A measured timing artifact and its identity must precede choosing this cadence.
    cadence_evidence: String,
    cadence_evidence_blake3: String,
}
impl ControllerBenchPlan {
    fn validate(&self, ids: &[u8]) -> Result<(), E> {
        use sim_runtime::controller_refinement::control::Policy;
        if self.experiment.electrical.as_ref().is_some_and(|e|e.sensing.supply_current_quantum_a.is_some()||e.sensing.winding_current_quantum_a.is_some()){return Err("This bridge exposes voltage and uncalibrated current counts, not calibrated supply or winding amperes. Attach and configure an independent current measurement adapter first.".into());}
        if self.experiment.electrical.as_ref().is_some_and(|e|(e.sensing.voltage_quantum_v-0.1).abs()>1e-12){return Err("The current bridge voltage channel has 0.1 V resolution; the declared controller sensor must match it".into());}


        self.experiment.validate()?;
        let e = &self.experiment;
        let limit = match &e.controller {
            Policy::RustPid { parameters } => parameters.duty_limit,
            Policy::Rhai { duty_limit, .. } => *duty_limit,
        };
        if self.control != "closed_loop_pwm"
            || ids != [4, 5, 6, 7, 8, 9, 10, 11, 12]
            || !(4..=12).contains(&e.device)
            || limit > 0.05
            || e.duration_s > 5.
            || !(0.05..=0.1).contains(&e.timing.period_s)
            || e.trajectory.iter().any(|k| k.position_rad.abs() > 0.25)
            || e.trajectory[0].position_rad != 0.
            || e.timing.maximum_observation_age_s > 0.15
            || e.timing.observation_delay_ticks != 0
            || self.cadence_evidence.trim().is_empty()
            || self.cadence_evidence_blake3.len() != 64
        {
            return Err("Bench controller requires all-nine stop scope, an identified ID4–12, <=5% duty, <=0.25 rad trajectory, <=5 s, 50–100 ms cadence, fresh undelayed observations and recorded cadence evidence".into());
        }
        Ok(())
    }
}
fn run_controller_bench(
    bus: &mut Bus,
    out: &std::path::Path,
    ids: &[u8],
    plan: &ControllerBenchPlan,
) -> Result<serde_json::Value, E> {
    use sim_runtime::controller_refinement::{
        control::{ControllerSession, Feedback},
        recording::{MeasuredFrame, Recording},
    };
    plan.validate(ids)?;
    let evidence = fs::read(&plan.cadence_evidence)?;
    if blake3::hash(&evidence).to_hex().as_str() != plan.cadence_evidence_blake3 {
        return Err("Cadence evidence file changed".into());
    }
    let mut recording = Recording {
        version: 1,
        experiment: plan.experiment.clone(),
        runtime: sim_runtime::physics_context::RuntimeIdentity::current(),
        frames: vec![],
        stop_request_s: 0.,
        stop_receipt_s: 0.,
        completed: false,
        failure: None,
        stop_verified: false,
        initial_registers: json!({}),
        transactions_origin_host_s: 0.,
        timing_evidence: format!(
            "{}; host-monotonic request/completion windows; encoder observation at midpoint is an assumption, device sample age unknown; raw transactions use the recorded origin offset",
            plan.cadence_evidence
        ),
        source_hashes: std::collections::BTreeMap::from([
            (
                "controller_host".into(),
                blake3::hash(include_bytes!("hx_controller.rs.inc"))
                    .to_hex()
                    .to_string(),
            ),
            ("cadence".into(), plan.cadence_evidence_blake3.clone()),
        ]),
    };
    let mut events = File::create(out.join("controller-frames.jsonl"))?;
    let mut origin = Instant::now();
    let result = (|| -> Result<(), E> {
        let status = supervisor(bus, servo_safety::Command::Stop)?;
        let mut initial = serde_json::Map::new();
        for &id in ids {
            write_checked(bus, id, 0x2c, &[0, 0])?;
            write_checked(bus, id, 0x28, &[0])?;
            let t = feedback(bus, id)?;
            check_feedback(&t, t.position_raw, 200, 60)?;
            if t.speed_raw != 0 || t.current_raw >= 2000 {
                return Err("Bench starts moving or overcurrent".into());
            }
            initial.insert(
                id.to_string(),
                json!({"telemetry":t,"registers":bus.read(id,0,40)?}),
            );
        }
        let id = plan.experiment.device;
        if bus.read(id, 0x21, 1)? != [2] {
            return Err(
                "Controller trial requires previously verified PWM mode; no automatic NVS changes"
                    .into(),
            );
        }
        let t = feedback(bus, id)?;
        let home = t.position_raw;
        if !(600..=3495).contains(&home) {
            return Err("Small-motion controller trial requires encoder travel clearance".into());
        }
        recording.initial_registers = json!({"supervisor":status,"devices":initial});
        recording.experiment.initial_encoder_rad = t.position_rad;
        recording.experiment.voltage_v = t.voltage_v;
        recording.experiment.temperature_c = t.temperature_c as f64;
        let mut controller = ControllerSession::new(recording.experiment.clone())?;
        supervisor(bus, servo_safety::Command::Arm(id))?.require_armed(id)?;
        sync_pwm(bus, &[id], 0, 10)?;
        sync_torque_on(bus, &[id])?;
        if bus.read(id, 0x28, 1)? != [1] {
            return Err("Torque-on verification failed".into());
        }
        origin = Instant::now();
        recording.transactions_origin_host_s = bus.start.elapsed().as_secs_f64();
        fs::write(
            out.join("recording.json"),
            serde_json::to_vec_pretty(&recording)?,
        )?;
        let mut next = 0.;
        while origin.elapsed().as_secs_f64() < recording.experiment.duration_s {
            if out.join("STOP").exists() {
                return Err("Operator STOP".into());
            }
            let now = origin.elapsed().as_secs_f64();
            if now < next {
                std::thread::sleep(Duration::from_secs_f64((next - now).min(0.002)));
                continue;
            }
            supervisor(bus, servo_safety::Command::Heartbeat(id))?.require_armed(id)?;
            let request_s = origin.elapsed().as_secs_f64();
            let t = feedback(bus, id)?;
            let completion_s = origin.elapsed().as_secs_f64();
            check_feedback(&t, home, 200, 60)?;
            if t.current_raw >= 2000 {
                return Err("Controller current threshold".into());
            }
            travel_guard(
                t.position_raw as f64,
                home as f64,
                t.speed_rad_s * 4096. / std::f64::consts::TAU,
                200.,
                0.3,
            )?;
            let time = origin.elapsed().as_secs_f64();
            if time >= recording.experiment.duration_s {
                break;
            }
            let frame = controller.tick(
                time,
                Feedback {
                    electrical:recording.experiment.electrical.as_ref().map(|_|sim_runtime::controller_refinement::power::Observation{supply_voltage_v:t.voltage_v,supply_current_a:None,winding_current_a:None}),
                    observed_s: (request_s + completion_s) * 0.5,
                    received_s: completion_s,
                    encoder_rad: t.position_rad,
                    request_s,
                    completion_s,
                },
            )?;
            let drive_counts = (frame.applied_duty * 1000.).round() as i16;
            let command_request_s = origin.elapsed().as_secs_f64();
            sync_pwm(bus, &[id], drive_counts, 10)?;
            let command_receipt_s = origin.elapsed().as_secs_f64();
            let measured = MeasuredFrame {
                control: frame,
                command_request_s,
                command_receipt_s,
                drive_counts,
                voltage_v: t.voltage_v,
                temperature_c: t.temperature_c as f64,
                current_raw_uncalibrated: t.current_raw,
            };
            writeln!(events, "{}", serde_json::to_string(&measured)?)?;
            events.flush()?;
            recording.frames.push(measured);
            next += recording.experiment.timing.period_s;
            if command_receipt_s > next + recording.experiment.timing.period_s {
                return Err("Controller missed a complete cadence interval; stop instead of bursting overdue commands".into());
            }
        }
        Ok(())
    })();
    recording.stop_request_s = origin.elapsed().as_secs_f64();
    let stop = supervisor(bus, servo_safety::Command::Stop).map_err(|e| e.to_string());
    recording.stop_receipt_s = origin.elapsed().as_secs_f64();
    let mut recovery = serde_json::Map::new();
    let mut final_ok = stop.is_ok();
    for &id in ids {
        let r = (|| -> Result<(), E> {
            write_checked(bus, id, 0x2c, &[0, 0])?;
            write_checked(bus, id, 0x28, &[0])?;
            Ok(())
        })()
        .map_err(|e| e.to_string());
        final_ok &= r.is_ok();
        recovery.insert(id.to_string(), json!({"stop_writes":r}));
    }
    let mut tails =
        std::collections::BTreeMap::<u8, std::collections::VecDeque<(Instant, u16)>>::new();
    let until = Instant::now() + Duration::from_secs(2);
    while Instant::now() < until {
        for &id in ids {
            match feedback(bus, id) {
                Ok(t) => {
                    let history = tails.entry(id).or_default();
                    history.push_back((Instant::now(), t.speed_raw));
                    while history.len() > 32 {
                        history.pop_front();
                    }
                }
                Err(_) => {
                    tails.remove(&id);
                }
            }
        }
        if ids.iter().all(|id| {
            tails
                .get(id)
                .is_some_and(|t| stationary_history(t, bus.start))
        }) {
            break;
        }
    }
    for &id in ids {
        let r = (|| -> Result<serde_json::Value, E> {
            let pwm = bus.read(id, 0x2c, 2)?;
            let torque = bus.read(id, 0x28, 1)?;
            let t = feedback(bus, id)?;
            if pwm != [0, 0]
                || torque != [0]
                || t.speed_raw != 0
                || !tails
                    .get(&id)
                    .is_some_and(|t| stationary_history(t, bus.start))
            {
                return Err("Final stationary PWM0/torque-off state not verified".into());
            }
            Ok(json!({"pwm":pwm,"torque":torque,"telemetry":t}))
        })()
        .map_err(|e| e.to_string());
        final_ok &= r.is_ok();
        recovery.get_mut(&id.to_string()).unwrap()["final_state"] = json!(r);
    }
    recording.stop_verified = final_ok;
    recording.completed = result.is_ok() && final_ok && recording.frames.len() >= 2;
    recording.failure = result
        .err()
        .map(|e| e.to_string())
        .or_else(|| (!final_ok).then(|| "Final stop verification failed".into()));
    if recording.completed {
        if let Err(error)=recording.validate() {
            recording.completed=false;
            recording.failure=Some(format!("Captured evidence validation failed: {error}"));
        }
    }
    fs::write(
        out.join("recording.json"),
        serde_json::to_vec_pretty(&recording)?,
    )?;
    Ok(
        json!({"completed":recording.completed,"failure":recording.failure,"frames":recording.frames.len(),"recording":"recording.json","fpga_stop":stop,"recovery":recovery}),
    )
}
