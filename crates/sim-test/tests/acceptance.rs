use sim_test::{ScenarioName, run};

#[test]
fn all_required_scenarios_pass() {
    for scenario in ScenarioName::ALL {
        let report = run(scenario).unwrap();
        let failures = report
            .checks
            .iter()
            .filter(|check| !check.passed)
            .map(|check| format!("{}: {} ({})", check.name, check.observed, check.expectation))
            .collect::<Vec<_>>();
        assert!(
            failures.is_empty(),
            "{} failed:\n{}",
            scenario.as_str(),
            failures.join("\n")
        );
    }
}

#[test]
fn same_machine_runs_are_trace_deterministic() {
    let first = run(ScenarioName::Obstruction).unwrap();
    let second = run(ScenarioName::Obstruction).unwrap();
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
}
