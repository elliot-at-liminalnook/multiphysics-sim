use sim_inspect::{animation::*, live::*, *};
fn fixture() -> (
    SystemDescription,
    spatial::SpatialDescription,
    AnimationDescription,
) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/systems-viewer/spatial");
    (
        serde_json::from_slice(
            &std::fs::read(root.join("motor-thermal.description.json")).unwrap(),
        )
        .unwrap(),
        serde_json::from_slice(&std::fs::read(root.join("motor-thermal.spatial.json")).unwrap())
            .unwrap(),
        serde_json::from_slice(&std::fs::read(root.join("motor-thermal.animation.json")).unwrap())
            .unwrap(),
    )
}
fn snapshot(
    source: &SystemDescription,
    a: &AnimationDescription,
    generation: u64,
    sequence: u64,
) -> LiveSnapshot {
    let mut d = source.clone();
    for o in d.observables.values_mut() {
        o.availability = Availability::Available;
    }
    d.seal().unwrap();
    let time = sequence as f64 * 0.01;
    LiveSnapshot {
        version: 1,
        source_description_id: source.id.clone(),
        frame: Some(SampleFrame {
            version: SAMPLE_FRAME_VERSION,
            description_id: d.id.clone(),
            model_revision: d.model_revision,
            run_id: "animation-test".into(),
            generation,
            sequence,
            step: sequence,
            time,
            values: a
                .observables()
                .into_iter()
                .map(|id| {
                    (
                        id,
                        SampleValue::Committed {
                            value: 1.,
                            sample_time: time,
                        },
                    )
                })
                .collect(),
        }),
        description: Some(d),
        status: Some(SessionStatus {
            phase: Phase::Paused,
            run_id: "animation-test".into(),
            generation,
            step: sequence,
            time,
            sequence,
            step_wall_seconds: 0.,
            events: 0,
            message: None,
        }),
        error: None,
    }
}
#[test]
fn bindings_require_real_parts_typed_observables_and_valid_axes_scales() {
    let (d, s, a) = fixture();
    a.validate(&d, &s).unwrap();
    let mut invalid = a.clone();
    invalid.rotations[0].observable = a.colors[0].observable.clone();
    assert!(invalid.validate(&d, &s).is_err());
    let mut invalid = a.clone();
    invalid.rotations[0].part = "imaginary".into();
    assert!(invalid.validate(&d, &s).is_err());
    let mut invalid = a.clone();
    invalid.rotations[0].axis = [0.; 3];
    assert!(invalid.validate(&d, &s).is_err());
    let mut invalid = a.clone();
    invalid.colors[0].range_kelvin = [300., 290.];
    assert!(invalid.validate(&d, &s).is_err());
    let mut invalid = a.clone();
    invalid.description_id = "foreign".into();
    assert!(invalid.validate(&d, &s).is_err());
    assert_eq!(a.colors[0].color(200.), a.colors[0].cold_srgb);
    assert_eq!(a.colors[0].color(500.), a.colors[0].hot_srgb);
}
#[test]
fn runtime_extension_accepts_added_states_but_rejects_physical_and_observation_changes() {
    let (d, _, a) = fixture();
    let mut runtime = snapshot(&d, &a, 0, 1).description.unwrap();
    let mut state = runtime.observables.values().next().unwrap().clone();
    state.id = "compiled-state".into();
    state.location = ObservationLocation::State {
        component: "example/motor-thermal/rotor".into(),
        state: "speed".into(),
    };
    runtime.observables.insert(state.id.clone(), state);
    runtime.seal().unwrap();
    validate_runtime_description(&d, &runtime).unwrap();
    let mut wrong = runtime.clone();
    wrong
        .components
        .get_mut("example/motor-thermal/motor")
        .unwrap()
        .parameters
        .get_mut("resistance")
        .unwrap()
        .value = 99.;
    wrong.seal().unwrap();
    assert!(validate_runtime_description(&d, &wrong).is_err());
    let id = &a.rotations[0].observable;
    let mut wrong = runtime.clone();
    wrong.observables.get_mut(id).unwrap().sign_convention = Some("reversed".into());
    wrong.seal().unwrap();
    assert!(validate_runtime_description(&d, &wrong).is_err());
    let mut wrong = runtime;
    wrong.observables.remove(id);
    wrong.seal().unwrap();
    assert!(validate_runtime_description(&d, &wrong).is_err());
}
#[test]
fn live_gate_holds_paused_frames_and_rejects_reset_races_and_foreign_data() {
    let (d, _, a) = fixture();
    let first = snapshot(&d, &a, 0, 1);
    let mut gate = LiveGate::default();
    gate.accept(&d, &first).unwrap();
    gate.accept(&d, &first).unwrap();
    let mut changed = first.clone();
    changed.frame.as_mut().unwrap().values.insert(
        a.rotations[0].observable.clone(),
        SampleValue::Committed {
            value: 2.,
            sample_time: 0.01,
        },
    );
    assert!(gate.accept(&d, &changed).is_err());
    let reset = snapshot(&d, &a, 1, 0);
    gate.accept(&d, &reset).unwrap();
    assert!(gate.accept(&d, &first).is_err());
    let mut foreign = snapshot(&d, &a, 1, 1);
    foreign.frame.as_mut().unwrap().run_id = "foreign".into();
    assert!(gate.accept(&d, &foreign).is_err());
    let mut restart = snapshot(&d, &a, 0, 0);
    restart.status.as_mut().unwrap().run_id = "new".into();
    restart.frame.as_mut().unwrap().run_id = "new".into();
    gate.accept(&d, &restart).unwrap();
    assert!(gate.accept(&d, &reset).is_err());
}
#[test]
fn sample_times_and_missing_values_are_not_fabricated() {
    let (d, _, a) = fixture();
    let mut snap = snapshot(&d, &a, 0, 1);
    let id = &a.rotations[0].observable;
    let f = snap.frame.as_mut().unwrap();
    f.values.insert(
        id.clone(),
        SampleValue::AcceptedStage {
            value: 3.,
            sample_time: 0.005,
            step_start: 0.,
            step_end: 0.01,
        },
    );
    assert_eq!(
        scalar(Some(f), id),
        Some(MeasuredScalar {
            value: 3.,
            time: 0.005,
            accepted_stage: true
        })
    );
    f.values.insert(
        id.clone(),
        SampleValue::Unavailable {
            reason: "no accepted sample".into(),
        },
    );
    assert_eq!(scalar(Some(f), id), None);
    f.values.clear();
    assert_eq!(scalar(Some(f), id), None);
}
#[cfg(unix)]
#[test]
fn native_transport_delivers_latest_reset_and_reports_closed_publisher() {
    use live::native::{Publisher, Subscriber};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    let (d, _, a) = fixture();
    let dir = selection::native::create_session(&d, selection::SelectionTarget::None).unwrap();
    let mut publisher = Publisher::new(dir.clone());
    let subscriber = Subscriber::new(Arc::new(d.clone()), dir.clone());
    publisher.publish(snapshot(&d, &a, 0, 1)).unwrap();
    let wait = |generation| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let r = subscriber.latest();
            if r.error.is_none()
                && r.snapshot
                    .as_ref()
                    .and_then(|s| s.status.as_ref())
                    .is_some_and(|s| s.generation == generation)
            {
                break r;
            }
            assert!(
                Instant::now() < deadline,
                "transport timeout: {:?}",
                r.error
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    let first = wait(0);
    assert_eq!(
        first
            .snapshot
            .as_ref()
            .unwrap()
            .frame
            .as_ref()
            .unwrap()
            .sequence,
        1
    );
    for seq in 2..=100 {
        publisher.publish(snapshot(&d, &a, 0, seq)).unwrap();
    }
    publisher.publish(snapshot(&d, &a, 1, 0)).unwrap();
    assert_eq!(
        wait(1)
            .snapshot
            .as_ref()
            .unwrap()
            .frame
            .as_ref()
            .unwrap()
            .time,
        0.
    );
    drop(publisher);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !subscriber
        .latest()
        .error
        .is_some_and(|e| e.contains("disconnected"))
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(subscriber);
    std::thread::sleep(Duration::from_millis(100));
    std::fs::remove_dir_all(dir).unwrap();
}
