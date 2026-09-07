# Local motor equation solve experiment

This is an opt-in solver experiment, not an accepted training model. Unlike the
quasistatic motor screen, it retains the detailed winding, rotor, gearbox,
driver and sampled-controller equations. The physical parameters, CAD model,
world, policy and nominal timestep are unchanged.

## Shared implementation

`ImplicitStepConfig.condense_auxiliary` defaults to false. When enabled, the
shared embedded integrator solves all original auxiliary equations at each
trial mechanical endpoint. Only independent mechanical velocities remain in
outer Newton: 18 rather than 66 unknowns for this robot. Auxiliary states are
still integrated and returned; they have not been removed from the physics.

The inner solve calls the existing component adapter, including current-dependent
driver voltage and held firmware state. It assumes no independence between
motors or other auxiliary components. It starts from the same immutable initial
states/rates for every mechanical trial and has no cross-trial physical cache.
Both endpoint-state and explicit-rate unknowns are supported. Inner Newton uses
100 times tighter absolute/relative tolerances, followed by a fresh check of
every original auxiliary residual against the outer absolute tolerance. A
failure rejects the trial. It never freezes a motor torque or advances a
controller clock inside Newton.

Outer Jacobian workspaces invalidate when the formulation changes. Physical
state and workspace changes remain transactional under the existing hybrid
scheduler. Diagnostics count inner solves, residual evaluations and iterations
separately from outer mechanical work. Per-interval totals describe successful
continuous trials (including discarded event-location trials); process-wide
profiles also include failed work. Nested profile time buckets overlap.

Elimination requires a locally solvable auxiliary subsystem at fixed mechanics.
A globally solvable coupled problem need not satisfy that condition. This
generic implementation therefore remains opt-in and can reject problems that
the simultaneous formulation solves. It makes no global uniqueness claim.

## Validation and first full-robot result

The independent motor/circuit/load calculation now tests both formulations and
both auxiliary coordinate choices. Additional coverage checks mutually coupled
internal states, driver resistance and active current foldback in both driving
directions, backlash engagement, sampled firmware timing, and rollback on
failure. Runtime recording preserves the option and replays exactly across
different host chunk sizes. The 31 targeted tests pass.

At 0.25 ms the full 2.8 s robot task completes. Compared with the validated
simultaneous formulation:

| Work or result | Simultaneous | Local auxiliary solve |
|---|---:|---:|
| Mechanical preparations in successful trials | 223,657 | 133,329 |
| Outer Newton iterations in successful trials | 114,695 | 73,259 |
| Endpoint evaluations in successful trials | 474,955 | 145,083 |
| Inner component evaluations in successful trials | — | 9,103,560 |
| Development wall time | 127.95 s | 126.28 s |

There is no established useful speedup. The reduced geometry work is offset by
inner nonlinear/derivative work. Component-equation time increases to about
45.2 s; closure mapping takes about 35.7 s. These are concurrent development
measurements, not an isolated repeated benchmark, and the nested buckets must
not be summed as disjoint costs.

The maximum foot difference is 0.406 mm, current difference is 0.164 A, and
per-foot impulse difference is 0.163 N·s. One sampled contact-pair set differs
and one motor guard has a different event count. Strict numerical equivalence
fails even though every sampled original closure equation passes and supported
lift still qualifies for 200 ms. Neither completion nor lift alone promotes it.

The first sampled current discrepancy above 1e-7 A occurs at 0.03 s. The
reference rejected its 0.02775→0.028 s trial and took two half steps; the
condensed version accepted the full step. Earlier event times agree to about
1e-11 s. This provides a specific integration-grid explanation to investigate,
not proof that every later discrepancy has the same cause. The condensed run
also has two later inner-solve rejections near 1.365 s at very small event
remainders. Tight residuals alone do not override correction convergence.

At 0.125 ms the complete run takes 216.55 s versus the earlier 205.30 s reference.
Foot differences fall to 43.5 nm, but maximum current difference is 0.0410 A and
contact-impulse difference is 0.000103 N·s. Guard counts and sampled contact pairs
match; event timing differs by up to 2.18 microseconds. Supported lift remains
210 ms. Three outer trials are rejected because of inner-solve failures,
including a post-engagement interval near 1.51983 s. Strict numerical equivalence
still fails. Very small residuals do not by themselves prove that correction
convergence has been achieved.

A fresh 0.03 s diagnostic pair samples every nominal step. Maximum current
difference before 0.028 s is 5.26e-9 A; at that step it reaches 0.000686 A, exactly
when the reference subdivides and the candidate does not. Both prefixes advance
all requested steps and then report the expected unfinished motion horizon.
They are diagnostic prefixes, not complete-task successes.

The full results are recorded in `auxiliary-condensation-status.json`. Neither
tested step establishes a useful runtime gain or strict numerical equivalence.
The option remains default-off and the full robot has no promoted condensed
viewer preset. Promotion requires trajectory validation and a meaningful total
runtime gain; neither follows from the reduced outer unknown count.

## Browser delivery

The `pendulum-condensed` fixture is available in the maintained viewer catalog.
Its complete 11-frame native/WASM comparison passes with maximum entry difference
1.74e-18, exact replay/reset, and preserved state after invalid requests or
changed replay data. CI runs that check alongside the original fixture.
All 21 full-catalog UI checks pass, including the new preset's run, readings,
downloadable recipe and replay. This short fixture proves portability/lifecycle behavior, not
full-robot portability or realtime. The installed passing robot viewer is not
replaced by this experiment.

The separate 19-preset bundle is
`runs/interactive/robot-lab-auxiliary-condensation-2026-09-07.zip`. Extract it,
enter `viewer`, run `node serve-viewer.mjs . 4173`, and open the printed local
URL. The ZIP integrity check passes. Existing robot presets retain their
previous solver choices; only the labeled pendulum fixture enables condensation.

## Reproduce

Prepare the point-feedback and final-refresh reference inputs first, then run:

```sh
node examples/full-robot/prepare_auxiliary_condensation.mjs
cargo test --locked -p sim-domain-robot --test embedded_motor --test embedded_step -p sim-runtime --test embedded_session
cargo build --locked --release -p sim-runtime --example integrate_embedding --example compare_embedding --example evaluate_lift
target/release/examples/integrate_embedding runs/full-robot/learning/point-feedback/scene.json runs/full-robot/learning/auxiliary-condensation/base.config.json > runs/full-robot/learning/auxiliary-condensation/base.execution.json
```

Repeat with `refined.config.json`. Use `compare_embedding` against the corresponding
final-refresh capture and `evaluate_lift --simulation-time` with the existing
lift requirements. `audit_hybrid_divergence.mjs` identifies the first sampled
electrical divergence. `summarize_auxiliary_condensation.mjs` records results,
strict gates and artifact hashes. The preparation manifest hashes unchanged
scene/config inputs; a frozen native runner and source snapshot accompany the
local experiment. These ignored local artifacts do not replace durable CAD
versioning or the reproducible preparation recipes.

Next work should distinguish solver-dependent subdivisions from changed
roots or implementation errors and resolve inner correction failures at short
event intervals. If the formulation is retained, reduce inner
derivative cost using declared and verified component structure or shared
analytic partials. Do not assume arbitrary controller/power boundaries are
independent merely because individual motors are separate components.
