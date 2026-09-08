# Moving body/foot-feedback ablation

Recorded direct-transfer target corrections oppose one another in every moving
shift, raise, lower, return and settle sample. During shift their aggregate
cosine is -0.759 and the norm-weighted cancellation fraction is 64.6%. The
recorded targets reconstruct from the additive feedback terms. This diagnoses
angular target interaction, not physical force cancellation or which objective
should take priority. Loaded-foot damping already failed to resolve sliding.

Before seeing alternative outcomes, freeze three 60-second development runs:
set `command.body_gain` to zero during nonzero motion commands; set
`command.point_gain` to zero during motion; and set both to zero during motion.
Keep the original tracking gain and every other input. Restore the original
gains for zero-motion commands, including the same stopping controller and
bounded settled integral. Use the previously recorded position-only direct
minute as the unchanged baseline, with its pinned executable and all recipe
inputs. This avoids an unnecessary duplicate baseline run.

The robot, friction, motor authority, timestep, seed, direct-transfer sequence,
controller source and 3.75 mm/s forward command remain fixed. Keep the original
task gates and prospective 5% loaded-contact-motion screen. Report every
completed run's speed, swing/support qualification, contact motion, heading,
stop error and work; preserve partial failures without full-minute claims.
Verify that only the declared gain channels change, only during motion, and
that baseline action reconstruction is exact. Do not promote an ablation solely
for better endpoint tracking or lower correction cancellation.

This is causal development evidence about the current additive controller.
It does not establish a deployable observation set, broader robustness,
terrain performance, timestep accuracy or realtime browser acceptance.
