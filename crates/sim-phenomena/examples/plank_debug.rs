//! Trajectory printer for tuning the walk-the-plank biped.
//!
//!     cargo run --release -p sim-phenomena --example plank_debug -- [stand|walk] [kp kd limit hip_height torso_gain torso_rate]
use sim_couple::Environment;
use sim_phenomena::scenarios::walk_the_plank::{Biped, Course, PlankEnv};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("walk");
    let num = |k: usize, d: f64| args.get(k).and_then(|s| s.parse().ok()).unwrap_or(d);
    let mut biped = Biped::default();
    biped.kp = num(2, biped.kp);
    biped.kd = num(3, biped.kd);
    biped.torque_limit = num(4, biped.torque_limit);
    biped.hip_height = num(5, biped.hip_height);
    biped.torso_gain = (num(6, biped.torso_gain.0), num(7, biped.torso_gain.1));
    let level = num(8, 0.0);
    let seed = num(9, 3.0) as u64;
    biped.margin = num(10, biped.margin);
    biped.lead = num(11, biped.lead);
    biped.stride = num(12, biped.stride);
    let mut env = PlankEnv::new(biped, Course::Flat);
    let f = env.reset(seed, level).unwrap();
    println!("terrain: {:?}", f.terrain.as_ref().unwrap().iter().map(|p| (format!("{:.2}", p.0), format!("{:.2}", p.1), format!("{:.2}", p.2))).collect::<Vec<_>>());
    let (hl, kl) = biped.standing(-0.08);
    let (hr, kr) = biped.standing(0.08);
    println!("  t     x      y     pitch   lh    lk    rh    rk   | lfx   lfy   rfx   rfy  | lfy_N  rfy_N | stance phase  T    clf");
    for k in 0..600 {
        let offset = num(13, 0.0);
        let a = if mode == "stand" { env.hold_action() } else if mode == "hold" { [hl + offset, kl, hr + offset, kr] } else { env.planner_action() };
        let f = match env.step(&a) { Ok(f) => f, Err(e) => { println!("error: {e}"); break; } };
        let p = &f.privileged;
        if k % 5 == 0 || f.done {
            let planner = env.planner.unwrap();
            println!("{:5.2} {:6.3} {:6.3} {:6.3} {:5.2} {:5.2} {:5.2} {:5.2} | {:5.2} {:5.2} {:5.2} {:5.2} | {:6.1} {:6.1} | {:1} {:5.2} {:5.2} {:6.3}",
                f.t, p[0], p[1], f.obs[8], f.obs[0], f.obs[2], f.obs[4], f.obs[6], p[2], p[3], p[4], p[5], p[6], p[7], p[8] as u8, p[9], planner.duration, p[18]);
        }
        if f.done { println!("done: success={} failed={} steps={}", p[20], p[21], p[19]); break; }
    }
}
