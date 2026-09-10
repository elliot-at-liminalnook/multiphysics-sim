# Slip optimization with physical tolerance inequalities

The first robot trial still fails qualification. Its coarse force error falls
to 0.148 N, but the dense audit reports 0.200 N, 0.0718 Nm moment error, a
-0.00295 Nm torque margin and 72.65% worst-foot loaded slip. The formulation is
implemented and analytically checked; this finite trial is not a successful
constrained gait, a speed record, or evidence of a physical speed maximum.

The previous weighted continuation traded slip against force and moment error.
This experiment instead treats the existing sampled physical gates as signed
inequalities and minimizes the same conservative slip-shaping objective. It
does not require the six unactuated wrench components to be exactly zero.
No contact schedule is supplied, and the CAD model, contact law, displacement,
actuator model and external acceptance gates are unchanged.

## Shared formulation

`sim-solve::inequality_augmented_lagrangian` minimizes a least-squares objective
subject to explicit variable bounds and dimensionless inequalities `c(x) <= 0`.
It uses the inequality augmented-Lagrangian derivation in
[Section 14.6, equation 46 of the Stanford-hosted optimization text](https://web.stanford.edu/class/msande310/310trialtext.pdf).
For penalty `rho > 0` and nonnegative multipliers `lambda`, each inner residual
is `sqrt(rho) * max(c + lambda/rho, 0)`. The omitted term
`-sum(lambda²)/(2*rho)` is independent of the inner variables. Multipliers update
as `max(lambda + rho*c, 0)` after each bounded LM subproblem. The penalty rises
when the shifted constraint norm fails to fall by the specified factor.

This is an additional numerical method for the IDTO-inspired offline planner,
not a claim to reproduce IDTO's solver or its online MPC implementation. The
inner solver remains the shared scaled bounded LM. Finite local subproblems
can remain infeasible. The result reports violation, complementarity, per-outer
penalty and inner diagnostics, and counts every model call against a total
evaluation budget. Stationarity within the solver's normalized tolerances does
not override the separate physical acceptance checks.

The runtime adapter uses one inequality per contact-point penetration, two
signed inequalities per force/moment component, and one per actuator margin:

- `(-gap - maximum_penetration) / penetration_scale <= 0`
- `wrench / tolerance - 1 <= 0` and `-wrench / tolerance - 1 <= 0`
- `-torque_margin / torque_tolerance - 1 <= 0`

The report supplies gaps, velocities, wrenches and torques. Capacity comes from
the existing effective-servo component, exactly as in the physical report.
Collision geometry and actual loaded-slip acceptance remain independent gates.
The four existing slip shaping residuals are the objective, not constraints.

## Matched robot experiment

`inequality32.recipe.json` starts from the same `slip32-shaped05` motion as
`slip32-coupled-control`, retaining its 32 controls, 144 check times, explicit
bounds, period, displacement and slip ratio scale 0.05. The new search uses
three outer iterations, each capped at ten LM iterations, with initial penalty
1, growth factor 10, and required constraint reduction 0.5. The total budget is
200,000 model calls. Its normalized constraint and complementarity convergence
tolerances are 1e-6; these do not relax any physical gate.

The old control is a single thirty-iteration weighted solve. Matching the total
iteration cap does not match computational cost: restarting inner damping and
updating multipliers/penalties intentionally change the algorithm. Costs from
different objectives or outer penalties must not be compared as physical gains.

The rebuilt audit evaluates the parent with identical physical, slip and compact
geometry results. Final coarse and dense audits use the same shared Rust curve
and contact model; the dense check has 512 physical frames and 513 geometry poses.
The fixed planned +45° displacement rate remains 0.2009265099 m/s, not a runtime
walking measurement or a physical bound.

| Search / audit | Force error N | Moment error Nm | Minimum torque margin Nm | Loaded slip |
| --- | ---: | ---: | ---: | ---: |
| Weighted control, dense | 0.223523 | 0.053710 | -0.001291 | 66.59% |
| Inequality search, 144 times | 0.148254 | 0.044846 | -0.000731 | 75.19% |
| Inequality search, 512 times | 0.200148 | 0.071828 | -0.002948 | 72.65% |

The dense point penetration is 0.1129 mm, within its 1 mm gate; sampled
interlink overlap remains 16.646 µm. Force, moment, torque, slip and collision
qualification fail. The coarse torque pass does not survive denser checking.
The changed quadrature also changes the sampled slip value, so compare searches
on the same dense grid rather than comparing a coarse slip value with a dense one.

The three outer penalties are 1, 10 and 100. Maximum normalized positive
constraint violation falls from 2.2609 to 2.1607 to 1.9651, still well above
zero. Every inner solve reaches its ten-iteration cap; the outer solve reaches
its three-iteration cap after 34,500 model calls. None is stationary. This
does not establish infeasibility, convergence of the augmented-Lagrangian
method, or an advantage in actual gait quality. It also does not demonstrate
a different gait family: all trials are local continuations of one motion.

Further work must address both the unresolved within-sample constraints and
the dense violations. Raising penalties without adequately solving inner
subproblems can worsen numerical conditioning; these short capped subproblems
do not supply the inner-accuracy assumptions of a convergence theorem. A
physically qualified starting gait and speed continuation, alongside targeted
sampling and sufficiently solved constrained subproblems, remain useful paths
toward faster qualified motion. The previously qualified slower runtime gait
must not be replaced by this failed planned path.

## Checks and reproduction

Six analytic solver tests cover an active linear inequality and multiplier,
inactive constraints, a nonlinear boundary, a fixed infeasible point, a hard
evaluation budget, and invalid inputs. All 16 planner tests pass, including
signed force/moment and penetration threshold checks, malformed report rejection,
and a frozen frictionless translation whose balance is valid while slip fails.

Build `optimize_contact_inequalities` and `audit_contact_implicit` in the shared
`sim-runtime` package. Run the optimizer with the recorded constrained scene,
`surface-markers.json`, and `inequality32.recipe.json`; stdout is the result and
stderr records evaluations. Audit the result with the coarse/dense recipes and
geometry subdivisions 4/16 respectively. `--pairs` records contact witnesses and
the authoritative planned poses. Run `summarize_inequality_trial.mjs` to validate
the input match, physical/slip audit parity, physical inequality rows and worst
actuator margins, last shifted residuals and multiplier updates, evaluation
budget, fixed displacement, and same-time dense frames.
