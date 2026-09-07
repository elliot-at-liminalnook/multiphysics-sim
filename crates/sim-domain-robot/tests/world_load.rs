use sim_domain_robot::world_load::{WorldLoadPulse, WorldLoadSchedule};

fn schedule() -> WorldLoadSchedule {
    WorldLoadSchedule {
        version: 1,
        base_link: "body".into(),
        provenance: "analytic test".into(),
        maximum_force_n: 5.0,
        maximum_moment_nm: 2.0,
        pulses: vec![WorldLoadPulse {
            name: "push".into(),
            start_s: 0.02,
            duration_s: 0.04,
            force_world_n: [3.0, 4.0, 0.0],
            moment_world_nm: [0.0, 0.0, 2.0],
        }],
    }
}

#[test]
fn bounded_schedule_integrates_impulse_and_selects_named_base() {
    for h in [0.02, 0.01, 0.005] {
        let n = (0.1 / h) as usize;
        let bound = schedule()
            .bind(&["other".into(), "body".into()], h, n)
            .unwrap();
        let mut integral = [0.0; 6];
        for i in 0..n {
            let mut loads = vec![0.0; 13];
            bound.add_to(i, &mut loads).unwrap();
            assert_eq!(&loads[..6], &[0.0; 6]);
            assert_eq!(loads[12], 0.0);
            for j in 0..6 {
                integral[j] += h * loads[6 + j];
            }
        }
        for (a, b) in integral.into_iter().zip([0.12, 0.16, 0.0, 0.0, 0.0, 0.08]) {
            assert!((a - b).abs() < 1e-14);
        }
        assert_eq!(bound.wrench(n), [0.0; 6]);
        assert!(bound.add_to(1, &mut [0.0; 6]).is_err());
    }
}

#[test]
fn schedules_reject_bad_timing_names_and_combined_bounds() {
    let bind = |s: WorldLoadSchedule| s.bind(&["body".into()], 0.01, 10);
    let s = schedule();
    assert!(s.bind(&["other".into()], 0.01, 10).is_err());
    for mode in 0..8 {
        let mut bad = s.clone();
        match mode {
            0 => bad.pulses[0].start_s = 0.025,
            1 => bad.pulses[0].duration_s = 0.1,
            2 => bad.provenance.clear(),
            3 => bad.pulses[0].force_world_n[0] = f64::NAN,
            4 => bad.maximum_force_n = 4.9,
            5 => bad.maximum_moment_nm = 1.9,
            6 => bad.pulses[0].duration_s = 0.0,
            _ => bad.pulses.push(bad.pulses[0].clone()),
        }
        assert!(bind(bad).is_err(), "case {mode}");
    }
    let mut overlap = s.clone();
    let mut p = overlap.pulses[0].clone();
    p.name = "second".into();
    p.start_s = 0.04;
    overlap.pulses.push(p);
    assert!(bind(overlap.clone()).err().unwrap().contains("combined"));
    overlap.pulses[1].start_s = 0.06;
    assert!(bind(overlap).is_ok());
}
