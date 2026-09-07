# Less repeated work in original linkage closure

This shared-library optimization separates numeric closure values from their
display labels, prepares fixed unit scales once per immutable mechanism map,
and stops constructing unused SVD vectors in the QR path. It retains every
original equation, rank threshold, solver tolerance and physical parameter.

## What changes

`Articulated::original_closure_values` and `original_closure` share one equation
traversal. Both evaluate position, velocity, acceleration, stabilization/CFM and
certified-identity rows at the actual supplied configuration. The numeric API
returns static unit references and no allocated diagnostic names. The named
API retains its existing names, units, order and serialized structure.

`original_closure_units` supplies immutable model metadata without evaluating
kinematics. `RigidEmbedding` prepares its row scales from this metadata once.
The direct closure Jacobian no longer evaluates the entire original closure
solely to recover units. A dimension mismatch fails explicitly; each fresh
numeric equation and closure check is still evaluated during solving.

The pivoted-QR path still computes SVD singular values for the original absolute
and relative rank checks, but requests no left/right vectors because QR solves
the equations itself. The SVD solve path continues requesting both vectors.
No pose-local row deletion, approximate factor reuse or singularity avoidance
is introduced.

## Evidence and limits

The selected suites pass 72 Rust tests, with one pre-existing experimental SDF
derivative test explicitly ignored. Tests include independent analytic
slider-crank geometry/tangents/curvature, rotated floating bases, signed
transmissions, mixed spatial joints, unit/row ordering, stabilization/CFM,
global rank thresholds, toggle rejection, force balance and timestep refinement.
Recording/replay tests also pass. The metadata test checks supported rotational
transmissions alongside metric loop-closure and dimensionless alignment rows.

Complete 2.8 s robot runs at both 0.25 and 0.125 ms are checked against the frozen
contact-history binary. Exact comparison covers every sampled frame, terminal
frame, event/subdivision record, solve diagnostic, accepted contact segment and
integrated impulse. These checks preserve the parent sampled lift and numerical
behavior; they do not resolve the remaining local/simultaneous model discrepancy
or establish accepted walking, hardware accuracy, or a trained policy.

The coarse development profile changes as follows:

| Timed work | Previous | Optimized |
|---|---:|---:|
| Entire linkage mapping | 38.37 s | 26.17 s |
| Closure Jacobian | 11.67 s | 7.50 s |
| Closure factorization | 12.88 s | 9.79 s |
| SVD inside factorization | 8.26 s | 5.17 s |

These timers are nested: do not add them. Other development jobs overlap these
runs, so they identify removed work without establishing an isolated speedup.
The separate sequential ABBA benchmark uses frozen before/after binaries and
identical configuration, checking complete results after every run. Its results
and declared hardware are recorded in `closure-preparation-status.json`.

The four sequential timings are 70.71, 58.44, 58.56 and 71.06 s. Means are
70.89 s before and 58.50 s after: **1.212× faster, or 17.5% less stepping time**,
on the recorded Intel i9-9980HK host. All four complete records match exactly.
This local-motor-solve benchmark advances 0.0479 simulated seconds per wall
second, still about 20.9 times slower than realtime. Two repeats per method on
one workload do not establish general performance or training throughput.

## Maintained browser and reproduction

The browser uses the same optimized Rust code and existing controller recipes.
Validation includes the full 281-frame `robot-point-final-refresh` motion against
its native reference, exact replay/reset, invalid-request preservation and the
complete maintained viewer interaction suite. The summary requires these checks
to pass. No new controller capability or readiness claim is inferred from a
faster linkage calculation.

The full browser run passes all 281 sampled frames at the existing 1e-7
absolute-entry diagnostic, with maximum difference 7.34e-8 N. Replay/reset are
exact and invalid requests preserve state. All 21 viewer interaction checks
pass on the 19-preset bundle; the robot view was visually inspected. The motion
took 92.53 s in this overlapping development run, with a longest simulation
chunk of 1.87 s. Responsive camera/UI controls do not establish realtime physics
or controller latency.

```sh
node examples/full-robot/prepare_closure_preparation.mjs
cargo test --locked -p sim-domain-robot --lib --test constraint_audit --test embedding --test embedded_step --test jacobian -p sim-runtime --test embedded_session
cargo build --locked --release -p sim-runtime --example integrate_embedding
target/release/examples/integrate_embedding runs/full-robot/learning/point-feedback/scene.json runs/full-robot/learning/closure-preparation/base.config.json > runs/full-robot/learning/closure-preparation/base.execution.json
```

Repeat for `refined`. Frozen source/binary are under
`runs/interactive/closure-preparation`. After other heavy jobs finish, run:

```sh
node examples/interactive/benchmark_exact_runtime.mjs runs/full-robot/learning/closure-preparation/benchmark runs/interactive/contact-history/native-runner runs/interactive/closure-preparation/native-runner runs/full-robot/learning/point-feedback/scene.json runs/full-robot/learning/closure-preparation/base.config.json runs/full-robot/learning/contact-history/base.execution.json
```

The benchmark tool is reusable for other frozen embedded-runtime comparisons;
it accepts input paths and always requires exact physical/numerical sections.
Rebuild/test/package the WASM viewer, then run
`summarize_closure_preparation.mjs` to recheck the captured evidence and hashes.
The shareable archive is
`runs/interactive/robot-lab-closure-preparation-2026-09-07.zip`.
The installed viewer and unsaved CAD session are preserved.

Further work must reduce repeated pose-dependent mechanical work and establish
task-specific integration accuracy while completing stepping, deployable
observations, learning and controller-routed interaction. This optimization
preserves the current mathematical model; it does not certify that model as
adequate for the full walking task.

A larger candidate is a reusable analytic slider-crank/transmission coordinate
mapping, when the authored topology and joint frames can be structurally
certified to fit it. It must preserve the internal link poses, tangent and
curvature (hence inertial effects), reject wrong branches/toggles, and retain
original position/velocity/acceleration closure diagnostics. First audit whether
the actual CAD mechanisms meet those assumptions; do not infer the mapping from
names or a single pose. Compare to the existing iterative chart and independent
analytic examples before using it in the full robot. No speedup or applicability
claim for this candidate has been established yet.
