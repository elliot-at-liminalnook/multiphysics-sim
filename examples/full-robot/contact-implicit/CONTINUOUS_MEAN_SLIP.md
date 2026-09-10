# Continuous mean-slip bound

The continuous bound removes the verified cutoff jump from the optimization
measure. The retained `mean68-warm` trajectory passes dense actual slip at
3.999%, with no sampled interlink overlap, and improves balance errors and
torque margin over the prior retained seed. Force and moment still fail their
acceptance gates. No new gait, runtime controller or physical maximum is qualified.

The previous strict actual-slip barrier exposed a hard loaded-set discontinuity:
a 1e-12 m vertical control perturbation crossed a 1 N load cutoff and jumped
sampled slip from 4.569% to 5.696%. This experiment replaces that optimization
measure with a continuous sufficient bound. Actual-slip reporting and the
external physical, dense-slip and collision gates remain unchanged.

## Shared formulation

For a contact point with normal load `fn`, group load `N`, tangential material
velocity `vt`, positive time weight `dt`, and explicit threshold `N0`, the new
`ContactSlip::mean_path_sample` returns
`dt * fn/max(N,N0) * |vt|` in metres. Sum these contributions over points and
time, then divide by body path to obtain the mean bound `L`.

Above the load cutoff, each group/time contribution is the actual-slip
contribution. Below it, the actual measure contributes zero while this bound
adds a nonnegative amount. Consequently `actual <= L`. Cauchy-Schwarz, total
time weight `T`, and `body_path >= displacement` give `L <= RMS_bound`.
The bound is continuous for valid loads and nonzero body path. Norm/max kinks
remain; this is not a claim of differentiability everywhere or continuous-time
acceptance from a finite quadrature.

The existing component owns sample validation for both formulas. Its registry
retains the original dimensionless RMS outputs and appends a typed
`mean_slip_path` output in metres. The focused example exercises both methods.
No robot-specific dynamics, contact law or alternate simulation is introduced.

`ContactSlipObjective.use_continuous_mean_bound` is explicit and defaults false.
When true, the report includes `continuous_mean_slip_upper_bound`, shaping uses
that bound, and the optional reciprocal barrier applies to its signed rows.
The existing RMS report stays available for comparison. Actual-slip constraint
rows, when requested separately, retain their original meaning. This trial
does not add those discontinuous rows: the sufficient bound protects sampled
actual slip. Old recipes and reports retain their default serialized form.

## Experiment and verification

`prepare_continuous_slip_trial.mjs` clones `recorded68-restore.recipe.json`.
It changes only the continuous-bound option and adds barrier weight 0.0001.
The recorded initial motion, physical model, 128-time grid, bounds, fixed
0.0670626816 m/s projected displacement rate, scaling exponent 0.25 and initial
one-outer/30-inner budget are unchanged. There is no multiplier checkpoint to
migrate. The native solver must evaluate the initial state strictly inside
the mean-bound domain before searching.

The independent dense audit uses 512 physical times and 513 CAD geometry poses.
Full runtime tracking, sustained speed, command responsiveness and physical
speed limits remain separate requirements. This feasibility experiment does
not replace the maximum-speed goal or qualify a new gait by itself.

All 63 control-component tests and 16 planner tests pass. They cover constant
slip, unloaded swing, mixed-load/direction bound ordering, continuity across the
cutoff, invalid inputs, registry units and execution, and planner barrier
rejection of a physically balanced but slipping trajectory. The focused
example returns exactly 1 for constant sliding and 0 for stationary contact
with unloaded swing. Five prior summaries remain exactly reproducible.

The new binary reproduces the entire previous default parent audit exactly.
Initial physics and geometry also exactly match the earlier 128-time audit.
On this grid, the initial maximum mean bound is 3.0704648%, below the 5% gate;
the prior diagnostic's 3.217% used a different 144-time grid. The grid difference
is explicit and is not an improvement in the motion itself.

The previous +/-1e-12 m vertical probes are re-evaluated with the new reporting
mode. All physical frames, geometry, actual slip and RMS values are unchanged.
The -X mean bounds are 0.06954046920775253 and 0.06954047239218375, a change of
3.1844e-9 while actual slip still jumps by 0.01126248. This verifies that the
new optimization measure removes this specific cutoff jump without changing
the independent actual-slip gate or claiming microscopic physical accuracy.

## First restoration

| Dense audit | Force error N | Moment error Nm | Torque margin Nm | Actual slip | Mean bound | Overlap |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Previous RMS restoration | 0.210964 | 0.123757 | +0.220302 | 4.729% | Not reported | 0 |
| Continuous mean barrier | 0.245495 | 0.117868 | +0.227807 | 3.954% | 4.959% | 0 |

The new trajectory has a stronger slip margin but still fails the 0.05 N force
and 0.02 Nm moment limits. Coarse actual slip is 3.611%, with mean bound 4.961%.
The dense audit reports 0.108737 mm maximum point penetration and positive
torque margin. Body path is about 1.069 times net displacement, essentially
unchanged from the recorded initial motion; increased path length is not the
source of its apparent slip improvement.

The initial solve consumes 34,486 evaluations, with five rejected evaluations,
and reaches its outer/inner iteration caps. Final damping is about 8,604 and
the projected gradient remains large. It restores most of the initial
15.5675 N / 2.96258 Nm imbalance, but has not converged to a feasible gait.
The persistent checkpoint retains 15,360 physical multipliers, a next penalty
of 1, and its exact final mean/barrier residuals.

Two further outer iterations resume this checkpoint with the same physical
model, bounds, fixed displacement, mean objective, weight and grid. A matched
comparison changes only scaling exponent 0.25 to 0.5, which normalizes the
Gauss-Newton diagonal above its numerical floor. Both runs use the same
initial checkpoint; this tests solver conditioning after removing the cutoff
discontinuity, not a new contact schedule or manually altered gait.

## Continuation comparison

| Dense audit | Force error N | Moment error Nm | Torque margin Nm | Actual slip | Mean bound | Overlap |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Earlier retained `warm68` | 0.129337 | 0.075612 | +0.277906 | 4.741% | Not reported | 0 |
| `mean68-warm`, exponent 0.25 | 0.118593 | 0.074403 | +0.310268 | 3.999% | 4.970% | 0 |
| `mean68-warm-jacobi`, exponent 0.5 | 0.168849 | 0.080277 | +0.245257 | 3.851% | 4.949% | 0 |

Retain `mean68-warm` as the next numerical seed: both new trials pass dense
slip, but exponent 0.25 has lower force/moment errors and more torque margin.
It also improves force, moment, torque margin and actual slip over the older
`warm68` seed. This is progress toward feasibility at the same fixed planned
speed, not a speed gain. The earlier evidence remains intact.

The two continuations consume 68,973 and 68,959 evaluations. Each uses penalties
1 then 10 and returns a next penalty of 100. Both inner iterations hit their
30-step caps, and both outer solves return `outer_iteration_limit`. Only six
and three evaluations are rejected respectively. Final projected gradients
remain large; this is neither a stationary solution nor proof that the motion
basis or robot cannot satisfy balance.

For the retained trajectory, maximum penetration is 0.107766 mm, body path is
1.06901 times net displacement, and the dense mean bound remains below 5%.
Force is still 2.37 times its allowed error and moment 3.72 times its limit.
Continue physical feasibility work from this preserved checkpoint before
speed continuation or runtime promotion. Larger solver budgets or motion-basis
refinement must retain the slip guarantee, finer-grid checks and collision
audit; solver stagnation cannot establish the physical speed ceiling.

## Reproduction

Build the shared `optimize_contact_inequalities` and `audit_contact_implicit`
examples in release mode. Use the recorded constrained scene and
`surface-markers.json`. Run the optimizer with the chosen recipe, then audit
with 4 geometry subdivisions and `--pairs`; repeat with its dense recipe and
16 subdivisions. `prepare_inequality_continuation.mjs` preserves the exact
stored checkpoint. The matched Jacobi recipe changes only scaling exponent.

Run `summarize_continuous_slip_trial.mjs` and
`summarize_continuous_slip_continuation.mjs` from the repository root. They
verify matched inputs, checkpoints, barrier residuals, physical rows, multiplier
updates, same-time dense physics and independent reductions of the native mean
bound. The default-parent and paired-probe audits preserve their old physics
exactly. Source/binary identities, test logs, inputs and all completed outputs
are retained in the evidence index. No browser bundle or CAD artifact changed.
