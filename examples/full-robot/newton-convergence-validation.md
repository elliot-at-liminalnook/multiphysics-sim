# Finishing a contracting modified-Newton tail

The sample-reuse experiment exposed a subdivision difference immediately after
a motor backlash engagement at 1.519829312 s. The reused-matrix trajectory took
two short remainder steps where the reference accepted one. This eventually
produced a 0.041 A sampled current difference despite only 44 nm of foot motion
difference in the full refined run. See `sample-reuse-validation.md` for the
unpromoted performance experiment and its strict comparison failures.

## What the audit establishes

`sim-solve` now exposes `solve_newton_numeric_cached_audited`, using exactly the
same numerical derivative and solve path as the existing helper. The embedding
accepts an optional `newton_audit_window_s` and appends the last four iteration
summaries to a failed trial's diagnostic reason. The window is validated and
defaults to absent. Audits do not control physical state or acceptance.

The 1.52 s audit replay preserves all 153 sampled physical frames exactly.
It intentionally stops before the full motion finishes; the resulting horizon
error remains recorded. At the recovered failure, the last iterations are:

| Iteration | Scaled residual norm | Largest correction / ordinary bound | Matrix |
|---|---:|---:|---|
| 37 | 4.72238e-8 | 4.39118 | Reused |
| 38 | 9.37547e-9 | 0.878430 | Reused |
| 39 | 1.86130e-9 | 0.170502 | Reused |
| 40 | 3.69537e-10 | Not calculated: iteration cap reached | Reused |

An ordinary correction passes when the ratio is at most one. A reused matrix
requires a hundredfold tighter correction (or the existing floating-point
floor), because its correction estimate is less reliable. This solve is still
contracting rapidly; it runs out of iterations while waiting for that stricter
test. The motor-mode jump itself already discarded the previous matrix. The
reused matrix in this table was constructed within the subsequent solve.

## Opt-in experiment

`NewtonConfig.refresh_before_iteration_limit` reserves the final two iterations
for fresh matrices. It changes neither the iteration cap nor the raw residual,
correction, event, closure, or contact tolerances. The default remains false.
It can add matrix construction work, so it needs complete-run cost measurements.

A scalar equation with a known root reproduces an exhausted modified-Newton
tail. With refresh enabled it reaches that root inside the same iteration cap
and satisfies the unchanged absolute residual bound. A discontinuous equation
with no root still fails with the option enabled. Audit tests compare states,
residual-call counts, solver outcomes and cache reuse with recording on/off.
The solver suite (13 tests), motor suite (13), and embedded-step suite (8) pass.

The focused robot test removes the rejection at 1.519829312 s and returns to
two accepted segments/seven continuous trials for that nominal interval. Maximum
sampled current difference against the reference through 1.52 s falls from
0.000445900 A to 9.11189e-10 A. The focused capture still reports the deliberate
incomplete-motion horizon.

The complete 2.8 s native runs also pass the previous strict numerical gates:

| Step | Maximum current difference | Maximum foot difference | Maximum contact impulse difference | Maximum ordered event-time difference |
|---|---:|---:|---:|---:|
| 0.25 ms | 2.74129e-9 A | 5.53867e-12 m | 1.41719e-9 N·s | 1.82401e-9 s |
| 0.125 ms | 9.11189e-10 A | 1.23762e-12 m | 1.84150e-10 N·s | 4.27782e-10 s |

Event counts and sampled contact pairs match. The coarser run's physical frames
are exactly unchanged from sample reuse before final refresh. Both sampled lift
checks pass (200/210 ms), while existing landing, balance, sensing and timestep
accuracy limitations remain. Strict numerical equivalence to this reference is
not evidence of hardware accuracy or accepted walking.

The refined late-current discrepancy is therefore resolved by the guarded final
refresh. One earlier recovered Newton failure remains at 0.022 s, also with a
40-iteration cap. It does not produce a strict trajectory discrepancy against
the reference; it is retained in diagnostics rather than hidden.

The refined candidate constructs 7,353 Jacobians versus 11,646 in the reference;
successful-trial endpoint evaluations are 724,793 versus 1,002,270. Candidate
wall times are 127.95/205.30 s, measured during concurrent development work.
They are not a controlled timing pair and do not establish another speedup.
Both runs remain far below realtime and learning-throughput targets.

Prepare the prior sample-reuse inputs and audit replay, then run:

```sh
node examples/full-robot/prepare_final_refresh.mjs
```

This derives effective tolerances from the recorded audit run. It writes the
focused 1.52 s config and complete 2.8 s configs at 0.25 and 0.125 ms. Keep the
focused horizon error explicit; it is not a passed full-motion test. Compare
complete trajectories, event/subdivision counts, currents, impulses, closure,
task outcomes, and total runtime before promotion. Browser/controller defaults
remain unchanged during this investigation.

## Browser delivery

All 281 sampled native/WASM frames pass the strict 1e-7 absolute-entry check;
maximum difference is 7.33633e-8 N in a reported force component. Replay/reset
are exact, invalid inputs preserve state, and changed replay configuration is
rejected without mutation. Twenty UI checks pass; the rendered robot, camera,
controller gain and target/actual telemetry view was inspected.

Browser stepping takes 143.12 s for 2.8 simulated seconds; the maximum worker
chunk is 2.324 s. Main-thread heartbeats continue during execution. These are
separate portability and responsiveness observations, not realtime acceptance.
The source catalog labels this preset as numerically checked but still an
experimental controller with landing/calibration limitations. Solver defaults
remain unchanged; the preset explicitly enables the validated options.

The separate bundle is `runs/interactive/final-refresh/viewer`, with a source
snapshot and the native executable preserved alongside the experiment evidence.
The previously installed point-feedback viewer remains untouched. The shareable
archive is `runs/interactive/robot-lab-final-refresh-2026-09-07.zip`.
