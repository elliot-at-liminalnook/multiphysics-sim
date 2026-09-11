# Predictive residual controller development

The three actuator-conditioned heads predict position, velocity and finite-
interval acceleration at 20, 100 and 200 ms. They use current dynamics and the
action prefix for their own horizon. Validation is chronological within the
preserved 90 s episode; independent closed-loop acceptance remains required.
`evidence.json` records artifact hashes, data identity and aggregate errors.
The individual validation files retain errors by channel and physical unit.

`forecast-bundle.json` contains the same trained heads with named action columns
permuted to the runtime contract. `preparation.json` records the equivalence
checks. `initial-actor.json` produces exactly zero residual and uses the existing
software command envelopes. Startup transition parity and the exact full 20 s
baseline evaluation are preserved. Inputs are ideal simulation observations;
the current CAD model declares no hardware sensors.

Build the example hosts from the repository root:

```sh
cargo build --locked --release -p sim-runtime \
  --example prepare_predictive_actor --example materialize_predictive_actor \
  --example train_ppo_policy --example benchmark_environment
```

To repeat the original short training experiment, decompress
`ppo-experiment.json.gz` to a fresh file and run:

```sh
target/release/examples/train_ppo_policy experiment.json runs/fresh-ppo --gzip-rollouts
```

The compressed experiment includes the scene, original commands, initial actor,
forecast heads, seeds, optimizer and exact baseline expectation. Rollout gzip
output requires `gzip` on PATH. The source runtime identity is retained; an
incompatible physics build must fail instead of silently relabeling the model.
Twenty-second training results are not 300-second acceptance results.

Bind an actor to an existing speed, command-response or recovery recording:

```sh
target/release/examples/materialize_predictive_actor \
  input-recording.json forecast-bundle.json actor.json fresh-recording.json
target/release/examples/benchmark_environment \
  fresh-recording.json runs/fresh-evaluation 300 runs/fresh-evaluation.cancel
```

Use the recording's complete horizon for acceptance (300 s sustained speed,
90 s command response, or 20 s for the authored initial-state recovery cases).
The materializer changes only the neural residual, forecast bundle and command
saturation flag. The shared runtime validates units, channel order, features and
physical provenance. It retains robot, world, task, commands, seed and timestep.
Extract an actual trained state's `actor` for learned-policy comparisons;
`best-policy.json` may still be the untrained neutral actor if no update improves.

Runtime forecasts describe the baseline controller's proposed actuator targets
before the actor adds its correction. Better forecast error alone does not prove
better action selection, speed, command response or disturbance recovery.
