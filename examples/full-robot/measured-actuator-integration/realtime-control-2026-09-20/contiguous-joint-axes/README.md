# Contiguous joint-axis storage

The shared articulated kinematics kernel now stores joint axes in one contiguous
buffer with per-joint ranges. Joint order and all arithmetic are unchanged.
Public force and diagnostic evaluations still expose the same owned vectors.
No model, equation, solver tolerance, or selected recipe changed.

Fresh native pair for three simulated seconds:

| Route | Wall seconds | Speedup over preserved binary |
| --- | ---: | ---: |
| Preserved contiguous motion columns | 25.018986694 | 1.000 |
| Contiguous joint axes | 23.895854360 | 1.047001 |
| Joint axes plus existing analytic motion option | 21.704750900 | 1.152696 |

The default path is exactly identical in all 151 saved physical frames and all
task transitions. Profiling also preserves both candidate trajectories exactly;
run `node verify-parity.mjs` here to reproduce the comparisons. Analytic motion
has only roundoff differences but remains unselected. Both candidates fail the
unchanged 1.2x promotion gate and realtime throughput/latency gates. These are
single paired measurements, not repeated-machine estimates.

110 tests pass and one existing experimental hybrid SDF derivative promotion
test remains ignored because its independent central-stencil audit fails. It is
not a passing test or a benchmark. See tests.log and tests/jacobian.rs in the
robot crate. Full sim-web wasm32 compilation passes. The browser bundle on port
4191 predates this change, so no browser speed improvement is established here.

Source snapshots, input and binary hashes, preserved executables, raw captures,
profiles, and qualification receipts are retained. Profiling reports closure
mapping 6.903 s and dynamics preparation 7.603 s; buckets may nest and cannot be
summed. Physical calibration and timestep convergence remain unproven.
