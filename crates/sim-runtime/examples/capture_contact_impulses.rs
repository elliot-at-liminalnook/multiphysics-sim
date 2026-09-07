//! Replay inputs with bounded per-report-window committed contact impulse audits.
//! Usage: capture_contact_impulses RECORDING [ATTEMPT_LIMIT_PER_WINDOW]
use sim_runtime::{contact_audit::committed_contact_impulses,session::{Recording,Session}};
use serde_json::json;
use std::collections::BTreeMap;

fn run() -> Result<(),String> {
    let args:Vec<_>=std::env::args().skip(1).collect();
    if args.is_empty() || args.len()>2 {return Err("usage: capture_contact_impulses RECORDING [ATTEMPT_LIMIT_PER_WINDOW]".into());}
    let recording:Recording=serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    if recording.version!=1 {return Err("unsupported recording version".into());}
    let limit=args.get(1).map(|s|s.parse::<usize>().map_err(|e|e.to_string())).transpose()?.unwrap_or(512);
    let mut session=Session::new(recording.scene.clone(),recording.seed)?;
    let mut windows=Vec::new();
    let mut totals:BTreeMap<(usize,Option<usize>),[f64;3]>=BTreeMap::new();
    for action in &recording.actions {
        session.set_attempt_audit_limit(limit)?;
        let start=session.frame().time_s;
        session.step(action)?;
        let frame=session.frame();
        let report=committed_contact_impulses(&session,start,frame.time_s)?;
        for contact in &report.contacts {
            let total=totals.entry((contact.link,contact.other)).or_default();
            for (sum,value) in total.iter_mut().zip(contact.impulse_ns) {*sum+=value;}
        }
        windows.push(json!({"frame":frame,"impulses":report,
            "solver_stats":session.robot.runtime.islands.iter().map(|i|i.stats).collect::<Vec<_>>()}));
    }
    let contacts:Vec<_>=totals.into_iter().map(|((link,other),impulse)|json!({
        "link":link,"name":session.robot.art.links[link].name,"other":other,"impulse_ns":impulse})).collect();
    println!("{}",serde_json::to_string(&json!({"version":1,"completed":true,"source":recording.scene.robot.source,
        "options":recording.scene.options,"seed":recording.seed,"period_s":recording.scene.period_s,
        "action_count":recording.actions.len(),"attempt_limit_per_window":limit,"windows":windows,
        "total_contact_impulses":contacts,"notes":[
            "Each reporting window requires a complete partition of committed implicit substeps; incomplete or unknown coverage is an error.",
            "Audit storage is cleared only between completed reporting windows. Snapshot retries revoke superseded local commits.",
            "Impulse uses the solver stage quadrature. Force moments, torsional impulses and energy are not included.",
            "Diagnostic contact reevaluations make this unsuitable as a timing benchmark."]})).map_err(|e|e.to_string())?);
    Ok(())
}
fn main() {if let Err(e)=run() {eprintln!("{e}");std::process::exit(2);}}
