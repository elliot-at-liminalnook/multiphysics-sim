// One host transport owner. The FPGA owns telemetry, controller timing and PWM.
fn run_device_bench(
    bus: &mut Bus,
    out: &std::path::Path,
    ids: &[u8],
    plan: &sim_runtime::controller_refinement::fpga::Plan,
) -> Result<serde_json::Value, E> {
    use sim_runtime::controller_refinement::{
        fpga_events::{Capture, Event, Kind},
        fpga_upload::{self, Receipt, Upload},
    };
    plan.validate()?;
    if ids != [4, 5, 6, 7, 8, 9, 10, 11, 12]
        || plan.control != "fpga_device_pd"
        || (plan.gains.kp_q8, plan.gains.kd_q8, plan.gains.kv_q8) != (4096, 0, 4096)
    {
        return Err("Device bench needs all-nine stop scope and compiled 4096/0/4096 gains".into());
    }
    let image = fs::read(&plan.bitstream_path)?;
    if blake3::hash(&image).to_hex().as_str() != plan.bitstream_blake3 {
        return Err("FPGA image identity changed".into());
    }
    fs::write(out.join("plan.json"), serde_json::to_vec_pretty(plan)?)?;
    fs::write(
        out.join("host-source.rs.txt"),
        include_bytes!("hx_device.rs.inc"),
    )?;
    fs::write(
        out.join("transport-source.rs.txt"),
        include_bytes!("characterize_hx_bridge.rs"),
    )?;
    fs::write(
        out.join("stop-source.rs.txt"),
        include_bytes!("hx_fpga.rs.inc"),
    )?;
    fs::write(
        out.join("controller-ir.rs.txt"),
        include_bytes!("../../sim-domain-control/src/fixed_pd.rs"),
    )?;
    fs::write(out.join("loaded-image-reference.fs"), image)?;
    let run_id = u32::from_le_bytes(
        blake3::hash(out.to_string_lossy().as_bytes()).as_bytes()[..4].try_into()?,
    );
    let mut capture: Option<Capture> = None;
    let mut initial = serde_json::Map::new();
    let mut stream = File::create(out.join("device-stream.jsonl"))?;
    let mut origin_host_s = 0.;
    let operation = (|| -> Result<(), E> {
        supervisor(bus, servo_safety::Command::Stop)?;
        let capability = bus.txn(254, 0xa2, &[4], 25)?;
        let capability = Receipt::decode(&packet(254, 0, &capability)?)?;
        if capability.capabilities != 7 {
            return Err("Expected autonomous fixed-gain FPGA profile 7".into());
        }
        let mut homes = [0; 9];
        for &id in ids {
            write_checked(bus, id, 0x2c, &[0, 0])?;
            write_checked(bus, id, 0x28, &[0])?;
            let t = feedback(bus, id)?;
            check_feedback(&t, t.position_raw, 180, 58)?;
            if t.speed_raw != 0
                || t.current_raw >= 2000
                || !(600..=3495).contains(&t.position_raw)
                || bus.read(id, 0x21, 1)? != [2]
            {
                return Err(format!(
                    "ID {id}: preflight needs healthy stationary PWM mode and travel clearance"
                )
                .into());
            }
            homes[(id - 4) as usize] = t.position_raw;
            initial.insert(id.to_string(), json!(t));
        }
        let upload = Upload::compile(plan, homes)?;
        fs::write(out.join("upload.json"), serde_json::to_vec_pretty(&upload)?)?;
        // Upload while torque is off; upload never renews the armed command lease.
        let transfer =
            fpga_upload::transfer(&upload, &std::sync::atomic::AtomicBool::new(false), |p| {
                let bytes = bus
                    .txn(p[2], p[4], &p[5..p.len() - 1], 25)
                    .map_err(|e| e.to_string())?;
                packet(254, 0, &bytes).map_err(str::to_string)
            })?;
        fs::write(
            out.join("transfer.json"),
            serde_json::to_vec_pretty(&transfer)?,
        )?;
        if !transfer.completed {
            return Err("Device upload incomplete".into());
        }
        capture = Some(Capture {
            upload,
            packets: Vec::new(),
        });
        for &id in &plan.ids {
            let t = feedback(bus, id)?;
            if t.position_raw != homes[(id - 4) as usize] {
                return Err("Home changed after sealing; START withheld".into());
            }
            supervisor(bus, servo_safety::Command::Arm(id))?.require_armed(id)?;
        }
        sync_pwm(bus, &plan.ids, 0, 10)?;
        sync_torque_on(bus, &plan.ids)?;
        for &id in &plan.ids {
            if bus.read(id, 0x28, 1)? != [1] {
                return Err("Torque enable readback failed".into());
            }
        }
        let mask = plan.ids.iter().fold(0u16, |m, id| m | (1 << (id - 4)));
        let heartbeat = packet(254, 0xa0, &[5, mask as u8, (mask >> 8) as u8])?;
        if out.join("STOP").exists() {
            return Err("Operator STOP before START".into());
        }
        let start = capture.as_ref().unwrap().upload.start_request(run_id)?;
        origin_host_s = bus.start.elapsed().as_secs_f64();
        writeln!(stream, "{}", json!({"host_s":origin_host_s,"tx":start}))?;
        stream.flush()?;
        bus.serial.write_all(&start)?;
        let deadline = Instant::now()
            + Duration::from_secs_f64(plan.period_s * plan.targets.len() as f64 + 1.);
        let mut next_heartbeat = Instant::now() + Duration::from_millis(50);
        let mut heartbeat_pending: Option<Instant> = None;
        let mut started = false;
        let mut terminal = false;
        let mut latched = false;
        while Instant::now() < deadline {
            if out.join("STOP").exists() {
                return Err("Operator STOP".into());
            }
            if heartbeat_pending.is_some_and(|sent| sent.elapsed() > Duration::from_millis(150)) {
                return Err("Heartbeat acknowledgement timeout".into());
            }
            if !terminal
                && !latched
                && heartbeat_pending.is_none()
                && Instant::now() >= next_heartbeat
            {
                writeln!(
                    stream,
                    "{}",
                    json!({"host_s":bus.start.elapsed().as_secs_f64(),"tx":heartbeat})
                )?;
                stream.flush()?;
                bus.serial.write_all(&heartbeat)?;
                heartbeat_pending = Some(Instant::now());
                next_heartbeat = Instant::now() + Duration::from_millis(50);
            }
            if let Some(p) = bus.poll_packet()? {
                writeln!(
                    stream,
                    "{}",
                    json!({"host_s":bus.start.elapsed().as_secs_f64(),"rx":p})
                )?;
                stream.flush()?;
                match p[2] {
                    253 => {
                        capture.as_mut().unwrap().packets.push(p.clone());
                        let event = Event::decode(&p)?;
                        if event.run_id != run_id {
                            return Err("Foreign device run identity".into());
                        }
                        if event.kind == Kind::Telemetry {
                            let t = Telemetry::decode(&event.data)?;
                            check_feedback(&t, homes[(event.motor_id - 4) as usize], 180, 58)?;
                            if t.current_raw >= 2000 {
                                return Err("Raw current threshold".into());
                            }
                        }
                        if event.kind == Kind::Terminal {
                            terminal = true;
                        }
                    }
                    254 if p.len() == 31 => {
                        let receipt = Receipt::decode(&p)?;
                        if started || receipt.capabilities != 7 {
                            return Err("Duplicate START or unexpected capability".into());
                        }
                        receipt.check_start(&capture.as_ref().unwrap().upload)?;
                        started = true;
                    }
                    254 if p.len() == 19 => {
                        if heartbeat_pending.take().is_none() {
                            return Err("Unsolicited local status".into());
                        }
                        let status = servo_safety::Status::decode(&reply(&p, 254, 13)?.parameters)?;
                        status.require_bench_profile()?;
                        latched = status.latched;
                        if !latched {
                            for &id in &plan.ids {
                                status.require_armed(id)?;
                            }
                        }
                    }
                    4..=12 => {
                        if p[4] != 0 {
                            return Err("Motor reported device error".into());
                        }
                    }
                    _ => return Err("Unexpected device stream packet".into()),
                }
            } else {
                std::thread::sleep(Duration::from_micros(100));
            }
            if terminal && heartbeat_pending.is_none() {
                if !started {
                    return Err("Missing START acknowledgement".into());
                }
                let review = capture.as_ref().unwrap().review()?;
                fs::write(
                    out.join("device-review.json"),
                    serde_json::to_vec_pretty(&review)?,
                )?;
                if !review.completed {
                    return Err(
                        format!("Device stopped with result {:?}", review.terminal_result).into(),
                    );
                }
                return Ok(());
            }
        }
        Err("Device capture deadline; no automatic restart".into())
    })();
    // Always request STOP before saving or attempting recovery, including parser errors.
    let stop_packet = servo_safety::Command::Stop.packet()?;
    let stop_write = bus
        .serial
        .write_all(&stop_packet)
        .map_err(|e| e.to_string());
    // Quarantine the trailing stream and STOP receipt before ordinary readback.
    // This explicit recovery boundary never makes a failed capture successful.
    let quarantine_until = Instant::now() + Duration::from_millis(600);
    let mut quarantine = File::create(out.join("recovery-stream.jsonl"))?;
    writeln!(
        quarantine,
        "{}",
        json!({"pending_at_stop":bus.pending.pending(),"stop_write":stop_write})
    )?;
    bus.pending = PacketBuffer::default();
    let mut bytes = [0; 512];
    while Instant::now() < quarantine_until {
        match bus.serial.read(&mut bytes) {
            Ok(n) if n > 0 => {
                writeln!(
                    quarantine,
                    "{}",
                    json!({"host_ns":bus.start.elapsed().as_nanos(),"rx":&bytes[..n]})
                )?;
                quarantine.flush()?;
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => {
                writeln!(quarantine, "{}", json!({"error":e.to_string()}))?;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let stop = supervisor(bus, servo_safety::Command::Stop).map_err(|e| e.to_string());
    let (verified, recovery) = verify_fpga_stop(bus, ids, stop.is_ok() && stop_write.is_ok());
    let failure = operation
        .err()
        .map(|e| e.to_string())
        .or_else(|| (!verified).then(|| "Physical stop not verified".into()));
    let mut result = json!({"completed":false,"failure":failure,"stop_verified":verified,"stop":stop,"recovery":recovery,"initial":initial});
    if let Some(capture) = capture {
        fs::write(
            out.join("device-capture.json"),
            serde_json::to_vec_pretty(&capture)?,
        )?;
        match capture.unverified_recording() {
            Ok(mut recording) => {
                recording.initial["physical_preflight"] = json!(initial);
                recording.transactions_origin_host_s = origin_host_s;
                recording.recovery = recovery;
                recording.stop_verified = verified;
                recording.failure = failure;
                recording.completed = recording.failure.is_none() && capture.review()?.completed;
                recording.sources.insert(
                    "host".into(),
                    blake3::hash(include_bytes!("hx_device.rs.inc"))
                        .to_hex()
                        .to_string(),
                );
                if let Err(e) = recording.validate_capture() {
                    recording.completed = false;
                    recording.failure = Some(e);
                }
                fs::write(
                    out.join("fpga-recording.json"),
                    serde_json::to_vec_pretty(&recording)?,
                )?;
                result["completed"] = json!(recording.completed);
                result["failure"] = json!(recording.failure);
                result["frames"] = json!(recording.frames.len());
            }
            Err(e) => {
                result["conversion_error"] = json!(e);
            }
        }
    }
    fs::write(
        out.join("device-result.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}

#[cfg(test)]
mod device_capture_tests {
    use super::*;
    use std::collections::VecDeque;
    struct MockDevice {
        rx: VecDeque<u8>,
        armed: u16,
        torque: [u8; 9],
        pwm: [[u8; 2]; 9],
        upload: sim_runtime::controller_refinement::fpga_upload::Upload,
        events: Vec<Vec<u8>>,
        written: u16,
        sealed: bool,
        corrupt: bool,
    }
    impl MockDevice {
        fn emit(&mut self, id: u8, p: &[u8]) {
            self.rx.extend(packet(id, 0, p).unwrap());
        }
        fn status(&mut self) {
            self.emit(
                254,
                &[
                    1,
                    (self.armed == 0) as u8,
                    if self.armed == 0 { 10 } else { 0 },
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
                ],
            );
        }
        fn receipt(&mut self, op: u8) {
            let mut p = vec![1, 0xa2, op, 0, self.sealed as u8, 0];
            p.extend(511u16.to_le_bytes());
            p.extend((self.upload.source_plan.targets.len() as u16).to_le_bytes());
            p.extend(self.written.to_le_bytes());
            p.extend(2_000_000u32.to_le_bytes());
            p.extend(
                if self.sealed {
                    self.upload.plan_crc32
                } else {
                    0
                }
                .to_le_bytes(),
            );
            p.extend(50_000_000u32.to_le_bytes());
            p.push(7);
            self.emit(254, &p);
        }
    }
    impl Write for MockDevice {
        fn write(&mut self, p: &[u8]) -> std::io::Result<usize> {
            assert_eq!(p[2..].iter().fold(0u8, |s, b| s.wrapping_add(*b)), 255);
            let params = &p[5..p.len() - 1];
            match p[4] {
                0xa0 => {
                    match params[0] {
                        0 => {
                            self.armed = 0;
                            self.torque = [0; 9];
                            self.pwm = [[0; 2]; 9];
                        }
                        1 => self.armed |= 1 << (params[1] - 4),
                        5 | 2 => {}
                        _ => panic!("unexpected supervisor request"),
                    }
                    self.status();
                }
                0xa2 => {
                    match params[0] {
                        0 => {
                            self.written = 0;
                            self.sealed = false;
                        }
                        1 => self.written += 1,
                        2 => self.sealed = true,
                        3 | 4 => {}
                        _ => panic!("upload op"),
                    }
                    self.receipt(params[0]);
                    if params[0] == 3 {
                        for (index, mut event) in self.events.clone().into_iter().enumerate() {
                            event[8..12].copy_from_slice(&params[5..9]);
                            let last = event.len() - 1;
                            event[last] =
                                !event[2..last].iter().fold(0u8, |s, b| s.wrapping_add(*b));
                            if self.corrupt && index == 1 {
                                event[20] ^= 1;
                            }
                            self.rx.extend(event);
                        }
                        self.armed = 0;
                        self.torque = [0; 9];
                        self.pwm = [[0; 2]; 9];
                    }
                }
                0x83 => {
                    let width = params[1] as usize;
                    for axis in params[2..].chunks(width + 1) {
                        let i = (axis[0] - 4) as usize;
                        match params[0] {
                            0x28 => self.torque[i] = axis[1],
                            0x2c => self.pwm[i].copy_from_slice(&axis[1..]),
                            _ => panic!("sync register"),
                        }
                    }
                    self.status();
                }
                3 => {
                    let i = (p[2] - 4) as usize;
                    match params[0] {
                        0x28 => self.torque[i] = params[1],
                        0x2c => self.pwm[i].copy_from_slice(&params[1..]),
                        _ => panic!("write register"),
                    }
                    self.emit(p[2], &[]);
                }
                2 => {
                    let i = (p[2] - 4) as usize;
                    let mut registers = [0; 80];
                    registers[0x21] = 2;
                    registers[0x28] = self.torque[i];
                    registers[0x2c..0x2e].copy_from_slice(&self.pwm[i]);
                    registers[0x38..0x3a].copy_from_slice(&self.upload.homes[i].to_le_bytes());
                    registers[0x3e] = 120;
                    registers[0x3f] = 40;
                    self.emit(
                        p[2],
                        &registers[params[0] as usize..params[0] as usize + params[1] as usize],
                    );
                }
                _ => panic!("unexpected packet"),
            }
            Ok(p.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Read for MockDevice {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            // Force both fragmentation and coalescing across event boundaries.
            let n = out.len().min(113).min(self.rx.len());
            for b in &mut out[..n] {
                *b = self.rx.pop_front().unwrap();
            }
            Ok(n)
        }
    }
    #[test]
    fn full_uart_capture_and_corruption_both_verify_all_nine_stopped() {
        use sim_runtime::controller_refinement::fpga_events::Capture;
        for corrupt in [false, true] {
            let base=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/actuators/hx30hm/hardware/2026-09-14-full-drive/verification/case0-capture.json");
            let fixture: Capture = serde_json::from_slice(&fs::read(base).unwrap()).unwrap();
            let out = std::env::temp_dir().join(format!(
                "hx-device-protocol-test-{}-{corrupt}",
                std::process::id()
            ));
            fs::create_dir(&out).unwrap();
            let image = out.join("synthetic-image.fs");
            fs::write(&image, b"software test only").unwrap();
            let mut plan = fixture.upload.source_plan.clone();
            plan.bitstream_path = image.to_string_lossy().into();
            plan.bitstream_blake3 = blake3::hash(b"software test only").to_hex().to_string();
            let mock = MockDevice {
                rx: VecDeque::new(),
                armed: 0,
                torque: [0; 9],
                pwm: [[0; 2]; 9],
                upload: fixture.upload,
                events: fixture.packets,
                written: 0,
                sealed: false,
                corrupt,
            };
            let mut bus = Bus {
                serial: Box::new(mock),
                pending: PacketBuffer::default(),
                raw_log: Some(File::create(out.join("uart-chunks.jsonl")).unwrap()),
                log: File::create(out.join("transactions.jsonl")).unwrap(),
                start: Instant::now(),
                seq: 0,
            };
            let result =
                run_device_bench(&mut bus, &out, &(4..=12).collect::<Vec<_>>(), &plan).unwrap();
            assert_eq!(result["stop_verified"], true, "{result:#}");
            assert_eq!(result["completed"], !corrupt, "{result:#}");
            assert!(out.join("device-capture.json").exists());
            if !corrupt {
                let recording: sim_runtime::controller_refinement::fpga::Recording =
                    serde_json::from_slice(&fs::read(out.join("fpga-recording.json")).unwrap())
                        .unwrap();
                recording.validate().unwrap();
                assert_eq!(recording.frames[1].pwm_readback, [1000; 9]);
                assert_eq!(recording.frames[3].pwm_readback, [-1000; 9]);
            }
            // Keep failures inspectable; remove only our generated successful test sandbox.
            fs::remove_dir_all(out).unwrap();
        }
    }
}
