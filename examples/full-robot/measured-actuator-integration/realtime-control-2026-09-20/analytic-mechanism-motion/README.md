# Analytic mechanism motion: measured incremental gain

The shared certified slider-crank component already computes first and second
coordinate derivatives. The new opt-in `embedding.analytic_mechanism_motion`
uses them to construct the independent-coordinate tangent and acceleration bias,
including signed independent-to-dependent transmissions. It requires analytic
positions and complete structural coverage. Unsupported mechanisms fail rather
than silently dropping constraints.

The original scaled dependent matrix still undergoes the same singular-value
rank checks, including global block thresholds. The full original Jacobian
checks every tangent direction, and every original position, velocity and
acceleration closure equation remains checked at the accepted motion. Analytic
motion avoids derivative back-solves, unused QR/SVD vectors, and the extra
curvature kinematics evaluation. It does not approximate forces or change the
physical definition, controller clocks, timestep, tolerances or acceptance gates.

## Paired native measurements

Three simulated seconds, same CAD/model, 6400 Hz BE, 400 Hz FPGA controller,
seed and 150 actions:

| Route | Wall seconds | Speedup versus same-binary numeric | Simulation / wall |
| --- | ---: | ---: | ---: |
| Preserved binary, numeric motion | 28.062812 | — | 0.10690 |
| Current binary, numeric motion | 28.069789 | 1.00000 | 0.10688 |
| Analytic motion | 25.371740 | 1.10634 | 0.11824 |
| Analytic motion + projected inertia | 26.119023 | 1.07469 | 0.11486 |

Analytic motion reduces measured wall time by 9.61%, but neither candidate
meets the unchanged 1.2x optimization gate. Neither is selected or realtime.
Their p95 policy transitions are 200.61 and 211.54 ms, respectively, versus the
20 ms realtime requirement. These are single paired measurements, not a claim
of a repeated-machine speed bound.

Both physical comparisons pass. Analytic motion differs by at most 2.442e-14
rad, 7.485e-15 m, 1.485e-13 A and 7.190e-11 N. All saved contact identities and
controller sample/application counts agree; no fall is detected. The projected
combination is similarly close. This is numerical equivalence on the recorded
trajectory, not hardware calibration or timestep convergence.

Profiled closure mapping falls from 8.774 to 6.728 s, with 129594 calls in both
runs. Numeric singular-value diagnostics remain active. The projected-inertia
combination raises inertia work to 4.530 s versus 2.872 s, exceeding the separate
0.783 s projection it saves. Buckets nest and must not be summed.

## Verification

- 95 tests pass: robot library, embedding, embedded motor and mechanical step
  suites. Expanded mechanism coverage includes both assembly branches, fixed
  and translated/rotated moving bases, both SVD/QR modes, blocked/unblocked rank
  checks, explicit rank rejection, unreachable geometry and invalid options.
- Signed transmission tests verify ratios -2, 0.125 and 7 with reordered
  independent coordinates, original closure rows and full acceleration parity.
- `wasm-check.log`: full sim-web wasm32 compilation passes. The browser bundle
  is unchanged; no browser throughput claim is made for these unselected paths.
- Every saved default-path frame field except timing, and all task transitions,
  exactly match the preserved binary. Profiling leaves all 151 frames and task
  transitions exactly unchanged in every measured route.
- Source snapshots, binary hashes, input hashes, test logs, profile receipts
  and unchanged gates are retained. Snapshot paths distinguish the source and
  test files named `embedding.rs`. An initial basename collision was recovered
  from an exact-hash-matching prior source snapshot; every preserved copy was
  checked against its originally recorded identity. The same collision in the
  preceding predictor candidate snapshot was also repaired and hash-verified.

Reproduce comparison receipts from the repository root:

```sh
node examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/qualify.mjs analytic-mechanism-motion/candidate analytic-mechanism-motion/numeric/candidate
node examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/qualify.mjs analytic-mechanism-motion/projected-inertia/candidate analytic-mechanism-motion/numeric/candidate
node examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/analytic-mechanism-motion/verify-parity.mjs
```

Benchmark sessions 2737 and 76956 completed normally. No candidate requires
restart. `warm-probes/config.json` remains the selected recipe.
