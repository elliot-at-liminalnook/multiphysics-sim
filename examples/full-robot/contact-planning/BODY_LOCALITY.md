# Compact support for joint body derivatives

The completed timing-aware AL motion does not sit against its experimental
lateral body-control bounds: normalized X controls range approximately
0.278–0.500 and Y controls 0.218–0.783 within their existing [-0.015, 0.015] m
intervals. This gives no evidence that simply widening those bounds will solve
its balance errors. Pitch-like rotation-vector controls approach their lower
bound, but that alone is not permission to relax runtime tilt acceptance.

The current native Jacobian instead spends up to two full CAD evaluations per
motion variable. Its 48 body-control variables account for 96 of 154 ordinary
motion probes, before domain failures or other fallbacks. A periodic cubic
B-spline's value and first two derivatives use only four neighboring controls.
At a fixed sample time, body controls with the same index modulo four have
disjoint support when the unique control count is a multiple of four.

The shared `Trajectory.periodic_support_controls` exposes those four indices
using the same cell calculation as trajectory sampling. It is conservative at
knots and handles the repeated endpoint as an alias of control zero. It applies
to fixed time/knot locations, not arbitrary simultaneous timing changes. The
new unit case perturbs every control in eight- and sixteen-control curves over
two cycles and checks exact equality of value, rate and acceleration outside
the declared support. Existing analytic trajectory tests remain applicable.

The CAD planner independently reconstructs its seed at each phase from the
same fixed initial state and sampled body pose. It does not continue the IK
seed from one frame to the next. Body-control values do not change the sampling
mesh, contact events, force knot times, or the speed-only objective. Therefore
this local support also applies to each frame's inverse dynamics, motor and
geometry rows while body values alone are perturbed. Force-node cone rows are
independent of body controls.

`audit_joint_body_locality` tests this declaration on every body channel, every
modulo-four group, both perturbation signs, every collocation frame and both
operating clocks. Perturbations use the optimizer's 1e-4 normalized step and
physical bounds. It computes individual and simultaneous perturbations through
full **uncached** CAD evaluation. Every frame field must serialize identically
between grouped and independent evaluation, and every individual probe must
leave frames outside its support identical to baseline. It rejects overlapping
groups or changed frame counts. The full baseline physical report is retained
for comparison with the pre-refactor result.

For eight unique controls this would reduce the body's two-sided probes from
96 to 48, and ordinary motion probes from 154 to 106. These are predicted
probe counts, not measured optimizer or browser speedups. Integration must
recompute local support whenever timing changes, keep Ipopt's full structural
Jacobian declaration, and handle invalid combined probes without silently
losing derivatives. No grouped differentiation is enabled in live searches by
this audit. No gait, physical limit, controller or acceptance gate changes.

## Completed evidence

Both recorded motions pass: the 0.2117102645 m/s fast reference and the
0.025 m/s completed AL motion. Each audit performs 145 uncached CAD evaluations,
48 signed/group cases and 12,000 combined-frame comparisons, plus the associated
individual outside-support checks. All frame comparisons are byte-identical.
The complete baseline reports also match the pre-refactor native reports after
signed-zero canonicalization. Wall times were 56.8 s and 71.1 s under concurrent
load; these are audit timings, not optimizer performance measurements.

Fourteen control unit tests and thirteen analytic trajectory integration tests
pass. The native-solver CI workflow now covers control changes, executes those
trajectory checks and compiles the CAD locality auditor. Its full CAD executions
were performed locally and archived; remote CI has not been run here. Both live
optimizer executable hashes remain unchanged.

Grouped numerical body differentiation is now implemented in the
shared joint NLP; see [the native integration and pilot](GROUPED_BODY_DERIVATIVES.md). Grouping is valid only for body-value probes at the current
fixed times; it must fall back to individual probes if a combined trial is
invalid. The permanent native Jacobian pattern must continue including entries
that can become nonzero as contact timing moves. A full native pilot should
compare derivatives and returned physics before relying on a throughput gain.
