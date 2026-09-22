use sim_runtime::controller_refinement::{
    fpga_events::{Capture, Event, Kind, STREAM_ID},
    fpga_upload::Upload,
};
const BASE: &str =
    "../../examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control";

fn fixture() -> Capture {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(BASE);
    let upload = serde_json::from_slice(
        &std::fs::read(base.join("trajectory-store/vectors/upload.json")).unwrap(),
    )
    .unwrap();
    let packets =
        std::fs::read_to_string(base.join("device-events/verification/device-events.hex"))
            .unwrap()
            .lines()
            .map(|line| {
                (0..line.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&line[i..i + 2], 16).unwrap())
                    .collect()
            })
            .collect();
    Capture { upload, packets }
}

fn encode(e: &Event) -> Vec<u8> {
    let mut p = vec![1, 0xa3, e.kind as u8];
    p.extend(e.run_id.to_le_bytes());
    p.extend(e.frame.to_le_bytes());
    p.extend([e.motor_id, e.outcome, e.sequence]);
    p.extend(e.request_ticks.to_le_bytes());
    p.extend(e.completion_ticks.to_le_bytes());
    p.extend([e.device_error, e.reported_width, e.data.len() as u8]);
    p.extend(&e.data);
    sim_runtime::acquisition::servo_bus::packet(STREAM_ID, 0, &p).unwrap()
}
fn edit(c: &mut Capture, index: usize, f: impl FnOnce(&mut Event)) {
    let mut event = Event::decode(&c.packets[index]).unwrap();
    f(&mut event);
    c.packets[index] = encode(&event);
}
fn failed_audit(mut c: Capture, overlong: bool) -> Capture {
    c.packets.truncate(12);
    edit(&mut c, 11, |e| {
        e.outcome = 1;
        if overlong {
            e.reported_width = 58;
            e.data = vec![0x55; 15];
        } else {
            e.data[4] = 1;
        }
    });
    let start = Event::decode(&c.packets[0]).unwrap();
    let audit = Event::decode(&c.packets[11]).unwrap();
    let mut terminal = start.clone();
    terminal.kind = Kind::Terminal;
    terminal.outcome = 7;
    terminal.reported_width = 27;
    terminal.completion_ticks = audit.completion_ticks + 501;
    terminal.data = vec![];
    terminal
        .data
        .extend((audit.completion_ticks + 1).to_le_bytes());
    terminal
        .data
        .extend(terminal.completion_ticks.to_le_bytes());
    terminal.data.extend([0; 11]);
    c.packets.push(encode(&terminal));
    c
}

#[test]
fn rtl_bytes_cover_all_nine_axes_and_full_width_clocks() {
    let c = fixture();
    let r = c.review().unwrap();
    assert!(r.completed);
    assert_eq!(r.completed_frames, 3);
    assert_eq!(r.events.len(), 59);
    assert_eq!(r.terminal_result, Some(0));
    assert!(r.events[0].request_ticks > u32::MAX.into());
    assert_eq!(
        r.events
            .iter()
            .filter(|e| e.kind == Kind::Telemetry && e.motor_id == 4)
            .map(|e| e.sequence)
            .collect::<Vec<_>>(),
        [254, 255, 0]
    );
    assert_eq!(
        r.events.iter().filter(|e| e.kind == Kind::Audit).count(),
        27
    );
    assert!(r
        .events
        .iter()
        .zip(&c.packets)
        .all(|(e, p)| encode(e) == *p));
    let saved: Capture = serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
    assert!(saved.review().unwrap().completed);
}

#[test]
fn interrupted_prefixes_remain_unscored_and_missing_middle_is_rejected() {
    let c = fixture();
    for end in 1..c.packets.len() {
        let mut partial = c.clone();
        partial.packets.truncate(end);
        let r = partial.review().unwrap();
        assert!(!r.completed);
        assert_eq!(r.terminal_result, None);
    }
    for index in [1, 9, 10, 11, 28, 57] {
        let mut bad = c.clone();
        bad.packets.remove(index);
        assert!(bad.review().is_err(), "omitted {index}");
    }
    let mut bad = c.clone();
    bad.packets.swap(1, 2);
    assert!(bad.review().is_err());
    let mut bad = c.clone();
    bad.packets.push(c.packets[1].clone());
    assert!(bad.review().is_err());
}

#[test]
fn failed_audit_retains_raw_readback_and_cannot_be_promoted() {
    for overlong in [false, true] {
        let mut c = failed_audit(fixture(), overlong);
        let r = c.review().unwrap();
        assert!(!r.completed);
        assert_eq!(r.terminal_result, Some(7));
        assert_eq!(r.completed_frames, 0);
        assert_eq!(r.events[11].reported_width, if overlong { 58 } else { 6 });
        edit(&mut c, 12, |e| e.outcome = 0);
        assert!(c.review().is_err());
    }
    let mut c = fixture();
    edit(&mut c, 11, |e| e.outcome = 1);
    assert!(c.review().is_err(), "false failed readback");
}

#[test]
fn source_identity_sequence_timing_and_arithmetic_are_enforced() {
    let c = fixture();
    for mutation in 0..8 {
        let mut bad = c.clone();
        match mutation {
            0 => edit(&mut bad, 0, |e| e.data[0] ^= 1),
            1 => edit(&mut bad, 1, |e| e.run_id += 1),
            2 => edit(&mut bad, 20, |e| e.sequence = 254),
            3 => edit(&mut bad, 1, |e| {
                e.completion_ticks = e.request_ticks + 2_500_000
            }),
            4 => edit(&mut bad, 2, |e| e.request_ticks -= 500),
            5 => edit(&mut bad, 10, |e| e.data[0] = 1), // within limit, wrong shared-law result
            6 => edit(&mut bad, 58, |e| {
                e.completion_ticks -= 1;
                e.data[8..16].copy_from_slice(&e.completion_ticks.to_le_bytes());
                let stop = u64::from_le_bytes(e.data[0..8].try_into().unwrap()) - 1;
                e.data[0..8].copy_from_slice(&stop.to_le_bytes());
            }),
            _ => bad.upload.source_plan.gains.kp_q8 += 1,
        }
        assert!(bad.review().is_err(), "mutation {mutation}");
    }
}

#[test]
fn sparse_selection_keeps_canonical_unselected_pwm() {
    let mut c = fixture();
    let old = c.review().unwrap();
    let mut plan = c.upload.source_plan.clone();
    plan.ids = vec![4, 12];
    c.upload = Upload::compile(&plan, c.upload.homes).unwrap();
    let mut events = old
        .events
        .into_iter()
        .filter(|e| {
            matches!(e.kind, Kind::Start | Kind::Control | Kind::Terminal)
                || e.motor_id == 4
                || e.motor_id == 12
        })
        .collect::<Vec<_>>();
    events[0].data[0..4].copy_from_slice(&c.upload.plan_crc32.to_le_bytes());
    events[0].data[4..6].copy_from_slice(&257u16.to_le_bytes());
    for e in &mut events {
        if e.kind == Kind::Control {
            e.data[2..16].fill(0);
        }
    }
    c.packets = events.iter().map(encode).collect();
    assert!(c.review().unwrap().completed);
    let control = c
        .packets
        .iter()
        .position(|p| Event::decode(p).unwrap().kind == Kind::Control)
        .unwrap();
    edit(&mut c, control, |e| e.data[2] = 1);
    assert!(c.review().is_err());
}

#[test]
fn malformed_frames_and_terminal_metadata_fail_without_panics() {
    let c = fixture();
    for p in &c.packets {
        for n in 0..p.len() {
            assert!(Event::decode(&p[..n]).is_err());
        }
        let mut corrupt = p.clone();
        corrupt[8] ^= 1;
        assert!(Event::decode(&corrupt).is_err());
    }
    let mut e = Event::decode(c.packets.last().unwrap()).unwrap();
    e.data[26] = 1;
    e.data[24] = 3;
    assert!(Event::decode(&encode(&e)).is_err());
    let mut e = Event::decode(&c.packets[0]).unwrap();
    e.frame = 256;
    assert!(Event::decode(&encode(&e)).is_err());
    let mut c = c;
    edit(&mut c, 1, |e| e.data[9] = 1);
    assert!(c.review().is_err());
}
