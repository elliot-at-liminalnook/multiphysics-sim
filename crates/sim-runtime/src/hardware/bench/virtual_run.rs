//! Explicit host-loop virtual acquisition over the existing Bench model.
//! Configured bench IDs map in order to Bench axes 1..3; no FPGA firmware is
//! emulated and no device-clock receipt or physical release proof is fabricated.
use super::{Signals, VirtualConfig, value, write_json};
use crate::{
    acquisition::{servo_bus::Telemetry, virtual_bench::Bench},
    controller_refinement::{
        fpga::Plan,
        live_reference::{Cursor, Received, Request},
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::Path,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(super) fn run(
    config: &VirtualConfig,
    ids: &[u8],
    dir: &Path,
    signals: Arc<Signals>,
) -> Result<(), String> {
    let out = dir.join("capture");
    fs::create_dir(&out).map_err(|e| e.to_string())?;
    // Fail before constructing or advancing a bench if exact implementation sources
    // cannot be retained durably. The identity describes host simulation, not FPGA proof.
    let source_identity = super::provenance::retain(&out).map_err(|e| e.to_string())?;
    let command = value(dir.join("plan.json"));
    let mut manifest = json!({"completed":false,"mode":"virtual_host_bench","kind":"virtual","bench_instance":config.bench_instance,"fidelity":"Existing Bench dynamics with host feedback loop; FPGA device timing and protocol not emulated or qualified","ids":ids,"axis_mapping":"Configured IDs in order map to virtual Bench axes 1..3","physical_stop_verified":false});
    manifest["source_blake3"] = source_identity["composite_blake3"].clone();
    manifest["source_identity"] = source_identity;
    crate::publication::publish(
        &out.join("run.json"),
        &serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
        crate::publication::Policy::ImmutableNew,
    ).into_result()?;
    write_json(out.join("virtual-configuration.json"), config)?;
    let mut bench = Bench::new(config.start, config.models.clone());
    let mut log = fs::File::create(out.join("live-telemetry.jsonl")).map_err(|e| e.to_string())?;
    if command["control"] == "inspect" {
        let devices:Vec<_>=bench.servos.iter().zip(ids).map(|(s,id)|json!({"id":id,"position_counts":s.position,"torque_enable":s.torque,"pwm":s.pwm})).collect();
        manifest["completed"] = json!(true);
        manifest["result"] = json!({"devices":devices,"simulated":true,"stop_verified":false});
        write_json(out.join("run.json"), &manifest)?;
        return Ok(());
    }
    let live: Option<Request> = if command["control"] == "fpga_live_device_reference" {
        Some(serde_json::from_value(command["request"].clone()).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let mut plan: Plan = serde_json::from_value(if live.is_some() {
        command["plan"].clone()
    } else {
        command
    })
    .map_err(|e| e.to_string())?;
    plan.validate()?;
    if let Some(request) = &live {
        request.validate(ids)?;
    }
    let mut cursor = live.as_ref().map(|r| Cursor::new(&r.initial));
    let mut fresh = Instant::now();
    let mut previous = [0i16; 9];
    let mut position = [0u16; 9];
    let origin = Instant::now();
    let mut clock = Instant::now();
    let mut frames = 0usize;
    let mut inputs = fs::File::create(out.join("live-inputs.jsonl")).map_err(|e| e.to_string())?;
    let operation = (|| -> Result<(), String> {
        if signals.cancelled() {
            return Err("Operator STOP before arming".into());
        }
        for (k, id) in ids.iter().enumerate() {
            let axis = k as u8 + 1;
            let (lo, hi) = config.axes[k].encoder_bounds();
            let (lo, hi) = (lo.unwrap() + 40, hi.unwrap() - 40);
            bench.handle(axis, 2, &[0x38, 15]);
            let mut window = vec![7, axis];
            window.extend((config.start[k].round() as i32).to_le_bytes());
            window.extend(lo.to_le_bytes());
            window.extend(hi.to_le_bytes());
            bench.handle(254, 0xa0, &window);
            bench.handle(axis, 3, &[0x37, 0]);
            bench.handle(axis, 3, &[0x21, 2]);
            bench.handle(axis, 3, &[0x37, 1]);
            bench.handle(254, 0xa0, &[1, axis]);
            if bench.latched || bench.armed & (1 << k) == 0 {
                return Err("Virtual taught-window arm refused".into());
            }
            if signals.cancelled() {
                return Err("Operator STOP while arming".into());
            }
            bench.handle(axis, 3, &[0x28, 1]);
            position[usize::from(*id - 4)] = config.start[k].round().rem_euclid(4096.) as u16;
        }
        for tick in 0..plan.targets.len() {
            let scheduled = tick as f64 * plan.period_s;
            while origin.elapsed().as_secs_f64() < scheduled {
                if signals.cancelled() || signals.expired() {
                    return Err("Operator STOP or owner heartbeat expired".into());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            if signals.cancelled() || signals.expired() {
                return Err("Operator STOP or owner heartbeat expired".into());
            }
            if origin.elapsed().as_secs_f64() > scheduled + plan.period_s {
                return Err("Missed virtual host schedule; no catch-up bursts".into());
            }
            bench.advance(clock.elapsed().as_secs_f64());
            clock = Instant::now();
            if bench.latched {
                return Err("Virtual supervisor watchdog stopped the session".into());
            }
            if let Some(request) = &live {
                let received: Received = serde_json::from_slice(
                    &fs::read(dir.join("latest-reference.json")).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                if received.session != dir.file_name().unwrap().to_string_lossy() {
                    return Err("Foreign live session".into());
                }
                let age = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_secs_f64()
                    - received.received_unix_s;
                if !age.is_finite() || !(0.0..=0.25).contains(&age) {
                    return Err("Live reference stale (>250 ms)".into());
                }
                if cursor.as_mut().unwrap().advance(&received.sample)? {
                    fresh = Instant::now();
                }
                if fresh.elapsed() > Duration::from_millis(250) {
                    return Err("Live simulation stalled (>250 ms)".into());
                }
                if tick > 0 {
                    let row = request.encode(&received.sample, &previous)?;
                    plan.targets[tick..].fill(row);
                }
                writeln!(
                    inputs,
                    "{}",
                    json!({"tick":tick,"received":received,"input_age_s":age})
                )
                .map_err(|e| e.to_string())?;
                inputs.flush().map_err(|e| e.to_string())?;
            }
            for (k, id) in ids.iter().enumerate() {
                let i = usize::from(*id - 4);
                let axis = k as u8 + 1;
                let response = bench.handle(axis, 2, &[0x38, 15]);
                let t =
                    Telemetry::decode(&response[5..response.len() - 1]).map_err(str::to_string)?;
                let s = &bench.servos[k];
                let (lo, hi) = config.axes[k].encoder_bounds();
                if s.position < lo.unwrap() as f64 + 40.
                    || s.position > hi.unwrap() as f64 - 40.
                    || t.temperature_c >= 58
                    || !(90..=126).contains(&t.voltage_raw)
                {
                    return Err(
                        "Virtual feedback exceeded taught travel or environment limits".into(),
                    );
                }
                let home = config.start[k].round() as i32;
                let target = home + i32::from(plan.targets[tick][i]);
                if target < lo.unwrap() + 40 || target > hi.unwrap() - 40 {
                    return Err("Virtual target exceeds taught travel window".into());
                }
                let pwm = sim_domain_control::fixed_pd::step(
                    plan.gains,
                    target.rem_euclid(4096) as u16,
                    t.position_raw,
                    position[i],
                    plan.targets[tick][i] - previous[i],
                )?;
                position[i] = t.position_raw;
                bench.handle(254, 0xa0, &[3, axis]);
                if signals.cancelled() {
                    return Err("Operator STOP before virtual drive".into());
                }
                let raw = pwm.unsigned_abs() | if pwm < 0 { 1024 } else { 0 };
                let [a, b] = raw.to_le_bytes();
                bench.handle(axis, 3, &[0x2c, a, b]);
                writeln!(log,"{}",json!({"frame":tick,"id":id,"host_s":origin.elapsed().as_secs_f64(),"telemetry":t,"home_raw":home,"target_counts":plan.targets[tick][i],"previous_target_counts":previous[i],"pwm":pwm,"pwm_limit":plan.gains.limit,"kind":"virtual","fidelity":"virtual_host_bench"})).map_err(|e|e.to_string())?;
            }
            log.flush().map_err(|e| e.to_string())?;
            previous = plan.targets[tick];
            frames += 1;
        }
        Ok(())
    })();
    bench.handle(254, 0xa0, &[0]);
    let stop = Instant::now();
    let mut stationary: Option<Instant> = None;
    while stop.elapsed() < Duration::from_secs(2) {
        bench.advance(clock.elapsed().as_secs_f64());
        clock = Instant::now();
        if bench
            .servos
            .iter()
            .all(|s| !s.torque && s.pwm == 0 && s.speed.abs() < 0.5)
        {
            stationary.get_or_insert(Instant::now());
        } else {
            stationary = None;
        }
        if stationary.is_some_and(|at| at.elapsed() >= Duration::from_millis(150)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let verified = stationary.is_some_and(|at| at.elapsed() >= Duration::from_millis(150));
    manifest["completed"] = json!(operation.is_ok() && verified && frames == plan.targets.len());
    manifest["result"] = json!({"stop_verified":verified,"physical_stop_verified":false,"simulated":true,"failure":operation.err().or_else(||(!verified).then(||"Virtual stationary tail not verified".into())),"frames":frames});
    write_json(out.join("executed-plan.json"), &plan)?;
    write_json(out.join("run.json"), &manifest)
}
