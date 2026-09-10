# Neural initialization for speed discovery

The objective remains sustained net horizontal displacement per second without
falling. CAD physics and actuator bounds remain in force. There is no slip,
appearance, tracking, lift, effort or gait-family penalty in candidate ranking.

This document retains the initial imitation experiments. The subsequent causal
dynamics predictors, PPO updates and measured comparisons are in
`PREDICTIVE_SPEED_TRAINING.md`. The best neural actor reaches 0.5177078967 m/s
over 60 seconds, but its 300-second speed is 0.4973228846 m/s; the original
teacher is faster over that longer comparison at 0.5143375634 m/s. All these
runs finish without a sampled fall. Those are nominal 0.625 ms results. At the
matched 0.3125 ms timestep, the neural actor reaches 0.5201547734 m/s over 300
seconds and the teacher reaches 0.4854009535 m/s. The ranking reverses; numerical
convergence remains unresolved. See `LONG_HORIZON_FIDELITY.md`.

The first direct-policy imitation experiment uses the verified 0.424 and
0.337 m/s teacher captures. The shared Rust `distillation` component fits a
33-input, 32/32-hidden-unit, 12-output tanh network from 2,000 recorded decisions.
Features are joint angles/velocities, body gravity direction/angular velocity,
and requested forward/lateral/yaw motion. They exclude reference trajectories,
phase, contact forces, body/foot positions and teacher feedback suggestions.
These remain ideal simulation observations, not calibrated hardware sensors.

Each output spans its full recorded actuator command interval: a constant
midpoint plus the network output scaled by the interval half-width. This uses
the existing Rust neural-residual execution interface as a direct actuator
policy. The Rhai program supplies only those constants; it contains no gait.
The student retains the teacher's robot, world, integration settings and command
schedule. The network can coordinate all twelve actuators without a prescribed
contact sequence. Its learned behavior is not yet a speed improvement.

Normalization and fitting use training samples only. Validation uses 4,000
decisions from seconds 20–60 of the same two teacher trajectories, excluding
their exactly repeated first 20 seconds. This tests later teacher states, not
independent disturbances or WASD generalization. The checkpoint is selected by
training loss. Closed-loop simulation, where prediction errors affect future
observations, provides the decisive measurement.

In this initial 300-epoch fit, normalized training MSE decreased from 0.275890
to 0.000216705; validation MSE was 0.000183021. Per-actuator validation RMS
errors range from 0.0110 to 0.0383 rad. Those numbers measure imitation only.

The first closed-loop student completed 20 seconds without a sampled fall, but
covered only 0.316124 m (0.0158062 m/s). It is substantially slower than both
teachers and must not be promoted as an improved gait. The result demonstrates
that low prediction error on teacher states does not establish useful control.

The shared `sim-script` example `label_policy_observations` now queries a Rhai
teacher on recorded learner states. Its regression check reproduces all 1,000
original teacher commands exactly, with no saturation. On learner states it
retains raw and actuator-bounded teacher advice separately from the actual
commands and physics. Those counterfactual labels support subsequent data
aggregation; they are not evidence that the advised recovery has succeeded.

The second experiment adds those 1,000 learner-state labels to the original
2,000 teacher decisions and continues fitting from the first network, preserving
its feature normalization. This is one dataset-aggregation iteration inspired by
[Ross et al.](https://proceedings.mlr.press/v15/ross11a.html), not PPO. Training
MSE decreases from 0.0186128 to 0.00604712, but the closed-loop student covers
only 0.057033 m in 20 seconds (0.00285164 m/s), again without falling. It is worse
than the first student and remains a failed speed-improvement experiment.

The closest pair of distinct student input vectors differs by 0.08376 RMS in
the frozen normalized feature space, while the corresponding teacher advice
differs by 0.41709 rad RMS. This suggests that teacher phase/history is missing
from the instantaneous student input, but does not prove a feedforward controller
cannot walk. More imitation epochs alone do not address that ambiguity.

The next initialization therefore retains the measured fast Rhai gait and adds
a zero-output network, using the existing learned hidden features. Each residual
output spans a full actuator command interval, sufficient to move between any
two valid commands. Explicit `neural_command_saturation` clamps the combined
request in the shared Rust runtime without changing CAD bounds, servo torque or
contact dynamics. Zero output must preserve the original physical trajectory.
Raw requests and saturation are recorded separately from applied commands.
This is an initialization for learning; it is not yet a trained improvement.
The 0.448-screen candidate was selected for the first residual initialization
because it averages 0.443124 m/s over 60 seconds. Faster 0.462/0.485 short screens average
only 0.425935/0.417178 m/s over that duration. The prepared 0.462 neural input is
retained as an unexecuted preparation. The completed 0.448 zero-residual check
reproduces all 1,001 physical frames exactly, including joint/motor states,
contacts and applied targets. Its measured speed remains 0.4483370316 m/s over
20 seconds. The new runtime passes tests for zero-output preservation, explicit
saturation, replay of ordinary neural corrections, and rejection of nonfinite
combined requests. This verifies initialization, not learned speed improvement.

The subsequent expanded search found a stronger teacher: 0.506660 m/s over
20 seconds and 0.508848 m/s over 60 seconds (30.530871 m net displacement),
without a sampled fall. Its half-timestep 20-second speed is 0.506537 m/s.
Enabling inter-link contact for the full minute gives 0.508847806 m/s, with
maximum chassis position difference 0.921 mm. The completed zero-residual replay
uses this teacher and the distance-only Rust learning task: all 1,001 physical
frames match exactly, and reward sum 10.1332027497 m equals measured displacement.

## Relationship to PLANC

[PLANC](https://arxiv.org/html/2601.06286v1) uses physics-generated motion
references and CLF rewards, followed by teacher/student distillation and PPO
fine-tuning. Its reduced-order planner is designed for bipedal stepping stones.
Our direct-policy experiment reuses the warm-start idea; it is not a PLANC
implementation. A quadruped planner and CLF guidance remain to be integrated.
The shared Rust path includes tested PPO gradients, Gaussian policy sampling,
rollout collection and updates. The full-robot predictive PPO experiment is
launched under `speed-ppo-predictive-v2.inputs.json`; it requires the independent
full-minute deterministic baseline to match the archived teacher before
stochastic rollouts and updates. V1 was stopped before any update because its
input preparation froze the command heartbeat. No learned speed improvement has
been established.
Existing episodic parameter search is not PPO.
The older Python compatibility example contains PPO but is not the architecture
for this full-robot work. A Rust implementation should reuse the shared network,
environment and replay contracts. [PPO's clipped policy objective](https://arxiv.org/abs/1707.06347)
is an optimizer mechanism; it does not justify adding gait-quality terms to the
user's speed objective.

The shared Rust environment now supports `Task.speed`, configured by
`speed-learning-task.json`. Each transition rewards the change in distance from
the reset origin, in metres. With discount 1, the return telescopes to net endpoint
distance; divide by the full episode duration for speed. Circling and reversal
cannot earn path-length rewards. The task detects body overturning, CAD hull
vertex ground clearance and body floor contact at transition endpoints. It adds
no actor sensor channels or gait-quality rewards. The complete-episode evaluator
withholds eligible scores on termination or errors; partial returns are diagnostic.

`speed-discovery-task.json` remains unchanged for existing experiments and has
no reward terms. Passing it to `train_residual_policy` still gives zero scores.
The new task is covered by analytic closed-path/fall checks and runtime
displacement, reset/replay and duration-normalization tests. Stochastic raw action
records now remain distinct from saturated actuator commands in the shared
runtime and PPO likelihood calculations. The PPO-Lagrangian update uses only
net progress and the fall condition; numerical failures cannot become learning
samples. Tests cover analytic gradients, a known bandit and replayable environment
rollouts. These establish the implementation, not successful robot learning.
See `PREDICTIVE_SPEED_TRAINING.md` for the causal trajectory model, its shared
actor-input integration, and the distinction between prediction and control
validation.

For the speed objective, reference guidance should help initialize learning and
then be removed from final ranking. The planner and learner must be allowed to
change placement, timing and contact patterns, and exceed the teacher's speed.
WASD cases and documented physical/sensor uncertainty should subsequently supply
training conditions; they must not introduce unrelated gait-quality gates.

## Reproduce the first initialization

From this worktree, restore the SHA-pinned captures in the study descriptor and
choose fresh output directories before rerunning:

```sh
node examples/interactive/prepare_direct_distillation.mjs examples/full-robot/contact-planning/speed-neural-imitation-v1.spec.json
cargo run --locked --release -p sim-runtime --example distill_policy -- runs/speed-neural-imitation-v1/input/experiment.json runs/speed-neural-imitation-v1/fit
node examples/interactive/materialize_direct_student.mjs runs/bayesian-speed-only-diagonal21/evaluation-004 runs/speed-neural-imitation-v1/input runs/speed-neural-imitation-v1/fit runs/speed-neural-imitation-v1/evaluation
```

Use the ordinary `run_environment` binary with the materialized student scene,
config, actions and `speed-discovery-task.json`; reduce the resulting capture
with `measureSpeedRun`. The browser remains on the verified teacher preset until
a learned candidate has its own measured evidence.
