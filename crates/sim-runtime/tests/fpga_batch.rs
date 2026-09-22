use sim_runtime::controller_refinement::{
    fpga_batch::{self, Frame},
    fpga_events::{self, Kind},
    fpga_upload::Upload,
};
use std::{fs, path::PathBuf};
fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/actuators/hx30hm/hardware/2026-09-20-buffered-capture/results")
}
fn packets(case: &str) -> Vec<Vec<u8>> {
    let b: Vec<_> = fs::read_to_string(dir().join(case).join("uart.hex"))
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
    let last = p.len() - 1;
    p[last] = !p[2..last].iter().fold(0u8, |s, b| s.wrapping_add(*b));
}
#[test]
fn actual_rtl_batches_pass_existing_controller_and_timing_auditor() {
    let upload:Upload=serde_json::from_slice(&fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/actuators/hx30hm/hardware/2026-09-15-fast-loop/verification/rtl-upload/upload.json")).unwrap()).unwrap();
    let packets = packets("case0-100hz");
    let review = fpga_events::review_execution(
        &upload.source_plan,
        upload.homes,
        upload.plan_crc32,
        &packets,
    )
    .unwrap();
    assert!(review.completed);
    assert_eq!(review.completed_frames, 25);
    assert_eq!(review.events.len(), 177);
    let direct: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(dir().join("case0-100hz/events.json")).unwrap()).unwrap();
    for (e, d) in review
        .events
        .iter()
        .filter(|e| matches!(e.kind, Kind::Telemetry | Kind::Control | Kind::Audit))
        .zip(direct)
    {
        assert_eq!(e.request_ticks, d["request"].as_u64().unwrap());
        assert_eq!(e.completion_ticks, d["completion"].as_u64().unwrap());
        assert_eq!(u64::from(e.motor_id), d["id"].as_u64().unwrap());
    }
    let frames: Vec<_> = packets
        .iter()
        .filter(|p| fpga_batch::is_batch(p))
        .map(|p| Frame::decode(p).unwrap())
        .collect();
    assert_eq!(frames.len(), 25);
    assert!(frames.iter().all(|f| f.raw.len() == 99
        && f.events.len() == 7
        && !f.partial
        && f.dropped_raw_bytes == 0));
    assert!(
        packets
            .iter()
            .filter(|p| fpga_batch::is_batch(p))
            .all(|p| p.len() == 226)
    );
}
#[test]
fn truncation_checksums_and_reference_corruption_are_rejected() {
    let p = packets("case0-100hz")
        .into_iter()
        .find(|p| fpga_batch::is_batch(p))
        .unwrap();
    for end in 0..p.len() {
        assert!(Frame::decode(&p[..end]).is_err());
    }
    for index in [0, 3, 4, 5, 6, 7, 22, 23, 24, 26, 29] {
        let mut bad = p.clone();
        bad[index] ^= 128;
        assert!(Frame::decode(&bad).is_err());
    }
    let meta = 29 + p[22] as usize;
    for (index, value) in [
        (meta + 9, 0),
        (meta + 1, 9),
        (meta + 12, 255),
        (meta + 11, 1),
        (meta, 4),
    ] {
        let mut bad = p.clone();
        bad[index] = value;
        fix(&mut bad);
        assert!(Frame::decode(&bad).is_err(), "index {index}");
    }
    // An outer checksum cannot conceal corrupted underlying motor evidence.
    let mut bad = p.clone();
    bad[35] ^= 1;
    fix(&mut bad);
    assert!(Frame::decode(&bad).is_err());
}
#[test]
fn overflow_is_decodable_as_evidence_but_cannot_pass_audit() {
    let mut p = packets("case0-100hz")
        .into_iter()
        .find(|p| fpga_batch::is_batch(p))
        .unwrap();
    p[26] = 3;
    p[27] = 1;
    fix(&mut p);
    assert_eq!(Frame::decode(&p).unwrap().dropped_raw_bytes, 1);
    assert!(
        fpga_batch::decode_events(&[p])
            .unwrap_err()
            .contains("overflow")
    );
}

#[test]
fn batch_identity_cannot_be_hidden_by_expansion() {
    let original = packets("case0-100hz");
    for index in [8, 12, 14, 24] {
        let mut changed = original.clone();
        changed[1][index] ^= 1;
        fix(&mut changed[1]);
        assert!(fpga_batch::decode_events(&changed).is_err());
    }
    let mut changed = original.clone();
    changed.remove(1);
    assert!(fpga_batch::decode_events(&changed).is_err());
    let mut changed = original;
    changed[1][26] = 1;
    fix(&mut changed[1]);
    assert!(fpga_batch::decode_events(&changed).is_err());
}

#[test]
fn faster_cadences_and_faults_use_the_same_auditor_without_relaxing_acquisition() {
    let original:Upload=serde_json::from_slice(&fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/actuators/hx30hm/hardware/2026-09-15-fast-loop/verification/rtl-upload/upload.json")).unwrap()).unwrap();
    for (case, hz) in [
        (0, 200),
        (0, 400),
        (1, 200),
        (2, 200),
        (3, 200),
        (4, 200),
        (5, 200),
        (6, 200),
        (7, 200),
    ] {
        let mut plan = original.source_plan.clone();
        plan.period_s = 1. / hz as f64;
        assert!(plan.validate().is_err());
        assert!(Upload::compile(&plan, original.homes).is_err());
        let upload = Upload::compile_offline_rate_study(&plan, original.homes).unwrap();
        assert!(upload.validate().is_err());
        let packets = packets(&format!("case{case}-{hz}hz"));
        let review = fpga_events::review_offline_rate_study(
            &plan,
            upload.homes,
            upload.plan_crc32,
            &packets,
        )
        .unwrap();
        assert_eq!(review.completed, case == 0, "case {case}");
        if case == 0 {
            assert_eq!(review.completed_frames, 25);
        }
        if case == 4 {
            assert!(
                review
                    .events
                    .iter()
                    .any(|e| e.kind == Kind::Audit && e.outcome == 1)
            );
        }
        if case == 6 || case == 7 {
            let partial = packets
                .iter()
                .find(|p| fpga_batch::is_batch(p))
                .map(|p| Frame::decode(p).unwrap())
                .unwrap();
            assert!(partial.partial);
            assert!(partial.events.is_empty());
            assert_eq!(partial.raw.len(), if case == 6 { 21 } else { 7 });
            if case == 6 {
                assert_ne!(
                    partial.raw[2..].iter().fold(0u8, |s, b| s.wrapping_add(*b)),
                    255
                );
            }
        }
    }
}
