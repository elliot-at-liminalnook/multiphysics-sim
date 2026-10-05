use sim_core::{BehaviorRegistry, ModelWorld};
use sim_dynamics::Integrator;
use sim_inspect::{FrameGate, ObservationLocation, runtime::FrameStamp};
use sim_runtime::system_session::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
fn config() -> SessionConfig {
    SessionConfig {
        interval: 0.1,
        integrator: Integrator::implicit_midpoint(),
        seed: 71,
        grid_snapping: false,
    }
}
fn source() -> ModelSource {
    let mut registry = BehaviorRegistry::default();
    sim_domain_thermal::register(&mut registry).unwrap();
    let mut model = ModelWorld::default();
    let tank = model
        .part(
            &registry,
            "Storage",
            sim_domain_thermal::CAPACITANCE,
            [("heat_capacity", 2.), ("initial.temperature", 313.15)],
        )
        .unwrap();
    let path = model
        .part(
            &registry,
            "Conduction",
            sim_domain_thermal::CONDUCTANCE,
            [("conductance", 0.5)],
        )
        .unwrap();
    let ambient = model
        .part(
            &registry,
            "Ambient",
            sim_domain_thermal::AMBIENT,
            [("temperature", 293.15)],
        )
        .unwrap();
    model.connect([tank.port("node"), path.port("a")]);
    model.connect([path.port("b"), ambient.port("node")]);
    ModelSource {
        model,
        registry,
        identities: Default::default(),
        source_hash: "thermal-session-fixture".into(),
        revision: 1, base: None
    }
}
fn session() -> SystemSession {
    let source = source();
    SystemSession::new("test-run".into(), config(), move |c| source.build(c)).unwrap()
}
fn ids(s: &SystemSession) -> Vec<String> {
    s.description().observables.keys().cloned().collect()
}
#[test]
fn headless_and_session_intervals_match_and_pause_is_a_noop() {
    let mut s = session();
    let selected = ids(&s);
    s.execute(Command::Subscribe {
        observables: selected.clone(),
    })
    .unwrap();
    let mut direct = source().build(&config()).unwrap();
    direct.runtime.seed(71);
    direct.runtime.set_observation_capture(true);
    let subscription = direct
        .inspection
        .subscribe(selected.iter().map(String::as_str))
        .unwrap();
    let initial = s.latest().clone();
    for _ in 0..20 {
        assert!(s.tick().unwrap().is_none());
    }
    assert_eq!(*s.latest(), initial);
    s.execute(Command::Start).unwrap();
    assert!(s.execute(Command::Step).is_err());
    for step in 1..=20 {
        s.tick().unwrap().unwrap();
        direct.runtime.advance(0.1, 0.1).unwrap();
        let frame = subscription
            .sample(
                &direct.runtime,
                FrameStamp {
                    run_id: "direct",
                    generation: 0,
                    sequence: step,
                    step,
                },
            )
            .unwrap();
        assert_eq!(s.latest().values, frame.values);
        assert_eq!(s.status().time, frame.time);
        assert_eq!(s.status().step, step);
    }
    s.execute(Command::Pause).unwrap();
    let paused = s.latest().clone();
    assert!(s.tick().unwrap().is_none());
    assert_eq!(*s.latest(), paused);
    s.execute(Command::Step).unwrap();
    assert_eq!(s.status().phase, Phase::Paused);
    assert_eq!(s.status().step, 21);
}
#[test]
fn recording_is_independent_of_display_and_stops_before_overflow() {
    let mut s = session();
    let all = ids(&s);
    let flows: Vec<_> = s
        .description()
        .observables
        .iter()
        .filter(|(_, d)| matches!(d.location, ObservationLocation::Through { .. }))
        .map(|(id, _)| id.clone())
        .collect();
    s.execute(Command::BeginRecording {
        observables: flows.clone(),
        capacity: 3,
    })
    .unwrap();
    s.execute(Command::Subscribe { observables: all }).unwrap();
    for _ in 0..2 {
        s.execute(Command::Step).unwrap();
    }
    assert_eq!(s.recording_len(), 3);
    let last = s.latest().clone();
    assert!(s.execute(Command::Step).unwrap_err().contains("capacity"));
    assert_eq!(s.status().phase, Phase::RecordingFull);
    assert_eq!(*s.latest(), last);
    assert_eq!(s.status().step, 2);
    assert!(s.tick().unwrap().is_none());
    let recording = s
        .execute(Command::TakeRecording)
        .unwrap()
        .recording
        .unwrap();
    assert_eq!(s.status().phase, Phase::Paused);
    assert_eq!(s.recording_len(), 0);
    recording.validate().unwrap();
    assert_eq!(recording.observables, flows.into_iter().collect());
    assert!(recording.at_or_before(-1.).is_none());
    assert!(recording.at_or_before(f64::NAN).is_none());
    assert_eq!(recording.at_or_before(0.15).unwrap().time, 0.1);
    assert_eq!(recording.at_or_before(1.).unwrap().time, 0.2);
    let mut replay: Recording =
        serde_json::from_slice(&serde_json::to_vec(&recording).unwrap()).unwrap();
    replay.validate().unwrap();
    assert_eq!(
        replay.at_or_before(0.2).unwrap().values,
        recording.frames[2].values
    );
    replay.frames[1].generation += 1;
    assert!(replay.validate().is_err());
    s.execute(Command::Step).unwrap();
    assert_eq!(s.status().step, 3);
}
#[test]
fn reset_rebuilds_and_returns_recording_without_reusing_a_generation() {
    let builds = Arc::new(AtomicUsize::new(0));
    let factory_builds = builds.clone();
    let source = source();
    let mut s = SystemSession::new("reset-test".into(), config(), move |c| {
        factory_builds.fetch_add(1, Ordering::Relaxed);
        source.build(c)
    })
    .unwrap();
    let all = ids(&s);
    s.execute(Command::Subscribe {
        observables: all.clone(),
    })
    .unwrap();
    let initial_values = s.latest().values.clone();
    s.execute(Command::BeginRecording {
        observables: all,
        capacity: 10,
    })
    .unwrap();
    s.execute(Command::Step).unwrap();
    let old = s.latest().clone();
    let reply = s.execute(Command::Reset).unwrap();
    assert_eq!(builds.load(Ordering::Relaxed), 2);
    assert_eq!(s.status().phase, Phase::Paused);
    assert_eq!(
        (s.status().step, s.status().generation, s.status().time),
        (0, 1, 0.)
    );
    assert_eq!(s.latest().values, initial_values);
    let recording = reply.recording.unwrap();
    recording.validate().unwrap();
    assert_eq!(recording.frames.len(), 2);
    assert_eq!(recording.end_status.phase, Phase::Cancelled);
    let mut gate = FrameGate::new("reset-test".into(), 1);
    assert!(gate.accept(s.description(), &old).is_err());
    gate.accept(s.description(), s.latest()).unwrap();
}
#[test]
fn failed_reset_and_invalid_commands_preserve_user_recording_and_values() {
    let source = source();
    let mut builds = 0;
    let mut s = SystemSession::new("reset-failure".into(), config(), move |c| {
        builds += 1;
        if builds > 1 {
            return Err("factory unavailable".into());
        }
        source.build(c)
    })
    .unwrap();
    let all = ids(&s);
    s.execute(Command::BeginRecording {
        observables: all.clone(),
        capacity: 10,
    })
    .unwrap();
    s.execute(Command::Subscribe { observables: all }).unwrap();
    s.execute(Command::Step).unwrap();
    let status = s.status().clone();
    let frame = s.latest().clone();
    assert!(s.execute(Command::Reset).is_err());
    assert!(
        s.execute(Command::Subscribe {
            observables: vec!["unknown".into()]
        })
        .is_err()
    );
    assert_eq!(*s.status(), status);
    assert_eq!(*s.latest(), frame);
    assert_eq!(s.recording_len(), 2);
    s.execute(Command::Cancel).unwrap();
    assert!(s.execute(Command::Start).is_err());
    assert!(s.tick().unwrap().is_none());
    s.execute(Command::TakeRecording)
        .unwrap()
        .recording
        .unwrap()
        .validate()
        .unwrap();
}
#[test]
fn failed_solve_keeps_last_completed_values_and_reset_recreates_behavior_state() {
    use sim_core::{
        Behavior, BehaviorDescriptor, Context, QuantityKind, StateDeclaration, signal_out,
    };
    // A residual that turns non-finite after 0.11 s: the solve fails.
    struct Failing;
    impl Behavior for Failing {
        fn states(&self) -> Vec<StateDeclaration> {
            vec![StateDeclaration::new("x", QuantityKind::Voltage, 1.)]
        }
        fn residual(&self, c: &mut Context) {
            let poison = if c.time > 0.11 { f64::NAN } else { 0.0 };
            c.set_state_residual(0, c.state_rate(0) + c.state(0) + poison);
            c.set_signal(0, c.state(0));
        }
    }
    let mut registry = BehaviorRegistry::default();
    registry
        .register(BehaviorDescriptor::new(
            "failing",
            "Failing",
            vec![signal_out("voltage", QuantityKind::Voltage)],
            |_| Ok(Box::new(Failing)),
        ))
        .unwrap();
    let mut model = ModelWorld::default();
    model.part(&registry, "failing", "failing", []).unwrap();
    let source = ModelSource {
        model,
        registry,
        identities: Default::default(),
        source_hash: "failure".into(),
        revision: 1, base: None
    };
    let mut s = SystemSession::new("failure".into(), config(), move |c| source.build(c)).unwrap();
    let all = ids(&s);
    s.execute(Command::Subscribe {
        observables: all.clone(),
    })
    .unwrap();
    s.execute(Command::BeginRecording {
        observables: all,
        capacity: 10,
    })
    .unwrap();
    s.execute(Command::Step).unwrap();
    let last = s.latest().clone();
    assert!(s.execute(Command::Step).is_err());
    assert_eq!(s.status().phase, Phase::Failed);
    assert_eq!((s.status().step, s.status().time), (1, 0.1));
    assert_eq!(*s.latest(), last);
    let recorded = s
        .execute(Command::TakeRecording)
        .unwrap()
        .recording
        .unwrap();
    recorded.validate().unwrap();
    assert_eq!(recorded.end_status.phase, Phase::Failed);
    assert_eq!(recorded.frames.len(), 2);
    s.execute(Command::Reset).unwrap();
    s.execute(Command::Step).unwrap();
    assert_eq!(s.status().step, 1);
    s.execute(Command::BeginRecording {
        observables: ids(&s),
        capacity: 10,
    })
    .unwrap();
    assert!(s.execute(Command::Step).is_err());
    let failed = s.execute(Command::Reset).unwrap().recording.unwrap();
    failed.validate().unwrap();
    assert_eq!(failed.completion, RecordingCompletion::Failed);
    assert!(failed.end_status.message.is_some_and(|m| !m.is_empty()), "the failure is reported");
}
#[test]
fn invalid_intervals_are_rejected_before_building() {
    for interval in [0., -1., f64::NAN, f64::INFINITY] {
        let mut c = config();
        c.interval = interval;
        assert!(SystemSession::new("bad".into(), c, |_| panic!("must not build")).is_err());
    }
}

#[test]
fn draining_event_history_preserves_hybrid_states_and_cumulative_counts() {
    use sim_core::{
        Behavior, BehaviorDescriptor, Context, QuantityKind, StateDeclaration, View, signal_out,
    };
    struct Pulse;
    impl Behavior for Pulse {
        fn states(&self) -> Vec<StateDeclaration> {
            vec![
                StateDeclaration::new("clock", QuantityKind::Time, 0.05),
                StateDeclaration::new("count", QuantityKind::Dimensionless, 0.),
            ]
        }
        fn residual(&self, c: &mut Context) {
            c.set_state_residual(0, c.state_rate(0));
            c.set_state_residual(1, c.state_rate(1));
            c.set_signal(0, c.state(1));
        }
        fn guards(&self, v: &View, out: &mut Vec<f64>) {
            out.push(v.state(0) - v.time);
        }
        fn scheduled_events(&self, v: &View, out: &mut Vec<(usize, f64)>) {
            out.push((0, v.state(0)));
        }
        fn jump(&mut self, _: usize, _: &View, states: &mut [f64]) {
            states[0] += 0.05;
            states[1] += 1.;
        }
    }
    let mut registry = BehaviorRegistry::default();
    registry
        .register(BehaviorDescriptor::new(
            "pulse",
            "Pulse",
            vec![signal_out("count", QuantityKind::Dimensionless)],
            |_| Ok(Box::new(Pulse)),
        ))
        .unwrap();
    let mut model = ModelWorld::default();
    model.part(&registry, "pulse", "pulse", []).unwrap();
    let source = ModelSource {
        model,
        registry,
        identities: Default::default(),
        source_hash: "pulse".into(),
        revision: 1, base: None
    };
    let mut direct = source.build(&config()).unwrap();
    direct.runtime.seed(71);
    direct.runtime.set_observation_capture(true);
    let mut s = SystemSession::new("pulse".into(), config(), move |c| source.build(c)).unwrap();
    let all = ids(&s);
    let subscribed = direct
        .inspection
        .subscribe(all.iter().map(String::as_str))
        .unwrap();
    s.execute(Command::Subscribe { observables: all }).unwrap();
    for step in 1..=100 {
        s.execute(Command::Step).unwrap();
        direct.runtime.advance(0.1, 0.1).unwrap();
        let frame = subscribed
            .sample(
                &direct.runtime,
                FrameStamp {
                    run_id: "direct",
                    generation: 0,
                    sequence: step,
                    step,
                },
            )
            .unwrap();
        assert_eq!(s.latest().values, frame.values);
        assert_eq!(s.status().events, direct.runtime.events() as u64);
    }
    assert!(s.status().events >= 199);
    assert_eq!(s.recording_len(), 0);
}

#[test]
fn recording_retains_effective_multirate_settings_and_rejects_false_completion() {
    let source = source();
    let behavior = source.model.behaviors.keys().next().unwrap();
    let mut s = SystemSession::new("multirate".into(), config(), move |c| {
        let mut built = source.build(c)?;
        built.runtime.set_island_step(behavior, Some(0.025));
        Ok(built)
    })
    .unwrap();
    s.execute(Command::BeginRecording {
        observables: ids(&s),
        capacity: 3,
    })
    .unwrap();
    s.execute(Command::Step).unwrap();
    let mut record = s
        .execute(Command::TakeRecording)
        .unwrap()
        .recording
        .unwrap();
    record.validate().unwrap();
    assert_eq!(record.islands.len(), 1);
    assert_eq!(record.islands[0].step_size, 0.025);
    assert_eq!(record.config.interval, 0.1);
    record.completion = RecordingCompletion::Failed;
    assert!(record.validate().is_err());
}
