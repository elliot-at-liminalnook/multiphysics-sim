use sim_runtime::controller_refinement::live_stream::Capture;
fn fixture(case: u8) -> Capture {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/actuators/hx30hm/hardware/2026-09-15-live-leg-control/verification");
    serde_json::from_slice(&std::fs::read(root.join(format!("case{case}-capture.json"))).unwrap())
        .unwrap()
}
#[test]
fn streamed_uart_reference_and_stop_cases_reproduce_shared_controller() {
    for (case, frames, terminal) in [(0, 280, 0), (3, 1, 4), (6, 8, 3)] {
        let c = fixture(case);
        let r = c.review().unwrap();
        assert_eq!(r.completed_frames, frames);
        assert_eq!(r.terminal_result, Some(terminal));
        assert_eq!(r.completed, case == 0);
        assert!(
            !c.unverified_recording().unwrap().completed,
            "UART evidence cannot establish physical stopping"
        );
    }
}
#[test]
fn changed_reference_missing_ack_and_foreign_run_are_rejected() {
    let c = fixture(0);
    let mut bad = c.clone();
    bad.plan.targets[256][6] += 1;
    assert!(bad.review().is_err());
    let mut bad = c.clone();
    bad.batches.pop();
    assert!(bad.review().is_err());
    let mut bad = c.clone();
    bad.run_id ^= 1;
    assert!(bad.review().is_err());
    let mut bad = c;
    bad.batches[1].request[13] ^= 1;
    assert!(bad.review().is_err());
}
