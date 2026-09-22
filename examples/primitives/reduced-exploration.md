# Reduced robot exploration

The reusable `sim_runtime::exploration` API prepares a paired detailed/reduced
experiment, captures both through `EmbeddedEnvironment`, compares them against
explicit accuracy and speed budgets, and exports a normal search journal only
after qualification. It does not contain another physics engine or optimizer.

Profiles can retain detailed motors or remove winding storage, enlarge the
physics timestep, or reduce the solver's mechanism-derivative work. The latter
uses a local mechanism tangent for derivative probes, exact residual checks,
and optional bounded Broyden updates. Its motor inner solves retain their
original derivative policy through `auxiliary_broyden_updates`. An optional
guarded velocity predictor is also available.
Rotor/gear inertia, backlash, friction, contact, full articulated
geometry, controller sampling/delay/quantization, and authored power settings
remain unchanged. CAD is never rewritten. This is a modest first reduction,
not yet a single-body walking approximation or an automatically calibrated model.

## Workflow

For the current quadruped, start with
`examples/primitives/reduced-exploration-outer-broyden.profile.json`.
It passed forward, stop and reversal comparisons at the recorded baseline,
with 1.156–1.166x stepping speedup. This preserves the full physical model and
reduces solver work. Requalify on your intended task; the recipe is not a blanket
approval of new gaits or different robots.

Build the qualification and search hosts with the same feature set so their
recorded runtime identities match:

```sh
cargo build --release --locked -p sim-runtime --features bayesian \
  --example reduced_exploration --example search_motion
target/release/examples/reduced_exploration prepare \
  YOUR_DETAILED_SPEC.json YOUR_PROFILE.json \
  NEW_DIRECTORY --fresh
RAYON_NUM_THREADS=1 target/release/examples/reduced_exploration qualify NEW_DIRECTORY
```

Preparation does not advance physics. It validates parameter/reference
materialization, declared command bounds, complete action schedules and aligned
clocks, then saves both specifications and exact declared differences. It does
not certify reachability, force allocation or gait stability. The existing
mechanism/contact primitives can screen planned motion; the runtime retains the
actual physical constraints and task checks.

Qualification advances one task interval at a time with progress on stderr.
Pass a cancellation-file path after the directory to stop between intervals.
Partial and failed captures remain evidence and cannot qualify. Output files
are never overwritten. `compare DIRECTORY` rechecks existing captures without
advancing physics (use it when no qualification report has yet been written).
The optional final `prefix-seconds` on `prepare` records an explicitly shortened
qualification case; qualification of a prefix is not qualification of a longer
episode.

The supplied profile's budgets are engineering acceptance choices, not measured
motor accuracy: maximum one degree of joint-position difference, one centimetre
of link-position difference, explicit velocity/contact/electrical budgets, and
at least 1.1x measured stepping speed. Every sampled channel must pass, including
startup. There is no lag fitting, automatic tolerance relaxation, or silent
fallback after failure. Rejected profiles and their captures must be retained.

Mechanical comparison uses the shared fidelity comparator. Additional checks
cover every motor's current, torque, internal gear speed and heating, driver
voltage/current/loss, and battery/branch observations when authored. Comparison
preserves exact robot, controller, task, seed, held inputs, horizon, source
identity and initial state; only the profile's declared edits may differ.
The winding approximation does not reproduce sub-sample electrical transients.
Captures provide sampled-endpoint validation, not continuous-time error bounds.

The winding-only profile in `reduced-exploration.profile.json` failed the initial
quadruped forward comparison despite a 1.209x speedup: it changed contact forces
and electrical response beyond the original budgets. Its captures and receipt
are retained under the quadruped evidence directory. It is an available
experimental reduction, not an approved default for that robot. The twice-step
profile retaining detailed motors also failed the same budgets. Keeping the
detailed actuator model and coarsening integration is a reduction in numerical
fidelity and cost, not a reduction in the number of physical states. Mechanism
probe/Broyden profiles reduce solver work while preserving full accepted-state
physics. See the [quadruped evidence](../full-robot/measured-actuator-integration/reduced-exploration-2026-09-19/README.md)
for each profile's measured status; merely having a profile file does not mean
it passed qualification.

Once a case qualifies, initialize an ordinary search journal with:

```sh
target/release/examples/search_motion init-reduced \
  QUALIFIED_DIRECTORY YOUR_SEARCH_SETTINGS.json NEW_SEARCH_DIRECTORY
```

This rechecks the actual captures and current source identity; editing a
receipt's `qualified` boolean cannot unlock a failed profile. Search remains
an explicit separate `search_motion advance` operation. Qualification binds the
baseline case, not every point in its parameter space. As exploration moves
away from tested motions, recheck detailed finalists and representative cases.
The supplied exploration examples declare a small 0.95–1.05 neighborhood for
pace and motion amplitude; baseline values remain 1.0. This makes future search
configuration ready without sampling a new gait candidate during qualification.
The neighborhood is a policy search range, not a certified physical envelope.

Prepare a finalist with its selected parameter values:

```sh
target/release/examples/reduced_exploration finalist \
  QUALIFIED_DIRECTORY candidate-values.json NEW_DETAILED_SPEC.json
```

This restores the original detailed dynamics and timestep before applying the
candidate's policy parameters. It writes a specification, not a promoted gait.
Run it through the usual detailed validation and geometry/tracking gates.
Reduced-model winners remain provisional until those pass.

## Shared APIs and scope

`Recipe::prepare`, `CaptureSession`, `qualify`, `qualified_journal`, and
`detailed_candidate` are reusable Rust APIs. The registry operation
`experiment.prepare_reduced` also exposes preparation to Rhai/inspectors.
The thin CLI owns files, wall timing, progress and cancellation. Captures record
runtime identity and inputs; a qualification records capture hashes, exact
differences, per-channel errors, speedup and rejection reasons. Host/timing
provenance is an attestation, not a cryptographic performance certificate.

Focused tests exercise preservation, clock/bounds rejection, the production
runtime, repeated-run determinism, incomplete/failed/slow/inaccurate rejection,
capture/recipe mismatch, and detailed-finalist restoration. These tests use
small generic robot fixtures and explicitly synthetic benchmark timings;
real quadruped timings and qualification outcomes are recorded separately.

No gait search or hardware operation is part of qualification. A passing
profile shows agreement with its detailed simulator on the recorded cases;
it does not establish loaded-leg or battery accuracy against hardware.
