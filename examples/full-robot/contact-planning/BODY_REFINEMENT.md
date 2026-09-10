# Enlarging the body-motion basis without changing the seed

The joint search is now running with an **eight-control body spline**, enlarged
from four controls through the existing shared Rust knot-insertion component.
This increases body variables from 24 to 48 and total joint variables from
173 to **197**. Timing, foot placement, swing paths and 120 force values remain
free as before. The initial reference speed is unchanged at **.211710 m/s**.
It still fails physical feasibility; refinement is not a speed gain.
`joint-body8-speed-launch.json` records the verified optimizer build, recipe and
initial report. The 8,000-evaluation solve uses direct force derivatives; its
live output and log are excluded from completed evidence until it terminates.

For periodic cubic controls P, the shared subdivision uses
`Q[2i] = (P[i-1] + 6 P[i] + P[i+1]) / 8` and
`Q[2i+1] = (P[i] + P[i+1]) / 2`, with wrapped indices and half-sized knot
spacing. This preserves the curve in exact arithmetic. The existing
`Trajectory::refined_periodic_config` component implements the operation;
the new native example only prepares and checks a planner recipe.

The recipe requires every original body control/channel to be an explicit
decision with uniform per-channel bounds. It preserves those same bounds:
±.015 m translation and ±.1 rad rotation-vector controls. Convex subdivision
keeps the starting controls inside them. No other decisions, bounds, physical
model fields, .30 m/s objective or 8,000-evaluation search controls change.

## Fidelity evidence

- Across 4,097 times, maximum changes are 2.081668e-17 in position/rotation-vector
  values, 3.330669e-16 in rates and 7.105427e-15 in accelerations.
- All 160 original frame constraints are retained. Their largest normalized
  physical difference is 1.110223e-12. Eight extra body-knot frames bring the
  refined audit to 168 frames across forward and reverse operation.
- All 360 direct force-column comparisons pass finite differences on this
  larger layout; maximum scaled derivative error is 6.613939e-9. The third
  interpolation case duplicates the first because the input is already linear,
  giving 240 distinct columns across two configurations. Six probe
  reports also match full uncached evaluations exactly.
- The independently evaluated initial report matches the prepared report
  exactly after JavaScript's canonicalization of 16 signed zeros. There are no
  nonzero numerical or structural differences. No raw byte-equality claim is
  made for that JSON extraction.
- Both existing periodic-trajectory tests pass, including three successive
  refinements that preserve values, rates and accelerations.

The original four-control model has not produced a feasible joint-force gait
in the completed trials. Increasing the body basis tests whether that motion
restriction contributes to the failure. It does not establish that eight
controls are sufficient, or exhaust force bases, contact patterns, phase counts
or sparse constrained optimization. The force-template layout remains the
same in this experiment, allowing its effect to be separated from body freedom.

## Reproduction

`refine_joint_contact_body scene.json markers.json joint-workspace-speed.recipe.json`
produces the expanded recipe and both complete CAD reports. The unchanged
diagonal validation scene and workspace markers are used.
`prepare_joint_body_refinement.mjs` checks all retained physical frame constraints
and writes `joint-body8-speed.recipe.json` and its initial report.
`audit_joint_force_jacobian` checks the new recipe; then
`check_joint_body_refinement.mjs` verifies the saved evidence.

The first refiner build failed because `ContactClock` has no `PartialEq`;
the corrected example compares its two scalar fields. Both build logs are
retained. No shared control or physics code was changed for this refinement.
