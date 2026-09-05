//! Where the trotting quadruped spends its time, headless.
//!
//!     cargo run --release -p sim-phenomena --example quadruped_profile
//!
//! Three passes over the same plant with the in-process C controller:
//! the plate's fixed 1 ms grid, the viewer's pattern (one step per frame
//! of `frame × time_scale`, jittering with the frame clock), and the
//! viewer's pattern with the Levitron's spin sweep running beside it as
//! it does for the first minutes after launch.

use sim_phenomena::scenarios::{levitron, quadruped_gait};
use sim_phenomena::world::registry;
use sim_solve::profile;
use std::time::Instant;

fn plant(q: &quadruped_gait::Quadruped) -> quadruped_gait::Plant {
    let registry = registry();
    let mut plant = q.model(&registry);
    let controller = q.controller_in(q.stride, quadruped_gait::Lang::Dylib).expect("dylib controller");
    plant.runtime.attach(plant.seam, controller).expect("seam");
    plant
}

fn pass(name: &str, q: &quadruped_gait::Quadruped, sim_seconds: f64, mut step_of: impl FnMut(u64) -> f64) {
    let mut p = plant(q);
    // Warm up through the stand-still phase so every pass measures the trot.
    p.runtime.advance(q.start, 1.0e-3).expect("warm-up");
    profile::reset();
    let started = Instant::now();
    let mut elapsed = 0.0;
    let mut frames = 0u64;
    while elapsed < sim_seconds {
        let dt = step_of(frames).min(sim_seconds - elapsed);
        p.runtime.advance(dt, 1.0e-3).expect("quadruped runs");
        elapsed += dt;
        frames += 1;
    }
    let wall = started.elapsed().as_secs_f64();
    let steps = profile::STEP.calls();
    println!("== {name}: {sim_seconds:.2} s of trot in {wall:.2} s wall ({:.1}× slower than real time), {steps} steps, {:.2} ms/step", wall / sim_seconds, 1.0e3 * wall / steps as f64);
    print!("{}", profile::report());
    println!();
}

fn main() {
    profile::enable();
    let q = quadruped_gait::Quadruped::default();
    {
        let p = plant(&q);
        let island = &p.runtime.islands[0];
        println!("quadruped: {} islands, {} unknowns stored, {} solved, controller period {} s, step 1 ms", p.runtime.islands.len(), island.system.state_ids.len(), island.system.reduced_dimension(), q.period);
    }
    let sim_seconds = 1.2;
    pass("plate: fixed 1 ms grid", &q, sim_seconds, |_| 1.0e-3);
    if std::env::args().nth(1).as_deref() == Some("plate") {
        return;
    }
    // The viewer: one call per frame of frame_dt × 0.03 with a jittering frame clock.
    let mut seed = 0x9E3779B97F4A7C15u64;
    let mut jitter = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % 1000) as f64 / 1000.0
    };
    let frame = 1.0 / 60.0;
    pass("viewer: frame × 0.03, jittering", &q, sim_seconds, |_| (frame + 0.002 * (jitter() - 0.5)) * 0.03);
    pass("viewer without jitter: exactly 0.5 ms per frame", &q, sim_seconds, |_| 0.5e-3);

    // The Levitron's spin sweep: how long one growth-rate evaluation takes,
    // and what it does to the quadruped when it runs alongside.
    let registry = registry();
    let started = Instant::now();
    let rate = levitron::Levitron::default().growth_rate(&registry);
    let one = started.elapsed().as_secs_f64();
    println!("levitron growth_rate: one evaluation {one:.2} s (rate {rate:.3}); the sweep makes 40 + 2×24 = 88 of them ≈ {:.0} s", 88.0 * one);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handle = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let registry = sim_phenomena::world::registry();
            let mut n = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                levitron::Levitron::default().growth_rate(&registry);
                n += 1;
            }
            n
        })
    };
    std::thread::sleep(std::time::Duration::from_millis(300));
    pass("viewer pattern with the Levitron sweep running beside it", &q, sim_seconds, |_| 0.5e-3);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = handle.join();
}
