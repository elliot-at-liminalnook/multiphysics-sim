# Refining the motion basis and collocation grid

The shared trajectory library now doubles periodic cubic controls by knot
insertion. It preserves the initial position, velocity and acceleration curve
instead of sampling the old curve as new controls. This lets experiments add
motion freedom independently of their initial guess and physics sampling.

`Trajectory::refined_periodic_config` uses the uniform cubic refinement masks
((previous + 6 current + next)/8, (current + next)/2), evaluated around the
current value to reduce cancellation. `PeriodicDriftTrajectory` preserves cycle
displacement, and `ContactImplicitPlanner::refine_periodic_controls` converts
the refined residual controls back to the translating representation. These are
shared components; no robot-specific dynamics or contact schedule is added.

The two shared trajectory tests pass. The refinement test checks three successive
doublings of an odd-sized, two-channel periodic curve at 2,001 times across two
cycles, comparing position, velocity and acceleration. Eleven contact-planning
tests also pass. Robot audits compare matching times before and after refinement:
position changes at most 2.23e-16, velocity 6.03e-15 and acceleration 5.90e-13 in
their respective coordinate units. Maximum base-wrench changes are below
3.27e-11. These are floating-point differences, not a changed initial motion.

## Sixteen controls at the same physics times

Two searches refine the previous uniform and recorded-gait solutions from eight
to sixteen controls while retaining 32 collocation times. They use 60 additional
iterations at the final contact model. Period, target, friction, stiffness,
actuator parameters, cost scales and physical gates remain unchanged. The
existing XY window is retained relative to the interpolated reference; its first
control gauge is assigned the exact refined value without translating the curve.
All retained constant height/orientation/joint bounds and displacement bounds
remain unchanged. No refined seed is clipped to fit bounds.

There are now 288 free variables and 192 base-balance components. More numerical
freedom improves force balance, but does not establish feasibility:

| Initial guess | Planned rate m/s | Force error before / after, 32 samples N | Force error, 128 samples N | Moment error, 128 samples Nm | Loaded slip / body path |
| --- | ---: | ---: | ---: | ---: | ---: |
| Uniform | .100012 | 1.430 / .730 | 1.207 | .851 | 258.6% |
| Recorded gait | .202232 | 2.771 / .262 | 3.371 | 1.338 | 96.4% |

At 128 samples, minimum torque margins are -.046427 / -.010313 Nm. The gates
remain .05 N, .02 Nm, .001 Nm torque violation and the development 5% slip limit.
Sampled leg overlaps reach 31.75 / 16.00 µm at 129 geometry poses. Both searches
terminate at their iteration limits, with accepted final steps and nonzero
projected gradients; neither establishes a physical limit or local optimality.
Independent uncached reports reproduce the final optimizer reports exactly, and
all physics fields are identical at times shared with the 128-sample audit.

## Further mesh refinement

The recorded candidate is also refined to 32 controls with 64 collocation times,
adding both motion freedom and new physical checks. The initial 64 samples match
the same times in the previous dense audit up to roundoff. Its initial force and
moment errors are 2.643 N and .919 Nm: those errors were already present between
the old planning samples. The comparison retains the same final physical model,
period, target and acceptance gates. Its final results are recorded separately
in `smooth32-recorded.summary.json` and the independent dense audit.

After 60 iterations, planned travel is .202335 m/s with 97.3% loaded slip.
Force/moment errors at 64 samples are .131172 N / .037661 Nm; both still fail.
At 128 samples they are 1.616277 N / .588533 Nm, and a 512-sample check finds
3.345274 N / 1.247955 Nm. The 512-sample torque margin is -.006546 Nm. Geometry
at 129 poses finds 15.29 µm leg overlap. None is a qualified gait. Cost falls
from 96.430 to 8.234, but the run ends at its iteration limit, not convergence.

The 512-sample audit localizes the largest force and moment errors to .774 ms
after the cycle seam, inside the first 6.189 ms planning interval. At a loaded
point on the +Y foot, normal force stays near 21.49 N while lateral velocity
changes from +.000223 to -.002100 m/s. Its lateral friction force changes from
-1.170 to +4.850 N. The authored .001 m/s stiction regularization is unchanged.
This directly exposes a rapid loaded friction reversal between planning samples;
smooth joint motion alone does not make contact force vary slowly.
`smooth32-force-peaks.json` and `smooth32-stiction-transition.json` retain the
point-level evidence. Physics at all shared times is exactly equal across the
64-, 128- and 512-sample reports; the discrepancy comes from new sample times.

## Remaining work

No result here is a measured walking speed. Detailed runtime, stable periodic
entry/repetition, collision avoidance, low slip and responsive WASD remain
unqualified. The current solver still trades weighted penalties; IDTO's
equality-constrained solver and online MPC remain unimplemented. Before using the
existing derivative diagnostic on these modes, share the periodic parameter
mapping with it: that example currently assumes a fixed-start layout.

Further work must address force peaks between planning samples and the large
loaded slip, not merely lower the sampled objective. In particular, select
additional collocation times near loaded friction reversals and retain enough
trajectory freedom to satisfy their balance constraints. Do not widen the
physical stiction parameter to hide these peaks. Numerical mesh size is not
a hardware bandwidth limit. CAD-derived power and conditional rate bounds do not
yet establish a global physical speed maximum. Existing browser gaits, CAD and
the detailed runtime model remain unchanged.
