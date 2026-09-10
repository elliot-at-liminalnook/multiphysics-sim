# Direct force derivatives and alternative stepping patterns

The two constant-body starts completed 3,500 evaluations each. Every motion,
timing and force group changed, but neither produced a feasible gait:

| Initial duty | Final reference speed | Force error | Moment error | Motor margin | Cone violation |
| --- | --- | --- | --- | --- | --- |
| .5 | .058721 m/s | 1.379454 N | .861706 Nm | -.038494 Nm | 2.067255 N |
| .75 | .038057 m/s | 1.300889 N | .531844 Nm | +.415987 Nm | 1.110101 N |

Both made ten accepted inner steps across two outer iterations. The .75 case
shows that a sequential stepping pattern can satisfy the sampled motor margin,
but its forces do not balance the body or satisfy the cones. These local
restoration results do not establish the physical speed ceiling. The two
original-body starts have also completed with the direct-force derivative path,
at their four-outer-iteration limits and below the 3,500-evaluation caps. Both
remain infeasible; [the completed results](FORCE_ALIGNMENT.md) retain the failures.
Their initial inequality vectors exactly reproduce the screen.
`joint-multistart-analytic-launch.json` records their historical launch identities.

## Shared derivative path

At fixed CAD pose, velocity, acceleration and force-knot layout, the wrench and
motor loads are affine in the force coefficients. The new shared
`linearize_joint_forces` API composes those exact CAD maps with each trajectory
basis. It differentiates both signed balance inequalities, signed motor margins
within a fixed torque-sign branch, and unilateral/circular force cones.
Motion columns still use bounded finite differences. Force columns affected by
a zero-torque/nonzero-speed capacity switch also request numerical differences.
Cone apexes use the zero tangential subgradient.

The shared bounded least-squares and inequality augmented-Lagrangian solvers
now accept optional partial Jacobians. They scale original-unit columns to
bounded normalized coordinates, apply the active shifted AL hinge, and retain
actual residual evaluation for trial acceptance. Each derivative callback
counts as one evaluation against both inner and total budgets. Invalid columns
terminate the derivative attempt instead of substituting fabricated values.
The default numerical path remains available and unchanged.

The opt-in native flag is `--analytic-forces`. Results explicitly record the
force derivative mode. This is still dense damped Gauss–Newton inside an
augmented Lagrangian, not a complete sparse TOWR-style constrained NLP. One
stance/swing per foot and restricted foot/body trajectory bases remain.

## Verification and practical limit

All **360 force-column comparisons** pass central differences in the recorded
audit. A later coverage check found that its `linear_templates` case repeats
the perturbed-reference case, since these robot recipes already use linear
forces: there are 240 distinct columns across two configurations. The newer
force-alignment audit explicitly switches interpolation for its third case.
Both operating clocks are included. The largest absolute derivative
error is **2.165833e-8**, and error divided by `1 + abs(analytic derivative)` is
at most **6.613939e-9**. No force column falls back in those audit cases. Six
probe reports match full uncached CAD evaluations exactly. The original full
reference report remains identical to the earlier recorded report.

The tests deliberately move audit force nodes away from cone apexes; these
perturbations are neither gait proposals nor physical-model changes. They do
not establish differentiability at contact events or torque-sign switches.
Thirty-two shared solver tests and four phase-planner tests pass, including
mixed-column scaling, fixed variables, callback budget accounting, invalid
derivative rejection and inequality-constrained optimization.

An independent one-step replay of the original numerical binary and the new
default binary is **byte-identical**, including candidate, report and history.
Both use 350 evaluations for one accepted step. The direct-force path uses
112 evaluations for one accepted step; its AL inner cost is 1,687,201 versus
1,913,838, and maximum normalized inequality is 244.431 versus 269.678. Both
remain infeasible. Different steps are expected at hinge/cone boundaries,
where direct subgradients and symmetric numerical probes differ. Fewer counted
evaluations is not a measured overall wall-time speedup: the expensive motion
derivatives remain numerical.

`audit_joint_force_jacobian` reproduces the derivative checks from the unchanged
validation scene, workspace markers and `joint-x25-warm.recipe.json`.
`joint-derivative-one-step.recipe.json` limits the same seed to one outer/inner
step. `check_joint_derivatives.mjs` verifies the complete reports, comparisons
and initial metrics of both finished alternative gait solves.
