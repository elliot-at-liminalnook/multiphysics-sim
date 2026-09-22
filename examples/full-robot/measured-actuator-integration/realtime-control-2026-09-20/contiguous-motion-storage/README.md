# Contiguous rigid motion storage

CPU sampling (`../native-cpu-sample/`) showed substantial allocation costs.
The shared rigid motion map now stores all link columns in one buffer with
per-link ranges, replacing a growing vector for every link. The compiled
topology determines the ranges; inherited and joint columns retain their exact
order. No column, weak coupling, force, constraint or state is removed. Full
inertia and original closure Jacobians use the same shared kernel.

## Native measurements

All runs use the same three-second CAD/model recipe, controller clocks, seed,
actions, solver tolerances and acceptance gates. Compilation finished before
benchmarks started.

| Route | Wall seconds | Speedup versus preserved baseline | Simulation / wall |
| --- | ---: | ---: | ---: |
| Preserved nested storage | 27.234492 | 1.00000 | 0.11015 |
| Contiguous storage | 24.949506 | 1.09158 | 0.12024 |
| Contiguous + analytic mechanism motion | 23.533808 | 1.15725 | 0.12748 |

Storage alone reduces wall time by 8.39%; the combination by 13.59%. Neither
single paired measurement meets the existing 1.2x promotion gate, and neither
is realtime. Their p95 policy transitions are 198.37 and 191.82 ms versus the
20 ms target. Analytic motion therefore remains disabled in the selected recipe.

Storage alone exactly preserves every one of 151 frame fields except wall
timing, and every task transition. The combination differs only at roundoff:
2.442e-14 rad, 7.485e-15 m, 1.485e-13 A and 7.190e-11 N. Contact identities and
controller sample/application counts agree; no fall is detected. Profiling
leaves every measured route's saved frames and transitions exactly unchanged.

The numeric-path profile records 1.723 s of rigid inertia and 1.353 s of closure
Jacobian work, versus 2.850 and 2.439 s in the preceding implementation's stored
profile. Those profile timings are diagnostic historical comparisons, not the
paired throughput measurements above. Counts remain 179489 dynamics
preparations and 129594 closure mappings. Profile buckets nest.

## Verification and reproducibility

54 targeted tests pass: rigid inertia, constraint audit, embedded mechanics
and embedding. Existing independent checks cover parallel-axis inertia,
branched mixed joints, multiple free/grounded bases, inverse dynamics, kinetic
energy, projected inertia, original closure equations and invalid inputs.
No test tolerance or acceptance gate was relaxed. Full sim-web wasm32
compilation passes; native release build and source whitespace checks pass.

`source-before/`, `source-candidate/`, `protocol.json` and binary receipts preserve
the change. `analytic/` contains the explicit combined recipe. Source snapshots
have been checked against their recorded hashes. A reproducible full-frame
verifier is included:

```sh
node examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/contiguous-motion-storage/verify-parity.mjs
node examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/qualify.mjs contiguous-motion-storage/candidate contiguous-motion-storage/before
node examples/full-robot/measured-actuator-integration/realtime-control-2026-09-20/qualify.mjs contiguous-motion-storage/analytic/candidate contiguous-motion-storage/before
```

Native benchmark session 38588 completed. Timestep convergence remains
unresolved as documented in `../STATUS.md`.

## Fresh browser delivery and verification

A new SIMD/LTO worker was built with recorded source/compiler/artifact hashes,
packaged independently and served at `http://127.0.0.1:4191`. It reuses the prior
identical-profile compiler cache through a target-directory symlink; the old
standalone artifact, bundle and manifest remain preserved at port 4190.

Sequential headless Chrome worker runs, with identical three-second inputs:

| Browser bundle / route | Wall seconds | Simulation / wall | p95 policy step |
| --- | ---: | ---: | ---: |
| Preserved earlier SIMD/LTO bundle | 28.767835 | 0.10428 | 230.54 ms |
| Fresh bundle, selected numeric recipe | 26.958385 | 0.11128 | 216.26 ms |
| Fresh bundle, analytic motion enabled | 24.971780 | 0.12014 | 201.95 ms |

The cumulative browser gains are 1.06712x and 1.15201x. These compare all exact
changes since the preceding browser build; they do not isolate contiguous
storage. Neither passes the 1.2x speedup gate or realtime requirements. The
selected recipe remains numeric motion; the storage layout changes no physics.

Every old/new default worker frame and complete task transition matches exactly.
Native/WASM differences are roundoff-sized (largest grouped physical discrepancy
1.744e-10); servo states, commands and controller counters match exactly.
`compare-browser.mjs` checks every physical frame field and all 151 task
transition records, including reset. The native harness stores transitions
separately while the worker places them in `frame.learning`; initial comparison
failures exposed this layout and index difference, and the corrected verifier
compares all records rather than excluding them. Initial failure logs remain.
The existing native/WASM verifier also passes both candidate configurations.

Rendered W/A/S/D and stop routing passes against the actual Rust worker, with
10 actions, 11 frames and no page/worker errors; screenshots were inspected.
The viewer's stale 0.08x status was updated to the measured 0.11x three-second
browser-physics result, explicitly separating live rendering and retaining the
warning that realtime and calibration are unproven. The label-only package was
checked again with the rendered test; the physical binary is unchanged.

Receipts include `browser-comparison.json`, `candidate.comparison.json`,
`analytic/candidate.comparison.json`, complete compiler/package manifests,
the standalone WASM artifact, and `rendered/` plus `rendered-current/`.
Browser benchmark session 6684 completed. The static server may remain running;
it is not an active benchmark.
