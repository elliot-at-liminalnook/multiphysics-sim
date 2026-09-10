# Continuing joint speed optimization from a near-balanced motion

The timing-aware joint AL search completed 8,000 model evaluations with a
0.025 m/s candidate, 0.07916036 N maximum force error, 0.02406937 Nm maximum
moment error, and +0.68521597 Nm minimum torque margin. It remains infeasible.
Its original fast starting motion had 12.52452385 N force error. The new
experiment uses the completed solver candidate as the initial point for Ipopt,
retaining the same 443 variables, physical bounds, all original acceptance
checks, CAD artifact, forward/reverse clocks and 0.30 m/s target.

This is initialization from a previous primal solution, not Ipopt's separate
primal/dual `warm_start_init_point` mode. No multiplier estimates are transferred
between the AL and Ipopt algorithms. The experiment tests whether beginning
near force balance lets the joint solver reach feasibility and increase speed.
The 0.025 m/s starting value is not a new objective or a successful gait.

The first native pilot exposed the effect of Ipopt's default initial-bound
push: the native starting speed becomes about 0.026683 m/s and force error
rises to about 0.420822 N before optimization takes a step. The unmodified
initial physical report is retained separately from the native initialization.

The shared `IpoptConfig` now exposes `initial_bound_push` and
`initial_bound_fraction`. They map directly to Ipopt's `bound_push` and
`bound_frac` options; defaults remain 0.01. Push must be finite and positive;
fraction must be finite and in (0, 0.5]. These options adjust the numerical
initial point only. Variable bounds, disabled bound relaxation, constraint
tolerances, physical acceptance and the objective remain unchanged. The
comparison uses 1e-8 for both normalized-coordinate distances. This is an
explicit numerical experiment, not a physical constant or claimed optimum.

Official option definitions:
https://coin-or.github.io/Ipopt/OPTIONS.html#OPT_Initialization

The native analytic audit includes cancellation at iteration zero for a scalar
variable initially at its lower bound on [0,1]. It checks the actual returned
native point for both distances (0.01 and 1e-8), unchanged bound feasibility,
and invalid-option rejection. Existing analytic optima, infeasibility, budget,
panic and cancellation cases remain in the audit. The real robot pilot checks
initial full-report identity and native/full uncached final-audit agreement.

No faster runtime gait, global physical speed ceiling, browser qualification or
sim-to-real validation follows from this initialization experiment.

## Completed pilot and launched search

All 32 shared solver tests and seven shared planner tests pass in release mode.
The nine native audit cases pass, including the two initial-distance cases;
the seven historical native case results remain exactly identical. Both robot
pilots use 316 model attempts and terminate at the requested one-iteration limit.
Their unmodified initial full reports match the completed AL report exactly
(after signed-zero canonicalization). All 6,776 returned native constraints and
the objective match an independent uncached final CAD audit.

| One-iteration pilot | Default distance 0.01 | Distance 1e-8 |
| --- | ---: | ---: |
| Returned speed (m/s) | 0.0264658130 | 0.0252727854 |
| Force error (N) | 0.41262223 | 0.07908250 |
| Moment error (Nm) | 0.12929275 | 0.02405498 |
| Torque margin (Nm) | +0.68108642 | +0.68643209 |
| Maximum original inequality | 7.25244450 | 0.58165001 |

Both fail physical acceptance. The smaller initialization adjustment preserves
near balance and its first step increases speed relative to the unmodified
0.025 m/s seed while slightly reducing force and moment errors. This supports a
longer initialization comparison; it is not evidence of convergence. In fact,
Ipopt's internal primal and dual residuals increase at this first step (they
include internal slack/multiplier behavior and are not identical to the original
CAD physical inequalities). Those native diagnostics remain in the results.

`joint-ipopt-warm-speed` is launched with 8,000 model attempts, 100 native
iterations and 1,000 callback requests, using both initial distances at 1e-8.
These are experiment budgets, not the user's stopping condition. The original
fast-seed Ipopt search continues independently with its unchanged executable.
The experiment still targets 0.30 m/s and retains all original physical rows;
the optional collision-domain projection remains disabled. Full runtime,
collision, slip, steering, timestep and browser validation would still be
required before promoting any feasible planner output.
