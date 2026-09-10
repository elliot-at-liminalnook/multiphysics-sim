# Matched-input fidelity comparisons

`sim_runtime::fidelity::compare` consumes captures from `run_environment` or the
same browser environment. It compares committed endpoints without advancing a
robot. `sim-web` and the worker expose this same Rust function as
`compare_environment_fidelity`; comparison work runs outside the UI thread.

An `ExecutionContext` includes the complete parsed scene and configuration, task,
and seed. This covers robot/world definitions, construction options, actuator
reductions and boundaries, initial conditions, controller sources/parameters,
solver settings, clocks, diagnostics and horizon. It records resolved serialized
settings, including legacy defaults; use `RobotDocument` inspection for original
CAD omissions and provenance. It is not yet the strict bound physical contract,
and legacy models do not automatically gain a binding. Version 2 forecast models
now check a compact physics/source profile separately; see `physics-bound-forecasts.md`.

Every difference needs an exact JSON pointer, before/after values, and explanation
in `ComparisonPlan.changes`. Missing fields and null have distinct representations.
Unknown, mismatched, duplicate and unused declarations fail. There are no implicit
exclusions for timing or diagnostic settings. Tasks, seeds, controller command
interpretation, command clocks and episode durations must remain identical even
if a caller declares a change. Physics-step changes require a corresponding horizon
step-count change and matching task endpoints. Approximation settings, including
changes to physics properties, remain explicit in the full report.

Held commands are reconstructed from recorded events with the existing causal
controller-action adapter. They must agree throughout the compared interval;
changes hidden within an observation interval are rejected. Failed runs retain
only their observed matching prefix. If a failed action commits additional physics
steps before its next task endpoint, the report distinguishes observed duration
from actual committed simulation duration. A failure before the first transition
can compare only the initial frame. No partial or failed episode receives an
eligible score, even when its initial trajectory errors are zero.

The report contains per-channel maximum absolute error, RMS, worst timestamp,
sample count, SI unit, explicit tolerance and result for:

- Joint positions and velocities, using the runtime's coordinate names and units.
- Named link world positions, linear/angular velocities, and rotation-matrix entries.
- Authored held IMU outputs, sample clocks and availability.
- Summed contact forces by named link, allowing contact-point counts to change.

Link matching uses names; joint metadata must retain identical indices, names and
units. Rotation entries use dimensionless error, not a geodesic angular error.
Contact sums are not a comparison of individual contact points, moments, impulse
history or stick/slip state. Hidden motor/servo state, continuous trajectories,
and finite-interval accelerations are not compared by this version. Changed
sensor availability/topology can make channel sets incomparable; this is reported
as an error rather than inventing missing measurements.

Full final task transitions, eligible undiscounted score and reward rate are
retained separately from trajectory errors. `categorical_outcomes_match` compares
completion, termination, termination reasons and failure messages; it does not
claim equality of continuous task values. Both numeric task outcomes remain in the
report. There is deliberately no automatic overall fidelity approval. A small
endpoint error does not establish timestep convergence or continuous-time safety.

Timing reports simulated seconds per wall second. Each side must declare capture,
source, executable, host and timing-scope references. These are host attestations,
not authenticated provenance. Wall costs from differently instrumented hosts
remain individually labeled; the API does not silently turn them into a speedup
claim. An evidence manifest must pin the referenced artifacts independently.

## Native and browser usage

```sh
cargo run --locked --release -p sim-runtime --example compare_environment_fidelity -- \
  reference.json candidate.json plan.json fresh-report.json
```

The CLI writes a report even when error tolerances fail; its successful exit means
the inputs were comparable and the report was produced. Callers must inspect the
named results and both outcomes before accepting a profile. Invalid inputs or
context mismatches return an error before writing the report. Existing output
files are never overwritten.

Worker request:

```javascript
{type: 'compare_environment_fidelity', reference, candidate, plan}
```

`prepare_fidelity_parity_plan.mjs` creates a same-settings native/WASM portability
plan with explicit absolute `1e-7` limits in every supported SI unit. These are
software-portability budgets, not physical-accuracy budgets. It pins the native
capture and both executables by SHA-256; the future browser capture is referenced
by path until independently archived. `web/tests/environment.mjs` accepts
`FIDELITY_PLAN_PATH`, `FIDELITY_CAPTURE_PATH`, and `FIDELITY_REPORT_PATH`, exercises
the comparator while an environment is loaded, and checks that valid and rejected
comparisons leave its frame unchanged. The native CLI processes the exact same
captures/plan; `check_fidelity_report_parity.mjs` compares the entire report.

CI exercises the CAD wheeled robot and quadruped through this path alongside
controller forecasting. Unit tests also compare actual wheeled trajectories with
halved physics steps, reject unmatched contexts/commands, measure known injected
errors, and check incomplete and failed-run scoring. These API checks do not
qualify either morphology's locomotion accuracy, browser realtime performance,
or a learned predictor.

## Recorded acceptance

`fidelity-evidence-v1.json` archives the inputs, release executables, isolated
browser bundle, captures, plans, reports and logs from `runs/shared-fidelity-v1`.
Fifteen focused tests passed. Browser checks rejected an undeclared timestep change
without changing the loaded environment; replay and reset remained exact.

| Case | Compared interval | Largest selected native/browser error |
|---|---:|---|
| Wheeled velocity controller | 30 ms, 11 frames | `4.34e-19 rad/s` held IMU angular velocity |
| Quadruped recorded controller | 100 ms, 6 frames | `3.34e-11 N` aggregate contact force |

All measured channels passed the explicit portability budgets. Complete native
and WASM comparison reports also agreed within the separate report-serialization
budget. Native and browser captures were generated with the same final runtime
code; the comparator CLI additionally buffers its large report writes. The live
user browser bundle was not modified.

A separate wheeled experiment halved the physics step from 3 ms to 1.5 ms while
retaining the same 3 ms commands and 30 ms episode. With exact-match diagnostic
limits (zero tolerances), it reported up to `24.525 µm` world-position difference,
`1.78e-9 rad` joint-position difference, and `2.85e-6 rad/s` joint-velocity
difference. These quantify timestep sensitivity; they do not establish convergence.
This wheeled case has contact disabled. The previously recorded contact-convergence
limitations remain unresolved.

Observed simulation/wall ratios were about `1.17` native and `0.61` browser for
the wheeled case, and `0.097` native and `0.081` browser for the quadruped. They
include different declared observation/transport costs on a shared host and are
not controlled performance comparisons. Browser walking realtime qualification
remains open.
