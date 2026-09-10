# Persistent constrained solves and a scaling comparison

Both continuations still fail balance qualification. The ordinary scaling
reduces dense force/moment errors while retaining passing sampled slip and
collision checks. Jacobi scaling improves moment error further but loses the
actual slip pass. Neither is a qualified new gait or a speed increase.

The recorded low-slip seed's first fit still failed force and moment balance.
This experiment continues its augmented-Lagrangian outer method without
discarding the multipliers already learned from those errors, and compares two
existing diagonal scalings from exactly the same continuation state.

## Shared continuation contract

`sim-solve::inequality_augmented_lagrangian::AugmentedWarmStart` records the
parameter values, objective and inequality residuals, nonnegative multipliers,
next penalty, preceding shifted-constraint norm and completed outer count.
A checkpoint is emitted at an outer-iteration limit, after the penalty update.
Interrupted inner evaluations, penalty-limit termination and stationary
termination do not emit a resumable outer checkpoint.

The warm-start entry point validates dimensions, finite state and multiplier
signs. It re-evaluates the initial objective/inequalities and requires exact
agreement with the checkpoint before optimizing. The runtime additionally
requires the checkpoint's parameter values to decode to the supplied initial
motion. Using those original values avoids unnecessary displacement rounding
through a decode/re-encode cycle. The caller must retain the same model,
constraint ordering and interpretation; agreement at one point is not a proof
of global model identity. Experiment hashes separately identify those inputs.

Each invocation has an explicit new evaluation budget, including its initial
re-evaluation. Outer iteration numbers continue from the checkpoint. Every
inner LM solve starts with configured damping, as in an uninterrupted outer
solve. The method does not pretend to resume halfway through an LM iteration.

The analytic split-run test serializes/deserializes a checkpoint and verifies
exact equality of final values, multipliers, continuation state and subsequent
iteration diagnostics against an uninterrupted solve. The split run costs one
additional initial model call. Invalid values, changed residuals and negative
multipliers are rejected. The planner test checks preserved physical reports
and rejection of a changed slip objective or initial motion. All seven AL tests
and all sixteen planner tests pass. `serde_json` is added only as a solver test
dependency; no runtime contact or actuator law changes.

## Robot preparation

`warm68` starts from `recorded68-restore.result.json`, keeping its period,
displacement, 128 physical check times, contact model, actuator bounds and slip
objective. Two additional outer iterations allow thirty inner LM iterations
each. The existing result predates checkpoint serialization, so
`prepare_inequality_continuation.mjs` reconstructs its next outer state from the
stored multipliers/history and an independent initial-state audit. The initial
large violation fell sufficiently that the next penalty remains **1**, rather
than rising automatically to 10. The preceding shifted norm is 4.8528405398.

The preparation script verifies matching parent config and slip objective;
only absent versus null `periodic_collocation_phases` is normalized, matching
the known optional Rust field. All physical properties and bounds remain
explicit. The rebuilt auditor reproduces every field of the complete parent
audit exactly. The native warm-start run must also reproduce its saved
objective and inequality vectors exactly before any new search step.

`warm68-jacobi` differs only in the existing LM scaling exponent, from 1/4 to
1/2. For positive normal-matrix diagonal `H_ii`, the latter uses
`D_ii=H_ii^(-1/2)`, giving `(D H D)_ii=1` before damping. The comparison tests
column conditioning while preserving the objective, constraint definitions,
initial motion, multipliers, bounds, sample grid and budgets. No gait sequence
is prescribed. The inherited motion came from a phase-based controller;
continuing it is not evidence of discovering a new gait family.

The parent's largest moment error is pitch, 0.123757 Nm, and its minimum torque
margin is +0.220302 Nm. That provides a reason to investigate numerical
conditioning instead of declaring actuator saturation or a physical speed limit.
Its planned projected rate remains 0.0670626816 m/s. This is preparation toward
subsequent speed increases, not a replacement for the full speed objective.

## Dense results

All comparisons use 512 physical samples and 513 geometry poses:

| Motion | Force error N | Moment error Nm | Minimum torque margin Nm | Loaded slip | Overlap |
| --- | ---: | ---: | ---: | ---: | ---: |
| Parent recorded-gait fit | 0.210964 | 0.123757 | +0.220302 | 4.729% | 0 |
| Continued, exponent 1/4 | 0.129337 | 0.075612 | +0.277906 | 4.741% | 0 |
| Continued, exponent 1/2 | 0.119690 | 0.043783 | +0.275381 | 7.158% | 0 |

Force must be at most 0.05 N and moment at most 0.02 Nm. Both continuations fail
both gates; only the ordinary continuation retains actual slip below 5%. Point
penetration remains below 0.109 mm, within the 1 mm gate. The physical path is
not certified between samples, and neither candidate is promoted to the runtime
or browser. The planned displacement rate remains 0.0670626816 m/s.

The ordinary run uses 68,971 evaluations and Jacobi uses 68,958. Both use outer
penalties 1 then 10. Every inner solve reaches its thirty-iteration cap, and
both runs stop after the two additional outer iterations. Their next checkpoint
penalties are 100 and 10 respectively, as consequences of the common adaptive
update rule. No stationarity or infeasibility proof
is claimed.

The useful next constraint is actual loaded slip: the current objective shapes
a conservative surrogate, but a finite solve can still sacrifice the external
5% slip pass while reducing balance error. Keep the ordinary continuation as a
starting point and explicitly constrain the existing actual-slip gate in the
optimization, while retaining the dense gate independently. This is not yet
implemented. Simply selecting the lower moment error would discard a required
gait property. No actuator saturation or physical speed maximum is established.

## Verification

`warm68-build-identities.json` records the new source and executable identities.
`warm68-parent-parity.json` records full parent-audit equality. Optimizer output
contains a new continuation when it reaches an outer boundary. The shared
`verify_inequality_evidence.mjs` checks physical inequality rows, actuator margins,
slip objective, shifted residuals, multiplier updates, evaluation accounting,
fixed displacement, same-time dense frames and the next checkpoint's penalty.
`summarize_warm_continuation.mjs` additionally checks both trials' matched inputs
and reports each dense force/moment component's peak and time.

Run `optimize_contact_inequalities SCENE surface-markers.json RECIPE`; audit
with `audit_contact_implicit SCENE surface-markers.json RECIPE RESULT 4 --pairs`
and the corresponding dense recipe with 16 subdivisions. All artifact paths
are in this directory; use the recorded constrained scene. Run the summarizer
with Node from the repository root. Physical gates, dense collision/slip checks,
runtime tracking and browser qualification remain independent of solver status.
