//! The seam's environment server: a batch of environments over stdio for
//! a learner in any language (`simloop.Gym` in Python).
//!
//!     sim-gym --task walk-the-plank --envs 8 --course flat

use clap::Parser;
use sim_couple::Environment;
use sim_phenomena::scenarios::walk_the_plank::{Biped, Course, PlankEnv};

#[derive(Parser)]
#[command(about = "Serve simulation environments over stdio (newline-delimited JSON)")]
struct Args {
    /// The task: `walk-the-plank`.
    #[arg(long, default_value = "walk-the-plank")]
    task: String,
    /// Environments held in this process, stepped on parallel threads.
    #[arg(long, default_value_t = 1)]
    envs: usize,
    /// Course for walk-the-plank: flat, varying, stairs-up, stairs-down.
    #[arg(long, default_value = "flat")]
    course: String,
    /// Perception error for the planner's references (m; stones look nearer).
    #[arg(long, default_value_t = 0.0)]
    perception_offset: f64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let envs: Vec<Box<dyn Environment>> = match args.task.as_str() {
        "walk-the-plank" => {
            let course = Course::parse(&args.course).ok_or_else(|| format!("unknown course `{}`", args.course))?;
            (0..args.envs.max(1))
                .map(|_| {
                    let mut env = PlankEnv::new(Biped::default(), course);
                    env.perception_offset = args.perception_offset;
                    Box::new(env) as Box<dyn Environment>
                })
                .collect()
        }
        other => return Err(format!("unknown task `{other}`; expected walk-the-plank").into()),
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    sim_couple::serve(envs, stdin.lock(), &mut stdout)?;
    Ok(())
}
