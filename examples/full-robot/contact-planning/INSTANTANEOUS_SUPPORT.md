# Separating force-curve restrictions from support geometry

At the fixed .211710 m/s reference, independently chosen point forces still
cannot balance the requested CAD motion within the declared tolerances at
**74 of 160 sampled frames**. The largest normalized lower bound is **19.9540**;
all rows would need magnitude at most 1 to pass. This is lower than the earlier
trajectory-basis bound of 35.6671, but remains incompatible with sampled balance.

The worst frame is cycle phase .71875, with only +X and -X feet in planned
contact, in both forward and reverse operating cases. The dual weight combines
mainly world-Y force and roll moment. The independent least-squares residual
there includes about 1.034 N of Y force and -.432 Nm of roll moment. These are
components of the diagnostic least-squares assignment, not separately attainable
minimum errors or measured runtime loads.

## What is relaxed and what is checked

The new shared `joint_force_frame_systems` API exposes the cached CAD-derived
force-to-wrench matrix and zero-force residual for each frame. It does not alter
the planner or runtime forces. The native audit:

- gives each instant independent forces, removing temporal force-curve limits;
- enforces zero force for planned swing feet;
- encloses all permitted force-node values in coordinate boxes, which contain
  every value of the current convex-interpolated force templates;
- omits friction and actuator constraints, making this a necessary-balance
  relaxation rather than a complete feasibility test;
- applies the shared affine dual bound with the original .05 N / .02 Nm scaling.

Reconstruction of every original wrench residual from these matrices and forces
agrees exactly with an independent full CAD evaluation (normalized maximum
error 0). The checked audit reproduces the initial audit's rows and lower bounds.
This is a floating-point, sampled, fixed-motion point-support result. A bound
below one would not itself certify feasibility. None of these bounds establishes
the robot's global maximum speed.

## Consequence for the speed search

More force coefficients alone cannot make this particular motion pass the
point-support model. The `joint-workspace-speed` search has now completed:
all 173 motion/timing/force variables remain free, XY placements use the recorded
CAD workspace boxes, and the 8,000-evaluation budget permits multiple complete
inner steps. Its initial reference is unchanged. All 8,000 evaluations and 23
accepted inner steps end infeasible at the minimum permitted reference speed,
.025 m/s; no gain is claimed.

The point model also omits moments available through a finite foot contact area.
The subsequent [finite-surface audit](FINITE_SURFACE_SUPPORT.md) evaluates the
current reference using actual CAD surface markers near the floor. It retains
the authored geometry, actual world pose and explicit contact eligibility,
without assuming every point on a nominal stance foot touches the ground.
Its allocation still fails balance in 94/160 frames; this is not an
infeasibility proof. The existing detailed runtime remains the validation model.

## Reproduction

`audit_joint_support_space scene.json markers.json recipe.json` records the
per-frame bounds and checks reconstruction against the complete CAD evaluation.
Use `joint-x25-warm.recipe.json` with the unchanged diagonal validation scene and
workspace markers. Both the initial and checked reports are preserved; only the
checked report adds the reconstruction metric. The existing affine-bound tests
cover incompatible balance, rank deficiency and the finite-box correction.

The new shared API is read-only. There is no new gait promotion, contact-law
change, actuator-limit change or browser change.
