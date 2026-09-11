//! Initialize a neutral residual actor around the preserved controller, using
//! shared forecast observations and the runtime's actual typed channel order.
use serde_json::{Value, json};
use sim_core::{Channel, QuantityKind};
use sim_domain_control::{
    neural::{Feature, Layer, Network, Output},
    ppo::{GaussianExploration, GaussianSampler},
};
use sim_runtime::{
    environment::{EmbeddedEnvironment, EnvironmentRecording},
    motion_forecast::{TrajectoryForecaster, samples_from_capture},
    predictive_policy::ForecastBundle,
};
use std::{fs, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 5 {
        return Err("usage: prepare_predictive_actor preserved-input.json motion-capture.json fresh-directory training-seconds horizon-model.json ...".into());
    }
    let mut input: EnvironmentRecording = serde_json::from_slice(&fs::read(&args[0])?)?;
    let capture: Value = serde_json::from_slice(&fs::read(&args[1])?)?;
    let contract = &capture["metadata"]["policy_contract"];
    let actuators: Vec<Channel> = serde_json::from_value(contract["actuators"].clone())?;
    let bounds: Vec<[f64; 2]> =
        serde_json::from_value(contract["software_target_bounds_rad"].clone())?;
    if actuators.len() != bounds.len() || actuators.is_empty() {
        return Err("missing actuator contract".into());
    }
    let names: Vec<_> = actuators.iter().map(|a| a.name.clone()).collect();
    let end = capture["frames"]
        .as_array()
        .and_then(|f| f.last())
        .and_then(|f| f["time_s"].as_f64())
        .ok_or("missing capture end")?;
    let mut heads = vec![];
    let mut permutations = vec![];
    for path in &args[4..] {
        let original: TrajectoryForecaster = serde_json::from_slice(&fs::read(path)?)?;
        original.validate()?;
        if !original.recipe.controller_inputs.is_empty()
            || original.recipe.actuator_targets.len() != names.len()
        {
            return Err("requires actuator-conditioned model".into());
        }
        let order = names
            .iter()
            .map(|n| {
                original
                    .recipe
                    .actuator_targets
                    .iter()
                    .position(|old| old == n)
                    .ok_or("actuator model name mismatch")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut head = original.clone();
        let count = names.len();
        let offset = head.recipe.future_action_offset() - count;
        let mut columns: Vec<usize> = (0..head.network.features.len()).collect();
        for step in 0..=*head.recipe.horizons_steps.last().unwrap() {
            for (i, old) in order.iter().enumerate() {
                columns[offset + step * count + i] = offset + step * count + old;
            }
        }
        head.recipe.actuator_targets = names.clone();
        head.network.features = columns
            .iter()
            .map(|i| original.network.features[*i].clone())
            .collect();
        head.network.layers[0].weights = original.network.layers[0]
            .weights
            .iter()
            .map(|row| columns.iter().map(|i| row[*i]).collect())
            .collect();
        head.validate()?;
        // Re-extract named action labels on both sides of the permutation and
        // check predictions throughout the real capture, including its tail.
        let before = samples_from_capture(&capture, &original.recipe, 0., end)?;
        let after = samples_from_capture(&capture, &head.recipe, 0., end)?;
        if before.is_empty() || before.len() != after.len() {
            return Err("permutation requires nonempty matching sample coverage".into());
        }
        let mut maximum = 0_f64;
        let mut checked = 0;
        for i in (0..before.len())
            .step_by((before.len() / 32).max(1))
            .chain(std::iter::once(before.len() - 1))
        {
            if before[i].targets != after[i].targets || before[i].prior != after[i].prior {
                return Err("permutation changed dynamics labels/reference".into());
            }
            let a = original.predict(&before[i].inputs, &before[i].prior)?;
            let b = head.predict(&after[i].inputs, &after[i].prior)?;
            for (a, b) in a.iter().zip(&b) {
                maximum = maximum.max((a - b).abs());
                if (a - b).abs() > 1e-10 * a.abs().max(1.) {
                    return Err("permutation changed model prediction".into());
                }
            }
            checked += 1;
        }
        permutations.push(json!({"source":path,"actuator_order":names,"checked_samples":checked,"maximum_absolute_prediction_difference":maximum}));
        heads.push(head);
    }
    let bundle = ForecastBundle { version: 1, heads };
    bundle.validate()?;
    let channels = &input
        .runtime
        .scene
        .controller
        .as_ref()
        .ok_or("missing controller")?
        .inputs;
    // Keep motion commands explicit actor observations; predictive features add
    // measured dynamics, finite-interval acceleration and predicted trajectories.
    let features: Vec<_> = channels
        .iter()
        .filter(|c| c.name.starts_with("command."))
        .map(|c| Feature {
            source: c.name.clone(),
            subtract: None,
            kind: c.kind,
            center: c.initial,
            scale: (c.upper - c.lower).abs().max(1.),
            clip: 8.,
        })
        .collect();
    if features.is_empty() {
        return Err("no explicit command observations".into());
    }
    let outputs = actuators
        .iter()
        .zip(&bounds)
        .map(|(c, b)| {
            if c.kind != QuantityKind::Angle
                || !b[0].is_finite()
                || !b[1].is_finite()
                || b[1] <= b[0]
            {
                return Err("actor requires existing finite angular command envelope");
            }
            Ok(Output {
                target: c.name.clone(),
                kind: c.kind,
                scale: b[1] - b[0],
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let width = 32;
    let actor = Network {
        version: 1,
        features: features.clone(),
        outputs: outputs.clone(),
        layers: vec![
            Layer {
                weights: vec![vec![0.; features.len()]; width],
                biases: vec![0.; width],
            },
            Layer {
                weights: vec![vec![0.; width]; outputs.len()],
                biases: vec![0.; outputs.len()],
            },
        ],
    };
    let mut actor = bundle.augment_actor(&actor)?;
    let input_width = actor.features.len();
    let mut random = GaussianSampler::new(
        GaussianExploration {
            standard_deviation: vec![1. / (input_width as f64).sqrt(); input_width],
        },
        input_width,
        2303,
    )?;
    for row in &mut actor.layers[0].weights {
        *row = random.sample(&vec![0.; input_width])?.0;
    }
    actor.validate()?;
    if actor
        .normalized_output(&vec![0.; input_width], false)?
        .iter()
        .any(|v| *v != 0.)
    {
        return Err("initial actor is not neutral".into());
    }
    let policy = input
        .runtime
        .config
        .policy
        .as_mut()
        .ok_or("missing baseline policy")?;
    if policy.neural_residual.is_some() || policy.trajectory_forecast.is_some() {
        return Err("source must retain the unmodified baseline controller".into());
    }
    policy.neural_residual = Some(actor.clone());
    policy.trajectory_forecast = Some(bundle);
    policy.neural_command_saturation = true;
    policy.neural_exploration = None;
    // Bind the entire result to the real shared runtime before writing it.
    let env = EmbeddedEnvironment::new(
        input.runtime.scene.clone(),
        input.runtime.config.clone(),
        input.task.clone(),
        input.runtime.seed,
    )?;
    let training_s: f64 = args[3].parse()?;
    let intervals = training_s / input.task.period_s;
    if !training_s.is_finite()
        || training_s <= 0.
        || training_s > input.runtime.config.steps as f64 * input.runtime.config.step_s
        || (intervals - intervals.round()).abs() > 1e-8
    {
        return Err(
            "training duration must lie on the existing task grid within the original episode"
                .into(),
        );
    }
    let (_, actions) = env.prepare_replay(input.clone())?;
    let mut config = input.runtime.config.clone();
    config.steps = (training_s / config.step_s).round() as usize;
    let mut scene = input.runtime.scene.clone();
    scene.duration_s = training_s;
    let experiment = json!({"version":1,"scene":scene,"config":config,"task":input.task,
        "actions":&actions[..intervals.round() as usize],"iterations":3,"episodes_per_iteration":2,"seed":input.runtime.seed,
        "exploration":{"standard_deviation":actor.outputs.iter().map(|o|0.03/o.scale).collect::<Vec<_>>()},
        "optimizer":{"epochs":4,"batch_size":64,"actor_learning_rate":0.0001,"critic_learning_rate":0.001,
            "clip_ratio":0.2,"maximum_gradient_norm":1.,"fall_multiplier_learning_rate":1.,"shuffle_seed":2304}});
    let root = Path::new(&args[2]);
    fs::create_dir(root)?;
    let write = |name: &str, v: &Value| -> Result<(), Box<dyn std::error::Error>> {
        fs::write(root.join(name), serde_json::to_vec(v)?)?;
        Ok(())
    };
    write("input.json", &json!(input))?;
    write("actor.json", &json!(actor))?;
    write("ppo-experiment.json", &experiment)?;
    write(
        "preparation.json",
        &json!({"version":1,"source_input":args[0],"source_capture":args[1],"permutations":permutations,
        "actor_inputs":actor.features.len(),"actor_outputs":actor.outputs.len(),"runtime_metadata":env.metadata(),
        "training_duration_s":training_s,"training_horizon_override":"config.steps and scene.duration_s only; source actions truncated on the unchanged clock. Short training episodes are not sustained-speed acceptance.",
        "scope":"Neutral residual actor with motion-command and shared predictive-dynamics features. Seeded hidden weights, zero output layer. Correction scale spans existing software bounds; combined requests use the same existing actuator envelope. No robot physics, baseline Rhai controller, task objective or input schedule changed. Physical neutral-parity and learned closed-loop evaluation remain required."}),
    )?;
    Ok(())
}
