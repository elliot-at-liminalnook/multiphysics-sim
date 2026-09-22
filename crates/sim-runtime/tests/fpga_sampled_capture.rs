use sim_runtime::controller_refinement::{fpga_batch, fpga_events::Kind};
use std::{fs, path::PathBuf};
fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/actuators/hx30hm/hardware/2026-09-20-decimated-capture/results")
}
fn packets(name: &str) -> Vec<Vec<u8>> {
    let b: Vec<u8> = fs::read_to_string(dir().join(name).join("uart.hex"))
        .unwrap()
        .split_whitespace()
        .map(|s| u8::from_str_radix(s, 16).unwrap())
        .collect();
    let mut out = vec![];
    let mut at = 0;
    while at < b.len() {
        let n = b[at + 3] as usize + 4;
        if b[at + 2] == 253 {
            out.push(b[at..at + n].to_vec());
        }
        at += n;
    }
    out
}
fn fix(p: &mut [u8]) {
    let n = p.len();
    p[n - 1] = !p[2..n - 1].iter().fold(0u8, |s, b| s.wrapping_add(*b));
}
#[test]
fn real_uart_retains_200hz_frames_while_all_400hz_transactions_continue() {
    let p = packets("case0-400hz");
    let c = fpga_batch::decode_sampled_capture(&p, 2).unwrap();
    assert_eq!(c.terminal.outcome, 0);
    assert!(!c.complete_controller_evidence);
    assert_eq!(
        c.frames.iter().map(|f| f.frame).collect::<Vec<_>>(),
        (0..25).step_by(2).collect::<Vec<_>>()
    );
    assert_eq!(
        c.intentionally_unlogged_frames,
        (1..25).step_by(2).collect::<Vec<_>>()
    );
    assert!(fpga_batch::decode_events(&p).is_err());
    let direct: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(dir().join("case0-400hz/events.json")).unwrap()).unwrap();
    assert_eq!(direct.len(), 175);
    for f in &c.frames {
        assert_eq!(f.events.len(), 7);
        assert_eq!(f.raw.len(), 99);
        assert!(!f.partial && !f.diagnostic);
        for e in &f.events {
            let kind = match e.kind {
                Kind::Telemetry => 0,
                Kind::Control => 1,
                Kind::Audit => 2,
                _ => panic!(),
            };
            let d = direct
                .iter()
                .find(|d| d["frame"] == f.frame && d["kind"] == kind && d["id"] == e.motor_id)
                .unwrap();
            assert_eq!(d["request"], e.request_ticks);
            assert_eq!(d["completion"], e.completion_ticks);
        }
    }
    assert!(
        p.iter()
            .filter(|p| fpga_batch::is_batch(p))
            .all(|p| p.len() == 226)
    );
}
#[test]
fn faults_and_odd_cycle_evidence_are_preserved() {
    for name in [
        "case1-400hz",
        "case2-200hz",
        "case3-400hz",
        "case4-400hz",
        "case5-400hz",
        "case6-400hz",
        "case7-400hz",
    ] {
        let c = fpga_batch::decode_sampled_capture(&packets(name), 2).unwrap();
        assert_ne!(c.terminal.outcome, 0, "{name}");
        if name == "case3-400hz" || name == "case4-400hz" {
            assert!(c.frames.iter().any(|f| f.frame == 1 && f.partial), "{name}");
        }
        if name == "case4-400hz" {
            assert!(c.frames.iter().any(|f| {
                f.diagnostic
                    && f.events
                        .iter()
                        .any(|e| e.kind == Kind::Audit && e.outcome != 0)
            }));
        }
    }
}
#[test]
fn sampled_review_rejects_unexpected_gaps_stride_and_fake_audit_success() {
    let p = packets("case0-400hz");
    let mut bad = p.clone();
    bad.remove(2);
    assert!(fpga_batch::decode_sampled_capture(&bad, 2).is_err());
    let mut bad = p.clone();
    bad[1][26] &= !4;
    fix(&mut bad[1]);
    assert!(fpga_batch::decode_sampled_capture(&bad, 2).is_err());
    let mut bad = p.clone();
    bad.swap(1, 2);
    assert!(fpga_batch::decode_sampled_capture(&bad, 2).is_err());
    assert!(fpga_batch::decode_sampled_capture(&p, 1).is_err());
    let mut bad = packets("case4-400hz");
    for p in &mut bad {
        if !fpga_batch::is_batch(p) {
            continue;
        }
        let count = p[23];
        let mut at = 29 + p[22] as usize;
        for _ in 0..count {
            let control = p[at] == 2;
            if p[at] == 3 && p[at + 10] != 0 {
                p[at + 10] = 0;
            }
            at += 13 + if control { 6 } else { 0 };
        }
        fix(p);
    }
    assert!(fpga_batch::decode_sampled_capture(&bad, 2).is_err());
}
