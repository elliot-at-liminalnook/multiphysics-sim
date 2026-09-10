# Hard servo-command limits in force planning

The 2,000-model native pilot completes its one-iteration check after 765 model
attempts (native iteration limit -1). Its final constraint vector exactly
matches the independent full CAD audit. The returned 0.0252074603 m/s motion
still violates a nominal servo-command bound by 0.0040717610 rad. This is a
validated integration check, not a faster or feasible runtime gait.

The shared force optimizer now enforces explicit nominal servo-command bounds
as hard linear inequalities at every collocation frame. They are independent
of the minimax balance variable, so the optimizer cannot accept an excessive
command by paying a larger balance error. Force boxes and circular friction
cones are unchanged. Torque-speed and geometry constraints still participate
in the independent physical audit, rather than the conic subproblem itself.

A common affine-force mapping routine supplies both the original six-component
wrench Jacobian and the new upper/lower command Jacobian. The latter reuses
CAD motor load maps and `EffectiveServo::reference_target`; it does not subtract
loaded reports or differentiate nonsmooth torque-capacity switches. Selected
force variables may be reordered or partial; nonselected loads remain in the
affine offset. A returned primal solution receives independent wrench and
command-map checks against full CAD evaluation. Without command limits, the
prior conic result is byte-identical.

## Result and next speed search

At the pilot's fixed motion, Clarabel reports **PrimalInfeasible** after 15
iterations with all **6,576** nominal command inequalities enforced. No primal
candidate or physical report is fabricated from the infeasibility ray. This
is floating-point evidence for this motion, force basis, force boxes and
contact model; it is not an interval certificate or global physical speed limit.
It motivates changing motion as well as forces in the next joint solve.

The full joint search starts from the checked pilot candidate with the same
491 decisions and 274 planning frames. It retains all previous bounds,
friction, geometry and actuator checks, including the newly explicit command
limits and four observed IK-failure phases. The experimental target remains
0.30 m/s. The run allows 100 native iterations and 20,000 model evaluations;
that computational budget is not a task time limit or physical stopping rule.
See `joint-servo-command-speed-launch.json`. Its live results are not a
completed gait. The older eight-control search also continues independently.

## Verification and reproduction

Seven shared planner tests pass, including an analytic force optimum limited
by a hard command row and an impossible constant command constraint that cannot
be repaired with balance slack. The independent command-map audit covers all
366 force columns and 183 reordered partial columns with three bounded force
probes each. Maximum discrepancy is **1.13686838e-13** in normalized command
inequalities. Both zero-selected-force baselines reproduce full CAD reports
byte for byte, and duplicate variables reject. The map's reference report also
matches the completed native pilot's final report exactly.

`check_joint_servo_conic.mjs` verifies the native snapshots, affine audits,
legacy replay, conic status and unchanged next-search input.
`record_joint_servo_conic.mjs` records source overlays, exact executable and
input hashes, commands, statuses and the native launch. Remote CI has not
been observed. No new measured gait speed or hardware-transfer claim follows
from this work.
