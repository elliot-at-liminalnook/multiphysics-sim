# Physical correction units for the local motor solve

The opt-in `ImplicitStepConfig.auxiliary_endpoint_correction_scale` measures
inner Newton corrections in physical endpoint-state coordinates. It requires
both auxiliary condensation and rate unknowns. Defaults and the simultaneous
reference path remain unchanged. This is a numerical experiment, not an
accepted training model, controller milestone, or realtime result.

## Why the units matter

For rate unknowns `u`, the endpoint is `x_new = x_old + h*u`. A correction
`delta_u` changes the physical state by `h*delta_u`. The former correction scale
was `1 + abs(u)`. The optional scale is `(1 + abs(x_new))/h`, expressing the
existing endpoint-state convention in rate coordinates. This still uses the
solver's scalar relative tolerance; it does not supply calibrated per-state
physical uncertainty or task tolerances.

The original component equations, numerical derivative perturbations, absolute
and relative residual bounds, outer mechanical checks, and final original
auxiliary-residual check are unchanged. The inner residual bounds remain 100
times tighter than the outer bounds. Correction acceptance changes explicitly;
this is not a bitwise-equivalent assembly optimization.

Shared numerical Newton APIs now accept a correction scale for ordinary and
colored probes. Existing entry points retain their previous scale. Failure
audits, enabled only in the configured time window, report the last two inner
iterations and convert corrections back to endpoint units. Unknown adapters
still receive ordinary derivative probes.

## Evidence

The independent nonlinear test solves `x*x = 2` in rate coordinates at steps
from 10 ms to 1 ns and checks the physical root and unchanged raw residual
bounds. Both ordinary and colored APIs reject `x*x + 1 = 0` despite an enormous
correction scale. The independent loaded motor/circuit comparison includes the
new option. Invalid option combinations fail, and recording/replay preserves
the selected coordinates. The selected suites pass 51 tests: solver coloring
5, convergence 13, embedded motor 15, embedded step 9, session 9.

A 30 ms robot prefix, reported every 0.125 ms, isolates an early failure. At
21.499468 ms, a 0.5315 microsecond trial repeatedly requests a rotor-speed
correction of approximately 2.56e-16 rad/s against a bound of 1.33e-16 rad/s.
The endpoint scale removes that rejection. At 22 ms another inner solve still
reaches its iteration cap with an unresolved last correction, so it correctly
rejects and subdivides. Both prefixes advance all 240 requested steps; their
exit status is 1 solely because the full motion program intentionally remains
unfinished. The first current difference, 68.8 microamps at 21.5 ms, coincides
with the changed subdivision. Small equation residuals alone do not prove a
state is converged.

| Full 2.8 s run | 0.25 ms | 0.125 ms |
|---|---:|---:|
| Rejected trials, previous → endpoint scale | 2 → 0 | 3 → 2 |
| Maximum foot difference from previous local solve | 8.75e-11 m | 7.46e-10 m |
| Maximum current difference from previous local solve | 1.84e-8 A | 7.26e-7 A |
| Maximum contact impulse difference from previous local solve | 1.97e-8 Ns | 2.84e-7 Ns |
| Maximum foot difference from simultaneous reference | 0.406 mm | 4.36e-8 m |
| Maximum current difference from simultaneous reference | 0.164 A | 0.0410 A |
| Sampled qualified lift span | 200 ms | 210 ms |

Both runs complete, satisfy every original closure check and retain the sampled
lift gate. The remaining local/simultaneous discrepancy is not fixed. The base
run still has one sampled contact-pair disagreement and a guard-count
disagreement with that reference; the refined run has matching counts and
sampled pairs. Current/impulse/event timing differences also prevent claiming
strict equivalence to the parent local solve at the finer timestep. These
numerical comparison limits are diagnostics, not hardware-derived task margins.

Development wall times were 95.69 and 152.09 seconds. Other simulations, builds,
and browser checks overlapped; do not infer a speedup from these timings. No
isolated benchmark or realtime claim is made. Removing a few rejected trials
does not eliminate the dominant repeated mechanical/component work.

The updated base profile attributes 39.3 s to closure mapping, 18.4 s to
mechanical dynamics preparation, 16.2 s to contact history, and 10.8 s to
component equations. Closure mapping includes 13.1 s of closure factorization
(8.43 s in SVD). Newton residual/assembly timers contain nested work and must
not be added to these totals. This points toward reducing repeated mechanical
endpoint preparation as the next performance investigation; eliminating all
remaining motor iterations alone would leave substantial mechanical cost.
These are diagnostic timings from the overlapping development run.

## Browser and reproduction

The maintained `pendulum-condensed` fixture enables the option explicitly.
Eleven native/WASM frames agree within 1.74e-18; replay, reset, invalid requests
and changed-recording rejection pass. The full robot preset retains its prior
validated configuration. No new full-robot portability claim follows from this
small fixture. Browser controls and lifecycle are exercised by the full viewer
regression; its report is required by the summary script.
All 21 viewer checks pass on the 19-preset bundle. The final-refresh robot view
was visually inspected. A shareable archive is retained as
`runs/interactive/robot-lab-endpoint-correction-2026-09-07.zip`; the installed
viewer and unsaved CAD session were not reloaded.

```sh
node examples/full-robot/prepare_endpoint_correction.mjs
cargo test --locked -p sim-solve --test coloring --test convergence -p sim-domain-robot --test embedded_motor --test embedded_step -p sim-runtime --test embedded_session
cargo build --locked --release -p sim-runtime --example integrate_embedding --example compare_embedding --example evaluate_lift
target/release/examples/integrate_embedding runs/full-robot/learning/point-feedback/scene.json runs/full-robot/learning/endpoint-correction/base.config.json > runs/full-robot/learning/endpoint-correction/base.execution.json
```

Repeat for `refined` and the two prepared prefixes. Compare complete captures
against the matching auxiliary-coloring and final-refresh captures with
`compare_embedding`, and run `evaluate_lift` using the existing forward-slow
requirements. `summarize_endpoint_correction.mjs` checks complete runs, closure,
original auxiliary residuals, prefix termination, and browser evidence and
hashes the inputs/outputs. Frozen source and binary are retained in
`runs/interactive/endpoint-correction`; versioned CAD and preparation scripts
remain the durable reconstruction path.

Next decisions should target the remaining expensive inner/component work and
task-relevant timestep accuracy. This correction-scale experiment alone does
not justify further tolerance relaxation, a physical travel limit, or claiming
walking/learning readiness.
