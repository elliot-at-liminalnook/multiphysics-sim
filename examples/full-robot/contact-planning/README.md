# Joint contact and body motion planning

Current development priority: [complete the missing joint force/motion/timing
optimization identified by the TOWR audit](TOWR_GAP_AUDIT.md). TOWR's approach
has not been exhausted. This supersedes the historical search priorities below;
the objective remains greater measured robot speed.

Research follow-up: [adaptive search and experiment selection](SEARCH_STRATEGY_RESEARCH.md)
compares contact-sequence tree search, constrained Bayesian optimization,
diversity archives and multi-fidelity selection. It proposes an outer search
around the existing joint optimizer and a measured-speed comparison; these
algorithms have not yet been integrated by this research update.

Follow-up implementation: [constrained Bayesian experiment selection](BAYESIAN_SEARCH.md)
now uses a shared Rust LogEI adapter and seeded Latin-hypercube control. The
first matched-count controller speed comparison is running; all 39 shared
solver tests pass and the original fast baseline replays exactly apart from
wall-clock telemetry. No comparative result or new speed gain is claimed yet.

Latest measured seed: [live joint snapshots and completed controller transfer](JOINT_CHECKPOINTS.md).
A conic-repaired eight-control motion measures 0.02572 / 0.02601 m/s, passes the
short control/slip gates and 20 lift checks, with no overlap at 401 sampled poses.
It is a low-speed initializer; timestep, sustained, steering and browser checks
remain outstanding. The eight-control search now records live candidates while
the sixteen-control search continues. No faster runtime gait is claimed.

Latest runtime finding: [servo-command limits now enter joint planning](SERVO_COMMAND_CONSTRAINTS.md).
The combined 10,266-frame CAD recovery passes the previous planning gates, but
the detailed controller screen rejects an out-of-bounds servo target at 0.72 s.
The new shared constraints use the runtime servo law and exact existing bounds;
force derivatives and legacy compilation pass their checks.
[Hard conic command constraints](SERVO_CONIC.md) now rule out load-only repair
in the fixed-motion subproblem, and the full joint speed search is running.
No runtime gait is promoted.

Latest completed breadth check: [256 alternative contact-pattern starts at
0.212 m/s](CONIC_PATTERN_SCREEN.md) produce no physically feasible initializer.
The shared conic solver and legacy batch replay match their earlier controls.
The sixteen-control comparison has finished without feasibility; its force
repair fails the subsequent dense IK audit. The refined eight-control search
continues. No new runtime speed gain or physical maximum is established.

Latest speed-search step: [adaptive joint mesh refinement](JOINT_MESH_REFINEMENT.md)
adds the worst dense failures to the optimization constraints while preserving
all prior physical frames. A convex force refit and native pilot reduce those
violations; the full refined joint speed search is running. No new runtime
speed gain or physical ceiling is established.

Current implementation: [joint force/motion/timing optimization](JOINT_FORCES.md).
Latest optimizer evidence: [force-cache fidelity and fixed-motion balance
limits](FORCE_BASIS_AUDIT.md). Neither short joint solve produced a feasible gait.
The [instantaneous support audit](INSTANTANEOUS_SUPPORT.md) separates this from
force-curve restrictions. The wider joint search finished without feasibility;
the [finite CAD foot-support audit and systematic contact starts](FINITE_SURFACE_SUPPORT.md)
record the next model check and search direction.
Two alternative stepping patterns have completed joint optimization without
feasibility. [Direct force derivatives and their verification](JOINT_DERIVATIVES.md)
reduce the evaluations needed per solver step; the remaining motion derivatives
are numerical and no new gait is promoted.
An [exact body-spline refinement](BODY_REFINEMENT.md) now runs an eight-control,
197-variable search from the unchanged fast starting motion. All four selected
four-control starts have now completed without feasibility. The
[contact-event force refinement](FORCE_ALIGNMENT.md) now runs a 443-variable
formulation with the same body/foot motion and verified richer force curves.
A [CAD-derived body-height interval](HEIGHT_SEARCH.md) now runs a further search
over leg postures while preserving the initial candidate and physical model.
The eight-control search has since finished without feasibility. New
[contact-relative force timing](CONTACT_TIMING.md) keeps load-transfer knots
attached to changing contact events; analytic and robot-model checks pass,
with a corresponding speed-search recipe prepared. No speed gain is established.
An opt-in [native Ipopt interface](NATIVE_IPOPT.md) now passes sparse constrained
reference, analytic load-sharing and failure-handling cases. The
[joint Ipopt integration](JOINT_IPOPT.md) passes its one-iteration robot pilot
and budget check, with a full search running. Both fixed-knot comparisons have
finished without feasibility; the timing-aware augmented-Lagrangian comparison
continues separately.
Latest completed runtime diagnostics: [.261/.249 m/s with all 46 short lift
checks passing and zero sampled overlap](SMOOTH_RETURN.md). Slip remains 16.4%;
the candidate is experimental and the browser is unchanged. The lower-slip
.234/.225 variant still fails the 5% slip gate at 10.6%.
Previous user-directed work: [direct measured diagonal speed continuation](CADENCE_SPEED.md).
This supersedes the earlier instruction below to prioritize contact-implicit
algorithm development. Speed gains must be measured in the runtime and their
slip, lift and collision failures retained.

This shared Rust planner produces references executed by the ordinary
Rhai/Rust controller. The latest manual candidates pass short control tests but
still fail slipping checks. Literature and design rationale are in
[planner-literature.md](../speed-ceiling/planner-literature.md).

## Latest measured diagonal candidate and contact-implicit priority

The requested .21 candidate is open at
http://127.0.0.1:62198/?preset=physics-diagonal21-5ms (Chrome tab 291878302,
server session 42574). The previous .068 and axial user bundles remain untouched.

The three-round speed continuation retained sampled feasible candidates at
.154593, .195790 and **.211710 m/s**, but each failed its independent denser audit.
The last dense audit has .041459 N force, .013488 Nm moment, -.017991 Nm torque
margin and .044261 mm penetration. Four-thousand-point compilation reports
-.018022 / -.011916 Nm forward/reverse margins. These fail the unchanged -.01 Nm
gate; no dense reference feasibility or maximum-speed claim is made.

An explicit diagnostic compiler option now permits executing such a reference
while retaining its failed physical audits. Default behavior still rejects bad
nominal/reverse references, and interpolation and static pause-window gates remain
mandatory. This matters because exact reference following is more restrictive
than actual closed-loop walking with tracking error. The .21 detailed 8 s test
measures **.210057 forward / .206958 reverse m/s**, passes short speed/heading/stop
checks, and clears **28/28** planned swings. Stops take .34/.30 s. It fails contact
quality at **7.73%** loaded-foot slip, and sampled CAD geometry reports 12.14 µm
maximum overlap in 190/401 poses. It is an experimental faster candidate.

The 5 ms preview measures .212119/.211562 m/s, but rendered walking reaches only
**.361× realtime / 100.4 ms p95** while completing W/stop/S/stop. No rendered
realtime or preview fidelity qualification is claimed. The immutable browser
bundle uses the earlier cached-runtime WASM; the new optional point-feedback
feature is not enabled in that bundle. Native and browser build identities are
recorded separately.

The unretuned .184 diagonal reference also walks at .180620/.180291 m/s despite
failing inverse-reference audits, but has 9.95% slip and 8.93 µm sampled overlap.
All 26 planned clearances pass. A shared Rust contact-velocity feedback diagnostic,
with time constant D/K=.020 s and load scale CAD weight/4=9.7517 N, reduces slip
only to 9.47% at .180284/.179839 m/s; this is insufficient. Its .001 m/rad Jacobian
regularization and .05 rad target cap are explicit controller/numerical choices.
The new point-feedback position gain defaults to 1; zero permits velocity-only
feedback without attraction toward an old world position path. Four shared
feedback tests pass, including unchanged damping with shifted position targets.
Two initial integration captures fail at time zero due to disabled/misnamed
observation channels; retain these failures, and use diagonal184-contact-feedback
as the corrected run. This is privileged teacher feedback, not deployable sensing.

The .068 baseline completes 60 s at .066697/.067242/.066415 m/s with 2.07% slip
and passing control/heading checks. Long and short heading measurements now report
orthogonal drift and enforce the same .1 rad heading gate. Sustained dense
geometry/clearance and hardware calibration remain incomplete. The -45° search
finds a .029390 dense feasible seed; its poorer local result is not a proof that
that direction is inferior. The +45° user preference remains the current default.

**The user's IDTO reminder changes the next algorithmic priority.** This planner
still prescribes one stance/swing sequence per foot per cycle. It is TOWR-inspired
phase optimization, not contact-implicit discovery and not PLANC's complete learned
control pipeline. Proceed to generalized-position whole-body optimization with
state-dependent smooth contact, so contact sequences can emerge. The equations,
model-fidelity distinction and shared Rust implementation route are recorded in
[planner-literature.md](../speed-ceiling/planner-literature.md). Do not substitute
another manual gait parameter sweep for that work.

## Diagonal continuation — 8 September 2026

Forward now means +45° relative to the unchanged CAD chassis. Joint contact/body
optimization retains this direction while varying cycle time, step distance,
individual foot timings, footholds and body motion. The earlier heading comparison
is not a camera rotation. Both forward and reverse load cases are mandatory.

Restoration first found a dense feasible 0.041099 m/s plan. A stronger speed
objective stalled at 0.041109 m/s retained feasibility despite a 0.078749 m/s
infeasible final iterate. The clipped motor capacity had lost its speed gradient
above no-load speed for nonzero motoring load. A shared optimization-only residual
extends that slope while preserving the exact zero-violation set. Braking and
zero-torque coasting remain free of a no-load speed cap; the actual actuator law,
CAD values and physical acceptance tolerances are unchanged. With this residual,
restoration retained **0.067807 m/s**, passing dense forward/reverse load checks.
Speed continuation is still running; no maximum has been established.

The compiler now audits reverse loads and can require them to pass. Static, odd
and even feedforward terms reconstruct sampled inverse loads at clock rates
0/+1/-1. A regression against the earlier axial compiler preserves joint and
static curves exactly, forward offsets within 8.67e-18 rad and reverse torque
within 4.44e-16 Nm. Intermediate rates, acceleration and yaw remain approximations.
An explicit reverse-required compile correctly rejects the old axial reference.

The 1,000-point diagonal curve exceeded the 2 rad/s² interpolation-error gate.
The shared offline sampling ceiling now permits 10,000 points; compiling with
4,000 while retaining all original audit phases passes without relaxing that gate.
The 0.067807 reference errors are 7.77e-7 rad, 7.24e-5 rad/s and 0.717 rad/s².

| Detailed diagonal capture | Forward / reverse (m/s) | Loaded-foot slip | Clearance |
|---|---:|---:|---:|
| First seed, 8 s / 0.625 ms | 0.040813 / 0.039497 | 2.65% | 16/16 |
| First seed, 20 s / 0.625 ms | 0.039491 / 0.041028 | 5.42% | 70/70 |
| Faster seed, 8 s / 0.625 ms | 0.064245 / 0.067133 | 2.07% | 18/18 |

The first seed passes short and longer control gates, but the longer steering
case fails the separate 5% slip gate. Its 1.25/0.625 ms body paths differ by
0.597 mm. The faster seed's forward short-window speed is 0.000171 m/s below the
±5% tracking gate; reverse, stops and heading checks pass. The longer 20 s tests now pass both
control and slip gates: 0.066647/0.067608 m/s at 0.625 ms, 4.51% loaded-foot slip
and 0.22475 rad turning response. The 12 s command-loss test also passes, with
0.98% slip. The 1.25/0.625 ms paths differ by 2.186 mm and speed by 0.189%, passing
the timestep comparison. The fine 20 s run passes 78/78 planned clearances with no overlap in 1,001 poses.
All listed captures have zero sampled CAD interlink overlap; 20 ms reporting
poses do not certify clearance between samples or sustained operation.

Large immutable trajectory parameters are now validated/cached through shared
Rust bindings rather than cloned every controller tick. Cached and uncached
native runs match all physical/policy fields in 401 frames exactly; measured wall
time falls from 39.32 to 18.37 s for an 8 s detailed run (concurrent background
work means this is not an isolated performance benchmark). All 35 script tests,
21 robot tests, the targeted planner tests and the UI motion-command tests pass.

The diagonal browser seed is available at
http://127.0.0.1:59612/?preset=physics-diagonal-seed-5ms.
The exact command endpoints now bypass HTML range rounding, which previously
made reverse input fall outside its declared bound. The fixed rendered test
executes all four W/stop/S/stop events and measures 65.86 mm active displacement,
but achieves only 0.703× realtime and 68.82 ms p95 transition latency. Realtime
browser acceptance is **not met**. The old axial user browser remains untouched.

An earlier generic live-performance test stood still because it did not support
this motion-command preset; its apparent realtime result is invalid walking
evidence. The harness now rejects that case, and the proper key-driven harness
can require observed body displacement. The retained erratum records this error.
Strict native/WASM parity also narrowly fails three force comparisons: maximum
1.253e-7 N difference, while maximum link-position difference is 1.08e-11 m.
The tolerance is unchanged; parity is not claimed.

Native profiling identifies mechanical Jacobian assembly as the largest bucket.
Existing guarded Broyden options complete the same 5 ms preview with body-path
differences below 2 nm and diagnostic wall times 5.30/5.69 s versus 5.98 s baseline.
Rendered walking with guarded Broyden improves to 0.875× realtime / 53.1 ms p95
for this first seed, still below acceptance.

The runtime v2 incremental archive and browser v2/v3 full archives retain the
captures and both original/fixed diagonal bundles. Indexes identify the restore
chain and SHA-256 hashes. Direct manifests are commit-scoped: earlier source
hashes describe their own checkpoints, not subsequent edits.


Latest interactive diagonal baseline: http://127.0.0.1:60389/?preset=physics-diagonal68-5ms
opened in Chrome tab 291878297, server session 30019. W/S request 45° travel;
A/D steer. Native 5 ms preview differs by at most 1.831 mm from the .625 ms
screen. Rendered W/stop/S/stop completes and moves 104.5 mm in the active window,
but .801× realtime / 46.06 ms p95 still fails browser responsiveness acceptance.
Keep this user bundle immutable. No hardware-speed or sustained-motion claim.

## Earlier axial result and initial heading comparison

Adaptive collocation restored the 0.187895 m/s warm start to a **0.184177 m/s**
forward plan passing a 1,000-point plus event/body-knot audit. The algorithm
automatically added the violating phase 0.2355; the physical tolerances stayed
at 0.05 N force, 0.02 Nm moment, 0.01 Nm torque and 0.1 mm penetration.
The accepted audit reports 0.04608 N, 0.01482 Nm, -0.00884 Nm torque margin and
0.03492 mm penetration. These are sampled inverse-dynamics checks, not a live
or continuous-time certificate.

The generic reference compiler derives joint curves, static/dynamic load
feedforward and an all-stance pause interval through shared Rust components.
An explicit initial base rotation preserves the generated posture and replay.
The Rhai policy uses shared contact-phase, trajectory and command-lease helpers.
The original rate-squared feedforward has now been replaced by separate odd
and even load terms (see below). Acceleration and inherited local yaw correction
remain approximations.

| Detailed runtime test | Forward / reverse (m/s) | Turn (rad) | Stop delay (s) | Maximum loaded-foot slip |
|---|---:|---:|---:|---:|
| 20 s, 1.25 ms | 0.182924 / 0.184506 | 0.243227 | 0.64 | 7.70% |
| 20 s, 0.625 ms | 0.182540 / 0.184022 | 0.243228 | 0.64 | 7.79% |
| 12 s command-loss, 0.625 ms | 0.179714 / 0.181061 | — | 0.80 / 0.46 | 10.60% |

All three pass the existing control gates. The timestep pair differs by at most
2.738 mm and 0.262% speed, passing that comparison. The command-loss result lies
on the 0.8 s limit; analysis allows only 1 ns timestamp roundoff, not an extra
reporting interval. All three fail the separate 5% slipping criterion.

Exact Rhai replay recovers independently timed swings. Explicit clearance-only
checks do not impose the old named opposite support pair and do not certify
balance. The 8 s screen passes 25/28 foot-clearance windows and the 20 s/1.25 ms
test 109/124. All failures are the +X foot in reverse or turning. Full sampled
CAD interlink geometry finds no overlap in 401 and 1,001 recorded poses,
respectively. Dense within-step clearance, sustained operation and browser
accuracy/realtime qualification remain unfinished.

A reverse inverse-load audit also rejects the same reference: minimum torque
margin is -0.07390 Nm. The planner therefore now supports additional reference
clocks as mandatory operating cases. It appends their physical residuals and
gates while keeping a single primary speed objective. Each direction's frames
match independent scalar-clock evaluations exactly in the recorded regression.

The user requested **45-degree travel relative to the chassis** to engage all
four belt hips. `prepare_heading_trials.mjs` compares 0/+45/-45 degrees without
rotating the CAD robot, terrain or foot-center layout. At the same 0.184177 m/s
warm-start speed, the four hip ranges change from 18.44/1.21/19.37/3.14 degrees
to 14.33/8.61/16.77/9.94 degrees at +45. The unretuned diagonal reference fails
balance and torque checks, so its body motion and independent foot timings must
be optimized. `heading-plus45-restoration.recipe.json` starts that search with
both forward/reverse clocks and 54 free variables; foot 0 fixes only the arbitrary
cycle origin. Scalar displacement along the requested direction prevents the
optimizer from gaining speed by changing the heading.

Historical frozen-pose rate screens also show a direction effect: at hip0/foot-60,
the stance shaft-rate budget is 0.4095 m/s on the chassis axis and 0.5667 m/s
diagonally. At hip45/foot-60 that comparison reverses. These omit acceleration,
changing geometry and loaded dynamics and are **not attainable speed ceilings**.
See `heading-conditional-bounds.json`; the 0.36 m/s search objective remains a
numerical-box target rather than a global physical maximum.

The requested candidate browser is preserved at
http://127.0.0.1:55732/?preset=physics-compiled184-5ms. The detailed 0.625 ms
profile is also in its selector. Native 5 ms preview completes the short screen;
its browser was verified ready, but native/WASM parity, rendered performance and
preview path error have not yet been measured for this new controller.

Restore `runtime-evidence/evidence-v1-index.json` for all 31 completed runtime
inputs/captures/audits, and `browser-evidence/evidence-v1-index.json` for the exact
25-file viewer bundle. Both archives were extracted and every file hash checked.
The earlier `evidence-index.json` is an immutable commit-scoped snapshot; new
source/report hashes are recorded separately. Never edit the user's live bundle.

The initial 53 variables jointly control cycle time, forward displacement, independent
foot phases and stance fractions, foot placement and swing offsets, and four
periodic 6D body spline controls. CAD closed-mechanism kinematics solve all twelve
independent actuator coordinates. Whole-body inverse dynamics includes leg and
transmission inertia; force allocation checks unilateral Coulomb friction and
reports unbalanced wrench. Signed speed-dependent motor torque capacity and CAD
sampled geometry are checked independently of the optimization cost.

## Reproduction

Restore the speed-ceiling evidence archive chain through v8 using its index.
From this worktree run:

~~~sh
cargo run --release -p sim-runtime --example optimize_contact_motion -- \
  runs/speed-ceiling/validation/constrained-front165-scale1-human-fine.scene.json \
  examples/full-robot/gait-exploration/workspace-markers.json \
  examples/full-robot/contact-planning/joint-search.recipe.json
~~~

Each recipe is complete; result JSON includes motion, solver history, physical
residuals and frame diagnostics. Logs record evaluation progress and elapsed
time. The scene CAD hash is checked. Numerical intervals and uncalibrated actuator
values are explicit experimental assumptions. The 0.36 m/s objective is the
maximum displacement/minimum period in the initial numerical box, not a physical
speed bound. No CAD or user browser changes were made.

## Results

| Experiment | Planned speed (m/s) | Maximum force residual (N) | Torque margin (Nm) | Max penetration (mm) | Sampled feasible |
|---|---:|---:|---:|---:|---|
| Baseline, 8 uniform + 8 phase samples | 0.104 | 1.704 | +0.747 | 0.248 | No |
| Joint search, same sampling | 0.196316 | 0.0610 | -0.0387 | 0.247 | No |
| Same result, 256 uniform + 8 phase audit | 0.196316 | 41.596 | -0.1801 | 0.298 | No |

The first five-iteration search used 537 evaluations in 10.81 s and reduced cost
150.502 to 2.468. That reduction is misleading as a physical performance metric:
the dense audit finds an all-feet-swing interval at phase 0.9863 whose prescribed
body motion needs 41.6 N vertical support. A flight phase is allowed, but its body
motion must satisfy unsupported dynamics. Coarse sampling missed this interval.
The dense diagnostic uses one evaluation and does not change the motion.

The refinement-32 experiment completed in 151.10 s with 2061
evaluations (iteration_limit). It ends at 0.131309 m/s planned speed,
7.998 N maximum force imbalance,
-0.2651 Nm minimum torque margin and
0.491 mm penetration. It remains infeasible.
More iterations on this formulation do not resolve the sampling and contact
representation issues listed below.

## Event-aware search and feasibility restoration

The explicit `contact_intervals` sampling mode partitions the cycle at every
lift-off and touchdown. Three samples per positive interval include both near
edges and its midpoint, in addition to uniform and foot-phase samples. Coincident
events retain zero-weight slots so the solver residual dimension remains fixed.
Duration weighting makes the optimization cost sensitive to the duration of a
new interval; independent maximum-error gates still check even brief intervals.
The default legacy mode is retained solely to replay the original diagnostics.

A regression constructs 1e-7-cycle unsupported intervals missed by a
256-point uniform grid; the event partition exposes both. The real invalid
0.196316 m/s plan is now rejected by the low-resolution event audit with
41.594 N force imbalance, agreeing with the independent dense diagnosis.

Three 12-iteration feasibility searches use the same physical tolerances and a
weak speed penalty before speed continuation. One finds 0.059272 m/s with
0.00239 N sampled force imbalance and +0.970 Nm torque margin. Its independent
512-point audit also passes: maximum force error 0.04828 N, moment 0.01566 Nm,
torque margin +0.7748 Nm, and penetration 0.000399 mm. The staggered wave and
neighbor-pair starts remain infeasible; this does not prove those gait families
impossible. Result JSONs record the declared contact state at every sample.

A 15-iteration speed continuation reaches 0.200021 m/s but fails physical gates:
0.12163 N force error, 0.03916 Nm moment error, and -0.01629 Nm torque margin.
The optimizer now separately retains the fastest candidate that passes all
sampled tolerances, even if a later cost-reducing step is infeasible. This run
retains 0.091548 m/s; that faster retained candidate has not yet passed a dense
audit. Further feasibility restoration starts from the 0.200021 m/s candidate.

The original foothold box only permitted 2 cm movement around the old stance.
`prepare_posture_starts.mjs` expands it to enclose footholds from 20 existing
CAD posture inspections and initializes 30° and 45° hip postures. All original
joint, collision and actuator checks remain active. The enclosing box is a
numerical search region, not certified free space or a complete physical range.

## Current restrictions and next corrections

- Event sampling is implemented; retain independent dense checking and proceed
  to detailed dynamics. Samples near an event do not prove continuous feasibility.
- Separate feasibility restoration from speed improvement. The current bounded
  least-squares solver minimizes weighted penalties; it is not a hard-constrained
  optimizer and stationary does not mean feasible.
- Compare independent initial phase patterns. One stance/swing per cycle, a fixed
  swing shape family and a local numerical box still restrict discovery.
- Make load sharing actuator-aware. Current inner force allocation enforces cones
  and fits the body wrench, but its torque margin is not the best possible margin
  over every support allocation.
- Resolve nominal foot-marker versus actual material contact geometry. Point
  supports and sampled nonpenetration are explicit approximations.
- Continue detailed tracking, sustained, command-loss and contact checks of the
  generated controller. Explicit independent-schedule clearance checks now exist;
  support and body-balance evidence remain separate.

Shared regression checks pass: 3 contact-phase tests, 5 runtime contact-audit
tests, plus the prior 20 robot tests, 8 solver tests,
and 23 closed-mechanism embedding integration tests. They verify the components,
not the physical feasibility of the experimental gait.

## Dense audit of the faster restoration

Restoring feasibility from 0.200021 m/s retains a 0.187895 m/s candidate
that passes the 48-uniform-point plus event checks. The independent 1,000-point
audit rejects it: 0.07813 N force error, 0.02512 Nm moment error, and
-0.01254 Nm minimum torque margin, against unchanged 0.05 N / 0.02 Nm /
-0.01 Nm limits. Penetration remains within tolerance at 0.0480 mm. This is
not the earlier missing-flight failure: the worst balance sample has two stance
feet at phase 0.4995. Next work should refine the constraint sampling around
reported violations and, if necessary, increase body-trajectory freedom.
Never relax the gates to count this candidate as a validated faster gait.

The wider 30° and 45° hip initializations both produce sampled feasible
candidates, with retained speeds 0.03946 and 0.03888 m/s respectively. These
are additional starting points for speed optimization and need dense auditing.
They do not establish a speed advantage for either posture.

The solver now preserves the exact evaluated parameter vector when no step is
accepted; a regression checks that normalization cannot change the returned
initial value while retaining its old residual. All 9 solver library tests pass.
