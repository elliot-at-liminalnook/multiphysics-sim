//! Evaluate reference event boundaries; no robot dynamics or controller policy.
use serde::Serialize;
use sim_domain_control::contact_phase::{ContactPhaseConfig, ContactPhaseMotion};
#[derive(Serialize)]
struct Failure {
    time_s: f64,
    error: String,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 1 {
        return Err("usage: audit_contact_boundaries motion.json".into());
    }
    let config: ContactPhaseConfig = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let motion = ContactPhaseMotion::new(config.clone())?;
    let mut times = Vec::new();
    for cycle in -2..=2 {
        for foot in &config.feet {
            for step in foot.steps() {
                for event in [step.phase_offset, step.phase_offset + step.stance_fraction] {
                    let t = (cycle as f64 + event) * config.period_s;
                    times.extend([
                        t.next_down().next_down(),
                        t.next_down(),
                        t,
                        t.next_up(),
                        t.next_up().next_up(),
                    ]);
                }
            }
        }
    }
    for i in 0..=4096 {
        times.push((i as f64 / 1024.0 - 2.0) * config.period_s);
    }
    times.sort_by(f64::total_cmp);
    times.dedup_by(|a, b| a.to_bits() == b.to_bits());
    let mut failures = Vec::new();
    let mut samples = Vec::new();
    for time_s in times {
        match motion.sample(time_s) {
            Ok(sample) => samples.push(serde_json::json!({"time_s":time_s,"sample":sample})),
            Err(error) => failures.push(Failure { time_s, error }),
        }
    }
    println!(
        "{}",
        serde_json::json!({"source":args[0],"samples":samples,"failures":failures,
        "scope":"Reference samples around every stance boundary and a uniform grid over four cycles. Valid configuration and finite output are necessary, not a dynamics or walking certificate."})
    );
    Ok(())
}
