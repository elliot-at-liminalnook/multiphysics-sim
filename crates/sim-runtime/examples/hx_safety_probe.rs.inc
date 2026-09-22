// Finite physical commissioning, using the same serial owner and protocol.
// Intentionally withholds one lease source at a time at 5% PWM on ID 12.
// It does not inject hazardous temperature, voltage or current into hardware.
fn run_safety_probe(
    bus: &mut Bus,
    out: &std::path::Path,
    ids: &[u8],
    physical_s2: bool,
) -> Result<serde_json::Value, E> {
    sim_runtime::controller_refinement::fpga::validate_physical_scope(ids, &[12])?;
    let mut events = File::create(out.join("probe-events.jsonl"))?;
    let mut outcomes = Vec::new();
    let execution = (|| -> Result<(), E> {
        let initial = supervisor(bus, servo_safety::Command::Stop)?;
        writeln!(events, "{}", json!({"phase":"preflight","status":initial}))?;
        let mut home = 0;
        for &id in ids {
            write_checked(bus, id, 0x28, &[0])?;
            write_checked(bus, id, 0x2c, &[0, 0])?;
            let t = feedback(bus, id)?;
            check_feedback(&t, t.position_raw, 200, 60)?;
            if t.speed_raw != 0 || t.current_raw >= 2000 || !(600..=3495).contains(&t.position_raw)
            {
                return Err(format!("ID {id}: unsafe commissioning preflight").into());
            }
            if id == 12 {
                home = t.position_raw;
                if bus.read(id, 0x21, 1)? != [2] {
                    return Err("ID 12 must already be in verified PWM mode".into());
                }
            }
        }
        let watchdogs = [
            ("command_loss", 8),
            ("telemetry_loss", 7),
            ("all_host_traffic_loss", 7),
        ];
        let button = [("physical_s2", 9)];
        for &(name, expected_reason) in if physical_s2 { &button[..] } else { &watchdogs[..] } {
            if out.join("STOP").exists() {
                return Err("operator STOP".into());
            }
            let t = feedback(bus, 12)?;
            check_feedback(&t, home, 200, 60)?;
            if t.speed_raw != 0 || t.current_raw >= 2000 {
                return Err("probe starts moving or overcurrent".into());
            }
            supervisor(bus, servo_safety::Command::Arm(12))?.require_armed(12)?;
            write_checked(bus, 12, 0x2c, &[0, 0])?;
            write_checked(bus, 12, 0x28, &[1])?;
            feedback(bus, 12)?;
            supervisor(bus, servo_safety::Command::Heartbeat(12))?.require_armed(12)?;
            let started = bus.start.elapsed().as_secs_f64();
            sync_pwm(bus, &[12], 50, 10)?;
            let motion_start = Instant::now();
            let deadline = motion_start + Duration::from_millis(if physical_s2 { 8000 } else { 800 });
            let mut trip = None;
            let mut saw_motion = false;
            let mut last_direction = 1;
            if name == "all_host_traffic_loss" {
                // No UART requests at all. The independent FPGA must stop it.
                std::thread::sleep(Duration::from_millis(400));
            }
            while Instant::now() < deadline {
                if out.join("STOP").exists() {
                    return Err("operator STOP".into());
                }
                if name == "command_loss" || physical_s2 {
                    let t = feedback(bus, 12)?;
                    writeln!(
                        events,
                        "{}",
                        json!({"probe":name,"host_s":bus.start.elapsed().as_secs_f64(),"telemetry":t})
                    )?;
                    check_feedback(&t, home, if physical_s2 { 60 } else { 200 }, 58)?;
                    saw_motion |= t.position_raw.abs_diff(home) >= 2;
                    if t.current_raw >= 2000 {
                        return Err("probe current threshold".into());
                    }
                }
                let s = supervisor(
                    bus,
                    if name == "telemetry_loss" || physical_s2 {
                        servo_safety::Command::Heartbeat(12)
                    } else {
                        servo_safety::Command::Status
                    },
                )?;
                let at = bus.start.elapsed().as_secs_f64();
                writeln!(events, "{}", json!({"probe":name,"host_s":at,"status":s}))?;
                events.flush()?;
                if s.latched {
                    if s.reason != expected_reason
                        || s.fault_id != if physical_s2 { 254 } else { 12 }
                        || s.armed_mask != 0
                    {
                        return Err(format!("{name}: unexpected trip {s:?}").into());
                    }
                    trip = Some((at, s));
                    break;
                }
                if physical_s2 {
                    // A short, bounded 5% oscillation gives the operator time
                    // to press S2 without sustained one-direction travel.
                    let direction = if (motion_start.elapsed().as_millis() / 200) % 2 == 0 { 1 } else { -1 };
                    if direction != last_direction {
                        sync_pwm(bus, &[12], 50 * direction, 10)?;
                        last_direction = direction;
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let (trip_s, status) = trip.ok_or("FPGA did not stop before commissioning deadline")?;
            if physical_s2 && !saw_motion {
                return Err("S2 latched, but motion before the button press was not observed".into());
            }
            // Observe autonomous stopping BEFORE any host zero/off command.
            let end = Instant::now() + Duration::from_millis(500);
            let mut stationary_since = None;
            let mut count = 0;
            while Instant::now() < end {
                let t = feedback(bus, 12)?;
                check_feedback(&t, home, 200, 60)?;
                let at = bus.start.elapsed().as_secs_f64();
                writeln!(
                    events,
                    "{}",
                    json!({"probe":name,"phase":"autonomous_stop_tail","host_s":at,"telemetry":t})
                )?;
                if t.speed_raw == 0 {
                    stationary_since.get_or_insert(at);
                    count += 1;
                } else {
                    stationary_since = None;
                    count = 0;
                }
            }
            let pwm = bus.read(12, 0x2c, 2)?;
            let torque = bus.read(12, 0x28, 1)?;
            if pwm != [0, 0]
                || torque != [0]
                || count < 3
                || stationary_since.is_none_or(|s| bus.start.elapsed().as_secs_f64() - s < 0.15)
            {
                return Err(
                    format!("{name}: autonomous mechanical/command stop not verified").into(),
                );
            }
            // Healthy telemetry must not silently rearm the FPGA.
            let still = supervisor(bus, servo_safety::Command::Status)?;
            if !still.latched || still.armed_mask != 0 || still.reason != expected_reason {
                return Err("fault latch did not persist".into());
            }
            outcomes.push(json!({"probe":name,"drive":50,"drive_request_s":started,"trip_observed_s":trip_s,"status":status,"stationary_since_s":stationary_since,"pwm":pwm,"torque_enable":torque,"automatic_rearm":false,"encoder_motion_observed":saw_motion}));
            println!(
                "{name}: fault {expected_reason}, autonomous zero/off and stationary tail verified"
            );
        }
        Ok(())
    })();
    // Cleanup runs on every execution error as well as success.
    let stop = supervisor(bus, servo_safety::Command::Stop).map_err(|e| e.to_string());
    let mut recovery = serde_json::Map::new();
    let mut final_ok = stop.is_ok();
    for &id in ids {
        let result = (|| -> Result<serde_json::Value, E> {
            write_checked(bus, id, 0x2c, &[0, 0])?;
            write_checked(bus, id, 0x28, &[0])?;
            let t = feedback(bus, id)?;
            if t.speed_raw != 0 {
                return Err("final shaft still moving".into());
            }
            Ok(json!({"telemetry":t,"pwm":[0,0],"torque_enable":0}))
        })()
        .map_err(|e| e.to_string());
        final_ok &= result.is_ok();
        recovery.insert(id.to_string(), json!(result));
    }
    Ok(
        json!({"completed":execution.is_ok() && final_ok,"physical_scope_ids":ids,"failure":execution.err().map(|e|e.to_string()),"outcomes":outcomes,"fpga_stop":stop,"recovery":recovery,"physical_s2_test":physical_s2,"limitations":"Host observation windows, not FPGA event timestamps. Telemetry loss is withholding reads; host loss is no UART traffic, not physical cable removal. Broken bus and power isolation are not tested. S2 is tested only in explicit physical_s2 mode with observed prior motion."}),
    )
}
