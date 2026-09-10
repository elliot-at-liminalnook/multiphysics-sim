# Force knots at contact events

The robot recipes already use **linear** force templates with seven knots per
foot. Their knot placement can nevertheless restrict load transfer. In an
isolated constant-total-load fixture with two alternating supports, phases
[0, .5] and duty .65, the original seven-node linear basis has a floating-point
max-residual lower bound of **.0212714 of the required load**. The bound even
permits node forces in [-2, 2] times that load, ignoring unilateral/friction and
actuator restrictions. Knots aligned to the contact events admit an explicit
nonnegative, complementary-ramp assignment with maximum error **1.11e-15**.
This diagnoses a representation restriction, not a robot-speed bound or proof
that the current CAD motion is feasible.

## Shared implementation and robot preparation

`JointContactMotion::with_event_aligned_linear_forces` adds knots at the current
contact events, stance midpoints and body knots. Every existing linear knot is
retained, and inserted values come from the shared trajectory sampler. This
preserves each linear force curve before optimization. Applying the operation
again after a timing change can add new event knots; knots stay in normalized
stance coordinates during an individual solve. This is explicit refinement,
not a continuously changing variable layout inside a derivative calculation.

The method also accepts quintic inputs, where conversion to linear changes the
curve and requires a fresh audit. That conversion is not used by this robot
experiment. Body/foot motion is unchanged in either case.

The eight-control CAD reference now has **443 joint variables**: 77 motion/timing
variables and **366 force values**, increased from 120. Both clocks have
[19, 17, 17, 16] nodes across the four feet. Original per-foot/clock/axis force
bounds are retained; nonuniform original coefficient bounds are rejected by
the preparation example instead of silently widened.

Across 4,097 samples per template, all eight original linear curves are preserved
within **7.105427e-15 N**. All 168 original physical frame checks remain, with
maximum normalized difference **1.421085e-13**. The added force knots bring the
audit to 250 frames. Optional CAD allocation then initializes the new node
values; it is only a starting point for the joint solve.

The initial .211710 m/s reference still fails feasibility. CAD initialization
changes maximum force error from 13.090909 to **12.524524 N**, moment error to
**3.066718 Nm**, and retains motor margin **-.269534 Nm**. Maximum normalized
inequality is **249.4905**. More variables have not themselves produced a speed
gain. The body/foot/timing decisions, .30 m/s target, 8,000-evaluation budget,
physical robot and acceptance limits are unchanged.
The 443-variable search is now running with direct force derivatives.
`joint-aligned-speed-launch.json` records its recipe, optimizer and initial
report; live output is excluded from completed evidence until termination.

The expanded derivative audit checks **1,098 distinct force columns** across
three configurations: perturbed reference loads, constant body motion, and
quintic interpolation as an explicit alternate to the actual linear input.
All pass, with maximum scaled derivative error **8.759447e-9** and six probe
reports equal to full uncached evaluations. The independently evaluated initial
report matches the prepared report after JSON signed-zero canonicalization.
All five shared phase-planner tests pass.

## Completed original-body multistart results

The remaining two four-control starts completed all four configured outer
iterations, each with 24 accepted inner steps. Neither is feasible:

| Initial duty | Evaluations | Final reference speed | Force error | Moment error | Motor margin | Cone violation |
| --- | --- | --- | --- | --- | --- | --- |
| .5 | 2,927 | .053317 m/s | 1.058361 N | .519551 Nm | -.177474 Nm | 4.735199 N |
| .75 | 2,318 | .046695 m/s | 2.330417 N | .742897 Nm | -.020987 Nm | 2.882873 N |

Together with the two constant-body starts, all four selected initial patterns
have now undergone bounded local joint solves without feasibility. That does
not exhaust the 257 initial configurations, continuous timing, phase counts or
trajectory bases. The separate body8 search is still running as a comparison.

## Corrections and reproduction

The first illustrative test used the quintic variant and yielded an 8.34%
bound. It was incorrectly described in commentary as the robot's old force
layout. The robot uses linear forces; the **2.127%** fixture above is the
corrected comparison. Both results and intermediate failed logs are retained.
The first linear fixture incorrectly split load independently at retained extra
knots; it was corrected to evaluate the analytic complementary ramps. No test
tolerance was relaxed. An earlier compile failure used equality on an enum
without `PartialEq`; the final code uses a pattern match.

The earlier derivative auditor's `linear_templates` case also repeated its
already-linear input. Those recorded audits contain 360 comparisons but only
240 distinct columns across two configurations. Their documents now identify
that limitation. The updated auditor deliberately switches interpolation;
the new 1,098-column check covers three distinct configurations.

`align_joint_contact_forces scene.json markers.json joint-body8-speed.recipe.json --seed-forces`
prepares and audits the shared refinement and optional CAD initialization.
`prepare_joint_force_alignment.mjs` writes `joint-aligned-speed.recipe.json`
and its initial report. `audit_joint_force_jacobian` checks that recipe, and
`check_joint_force_alignment.mjs` verifies the saved comparisons.

The first alignment report is reproduced by the final helper in every original
field; the final helper adds explicit curve-preservation metrics. No runtime
contact force is injected and no controller or browser gait is promoted.
