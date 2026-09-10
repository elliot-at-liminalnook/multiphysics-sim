//! Present the shared CAD joint problem to the verified native NLP interface.
use serde::{Deserialize, Serialize};
#[path = "support/joint_checkpoint.rs"]
mod checkpoint;
use sim_runtime::{
    contact_planning::{
        ContactPlanRecipe, ContactPlanner, JointContactMotion, JointContactVariable,
        JointIpoptConfig,
    },
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use sim_solve::ipopt::IpoptLibrary;
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    robot: ContactPlanRecipe,
    candidate: JointContactMotion,
    variables: Vec<JointContactVariable>,
    #[serde(rename = "search")]
    _legacy_search: serde_json::Value,
}
fn run() -> Result<(), String> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 5 && a.len() != 6 {
        return Err("usage: optimize_joint_ipopt scene.json markers.json joint.recipe.json verified-libipopt.dylib ipopt-search.json [new-checkpoint-directory]".into());
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&a[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&a[1])?).map_err(|e| e.to_string())?;
    let recipe: Recipe = serde_json::from_slice(&read(&a[2])?).map_err(|e| e.to_string())?;
    let config: JointIpoptConfig =
        serde_json::from_slice(&read(&a[4])?).map_err(|e| e.to_string())?;
    // Caller supplies the recorded trusted Ipopt 3.14.19 f64/i32/C-bool ABI.
    let library = unsafe { IpoptLibrary::load_f64_i32_c_bool(std::path::Path::new(&a[3])) }?;
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, recipe.robot.clone())?;
    let mut checkpoints = a
        .get(5)
        .map(|path| {
            checkpoint::Checkpoints::new(std::path::Path::new(path), &recipe, &config, &a[..5])
        })
        .transpose()?;
    let mut checkpoint_error = None;
    let mut count = 0;
    let optimization=planner.optimize_joint_ipopt_with_observer(&recipe.candidate,&recipe.variables,&config,&library,|candidate,r|{
        count+=1;if count==1||count%10==0{eprintln!("valid report {count}: speed {:.6}, force {:.6} N, moment {:.6} Nm, torque {:.6} Nm, maximum inequality {:.6}, feasible {}",r.motion_report.speed_m_s,r.motion_report.maximum_force_error_n,r.motion_report.maximum_moment_error_nm,r.motion_report.minimum_torque_margin_nm,r.constraints.inequalities.iter().copied().fold(0.0_f64,f64::max),r.sampled_feasible);}
        if let Some(writer) = &mut checkpoints {
            if let Err(error) = writer.observe(candidate,r,count) {
                eprintln!("checkpoint writing disabled; numerical solve continues unchanged: {error}");
                checkpoint_error=Some(error);checkpoints=None;
            }
        }
    });
    if let Some(writer) = &mut checkpoints {
        if let Err(error) = writer.flush() {
            checkpoint_error = Some(error);
        }
    }
    let result = optimization?;
    println!(
        "{}",
        serde_json::to_string(&result).map_err(|e| e.to_string())?
    );
    if let Some(error) = checkpoint_error {
        return Err(format!(
            "optimization result written, but checkpoint output failed: {error}"
        ));
    }
    Ok(())
}
pub(crate) fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
