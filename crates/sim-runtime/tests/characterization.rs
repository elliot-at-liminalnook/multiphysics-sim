//! Characterization campaign (examples/actuators/hx30hm/hardware/
//! characterization-campaign/PLAN.md) on the simulated leg: the campaign
//! recovers a hidden truth, every safety layer stops what it should, and
//! interrupted campaigns resume from their receipts.
use sim_runtime::acquisition::{
    characterization::{self as ch, Abort, Plan, SimRig, StageResult},
    virtual_bench::{Bench, MotorModel},
};

const RAD: f64 = std::f64::consts::TAU / 4096.;

fn truth() -> [MotorModel; 3] {
    let knee = MotorModel { speed_gain: 2900., lag_s: 0.07, breakaway_duty: 0.14, moving_friction_duty: 0.07, backlash_counts: 4., compliance_counts_per_duty: 20., ..Default::default() };
    let worm = MotorModel { speed_gain: 3290., lag_s: 0.061, breakaway_duty: 0.066, moving_friction_duty: 0.045, backlash_counts: 10., compliance_counts_per_duty: 8., ..Default::default() };
    let belt = MotorModel { speed_gain: 3030., lag_s: 0.068, breakaway_duty: 0.08, moving_friction_duty: 0.06, gravity_duty: 0.03, gravity_zero_counts: 1100., backlash_counts: 6., compliance_counts_per_duty: 40., ..Default::default() };
    [knee, worm, belt]
}
fn prior() -> MotorModel {
    MotorModel { speed_gain: 5.5115660589294615 / RAD, lag_s: 0.03, breakaway_duty: 0.0085, moving_friction_duty: 0.0085, ..Default::default() }
}
fn plan() -> Plan {
    let mut p: Plan = serde_json::from_str(include_str!("../../../examples/actuators/hx30hm/hardware/characterization-campaign/plan.sim.json")).unwrap();
    p.j_max_s = 240.;
    p
}
fn rig(plan: &Plan, models: [MotorModel; 3]) -> SimRig {
    let c = |id: u8| plan.axes.iter().find(|a| a.id == id).map_or(2048., |a| a.center());
    SimRig::new(Bench::new([c(1), c(2), c(3)], models), 0.01)
}
fn clear(_: &[(u8, f64)]) -> ch::R<()> {
    Ok(())
}
fn fitted(report: &ch::Report, id: u8, name: &str) -> f64 {
    report.fitted.iter().find(|(i, _)| *i == id).unwrap().1.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("{name} not fitted on {id}")).value
}
fn aborted<'a>(report: &'a ch::Report) -> Vec<&'a StageResult> {
    report.stages.iter().filter(|s| !s.completed).collect()
}

#[test]
fn campaign_recovers_the_hidden_leg() {
    let plan = plan();
    let mut real = rig(&plan, truth());
    let report = ch::run(&plan, &mut real, Some(prior()), &clear, &mut |_| {}).unwrap();
    assert!(aborted(&report).is_empty(), "{:?}", aborted(&report).iter().map(|s| (&s.stage, s.id, &s.abort)).collect::<Vec<_>>());
    let truth = truth();
    let close = |id: u8, name: &str, want: f64, tol: f64| {
        let got = fitted(&report, id, name);
        assert!((got - want).abs() <= tol * want.abs(), "axis {id} {name}: fitted {got}, truth {want}");
    };
    for (k, m) in truth.iter().enumerate() {
        let id = k as u8 + 1;
        close(id, "speed_gain", m.speed_gain, 0.15);
        close(id, "backlash", m.backlash_counts, 0.2);
        close(id, "compliance", m.compliance_counts_per_duty, 0.2);
        // Heat capacity × thermal resistance = 108 s; fitted before the plateau.
        close(id, "thermal_time_constant", m.heat_capacity_j_k * m.thermal_resistance_k_w, 0.15);
        // Small-signal lag from the sine sweep, friction included.
        close(id, "small_signal_time_constant", m.lag_s, 0.4);
    }
    close(3, "gravity_amplitude", 0.03, 0.15);
    let zero = fitted(&report, 3, "gravity_zero");
    assert!((zero - 1100.).abs() < 64., "gravity zero {zero}");
    // Replay: the fitted model explains the recorded segments far better than the CAD prior.
    for id in 1..=3u8 {
        let before = ch::replay_error(&report.samples, id, &prior(), 0.01);
        let after = ch::replay_error(&report.samples, id, &ch::fitted_model(&prior(), &report.fitted.iter().find(|(i, _)| *i == id).unwrap().1), 0.01);
        assert!(after < 0.3 * before, "axis {id} replay {before} -> {after}");
    }
    // Test ranking and promotion exist and carry provenance.
    let ranking = ch::select_tests(&report, &plan.sensitivity);
    assert!(!ranking.is_empty());
    let coordinates = [(1u8, vec!["joint.a".to_string()]), (2, vec!["joint.b".into()]), (3, vec!["joint.c".into()])].into_iter().collect();
    let promotion = ch::promotion(&report, &serde_json::json!({}), &coordinates, "test");
    let patch = &promotion["gait_search_patch"];
    let speed = patch["governor_speed_rad_s_upper"].as_f64().unwrap();
    // 0.8 × the slowest axis's full-drive speed (knee: 2900 × (1 − 0.07) counts/s).
    let expected = 0.8 * 2900. * (1. - 0.07) * RAD;
    assert!((speed - expected).abs() < 0.2 * expected, "governor speed {speed}, expected about {expected}");
}

#[test]
fn rehearsal_divergence_stops_a_stuck_axis_and_blocks_escalation() {
    let plan = plan();
    let mut models = truth();
    // The knee seizes: nothing below 0.6 duty moves it.
    models[0].breakaway_duty = 0.6;
    models[0].moving_friction_duty = 0.55;
    let mut real = rig(&plan, models);
    let report = ch::run(&plan, &mut real, Some(truth()[0].clone()), &clear, &mut |_| {}).unwrap();
    let knee: Vec<_> = report.stages.iter().filter(|s| s.id == 1).collect();
    let first_failure = knee.iter().position(|s| !s.completed).expect("a knee stage must abort");
    // Nothing escalates on the knee after the failure (A and B may still run).
    assert!(knee[first_failure + 1..].iter().all(|s| matches!(s.stage.as_str(), "A" | "B")), "{:?}", knee.iter().map(|s| &s.stage).collect::<Vec<_>>());
    assert!(!report.stages.iter().any(|s| s.id == 1 && s.stage == "G"), "no effort ladder on a failed axis");
    // Other axes are unaffected.
    assert!(report.stages.iter().filter(|s| s.id == 2).all(|s| s.completed));
}

#[test]
fn divergence_abort_triggers_against_a_wrong_prediction() {
    let plan = plan();
    let mut real = rig(&plan, truth());
    let mut s = ch::Session::new(&mut real, plan.gates.clone(), plan.axes.clone()).unwrap();
    // Model claims the worm is 3× faster than it is.
    let wrong = MotorModel { speed_gain: 3. * 3290., ..truth()[1].clone() };
    s.model = Some(Box::new(ch::model_predictor(wrong)));
    let duties = vec![0.4; 40];
    s.rehearse(2, &duties).unwrap();
    let mut abort = None;
    for d in duties {
        if let Err(a) = s.step(2, Some(d)).unwrap() {
            abort = Some(a);
            break;
        }
    }
    assert!(matches!(abort, Some(Abort::Divergence { .. })), "{abort:?}");
}

#[test]
fn supply_sag_gate_stops_the_ladder() {
    let plan = plan();
    let mut real = rig(&plan, truth());
    // A weak supply: 1.5 Ω source resistance.
    real.bench.supply_resistance_ohm = 1.5;
    let report = ch::run(&plan, &mut real, Some(prior()), &clear, &mut |_| {}).unwrap();
    assert!(report.stages.iter().any(|s| matches!(s.abort, Some(Abort::Sag { .. }))), "{:?}", report.stages.iter().map(|s| (&s.stage, s.id, &s.abort)).collect::<Vec<_>>());
    // Every sample obeys the gate to within one control period of reaction.
    let pre = 11.8;
    assert!(report.samples.iter().all(|x| x.voltage_v > pre * 0.8));
}

#[test]
fn temperature_gate_bounds_winding_temperature() {
    let mut plan = plan();
    // The bench starts at 31 °C (warm from earlier use).
    plan.gates.max_temperature_c = 33.;
    plan.gates.cool_down_to_c = 30.;
    plan.axes.truncate(1);
    plan.axes[0] = ch::Axis { id: 2, role: "worm".into(), lower: 500., upper: 1700. };
    plan.j_duty = 0.9;
    plan.j_max_s = 900.;
    let mut real = rig(&plan, truth());
    let report = ch::run(&plan, &mut real, Some(prior()), &clear, &mut |_| {}).unwrap();
    // No sample exceeds the gate by more than a period's heating.
    let peak = report.samples.iter().map(|s| s.temperature_c).fold(0., f64::max);
    assert!(peak < 33.2, "peak {peak}");
    let j = report.stages.iter().find(|s| s.stage == "J");
    if let Some(j) = j {
        assert_eq!(j.metrics["stopped_by"], "temperature gate");
    } else {
        assert!(report.stages.iter().any(|s| matches!(s.abort, Some(Abort::Temperature { .. }))));
    }
}

#[test]
fn drift_gate_and_travel_guard() {
    let mut plan = plan();
    plan.gates.max_drift_counts = 0.;
    let mut real = rig(&plan, truth());
    let report = ch::run(&plan, &mut real, Some(prior()), &clear, &mut |_| {}).unwrap();
    assert!(report.stages.iter().any(|s| matches!(s.abort, Some(Abort::Drift { .. }))), "{:?}", report.stages.iter().map(|s| (&s.stage, s.id, &s.abort)).collect::<Vec<_>>());

    // Travel: an open-loop push through the saved pose is stopped at it.
    let plan = self::plan();
    let mut real = rig(&plan, truth());
    let mut s = ch::Session::new(&mut real, plan.gates.clone(), plan.axes.clone()).unwrap();
    let mut abort = None;
    for _ in 0..400 {
        if let Err(a) = s.step(1, Some(0.5)).unwrap() {
            abort = Some(a);
            break;
        }
    }
    assert!(matches!(abort, Some(Abort::Travel { .. })), "{abort:?}");
    let last = s.samples.last().unwrap().position;
    assert!(last < 3336. + 60., "stopped near the pose: {last}");
}

#[test]
fn collision_check_vetoes_joint_combinations() {
    let plan = plan();
    let mut real = rig(&plan, truth());
    let veto = |combo: &[(u8, f64)]| -> ch::R<()> {
        if combo.iter().any(|(id, c)| *id == 3 && *c > 1900.) { Err("hip into thigh".into()) } else { Ok(()) }
    };
    let report = ch::run(&plan, &mut real, Some(prior()), &veto, &mut |_| {}).unwrap();
    let h = report.stages.iter().find(|s| s.stage == "H").unwrap();
    assert!(matches!(&h.abort, Some(Abort::Collision { .. })), "{:?}", h.abort);
}

#[test]
fn interrupted_campaign_resumes_from_receipts() {
    let plan = plan();
    // Interrupt after 300 simulated seconds by failing the rig.
    struct Cutoff(SimRig, f64);
    impl ch::Rig for Cutoff {
        fn now(&self) -> f64 { self.0.now() }
        fn period(&self) -> f64 { self.0.period() }
        fn read(&mut self, id: u8) -> ch::R<ch::Reading> { self.0.read(id) }
        fn drive(&mut self, id: u8, duty: f64) -> ch::R<()> { self.0.drive(id, duty) }
        fn goal(&mut self, id: u8, c: f64, v: f64) -> ch::R<()> { self.0.goal(id, c, v) }
        fn set_mode(&mut self, id: u8, m: sim_runtime::acquisition::virtual_bench::ServoMode) -> ch::R<()> { self.0.set_mode(id, m) }
        fn tick(&mut self) -> ch::R<()> {
            if self.0.now() > self.1 { return Err("cancelled".into()); }
            self.0.tick()
        }
        fn stop(&mut self) -> ch::R<()> { self.0.stop() }
        fn attach_load(&mut self, id: u8, d: f64) -> ch::R<()> { self.0.attach_load(id, d) }
    }
    let mut receipts = Vec::new();
    let mut first = Cutoff(rig(&plan, truth()), 300.);
    let err = ch::run_with(&plan, &mut first, Some(Box::new(ch::model_predictor(prior()))), &clear, &mut |_| {}, &[], &mut |r| receipts.push(r.clone())).unwrap_err();
    assert_eq!(err, "cancelled");
    assert!(receipts.len() >= 3, "{} receipts", receipts.len());
    let mut log = Vec::new();
    let mut second = rig(&plan, truth());
    let report = ch::run_with(&plan, &mut second, Some(Box::new(ch::model_predictor(prior()))), &clear, &mut |m| log.push(m.to_string()), &receipts, &mut |_| {}).unwrap();
    assert_eq!(log.iter().filter(|m| m.contains("reused receipt")).count(), receipts.len());
    assert!(aborted(&report).is_empty());
    let full = ch::run(&plan, &mut rig(&plan, truth()), Some(prior()), &clear, &mut |_| {}).unwrap();
    assert!(report.simulated_s < full.simulated_s - 150., "resumed run skips finished stages: {} vs {}", report.simulated_s, full.simulated_s);
}

/// The belt/hip limits from plan.hardware.json, applied to the simulated leg.
fn belt_limits(plan: &mut Plan) {
    let hardware: Plan = serde_json::from_str(include_str!("../../../examples/actuators/hx30hm/hardware/characterization-campaign/plan.hardware.json")).unwrap();
    plan.limits = hardware.limits;
    assert!(plan.limits.contains_key("belt/hip"), "hardware plan limits the belt");
}

#[test]
fn belt_limits_hold_acceleration_on_every_stage() {
    let mut plan = plan();
    belt_limits(&mut plan);
    let cap = plan.limits["belt/hip"].max_acceleration_counts_s2.unwrap();
    let mut log = Vec::new();
    let report = ch::run(&plan, &mut rig(&plan, truth()), Some(prior()), &clear, &mut |m| log.push(m.to_string())).unwrap();
    assert!(aborted(&report).is_empty(), "{:?}", aborted(&report).iter().map(|s| (&s.stage, s.id, &s.abort)).collect::<Vec<_>>());
    assert!(log.iter().any(|m| m == "axis 3: stage F skipped (axis limits)"), "{log:?}");
    // Belt samples: 50 ms acceleration never exceeds the cap; duty never exceeds max.
    let belt: Vec<_> = report.samples.iter().filter(|x| x.id == 3).collect();
    let max_duty = plan.limits["belt/hip"].max_duty.unwrap();
    let mut peak: f64 = 0.;
    for w in belt.windows(6) {
        let dt = w[5].t - w[0].t;
        if dt > 0.04 && dt < 0.07 { peak = peak.max(((w[5].speed - w[0].speed) / dt).abs()); }
    }
    eprintln!("belt peak acceleration {peak:.0} counts/s² (cap {cap})");
    assert!(peak <= cap, "belt acceleration {peak}");
    assert!(belt.iter().filter(|x| x.duty.is_finite()).all(|x| x.duty.abs() <= 1.) && max_duty <= 0.6);
    // The other axes are not slowed by the belt's limits.
    let knee_g = report.stages.iter().find(|s| s.stage == "G" && s.id == 1).unwrap();
    assert!(knee_g.completed);
}

#[test]
fn belt_skip_spike_stops_the_test() {
    let mut plan = plan();
    belt_limits(&mut plan);
    plan.axes.retain(|a| a.id == 3);
    // A skipped tooth: the loaded motor side lurches 30 counts in one period.
    struct Skipping(SimRig, bool);
    impl ch::Rig for Skipping {
        fn now(&self) -> f64 { self.0.now() }
        fn period(&self) -> f64 { self.0.period() }
        fn read(&mut self, id: u8) -> ch::R<ch::Reading> {
            let mut r = self.0.read(id)?;
            if id == 3 && self.0.now() > 40. && !self.1 && r.speed.abs() > 50. {
                self.1 = true;
                self.0.bench.servos[2].speed += 3000. * r.speed.signum();
                r = self.0.read(id)?;
            }
            Ok(r)
        }
        fn drive(&mut self, id: u8, duty: f64) -> ch::R<()> { self.0.drive(id, duty) }
        fn goal(&mut self, id: u8, c: f64, v: f64) -> ch::R<()> { self.0.goal(id, c, v) }
        fn set_mode(&mut self, id: u8, m: sim_runtime::acquisition::virtual_bench::ServoMode) -> ch::R<()> { self.0.set_mode(id, m) }
        fn tick(&mut self) -> ch::R<()> { self.0.tick() }
        fn stop(&mut self) -> ch::R<()> { self.0.stop() }
        fn attach_load(&mut self, id: u8, d: f64) -> ch::R<()> { self.0.attach_load(id, d) }
    }
    let mut r = Skipping(rig(&plan, truth()), false);
    let report = ch::run(&plan, &mut r, Some(prior()), &clear, &mut |_| {}).unwrap();
    assert!(r.1, "the skip happened");
    let hit = report.stages.iter().find(|s| matches!(s.abort, Some(Abort::Acceleration { axis: 3, .. })));
    assert!(hit.is_some(), "{:?}", report.stages.iter().map(|s| (&s.stage, &s.abort)).collect::<Vec<_>>());
    // Nothing escalates on the belt afterwards.
    let at = report.stages.iter().position(|s| s.abort.is_some()).unwrap();
    assert!(report.stages[at + 1..].iter().all(|s| matches!(s.stage.as_str(), "A" | "B")), "{:?}", report.stages.iter().map(|s| &s.stage).collect::<Vec<_>>());
}

/// A lesson's lab step: bounded, guarded, ends stopped, reports a steady speed.
#[test]
fn lab_steps_are_bounded_guarded_and_measure_a_steady_speed() {
    let plan = plan();
    let mut real = rig(&plan, truth());
    let axis = plan.axes.iter().find(|a| a.id == 1).unwrap().clone();
    let mut session = ch::Session::new(&mut real, ch::Gates::default(), vec![axis.clone()]).unwrap();
    let step = ch::lab_step(&mut session, 1, 0.15, 1.0).unwrap();
    let steady = step.steady_counts_s.unwrap_or_else(|| panic!("no steady speed: {:?} after {} samples", step.stopped, step.samples.len()));
    // Speed ≈ gain × (duty − moving friction) once the lag has settled.
    let want = truth()[0].speed_gain * (0.15 - truth()[0].moving_friction_duty);
    assert!((steady - want).abs() < 0.15 * want, "steady {steady} vs {want}");
    assert!(step.stopped.is_none() || step.stopped.as_deref().unwrap().contains("travel"));
    assert!(ch::lab_step(&mut session, 1, 0.9, 0.5).unwrap_err().contains("outside"));
    assert!(ch::lab_step(&mut session, 1, 0.3, 30.).unwrap_err().contains("at most"));
    // A long run toward the window's end stops before the limit, not at it.
    let long = ch::lab_step(&mut session, 1, 0.5, 5.).unwrap();
    assert!(long.stopped.as_deref().is_some_and(|s| s.contains("travel")), "{:?}", long.stopped);
    let last = long.samples.last().unwrap().position;
    assert!(last < axis.upper && last > axis.lower, "stayed inside the window");
}
