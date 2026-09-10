//! Independent central-difference checks of CAD-derived force columns.
use serde::Deserialize;
use sim_runtime::{
    contact_planning::{
        ContactPlanRecipe, ContactPlanner, JointContactDecision, JointContactMotion,
        JointContactVariable,
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
    if args.len() != 3 && args.len() != 4 {
        return Err("usage: audit_joint_force_jacobian scene.json markers.json recipe.json [--case=shifted_reference|constant_body|alternate_interpolation]".into());
    }
    let selected = args
        .get(3)
        .map(|a| a.strip_prefix("--case=").ok_or("expected --case=name"))
        .transpose()?;
    if selected.is_some_and(|s| {
        ![
            "shifted_reference",
            "constant_body",
            "alternate_interpolation",
        ]
        .contains(&s)
    }) {
        return Err("unknown force derivative audit case".into());
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
    let original = planner.evaluate_joint_uncached(&recipe.candidate)?;
    let mut rows = Vec::new();
    for case in [
        "shifted_reference",
        "constant_body",
        "alternate_interpolation",
    ]
    .into_iter()
    .filter(|case| selected.is_none_or(|s| s == *case))
    {
        let mut candidate = recipe.candidate.clone();
        // Keep numerical probes away from cone apexes; these are derivative
        // audit loads, not controller proposals or physical model overrides.
        for templates in &mut candidate.force_templates {
            for (foot, template) in templates.iter_mut().enumerate() {
                if case == "alternate_interpolation" {
                    template.interpolation = if matches!(
                        template.interpolation,
                        sim_domain_control::trajectory::Interpolation::Linear
                    ) {
                        sim_domain_control::trajectory::Interpolation::QuinticRestToRest
                    } else {
                        sim_domain_control::trajectory::Interpolation::Linear
                    };
                }
                let last = template.keyframes.len() - 1;
                for (node, k) in template.keyframes.iter_mut().enumerate().take(last).skip(1) {
                    k.values[0] += 0.123 + foot as f64 * 0.011;
                    k.values[1] -= 0.071 + node as f64 * 0.013;
                    k.values[2] += 0.031;
                }
            }
        }
        if case == "constant_body" {
            let n = candidate.motion.body.keyframes.len() - 1;
            let mean = (0..6)
                .map(|i| {
                    candidate.motion.body.keyframes[..n]
                        .iter()
                        .map(|k| k.values[i])
                        .sum::<f64>()
                        / n as f64
                })
                .collect::<Vec<_>>();
            for k in &mut candidate.motion.body.keyframes {
                k.values = mean.clone();
            }
        }
        let began = std::time::Instant::now();
        let (report, columns) = planner.linearize_joint_forces(&candidate, &recipe.variables)?;
        let analytic_seconds = began.elapsed().as_secs_f64();
        let mut checked = 0;
        let mut fallback = 0;
        let mut max_error = 0.0_f64;
        let mut max_relative = 0.0_f64;
        let mut worst = (0, 0);
        let mut uncached_equal = 0;
        for (index, (variable, column)) in recipe.variables.iter().zip(columns).enumerate() {
            if matches!(variable.decision, JointContactDecision::Motion { .. }) {
                if column.is_some() {
                    return Err("motion column claimed analytic".into());
                }
                continue;
            };
            let Some(column) = column else {
                fallback += 1;
                continue;
            };
            let h = 1e-5;
            let mut probes = Vec::new();
            for sign in [-1.0, 1.0] {
                let mut probe = candidate.clone();
                let value = candidate.force_node_value(&variable.decision)?;
                probe.set_force_node(&variable.decision, value + sign * h)?;
                let r = planner.evaluate_joint(&probe)?;
                if checked == 0 {
                    let full = planner.evaluate_joint_uncached(&probe)?;
                    if serde_json::to_vec(&r).map_err(|e| e.to_string())?
                        != serde_json::to_vec(&full).map_err(|e| e.to_string())?
                    {
                        return Err("force Jacobian probe cache mismatch".into());
                    }
                    uncached_equal += 1;
                }
                probes.push(
                    r.constraints
                        .objective
                        .into_iter()
                        .chain(r.constraints.inequalities)
                        .collect::<Vec<_>>(),
                );
            }
            if column.len() != probes[0].len() {
                return Err("derivative dimension mismatch".into());
            }
            for (row, &analytic) in column.iter().enumerate() {
                let numerical = (probes[1][row] - probes[0][row]) / (2. * h);
                let error = (numerical - analytic).abs();
                let relative = error / (1. + analytic.abs());
                max_error = max_error.max(error);
                if relative > max_relative {
                    max_relative = relative;
                    worst = (index, row);
                }
            }
            checked += 1;
        }
        if checked == 0 || max_relative > 1e-5 {
            return Err(format!(
                "force derivative audit failed: {case}, columns {checked}, relative error {max_relative}, worst {worst:?}"
            ));
        }
        rows.push(serde_json::json!({"case":case,"checked_force_columns":checked,"fallback_columns":fallback,"max_absolute_error":max_error,"max_scaled_error":max_relative,"worst_variable_row":worst,"analytic_seconds_including_geometry":analytic_seconds,"independent_uncached_probe_matches":uncached_equal,"physical_rows":report.constraints.inequalities.len()}));
    }
    println!(
        "{}",
        serde_json::json!({"reference_report":original,"cases":rows,"difference_step_n":1e-5,"scaled_error_tolerance":1e-5,"scope":"Force derivative audit only, using perturbed audit loads away from cone apexes, both operating clocks and all movable force nodes. Motion derivatives remain numerical. No gait feasibility or physical speed claim."})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
