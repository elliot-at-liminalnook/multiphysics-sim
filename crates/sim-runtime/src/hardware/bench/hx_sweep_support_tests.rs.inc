#[cfg(test)]
mod sweep_tests {
    use super::*;
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };
    // Protocol double only: registers are deliberately stationary. Motor
    // physics is validated separately through the shared compiled runtime.
    struct State {
        registers: [[u8; 71]; 9],
        armed: u16,
        latched: bool,
        motion_packets: usize,
        inject_current: bool,
        saw_current: bool,
        clock_ms: u64,
        feedback_ms: [u64; 9],
        heartbeat_ms: [u64; 9],
        late_settle: bool,
        was_driven: bool,
        zero_at: Option<Instant>,
    }
    struct Mock {
        state: Arc<Mutex<State>>,
        rx: VecDeque<u8>,
    }
    impl State {
        fn new(inject_current: bool) -> Self {
            let mut s = Self {
                registers: [[0; 71]; 9],
                armed: 0,
                latched: true,
                motion_packets: 0,
                inject_current,
                saw_current: false,
                clock_ms: 0,
                feedback_ms: [0; 9],
                heartbeat_ms: [0; 9],
                late_settle: false,
                was_driven: false,
                zero_at: None,
            };
            for (i, r) in s.registers.iter_mut().enumerate() {
                r[5] = (i + 4) as u8;
                r[0x21] = 2;
                r[0x37] = 1;
                r[0x39] = 8;
                r[0x3e] = 120;
                r[0x3f] = 40;
            }
            s
        }
        fn stop(&mut self) {
            self.latched = true;
            self.armed = 0;
            for r in &mut self.registers {
                r[0x28] = 0;
                r[0x2c] = 0;
                r[0x2d] = 0;
            }
        }
        fn status(&self) -> Vec<u8> {
            vec![
                1,
                self.latched as u8,
                if self.latched { 10 } else { 0 },
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
            ]
        }
    }
    impl Write for Mock {
        fn write(&mut self, p: &[u8]) -> std::io::Result<usize> {
            assert_eq!(p[2..].iter().fold(0u8, |a, b| a.wrapping_add(*b)), 255);
            let mut s = self.state.lock().unwrap();
            // Hardware transactions measured about 6 ms each. Advance a
            // deterministic transport clock so all-nine setup/cleanup cannot
            // pass the mock while exceeding real independent watchdog leases.
            s.clock_ms += 6;
            if (0..9).any(|n| {
                s.armed & (1 << n) != 0
                    && (s.clock_ms - s.feedback_ms[n] >= 200
                        || s.clock_ms - s.heartbeat_ms[n] >= 300)
            }) {
                s.stop();
            }
            let id = p[2];
            let inst = p[4];
            let params = &p[5..p.len() - 1];
            let payload = if id == 254 && inst == 0xa0 {
                match params[0] {
                    0 | 4 => s.stop(),
                    1 => {
                        let n = (params[1] - 4) as usize;
                        assert!(s.clock_ms - s.feedback_ms[n] < 200);
                        s.latched = false;
                        s.armed |= 1 << (params[1] - 4);
                        s.heartbeat_ms[n] = s.clock_ms;
                    }
                    2 => {}
                    3 => {
                        s.heartbeat_ms[(params[1] - 4) as usize] = s.clock_ms;
                    }
                    _ => panic!("unknown supervisor command"),
                };
                s.status()
            } else if id == 254 && inst == 0x83 {
                let width = params[1] as usize;
                assert!(params[..2] == [0x2c, 2] || params[..2] == [0x28, 1]);
                for c in params[2..].chunks(width + 1) {
                    let n = (c[0] - 4) as usize;
                    if params[0] == 0x2c {
                        if c[1] != 0 || c[2] != 0 {
                            assert!(
                                !s.latched && s.armed & (1 << n) != 0 && s.registers[n][0x28] == 1
                            );
                            s.motion_packets += 1;
                            s.was_driven = true;
                        } else if s.was_driven {
                            s.zero_at = Some(Instant::now());
                            s.was_driven = false;
                        }
                        s.registers[n][0x2c..0x2e].copy_from_slice(&c[1..]);
                        // Conservative device behavior: even a zero PWM write
                        // may enable braking. Coast tests must not send it.
                        s.registers[n][0x28] = 1;
                    } else {
                        if c[1] != 0 {
                            assert!(!s.latched && s.armed & (1 << n) != 0);
                        }
                        s.registers[n][0x28] = c[1];
                    }
                }
                s.status()
            } else {
                let n = (id - 4) as usize;
                let addr = params[0] as usize;
                if inst == 2 {
                    if addr == 0x38 {
                        s.feedback_ms[n] = s.clock_ms;
                    }
                    if addr == 0x38 && s.late_settle {
                        let settling = s
                            .zero_at
                            .is_some_and(|at| (0.45..0.52).contains(&at.elapsed().as_secs_f64()));
                        s.registers[n][0x3a] = if settling { 100 } else { 0 };
                    }
                    if addr == 0x38 && s.inject_current && s.motion_packets > 0 && !s.saw_current {
                        s.registers[n][0x45] = 208;
                        s.registers[n][0x46] = 7;
                        s.saw_current = true;
                    }
                    if addr == 0x28 && params[1] == 1 && s.registers[n][addr] == 0 {
                        s.armed &= !(1 << n);
                    }
                    s.registers[n][addr..addr + params[1] as usize].to_vec()
                } else {
                    assert_eq!(inst, 3);
                    s.registers[n][addr..addr + params.len() - 1].copy_from_slice(&params[1..]);
                    if addr == 0x2c {
                        s.registers[n][0x28] = 1;
                    }
                    vec![]
                }
            };
            self.rx.extend(packet(id, 0, &payload).unwrap());
            Ok(p.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Read for Mock {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            // Force short reads, exercising the real transaction assembler.
            let count = b.len().min(self.rx.len()).min(3);
            for byte in &mut b[..count] {
                *byte = self.rx.pop_front().unwrap();
            }
            Ok(count)
        }
    }
    fn run_case(inject: bool, waveforms: bool) -> (serde_json::Value, Arc<Mutex<State>>, PathBuf) {
        run_case_groups(inject, waveforms, true)
    }
    fn run_case_groups(
        inject: bool,
        waveforms: bool,
        individual: bool,
    ) -> (serde_json::Value, Arc<Mutex<State>>, PathBuf) {
        run_case_options(inject, waveforms, individual, false)
    }
    fn run_case_options(
        inject: bool,
        waveforms: bool,
        individual: bool,
        late_settle: bool,
    ) -> (serde_json::Value, Arc<Mutex<State>>, PathBuf) {
        run_case_rotation(inject, waveforms, individual, late_settle, false)
    }
    fn run_case_rotation(
        inject: bool,
        waveforms: bool,
        individual: bool,
        late_settle: bool,
        rotation: bool,
    ) -> (serde_json::Value, Arc<Mutex<State>>, PathBuf) {
        let out = std::env::temp_dir().join(format!(
            "hx-sweep-test-{}-{}-{}-{}-{}-{}",
            std::process::id(),
            inject,
            waveforms,
            individual,
            late_settle,
            rotation
        ));
        if out.exists() {
            fs::remove_dir_all(&out).unwrap();
        }
        fs::create_dir(&out).unwrap();
        let state = Arc::new(Mutex::new(State::new(inject)));
        state.lock().unwrap().late_settle = late_settle;
        if rotation {
            for r in &mut state.lock().unwrap().registers {
                r[0x38] = 250;
                r[0x39] = 15;
            }
        }
        let mut bus = Bus {
            pending: PacketBuffer::default(),
            raw_log: None,
            serial: Box::new(Mock {
                state: state.clone(),
                rx: VecDeque::new(),
            }),
            log: File::create(out.join("transactions.jsonl")).unwrap(),
            start: Instant::now(),
            seq: 0,
        };
        let mut plan = SweepPlan {
            control: "pwm_sweep".into(),
            schedule: SweepSchedule {
                levels: vec![25],
                repetitions: 1,
                include_concurrent: true,
                include_individual: individual,
            },
            pulse_ms: 200,
            rest_ms: 500,
            travel_limit_counts: 1200,
            free_rotation_maximum_counts_s: rotation.then_some(8192.),
            temperature_cutoff_c: 60,
            direction_bit: 10,
            bench_condition: "software protocol double; stationary registers".into(),
            resume_from: None,
            waveforms: None,
        };
        if waveforms {
            plan.schedule.levels.clear();
            plan.waveforms = Some(
                [true, false]
                    .into_iter()
                    .map(|keep_torque| WaveformTemplate {
                        name: if keep_torque {
                            "reversal_zero"
                        } else {
                            "reversal_off"
                        }
                        .into(),
                        segments: vec![
                            DriveSegment {
                                drive: 25,
                                duration_ms: 200,
                                torque_enabled: true,
                            },
                            DriveSegment {
                                drive: -25,
                                duration_ms: 200,
                                torque_enabled: true,
                            },
                            DriveSegment {
                                drive: 25,
                                duration_ms: 200,
                                torque_enabled: true,
                            },
                            DriveSegment {
                                drive: 0,
                                duration_ms: 200,
                                torque_enabled: keep_torque,
                            },
                        ],
                    })
                    .collect(),
            );
        }
        let result = run_sweep(&mut bus, &out, &(4..=12).collect::<Vec<_>>(), &plan).unwrap();
        (result, state, out)
    }
    #[test]
    fn free_rotation_all_nine_accepts_encoder_boundary_and_verifies_stop() {
        let (r, _, out) = run_case_rotation(false, false, false, false, true);
        assert_eq!(r["completed"], true, "{r}");
        assert_eq!(r["completed_trials"], 2);
        let mut plan: SweepPlan = serde_json::from_value(
            serde_json::from_slice::<serde_json::Value>(
                &fs::read(out.join("preflight.json")).unwrap(),
            )
            .unwrap()["plan"]
                .clone(),
        )
        .unwrap();
        plan.pulse_ms = 5000;
        assert!(plan.validate(&(4..=12).collect::<Vec<_>>()).is_ok());
        plan.free_rotation_maximum_counts_s = None;
        assert!(plan.validate(&(4..=12).collect::<Vec<_>>()).is_err());
    }
    #[test]
    fn captured_controller_commands_and_fault_cleanup_use_the_supervised_protocol() {
        use crate::controller_refinement::control::{Experiment, Knot, Policy};
        for fault in [false,true] {
            let out=std::env::temp_dir().join(format!("hx-controller-mock-{}-{fault}",std::process::id()));
            if out.exists(){fs::remove_dir_all(&out).unwrap();}fs::create_dir(&out).unwrap();
            let state=Arc::new(Mutex::new(State::new(fault)));
            let mut bus=Bus{pending:PacketBuffer::default(),raw_log:None,serial:Box::new(Mock{state:state.clone(),rx:VecDeque::new()}),log:File::create(out.join("transactions.jsonl")).unwrap(),start:Instant::now(),seq:0};
            let evidence=out.join("cadence.json");fs::write(&evidence,b"synthetic protocol-double cadence").unwrap();
            let mut e=Experiment::default();e.duration_s=0.3;e.timing.period_s=0.05;e.timing.observation_delay_ticks=0;e.timing.command_delay_ticks=0;e.timing.maximum_observation_age_s=0.1;
            e.trajectory=vec![Knot{time_s:0.,position_rad:0.},Knot{time_s:0.02,position_rad:0.1}];
            if let Policy::RustPid{parameters}=&mut e.controller{parameters.duty_limit=0.05;}
            let plan=ControllerBenchPlan{control:"closed_loop_pwm".into(),experiment:e,cadence_evidence:evidence.display().to_string(),cadence_evidence_blake3:blake3::hash(&fs::read(&evidence).unwrap()).to_hex().to_string()};
            let result=run_controller_bench(&mut bus,&out,&(4..=12).collect::<Vec<_>>(),&plan).unwrap();
            assert_eq!(result["completed"],!fault,"{result}");
            let s=state.lock().unwrap();assert!(s.latched&&s.armed==0);assert!(s.registers.iter().all(|r|r[0x28]==0&&r[0x2c]==0&&r[0x2d]==0));
            assert!(s.motion_packets>0);
            if !fault {let r:crate::controller_refinement::recording::Recording=serde_json::from_slice(&fs::read(out.join("recording.json")).unwrap()).unwrap();r.validate().unwrap();assert!(r.stop_verified);}
        }
    }
    #[test]
    fn late_settling_extends_zero_drive_observation_without_restarting_motion() {
        let (r, state, out) = run_case_options(false, false, false, true);
        assert_eq!(r["completed"], true, "{r}");
        assert_eq!(state.lock().unwrap().motion_packets, 18);
        let cp: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("checkpoint.json")).unwrap()).unwrap();
        for o in cp["outcomes_this_run"].as_array().unwrap() {
            let rest = &o["segment_events"][1];
            let duration =
                rest["poll_end_s"].as_f64().unwrap() - rest["request_s"].as_f64().unwrap();
            assert!((0.6..1.1).contains(&duration), "rest duration {duration}");
        }
    }
    #[test]
    fn concurrent_only_keeps_all_nine_leases_and_verifies_cleanup() {
        let (r, state, _) = run_case_groups(false, false, false);
        assert_eq!(r["completed"], true, "{r}");
        assert_eq!(r["completed_trials"], 2);
        let s = state.lock().unwrap();
        assert_eq!(s.motion_packets, 18);
        assert!(s.latched && s.armed == 0);
        assert!(
            s.registers
                .iter()
                .all(|r| r[0x28] == 0 && r[0x2c] == 0 && r[0x2d] == 0)
        );
    }
    #[test]
    fn complete_individual_and_concurrent_protocol_and_checkpoint() {
        let (result, state, out) = run_case(false, false);
        assert_eq!(result["completed"], true, "{result}");
        assert_eq!(result["completed_trials"], 20);
        let s = state.lock().unwrap();
        assert_eq!(s.motion_packets, 36);
        assert!(s.latched && s.armed == 0);
        assert!(
            s.registers
                .iter()
                .all(|r| r[0x28] == 0 && r[0x2c] == 0 && r[0x2d] == 0)
        );
        let cp: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("checkpoint.json")).unwrap()).unwrap();
        assert_eq!(cp["completed_trials"].as_array().unwrap().len(), 20);
        let trials = SweepSchedule {
            levels: vec![25],
            repetitions: 1,
            include_concurrent: true,
            include_individual: true,
        }
        .trials(&(4..=12).collect::<Vec<_>>())
        .unwrap();
        assert_eq!(
            completed_from_checkpoint(&cp, cp["plan_identity"].as_str().unwrap(), &trials)
                .unwrap()
                .len(),
            20
        );
        let mut incompatible = cp.clone();
        incompatible["runner_identity"] = json!("different_controller");
        assert!(
            completed_from_checkpoint(
                &incompatible,
                cp["plan_identity"].as_str().unwrap(),
                &trials
            )
            .is_err()
        );
    }
    #[test]
    fn sensor_fault_aborts_escalation_and_cleans_up_every_servo() {
        let (result, state, _) = run_case(true, false);
        assert_eq!(result["completed"], false);
        assert_eq!(result["completed_trials"], 0);
        assert!(result["failure"].as_str().unwrap().contains("current"));
        let s = state.lock().unwrap();
        assert_eq!(s.motion_packets, 1);
        assert!(s.latched && s.armed == 0);
        assert!(
            s.registers
                .iter()
                .all(|r| r[0x28] == 0 && r[0x2c] == 0 && r[0x2d] == 0)
        );
    }
    #[test]
    fn all_nine_waveforms_keep_segment_order_and_distinguish_zero_from_torque_off() {
        let (result, state, out) = run_case(false, true);
        assert_eq!(result["completed"], true, "{result}");
        assert_eq!(result["completed_trials"], 20);
        assert_eq!(state.lock().unwrap().motion_packets, 108);
        let checkpoint: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("checkpoint.json")).unwrap()).unwrap();
        for trial in checkpoint["outcomes_this_run"].as_array().unwrap() {
            let events = trial["segment_events"].as_array().unwrap();
            assert_eq!(
                events
                    .iter()
                    .map(|e| e["command"]["drive"].as_i64().unwrap())
                    .collect::<Vec<_>>(),
                vec![25, -25, 25, 0, 0]
            );
            let off = trial["trial"]["label"] == "reversal_off";
            assert_eq!(events[3]["command"]["torque_enabled"], !off);
            assert_eq!(events[4]["command"]["torque_enabled"], !off);
        }
        let csv = fs::read_to_string(out.join("sweep.csv")).unwrap();
        for trial in [18, 19] {
            for segment in 0..4 {
                let ids: std::collections::BTreeSet<u8> = csv
                    .lines()
                    .skip(1)
                    .filter_map(|line| {
                        let cells: Vec<_> = line.split(',').collect();
                        (cells[0].parse::<usize>().unwrap() == trial
                            && cells[12].parse::<usize>().unwrap() == segment)
                            .then(|| cells[1].parse().unwrap())
                    })
                    .collect();
                assert_eq!(ids, (4..=12).collect());
            }
        }
    }
}
