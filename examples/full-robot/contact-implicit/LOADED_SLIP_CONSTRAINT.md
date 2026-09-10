# Actual loaded-slip constraints

The added constraint rows are implemented and verified, but this finite solve
still violates them. Dense slip is 7.520%, versus 7.158% in the matched control;
force and moment balance also fail. The earlier `warm68` trajectory retains its
4.741% sampled-slip pass and remains the more useful starting point. No new gait
is qualified or promoted.

The previous Jacobi-scaled continuation reduced balance errors but increased
dense actual slip to 7.158%, above the existing 5% gate. This experiment adds
that actual gate to the optimizer without changing the physical model or the
separate validation criterion. A finite augmented-Lagrangian solve can still
violate its constraints; the new option is not a guarantee of qualification.

## Shared implementation

`ContactSlipObjective.constrain_loaded_slip` is an explicit optional policy
choice, defaulting to false and omitted from serialization when false.
`ContactImplicitPlanner::inequality_residuals` composes the existing signed
physical inequalities with one appended inequality per slip-report group:

`(sampled_loaded_slip_ratio - target_ratio) / ratio_scale <= 0`.

The ratio is the existing load-weighted tangential path divided by body path,
using the explicit load threshold and sample quadrature in the shared Rust slip
report. It is not the RMS upper bound. `ratio_scale` normalizes the added rows
as well as the existing RMS objective; it does not change the accepted ratio.
The RMS shaping objective remains unchanged, and the actual dense gate remains
independent. There are no prescribed contact flags or phase sequences.

The actual metric is piecewise smooth because a foot enters/exits the loaded
set at the declared force threshold. A finite sample grid can also miss changes
between samples. Neither this implementation nor its numerical derivatives
establish continuous-time slip acceptance. Dense physical and geometry audits
remain mandatory, and solver tolerance does not relax their gates.

The ordinary weighted restoration API rejects the flag, rather than ignoring
the requested inequality. Auditing can still report slip with the flag enabled.
The frozen translating-body test demonstrates why the distinction matters:
balance is valid while actual slip is 100%. With the option enabled, the
constrained solver cannot report that fixed motion as stationary within its
constraint tolerance. Tests also cover the exact slip boundary, a strictly
passing row, unchanged RMS objective/physical rows, and default compatibility.
All sixteen planner tests pass.

## Matched experiment

`sliplimit68-jacobi.recipe.json` repeats the previous `warm68-jacobi` experiment
with only the actual-slip option and its appended constraint rows changed.
Motion, period, fixed displacement, contact and actuator models, bounds, 128
check times, objective, scaling and evaluation/iteration budgets match.
Both start from `recorded68-restore.result.json` so the comparison isolates the
added constraints rather than confounding them with a different initial motion.
This diagnostic comparison does not discard the later retained `warm68` motion.

The source continuation retains all 15,360 physical multipliers. Four added
slip multipliers start at zero, in deterministic slip-report group order.
Their initial signed rows are -0.829617, -0.822518, -0.076177 and -0.823468,
so all initially pass. The preceding shifted norm and next penalty therefore
remain unchanged. The shared solver must exactly reproduce the extended initial
residual vector before taking a step. This is explicitly an extension of the
constraint set, not a claim of uninterrupted equivalence to the old problem.

The control's planned +45° projected displacement rate is 0.0670626816 m/s.
This remains a low-slip starting-motion investigation toward speed continuation,
not a new speed target, measured speed gain or physical upper bound.

## Results

| Audit | Force error N | Moment error Nm | Minimum torque margin Nm | Actual loaded slip | Overlap |
| --- | ---: | ---: | ---: | ---: | ---: |
| Matched control, dense | 0.119690 | 0.043783 | +0.275381 | 7.158% | 0 |
| Added constraints, 128 times | 0.101165 | 0.042090 | +0.252825 | 6.961% | 0 |
| Added constraints, 512 times | 0.117246 | 0.042090 | +0.247133 | 7.520% | 0 |

The dense audit includes 513 geometry poses and reports 0.108361 mm maximum
point penetration. Torque, point penetration and sampled collision checks pass,
but force exceeds 0.05 N, moment exceeds 0.02 Nm, and slip exceeds 5%. The slip
failure is present on both optimization and validation grids; it cannot be
explained solely by a missed between-sample peak.

The final -X foot slip inequality is +0.392176, with a positive multiplier;
the other three slip rows are nonpositive. The maximum overall normalized
violation is 1.10450. Two thirty-iteration inner solves, with penalties 1 and
10, consume 68,957 evaluations and both reach their iteration caps. The outer
solver reports `outer_iteration_limit`, not successful constraint convergence.
The next checkpoint penalty is 10.

This result shows why declaring inequalities is insufficient when subproblems
are capped far from convergence. It does not prove infeasibility or a physical
speed maximum. The next numerical work should preserve the already passing
actual-slip region during balance improvement, with explicit handling of
constraint boundaries and dense checking. A new feasibility-preserving step
method is not implemented here. Do not replace the retained `warm68` seed with
this failed result or relax the external slip criterion.

## Verification and reproduction

`prepare_loaded_slip_trial.mjs` checks the parent state, preserves all existing
multipliers and appends only the four nonpositive initial slip rows with zero
multipliers. `sliplimit68-build-identities.json` records the compiled sources and
executables. The new build reproduces the entire previous parent audit exactly,
including physical frames, slip values and geometry poses.

The shared evidence verifier now checks the appended actual-slip rows against
the independently audited slip report, in addition to physical inequalities,
multiplier updates, budgets, fixed displacement, continuation state and dense
same-time physical frames. All three prior summaries remain byte-for-byte
reproducible. `summarize_loaded_slip_trial.mjs` verifies the matched experiment
and compares its final dense metrics with the previous Jacobi control.

Run the shared `optimize_contact_inequalities` executable with the recorded
constrained scene, `surface-markers.json` and this recipe. Independently audit
the result with `audit_contact_implicit` using 4 geometry subdivisions, then
the dense recipe with 16 subdivisions; `--pairs` retains contact witnesses and
authoritative planned poses. Run the summarizer with Node from the repository
root. No browser or runtime controller is promoted by an offline result alone.
