//! Freeze smooth velocity/acceleration trajectories before acquisition.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, io::Write};
    let a = std::env::args().collect::<Vec<_>>();
    if a.len() != 5 {
        return Err(
            "plan_fpga_controller BITSTREAM zero|pilot|complex|nine|nine-fast ROLE NEW_PLAN".into(),
        );
    }
    let name = &a[2];
    if !["zero", "pilot", "complex", "nine", "nine-fast"].contains(&name.as_str()) {
        return Err("Unknown trajectory".into());
    }
    let all = name.starts_with("nine") || name == "zero";
    let period = if all { 0.15 } else { 0.1 };
    let count = if name == "zero" {
        12
    } else if all {
        54
    } else {
        81
    };
    let mut targets = Vec::new();
    for n in 0..count {
        let t = n as f64 * period;
        let mut values = [0i16; 9];
        for (i, v) in values.iter_mut().enumerate() {
            let phase = if all {
                i as f64 * std::f64::consts::TAU / 9.
            } else {
                0.
            };
            let envelope = if t < 1. {
                smooth(t)
            } else if t > 7. {
                smooth((8. - t).max(0.))
            } else {
                1.
            };
            let hz = if name == "nine-fast" { 0.6 } else { 0.3 };
            let wave = if name == "pilot" {
                (std::f64::consts::TAU * 0.2 * t).sin()
            } else {
                0.7 * (std::f64::consts::TAU * hz * t + phase).sin()
                    + 0.3 * (std::f64::consts::TAU * (hz * 2.3) * t - phase).sin()
            };
            *v = if name == "zero" {
                0
            } else {
                (envelope * if name == "pilot" { 20. } else { 40. } * wave).round() as i16
            };
        }
        targets.push(values);
    }
    let plan = sim_runtime::controller_refinement::fpga::Plan {
        control: "fpga_pd".into(),
        name: format!("FPGA {name}: smooth multi-frequency displacement"),
        role: a[3].clone(),
        ids: if all { (4..=12).collect() } else { vec![4] },
        period_s: period,
        gains: sim_domain_control::fixed_pd::Gains {
            kp_q8: 768,
            kd_q8: if all { 85 } else { 128 },
            kv_q8: if all { 1024 } else { 1536 },
            limit: if name == "zero" { 0 } else { 50 },
        },
        targets,
        rms_limit_counts: 3.,
        peak_limit_counts: 10.,
        bitstream_path: a[1].clone(),
        bitstream_blake3: blake3::hash(&fs::read(&a[1])?).to_hex().to_string(),
    };
    plan.validate()?;
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&a[4])?;
    f.write_all(&serde_json::to_vec_pretty(&plan)?)?;
    Ok(())
}
fn smooth(x: f64) -> f64 {
    x * x * x * (10. + x * (-15. + 6. * x))
}
