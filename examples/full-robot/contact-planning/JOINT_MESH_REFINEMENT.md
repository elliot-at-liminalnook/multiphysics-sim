# Adaptive joint mesh refinement after convex force initialization

The latest native pilot's 0.025188 m/s reference passes its original 250 frames
but fails the 2,250-frame dense audit. The largest normalized inequality is
0.797396: moment error 0.035948 Nm exceeds the unchanged 0.02 Nm tolerance.
This work adds the missed physical constraints to the joint speed problem;
it does not accept the coarse solution as a qualified gait.

## Shared selection algorithm

`select_contact_refinement_phases` exposes the existing adaptive collocation
ranking for other planner paths. It ranks force, moment, torque and penetration
failures, chooses the worst unoccupied phases, and enforces circular spacing.
Inputs are validated for finite values, dimensions, positive physical scales
and bounded refinement settings. Floor and inter-link tolerances are separate;
zero permitted overlap ranks every positive inter-link penetration as a strict
failure. The previous adaptive least-squares path retains its original ranking
and tolerance behavior. Tests cover the existing ordering/spacing behavior,
strict overlap priority, and invalid-input rejection.

`refine_joint_contact_mesh` evaluates the same candidate on the original and
dense grids, calls that shared selector, and evaluates the resulting refined
recipe. The physical model, controller candidate, force coefficients, variable
bounds and physical tolerances are preserved. Only `additional_phases` changes.
The dense grid retains the original uniform phases as well as the automatically
included force/body/contact events. Both forward and reverse operating clocks
remain in every report.

The configuration reuses the earlier adaptive experiment's settings: 1,000
uniform audit samples, at most eight added phases, minimum phase spacing
0.00025. These are numerical refinement settings, not changes to hardware limits.
The eight selected phases lie from 0.1855 to 0.1925. The refined mesh has 266
frames and includes the observed worst dense failure; all 250 prior physical
frames are retained unchanged apart from residual weights.

## Force initialization and joint search

The existing convex force solver refits all 366 force coefficients on the
refined mesh. It returns `AlmostSolved` with a worst normalized balance error
near 1.068014, force error 0.053401 N, moment error 0.021360 Nm and positive
+1.091791 Nm torque margin. Force boxes and circular friction cones have zero
violations. Approximate convergence is retained; neither optimality nor
feasibility is claimed. The independent CAD reconstruction check passes.

This improved force assignment initializes the full 443-variable joint problem,
including body motion, foot placement and contact timing. The .30 m/s target and
all physical gates are unchanged. Adding violated phases is a reason to change
the optimized motion, not a reason to loosen the force/moment tolerances.

## Reproduction

```
cargo test --locked --release -p sim-runtime --features conic --lib contact_planning
cargo build --locked --release -p sim-runtime --features conic --example refine_joint_contact_mesh
```

Run `refine_joint_contact_mesh scene.json markers.json recipe.json refinement.json`.
The returned `refined_recipe` is the input to `solve_joint_force_cones`, whose
primal candidate initializes `optimize_joint_ipopt` with the stored search config.
`check_joint_conic_mesh.mjs` checks old-frame retention, unchanged physical fields,
incorporation of the worst dense failure and native/full-CAD pilot agreement.
The conic CI workflow builds the refiner and runs the shared tests; remote CI
has not been executed here. Source/binary/input identities and terminal results
are recorded beside this document. A running search is not a validated result.

## Native pilot and full search

The one-step native pilot completes with 219 model evaluations and no rejected
callbacks. Its maximum inequality falls from 0.068014 to 0.062534, with zero
friction-cone violations and positive actuator margin. Reference speed changes
from 0.025188 to 0.025159 m/s during restoration. It remains infeasible; native
status -1 is the one-step iteration limit, not a physical infeasibility result.
The initial full report matches the conic refit; native final constraints match
the independently evaluated full CAD report.

The full refined search is launched from the same conic initializer, with all
443 decisions, the unchanged .30 m/s target, 8,000 model attempts and at most
100 native iterations. Its launch record is versioned separately; ongoing logs
and output are not terminal evidence. The older sixteen-control comparison is
also still live. Neither is a physical speed ceiling or a new runtime result.
