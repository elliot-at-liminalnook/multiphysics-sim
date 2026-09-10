# Fixed-motion support checks after the timing-aware search

These checks do not establish a faster gait, feasible initializer or physical
speed maximum. They examine the 0.025 m/s reference returned by the completed
contact-timed augmented-Lagrangian search, before drawing conclusions from the
native joint solver's friction failures. The joint searches keep body motion,
placements and timing free; this conditional audit does not replace them.

## Results

| Fixed motion / audit mesh | Whole force-curve bound | Instantaneous bound | Excluded instantaneous frames |
|---|---:|---:|---:|
| Eight body controls / 250 frames | 0.587214 | 0.990098 | 0 / 250 |
| Exact sixteen-control refinement / 266 frames | 0.704338 | 0.990098 | 0 / 266 |

A normalized lower bound above one would exclude the sampled balance tolerances
in the corresponding relaxation. These bounds are below one: they neither
prove feasibility nor rule it out. The instantaneous relaxation omits friction,
actuator limits and temporal force curves. The curve relaxation retains the
experimental force boxes but omits friction and actuator constraints. Neither
bound is a global speed ceiling or an interval-arithmetic hardware proof.

A separate native solve varies only the 366 force coefficients of the original
warm reference, preserving its motion, robot, force bounds and physical gates.
It stops at the 100-iteration limit (native status -1), with 276 model evaluations
and no feasible candidate. Its final force error is 0.115839 N, moment error
0.028428 Nm, torque margin +0.690473 Nm, and cone violation 0.296864 N.
The initial errors were 0.079160 N and 0.024069 Nm. This local failed solve
is not an infeasibility certificate. Its speed objective is constant because
motion is held fixed; success would only supply an initializer for speed search.
The final projected native constraints match the independent full CAD audit;
all omitted fixed/collision rows remain mathematically zero.

## Avoiding an invalid numerical inference

The initial unit-force-subtraction basis audit fails its 1e-8 reconstruction
tolerance. Using the exact analytic CAD map also fails at the unrestricted
least-squares fit: that fit requests approximately 2.8 billion N, far outside
the largest force-box endpoint of 39.006905 N. The failed logs and empty result
files are preserved. These extrapolated fits are not physically viable forces.

`ContactPlanner::joint_balance_jacobian` now provides the exact fixed-motion
force-node map directly from the shared CAD inverse-load Jacobian and resolved
force interpolation. It supports selected and reordered force columns, retains
clock separation, and rejects empty, duplicate, non-force and invalid decisions.
Motor and cone nonsmooth points do not enter this balance-only API.

The auditor's new `--analytic-box-audit` mode retains the failed out-of-box
reconstruction separately and verifies five bounded probes through independent
uncached CAD evaluations, without loosening the 1e-8 consistency tolerance.
The largest error is 1.365e-12. A second check changes 183 reordered/selected
coefficients while retaining every other coefficient; it agrees to 6.822e-13.
Both clocks and both body meshes are covered. Seven shared planner tests pass.
These checks validate use of the affine map in this domain, not every physical
constraint or continuous-time contact behavior.

## Reproduction and provenance

Build the selected examples without overwriting a running optimizer:

```
CARGO_TARGET_DIR=/Users/elliot/physics-simulator/target/gait-exploration CARGO_BUILD_JOBS=2 /Users/elliot/.cargo/bin/cargo build --release -p sim-runtime --features native-ipopt --example audit_joint_force_basis --example audit_joint_support_space
```

Run `audit_joint_force_basis scene.json markers.json recipe.json
--analytic-box-audit` for the warm and body16 recipes. Run
`audit_joint_support_space scene.json markers.json recipe.json` for the
instantaneous relaxation. `check_joint_warm_support.mjs` verifies saved results
and writes a new verification file; its output uses exclusive creation.

`joint-warm-support-build.json` records source, binary, input and artifact hashes.
The original three box/instantaneous process handles were lost when tool output
was truncated: process absence and complete JSON were subsequently observed,
but their exit codes were not recovered. The final contract audits rerun both
box cases after adding the selected-column and invalid-input checks; both exit
zero and retain exactly the original bounds, probes and fitted reports.
The force-only solve and original warm instantaneous audit have independently
observed terminal exit zero. Failed extrapolation audits have observed exit one.

## Implication for the speed objective

The ongoing eight- and sixteen-control joint searches have not established a
new measured speed gain. Friction-violating native iterates motivate examining
force constraint handling, rather than declaring a physical ceiling or changing
friction limits. TOWR-inspired joint optimization remains restricted to the
supplied contact event order and trajectory parameterization. Broader contact
families and a reliably feasible joint solution remain unexhausted.
