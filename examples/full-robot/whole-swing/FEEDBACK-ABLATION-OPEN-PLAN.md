# Explicit zero-gain input profile for the feedback ablation

All three original ablation attempts were rejected before the first physics
transition: their controller input schema fixed both body and point gains at
0.25, so zero was outside the allowed action bounds. Retain those rejected
captures and their original plan. They provide no physical controller result.

For the same predeclared ablation, explicitly lower the permitted input minimum
for `command.body_gain` and `command.point_gain` from 0.25 to 0. Keep their
maximum and initial value at 0.25. This changes controller gain-input validation,
not CAD actuator authority, physical robot properties or task thresholds. The
versioned source scene plus this two-field derivation reproduces the profile.
Validate every action against the resulting schema before starting the runs.

Run four frozen 60-second cases in a new output directory: the original actions
under the expanded input schema, then the previously prepared no-body, no-point
and angles-only actions. The new bounds-only baseline must reproduce every
physical frame of the old baseline exactly (excluding wall time). Its scene
may differ only in the two declared input minima. All other comparisons and
the original task/5% contact-motion screen remain as in FEEDBACK-ABLATION-PLAN.
