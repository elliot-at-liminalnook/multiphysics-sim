#[cfg(test)]
mod device_capture_tests {
    use super::*;
    use std::collections::VecDeque;
    struct MockDevice {
        rx: VecDeque<u8>,
        armed: u16,
        torque: [u8; 9],
        pwm: [[u8; 2]; 9],
        upload: crate::controller_refinement::fpga_upload::Upload,
        events: Vec<Vec<u8>>,
        written: u16,
        read_remaining: usize,
        sealed: bool,
        corrupt: bool,
    }
    impl MockDevice {
        fn emit(&mut self, id: u8, p: &[u8]) {
            self.rx.extend(packet(id, 0, p).unwrap());
        }
        fn status(&mut self) {
            self.emit(
                254,
                &[
                    1,
                    (self.armed == 0) as u8,
                    if self.armed == 0 { 10 } else { 0 },
                    254,
                    self.armed as u8,
                    (self.armed >> 8) as u8,
                    255,
                    1,
                    60,
                    90,
                    126,
                    208,
                    7,
                ],
            );
        }
        fn receipt(&mut self, op: u8) {
            let mut p = vec![1, 0xa2, op, 0, self.sealed as u8, 0];
            let mask = self.upload.source_plan.ids.iter().fold(0u16, |m, id| m | (1 << (id - 4)));
            p.extend(mask.to_le_bytes());
            p.extend((self.upload.source_plan.targets.len() as u16).to_le_bytes());
            p.extend(self.written.to_le_bytes());
            p.extend(((self.upload.source_plan.period_s * 50_000_000.).round() as u32).to_le_bytes());
            p.extend(
                if self.sealed {
                    self.upload.plan_crc32
                } else {
                    0
                }
                .to_le_bytes(),
            );
            p.extend(50_000_000u32.to_le_bytes());
            p.push(7);
            self.emit(254, &p);
        }
    }
    impl Write for MockDevice {
        fn write(&mut self, p: &[u8]) -> std::io::Result<usize> {
            assert_eq!(p[2..].iter().fold(0u8, |s, b| s.wrapping_add(*b)), 255);
            let params = &p[5..p.len() - 1];
            match p[4] {
                0xa0 => {
                    match params[0] {
                        0 => {
                            self.armed = 0;
                            self.torque = [0; 9];
                            self.pwm = [[0; 2]; 9];
                        }
                        1 => self.armed |= 1 << (params[1] - 4),
                        5 | 2 => {}
                        _ => panic!("unexpected supervisor request"),
                    }
                    // The real UART arbiter inserts local replies between records.
                    // Keep any partially read record intact, but do not queue a
                    // heartbeat behind the entire synthetic future experiment.
                    let tail = self.rx.split_off(self.read_remaining);
                    self.status();
                    self.rx.extend(tail);
                }
                0xa2 => {
                    match params[0] {
                        0 => {
                            self.written = 0;
                            self.sealed = false;
                        }
                        1 => self.written += 1,
                        2 => self.sealed = true,
                        3 | 4 => {}
                        _ => panic!("upload op"),
                    }
                    self.receipt(params[0]);
                    if params[0] == 3 {
                        for (index, mut event) in self.events.clone().into_iter().enumerate() {
                            event[8..12].copy_from_slice(&params[5..9]);
                            let last = event.len() - 1;
                            event[last] =
                                !event[2..last].iter().fold(0u8, |s, b| s.wrapping_add(*b));
                            if self.corrupt && index == 1 {
                                event[20] ^= 1;
                            }
                            self.rx.extend(event);
                        }
                        self.armed = 0;
                        self.torque = [0; 9];
                        self.pwm = [[0; 2]; 9];
                    }
                }
                0x83 => {
                    let width = params[1] as usize;
                    for axis in params[2..].chunks(width + 1) {
                        let i = (axis[0] - 4) as usize;
                        match params[0] {
                            0x28 => self.torque[i] = axis[1],
                            0x2c => self.pwm[i].copy_from_slice(&axis[1..]),
                            _ => panic!("sync register"),
                        }
                    }
                    self.status();
                }
                3 => {
                    let i = (p[2] - 4) as usize;
                    match params[0] {
                        0x28 => self.torque[i] = params[1],
                        0x2c => self.pwm[i].copy_from_slice(&params[1..]),
                        _ => panic!("write register"),
                    }
                    self.emit(p[2], &[]);
                }
                2 => {
                    let i = (p[2] - 4) as usize;
                    let mut registers = [0; 80];
                    registers[0x21] = 2;
                    registers[0x28] = self.torque[i];
                    registers[0x2c..0x2e].copy_from_slice(&self.pwm[i]);
                    registers[0x38..0x3a].copy_from_slice(&self.upload.homes[i].to_le_bytes());
                    registers[0x3e] = 120;
                    registers[0x3f] = 40;
                    self.emit(
                        p[2],
                        &registers[params[0] as usize..params[0] as usize + params[1] as usize],
                    );
                }
                _ => panic!("unexpected packet"),
            }
            Ok(p.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Read for MockDevice {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            // Retain packet boundaries for the reply arbiter above. Fragmentation
            // and coalescing are independently covered by the Bus tests.
            if self.read_remaining == 0 && !self.rx.is_empty() {
                self.read_remaining = usize::from(self.rx[3]) + 4;
            }
            let n = out.len().min(113).min(self.rx.len()).min(self.read_remaining);
            for b in &mut out[..n] { *b = self.rx.pop_front().unwrap(); }
            self.read_remaining -= n;
            Ok(n)
        }
    }
    fn verify_uart_fixture(fixture_path: &str) {
        use crate::controller_refinement::fpga_events::Capture;
        for corrupt in [false, true] {
            let base=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(fixture_path);
            let fixture: Capture = serde_json::from_slice(&fs::read(base).unwrap()).unwrap();
            let out = std::env::temp_dir().join(format!(
                "hx-device-protocol-test-{}-{}-{corrupt}",
                std::process::id(), &blake3::hash(fixture_path.as_bytes()).to_hex()[..8]
            ));
            fs::create_dir(&out).unwrap();
            let image = out.join("synthetic-image.fs");
            fs::write(&image, b"software test only").unwrap();
            let mut plan = fixture.upload.source_plan.clone();
            plan.bitstream_path = image.to_string_lossy().into();
            plan.bitstream_blake3 = blake3::hash(b"software test only").to_hex().to_string();
            let mock = MockDevice {
                rx: VecDeque::new(),
                armed: 0,
                torque: [0; 9],
                pwm: [[0; 2]; 9],
                upload: fixture.upload,
                events: fixture.packets,
                written: 0,
                read_remaining: 0,
                sealed: false,
                corrupt,
            };
            let mut bus = Bus {
                serial: Box::new(mock),
                pending: PacketBuffer::default(),
                raw_log: Some(File::create(out.join("uart-chunks.jsonl")).unwrap()),
                log: File::create(out.join("transactions.jsonl")).unwrap(),
                start: Instant::now(),
                seq: 0,
            };
            let result =
                run_device_bench(&mut bus, &out, &plan.ids, &plan).unwrap();
            assert_eq!(result["stop_verified"], true, "{result:#}");
            assert_eq!(result["completed"], !corrupt, "{result:#}");
            assert!(out.join("device-capture.json").exists());
            if !corrupt {
                let recording: crate::controller_refinement::fpga::Recording =
                    serde_json::from_slice(&fs::read(out.join("fpga-recording.json")).unwrap())
                        .unwrap();
                recording.validate().unwrap();
                for id in &plan.ids {
                    assert_eq!(recording.frames[1].pwm_readback[(id - 4) as usize], 1000);
                    assert_eq!(recording.frames[3].pwm_readback[(id - 4) as usize], -1000);
                }
                assert_eq!(recording.initial["physical_scope_ids"], json!(plan.ids));
            }
            // Keep failures inspectable; remove only our generated successful test sandbox.
            fs::remove_dir_all(out).unwrap();
        }
    }
    #[test]
    fn full_uart_capture_and_corruption_both_verify_all_nine_stopped() {
        verify_uart_fixture("../../examples/actuators/hx30hm/hardware/2026-09-14-full-drive/verification/case0-capture.json");
    }
    #[test]
    fn fast_three_motor_uart_capture_and_corruption_verify_scope_stopped() {
        verify_uart_fixture("../../examples/actuators/hx30hm/hardware/2026-09-15-fast-loop/verification/case0-capture.json");
    }
}
