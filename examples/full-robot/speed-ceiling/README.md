# Physics speed targets

Latest diagonal runtime result: [.234 m/s with passing short foot-lift checks
and zero sampled overlap](../contact-planning/PATH_AND_HOLD.md), reproduced at
two physics timesteps. It still has 13.5% slip and is experimental.

Current contact-property correction: the compiled nylon-foot/world kinetic friction is .25, not the .317241 world scalar used by some earlier planning screens. The conditional constant-height traction bound is 9.751726 N; this is not a velocity ceiling. See [runtime evidence and scope](../contact-implicit/RUNTIME_TRACKING.md). Prior measured runtime results remain unchanged.

The newest constrained-force candidate reaches **0.169–0.172 m/s**, including
a completed 60 s run. Its standard 20 s schedule passes all 166 dense lift checks
at both .625 and .3125 ms physics. The completed sustained dense audit passes 557/564 lifts, and the command-loss
episode passes 51/52 dense lift checks.
The new browser is an explicitly approximate preview. See
[constrained-support.md](constrained-support.md) and [CHECKPOINT.md](CHECKPOINT.md).

The smooth reference with dynamic load compensation and a 10 mm +X foot lift
now sustains **0.154–0.155 m/s** for a 60 s detailed simulation, with **11.4%**
loaded-foot slip and **512/512** planned lifts passing. Turns, release braking
and command-loss stops pass separate trials. This is an experimental speed
gain, not a low-slip or hardware-qualified gait. Its new browser previews
remain below realtime and have measured approximation errors.

The user's original browser remains the flatter-return 0.125 m/s command,
which sustains **0.127–0.129 m/s** for 60 s with 8.5% loaded-foot slip.
Its commanded turns, release braking and command-loss stop pass;
710/710 planned lifts across four additional audits pass.
The hold-compensated 0.10 m/s controller retains the strongest low-slip result
at **0.100–0.103 m/s** and 4.3% sustained slip. The first calculated target was
**0.111 m/s**, the no-load joint-rate budget of its current periodic reference.
This is a conditional design ceiling for that trajectory, not the robot's
global physical maximum. Improving the trajectory can raise it.

The shared Rust CAD Jacobian calculation gives:

| Crouch and direction | Straight stance | Ideal constant-speed return | Existing quintic world return |
|---|---:|---:|---:|
| Hip 0°, foot −60°, forward | 0.410 m/s | 0.273 m/s | 0.111 m/s |
| Hip 45°, foot −60°, forward | 0.567 m/s | 0.378 m/s | 0.154 m/s |
| Hip 0°, foot −20°, forward | 0.469 m/s | 0.312 m/s | 0.127 m/s |

These are frozen-pose, no-load shaft-rate screens, with 60% stance duty factor.
The stance column holds body height/orientation and the foot fixed. The return
columns include the time needed to bring the foot back; they omit lift and
acceleration. The exact existing piecewise-linear joint reference gives
**0.110770425 m/s**, limited by the **+X worm motor**. Hip 45° shares motion with
the belt, but the full stride still needs collision and dynamic validation.
All 20 inspected stationary poses passed the sampled inter-link audit; that
does not certify paths connecting them.

Calculation: solve `J q̇ = −v e` through the original closed mechanism, including
the 1:1 belt, 5:1 worm and slider-crank; then take
`v_budget = min_i(ω_budget_i / |q̇_i/v|)`. For duty factor β, the ideal return
budget is `V*(1−β)/β`. A quintic world foot return requires peak relative speed
`v*(1.875/(1−β)−1)`. The joint reference calculation uses every segment slope,
so it includes the reference's lift demand as well as translation.

The model's 12 motors provide at most **48.645 W of positive mechanical power**
in aggregate under their effective torque-speed curves, `Σ τ_stall*ω_0/4`.
They cannot deliver stall torque and no-load speed simultaneously. No-load
speed is not a hard backdrive cap. Static floor friction bounds horizontal
force/acceleration; dividing power by `μmg` would incorrectly treat it as drag.

The shared inverse dynamics also evaluates gravity/passive loads and explicit
point-support allocations. At the current crouch, four-foot support requires
up to **0.864 N·m**; the opposite-pair candidates require about **1.81 N·m**,
before movement/acceleration loads. Their point-force static moment balance is
imperfect because the COM is off the support line. This calls for dynamic
centroidal control or finite-foot support modeling, not an invented static
stability guarantee. The four-foot constant-static-torque forward rate screen
is **0.388 m/s**, still above the current return-speed budget. Each allocation
reports residual wrench, friction feasibility and foot height spread; a
minimum-norm allocation is only one candidate.

Following the physics-guided planning idea in
[Walk the PLANC](https://arxiv.org/html/2601.06286v1), the next targets come from
these motion and support constraints, then undergo full dynamics validation.
The humanoid support/impact assumptions are not transferred unchanged to this
radial quadruped.

There is currently **no defensible single global maximum speed** from this
uncalibrated model: most hardware travel limits are unknown, the effective
motor profile omits thermal/electrical/transmission effects, and global
loaded gait optimization is unfinished. The 0.111 m/s first target and roughly
0.15 m/s posture/return target guide continued experiments; reaching either
does not finish the user goal.

Reproduce from the repository root:

```sh
node examples/full-robot/speed-ceiling/prepare_capability.mjs
cargo run --release -p sim-runtime --example analyze_motion_capability -- \
  examples/full-robot/fast-wasd/braked-5ms.scene.json \
  examples/full-robot/gait-exploration/workspace-markers.json \
  examples/full-robot/speed-ceiling/capability-recipe.json
```

## Experimental progress

Longer strides, larger lifts, 30°/45° hip postures, a flatter foot-return speed
profile, extended leg posture and static load feedforward were tested. Their
recorded failures remain in the catalog. Three wider-hip paths were initially
blocked by a coarse SDF false positive: the CAD probe points were **0.219 mm
outside** the −Y pulley. A bounded 0.5 mm CAD-derived grid patch enabled those
paths, without increasing physical clearances or changing exclusions.

The most effective change so far is a faster cadence on the accepted **52 mm
stride**. At 0.10 m/s command, the 5 ms model passes a 60 s walking/turn/reverse
test, but smaller timesteps reveal excessive slip. A 20 s run at 0.625 ms with
0.095 m/s command passes the motion checks at **0.0950/0.0945 m/s**, with **4.93%
slip**; its margin is narrow.

Accounting for held commands improves the 0.10 m/s case. Over a 20 ms hold, the
mean desired position advances by `q̇*dt/2`. The controller's velocity lead is
therefore `D/K + dt/2 = 0.030 s`, retaining the outer tracking gain of 0.5.
At 0.625 ms, this produces **0.10271 m/s forward, 0.10168 m/s reverse, 4.81%
slip**, a 0.209 rad walking turn, and settles below 1 mm/s 0.36 s after release.
The 60 s run at 0.625 ms sustains **0.1004–0.1027 m/s** with **4.30% slip**.
The 0.3125 ms human-command run gives **0.10267/0.10141 m/s**, **4.89% slip**;
command-loss recovery also passes. Native/browser parity passes 1,000 task
transitions within the documented numerical budget; browser replay/reset are
exact. Rendered browser performance is measured separately.

Exact replay of every Rhai policy observation recovers the controller state
with **zero command discrepancy**. This verifies all complete planned swings
in the declared steady-command windows: **96/96** coarse human, **96/96** fine
human, **338/338** sustained, and **32/32** dropout. All 5,604 saved poses have
zero sampled inter-link penetration. This is sampled SDF coverage, not exact
CAD or between-frame collision certification. Extra stance unloads remain:
they occupy one or two 20 ms snapshots and peak below **0.378 mm** clearance.
The earlier force-only detector counted some of these as failed swings; its
outputs are preserved alongside the phase-based audit, not discarded.

Higher cadence remains mobile: commands 0.105, 0.110, and 0.125 m/s produce
forward/reverse speeds **0.1083/0.1089**, **0.1143/0.1157**, and
**0.1323/0.1358 m/s**, with slip **7.38%, 8.69%, and 11.94%**. All three stop
within 0.40 s and remain below 0.014 rad tilt. The latter two overspeed their
commands beyond the tracking tolerance. These are exploratory captures,
without full collision/lift/long-duration qualification. The 5% slip and
tracking thresholds are development criteria, not physical speed limits;
they must not be used to terminate the maximum-speed search.

Further commands 0.150, 0.200 and 0.300 m/s produce forward/reverse speeds
**0.1671/0.1666**, **0.2076/0.2359**, and **0.3074/0.3295 m/s**, with slip
**17.3%, 57.9%, and 41.9%**. All remain upright and stop in these 20 s tests.
At 0.150 m/s, only **125/150** planned swings meet the sampled clearance-duration
check; at 0.300 m/s, **235/302** pass. Neither capture has sampled inter-link
penetration. These results demonstrate faster motion but do not qualify a
reliable fast gait. The +X foot's incomplete transfers and shaft-rate demand
motivate redistributing work through the belt and revising the return path.

The rendered 0.100 m/s browser test completes WASD without page errors but
achieves only **0.447× realtime**, **110 ms p95** transition latency on the
recorded host. The explicit 10 ms browser-timestep trial passes the motion
checks, but its path differs from the 1.25 ms reference by **7.22 mm**, exceeding
the existing 3 mm comparison threshold. It has not replaced the 5 ms viewer
profile. Browser runtime work remains necessary.

The original 0.10 m/s cadence candidate passed sampled geometry at every pose
of its 20 s coarse/fine, 60 s and dropout captures. The sustained run qualified
**340/340** force-detected lift intervals, with cadence count/gap checks to
prevent choosing isolated successful lifts. These checks do not excuse its
fine-step slip failure, nor do they transfer automatically to later controllers.

Slip metrics currently integrate material-contact motion from 20 ms recording
samples. Further report-frequency sensitivity remains useful before treating
a narrow margin as precise. All model limits remain provisional.

## Belt, support placement and return-shape exploration

`exploration-summary.json` records 31 additional development screens and seven
fine-timestep WASD captures. Wider hips engage the belt: at 45°/0.15 m/s, the
front hip reaches about 2.4 rad/s. Recomputing the exact reference cycle gives:

| Reference | Hip posture | Conditional no-load cycle budget |
|---|---:|---:|
| Original 52 mm return | 0° | 0.11077 m/s |
| Original 52 mm return | 15° | 0.11500 m/s |
| Original 52 mm return | 30° | 0.12683 m/s |
| Original 52 mm return | 45° | 0.15086 m/s |
| Flatter 52 mm return | 0° | 0.17130 m/s |
| Flatter 52 mm return | 30° | 0.18395 m/s |
| Flatter 52 mm return, centered | 45° | 0.18986 m/s |
| Flatter 80 mm return | 30° | 0.19780 m/s |
| Flatter 80 mm return | 45° | 0.22874 m/s |

The flatter return replaces the horizontal quintic displacement with a smooth
acceleration ramp, constant-speed middle, and symmetric deceleration ramp.
Its peak normalized world return rate is `1/(1−0.25) = 4/3`, compared with
`1.875` for the quintic. Stride and lift height are held fixed in the first
comparison. The Rust closed-mechanism analysis includes simultaneous lift;
with wider hips, the limiting coordinate moves from a worm to a foot motor.
These calculations guide the search; loaded tracking remains the harder limit.

CAD COM centering used the existing Rust mass/pose support inspector, then
shifted each opposite pair along its support-line normal at planned midstance.
Initial errors of roughly 6–7 mm fell to 0.8–1.0 mm. At 45°/0.15 m/s, this
reduced coarse slip from 43% to 17%, but it worsened slower variants. It is a
geometric heuristic, not a dynamic support certificate. A bounded integral
using the existing Rust kernel did not materially improve the tested gaits.

Wider hips also weaken steering. Mapping all three leg joints through the
initial CAD Jacobian improved the 45° turn from 0.056 to 0.072 rad, still well
short of the requested 0.24 rad. Human control checks now explicitly require
0.24±0.06 rad over the four-second turn. These variants are not promoted;
state feedback or a changing Jacobian is still needed for reliable wide-hip yaw.

The flatter-return 0° gait is the strongest faster candidate so far:

| Command | Fine forward/reverse | Slip | Planned lifts | Turn |
|---|---|---:|---:|---:|
| 0.110 m/s | 0.11014 / 0.11088 m/s | 6.20% | 110/110 | 0.211 rad |
| 0.125 m/s | 0.12690 / 0.12874 m/s | 8.64% | 124/124 | 0.223 rad |
| 0.150 m/s | 0.16351 / 0.16299 m/s | 11.31% | 138/150 | 0.241 rad |

All three 20 s/0.625 ms captures have zero sampled inter-link penetration and
stop within 0.40 s. The first two pass speed, tilt, turning and stopping checks;
all miss the 5% slip quality criterion. At 0.15 m/s the gait also overspeeds its
command and misses some lift-duration checks. Lowering lift height worsened
slip; increasing it to 10 mm gave little improvement. The 0.10 m/s candidate
still has the strongest completed low-slip qualification. The faster 0.125 m/s
candidate's additional qualification is reported below.

`reference-tracking-diagnosis.json` aligns reference phase with original sensors
using exact Rhai replay. The front/rear worm errors vary with phase and load;
the rear worm error correlates with an adjacent-slope acceleration proxy
(about 0.71 for the flatter 0.11 m/s gait). This supports investigating dynamic
feedforward, but is not proof of causation. The literal linear table has slope
jumps: a differentiable reference is needed before claiming exact acceleration
feedforward. The existing shared inverse-load calculation can provide the
inertial terms once positions, velocities and accelerations are consistent.

## Faster gait qualification and numerical performance

The flat125 family in `validation-summary.json` completes 60 s sustained and
12 s command-loss runs at 0.625 ms, plus human-command runs at 5, 1.25 and
0.3125 ms. Sustained forward/reverse windows measure 0.12668, 0.12882 and
0.12674 m/s, with 8.48% slip. Release at 56 s settles below 1 mm/s by 56.26 s,
with 6.66 mm displacement. A frozen command at 3 s stops by 3.48 s, with
37.19 mm displacement. All control checks pass; the separate 5% contact-quality
screen fails. That quality threshold remains distinct from a physical limit.

Four additional exact-phase audits pass 424/424 sustained, 38/38 dropout,
124/124 human 5 ms and 124/124 human 0.3125 ms planned lifts. All 5,604 recorded
poses show zero sampled inter-link penetration, with no incidental stance
unloads in those audits. These are surface/SDF checks every 20 ms, with authored
exclusions retained; they do not prove continuous CAD collision clearance.

Existing shared solver options (`broyden_updates`, guarded linearized Jacobian
probes, and exact probe-base reuse) reduce expensive Jacobian/closure work.
They retain the original convergence tolerances and exact residual checks.
The optimized 5 ms body path changes by at most 5.89 nm against the original
solver. However, raw force values fail the strict 1e-7 absolute + 1e-8 relative
comparison, so solver on/off is **not numerically identical**. External native
profiling itself preserves recorded values exactly. The largest optimized
numeric discrepancy is 0.000064 N in a 21.84 N contact-force component. See
`solver-comparison.json`.

Separate native/WASM comparisons pass all 1,000 transitions for both reference
and optimized solver configurations, with maximum numeric differences of
7.60e-9 and 8.51e-9 respectively. Same-host replay/reset are exact. Sequential
rendered WASD reviews measure 0.351× realtime / 125.5 ms p95 for the reference
solver and 0.580× / 68.0 ms p95 for the optimized solver. Both finish without
page errors. This is observed performance on the shared host, not an isolated
benchmark or realtime acceptance. The terminal screenshot also shows the robot
moving partly outside the fixed camera view; automatic following is not present.

`compare_temporal_trials.mjs` verifies scene, task, seed, command values/times,
controller, actuators/contact and non-overridden config, then compares all
recorded frames against the finest completed 0.3125 ms backward-Euler run.
The 0.625 ms path differs by at most 1.50 mm. The browser's 5 ms backward-Euler
preview differs by 13.31 mm; its slip estimate falls from 8.68% to 6.83%.
The shared second-order SDIRK2 method at 5 ms reduces path difference to
3.47 mm but still misses the existing 3 mm screen. At 10/20 ms its errors grow
to 11.17/24.67 mm, and 20 ms also fails speed tracking. No coarse numerical
profile is promoted as physically accurate based on these results. SDIRK2
2.5/1.25 ms completes with 0.69/1.92 mm path differences and passes control
checks, but slip remains 8.87/8.92%. The non-monotonic difference against a
finite-step BE reference is not evidence of uniform convergence or that 2.5 ms
is more accurate than 1.25 ms. These profiles still need browser performance,
parity and their own lift/geometry qualification before promotion. Full results
are retained in `temporal-finest-comparison.json`.

The user-facing isolated bundle is `runs/speed-ceiling/viewer-solver`, preset
`physics-flat125`. It uses the optimized 5 ms preview and explicitly reports
its approximation and slip. Start its `serve-viewer.mjs` on an available local
port, then open `/?preset=physics-flat125`. W/S translate; combine A/D to steer;
release requests a stop after transfer. Existing user tabs and older bundles
are preserved. The global speed goal remains active.

## Smooth reference and acceleration-dependent load compensation

The shared Rust `Trajectory` now supports `periodic_cubic_b_spline`. Uniform
control points produce a periodic C² curve inside their convex hull, rather
than passing through every point. `trajectory_sample` exposes the same schema
and position/rate/acceleration calculation to Rhai. The existing linear and
quintic endpoint-hold semantics are retained. Analytic rate maxima include
interior extrema: the smoothed 52 mm reference has a **0.171547 m/s** conditional
no-load speed budget, still limited by the +X worm, versus 0.171300 m/s before
smoothing. This is not a global speed ceiling or an attainable loaded speed.

`reference_load_feedforward` also accepts that shared curve with explicit
phase rate/acceleration and floating-base velocity/acceleration. It uses the
existing inverse dynamics and prescribed point-force shares, and reports the
unbalanced base moment. Dynamic increments subtract the static allocation at
the same position. The tested tables use separate forward/reverse steady rates,
5 ms reference-phase spacing and a 0.06 rad offset cap; compensation is enabled
only at nominal cadence, zero requested yaw and no braking. Acceleration and
turning transients are not compensated. The predicted offsets peak at
0.016–0.025 rad, but residual moment reaches 2.96–3.96 N·m, so this allocation
is an approximation with significant missing body-moment balance.

Seven 20 s, 0.625 ms human-command runs complete:

| Reference / compensation | Command | Forward / reverse m/s | Slip | Planned lifts | Control checks |
|---|---:|---|---:|---:|---|
| Smooth / none | .125 | .12765 / .12922 | 7.97% | 124/124 | Pass |
| Smooth / none | .150 | .16126 / .16496 | 11.68% | 133/150 | Fail |
| Smooth / none | .175 | .19225 / .19772 | 14.65% | 134/174 | Fail |
| Smooth / half dynamic | .125 | .12609 / .12686 | 7.95% | 124/124 | Pass |
| Smooth / full dynamic | .125 | .12423 / .12439 | 7.54% | 124/124 | Pass |
| Smooth / half dynamic | .150 | .15760 / .16004 | 11.41% | 143/150 | Fail |
| Smooth / full dynamic | .150 | .15387 / .15502 | 11.49% | 143/150 | Pass |

All 7,007 recorded poses have zero sampled inter-link penetration; exact Rhai
state replay reproduces commands. Full dynamic compensation improves speed
tracking and planned lifts at .150, but does not establish a qualified faster
gait. All variants miss the 5% slip quality screen. Seven failed lifts in the
full-compensation .150 case belong to +X; support-force checks pass, but
clearance/unloading do not qualify for a consecutive 20 ms sampled span.
Three occur during steering, when compensation is disabled. Next investigate
that foot's clearance duration and load tracking, with denser reporting if
needed to distinguish brief motion from a sampling limitation. Smoothing alone
also introduces some incidental stance force unloads, reported separately.

Validation includes controller/script regressions, analytic spline values and
derivatives, C² cycle continuity, convex-hull bounds, exact interior rate
extrema, and the existing analytic pendulum inverse-load test. CLI tests retain
archived static target offsets exactly, reproduce static loads at zero motion,
and reject missing CAD hashes, overflowing support sums and discontinuous or
ambiguous dynamic references. These checks validate the calculation, not the
assumed support distribution. The original user browser remains unchanged.
The subsequent selective-lift candidate and its qualification are described
below; these seven original references retain their own narrower evidence.

## Selective lift, sustained qualification and browser measurements

The all-8 mm smooth/full-dynamic .150 case missed seven +X lift windows in its
20 ms reports. Shared `capture_embedded_window --replay` now captures bounded
windows through the same `EmbeddedSession::prepare_replay` and `advance` path.
It preserves the physics timestep, seed, controller and input schedule.
`evaluate_lift` accepts completed bounded replay windows with an exactly
matching recorded scene/world; it does not claim full-episode completion.

Two 1.25 ms observation windows (2–2.4 s and 6.5–7 s, unchanged .625 ms physics)
match all **47** common physical/policy endpoints exactly. The next held input
is prequeued at replay host boundaries, so `policy_inputs` and wall time are
excluded from that comparison. The previously failed forward lift qualifies
for 20 ms in dense samples; the turning lift qualifies for only 17.5 ms and
still fails. All 722 dense poses have zero sampled inter-link overlap. This
changes the interpretation of one sampled failure, not the qualification of
the whole gait. CLI checks reject zero sample periods, windows beyond recorded
completion, duplicate events, incomplete windows and mismatched worlds.

CAD IK recipes in `front-clearance-plan.json` increase only the +X lift from
8 mm to 10 or 12 mm. Shared B-spline smoothing and signed dynamic tables are
regenerated for each reference/cadence, with unchanged CAD, motors, world and
WASD law. Six 20 s/.625 ms trials give:

| +X lift | Command | Dynamic increment | Forward/reverse m/s | Slip | Lifts | Control |
|---|---|---|---|---|---|---|
| 10 mm | .150 | off | .16234/.16517 | 11.60% | 146/150 | fail |
| 12 mm | .150 | off | .16321/.16533 | 12.05% | 150/150 | fail |
| 10 mm | .165 | off | .18500/.18641 | 13.87% | 154/166 | fail |
| 10 mm | .150 | full | .15457/.15498 | 11.55% | 150/150 | pass |
| 12 mm | .150 | full | .15551/.15474 | 11.87% | 150/150 | pass |
| 10 mm | .165 | full | .17468/.17545 | 13.90% | 154/166 | fail |

All 6,006 recorded poses have zero sampled inter-link overlap; this retains
authored exclusions and does not establish between-frame or exact-CAD safety.
The 10 mm/full/.150 candidate has no incidental stance unloads in this trial;
12 mm/full has three. None passes the separate 5% contact-quality screen.

Exact polynomial rate extrema now give conditional nominal no-load budgets
of **.162219812 m/s** for the 10 mm reference and **.137753905 m/s** for 12 mm,
both limited by the **+X foot motor**. Raising the lift transfers the nominal
bottleneck from the worm to the foot drive. These screen reference time scaling,
not actual body speed under tracking error/slip, and are not hard backdrive or
global robot limits. Recorded 20 ms motor samples show about **7.3 W** peak
aggregate positive power. Their finite sampling and unequal joint demands do
not prove the remaining aggregate 48.645 W can produce useful forward speed.

`front150-validation.json` qualifies the selected 10 mm/full/.150 candidate:

- **60 s/.625 ms:** .15424/.15520/.15412 m/s, 11.38% slip, 512/512 lifts,
  release stop in .42 s over 27.95 mm; late drift .262 mm.
- **12 s command dropout/.625 ms:** 46/46 lifts; lost packets at 3 s stop by
  3.58 s over 57.71 mm; late drift .303 mm. Explicit release also passes.
- **20 s/.3125 ms:** .15458/.15516 m/s, 11.25% slip, 150/150 lifts.
  Four one-sample stance-force unloads while turning have slightly negative
  sampled floor clearance; they are not extra successful lifts.
- These three audits and both preview audits cover **6,605 poses**, all with
  zero sampled inter-link overlap. Policy command replay is exact.

`front150-finest-comparison.json` checks the unchanged scene/task/inputs and
controller against .3125 ms. The .625 ms run differs by at most **1.18 mm** in
body position and **.119%** in steady speed, passing the existing 3 mm/2%
screen. At 1.25 ms, path error is **3.25 mm**, failing that screen. The 5 ms
preview has **10.10 mm / 1.60%** error; 10 ms has **17.68 mm / 3.15%**. Preview
slip is 8.35%/6.32%, understating the detailed 11.25%. The finest run remains a
numerical approximation, not ground truth. No accuracy gate was relaxed.

The immutable `viewer-front150` bundle contains explicitly named 5/10 ms
previews, separate from the user's port-59048 bundle. Both use current shared
Rust/WASM and the existing optimized solver flags. Native/WASM parity at 5 ms
passes 1,000 transitions (maximum difference 6.94e-9); 10 ms **fails** with
106 values outside the existing 1e-7 absolute + 1e-8 relative tolerance
(maximum force difference 9.08e-7 N; some speed/torque values also fail).
Both retain exact same-host replay/reset and rejected-input preservation.

Sequential rendered WASD reviews complete without page errors: **.750× real
time / 44.2 ms p95** at 5 ms and **.885× / 52.6 ms p95** at 10 ms. Active timing
includes rendering, scheduling and worker transport over .4–16.4 s; worker-only
parity timings are not rendered acceptance. These are observations on a shared
host, not isolated benchmarks. Screenshots show the fixed camera letting the
robot partly leave the view. Neither preview establishes required realtime
walking. Reports, input recordings, timings and screenshots are retained.

The next physical opportunities are clearance timing without excess foot
speed, motion redistribution through wider hip postures, and feedback for
loaded body momentum/yaw. Dynamic increments still apply only at their exact
signed nominal cadence, zero requested turn and no braking; new rates/paths
need new analysis. More cadence alone overspeeds and misses lifts at .165.

Reproduce preparation with `prepare_clearance_trials.mjs front-clearance-plan.json`,
`prepare_smooth_trials.mjs front-smooth-batch.json`,
`prepare_dynamic_load_trials.mjs front-dynamic-batch.json` and
`prepare_validation.mjs front150-validation.json` (use full recipe paths).
The generalized comparison takes `front150 front150-human-0p3125ms
front150-finest-comparison`. `inspect_dense_lifts.mjs` verifies the two bounded
windows; `check_dense_replay.mjs` exercises rejection cases. Restored experiments
are immutable; use fresh case names/output roots for changed inputs.

## Rate-based phase redistribution

Uniform cadence makes the whole cycle wait for its single largest joint-rate
peak. For a fixed joint path `q(s)` with independent absolute rate budgets
`b_i`, the rate-only minimum traversal time is
`T_min = integral max_i(abs(dq_i/ds)/b_i) ds`. Shared Rust
`Trajectory::rate_traversal_bounds` brackets this integral. Each cell contributes
`max_i(abs(delta q_i)/b_i)` to the lower duration bound and its width times
the exact polynomial rate maximum to the upper bound. Nested subdivisions
4 → 64 → 256 tighten the bracket; all old uniform-rate extrema and budgets
are checked exactly against their archived reports.

| Original smooth path | Uniform budget m/s | Nonuniform rate-only speed bracket m/s |
|---|---:|---:|
| All 8 mm lifts | .171547 | .277967–.278048 |
| +X lift 10 mm | .162220 | .272060–.272145 |
| +X lift 12 mm | .137754 | .264428–.264521 |

These brackets assume the declared 52 mm stride and omit acceleration, torque,
phase-rate continuity, endpoint holds, contact and stability. They use f64,
not outward-rounded interval arithmetic. They are conditional reference-path
screens, not robot speed ceilings or achieved gaits. The two worm motors still
dominate most of the rate-limited traversal duration, even where the single
largest uniform-rate peak belongs to the +X foot motor.

`Trajectory::redistribute_periodic_rates` turns that rate envelope into a
candidate reference while anchoring the original phase times at 0/.04/.36/.4/
.44/.76/.8 s. It blends old and redistributed cell durations, samples the old
curve at the new phases, then uses those samples as 160 uniform B-spline
controls. This yields a C2 periodic reference but changes the path. Anchors
apply before the final smoothing, so actual lift and stopping behavior must
still be checked. The zero-blend case measures the additional resampling effect.

For the 10 mm path, zero/.5/.8 blend gives uniform rate budgets of
**.163414/.178849/.188208 m/s**. Preserving the transfer intervals therefore
captures only part of the unrestricted rate-only gain. The robot, world,
initial pose, 20 ms holds and Rhai steering/braking/lease logic are unchanged.
Signed dynamic load tables are rederived at .175 m/s for both nonzero blends.
Their prescribed support allocation leaves maximum unbalanced moments of
**8.53/16.88 N·m**; peak suggested increments are **.0509/.0680 rad**, with
the latter clipped by the retained .06 rad controller cap. Lower peak joint
speed does not imply lower acceleration loads or a feasible support wrench.

Two initial .175 trials were rejected at .4 s because arithmetic produced
`.17500000000000002` outside the exact declared `.175` input bound. These are
input-validation failures, not evidence about walking stability. Their captures
remain immutable. Fresh `bounded-inputs` cases and the not-yet-executed dynamic
cases use exact boundary commands; `retiming-input-corrections.json` records
all old/new hashes and the 2.78e-17 m/s correction. Original dynamic preparation
fingerprints precede this correction; the validation catalog and correction
ledger identify the inputs actually executed.

Six complete 20 s/.625 ms all-channel redistribution trials give:

| Blend | Command | Dynamic increment | Forward/reverse m/s | Slip | Lifts |
|---|---|---|---|---|---|
| 0 | .150 | off | .16230/.16516 | 11.15% | 146/150 |
| .5 | .150 | off | .16458/.16785 | 13.52% | 145/150 |
| .5 | .175 | off | .20020/.19834 | 15.69% | 156/174 |
| .8 | .175 | off | .20047/.19798 | 15.93% | 158/174 |
| .5 | .175 | full | .18866/.18611 | 14.84% | 158/174 |
| .8 | .175 | full | .18863/.18726 | 15.84% | 152/174 |

These overspeed and fail the existing control/contact-quality checks. The
compensated candidates miss +X/-X lifts and have 50/62 incidental stance-force
unload intervals; they do not supersede the .155 m/s sustained candidate.
All six audits have zero sampled inter-link overlap across 6,006 poses and
exact policy-command replay. Retiming stance joints as well as swing joints
also changes the body motion demanded by no-slip stance, while the prescribed
load calculation assumes constant base speed. That is a model-consistency
concern, not a proof that every missed lift has that cause.

The subsequent `swing-redistribution-batch.json` uses explicit per-coordinate
intervals: -Y/+Y joints retime only .04–.36 s; +X/-X only .44–.76 s.
Outside those intervals, source controls use the original phase; final spline
smoothing still changes the reference slightly. This keeps the same .178849/
.188208 m/s rate budgets while reducing the prescribed residual body moment
to **6.59/7.86 N·m** at .175 m/s. New signed load tables, physical trials and
geometry audits remain distinct from the all-channel cases.

All four swing-only 20 s/.625 ms trials complete:

| Blend | Dynamic increment | Forward/reverse m/s | Slip | Lifts | Incidental stance unload intervals |
|---|---|---|---|---|---|
| .5 | off | .20216/.20073 | 14.02% | 156/174 | 37 |
| .8 | off | .20535/.20327 | 14.17% | 154/174 | 45 |
| .5 | full | .18869/.18656 | 14.05% | 156/174 | 41 |
| .8 | full | .18968/.18940 | 13.96% | 152/174 | 61 |

Slip decreases relative to the corresponding all-channel trials, but planned
lift counts do not improve. All four still overspeed the .175 request and fail
the separate 5% slip screen. Their turns are .2659–.2667 rad, release stops in
.36 s over 28.6–32.4 mm, and late drift is .272–.280 mm. All 4,004 poses have
zero sampled inter-link overlap and policy commands replay exactly. These
results do not establish collision freedom between samples or promote a new
sustained gait. No new long-duration, dropout, timestep or browser qualification
is claimed for the retimed paths.

The calculation exposes rate-budget headroom, but the physical tests show that
reducing reference rate peaks alone is insufficient. Useful next steps include
support-consistent acceleration/load planning and measured body-speed feedback
with load compensation valid across the resulting rates. Existing feedforward
tables apply only at their exact signed nominal cadence and zero requested yaw;
they must not simply be reused or scaled when adding rate feedback.

The 13 trajectory tests cover analytic linear/quintic traversal time, periodic
total variation, nested bounds, rate-switch coverage, monotone phase mapping,
phase anchors, C2 continuity, selective channel timing and invalid inputs. The full control crate tests
pass. Initial test-development failures (a Rust float literal and an overly
tight guessed upper-sum error) are retained; the final bound test uses the
analytic total-variation error bound. No gait acceptance criterion changed.

Reproduce the calculations with `analyze_retiming.mjs` after building
`analyze_motion_capability`. Build `redistribute_trajectory` from the control
crate before `prepare_redistributed_trials.mjs`; dynamic preparation uses
`redistribution-dynamic-batch.json`. Use fresh names/paths for changed inputs.

## Reproduction and preservation

The recipe pins CAD, scene, controller and marker inputs by SHA-256. Restore
the older archived raw inputs using the fast-wasd README if needed. The
capability report and recipes are durable here. `evidence-v1-index.json`
contains hashes for **316 files**, archived in eight parts totaling
368,848,584 bytes. Every extracted file and every source file was checked.
The joined gzip SHA-256 is
`c95b30edf42c0e590033a22e9db6c487cc874067e191497c0bd85a4a3bf2b046`.

```sh
cat examples/full-robot/speed-ceiling/evidence-v1.part-* > /tmp/speed-ceiling-v1.tar.gz
mkdir -p runs/speed-ceiling
tar -xzf /tmp/speed-ceiling-v1.tar.gz -C runs/speed-ceiling
```

The v1 snapshot covers experiments through the three sampled-controller trials.
`evidence-v2-index.json` adds **120 files** in four parts totaling
**169,823,992 bytes**, including synchronized qualifications, the six higher
cadence captures, their selected geometry audits, and the browser bundle.
All 120 extracted files and all 436 current source files were hash-verified.
Restore v1 first, then concatenate/extract `evidence-v2.part-*` into the same
`runs/speed-ceiling` directory. The index contains the complete restore command.
The joined v2 SHA-256 is
`2314a998841cb59103d03fc1abace0e22a3c0e0383ea439810d18570f6f3b3b5`.

Qualification inputs can also be rebuilt from the archived selected controller with
`prepare_validation.mjs synchronized-validation.json` (pass the full recipe
path from the repository root). `run_validation.mjs` resumes only cases absent
from the status ledger and refuses to overwrite captures.

Build the generic phase diagnostic with
`cargo build --release -p sim-script --example replay_policy_state` and run it
on a complete environment capture. `check_policy_replay.mjs` checks command
reproduction and rejection of corrupted recordings. `audit_planned_swings.mjs`
uses the recovered state to form all planned windows, then calls the shared
Rust lift/geometry audit. Set `SIM_EXAMPLES` when binaries use a custom target
directory. Existing audit outputs are immutable; use a new named experiment
when changing the inputs or requirements.

The next incremental snapshot, `evidence-v3-index.json`, preserves 336 added
files in seven parts (329,848,803 bytes), with 336 extracted and 772 source
files verified. Restore the complete v1 → v2 → v3 chain using its `restore`
command. It includes the belt/centering/integral/flat-return exploration and
seven fine captures. The later `flat125-validation.json` family and solver
experiments are not included in v3; the subsequent snapshot records them.

`evidence-v4-index.json` adds 106 files in five parts (191,223,346 bytes),
including all flat125 qualification, profiling, six temporal trials and the
optimized/reference browser bundle. All 106 extracted files and all 878 source
files were hash-verified. Its restore command includes the full v1 → v2 → v3
→ v4 chain. Joined SHA-256:
`2857a63494f3f57eb5f705100b2ffe5e30bec4f5ab6bfe8302660cd4e0510879`.
Rendered reviews, their recordings, timings and screenshots are versioned
directly beside the reports. Reproduce solver inputs with
`prepare_solver_trials.mjs`, numerical profiles with
`prepare_temporal_solver_trials.mjs` (then `--refine` for 2.5/1.25 ms), and
comparisons with `compare_solver_trials.mjs` / `compare_temporal_trials.mjs`.
Input preparers refuse overwrites; restored captures need analysis only.

`evidence-v5-index.json` adds the seven smooth/dynamic captures and audits,
signed load tables/recipes and inverse-load CLI checks: 118 added files in two
parts (93,381,545 bytes), with all 118 extracted and 996 source files verified.
Joined SHA-256:
`b32968b3c63c51aa49394b0beb0e914c115334120e82688f6638f2d58787784c`.
Its restore command includes v1 through v5. Reproduce inputs using
`prepare_smooth_trials.mjs`, then `prepare_dynamic_load_trials.mjs`; build the
updated `run_environment`, `replay_policy_state`, `reference_load_feedforward`
and `analyze_motion_capability` examples first. Use `smooth-capability-recipe.json`
with the smoothed scene for the updated conditional rate screen. The CLI checks
accept a fresh output directory: `check_dynamic_loads.mjs runs/speed-ceiling/checks-new`.
Do not overwrite the preserved captures or audit outputs when changing a policy.

`evidence-v6-index.json` adds 174 files in five parts (198,493,297 bytes),
including the selective-lift plans, smooth/dynamic references, all qualification
captures and audits, bounded replay windows/rejection inputs, capability
reports and the immutable new browser bundle. All 174 extracted files and
1,170 source files were hash-verified. Joined SHA-256:
`9dab68402041e102705cead63b8bb8544a18525d341cc8069f84a44d32e1e9ec`.
Its restore command includes v1 through v6. Rendered browser reports, input
recordings, timings and screenshots are versioned directly alongside the
archive. `package_browser_profiles.mjs` accepts an explicit spec and refuses
existing bundle paths; use a new output path to rebuild with changed code.

`evidence-v7-index.json` adds 168 files in three parts (136,344,726 bytes):
rate-only refinement reports, redistributed references/recipes, signed loads,
ten completed trials and audits, two input-rejected captures, and the exact
input-correction evidence. All 168 extracted files and 1,338 source files were
hash-verified. Joined SHA-256:
`c489da87a80e86b20678d49d273ea2e902c94d8f8aa8bb931522641575818f1a`.
Restore the complete v1 through v7 chain using the index's `restore` command.
The archive retains the unsuccessful trials as well as the usable reference
calculations; it does not promote a new browser or sustained gait.
## Latest direction and evidence

The current force-allocation experiments are documented in
[constrained-support.md](constrained-support.md). The new interactive preview
uses the .169–.172 m/s candidate; qualification limits remain in the checkpoint.
Following user feedback, [planner-literature.md](planner-literature.md) reviews
PLANC, TOWR, contact-implicit planning and reference-free sampling MPC, and sets
the next priority: joint motion/contact planning using shared Rust physics.
