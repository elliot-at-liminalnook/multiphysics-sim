# Trusted baseline and predictive controller

This work follows the user's four tasks: establish a trusted fastest baseline;
validate the physical model and measure influential uncertainties on hardware;
train and evaluate an action-conditioned predictive controller; and demonstrate
the same APIs on the wheeled robot. Work takes place on `main` in
`/Users/elliot/physics-simulator`.

| Task | Required evidence | Current state |
|---|---|---|
| Baseline | Reproduce the preserved 300 s gait; report sustained net speed and sampled falls. Measure acceleration, braking, left/right turns and reversal from recorded WASD-equivalent commands. Compare matched physical-time inputs across timesteps. | Complete 300 s reproduction exactly matches historical distance/speed: 172.069173 m and 0.5735639103 m/s, no sampled fall. WASD and three 20 s timestep cases completed. Fresh coarser-step 300 s run gives 0.5621811105 m/s, 1.98% lower; full finer-step comparison is running. |
| Physical model | Trace actuator/transmission limits, contacts and sensors to CAD; rank influential uncertain parameters using explicit experimental perturbations, then obtain corresponding hardware measurements and promote accepted values through CAD. | Effective actuator derivations match CAD; all eight transmission relations hold across 4,501 captured samples. Winning recipe remains uncalibrated, omits inter-link contact, and declares no sensors. Its observations are ideal simulator values. Hardware availability/interface requested from user. |
| Predictive controller | Train with commands and observed dynamics, predict future trajectories alongside actions, and compare against the original gait on sustained speed, command response and recovery using reserved evaluation cases. | Three PPO updates completed: best 20 s training speed 0.566927 m/s (+0.59%). The earlier 0.564950 m/s actor remains frozen for evaluation; both held-out initial-state recovery cases pass with about 0.23% higher speed. The 90 s command protocol completes without a sampled fall but shows more forward heading drift. Full sustained comparison remains running. |
| Second morphology | Use the same observation, action, prediction and experiment contracts on the wheeled robot, including sustained locomotion and predictive evaluation. | Complete 10 s forward run, 490 live forecast queries and exact 501-frame experiment/checkpoint replay passed. Learned forecast is tested on a separate forward episode and exposes generalization failure. Evidence in `../wheeled-robot/predictive-baseline/`. |

Speed remains net displacement divided by the complete requested duration, with
no eligible score for failed or incomplete episodes. Slippage is diagnostic;
no separate slip, heading or gait-style penalty is introduced. Command response
and recovery are measured separately so a high straight-line speed cannot conceal
poor control behavior.

The original 0.5735639103 m/s result used a 0.15625 ms physics step for 300 s.
Its own coarser-step result differs by about 2% and has substantial endpoint
divergence. Neither result establishes numerical convergence or hardware accuracy.
The new sweep holds robot, controller, task, seed and physical command times fixed;
only timestep and corresponding integer step indices change.

`benchmark_environment` streams ordinary typed environment transitions, their
held actions, metadata and a replay recording. It uses the same `EmbeddedEnvironment`
as the browser, supports an explicit prefix and cancellation file, and provides
wall-clock progress. Prefix/cancelled results cannot acquire an eligible speed.
The source recording's full horizon remains unchanged during prefix inspection.

```sh
cargo build --locked --release -p sim-runtime --example benchmark_environment
target/release/examples/benchmark_environment \
  examples/full-robot/trusted-baseline/fastest573.input.json \
  runs/fresh-baseline-output 300 runs/baseline.cancel
```

Initial execution records live under `runs/trusted-baseline-v1/`. The pinned
binary/source hashes, run handles, cancellation paths and startup checks are in
`execution.json`; a run is complete only when its terminal summary and process
status prove it. Final evidence must be retained before making acceptance claims.

The first 20 s checks completed without sampled falls at 0.3125, 0.15625 and
0.078125 ms. Net speeds are respectively 0.5636470, 0.5636030 and 0.5636007 m/s;
relative to 0.15625 ms, endpoints differ by 0.277 m and 0.133 m. These are prefix
diagnostics, not sustained speed or convergence acceptance. The preserved CAD
artifact hash matches the winning export's declared CAD hash. See
`../full-robot/trusted-baseline/initial-checks.json`.

`prepare_command_benchmark.mjs` uses the production viewer's keyboard mapping and
heartbeat, retaining the winning robot, controller, fixed gains, seed and physics
step. The 90 s protocol is versioned in `command-protocol.json`: forward, brake,
restart, left/right while walking, reverse, and stop. Three read-only body-axis
observations support projected-heading measurement. Its initial actions and all
pre-existing physical observations match the baseline exactly in a 0.1 s check.
The complete 90 s command-response run finished without sampled termination.
`../full-robot/trusted-baseline/command-response-result.json` preserves the
measurements and original capture physics identity. Its 4,500 executed actions
match the preserved command schedule. The shared `motion_response` API binds
typed observation sources, handles consistent aliases and measures nonuniform
sample windows without changing the objective.

The first forward stage covered 11.272 m in 20 s (0.563603 m/s). Releasing the
keys produced 0.151 m net displacement over the 10 s braking stage and a final
speed of 0.0000153 m/s. W+A changed sampled unwrapped heading by +11.76 degrees
over 10 s; W+D still changed it by +1.41 degrees. The preset's positive steering
trim means W+D requests a small positive yaw rate, not a negative one. S produced
-318.02 degrees of sampled unwrapped rotation over 20 s and only 1.008 m net
displacement despite 0.463 m/s mean sampled speed. These are measured shortcomings
for command-response training, not extra rewards or failure penalties.

Heading is measured from sampled body orientation. Unwrapping assumes less than
pi true rotation between observations; the largest observed increment is small,
but that alone cannot rule out hidden turns. Integrating 20 ms angular-velocity
samples gives substantially different changes during walking. The one-second
`motion-sampling-audit.json` replays the same trajectory at every physics step:
integration error falls from 0.035857 rad at 20 ms sampling to 0.000029 rad at
0.15625 ms sampling, with exactly matching environment endpoint position/velocity.
This identifies substantial sampling/quadrature error; it does not establish
continuous-time accuracy. The largest finite-interval acceleration rises from
6.35 to 40.38 m/s² as sampling gets finer. Report its interval explicitly.

The physical declaration audit is in `physical-audit.json` beside the baseline.
All 12 effective actuator parameter sets match their CAD ratings/static gain
derivation within 1e-12. This does not validate those estimates on hardware.
`transmission-motion-audit.json` checks the shared original constraint equations
on all 4,501 frames of the 90 s capture: eight declared belt/worm relations have
maximum position residual 1.11e-16 rad and velocity residual 7.11e-15 rad/s.
A synthetic 0.01 rad driver perturbation produces the expected 0.01 rad residual.
This verifies sampled enforcement of the ideal CAD ratios, not hardware ratio,
backlash, efficiency or torque accuracy.
`geometry-motion-summary.json` and its compressed full report inspect all 4,501
poses through shared contact geometry, including inter-link pairs omitted from
the fast dynamics. Maximum sampled overlap is 0.189 mm, on the +Y sector gear /
hip shaft pair. The other reported pairs are the -X hip shaft/chassis and +X
sector gear/hip shaft. No reported pair joins different legs. This uses compiled
surface samples, SDFs and neighbour exclusions; it is not exact CAD geometry or
continuous-time clearance certification. The audit does not alter gait scoring.
Thirty-one of 32 moving joints have no declared travel limits, and the robot has
no declared sensors. The sensitivity plan perturbs active torque (+/-10%), servo
stiffness (+/-15%), no-load speed (+/-10%, exploratory range) and floor friction
(+/-20%). `prepare_physical_sensitivity` reuses the shared Scene input-preservation
API so floor changes carry original-value receipts; configuration changes carry
an explicit manifest. All eight matched 20 s prefix runs and the two active
material-friction cases completed on the pinned baseline binary. They do not
replace full-horizon evaluation.

Verification: three analytic response tests pass, including wrap handling,
nonuniform braking, closed paths, reordered observations and source aliases.
The analyzer rejects a deliberately wrong command specification and a missing
transition. Sensitivity preparation preserves action schedule, policy, seed and
time grid and passes robot receipt readback. Predictive training uses
full motion/actuator-target captures; the diagnostic transition stream does not
contain all link poses and must not be padded with invented state. The optional
`benchmark_environment --motion` capture now supplies these through shared
`MotionSnapshot`; all 4,501 quadruped transition rows remain byte-identical to
the earlier WASD run.

## Captures and initial prediction models

`sustained-result.json` retains full baseline outcome and source identity.
`sensitivity-initial-result.json` uses distance divided by the observed 20 s,
not the task's original 300 s horizon. Torque -10% gives 0.525294 m/s (6.8%
lower), not 0.035 m/s: the latter is the unfinished task's horizon-normalized
diagnostic. The first verbal report used that denominator incorrectly.
`benchmark_environment` now names both diagnostics explicitly.

The friction audit found `world.floor_friction` is not read by this articulated
contact path. It resolves material/world table entries through
`PhysicalModel::friction_between`; regularized Coulomb uses their kinetic value.
The unchanged world-field tests do not establish friction robustness. Two
additional material-table perturbations completed from
`material-sensitivity-plan.json`: -20% gives 0.578085 m/s and +20% gives
0.553528 m/s over matched 20 s prefixes. Results and receipts are retained in
`material-sensitivity-result.json`; the preserved baseline remains unchanged.
Among the screened ranges, no-load speed and stall torque have the largest
adverse speed effects, followed by servo stiffness and material friction.
The no-load-speed range is exploratory, and this local ranking does not cover
coupled uncertainties or replace loaded actuator/contact measurements.

The quadruped forecaster has 309 inputs and 279 outputs: all 28 generalized
coordinates plus body translation, predicting position, velocity and finite-
interval acceleration at 20, 100 and 200 ms. Current dynamics, gravity direction,
angular velocity, ground-relative height and the complete held command sequence
are inputs. Training uses 0–70 s; validation uses 70.02–90 s with complete
history/target windows kept inside each interval. This is a chronological
development split within one episode, not independent-episode acceptance.
Normalized validation MSE: learned 0.207178, constant velocity 0.345705, constant
acceleration 0.754837. This prediction improvement does not establish better
actuator commands, speed, command response or recovery.

The compressed capture, model, recipe and reports are versioned beside the
baseline. `predictive-data.json` records the decompressed SHA-256 and verifies
transition parity. To repeat training into a fresh directory:

```sh
mkdir runs/predictive-reproduction
gzip -dc examples/full-robot/trusted-baseline/command-motion.capture.json.gz > runs/predictive-reproduction/capture.json
target/release/examples/prepare_motion_training runs/predictive-reproduction/capture.json 'Robot | Chassis and hip mounts' 70 runs/predictive-reproduction/experiment.json
target/release/examples/train_motion_forecast runs/predictive-reproduction/experiment.json runs/predictive-reproduction/model
```

These tools preserve the capture's original physics identity. The packer rejects
numerically failed captures; the trainer rejects overlapping training/validation
windows in identical captures, including byte-identical files at different paths.

Separate actuator-conditioned heads at 20, 100 and 200 ms completed training.
Their normalized validation MSEs are respectively 0.14146, 0.11100 and 0.07540,
versus constant velocity 0.62828, 0.24590 and 0.15430. These aggregate improvements
do not mean every state channel improves. Models, experiments and per-output
reports are in `../full-robot/trusted-baseline/predictive-controller-v1/`.
They use recorded actuator channel names (`*.target`), not generalized-coordinate
names. A name-based permutation binds their input columns to runtime actuator
order; 34 real samples per head retain predictions within 3.5e-13 absolute error.

The shared forecast bundle augments a residual actor with motion commands,
current dynamics and proposed-action forecasts: 519 inputs, 12 actuator outputs.
Its initially zero output preserves the existing gait exactly at startup and
through the full 20 s training horizon. Forecasts describe the baseline Rhai
proposal before neural correction, not a promise about subsequent applied neural
actions. Inputs are ideal simulation observations, not a deployable sensor policy.
The first PPO update completes without a sampled fall at 0.563406 m/s, versus
0.563603 m/s for the neutral baseline, so it is not accepted as a speed improvement.
The second reaches 0.564950 m/s (+0.24%) and is frozen before held-out evaluation;
`candidate-selection.json` identifies its exact actor and selection rule. The
third and final update reaches 0.566927 m/s (+0.59%) over the training horizon.
`training-result.json` preserves six completed stochastic rollout receipts,
all deterministic validation results and the final optimizer state. Its final
training actor is kept separately from the frozen evaluation candidate.
`recovery-cases.json` declares initial pose perturbations; the original gait
completes both 20 s cases without sampled falls at 0.563610 and 0.563404 m/s.
Those cases do not represent timed mid-gait pushes or measured hardware
disturbance distributions. The frozen learned candidate completes them at
0.564887 and 0.564715 m/s, about 0.23% higher, without sampled falls. Its full
300 s test exactly reproduces the training evaluation's first 20 s observations
and distance. The full 300 s result remains pending; prefix parity is not
sustained acceptance. See `recovery-comparison.json`.

The 90 s learned command run completes all 4,500 actions without sampled falls.
`command-comparison.json` uses the same analyzer on baseline and candidate.
Forward-start speed rises from 0.563603 to 0.564950 m/s, while sampled heading
change rises from 3.08 to 11.32 degrees. First braking-stage displacement changes
from 0.150766 to 0.152126 m. Left-turn heading change is 11.76 versus 15.20 degrees;
the D combination remains a positive yaw request and produces 1.41 versus 8.00
degrees. Reversal still rotates heavily: -318.02 versus -308.36 degrees, with
1.008 versus 1.286 m net displacement. These are measured response tradeoffs,
not proof of improved control. No heading, style or slip penalty was introduced.
The full candidate transition/action stream is preserved compressed alongside
the report, protocol receipt and frozen actor.
