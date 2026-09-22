# Shared force calculation with contiguous torque outputs

The articulated force kernel now returns a private complete result with flat
needed/passive joint torque arrays and compact joint wrenches. Public evaluations
materialize the same owned per-joint diagnostics. Prepared embedded dynamics
consumes flat torques directly and borrows its existing kinematics, avoiding
per-joint vectors and diagnostic pose copies that it immediately discarded.

There is one force calculation. Cable forces, contact forces/history, original
constraint reactions, joint friction/stops/springs, base wrenches, modal forces
and all arithmetic order are retained. No physical recipe, timestep, tolerance,
acceptance gate, controller or model property changes.

All 188 robot library/integration tests pass. One preexisting experimental hybrid
SDF derivative promotion test remains ignored because its independent audit
fails; it is not counted as a pass. Coverage includes analytic pendulum inverse
loads, energy, contact/friction, transmissions, compliant/flexible reactions,
closure, Jacobians, motor coupling and discrete controller clocks. See tests.log
and tests-summary.json. Performance results and browser verification are recorded
in the raw paired-run and qualification files as they complete.

## Measured results

| Native route | Wall seconds for 3 s | Speedup vs preserved binary |
| --- | ---: | ---: |
| Preserved joint-axis build | 23.862128341 | 1.00000 |
| Contiguous force output | 22.967648675 | 1.03895 |
| Force output + prior analytic motion | 20.928729663 | 1.14016 |

The selected route uses 3.75% less wall time. All 151 saved physical frames and
all task transitions are exactly equal; profiling preserves both candidate
trajectories exactly. Every retained motor-solver counter and interval diagnostic
also matches the preceding build. The existing analytic option has only roundoff
physical differences but remains unselected. Neither route clears the unchanged
1.2x promotion gate, realtime throughput, or p95 policy latency.

The saved diagnostic profile has 2.598 s force evaluation versus the preceding
3.240 s, and 6.480 s dynamics preparation versus 7.624 s. These are historical
profile comparisons with nested timers, not additive or unprofiled acceptance
measurements. All solve counts are identical.

The fresh SIMD/LTO browser pair covers both joint-axis and force-output storage:
preserved bundle 27.025395 s, current 26.637505 s (1.01456x). This small single-pair
change is not a robust browser speedup estimate. Current throughput is 0.11262x
and p95 policy latency 217.335 ms; the preserved p95 is 217.105 ms. The analytic
experiment takes 23.343490 s (0.12852x), p95 187.025 ms. All timing gates fail.
No rendering work is included in those worker measurements.

All old/current worker physical fields and task transitions agree exactly.
Native/worker comparisons include reset and all 151 full task transitions, with
every numeric field within 1e-7. The actual rendered W/A/S/D/stop test passes
10 actions and 11 frames without page or worker errors. Its screenshot was
inspected; the 0.11x physics benchmark label and 0.10x live display are distinct.
The local bundle is served at http://127.0.0.1:4192. Previous bundles are retained.

Build/source/input/binary/module hashes were verified. Raw executables, WASM,
source snapshots, build manifests, profiles, timing and parity receipts, and
screenshots are retained. Tests and all timed runs are complete. The selected
physical recipe remains warm-probes/config.json. Known timestep-convergence and
hardware-calibration limitations remain; no maximum-speed claim is supported.
