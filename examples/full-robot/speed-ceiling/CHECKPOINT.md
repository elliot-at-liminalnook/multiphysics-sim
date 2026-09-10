# Active goal checkpoint

The goal remains active with no time limit. No global physical or hardware speed
maximum is proved. The 5% slip criterion is a development quality threshold,
not a physical speed limit or permission to end the search.

Worktree: `/Users/elliot/physics-simulator-gait-exploration`, branch
`physics-gait-exploration`. Original user worktree, CAD and prior viewer servers
remain untouched. No subagents were used.

## Current checkpoint — exact translating-periodic boundary and cycle searches

Previous goal turn: progress, commit01073c4. This turn: progress. Goal ACTIVE;
no global/hardware maximum or qualified new gait. No agents, CAD/original-worktree/
browser changes. Shared contact planner adds periodic_horizontal_translation
(defaultfalse); require initial_velocity=[] in periodic mode. All unique poses,
including first pose, are free. Final endpoint equals first except netXYtravel;
initial spatial velocity wraps from last backward-difference velocity. Closure
is exact parameterization, not a penalty. Bounds layout: K rows of n pose values
and one2-entry displacement row; encoding K*n+2 variables. Explicit firstXYfixed
bounds remove translation gauge. Existing startup layout/evaluation unchanged.

Nine contact-implicit tests pass: seam/cache dependence, translated repetitions,
free-first-pose gravity balance, exact closure/error rejection. Old .168 startup
full audit exactly unchanged. At-rest compiler explicitly rejects periodic input;
no new runtime initial-velocity support/entry controller has been implemented.

All jobs TERMINAL: tests44409, build55461, initial audits60737/16070, matched
searches34894/65487, refinement67715, standalone audits. periodic-uniform and
periodic-perturbed share .39608749s/24unique knots/432freevariables, .25m/s diagonal
target, physical model/objectives/bounds, 4x40-iteration contact continuation.
Perturbed seed uses small deterministic3-harmonic joint noise (seed271828183),
no contact/leg phases. Both initial CAD audits have zero overlap. Final periodic
reports also have zero overlap at121poses, but fail physical balance and slip.
Uniform cost1739.24 vsperturbed136.88;100more final-model iterations improve the
latter to131.21. Finalrefinement planned .250676m/s, force1.44434N,moment.82684Nm,
margin-.007500Nm vs .001 tolerance,slip107.09%,floorpenetration.17893mm.
These are infeasible planning rates, NOT runtime measurements or a speed record.

Actuator diagnostics show tinyhipspans .0497–.0707rad with16–18reversals/cycle;
individual modelpeakpower use~98.76% in the infeasible perturbed candidate.
Sum optimistic modelpeakpower48.645W is not a global m/s or thermal bound.
Next: smooth periodic trajectory position/velocity/acceleration through shared
components, between-control balance/grid checks, equality-constrained base
balance and periodic-compatible derivative parameterization. Avoid treating
knot-level rapid reversals as verified continuous motion or a physical ceiling.
Then moving-state diagnostic/entry transition, sustained tracking/slip/collision/
timestep/WASD qualification. Runtime archivev3 remains latest; no runtime files
added this turn. See ../contact-implicit/PERIODIC_CYCLES.md for full evidence.

## Previous checkpoint — derivative recovery and .168 m/s startup tracking

Previous goal turn: progress, commit84bca35. This turn: progress. Goal ACTIVE;
no global/calibrated hardware maximum or qualified new gait. No agents, CAD,
original-worktree or browser changes. Shared derivative diagnostics identified
that the stalled1e-7 gradient points uphill under small directional verification;
1e-9 agrees with measured descent. Gradient cosine -.177912. Motor-penalty
activation crossings matter; previous penalty-only diagnosis was incomplete.

New optional shared derivative refinement retries damping stalls at smaller
probes within the same total budgets. Analytic rotating-residual regression
recovers real cost reduction;13solve and7contact-implicit tests pass. Final build
exactly replays exploratory coordinate/directional diagnostics. New sources and
binaries are identified in derivative-refinement-build-identities.json.

resolved-work-finegrid-adaptive refines1e-7→1e-8 once at iteration18;100iterations
finish planningPASS (.032756N,.015437Nm,margin-.00026505Nm vs .001Nm tolerance).
Independent uncached audit exactly agrees;121geometryposes zerooverlap,
.13180mm floor penetration. Compiler passes. Plannedmean.185303,slip71.85%.
Actual adaptive-finegrid-ff32/ff64 uses matched prior clocks4.125911ms controller,
.515739/.257869ms physics,97reports. Both complete .39608749s at.168589/.168373m/s,
slip74.67/75.08%,zerooverlap,maxjointerror.01647/.01682rad. Timestepbodydifference
.15965mm,speeddifference.12847%. Compared old.158038 startup:6.54%speed gain,
slip99.88→75.08%,jointerror.04327→.01682rad; several planner changes contribute.
Still fails5%slip and lacks sustained/terminal/periodic/WASD qualification.

Tests/builds/search40215/replays27951/runtime85255+92198 and audits TERMINAL.
Runtime archivev3 onv2 contains16newfiles,84total; self-verification recorded in
its index. No optimizer remains live. See ../contact-implicit/DERIVATIVE_RECOVERY.md.
Next: constrained whole-body balance with verified derivatives, longer horizons
and terminal/periodic viability, smooth references and detailed runtime/slip/
collision/WASD qualification. Do not infer a physical limit from local failure.

## Previous checkpoint — exact frame cache and finer-grid solver failures

Previous goal turn: progress, commit 0d2cd75. This turn: progress. Goal ACTIVE;
no physical/global/hardware speed maximum and no qualified new gait. No agents,
CAD changes, original-worktree changes or browser-bundle changes.

All jobs TERMINAL: tests50347, release48164, benchmark23739, searches18923/68819/
18873/47018 and standalone audits. Seven contact-implicit tests pass. The new
bounded per-planner frame cache matches all865 actual-CAD probes exactly and
reproduces the retained pre-cache full audit exactly; evaluation benchmark7.8932x.
Uncached final reports remain the independent acceptance path.

See ../contact-implicit/FINE_GRID.md. Four24-interval searches over .39608749s
use resolved nylon friction. First100+100 iterations reduce force error45.28N
to .01503N but minimum torque margin -.01103Nm fails the unchanged -.01 gate.
Tightening torque tolerance/residual scale to .001Nm improves torque margin but
fails force/moment balance (.117N/.049Nm), DampingLimit. Smaller1e-7 derivative
probe still fails (.11745N/.05054Nm), DampingLimit. All121-pose CAD audits show
zero overlap, ~.132mm floor penetration; planned slip72.55% remains unacceptable.
Planned~.1853m/s is not a runtime measurement. No new compile/controller run.

Next: shared equality-constrained balance solver/globalization and directional
finite-difference validation using these retained failures. Current scaled damped
Gauss-Newton penalties are not full IDTO dogleg/KKT/MPC. Do not loosen gates or
infer a physical limit from solver failure. Then finer-grid consistency, matched
runtime clocks, longer horizon and terminal/periodic viability. Original .21
browser candidate remains unchanged and still unqualified for slip/performance.
Runtime archive v2 remains latest; this turn adds direct evidence only.

## Previous checkpoint — runtime tracking, resolved properties and sliding work

Previous goal turn: progress, commit cb380a7. This turn: progress. New shared
servo reference-target helper, finite-horizon compiler/Rhai diagnostic, recorded
CAD geometry auditor, resolved floor-contact inspection API and optional physical
sliding-work objective. No CAD, original-worktree or browser-bundle changes.
No agents. Goal remains active; no global/calibrated hardware speed maximum.

All jobs TERMINAL, including controller runs9374/65034/47986, resolved searches
79337/15791, corrected runtime29565/53235/1198/42554, and archives11239/22881.
Runtime archives v1 (36files,16,760,641bytes) and incremental v2 are self-verified.
No optimizer remains live. Test logs:2effective-servo unit tests,6contact-implicit
integration tests, release builds; compiler rejects the known failed surface25
plan. Initial two runtime setup attempts fail BEFORE integration (clock mismatch,
unsupported per-step contact audit), retained with empty capture/error logs.
Actual original runs carry v3 suffix. Contact logging is at97reportposes.

Important correction: the runtime uses nylon/world kinetic friction .25, not the
world scalar .317241 copied by the earlier planner. Resolved normal dissipation
is .2s/m (IDTO vd=5m/s), not vd=.2; selected runtime slip regularization .001m/s,
not .02. Source profile is regularized Coulomb PATCH contact, NOT bristle;
interlink forces omitted, full sampled geometry audited separately. Shared
floor_contact_profile exposes resolved compiled values; corrected recipes use
these. Pointwise planning friction and10µm smoothing still differ from runtime.
Conditional constant-height nylon-foot traction is9.751726N, acceleration2.4525m/s²;
neither is a speed ceiling. Prior runtime measurements stand; prior planner
bounds using the world scalar require correction. See RUNTIME_TRACKING.md.

Original .183028 plan executes at .131419/.131644m/s (64/128substeps per33msplan
interval). PD-only .099582. All complete .39608749s without tilt/height failures,
zero sampled self-overlap; excessive loaded slip~148% and20.49mm body tracking
error. Initial body pose matches the plan exactly. .3mm timestep path difference
is much smaller than tracking error. No sustained or command-response claim.

Corrected-model control: planned .189191, actual .141542/.141752m/s, slip127.50%.
Optional sliding-work objective is exact sum(dt * -ft dot vt)/scale, scale
.04828171J=.05*mu*CADmass*g*requested_speed*horizon; no stance flags. Matched
search reduces planned work .872024→.660639J. Work case planned .184967, actual
.157950/.158038m/s; slip99.88%,13.02mm max body error,.04327rad joint error,
.07702rad tilt,zero sampled overlap,.296773mm floor penetration. Fine/coarse
path difference .297991mm,speed .0554%. All slip values FAIL5% quality criterion;
not physical maxima. Planning torque margins remain slightly negative within
-.01Nm gate. No new qualified gait or replacement for the .21 viewer.

Critical next evidence: linearly subdividing the work plan at HALF planning dt
without reoptimization fails force45.2815N/moment10.5474Nm/torque margin-1.71333Nm,
while121posegeometry passes. Coarse implicit knot feasibility does NOT certify
interpolated control. Next implement exact cached/uncached-equivalent reuse of
local THREE-KNOT frame evaluations, then finer-grid REOPTIMIZATION and longer
horizons with resolved contact/work objective from initialization and multiple
generic seeds. Do not merely interpolate and call it feasible. Address periodicity
or terminal viability and eventual feedback/WASD before browser promotion.

Direct documentation: ../contact-implicit/RUNTIME_TRACKING.md, runtime-tracking.summary,
resolved-{control,work}-runtime.summary and resolved-work-halfgrid.audit JSON.
Runtime binary identities for original7captures are in runtime-tracking-binaries;
current compiler/optimizer identities in direct evidence-v3-index. Preserve older
commit-scoped manifests and both runtime archive layers. .21 viewer remains
http://127.0.0.1:62198/?preset=physics-diagonal21-5ms, server42574, immutable.

## Previous checkpoint — CAD surface contact and two schedule-free seeds

Previous goal turn: progress, commit 0da0a7f. This turn: progress. Shared
tracking::compiled_surface_markers exports the exact detailed-runtime sampled
foot geometry (96 points, stiffness per sample). CAD, detailed runtime laws,
original worktree and all immutable browser bundles remain unchanged. No agents.

Stationary q0 is the first zero-overlap audited knot from the previous path
(knot1), explicitly raised .157698447mm using the measured floor clearance.
Initial velocity is zero. Independent13pose audit confirms zero sampled overlap
and20µm minimum clearance, matching the planner's surface gaps. The old6.68mm
initial foot-surface error is removed through an explicit pose change, not a CAD
or floor edit. New tilted-surface test verifies edge forces/moments and exact
runtime gap agreement; all4contact-implicit tests and release example builds pass.

Two seeds share exactly the same .25m/s +45° task/model/bounds/weights/initial
state: future stationary poses, or future body translations with constant joint
coordinates. No stance flags, swing curves, phases or joint gait supplied. Both
use12intervals/.39608749s, k2000/10000/50000/200000 and sigma3/1/.1/.01mm,
24iterations/stage, then100iterations on unchanged final model.

Stationary refinement: planning gates PASS, .055747m/s mean/.202025 terminal,
force.020972N/moment.008915Nm/torque margin+.095400Nm. Translating refinement:
planning gates PASS, .183028m/s mean/.071302 terminal, force.009410N/moment
.004334Nm/torque margin-.007529Nm (within-.01 tolerance, NOT strict positive
capacity). Both61poseaudits show zero sampled interlink overlap; maximum floor
penetration .114164/.128742mm respectively. Exact independent reports reproduced.
Initial translating continuation failedforce2.1169N/moment1.9022Nm/margin-.5536;
retained as evidence, then refined rather than promoted prematurely.

These are short smoothed-contact planned paths, NOT measured walking, periodic
or sustained gaits, hardware-safe limits, or new speed records. Terminal velocity
and unverified between-knot dynamics matter. All searches hit iteration limits;
no local stationarity or physical maximum proved. All writers TERMINAL:21601,
35414,53541,12408 and all audits/builds/tests. No live optimizer to restart/poll.

Next: validate finite-horizon planned control in the shared detailed runtime and
extend horizon/repeatability while keeping contact sequence free. Before larger
horizons, cache unchanged local three-knot residual frames (finite-difference
probes currently recompute every frame), with exact cached/uncached equivalence
checks and an independent audit path. Stronger KKT balance remains optional based
on evidence; surface geometry and ordinary continuation now yield feasibility.
Retain steering/reverse/stopping/dropout/timestep/slip/browser requirements and
.25m/s task target. The .21 browser at62198/session42574 remains experimental and
immutable. Goal remains active; no physical speed maximum proved.

See ../contact-implicit/SURFACE_RESULTS.md and surface*.summary.json. New direct
manifest evidence-v2-index.json is commit-scoped; preserve earlier manifests.

## Previous checkpoint — shared contact-implicit planner

Previous goal turn: progress, commit ab5dbbe (.21 diagonal preview and IDTO steering).
This turn: progress. Shared smooth contact (IDTO equations 3–6), position-only
whole-CAD inverse-dynamics search, explicit continuation, optional H^(-1/4)
scaling, independent full sampled geometry audit, and analytic tests implemented.
No new qualified gait or physical speed ceiling. No agents or browser/CAD changes.
See ../contact-implicit/README.md, summary.json and evidence-v1-index.json.

All experiment/test processes are TERMINAL, including scaled optimizer27137,
unscaled control16229 and scaled geometry65374. No live data writers to preserve.
The .21 viewer at62198/session42574 remains immutable and experimental; its
slip/overlap/realtime failures remain. Goal stays active with no time limit.

The new planner prescribes no support sequence: all body/joint positions after
q0 are free, velocities/accelerations follow differences, smooth point forces
follow gaps and material velocities, and shared CAD dynamics enforce penalties.
216 variables, 12 intervals, .39608749 s horizon, target .25 m/s along +45°.
The .21 reference supplies a weak seed and moving initial state only. This is
not periodic motion, a start-from-rest controller, online MPC or complete IDTO.
Weighted damped Gauss–Newton is not the paper's equality-constrained dogleg.

Soft k2000/sigma.001 reaches small balance errors but penetrates8.666mm (1mm gate).
Stiffness continuation ends k200000/sigma10µm; matching runtime k does not make
single-point forces equivalent to detailed surface contact. Refinement ends
force1.276008N/moment.640324Nm/torquemargin-.545637Nm, all failing. With identical
warm start/model/bounds/24iterations, scaled cost663.264 vs unscaled671.093;
scaled force1.240348N/moment.635298Nm/margin-.544334Nm, still fails. Neither is
stationary; all termination limits are numerical budgets, not physical limits.

Independent audit reproduces final planning fields exactly. Scaled61poses show
.217969mm interlink overlap and6.679536mm floor penetration at fixed q0. The foot
marker misses extended foot geometry. Next: surface-aware contact from authored
CAD samples and a geometry-consistent initial state, then stronger unactuated
balance enforcement (IDTO constrained dogleg/KKT) and sparse/accurate derivatives.
Do not return to manual timing/height sweeps or promote failed paths as walking.
Retain all detailed runtime/steering/dropout/timestep/browser requirements.

Tests:2shared contact analytic/derivative,3runtime equilibrium/freeflight/emergent
touchdown/registry,5least-squares tests including scaling and existing regressions.
New examples build release; evidence summary checks exact audits/matched controls.
Final binary identity applies to scaled/control/latest audit. Earlier three
searches preceded scaling; original binary hashes not recorded, explicitly noted.

## Previous checkpoint — .21 diagonal preview and explicit IDTO steering

Previous goal turn: progress (commit 930b073, .068 diagonal validated/opened).
This goal turn: progress: completed speed continuation, measured/opened .21
candidate, added explicit diagnostic compiler path and velocity-only shared
feedback, ran 60 s baseline, and checked the user's cited IDTO source.

The latest user asked to see .21 diagonal and reminded us specifically of
https://idto.github.io/. The candidate is open in Chrome tab 291878302:
http://127.0.0.1:62198/?preset=physics-diagonal21-5ms, server session 42574.
Keep it immutable, alongside .068 at60389/session30019 and axial at55732/session76389.
Rendered .21 walking completes W/stop/S/stop but only .361× realtime /100.4ms p95.

Optimizer17738 is TERMINAL. Three rounds retain .154593/.195790/.211710 sampled
feasible plans; all dense audits fail. Last torque margin -.017991 Nm vs-.01.
Result/log now stable. Diagnostic .21 compiler retains failure and passes 4000-point
interpolation/static pause checks. Detailed8s .210057/.206958 m/s, short controls
pass, stops .34/.30s, 28/28 planned clearance. Quality fails:7.73% slip and12.14µm
maximum sampled overlap190/401poses. No long steering/dropout/sustained/fidelity
qualification or maximum-speed proof. Source/data paths use diagonal21-*.

Minus45 restoration43942 also TERMINAL: .029390 dense feasible, one local seed
only. Raw .184 diagonal diagnostic reaches .180620/.180291,26/26lifts,but9.95%slip
and8.93µm overlap. Contact-feedback diagnostic reaches .180284/.179839,9.47%slip;
insufficient. Shared point_feedback.position_gain defaults1, zero supports pure
loaded contact-velocity damping. Four feedback tests pass. Two earlier feedback
captures fail at t=0 from disabled/misnamed observation wiring, not physics;
corrected prefix diagonal184-contact-feedback. Explicit D/K=.02s, loadmg/4,
.001m/rad Jacobian damping/.05rad cap. This is privileged teacher sensing.

The .06860s sustained run96976 is TERMINAL: .066697/.067242/.066415,2.07%slip,
controls and heading pass; no sustained dense geometry audit. Removing unused
feedback config payloads matches401physicalframes exactly, but the original
feedback_observations=false already disabled computation: do not claim speedup.

Latest user priority is CONTACT-IMPLICIT discovery. Current phase planner has one
stance/swing per foot/cycle and does not implement IDTO. PLANC CLF-guided learned
tracking is also absent for this gait. Next implement shared smooth state-dependent
contact and generalized-position inverse-dynamics optimization, initially with an
analytic example, then whole CAD robot. No fixed foot-contact schedule. Keep planner
smoothing explicit and separate from detailed validation physics. See the newly
updated planner-literature.md (primary equations3–13 rechecked). Do not spend the
next turn on another manual timing/height sweep. Browser responsiveness and .21
quality remain requirements of the active full goal; no physical maximum proved.

Runtime archivev4 finished and self-verified over v3 (48 files,98,316,651bytes);
browserv6 finished and self-verified, retaining .21 (24files,6,245,527bytes). Preserve older
commit-scoped manifests. Current turn source changes are after930b073; final native
run_environment includes position_gain, compiler includes diagnostic flag, current
optimizer/WASM remain earlier binary versions with unchanged physical laws.

## Previous checkpoint — diagonal references and shared runtime improvements

HEAD before this checkpoint: 01e25b5. This turn made progress; the goal remains
active. Forward is +45° relative to CAD chassis. First dense feasible diagonal
reference .041099 m/s, then .067807 after correcting the optimizer's clipped
motor-envelope gradient without changing the physical torque law or audit gates.
A stronger speed continuation is active (session 17738, diagonal-gradient-speed).
Do not hash/stage its result/log until terminal; intermediate .15–.17 m/s iterates
are not measured walking or final dense acceptance.

The .067807 reference passes 4,000-point interpolation and forward/reverse load
checks. Detailed 8 s walking .064245/.067133 m/s, 2.07% slip, 18/18 clearances,
zero overlap in 401 reported poses. Forward narrowly misses the ±5% short-window
tracking gate. All standard 20 s/1.25 and .625 ms and 12 s dropout captures pass
control plus 5% slip gates. Fine human .066647/.067608 m/s, turn .22475 rad, slip
4.51%; dropout .065859/.067075, slip .98%. Timestep difference 2.186 mm / .189%.
Detailed long clearance passes 78/78, with zero overlap in 1,001 poses.
Sustained/dense-between-step checks remain separate.

The compiler now requires reverse feasibility when explicitly requested, and
splits static/odd/even feedforward for clock rates 0/+1/-1. Regression preserves
old forward behavior and rejects the known bad reverse axial reference. Shared
immutable trajectory caching preserves all 401 native physical/policy frames
exactly while avoiding repeated large parameter cloning. All 35 script and 21
robot tests pass, as do targeted planner, cache and UI regression checks.

Preserved user axial browser: http://127.0.0.1:55732/?preset=physics-compiled184-5ms
Fixed experimental diagonal seed: http://127.0.0.1:59612/?preset=physics-diagonal-seed-5ms
Original diagonal bundle at 59174 has a known reverse UI rounding bug. Do not
promote it; the fixed bundle keeps exact typed command bounds. Fixed rendered
walking completes W/stop/S/stop and moves 65.86 mm, but only .703× realtime /
68.82 ms p95. Native/WASM parity narrowly fails three force comparisons (1.253e-7 N;
link pose difference 11 pm); tolerance unchanged. An earlier live harness stood
still: retained erratum invalidates that supposed walking result, and the harness
now fails closed for unsupported presets. The proper harness verifies body motion.

Existing guarded Broyden profiles have <2 nm native body-path differences and
preliminary wall times 5.30/5.69 vs 5.98 s. Rendered seed Broyden improves to .875× / 53.1 ms p95, still below acceptance. Mechanical
Jacobian assembly is the dominant profile bucket; reuse shared solver components.

Runtime evidence v2 is incremental over v1; browser v2/v3 are full original/fixed
diagonal bundles. Both finished and self-verified. Runtime v3 and browser v4/v5 archives finished and self-verified for the new .068 validation and guarded Broyden bundles. New direct manifest must be
commit-scoped; do not overwrite older evidence manifests. The original user
worktree and all old browser bundles remain untouched; no agents were used.

Next: finish speed continuation and independently compile/run its retained plan;
finish .068 long clearance and dense/sustained checks; improve actual rendered
walking pace using measured shared solver changes; continue broader contact/body
planning if local search stalls. Neither inverse-reference failure nor a local
search box establishes the global or hardware physical speed maximum.


Latest interactive diagonal baseline: http://127.0.0.1:60389/?preset=physics-diagonal68-5ms
opened in Chrome tab 291878297, server session 30019. W/S request 45° travel;
A/D steer. Native 5 ms preview differs by at most 1.831 mm from the .625 ms
screen. Rendered W/stop/S/stop completes and moves 104.5 mm in the active window,
but .801× realtime / 46.06 ms p95 still fails browser responsiveness acceptance.
Keep this user bundle immutable. No hardware-speed or sustained-motion claim.

## Previous checkpoint — generated controller and diagonal travel

Previous goal turn: progress (built and opened the new requested candidate).
This turn: progress: closed-loop control/geometry audits, forward/reverse inverse
load comparison, shared multiple-clock planning, and 0/+45/-45 degree CAD heading
audits. The user now wants forward travel 45 degrees relative to the chassis to
use all belt hips. Preserve that steering in further work; do not rotate only the
viewer or the whole robot/world together. No physical/global maximum is proved.

Worktree HEAD before this checkpoint: ed94e5c. New detailed documentation and
versioned direct results are in ../contact-planning/README.md. Adaptive refinement
automatically added phase .2355 and restored dense sampled forward feasibility at
.1841768 m/s. The generated controller ran 8 s, then standard 20 s steering/reverse
at 1.25 and .625 ms, plus 12 s command loss. Both human cases and dropout pass
control checks. Human speeds .182924/.184506 and .182540/.184022; turn .24323 rad;
stop .64 s. Timestep path difference 2.738 mm, speed .262%. Dropout settles after
.80/.46 s, first exactly on the limit (1 ns timestamp roundoff only).

Failures remain: loaded-foot slip 7.70/7.79% human and 10.60% dropout, versus 5%
development quality gate. Independent-schedule clearance passes 25/28 in the
8 s screen and 109/124 in 20 s/1.25 ms. Failed +X foot swings occur in reverse and
turning. No full sampled interlink overlap in 401/1,001 recorded poses. These are
20 ms reporting checks, not dense between-step audits. Sustained and browser
fidelity/realtime validation are incomplete. Reverse inverse-load audit fails
the torque gate at -.07390 Nm, despite passing forward at -.00884 Nm.

The shared planner now appends explicit additional reference-clock operating
cases, retaining one primary speed objective and all physical gates. Combined
forward/reverse frames exactly match independent scalar-clock CAD evaluations.
Directional displacement is a scalar along an explicit unit vector; overlapping
Cartesian/directional decisions and silently changed initial headings are rejected.

At unchanged .1841768 m/s reference speed, heading0 hip ranges are
18.44/1.21/19.37/3.14 degrees; +45 ranges 14.33/8.61/16.77/9.94. Diagonal warm
starts fail balance/torque and need optimization. `heading-plus45-restoration`
is actively optimizing 54 variables with both forward/reverse clocks. The
initial feasibility stage may slow the motion; its intermediate speeds are not
gait candidates or physical limits. Subsequent work must continue speed search,
compare other support-pattern/posture seeds, and validate live diagonal control.

Active optimizer at checkpoint preparation: exec session **5191**, writing only
../contact-planning/heading-plus45-restoration.{result.json,log}. Poll this exact
handle before assuming termination or restarting. Those two live files must not
be staged, normalized or hashed until the writer is terminal. Recipe is stable.
The source binary is the final directional build. Tests and other experiments
are terminal. One attempted test invocation used nonexistent integration target
contact_audit; the corrected invocation and 5 library tests pass. Other checks:
3 planner tests, 4 contact-phase tests, 1 Rhai binding test, 1 initial-pose test,
3 lift tests, 5 motion-provenance tests and 6 walking-task tests pass.

New user browser: http://127.0.0.1:55732/?preset=physics-compiled184-5ms,
tab 291878290, server **76389**, immutable runs/speed-ceiling/viewer-compiled184.
Verified ready and marked deliverable. Do not refresh, overwrite or close it.
The old user worktree, CAD, .129 and constrained165 bundles remain untouched.

Runtime archive ../contact-planning/runtime-evidence/evidence-v1-index.json:
31 files, 47,055,543 bytes, SHA 5fd1c007a946e0f79ef0b54daef9eb131fc0d266447a4f1c59064aebd73cbc63.
Browser archive ../contact-planning/browser-evidence/evidence-v1-index.json:
25 files, 6,886,849 bytes, SHA 92dc0657350fbbd6cc89fa55457d24cd2bb4ec4b9c82f770d5ee2f1f5a6fe570.
Both extracted and every file hash rechecked. Earlier speed-ceiling v1-v8 chain
still restores the prior CAD analysis scene; new archives retain their own roots.

## Prior checkpoint — event-aware joint planning and wider posture seeds

Previous goal turn: progress (dense audit disproved the first coarse plan).
This turn: progress: corrected contact interval sampling, three contact-pattern
restorations, speed continuation/restoration, wider CAD hip posture seeds,
independent dense audits, solver reporting correction and passing regressions.
The objective remains active; no global or hardware speed maximum is proved.
No subagents used. All changes belong to this isolated exploration worktree.

The shared contact phase library now partitions every stance/swing event and
keeps zero-duration slots for coincident events. Explicit contact_intervals mode
samples each positive interval near both edges and at its midpoint; duration
weights affect cost only, while maximum physical-error gates inspect every
sample. Legacy uniform mode remains available for earlier diagnostic recipes.
A regression exposes 1e-7-cycle flight intervals that a 256-point grid misses.
The formerly misleading 0.196316 m/s plan now reports 41.594 N imbalance even
at the event-aware coarse sampling density.

Three independent phase initializations were optimized for feasibility. The
successful paired start reaches 0.059272 m/s and passes an independent 512-point
audit (0.04828 N force, 0.01566 Nm moment, +0.7748 Nm torque margin, 0.000399 mm
penetration). Wave and neighboring-pair searches remain infeasible; this is not
a proof against those gait families. The initial 53 variables jointly optimize
period, stride, foot phases/placements/swing offsets and 6D body spline controls.

Speed continuation reaches 0.200021 m/s but is infeasible. The optimizer now
retains its fastest candidate passing sampled tolerances, independently of cost.
A subsequent weak-speed-penalty restoration retains 0.187895 m/s, passing the
48-uniform-point plus event checks. Independent 1000-point audit FAILS:
force 0.078126 N > 0.05; moment 0.025121 Nm > 0.02; minimum torque margin
-0.012537 Nm < -0.01. Penetration 0.048025 mm passes the 0.1 mm threshold.
Worst balance is phase 0.4995 (two stance feet), not an unobserved flight gap.
Worst torque is phase 0.2415, joint.-X | Worm servo output, +5.52704 rad/s and
+0.012537 Nm required, beyond its 5.51157 rad/s no-load speed in the motoring
direction. See ../contact-planning/speed-restoration-audit-1000.summary.json.
No new contact-planned motion has been executed as a closed-loop controller.

Next: use reported violations to refine the constraint mesh (including body
spline knot phases and the rear worm rate peak), restore feasibility, and check
independently again. Increase body spline freedom if the current four controls
cannot represent dynamically balanced motion accurately enough. Keep dense
feasibility separate from optimization cost; do not waive the physical gates.
Then integrate the generated schedule with shared runtime control, including
acceleration, reverse, steering and stopping. Old paired lift gates must not
silently forbid other valid support schedules.

The initial +/-2 cm foothold box was too narrow to explore larger belt/hip
postures. prepare_posture_starts.mjs now derives a wider box from 20 previously
inspected CAD postures, with an explicit 2 cm fringe. Both 30 and 45 degree hip
initializations find sampled feasible candidates (retained 0.03946 and 0.03888
m/s). These are ready for dense checking and independent speed continuation.
The enclosing box is not a complete workspace or certified free space. Original
CAD joint search and collision/actuator limits remain in force.

Current checks: 3 contact-phase tests, 5 runtime contact-audit tests and 9 solver
library tests pass. Earlier 20 robot and 23 embedding tests pass. The added
solver regression preserves the exact initial parameter/residual pair if no
trial is accepted; normalization used to change its final bits. JS syntax and
git diff --check pass. Every computational job in this checkpoint is terminal:
58854, 50641, 51965, 81566, 67775, 28451, 90723, 53459, 82952 and 8982.
All new recipes, reports, summaries and logs are direct versioned artifacts in
../contact-planning; they are not included in the old archive v8. HEAD before
saving this checkpoint is a7d3b29; inspect git log for the resulting commit.

Preserve user browsers: latest http://127.0.0.1:64517/?preset=physics-constrained165-5ms
(tab 291878006, server 97345, immutable runs/speed-ceiling/viewer-constrained165),
and original .129 m/s at port 59048. Do not refresh or overwrite their bundles.
The latest physical candidate remains .169–.172 m/s; planning results do not
replace it. It passes 166/166 dense standard 20 s lifts at .625 and .3125 ms.
Sustained dense auditing is COMPLETE: 557/564 lifts at 2.5 ms reporting;
17,843 sampled poses, 2,233 exact common endpoints, no sampled overlap. Seven
reverse lifts fail the unchanged duration criterion. Dropout remains 51/52.
The retimed .182–.184 candidate remains 164/174 and misses reverse speed tolerance.
These failures do not prove a physical speed maximum.

Latest rendered preview measurements are COMPLETE: 5 ms 0.5991x simulation/wall
speed, p95 frame 61.435 ms; 10 ms 0.7128x, p95 63.660 ms. Both below realtime;
10 ms measurement overlapped compilation. Native/WASM parity passes for both.
Approximation path errors remain 8.94 mm (5 ms) and 25.90 mm (10 ms), failing
the 3 mm path screen. Browser responsiveness/fidelity is still unresolved.

Archive v8 is COMPLETE and VERIFIED: 295 additions/changes, 13 parts,
606,458,139 bytes; full snapshot 1,633 files. SHA256:
4b55687079dddddd6352cbebbf17307d8a2f981c8f82c671f679ff1bff456a28.
It contains prior constrained experiments and viewer bundle, not the new direct
contact-planning JSON files. Preserve the archive chain and web/node_modules
symlink. Do not stage the symlink. No subagents used; original user worktree
and CAD remain untouched. Global maximum unproved; keep the goal active.

## Previous checkpoint — rate redistribution and selective swing timing

Previous goal turn: progress (7682c0f, selective-lift sustained qualification,
browser measurements and verified archive v6). This turn: progress with shared
rate-only time brackets, reference redistribution, ten completed physical
experiments and corresponding planned-lift/geometry evidence. The global
physical maximum remains unproven. Keep the full goal active; no agents used.

The original user worktree and all user browser tabs/bundles remain unchanged.
The .129 m/s port-59048 trial stays available. No new browser was built in this
round; realtime performance remains unresolved. The previous .154–.155 m/s,
60 s, 512/512-lift candidate remains the strongest sustained faster result.

Shared `Trajectory::rate_traversal_bounds` brackets
`integral max_i(abs(dq_i/ds)/budget_i) ds` using endpoint-displacement lower
durations and analytic polynomial-rate upper durations. Nested 4/64/256
subdivisions tighten all three tested paths. Existing uniform maxima and
budgets remain exactly unchanged against their archived reports.

Rate-only speed brackets at the declared 52 mm stride:
- all 8 mm: .277967–.278048 m/s (uniform .171547);
- +X 10 mm: .272060–.272145 (uniform .162220);
- +X 12 mm: .264428–.264521 (uniform .137754).
They omit torque/acceleration, contact, phase continuity, holds and stability;
f64 bounds are not outward-rounded interval arithmetic or global speed limits.

Shared `redistribute_periodic_rates` and generic `redistribute_trajectory`
example form an explicit phase-duration blend between anchored intervals,
then sample the original curve into 160 periodic B-spline controls. New curve
is C2 but changes geometry and may shift actual swing edges after smoothing.
Anchors 0/.04/.36/.4/.44/.76/.8 s preserve old nominal transfer/braking times.
Blend 0/.5/.8 uniform rate budgets: .163414/.178849/.188208 m/s.
No CAD, actuator, world, initial-pose or steering/lease/braking changes.

All-channel .175 dynamic tables have residual moments 8.53/16.88 N·m and
suggested peak increments .0509/.0680 rad (retained cap .06). Their complete
20 s/.625 ms trials reach .18866/.18611 and .18863/.18726 m/s, slip 14.84/15.84%,
lifts 158/174 and 152/174. Control checks fail speed; turns/release stops pass.
They are exploratory faster motion, not a better qualified gait.

Two initial uncorrected .175 cases fail input validation at .4 s because
arithmetic produced .17500000000000002 outside .175. Preserve these rejected
captures; never interpret their partial slip ratio as locomotion evidence.
Fresh `bounded-inputs` cases retain identical scene/config and correct exactly
780 input values by 2.77555756e-17 m/s. Not-yet-executed dynamic cases were also
corrected with original actions retained. `retiming-input-corrections.json`
maps old/new hashes; dynamic preparation fingerprints precede the correction.
Final executed hashes are in the validation catalog and correction ledger.

All-channel .175 without compensation reaches ~.200/.198 m/s, slip 15.7–15.9%,
lifts 156/174 and 158/174. At .150, zero blend has 146/150 lifts and 11.15% slip;
.5 blend has 145/150 and 13.52%. All six complete all-channel audits have 6,006
poses with zero sampled overlap and exact policy command replay. No exact-CAD
or between-frame collision claim. Detailed table is in README.

Because retiming stance joints changes the base motion implied by no-slip
support, `RateRedistributionConfig.coordinate_intervals` now optionally
selects which channel intervals use the new phase. Empty channel lists keep
original phase; endpoints must be anchors. Swing-only recipes retime -Y/+Y
joints only .04–.36 s and +X/-X only .44–.76 s. They preserve the rate budgets
while reducing prescribed residual moments to 6.59/7.86 N·m. Constant-base-speed
feedforward still has incomplete support moments and is only a candidate.

All four selective native trials and final analyzer completed exit 0 in session
48402. Geometry sessions 37593, 41182 and 59162 all completed exit 0.
Full control regression session 28671 completed exit 0 with 13 trajectory tests.
Tests cover analytic times/total variation, bounded refinement, rate switches,
monotone anchored phase mapping, C2 continuity and inactive-channel controls.
Three new JS scripts pass syntax checks. Initial test-development failures are
retained; final error-bound assertion uses the analytic total-variation bound.

Selective swing results (command .175; all 20 s/.625 ms):
- .5/off: .20216/.20073 m/s, 14.02% slip, 156/174 lifts, 37 incidental unloads;
- .8/off: .20535/.20327 m/s, 14.17% slip, 154/174 lifts, 45 incidental unloads;
- .5/full: .18869/.18656 m/s, 14.05% slip, 156/174 lifts, 41 incidental unloads;
- .8/full: .18968/.18940 m/s, 13.96% slip, 152/174 lifts, 61 incidental unloads.
All four overspeed and fail the separate slip screen; turns and release stops
pass. Release stops in .36 s over 28.6–32.4 mm, late drift .272–.280 mm.
All 4,004 poses have zero sampled inter-link overlap; exact command replay.
No new candidate promoted, no new sustained/dropout/timestep/browser claims.

All experiment, analysis, geometry and test jobs from this round are terminal.
Archive v7 session 30700 completed exit 0: 168 added files, three parts,
136,344,726 bytes. All 168 extracted files and all 1,338 current source files
were hash-verified. Joined SHA-256:
`c489da87a80e86b20678d49d273ea2e902c94d8f8aa8bb931522641575818f1a`.
Restore the entire v1 → v2 → v3 → v4 → v5 → v6 → v7 chain using
`evidence-v7-index.json`. No experiment/archive job remains running.
The .155 m/s candidate remains best sustained; this branch identifies why
reference-rate headroom alone does not yield a better loaded gait.

Next physics/control work: derive support-consistent body/foot accelerations
and force/moment allocation, or add measured body-speed feedback with load
compensation valid across the resulting rates and transients. Swing-only
retiming reduces slip but not missed lifts at .175. Do not treat the .272
rate-only bracket or these failed trajectories as a global physical maximum.
Do not silently relax existing control/contact-quality or numerical gates.

## Previous checkpoint — selective lift and sustained .155 m/s gait

Previous goal work made progress with six front-lift trials, bounded replay,
rate bounds and sustained qualification. The intervening user request to open
the .129 m/s browser was completed without changing physics. This continuation
makes progress by checking terminal parity results, completing two rendered
browser measurements and preserving/reviewing the new evidence. The physical
maximum remains unproven; useful safe work remains. Do not mark blocked or
complete, and do not spawn agents without authorization.

The selected candidate is `smooth-dynamic-front10-v150-scale1-human-fine`:
same CAD/actuators/world, 52 mm stride, hip 0°, shared periodic B-spline,
8/10/8/8 mm requested lifts in leg order -Y/+X/+Y/-X. Full signed dynamic
increment tables were rederived at .150 m/s, enabled only at the exact nominal
cadence, zero requested yaw and no braking. Original WASD phase/braking/lease
law remains. Planning, all six new 20 s trials, capability analysis and geometry
audits completed. Their summaries and limitations are in README.

Selected 20 s/.625 ms: **.154567/.154978 m/s, 11.55% slip, 150/150 lifts**,
no incidental stance unloads. All control checks pass; the 5% slip screen fails.
12 mm +X lift also gets all lifts but slightly more slip/unloads. At .165 both
with/without dynamics overspeed and miss 12 of 166 lifts. This is not a global
physical ceiling. Exact nominal no-load reference budgets: 10 mm **.162219812**,
12 mm **.137753905 m/s**, both +X FOOT motor limited; more lift consumes motor
speed. Actual body speeds under tracking/slip are not these reference budgets.

`front150-validation.json` is complete:
- Sustained 60 s/.625 ms: .15424/.15520/.15412 m/s, 11.38% slip,
  **512/512 lifts**, no stance unloads, release 56→56.42 s /27.95 mm,
  .262 mm late drift.
- Dropout 12 s/.625 ms: **46/46 lifts**, no stance unloads, packet loss
  3→3.58 s /57.71 mm, .303 mm late drift. Release 9→9.42 s /28.15 mm.
- Finest 20 s/.3125 ms: .15458/.15516 m/s,11.25% slip, **150/150 lifts**;
  four one-sample turning stance-force unloads with slightly negative clearance.
- These three + two preview audits have **6,605 poses**, zero sampled inter-link
  overlap. Original six new 20 s cases add 6,006 such poses. No exact-CAD or
  between-frame collision guarantee. Rhai policy command replay is exact.

Generalized `compare_temporal_trials.mjs front150 front150-human-0p3125ms
front150-finest-comparison` confirms .625 ms differs from finest by 1.18 mm /
.119% steady speed. At 1.25 ms, 3.25 mm misses the existing3 mm screen.
5 ms: 10.10 mm/1.60%; 10 ms: 17.68 mm/3.15%. Both previews fail existing path
accuracy; 10 ms also fails speed. Slip is understated (8.35/6.32% vs 11.25%).
The fine reference is not ground truth. Do not silently weaken the 3 mm/2% gate.

New isolated `runs/speed-ceiling/viewer-front150` uses current WASM and presets
`physics-front150-5ms` / `physics-front150-10ms`; packaging spec and manifest
are versioned. The bundle is immutable and has no persistent user server yet.
Old user port 59048/bundle/tab must remain untouched.

Parity jobs 16925/24870 are terminal (handles missing, complete JSON/logs):
5 ms PASS, 1,000 transitions, max 6.934e-9; 10 ms FAIL, 106 values beyond unchanged
numeric budget, max force 9.078e-7 N, some velocity/torque values also fail.
Both same-host replay/reset exact, invalid input/replay recipe preserved.
No tolerance waiver. Their parallel worker-only timings are not fair rendered
performance acceptance.

Sequential rendered reviews 51827/27049 completed exit 0: 5 ms **.750×/44.2 ms
p95**, 10 ms **.885×/52.6 ms p95**, no page errors and exact scheduled key times.
Saved screenshots inspected: fixed camera lets robot partly leave viewport.
Neither meets realtime. Keep speed in simulation distinct from wall speed.

Shared diagnostic changes: `capture_embedded_window --replay recording.json
START END PERIOD` uses production replay with unchanged config/input schedule.
`evaluate_lift` accepts completed bounded replay windows with matching scene.
Two windows of the previous all-8 mm dynamic150 case at 1.25 ms observations
match **47/47** common 20 ms physical/policy endpoints exactly (excluding only
wall time and next host-boundary held inputs). One old forward failure now
has 20 ms qualifying span; old turning failure has 17.5 ms and still fails.
All 722 dense poses have zero sampled overlap. Does not requalify the whole original gait.
CLI rejections and aligned-time unit test pass; final examples built.

Batch preparers now accept named smooth/dynamic specs with explicit hashes.
Validation supports recorded solver overrides and duration-scaled wall guards.
Analyzer retains only comparison body coordinates instead of full captures;
all prechange summary values were verified exactly unchanged. Motor-demand
summary is recorded statistics, not alternate dynamics: about 7.3 W peak total
positive power at 20 ms samples does not establish usable spare power or extrema.

Archive v6 session 25864 completed exit 0: 174 added files in five parts,
198,493,297 bytes. All 174 extracted files and all 1,170 source files were
hash-verified. Joined SHA-256:
`9dab68402041e102705cead63b8bb8544a18525d341cc8069f84a44d32e1e9ec`.
Restore the full v1 → v2 → v3 → v4 → v5 → v6 chain using
`evidence-v6-index.json`. Browser reports/recordings/timings/screenshots are
versioned directly beside the archive. No simulation, browser test or archive
job remains running. Eleven changed/new JS scripts pass syntax checks. Final
`cargo test -p sim-runtime --example capture_embedded_window --example
evaluate_lift` completed exit 0 (session 38591); the replay-window aligned-time
test passes, and the lift CLI's positive/rejection evidence is retained separately.

Next useful work:
- Retime/broaden +X clearance without raising peak/rate demands; regenerate CAD
  IK, smooth reference and signed loads. If changing lift-support bounds, also
  change phase-safe stopping and force-allocation timing consistently.
- Revisit wider hip 30°/45° postures to redistribute worm/foot demand into belt
  motion, with measured yaw/body feedback. Previous open-loop yaw Jacobian
  understeered; larger nominal budgets alone do not qualify a faster gait.
- Improve browser runtime cost while retaining detailed validation and explicit
  approximation error. Neither current timestep preview meets all requirements.
- Full momentum/support planning remains unresolved (prior prescribed support
  residual body moment ~3–4 N·m). Do not generalize steady tables to transient
  cadence or yaw without a derivation and validation.

## Previous checkpoint — smooth reference and dynamic loads

Previous goal turn: progress (commit 683afa2, qualification and numerical/browser
evidence). Current turn: progress (shared smooth trajectories, exact polynomial
rate bounds, dynamic inverse-load reference analysis, seven physical trials and
all corresponding phase/geometry audits). The goal remains active and unproven.

Native/build/test/analysis jobs 1856, 26886, 72577, 72833, 22062, 86440, 18829,
13074, 97915, 36535 and 73725 completed exit 0. The initial Rhai trajectory test
failed on an unqualified top-level constant; the final state-free config-based
API and all final relevant tests pass. No failed behavior is hidden by a test
waiver. Archive v5 session 14075 completed exit 0: 118 added files, two parts,
93,381,545 bytes; all 118 extracted files and all 996 source files verified.
Joined SHA256: `b32968b3c63c51aa49394b0beb0e914c115334120e82688f6638f2d58787784c`.
Restore the complete v1 → v2 → v3 → v4 → v5 chain from `evidence-v5-index.json`.
No experiment/build/audit/archive job remains running from this round.

Shared Rust `Trajectory` adds `PeriodicCubicBSpline`, with uniform periodic
control points, a repeated final value, analytic derivatives and exact absolute
rate maxima including interior extrema. Linear/quintic behavior is unchanged.
`trajectory_sample(config,time)` in Rhai uses this same implementation and
retains numeric controller state. The smoothed reference remains in the control
point hull, but changes achieved foot paths and does not imply CAD clearance.
The 52 mm exact nominal rate budget is **.171547228 m/s**, +X worm limited.

`reference_load_feedforward` now accepts optional shared trajectory and explicit
phase/base derivatives; chain rule includes phase acceleration. It reports
required torques, prescribed support forces and unbalanced base moment, and
exports dynamic increments relative to a static allocation at the same pose.
It rejects linear dynamic references, both missing CAD hashes and nonfinite
support sums. Archived static target offsets remain exactly unchanged.
Analytic inverse-load tests and CLI zero-motion/rejection tests pass.

Seven fine 20 s trials and audits are complete (`smooth-trials.json`,
`dynamic-load-trials.json`, `validation-summary.json`):

| Command | Compensation | Forward/reverse m/s | Slip | Lifts | Control |
|---|---|---|---|---|---|
| .125 | smooth only | .12765/.12922 | 7.97% | 124/124 | pass |
| .150 | smooth only | .16126/.16496 | 11.68% | 133/150 | fail |
| .175 | smooth only | .19225/.19772 | 14.65% | 134/174 | fail |
| .125 | half dynamic | .12609/.12686 | 7.95% | 124/124 | pass |
| .125 | full dynamic | .12423/.12439 | 7.54% | 124/124 | pass |
| .150 | half dynamic | .15760/.16004 | 11.41% | 143/150 | fail |
| .150 | full dynamic | .15387/.15502 | 11.49% | 143/150 | pass |

All seven captures reproduce commands under exact Rhai state replay and all
7,007 recorded poses show zero sampled inter-link penetration. None passes
the 5% slip screen. No new browser bundle was built or installed; the user tab
at port 59048 retains its prior 5 ms flat125 candidate without interruption.

**Next physical experiment:** all seven failed lifts of full-dynamic .150 are
the +X foot. No support-force failures; clearance/unloading do not meet a
consecutive 20 ms recorded span (peak clearance 3.45 mm forward, 4.56 mm while
steering). Three occur during steering, when the increment is disabled. Try
selective +X lift/plateau improvement using existing CAD IK (`lift_by_leg_mm` in
`prepare_clearance_trials.mjs`), then regenerate the smooth curve and signed
dynamic tables. Consider denser same-runtime capture to distinguish brief
clearance from a 20 ms sampling limitation; changing `report_every` alone in
the environment does not increase recorded endpoint frequency.

Dynamic tables are approximations: increments peak .016–.025 rad, capped .06;
separate signed steady-cadence tables, 5 ms phase spacing, half/full scale.
Enabled only at exact nominal cadence, zero requested yaw and no braking.
The prescribed allocation leaves up to 2.96/3.96 N·m residual body moment for
.125/.150, so loaded whole-body planning remains unresolved. Do not extend
these tables to other cadences, new paths or turning without rederiving them.
The newest recipes are intentionally immutable; preparers currently target
fixed source names and will need explicit new-case inputs for the next batch.

## Previous numerical round and user viewer

Previous turn: progress (opened and verified the requested robot trial).
Current goal turn: progress (completed solver parity/rendered measurements,
finest-reference trajectory comparisons, and durable qualification reporting).
The goal is neither achieved nor blocked.

Qualification session 40274, original temporal session 14921 and reference
browser parity session 18037 all completed with exit 0. Rendered reference and
optimized reviews (85442, 65865) also completed with exit 0.

**Session 21640 completed exit 0**, including additional SDIRK2 2.5/1.25 ms
cases, standard analysis and finest-reference comparison. Its logs are
`temporal-refined-native.log`, `temporal-refined-analysis.log` and
`temporal-finest-comparison.log`. No expensive browser test remains running.

The user requested the 0.129 m/s robot and it was opened and visibly verified:
http://127.0.0.1:59048/?preset=physics-flat125, server session **22172**, bundle
`runs/speed-ceiling/viewer-solver`. Chrome tab 291877897 was marked deliverable.
It uses 0.125 m/s command, optimized 5 ms solver, and the same Rhai WASD policy.
Do not reload, close, change its bundle, or interrupt the user's trial.
The older 0.10 m/s viewer at port 57909 (15198) is also preserved.

## Flat125 qualification and solver evidence

The additional flat125 qualification is complete: 60 s at 0.625 ms sustains
0.12668 / 0.12882 / 0.12674 m/s with 8.48% slip. Release at 56 s stops by
56.26 s, 6.66 mm displacement. Frozen commands at 3 s stop by 3.48 s,
37.19 mm displacement. Human runs at 5, 1.25 and 0.3125 ms also pass control
checks. All fail the separate 5% contact-quality criterion.
Four new planned-phase audits pass **710/710** lifts (424 sustained, 38 dropout,
124 human 5 ms, 124 human 0.3125 ms); 5,604 poses have no sampled inter-link
penetration, and no incidental stance unloads were found in these audits.

No Rust model/code change was needed for solver trials: existing Broyden and
guarded linearized Jacobian options substantially reduce closure work.
External profiling preserves all reference physical values exactly. Optimized
solver vs reference is NOT within the strict raw numeric tolerance (some force
differences fail), though maximum body path difference is 5.89 nm. Separate
native/WASM parity passes for each configuration (1,000 transitions), with
7.60e-9 reference / 8.51e-9 optimized maximum differences; replay/reset exact.

Rendered WASD measurements on the shared host: reference **0.351× / 125.5 ms
p95**, optimized **0.580× / 68.0 ms p95**, both complete without page errors.
These are observed sequential timings, not isolated benchmarks; realtime still
fails. The fixed camera lets the robot move partly out of frame by the end.
Worker-only timings must not be substituted for rendered timing.

`compare_temporal_trials.mjs` now verifies unchanged scene/task/seed/command
values and simulation times, actuator/contact/controller configuration and
convergence tolerances, then compares every recorded pose with the finest
0.3125 ms backward-Euler run. Maximum body differences: 0.625 ms BE **1.50 mm**,
1.25 ms BE **4.30 mm**, 5 ms BE **13.31 mm**, 10 ms BE **19.57 mm**; SDIRK2
5/10/20 ms **3.47/11.17/24.67 mm**. All coarse profiles miss the 3 mm screen;
20 ms SDIRK2 also fails commanded speed tracking. No numerical profile is
promoted based on selectively comparing against a coarser reference.
Additional SDIRK2 2.5/1.25 ms runs give 0.69/1.92 mm path difference, passing
the existing path/speed and control screens, with 8.87/8.92% slip. Difference
against a finite BE reference is non-monotonic; do not claim uniform convergence
or conclude 2.5 ms is more accurate. These variants are not browser/geometry
qualified. Realtime performance and lower slip remain unresolved.

Archive v4 now preserves this qualification/performance family: 106 added
files, five parts, 191,223,346 bytes. All 106 extracted files and all 878 source
files were hash-verified. Joined SHA256:
`2857a63494f3f57eb5f705100b2ffe5e30bec4f5ab6bfe8302660cd4e0510879`.
Restore the full v1 → v2 → v3 → v4 chain from `evidence-v4-index.json`.
Archive session 80765 completed exit 0. No experiment/build/archive job remains
running from this round; viewer servers are intentionally retained.

## Completed progress

The prior hold-compensated 52 mm gait at 0.10 m/s still has the strongest
completed low-slip qualification: 0.1004–0.1027 m/s sustained, 4.30% slip,
fine timestep, dropout and 562/562 planned lifts checked. Its browser remains
slow: .4468× realtime / 109.6 ms p95. A 10 ms profile had 7.22 mm path error
against 1.25 ms and missed the existing 3 mm comparison gate.

This round adds 31 development screens and seven fine WASD captures, summarized
in `exploration-summary.json`. All use shared Rust planning, physics and
analysis; new code is robot-policy/experiment configuration and orchestration.
The existing Rust angular-integral kernel tests pass (3 tests).

Wider hips redistribute work to the belt but alone do not improve low-slip
walking. Exact 52 mm-cycle no-load budgets rise from .11077 m/s at 0° to
.11500 at 15°, .12683 at 30°, and .15086 at 45°. The 45°/0.15 m/s screen uses
about 2.4 rad/s front hip speed. COM centering from the existing shared Rust
mass/pose support calculation reduces midstance line errors from 6–7 mm to
0.8–1.0 mm. It improves 45° high-speed coarse slip from 43% to 17%, but worsens
slower gaits; it is not a universal balance solution. Bounded integral feedback
did not materially improve the tested combinations.

The useful new change is a flatter horizontal foot-return profile, with smooth
quarter-duration acceleration/deceleration ramps and constant-speed middle.
Peak normalized world return rate drops from 1.875 to 4/3. Exact cycle rate
screens become .17130 m/s at 0°/52 mm, .18395 at 30°/52 mm, .18986 at centered
45°/52 mm, .19780 at 30°/80 mm and .22874 at 45°/80 mm. Wider-hip flat returns
become foot-motor limited. These are no-load trajectory screens, not global
ceilings. Longer strides and smaller/larger lift heights were also tested.

Fine 20 s / 0.625 ms flat-return results (hip 0°, stride 52 mm, lift 8 mm):

| Command | Forward / reverse m/s | Slip | Planned lifts | Turn |
|---|---|---|---|---|
| .110 | .11014 / .11088 | 6.20% | 110/110 | .21069 rad |
| .125 | .12690 / .12874 | 8.64% | 124/124 | .22316 rad |
| .150 | .16351 / .16299 | 11.31% | 138/150 | .24107 rad |

All three have zero sampled inter-link penetration and stop within .40 s.
The first two pass speed/tilt/turn/stop checks but exceed the 5% contact-quality
criterion. The .150 case also overspeeds and misses lift-duration windows.
This supports investigating .125 as a faster usable candidate while retaining
its slip limitation, rather than calling .100 a physical maximum.

WASD steering weakens at wider hips. Initial CAD least-squares hip-only steering
at 45° produces .056 rad over a requested .24 rad turn. Mapping all three joints
with the existing Rust Jacobian/rate solver improves it only to .072 rad.
At 30°, the corresponding changes are .131 to .140 rad. These are not promoted.
Human control checks now explicitly require .24±.06 rad yaw change in addition
to speed/tilt/stop. Static initial Jacobian matching is insufficient; consider
angular-rate feedback and/or a changing Jacobian for wide-hip steering.

## Diagnostics and next physics work

`reference-tracking-diagnosis.json` uses exact Rhai replay to align reference
phase with original joint sensors. Front/rear worm errors vary with phase/load;
rear-worm error correlates ~.71 with a neighboring-slope acceleration proxy in
the flatter .110 gait. This is diagnostic evidence, not causation or a fitted
controller. The literal joint table is piecewise linear and has slope jumps.

A useful next model-based step is a differentiable joint reference plus dynamic
inverse-load feedforward. Reuse `RigidEmbedding::solve(seed,q,reduced_velocity)`
and `prepare_dynamics(...).required_reduced_forces(reduced_acceleration)`.
`reference_load_feedforward` currently evaluates only static loads and prescribed
point-force shares. If extending it, preserve the explicit residual base moment
and never apply it secretly to the robot. Velocity/acceleration tables must be
consistent with the actual controller interpolation and signed cadence, including
reversal/stop. Also fix its permissive missing-CAD-hash equality and nonfinite
support-weight sum if touching that example. Existing analytic inverse-load
checks provide the starting validation. Avoid adding unused parallel physics.

Dense reporting remains necessary for reliable brief lift/slip/peak-load claims.
Current summaries sample every 20 ms, not every physics step. The environment
uses the controller/task period; changing report_every alone is insufficient.
Browser runtime performance remains a required separate workstream in shared
Rust components, with explicit measured approximation error.

## Durable evidence

`evidence-v3-index.json` is an incremental overlay on v2, which requires v1.
It archives 336 added files in seven parts (329,848,803 bytes); all 336 extracted
files and all 772 source files were hash-verified. Joined SHA256:
`f4b3536571cf3ff487b820094093e95fd00fda42c47ed858f4228d80e4ec9fc7`.
The index's restore command restores the entire chain in order. It includes
all completed belt/integral/flat-return experiments, exact-cycle capability
reports, CAD support/steering derivations, seven fine captures and three flat
planned-lift audits. It predates the completed flat125 family archived in v4.
