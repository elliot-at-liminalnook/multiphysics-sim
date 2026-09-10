# Reproducible motion experiments

`sim_runtime::experiment` executes parameterized controller candidates through
`EmbeddedEnvironment`. The same evaluator supports native tools and browser
workers. It adds experiment identity, proposals, bounded advancement, checkpoint
replay, result eligibility and a serializable journal; robot physics and controller
execution remain in the existing runtime.

## Contracts and lifecycle

`ExperimentSpec` contains the scene, physics configuration, task, motion
parameterization, source command schedule, baseline parameter values, environment
seed and explicit objective. `Experiment::bind` validates the baseline through the
ordinary environment and requires exactly one command row per task interval.
It records the current library source/features and hashes the complete specification.
Changing physics, task, mapping, commands, bounds, seed or horizon changes context.
Canonical JSON hashing preserves harmless signed-zero and integral-float spelling
changes between hosts; other finite floating-point values retain their exact bits.

`propose(values, method)` validates named search coordinates and gives the candidate
a content-derived ID. `Journal::submit` records it before evaluation and rejects
duplicates. Methods retain the proposal's selection provenance; duplicate detection
depends on context and values, independently of that descriptive label.

`Experiment::start` materializes ordinary controller references/commands and creates
the shared environment. `Evaluation::advance(n)` counts at most `n` task intervals,
including replay intervals. Hosts can pause between calls. `frame()` and
`metadata()` expose the ordinary runtime output and shared motion parameter
declarations, including units and integer domains. `checkpoint()` captures the
recording, final physical frame, task transition and accumulated reward.

`Experiment::resume` validates the context and command prefix, then reconstructs
state through the existing recording/replay API. Before executing any new command,
it checks the rebuilt physical frame, task transition and accumulated reward
against the checkpoint. The frame comparison removes only `stepping_wall_s` and
uses canonical numeric equivalence. A changed physical value is an error.
During reconstruction, checkpoint requests return the original checkpoint so a
second interruption does not move saved progress backwards. Replay cost grows with
the completed prefix; this is not an instantaneous physical-state restore.

Only complete, nonterminated, nonfailed episodes have eligible scores:

- `net_speed` reads the explicit progress/legacy speed task's final net speed.
  It requires such a task and adds no slip, gait, tracking or energy penalty.
- `reward_rate` divides the explicit task's undiscounted return by episode time.
- Partial, terminated and failed evaluations retain diagnostics and have no score.
  A physics/controller failure cannot be resumed as a healthy episode.

`Journal::record` updates pending progress, rejects regression and terminal-result
replacement, and binds the checkpoint to its proposal. `best()` considers eligible
observed results only. Saved diagnostics are host-attested; checksums and context
checks detect accidental mixing/corruption, not maliciously fabricated physics.

## Bayesian selection and native persistence

The optional native `bayesian` feature connects the existing EGObox adapter to the
journal. Selection starts with the baseline and a seeded Latin hypercube, then
uses the existing Gaussian-process acquisition optimizer. Negative measured scores
implement maximization. Failed evaluations retain identities without fabricated
objective values or constraint residuals. A pending trial must finish before a new
proposal is requested. Parameters with equal bounds remain fixed. Free integer
parameters require a discrete selector; this adapter refuses them without rounding.

Selection is recomputed from the explicit settings and complete retained history.
Insufficient successful data, repeated proposals or backend failures return errors;
they are not evidence of a physical maximum and do not silently change the strategy.

Native proposal selection is synchronous and has no cooperative cancellation hook.
A host can terminate the native process and reopen its last committed revision;
cancellation-file detection waits for control to return. Selection latency is not
qualified for realtime planning. Incremental evaluator calls remain separately
controllable between physics/action intervals.

```sh
cargo build --locked --release -p sim-runtime --features bayesian \
  --example search_motion --example check_motion_experiment
target/release/examples/search_motion init spec.json settings.json fresh-directory
target/release/examples/search_motion advance fresh-directory 100 20 pause.request
```

`advance` accepts a **new-action budget** and a total trial budget. Reconstruction
cost is reported separately and is cancellable between replay intervals. If the
optional cancellation file exists, the call pauses without invalidating the trial.
To continue, remove the request or omit that optional argument. These host budgets
bound a command invocation; they are not limits on the overall optimization goal.

The native CLI uses an OS file lock that releases when its process exits. It writes
immutable numbered journal revisions through a flushed/synced temporary file and
atomic rename, then syncs the directory. A proposal is saved before execution.
Revisions contain checksums and parent references; loading validates the latest
revision and its immediate parent. Incomplete temporary files are ignored, and
existing committed revisions are not overwritten. Evaluation checkpoints are
stored in the journal itself, so solver evidence references resolve to retained data.
This filesystem host is exercised on macOS; directory-sync behavior is platform dependent.

## Browser and acceptance evidence

WASM exports `bind_motion_experiment` and `MotionEvaluation` with start/resume,
advance, checkpoint, frame and metadata methods. Worker messages use the
`experiment_` prefix. They preserve the separate ordinary viewer simulation.
Hosts should request small advance chunks to retain responsiveness. The Bayesian
backend is native; evaluations use the same Rust code in either host. Build the
browser with `--features sim-runtime/bayesian` when sharing a context with the
native Bayesian executable, since runtime identity conservatively includes features.

`experiment-evidence-v1.json` retains inputs, binaries, native/browser captures,
immutable search journals and verification logs. Acceptance includes:

- 62 selected tests covering context rejection, exact replay, outcome eligibility,
  JSON numeric equivalence, shared forecasts, motion bindings and fidelity checks.
- Native and browser replay/resume on the quadruped and original wheeled CAD form,
  with exact same-host physical frames across the complete replayed/resumed path.
- Four-trial searches on both forms, including an actual Bayesian proposal, with
  interrupted/uninterrupted proposals and final checkpoints compared exactly.
- A cancellation request preserves the saved revision and advances no physics.

The wheel case enables shared terrain contact at 0.125 ms physics steps and uses
an explicit chassis-inversion termination bound. Its 30 ms horizon includes
settling transients. The quadruped uses a 100 ms prefix. These cases validate the
optimization workflow, not sustained locomotion speed, convergence, learned
prediction accuracy, cross-host exact checkpoint transfer or global optimality.
Longer locomotion validation, efficient complete-state snapshots, coordinated
motion timing, discrete selector integration and morphology-conditioned learning
remain open parts of the broader goal.
