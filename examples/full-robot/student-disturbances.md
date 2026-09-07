# Physical disturbance evaluation

The maintained `robot-student-push` WASM preset applies a 0.1 N force in world
+Y at the chassis center of mass, from 7.00 to 7.20 seconds. WASD still commands
the controller. The sidebar shows scheduled times and the current force/moment;
reset and recorded-input replay repeat the physical load. No controller weights
were changed for this milestone.

`sim-domain-robot::world_load` validates a named floating base, force/moment
bounds, provenance, pulse intervals and overlapping loads. The runtime adds the
load through its existing generalized-force interface for all integration paths.
Pulse boundaries must align with nominal physics steps. Every solver trial or
subdivision inside a step sees the same held force, avoiding timestep-endpoint
ambiguity. Schedules belong to the experiment, not the robot or controller. The
force uses world axes; the moment is about the selected base's center of mass.
The bound applies to the summed scheduled disturbance, additional to separately
declared fixed loads and actuator forces.

Independent tests check linear and angular impulse on a free cube, physical
translation of a floating assembly through explicit and implicit integration,
grid refinement, overlapping limits, invalid inputs and exact replay across host
chunks. The existing environment/session tests still pass. Native/WASM agreement
for the full 24-second push sequence has maximum numeric difference
1.56e-10 under the existing absolute-plus-relative portability tolerance;
same-host replay and reset remain exact. These are implementation checks, not
hardware calibration.

The rendered 24-second forward/reverse push episode reaches **1.003×** during
active motion on the documented Intel Mac/Chrome host. Active transition p95 is
**22.75 ms**, still missing the **20 ms** target. The viewer preserves the force
schedule and keyboard inputs in the recording. This new disturbance capability
does not resolve the remaining latency requirement.

## What the robot tests establish

The current CAD-derived model has 3.976 kg of declared link mass. The initial
development set contains four hypothetical 200 ms pushes: +0.1 N and -0.1 N
sideways, +0.5 N sideways, and +0.5 N forward. All complete nine qualified steps
and pass the existing short stopping gate. The four-case manifest was written
before the latter three evaluations. These are development cases, not a final
independent test set after future training uses their results.

Stronger exploratory pushes expose failures:

| Case | Qualified steps | Final body error | Existing task gate |
|---|---:|---:|---|
| Four initial small pushes | 9 / 9 each | 0.667–0.670 mm | Pass |
| 5 N sideways, 200 ms | 8 / 9 | 0.698 mm | Fail: one support/clearance window is 180 ms; 200 ms required |
| 15 N sideways, 200 ms | 8 / 9 | 40.915 mm | Fail: shortened window and stopping error |
| 0.5 N sideways, 10 ms physics step | 8 / 9 | 0.674 mm | Fail: later step qualifies for 180 ms |
| No push, 10 ms physics step | 8 / 9 | 0.673 mm | Fail: later step |
| 0.5 N sideways, 5 ms physics step | 8 / 9 | 0.677 mm | Fail: later step |

All of these simulations finish without solver errors, and sampled inter-link
geometry audits find no overlaps. Finishing the simulation is not equivalent to
passing the task. No acceptance budget was weakened.

The 20-to-10 ms comparison changes sampled body position by up to 0.547 mm;
10-to-5 ms changes it by 0.394 mm. The same 50 Hz controller and reporting clock
are retained. This is not a demonstrated convergence order or an accepted
refined reference. The unforced refined failure shows the marginal step is not
caused solely by the new disturbance. The initial gait needs more support and
clearance margin across timesteps.

The reward audit exposes a separate training problem: the failed 15 N case
scores **23.77786**, slightly above **23.77514** for the accepted 0.1 N case.
Current survival, motor tracking/effort and upright rewards do not adequately
measure actual step qualification or body tracking against the walking reference.
Before disturbance training, improve the shared task observations and objective,
then check that the objective distinguishes these recorded successes and failures.
Keep physical acceptance independent of reward. This evaluation performs no
robustness training and makes no hardware-transfer claim.

See `student-disturbances-status.json` for source hashes, failed step reports,
impulse checks, comparison trajectories, browser checks and rendered timing.
These pushes are hypothetical engineering challenges; measured friction,
actuator response and actual disturbances remain needed for calibration.

## Reproduce

The versioned configurations retain the source student and physics profile.
`refined` and `refined-5ms` alter only physics timestep/count; the pulse and
controller times stay fixed. The unforced refined case omits the schedule.

```sh
cargo test --locked -p sim-domain-robot --test world_load
cargo test --locked -p sim-domain-robot --test embedded_step world_load_pulse
cargo test --locked -p sim-runtime --test world_load --test embedded_session --test environment
cargo build --locked --release -p sim-runtime --example run_environment --example evaluate_lift
mkdir -p runs/full-robot/learning/student-disturbances
target/release/examples/run_environment examples/full-robot/student-distillation/scene.json examples/full-robot/student-disturbances/lateral.config.json examples/full-robot/student-distillation/task.json examples/full-robot/neural-teacher/train.actions.json > runs/full-robot/learning/student-disturbances/lateral.native.json
node examples/full-robot/check_online_steps.mjs runs/full-robot/learning/student-disturbances/lateral.native.json runs/full-robot/learning/student-disturbances/lateral-acceptance
cargo build --locked --release -p sim-web --target wasm32-unknown-unknown
node web/build-viewer.mjs runs/interactive/student-disturbances/viewer --environment-only
node web/tests/environment.mjs runs/interactive/student-disturbances/viewer robot-student-push runs/full-robot/learning/student-disturbances/lateral.native.json runs/interactive/student-disturbances/lateral-parity.json
node web/tests/viewer.mjs runs/interactive/student-disturbances/viewer runs/interactive/student-disturbances/viewer-report.json
node web/tests/live_performance.mjs runs/interactive/student-disturbances/viewer robot-student-push runs/interactive/student-disturbances/live-performance.json forward-reverse
node web/serve-viewer.mjs runs/interactive/student-disturbances/viewer 4184
```

Repeat the native commands using each configuration name in the table/status
before running `summarize_student_disturbances.mjs`. Failed acceptance commands
return nonzero and still preserve the summary. The comparison baseline is the
versioned student's unforced short configuration and the same train actions;
reproduction is documented in `student-distillation.md`.

Run timing without competing jobs. The browser episode is only 24 seconds;
this probe does not replace the sustained walking/turning/terrain acceptance
required by the full goal, nor does it measure command-to-visible-response latency.
