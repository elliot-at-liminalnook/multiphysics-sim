// Included by characterize_hx_bridge: one serial owner and packet log.
use sim_runtime::acquisition::{
    actuator_sweep::{SweepSchedule, Trial, resume_index, travel_guard},
    servo_safety,
};
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SweepPlan {
    control: String,
    schedule: SweepSchedule,
    pulse_ms: u64,
    rest_ms: u64,
    travel_limit_counts: u16,
    temperature_cutoff_c: u8,
    direction_bit: u8,
    /// Experimental condition, recorded with every resumed run; never inferred.
    bench_condition: String,
    #[serde(default)]
    resume_from: Option<PathBuf>,
}
impl SweepPlan {
    fn validate(&self, ids: &[u8]) -> Result<Vec<Trial>, E> {
        if self.control != "pwm_sweep"
            || ids.iter().any(|id| !(4..=12).contains(id))
            || self.direction_bit != sim_runtime::acquisition::servo_bus::HX30HM_PWM_DIRECTION_BIT
            || !(20..=300).contains(&self.pulse_ms)
            || !(500..=5000).contains(&self.rest_ms)
            || !(100..=1500).contains(&self.travel_limit_counts)
            || !(40..=60).contains(&self.temperature_cutoff_c)
            || self.bench_condition.trim().is_empty()
        {
            return Err("invalid supervised PWM sweep plan".into());
        }
        Ok(self.schedule.trials(ids)?)
    }
    fn identity(&self) -> Result<String, E> {
        let mut p = self.clone();
        p.resume_from = None;
        Ok(blake3::hash(&serde_json::to_vec(&p)?).to_hex().to_string())
    }
}
fn supervisor(bus: &mut Bus, command: servo_safety::Command) -> Result<servo_safety::Status, E> {
    let s = servo_safety::Status::decode(&bus.txn(
        servo_safety::BRIDGE_ID,
        servo_safety::INSTRUCTION,
        &command.parameters()?,
        servo_safety::STATUS_WIDTH,
    )?)?;
    s.require_bench_profile()?;
    Ok(s)
}
fn sync_pwm(bus: &mut Bus, ids: &[u8], drive: i16, bit: u8) -> Result<(), E> {
    let value = signed_pwm_write_parameters(drive, bit)?;
    let mut params = vec![0x2c, 2];
    for id in ids {
        params.extend([*id, value[1], value[2]]);
    }
    // Safety profile returns a local status only after the complete SYNC
    // packet leaves the wire. This is a bridge receipt, NOT a servo ACK.
    let status =
        servo_safety::Status::decode(&bus.txn(254, 0x83, &params, servo_safety::STATUS_WIDTH)?)?;
    status.require_bench_profile()?;
    if drive != 0 {
        for id in ids {
            status.require_armed(*id)?;
        }
    }
    Ok(())
}
fn sweep_feedback(t: &Telemetry, home: u16, plan: &SweepPlan) -> Result<(), E> {
    check_feedback(t, home, plan.travel_limit_counts, plan.temperature_cutoff_c)?;
    if t.current_raw >= 2000 {
        return Err("uncalibrated servo current threshold reached".into());
    }
    travel_guard(
        t.position_raw as f64,
        home as f64,
        t.speed_rad_s * 4096. / std::f64::consts::TAU,
        plan.travel_limit_counts as f64,
        0.3,
    )?;
    Ok(())
}
fn run_sweep(
    bus: &mut Bus,
    out: &std::path::Path,
    ids: &[u8],
    plan: &SweepPlan,
) -> Result<serde_json::Value, E> {
    let trials = plan.validate(ids)?;
    let identity = plan.identity()?;
    let mut completed: Vec<Trial> = Vec::new();
    if let Some(path) = &plan.resume_from {
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
        if saved["plan_identity"] != identity {
            return Err("resume plan/bench condition mismatch".into());
        }
        completed = serde_json::from_value(saved["completed_trials"].clone())?;
        resume_index(&trials, &completed)?;
    }
    let initial_status = supervisor(bus, servo_safety::Command::Stop)?;
    fs::write(
        out.join("fpga-preflight.json"),
        serde_json::to_vec_pretty(&initial_status)?,
    )?;
    let mut homes = std::collections::BTreeMap::new();
    let mut snapshots = serde_json::Map::new();
    let mut csv = File::create(out.join("sweep.csv"))?;
    writeln!(
        csv,
        "trial,id,phase,drive_counts,request_s,completion_s,position_raw,speed_rad_s,voltage_v,temperature_c,current_raw,status"
    )?;
    let mut outcomes = Vec::new();
    let execution = (|| -> Result<(), E> {
        for &id in ids {
            if out.join("STOP").exists() {
                return Err("operator STOP".into());
            }
            if bus.read(id, 5, 1)? != [id] {
                return Err("servo ID verification failed".into());
            }
            write_checked(bus, id, 0x28, &[0])?;
            write_checked(bus, id, 0x2c, &[0, 0, 0, 0])?;
            let t = feedback(bus, id)?;
            if t.speed_raw != 0
                || t.position_raw < plan.travel_limit_counts + 60
                || t.position_raw > 4095 - plan.travel_limit_counts - 60
            {
                return Err(format!(
                    "ID {id}: requires stationary shaft with sufficient travel clearance"
                )
                .into());
            }
            sweep_feedback(&t, t.position_raw, plan)?;
            homes.insert(id, t.position_raw);
            snapshots.insert(
                id.to_string(),
                json!({"telemetry":t,"configuration":bus.read(id,0,40)?}),
            );
            // Torque-off disarms in the FPGA. Fresh read + explicit arm is
            // needed for configuration writes; torque remains off throughout.
            feedback(bus, id)?;
            supervisor(bus, servo_safety::Command::Arm(id))?.require_armed(id)?;
            let mode = bus.read(id, 0x21, 1)?;
            if mode != [0] && mode != [2] {
                return Err("unexpected mode".into());
            }
            if mode != [2] {
                write_checked(bus, id, 0x37, &[0])?;
                write_checked(bus, id, 0x21, &[2])?;
            }
            write_checked(bus, id, 0x37, &[1])?;
            write_checked(bus, id, 0x28, &[0])?;
        }
        fs::write(
            out.join("preflight.json"),
            serde_json::to_vec_pretty(
                &json!({"servos":snapshots,"homes":homes,"plan":plan,"plan_identity":identity}),
            )?,
        )?;
        for trial in trials.iter().skip(completed.len()) {
            if out.join("STOP").exists() {
                return Err("operator STOP".into());
            }
            for &id in &trial.ids {
                let t = feedback(bus, id)?;
                sweep_feedback(&t, homes[&id], plan)?;
                if t.speed_raw != 0 {
                    return Err("trial starts with moving servo".into());
                }
                supervisor(bus, servo_safety::Command::Arm(id))?.require_armed(id)?;
                write_checked(bus, id, 0x2c, &[0, 0])?;
                write_checked(bus, id, 0x28, &[1])?;
            }
            let mut start_positions = std::collections::BTreeMap::new();
            for &id in &trial.ids {
                let t = feedback(bus, id)?;
                sweep_feedback(&t, homes[&id], plan)?;
                start_positions.insert(id, t.position_raw);
                supervisor(bus, servo_safety::Command::Heartbeat(id))?.require_armed(id)?;
            }
            let command_request_s = bus.start.elapsed().as_secs_f64();
            let mut zero_request_s = 0.;
            let mut zero_receipt_s = 0.;
            let start = Instant::now();
            let pulse_end = start + Duration::from_millis(plan.pulse_ms);
            sync_pwm(bus, &trial.ids, trial.drive, plan.direction_bit)?;
            let mut tail =
                std::collections::BTreeMap::<u8, std::collections::VecDeque<(Instant, u16)>>::new();
            let mut peak = std::collections::BTreeMap::<u8, f64>::new();
            for phase in ["pulse", "rest"] {
                let end = if phase == "pulse" {
                    pulse_end
                } else {
                    zero_request_s = bus.start.elapsed().as_secs_f64();
                    sync_pwm(bus, &trial.ids, 0, plan.direction_bit)?;
                    zero_receipt_s = bus.start.elapsed().as_secs_f64();
                    Instant::now() + Duration::from_millis(plan.rest_ms)
                };
                'poll: while Instant::now() < end {
                    for &id in &trial.ids {
                        if Instant::now() >= end {
                            break 'poll;
                        }
                        if out.join("STOP").exists() {
                            return Err("operator STOP".into());
                        }
                        let request = bus.start.elapsed().as_secs_f64();
                        let t = feedback(bus, id)?;
                        writeln!(
                            csv,
                            "{},{},{},{},{:.9},{:.9},{},{:.9},{:.3},{},{},{}",
                            trial.index,
                            id,
                            phase,
                            if phase == "pulse" { trial.drive } else { 0 },
                            request,
                            bus.start.elapsed().as_secs_f64(),
                            t.position_raw,
                            t.speed_rad_s,
                            t.voltage_v,
                            t.temperature_c,
                            t.current_raw,
                            t.status
                        )?;
                        csv.flush()?;
                        sweep_feedback(&t, homes[&id], plan)?;
                        peak.entry(id)
                            .and_modify(|v| *v = v.max(t.speed_rad_s.abs()))
                            .or_insert(t.speed_rad_s.abs());
                        if phase == "rest" {
                            let samples = tail.entry(id).or_default();
                            samples.push_back((Instant::now(), t.speed_raw));
                            while samples
                                .front()
                                .is_some_and(|(at, _)| at.elapsed() > Duration::from_millis(300))
                            {
                                samples.pop_front();
                            }
                        }
                        // Deadline takes priority over lease renewal. Polling
                        // alone never feeds the FPGA command watchdog.
                        if Instant::now() < end {
                            supervisor(bus, servo_safety::Command::Heartbeat(id))?
                                .require_armed(id)?;
                        }
                    }
                }
            }
            let mut displacements = serde_json::Map::new();
            for &id in &trial.ids {
                if !tail.get(&id).is_some_and(|v| {
                    v.len() >= 3
                        && v.iter().all(|(_, speed)| *speed == 0)
                        && v.front().unwrap().0.elapsed() >= Duration::from_millis(150)
                }) {
                    return Err(
                        format!("ID {id}: no verified stationary tail after zero drive").into(),
                    );
                }
                let t = feedback(bus, id)?;
                sweep_feedback(&t, homes[&id], plan)?;
                let delta = t.position_raw as i32 - start_positions[&id] as i32;
                if delta.abs() >= 3 && delta.signum() != i32::from(trial.drive).signum() {
                    return Err("PWM direction mismatch".into());
                }
                if bus.read(id, 0x2c, 2)? != [0, 0] {
                    return Err("zero PWM readback failed".into());
                }
                write_checked(bus, id, 0x28, &[0])?;
                displacements.insert(id.to_string(), json!(delta));
            }
            outcomes.push(json!({"trial":trial,"peak_observed_speed_rad_s":peak,"displacement_counts":displacements,"elapsed_s":start.elapsed().as_secs_f64(),"stationary_verified":true,"drive_request_s":command_request_s,"zero_request_s":zero_request_s,"zero_bridge_receipt_s":zero_receipt_s}));
            completed.push(trial.clone());
            let checkpoint = json!({"plan_identity":identity,"completed_trials":completed,"outcomes_this_run":outcomes});
            // Atomic replacement makes a partial write resumable at the last
            // fully completed trial; output directories are never overwritten.
            fs::write(
                out.join("checkpoint.tmp"),
                serde_json::to_vec_pretty(&checkpoint)?,
            )?;
            fs::rename(out.join("checkpoint.tmp"), out.join("checkpoint.json"))?;
            println!(
                "trial {}/{}: {:?}, PWM {}/1000, stopped",
                trial.index + 1,
                trials.len(),
                trial.ids,
                trial.drive
            );
        }
        Ok(())
    })();
    let stop = supervisor(bus, servo_safety::Command::Stop).map_err(|e| e.to_string());
    let mut recovery = serde_json::Map::new();
    let mut final_ok = true;
    for &id in ids {
        let zero = write_pwm(bus, id, 0).map_err(|e| e.to_string());
        let off = write_checked(bus, id, 0x28, &[0]).map_err(|e| e.to_string());
        let lock = write_checked(bus, id, 0x37, &[1]).map_err(|e| e.to_string());
        let state = (|| -> Result<serde_json::Value, E> {
            let t = feedback(bus, id)?;
            let pwm = bus.read(id, 0x2c, 2)?;
            let torque = bus.read(id, 0x28, 1)?;
            if t.speed_raw != 0 || pwm != [0, 0] || torque != [0] {
                return Err("final stop not verified".into());
            }
            Ok(json!({"telemetry":t,"pwm":pwm,"torque_enable":torque}))
        })()
        .map_err(|e| e.to_string());
        final_ok &= zero.is_ok() && off.is_ok() && lock.is_ok() && state.is_ok();
        recovery.insert(
            id.to_string(),
            json!({"zero":zero,"off":off,"lock":lock,"final_state":state}),
        );
    }
    Ok(
        json!({"completed":execution.is_ok() && final_ok && stop.is_ok(),"failure":execution.err().map(|e|e.to_string()),
        "completed_trials":completed.len(),"total_trials":trials.len(),"fpga_stop":stop,"recovery":recovery,
        "limitation":"Observed pulse peaks are not steady-state speed; no load, torque, current calibration, or thermal-fit claims."}),
    )
}
#[cfg(test)]
mod sweep_tests {
    use super::*;
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };
    // Protocol double only: registers are deliberately stationary. Motor
    // physics is validated separately through the shared compiled runtime.
    struct State {
        registers: [[u8; 71]; 9],
        armed: u16,
        latched: bool,
        motion_packets: usize,
        inject_current: bool,
        saw_current: bool,
    }
    struct Mock {
        state: Arc<Mutex<State>>,
        rx: VecDeque<u8>,
    }
    impl State {
        fn new(inject_current: bool) -> Self {
            let mut s = Self {
                registers: [[0; 71]; 9],
                armed: 0,
                latched: true,
                motion_packets: 0,
                inject_current,
                saw_current: false,
            };
            for (i, r) in s.registers.iter_mut().enumerate() {
                r[5] = (i + 4) as u8;
                r[0x21] = 2;
                r[0x37] = 1;
                r[0x39] = 8;
                r[0x3e] = 120;
                r[0x3f] = 40;
            }
            s
        }
        fn stop(&mut self) {
            self.latched = true;
            self.armed = 0;
            for r in &mut self.registers {
                r[0x28] = 0;
                r[0x2c] = 0;
                r[0x2d] = 0;
            }
        }
        fn status(&self) -> Vec<u8> {
            vec![
                1,
                self.latched as u8,
                if self.latched { 10 } else { 0 },
                254,
                self.armed as u8,
                (self.armed >> 8) as u8,
                255,
                1,
                60,
                90,
                126,
                208,
                7,
            ]
        }
    }
    impl Write for Mock {
        fn write(&mut self, p: &[u8]) -> std::io::Result<usize> {
            assert_eq!(p[2..].iter().fold(0u8, |a, b| a.wrapping_add(*b)), 255);
            let mut s = self.state.lock().unwrap();
            let id = p[2];
            let inst = p[4];
            let params = &p[5..p.len() - 1];
            let payload = if id == 254 && inst == 0xa0 {
                match params[0] {
                    0 | 4 => s.stop(),
                    1 => {
                        s.latched = false;
                        s.armed |= 1 << (params[1] - 4);
                    }
                    2 | 3 => {}
                    _ => panic!("unknown supervisor command"),
                };
                s.status()
            } else if id == 254 && inst == 0x83 {
                assert_eq!(params[..2], [0x2c, 2]);
                for c in params[2..].chunks(3) {
                    let n = (c[0] - 4) as usize;
                    if c[1] != 0 || c[2] != 0 {
                        assert!(!s.latched && s.armed & (1 << n) != 0);
                        s.motion_packets += 1;
                    }
                    s.registers[n][0x2c..0x2e].copy_from_slice(&c[1..]);
                }
                s.status()
            } else {
                let n = (id - 4) as usize;
                let addr = params[0] as usize;
                if inst == 2 {
                    if addr == 0x38 && s.inject_current && s.motion_packets > 0 && !s.saw_current {
                        s.registers[n][0x45] = 208;
                        s.registers[n][0x46] = 7;
                        s.saw_current = true;
                    }
                    s.registers[n][addr..addr + params[1] as usize].to_vec()
                } else {
                    assert_eq!(inst, 3);
                    s.registers[n][addr..addr + params.len() - 1].copy_from_slice(&params[1..]);
                    if addr == 0x28 && params[1] == 0 {
                        s.armed &= !(1 << n);
                    }
                    vec![]
                }
            };
            self.rx.extend(packet(id, 0, &payload).unwrap());
            Ok(p.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Read for Mock {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            // Force short reads, exercising the real transaction assembler.
            let count = b.len().min(self.rx.len()).min(3);
            for byte in &mut b[..count] {
                *byte = self.rx.pop_front().unwrap();
            }
            Ok(count)
        }
    }
    fn run_case(inject: bool) -> (serde_json::Value, Arc<Mutex<State>>, PathBuf) {
        let out =
            std::env::temp_dir().join(format!("hx-sweep-test-{}-{}", std::process::id(), inject));
        if out.exists() {
            fs::remove_dir_all(&out).unwrap();
        }
        fs::create_dir(&out).unwrap();
        let state = Arc::new(Mutex::new(State::new(inject)));
        let mut bus = Bus {
            serial: Box::new(Mock {
                state: state.clone(),
                rx: VecDeque::new(),
            }),
            log: File::create(out.join("transactions.jsonl")).unwrap(),
            start: Instant::now(),
            seq: 0,
        };
        let plan = SweepPlan {
            control: "pwm_sweep".into(),
            schedule: SweepSchedule {
                levels: vec![25],
                repetitions: 1,
                include_concurrent: true,
            },
            pulse_ms: 20,
            rest_ms: 500,
            travel_limit_counts: 1200,
            temperature_cutoff_c: 60,
            direction_bit: 10,
            bench_condition: "software protocol double; stationary registers".into(),
            resume_from: None,
        };
        let result = run_sweep(&mut bus, &out, &(4..=12).collect::<Vec<_>>(), &plan).unwrap();
        (result, state, out)
    }
    #[test]
    fn complete_individual_and_concurrent_protocol_and_checkpoint() {
        let (result, state, out) = run_case(false);
        assert_eq!(result["completed"], true, "{result}");
        assert_eq!(result["completed_trials"], 20);
        let s = state.lock().unwrap();
        assert_eq!(s.motion_packets, 36);
        assert!(s.latched && s.armed == 0);
        assert!(
            s.registers
                .iter()
                .all(|r| r[0x28] == 0 && r[0x2c] == 0 && r[0x2d] == 0)
        );
        let cp: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("checkpoint.json")).unwrap()).unwrap();
        assert_eq!(cp["completed_trials"].as_array().unwrap().len(), 20);
    }
    #[test]
    fn sensor_fault_aborts_escalation_and_cleans_up_every_servo() {
        let (result, state, _) = run_case(true);
        assert_eq!(result["completed"], false);
        assert_eq!(result["completed_trials"], 0);
        assert!(result["failure"].as_str().unwrap().contains("current"));
        let s = state.lock().unwrap();
        assert_eq!(s.motion_packets, 1);
        assert!(s.latched && s.armed == 0);
        assert!(
            s.registers
                .iter()
                .all(|r| r[0x28] == 0 && r[0x2c] == 0 && r[0x2d] == 0)
        );
    }
}
