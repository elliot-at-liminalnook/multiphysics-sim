# Finer-grid contact-implicit planning

Follow-up: [derivative diagnostics and recovery](DERIVATIVE_RECOVERY.md) identify
a reversed numerical gradient, recover planning feasibility and improve detailed
runtime startup motion. The earlier solver diagnosis below was incomplete.

This extends commit `0d2cd75`. Shared evaluation caching is verified and makes
finer-grid optimization practical. Four new searches remain infeasible; no new
runtime walk, browser candidate, speed record or physical maximum is claimed.

## Exact shared cache

`ContactImplicitPlanner::evaluator` owns a bounded cache tied to one immutable
planner. Each time-step entry keys the current and previous position and the
previous spatial velocity by their exact floating-point bits. This captures the
three-knot dependency, including the rotation-vector-to-angular-velocity mapping.
Contact continuation stages cannot share entries. A changed input recomputes its
affected frames. Invalid inputs still fail validation. Final optimization reports
and the standalone auditor use uncached evaluation.

Residuals and frame diagnostics are assembled in their original order. Contact
work retains individual point contributions to preserve floating-point summation
order. All seven contact-implicit integration tests pass, including exact report
comparison for forward/backward probes at every future coordinate, rotations,
initial velocity, optional friction-work cost, endpoint reuse and invalid inputs.
The full retained `resolved-contact-work` CAD audit is exactly unchanged from its
pre-cache version (`cache-regression.audit.json`).

`benchmark_contact_implicit` checks the actual 25-knot CAD path, its baseline and
both signs of every future coordinate perturbation: **865 exact serialized report
matches**. Evaluation took 5.823858 s uncached and .737830 s cached, **7.8932×**.
2,531 frames were computed and 18,229 reused. Serialization and equality checks
are outside these timers. This is one local wall-time benchmark, not a complete
optimizer speedup or an MPC/realtime claim. Binary and source hashes are retained
in `finegrid-build-identities.json`.

## Finer-grid results

The original .39608749 s path has 12 intervals of 33.007291 ms. Its existing linear
subdivision audit fails at 45.281532 N force error, 10.547389 Nm moment error and
-1.713335 Nm minimum torque margin. `prepare_finegrid_trial.mjs` instead builds a
24-interval, 16.503645 ms optimization problem with 432 free coordinates. It keeps
the duration, fixed initial state, +45-degree .25 m/s target, reference-relative
body bounds, absolute joint bounds, resolved material/contact parameters and
friction-work normalization. There is no contact schedule.

| Trial | Force error N | Moment error Nm | Min torque margin Nm | Planned loaded slip | Termination |
| --- | ---: | ---: | ---: | ---: | --- |
| `resolved-work-finegrid` | .029604 | .007411 | -.011570 | 85.17% | 100 iterations |
| `resolved-work-finegrid-refinement` | .015030 | .006192 | -.011032 | 72.59% | 100 more iterations |
| `resolved-work-finegrid-torque` | .117147 | .048742 | -.000978 | 72.55% | Damping limit, 3,478 evaluations |
| `resolved-work-finegrid-derivative` | .117452 | .050535 | -.000800 | 72.55% | Damping limit, 19,905 evaluations |

The first two keep .05 N / .02 Nm balance tolerances and .01 Nm torque tolerance.
They fail the torque gate. The third tightens torque tolerance/residual scale to
.001 Nm; it passes that gate but fails force and moment balance. The fourth keeps
that entire problem and reduces the normalized derivative probe from 1e-6 to
1e-7; it still fails force and moment balance. Acceptance was never loosened.
The derivative change is a solver diagnostic, not evidence of derivative accuracy.

Each final report exactly matches its independent uncached audit. Each samples
121 full CAD poses with zero interlink overlap and about .132 mm maximum floor
penetration. Planned mean displacement rates are .18528–.18530 m/s; these are
**infeasible planning diagnostics**, not measured robot speeds. Sliding work falls
from .61543 to .51868 J, but slip remains far beyond the development 5% criterion.
No reference is compiled or executed from these failed results.

## What remains

The results expose the limitations of weighted balance penalties, not a physical
speed limit. The current solver is bounded damped Gauss–Newton with diagonal
Hessian scaling; it is not IDTO's equality-constrained dogleg/KKT solver or MPC.
Next, use these retained cases to validate directional derivatives and implement
shared equality-constrained balance with bound-aware globalization/restoration.
The analytic tests must cover feasibility separately from cost reduction, bounds,
rank/conditioning and rejected steps. Do not replace failure with a fabricated
residual, loosen physical gates or resume prescribed gait timing sweeps.

Then reoptimize and audit the finer grid, verify further time-grid consistency,
execute only eligible paths in the shared runtime at matched controller/physics
clocks, and extend to longer horizons with terminal/periodic viability. The .396 s
startup paths cannot establish sustained speed or responsive WASD control.

The methodological source is [IDTO](https://idto.github.io/) and its
[paper](https://arxiv.org/html/2309.01813v2): contact emerges through whole-body
optimization; its constrained solver, sparse temporal structure and receding
horizon feedback are distinct steps beyond this implementation. PLANC's learned
teacher/student pipeline remains unimplemented here.
