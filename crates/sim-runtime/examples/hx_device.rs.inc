// One host transport owner. The FPGA owns telemetry, controller timing and PWM.
#[derive(serde::Serialize)]
#[serde(untagged)]
enum DeviceRunCapture {
    Sealed(sim_runtime::controller_refinement::fpga_events::Capture),
    Live(sim_runtime::controller_refinement::live_stream::Capture),
}
impl DeviceRunCapture {
    fn upload(&self)->&sim_runtime::controller_refinement::fpga_upload::Upload {match self {Self::Sealed(c)=>&c.upload,Self::Live(c)=>&c.base}}
    fn packets(&mut self)->&mut Vec<Vec<u8>> {match self {Self::Sealed(c)=>&mut c.packets,Self::Live(c)=>&mut c.packets}}
    fn review(&self)->Result<sim_runtime::controller_refinement::fpga_events::Review,String>{match self{Self::Sealed(c)=>c.review(),Self::Live(c)=>c.review()}}
    fn unverified_recording(&self)->Result<sim_runtime::controller_refinement::fpga::Recording,String>{match self{Self::Sealed(c)=>c.unverified_recording(),Self::Live(c)=>c.unverified_recording()}}
}
fn run_device_bench(bus:&mut Bus,out:&std::path::Path,ids:&[u8],plan:&sim_runtime::controller_refinement::fpga::Plan)->Result<serde_json::Value,E>{
    run_device_bench_input(bus,out,ids,plan,None)
}
fn run_device_bench_input(
    bus: &mut Bus,
    out: &std::path::Path,
    ids: &[u8],
    plan: &sim_runtime::controller_refinement::fpga::Plan,
    live_request:Option<&sim_runtime::controller_refinement::live_reference::Request>,
) -> Result<serde_json::Value, E> {
    use sim_runtime::controller_refinement::{
        fpga_events::{Capture, Event, Kind},
        fpga_upload::{self, Receipt, Upload},
    };
    plan.validate()?;
    if let Some(request)=live_request {
        request.validate(ids)?;
        if plan.ids.len()!=3 || plan.period_s!=0.01 || !(16..=1200).contains(&plan.targets.len()) || plan.targets.iter().flatten().any(|v|*v!=0){return Err("Live device run needs 3 axes, 100 Hz, 16..1200 initial zero references".into());}
    }
    sim_runtime::controller_refinement::fpga::validate_physical_scope(ids, &plan.ids)?;
    if plan.control != "fpga_device_pd" { return Err("Device bench needs autonomous control".into()); }
    plan.gains.validate_power2()?;
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
    let mut capture: Option<DeviceRunCapture> = None;
    let mut initial = serde_json::Map::new();
    let mut stream = File::create(out.join("device-stream.jsonl"))?;
    let mut live = File::create(out.join("live-telemetry.jsonl"))?;
    let mut origin_host_s = 0.;
    let operation = (|| -> Result<(), E> {
        supervisor(bus, servo_safety::Command::Stop)?;
        let capability = bus.txn(254, 0xa2, &[4], 25)?;
        let capability = Receipt::decode(&packet(254, 0, &capability)?)?;
        if capability.capabilities & 3 != 3 || (capability.capabilities & 16 == 0 && (plan.gains.kp_q8,plan.gains.kd_q8,plan.gains.kv_q8)!=(4096,0,4096)) || (live_request.is_some() && capability.capabilities & 8 == 0) {
            return Err("Expected autonomous FPGA profile with compatible gains".into());
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
        let mut base_plan=plan.clone();
        if live_request.is_some(){base_plan.targets=vec![[0;9];2];}
        let upload = Upload::compile(&base_plan, homes)?;
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
        capture = Some(if live_request.is_some() {
            use sim_runtime::controller_refinement::live_stream as ls;
            let config=ls::configuration(run_id,plan.targets.len())?;
            let rx=bus.txn(254,0xa4,&config[5..config.len()-1],25)?;
            let config_receipt=packet(254,0,&rx)?;
            ls::Receipt::decode(&config_receipt)?.check(run_id,plan.targets.len(),&plan.ids,0,0)?;
            let mut c=ls::Capture{base:upload,plan:plan.clone(),run_id,configuration_receipt:config_receipt,batches:vec![],packets:vec![]};
            for first in [0,8] {
                let count=(plan.targets.len()-first).min(8);if count==0{break;}
                let request=ls::append_packet(run_id,first,&plan.ids,&vec![[0;9];count])?;
                let rx=bus.txn(254,0xa4,&request[5..request.len()-1],25)?;let receipt=packet(254,0,&rx)?;
                ls::Receipt::decode(&receipt)?.check(run_id,plan.targets.len(),&plan.ids,first+count,1)?;
                c.batches.push(ls::AcceptedBatch{request,receipt});
            }
            DeviceRunCapture::Live(c)
        } else {DeviceRunCapture::Sealed(Capture{upload,packets:Vec::new()})});
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
        if out.join("STOP").exists() || out.parent().is_some_and(|p|p.join("STOP").exists()) {
            return Err("Operator STOP before START".into());
        }
        let start = capture.as_ref().unwrap().upload().start_request(run_id)?;
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
        let mut next_row=16.min(plan.targets.len());
        let mut observed_frame=0usize;
        let mut previous_pwm=[0i16;9];
        let mut append_pending:Option<(Instant,Vec<u8>,Vec<[i16;9]>)>=None;
        let mut cursor=live_request.map(|r|sim_runtime::controller_refinement::live_reference::Cursor::new(&r.initial));
        let mut fresh=Instant::now();
        let mut live_inputs=File::create(out.join("live-inputs.jsonl"))?;
        while Instant::now() < deadline {
            if out.join("STOP").exists() || out.parent().is_some_and(|p|p.join("STOP").exists()) {
                return Err("Operator STOP".into());
            }
            if append_pending.as_ref().is_some_and(|(at,_,_)|at.elapsed()>Duration::from_millis(150)){return Err("Live append acknowledgement timeout".into());}
            if heartbeat_pending.is_some_and(|sent| sent.elapsed() > Duration::from_millis(150)) {
                return Err("Heartbeat acknowledgement timeout".into());
            }
            if !terminal
                && !latched
                && heartbeat_pending.is_none()
                && append_pending.is_none()
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
            if let Some(request)=live_request {
                use sim_runtime::controller_refinement::{live_stream as ls,live_reference as lr};
                if started && !terminal && !latched && heartbeat_pending.is_none() && append_pending.is_none() && next_row<plan.targets.len() && next_row<=observed_frame+8 {
                    let parent=out.parent().ok_or("Missing live session directory")?;
                    let received:lr::Received=serde_json::from_slice(&fs::read(parent.join("latest-reference.json"))?)?;
                    if received.session!=parent.file_name().unwrap().to_string_lossy(){return Err("Foreign live session".into());}
                    let age=SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64()-received.received_unix_s;
                    if !age.is_finite() || !(0.0..=0.25).contains(&age){return Err("Live reference stale (>250 ms)".into());}
                    if cursor.as_mut().unwrap().advance(&received.sample)?{fresh=Instant::now();}
                    if fresh.elapsed()>Duration::from_millis(250){return Err("Live simulation stalled (>250 ms)".into());}
                    let c=match capture.as_ref().unwrap(){DeviceRunCapture::Live(c)=>c,_=>unreachable!()};
                    let previous=c.plan.targets[next_row-1];
                    let target=request.encode(&received.sample,&previous)?;
                    let rows=ls::segment(previous,target,(plan.targets.len()-next_row).min(8))?;
                    let tx=ls::append_packet(run_id,next_row,&plan.ids,&rows)?;
                    writeln!(live_inputs,"{}",json!({"first_frame":next_row,"rows":rows,"received":received,"input_age_s":age}))?;
                    bus.serial.write_all(&tx)?;append_pending=Some((Instant::now(),tx,rows));
                }
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
                        capture.as_mut().unwrap().packets().push(p.clone());
                        let event = Event::decode(&p)?;
                        if event.run_id != run_id {
                            return Err("Foreign device run identity".into());
                        }
                        observed_frame=observed_frame.max(usize::from(event.frame));
                        if event.kind==Kind::Control {for(i,pwm)in previous_pwm.iter_mut().enumerate(){let raw=u16::from_le_bytes([event.data[2*i],event.data[2*i+1]]);*pwm=(raw&1023)as i16*if raw&1024!=0{-1}else{1};}}
                        if event.kind == Kind::Telemetry {
                            let t = Telemetry::decode(&event.data)?;
                            check_feedback(&t, homes[(event.motor_id - 4) as usize], 180, 58)?;
                            if t.current_raw >= 2000 {
                                return Err("Raw current threshold".into());
                            }
                            let axis=usize::from(event.motor_id-4);
                            let source_plan=match capture.as_ref().unwrap(){DeviceRunCapture::Live(c)=>&c.plan,DeviceRunCapture::Sealed(c)=>&c.upload.source_plan};
                            let previous_target=if event.frame==0{0}else{source_plan.targets[usize::from(event.frame)-1][axis]};
                            writeln!(live, "{}", json!({"frame":event.frame,"id":event.motor_id,"period_s":plan.period_s,"total_frames":plan.targets.len(),"previous_target_counts":previous_target,"pwm":previous_pwm[axis],"pwm_limit":plan.gains.limit,
                                "request_ticks":event.request_ticks,"completion_ticks":event.completion_ticks,
                                "host_s":bus.start.elapsed().as_secs_f64(),"telemetry":t,
                                "home_raw":homes[(event.motor_id-4) as usize]}))?;
                            live.flush()?;
                        }
                        if event.kind == Kind::Terminal {
                            terminal = true;
                        }
                    }
                    254 if p.len()==31 && p[6]==0xa4 => {
                        use sim_runtime::controller_refinement::live_stream as ls;
                        let (_,request,rows)=append_pending.take().ok_or("Unsolicited live append receipt")?;
                        let count=rows.len();
                        ls::Receipt::decode(&p)?.check(run_id,plan.targets.len(),&plan.ids,next_row+count,1)?;
                        let c=match capture.as_mut().unwrap(){DeviceRunCapture::Live(c)=>c,_=>return Err("Live receipt in sealed run".into())};
                        c.plan.targets[next_row..next_row+count].copy_from_slice(&rows);
                        c.plan.targets[next_row+count..].fill(*rows.last().unwrap());
                        c.batches.push(ls::AcceptedBatch{request,receipt:p});next_row+=count;
                    }
                    254 if p.len() == 31 => {
                        let receipt = Receipt::decode(&p)?;
                        if started || receipt.capabilities & 3 != 3 {
                            return Err("Duplicate START or unexpected capability".into());
                        }
                        receipt.check_start(capture.as_ref().unwrap().upload())?;
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
            if terminal && heartbeat_pending.is_none() && append_pending.is_none() {
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
    let mut result = json!({"completed":false,"failure":failure,"stop_verified":verified,"physical_scope_ids":ids,"stop":stop,"recovery":recovery,"initial":initial});
    if let Some(capture) = capture {
        fs::write(
            out.join("device-capture.json"),
            serde_json::to_vec_pretty(&capture)?,
        )?;
        match capture.unverified_recording() {
            Ok(mut recording) => {
                recording.initial["physical_preflight"] = json!(initial);
                recording.initial["physical_scope_ids"] = json!(ids);
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
        read_remaining: usize,
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
            let mask = self.upload.source_plan.ids.iter().fold(0u16, |m, id| m | (1 << (id - 4)));
            p.extend(mask.to_le_bytes());
            p.extend((self.upload.source_plan.targets.len() as u16).to_le_bytes());
            p.extend(self.written.to_le_bytes());
            p.extend(((self.upload.source_plan.period_s * 50_000_000.).round() as u32).to_le_bytes());
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
                    // The real UART arbiter inserts local replies between records.
                    // Keep any partially read record intact, but do not queue a
                    // heartbeat behind the entire synthetic future experiment.
                    let tail = self.rx.split_off(self.read_remaining);
                    self.status();
                    self.rx.extend(tail);
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
            // Retain packet boundaries for the reply arbiter above. Fragmentation
            // and coalescing are independently covered by the Bus tests.
            if self.read_remaining == 0 && !self.rx.is_empty() {
                self.read_remaining = usize::from(self.rx[3]) + 4;
            }
            let n = out.len().min(113).min(self.rx.len()).min(self.read_remaining);
            for b in &mut out[..n] { *b = self.rx.pop_front().unwrap(); }
            self.read_remaining -= n;
            Ok(n)
        }
    }
    fn verify_uart_fixture(fixture_path: &str) {
        use sim_runtime::controller_refinement::fpga_events::Capture;
        for corrupt in [false, true] {
            let base=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(fixture_path);
            let fixture: Capture = serde_json::from_slice(&fs::read(base).unwrap()).unwrap();
            let out = std::env::temp_dir().join(format!(
                "hx-device-protocol-test-{}-{}-{corrupt}",
                std::process::id(), &blake3::hash(fixture_path.as_bytes()).to_hex()[..8]
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
                read_remaining: 0,
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
                run_device_bench(&mut bus, &out, &plan.ids, &plan).unwrap();
            assert_eq!(result["stop_verified"], true, "{result:#}");
            assert_eq!(result["completed"], !corrupt, "{result:#}");
            assert!(out.join("device-capture.json").exists());
            if !corrupt {
                let recording: sim_runtime::controller_refinement::fpga::Recording =
                    serde_json::from_slice(&fs::read(out.join("fpga-recording.json")).unwrap())
                        .unwrap();
                recording.validate().unwrap();
                for id in &plan.ids {
                    assert_eq!(recording.frames[1].pwm_readback[(id - 4) as usize], 1000);
                    assert_eq!(recording.frames[3].pwm_readback[(id - 4) as usize], -1000);
                }
                assert_eq!(recording.initial["physical_scope_ids"], json!(plan.ids));
            }
            // Keep failures inspectable; remove only our generated successful test sandbox.
            fs::remove_dir_all(out).unwrap();
        }
    }
    #[test]
    fn full_uart_capture_and_corruption_both_verify_all_nine_stopped() {
        verify_uart_fixture("../../examples/actuators/hx30hm/hardware/2026-09-14-full-drive/verification/case0-capture.json");
    }
    #[test]
    fn fast_three_motor_uart_capture_and_corruption_verify_scope_stopped() {
        verify_uart_fixture("../../examples/actuators/hx30hm/hardware/2026-09-15-fast-loop/verification/case0-capture.json");
    }
}
