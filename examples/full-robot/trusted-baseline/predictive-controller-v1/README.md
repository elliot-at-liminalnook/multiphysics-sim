# Predictive residual controller development

The three actuator-conditioned heads predict position, velocity and finite-
interval acceleration at 20, 100 and 200 ms. They use current dynamics and the
action prefix for their own horizon. Validation is chronological within the
preserved 90 s episode. The completed closed-loop comparisons below do not
establish a sustained-speed improvement.
`evidence.json` records artifact hashes, data identity and aggregate errors.
The individual validation files retain errors by channel and physical unit.

`forecast-bundle.json` contains the same trained heads with named action columns
permuted to the runtime contract. `preparation.json` records the equivalence
checks. `initial-actor.json` produces exactly zero residual and uses the existing
software command envelopes. Startup transition parity and the exact full 20 s
baseline evaluation are preserved. Inputs are ideal simulation observations;
the current CAD model declares no hardware sensors.

The trained artifacts bind runtime source hash
`56acc2aeca9730e9448ea56748a3c5461f0a86317d0531e555890508b1bf4cc1`.
Git revision `81304004` contains that source and these example hosts. Preserve
that revision for reproduction if later library changes alter the physics
identity; never edit the models' identity to bypass a mismatch.

Build the example hosts from a matching repository revision:

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

## Completed training and initial-state recovery

`training-result.json` records the completed three-update PPO run and hashes of
all six local compressed rollouts. `final-state.json` retains actor, critic and
optimizer continuation state; `best-policy.json` is the final short-training
winner (0.566927 m/s over 20 s). The compressed experiment and seeds allow the
rollouts to be regenerated; the large rollout files themselves remain in `runs/`.

The earlier `candidate-iteration001.actor.json` was frozen before held-out
evaluation and remains the subject of the matched comparison. Both authored
initial-state recovery cases complete without sampled falls, with approximately
0.23% higher speed than the original gait. `recovery-comparison.json` retains
both sides, source receipts and sampled clearance diagnostics. Its first 20 s
in the long run exactly match its training evaluation. These initial-state cases
do not establish recovery from timed pushes during walking.

`command-comparison.json` records the complete 90 s WASD comparison with the
same analyzer on both policies. All 4,500 commanded actions are verified and
neither policy has a sampled fall. The candidate is slightly faster, but its
first forward stage changes heading by 11.32 degrees versus 3.08 degrees for the
baseline. Braking distance is similar; reversal still produces a large turn.
`command-transitions.jsonl.gz` retains the full candidate observation/action
stream and its receipt records the decompressed hash. Short speed gains do not
establish improved command control. No heading or slip penalty was added.

The full 300 s comparison is in `sustained-comparison.json`. The candidate
completes without a sampled fall at **0.432011 m/s**, versus **0.573564 m/s** for
the preserved gait, a **24.7% reduction**. Sampled chord-path lengths are
177.572 m and 176.751 m, but net displacements are 129.603 m and 172.069 m.
This is consistent with increased path curvature rather than less sampled
motion. `sustained-paths.json.gz` preserves both paths at 20 ms intervals; these
are not continuous-time path lengths. The candidate is rejected as a speed
improvement. The later short-training winner has no corresponding 300 s result.
Future optimization must address the long-horizon net-displacement objective;
the original fastest gait remains the accepted baseline.
