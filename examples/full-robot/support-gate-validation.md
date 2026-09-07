# Support-aware reference execution

The earlier trajectory advanced with elapsed simulation time even when a stance
foot stopped supporting the robot. The shared Rust `control.motion_clock`
component now separates reference progress from physics time. A condition can
pause reference progress over a declared interval, require sustained recovery,
and latch a timeout. Physics and registered motor firmware continue running.

`MotionClockConfig` declares the sample period, motion duration, guarded reference
interval, qualification duration and maximum continuous pause, all in seconds.
Durations lie on the controller's sample grid. The registry exposes the same
parameters and typed reference-time/advancing/timeout signals for component
discovery. Input `condition >= 1` means ready. A 50 ms qualification at 10 ms
sampling requires six consecutive ready observations spanning five intervals.
Timeout is latched. Component state is explicit and checkpointed; repeated jumps
from the same saved state produce the same result.

The embedded diagnostic calls this reusable clock at 100 Hz using accepted
mechanical states. Pending controller updates commit with successful physics
steps; failed solver intervals discard them. A clock timeout is a deliberate
episode failure at the current physics time, with a terminal state and trace.
The complete prescribed motion must finish within the configured simulation
horizon; exhausting the horizon with unfinished reference progress also fails.
The target law is pure during Newton/event trials and is sampled by the original
servo clocks. No motor strength, gain, latency, geometry or contact parameter is
changed by this governor.

## Observation and experiment contract

`ideal_upward_floor_forces` reports named links' total world-Z floor forces,
excluding internal body contacts. It requires world -Z gravity, unique existing
links and finite results. The recipe explicitly names its observation source
`ideal_runtime_floor_force`. These are privileged simulator observations, not
CAD-authored hardware force sensors or the deployed student-policy contract.

The first experiment uses the same motor-corrected CAD export and coordinated
body-shift/-Y-foot-lift trajectory as before. During reference time 0.5–1.1 s,
each other foot (+X, +Y, -X) must carry at least 1 N. The threshold is an explicit
diagnostic choice, not a calibrated balance requirement. Recovery requires
50 ms of qualifying observations; a continuous 300 ms pause times out. The
physics horizon is 2.2 s, allowing room for pauses in the 1.6 s reference.

```sh
cargo run --locked --release -p sim-runtime --example integrate_embedding -- runs/full-robot/learning/catalog-stall-consistent.scene.json examples/full-robot/mechanical-servo-support-gated-motion.json
```

The `-refined.json` recipe halves physics steps from 0.25 to 0.125 ms while
retaining the 10 ms policy period, firmware schedule, reference and thresholds.
Raw results retain `completed: false` when the gate times out; they are not
rewritten as successful runs to pass comparison tools. `compare_embedding`
checks optional gate configuration identity, including backward compatibility
with older ungated captures. The existing wall-time-aligned `compare_motion`
explicitly rejects gated captures until phase-aware comparison is implemented.

## What this does and does not establish

The coarse run pauses at physics/reference time 0.69 s, when +Y upward force is
0.878 N. Physics continues with held motor targets. At physics time 0.99 s the
reference is still at 0.69 s, the +Y force is only 0.405 N, and the 300 ms timeout
fires. There are no accepted internal body contacts. This is a correctly reported
failed stepping attempt, not a solver convergence failure or successful walking.

The refined run makes the same decision: first pause at 0.69 s, reference held
at 0.69 s, timeout at physics time 0.99 s. Its +Y load at first pause is 0.900 N
and at timeout 0.406 N, versus 0.878/0.405 N coarse. It also has zero accepted
internal contacts. Both traces contain the same clock states/ready decisions at
100 policy samples. This establishes the failure outcome under this refinement,
not full trajectory/force equivalence or a timestep acceptance gate. Diagnostic
stepping times are 79.60/95.36 wall seconds; small concurrent checks make them
unsuitable as controlled performance benchmarks.

Pausing alone does not redistribute enough load in this pose. The next controller
experiment needs active stance/weight-transfer correction with bounded commands
and explicit actuator/tracking margins. The pause can also change reference
velocity abruptly; acceleration/jerk limiting and a physical recovery strategy
are not supplied by this clock. It is not a safety certificate.

Seven control tests pass, including analytic pause/recovery/timeout cases and
registered-component checkpoint parity. Three support tests pass, including a
known 2 N static floor load and named/duplicate-link validation. Three capture
comparison tests pass; the runtime library compiles for WASM. The CI workflow
includes motion-clock and support tests. Exact sources, recipes, traces and
refinement outcomes are recorded in `support-gate-status.json`.
