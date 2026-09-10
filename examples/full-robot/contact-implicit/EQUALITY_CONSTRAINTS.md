# Equality-constrained contact-implicit search

The shared planner now has an equality-constrained search path, following
[IDTO Section V-B, equations 16–20](https://arxiv.org/html/2309.01813v2).
The six unactuated base-wrench components at each sample are explicit normalized
equalities instead of objective penalties. Contact timing remains implicit in
whole-body motion and the smooth contact law; no leg phase sequence is supplied.
**No new candidate passes physical validation.** The .21 m/s browser still uses
the previous prescribed-phase planner. This work does not establish a physical
speed maximum, online MPC performance, or sim-to-real accuracy.

## Shared implementation and departures

`sim-solve::equality_dogleg` uses finite-difference residual Jacobians, scaled
Gauss–Newton curvature, KKT constrained steps and a frozen-multiplier Lagrangian
dogleg merit model. The diagonal exponent is 1/4. Exactly fixed parameters are
eliminated. Other box bounds shorten steps; this is not a full active-set method
and can stop at a bound without establishing optimality.

The original Schur fallback lost five constraint directions on this robot.
Factoring H = L Lᵀ and decomposing A L⁻ᵀ directly recovers those directions without
squaring the condition number. Every full step is checked against the original
KKT equations; inconsistent or inaccurate solves stop with diagnostics.

A separate `exact_penalty_newton` option uses the exact-L1 equality merit with a
nondecreasing weight above the multiplier infinity norm, radially shortened
Newton steps and trust-ratio acceptance. This is an experimental globalization
alternative, not the paper's signed-Lagrangian dogleg or an exact reproduction
of upstream's optional fixed-weight backtracking implementation. See the
[pinned upstream implementation](https://github.com/ToyotaResearchInstitute/idto/blob/de0629c7811aa9b330e56c4385629005b09495f0/optimizer/trajectory_optimizer.cc).

`ContactImplicitParameters` shares periodic/startup variable encoding across
both solver paths and derivative audits. `equality_residuals` removes precisely
the six base-wrench penalty rows per frame, retaining the tracking, motor,
penetration and optional per-point sliding-work objective rows. The physical
report is independently evaluated after optimization.

## Matched robot trials

All three runs start from `targeted32-recorded`: 32 C2 periodic controls, 80
collocation samples, 576 free parameters and 480 equality components over
.39608749 s. CAD, +45° direction, contact law, bounds and .25 m/s numerical target
are unchanged. Each permits 30 iterations/200,000 evaluations. The equality
threshold .1 corresponds to .005 N/.002 Nm, stricter than the physical planning
gates .05 N/.02 Nm. The linear residual threshold remains 1e-6.

| Solver | Stop | Dense force error N | Dense moment error Nm | Dense torque margin Nm | Dense loaded slip |
| --- | --- | ---: | ---: | ---: | ---: |
| Initial Schur | Linear check; no step | 1.878240 | .702679 | −.004206 | 93.81% |
| Whitened dogleg | 30 iterations | 33.108223 | 8.280432 | −.013774 | 36.94% |
| Whitened exact-L1 | Linear check after 23 steps | 1.837673 | .681857 | −.031938 | 90.85% |

The first failure reports rank 475/480 and relative KKT error .0062613. Whitening
retains all 480 directions, with a final linear residual 1.28e-10 in the dogleg
run. However, its nonlinear force error worsens sharply while objective cost
falls from 7.5784 to 3.4356. Accurate linear solves and reduced objective cost do
not demonstrate physical feasibility.

Exact-L1 modestly reduces sampled force error from .176369 to .170793 N, then
stops when the next full-rank linear solve has relative residual 1.47803e-6,
above the unchanged 1e-6 requirement. The dense audit still finds much larger
force error and a worse torque violation. Its planned displacement rate is
.201115 m/s, compared with the starting .202350 m/s; neither is measured walking
speed. All trials retain about 15–15.4 µm sampled interlink overlap. None meets
the force, moment, torque, 5% slip and collision requirements together.

## Verification and evidence

- 19 shared solver tests pass, including constrained affine and nonlinear
  optima, consistent/redundant versus inconsistent constraints, infeasible
  bounds, preservation of a weak constraint direction, both globalizations,
  and nonfinite residual handling.
- 13 contact-implicit integration tests pass, including residual partitioning,
  parameterization validation and a frictionless unactuated-body case that
  cannot satisfy requested acceleration by inventing a base force.
- The initial derivative audit covers all 576 parameters, including cycle
  displacement. The largest relative derivative change from probes 1e-8 to
  1e-9 is about 1.56e-5. The directional slope approaches the predicted descent
  as the probe shrinks. This audits the original full planning residual vector;
  it is not a separate certificate for the partitioned KKT Jacobian.
- `node examples/full-robot/contact-implicit/summarize_equality_trials.mjs`
  reproduces `equality32-summary.json`, asserts exact independent final-report
  agreement and exact physical-frame agreement at shared times on the 512-point
  audits. Loaded slip integrates each foot's normal-force-weighted horizontal
  material speed when total normal force is at least 1 N, divided by body XY
  path length, taking the worst foot.

Recipes, native output, logs, tests and three separate source/binary identity
files preserve the unsuccessful experiments. Initial code is in `1082997`;
whitened factorization is in `6349d7f`; the exact-L1 variant accompanies this
report. The turn began at `233402f`. The original failed solve's dense evidence
is its identical parent's `targeted32-recorded-dense.audit.json`.

Next numerical work should diagnose primal and dual KKT residuals separately
and test iterative refinement against the original equations. Do not relax
checks to accept this failure. Dense contact-transition coverage, bound
handling, slip/collision constraints and runtime entry/tracking remain open.
