//! Passive HX transaction recorder: FPGA capture stream -> raw + irregular SI log.
//! All motion remains in the existing FPGA controller; this host sends no commands.
use serde_json::{Value, json};
use sim_core::{Channel, QuantityKind};
use sim_runtime::acquisition::{BusTransaction, FrameDecoder};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

fn observation(t: &BusTransaction) -> Option<(Channel, f64, u16)> {
    if t.outcome != 0 || t.device_error != 0 || t.instruction != 2 || t.request.len() != 2 {
        return None;
    }
    // Older FPGA captures read two bytes here: the second byte is temperature,
    // not the upper voltage byte. Accept both widths while retaining raw packets.
    if t.request[0] == 0x3e {
        if ![1, 2].contains(&t.request[1]) || t.reply.len() != t.request[1] as usize {
            return None;
        }
        let raw = t.reply[0] as u16;
        return Some((
            Channel {
                name: format!("servo.{}.voltage", t.device_id),
                kind: QuantityKind::Voltage,
            },
            raw as f64 * 0.1,
            raw,
        ));
    }
    let (name, kind, width, scale, signed) = match t.request[0] {
        0x38 => (
            "position",
            QuantityKind::Angle,
            2,
            std::f64::consts::TAU / 4096.,
            false,
        ),
        0x3a => (
            "speed",
            QuantityKind::AngularVelocity,
            2,
            std::f64::consts::TAU / 4096.,
            true,
        ),
        0x45 => ("current", QuantityKind::Current, 2, 0.001, false),
        0x3f => ("case_temperature", QuantityKind::Temperature, 1, 1., false),
        0x2a => (
            "target_position_readback",
            QuantityKind::Angle,
            2,
            std::f64::consts::TAU / 4096.,
            false,
        ),
        0x2e => (
            "target_speed_readback",
            QuantityKind::AngularVelocity,
            2,
            std::f64::consts::TAU / 4096.,
            true,
        ),
        _ => return None,
    };
    if t.request[1] as usize != width || t.reply.len() != width {
        return None;
    }
    let raw = if width == 1 {
        t.reply[0] as u16
    } else {
        u16::from_le_bytes([t.reply[0], t.reply[1]])
    };
    let value = if signed {
        let m = (raw & 0x7fff) as f64;
        if raw & 0x8000 != 0 { -m } else { m }
    } else {
        raw as f64
    };
    let value = if t.request[0] == 0x3f {
        value + 273.15
    } else {
        value * scale
    };
    Some((
        Channel {
            name: format!("servo.{}.{}", t.device_id, name),
            kind,
        },
        value,
        raw,
    ))
}
fn command(t: &BusTransaction) -> Option<(Channel, f64, u16)> {
    // Retain attempted commands even if the reply fails. A command request is
    // separate from its acknowledgement and from measured actuator motion.
    if t.instruction != 3 || t.request.len() != 3 {
        return None;
    }
    let raw = u16::from_le_bytes([t.request[1], t.request[2]]);
    let (name, kind, counts) = match t.request[0] {
        0x2a => ("position_requested", QuantityKind::Angle, raw as f64),
        0x2e => (
            "speed_requested",
            QuantityKind::AngularVelocity,
            (raw & 0x7fff) as f64 * if raw & 0x8000 != 0 { -1. } else { 1. },
        ),
        _ => return None,
    };
    Some((
        Channel {
            name: format!("servo.{}.{}", t.device_id, name),
            kind,
        },
        counts * std::f64::consts::TAU / 4096.,
        raw,
    ))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = None;
    let mut port = None;
    let mut metadata = None;
    let mut seconds: f64 = 10.;
    let mut output = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str(){
        "--input"|"--port"|"--metadata"|"--seconds"=>{
            let value=args.get(i+1).ok_or("missing option value")?.clone();
            match args[i].as_str(){"--input"=>input=Some(value),"--port"=>port=Some(value),"--metadata"=>metadata=Some(value),_=>seconds=value.parse()?};i+=2;
        },s if !s.starts_with('-') && output.is_none()=>{output=Some(PathBuf::from(s));i+=1;},_=>return Err("usage: capture_hx (--input wire.bin | --port /dev/cu.usbserial-...) --metadata experiment.json [--seconds 10] NEW-output-directory".into())}
    }
    if input.is_some() == port.is_some() || !seconds.is_finite() || seconds <= 0. {
        return Err("choose exactly one source and a finite positive duration".into());
    }
    let metadata: Value = serde_json::from_slice(&fs::read(metadata.ok_or(
        "--metadata is required: record fixture, firmware identity, voltage and load conditions",
    )?)?)?;
    if !metadata.is_object() {
        return Err("metadata must be a JSON object".into());
    }
    let out = output.ok_or("missing output directory")?;
    // Reserve the output before changing serial settings; existing recordings
    // are never overwritten. Serial raw mode uses 100 ms timeout, no writes.
    fs::create_dir(&out)?;
    let manifest = json!({"version":1,"source":if port.is_some(){"serial_hardware_unverified"}else{"offline_wire_replay"},"device":port,"input_file":input,
        "created_unix_ns":SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
        "runtime_identity":sim_runtime::physics_context::RuntimeIdentity::current(),
        "logger_source_blake3":blake3::hash(include_bytes!("capture_hx.rs")).to_hex().to_string(),
        "requested_duration_s":seconds,"baud":1000000,"experiment":metadata,
        "timing":"FPGA request acceptance and reply completion bracket bus acquisition; internal servo sensor age is unknown. Host arrival time is not sample time.",
        "position":"Unsigned 16-bit raw register preserved. SI angle has no zero calibration or rollover unwrapping; never silently assume a 4096-count wrap.",
        "current":"Internal current register from prior bench notes, not independently calibrated total supply current.",
        "command":"Write transactions are requests with reply status, not measured instant of actuator application. Preserve request bytes even for timeout.",
        "resampling":"None. Separate reads remain separate records. This long-form log is not direct input to the legacy uniform-time fitter.",
        "completed":false});
    fs::write(out.join("run.json"), serde_json::to_vec_pretty(&manifest)?)?;
    let mut source = if let Some(p) = &port {
        // Keep the device open while configuring it; FTDI macOS defaults reset
        // when the final descriptor closes.
        let serial = OpenOptions::new().read(true).open(p)?;
        let flag = if cfg!(target_os = "macos") {
            "-f"
        } else {
            "-F"
        };
        if !std::process::Command::new("stty")
            .args([
                flag, p, "1000000", "raw", "-echo", "clocal", "-hupcl", "min", "0", "time", "1",
            ])
            .status()?
            .success()
        {
            return Err("serial configuration failed".into());
        }
        serial
    } else {
        File::open(input.as_ref().unwrap())?
    };
    let mut raw = File::create(out.join("wire.bin"))?;
    let mut events = File::create(out.join("transactions.jsonl"))?;
    let mut csv = File::create(out.join("observations.csv"))?;
    let mut commands = File::create(out.join("commands.csv"))?;
    writeln!(
        commands,
        "epoch,transaction_id,servo_id,request_tick,completion_tick,clock_hz,outcome,device_error,channel,unit,requested_value,raw_u16"
    )?;
    writeln!(
        csv,
        "epoch,transaction_id,servo_id,request_tick,completion_tick,clock_hz,window_start_s,window_end_s,host_receive_elapsed_ns,channel,unit,value,raw_u16"
    )?;
    let mut decoder = FrameDecoder::default();
    let mut buffer = [0; 4096];
    let start = Instant::now();
    let mut previous: Option<BusTransaction> = None;
    let mut epoch = 0u64;
    let mut records = 0u64;
    let mut samples = 0u64;
    let mut invalid_records = 0u64;
    let mut transaction_gaps = 0u64;
    let mut sequence_gaps = 0u64;
    let mut duplicate_ids = 0u64;
    let mut failed_transactions = 0u64;
    let mut max_device_dropped = 0u32;
    let mut hasher = blake3::Hasher::new();
    let mut read_error = None;
    loop {
        if port.is_some() && start.elapsed().as_secs_f64() >= seconds {
            break;
        }
        let n = match source.read(&mut buffer) {
            Ok(n) => n,
            Err(e) => {
                read_error = Some(e.to_string());
                break;
            }
        };
        if n == 0 {
            decoder.discard_partial();
            if port.is_none() {
                break;
            } else {
                continue;
            }
        }
        let receive_ns = start.elapsed().as_nanos();
        raw.write_all(&buffer[..n])?;
        hasher.update(&buffer[..n]);
        for frame in decoder.feed(&buffer[..n]) {
            let t = match BusTransaction::from_frame(&frame) {
                Ok(t) => t,
                Err(e) => {
                    invalid_records += 1;
                    serde_json::to_writer(
                        &mut events,
                        &json!({"invalid_frame":frame,"reason":e,"host_receive_elapsed_ns":receive_ns}),
                    )?;
                    writeln!(events)?;
                    continue;
                }
            };
            let mut reset = false;
            let mut gap = 0;
            if let Some(p) = &previous {
                if t.window.request_tick < p.window.request_tick
                    || t.window.clock_hz != p.window.clock_hz
                {
                    epoch += 1;
                    reset = true;
                } else {
                    if t.transaction_id == p.transaction_id {
                        duplicate_ids += 1;
                    } else {
                        gap = t
                            .transaction_id
                            .wrapping_sub(p.transaction_id)
                            .wrapping_sub(1);
                        transaction_gaps += gap as u64;
                    }
                    sequence_gaps += t
                        .transport_sequence
                        .wrapping_sub(p.transport_sequence)
                        .wrapping_sub(1) as u64;
                }
            }
            records += 1;
            if t.outcome != 0 || t.device_error != 0 {
                failed_transactions += 1;
            }
            max_device_dropped = max_device_dropped.max(t.device_dropped_total);
            let observed = observation(&t);
            if let Some((channel, value, raw)) = command(&t) {
                writeln!(
                    commands,
                    "{epoch},{},{},{},{},{},{},{},{},{},{value:.12},{raw}",
                    t.transaction_id,
                    t.device_id,
                    t.window.request_tick,
                    t.window.completion_tick,
                    t.window.clock_hz,
                    t.outcome,
                    t.device_error,
                    channel.name,
                    channel.unit()
                )?;
            }
            serde_json::to_writer(
                &mut events,
                &json!({"epoch":epoch,"clock_reset_detected":reset,"transaction_gap":gap,"host_receive_elapsed_ns":receive_ns,"transaction":t,
                "observation":observed.as_ref().map(|(c,v,raw)|json!({"channel":c,"value":v,"raw_u16":raw}))}),
            )?;
            writeln!(events)?;
            if let Some((c, v, raw)) = observed {
                writeln!(
                    csv,
                    "{epoch},{},{},{},{},{},{:.9},{:.9},{receive_ns},{},{},{v:.12},{raw}",
                    t.transaction_id,
                    t.device_id,
                    t.window.request_tick,
                    t.window.completion_tick,
                    t.window.clock_hz,
                    t.window.start_s(),
                    t.window.end_s(),
                    c.name,
                    c.unit()
                )?;
                samples += 1;
            }
            previous = Some(t);
        }
        raw.flush()?;
        events.flush()?;
        csv.flush()?;
        commands.flush()?;
    }
    decoder.discard_partial();
    raw.sync_all()?;
    events.sync_all()?;
    csv.sync_all()?;
    commands.sync_all()?;
    let mut receipt = manifest;
    receipt["completed"] = json!(read_error.is_none());
    receipt["read_error"] = json!(read_error);
    receipt["elapsed_host_s"] = json!(start.elapsed().as_secs_f64());
    receipt["wire_blake3"] = json!(hasher.finalize().to_hex().to_string());
    receipt["quality"] = json!({"records":records,"observations":samples,"decoder":decoder.statistics,"invalid_records":invalid_records,"transaction_gaps":transaction_gaps,"transport_sequence_gaps_mod256":sequence_gaps,"duplicate_ids":duplicate_ids,"clock_resets":epoch,"failed_transactions":failed_transactions,"max_device_dropped_total":max_device_dropped});
    fs::write(out.join("run.json"), serde_json::to_vec_pretty(&receipt)?)?;
    println!("{}", receipt["quality"]);
    if records == 0 || read_error.is_some() {
        return Err(
            "capture has no valid records or ended with an input error; evidence preserved".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hx_observations_preserve_units_direction_and_reject_failed_or_short_reads() {
        let mut t = BusTransaction {
            version: 1,
            transport_sequence: 0,
            transaction_id: 0,
            device_dropped_total: 0,
            window: sim_runtime::acquisition::ClockWindow {
                clock_hz: 50_000_000,
                request_tick: 0,
                completion_tick: 100,
            },
            device_id: 2,
            instruction: 2,
            outcome: 0,
            device_error: 0,
            control_flags: 0,
            request: vec![0x3a, 2],
            reply: vec![0, 0x81],
        };
        let (channel, v, raw) = observation(&t).unwrap();
        assert_eq!(channel.kind, QuantityKind::AngularVelocity);
        assert_eq!(raw, 0x8100);
        assert!((v + std::f64::consts::TAU / 16.).abs() < 1e-12);
        t.request = vec![0x3e, 2];
        t.reply = 11119u16.to_le_bytes().to_vec();
        assert!((observation(&t).unwrap().1 - 11.1).abs() < 1e-12);
        t.request = vec![0x3e, 1];
        t.reply = vec![104];
        assert!((observation(&t).unwrap().1 - 10.4).abs() < 1e-12);
        t.request = vec![0x3f, 1];
        t.reply = vec![25];
        assert_eq!(observation(&t).unwrap().1, 298.15);
        t.outcome = 2;
        assert!(observation(&t).is_none());
        t.outcome = 0;
        t.device_error = 1;
        assert!(observation(&t).is_none());
        t.device_error = 0;
        t.reply.clear();
        assert!(observation(&t).is_none());
        t.instruction = 3;
        t.request = vec![0x2e, 0, 0x81];
        t.outcome = 2;
        assert!(
            command(&t).unwrap().1 < 0.,
            "timed-out reverse command remains an attempted command"
        );
        t.request = vec![0x2a, 0, 0x10];
        assert_eq!(command(&t).unwrap().1, std::f64::consts::TAU);
    }
}
