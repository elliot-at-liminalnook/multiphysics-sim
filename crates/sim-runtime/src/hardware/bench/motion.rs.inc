#[derive(Clone, serde::Deserialize, serde::Serialize)]
struct MotionStage {
    name: String,
    amplitude_counts: u16,
    speed_counts_s: u16,
    dwell_s: f64,
    repetitions: usize,
}
#[derive(serde::Deserialize, serde::Serialize)]
struct MotionPlan {
    stages: Vec<MotionStage>,
    temperature_cutoff_c: u8,
    #[serde(default)]
    concurrent: bool,
}
fn validate_plan(plan: &MotionPlan) -> Result<(), E> {
    if plan.stages.is_empty()
        || plan.stages.len() > 12
        || !(40..=60).contains(&plan.temperature_cutoff_c)
    {
        return Err("invalid motion plan limits".into());
    }
    let mut previous_speed = 0;
    let mut previous_amplitude = 0;
    for s in &plan.stages {
        if s.name.is_empty()
            || s.amplitude_counts == 0
            || s.amplitude_counts > 1024
            || s.speed_counts_s == 0
            || s.speed_counts_s > 6000
            || !s.dwell_s.is_finite()
            || s.dwell_s < 0.5
            || s.dwell_s > 5.
            || s.repetitions == 0
            || s.repetitions > 20
        {
            return Err("invalid stage bounds".into());
        }
        if s.speed_counts_s < previous_speed || s.amplitude_counts < previous_amplitude {
            return Err("stages must escalate monotonically".into());
        }
        if 2. * s.amplitude_counts as f64 / s.speed_counts_s as f64 + 0.3 > s.dwell_s {
            return Err("insufficient movement and settling time".into());
        }
        previous_speed = s.speed_counts_s;
        previous_amplitude = s.amplitude_counts;
    }
    Ok(())
}
fn sync_positions(targets: &[(u8, u16)]) -> Result<Vec<u8>, E> {
    let mut seen = std::collections::BTreeSet::new();
    if targets.is_empty() || targets.len() > 18 {
        return Err("SYNC WRITE exceeds bridge capacity".into());
    }
    let mut p = vec![0x2a, 2];
    for &(id, target) in targets {
        if id > 253 || target > 4095 || !seen.insert(id) {
            return Err("invalid or duplicate target".into());
        }
        p.push(id);
        p.extend(target.to_le_bytes());
    }
    Ok(p)
}
fn command_bytes(target: u16, speed: u16) -> Vec<u8> {
    let mut p = vec![0x2a];
    p.extend(target.to_le_bytes());
    p.extend([0, 0]);
    p.extend(speed.to_le_bytes());
    p
}
fn write_position(bus: &mut Bus, id: u8, target: u16, speed: u16) -> Result<(), E> {
    let p = command_bytes(target, speed);
    bus.txn(id, 3, &p, 0)?;
    if bus.read(id, 0x2a, 6)? != p[1..] {
        return Err(format!("ID {id}: command readback differs").into());
    }
    Ok(())
}
fn feedback(bus: &mut Bus, id: u8) -> Result<Telemetry, E> {
    Ok(Telemetry::decode(&bus.read(id, 0x38, 15)?)?)
}
fn check_feedback(t: &Telemetry, home: u16, amplitude: u16, cutoff: u8) -> Result<(), E> {
    if t.status != 0
        || t.temperature_c >= cutoff
        || t.voltage_raw < 90
        || t.voltage_raw > 126
        || t.position_raw > 4095
        || t.position_raw.abs_diff(home) > amplitude + 60
    {
        return Err(format!(
            "feedback limit: position={} home={} temperature={} voltage={:.1} status={}",
            t.position_raw, home, t.temperature_c, t.voltage_v, t.status
        )
        .into());
    }
    Ok(())
}
fn run_motion(
    bus: &mut Bus,
    out: &std::path::Path,
    ids: &[u8],
    plan: &MotionPlan,
) -> Result<serde_json::Value, E> {
    validate_plan(plan)?;
    let mut original = std::collections::BTreeMap::new();
    let mut homes = std::collections::BTreeMap::new();
    let mut configs = std::collections::BTreeMap::new();
    let max_amplitude = plan
        .stages
        .iter()
        .map(|s| s.amplitude_counts)
        .max()
        .unwrap();
    for &id in ids {
        if bus.read(id, 5, 1)? != [id]
            || bus.read(id, 0x21, 1)? != [0]
            || bus.read(id, 0x28, 1)? != [1]
        {
            return Err(
                format!("ID {id}: identity/position mode/torque-enable preflight failed").into(),
            );
        }
        let t = feedback(bus, id)?;
        check_feedback(&t, t.position_raw, max_amplitude, plan.temperature_cutoff_c)?;
        if t.position_raw < max_amplitude + 60
            || t.position_raw > 4095 - max_amplitude - 60
            || t.speed_raw != 0
        {
            return Err(format!("ID {id}: insufficient travel margin or already moving").into());
        }
        let p = bus.read(id, 0x2a, 6)?;
        let target = u16::from_le_bytes([p[0], p[1]]);
        if target.abs_diff(t.position_raw) > 15 {
            return Err(format!("ID {id}: initial target is not settled").into());
        }
        original.insert(id, p);
        homes.insert(id, t.position_raw);
        configs.insert(id, bus.read(id, 0, 40)?);
    }
    fs::write(
        out.join("preflight.json"),
        serde_json::to_vec_pretty(
            &json!({"homes":homes,"original_target_time_speed":original,"registers_0_through_39":configs,"plan":plan}),
        )?,
    )?;
    let mut csv = File::create(out.join("motion.csv"))?;
    writeln!(
        csv,
        "stage,id,repetition,move_index,command_start_s,request_s,completion_s,target_raw,speed_limit_raw,position_raw,speed_raw,speed_rad_s,voltage_v,temperature_c,current_raw,status"
    )?;
    let mut completed = Vec::new();
    let mut active_ids = Vec::new();
    let execution = (|| -> Result<(), E> {
        for (stage_index, stage) in plan.stages.iter().enumerate() {
            println!(
                "Stage {} {}: +/-{:.1} degrees, command {:.1} degrees/s",
                stage_index + 1,
                stage.name,
                stage.amplitude_counts as f64 * 360. / 4096.,
                stage.speed_counts_s as f64 * 360. / 4096.
            );
            if plan.concurrent {
                active_ids = ids.to_vec();
                for &id in ids {
                    if bus.read(id, 0x21, 1)? != [0] {
                        return Err("mode changed during concurrent test".into());
                    }
                    let t = feedback(bus, id)?;
                    check_feedback(
                        &t,
                        homes[&id],
                        stage.amplitude_counts,
                        plan.temperature_cutoff_c,
                    )?;
                    write_position(bus, id, homes[&id], stage.speed_counts_s)?;
                }
                for repetition in 0..stage.repetitions {
                    for (move_index, sign) in [1i32, -1, 0].into_iter().enumerate() {
                        if out.join("STOP").exists() {
                            return Err("operator STOP requested".into());
                        }
                        let targets: Vec<(u8, u16)> = ids
                            .iter()
                            .map(|&id| {
                                (
                                    id,
                                    (homes[&id] as i32 + sign * stage.amplitude_counts as i32)
                                        as u16,
                                )
                            })
                            .collect();
                        let command_start = bus.start.elapsed().as_secs_f64();
                        let params = sync_positions(&targets)?;
                        let p = packet(254, 0x83, &params)?;
                        bus.serial.write_all(&p)?;
                        // 35 bytes take ~3.04 ms on the host UART, then 0.35 ms
                        // on the servo bus. No ACK for a broadcast SYNC WRITE.
                        std::thread::sleep(Duration::from_millis(5));
                        writeln!(
                            bus.log,
                            "{}",
                            json!({"sequence":bus.seq,"id":254,"instruction":131,"request_host_ns":(command_start*1e9) as u64,"completion_host_ns":bus.start.elapsed().as_nanos(),"tx":p,"rx":[],"no_reply_expected":true,"completion_meaning":"host guard elapsed, not device acknowledgement"})
                        )?;
                        bus.log.flush()?;
                        bus.seq += 1;
                        let until = Instant::now() + Duration::from_secs_f64(stage.dwell_s);
                        let mut tails: std::collections::BTreeMap<
                            u8,
                            std::collections::VecDeque<(Instant, u16)>,
                        > = ids
                            .iter()
                            .map(|&id| (id, std::collections::VecDeque::new()))
                            .collect();
                        while Instant::now() < until {
                            for &(id, target) in &targets {
                                if out.join("STOP").exists() {
                                    return Err("operator STOP requested".into());
                                }
                                let request = bus.start.elapsed().as_secs_f64();
                                let t = feedback(bus, id)?;
                                writeln!(
                                    csv,
                                    "{stage_index},{id},{repetition},{move_index},{command_start:.9},{request:.9},{:.9},{target},{},{},{},{:.9},{:.3},{},{},{}",
                                    bus.start.elapsed().as_secs_f64(),
                                    stage.speed_counts_s,
                                    t.position_raw,
                                    t.speed_raw,
                                    t.speed_rad_s,
                                    t.voltage_v,
                                    t.temperature_c,
                                    t.current_raw,
                                    t.status
                                )?;
                                check_feedback(
                                    &t,
                                    homes[&id],
                                    stage.amplitude_counts,
                                    plan.temperature_cutoff_c,
                                )?;
                                let tail = tails.get_mut(&id).unwrap();
                                tail.push_back((Instant::now(), t.position_raw.abs_diff(target)));
                                while tail
                                    .front()
                                    .is_some_and(|(t, _)| t.elapsed() > Duration::from_millis(250))
                                {
                                    tail.pop_front();
                                }
                            }
                            csv.flush()?;
                        }
                        for &(id, target) in &targets {
                            let tail = &tails[&id];
                            if tail.len() < 3 || tail.iter().any(|(_, e)| *e > 12) {
                                return Err(format!(
                                    "ID {id}: concurrent {} failed settling",
                                    stage.name
                                )
                                .into());
                            }
                            let target_read = bus.read(id, 0x2a, 2)?;
                            if u16::from_le_bytes([target_read[0], target_read[1]]) != target {
                                return Err(
                                    format!("ID {id}: concurrent target readback failed").into()
                                );
                            }
                        }
                    }
                }
                for &id in ids {
                    let mut p = vec![0x2a];
                    p.extend(&original[&id]);
                    bus.txn(id, 3, &p, 0)?;
                    if bus.read(id, 0x2a, 6)? != original[&id] {
                        return Err(format!("ID {id}: concurrent RAM restoration failed").into());
                    }
                    completed.push(
                        json!({"stage":stage_index,"id":id,"name":stage.name,"concurrent":true}),
                    );
                }
                fs::write(
                    out.join("progress.json"),
                    serde_json::to_vec_pretty(&completed)?,
                )?;
                println!(
                    "  All {} servos passed concurrently; original RAM restored",
                    ids.len()
                );
                active_ids.clear();
                continue;
            }
            for &id in ids {
                active_ids = vec![id];
                let home = homes[&id];
                // Recheck identity/mode each stage; another controller must not own this bus.
                if bus.read(id, 0x21, 1)? != [0] {
                    return Err("mode changed during experiment".into());
                }
                for repetition in 0..stage.repetitions {
                    for (move_index, target) in [
                        home + stage.amplitude_counts,
                        home - stage.amplitude_counts,
                        home,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        if out.join("STOP").exists() {
                            return Err("operator STOP requested".into());
                        }
                        let t = feedback(bus, id)?;
                        check_feedback(
                            &t,
                            home,
                            stage.amplitude_counts,
                            plan.temperature_cutoff_c,
                        )?;
                        let command_start = bus.start.elapsed().as_secs_f64();
                        write_position(bus, id, target, stage.speed_counts_s)?;
                        let until = Instant::now() + Duration::from_secs_f64(stage.dwell_s);
                        let mut last = t;
                        let mut tail_errors = std::collections::VecDeque::new();
                        while Instant::now() < until {
                            if out.join("STOP").exists() {
                                return Err("operator STOP requested".into());
                            }
                            let request = bus.start.elapsed().as_secs_f64();
                            last = feedback(bus, id)?;
                            writeln!(
                                csv,
                                "{stage_index},{id},{repetition},{move_index},{command_start:.9},{request:.9},{:.9},{target},{},{},{},{:.9},{:.3},{},{},{}",
                                bus.start.elapsed().as_secs_f64(),
                                stage.speed_counts_s,
                                last.position_raw,
                                last.speed_raw,
                                last.speed_rad_s,
                                last.voltage_v,
                                last.temperature_c,
                                last.current_raw,
                                last.status
                            )?;
                            check_feedback(
                                &last,
                                home,
                                stage.amplitude_counts,
                                plan.temperature_cutoff_c,
                            )?;
                            tail_errors
                                .push_back((Instant::now(), last.position_raw.abs_diff(target)));
                            while tail_errors
                                .front()
                                .is_some_and(|(t, _)| t.elapsed() > Duration::from_millis(150))
                            {
                                tail_errors.pop_front();
                            }
                        }
                        csv.flush()?;
                        // Require a stable tail, not just one sample crossing the target.
                        if tail_errors.len() < 5 || tail_errors.iter().any(|(_, e)| *e > 12) {
                            return Err(format!(
                                "ID {id}: stage {} failed settling, final error {} counts",
                                stage.name,
                                last.position_raw.abs_diff(target)
                            )
                            .into());
                        }
                    }
                }
                // Restore all six original RAM bytes after every stage, then verify them.
                let mut restore = vec![0x2a];
                restore.extend(&original[&id]);
                bus.txn(id, 3, &restore, 0)?;
                if bus.read(id, 0x2a, 6)? != original[&id] {
                    return Err(format!("ID {id}: RAM restore failed").into());
                }
                completed.push(json!({"stage":stage_index,"id":id,"name":stage.name}));
                fs::write(
                    out.join("progress.json"),
                    serde_json::to_vec_pretty(&completed)?,
                )?;
                println!("  ID {id}: passed; original RAM settings restored");
                active_ids.clear();
            }
        }
        Ok(())
    })();
    let mut recovery = Vec::new();
    if let Err(ref error) = execution {
        eprintln!("Stopping escalation: {error}");
        for id in active_ids {
            // Stop toward the latest measured position, not a distant home after a fault.
            let held = (|| -> Result<(), E> {
                let t = feedback(bus, id)?;
                if t.position_raw > 4095 {
                    return Err("cannot hold out-of-range position".into());
                }
                write_position(bus, id, t.position_raw, 600)
            })();
            recovery.push(
                json!({"id":id,"hold_confirmed":held.is_ok(),"error":held.err().map(|e|e.to_string()),"original_RAM_restored":false}),
            );
        }
    }
    let mut final_states = serde_json::Map::new();
    for &id in ids {
        let state = (|| -> Result<serde_json::Value, E> {
            let t = feedback(bus, id)?;
            let p = bus.read(id, 0x2a, 6)?;
            let cfg = bus.read(id, 0, 40)?;
            Ok(
                json!({"telemetry":t,"target_time_speed":p,"original_RAM_restored":p==original[&id],"registers_0_through_39_unchanged":cfg==configs[&id]}),
            )
        })();
        final_states.insert(
            id.to_string(),
            match state {
                Ok(v) => v,
                Err(e) => json!({"error":e.to_string()}),
            },
        );
    }
    Ok(
        json!({"completed":execution.is_ok(),"failure":execution.err().map(|e|e.to_string()),"completed_stages":completed,"recovery":recovery,"final_states":final_states,"elapsed_s":bus.start.elapsed().as_secs_f64()}),
    )
}
#[derive(serde::Deserialize, serde::Serialize)]
struct PwmPlan {
    control: String,
    drive_counts: Vec<i16>,
    #[serde(default)]
    direction_bit: Option<u8>,
    pulse_ms: u64,
    rest_ms: u64,
    travel_limit_counts: u16,
    temperature_cutoff_c: u8,
}
fn validate_pwm_plan(p: &PwmPlan) -> Result<(), E> {
    if p.control != "pwm"
        || p.drive_counts.is_empty()
        || p.drive_counts.len() > 12
        || p.drive_counts
            .iter()
            .any(|&v| v == 0 || v.unsigned_abs() > 200)
        || p.drive_counts
            .windows(2)
            .any(|w| w[1].unsigned_abs() < w[0].unsigned_abs())
        || p.direction_bit.is_some_and(|b| !(10..=15).contains(&b))
        || (p.drive_counts.iter().any(|&v| v < 0) && p.direction_bit.is_none())
        || !(20..=200).contains(&p.pulse_ms)
        || !(500..=3000).contains(&p.rest_ms)
        || !(57..=512).contains(&p.travel_limit_counts)
        || !(40..=60).contains(&p.temperature_cutoff_c)
    {
        return Err("invalid bounded PWM pilot plan".into());
    }
    Ok(())
}
fn write_checked(bus: &mut Bus, id: u8, addr: u8, values: &[u8]) -> Result<(), E> {
    let mut params = vec![addr];
    params.extend(values);
    bus.txn(id, 3, &params, 0)?;
    if bus.read(id, addr, values.len() as u8)? != values {
        return Err(format!("ID {id}: register {addr:#x} readback mismatch").into());
    }
    Ok(())
}
fn write_pwm(bus: &mut Bus, id: u8, drive: u16) -> Result<(), E> {
    let params = pwm_write_parameters(drive)?;
    bus.txn(id, 3, &params, 0)?;
    Ok(())
}
fn run_pwm(
    bus: &mut Bus,
    out: &std::path::Path,
    ids: &[u8],
    plan: &PwmPlan,
) -> Result<serde_json::Value, E> {
    validate_pwm_plan(plan)?;
    let mut snapshots = serde_json::Map::new();
    let mut homes = std::collections::BTreeMap::new();
    for &id in ids {
        if homes.contains_key(&id) || bus.read(id, 5, 1)? != [id] {
            return Err("duplicate or unverified servo ID".into());
        }
        let mode = bus.read(id, 0x21, 1)?[0];
        if mode != 0 && mode != 2 {
            return Err("unexpected initial servo mode".into());
        }
        let t = feedback(bus, id)?;
        check_feedback(&t, t.position_raw, 0, plan.temperature_cutoff_c)?;
        if t.speed_raw != 0
            || t.position_raw < plan.travel_limit_counts + 60
            || t.position_raw > 4095 - plan.travel_limit_counts - 60
        {
            return Err(format!("ID {id}: not stationary or insufficient travel margin").into());
        }
        let lock = bus.read(id, 0x37, 1)?;
        if lock[0] > 1 {
            return Err("unexpected lock value".into());
        }
        snapshots.insert(
            id.to_string(),
            json!({"mode":mode,"telemetry":t,
            "lock":lock,"torque_enable":bus.read(id,0x28,1)?,
            "original_target_time_speed":bus.read(id,0x2a,6)?,
            "registers_0_through_39":bus.read(id,0,40)?}),
        );
        homes.insert(id, t.position_raw);
    }
    fs::write(
        out.join("preflight.json"),
        serde_json::to_vec_pretty(&json!({
            "servos":snapshots,"plan":plan,
            "intent":"Persist PWM mode on tested servos; finish at zero drive with torque disabled",
            "timing":"Host-timed pulses, not an autonomous FPGA watchdog. Nonzero drive is not a self-expiring servo command.",
            "encoding":"Time-register drive; negative commands require explicit direction_bit in the recorded plan"
        }))?,
    )?;
    let mut csv = File::create(out.join("pwm.csv"))?;
    writeln!(
        csv,
        "id,stage,phase,drive_raw,command_start_s,request_s,completion_s,position_raw,speed_raw,speed_rad_s,voltage_v,temperature_c,current_raw,status"
    )?;
    let mut completed = Vec::new();
    let mut recovery = Vec::new();
    let mut active = None;
    let execution = (|| -> Result<(), E> {
        for &id in ids {
            active = Some(id);
            // No nonzero drive is allowed during a mode transition. One-byte
            // writes preserve adjacent acceleration and configuration registers.
            write_checked(bus, id, 0x28, &[0])?;
            write_checked(bus, id, 0x2c, &[0, 0, 0, 0])?;
            if bus.read(id, 0x21, 1)? != [2] {
                write_checked(bus, id, 0x37, &[0])?;
                write_checked(bus, id, 0x21, &[2])?;
            }
            write_checked(bus, id, 0x37, &[1])?;
            write_checked(bus, id, 0x28, &[1])?;
            if feedback(bus, id)?.speed_raw != 0 {
                return Err("motion detected at zero PWM".into());
            }
            println!("ID {id}: PWM mode 2 verified; zero drive verified");
            for (stage, &drive) in plan.drive_counts.iter().enumerate() {
                if out.join("STOP").exists() {
                    return Err("operator STOP requested".into());
                }
                let t = feedback(bus, id)?;
                check_feedback(
                    &t,
                    homes[&id],
                    plan.travel_limit_counts,
                    plan.temperature_cutoff_c,
                )?;
                if t.position_raw.abs_diff(homes[&id]) >= plan.travel_limit_counts {
                    return Err("PWM cumulative travel limit reached".into());
                }
                let command_start = bus.start.elapsed().as_secs_f64();
                let pulse_start_position = t.position_raw;
                let deadline = Instant::now() + Duration::from_millis(plan.pulse_ms);
                let params = if let Some(bit) = plan.direction_bit {
                    signed_pwm_write_parameters(drive, bit)?
                } else {
                    pwm_write_parameters(drive as u16)?
                };
                bus.txn(id, 3, &params, 0)?;
                for phase in ["pulse", "zero_drive"] {
                    let until = if phase == "pulse" {
                        deadline
                    } else {
                        // Zero goes out before any further read or analysis.
                        write_pwm(bus, id, 0)?;
                        if bus.read(id, 0x2c, 2)? != [0, 0] {
                            return Err("PWM zero readback failed".into());
                        }
                        Instant::now() + Duration::from_millis(plan.rest_ms)
                    };
                    let mut tail = std::collections::VecDeque::new();
                    while Instant::now() < until {
                        if out.join("STOP").exists() {
                            return Err("operator STOP requested".into());
                        }
                        let request = bus.start.elapsed().as_secs_f64();
                        let t = feedback(bus, id)?;
                        writeln!(
                            csv,
                            "{id},{stage},{phase},{},{command_start:.9},{request:.9},{:.9},{},{},{:.9},{:.3},{},{},{}",
                            if phase == "pulse" { drive } else { 0 },
                            bus.start.elapsed().as_secs_f64(),
                            t.position_raw,
                            t.speed_raw,
                            t.speed_rad_s,
                            t.voltage_v,
                            t.temperature_c,
                            t.current_raw,
                            t.status
                        )?;
                        csv.flush()?;
                        check_feedback(
                            &t,
                            homes[&id],
                            plan.travel_limit_counts,
                            plan.temperature_cutoff_c,
                        )?;
                        if t.position_raw.abs_diff(homes[&id]) > plan.travel_limit_counts {
                            return Err("PWM cumulative travel limit reached".into());
                        }
                        tail.push_back((Instant::now(), t.speed_raw));
                        while tail
                            .front()
                            .is_some_and(|(at, _)| at.elapsed() > Duration::from_millis(150))
                        {
                            tail.pop_front();
                        }
                    }
                    if phase == "zero_drive"
                        && (tail.len() < 5 || tail.iter().any(|(_, v)| *v != 0))
                    {
                        return Err("servo did not stop after zero PWM".into());
                    }
                }
                let t = feedback(bus, id)?;
                let displacement = t.position_raw as i32 - pulse_start_position as i32;
                if displacement.abs() >= 3 && displacement.signum() != (drive as i32).signum() {
                    return Err(format!("ID {id}: PWM direction mismatch: requested {drive}, displacement {displacement} counts").into());
                }
                completed.push(json!({"id":id,"stage":stage,"drive_raw":drive,
                    "position_raw":t.position_raw,"displacement_counts":displacement,
                    "direction_observed":if displacement.abs()>=3 {Some(displacement.signum())} else {None},
                    "zero_drive_verified":true}));
                fs::write(
                    out.join("progress.json"),
                    serde_json::to_vec_pretty(&completed)?,
                )?;
                println!(
                    "  drive {drive}/1000 for {} ms: stopped, position {}",
                    plan.pulse_ms, t.position_raw
                );
            }
            write_checked(bus, id, 0x28, &[0])?;
            active = None;
        }
        Ok(())
    })();
    if let Some(id) = active {
        // Attempt both independent stop paths even if a reply is lost.
        let zero = write_pwm(bus, id, 0).map_err(|e| e.to_string());
        let disable = write_checked(bus, id, 0x28, &[0]).map_err(|e| e.to_string());
        let lock = write_checked(bus, id, 0x37, &[1]).map_err(|e| e.to_string());
        recovery.push(json!({"id":id,"zero_command_ack":zero.is_ok(),
            "torque_disabled_verified":disable.is_ok(),"lock_verified":lock.is_ok(),
            "zero_error":zero.err(),"disable_error":disable.err(),"lock_error":lock.err()}));
    }
    let mut final_states = serde_json::Map::new();
    for &id in ids {
        let state = (|| -> Result<serde_json::Value, E> {
            Ok(
                json!({"mode":bus.read(id,0x21,1)?,"pwm":bus.read(id,0x2c,2)?,
                "torque_enable":bus.read(id,0x28,1)?,"lock":bus.read(id,0x37,1)?,
                "telemetry":feedback(bus,id)?,"registers_0_through_39":bus.read(id,0,40)?}),
            )
        })();
        final_states.insert(
            id.to_string(),
            match state {
                Ok(v) => v,
                Err(e) => json!({"error":e.to_string()}),
            },
        );
    }
    Ok(
        json!({"completed":execution.is_ok(),"failure":execution.err().map(|e|e.to_string()),
        "completed_pulses":completed,"recovery":recovery,"final_states":final_states,
        "elapsed_s":bus.start.elapsed().as_secs_f64()}),
    )
}
