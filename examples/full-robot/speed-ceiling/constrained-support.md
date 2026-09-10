# Constrained support-force exploration

The full dynamic correction on the +X 10 mm lift reference reaches
0.169212 m/s forward and 0.172176 m/s reverse at a 0.165 m/s command. All
166 original planned lifts pass dense replay, with no overlap in 10,562
sampled poses. Loaded-foot slip remains 12.52%. These results establish a
faster simulated candidate, not a global physical maximum or hardware speed.

CAD, contact properties, actuator capacities, controller feedback, braking,
packet lease and steering are unchanged. Corrections are bounded joint-target
increments evaluated by the shared Rust/Rhai runtime. No external support
forces are applied to the simulator.

## Physical formulation

The earlier prescribed force shares balanced net force while leaving substantial
body moments. `motion_capability::weighted_minimum_norm_point_forces` now
allocates a requested force/moment wrench across declared support points.
Relative nonnegative weights express planned support availability; zero weights
disable a point. The all-one case preserves the old minimum-norm calculation.

Unconstrained allocation can require pulling on the floor. The first six prepared
weighted controllers were therefore retained as diagnostic inputs and excluded
from physical execution. Their forward tables satisfy friction/unilateral checks
in only 133/161, 129/161 and 125/161 frames at the three tested references; minimum
normal forces reach -1.47, -2.28 and -2.76 N. See
`weighted-screen-disposition.json`; no executed failure was removed.

`constrained_point_forces` instead minimizes

```
0.5 ||A_scaled D g - b_scaled||² + 0.5 lambda ||g||²
f = D g, D_i = sqrt(weight_i / max_weight)
f_z >= 0, hypot(f_x, f_y) <= mu f_z
```

Moment rows are divided by a declared length scale. Circular-cone projection,
a spectral gradient step and accelerated iteration are shared Rust components.
The report retains convergence, a projected-gradient residual and the actual
force/moment imbalance. Convergence does not prove the requested wrench is
balanced. Current constraints omit actuator torque limits, contact height and
complementarity; planned availability is not proof that a foot actually supports
the robot.

The experimental recipe uses length scale 0.35 m, lambda 0.0001, at most 20,000
iterations and a 1e-6 N gradient tolerance. Length scale and regularization are
explicit numerical choices. Friction 0.31724137931034485 comes from the existing
model. Every dynamic and static allocation in all six signed tables converges
and satisfies its cones. Forward maximum residuals are:

| Reference | Moment error (N m) | Force error (N) | Peak dynamic target increment (rad) |
| --- | ---: | ---: | ---: |
| +X 10 mm, 0.150 m/s | 0.2614 | 0.8077 | 0.02531 |
| +X 10 mm, 0.165 m/s | 0.3195 | 1.5625 | 0.03121 |
| Selectively retimed swing, 0.175 m/s | 0.2386 | 8.4735 | 0.05115 |

Dynamic correction is selected dynamic inverse-load torque minus selected static
inverse-load torque at the same pose, divided by servo stiffness. Both allocations
use the same weights and constraints. Zero-motion correction vanishes; old
default tables and existing frame values reproduce exactly.

## Six physical trials

Each runs the same 20-second forward/turn/reverse/stop schedule at 0.625 ms.
Every trial completes, passes turning and stopping, reproduces policy commands
exactly, and has zero overlap in its 1,001 sampled poses.

| Reference / correction scale | Forward / reverse (m/s) | Slip | Speed/control gate | Original 20 ms lifts |
| --- | --- | ---: | --- | --- |
| 0.150 / half | .156253 / .158233 | 9.97% | Fails reverse overspeed | 150/150 |
| 0.150 / full | .151555 / .152822 | 10.48% | Passes | 150/150 |
| 0.165 / half | .176398 / .179247 | 13.03% | Fails overspeed | 157/166 |
| 0.165 / full | .169212 / .172176 | 12.52% | Passes | 156/166 |
| 0.175 retimed / half | .192390 / .192203 | 13.94% | Fails overspeed | 156/174 |
| 0.175 retimed / full | .182301 / .184044 | 14.27% | Fails reverse overspeed | 162/174 |

The 5% command tracking and 5% slip gates are development criteria, not physical
speed limits. Incidental stance unload intervals remain separately reported.

For the full 0.165 candidate, all ten apparent missed lifts at 20 ms become
passes at 1.25 ms observation spacing. The unchanged 0.625 ms physics, input
schedule and seed reproduce all 662 common endpoints exactly. All 166 original
requirements are covered once across [1.4, 9.8] and [11, 15.8] seconds. Clearance,
unloading, support forces and required 20 ms qualifying duration are unchanged.
This is reporting aliasing, not a threshold waiver. Geometry remains sampled;
the dense windows do not cover every instant of the full episode.

## Actuator diagnostic

`reference_load_feedforward` now records available torque magnitude in the
required torque direction, using the shared `EffectiveServo::torque_capacity`,
and capacity minus absolute required torque. All previous load values reproduce
exactly. Front150 forward/reverse minimum margins are -0.521/-0.178 N m;
front165 -0.784/-0.416; retimed175 -0.819/-0.608. See
`constrained-capacity-summary.json` for counts and motors.

These negative margins rule out exact delivery of that sampled ideal reference
and force allocation. They do not prove a global speed ceiling: closed-loop
motion differs from the reference, other allocations may exist, and foot slip
changes the task. Torque-constrained allocation and dynamically consistent body
motion remain useful next steps.

## Reproduction and validation

Use `prepare_dynamic_load_trials.mjs constrained-load-batch.json` with full paths
from the repository root, then `run_validation.mjs` and
`audit_planned_swings.mjs CASE...`. Preparations and captures refuse overwrite.
`check_weighted_loads.mjs FRESH_RUN_DIRECTORY FRESH_REPORT_NAME` checks default
compatibility, zero-motion behavior, invalid recipes and nonconvergence reporting.
Shared analytic tests cover load balance, moment-induced load transfer,
unilateral/friction projection, disabled supports and iteration limits.
The final robot library's 19 tests and analytic inverse-pendulum test pass.

`constrained165-validation.json` defines the finer, sustained, dropout and
browser numerical profiles. `inspect_dense_lifts.mjs CASE SPEC` preserves
original lift requirements and checks exact common endpoints. The optional
sample period is capped at 2.5 ms, must divide 20 ms and must align with unchanged
physics steps. Full-coverage mode rejects missing or duplicate lift requirements.

`constrained165-browser-spec.json` builds the isolated `viewer-constrained165`
bundle, using shared Rust/WASM physics and the existing WASD controls. The
original port-59048 browser and user worktree remain untouched.

## Completed extended qualification and archive

The 0.165-command candidate repeats 0.169208/0.172248 m/s at 0.3125 ms
physics, with all 166 standard dense lift checks passing. The 60 s run reaches
0.169293/0.172205/0.169266 m/s; its completed dense audit passes 557/564
lifts, with seven reverse failures. All 17,843 dense poses have zero sampled
overlap and all 2,233 common endpoints reproduce exactly. The command-loss
episode passes 51/52 dense lifts. Full qualification is therefore still unmet.

Rendered 5 ms and 10 ms previews achieve 0.5991x and 0.7128x simulation/wall
speed, with p95 frame times 61.435 and 63.660 ms. Both remain below realtime;
the 10 ms measurement overlapped compilation. Both pass native/WASM parity,
but path errors of 8.94 and 25.90 mm fail the 3 mm approximation screen.

Archive v8 is verified: 295 added/changed files, 13 parts, 606,458,139 bytes;
1,633 files in the reconstructed snapshot. See evidence-v8-index.json. The
new generalized contact-planning experiments are directly versioned in the
sibling contact-planning directory and are outside this archive.
