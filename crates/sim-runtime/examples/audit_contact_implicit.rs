use sim_runtime::{
    contact_implicit::{ContactImplicitConfig, ContactImplicitPlanner, ContactSlipObjective},
    session::{Scene, Session},
    tracking::CaptureConfig,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() < 5
        || a[5..].iter().any(|flag| flag != "--pairs" && flag != "--inequalities")
        || a[5..].iter().collect::<std::collections::BTreeSet<_>>().len() != a.len() - 5
    {
        return Err(
            "usage: audit_contact_implicit scene markers recipe result geometry_subdivisions [--pairs] [--inequalities]"
                .into(),
        );
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&a[0])?)?;
    let markers: CaptureConfig = serde_json::from_slice(&std::fs::read(&a[1])?)?;
    let recipe: serde_json::Value = serde_json::from_slice(&std::fs::read(&a[2])?)?;
    let result: serde_json::Value = serde_json::from_slice(&std::fs::read(&a[3])?)?;
    let config: ContactImplicitConfig = serde_json::from_value(recipe["config"].clone())?;
    let positions: Vec<Vec<f64>> = serde_json::from_value(result["positions"].clone())?;
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let smooth = config.periodic_cubic_subdivisions.is_some();
    let planner = ContactImplicitPlanner::new(&session.robot.art, &seed, &markers, config)?;
    let report = planner.evaluate(&positions)?;
    let slip_objective: Option<ContactSlipObjective> =
        serde_json::from_value(recipe["slip_objective"].clone())?;
    let slip = slip_objective
        .as_ref()
        .map(|o| planner.slip_report(&report, o))
        .transpose()?;
    let inspection = if a[5..].iter().any(|flag| flag == "--inequalities") {
        let bounds: Vec<Vec<sim_solve::least_squares::VariableBound>> =
            serde_json::from_value(recipe["bounds"].clone())?;
        let parameters = planner.parameterization(&positions, &bounds)?;
        if parameters.decode(&parameters.values)? != positions {
            return Err("inspection parameters must exactly round-trip the supplied motion".into());
        }
        let residuals = planner.inequality_residuals(
            &report, slip_objective.as_ref().ok_or("inequality inspection requires a slip objective")?,
        )?;
        Some(serde_json::json!({"values":parameters.values,"bounds":parameters.bounds,
            "residuals":residuals,
            "scope":"Shared native optimizer coordinates and signed residual rows, evaluated at the supplied motion. No optimizer step or checkpoint migration is performed."}))
    } else { None };
    let include_pairs = a[5..].iter().any(|flag| flag == "--pairs");
    let geometry = if include_pairs {
        planner.audit_geometry_detailed(&positions, a[4].parse()?)?
    } else {
        planner.audit_geometry(&positions, a[4].parse()?)?
    };
    let mut output = serde_json::json!({"planning":report,"geometry":geometry,"slip":slip,
        "scope":if smooth {"Independent analytic-motion collocation audit plus full sampled CAD geometry on the same periodic cubic curve. Additional sample grids, runtime tracking, continuous collision and stable orbit validation remain separate."} else {"Independent final-model inverse-dynamics re-evaluation plus full sampled CAD geometry at linear position-space subdivisions. Geometry is not interpolated force feasibility or a closed-loop walking test."}});
    if include_pairs {
        output["link_names"] = serde_json::to_value(
            session
                .robot
                .art
                .links
                .iter()
                .map(|l| &l.name)
                .collect::<Vec<_>>(),
        )?;
    }
    if let Some(inspection) = inspection {
        output["inequality_inspection"] = inspection;
    }
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
