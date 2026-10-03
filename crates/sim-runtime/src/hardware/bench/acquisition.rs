//! Finite HX bench acquisition through the existing transparent FPGA bridge.
//! Host-monotonic transaction windows, NOT FPGA timestamps or sensor sample times.
use crate::acquisition::servo_bus::{
    PacketBuffer, Telemetry, packet, pwm_write_parameters, reply, signed_pwm_write_parameters,
};
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
type E = Box<dyn std::error::Error>;
include!("hx_sweep_support.rs.inc");
include!("hx_safety_probe.rs.inc");
include!("hx_controller.rs.inc");
include!("hx_fpga.rs.inc");
include!("hx_device.rs.inc");
use crate::acquisition::calibration_serial::{CalibrationBus, SerialIo};
struct Bus {
    serial: Box<dyn SerialIo>,
    pending: PacketBuffer,
    raw_log: Option<File>,
    log: File,
    start: Instant,
    seq: u64,
}
impl Bus {
    fn txn(&mut self, id: u8, instruction: u8, params: &[u8], width: usize) -> Result<Vec<u8>, E> {
        let p = packet(id, instruction, params)?;
        let t0 = self.start.elapsed().as_nanos();
        self.serial.write_all(&p)?;
        let rx = self.read_packet(Instant::now() + Duration::from_millis(150))?;
        let result = reply(&rx, id, width);
        let error = result.as_ref().err().copied();
        writeln!(
            self.log,
            "{}",
            json!({"sequence":self.seq,"id":id,"instruction":instruction,"request_host_ns":t0,"completion_host_ns":self.start.elapsed().as_nanos(),"tx":p,"rx":rx,"decode_error":error})
        )?;
        self.log.flush()?;
        self.seq += 1;
        let r = result?;
        if r.error != 0 {
            return Err(format!("servo {id} device error {}", r.error).into());
        }
        Ok(r.parameters)
    }
    /// Preserve USB chunks before parsing; keep coalesced frames for the next read.
    fn poll_packet(&mut self) -> Result<Option<Vec<u8>>, E> {
        if let Some(p) = self.pending.next_packet()? {
            return Ok(Some(p));
        }
        let mut bytes = [0; 512];
        match self.serial.read(&mut bytes) {
            Ok(n) if n > 0 => {
                if let Some(log) = &mut self.raw_log {
                    writeln!(
                        log,
                        "{}",
                        json!({"host_ns":self.start.elapsed().as_nanos(),"rx":&bytes[..n]})
                    )?;
                    log.flush()?;
                }
                self.pending.push(&bytes[..n])?;
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        Ok(self.pending.next_packet()?)
    }
    fn read_packet(&mut self, deadline: Instant) -> Result<Vec<u8>, E> {
        while Instant::now() < deadline {
            if let Some(p) = self.poll_packet()? {
                return Ok(p);
            }
            std::thread::sleep(Duration::from_micros(100));
        }
        Err(format!(
            "Bridge reply timeout; retained {} partial bytes",
            self.pending.pending().len()
        )
        .into())
    }
    /// At most two read-only packets per USB write, matching the installed
    /// bridge's two-slot host queue. Drain both replies before sending more.
    /// Timestamps bound host request/reply windows, not simultaneous sampling.
    fn read_pairs(
        &mut self,
        ids: &[u8],
        addr: u8,
        width: u8,
    ) -> Result<Vec<(u8, f64, f64, Vec<u8>)>, E> {
        let mut results = Vec::new();
        for pair in ids.chunks(2) {
            let packets = pair
                .iter()
                .map(|id| packet(*id, 2, &[addr, width]))
                .collect::<Result<Vec<_>, _>>()?;
            let tx: Vec<u8> = packets.iter().flatten().copied().collect();
            let request = self.start.elapsed();
            self.serial.write_all(&tx)?;
            let deadline = Instant::now() + Duration::from_millis(150);
            for (&id, tx) in pair.iter().zip(packets) {
                let rx = self.read_packet(deadline)?;
                let completion = self.start.elapsed();
                let decoded = reply(&rx, id, width as usize);
                writeln!(
                    self.log,
                    "{}",
                    json!({"sequence":self.seq,"id":id,"instruction":2,"request_host_ns":request.as_nanos(),"completion_host_ns":completion.as_nanos(),"tx":tx,"rx":rx,"decode_error":decoded.as_ref().err().copied(),"paired_read":true})
                )?;
                self.log.flush()?;
                self.seq += 1;
                let decoded = decoded?;
                if decoded.error != 0 {
                    return Err(format!("servo {id} device error {}", decoded.error).into());
                }
                results.push((
                    id,
                    request.as_secs_f64(),
                    completion.as_secs_f64(),
                    decoded.parameters,
                ));
            }
        }
        Ok(results)
    }
    fn read(&mut self, id: u8, addr: u8, width: u8) -> Result<Vec<u8>, E> {
        self.txn(id, 2, &[addr, width], width as usize)
    }
}
pub fn cli(args: Vec<String>) -> Result<(), E> {
    if args.get(1).is_some_and(|a| a == "--validate-sweep") {
        return run_args(args, 115200, || {
            Err("Validation never opens a transport".into())
        });
    }
    if args.len() != 5 && args.len() != 6 {
        return Err(
            "usage: characterize_hx_bridge PORT SECONDS IDS_COMMA_SEPARATED NEW_OUTPUT [PROGRESSIVE_MOTION_PLAN.json]"
                .into(),
        );
    }
    let port = args[1].clone();
    let baud: u32 = std::env::var("HX_BAUD")
        .unwrap_or_else(|_| "115200".into())
        .parse()?;
    let out = PathBuf::from(&args[4]);
    let _lease = crate::hardware::ownership::DeviceLease::acquire(&port, "acquisition CLI")?;
    run_args(args, baud, || {
        Ok(
            CalibrationBus::open_baud(&port, baud, &out.join("calibration-transport.jsonl"))?
                .into_transport(),
        )
    })
}
fn run_args(
    args: Vec<String>,
    baud: u32,
    open: impl FnOnce() -> Result<Box<dyn SerialIo>, E>,
) -> Result<(), E> {
    if args.get(1).is_some_and(|a| a == "--validate-sweep") {
        if args.len() != 4 {
            return Err("usage: --validate-sweep PLAN.json IDS_COMMA_SEPARATED".into());
        }
        let plan: SweepPlan = serde_json::from_slice(&fs::read(&args[2])?)?;
        let ids: Vec<u8> = args[3]
            .split(',')
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        let trials = plan.validate(&ids)?;
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"software_only":true,"plan_identity":plan.identity()?,"trials":trials,"minimum_excitation_and_rest_s":trials.iter().map(|t| (if t.segments.is_empty(){plan.pulse_ms}else{t.segments.iter().map(|s|s.duration_ms).sum::<u64>()})+plan.rest_ms).sum::<u64>() as f64/1000.})
            )?
        );
        return Ok(());
    }

    if args.len() != 5 && args.len() != 6 {
        return Err(
            "usage: characterize_hx_bridge PORT SECONDS IDS_COMMA_SEPARATED NEW_OUTPUT [PROGRESSIVE_MOTION_PLAN.json]".into(),
        );
    }
    let secs: f64 = args[2].parse()?;
    if !secs.is_finite() || secs <= 0. || secs > 600. {
        return Err("duration must be 0..600 s".into());
    }
    let ids: Vec<u8> = args[3]
        .split(',')
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    if ids.is_empty() || ids.iter().any(|x| *x > 253) {
        return Err("invalid IDs".into());
    }
    if ![115200, 1_000_000].contains(&baud) {
        return Err("Unsupported bridge baud".into());
    }
    let out = PathBuf::from(&args[4]);
    fs::create_dir(&out)?;
    let mut manifest = json!({"completed":false,"mode":"read_only","ids":ids,"port":args[1],"baud":baud,"seconds":secs,"started_unix_s":SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64(),"timing":"Host monotonic request/reply windows include USB/FPGA buffering; not device clock or simultaneous sensor samples.","source_blake3":blake3::hash(include_bytes!("acquisition.rs")).to_hex().to_string(),"voltage":"0x3e single byte, 0.1 V/count; temperature is separate byte 0x3f","current":"0.001 A/count uncalibrated internal current; not measured total supply current"});
    fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
    let serial = open()?;
    let mut bus = Bus {
        serial,
        pending: PacketBuffer::default(),
        raw_log: Some(File::create(out.join("uart-chunks.jsonl"))?),
        log: File::create(out.join("transactions.jsonl"))?,
        start: Instant::now(),
        seq: 0,
    };
    if let Some(plan_path) = args.get(5) {
        let value: serde_json::Value = serde_json::from_slice(&fs::read(plan_path)?)?;
        if value["control"] == "fpga_live_reference"
            || value["control"] == "fpga_live_device_reference"
        {
            let plan: crate::controller_refinement::fpga::Plan =
                serde_json::from_value(value["plan"].clone())?;
            let request = serde_json::from_value(value["request"].clone())?;
            // This finite interactive control process must not inherit a
            // background scheduling class from the launcher. This is a QoS
            // request, not real-time scheduling; all deadlines remain enforced.
            #[cfg(target_os = "macos")]
            {
                unsafe extern "C" {
                    fn pthread_set_qos_class_self_np(class: u32, relative_priority: i32) -> i32;
                }
                // SAFETY: documented Darwin pthread API, current thread only,
                // QOS_CLASS_USER_INTERACTIVE from sys/qos.h; no pointers.
                let error = unsafe { pthread_set_qos_class_self_np(0x21, 0) };
                if error != 0 {
                    return Err(std::io::Error::from_raw_os_error(error).into());
                }
                manifest["host_scheduling"] = json!(
                    "Darwin user-interactive QoS; deadlines still enforced; not hard realtime"
                );
            }
            manifest["mode"] = value["control"].clone();
            manifest["seconds"] = json!(plan.period_s * plan.targets.len() as f64);
            fs::write(
                out.join("live-request.json"),
                serde_json::to_vec_pretty(&value)?,
            )?;
            match if value["control"] == "fpga_live_device_reference" {
                run_device_bench_input(&mut bus, &out, &ids, &plan, Some(&request))
            } else {
                run_fpga_bench_input(&mut bus, &out, &ids, &plan, Some(&request))
            } {
                Ok(result) => {
                    manifest["completed"] = result["completed"].clone();
                    manifest["result"] = result;
                }
                Err(e) => manifest["error"] = json!(e.to_string()),
            }
            fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
            return if manifest["completed"] == true {
                Ok(())
            } else {
                Err("Live hardware session ended; inspect stop verification".into())
            };
        }
        if value["control"] == "fpga_device_pd" {
            if baud != 1_000_000 {
                return Err("Device experiment requires HX_BAUD=1000000".into());
            }
            let plan = serde_json::from_value(value)?;
            manifest["mode"] = json!("fpga_device_clock_pd");
            match run_device_bench(&mut bus, &out, &ids, &plan) {
                Ok(r) => {
                    manifest["completed"] = r["completed"].clone();
                    manifest["result"] = r;
                }
                Err(e) => manifest["error"] = json!(e.to_string()),
            }
            fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
            return if manifest["completed"] == true {
                Ok(())
            } else {
                Err("Device controller trial incomplete; inspect retained evidence".into())
            };
        }
        if value["control"] == "fpga_pd" {
            let plan: crate::controller_refinement::fpga::Plan = serde_json::from_value(value)?;
            manifest["mode"] = json!("fpga_computed_pd_host_scheduled");
            let result = run_fpga_bench(&mut bus, &out, &ids, &plan);
            match result {
                Ok(r) => {
                    manifest["completed"] = r["completed"].clone();
                    manifest["result"] = r;
                }
                Err(e) => manifest["error"] = json!(e.to_string()),
            }
            fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
            return if manifest["completed"] == true {
                Ok(())
            } else {
                Err("FPGA controller trial incomplete; inspect retained evidence".into())
            };
        }
        if value["control"] == "closed_loop_pwm" {
            let plan: ControllerBenchPlan = serde_json::from_value(value)?;
            plan.validate(&ids)?;
            manifest["mode"] = json!("fpga_supervised_closed_loop_pwm");
            manifest["plan"] = serde_json::to_value(&plan)?;
            fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
            match run_controller_bench(&mut bus, &out, &ids, &plan) {
                Ok(result) => {
                    manifest["completed"] = result["completed"].clone();
                    manifest["result"] = result;
                }
                Err(e) => manifest["error"] = json!(e.to_string()),
            }
            fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
            return if manifest["completed"] == true {
                Ok(())
            } else {
                Err(
                    "Controller trial incomplete; inspect retained recording and stop verification"
                        .into(),
                )
            };
        }
        if value["control"] == "inspect" {
            manifest["mode"] = json!("read_only_supervised_bench_inspection");
            let result = (|| -> Result<serde_json::Value, E> {
                let status = supervisor(&mut bus, servo_safety::Command::Status)?;
                let mut devices = serde_json::Map::new();
                for &id in &ids {
                    devices.insert(
                        id.to_string(),
                        json!({
                            "telemetry": feedback(&mut bus, id)?,
                            "mode": bus.read(id, 0x21, 1)?,
                            "torque_enable": bus.read(id, 0x28, 1)?,
                            "pwm": bus.read(id, 0x2c, 2)?,
                            "configuration": bus.read(id, 0, 40)?
                        }),
                    );
                }
                Ok(json!({"supervisor": status, "devices": devices}))
            })();
            manifest["completed"] = json!(result.is_ok());
            match result {
                Ok(result) => manifest["result"] = result,
                Err(error) => manifest["error"] = json!(error.to_string()),
            }
            fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
            return if manifest["completed"] == true {
                Ok(())
            } else {
                Err("read-only inspection failed; inspect run.json".into())
            };
        }
        if value["control"] == "safety_probe" {
            manifest["mode"] = json!("low_drive_hardware_watchdog_commissioning");
            manifest["plan"] = value.clone();
            manifest["probe_source_blake3"] = json!(
                blake3::hash(include_bytes!("hx_safety_probe.rs.inc"))
                    .to_hex()
                    .to_string()
            );
            fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
            match run_safety_probe(&mut bus, &out, &ids, value["physical_s2"] == true) {
                Ok(result) => {
                    manifest["completed"] = result["completed"].clone();
                    manifest["result"] = result;
                }
                Err(e) => manifest["error"] = json!(e.to_string()),
            }
            fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
            return if manifest["completed"] == true {
                Ok(())
            } else {
                Err("hardware watchdog commissioning failed; inspect run.json".into())
            };
        }
        if value["control"] == "pwm_sweep" {
            let plan: SweepPlan = serde_json::from_value(value)?;
            plan.validate(&ids)?;
            manifest["mode"] = json!("fpga_supervised_pwm_sweep");
            manifest["plan"] = serde_json::to_value(&plan)?;
            manifest["sweep_source_blake3"] = json!(
                blake3::hash(include_bytes!("hx_sweep_support.rs.inc"))
                    .to_hex()
                    .to_string()
            );
            fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
            let result = run_sweep(&mut bus, &out, &ids, &plan);
            match result {
                Ok(result) => {
                    manifest["completed"] = result["completed"].clone();
                    manifest["result"] = result;
                }
                Err(e) => {
                    manifest["error"] = json!(e.to_string());
                }
            }
            fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
            return if manifest["completed"] == true {
                Ok(())
            } else {
                Err("supervised sweep stopped; inspect run.json".into())
            };
        }
        if value["control"] == "pwm" {
            let plan: PwmPlan = serde_json::from_value(value)?;
            validate_pwm_plan(&plan)?;
            manifest["mode"] = json!("bounded_open_loop_pwm_pulses");
            manifest["plan"] = serde_json::to_value(&plan)?;
            manifest["seconds"] = serde_json::Value::Null;
            fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
            let result = run_pwm(&mut bus, &out, &ids, &plan)?;
            manifest["completed"] = result["completed"].clone();
            manifest["result"] = result;
            fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
            println!("PWM run complete: {}", manifest["completed"]);
            return if manifest["completed"] == true {
                Ok(())
            } else {
                Err("PWM run stopped; inspect run.json".into())
            };
        }
        let plan: MotionPlan = serde_json::from_value(value)?;
        validate_plan(&plan)?;
        manifest["mode"] = json!("progressive_bounded_position_motion");
        manifest["plan"] = serde_json::to_value(&plan)?;
        manifest["seconds"] = serde_json::Value::Null;
        fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
        let result = run_motion(&mut bus, &out, &ids, &plan)?;
        manifest["result"] = result.clone();
        manifest["completed"] = result["completed"].clone();
        fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
        println!("Motion run complete: {}", result["completed"]);
        return if result["completed"] == true {
            Ok(())
        } else {
            Err("motion escalation stopped; inspect run.json".into())
        };
    }
    let mut csv = File::create(out.join("telemetry.csv"))?;
    writeln!(
        csv,
        "id,request_s,completion_s,position_raw,position_rad,speed_raw,speed_rad_s,load_raw,voltage_raw,voltage_v,temperature_c,status,moving,current_raw,current_a_uncalibrated"
    )?;
    let end = Instant::now() + Duration::from_secs_f64(secs);
    let mut count = 0;
    let mut failures = 0;
    while Instant::now() < end {
        for &id in &ids {
            if Instant::now() >= end {
                break;
            }
            let t0 = bus.start.elapsed().as_secs_f64();
            match bus
                .read(id, 0x38, 15)
                .and_then(|p| Ok(Telemetry::decode(&p)?))
            {
                Ok(t) => {
                    writeln!(
                        csv,
                        "{id},{t0:.9},{:.9},{},{:.9},{},{:.9},{},{},{:.3},{},{},{},{},{:.6}",
                        bus.start.elapsed().as_secs_f64(),
                        t.position_raw,
                        t.position_rad,
                        t.speed_raw,
                        t.speed_rad_s,
                        t.load_raw,
                        t.voltage_raw,
                        t.voltage_v,
                        t.temperature_c,
                        t.status,
                        t.moving,
                        t.current_raw,
                        t.current_a_uncalibrated
                    )?;
                    count += 1;
                }
                Err(e) => {
                    failures += 1;
                    eprintln!("ID {id}: {e}");
                }
            }
        }
        csv.flush()?;
        if failures >= 10 {
            break;
        }
    }
    manifest["completed"] = json!(failures < 10);
    manifest["stop_reason"] = json!(if failures >= 10 {
        "failure_threshold"
    } else {
        "duration_complete"
    });
    manifest["samples"] = json!(count);
    manifest["failures"] = json!(failures);
    manifest["elapsed_s"] = json!(bus.start.elapsed().as_secs_f64());
    fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
    println!("{count} samples, {failures} failures: {}", out.display());
    Ok(())
}

include!("motion.rs.inc");
include!("acquisition_tests.rs.inc");

/// Execute finite acquisition on the calling worker. The lease is owned by `Work`.
pub(super) fn run(
    port: &str,
    ids: &[u8],
    out: &std::path::Path,
    plan: &std::path::Path,
    safety: std::sync::Arc<super::Signals>,
) -> Result<(), String> {
    let args = vec![
        "in-process acquisition".into(),
        port.into(),
        "2".into(),
        ids.iter().map(u8::to_string).collect::<Vec<_>>().join(","),
        out.to_string_lossy().into_owned(),
        plan.to_string_lossy().into_owned(),
    ];
    let log = out.join("calibration-transport.jsonl");
    run_args(args, 1_000_000, || {
        let bus = CalibrationBus::open_baud(port, 1_000_000, &log)?;
        Ok(Box::new(SafetyIo {
            inner: bus.into_transport(),
            safety,
            stopped: false,
        }))
    })
    .map_err(|e| e.to_string())
}
struct SafetyIo {
    inner: Box<dyn SerialIo>,
    safety: std::sync::Arc<super::Signals>,
    stopped: bool,
}
impl SafetyIo {
    fn latch(&mut self) -> std::io::Result<()> {
        if self.safety.expired() {
            self.safety.stop();
        }
        if self.safety.cancelled() && !self.stopped {
            self.stopped = true;
            self.inner
                .write_all(&packet(254, 0xa0, &[0]).map_err(std::io::Error::other)?)?;
        }
        Ok(())
    }
}
impl Read for SafetyIo {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.latch()?;
        self.inner.read(bytes)
    }
}
impl Write for SafetyIo {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.latch()?;
        if self.safety.cancelled() {
            let mut pending = PacketBuffer::default();
            pending.push(bytes).map_err(std::io::Error::other)?;
            while let Some(p) = pending.next_packet().map_err(std::io::Error::other)? {
                let args = &p[5..p.len() - 1];
                let allowed = match p[4] {
                    2 => true,
                    0xa0 => matches!(args.first(), Some(0 | 2 | 4)),
                    3 => {
                        args.first().is_some_and(|r| matches!(*r, 0x28 | 0x2c))
                            && args[1..].iter().all(|v| *v == 0)
                    }
                    0x83 => {
                        args.len() >= 2
                            && matches!(args[0], 0x28 | 0x2c)
                            && args[1] > 0
                            && args[2..]
                                .chunks(usize::from(args[1]) + 1)
                                .all(|axis| axis[1..].iter().all(|v| *v == 0))
                    }
                    _ => false,
                };
                if !allowed {
                    return Err(std::io::Error::other(
                        "Acquisition cancelled; only STOP and release readback are accepted",
                    ));
                }
            }
            if !pending.pending().is_empty() {
                return Err(std::io::Error::other("Partial command refused after STOP"));
            }
        }
        self.inner.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl Drop for SafetyIo {
    fn drop(&mut self) {
        self.safety.stop();
        // Panic/unwind or acquisition failure: write STOP without waiting on the
        // ordinary queue. This is best effort; no release readback is inferred.
        if let Ok(stop) = packet(254, 0xa0, &[0]) {
            let _ = self.inner.write_all(&stop);
        }
    }
}
