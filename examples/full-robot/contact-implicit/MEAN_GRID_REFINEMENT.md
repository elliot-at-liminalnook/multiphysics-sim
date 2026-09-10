# Denser physical constraints with retained dual state

The 64-control seed improves balance, but its 128-time optimizer misses larger
errors between samples. This experiment doubles physical checks to 256 times
on the same curve and compares against a continuation at 128 times. Independent
final audits use 1,024 physical times and 1,025 full CAD geometry poses.

## Exact shared-time migration

No Rust solver, contact, dynamics or controller code changes. The existing
native inspector evaluates coordinates, bounds and signed inequalities on
both grids. Every old-time physical frame and inequality block is exactly
unchanged, and optimizer coordinates/bounds remain identical. The 128 new
times add 15,360 physical rows, for 30,720 in total.

`prepare_inequality_grid_checkpoint.mjs` maps old rows by their unique physical
sample times and retains their multipliers exactly. Only newly added rows get
zero multipliers. The native inspection supplies the new residual vector,
including the recomputed mean/barrier objective: quadrature changes even
though the continuous motion does not. The initial state must remain strictly
inside the new-grid mean bound. There are no thresholded actual-slip constraint
rows in this experiment; the continuous sufficient bound protects sampled slip.

The next penalty stays 100 and the completed outer count stays 4. The stored
previous shifted norm is explicitly extended from 0.428264 to 0.681677 by taking
the maximum of its old value and the largest positive added-row violation.
New zero-multiplier rows contribute `max(g,0)` to that measure. This is a
declared change of the constraint set and its history measure, not a claim of
uninterrupted equivalence to the old problem. Native warm-start validation must
exactly reproduce each new checkpoint before taking a step.

Both trials retain the 64-control trajectory, CAD/contact/actuator definitions,
all physical gates, bounds, fixed period/displacement and mean-barrier weight.
Each uses two outer iterations of at most 30 inner iterations, allowing a dual
update between them while retaining a total requested 60 inner iterations.
The solver evaluation budget and scaling exponent 0.25 stay unchanged. Denser
physical evaluation is more expensive; equal iteration counts are not equal
wall time. The projected +45 degree displacement rate remains 0.0670626816 m/s.

## Initial grid evidence

| Check times | Force error N | Moment error Nm | Actual slip | Mean bound |
| --- | ---: | ---: | ---: | ---: |
| 128 | 0.070131 | 0.028565 | 4.119% | 4.928% |
| 256 | 0.080737 | 0.033634 | 3.891% | 4.939% |
| 1,024 | 0.081499 | 0.033634 | 3.951% | 4.944% |

The new grid captures most of the previously missed balance peaks while
retaining an interior mean-bound starting point. The finest initial audit
reports a small further force peak; all shared-time frames are exactly equal.
These are motion-sampling comparisons, not runtime timestep-convergence tests.

## Completed comparison and stopping point

| Optimizer times | Dense force error N | Dense moment error Nm | Dense actual slip | Overlap |
| --- | ---: | ---: | ---: | ---: |
| 128 | 0.091507 | 0.035613 | 3.950% | 0 |
| 256 | 0.066862 | 0.027013 | 3.946% | 0 |

Both runs completed 138,086 evaluations and stopped at the outer iteration
limit. The 256-time result improves dense balance but still fails the unchanged
0.05 N / 0.02 Nm gates. Neither improves the fixed 0.067063 m/s planned rate,
and neither was executed as a controller. `mean-grid-summary.json` is reproduced
by `node examples/full-robot/contact-implicit/summarize_mean_grid_refinement.mjs`.

The user has redirected priority to measured maximum speed. Preserve these
results as offline research; do not spend another continuation solely on this
fixed-speed restoration without a concrete route to faster runtime walking.

## Runtime transfer remains separate

Inspection of `compile_contact_implicit` confirms that its current compiler
accepts only fixed, at-rest finite-horizon plans and rejects periodic startup.
The existing finite-plan Rhai policy is a timed diagnostic, with no periodic
entry, stopping or steering contract. The other periodic reference compiler
uses the older contact-phase representation and validated static pause windows;
it cannot directly consume these whole-body cubic controls.

Consequently these offline trajectories still need an explicit entry transition
and shared periodic controller adapter before detailed runtime replay. Their
planning failures must remain visible during any diagnostic transfer. No new
compiler or runtime controller is implemented in this grid experiment, and no
browser bundle is promoted. The physical speed maximum remains unestablished.
