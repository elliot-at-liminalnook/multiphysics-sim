use sim_phenomena::ALL;

#[test]
fn every_surprise_passes() {
    let mut failures = Vec::new();
    for (name, run) in ALL {
        let report = run();
        for failure in report.failures() {
            failures.push(format!("{name}: {failure}"));
        }
    }
    assert!(failures.is_empty(), "failed checks:\n{}", failures.join("\n"));
}
