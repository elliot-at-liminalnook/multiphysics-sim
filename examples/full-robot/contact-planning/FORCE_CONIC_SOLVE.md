# Convex force-trajectory solve for the speed optimizer

The optional `conic` feature adds a shared Rust linear conic solver and a
fixed-motion robot example. Its purpose is to obtain physically better force
initializers, or identify incompatible sampled support requirements before
spending another full joint speed-search budget. It does not restrict the
user's goal to low-speed force fitting.

## Formulation

For a fixed body/foot motion, the verified shared CAD map gives normalized
balance residuals `r = B f + d`. The new example minimizes a scalar `t` subject
to `-t <= r_i <= t`, the original finite force-variable boxes, `t >= 0`, and
`||(Fx,Fy)|| <= mu Fz` at every force node. This is a convex second-order cone
problem. Linear or quintic convex interpolation preserves the node cones
between knots. Timing is resolved by the existing shared planner.

The example reads friction from the model; it does not introduce a replacement
friction coefficient, project forces after solving, or relax physical gates.
The conic optimization omits motor and geometry limits. A returned primal force candidate
receives an independent full CAD report covering those limits and balance;
force-box errors and any tiny positive cone residuals remain visible.
An optimizer status alone does not qualify the candidate.

`sim-solve::conic::solve_linear_conic` validates finite matrix dimensions and
zero/nonnegative/Lorentz cone layouts, constructs checked sparse matrices,
and retains native statuses, primal/dual vectors and iteration counts. It
independently recomputes original-coordinate cone violations, stationarity
and the primal-dual objective gap. Infeasibility rays are never interpreted
as robot force candidates. Primal iterates from approximate convergence or budget stops also receive the
full candidate audit, while their nonoptimal native status remains explicit.
`Solved` is never inferred from a physically admissible iterate.

The implementation pins Clarabel 0.11.1 through the optional dependency and
Cargo.lock. No Python or external native library is used. The browser's default
runtime does not enable the feature. The analytic test has exact optimum
`x = .8` for maximizing `x` subject to `x²+y² <= 1, y = .6`; other tests cover
minimax balance, infeasibility, iteration exhaustion and malformed inputs.

Primary implementation references:
[Clarabel's conic problem format](https://clarabel.org/stable/rust/getting_started_rs/)
and [the pinned Rust API](https://docs.rs/clarabel/0.11.1/clarabel/solver/implementations/default/type.DefaultSolver.html).
The installed pinned crate's solution/settings source and SOCP example were
also inspected; its constructor returns `Result`, unlike an older documentation
snippet.

## Reproduction

```
cargo test --locked --release -p sim-solve --features conic --lib
cargo test --locked --release -p sim-runtime --features conic --lib contact_planning
cargo build --locked --release -p sim-runtime --features conic --example solve_joint_force_cones
```

`solve_joint_force_cones scene.json markers.json recipe.json` optimizes all force
decisions in a recipe while retaining its motion and every nonselected force.
The conic CI workflow runs analytic/shared planner tests and builds this example.
Full CAD results are local evidence; remote CI and runtime/browser validation
must not be inferred from those checks.

## Initial robot results

| Fixed motion | Speed (m/s) | Best normalized balance | Full sampled physical gates |
|---|---:|---:|---|
| Original warm motion | 0.025000 | 1.445143 | Fail balance |
| Completed warm joint-search motion | 0.025152 | 0.716108 | Pass on 250 frames |
| Original fast motion | 0.211710 | 34.311055 | Fail balance and actuator limits |

All three convex solves return `Solved` in 12–15 iterations, with zero original
force-box and circular-cone violations. The independently evaluated CAD wrench
residuals match the affine predictions within 2.85e-13. These are fixed-motion
results, not speed optima. The corrected warm final forces form a sampled
feasible initializer with force error 0.035805 N, moment error 0.013715 Nm and
torque margin +1.091287 Nm.

The denser 2,250-frame audit rejects that initializer: force error 0.080568 N,
moment error 0.035909 Nm, with zero cone violation and positive torque margin.
It retains every original phase and adds 1,000 uniform samples per clock.
The first dense conic refit returns `AlmostSolved`, with an objective near
1.066887 and a 3.2e-6 numerical duality gap. This approximate result does not
certify dense feasibility or an exact optimum. Further mesh refinement and
joint motion changes are required before runtime qualification.

The original eight-control native joint search also finished: 8,000 model
evaluations exhausted, status -13, no feasible candidate. Its final 0.025152 m/s
motion has valid sampled body balance but 0.260420 N cone violation. The conic
solve repairs those forces on the original mesh without changing body motion.
The one-step native pilot uses this repaired reference with all 443 motion and
force variables available. A prepared full-speed configuration is not a launched
or completed search; the dense failure must enter the next refinement first.

The final approximate dense iterate was also independently audited: force error
0.053344 N, moment error 0.021338 Nm, positive +1.092031 Nm torque margin, zero
force-box and circular-cone violations. The initial and final implementations
produce identical native iterates; the final one retains a full physical audit
for approximate convergence while distinguishing it from optimal convergence.

The native pilot takes 244 model evaluations and increases reference speed to
0.025188 m/s with all original sampled gates still passing. Its initial report
matches the conic candidate, and its native final constraints match the full
CAD audit. All 250 original physical frames are unchanged in the dense audit
(apart from sample-dependent residual weights). This is progress in obtaining
a feasible optimization path, not a measured runtime speed gain. The force boxes
remain experimental search bounds, not calibrated physical force ceilings.

The next action is adaptive refinement using the dense missed phases and joint
motion changes, followed by another independent dense/runtime audit. The
sixteen-control comparison remains live and is not included as a completed result.
