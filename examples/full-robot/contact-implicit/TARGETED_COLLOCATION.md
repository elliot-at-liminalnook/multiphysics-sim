# Targeted collocation with a matched control

Targeting previously missed force peaks improves the independent dense audit,
but does not produce a qualified gait or a speed gain. The targeted candidate
reduces the worst dense force error from the matched control's 3.018 N to
1.878 N (37.8%). It still fails force, moment, torque, slip and sampled collision
checks. No runtime/controller or browser gait was replaced.

## Shared implementation

`ContactImplicitConfig.periodic_collocation_phases` optionally supplies sorted
phases in (0,1], ending exactly at 1. It requires analytic periodic cubic motion.
The planner validates the phases, computes times and positive preceding-interval
quadrature weights once, and fixes that grid for the solve. Position, velocity,
acceleration, contact and inverse dynamics still use the same shared Rust path.
The frame cache belongs to that immutable planner, so different time weights
cannot reuse entries across grids. Omitted phases preserve the old arithmetic.

Twelve contact-planning tests pass. The new test checks irregular sample times,
analytic force balance, matching physics at shared times, all-control cache
probes, constant-motion cost/work quadrature and invalid phase rejection. The
previous complete 32-control robot audit reproduces exactly. All 80 initial
targeted frames exactly match the prior 512-point audit at their shared times.
The standard workspace CI includes these tests; this turn ran the focused suite.

The evidence reducer now integrates nonuniform intervals using actual frame
times. Adding a sample does not multiply energy or path cost by counting every
node equally. The uniform reducer's prior output also reproduces exactly.

## Selection and experiment

`prepare_targeted_collocation.mjs` retains all 64 original times. It scores each
previous dense frame by the maximum normalized force, moment or torque violation,
finds the worst missed point in each original interval, and adds the 16 most
violating intervals' points. This yields 80 samples with 32 controls, 576 free
variables and 480 base-balance components. No foot phase, touchdown sequence or
leg identity enters selection. Collision and slip are separately audited, not
silently substituted for this residual selection rule.

Both searches start from exactly the same retained curve and run 60 iterations
at the same final contact model. Bounds, period, target, motor envelopes,
friction, cost scales and acceptance gates are identical. Only collocation times
and their corresponding quadrature weights differ. Independent quality is
compared at the same 512 times and 513 geometry poses:

| Result | Uniform control | Targeted grid |
| --- | ---: | ---: |
| Planning samples | 64 | 80 |
| Maximum force error at planning samples, N | .134153 | .176369 |
| Maximum force error at 512 samples, N | 3.018066 | 1.878240 |
| Maximum moment error at 512 samples, Nm | 1.123588 | .702679 |
| Minimum torque margin at 512 samples, Nm | -.005908 | -.004206 |
| Loaded slip / body path at 512 samples | 94.81% | 93.81% |
| Sampled interlink overlap, µm | 15.205 | 15.193 |
| Planned diagonal travel rate, m/s | .202338 | .202350 |
| Sliding work at 512 samples, J | .284794 | .282799 |

Neither passes the .05 N / .02 Nm / .001 Nm planning gates or the development
5% slip limit. Both terminate at iteration limits with accepted final steps and
nonzero projected gradients. Initial/final costs cannot be directly compared
between grids because numerical quadrature differs. No local optimum or physical
speed ceiling is established.

The original largest force error was at .774 ms. At that point, targeted
optimization reduces the largest force component from 3.345 N to .070 N. The
new dense maximum moves to 2.321 ms, which was not among the selected points.
`targeted32-peak-movement.json` records this movement. The extra samples address
their selected failures, but one fixed selection cannot certify the whole curve.
Every optimized frame still exactly matches its independent dense counterpart.

## Next formulation work

The next solve needs iterative coverage of newly exposed contact transitions
and stronger enforcement of base balance. This is still weighted-penalty
Gauss–Newton. IDTO Section V-B explains the residual feasibility limitation of
quadratic penalties and develops a constrained Newton/dogleg step using Lagrange
multipliers. Its equations 16–20 provide the next solver reference, not a feature
already implemented here. Keep bounds, contact-query accuracy and rank-deficiency
handling explicit when contributing that solver to `sim-solve`.

Primary reference, rechecked this turn:
[Kurtz et al., equations 16–20 and Remark 3](https://arxiv.org/html/2309.01813v2#S5.SS2).
The paper's solver does not remove the need for independent between-sample and
detailed-runtime validation. Sharing the periodic parameter mapping with
derivative diagnostics remains necessary before auditing those solver steps.

Low slip, geometric clearance, periodic entry/repetition and responsive WASD
remain required. Physical speed maximum and sim-to-real accuracy remain unproven.
Direct evidence v9 records this work; existing runtime archives are unchanged.
