# Frozen-mechanics velocity predictor: rejected for performance

An opt-in numerical predictor proposes only reduced endpoint velocities. It
uses the shared prepared dynamics response and original component callbacks,
with frozen inertia/passive loads and a local tangent approximation. Its
approximate state cannot be returned as a physical step. The unchanged exact
coupled solve starts from the original physical seed and validates the accepted
endpoint. Failed prediction falls back to the original guess; failed correction
retries cold. Failed intervals leave the caller's workspace unchanged.

`mechanical_predictor` defaults false. No selected recipe or tolerance changed.
The reduced force-response method shares the original factorized dynamics solve.
Only numerical matrices persist between steps, with time, step, layout and
contact invalidation. This is not a reduced physical model.

## Results

Three simulated seconds, same CAD, model, 400 Hz FPGA controller, seed, 150
actions and 6400 Hz BE recipe:

| Route | Wall seconds | Simulation / wall |
| --- | ---: | ---: |
| Preserved binary, prediction disabled | 27.614914 | 0.10864 |
| Current binary, prediction disabled | 27.968132 | 0.10726 |
| Current binary, prediction enabled | 34.427664 | 0.08714 |

The predictor is 0.8124x as fast as the same-binary baseline and fails the
unchanged 1.2x speedup gate. Its p95 policy transition is 306.06 ms, also above
the 20 ms realtime gate. It remains disabled and is not selected.

The physical comparison passes: maximum motor-angle difference 1.478e-8 rad,
link position 4.973e-9 m, current 1.150e-7 A, and contact force 5.208e-5 N.
All contact identities and controller sample/application counts agree; neither
run detects a fall. These are short simulated trajectory comparisons, not
hardware calibration or a proof of timestep convergence.

The profile records 19200 predictor attempts, 19020 used guesses and 180
fallbacks. Prediction costs 7.117 s. Total mapping calls increase from 129594
to 144597, dynamics preparations from 179489 to 198801 and component calls from
835596 to 1315332. The predictor does not save enough exact work to pay for
itself. Profile buckets nest and must not be summed.

## Verification and reproducibility

- `tests-final.log`: 94 tests pass across the robot library and motor, mechanical
  step and embedding suites. Independent circuit/gearbox solutions cover BE
  and both SDIRK stages, differential and algebraic current, and matrix reuse.
- `closure-tests-final.log`: expanded fixed/rotating closed-linkage test passes
  with prediction enabled/disabled and with exact/linearized Jacobian probes.
  Original position, velocity and acceleration closure checks remain enforced.
- Stiff nonlinear-load fallback, invalid-config rejection before callbacks and
  failed-interval replay are tested. The initial stiff fixture also defeated
  the ordinary solver, so its coefficient was reduced to isolate predictor
  fallback. Initial failure logs remain. The expanded linkage fixture initially
  required temporal extrapolation even when the mechanical predictor supplied
  the guess; it now asserts use of the selected predictor, preserving all
  physical assertions and original temporal-only cases.
- `wasm-check.log`: full sim-web wasm32 compilation passes. No new browser
  throughput claim is made for this rejected candidate.
- `verify-parity.mjs`: every frame field except wall timing, and every task
  transition, matches exactly before/after the disabled default path. Profiling
  also leaves all 151 frames and transitions exactly unchanged in both modes.
- `source-before/`, `source-candidate/`, `protocol.json`, both binaries and their
  SHA256 receipts preserve the tested implementations and unchanged gates.

Reproduce the physical gate comparison from the repository root:

```sh
node examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/qualify.mjs mechanical-predictor/candidate mechanical-predictor/numeric/candidate
node examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/mechanical-predictor/verify-parity.mjs
```

The benchmark session completed normally; no predictor run needs restarting.
