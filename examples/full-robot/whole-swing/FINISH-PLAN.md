# Finish horizontal travel before touchdown

The asymmetric 3.75 mm/s short case clears geometry but fails stopping and
20/5 ms trajectory agreement. The new hypothesis is that horizontal motion
late in lowering contributes to contact drift. Compare horizontal completion
at 75% and 85% of the combined 0.76-second swing, at 20 and 5 ms physics steps.
Retain the successful asymmetric stance, original order, 3.75 mm/s command,
half body overlap, student weights, physical model, 24-second duration, seed 0
and existing actions. The prior 100% pair is the unchanged reference.

This completes XY travel 190 or 114 ms before the scheduled landing. Peak
horizontal speed is respectively 2/3 or 10/17 of the original raise-only
profile for the same step. Vertical clearance, support checks and physical
motor authority remain unchanged. The shared parameter is constrained to
[0.5,1], defaults to 1, and applies only when whole-swing motion is enabled.

All physical and numerical gates from PLAN.md remain unchanged. A successful
short case must pass both timestep task checks, maximum 1 mm sampled foot
and 0.5 mm body differences before promotion to sustained/steering evaluation.
Retain failed trials; do not alter budgets in response to the results.
