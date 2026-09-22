#![cfg(not(target_arch = "wasm32"))]
use sim_runtime::{
    system_session::{Command, Phase, SessionConfig},
    system_worker::{Client, Event, Launch},
};
use std::time::{Duration, Instant};
fn wait(client: &mut Client, predicate: impl Fn(&Event) -> bool) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        for event in client.poll() {
            if predicate(&event) {
                return event;
            }
            if let Event::Error { message, .. } = &event {
                panic!("unexpected worker error: {message}");
            }
        }
        assert!(Instant::now() < deadline, "worker response timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn process_builds_steps_rejects_stale_commands_and_can_be_terminated() {
    let captured: serde_json::Value = serde_json::from_str(include_str!(
        "../../../examples/systems-viewer/evidence/pre-migration-baseline.json"
    ))
    .unwrap();
    let case = captured["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "thermal")
        .unwrap();
    let bytes = serde_json::to_vec(&case["model"]).unwrap();
    let launch = Launch {
        version: 1,
        binding: None,
        run_id: "worker-smoke".into(),
        model: serde_json::from_slice(&bytes).unwrap(),
        source_hash: blake3::hash(&bytes).to_hex().to_string(),
        revision: 1,
        config: SessionConfig {
            interval: 0.01,
            integrator: sim_dynamics::Integrator::implicit_midpoint(),
            seed: 71,
        },
    };
    let mut client = Client::spawn(
        std::path::Path::new(env!("CARGO_BIN_EXE_sim-system-worker")),
        &[],
        launch,
    )
    .unwrap();
    let Event::Ready { reply } = wait(&mut client, |e| matches!(e, Event::Ready { .. })) else {
        unreachable!()
    };
    assert_eq!(reply.status.phase, Phase::Paused);
    let description = reply.description.unwrap();
    let id = client
        .command(
            0,
            Command::Subscribe {
                observables: description.observables.keys().cloned().collect(),
            },
        )
        .unwrap();
    wait(
        &mut client,
        |e| matches!(e, Event::Reply { id: got, .. } if *got == id),
    );
    let id = client.command(0, Command::Step).unwrap();
    let Event::Reply { reply, .. } = wait(
        &mut client,
        |e| matches!(e, Event::Reply { id: got, .. } if *got == id),
    ) else {
        unreachable!()
    };
    assert_eq!(reply.status.step, 1);
    assert_eq!(reply.status.time, 0.01);
    reply.frame.unwrap().validate(&description).unwrap();
    let id = client.command(0, Command::Reset).unwrap();
    let Event::Reply { reply, .. } = wait(
        &mut client,
        |e| matches!(e, Event::Reply { id: got, .. } if *got == id),
    ) else {
        unreachable!()
    };
    assert_eq!(reply.status.generation, 1);
    let id = client.command(0, Command::Step).unwrap();
    let Event::Error { message, .. } = wait(
        &mut client,
        |e| matches!(e, Event::Error { id: Some(got), .. } if *got == id),
    ) else {
        unreachable!()
    };
    assert!(message.contains("stale"));
    client.terminate().unwrap();
    wait(&mut client, |e| matches!(e, Event::Exited { .. }));
}

#[test]
fn authored_motor_connection_samples_match_headless_session_and_reset() {
    use sim_inspect::{Availability, plot, selection::SelectionTarget};
    use sim_runtime::system_session::{ModelSource, SystemSession};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/systems-viewer/spatial");
    let launch: Launch =
        serde_json::from_slice(&std::fs::read(root.join("motor-thermal.live.json")).unwrap())
            .unwrap();
    let authored: sim_inspect::SystemDescription = serde_json::from_slice(
        &std::fs::read(root.join("motor-thermal.description.json")).unwrap(),
    )
    .unwrap();
    launch.validate_binding(&sim_runtime::registry()).unwrap();
    let mut corrupt = launch.clone();
    corrupt.binding.as_mut().unwrap().model_hash = "wrong".into();
    assert!(
        corrupt
            .validate_binding(&sim_runtime::registry())
            .unwrap_err()
            .contains("hash")
    );
    corrupt = launch.clone();
    corrupt.binding.as_mut().unwrap().description_id = "wrong".into();
    assert!(
        corrupt
            .validate_binding(&sim_runtime::registry())
            .unwrap_err()
            .contains("description")
    );
    let source = ModelSource {
        model: launch.model.clone(),
        registry: sim_runtime::registry(),
        identities: launch.binding.as_ref().unwrap().identities.clone(),
        source_hash: launch.source_hash.clone(),
        revision: launch.revision,
    };
    let mut headless = SystemSession::new(launch.run_id.clone(), launch.config.clone(), move |c| {
        source.build(c)
    })
    .unwrap();
    let mut client = Client::spawn(
        std::path::Path::new(env!("CARGO_BIN_EXE_sim-system-worker")),
        &[],
        launch,
    )
    .unwrap();
    let Event::Ready { reply } = wait(&mut client, |e| matches!(e, Event::Ready { .. })) else {
        unreachable!()
    };
    let runtime = reply.description.unwrap();
    sim_inspect::live::validate_runtime_description(&authored, &runtime).unwrap();
    assert_eq!(runtime.components, authored.components);
    assert_eq!(runtime.nets, authored.nets);
    assert_eq!(runtime.id, headless.description().id);
    let choices: std::collections::BTreeSet<_> = authored
        .nets
        .keys()
        .flat_map(|net| {
            plot::options(&runtime, &SelectionTarget::net(net))
                .unwrap()
                .into_iter()
                .filter(|o| o.availability == Availability::Available)
                .map(|o| o.id.clone())
        })
        .collect();
    assert!(!choices.is_empty());
    let subscribe = Command::Subscribe {
        observables: choices.into_iter().collect(),
    };
    headless.execute(subscribe.clone()).unwrap();
    let id = client.command(0, subscribe).unwrap();
    wait(
        &mut client,
        |e| matches!(e, Event::Reply { id: got, .. } if *got == id),
    );
    let mut history = plot::History::new("motor-thermal-preview".into(), 0, 100);
    for _ in 0..20 {
        let expected = headless.execute(Command::Step).unwrap().frame.unwrap();
        let id = client.command(0, Command::Step).unwrap();
        let Event::Reply { reply, .. } = wait(
            &mut client,
            |e| matches!(e, Event::Reply { id: got, .. } if *got == id),
        ) else {
            unreachable!()
        };
        let actual = reply.frame.unwrap();
        assert_eq!(
            actual, expected,
            "worker and headless must use the same execution path"
        );
        history.push(&runtime, actual).unwrap();
    }
    assert!(history.frames().iter().any(|f| {
        f.values
            .values()
            .any(|v| matches!(v, sim_inspect::SampleValue::AcceptedStage { .. }))
    }));
    assert!(
        runtime.observables.keys().any(|id| {
            let p: Vec<_> = history.series(id).into_iter().flatten().collect();
            p.len() > 1 && p.iter().any(|x| x.value != p[0].value)
        }),
        "graphs must contain changing physics values"
    );
    // Verify the quantities used by the actual animation, including an analytic
    // motor solution. V=2, R=2, k=.1, J=.1, b=.02 -> omega=4(1-exp(-t/4)).
    let animation: sim_inspect::animation::AnimationDescription =
        serde_json::from_slice(&std::fs::read(root.join("motor-thermal.animation.json")).unwrap())
            .unwrap();
    let spatial =
        serde_json::from_slice(&std::fs::read(root.join("motor-thermal.spatial.json")).unwrap())
            .unwrap();
    animation.validate(&authored, &spatial).unwrap();
    let latest = history.frames().back().unwrap();
    for observable in animation.observables() {
        assert!(
            sim_inspect::animation::scalar(Some(latest), &observable).is_some(),
            "missing animation sample: {observable}"
        );
    }
    let angle =
        sim_inspect::animation::scalar(Some(latest), &animation.rotations[0].observable).unwrap();
    let expected_angle = 4. * (angle.time - 4. * (1. - (-angle.time / 4.).exp()));
    assert!(
        (angle.value - expected_angle).abs() < 2e-6,
        "angle {} vs analytic {}",
        angle.value,
        expected_angle
    );
    let temperature =
        sim_inspect::animation::scalar(Some(latest), &animation.colors[0].observable).unwrap();
    let t = temperature.time;
    let decay = (-t / 4.).exp();
    let expected_temperature =
        293.15 + 2.56 * (1. - decay) + 0.32 * t * decay + 0.16 * (decay - (-t / 2.).exp());
    assert!(
        (temperature.value - expected_temperature).abs() < 2e-6,
        "temperature {} vs analytic {}",
        temperature.value,
        expected_temperature
    );
    client.command(0, Command::Start).unwrap();
    let Event::Frame { frame, .. } = wait(&mut client, |e| matches!(e, Event::Frame { .. })) else {
        unreachable!()
    };
    assert!(frame.step > 20);
    let id = client.command(0, Command::Pause).unwrap();
    let Event::Reply { reply: paused, .. } = wait(
        &mut client,
        |e| matches!(e, Event::Reply { id: got, reply } if *got == id && reply.status.phase == Phase::Paused),
    ) else {
        unreachable!()
    };
    let paused_frame = paused
        .frame
        .as_ref()
        .expect("pause must publish the exact final accepted sample");
    assert_eq!(paused_frame.sequence, paused.status.sequence);
    assert_eq!(paused_frame.time, paused.status.time);
    let snapshot = sim_inspect::live::LiveSnapshot {
        version: 1,
        source_description_id: authored.id.clone(),
        description: Some(runtime.clone()),
        status: Some(paused.status.clone()),
        frame: paused.frame.clone(),
        error: None,
    };
    snapshot.validate(&authored).unwrap();
    std::thread::sleep(Duration::from_millis(70));
    let id = client.command(0, Command::Describe).unwrap();
    let Event::Reply { reply, .. } = wait(
        &mut client,
        |e| matches!(e, Event::Reply { id: got, .. } if *got == id),
    ) else {
        unreachable!()
    };
    assert_eq!(
        reply.status.time, paused.status.time,
        "pause cannot advance physics"
    );
    let id = client.command(0, Command::Reset).unwrap();
    let Event::Reply { reply, .. } = wait(
        &mut client,
        |e| matches!(e, Event::Reply { id: got, .. } if *got == id),
    ) else {
        unreachable!()
    };
    assert_eq!(reply.status.generation, 1);
    assert_eq!(reply.status.time, 0.);
    assert!(history.push(&runtime, reply.frame.unwrap()).is_err());
    client.terminate().unwrap();
}
