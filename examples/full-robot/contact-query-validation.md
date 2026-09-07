# Contact cost audit and rejected micro-optimizations

The retained change is detailed profiling and reusable exact-comparison tools.
Two contact-query optimizations preserved behavior but saved only about 2% of
complete-run time. Both were removed from the working implementation. The
previous verified analytic-position model remains the active experimental path.

## What the profile establishes

New nested timers split embedded dynamics preparation into rigid inertia,
inertia projection and force evaluation, and split contact geometry into
exclusion metadata, sample transforms and pair queries. All three instrumentation
captures match every recorded physical and numerical section of the prior full
2.8 s robot run exactly, including frames, events, solves and contact impulses.

| Work | Time in first instrumented run |
|---|---:|
| Total stepping | 46.92 s |
| Embedded dynamics preparation | 16.54 s |
| Rigid inertia | 3.22 s |
| Inertia projection | 0.60 s |
| Embedded force evaluation | 11.60 s |
| Contact geometry across all queries | 9.62 s |

These timers overlap/nest and must not be added. A second instrumented run
attributes 8.55 s to contact pair queries, 0.39 s to exclusion metadata and
0.66 s to sample transforms. This does not justify prioritizing a new inertia
algorithm or a large static metadata cache. Timers are disabled by default;
process-global profiling remains unavailable in the browser.

## Tested and removed

The combined candidate prepared pair-invariant values once per source-link /
candidate-target pair: SDF availability, exclusion-mask lookup, moving joint-band
center and inverse target rotation. It preserved sample order and every original
collision test. It also skipped distance-grid gradient construction when the
unchanged signed-distance test found no penetration. The latter was implemented
through a shared `sample_penetrating` trial API, with zero/nonfinite handling and
the original gradient fallback retained.

Both complete timestep cases (0.25 and 0.125 ms) matched the prior implementation
exactly. Trial tests covered 24,273 grid points, boundaries, an independent plane,
flat fields, signed zero, tiny negative values, NaN, moving exclusion bands,
duplicate-pair priority, loaded friction and contact transitions. Selected tests
passed 35 executions; the pre-existing hybrid SDF-derivative test stayed ignored.

The isolated ABBA benchmark on the recorded Intel i9-9980HK gave 46.473, 45.954,
45.613 and 46.642 s: means **46.557 → 45.783 s, only 1.017× faster**. All four
runs exactly repeated their physical and numerical records. The pair preparation
also adds an allocation. This modest result did not justify retaining that
complexity as the next route to useful training throughput.

A separate normal-only run took **45.600 s**, about **2.1%** below the paired
baseline mean, with exact results. It failed the predeclared 5% preliminary
threshold for spending further effort on this branch. That threshold governs
engineering priority, not physics accuracy; exact comparison was still required.
No repeated normal-only speedup is claimed. The trial API and pair changes were
removed. Frozen trial binaries and source files preserve the experiment.

## The larger-step obstacle

A fresh comparison of the current analytic-position captures at 0.25 versus
0.125 ms finds a maximum foot-marker difference of **2.570 mm at 1.45 s**, during
lowering. The policy reference times agree there. At the preceding 1.448 s policy
sample, the hip motor differs by about 0.58 degrees while the foot motor differs
by about 0.018 degrees. This is a diagnostic lead, not proof of causation or error
against a converged/hardware reference. Larger timesteps cannot be assumed safe
merely because the nonlinear solve converges.

`diagnose_timestep_tracking.mjs` records the relevant poses, motor observations,
contact identities and nominal time-window differences. The next investigation
should trace loaded hip target/angle, torque, current, backlash mode and contact
through 1.2–1.5 s at finer resolution, then evaluate integration methods or an
explicitly validated effective actuator model. Continue toward task-valid
throughput and learning rather than indefinitely optimizing small geometry loops.

## Reproduce

The retained generic tools preserve and verify complete experiments:

```sh
node examples/interactive/prepare_exact_runtime.mjs runs/full-robot/learning/contact-pairs runs/full-robot/learning/analytic-positions
node examples/interactive/verify_exact_runtime.mjs runs/full-robot/learning/contact-pairs
node examples/full-robot/diagnose_timestep_tracking.mjs
```

Use `runs/interactive/contact-pairs/native-runner` for the discarded combined
candidate and `normal-only-runner` for its simpler variant, with the point-feedback
scene and preserved base configuration. `runs/interactive/dynamics-breakdown/profile-runner`
is the instrumented baseline. The benchmark manifest hashes all inputs and repeats.
The trial sources are `pair-preparation.rs`, `normal-only-preparation.rs` and
`normal-skipping.rs`; these are archived evidence, not active library APIs.

The final retained instrumented runner and shared WASM build are checked against
the prior analytic-position capture. No controller or CAD properties changed.
Walking, accepted task margins, hardware calibration, learning and realtime
performance remain open requirements.
