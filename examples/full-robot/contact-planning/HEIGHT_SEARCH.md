# Body-height search from sampled CAD reach

The current joint optimizer restricted each body-spline Z control to ±.015 m
around the configured base offset. This is a planning interval, not an authored
hardware limit. The new recipe expands only those eight control bounds to
**[-.1292195774, +.0531730097] m**, derived from the recorded CAD workspace.
The other 435 variables, initial body/foot/force curves, .30 m/s objective,
8,000-evaluation search controls and all physical gates are unchanged.

This permits the optimizer to explore different body heights and the associated
leg postures. The interval is an experimental search domain, not a certificate
that every enclosed pose is reachable or collision-free, and not a physical
speed bound. It comes from sampled configurations at a fixed body orientation;
changing orientation and swing motion still require full native evaluation.

## Derivation and provenance

For each foot, let `[z_min, z_max]` be its recorded valid sampled world-Z range,
`z_target` its declared floor target, and `z_initial` the configured base
translation. Translation invariance gives a candidate body-control interval

`[z_target - z_max - z_initial, z_target - z_min - z_initial]`.

Intersecting these four intervals gives the bounds above. The active endpoints
come from the -Y foot's sampled range [-.4251290121, -.2427364249] m. The declared
floor target is -.4410181229 m and the base translation is -.0690621205 m.
These bounds apply to the body-spline Z coefficients relative to that base
offset; they are not absolute heights above the floor.

The preparation verifies the workspace's recorded source hashes, CAD hash and
coordinate frame. All CAD link definitions apart from collision fields, and
all joint definitions, match the current validation scene exactly. The current
scene retains its refined collision fields. The workspace deliberately omitted
its initial base vertical offset; the formula accounts for it explicitly.

The recorded extrema are not a complete workspace boundary. They enclose
untested combinations and may miss other valid heights. No limit is promoted
into CAD and no robot, world, actuator, friction or compliance property changes.

## Verification and reproduction

`prepare_joint_height_workspace.mjs` derives the interval, verifies all inputs
and changes only the eight Z-control bounds in `joint-aligned-speed.recipe.json`.
It records the individual foot intervals and checks that the existing search
range and starting controls are contained in the new interval.

An independent `optimize_joint_contact ... --evaluate` run produces a complete
native initial report exactly equal to the aligned-force reference report,
including signed zeros. Its .211710 m/s reference still fails feasibility:
12.524524 N force error, 3.066718 Nm moment error and -.269534 Nm motor margin.
Thus the preparation changes the allowed search, not the starting physical
result. The joint solve is now running, with the unchanged collision,
actuator and balance checks rejecting inadmissible poses.
`joint-height-speed-launch.json` records its verified optimizer, input files
and initial report. Live output is excluded from completed evidence.
