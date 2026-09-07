//! Re-solve a captured step using a second capture's initial state.
//! Usage: resolve_captured_step SCENE POINT COMMON_INITIAL_POINT [NEWTON_CONFIG [SUBSTEPS|doubling]]
//! Each point is {seed, island, solve: ImplicitAttempt}. Does not advance events.
use serde::Deserialize;
use sim_dynamics::{
    ImplicitAttempt,
    attempt_check::{check_implicit_step_doubling, refine_implicit_attempt},
};
use sim_runtime::{
    session::{Scene, Session},
    validation::state_labels,
};

#[derive(Deserialize)]
struct Point {
    #[serde(default)]
    seed: u64,
    island: usize,
    solve: ImplicitAttempt,
}
fn read<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn run() -> Result<bool, String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 3 || args.len() > 5 {
        return Err(
            "usage: resolve_captured_step SCENE POINT COMMON_INITIAL_POINT [NEWTON_CONFIG [SUBSTEPS|doubling]]".into(),
        );
    }
    let point: Point = read(&args[1])?;
    let common: Point = read(&args[2])?;
    if point.seed != common.seed
        || point.island != common.island
        || point.solve.start_time != common.solve.start_time
        || point.solve.step != common.solve.step
        || point.solve.theta != common.solve.theta
    {
        return Err("captures must share seed, island, step interval and method".into());
    }
    let config = match args.get(3) {
        Some(p) => read(p)?,
        None => sim_runtime::newton(),
    };
    let doubling = args.get(4).is_some_and(|s| s == "doubling");
    let substeps = if doubling {
        2
    } else {
        args.get(4)
            .map(|s| s.parse::<usize>())
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or(1)
    };
    let session = Session::new(read::<Scene>(&args[0])?, point.seed)?;
    let island = session
        .robot
        .runtime
        .islands
        .get(point.island)
        .ok_or("invalid island")?;
    sim_solve::profile::enable();
    sim_solve::profile::reset();
    let started = std::time::Instant::now();
    let step_doubling = if doubling {
        Some(check_implicit_step_doubling(
            &island.system,
            &point.solve,
            &common.solve.initial_state,
            config,
        )?)
    } else {
        None
    };
    let solves = if let Some(report) = &step_doubling {
        report.fine.clone()
    } else {
        refine_implicit_attempt(
            &island.system,
            &point.solve,
            &common.solve.initial_state,
            config,
            substeps,
        )?
    };
    let solve_wall_s = started.elapsed().as_secs_f64();
    let solver_work: Vec<_> = sim_solve::profile::all()
        .iter()
        .map(|b| serde_json::json!({"name":b.name,"calls":b.calls(),"seconds":b.seconds()}))
        .collect();
    let solve = solves.last().ok_or("no captured substeps")?;
    let completed = solves.len() == substeps
        && solves.iter().all(|s| s.solve_succeeded)
        && step_doubling
            .as_ref()
            .is_none_or(|r| r.coarse.solve_succeeded);
    let labels = state_labels(&session.robot.runtime);
    let coordinates: Vec<_> = island
        .system
        .full_of
        .iter()
        .map(|i| &labels[&island.system.state_ids[*i]])
        .collect();
    let physical_at = |solve: &ImplicitAttempt| -> Result<_, String> {
        Ok(
            match session
                .robot
                .generalized_at_solver_point(point.island, solve)?
            {
                Some(g) => {
                    let evaluation = session.robot.art.evaluate(&g);
                    Some(serde_json::json!({
                        "constraints":session.robot.art.audit_constraints(&g, &Default::default())?,
                        "contacts":evaluation.contacts.iter().map(|c| serde_json::json!({
                            "link":c.link,"other":c.other,"point_m":c.point.as_slice(),
                            "force_n":c.force.as_slice(),"penetration_m":c.penetration
                        })).collect::<Vec<_>>()
                    }))
                }
                None => None,
            },
        )
    };
    let physical = physical_at(solve)?;
    let coarse_physical = step_doubling
        .as_ref()
        .map(|r| physical_at(&r.coarse))
        .transpose()?;
    let attempts: Vec<_> = solves
        .iter()
        .map(|s| {
            Ok(serde_json::json!({
                "solve":s, "physical_at_stage":physical_at(s)?
            }))
        })
        .collect::<Result<_, String>>()?;
    println!("{}", serde_json::to_string(&serde_json::json!({
        "seed":point.seed, "island":point.island, "config":config,
        "coordinates":coordinates, "solve":solve, "physical_at_stage":physical,
        "substeps":substeps, "completed":completed, "attempts":attempts,
        "step_doubling":step_doubling, "coarse_physical_at_stage":coarse_physical,
        "solver_work":solver_work, "diagnostic_solve_wall_s":solve_wall_s,
        "notes":["Original terminal residual reproduced bit for bit in the supplied scene context.",
            "Fresh-matrix production steps with common initial state; one-step stage guess or refined rate predictors.",
            "External context held; no events, automatic subdivisions or hidden-history replay. Success does not establish timestep accuracy.",
            "Step doubling supplies smooth-regime differential endpoint estimates, not force error bounds or automatic acceptance.",
            "Work counts cover production solver probes; terminal/context audit evaluations add uncounted work. Wall time includes capture overhead and is not an end-to-end benchmark."]
    })).map_err(|e| e.to_string())?);
    Ok(completed)
}
fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
}
