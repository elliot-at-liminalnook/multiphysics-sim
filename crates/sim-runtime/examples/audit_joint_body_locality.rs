//! Validate compact body support and simultaneous probes in full CAD physics.
use serde::Deserialize;
use sim_domain_control::trajectory::Trajectory;
use sim_runtime::{
    contact_planning::{
        ContactDecision, ContactPlanRecipe, ContactPlanner, JointContactDecision,
        JointContactMotion, JointContactVariable,
    },
    session::{Scene, Session},
    tracking::CaptureConfig,
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    robot: ContactPlanRecipe,
    candidate: JointContactMotion,
    variables: Vec<JointContactVariable>,
    #[serde(rename = "search")]
    _search: serde_json::Value,
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: audit_joint_body_locality scene.json markers.json recipe.json".into());
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let recipe: Recipe = serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, recipe.robot)?;
    let candidate = recipe.candidate;
    let trajectory = Trajectory::new(candidate.motion.body.clone())?;
    let n = candidate.motion.body.keyframes.len() - 1;
    if n < 8 || n % 4 != 0 {
        return Err("audit requires a multiple of four body controls, at least eight".into());
    }
    let start = std::time::Instant::now();
    let base = planner.evaluate_joint_uncached(&candidate)?;
    let mut evaluations = 1;
    let mut frame_checks = 0;
    let mut groups = 0;
    let mut single_columns = 0;
    let mut cases = Vec::new();
    for channel in 0..6 {
        for residue in 0..4 {
            let variables = recipe
                .variables
                .iter()
                .filter_map(|v| match v.decision {
                    JointContactDecision::Motion {
                        decision:
                            ContactDecision::BodyControl {
                                control,
                                channel: c,
                            },
                    } if c == channel && control % 4 == residue => Some((control, v.bound.clone())),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if variables.len() != n / 4 {
                return Err("one variable per grouped body control required".into());
            }
            single_columns += variables.len();
            groups += 1;
            for sign in [-1., 1.] {
                let mut combined = candidate.clone();
                let mut singles = Vec::new();
                for (control, bound) in &variables {
                    let value = candidate.motion.body.keyframes[*control].values[channel];
                    let next = (value + sign * 1e-4 * (bound.upper - bound.lower))
                        .clamp(bound.lower, bound.upper);
                    let mut single = candidate.clone();
                    single.motion.body.keyframes[*control].values[channel] = next;
                    single.motion.body.keyframes[n].values =
                        single.motion.body.keyframes[0].values.clone();
                    combined.motion.body.keyframes[*control].values[channel] = next;
                    let report = planner.evaluate_joint_uncached(&single)?;
                    evaluations += 1;
                    if report.motion_report.frames.len() != base.motion_report.frames.len() {
                        return Err("body probe changed frame layout".into());
                    }
                    singles.push((*control, report));
                }
                combined.motion.body.keyframes[n].values =
                    combined.motion.body.keyframes[0].values.clone();
                let report = planner.evaluate_joint_uncached(&combined)?;
                evaluations += 1;
                if report.motion_report.frames.len() != base.motion_report.frames.len() {
                    return Err("combined body probe changed frame layout".into());
                }
                for (frame_index, frame) in report.motion_report.frames.iter().enumerate() {
                    let support = trajectory.periodic_support_controls(
                        frame.time_s.rem_euclid(candidate.motion.period_s),
                    )?;
                    let influencing = singles
                        .iter()
                        .filter(|(control, _)| support.contains(control))
                        .collect::<Vec<_>>();
                    if influencing.len() > 1 {
                        return Err("overlapping body-control color".into());
                    }
                    let expected = if let Some((_, r)) = influencing.first() {
                        &r.motion_report.frames[frame_index]
                    } else {
                        &base.motion_report.frames[frame_index]
                    };
                    let bytes = |v: &sim_runtime::contact_planning::ContactPlanFrame| {
                        serde_json::to_vec(v).map_err(|e| e.to_string())
                    };
                    if bytes(frame)? != bytes(expected)? {
                        return Err(format!(
                            "combined probe differs from independent column at channel {channel}, group {residue}, sign {sign}, frame {frame_index}"
                        ));
                    }
                    for (control, single) in &singles {
                        if !support.contains(control)
                            && bytes(&single.motion_report.frames[frame_index])?
                                != bytes(&base.motion_report.frames[frame_index])?
                        {
                            return Err(format!(
                                "body influence outside declared support: control {control}, channel {channel}, frame {frame_index}"
                            ));
                        }
                    }
                    frame_checks += 1;
                }
                cases.push(serde_json::json!({"channel":channel,"residue":residue,"sign":sign,"controls":variables.iter().map(|(c,_)|*c).collect::<Vec<_>>() }));
            }
        }
        eprintln!(
            "audited body channel {channel}: {evaluations} uncached CAD evaluations, {frame_checks} frame comparisons"
        );
    }
    println!(
        "{}",
        serde_json::json!({"body_controls":n,"body_variables":single_columns,"groups":groups,"ordinary_two_sided_probes":2*single_columns,"grouped_two_sided_probes":2*groups,"audit_evaluations":evaluations,"frame_checks":frame_checks,"seconds":start.elapsed().as_secs_f64(),"initial_report":base,"cases":cases,"scope":"Equation-derived periodic cubic support, validated with independent and simultaneous body-control perturbations in full uncached CAD, both signs and every sampled operating clock. All frame fields must be byte-identical outside support and between grouped and independent probes. Timing is fixed within each group; no claim of global NLP sparsity, continuous-time completeness, solver speedup or improved runtime gait. Grouped differentiation is not yet integrated into optimization."})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
