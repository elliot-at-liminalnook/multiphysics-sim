# Derivative recovery and measured finer-grid tracking

This extends commit `84bca35`. The finer-grid planner now passes its declared
planning tolerances and runs at **.16837 m/s** in the detailed runtime, up from
.15804 m/s at matched clocks. It still slips excessively and covers only .396 s
from rest. This is not a qualified gait, browser update, or physical speed ceiling.

## A measured numerical failure

The earlier damping-limit failure was not sufficient evidence that the penalty
formulation alone caused the stall. The new shared `central_difference_audit`
measures residual derivatives, changes between probe sizes, symmetric Taylor
remainders, and forward/backward cost slopes. Tests verify second-order behavior
on an analytic polynomial and expose a discontinuity without fabricating residuals.
`audit_contact_derivatives` applies it to all 432 free CAD trajectory coordinates.

The stalled trajectory's normalized 1e-7 and 1e-9 cost gradients have cosine
**-.177912**: they disagree even about descent direction. For the direction formed
from the 1e-7 gradient, the predicted slope is **-22,176.8**, but an independent
small directional probe measures **+6,175.3**, uphill. The direction formed from
the 1e-9 gradient predicts **-40,577.3** and measures **-40,577.3**; at a 1e-9
forward step its cost actually decreases. The largest component sign reversal is
at knot 2, coordinate 17: -4,867.49 versus +8,086.27.

Several large derivative changes occur in the motor-envelope penalty rows. A
central probe can cross an activation boundary even when the current iterate is
on one side of it. Refining only from 1e-6 to 1e-7 did not resolve the issue in the
previous turn. Most residual columns looking stable also did not certify the
combined cost gradient, whose components involve cancellation. These diagnostics
are local evidence, not a universal optimal finite-difference step or proof that
all model derivatives are accurate.

The final build exactly reproduces both exploratory diagnostics: all coordinate
samples in `stalled-derivatives.audit.json`, and the full coordinate/directional
report in `stalled-descent.audit.json` match `stalled-descent.replay.json`.
Original intermediate binary identities were not recorded; the final replay's
source/binary identities are retained in `derivative-refinement-build-identities.json`.

## Shared automatic recovery

`bounded_least_squares_scaled_refining` accepts an optional explicit derivative
floor and reduction factor. A damping-limit stall shrinks the probe, resets
damping, and retries from the same retained evaluated state within the existing
total iteration/evaluation budgets. It records probe sizes and refinement count.
Exhausting the floor still reports failure. Calls without this option retain the
old arithmetic/serialization. No physical gate, cost weight, bound or contact
sequence is changed by this mechanism.

An analytic regression uses a rapidly rotating residual vector with exact cost
.5*(1+x)^2. A wide finite difference reverses its gradient and the original solver
stalls at x=0. Automatic refinement produces a verified decreasing, bounded step.
All 13 `sim-solve` unit tests and seven contact-implicit integration tests pass.
The release planner, compiler, geometry auditor and derivative auditor build.

`resolved-work-finegrid-adaptive` starts from the retained failed derivative case,
with its identical 24-interval problem and .001 Nm torque tolerance. The explicit
probe floor is 1e-9, reduction factor .1. It actually refines once, from 1e-7 to
1e-8 at iteration 18, and uses the remaining 100-iteration total budget. This is
local recovery, not the full IDTO equality-constrained dogleg solver or MPC.

Final cost is 11.609834, versus 11.955850 initially. Its independent uncached audit
exactly matches the optimizer's report: force error .032756 N, moment error
.015437 Nm, minimum torque margin **-.00026505 Nm**, all within declared .05 N /
.02 Nm / .001 Nm tolerances. Negative margin is a small allowed planning error,
not strict positive actuator headroom. The runtime still uses exact saturation.
121 CAD geometry poses show zero interlink overlap and .13180 mm maximum floor
penetration. Planned mean speed .185303 m/s and loaded slip71.85% are diagnostics.
The compiler accepts this path only after independently repeating these gates.

## Detailed runtime evidence

The 25-knot plan uses four policy samples per interval, preserving the earlier
4.125911 ms controller period and 97 reporting poses. Physics steps are .515739
and .257869 ms (32/64 substeps per plan interval). The preparation tool's optional
policy-sample count keeps this comparison fair when changing planning resolution;
its previous eight-sample default remains. The analysis reports actual knot counts.

| Measured quantity | .515739 ms physics | .257869 ms physics |
| --- | ---: | ---: |
| Mean +45-degree body speed, m/s | .168589 | .168373 |
| Terminal body speed, m/s | .129871 | .130626 |
| Maximum joint error at 25 knots, rad | .016474 | .016819 |
| Maximum body error at 25 knots, mm | 9.879 | 10.005 |
| Loaded material slip / body path | 74.67% | 75.08% |
| Sampled interlink overlap | 0 | 0 |
| Maximum sampled floor penetration, mm | .28060 | .31448 |

Both runs complete without task-bound termination. Halving the physics timestep
changes the body path by at most .15965 mm and mean speed by .12847%. Compared
with the earlier .257869 ms `resolved-work-ff128` run, mean speed improves6.54%,
joint tracking error falls from .043269 to .016819 rad, and slip falls from99.88%
to75.08%. The earlier errors were sampled at13plan knots, current errors at25.
`adaptive-finegrid-comparison.json` verifies identical typed CAD/physics options,
effective actuators, initial pose, controller source, environment policy, and
report times. The trajectory, planning resolution, torque tolerance/weight and
derivative handling differ; the improvement cannot be attributed to each in
isolation.

No browser bundle changes. Slip still fails the5% development criterion by a wide
margin. The planning point-contact model still differs from the runtime's friction
patch, and neither is newly hardware-calibrated. These short references also lack
terminal viability, a repeatable cycle, sustained speed or WASD response checks.
Runtime inputs/captures/geometry are durably retained in incremental runtime
archivev3 on top ofv1+v2; direct evidence has its own commit-scoped manifest.

## Next work

Keep this derivative failure as a regression case when improving constrained
whole-body optimization. Enforce base balance separately from cost, retaining
actuator/contact constraints and derivative diagnostics. Extend the planning
horizon and add terminal/periodic viability so contacts can emerge over repeated
steps. Smooth reference interpolation and further grid convergence need explicit
validation: linearly subdividing positions produces alternating discrete
accelerations and cannot by itself certify a continuously feasible control path.
Continue through the same Rust/Rhai runtime, slip/collision checks and responsive
WASD qualification. A solver stall is not a demonstrated physical speed limit.

The [IDTO paper](https://arxiv.org/html/2309.01813v2), Remark3, warns that derivative
accuracy through contact is crucial. Its constrained solver, temporal sparsity
and receding-horizon feedback remain relevant work beyond this local recovery.
