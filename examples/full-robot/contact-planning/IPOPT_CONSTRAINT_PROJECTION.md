# Constraint representation experiment

This opt-in experiment changes the numerical representation of the same joint
CAD problem. It does not change any physical acceptance threshold, geometry,
actuator, motion variable, search bound, speed objective or contact schedule.

The previous native problem contains 32 identically zero cone inequalities:
first and last force coefficients are exactly zero for every foot and clock,
as required by `PreparedForces`. These rows have no decision-variable
influence. It also contains 250 nonnegative interlink penetration inequalities,
each required to be zero. Unlike a signed clearance constraint, this clamped
quantity is flat throughout separated configurations and has no strictly
negative feasible values. These are formulation concerns, not proof that they
explain the observed convergence failures.

`JointIpoptConfig.collision_free_domain = true` removes the 32 fixed endpoint
rows and represents the 250 collision rows as an explicit evaluation domain.
Every motion evaluation, analytic linearization and numerical derivative probe
checks all original rows. Any nonzero collision penetration is rejected,
including values below the normalization tolerance. Nonzero endpoint rows,
nonfinite values and changed dimensions are errors. A colliding initial guess
is rejected before native optimization. The returned candidate still receives
an independent uncached audit of all 6,776 original physical inequalities.

The native vector contains the remaining 6,494 inequalities in original order.
The result records the full row count and every omitted original row index.
The Jacobian uses the corresponding fixed structural projection; it does not
drop entries because a numerical derivative happens to be zero. Numeric probes
continue differentiating the full report, so projecting row indices cannot
change their physical meaning. The default is false, preserving earlier runs.

For source-valid candidates, this has the same sampled feasible set: fixed
endpoint rows equal zero by construction, every allowed trial has zero sampled
interlink overlap, and all other inequalities remain unchanged. It does change
the search path: Ipopt can no longer cross a sampled colliding configuration or
repair one supplied initially. No signed collision distance or useful collision
boundary gradient is added. Domain rejection can itself impede a local search;
other nonsmooth terms, fixed contact ordering and collocation limitations remain.

Two focused tests check row identities, exact acceptance equivalence on the
valid domain, rejection of arbitrarily small overlap and corrupted endpoint
identities, finite/dimension guards and preservation of the legacy vector.
A one-iteration comparison and legacy replay use the same real CAD problem and
native library as the original pilot. `check_joint_ipopt_projection.mjs` checks
full initial-report fidelity, all retained native values against the independent
final audit, zero omitted rows, and preservation of all prior legacy result
fields except explanatory scope text. Results are recorded separately; a pilot
is not a convergence result or measured speed gain.

The native interface follows the documented requirement that Jacobian structure
remain fixed and include every potentially nonzero entry:
https://coin-or.github.io/Ipopt/INTERFACES.html

## Completed result

All seven shared contact-planning tests pass in release mode with the native
feature. Both CAD pilots completed at the requested one-iteration limit with
316 model attempts and six native callbacks. The legacy replay preserves every
previous result field except explanatory scope text, after signed-zero
canonicalization. The projected initial report is identical; all 6,494 returned
native inequalities equal the corresponding independent full-audit rows, and
all 282 omitted rows are exactly zero (either sign).

| One-iteration pilot | Original | Projected |
| --- | ---: | ---: |
| Speed (m/s) | 0.2116282431 | 0.2116278728 |
| Maximum force error (N) | 12.46487177 | 12.46487499 |
| Maximum moment error (Nm) | 3.08004481 | 3.08004473 |
| Minimum torque margin (Nm) | -0.26901585 | -0.26901458 |
| Maximum normalized inequality | 248.29743530 | 248.29749974 |
| Structural Jacobian entries | 1,599,232 | 1,579,982 |

Both are infeasible and numerically very similar. This provides no evidence of
a useful convergence or speed improvement. No long projected search is launched
on this evidence; the original full-budget native search continues unchanged.
The option remains available for further formulation comparisons. A signed
clearance formulation, richer contact families, faster motion derivatives and
better feasible initialization remain separate, uncompleted work.

The first JS verification attempt distinguished -0 from +0 using strict scalar
assertion. The corrected omitted-row check uses numeric equality to zero, as the
Rust gate does; it still rejects every nonzero value. No solver result or
physical acceptance was altered to address this verification-only issue.
