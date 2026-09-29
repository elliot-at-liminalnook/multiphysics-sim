//! Screening and proposal behaviour for gait search, checked against the
//! recorded 2026-09-19 comparison (198 attempts) and synthetic problems.
use sim_domain_control::motion_primitives::ContactTemplate;
use sim_runtime::contact_reference;
use std::path::Path;

const STUDY: &str = "../../examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19";

#[test]
fn schedule_screen_rejects_every_recorded_stop_failure_and_no_success() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(STUDY);
    let template: ContactTemplate = serde_json::from_str(&std::fs::read_to_string(root.join("contact-template.json")).unwrap()).unwrap();
    let (mut stop, mut caught_stop, mut ok, mut rejected_ok, mut other_caught) = (0, 0, 0, 0, 0);
    for dir in ["comparison", "comparison-independent-seed"] {
        for entry in std::fs::read_dir(root.join(dir)).unwrap() {
            let path = entry.unwrap().path().join("trial.json");
            if !path.is_file() {
                continue;
            }
            let trial: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            let values: Vec<f64> = serde_json::from_value(trial["observation"]["values"].clone()).unwrap();
            let named = template.space.named_values(&values).unwrap();
            let screened = contact_reference::pause_windows(&template.materialize(&named).unwrap(), 0.06).is_err();
            let outcome = &trial["observation"]["outcome"];
            let reason = outcome["reason"].as_str().unwrap_or("");
            if outcome["status"] == "complete" {
                ok += 1;
                rejected_ok += usize::from(screened);
            } else if reason.contains("all-stance") {
                stop += 1;
                caught_stop += usize::from(screened);
            } else {
                other_caught += usize::from(screened);
            }
        }
    }
    eprintln!("stop failures caught {caught_stop}/{stop}; other failures caught {other_caught}; successes rejected {rejected_ok}/{ok}");
    assert_eq!((stop, ok), (20, 87), "recorded study");
    assert_eq!(caught_stop, stop, "every stop-controller failure is predicted without kinematics");
    assert_eq!(rejected_ok, 0, "no gait that completed is rejected");
}

/// Proposal behaviour needs the optimizer backends (feature `evolution`).
#[cfg(feature = "evolution")]
mod proposals {
use sim_runtime::search_comparison::{self, Algorithm, Settings};
use sim_solve::bayesian::{Observation, Outcome, Parameter, Problem};

fn problem() -> Problem {
    Problem {
        context_id: "synthetic-screening".into(),
        parameters: (0..2).map(|i| Parameter { name: format!("x{i}"), unit: "1".into(), bounds: [0., 1.] }).collect(),
        objective_name: "f".into(),
        objective_unit: "1".into(),
        constraints: vec![],
    }
}
fn settings(candidates: usize) -> Settings {
    Settings { seed: 7, initial_design: 4, acquisition_starts: 4, maximum_training_rows: 64, cma_population: 6, cma_sigma: 0.3, feasibility_candidates: candidates, feasibility_length_scale: None }
}
fn observation(p: &Problem, values: Vec<f64>, ok: bool) -> Observation {
    Observation {
        context_id: p.context_id.clone(),
        outcome: if ok { Outcome::Complete { objective: (values[0] - 0.3).powi(2) + (values[1] - 0.2).powi(2), residuals: vec![] } } else { Outcome::Failed { reason: "infeasible".into() } },
        values,
        evidence: "synthetic".into(),
    }
}

#[test]
fn parallel_screened_proposals_equal_one_at_a_time() {
    // Feasible only below the diagonal; "schedule" rejects x1 > 0.9 cheaply.
    let p = problem();
    let baseline = vec![0.2, 0.1];
    for algorithm in [Algorithm::CmaEs, Algorithm::Bayesian] {
        let run = |parallel: usize| {
            let mut history = vec![observation(&p, baseline.clone(), true)];
            let mut log = Vec::new();
            for _ in 0..6 {
                let (screened, accepted) = search_comparison::propose_until_prepared(
                    &p, &baseline, &history, algorithm, &settings(3), parallel, 40,
                    |v| if v[1] > 0.9 { Err("schedule".into()) } else { Ok(()) },
                    |v| if v[0] + v[1] < 0.8 { Ok(v.to_vec()) } else { Err("prepare".into()) },
                    |i| format!("candidate-{i}"),
                ).unwrap();
                for s in &screened {
                    history.push(s.observation.clone());
                    log.push((s.stage.clone(), s.observation.values.clone()));
                }
                let (values, prepared, _) = accepted.expect("a feasible candidate within budget");
                assert_eq!(values, prepared);
                assert!(values[0] + values[1] < 0.8);
                history.push(observation(&p, values.clone(), true));
                log.push(("trial".into(), values));
            }
            log
        };
        assert_eq!(run(1), run(4), "{algorithm:?}: thread timing and batch size never change proposals");
    }
}

#[test]
fn feasibility_falls_near_failures_and_guides_bayesian_proposals() {
    let p = problem();
    let history = vec![
        observation(&p, vec![0.2, 0.2], true),
        observation(&p, vec![0.25, 0.1], true),
        observation(&p, vec![0.9, 0.9], false),
        observation(&p, vec![0.85, 0.95], false),
    ];
    let near_fail = search_comparison::feasibility(&p, &history, &[0.88, 0.9], 0.15).unwrap();
    let near_ok = search_comparison::feasibility(&p, &history, &[0.22, 0.15], 0.15).unwrap();
    let far = search_comparison::feasibility(&p, &[], &[0.5, 0.5], 0.15).unwrap();
    assert!(near_fail < 0.3 && near_ok > 0.7 && (far - 0.5).abs() < 1e-12, "{near_fail} {near_ok} {far}");
}
}
