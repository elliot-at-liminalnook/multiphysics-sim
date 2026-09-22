//! Finite replay of retained Rust-controller references through the normal live
//! acquisition path. Does not compute motor PWM or simulate a plant.
use sim_runtime::controller_refinement::{
    fpga::Plan,
    live_reference::{Received, Request, Sample},
    trajectory_binding::ReferenceTrace,
};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 8 {
        return Err("replay_live_motor_reference PORT PLAN TRACE REQUEST NEW_SESSION hold|replay|stale ACQUISITION_BINARY".into());
    }
    if !["hold", "replay", "stale"].contains(&a[6].as_str()) {
        return Err("Unknown replay mode".into());
    }
    let mut plan: Plan = serde_json::from_slice(&fs::read(&a[2])?)?;
    let trace: ReferenceTrace = serde_json::from_slice(&fs::read(&a[3])?)?;
    trace.validate()?;
    let mut request: Request = serde_json::from_slice(&fs::read(&a[4])?)?;
    request.initial = Sample {
        sequence: 0,
        time_s: 0.,
        targets_rad: trace
            .coordinates
            .iter()
            .cloned()
            .zip(trace.targets_rad[0].iter().copied())
            .collect(),
    };
    request.validate(&plan.ids)?;
    plan.control = "fpga_device_pd".into();
    plan.period_s = 0.01;
    plan.targets = vec![[0; 9]; 1200];
    plan.validate()?;
    let out = Path::new(&a[5]);
    fs::create_dir(out)?;
    fs::write(
        out.join("reference-trace.json"),
        serde_json::to_vec_pretty(&trace)?,
    )?;
    let input = out.join("command.json");
    fs::write(
        &input,
        serde_json::to_vec_pretty(
            &serde_json::json!({"control":"fpga_live_device_reference","plan":plan,"request":request}),
        )?,
    )?;
    let session = out
        .file_name()
        .ok_or("Missing session name")?
        .to_string_lossy()
        .to_string();
    let publish = |sample: Sample| -> Result<(), Box<dyn std::error::Error>> {
        let r = Received {
            session: session.clone(),
            received_unix_s: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64(),
            sample,
        };
        fs::write(out.join("reference.tmp"), serde_json::to_vec(&r)?)?;
        fs::rename(out.join("reference.tmp"), out.join("latest-reference.json"))?;
        Ok(())
    };
    publish(request.initial.clone())?;
    let ids = plan
        .ids
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut child = Command::new(&a[7])
        .env("HX_BAUD", "1000000")
        .args([&a[1], "12", &ids])
        .arg(out.join("capture"))
        .arg(&input)
        .spawn()?;
    let start = Instant::now();
    let mut sequence = 0;
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err("Acquisition stopped unsuccessfully; retained capture includes stop verification".into())
            };
        }
        let t = start.elapsed().as_secs_f64();
        if t > 15. {
            fs::write(out.join("STOP"), b"Replay producer deadline")?;
            child.wait()?;
            return Err("Producer deadline; STOP requested".into());
        }
        if a[6] != "stale" || t < 2. {
            sequence += 1;
            // One second of initial hold, then recorded reference rows at their
            // original simulation cadence, followed by a final hold.
            let source_t = if a[6] == "hold" { 0. } else { (t - 1.).max(0.) };
            let index = trace
                .times_s
                .partition_point(|v| *v <= source_t)
                .saturating_sub(1);
            publish(Sample {
                sequence,
                time_s: t,
                targets_rad: trace
                    .coordinates
                    .iter()
                    .cloned()
                    .zip(trace.targets_rad[index].iter().copied())
                    .collect(),
            })?;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}
