# Contact-implicit whole-body planning

Current user priority: [measured diagonal speed improvement](../contact-planning/PATH_AND_HOLD.md). Offline restoration is paused; it has not yielded a runtime speed gain.

Latest offline result: [denser physical constraints with retained dual state](MEAN_GRID_REFINEMENT.md).
Previous: [exact motion refinement and preserved-dual balance comparison](MEAN_BOUND_REFINEMENT.md).
Previous: [continuous mean-slip bound and matched balance-restoration continuations](CONTINUOUS_MEAN_SLIP.md).
Previous: [strict sampled-slip barrier and a verified load-cutoff discontinuity](SLIP_BARRIER.md).
Previous: [actual loaded-slip inequalities and their finite-solve failure](LOADED_SLIP_CONSTRAINT.md).
Previous: [persistent constrained solves and a scaling comparison](WARM_CONTINUATION.md).
Previous: [longer inner solves and a recorded low-slip warm-start](INNER_BUDGET_AND_RECORDED_SEED.md).
Previous: [physical inequality constrained slip optimization](INEQUALITY_SLIP.md).
Previous: [continued slip shaping and collision attribution](COUPLED_SLIP.md).
Previous: [shared slip objective and matched physical tradeoffs](SLIP_OBJECTIVE.md).
Previous: [motion-basis comparison and a conservative sampled slip bound](BASIS_RESTORATION.md).
Previous: [fixed-speed feasibility restoration and repeated residual targeting](FEASIBILITY_RESTORATION.md).
Previous: [reproduced linear failure resolved by residual refinement](KKT_REFINEMENT.md).
Previous: [equality-constrained solver and independently audited failures](EQUALITY_CONSTRAINTS.md).
Previous: [targeted collocation and matched dense-grid comparison](TARGETED_COLLOCATION.md).
Previous: [motion-basis and collocation refinement](REFINED_PERIODIC.md).
Previous: [analytic smooth periodic searches and denser audits](SMOOTH_PERIODIC.md).
Previous: [exact periodic boundaries and first cycle searches](PERIODIC_CYCLES.md).
Previous: [derivative recovery and measured finer-grid tracking](DERIVATIVE_RECOVERY.md).
Earlier: [exact evaluation cache and finer-grid solver failures](FINE_GRID.md).
No new qualified gait or physical speed maximum.

This work follows the user's explicit [IDTO](https://idto.github.io/) steering.
It introduces a shared offline planner with no prescribed stance flags, foot
phases, swing curves or support allocations. It has **not produced a qualified
robot gait**, an online controller, or a physical speed maximum.

The existing experimental .21 m/s diagonal browser remains available at
http://127.0.0.1:62198/?preset=physics-diagonal21-5ms. Its speed comes from the
previous phase planner, not this implementation. Its documented slip, overlap
and browser performance failures remain unchanged.

Latest progress: [surface-based contacts and schedule-free initial guesses](SURFACE_RESULTS.md) replace the single-point approximation with 96 compiled CAD foot samples and provide an independently audited planning-feasible path. Detailed walking validation remains outstanding.

Latest measured validation and model corrections: [runtime tracking](RUNTIME_TRACKING.md). The energy-aware path reaches .158 m/s over a short startup, but fails sliding quality and finer-grid planning consistency.

## Implemented method

- `sim-domain-multibody::smooth_contact` implements IDTO equations 3–6, including
  stable softplus compliance, velocity-dependent normal dissipation, regularized
  friction, and analytic local derivatives. The registry exposes typed ports,
  units and required validated parameters as `contact.smooth_planning_force`.
- `sim-runtime::contact_implicit` optimizes generalized position knots using the
  shared CAD rigid embedding, point Jacobians and whole-body inverse dynamics.
  Finite differences determine velocities and accelerations. Contact forces
  follow point distance and material velocity. The objective penalizes the six
  unactuated wrench components, actuator envelope violations, point penetration,
  tracking error and effort. The fixed initial state is preserved exactly.
- The shared bounded least-squares solver supports optional diagonal scaling
  `D_ii = max(H_ii, 1e-24)^(-exponent)`. Exponent 1/4 follows IDTO Section V-A;
  zero preserves the prior solver's arithmetic. This is damped Gauss–Newton with
  weighted penalties. A separate [equality-constrained solver](EQUALITY_CONSTRAINTS.md)
  now implements constrained dogleg steps and an experimental exact-L1 alternative;
  neither has yet produced a qualified robot gait.
- Independent auditing re-evaluates the final planning model and samples full
  compiled CAD collision geometry along linear position-space interpolation.
  This does not certify force feasibility between knots or closed-loop tracking.

Primary formulation: [Kurtz et al., 2023, equations 3–13 and Section V](https://arxiv.org/html/2309.01813v2).
The paper's smooth planning contact permits force at positive separation and
differs from its validation contact. That approximation is explicit here too;
it never replaces the authored detailed runtime contact model.

## Robot experiment

The CAD r1357 model supplies 29 links, masses/inertias, transmissions and 12
independent actuator coordinates. The requested forward direction is +45° in
the CAD chassis frame. The numerical target is .25 m/s, not a physical bound.
There are 12 intervals (216 free position variables) over .39608749 seconds.
The previous .21 reference supplies an initial guess and a moving initial state;
it supplies no contact schedule or periodic constraint. All body and joint knots
after the initial state are free within explicit numerical/software bounds.

The first model uses one declared foot marker per leg, friction from the scene,
stiffness 2,000 N/m, dissipation velocity .2 m/s and stiction .02 m/s. Those are
explicit planning assumptions. Smoothing continuation runs .01/.003/.001 m.
Subsequent stiffness continuation ends at 200,000 N/m and 10 µm smoothing.
Matching the runtime's stiffness number does **not** equate this point law with
the runtime's extended-surface contact model.

Planning gates are .05 N force, .02 Nm moment, -.01 Nm minimum torque margin and
1 mm maximum point penetration. None of the following trials pass all gates.

| Trial | Max force error N | Max moment error Nm | Min torque margin Nm | Point penetration mm |
| --- | ---: | ---: | ---: | ---: |
| `diagonal25` soft continuation | .007051 | .001733 | -.004207 | 8.6662 |
| `diagonal25-stiffness` | 1.664669 | .680381 | -.603788 | .15065 |
| `diagonal25-refinement` | 1.276008 | .640324 | -.545637 | .15050 |
| `diagonal25-scaled` | 1.240348 | .635298 | -.544334 | .15049 |
| `diagonal25-unscaled-control` | 1.275583 | .640248 | -.545599 | .15050 |

The first trial changes the thresholded foot-force pattern without an imposed
sequence, but uses excessive penetration. Four static contacts at the permitted
1 mm penetration cannot support the robot's weight with that soft stiffness;
the failure motivated explicit stiffness continuation, not a relaxed gate.
The stiffer solve improves penetration but fails balance and actuator limits.
Refinement uses a smaller derivative probe (1e-6 normalized coordinates), 24
iterations and the same physical gates. It still fails. These are finite-horizon
planned paths, not measured walking speeds; their displacement rate is not a
new speed record.

The scaled and unscaled control trials start from exactly the same refinement
result with identical physical models, weights, bounds and 24-iteration budgets.
Scaling reduces cost from 671.384 to 663.264; the unscaled control reaches
671.093. Both hit the iteration limit and fail feasibility. This is a modest
local numerical improvement, not solver convergence or evidence of a maximum.
The scaled audit exactly reproduces the final report and finds .217969 mm
maximum interlink overlap across 61 poses, with the same initial floor failure.
`summarize_trials.mjs` verifies matched inputs and exact audit agreement and
produces `summary.json`; thresholded force patterns are diagnostics, not supplied
contact constraints.

The independent refinement audit reproduces every knot wrench exactly. Across
61 geometry poses it finds .217918 mm maximum sampled interlink overlap and
6.679536 mm maximum floor penetration, already present at the fixed initial
pose. The single foot marker does not represent the entire foot surface, so
point penetration alone is insufficient. A useful next model must account for
authored foot surfaces and begin from a geometry-consistent state. Do not promote
these trials or silently shift the floor/CAD to make them pass.

## Validation and remaining work

Analytic checks cover gravity, free flight, softplus equilibrium and discovering
touchdown from an airborne stationary guess without supplied contact timing.
The local force derivative checks cover normal dissipation joins, friction bounds,
separation and extreme gaps. Registry checks verify discoverability, typed ports
and rejection of missing/invalid parameters. Shared solver tests include stiff
and soft directions in the same least-squares problem and existing bounds/budget
regressions. Logs are stored beside the experiment recipes and results.

Still required: final-model feasibility, surface-aware contacts, repeatable or
terminally viable motion, multiple initial guesses, timestep refinement, detailed
runtime tracking with steering/reverse/stopping/command loss, collision and slip
checks, and responsive rendered browser walking. If scaling is insufficient,
the next solver work is enforcing unactuated balance with constrained optimization
and improving derivatives/sparsity. Local failure does not prove a speed ceiling.

## Replay and provenance

Build the shared Rust examples:

```sh
cargo build --release -p sim-runtime --example optimize_contact_implicit --example audit_contact_implicit --example sample_contact_reference
cargo test -p sim-runtime --test contact_implicit
cargo test -p sim-domain-multibody smooth_contact
cargo test -p sim-solve least_squares
```

Run a recipe with `optimize_contact_implicit SCENE MARKERS RECIPE`, writing stdout
to a fresh result and stderr to a fresh log. Independently audit with
`audit_contact_implicit SCENE MARKERS RECIPE RESULT 5`. The scene is
`runs/speed-ceiling/validation/constrained-front165-scale1-human-fine.scene.json`;
markers are `examples/full-robot/foot-markers.json`. Restore the scene through
the existing versioned speed-ceiling evidence archive chain; ignored `runs/`
alone is not provenance. Recipes record CAD/scene/reference identities and all
planning overrides. `prepare_robot_trial.mjs` prepares robot-specific data using
the shared Rust sampler; it does not implement physics and refuses overwrite.
`evidence-v1-index.json` records final source, input, output and binary identities.
Early soft/stiffness/refinement searches predate the optional scaling addition;
their original executable hashes were not recorded. Their recipes and outputs
are retained, and the refinement report was independently reproduced exactly.
Final executable identities apply to the scaled/control comparison and latest
audit, not retroactively to the earlier searches.

No browser bundles, CAD data or files in the original user worktree were changed.
