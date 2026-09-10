# Predictive learning for sustained speed

The objective is complete-horizon net displacement per second without falling.
Prediction accuracy is a model-training diagnostic, not an extra gait acceptance
condition. CAD geometry, transmissions, contact and actuator physics stay in the
shared Rust runtime.

In the initial teacher/PPO comparison, the current-dynamics actor reaches
**0.5177078967 m/s over 60 seconds**.
Over the longer **300-second** comparison, the original teacher is faster:
**0.5143375634 m/s**, versus **0.4973228846 m/s** for that actor. Both finish
without a sampled fall. That comparison uses a 0.625 ms timestep. At the matched
0.3125 ms timestep, the neural actor reaches **0.5201547734 m/s** over 300 seconds
while the teacher reaches **0.4854009535 m/s**, again without sampled falls.
The ranking reverses. At 0.15625 ms the teacher reaches 0.4559723646 m/s and
the neural actor reaches **0.5204327404 m/s**, both over 300 seconds without a
sampled fall. Neural net speed changes by 0.0534% between the two finer
resolutions, though endpoints differ by 34.31 m. The neural actor outperforms that
original teacher at those resolutions; trajectory convergence and physical limits remain
unresolved. See `LONG_HORIZON_FIDELITY.md`.
Both fresh-seed PPO continuations completed four
updates without improving it. Independently scheduled command data now improves
the trajectory predictor; the latest study at the end of this document separates
prediction accuracy from measured controller speed. Earlier sections retain the
experimental history, including their then-pending next steps.

Subsequent measured-response steering improves the teacher candidate to
**0.5467083058 m/s over 300 seconds at 0.625 ms**, and a separately calibrated
steering command reaches **0.5468916069 m/s at 0.3125 ms**, both without sampled
falls. This is a measured model-guided policy improvement, not a new neural
learning result. The fine winner is under a fixed-controller 0.15625 ms test;
the differently tuned coarse/fine results do not establish convergence. See
`STEERING_RESPONSE.md`. The observation-sampling audit and subsequent heading
feedback experiments are in `DYNAMICS_OBSERVATION_SAMPLING.md`.

The first trajectory model consumes positions, velocities, finite-interval
acceleration estimates, body gravity direction/angular velocity, previous applied
targets and a proposed future actuator-target sequence. It predicts position,
velocity and acceleration estimates at 20, 100 and 200 milliseconds for the 12
driven coordinates and Cartesian chassis/four-foot link COMs. Link trajectories
use a frame fixed at the current chassis pose; future vectors do not rotate with
the chassis. Joint units and indices come from recorded runtime metadata.

Its reference is the explicit kinematic extrapolation
`p(t)=p0+v0*t+0.5*a0*t²`, `v(t)=v0+a0*t`, `a(t)=a0`. A neural network learns
action-conditioned residuals to that reference. All targets come from committed
Rust simulation frames. Acceleration labels are differences of velocity over the
recorded interval, including impacts; they are not instantaneous sensor readings.
The linear prediction head has no speed/output cap.

`motion-forecast-v1.spec.json` trains on the first 20 seconds of the 0.448 and
0.507-screen teachers. Validation uses seconds 20–60 of those trajectories;
complete future windows stay on their own side of the split. Input normalization
and output residual scales use training data only. This tests later nominal
motion, not new terrain, sensor noise, held-action extrapolation or unseen gaits.
The actor must supply a candidate action sequence when using the predictor;
recorded future actions used as supervised conditioning are not future state
observations available to a deployed controller.

The first fit used 1,980 training and 3,980 validation windows. Its normalized
validation MSE decreased from 1.032526 to 0.0732833 relative to the kinematic
reference. At 20 ms, Cartesian component RMSE is 1.118 mm and driven-joint angle
RMSE is 0.018919 rad. At 200 ms these errors increase to 53.31 mm and 0.80325 rad.
This is a substantial correction to an acceleration extrapolation that can drift
badly through contact changes. The same validation windows give normalized MSE
0.462038 for constant velocity and 2.205674 for last-value prediction, versus
0.0732833 for the learned model. Individual long-horizon quantities can still be
worse than a simple baseline: the 0.448 gait's 200 ms Cartesian component error
is 56.72 mm learned versus 53.87 mm for last-value prediction. Forecast
accuracy has not yet been shown to improve locomotion speed or sample efficiency.

The controlled v2 comparison keeps the same data, network width, seed and fit
settings, but uses constant velocity as the reference. Acceleration estimates
remain inputs and prediction targets; only their extrapolation in the reference
changes. This improves every horizon/unit group in the physical-error comparison.
Cartesian component RMSE is 0.954 mm at 20 ms, 5.706 mm at 100 ms and 11.618 mm at
200 ms, versus 3.606/40.533/84.146 mm for constant velocity alone. Driven-joint
angle RMSE is 0.015999/0.074571/0.150669 rad. Normalized losses across v1 and v2
cannot be compared directly because their residual scales differ.

Review found a causal limitation in both v1 and v2: their shorter predictions
could consume commands scheduled after the predicted endpoint. Those commands
can encode information from the teacher's subsequent feedback. These archived
fits are diagnostic comparisons, not validated causal controller models.

V3 uses separate 20, 100 and 200 ms networks. Each consumes only the command
prefix through its own endpoint. The same training and validation anchor times,
width, seed and optimizer settings are retained. Cartesian component RMSE on the
held-out nominal segments is 0.723/6.253/12.037 mm. These results still condition
on recorded action sequences; they do not establish counterfactual accuracy for
new commands. Closed-loop teacher data alone cannot establish that the model
has learned the effect of independently varied actions.

`ForecastBundle` exposes typed current-dynamics and forecast channels to the
actor through `PolicyConfig.trajectory_forecast`. Online inference uses current
Rust kinematics, the previous sampled state and applied commands, and the current
clamped Rhai proposal held over each horizon. No future recording is read. The
Rhai contract stays unchanged; neural corrections are applied after prediction
and go through the existing actuator execution. Forecasts therefore describe the
baseline proposal, not the trajectory after the actor's correction. A separate
valid flag marks missing startup history, whose placeholder features normalize
to zero. Reset reconstructs that history; replay reconstructs it from inputs.

`prepare_predictive_policy` adds the shared feature definitions and zero input
columns to an existing actor, preserving its initial outputs. Current dynamics
use training normalization; predicted motion is represented as a correction to
its explicit physical-unit kinematic reference. Focused runtime tests verify
zero-policy physical parity, backward acceleration timing, reset/replay, contract
rejection, per-head causality and Gaussian likelihoods from augmented inputs.
The full-robot zero-output replay matches all 1,001 frames in poses, joint states,
servo targets and contacts, retaining 0.5066601375 m/s over 20 seconds. Its online
20 ms forecasts match the commands actually executed for all 999 valid windows,
with Cartesian component RMSE 0.707 mm. None of the 100/200 ms held proposals
matches the changing teacher commands over its entire horizon, so the audit
does not assign those forecasts a measured error. Held-command and
learner-trajectory validation, and a comparison of training with and without
predictions, are needed before claiming improved sample efficiency.

The shared Rust code includes a seeded Gaussian actuator policy, raw-action
likelihood records, PPO-Clip gradients, linear value heads, finite-horizon
advantages and a shared Adam optimizer. Runtime replay tests verify saturation
does not replace the sampled action in likelihood calculations. Numerical
failures are preserved and excluded from policy updates. Rollouts retain joint
and link motion plus applied targets for subsequent dynamics-model fitting.

The speed trainer uses undiscounted finite-horizon returns and a PPO-Lagrangian
surrogate. Its sole cost is a sampled fall, with target rate zero; a learned
multiplier adjusts that cost rather than introducing a fixed gait-quality reward.
Full-horizon deterministic evaluations select checkpoints only when they finish
without a sampled fall and improve net speed. PPO clipping and gradient-norm
limits govern optimizer updates, not the robot's motion range.

The predictive PPO experiment starts from the parity-checked 0.507 teacher using
496 actor features and all 12 actuator outputs. V3 forecast heads
are frozen during this experiment. Each stochastic episode and deterministic
evaluation covers the full 60-second task. Two seeded episodes feed each of four
initial updates; normalized action standard deviation is 0.01. This is an initial
optimizer experiment, not an action-range restriction or a deadline for the speed
goal. The initial v1 setup incorrectly repeated the first action packet, including
its heartbeat counter. The command lease expired and the baseline covered only
0.065996 m in 60 seconds. That run was stopped before any policy update; its
evidence is retained in `predictive-ppo-setup-evidence.json`.

V2 restores the exact verified 3,000-packet full-minute schedule and requires
baseline distance 30.5308708205 m within 1e-8 m before collecting stochastic
rollouts. This checks experiment setup only; it is not a constraint on learned
motions. `speed-ppo-predictive-v2.inputs.json` pins its code, executable and
corrected recipe. Its baseline passed at 30.530870820514558 m. The first two
noisy full-minute episodes completed without falling at 0.506137 and 0.505501 m/s.
The first PPO update completes the independent full minute without falling at
0.501812328 m/s, slower than the 0.508847847 baseline, and is not accepted.
Further updates are running. `predictive-ppo-first-update.json` retains its
score and optimizer diagnostics. No learned speed gain has been established.

The shared forecast data adapter accepts `SpeedRollout` directly, including
physically terminated episodes, with `format: "speed_rollout"` in training
windows. It verifies contiguous motion and the recorded actuator order, then
uses the same physical labels and frame transforms as native captures. Numerical
failures remain excluded. This enables fitting and testing dynamics models on
states and noisy actuator commands actually visited by the learner, without
using its predicted motion as ground truth.

V4 adds the first 40 seconds of learner seed 74 to forecast training and its
last 20 seconds to validation, retaining the earlier teacher data and fit
settings. On independent seed 75, Cartesian component errors improve from
1.297/8.014/15.864 mm to 1.061/6.662/12.992 mm at 20/100/200 ms. Seed 75 is
excluded from both generations' training. `learner-forecast-comparison.json`
records that comparison; improved forecast accuracy is not a speed result.

An explicit policy-only held-command probe supplies a harder action test. It
holds bounded targets for five 400 ms windows while the original command lease
and baseline state continue. The 12-second run finishes without falling. V3's
100/200 ms position errors on the 80/55 action-matched windows are 25.118/53.950 mm,
versus 16.397/51.385 mm for velocity extrapolation. V4 reduces those errors to
20.024/43.643 mm without training on the probe, but its 100 ms error is still
worse than the simple reference. These results expose limits of nominal-gait
validation. `audit_online_forecast` can compare replacement models on exactly
the recorded proposal-matched windows; recomputing V3 reproduces its original
online errors. The live PPO run continues with its frozen V3 models.

This follows the model-guided learning direction in
[PLANC](https://arxiv.org/html/2601.06286v1), but PLANC uses a reduced-order
biped stepping planner and CLF guidance. Constant-acceleration extrapolation is
not that planner and contains no contact-force planning. The trajectory model
needs on-policy validation before any claim that it improves locomotion or
learning speed.
[PPO](https://arxiv.org/abs/1707.06347) and
[PPO-Lagrangian](https://github.com/openai/safety-starter-agents) supply optimizer
ideas; their use does not establish a physical speed maximum.

The completed zero-residual replay of the new teacher reproduces all 1,001
physical frames exactly at 0.5066601375 m/s over 20 seconds. The Rust reward sum
is 10.1332027497 m, matching endpoint displacement. This validates the starting
controller and distance reward, not a learned speed improvement.

## First measured learned improvements

Replaying the first update on the same saved seed-74/75 rollouts reproduces the
legacy actor, critic, Adam moments and multiplier exactly. A controlled replay
changes only minibatch ordering to seeded sample shuffling. Its deterministic
60-second evaluation reaches 30.6962986577 m (0.5116049776 m/s), above the
0.5088478470 m/s teacher. Half-timestep and inter-link-contact checks reach
0.5101422295 and 0.5116046525 m/s, respectively, with no sampled falls. The
original cyclic-batch learner separately reaches 0.5106368290 m/s after its
second update; its third update drops to 0.5069189773 m/s and is not selected.

Both updates have positive empirical surrogate losses after optimization.
Exact fixed-covariance Gaussian KL is now reported separately from sampled
likelihood diagnostics. Neither quantity is a physical speed score. The shared
optimizer supports seeded Fisher–Yates sample permutations per epoch, with
identical actor/critic indices, following the batching pattern in
[OpenAI PPO2](https://github.com/openai/baselines/blob/master/baselines/ppo2/ppo2.py).
Omitting `shuffle_seed` preserves old cyclic ordering and exploration draws.
This is a controlled single-data-set comparison, not general evidence that
shuffling guarantees better locomotion.

Swapping only this actor's V3 predictor for V4 lowers speed to 0.5106338886 m/s,
despite V4's lower held-out prediction errors. The actor was trained with V3;
this comparison tests an immediate model replacement, not retraining with V4.
There is still no ablation demonstrating that forecasts accelerate learning or
improve speed over an otherwise identical policy without forecast inputs.

The completed 34-observation minute-long Bayesian study supplies a faster
teacher: 31.0121265317 m (0.5168687755 m/s). Half-timestep and full inter-link
contact checks give 0.5160008921 and 0.5168688425 m/s. Adding the learned actor
to that teacher gives 0.5158248388 m/s; the teacher alone remains faster.
All these evaluations complete 60 seconds without a sampled fall.

`predictive-speed-gains-v1.json` records the comparison and
`predictive-speed-gains-evidence-v1.json` durably archives inputs and evidence.
The generic Rust `evaluate_policy` and `replay_ppo_update` examples use the same
environment as training. `train_ppo_policy` now accepts an explicit complete
`initial_state`, validates its actor contract and optimizer state, and preserves
the global update counter when choosing episode seeds. Tests cover exact
serialization/resume, stale critic rejection, deterministic shuffling and
analytic Gaussian KL. Two new fixed experiments continue the shuffled learner
on its original teacher and start a fresh shuffled learner on the faster teacher.
Their inputs are archived; completion or improvement is not yet asserted.

## Prediction ablation and model-based action proposals

The original four-update PPO run is now complete. Its final update reaches
30.7793002569 m (0.5129883376 m/s) over 60 seconds without a sampled fall. It
is the best learned result from that run, still below the 0.5168687755 m/s
physics-search teacher. This newer actor has not yet received its own finer-step
and inter-link-contact validation.

An immediate ablation of the validated 0.5116049776 actor zeros only its 243
future-prediction input columns. Current motion, acceleration and proposed-action
features remain. Speed falls to 0.5103730545 m/s, a 0.2408% reduction. This tests
the trained actor's use of predictions; it does not measure sample efficiency.
A matched fresh training branch on the 0.5168687755 teacher removes those
prediction features from both actor and critic, retaining 253 inputs, the same
initial controller outputs, episode seeds and optimizer settings. Its baseline
guard passes. Training comparisons remain in progress.

The shared network now exposes input vector-Jacobian products. The trajectory
forecaster converts these to physical units, with its kinematic prior held fixed;
that is sufficient for future-action derivatives because the prior has no
future-action dependence. The shared projected-ascent optimizer normalizes by
declared bound widths and backtracks for predicted objective improvement.
`search_forecast_actions` supplies a focused Rust example: optimize future
actuator packets against a same-unit weighted trajectory output, keeping state
and past actions fixed. Explicit actuator bounds come from the recorded runtime
contract. No contact or actuator physics is replaced by the network.

The first robot experiment optimizes all 120 target values over 200 ms at the
start of the earlier held-command probe. Its objective is predicted displacement
along the current net-travel direction. Predicted displacement rises from
95.838 to 125.688 mm. The real runtime gives 74.231 versus 82.153 mm: a measured
7.923 mm gain, considerably smaller than the predicted 29.850 mm gain. All 101
pre-proposal physical frames match exactly, all 120 applied targets match the
proposal, and no sampled fall occurs through the 2.2-second diagnostic horizon.
The candidate prediction error grows from 21.608 to 43.534 mm, exposing model
exploitation/error even though this proposal improves local measured progress.
This is not a minute-long gait result or a deployed receding-horizon controller.
Extending that same one-time action sequence through the original 12-second
probe completes without falling, reaching 4.5406553756 m versus 4.5065134012 m
for the reference. The 34.142 mm net gain persists through recovery. Both runs
retain the probe's other held-command windows, so these speeds should not be
compared directly to continuous-walking candidates.

[Nagabandi et al.](https://arxiv.org/abs/1708.02596) provide a relevant precedent
for learned dynamics with model-predictive control followed by policy learning.
Our local gradient search is an adaptation, not a reproduction of their planner
or PLANC. Analytic tests check network input derivatives, physical normalization
scales and bounded optimization against a known quadratic. The recorded robot
probe tests whether a model-selected action sequence survives real simulation.
`predictive-control-study-v1.json` summarizes these measurements;
`predictive-control-study-evidence-v1.json` archives the inputs, terminal outputs,
tests and physical replay. Next, evaluate model-selected actions across more
states and use a receding-horizon policy through the shared Rust runtime before
claiming a model-planning speed improvement.

## Receding-horizon trial and matched learning comparison

`PolicyConfig.forecast_action_search` now enables a shared Rust planner. At each
controller tick it searches the longest causal forecast head, initializes from
the held Rhai proposal or a better shifted previous plan, and applies the first
optimized motor packet. The requested planar direction is rotated by an
explicit heading offset and projected onto the horizontal plane using measured
gravity orientation. Existing intersected software/CAD actuator bounds define
the search domain. The shared command lease and zero motion requests return to
the baseline controller. Neural corrections, if configured, follow planning.
Telemetry preserves planned sequences, predicted objectives and optimizer work;
physics remains in the existing Rust environment.

The V3 pilot uses five ascent steps per 20 ms control tick and a 200 ms horizon.
It completes 60 seconds without a sampled fall but reaches only 20.4466099353 m,
or 0.3407768323 m/s, despite predicting an average 10.684 mm improvement per
planning tick. Its 2,999 planned first actions give 20 ms Cartesian prediction
RMSE of 3.501 mm versus 4.032 mm for velocity extrapolation. No full 100/200 ms
proposed sequences were executed unchanged, so those online prediction errors
cannot be scored as if the plans were followed. Wall time was 230.2 seconds in
the concurrent experiment environment; this is not realtime-browser validation.
The pilot is slower and is not promoted.

The matched fresh PPO comparison is more favorable without predicted trajectory
features: its first update reaches **31.0307905350 m in 60 seconds**, or
**0.5171798423 m/s**. Finer-timestep and inter-link-contact checks give
0.5166651163 and 0.5171796510 m/s, respectively, all without sampled falls. Its
nominal gain over the physics-search teacher is small (0.0602%), but it also
leads in the matched finer-step condition. The prediction-input arm reaches
0.5163084851 m/s after that same first update. Both arms' two initial rollouts
match exactly across all 6,000 physical transitions, applied targets, rewards,
fall costs and initial critic values. Only neural feature sets and subsequent
updates differ. This single comparison does not prove general superiority or
learning sample efficiency. Current velocity and acceleration inputs remain
in both arms; the later training updates continue independently.

V5 dynamics fitting adds the first 39.82 seconds of the planner's executed
trajectory to V4's training corpus and reserves 40–59.82 seconds for validation.
Network width, seed and fitting settings remain unchanged. Targets come from
physical motion under executed actions, including the effects of replanning;
predicted future states are never training labels. Fitting and a subsequent
controlled planner replay must establish whether this additional experience
improves predictions and measured speed.

`receding-forecast-study-v1.json` records the completed comparisons and
`receding-forecast-study-evidence-v1.json` preserves the corresponding evidence.
Tests cover first-command application, command expiry, stop/reverse requests,
reset and exact replay, typed input rejection, and unchanged zero-policy physics.
The older binary's exact source snapshots are hash-checked and archived; later
metadata clarification does not change its measured control behavior.

V5 fitting is complete. On identical held-out executed-action windows from
40–59.82 seconds, Cartesian position RMSE changes from 3.546/27.602/60.906 mm
with V3 to 1.983/15.297/32.253 mm with V5 at 20/100/200 ms. Both generations
receive the same recorded future action sequence in this offline comparison;
these are not errors for unexecuted counterfactual plans. A new full-minute
controller trial changes only the forecast bundle to V5. It completes without
falling but drops further to 7.5972201284 m, or 0.1266203355 m/s. Its executed
20 ms forecasts have 2.061 mm position RMSE versus 3.647 mm for velocity
extrapolation. Better prediction accuracy has not produced better control.

The sampled path-length rates are 0.3942 m/s with V3 and 0.1873 m/s with V5,
above their respective net-distance rates of 0.3408 and 0.1266 m/s. The local
planner objective follows requested travel direction as the body rotates; the
task instead scores net displacement from the episode origin. This mismatch
must be addressed using the same net-distance objective, without adding a
heading penalty. Curving is not the entire slowdown: path-length rates also
remain below the teacher. The short horizon and counterfactual model errors
remain unresolved. Neither planner result replaces the faster learned gait.

The current-dynamics-only learner's second update reaches 31.0624738008 m,
or 0.5177078967 m/s, without a sampled fall. Its completed finer-step and contact
checks reach 0.5190404374 and 0.5177078626 m/s, respectively, also without sampled
falls over full minutes. It is now the fully checked nominal leader. All three
training branches continue. `learned-speed5177-validation.json` and
`learned-speed5177-evidence-v1.json` record the completed comparison.
`receding-forecast-v5-evidence-v1.json` preserves the completed V5 controller
trial, its online forecast audit, path diagnostics and this finer-step result.

## Net-distance planner comparison

The optional `episode_net_displacement` planning objective now uses the same
XY endpoint-distance function as the speed monitor. It transforms each predicted
endpoint from the current body frame into world coordinates and differentiates
the resulting increase in distance from the reset origin. This is a speed-search
mode: a live nonzero request activates it, but its direction only selects a norm
subgradient at zero distance. Ordinary requested-direction control remains the
default. The change adds no heading, slip, effort or gait-quality penalty.

The two controlled full-minute trials change only that objective, retaining the
corresponding V3/V5 models and planner settings. Neither improves speed:

| Forecast model | Requested-direction objective | Net-distance objective |
| --- | ---: | ---: |
| V3 | 0.3407768323 m/s | 0.2984339445 m/s |
| V5 | 0.1266203355 m/s | 0.0298699403 m/s |

All four trials finish without sampled falls. Correcting the objective mismatch
is therefore insufficient. The new planners still predict mean local gains of
13.453 and 16.708 mm per tick. Their executed 20 ms position RMSE is 3.723 and
2.374 mm; again, none of their complete 100/200 ms plans was executed unchanged.
Those longer online errors remain unmeasured. Analytic derivative tests,
reset/replay tests, and all 101 physical frames of a separate two-second legacy
replay pass. Neither slower planner is promoted.

All three prior four-update PPO branches have now finished. The forecast-input
branch never beats its 0.5168687755 m/s initial teacher. The earlier shuffled
resume peaks at 0.5126732541 m/s. The current-dynamics branch retains its second
update, 0.5177078967 m/s, as its best; the final two updates are slower. Two new
continuations restore that winning actor, critic, Adam state and update count.
Both use fresh base/shuffle seed 1031, comparing normalized action exploration
standard deviations 0.01 and 0.03. Their initial full-minute baseline must match
31.062473800777763 m within 1e-8 m. These are pending experiments, not new gains.

An additional observability check identifies missing physical information in
the current forecast recipe. Vertically translating an airborne body and foot
and their preceding state leaves every body-relative input and reference
prediction identical. Yet on an explicitly fixed ground plane the lower point
can contact within 200 ms while the higher point remains airborne. The focused
analytic test documents this ambiguity; it does not quantify its contribution
to the robot's slowdown. A future recipe should expose terrain-relative height
or clearance from the shared world/contact components, with explicit provenance
and compatibility for old captures. No terrain feature has been added yet.

`net-forecast-study-v1.json` records the completed comparisons;
`net-forecast-study-evidence-v1.json` preserves source, tests, captures, terminal
PPO results and the immutable inputs of the new continuations. The best gait
still uses current dynamics without consuming predicted-trajectory columns.
The forecasting subsystem remains available, but neither accelerated learning
from predictions nor a global physical speed maximum has been demonstrated.

## Terrain-aware forecast observations

`ForecastRecipe.terrain_relative_links` optionally adds one length-valued input
per named link: its current COM world-Z position minus the authored surface
height at that link's current XY. `World::floor_height` and articulated contact
share the same surface query. The observation describes COM height, with no
clearance threshold or change to the speed objective. It remains an ideal
runtime observation until hardware estimation is bound explicitly.

Online prediction samples the active articulated world. Native-capture training
reconstructs the same values from each current pose and the recorded world;
speed-rollout adaptation now preserves that world as well. Terrain-aware
recipes reject missing explicit floor/terrain definitions, malformed surfaces,
missing links and conflicting recorded ground observations. Empty recipes
preserve legacy input order, model compatibility and physical behavior. Future
action offsets come from the shared recipe so the extra inputs cannot shift
causal action-prefix validation silently.

Thirteen focused runtime tests pass. They include an airborne-height ambiguity
case resolved by the added inputs, whole-world translation invariance, flat and
sloped surface observations, identical live/offline features, unchanged
zero-output-controller motion, native/learner data adapters, reset/replay and
rejection cases. A full-robot two-second replay matches all 101 sampled physical
frames, policy predictions and planned commands from the older V5 controller.
Its speed denominator and endpoint truncation correctly reflect the shorter
declared horizon; net displacement and progress rewards remain identical.

V6 retrains all three heads on the unchanged V5 corpus with five added height
features, width 32, seed 73 and 40 epochs. The input counts are 116/164/224 for
20/100/200 ms. Adding columns changes the seeded hidden-layer initialization
shape, so this is not a comparison of identical initial hidden weights.
Aggregate normalized validation MSE changes from
0.093087/0.108863/0.114771 to 0.092892/0.108507/0.112050. The held-command probe,
excluded from both training sets, improves in Cartesian position RMSE from
1.143/9.281/21.431 mm to 1.124/9.093/21.204 mm. On the entirely unseen net-V5
planner trajectory, errors change from 2.374/16.925/35.193 mm to
2.249/17.958/35.765 mm: the longer horizons worsen. These comparisons use the
same actually executed future actions per model pair, not unexecuted plans.

Changing the net-distance controller from V5 to V6 produces 3.3759431471 m in
60 seconds, or 0.0562657191 m/s, without a sampled fall. This improves on that
specific V5 planner's 0.0298699403 m/s but remains far below the learned
0.5177078967 m/s incumbent. Its 2,999 executed 20 ms command prefixes give
2.277 mm Cartesian position RMSE. No complete 100/200 ms proposed sequence is
executed unchanged, so those online errors remain unmeasured. The two continuing PPO runs also fail to beat the
incumbent in their first updates: sigma 0.01 reaches 0.5117303745 m/s and
sigma 0.03 reaches 0.4998237301 m/s. Subsequent updates continue.

The next planned comparison is a future-command initialization from a validated
walking trajectory. Current planning initializes from a held Rhai command or a
shifted previous plan. An authored sequence of future actuator commands could
provide a better walking proposal while leaving every bounded action variable
free to improve net distance. This would supply planned commands, not future
physical state observations. Its speed benefit remains untested.

`terrain-forecast-study-v1.json` summarizes the completed results and
`terrain-forecast-study-evidence-v1.json` preserves the code, tests, trained
models, comparisons, controller capture and closed first-update PPO checkpoints.
The later PPO runs remain active; their unfinished outputs are excluded.

## Authored action proposals and independent command data

`ForecastActionConfig.reference_proposal` optionally supplies a typed actuator
trajectory on episode simulation time. The shared Rust planner compares its
predicted net progress with the held baseline command and shifted previous plan,
then optimizes the best initial candidate. All action variables retain their
existing bounds and remain free to move away from the reference. There is no
tracking reward. Reference commands must match the forecast's CAD hash, actuator
names, order, units and bounds; no future physical observation is supplied.
The first planned command is sampled at the current policy time, because it
executes over the next interval. Reset and replay preserve that timing.

The full-minute reference comes from the hash-verified 0.5168687755 m/s teacher.
Adding this initialization changes the V3 net-distance planner from
0.2984339445 to 0.2038550220 m/s and V6 from 0.0562657191 to 0.2434687116 m/s.
Both complete 60 seconds without a sampled fall, but neither beats the
incumbent. The disabled feature reproduces all 101 physical and policy frames
of a two-second legacy replay exactly. Seven focused runtime tests pass,
including reference timing, selection, free optimization, contract rejection,
command expiry and reset/replay.

Closed-loop recorded commands can encode the teacher's response to later
physical states. To improve action coverage, `perturb_actuator_reference`
precomputes independently sampled Gaussian command perturbations in Rust before
simulation. Noise scales existing actuator command spans and is clipped only to
those same recorded bounds. A Rhai adapter uses the shared Rust trajectory and
command-lease components to apply that fixed plan. Current sensor feedback does
not choose the active targets. The baseline still advances, and an expired or
zero motion request returns control to it. This is a system-identification
experiment, not a new robot runtime or a deployed feedback controller.

Two seed-1701 captures use standard-deviation fractions 0.03 and 0.10; seed 1702
provides another 0.10 capture. All finish 60 seconds without a sampled fall, at
0.3951151802, 0.2030249020 and 0.1842841395 m/s respectively. These are data
collection runs, not speed improvements. `audit_actuator_reference` checks all
3,000 intervals and 12 commands per capture against the shared trajectory sampler:
all 108,000 comparisons match exactly. Repeating a generator seed/specification
reproduces identical command bytes. The unperturbed adapter reproduces all 101
sampled poses, joint states, contacts and targets of the teacher's first two
seconds. Its focused test also checks sensor independence, stop, lease expiry,
baseline state advancement and reset/replay.

V7 retains the V6 recipes, width 32, seed 73, 40 epochs and optimizer settings.
It adds seconds 0–39.82 of both seed-1701 captures to fitting and seconds
40–59.82 to validation; seed 1702 is entirely excluded from fitting. Separate
20/100/200 ms heads consume only the command prefix through their own endpoint.
All three fits finish. Changing the corpus changes training normalization, so
the normalized loss values are not directly comparable with V6. The fixed final
epoch is evaluated; seed 1702 is in the declared validation set, not a hidden
test set used only after all model development.

On identical actual future-command windows, Cartesian component position RMSE
changes as follows (millimeters, V6 → V7):

| Excluded-from-fitting trajectory | 20 ms | 100 ms | 200 ms |
| --- | ---: | ---: | ---: |
| Independent commands, seed 1702 | 3.617 → 2.613 | 24.128 → 17.283 | 49.320 → 35.329 |
| Held-command probe | 1.124 → 1.083 | 9.093 → 8.860 | 21.204 → 19.096 |
| Later V3 planner segment | 1.963 → 1.900 | 15.778 → 13.750 | 32.119 → 28.960 |
| V6 planner with authored reference | 1.927 → 1.536 | 14.625 → 10.737 | 28.223 → 21.256 |

Seed-1702 joint angle, joint velocity, joint acceleration estimate, Cartesian
velocity and Cartesian acceleration estimate errors also decrease at every
horizon. For example, Cartesian velocity RMSE at 200 ms decreases from 0.3684
to 0.3031 m/s and acceleration-estimate RMSE from 16.419 to 14.217 m/s². These
are supervised forecasts conditioned on commands that actually executed. They
do not prove that optimizing the model's predictions improves real trajectories.
[Nagabandi et al.](https://arxiv.org/abs/1708.02596) provide a precedent for
neural dynamics learned from random-action data and used with MPC, followed by
model-free improvement. Their sample-efficiency result is not established here.

Both PPO continuations from the incumbent also finish. With noise fraction
0.01, the four deterministic updated policies reach
0.511730/0.509913/0.507391/0.499790 m/s. With fraction 0.03 they reach
0.499824/0.474807/0.470209/0.487315 m/s. All eight stochastic episodes per
continuation complete without sampled falls, but no deterministic update beats
0.5177078967 m/s. The full optimizer states and losing results are retained;
those completed settings should not be silently restarted as a new experiment.

The matched full-minute planner comparison changes only the V6 heads to V7.
It completes without a sampled fall but covers only 6.2547103399 m, or
0.1042451723 m/s, versus V6's 0.2434687116 m/s with the same authored proposal.
The 2,999 actually executed 20 ms prefixes yield 3.150 mm online Cartesian
position RMSE. No complete 100/200 ms proposed sequence is executed unchanged,
so those online errors are not assigned. Better errors on the held-out recorded
trajectories therefore did not produce better action selection. V7 remains a
diagnostic predictor, not the speed incumbent.

Matched 300-second evaluations subsequently finish with the same physics and
startup, extending only the horizon and command heartbeat schedule. The teacher
covers 154.3012690273 m, averaging 0.5143375634 m/s. The neural actor covers
149.1968653738 m, averaging 0.4973228846 m/s. Both finish all 15,000 control
intervals without a sampled fall or numerical error. The teacher's advantage
is 0.0170146788 m/s over this horizon. This uses net endpoint distance, with no
new heading or tracking penalty. It overturns the minute-long ranking for this
longer task and makes the teacher the stronger baseline for subsequent sustained
speed work. It does not establish an infinite-horizon ranking or physical maximum.

`independent-command-forecast-study-v1.json` records this update and
`independent-command-forecast-study-evidence-v1.json` preserves the exact code,
tests, executables, command plans, captures, training recipes, model weights,
physical-error reports, completed PPO checkpoints and evaluation results.
`sustained300-speed-comparison-v1.json` supplies the now-completed longer
comparison, superseding the pending rows in the immediately preceding aggregate.
The earlier `reference-forecast-study-v1.json` is a historical partial snapshot;
its then-pending excitation and PPO work is superseded by the new aggregate.
