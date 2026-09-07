# CAD-derived walking and learning delivery

The active goal is the complete workflow: quadruped-appropriate PLANC-inspired
footstep guidance → teacher-policy RL → student distillation → further training
with bounded environmental perturbations. Deliver a fast training model and a
detailed validation model from the same CAD physical definition, using shared
Rust components, with Python confined to CAD work. A passing small example or
faster solver kernel does not complete this goal.

## Current evidence and sequence

The [browser latency investigation](latency-profile.md) now separates WASM work,
transport and UI costs. Numerical Jacobian assembly dominates the native profile
(60%); inter-part collision queries account for a nested 26.5%. A browser-only
precision experiment preserves sampled motion but still misses the 20 ms p95
target (27.5 ms), and a looser setting fails on the first lift. No precision
change is promoted. Next investigate fewer full derivative evaluations or an
explicit browser collision reduction with independent geometry validation.

The current complete goal is `active-goal.md`. Realtime browser walking and WASD
are required early milestones. A substantially simplified, explicitly declared
browser physics profile is permitted; retain the detailed model for validation.
Do not wait for detailed-model realtime or completed RL before delivering browser
walking. `teacher-environment.md` provides the shared sampled task interface,
versioned robot recipe and native/WASM replay path for this next stage.

Latest controller work: `reversal-validation.md` and `reversal-status.json` add
reversal-aware foot ordering, a smaller reverse body shift to avoid a reference
collision, and support-dependent standing feedback. Both reversal directions,
three switch timings, and a turn/reverse/stop sequence pass the sampled gates.
The 60 s sustained recipe passes 28 swings and stops with 0.965 mm body error.
A 10 ms refinement changes foot/body positions by at most 0.479/0.221 mm in the
24 s reversal case. The 60 s turn/reverse native/WASM trajectory agrees within
1.78e-10, with exact replay/reset. The rendered sustained keyboard run reaches
1.001× realtime during active motion, but p95 transition latency is 29 ms.
Next measure the expensive active transitions, improve latency and gait speed,
and expand command/terrain coverage before treating this as general teleoperation.
The fixed crawl and earlier online prototype remain available. Planning,
teacher/student learning, disturbance training and hardware calibration remain
required work; this controller checkpoint does not complete the active goal.

Earlier browser profile: `effective-servo.md` introduces a registered bounded
position servo with explicit CAD-derived gains and torque/speed limits. The
20 ms browser profile preserves the mechanism/contact model and runs the short
2.8 s reference at 1.71× realtime in headless Chrome, with p95 transition latency
22.3 ms. Native/WASM comparison, exact reset and replay pass. Sampled foot/body
differences versus a 1 ms effective-actuator reference are 0.63/0.41 mm.
This is a short single-foot experiment, not sustained walking or performance
acceptance: the 20 ms latency target, full gait, WASD, rendering latency and
hardware calibration remain outstanding. Continue toward a controllable gait
using this browser profile while preserving detailed-model validation. Existing
CI thermoelastic convergence and detailed pendulum portability failures still
need diagnosis; do not report the whole CI suite as green.

Previous delivery: `drive-backlash-validation.md` introduces v4 explicit
drive-connection backlash with provenance, CAD/UI/REST validation and undo,
and shared Rust resolution. Older scenes preserve their exact trajectories.
The robot's twelve additional connection gaps are explicitly estimated as zero;
this is an uncalibrated physical experiment, not a new hardware measurement.
All six full motions pass 360 ms of sampled lift. The 1 ms case passes the
existing preliminary motion screen versus 0.0625 ms (maximum foot difference
0.131 mm), but current transients still differ by up to 0.173 A. An isolated
ABBA comparison reduces 0.25 ms stepping from 35.951 s to 14.568 s at 1 ms,
2.468× faster and still 5.2× slower than realtime. Firmware events limit both
1 and 2 ms nominal recipes to 2,800 accepted segments. Full 281-frame browser
parity, exact replay/reset, unknown-property rejection and 24 UI checks pass.
Next examine effective actuator integration and remaining per-segment work,
while progressing planned steps and the learning pipeline. Do not turn the
provisional numerical screen into a hardware or training-model acceptance claim.

Latest physical-model finding: `hip-timestep-validation.md` compares three
nominal timesteps in a 0.7–1.65 s observation window. A geometry-derived radial
bearing clearance / COM lever estimate supplies the -Y hip's entire 1.635°
motor-backlash gap. Setting only this inferred value to zero in a recorded
sensitivity scene reduces successive timestep foot disagreements from
2.570/1.406 mm to 0.184/0.030 mm. This is not hardware calibration or a promoted
model. Prioritize separating bearing play from drivetrain lost rotation in the
CAD/export contract, preserving provenance, then run full-motion and larger-step
task gates. The original two observation windows match all 96 shared full frames
exactly; the generic Rust capture tool is read-only. Avoid optimizing an
unsupported physical assumption merely to match the old detailed reference.

Latest investigation: `contact-query-validation.md` profiles the remaining
dynamics cost. Contact pair queries account for about 8.55 s of a 46.9 s run;
inertia assembly is only 3.22 s. Pair preparation plus omitted unused normals
preserved both trajectories but improved an isolated ABBA benchmark by only
1.7%; the normal-only screen improved about 2.1%. Both contact changes were
removed; the detailed timers and reusable exact-experiment tools remain.
A fresh current-model timestep comparison shows 2.570 mm foot disagreement at
1.45 s, with about 0.58 degrees of hip-motor disagreement at the preceding policy
sample. Next trace loaded hip dynamics and feedback during lowering, then test
integration accuracy / effective-actuator options that can reduce evaluation
count. Do not extend a chain of low-yield geometry micro-optimizations.

Current experiment: `analytic-positions-validation.md` adds structurally checked
direct slider-crank/transmission positions. All four actual knee loops qualify;
the mechanisms cover all 16 dependent coordinates. Both full timestep runs pass
the existing strict numerical comparison gates and sampled lift requirements.
Original numeric rank, tangent, curvature and closure checks remain. Both full
281-frame browser comparisons and all 22 viewer checks pass. An isolated ABBA
benchmark measures 59.882→46.457 s mean (1.289× faster, still 16.6× slower than
realtime); each method repeats exactly. Evidence is recorded in
`analytic-positions-status.json`. This is a default-off solver experiment, not an accepted training
model. Next, use its measured profile to decide whether analytic tangent/curvature
mapping or structured actuator integration best advances task-valid throughput.

Current optimization: `closure-preparation-validation.md` separates numeric
closure values from display labels, caches fixed unit scales, and omits unused
SVD vectors in the QR path. All original equations and singular-value rank
checks remain. Both complete native timestep runs match the prior binary
exactly, including contacts, events, subdivisions and solver diagnostics.
The coarse development profile reduces linkage mapping from 38.37 to 26.17 s.
A sequential ABBA benchmark measures 70.89→58.50 s mean for 2.8 simulated
seconds (1.212× faster, still 20.9× slower than realtime). Full 281-frame browser
parity, exact replay/reset and all 21 viewer checks pass.
This preserves the existing model and does not establish walking-task adequacy.
The next larger candidate is a structurally certified generic analytic
slider-crank/transmission mapping. Audit applicability to actual CAD topology,
retain internal motion/inertia and all original closure diagnostics, and verify
branches/toggles before replacing iterative reconstruction. Its benefit remains
unmeasured; task acceptance, learning and browser/controller delivery remain
the objective.

Previous optimization: `contact-history-validation.md` removes inverse-dynamics
work from contact-history queries. For the selected memoryless friction law,
the same history-decay equations require no geometry query. Both complete native
motions match the previous binary exactly, including contacts, events and
subdivisions. The contact-history portion drops from 16.24 to 0.39 s in the
base development profile. A sequential ABBA benchmark measures 84.54→71.23 s
mean for 2.8 simulated seconds (1.187× faster, still 25.4× slower than realtime).
All 281 frames of the maintained full-robot browser preset pass native/WASM
comparison and exact replay/reset; all 21 viewer checks pass. Closure mapping
remains the largest measured component.
This does not remove the parent model discrepancy or finish task acceptance.

The preceding `endpoint-correction-validation.md` experiment checks inner
rate-coordinate corrections in physical endpoint units. It removes some tiny
step rejections while retaining original residual bounds, but does not resolve
the local/simultaneous solver differences. Keep its option explicit and the
existing validated controller configuration available in the browser. Further
work must serve fast task-valid simulation and learning, not only strict
numerical equivalence or isolated kernel improvements.

Earlier measured optimization: `auxiliary-coloring-validation.md` compresses
inner numerical probes using declared motor/driver independence. Unknown
adapters keep ordinary probes, and independent audits test the declared zeros.
Both full native trajectories match the uncolored local solve exactly, including
events and contact traces. A sequential ordinary/colored/colored/ordinary pair
of repeats measures 125.28→88.29 s mean wall time for 2.8 simulated seconds
(1.419×). This still runs about 31.5 times slower than realtime. The parent
condensation/reference discrepancies and short-event convergence failures remain;
derivative compression neither changes nor resolves them. The browser motor
fixture passes, while the full condensed robot narrowly fails two force entries
in the strict portability diagnostic (maximum 1.25e-7 N against 1e-7 N).
That experimental robot preset is not exposed; existing robot presets retain
their original configurations. The subsequent coordinate-aware correction
experiment is recorded above; task accuracy and the full learning pipeline
remain in scope.

Latest computational experiment: `auxiliary-condensation-validation.md` retains
all detailed motor equations while solving auxiliary states inside each trial
mechanical endpoint (18 outer unknowns instead of 66). At 0.25 ms, mechanical
preparations fall about 40%, but 9.1 million inner component evaluations consume
the saving: 126 s versus an earlier 128 s for 2.8 simulated seconds. At 0.125 ms,
the candidate takes 217 s versus 205 s. These are development measurements,
not controlled speedup benchmarks. Both runs pass sampled lift, but strict
equivalence fails (0.406 mm / 43.5 nm foot difference; 0.164 / 0.041 A current
difference). A fresh diagnostic pair localizes the first coarse discrepancy to
a reference-only subdivision at 0.028 s. Inner convergence failures also cause
extra subdivisions. Keep this default-off; investigate those failures and
structured inner derivatives before promotion. The browser fixture passes
native/WASM parity and all 21 full-catalog UI checks pass. The accepted fast
training model, task accuracy and learning pipeline remain unfinished.

Latest fast-model investigation: `fast-motor-validation.md` adds explicit shared
winding/rotor quasistatic modes while preserving source CAD values. All robot
runs complete, but combined reduction fails the initial accuracy screen: 4.10 mm
foot difference at 1 ms and 2.81 mm at 0.5 ms versus the detailed reference.
Same-step isolation attributes 1.53 mm to omitting winding dynamics; rotor-only
error is tiny but does not reduce execution cost. Detailed 1 ms integration also
differs by 2.88 mm from the finer reference. Do not promote these reductions or
increase the timestep on completion/lift alone. Keep winding dynamics and test
local motor equation elimination to reduce global solve work; integration
accuracy, task acceptance and the full learning pipeline remain open. The
previous validated browser build remains installed/preserved.

Latest validated numerical result: `newton-convergence-validation.md` identifies
and resolves the sample-reuse refined-current discrepancy. The motor solve was
contracting but exhausted its budget while applying the stricter stale-matrix
correction test. An opt-in shared solver setting reserves the final two
iterations for fresh matrices at unchanged tolerances and cap. Both complete
0.25/0.125 ms comparisons now pass strict numerical limits; the refined maximum
current difference drops from 0.041 A to 9.12e-10 A. Jacobian builds remain lower
than the ordinary-reuse reference (7,353 versus 11,646 at the finer step).
All 281 native/WASM sampled frames pass, replay/reset are exact, and 20 UI
checks pass. A separate shareable final-refresh bundle preserves this result.
This resolves numerical equivalence, not calibrated physics, walking or realtime.
Keep the shared fast training model and task acceptance next in priority;
further small solver changes alone will not close the performance gap.

Earlier sample-reuse result (superseded by the final-refresh comparison above):
`sample-reuse-validation.md` retains a guarded
Jacobian proposal across explicitly eligible servo sampling events. A fresh
native pair improves 160 → 116 s for 2.8 simulated seconds (1.375×); at 0.25 ms,
physical differences stay within strict roundoff limits. The refined 0.125 ms
comparison fails those limits: 0.041 A motor current difference despite only
44 nm of foot-position difference. A fresh reference confirms the discrepancy.
A local extra subdivision near 1.5198 s precedes it; shared scheduler rejection
diagnostics identify a 40-iteration Newton cap immediately after engagement,
with a tiny residual. Diagnostic replay exactly preserves the earlier physical
frames; The subsequent iteration audit and resolution are recorded above. Browser parity/replay and 19 UI
checks pass. This remains experimental and the installed viewer is preserved.

Earlier block-factor experiment: `block-factor-validation.md` factors each current
closure matrix into exact independent blocks, preserving global rank thresholds
and every original closure check. A native pair improves 159 → 152 s (1.045×).
The 0.25 ms trajectory differs only at roundoff scale, but the 0.125 ms run
exceeds strict diagnostic gates: 44 nm at feet, 0.000103 N·s contact impulse,
2.54 µs event timing, and 0.041 A motor current. Supported lift is unchanged.
One browser force reading also narrowly exceeds the strict native/WASM limit.
This remains opt-in and unpromoted; the original factorization remains default
and the previously passing viewer stays installed. Larger gains
must come from reducing repeated residual/geometry work, while retaining full
internal-state validation rather than accepting animation alone.

Latest controller experiment: `point-feedback-validation.md` adds bounded,
phase-activated world foot-position feedback through the same Rust/Rhai motor
path. Both timesteps improve peak swing error (about 4.15 → 3.76 mm) and supported
lift (170 → 200/210 ms), but final horizontal placement worsens to 1.03/1.25 mm
and refinement differences reach 2.18 mm. It remains an experiment, not a promoted
controller. The previous body-feedback preset remains available. Both use ideal
teacher observations; orientation control, landing accuracy, deployable sensing,
walking and learned policies remain unfinished.

The next work must address both practical execution cost and task completion:
the current complete-run profile (`point-feedback-validation.md`) locates
103.5 s in Jacobian assembly, with 85.4 s of closure mapping nested in residual/
derivative work. Investigate dependency-safe closure reuse or an explicit shared
fast-model reduction, then establish task-specific stance/landing
acceptance before extending to coordinated steps and teacher learning. Do not
keep tuning gains indefinitely or treat small endpoint improvements as walking.
The point-feedback browser milestone passes all 281 native/WASM frames, exact
replay/reset, and 17 UI checks. Browser milestones must continue to accompany
controller changes.

Latest rate experiment: `forward-rate-validation.md` retimes the same placement
to 1.5 times its duration. Both tested small timesteps pass sampled supported
lift (270 / 180 ms), but foot trajectories differ by up to 1.73 mm. A focused
CAD audit identifies false hip contact caused by sign classification on a
non-watertight compound mesh. Shared solid-by-solid export removes accepted
internal contacts at both timesteps. Its maximum foot-path effect is only
0.00612 mm, so it does not explain the 1.73 mm refinement discrepancy. Corrected
browser delivery and subsequent body/foot feedback are now available. The uncorrected experiment
remains explicitly labeled in the browser, with full 281-frame native/WASM
comparison and 14 passing UI checks. This is progress toward stepping, not
accepted walking, calibration or training throughput.

Earlier forward-placement attempt: `forward-placement-validation.md` changes the
foot reference to a net 10 mm +X placement. Geometry passes, but both physical
timesteps fail the existing 50 ms supported-lift requirement (40 ms achieved).
The opposite +Y support foot unloads. Endpoint position alone is insufficient;
weight-transfer tracking and balance are now the concrete next control problem.
The failed attempt is labeled and maintained in the browser alongside prior
presets; it is not an accepted walking controller.

Latest task measurement: `phase-tracking-validation.md` compares actual motion at
recorded plan time, including pauses. At support qualification the moving foot
has 2.02 / 2.15 mm world XY error in the two timestep runs. Support qualification
therefore remains separate from landing placement. Task-space references and an
explicit foothold/error budget are required before extending this returning
foot-lift demonstration into forward steps.

Latest controller experiment: `landing-checkpoint-validation.md` adds a
support-qualified body return through the registered Rust motion clock. The
first run pauses for 10 ms to complete its 100 ms sampled support requirement.
The viewer exposes this phase and failed runs now replay their failed attempt.
This qualifies support only; task-space landing placement and active balance
remain the next controller requirements. Refinement and browser evidence are
recorded with this experiment rather than inherited from the earlier lift.

Earlier interface: `task-observation-validation.md` records 45 additional named
body/foot teacher observations through the shared Rust/Rhai/WASM path. All sampled
physical frames remain unchanged, and browser parity/replay and the inspector
pass. These are ideal diagnostics; CAD currently declares no sensors. Landing
and balance policies must still use this information to close the task loop.

Earlier controller: `joint-feedback-validation.md` records a sampled Rhai joint
correction in the shared native/WASM motor session. It improves body-relative
tracking and supported lift in this experiment; refinement retains task results
but not identical backlash histories. The browser offers live gain input and
exact recorded-input replay. Observations remain ideal simulator state; landing,
balance, deployed sensors, learning and WASD are still required.

Earlier runtime: `embedded-session-validation.md` records the shared incremental
motor session and full-robot native/WASM trajectory comparison. The browser now
runs the fixed-reference lift live, with pause/reset/recipe replay and responsive
camera controls. Walking command inputs and learned controllers remain absent.

`lift-and-viewer-validation.md` records a sampled 1.68 mm supported foot
lift at two timesteps after CAD collision-grid corrections. Landing and walking
remain unaccepted. `web/README.md` documents the live quadruped, recorded
comparisons and live Rust/Rhai fixture. Its tested UI now accompanies controller
work. The earlier experiment descriptions below are historical evidence.

| Delivery | Current state | Evidence required to finish |
| --- | --- | --- |
| CAD physical source and reduction ledger | Versioned revision 1357 exists; masses, servo dynamics, losses, and sensing remain provisional | Source hashes, units, frames, provenance and uncertainty for every retained/effective parameter; documented derivation and validity range of every simplification |
| Task accuracy and hardware calibration | Sampled marker comparison implemented; robot measurements and task budgets not validated | Foot placement/clearance, loaded tracking, balance, stepping, recovery and timestep-refinement suites, raw held-out measurements and reproducible import/procedures |
| Fast shared Rust model | Explicit winding/rotor quasistatic prototypes and same-step isolation are implemented. Winding reduction and coarse integration fail preliminary task screens; rotor reduction has no measured speed benefit. No training model is accepted; see `fast-motor-validation.md` | Motor-to-foot kinematics and linkage coupling, effective mass/inertia, actuator limits/delay, backlash and contact/slip verified against task gates throughout operating range |
| Detailed reference | Short full-robot runs available; numerical and physical accuracy remain unresolved | Refined trajectories, original closure diagnostics, energy/contact checks, calibrated parameters and explicit uncertainty; retain useful solver improvements only with full-run timing/accuracy evidence |
| Observation/action contract | Shared runtime has named/unit-typed channels; robot export includes ideal diagnostic measurements | Versioned deployed sensor mapping, rates, delays/noise, action limits/units, motor calibration, observation histories and separate teacher-only state; identical contract across training/native/browser |
| Quadruped planner | Shared 3D marker/base path compiler and static support diagnostics implemented. Single-foot and weight-transfer motor replays complete but do not achieve an accepted step; see `marker-planning-validation.md`. Automatic foothold/timing search and balance controller remain unimplemented | Support schedule, reachable footholds, clearance and balance references constrained by actuator/mechanism feasibility; comparisons with actual simulated execution |
| Teacher RL | Not implemented | Seeded Rust training, rewards tied to planner references, checkpoint/restart, independent episode evaluation and a learned walking teacher that passes declared task gates |
| Student and robust training | Not implemented | Distillation using deployable observations, subsequent learning, bounded uncertainty/randomization and pushes, held-out terrain/parameter/disturbance evaluation |
| Viewable policies and motions | Live quadruped lift uses the shared incremental Rust motor session, with full sampled native/WASM comparison and UI checks; walking view and learned controllers unfinished | Shared Rust execution for planned and learned motions, responsive controls and rendering, replay/export; no viewer-only physics or hidden ideal actuator path |
| Performance and reproducibility | One-second knee-motion diagnostics cost 20.45 s at 1 ms or 58.88 s at 0.125 ms on the development CPU (single runs). Jacobian reuse and exact preparation help, but realtime and training throughput are not achieved | Matched-error native/browser latency and throughput benchmarks, whole episodes, rejected work included, seeds/configuration/model/software/hardware recorded, representative CI gates |

Implement in this order: task evidence and calibration import; explicit reusable
mechanism/actuator reduction; full-robot fast-model trajectory gates; deployable
environment contract and quadruped planner; teacher training; distillation and
robust training; held-out evaluation and interactive delivery. UI/replay and
benchmark support evolve alongside each stage. Begin initial learning once the
corresponding simulation task gates pass. Missing hardware evidence may permit
explicitly uncalibrated simulation experiments, never a hardware-transfer claim.

The support-load and grid investigation is in `support-load-and-grid-validation.md`.
A static load requirement now rejects the old under-supported reference. Exact
CAD inspection exposed and reproduced an incorrect nearest-triangle search in
distance-grid export; a shared exporter correction removes the inspected false
contact. A retracted 16 mm body-shift reference passes sampled geometric and
static load checks. Both physical timestep runs fail to lift the foot; a 5 mm lift candidate now
passes reference checks and awaits motor execution.

The latest feedback-execution experiment is in `support-gate-validation.md`.
A reusable registered motion clock pauses the lift when a required support foot
falls below an explicit provisional load threshold. Both timestep variants time
out without recovering enough support. This adds reproducible feedback/failure
handling, but active balance correction and successful stepping remain open.

The earlier actuator consistency check is in `actuator-consistency-validation.md`.
A missing-resistance catalog derivation had made modeled stall torque about half
the declared value. A recorded export correction improves tracking and lets the
selected foot lift, but the opposite foot still briefly unloads. The follow-up
`moving-joint-band-validation.md` identifies and corrects a shared runtime frame
bug: a joint's collision-exclusion region must move with its anchor, not remain
at the original world location. This removes a spurious newly reported knee
contact without establishing a successful step or calibrated hardware response.

The earlier foot-path and weight-transfer evidence is in `marker-planning-validation.md`.
Prescribed foot paths now compile to physical motor references, but a 3 mm isolated
lift also unloads the opposite foot. A revised body-shift reference improves static
support margin while still failing to lift the selected foot under load. These
are failed stepping attempts that identify needed tracking/balance work, not a
walking baseline. The mechanism, collision and reporting capabilities are reusable.

The earlier tracking follow-up is in `servo-timing-and-rate-validation.md`.
Shared phase-anchored servo clocks remove the slow-run deadline drift, and an
opt-in auxiliary-rate formulation completes previously failing unloaded backlash
events without changing residual tolerances. Loaded/unloaded coarse/refined runs
and a slower loaded program now complete. Sampled return lag changes from about
3.70 degrees loaded to 2.88 unloaded or 2.89 with slower transitions. Unloaded
backlash event counts still differ under refinement, so these are diagnostic
results, not a promoted timestep or learning gate.

## Provisional first experiment requirements

These are proposed engineering targets, not measured capabilities or agreed
hardware limits. Tighten or revise them with recorded rationale when the task,
geometry and measurements establish the actual margins. Do not loosen them
merely to accept an optimization.

- First task: low-speed level-ground stepping, then conservative walking;
  quantify foothold size, required clearance and reachable operating range before
  treating any placement threshold as a robot acceptance gate.
- Marker-tool demonstration: 20 mm placement margin, 8 mm reserve for errors
  outside the comparison, plus explicitly supplied measurement uncertainty.
  This arithmetic fixture is not the robot's accepted error budget.
- Interactive target on the Intel i9-9980HK development CPU: sustain at least
  1 simulated second per wall second and a 50 Hz policy period, with p95 physics
  work per 20 ms policy update below 20 ms. Report p99, worst stalls and startup
  separately. Browser and native are separate measurements; include render/worker
  responsiveness in the browser result.
- Initial native learning-throughput target on that CPU: at least 1,000 aggregate
  environment transitions/s at a 20 ms policy period across a declared batch
  size/worker count (20 aggregate simulated seconds/s). Measure inference,
  observations, resets and rejected solver work; report optimizer time separately.
  This target is not currently achieved. Determine total training cost from
  learning curves, not this throughput target alone.
- Numerical acceptance: hold controller/sensor schedules fixed while refining
  physics steps; compare task outcomes over complete episodes, not only final
  states. Contact transitions need event timing and impulse comparisons as well
  as smooth-interval force checks.

## Fidelity and calibration ledger to build

Keep source geometry and physical properties in CAD. A reduction must identify
its source parameters and preserve relevant output behavior: transmission ratios
and internal linkage motion; mass/inertial coupling; motor torque/speed limits,
loaded response and latency; effective friction/backlash/backdriving behavior;
foot contact and slip; actual sensor availability and timing. Thermal and material
detail may be reduced only with explicit duration/load/temperature limits and a
test of the resulting actuator or foot behavior. An ideal gear relation is not
evidence of worm efficiency or self-locking.

Use separate data for fitting and evaluating. Record supply voltage, payload,
temperature, fixture/frame, commanded and measured motor motion, timestamps and
sensor processing. Provide procedures for missing masses/COM, inertia, motor
response, backlash and friction/contact measurements. Unknown bounds remain
unknown; arbitrary randomization is not hardware calibration.

Every progress report should explain the subsystem, test and its purpose,
observed result, limits of the evidence, and the next decision it enables.

## Conservative initial operating envelope

The user's proposed bounded motion domain is appropriate for initial standing
and low-speed stepping. Derive it from the CAD mechanisms and task, rather than
turning every solver failure into a physical motor limit. A useful envelope
must cover coupled motor configurations, reachable foot points, enclosure/rod
clearance, original linkage closure and conditioning, and achievable actuator
loads/speeds. Independent per-joint angle clamps alone do not establish these.
Record a margin inside true toggles and collisions and check planned trajectories
throughout each segment, including velocity and tracking-error allowance.

The current embedding checks dependent-coordinate rank using SVD in ordinary
poses too; SVD is not itself a failure mode. A true toggle rejection is covered
by a regression. The earlier SVD/QR free-fall discrepancy and rotated-linkage
checks establish that some failures are numerical, not physical travel limits.
Contact changes and backlash engagement occur during useful interior motion and
must remain modeled inside the envelope. A bounded operating region complements
correct event handling; it does not replace it. No complete robot envelope or
walking feasibility gate has been certified yet.

## Maintained browser viewer and interactor — continuous acceptance

The user requires the WASM viewer to stay polished and useful throughout the
controller journey. It is a deliverable at every controller milestone, not a
final integration task. A headless-only controller result is an experiment;
it is not a delivered controller milestone.

- Run the current CAD-derived robot, Rust physics and controller configuration
  through the shared native/browser runtime and observation/action contract.
  Expose labeled controller and demo presets, including current experimental
  motions with honest readiness and calibration labels.
- Keep loading, progress, errors, cancellation and recovery understandable.
  Camera orbit/pan/zoom/fit, pause/reset/replay, selection and mode affordances
  must remain responsive while physics runs in a worker.
- Route WASD and other user motion requests through the controller. Disable
  unsupported actions with a clear explanation; never teleport the robot or
  silently bypass actuator behavior to make a demo appear interactive.
- Show simulation time, simulation/wall-time ratio, controller state, requested
  versus actual movement, and useful contact/force overlays. Label replay,
  prescribed references and live physical execution distinctly. Playback is
  useful evidence but does not satisfy live-controller support by itself.
- Preserve model/controller/environment versions, seeds and input streams for
  reproducible replay. Keep an easily shareable WASM build with straightforward
  startup instructions and an explicit latest usable milestone.
- At each controller milestone, verify headless/native/browser behavior at
  matched inputs and check browser startup, preset switching, interaction,
  pause/reset/replay, errors and responsiveness. Maintain automated smoke/parity
  checks and inspect the actual user experience. A passing WASM compilation
  check alone is insufficient.

Current gap: the browser delivers the current physical lift and placement
experiments, but accepted walking, controller-routed WASD motion and learned
policy presets remain required. Maintain the passing browser experience while
adding the fast training model, task acceptance and policy learning.
