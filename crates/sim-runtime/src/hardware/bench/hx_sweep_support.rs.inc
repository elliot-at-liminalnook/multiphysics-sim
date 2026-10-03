// Included by characterize_hx_bridge: one serial owner and packet log.
use crate::acquisition::{
    actuator_sweep::{
        CyclicEncoder, DriveSegment, SweepSchedule, Trial, WaveformTemplate, resume_index,
        stationary_tail, travel_guard,
    },
    servo_safety,
};
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SweepPlan {
    control: String,
    schedule: SweepSchedule,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    waveforms: Option<Vec<WaveformTemplate>>,
    pulse_ms: u64,
    rest_ms: u64,
    travel_limit_counts: u16,
    /// Only for externally confirmed unrestricted shafts. This is a sampling
    /// ambiguity bound, not a measured motor maximum or a CAD property.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    free_rotation_maximum_counts_s: Option<f64>,
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
            || self.direction_bit != crate::acquisition::servo_bus::HX30HM_PWM_DIRECTION_BIT
            || !(20..=if self.free_rotation_maximum_counts_s.is_some() {
                5000
            } else {
                300
            })
                .contains(&self.pulse_ms)
            || self
                .free_rotation_maximum_counts_s
                .is_some_and(|v| !v.is_finite() || !(4096.0..=16384.0).contains(&v))
            || !(500..=5000).contains(&self.rest_ms)
            || !(100..=1500).contains(&self.travel_limit_counts)
            || !(40..=60).contains(&self.temperature_cutoff_c)
            || self.bench_condition.trim().is_empty()
        {
            return Err("invalid supervised PWM sweep plan".into());
        }
        Ok(if let Some(waves) = &self.waveforms {
            self.schedule.waveform_trials(ids, waves)?
        } else {
            self.schedule.trials(ids)?
        })
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
fn sync_torque_off(bus: &mut Bus, ids: &[u8]) -> Result<f64, E> {
    let mut params = vec![0x28, 1];
    for id in ids {
        params.extend([*id, 0]);
    }
    servo_safety::Status::decode(&bus.txn(254, 0x83, &params, servo_safety::STATUS_WIDTH)?)?
        .require_bench_profile()?;
    let receipt_s = bus.start.elapsed().as_secs_f64();
    // The FPGA keeps every watchdog armed until this readback confirms off.
    for &id in ids {
        if bus.read(id, 0x28, 1)? != [0] {
            return Err(format!("ID {id}: torque-off readback failed").into());
        }
    }
    Ok(receipt_s)
}
fn sync_torque_on(bus: &mut Bus, ids: &[u8]) -> Result<(), E> {
    let mut params = vec![0x28, 1];
    for id in ids {
        params.extend([*id, 1]);
    }
    let s =
        servo_safety::Status::decode(&bus.txn(254, 0x83, &params, servo_safety::STATUS_WIDTH)?)?;
    s.require_bench_profile()?;
    for &id in ids {
        s.require_armed(id)?;
    }
    Ok(())
}
fn sweep_feedback(t: &Telemetry, home: u16, plan: &SweepPlan) -> Result<(), E> {
    let bounded = plan.free_rotation_maximum_counts_s.is_none();
    check_feedback(
        t,
        if bounded { home } else { t.position_raw },
        plan.travel_limit_counts,
        plan.temperature_cutoff_c,
    )?;
    if t.current_raw >= 2000 {
        return Err("uncalibrated servo current threshold reached".into());
    }
    if bounded {
        travel_guard(
            t.position_raw as f64,
            home as f64,
            t.speed_rad_s * 4096. / std::f64::consts::TAU,
            plan.travel_limit_counts as f64,
            0.3,
        )?;
    }
    Ok(())
}
fn sweep_runner_identity() -> String {
    super::provenance::fingerprint()
}
fn stationary_history(v: &std::collections::VecDeque<(Instant, u16)>, start: Instant) -> bool {
    let samples: Vec<_> = v
        .iter()
        .map(|(at, speed)| (at.duration_since(start).as_secs_f64(), *speed == 0))
        .collect();
    stationary_tail(&samples, 3, 0.15, 0.2).is_ok_and(|ready| ready)
}
fn completed_from_checkpoint(
    saved: &serde_json::Value,
    identity: &str,
    trials: &[Trial],
) -> Result<Vec<Trial>, E> {
    if saved["plan_identity"] != identity
        || saved["runner_identity"] != sweep_runner_identity()
        || saved["schema_version"] != 2
    {
        return Err(
            "resume plan, bench condition, runner source, or checkpoint version mismatch".into(),
        );
    }
    let completed: Vec<Trial> = serde_json::from_value(saved["completed_trials"].clone())?;
    resume_index(trials, &completed)?;
    Ok(completed)
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
        completed = completed_from_checkpoint(&saved, &identity, &trials)?;
    }
    let acquisition_source = super::provenance::retain(out)?;
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
        "trial,id,phase,drive_counts,request_s,completion_s,position_raw,speed_rad_s,voltage_v,temperature_c,current_raw,status,segment_index,torque_enabled,position_unwrapped_counts"
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
                || (plan.free_rotation_maximum_counts_s.is_none()
                    && (t.position_raw < plan.travel_limit_counts + 60
                        || t.position_raw > 4095 - plan.travel_limit_counts - 60))
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
            // Verified torque-off disarms in the FPGA. Fresh read + explicit arm is
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
                &json!({"servos":snapshots,"homes":homes,"plan":plan,"plan_identity":identity,"acquisition_source":acquisition_source}),
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
            }
            // Two group writes keep setup inside the first servo's 200 ms
            // feedback lease. Nine sequential write/readback pairs do not.
            sync_pwm(bus, &trial.ids, 0, plan.direction_bit)?;
            sync_torque_on(bus, &trial.ids)?;
            for &id in &trial.ids {
                if bus.read(id, 0x28, 1)? != [1] {
                    return Err(format!("ID {id}: torque-enable readback failed").into());
                }
            }
            let mut start_positions = std::collections::BTreeMap::new();
            let mut encoders = std::collections::BTreeMap::new();
            for &id in &trial.ids {
                let request = bus.start.elapsed().as_secs_f64();
                let t = feedback(bus, id)?;
                sweep_feedback(&t, homes[&id], plan)?;
                start_positions.insert(id, t.position_raw);
                if let Some(bound) = plan.free_rotation_maximum_counts_s {
                    let mut encoder = CyclicEncoder::new(4096, bound)?;
                    encoder.observe(
                        t.position_raw as u32,
                        request,
                        bus.start.elapsed().as_secs_f64(),
                    )?;
                    encoders.insert(id, encoder);
                }
                supervisor(bus, servo_safety::Command::Heartbeat(id))?.require_armed(id)?;
            }
            let command_request_s = bus.start.elapsed().as_secs_f64();
            let mut zero_request_s = 0.;
            let mut zero_receipt_s = 0.;
            let start = Instant::now();
            let mut segments = if trial.segments.is_empty() {
                vec![DriveSegment {
                    drive: trial.drive,
                    duration_ms: plan.pulse_ms,
                    torque_enabled: true,
                }]
            } else {
                trial.segments.clone()
            };
            let excitation_count = segments.len();
            segments.push(DriveSegment {
                drive: 0,
                duration_ms: plan.rest_ms,
                torque_enabled: segments.last().unwrap().torque_enabled,
            });
            let mut torque_active = true;
            let mut poll_index = 0usize;
            let mut segment_events = Vec::new();
            let mut tail =
                std::collections::BTreeMap::<u8, std::collections::VecDeque<(Instant, u16)>>::new();
            let mut peak = std::collections::BTreeMap::<u8, f64>::new();
            for (segment_index, segment) in segments.iter().enumerate() {
                let is_rest = segment_index == excitation_count;
                let phase = if is_rest {
                    "rest"
                } else if trial.segments.is_empty() {
                    "pulse"
                } else {
                    "excitation"
                };
                if out.join("STOP").exists() {
                    return Err("operator STOP".into());
                }
                let requested_s = bus.start.elapsed().as_secs_f64();
                let until = Instant::now() + Duration::from_millis(segment.duration_ms);
                // Rest is a minimum observation period. Allow a bounded extra
                // 500 ms at zero drive for a late settling observation, while
                // continuing all telemetry, lease and travel checks.
                let settling_until = until + Duration::from_millis(500);
                let command_sent = torque_active;
                let receipt_s = if !segment.torque_enabled && torque_active {
                    let receipt = sync_torque_off(bus, &trial.ids)?;
                    torque_active = false;
                    receipt
                } else if torque_active {
                    sync_pwm(bus, &trial.ids, segment.drive, plan.direction_bit)?;
                    bus.start.elapsed().as_secs_f64()
                } else {
                    bus.start.elapsed().as_secs_f64()
                };
                if is_rest {
                    zero_request_s = requested_s;
                    zero_receipt_s = receipt_s;
                }
                let mut samples = 0usize;
                let mut observed_ids = std::collections::BTreeSet::new();
                while Instant::now() < until
                    || (is_rest
                        && Instant::now() < settling_until
                        && !trial.ids.iter().all(|id| {
                            tail.get(id)
                                .is_some_and(|v| stationary_history(v, bus.start))
                        }))
                {
                    if out.join("STOP").exists() {
                        return Err("operator STOP".into());
                    }
                    let id = trial.ids[poll_index];
                    // Keep a round-robin cursor across command boundaries.
                    // Restarting at the first ID can starve later servos when
                    // waveform segments are shorter than one complete poll.
                    if torque_active {
                        supervisor(bus, servo_safety::Command::Heartbeat(id))?.require_armed(id)?;
                        if !is_rest && Instant::now() >= until {
                            break;
                        }
                    } else if bus.read(id, 0x28, 1)? != [0] {
                        return Err("torque re-enabled during coast observation".into());
                    }
                    let request = bus.start.elapsed().as_secs_f64();
                    let t = feedback(bus, id)?;
                    let completion = bus.start.elapsed().as_secs_f64();
                    let position = if let Some(encoder) = encoders.get_mut(&id) {
                        encoder.observe(t.position_raw as u32, request, completion)?
                    } else {
                        t.position_raw as i64
                    };
                    poll_index = (poll_index + 1) % trial.ids.len();
                    samples += 1;
                    observed_ids.insert(id);
                    writeln!(
                        csv,
                        "{},{},{},{},{:.9},{:.9},{},{:.9},{:.3},{},{},{},{},{},{}",
                        trial.index,
                        id,
                        phase,
                        segment.drive,
                        request,
                        completion,
                        t.position_raw,
                        t.speed_rad_s,
                        t.voltage_v,
                        t.temperature_c,
                        t.current_raw,
                        t.status,
                        segment_index,
                        torque_active,
                        position
                    )?;
                    csv.flush()?;
                    sweep_feedback(&t, homes[&id], plan)?;
                    peak.entry(id)
                        .and_modify(|v| *v = v.max(t.speed_rad_s.abs()))
                        .or_insert(t.speed_rad_s.abs());
                    if is_rest {
                        let list = tail.entry(id).or_default();
                        list.push_back((Instant::now(), t.speed_raw));
                        while list
                            .front()
                            .is_some_and(|(at, _)| at.elapsed() > Duration::from_millis(500))
                        {
                            list.pop_front();
                        }
                    }
                }
                if !is_rest && trial.ids.iter().any(|id| !observed_ids.contains(id)) {
                    return Err(format!("trial {} segment {segment_index}: incomplete feedback coverage; increase segment duration or acquisition rate",trial.index).into());
                }
                segment_events.push(json!({"segment":segment_index,"phase":phase,"command":segment,
                    "request_s":requested_s,"receipt_s":receipt_s,"poll_end_s":bus.start.elapsed().as_secs_f64(),"samples":samples,"command_sent":command_sent}));
            }
            let mut displacements = serde_json::Map::new();
            // Disarm as a group before the slower final per-servo inspection.
            // The FPGA retains each watchdog until its off readback arrives.
            if torque_active {
                sync_torque_off(bus, &trial.ids)?;
            }
            for &id in &trial.ids {
                if !tail
                    .get(&id)
                    .is_some_and(|v| stationary_history(v, bus.start))
                {
                    return Err(
                        format!("ID {id}: no verified stationary tail after zero drive").into(),
                    );
                }
                let t = feedback(bus, id)?;
                sweep_feedback(&t, homes[&id], plan)?;
                // Continuous displacement ends at the last stationary rest
                // sample, before slower disarmed register inspection.
                let delta = encoders
                    .get(&id)
                    .map_or(t.position_raw as i64, CyclicEncoder::position)
                    - start_positions[&id] as i64;
                let one_direction = trial
                    .segments
                    .iter()
                    .filter(|s| s.drive != 0)
                    .all(|s| s.drive.signum() == trial.drive.signum());
                if one_direction
                    && delta.abs() >= 3
                    && delta.signum() != i64::from(trial.drive).signum()
                {
                    return Err("PWM direction mismatch".into());
                }
                if !torque_active {
                    // A PWM write can re-enable torque. Clear the remembered
                    // setpoint only after the coast measurement is stationary,
                    // then disable torque again immediately.
                    write_pwm(bus, id, 0)?;
                    write_checked(bus, id, 0x28, &[0])?;
                }
                if bus.read(id, 0x2c, 2)? != [0, 0] {
                    return Err("zero PWM readback failed".into());
                }
                write_checked(bus, id, 0x28, &[0])?;
                displacements.insert(id.to_string(), json!(delta));
            }
            outcomes.push(json!({"trial":trial,"peak_observed_speed_rad_s":peak,"displacement_counts":displacements,"elapsed_s":start.elapsed().as_secs_f64(),"stationary_verified":true,"drive_request_s":command_request_s,"zero_request_s":zero_request_s,"zero_bridge_receipt_s":zero_receipt_s,"segment_events":segment_events}));
            completed.push(trial.clone());
            let checkpoint = json!({"schema_version":2,"runner_identity":sweep_runner_identity(),"runner_identity_schema":super::provenance::SCHEMA,"plan_identity":identity,"completed_trials":completed,"outcomes_this_run":outcomes,"acquisition_source":acquisition_source});
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
        recovery.insert(
            id.to_string(),
            json!({"zero":zero,"off":off,"lock":lock,"immediate_state":state}),
        );
    }
    // An off acknowledgement is not proof of a stopped shaft. Observe every
    // servo after all stop writes, allowing finite coast-down without issuing
    // further motion commands. Preserve immediate evidence separately.
    let mut stop_tail =
        std::collections::BTreeMap::<u8, std::collections::VecDeque<(Instant, u16)>>::new();
    let stop_until = Instant::now() + Duration::from_secs(2);
    let mut stop_observations = Vec::new();
    while Instant::now() < stop_until {
        for &id in ids {
            let request_s = bus.start.elapsed().as_secs_f64();
            let sample = feedback(bus, id).map_err(|e| e.to_string());
            let list = stop_tail.entry(id).or_default();
            match &sample {
                Ok(t) => list.push_back((Instant::now(), t.speed_raw)),
                Err(_) => list.clear(),
            }
            while list
                .front()
                .is_some_and(|(at, _)| at.elapsed() > Duration::from_millis(500))
            {
                list.pop_front();
            }
            stop_observations.push(json!({"id":id,"request_s":request_s,"completion_s":bus.start.elapsed().as_secs_f64(),"sample":sample}));
        }
        if ids.iter().all(|id| {
            stop_tail
                .get(id)
                .is_some_and(|v| stationary_history(v, bus.start))
        }) {
            break;
        }
    }
    let mut final_ok = true;
    for &id in ids {
        let state = (|| -> Result<serde_json::Value, E> {
            let t = feedback(bus, id)?;
            let pwm = bus.read(id, 0x2c, 2)?;
            let torque = bus.read(id, 0x28, 1)?;
            let lock = bus.read(id, 0x37, 1)?;
            if t.speed_raw != 0
                || t.status != 0
                || pwm != [0, 0]
                || torque != [0]
                || lock != [1]
                || !stop_tail
                    .get(&id)
                    .is_some_and(|v| stationary_history(v, bus.start))
            {
                return Err("final stop not verified after bounded coast observation".into());
            }
            Ok(json!({"telemetry":t,"pwm":pwm,"torque_enable":torque,"nvs_lock":lock}))
        })()
        .map_err(|e| e.to_string());
        final_ok &= state.is_ok();
        recovery.get_mut(&id.to_string()).unwrap()["final_state"] = json!(state);
    }
    Ok(
        json!({"completed":execution.is_ok() && final_ok && stop.is_ok(),"failure":execution.err().map(|e|e.to_string()),
        "completed_trials":completed.len(),"total_trials":trials.len(),"acquisition_source":acquisition_source,"fpga_stop":stop,"recovery":recovery,"settling_extension_limit_ms":500,"stop_observations":stop_observations,"stop_observation_limit_s":2,
        "limitation":"Observed pulse peaks are not steady-state speed; no load, torque, current calibration, or thermal-fit claims."}),
    )
}
include!("hx_sweep_support_tests.rs.inc");
