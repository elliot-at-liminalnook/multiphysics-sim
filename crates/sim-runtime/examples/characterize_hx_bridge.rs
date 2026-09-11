//! Finite HX bench acquisition through the existing transparent FPGA bridge.
//! Host-monotonic transaction windows, NOT FPGA timestamps or sensor sample times.
use serde_json::json;
use sim_runtime::acquisition::servo_bus::{Telemetry, packet, reply};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
type E = Box<dyn std::error::Error>;
struct Bus {
    serial: File,
    log: File,
    start: Instant,
    seq: u64,
}
impl Bus {
    fn txn(&mut self, id: u8, instruction: u8, params: &[u8], width: usize) -> Result<Vec<u8>, E> {
        let p = packet(id, instruction, params)?;
        let t0 = self.start.elapsed().as_nanos();
        self.serial.write_all(&p)?;
        let deadline = Instant::now() + Duration::from_millis(150);
        let mut rx = Vec::new();
        let mut b = [0; 64];
        while Instant::now() < deadline {
            match self.serial.read(&mut b) {
                Ok(n) if n > 0 => rx.extend_from_slice(&b[..n]),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
            if rx.len() >= 4 && rx.len() >= rx[3] as usize + 4 {
                break;
            }
            std::thread::sleep(Duration::from_micros(200));
        }
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
    fn read(&mut self, id: u8, addr: u8, width: u8) -> Result<Vec<u8>, E> {
        self.txn(id, 2, &[addr, width], width as usize)
    }
}
fn main() -> Result<(), E> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err(
            "usage: characterize_hx_bridge PORT SECONDS IDS_COMMA_SEPARATED NEW_OUTPUT".into(),
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
    let out = PathBuf::from(&args[4]);
    fs::create_dir(&out)?;
    let mut manifest = json!({"completed":false,"mode":"read_only","ids":ids,"port":args[1],"baud":115200,"seconds":secs,"started_unix_s":SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64(),"timing":"Host monotonic request/reply windows include USB/FPGA buffering; not device clock or simultaneous sensor samples.","source_blake3":blake3::hash(include_bytes!("characterize_hx_bridge.rs")).to_hex().to_string(),"voltage":"0x3e single byte, 0.1 V/count; temperature is separate byte 0x3f","current":"0.001 A/count uncalibrated internal current; not measured total supply current"});
    fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
    let nonblock = if cfg!(target_os = "macos") { 4 } else { 2048 };
    let serial = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(nonblock)
        .open(&args[1])?;
    let flag = if cfg!(target_os = "macos") {
        "-f"
    } else {
        "-F"
    };
    if !std::process::Command::new("stty")
        .args([
            flag, &args[1], "115200", "raw", "-echo", "clocal", "-hupcl", "min", "0", "time", "0",
        ])
        .status()?
        .success()
    {
        return Err("stty failed".into());
    }
    let mut bus = Bus {
        serial,
        log: File::create(out.join("transactions.jsonl"))?,
        start: Instant::now(),
        seq: 0,
    };
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
