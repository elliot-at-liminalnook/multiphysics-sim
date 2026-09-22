use sim_runtime::controller_refinement::{fpga::Plan, fpga_upload::Upload};
fn plan() -> Plan {
    Plan {
        control: "fpga_pd".into(),
        name: "offline trajectory fixture".into(),
        role: "timing".into(),
        ids: (4..=12).collect(),
        period_s: 0.05,
        gains: sim_domain_control::fixed_pd::Gains {
            kp_q8: 1536,
            kd_q8: 512,
            kv_q8: 1536,
            limit: 75,
        },
        targets: vec![
            [0; 9],
            [8, -8, 8, -8, 8, -8, 8, -8, 8],
            [-8, 8, -8, 8, -8, 8, -8, 8, -8],
        ],
        rms_limit_counts: 3.,
        peak_limit_counts: 10.,
        bitstream_path: "synthetic-not-loaded".into(),
        bitstream_blake3: "0".repeat(64),
    }
}
#[test]
fn uploads_are_bounded_nonexecuting_and_agree_with_existing_controller_packets() {
    let p = plan();
    let homes = std::array::from_fn(|i| 2048 + i as u16 * 10);
    let upload = Upload::compile(&p, homes).unwrap();
    upload.validate().unwrap();
    assert_eq!(upload.packets.len(), 5);
    assert_eq!(
        upload.packets.iter().map(Vec::len).collect::<Vec<_>>(),
        [42, 45, 45, 45, 11]
    );
    for (i, packet) in upload.packets.iter().enumerate() {
        assert_eq!(&packet[..3], &[255, 255, 254]);
        assert_eq!(packet[4], 0xa2);
        assert_eq!(packet[3] as usize + 4, packet.len());
        assert_eq!(packet[2..].iter().fold(0u8, |a, b| a.wrapping_add(*b)), 255);
        assert_eq!(
            packet[5],
            if i == 0 {
                0
            } else if i == 4 {
                2
            } else {
                1
            }
        );
        if (1..=3).contains(&i) {
            assert_eq!(&packet[8..44], &p.parameters(i - 1, &homes).unwrap()[10..]);
        }
    }
    let bytes = upload.canonical_bytes().unwrap();
    assert_eq!(bytes.len(), 34 + 3 * 36);
    assert_eq!(&bytes[..8], &[255, 1, 3, 0, 160, 37, 38, 0]);
    let archive: Upload = serde_json::from_slice(&serde_json::to_vec(&upload).unwrap()).unwrap();
    archive.validate().unwrap();
}
#[test]
fn source_or_packet_changes_invalidate_saved_upload() {
    let good = Upload::compile(&plan(), [2048; 9]).unwrap();
    let mut bad = good.clone();
    bad.source_plan.role = "training".into();
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    bad.homes[0] += 1;
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    bad.packets[1][8] ^= 1;
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    bad.plan_crc32 ^= 1;
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    bad.clock_hz = 48_000_000;
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    bad.protocol_version = 2;
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    bad.packets.clear();
    assert!(bad.canonical_bytes().is_err());
    let mut bad = good;
    bad.packets.push(vec![255, 255, 254, 3, 160, 2, 0]);
    assert!(bad.validate().is_err());
}
#[test]
fn rejects_unrepresentable_timing_invalid_homes_and_firmware_identity() {
    let mut p = plan();
    p.period_s = 0.050000001;
    assert!(Upload::compile(&p, [2048; 9]).is_err());
    p = plan();
    assert!(Upload::compile(&p, [599; 9]).is_err());
    assert!(Upload::compile(&p, [3496; 9]).is_err());
    p = plan();
    p.bitstream_blake3 = "x".repeat(64);
    assert!(Upload::compile(&p, [2048; 9]).is_err());
    p = plan();
    p.targets = vec![[0; 9]; 241];
    assert!(Upload::compile(&p, [2048; 9]).is_err());
    p.targets = vec![[0; 9]; 240];
    assert!(Upload::compile(&p, [2048; 9]).is_ok());
}
#[test]
fn sparse_plan_zeroes_unused_axes_but_preserves_original_evidence() {
    let mut p = plan();
    p.ids = vec![4, 12];
    let homes = [2048, 0, 65535, 0, 0, 0, 0, 0, 3495];
    let upload = Upload::compile(&p, homes).unwrap();
    assert_eq!(upload.homes, homes);
    assert_eq!(upload.source_plan.targets, p.targets);
    let bytes = upload.canonical_bytes().unwrap();
    assert_eq!(&bytes[18..32], &[0; 14]);
    for row in bytes[34..].chunks_exact(36) {
        assert_eq!(&row[4..32], &[0; 28]);
    }
    assert_eq!(&bytes[..2], &[1, 1]);
}
#[test]
fn rtl_fixture_is_reproducible_from_its_frozen_rust_source() {
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/trajectory-store/vectors");
    let upload: Upload =
        serde_json::from_slice(&std::fs::read(root.join("upload.json")).unwrap()).unwrap();
    upload.validate().unwrap();
    let bytes = upload.canonical_bytes().unwrap();
    let hex = |b: &[u8]| {
        b.iter()
            .rev()
            .map(|v| format!("{v:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        std::fs::read_to_string(root.join("header.hex")).unwrap(),
        format!("{}\n", hex(&bytes[..34]))
    );
    assert_eq!(
        std::fs::read_to_string(root.join("rows.hex")).unwrap(),
        bytes[34..]
            .chunks_exact(36)
            .map(|r| format!("{}\n", hex(r)))
            .collect::<String>()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("crc.hex")).unwrap(),
        format!("{:08x}\n", upload.plan_crc32)
    );
    let packets = upload
        .packets
        .iter()
        .map(|p| {
            let mut padded = [0u8; 64];
            padded[..p.len()].copy_from_slice(p);
            format!("{}\n", hex(&padded))
        })
        .collect::<String>();
    let lengths = upload
        .packets
        .iter()
        .map(|p| format!("{:02x}\n", p.len()))
        .collect::<String>();
    assert_eq!(
        std::fs::read_to_string(root.join("packets.hex")).unwrap(),
        packets
    );
    assert_eq!(
        std::fs::read_to_string(root.join("lengths.hex")).unwrap(),
        lengths
    );
}

#[test]
fn actual_rtl_uart_receipts_decode_and_confirm_each_upload_stage() {
    use sim_runtime::controller_refinement::fpga_upload::Receipt;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control",
    );
    let upload: Upload = serde_json::from_slice(
        &std::fs::read(root.join("trajectory-store/vectors/upload.json")).unwrap(),
    )
    .unwrap();
    let text =
        std::fs::read_to_string(root.join("bridge-trajectory/verification/uart-replies.hex"))
            .unwrap();
    let bytes: Vec<u8> = text
        .lines()
        .map(|s| u8::from_str_radix(s, 16).unwrap())
        .collect();
    let mut cursor = 0;
    let mut receipts = Vec::new();
    let mut stop_statuses = 0;
    while cursor < bytes.len() {
        let size = usize::from(bytes[cursor + 3]) + 4;
        let frame = &bytes[cursor..cursor + size];
        if size == 31 {
            receipts.push(Receipt::decode(frame).unwrap());
        } else {
            assert_eq!(size, 19);
            sim_runtime::acquisition::servo_bus::reply(frame, 254, 13).unwrap();
            stop_statuses += 1;
        }
        cursor += size;
    }
    assert_eq!(stop_statuses, 1);
    assert!(receipts.len() >= 12);
    for (index, receipt) in receipts[..5].iter().enumerate() {
        receipt.check_for(&upload, index).unwrap();
    }
    assert!(receipts[0].check_for(&upload, 1).is_err());
    assert!(
        receipts[3].check_for(&upload, 4).is_err(),
        "last row receipt is not a seal"
    );
    assert!(receipts[6].rejected);
    assert!(!receipts[6].valid);
    assert!(receipts[6].check_for(&upload, 1).is_err());
    let mut wrong = receipts[4].clone();
    wrong.crc32 ^= 1;
    assert!(wrong.check_for(&upload, 4).is_err());
    let mut wrong = receipts[4].clone();
    wrong.capabilities = 5;
    assert!(wrong.validate().is_err());
    let mut bad_frame = bytes[..31].to_vec();
    bad_frame[5] = 2;
    bad_frame[30] = !bad_frame[2..30]
        .iter()
        .fold(0u8, |sum, b| sum.wrapping_add(*b));
    assert!(
        Receipt::decode(&bad_frame).is_err(),
        "unknown protocol must not be accepted"
    );
    let request = sim_runtime::controller_refinement::fpga_upload::status_request();
    assert_eq!(&request[..6], &[255, 255, 254, 3, 162, 4]);
}

#[test]
fn upload_transfer_checks_capability_and_stops_on_rejection_or_cancellation() {
    use sim_runtime::controller_refinement::fpga_upload::{Transfer, transfer};
    use std::sync::atomic::{AtomicBool, Ordering};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control",
    );
    let upload: Upload = serde_json::from_slice(
        &std::fs::read(root.join("trajectory-store/vectors/upload.json")).unwrap(),
    )
    .unwrap();
    let text =
        std::fs::read_to_string(root.join("bridge-trajectory/verification/uart-replies.hex"))
            .unwrap();
    let bytes: Vec<u8> = text
        .lines()
        .map(|s| u8::from_str_radix(s, 16).unwrap())
        .collect();
    let frames: Vec<Vec<u8>> = bytes[..7 * 31]
        .chunks_exact(31)
        .map(|r| r.to_vec())
        .collect();
    let cancel = AtomicBool::new(false);
    let mut requests = Vec::new();
    let completed = transfer(&upload, &cancel, |packet| {
        requests.push(packet.to_vec());
        Ok(if requests.len() == 1 {
            frames[5].clone()
        } else {
            frames[requests.len() - 2].clone()
        })
    })
    .unwrap();
    assert!(completed.completed);
    assert_eq!(requests.len(), 6);
    assert!(requests.iter().all(|p| p[4] == 0xa2));
    completed.validate_capture(&upload).unwrap();
    let mut reloaded: Transfer =
        serde_json::from_slice(&serde_json::to_vec(&completed).unwrap()).unwrap();
    reloaded.replies.pop();
    assert!(reloaded.validate_capture(&upload).is_err());
    let mut count = 0;
    let rejected = transfer(&upload, &cancel, |_| {
        count += 1;
        Ok(match count {
            1 => frames[5].clone(),
            2 => frames[0].clone(),
            _ => frames[6].clone(),
        })
    })
    .unwrap();
    assert_eq!(count, 3);
    assert!(!rejected.completed);
    assert!(rejected.failure.unwrap().contains("rejected"));
    let cancelled = transfer(&upload, &AtomicBool::new(true), |_| {
        panic!("cancelled upload must not access transport")
    })
    .unwrap();
    assert!(cancelled.cancelled);
    cancelled.validate_capture(&upload).unwrap();
    let mut count = 0;
    let cancelled = transfer(&upload, &cancel, |_| {
        count += 1;
        if count == 1 {
            Ok(frames[5].clone())
        } else {
            cancel.store(true, Ordering::Relaxed);
            Ok(frames[0].clone())
        }
    })
    .unwrap();
    assert!(cancelled.cancelled);
    assert_eq!(count, 2);
    assert_eq!(cancelled.replies.len(), 1);
    cancelled.validate_capture(&upload).unwrap();
    let failed = transfer(&upload, &AtomicBool::new(false), |_| {
        Err("link timeout".into())
    })
    .unwrap();
    failed.validate_capture(&upload).unwrap();
    assert_eq!(failed.failure.as_deref(), Some("link timeout"));
}

#[test]
fn fast_device_period_is_limited_to_three_motors() {
    let mut p = plan();
    p.control = "fpga_device_pd".into();
    p.ids = vec![4, 8, 12];
    p.period_s = 0.01;
    p.gains.limit = 1000;
    Upload::compile(&p, [2048; 9]).unwrap().validate().unwrap();
    p.period_s = 0.00999998;
    assert!(Upload::compile(&p, [2048; 9]).is_err());
    p.period_s = 0.01;
    p.ids = vec![4, 8, 11, 12];
    assert!(Upload::compile(&p, [2048; 9]).is_err());
    p.period_s = 0.04;
    Upload::compile(&p, [2048; 9]).unwrap().validate().unwrap();
    let mut receipt = sim_runtime::controller_refinement::fpga_upload::Receipt {
        operation: 2,
        rejected: false,
        valid: true,
        busy: false,
        mask: 448,
        frames: 3,
        written: 3,
        period_ticks: 500_000,
        crc32: 0,
        clock_hz: 50_000_000,
        capabilities: 7,
    };
    receipt.validate().unwrap();
    receipt.mask = 449;
    assert!(receipt.validate().is_err());
    receipt.mask = 448;
    receipt.period_ticks = 499_999;
    assert!(receipt.validate().is_err());
}

#[test]
fn full_drive_requires_device_clock_and_40ms_period() {
    let mut p = plan();
    p.gains.limit = 1000;
    assert!(p.validate().is_err());
    p.control = "fpga_device_pd".into();
    assert!(p.validate().is_err());
    p.period_s = 0.04;
    Upload::compile(&p, [2048; 9]).unwrap().validate().unwrap();
    p.gains.limit = 1001;
    assert!(p.validate().is_err());
    p.gains.limit = 1000;
    p.period_s = 0.039;
    assert!(p.validate().is_err());
}

#[test]
fn start_binds_sealed_plan_identity_cadence_and_compiled_gains() {
    use sim_runtime::controller_refinement::fpga_upload::Receipt;
    let mut p = plan();
    assert!(
        Upload::compile(&p, [2048; 9])
            .unwrap()
            .start_request(17)
            .is_err()
    );
    p.control = "fpga_device_pd".into();
    p.period_s = 0.04;
    p.gains = sim_domain_control::fixed_pd::Gains {
        kp_q8: 4096,
        kd_q8: 0,
        kv_q8: 4096,
        limit: 1000,
    };
    let u = Upload::compile(&p, [2048; 9]).unwrap();
    let packet = u.start_request(0x12345678).unwrap();
    assert_eq!(packet.len(), 15);
    assert_eq!(&packet[6..10], &u.plan_crc32.to_le_bytes());
    assert_eq!(&packet[10..14], &0x12345678u32.to_le_bytes());
    assert_eq!(packet[2..].iter().fold(0u8, |s, b| s.wrapping_add(*b)), 255);
    let receipt = Receipt {
        operation: 3,
        rejected: false,
        valid: true,
        busy: false,
        mask: 511,
        frames: 3,
        written: 3,
        period_ticks: 2_000_000,
        crc32: u.plan_crc32,
        clock_hz: 50_000_000,
        capabilities: 7,
    };
    receipt.check_start(&u).unwrap();
    for mutate in [
        |r: &mut Receipt| r.crc32 ^= 1,
        |r: &mut Receipt| r.period_ticks += 1,
        |r: &mut Receipt| r.capabilities = 1,
        |r: &mut Receipt| r.mask = 255,
        |r: &mut Receipt| r.operation = 2,
        |r: &mut Receipt| r.rejected = true,
    ] {
        let mut bad = receipt.clone();
        mutate(&mut bad);
        assert!(bad.check_start(&u).is_err());
    }
    p.gains.kp_q8 = 2048;
    let changed = Upload::compile(&p, [2048; 9]).unwrap();
    let mut mismatch = receipt;
    mismatch.crc32 = changed.plan_crc32;
    assert!(mismatch.check_start(&changed).is_err());
    mismatch.capabilities = 3;
    mismatch.check_start(&changed).unwrap();
    mismatch.capabilities = 27;
    mismatch.check_start(&changed).unwrap();
    p.gains.kp_q8 = 2047;
    let unsupported = Upload::compile(&p, [2048; 9]).unwrap();
    mismatch.crc32 = unsupported.plan_crc32;
    assert!(mismatch.check_start(&unsupported).is_err());
}
