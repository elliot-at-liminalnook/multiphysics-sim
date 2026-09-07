//! Replay recorded inputs and capture declared-unit measurements on a supplied
//! integration grid. This records evidence; it is not an accuracy gate.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sim_dynamics::System;
use sim_runtime::{
    session::{Recording, Session},
    validation::{implicit_attempt_report, measurement_snapshot, state_labels},
};

#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    step_breakpoints_s: Vec<f64>,
    attempt_audit_limit: usize,
    /// Retain only the final action's attempts, including a failed action.
    /// Useful when warmup would otherwise consume bounded audit storage.
    #[serde(skip_serializing_if = "is_false")]
    audit_last_action_only: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn snapshot(session: &Session) -> Value {
    json!({
        "frame": session.frame(),
        "measurements": measurement_snapshot(session),
        "solver_stats": session.robot.runtime.islands.iter().map(|i| i.stats).collect::<Vec<_>>(),
    })
}

fn capture(recording: &Recording, config: &Config) -> Result<Value, String> {
    if recording.version != 1 {
        return Err("unsupported recording version".into());
    }
    let mut session = Session::new(recording.scene.clone(), recording.seed)?;
    session.set_attempt_audit_limit(config.attempt_audit_limit)?;
    for island in &mut session.robot.runtime.islands {
        island
            .set_step_breakpoints(config.step_breakpoints_s.clone())
            .map_err(|e| e.to_string())?;
    }
    let labels = state_labels(&session.robot.runtime);
    let island_contracts: Vec<_> = session.robot.runtime.islands.iter().enumerate().map(|(index, island)| {
        let ids: Vec<_> = island.system.full_of.iter().map(|i| island.system.state_ids[*i]).collect();
        json!({
            "island": index,
            "coordinates": ids.iter().map(|id| &labels[id]).collect::<Vec<_>>(),
            "units": ids.iter().map(|id| session.robot.runtime.model.state.entry(*id).unwrap().quantity.unit()).collect::<Vec<_>>(),
            "algebraic": island.system.algebraic().unwrap_or_else(|| vec![false; island.system.dimension()]),
        })
    }).collect();
    let mut frames = vec![snapshot(&session)];
    let mut error = None;
    let mut audit_start_s = 0.0;
    for action in &recording.actions {
        if config.audit_last_action_only {
            session.set_attempt_audit_limit(config.attempt_audit_limit)?;
            audit_start_s = session.frame().time_s;
        }
        let result = session.step(action);
        frames.push(snapshot(&session));
        if let Err(e) = result {
            error = Some(e);
            break;
        }
    }
    let attempts = if config.attempt_audit_limit > 0 {
        Some(implicit_attempt_report(&session)?)
    } else {
        None
    };
    let mut report = json!({
        "version": 1, "completed": error.is_none(), "error": error,
        "recording": recording, "config": config, "frames": frames,
        "island_contracts": island_contracts,
        "event_trace": session.robot.runtime.islands.iter().map(|i| &i.events).collect::<Vec<_>>(),
        "attempt_audit": attempts,
        "notes": [
            "Completed means replay finished, not that the trajectory is physically accurate.",
            "Breakpoints constrain integration boundaries; adaptive subdivisions can add more.",
            "Inspect attempt capacity flags before claiming complete solver-attempt coverage.",
            "Snapshots add diagnostic evaluations; do not use capture runtime as a performance benchmark."
        ]
    });
    if config.audit_last_action_only {
        report["attempt_audit_window"] = json!({
            "start_s":audit_start_s, "last_committed_time_s":session.frame().time_s,
            "scope":"Final attempted action only; earlier warmup attempts were cleared.",
        });
    }
    Ok(report)
}

fn run() -> Result<bool, String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args.len() > 2 {
        return Err("usage: capture_trajectory recording.json [config.json]".into());
    }
    let recording = serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let config = match args.get(1) {
        Some(p) => serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?,
        None => Config::default(),
    };
    let report = capture(&recording, &config)?;
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|e| e.to_string())?
    );
    Ok(report["completed"] == true)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn recording() -> Recording {
        Recording {
            version: 1,
            scene: serde_json::from_str(include_str!(
                "../../../examples/interactive/pendulum.scene.json"
            ))
            .unwrap(),
            seed: 0,
            actions: vec![vec![0.2]; 3],
        }
    }

    #[test]
    fn capture_preserves_replay_and_records_internal_states_and_requested_boundaries() {
        let recording = recording();
        let config = Config {
            step_breakpoints_s: vec![0.0037, 0.0139],
            attempt_audit_limit: 512,
            audit_last_action_only: false,
        };
        let report = capture(&recording, &config).unwrap();
        assert_eq!(report["completed"], true);
        assert_eq!(report["frames"].as_array().unwrap().len(), 4);
        let mut direct = Session::new(recording.scene.clone(), recording.seed).unwrap();
        for island in &mut direct.robot.runtime.islands {
            island
                .set_step_breakpoints(config.step_breakpoints_s.clone())
                .unwrap();
        }
        for action in &recording.actions {
            direct.step(action).unwrap();
        }
        assert_eq!(
            report["frames"][3]["frame"],
            serde_json::to_value(direct.frame()).unwrap()
        );
        assert_eq!(
            report["frames"][3]["measurements"],
            serde_json::to_value(measurement_snapshot(&direct)).unwrap()
        );
        assert!(
            report["frames"][3]["measurements"]
                .as_object()
                .unwrap()
                .len()
                > direct.frame().joint_positions.len()
        );
        let islands = report["attempt_audit"]["islands"].as_array().unwrap();
        assert!(islands.iter().all(|i| i["capacity_reached"] == false));
        let contract = &report["island_contracts"][0];
        assert_eq!(contract["coordinates"], islands[0]["coordinates"]);
        let coordinates = contract["coordinates"].as_array().unwrap();
        assert_eq!(
            contract["units"].as_array().unwrap().len(),
            coordinates.len()
        );
        let algebraic = contract["algebraic"].as_array().unwrap();
        assert_eq!(algebraic.len(), coordinates.len());
        assert!(algebraic.iter().any(|v| v == true) && algebraic.iter().any(|v| v == false));
        for boundary in &config.step_breakpoints_s {
            assert!(islands
                .iter()
                .flat_map(|i| i["attempts"].as_array().unwrap())
                .any(|a| {
                    let s = &a["solve"];
                    (s["start_time"].as_f64().unwrap() + s["step"].as_f64().unwrap() - boundary)
                        .abs()
                        < 1e-12
                }));
        }
        let mut invalid = config;
        invalid.step_breakpoints_s = vec![0.01, 0.005];
        assert!(capture(&recording, &invalid).is_err());
    }

    #[test]
    fn final_action_audit_preserves_replay_and_retains_the_requested_window() {
        let recording = recording();
        let complete = capture(
            &recording,
            &Config {
                attempt_audit_limit: 512,
                ..Default::default()
            },
        )
        .unwrap();
        let last = capture(
            &recording,
            &Config {
                attempt_audit_limit: 512,
                audit_last_action_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(complete["frames"], last["frames"]);
        assert_eq!(complete["event_trace"], last["event_trace"]);
        assert!(complete.get("attempt_audit_window").is_none());
        assert!(complete["config"].get("audit_last_action_only").is_none());
        let start = last["attempt_audit_window"]["start_s"].as_f64().unwrap();
        assert!(start > 0.0);
        for (all, retained) in complete["attempt_audit"]["islands"]
            .as_array()
            .unwrap()
            .iter()
            .zip(last["attempt_audit"]["islands"].as_array().unwrap())
        {
            assert_eq!(retained["capacity_reached"], false);
            let attempts = retained["attempts"].as_array().unwrap();
            assert!(!attempts.is_empty());
            let expected: Vec<_> = all["attempts"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|a| a["solve"]["start_time"].as_f64().unwrap() >= start)
                .cloned()
                .collect();
            assert_eq!(*attempts, expected);
            assert!(attempts.len() < all["attempts"].as_array().unwrap().len());
        }
    }
}
