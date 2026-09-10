use serde_json::{Value, json};
use sim_runtime::{
    environment::{Task, TerminationBound},
    experiment::*,
};

fn spec() -> ExperimentSpec {
    let robot: Value = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/baseline/robot.simrobot.json"
    ))
    .unwrap();
    let inputs: Value = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/velocity-controller.inputs.json"
    ))
    .unwrap();
    let scene=serde_json::from_value(json!({"version":1,"robot":robot,"options":{"contact":false,"flex":false},
        "period_s":0.003,"duration_s":0.03,"controller":{"inputs":inputs,
        "parameters":{"period_s":0.003,"initial_left":0.01,"initial_right":-0.02},
        "sources":{"entry":"velocity-controller.rhai","files":{"velocity-controller.rhai":include_str!("../../../examples/wheeled-robot/velocity-controller.rhai")}}}})).unwrap();
    let config = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/imu-policy.config.json"
    ))
    .unwrap();
    let mut task: Task = serde_json::from_str(include_str!(
        "../../../examples/wheeled-robot/imu-policy.task.json"
    ))
    .unwrap();
    task.rewards.clear();
    task.progress = Some(serde_json::from_value(json!({"link":"chassis","axes":"xy"})).unwrap());
    let parameterization=serde_json::from_value(json!({"version":1,"space":{"parameters":[
        {"name":"turn","kind":"AngularVelocity","bounds":[-0.5,0.5]}]},"trajectories":[],"commands":[
        {"input":"command.right_speed","kind":"AngularVelocity","scale":{"source":"constant","value":1},
        "center":{"source":"constant","value":0},"offset":{"source":"parameter","name":"turn"}}]})).unwrap();
    ExperimentSpec {
        version: 1,
        scene,
        config,
        task,
        parameterization,
        source_actions: vec![vec![0.1, 0.1]; 10],
        baseline: [("turn".into(), 0.)].into(),
        seed: 7,
        objective: Objective::NetSpeed,
    }
}
fn proposal(e: &Experiment) -> Proposal {
    e.propose(e.spec.baseline.clone(), "test".into()).unwrap()
}

#[test]
fn json_integer_and_float_spelling_preserves_context_and_checkpoint_replay() {
    fn normalize(value: &mut Value) {
        match value {
            Value::Array(a) => a.iter_mut().for_each(normalize),
            Value::Object(o) => o.values_mut().for_each(normalize),
            Value::Number(n) => {
                if let Some(v) = n.as_f64() {
                    if v.fract() == 0. && v.abs() <= 9007199254740991. {
                        *value = json!(v as i64);
                    }
                }
            }
            _ => {}
        }
    }
    let mut s = spec();
    s.scene.controller.as_mut().unwrap().parameters["formatting_probe"] = json!(-0.0);
    let experiment = Experiment::bind(s).unwrap();
    let mut serialized = serde_json::to_value(&experiment).unwrap();
    normalize(&mut serialized);
    let compatible: Experiment = serde_json::from_value(serialized).unwrap();
    compatible.validate().unwrap();
    let mut evaluation = experiment.start(proposal(&experiment)).unwrap();
    evaluation.advance(3).unwrap();
    let mut serialized = serde_json::to_value(evaluation.checkpoint().unwrap()).unwrap();
    normalize(&mut serialized);
    let checkpoint = serde_json::from_value(serialized).unwrap();
    let mut resumed = compatible.resume(checkpoint).unwrap();
    resumed.advance(100).unwrap();
    evaluation.advance(100).unwrap();
    assert_eq!(
        resumed.checkpoint().unwrap().final_transition,
        evaluation.checkpoint().unwrap().final_transition
    );
}

#[test]
fn interruption_json_checkpoint_and_bounded_replay_match_uninterrupted_state() {
    let experiment = Experiment::bind(spec()).unwrap();
    let p = proposal(&experiment);
    let mut full = experiment.start(p.clone()).unwrap();
    full.advance(100).unwrap();
    let expected = full.checkpoint().unwrap();
    assert_eq!(expected.status(), Status::Complete);
    assert!(expected.score(Objective::NetSpeed).unwrap().is_some());
    let mut partial = experiment.start(p.clone()).unwrap();
    partial.advance(4).unwrap();
    let checkpoint = partial.checkpoint().unwrap();
    assert_eq!(checkpoint.status(), Status::Running);
    assert_eq!(checkpoint.score(Objective::NetSpeed).unwrap(), None);
    let mut journal = Journal::new(experiment);
    journal.submit(p).unwrap();
    journal.record(checkpoint.clone()).unwrap();
    assert!(journal.best().unwrap().is_none());
    let mut restored: Journal =
        serde_json::from_slice(&serde_json::to_vec(&journal).unwrap()).unwrap();
    restored.validate().unwrap();
    let saved = restored.pending().unwrap().checkpoint.clone().unwrap();
    let mut resumed = restored.experiment.resume(saved).unwrap();
    assert_eq!(resumed.advance(2).unwrap(), Status::Replaying);
    assert!(
        serde_json::to_value(resumed.checkpoint().unwrap()).unwrap()
            == serde_json::to_value(checkpoint).unwrap()
    );
    assert_eq!(resumed.advance(2).unwrap(), Status::Running);
    resumed.advance(100).unwrap();
    let got = resumed.checkpoint().unwrap();
    assert!(serde_json::to_value(&got).unwrap() == serde_json::to_value(expected).unwrap());
    restored.record(got).unwrap();
    assert!(restored.pending().is_none());
    assert!(restored.best().unwrap().is_some());
}

#[test]
fn task_termination_and_controller_failure_never_produce_eligible_speed() {
    let mut s = spec();
    s.task.termination_bounds.push(TerminationBound {
        observation: "right.reference".into(),
        lower: -1.,
        upper: -0.03,
    });
    let experiment = Experiment::bind(s).unwrap();
    let mut evaluation = experiment.start(proposal(&experiment)).unwrap();
    assert_eq!(evaluation.advance(100).unwrap(), Status::Terminated);
    let c = evaluation.checkpoint().unwrap();
    assert_eq!(c.score(Objective::NetSpeed).unwrap(), None);
    let mut s = spec();
    let source = s
        .scene
        .controller
        .as_mut()
        .unwrap()
        .sources
        .files
        .get_mut("velocity-controller.rhai")
        .unwrap();
    *source = source.replace(
        "let p = parameters();",
        "if t >= 0.006 { throw \"injected failure\"; } let p = parameters();",
    );
    let experiment = Experiment::bind(s).unwrap();
    let mut evaluation = experiment.start(proposal(&experiment)).unwrap();
    assert_eq!(evaluation.advance(100).unwrap(), Status::Failed);
    let c = evaluation.checkpoint().unwrap();
    assert_eq!(c.score(Objective::NetSpeed).unwrap(), None);
    assert!(experiment.resume(c).is_err());
}

#[test]
fn changed_context_commands_and_saved_state_are_rejected_without_new_motion() {
    let experiment = Experiment::bind(spec()).unwrap();
    let mut evaluation = experiment.start(proposal(&experiment)).unwrap();
    evaluation.advance(3).unwrap();
    let checkpoint = evaluation.checkpoint().unwrap();
    let mut changed = experiment.clone();
    changed.spec.seed += 1;
    assert!(changed.validate().is_err());
    let mut changed = experiment.clone();
    changed.spec.config.step_s *= 0.5;
    assert!(changed.validate().is_err());
    let mut changed = experiment.clone();
    changed.runtime.library_source_blake3 = "0".repeat(64);
    assert!(changed.validate().is_err());
    let mut bad = checkpoint.clone();
    bad.recording.runtime.input_events[0].values[0] += 0.01;
    assert!(experiment.resume(bad).is_err());
    let mut bad = checkpoint;
    let value = bad.frame["joint_positions"][0].as_f64().unwrap();
    bad.frame["joint_positions"][0] = json!(value + 0.01);
    let mut replay = experiment.resume(bad).unwrap();
    assert!(replay.advance(100).is_err());
    assert!(replay.advance(1).is_err());
    assert_eq!(replay.status(), Status::Replaying);
}

#[test]
fn journal_rejects_duplicates_terminal_replacement_and_progress_regression() {
    let experiment = Experiment::bind(spec()).unwrap();
    let p = proposal(&experiment);
    let mut j = Journal::new(experiment.clone());
    j.submit(p.clone()).unwrap();
    assert!(j.submit(p.clone()).is_err());
    let mut e = experiment.start(p.clone()).unwrap();
    e.advance(1).unwrap();
    let earlier = e.checkpoint().unwrap();
    e.advance(1).unwrap();
    j.record(e.checkpoint().unwrap()).unwrap();
    assert!(j.record(earlier).is_err());
    e.advance(100).unwrap();
    let final_state = e.checkpoint().unwrap();
    j.record(final_state.clone()).unwrap();
    assert!(j.record(final_state).is_err());
    let mut values = experiment.spec.baseline.clone();
    values.insert("turn".into(), 0.5);
    let p = experiment
        .propose(values, "external selector".into())
        .unwrap();
    j.submit(p.clone()).unwrap();
    j.preparation_failed(&p.id, "declared candidate could not be prepared".into())
        .unwrap();
    assert!(j.pending().is_none());
    assert!(j.best().unwrap().is_some());
    let mut bad = spec();
    bad.source_actions.pop();
    assert!(Experiment::bind(bad).is_err());
    let mut bad = spec();
    bad.task.progress = None;
    assert!(Experiment::bind(bad).is_err());
}

#[cfg(all(feature = "bayesian", not(target_arch = "wasm32")))]
#[test]
fn bayesian_proposals_reconstruct_from_the_same_completed_journal() {
    use sim_runtime::experiment_search::{Settings, ask};
    let mut s = spec();
    s.task.progress = None;
    // An explicit joint-position task creates a measurable response for
    // this short selector test; net-speed behavior is tested separately above.
    s.task.rewards=serde_json::from_value(json!([{"name":"tracking","observation":"right","target":{"kind":"constant","value":-0.016},"scale":1,"weight_per_s":1}])).unwrap();
    s.objective = Objective::RewardRate;
    let experiment = Experiment::bind(s).unwrap();
    let mut journal = Journal::new(experiment);
    let settings = Settings {
        seed: 19,
        initial_design: 2,
        acquisition_starts: 2,
        maximum_training_rows: 16,
    };
    for _ in 0..3 {
        let p = ask(&journal, &settings).unwrap();
        journal.submit(p.clone()).unwrap();
        assert!(ask(&journal, &settings).is_err());
        let mut e = journal.experiment.start(p).unwrap();
        e.advance(100).unwrap();
        journal.record(e.checkpoint().unwrap()).unwrap();
    }
    let restored: Journal = serde_json::from_slice(&serde_json::to_vec(&journal).unwrap()).unwrap();
    let a = ask(&journal, &settings).unwrap();
    let b = ask(&restored, &settings).unwrap();
    assert_eq!(a, b);
    assert!(a.method.ends_with(":22"));
    assert!(!journal.trials.iter().any(|t| t.proposal.values == a.values));
}
