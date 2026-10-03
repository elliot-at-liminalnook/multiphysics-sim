#[cfg(test)]
mod motion_tests {
    use super::*;
    #[test]
    fn pwm_pilot_is_bounded_and_progressive() {
        let mut p = PwmPlan {
            control: "pwm".into(),
            drive_counts: vec![25, 50, 100],
            direction_bit: None,
            pulse_ms: 100,
            rest_ms: 800,
            travel_limit_counts: 342,
            temperature_cutoff_c: 60,
        };
        assert!(validate_pwm_plan(&p).is_ok());
        p.drive_counts = vec![100, -100];
        assert!(validate_pwm_plan(&p).is_err());
        p.direction_bit = Some(11);
        assert!(validate_pwm_plan(&p).is_ok());
        p.drive_counts = vec![50, 25];
        assert!(validate_pwm_plan(&p).is_err());
        p.drive_counts = vec![201];
        assert!(validate_pwm_plan(&p).is_err());
        p.drive_counts = vec![100];
        p.pulse_ms = 1000;
        assert!(validate_pwm_plan(&p).is_err());
    }
    #[test]
    fn bounded_plan_rejects_unsafe_progression() {
        let stage = MotionStage {
            name: "small".into(),
            amplitude_counts: 57,
            speed_counts_s: 300,
            dwell_s: 1.,
            repetitions: 1,
        };
        let mut p = MotionPlan {
            stages: vec![stage.clone()],
            temperature_cutoff_c: 60,
            concurrent: false,
        };
        assert!(validate_plan(&p).is_ok());
        p.stages[0].amplitude_counts = 2000;
        assert!(validate_plan(&p).is_err());
        p.stages[0] = stage.clone();
        p.stages[0].dwell_s = f64::NAN;
        assert!(validate_plan(&p).is_err());
        p.stages = vec![
            stage.clone(),
            MotionStage {
                speed_counts_s: 150,
                ..stage
            },
        ];
        assert!(validate_plan(&p).is_err());
    }
    #[test]
    fn nine_servo_sync_packet_is_bounded_and_targeted() {
        let targets: Vec<_> = (4..=12).map(|id| (id, 2048)).collect();
        let p = sync_positions(&targets).unwrap();
        assert_eq!(packet(254, 0x83, &p).unwrap().len(), 35);
        assert_eq!(&p[..5], &[0x2a, 2, 4, 0, 8]);
        assert!(sync_positions(&[(4, 2048), (4, 2048)]).is_err());
        assert!(sync_positions(&[(4, 4096)]).is_err());
    }
    #[test]
    fn command_changes_only_position_time_and_speed() {
        assert_eq!(command_bytes(2048, 600), [0x2a, 0, 8, 0, 0, 0x58, 2]);
    }
    #[test]
    fn feedback_gates_faults_temperature_voltage_and_travel() {
        let mut p = [0; 15];
        p[1] = 8;
        p[6] = 105;
        p[7] = 48;
        let t = Telemetry::decode(&p).unwrap();
        assert!(check_feedback(&t, 2048, 57, 60).is_ok());
        // Use register counts at the inclusive voltage boundary: 126 * 0.1
        // can round slightly above 12.6 in binary floating point.
        for voltage in [90, 126] {
            let mut b = p;
            b[6] = voltage;
            assert!(check_feedback(&Telemetry::decode(&b).unwrap(), 2048, 57, 60).is_ok());
        }
        for (index, value) in [(7, 60), (6, 89), (6, 127), (9, 1), (1, 9)] {
            let mut b = p;
            b[index] = value;
            assert!(check_feedback(&Telemetry::decode(&b).unwrap(), 2048, 57, 60).is_err());
        }
    }
}

#[cfg(test)]
mod paired_read_tests {
    use super::*;
    use std::collections::VecDeque;
    struct PairedMock { rx: VecDeque<u8>, wrong_id: bool }
    impl Write for PairedMock {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            assert!(self.rx.is_empty(), "drain replies before next pair");
            assert!(bytes.len() <= 16, "at most two eight-byte read packets");
            for p in bytes.chunks_exact(8) {
                assert_eq!(p[4], 2, "read only");
                let id = if self.wrong_id { 99 } else { p[2] };
                self.rx.extend(packet(id, 0, &vec![p[2]; p[6] as usize]).unwrap());
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    impl Read for PairedMock {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let n = bytes.len().min(3).min(self.rx.len());
            for b in &mut bytes[..n] { *b=self.rx.pop_front().unwrap(); }
            Ok(n)
        }
    }
    #[test]
    fn paired_reads_preserve_order_fragmented_replies_and_queue_bound() {
        for wrong_id in [false,true] {
            let path=std::env::temp_dir().join(format!("paired-reads-{}-{wrong_id}.jsonl",std::process::id()));
            let mut bus=Bus{serial:Box::new(PairedMock{rx:VecDeque::new(),wrong_id}), pending:PacketBuffer::default(),raw_log:None,log:File::create(&path).unwrap(),start:Instant::now(),seq:0};
            let result=bus.read_pairs(&[10,11,12],0x28,6);
            if wrong_id { assert!(result.is_err()); } else {
                let reads=result.unwrap();assert_eq!(reads.len(),3);
                for (i,(id,begin,end,bytes)) in reads.iter().enumerate(){assert_eq!(*id,10+i as u8);assert_eq!(bytes,&vec![*id;6]);assert!(end>=begin);}
            }
            drop(bus);fs::remove_file(path).unwrap();
        }
    }
}
