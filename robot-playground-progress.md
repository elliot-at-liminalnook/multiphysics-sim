# Robot playground delivery

Active objective: complete the CAD-derived fast/detailed model → quadruped
footstep guidance → teacher RL → student distillation → robust training and
viewable policy workflow, following AGENTS.md. The user activated the broader
learning goal on 2026-09-06. The full delivery and acceptance map is in
`examples/full-robot/learning-plan.md`. Prior solver work remains part of that
goal; it is not the whole objective. Unchecked items are not complete.

## Continuous viewer requirement

Every controller milestone must remain usable in the shared WASM viewer:
labeled presets, responsive controls, controller-routed WASD, pause/reset/replay,
clear feedback and inspectable targets/actual motion/contacts/performance, with
reproducible shareable builds and native/browser checks. The detailed acceptance
list is in `examples/full-robot/learning-plan.md`. The current lift experiment is
now runnable live in the browser; task acceptance and the subsequent walking
controllers still require their own maintained browser milestones.

## Task-focused validation and learning

### Current experiment: 10 mm forward placement, support failure preserved

The foot now has a reference that ends 10 mm forward rather than returning to
its start. Both motor-driven 2 s runs finish, but only sustain qualifying
supported swing for 40 ms against the unchanged 50 ms requirement. The +Y
support foot repeatedly unloads; at the planned peak the body undershoots its
sideways shift by about 2.53 mm. Both runs remain failed stepping candidates.
See `examples/full-robot/forward-placement-validation.md` for exact observations,
reproduction and the maintained browser preset. The next controller investigation
is weight transfer, separating motion-rate effects from persistent tracking error.

### Current measurement: plan-time landing error

Shared Rust motion tracking now follows the controller's recorded reference
time through pauses and reports world and body-relative marker errors separately.
At the support checkpoint, the moving foot is 2.016 / 2.153 mm away in world XY
for the two tested timesteps. This exposes a gap between sustained support and
accurate landing. The synthetic drift/lag tests and existing tracking tests pass;
the viewer/controller are unchanged. See `examples/full-robot/phase-tracking-validation.md`.

### Current: support-qualified body return and failed-run replay

The gain-0.5 lift now has an explicit 100 ms, four-foot support checkpoint before
body return, implemented through the existing registered Rust motion clock.
The first complete 2 s run waits from 1.100 to 1.110 s and finishes its reference
at 1.610 s, with no accepted internal contacts. It takes approximately 118 s on
the development CPU; realtime and training throughput remain unmet.

The WASM preset exposes reference progress, support dwell and timeout. Recording
version 3 includes failed attempts: native regression tests verify identical
timeout, first-step policy error and horizon failure replay, while rejecting a
different expected failure. Browser UI tests pass, including visible missing-
support timeout, recording/replay, reset and earlier controller presets. Full
native/WASM trajectory and timestep-refinement evidence belongs to
`examples/full-robot/landing-checkpoint-validation.md` and its status artifact.

This is a support-qualified transition, not an accepted landing, balance or
walking controller. Task-space landing correction, deployable sensing, learning,
WASD locomotion and throughput remain in the full goal.

### Earlier: shared body/foot observations for task-space policies

The opt-in `TaskObserver` adds 45 named ideal-state channels to the sampled Rhai
contract: body-frame gravity direction and velocity, relative foot-marker
position/velocity, and link floor-force resultants. Independent derivative/frame
and force-summation tests pass. A Rhai fixture uses a marker observation to change
its target and replays exactly, including through WASM. The current CAD export
has no declared sensors; these channels remain explicitly nondeployable.

All 161 sampled physical frames match the previous gain-0.5 robot run exactly
when observations are enabled. Full native/WASM comparison passes, including
the new readings (max entry difference 4.565e-9 N); browser replay/reset are exact.
The viewer offers a labeled body/foot preset and timestamped collapsible readings.
See `examples/full-robot/task-observation-validation.md` and its status JSON.

This adds the information needed for landing/balance policies, not those policies
themselves. The robot still uses joint feedback. Next: use explicit foothold and
support references in a task-space controller, qualify touchdown and safe phase
transitions, and map actual sensor hardware into CAD and the observation contract.
Training throughput, teacher/student learning and WASD walking remain unfinished.

### Previous: sampled Rhai feedback in native and WASM

The shared motor session now runs named, bounded Rhai joint policies and records
external command changes in simulation time. Browser replay restores both the
physical state and command controls. Six session tests and three comparison tests
pass; the small policy fixture is included in browser CI.

The full-robot outer tracking experiment improves worst body-relative foot error
from 4.770 to 3.142 mm and peak clearance from 1.680 to 2.685 mm. Half-timestep
execution preserves the lift outcome, but world foot trajectories differ by up
to 0.344 mm and some backlash events differ. All 161 browser frames pass native
comparison (max entry difference 3.208e-9), with exact browser replay. The complete
browser run takes 106.85 wall seconds for 1.6 simulated seconds. Real UI checks
cover live gain changes and plan/target/actual readings. See
`examples/full-robot/joint-feedback-validation.md` and its status JSON.

This is an ideal-state joint correction, not landing/balance control or deployed
sensing. Next: task-space landing/balance feedback, hardware-compatible state
observations, and throughput sufficient for initial learning. Teacher/student
training, WASD locomotion, calibration and held-out walking remain unfinished.

### Previous: live quadruped through the shared incremental motor runtime

`EmbeddedSession` now owns the existing mechanism/motor orchestration in the
shared Rust library; `integrate_embedding` is its thin headless host. Four
incremental-session tests pass, including uneven chunks, exact replay, observation
without mutation, invalid requests and latched feedback timeout. The native
quadruped extraction matches all 36 original report fields checked except wall
time. The full 1.6 s browser run compares 161 frames against native, with maximum
entry difference 3.432e-8 (a contact-force component in N), and exact browser
physical replay/reset. It takes 97.16 wall seconds, so realtime is still open.

The viewer now offers **Quadruped · live lift experiment** through a worker,
alongside recorded comparisons and the live Rhai fixture. Rendered UI checks
cover advancing, camera fit, pause, saved-recipe replay and reset. The browser
replay verifies the loaded scene/controller recipe and reports progress with
cancellation. This fixed-reference Rust servo experiment does not yet expose
walking commands or a learned policy. Read `examples/interactive/embedded-session.md`
and `examples/full-robot/embedded-session-validation.md` for scope and evidence.

Next: close the remaining task/control gaps (landing, slip, loaded tracking and
balance), provide the deployed observation/action contract, and improve the
fast-model throughput enough for initial teacher learning. Keep each new
controller runnable through this same native/browser session path. Calibration,
teacher/student training, WASD locomotion and held-out walking remain required.

### Previous: supported lift and usable browser workspace

The 5 mm request with corrected thigh and hip distance grids produces 1.682 mm
peak foot clearance. At both 0.25/0.125 ms physics steps, the sampled lift conditions
hold for 120 ms and no accepted-step internal contacts are reported. Sampled world
foot differences reach 0.03112 mm; backlash event timing remains different.
Runs take 92.46/128.62 wall seconds for 1.6 simulated seconds. Landing, slip,
calibration and walking are still unaccepted. See
`examples/full-robot/lift-and-viewer-validation.md` and its status JSON.

`web/viewer` now renders recorded robot trials with selection/fit, playback,
scrubbing, contact arrows and target/actual readings. A separately labeled live
Rust/Rhai pendulum supports target adjustment, reset and saved-input replay.
Real-browser desktop/mobile, failure-recovery and cancellation checks pass; the
fixture UI check is wired into browser CI. Build/share instructions are in
`web/README.md`. Full-robot live WASM still requires an incremental shared runtime
interface for the newer embedded motor runner; recorded playback does not finish
that requirement. Earlier progress entries below retain their historical scope.

### Previous: support-load feasibility and collision-grid correction

The static tripod load check shows that the old 5 mm body-shift reference asks
only about 0.50 N of the +Y support at lift preparation, below the execution
clock's 1 N threshold. A new shared Rust calculation checks minimum vertical
loads and computes a nearest COM target under an explicitly static three-point
support assumption. It does not certify actuator, contact or dynamic feasibility.

Larger local placement searches encountered bounds and a reported crosshead /
thigh-assembly collision. Exact captured-pose CAD inspection finds the reported
point outside all target solids. An analytic beam fixture reproduces a CAD
export bug: eight nearest triangle centroids can exclude a nearby long face.
The shared exporter now uses triangle AABB candidates and exact mesh distances.
See `examples/full-robot/support-load-and-grid-validation.md` for tests, raw
reproduction commands and the current robot experiment outcome. No new walking
or learning gate is promoted. A 16 mm reference passes sampled geometry/static
load checks, but both physical timestep runs keep the selected foot on the floor.
The next saved candidate requests 5 mm lift and awaits motor execution. Refining
the 3 mm trial changes sampled world foot positions by at most 0.0312 mm, so its
failed lift is reproducible at both tested resolutions.

### Previous: support-gated reference execution

The shared registered `control.motion_clock` can pause reference progress while
physics and servo firmware keep running. It requires sustained ready observations
before resuming and latches a timeout. The embedded diagnostic commits pending
clock updates with successful physics steps and records physical/reference time,
support forces and checkpoint state. Gate parameters have declared time units.

The first robot recipe explicitly uses privileged simulated floor forces, not
deployed force sensors. During the lift window each of the other three feet must
carry at least a provisional 1 N; recovery must last 50 ms and a 300 ms pause
times out. Both 0.25/0.125 ms replays pause at reference/physics time 0.69 s and
time out at physics time 0.99 s. Opposite-foot load settles to 0.405/0.406 N;
there are no internal contacts. These are deliberately retained failed episodes,
not solver failures or stepping successes. Pausing does not redistribute enough
load; next implement/test active stance and weight-transfer correction.

Seven control tests, three support tests, three comparison tests and WASM
library compilation pass. See `examples/full-robot/support-gate-validation.md`
and its status JSON. Hardware calibration, sensor mapping, accepted stepping,
teacher/student learning, interactive policy delivery and realtime remain open.

### Previous: joint contact-frame correction, validated against CAD

The -Y knee contact is traced to a shared runtime frame error: joint exclusion
regions stayed at their original world positions. Exact CAD inspection follows
the reported sample through captured poses; its tiny existing overlap stays
constant. At 0.87 s the sample is 12.0 mm from the stale center but only 6.4 mm
from the current center, within the existing 10 mm joint region.

The library now caches anchor-local joint metadata and transforms the center
with the current anchor pose. Translation/rotation tests cover ordinary and loop
joints, retaining boundary and outer contacts and safe geometry reuse. Two CAD
tests, two Rust exclusion tests in native/no-default configurations, 34 motor/
step/Jacobian checks and a WASM compile pass; the preexisting experimental SDF
derivative promotion test remains ignored.

Both 1.6 s robot replays complete at 0.25/0.125 ms with zero internal contacts.
The fix changes sampled foot motion by under 0.002 mm from the old coarse run.
Selected-foot peak clearance remains about 1.05 mm; the opposite foot still
briefly unloads and tracking error remains about 2.48 mm. Refinement differs
by up to 0.040 mm and backlash-event counts remain unequal. No successful step,
hardware accuracy or realtime result is claimed. See
`examples/full-robot/moving-joint-band-validation.md` and its status JSON.
Next improve coordinated weight transfer and stance support; teacher/student
learning, the deployed sensor contract and interactive learned policy remain open.

### Previous: motor consistency and loaded-motion replay

The CAD catalog derived torque constant from declared 3 A stall current but
left a resistance estimate implying only 1.46 A. New catalog motors now derive
missing resistance consistently. The shared Rust registered-equation stall audit
finds 1.423 N m originally versus 2.903 N m after correction, compared with the
declared 2.942 N m. Existing CAD remains unchanged; a hash-checked ledger records
the experimental exported resistance/inductance correction for all twelve motors.

Replaying the same coordinated 1.6 s reference now lifts the selected foot
1.054 mm at peak (1.056 mm with half-sized timesteps). Worst body-relative foot
tracking improves from 2.905 to 2.483 mm. The opposite foot still briefly unloads,
and both corrected runs report about 4.5 micrometres maximum internal overlap
between the curved knee link and crosshead. This is not a successful step.
Refinement changes world-foot position by up to 0.040 mm; contact/event differences
remain. The run still costs far more than realtime.

CAD regression tests, independent analytic motor/driver checks and a WASM library
compile pass. See `examples/full-robot/actuator-consistency-validation.md` and
`actuator-consistency-status.json`. Next inspect the loaded internal contact and
improve stance support/weight transfer. Calibration, sensor contract, automatic
step planning, teacher/student learning and interactive learned policies remain
unfinished; no learning gate was promoted.

### Previous: compiled foot paths and motor-driven weight transfer

Shared Rust inverse kinematics now compiles full 3D foot-marker paths and optional
body translation into the existing sampled motor trajectory format. It retains
original linkage closure, authored limits and explicit search bounds, rejects
sampled internal collisions, and checks interpolation at command midpoints.
Compiled poses are geometric references; physical playback applies motor forces.

A 1 mm foot-lift request remained loaded. A 3 mm request lifted the selected and
opposite feet together, leaving the lateral pair supporting the robot at peak.
Refining 0.25 to 0.125 ms retained that behavior: selected/opposite surface gaps
were about 0.425/0.574 mm, while the assumed three-foot COM margin was -1.16 mm.
The original stance has only 0.098 mm initial static margin for that support set.

A 5 mm body-weight-transfer reference hit the provisional foot extension bound.
A more retracted startup compiled successfully and completed a 1.6 s motor-driven
run: actual chassis shift 3.50 mm, versus 5 mm requested; static support margin
+2.76 mm at peak. All four feet remained touching in sampled lift-window frames,
so this still does not establish a successful step. Body-relative marker tracking
error reaches 2.91 mm. No accepted internal contacts or solver failures occurred.

Analytic geometry, interpolation/collision rejection, translated-base compensation,
surface clearance and static support tests pass. The library and CLI work is
recorded in `examples/full-robot/marker-planning-validation.md` and
`marker-planning-status.json`. Next address loaded
actuator tracking in the coordinated reference, then establish stepping acceptance
before training. Automatic quadruped footstep search, teacher/student learning,
hardware calibration and interactive policy delivery remain unfinished.

### Previous: commanded knee motion and measured tracking

The lag diagnostics now complete. Shared servo deadlines are anchored to their
declared clock phase, removing accumulated drift and a tiny leftover-step failure.
An opt-in auxiliary-rate solve avoids subtractive cancellation during backlash
event location while retaining original equations/tolerances. Five robot replays
complete: loaded/unloaded at 0.25/0.125 ms and a slower loaded 1.4 s program.
The -Y return gap is about 3.70 degrees loaded, 2.88 with gravity/contact removed,
and 2.89 with transitions twice as long. Modeled response and loading both matter;
this is not hardware calibration. Loaded refinement still differs by 0.226 mm
in world-foot position; unloaded backlash event counts also differ. The rate
option remains experimental. See `servo-timing-and-rate-validation.md` and
`servo-timing-and-rate-status.json` for source/capture hashes and validation.

A shared validated trajectory sampler now supplies linear or quintic rest-to-rest
references. Both the detailed-runtime trajectory adapter and reduced servo path
use it. `connect_target_law` leaves registered firmware sampling, delay, PID,
quantization and saturation unchanged. The integration diagnostic requires named
motor order, matching CAD source/initial targets, and authored independent
reference bounds. Default fixed-target captures remain exactly unchanged.

A four-knee 0 → -0.2 rad → 0 program gives about 2.6 mm ideal retraction.
The 201-pose audit retains closure/rank/inertia and authored limits with no
reported internal contact. A positive candidate exceeded +X's +4.5-degree
limit; a -0.4 rad candidate reported -Y crosshead/sector-gear contact near
-0.273 rad. Physical interference versus proxy geometry remains unresolved.
Only +X currently has authored foot-motor limits; other limits are not inferred.

Loaded one-second runs complete at 0.125/0.25/1 ms, all with 1,000 servo ticks
and no accepted internal contacts. Shared body-relative marker reporting finds
maximum tracking errors of 1.805/1.766/1.707 mm respectively, substantial next
to the 2.6 mm command. At 0.69 s the finest -Y foot target is -0.1186 rad while
actual motion is still -0.1833 rad. Lag and load-dependent offsets persist with
step refinement; this is a plant/controller behavior, not fixed by Newton
accuracy alone. Body lowering is about 3.1 mm with sampled tilt below 0.030°.

Single native timings are 58.88/41.76/20.45 s for one simulated second; no
matched-error speedup, realtime or learning-readiness claim. World-foot/duty
refinement differences remain approximately 0.228 mm/0.091 at 0.25 ms and
0.497 mm/0.157 at 1 ms versus the finest run. All samples are 10 ms apart.

Validation: 4 trajectory tests, 10 embedded-motor tests native and without
default features, 19 runtime/session, 5 tracking, 3 posture tests and WASM
compilation pass. Current CLI accepts the valid named/bounded program and
rejects the positive out-of-bounds input before stepping. Source snapshots,
recipes and results are in `examples/full-robot/knee-motion-status.json` and
`knee-motion-validation.md`. Next separate dynamic lag from load offsets,
resolve the collision-model question before extending the motion envelope,
and connect feasible references to the environment/planning/learning path.
The complete goal remains active; planner and learned walking are unfinished.

### Previous: one-second hold and timestep-cost comparison

The same loaded startup now completes one second at 0.125, 0.25, 0.5 and 1 ms
nominal timesteps, with all 12 servos ticking exactly 1,000 times and no accepted
internal part contacts. At 0.25/0.125 ms the body drops 0.327/0.336 mm, stays
within 0.0231/0.0202 degrees of upright at the sampled frames and finishes with
about 39.007 N upward support. Feet move by up to 0.947/0.973 mm from their
initial positions; near-upright posture is not a no-slip or balance-policy claim.

Single-run times are 45.02, 30.47, 25.40 and 14.23 s for 0.125, 0.25, 0.5 and
1 ms. These exploratory timings are not matched-error speedups. Relative to
0.125 ms, the 0.25/0.5/1 ms maximum sampled foot differences are 0.228/0.447/
0.497 mm and duty differences 0.091/0.159/0.157. Largest 10 ms contact impulse
window differences are about 0.035 N s. Samples are every 10 ms here, versus
2 ms in the earlier short comparison. No timestep is promoted from these
marker-only results; loaded tracking/contact accuracy remains unresolved.

The new shared Rust `posture::summarize_hold` and CLI report tilt, body drift
and marker displacement with declared axes and sample coverage, an optional
CAD source guard, and no silently chosen physical acceptance limits. Three
synthetic/invalid-evidence tests and WASM compilation pass; CI includes them.
See `examples/full-robot/hold-validation.md` and `hold-1s-status.json` for
recipes, results, limitations and source hashes. Next: use these metrics on
commanded motions within a derived coupled operating envelope, then connect
validated reference motions to the shared environment/planning/learning path.
The full objective remains active and incomplete.

### Previous: guarded Jacobian reuse across accepted steps

The shared solver now offers a numerical-Jacobian cache with cheaply cloned,
immutable factor storage. The reduced motor/servo adapter carries an opt-in
workspace transactionally across accepted intervals. Discrete jumps, changed
contact-pair lists, changed timesteps/noncontiguous time and bounded factor age
clear reuse. Actual mechanics, contact and component residuals remain fresh;
stale-matrix convergence checks still rebuild. Rejected trials and failed whole
intervals do not change the caller's workspace. Default behavior remains fresh.

The full-robot 100 ms runs complete at 0.25/0.125 ms with the same accepted
segment counts and contact identities as their respective reference, the same
event counts, foot differences below 2.84e-14 m, and accepted-force differences
below 1.31e-8 N. The fresh numerical-helper path reproduces the previous capture
exactly. Three alternating same-executable timing pairs give 8.57/6.90,
9.03/6.39 and 8.94/8.12 s (off/on), median paired improvement 1.24x. Desktop
runtime varies; this remains far from realtime. Separate profiled captures are
8.42/5.67 s and preserve the unprofiled trajectories.

Coarse Jacobian builds fall 495 → 326; successful-trial endpoint evaluations
36,718 → 25,904, while Newton iterations rise 3,144 → 3,483. Accepted segments
remain 406. The updated on-profile spends 60.1% in outer Jacobian construction;
closure mapping 46.3%, contact-history condensation 22.6% and dynamics
preparation 23.7% overlap that enclosing cost. The unchanged timestep comparison
still differs by 0.235 mm at the feet and 0.091 duty. Speed is improved without
repairing that accuracy limitation.

Validation: 38 robot tests native and without default features, 2 solver unit
and 11 convergence tests, forced-sparse cache regression, and WASM runtime
compilation pass. CI includes the new shared regressions. Recipes, source/binary
snapshot and comparisons are recorded in `examples/full-robot/jacobian-reuse-status.json`.
Next: extend the loaded trajectory beyond 100 ms and diagnose task-level
stability/accuracy before calling this a walking/training backend. The full
planner/teacher/student/robust-learning/viewable-policy objective remains active.

### Previous: shared direct closure derivatives

The rigid inertia kernel's motion columns now also assemble the Jacobian of
every original linkage/axis/transmission closure equation. The opt-in
`embedding.direct_closure_jacobian` replaces repeated whole-mechanism velocity
probes with one shared geometry/motion-map pass. All rows, SVD/QR rank checks,
scales, tolerances and physical equations remain. The reference path remains
the default and available for independent comparison.

Spatial mixed-joint/multiple-base cases, signed gear ratios, planar closure and
toggle rejection pass. A 41-pose full-robot sweep around the retracted startup
has maximum scaled derivative disagreement 1.67e-15 against original velocity
probes. Both 100 ms simulations complete at 0.25/0.125 ms. Differences from the
reference are at most 5.02e-14 m at sampled foot markers, 6.04e-11 duty, 1.30e-8 N
in accepted contact forces and 3.27e-11 s in event timing. Event counts and
accepted-stage contact identities match. These are numerical comparisons on the
declared cases, not hardware accuracy or a global motion certificate.

Three same-executable unprofiled paired runs take 19.63/7.41, 20.06/7.58 and
19.98/7.43 s per 100 ms (reference/direct): median paired speedup 2.65x. Repeats
are deterministic within each method. Timers leave trajectories unchanged.
The profiled reference spends 13.32 s constructing closure Jacobians and 0.713 s
in closure SVD. Direct construction falls to 1.046 s while SVD stays at 0.720 s.
Total profiled time falls from 19.85 to 7.50 s. SVD is 9.6% of the faster run;
even eliminating its cost entirely would yield only about 1.11x overall.

The current larger cost remains the 495 numerical Jacobian builds probing 66
unknowns (70.8% of wall time). Mechanism mapping is now 45.8%, contact-history
condensation 23.0%, and dynamics preparation 23.4%; these phases overlap the
enclosing Jacobian timing. Next: reduce repeated complete residual/Jacobian
work and share contact preparation where all dependencies are unchanged.

The coarse/refined comparison still differs by 0.235 mm at the feet and 0.091
duty: this optimization preserves the existing loaded-control/contact limitation.
Realtime, calibrated task acceptance, planning, teacher/student learning and
viewable walking remain incomplete. Validation: 37 robot tests native and
without default features, 3 comparator tests and WASM runtime compilation pass;
CI now also includes the no-default constraint audit. Recipes, source/binary
snapshots and measured evidence are in `examples/full-robot/direct-closure-status.json`
and `embedded-integration.md`. The full goal remains active.

### Previous: exact dynamics reuse and updated cost profile

The shared mechanism library now exposes immutable prepared dynamics for one
exact mechanical state. It retains the original inertia, passive/contact loads,
projection arithmetic and residual check; each component trial still supplies
fresh motor loads and computes a new acceleration. The opt-in
`reuse_mechanical_dynamics` extends exact endpoint reuse within one continuous
solve, with no sharing across changed motion, histories, steps or events.
Independent force/mass and loaded motor-circuit tests pass. Event/rollback and
contact checks remain in the shared suites.

Full-robot 100 ms captures at 0.25/0.125 ms reproduce the original states, contact
traces, accepted steps, event schedules and original solver diagnostics exactly.
In the coarse capture, dynamics preparations within successful continuous trials
fall from 36,718 to 13,019, with 23,699 reuse hits; residual evaluations and Newton
iterations are unchanged. Three sequential matched unprofiled run pairs give
speedups of about 1.13–1.37x, median paired 1.16x. Individual off/on wall times
are 20.04/17.73, 24.28/17.66 and 25.30/21.90 s per 100 ms simulated. Desktop timing
varies; this is a modest gain, not realtime. Reuse remains explicitly opt-in.

Optional phase timers now identify the remaining cost. In the profiled reuse-on
capture (17.51 s total), closure mapping consumes 13.88 s (79.3%), contact-history
condensation 1.50 s (8.6%), dynamics preparation 1.60 s (9.2%), and the applied
force solve 0.11 s (0.6%). Across the enclosing solver, numerical Jacobian
assembly takes 12.43 s (71.0%): 495 builds probing 66 unknowns. These percentages
overlap and must not be added. Outer Newton factorization is only 0.022 s
(0.12%). SVD's contribution within closure mapping has not been isolated.

This shifts the next investigation toward repeated linkage mapping and derivative
construction, rather than changing the main linear solver. The placement/contact
and loaded-control accuracy limitations below are reproduced, not repaired.
Validation: 28 robot tests native and without default features, 10 solver
convergence tests, 3 comparator tests, and WASM runtime compilation pass. Timed
and untimed trajectories also match exactly. CI already runs these suites;
source/binary snapshots and recipe hashes are recorded in
`examples/full-robot/prepared-dynamics-status.json` and `embedded-integration.md`.
The full planning/teacher/student/robust-learning and viewable walking objective
remains active and incomplete.

### Previous: bounded support placement and a cleaner loaded startup

The shared mechanism library now provides point Jacobians and bounded local
point-to-plane placement. Derivatives are exact rigid unit-velocity kinematics
composed with the existing closure tangent; independent analytic geometry and
position perturbations check them. Placement preserves the base pose and every
original closure equation, rejects authored dependent-joint limits, and never
mutates the input on failure. These are geometric tools, not collision or static
equilibrium certificates.

An initial +/-0.25 rad search could not level the feet. Allowing +/-0.4 rad at
the worm motors geometrically succeeds, but reveals a 0.6403 mm reported internal
interference between the +Y worm/input spindle and hip output shaft/pulley,
producing about 128 N. Whether this is real interference or a collision-model
artifact remains unresolved. That pose is not the clean support reference.

Instead, retracting mainly the longer +X foot (-15.821 degrees at its worm motor)
and translating the initial floating body down 8 mm puts all four feet about
20 micrometres above the unchanged floor with no initial internal contacts.
The complete exported collision vertices agree with the runtime support samples.
No CAD geometry, mass, material, transmission or stop was changed. The assigned
+X foot mass/asymmetry and all proposed operating bounds remain uncalibrated.

The integration diagnostic now records explicit initial motor coordinates/base
translation and initializes the original motor component's gear angle at the
joint position, without current, speed or elastic preload. Both 100 ms servo
runs (0.25/0.125 ms steps, unchanged 1 kHz control clocks) complete: 406/807
accepted segments, 19.905/34.773 s wall time. Neither accepted contact trace has
internal contacts. Final upward floor forces are 39.002/39.038 N for the
provisional 3.976 kg model. This is an initially unloaded release/hold test, not
proof of settled stance or sustained walking.

Halving the step changes sampled foot positions by up to 0.2352 mm, duty by
0.090998, current by 0.09896 A and shaft torque by 0.08153 N m. The largest
2 ms floor-impulse difference is 0.01825 N s at -X over 16–18 ms. These are
cleaner results than the uneven initial landing, but still show material
actuator/contact sensitivity and remain hundreds of times slower than realtime.
Changing the starting pose is not a matched-physics solver speedup.

Validation: embedding tests now total 11 native and 11 without default features;
comparator tests (3), placement/integration example compilation and WASM runtime
compilation pass. Source/binaries are snapshotted. Recipes and compact evidence
are `examples/full-robot/support-placement-status.json` and
`examples/full-robot/embedded-integration.md`. Next: use this reproducible startup
for loaded tracking and contact diagnostics, establish the useful motion range,
and address the repeated mechanical work/actuator reduction needed for training
throughput. Planner, teacher/student learning and a viewable walking policy
remain incomplete; the full goal remains active.

### Previous: explicit regularized friction experiment

The shared multibody library now supplies scalar/vector regularized Coulomb
friction. The robot's opt-in `floor_friction` model uses the CAD material's
kinetic coefficient and a combined sliding/twisting patch capacity. It opposes
slip, dissipates instantaneous work, and goes to zero with normal load. A
single contact point has no independent twist resistance. It has no exact
static stiction or stored bristle energy and allows small creep. The original
bristle model remains the default; no CAD material, geometry or actuator value
was changed. Registry parameters and runtime scene serialization select the
same implementation, including in replay and numerical-reference comparisons.

Simple-body tests establish force/twist bounds, unloading, independence from
stale memory, predicted subcapacity creep, monotone kinetic-energy decay and
stopping-distance refinement. Both full-robot 100 ms runs complete at
0.25/0.125 ms, with all twelve servo clocks ticking 100 times. Floor force ratios
stay <=0.25, as intended. However, sampled foot positions still differ by up to
0.856 mm and held servo duty by 1.062785 (-X worm at 66 ms). The largest 2 ms
floor-impulse difference is 0.192263 N s over 66–68 ms. Timings are
73.502/100.985 s. This fixes the experimental force-capacity behavior but does
not establish converged loaded control, speed improvement, or training readiness.

Rest-pose geometry also shows an important starting-condition issue: +X foot
clearance is 0.0181 mm while the other feet are about 8.018 mm above the floor.
The +X foot's provisional mass is 221 g versus 77 g for the others. These zero-
target runs begin with an uneven landing, not a settled four-foot stance. The
user has been asked whether that hardware asymmetry is intentional; source CAD
is preserved. Next: derive/document a feasible initial stance independently of
rendering, then distinguish normal-impact, engagement and estimated servo
response effects in a controlled loaded test. Exact mechanical dynamics
preparation is also repeated for electrical-only perturbations and is a concrete
remaining reuse candidate; it has not yet been changed or benchmarked.

Validation: shared kernel tests (2), robot friction tests (5 native and 5 without
default features), existing articulated/embedding/step/motor tests (33), session
tests (18 existing plus 1 new replay/numerical comparison), and WASM compilation
pass. CI includes the new tests. Recipe and evidence are
`examples/full-robot/regularized-floor-experiment.json`, `regularized-floor-status.json`
and `embedded-integration.md`. Full walking/planning/teacher/student learning
and viewable policies remain incomplete; the full goal stays active.

### Previous: contact sensitivity and duplicate linkage work

Matched +0.02 rad +X worm-target runs at 0.25/0.125 ms show maximum held-duty
differences of 0.547612 with contact enabled and 0.0182297 with only contact
disabled. Maximum sampled foot-position differences are 0.797 mm versus
0.0618 mm. The no-contact case still includes gravity, all links, servo/driver/
motor equations and backlash, but it is a diagnostic free-fall case, not a
walking model. This implicates contact-coupled response without separating
normal impact, friction and engagement effects. Contact-on runs cost
68.553/82.459 s per 100 ms; contact-off runs cost 11.267/21.060 s.

The original hold trace has very large unloading friction transients. In the
finer run, -X produces 56.475 N tangential force with 0.989779 N normal load at
37.5 ms, a ratio of 57.059. Its declared material coefficients are 0.3 static
and 0.25 kinetic; the existing bristle law does not enforce these as transient
force bounds. The comparison tool now reports patch-resultant floor force
ratios on accepted stages with an explicit 0.1 N reporting cutoff. This is a
diagnostic, not a fitted coefficient or a new acceptance gate. Next physics
work should test unloading/friction behavior and a load-bounded alternative
against independent sliding, support, dissipation and loaded-servo cases.

The shared linkage map now factorizes its final closure Jacobian once for both
velocity mapping and acceleration curvature. Rank checks remain unchanged;
there is no reuse between poses. A full hold capture reproduces prior frames,
terminal state, accepted contact trace, events and solver counts exactly. Timing
changes only from 74.675 to 73.527 s in single captures, so this small saving
does not solve realtime performance.

Separately, floor penetration telemetry now reports geometric overlap rather
than force divided by stiffness, which incorrectly included normal-velocity
damping. A fixed-depth regression fails before the fix and passes after it.
The contact force law is unchanged. Earlier captures retain legacy penetration
telemetry and must not be used as corrected geometric-clearance evidence.

Validation: 24 embedding/step/motor tests native and without default features,
9 articulated tests, 4 contact-audit tests, 3 comparison tests and WASM runtime
compilation pass. See `examples/full-robot/servo-contact-diagnosis-status.json`
and `embedded-integration.md` for hashes, limits and reproduction. The full
walking/planning/learning objective remains active and incomplete.

### Previous: sampled servo-to-driver-to-motor feedback

The experimental reduced path now accepts CAD-derived joint-angle targets
through the registered sampled servo firmware, averaged H-bridge and motor
components. It preserves held commands, quantization, saturation, delay queues
and clock events. The same firmware parameter derivation is used by the detailed
adapter. A failed interval discards all controller and motor state changes.
An independently assembled clocked controller/circuit/load reference passes,
alongside exact firmware-state and rollback tests. A reproduced clock-roundoff
failure is fixed at ulp-scale boundaries, without loosening physical event
location tolerance.

Three full-robot 100 ms runs complete: zero-angle hold at 0.25/0.125 ms physics
steps, and a +0.02 rad +X worm-motor target at 0.25 ms. Each processes exactly
100 ticks for every one of twelve CAD-configured 1 kHz controllers. Timings are
74.675/89.308/68.553 s respectively; these are single diagnostic captures, not
realtime or matched-physics speedup claims. All runs impose 11.1 V and 293.15 K.

Halving the physics step changes sampled foot positions by up to 1.146 mm and
held servo duty by 0.949 despite identical servo clock times. At 48 ms the -X
worm commands are -0.051 and -1.0. The largest 2 ms floor-impulse difference is
0.169579 N s at -X over 48–50 ms. These are material actuator/contact differences;
the reduced closed-loop model has not passed loaded tracking or learning gates.
The changed target alters the +X worm path by up to 0.0164312 rad, but finishes
at -0.00987333 rad against a +0.02 rad target. This establishes response to the
command, not settled tracking or walking.

Native and no-default motor tests (9 each), runtime session tests (18), shared
hybrid/root/schedule tests (10), comparator tests (3), and WASM compilation pass.
Recipes are in `examples/full-robot/embedded-integration.md`; compact hashes and
diagnostics are in `servo-integration-status.json`. Next: isolate contact and
engagement sensitivity under feedback, establish a conservative mechanism range,
and validate task-relevant reductions before using the model for stepping data.
Battery/thermal coupling, deployed sensors, calibration, planner, teacher/student
training and viewable walking remain incomplete. The full goal stays active.

### Previous: shared driver-to-motor feedback

The reduced motor path now accepts pure, trial-state-dependent electrical
boundaries. EmbeddedDriverBank reuses the registered H-bridge law, eliminating
its algebraic current through KCL while retaining voltage drop/current foldback
inside the same mechanical/motor Newton solve. CAD driver parameter derivation
is shared with the detailed runtime. No extra Newton unknown or copied driver
law is introduced. The existing current foldback is not a hard current cap.

Independent loaded circuit/mechanism tests pass in both directions, with
ordinary resistance and active foldback. A full 100 ms twelve-driver zero-duty
run reproduces the direct zero-voltage braking trajectory, events and accepted
contact trace exactly. A second run with 5% +X worm duty completes, supplies
0.526–0.555 V to that motor and changes its joint path by up to 0.0124225 rad
relative to braking. It still moves under load: this is open-loop force
influence, not servo position control or walking. CAD electrical constants,
11.1 V imposed supply and 293.15 K winding boundary remain provisional.

Recipes and limits are in examples/full-robot/embedded-integration.md and compact
evidence in driver-integration-status.json. Next is the shared sampled firmware
connection, preserving its quantization, latency, saturation and deadlines;
battery/thermal coupling, deployed sensors and controlled task gates still
precede the full planner/teacher/student/robust-learning delivery. Contact and
timestep accuracy limitations below remain unresolved. The full goal is active.

### Previous: accepted contact impulse evidence

The reduced motor path now optionally records every accepted continuous contact
stage, excluding event-search probes and failed intervals. It reuses the detailed
runtime's complete-coverage impulse integrator and adds equal-window comparisons.
All three 100 ms audit runs reproduce their earlier sampled motion, terminal
state and event histories exactly, with 248/465/870 accepted segments recorded.

For 0.25/0.125 ms, total upward floor impulse differs by about 0.35%, but -X
world-X impulse changes from 0.249471 to 0.196277 N s (about 21%). The -X impulse
vector differs by 0.262235 N s over 70–72 ms despite closer whole-run totals.
The first 2 ms window above an illustrative 0.01 N s discrepancy is 22–24 ms
at +X recontact. These are numerical diagnostics of unpowered short-circuit
braking, not calibrated support, traction or walking evidence. Ground contact
timing and backlash sequences remain timestep-sensitive.

See `examples/full-robot/contact-integration-status.json` and the accepted-stage
section in `embedded-integration.md` for recipes, hashes and limitations. Next:
inspect accepted motion/contact states around recontact, distinguish normal and
tangential force effects, and complete the shared driver/firmware/thermal and
sensor contracts required for controlled task tests. The full planning,
teacher/student learning and viewable-policy goal remains incomplete.

### Previous: scheduled motor engagement and mechanical reuse

The event-capable reduced model now completes 100 ms with all twelve registered
motor components, original contact and joint friction. It schedules existing
startup/engagement/release jumps through shared Rust event handling; no motor
law was copied into a robot-specific runtime. Boundaries are 0 V (short-circuit
braking), fixed 293.15 K, not powered servo control. The unscheduled failures
below are retained as history, not the current event-capable result.

The 0.5/0.25 ms runs take 129.026/168.024 s in single diagnostic captures.
Sampled foot-tip differences reach 1.793 mm (-X); the other feet stay below
0.576 mm. Contact-pair sets differ at 18/51 output samples and five motor guards
have different switch counts. Completing both runs has not established timestep
accuracy, realtime operation or training readiness.

Exact mechanical-endpoint reuse is opt-in. Both 0.5 and 0.25 ms full-robot
100 ms captures match their uncached saved frames, terminal state and event
schedule exactly. Single-run cost drops from 129.026 to 53.065 s and from
168.024 to 70.137 s, respectively (about 2.4x, not a promoted realtime result).
It reuses only identical mechanical inputs within one solve; component forces
and accelerations remain fresh. Focused linear motor/load and event tests match
the uncached result exactly. The 19 embedding/step/motor tests pass both native
default and no-default builds; 21 shared dynamics/root/schedule tests and 18
runtime session tests pass. WASM compilation passes with the prior unused
`rotor_speed` warning. Details, reproduction commands and limits are in
`examples/full-robot/embedded-integration.md`. Next: quantify full-run reuse,
refine task trajectories/contact events, and complete driver/firmware/thermal
coupling and deployable sensors before planning/learning gates. The full
PLANC-inspired quadruped, teacher/student and robust-learning objective remains
active; none of the solver experiments substitutes for that delivery.

A further 0.125 ms cached run completes in 73.518 s, but sampled foot differences
against 0.25 ms still reach 1.698 mm and do not consistently contract. The first
0.5/0.25 ms sampled contact-set disagreement is at 18 ms at +X foot lift-off.
Next accuracy evidence needs accepted-step contact impulses and denser event
localization. Compact hashes, timings, comparison results and limitations are
in `examples/full-robot/event-integration-status.json`; raw ignored captures
are not versioned golden tests.

### Historical: reduced stepping, QR validation and shared motor coupling

The reduced mechanism now supports `step_implicit_coupled`: auxiliary component
unknowns and scaled equations are solved simultaneously with mechanics. The
load-only method uses the same path with no auxiliaries. `EmbeddedMotorBank`
calls the registered `MotorUnit::residual`, including original winding, rotor,
gearbox, backlash/loss and heat-output equations. CAD parameter derivation is
shared with the detailed runtime through `cad_motor_unit_parameters`; its prior
floors/estimates remain explicit, not newly calibrated values. Failed solves
leave both mechanical and auxiliary input states untouched.

QR is still opt-in (`embedding.dependent_solve: pivoted_qr`), retains all original
rows and SVD rank checks, and now passes analytic base-pose/slider-crank tests,
loaded acceleration against an independent constrained system, toggle rejection
and implicit refinement toward an explicit reference. The contact-free robot
with original joint friction completes 100 ms with QR; the SVD path previously
failed at 74.5 ms despite negligible joint motion. Original-contact QR still
fails: 37.5 ms with 0.5 ms steps, 65 ms with 0.25 ms steps. This distinction is
why numerical failures must not automatically become physical travel limits.

Tests: 17 embedding/stepping/motor-adapter tests pass in native/default and
no-default builds, plus 5 existing motor-Jacobian tests and 18 runtime-session
checks. WASM runtime compilation passes with the existing unused `rotor_speed`
warning. CI includes the new motor-adapter cases. An independent five-variable
linear circuit/mechanics solve checks loaded motor motion with finite or zero
inductance, gearbox compliance, reflected inertia, back-EMF, torque and heating.

`integrate_embedding` optionally couples all twelve CAD motors, recording their
bindings, parameter maps, states, current/torque/speed/heating and equation
residuals. `examples/full-robot/mechanical-motor-boundary.json` declares 0 V and
293.15 K boundaries (short-circuit braking, not open-circuit or servo hold).
Original contact and joint friction remain. Driver, battery, firmware, sampled
sensors, thermal-network and motor-event scheduling are not yet connected.
The 0.5 ms test stops at 13 ms; the 0.25 ms test at 16.75 ms.

A new optional bounded trial capture identifies a discontinuity at +X foot
motor engagement: gap differs by 4.55e-9 rad across two probes while torque jumps
from 0.000530 to 0.010605 N m, with nearly equal relative speeds. These are
unaccepted Newton trial values, not measured robot motion. Trace-enabled capture
reproduces committed motion bitwise. This identifies a force-law jump, not proof
of the sole failure cause or of absence of a coupled root. Local evidence is in
`runs/full-robot/learning/mechanical-motor-boundary-trials.json` and
`backlash-trial-summary.json`. The adapter currently rejects event-mode motors
until a scheduler is connected; do not silently enable them without guard/jump
handling.

Next: reuse the shared event handling for backlash engagement, distinguish that
from contact/conditioning failures, then add driver/firmware/thermal coupling
and full task gates. Do not weaken residual checks or ban normal engagement as
a travel limit. No complete fast-model episode, controller-training gate or
realtime promotion has passed. Details: `examples/full-robot/embedded-integration.md`.

### Experimental time stepping: smooth tests pass; stiff robot case rejected

Added shared `RigidEmbedding::step_midpoint` with independent-coordinate
midpoint advancement, world-frame quaternion updates, stage-consistent bristle
history, endpoint acceleration/contact output and atomic failure. Load callbacks
are pure; sampled controllers must hold commands and split at deadlines outside
the method. Authored IMUs are rejected until their sampling is integrated.
The stricter `scaled_position_tolerance=1e-11` leaves the original 1e-8 velocity
and acceleration checks intact; base position/quaternion rates are now populated.

Native/default and no-default tests: 9 pass each (6 embedding, 3 stepping).
The WASM runtime compiles, with the existing unused `rotor_speed` warning.
Added analytic free-body motion,
oscillator refinement, contact-memory decay/failure preservation and integrated
closed-linkage energy/closure checks. The linkage needs refined test steps to
resolve >35 rad/s motion; its 1e-7 J energy gate passes in that resolved regime.

The new `integrate_embedding` diagnostic keeps motor states explicitly out of
scope and applies declared generalized loads. On the full robot with zero
applied loads, original contact/friction and 0.5 ms steps, the first version
stops at 17 ms; tightening geometric closure stops at 23 ms, and 0.25 ms steps
stop at 19 ms. Contact-free with original joint friction also stops (11 ms).
Only the diagnostic copy with contact and joint friction removed completes
100 ms, with negligible internal motion. That is evidence isolating stiff
dissipative terms, not authorization to remove friction or promote the backend.
An unstable contact sample has about 1,149 N tangential versus 0.194 N normal
force. Separate motor inertia/response remains uncoupled and matters to these
timescales. No full-robot runtime promotion or learning gate passed.

Details: `examples/full-robot/embedded-integration.md`; local diagnostic artifacts
under `runs/full-robot/learning/mechanical-*`. Next: implicit dissipative updates
and actuator coupling, not more promotion claims from instantaneous kernels.

### CAD-derived independent mechanism coordinates

Added `RigidEmbedding` in the shared robot library. It solves dependent joint
positions with every original geometric loop/transmission row retained, obtains
the velocity map using unit-velocity kinematics, and computes the nonlinear
acceleration bias. A rank loss or inconsistent independent-coordinate choice
fails explicitly. It preserves a floating base and all link inertia through
`Tᵀ M T`. A separate instantaneous acceleration method uses the shared original
gravity/contact/passive-load evaluator. This is an explicitly ideal-closure
experiment; detailed CFM/stabilization equations and runtime defaults stay intact.

Five native/default and no-default tests pass, including closed-form
slider-crank geometry/velocity/curvature, floating-base inverse-dynamics inertia,
invalid charts, a geometric toggle, and loaded acceleration against an
independent original-row KKT/SVD solve. `audit_embedding` applies it to declared
CAD motor bindings. Initial full-robot kinematic sweep: 41 configurations,
34 → 18 mechanical velocity coordinates, 29 links retained; maximum scaled
position/velocity/acceleration closure errors 1.31e-12 / 1.56e-15 / 2.32e-16.
The extended robot force-balance audit passes at all 41 configurations: feeding
the reduced accelerations into original inverse dynamics gives maximum projected
force/torque discrepancy 2.13e-14. WASM runtime compilation also passes, with the
existing unused `rotor_speed` warning. In this single diagnostic run, mapping
took 55.68 ms total and instantaneous dynamics 5.06 ms total across 41 points;
neither is a full-simulation throughput measurement. This is prescribed geometry
and instantaneous dynamics, not a completed timestep, actuator model, walking
episode, or speedup claim. See `examples/full-robot/mechanism-reduction.md`.

### Sampled task-space tracking

Implemented shared `tracking` evidence capture/comparison and the `sim-track`
CLI. A marker uses the full link pose and an explicit local point in metres.
Reports compare aligned trajectories against a declared placement error budget,
subtracting both measurement uncertainty bounds and a reserve for other errors.
Missing points, truncated intervals, excessive sample gaps, invalid values,
incorrect frame/experiment identity and timestamp mismatches fail closed.
Source labels distinguish synthetic, simulation and imported hardware data;
simulation captures carry the complete recording and marker configuration.

Four native tests pass: known vector-error arithmetic with uncertainty, invalid
and incomplete inputs, rotated marker geometry with ambiguous-link rejection,
and an actual moving pendulum replay plus timestep-refined comparison. The CLI
synthetic example reports the expected 5 mm error and 4 mm remaining margin.
CI now includes these checks; the shared runtime also compiles for WASM (the
pre-existing unused `rotor_speed` warning remains). Marker configuration can
reject a different declared CAD source hash, covered by the moving-runtime test.

Full-robot 100 ms captures at 0.5 and 0.25 ms physics steps completed with 51
samples of all four revision-1357 foot-surface markers. Controller/action
schedules and initial scene remain equal. Maximum point discrepancies were
0.541 mm (-Y), 0.953 mm (+X), 0.557 mm (+Y), and 1.412 mm (-X). The provisional
1 mm diagnostic fails on two -X samples, with the worst at 44 ms. This is
simulation-to-simulation disagreement, not error against hardware, a converged
reference, or an accepted foothold budget. Capture time is not a throughput
benchmark. Artifacts: `runs/full-robot/learning/foot-track*.json`.

Inspection also found no authored hardware sensors in this exported model
(`sensors: []`), and a 0.22144 kg +X sliding foot/crosshead versus about
0.07689 kg in each of the other legs. This is a calibration/derivation audit
item, not a confirmed error; do not silently make them equal. Preserve these
source properties in any initial reduced model and record uncertainty explicitly.
The tool and fixtures do not authorize robot walking-policy training.

## Retained solver work: contact reuse, convergence and differentiation

### Selected state-derivative rows: local checks pass, runtime promotion rejected

Added a shared `Behavior::state_row_jacobian_at` hook and compiler assembly for
complete state derivatives of selected local state residuals. Other local rows
and all rate derivatives retain their previous calculation; full Jacobian hooks
take precedence. Initial tests pass for exact zero rows, nonzero rate-dependent
state coupling, both rate paths, and deterministic worker counts. Provider-port
mapping and preservation of other components' shared torque contributions also
pass. Complete Jacobian precedence is now tested (three compiler integration
tests pass). Invalid-claim rejection still needs dedicated coverage.

An opt-in `constraint_state_step` uses the shared cheap closure evaluator for
central h/h2 Richardson probes of angular loop state rows, with explicit CFM
diagonals and kinematic dependency filtering. It defaults off and is omitted
from serialized runtime options when absent. Local loaded rigid/flexible
full-residual stencil comparisons pass, including worker equality. The missing
registry declaration was fixed and release replays completed. Native robot
checks pass (22 tests, one pre-existing derivative gate ignored); no-default
compiler/provider tests and the WASM runtime check also passed.

The rate-partial plus selected-row experiment completes 100 ms where the prior
rate-only experiment failed around 35.6 ms. However, it requires 133 subdivisions
versus 44 for the default and differs by up to 0.650 mm in position and 35.13 N
in a contact-force component, with contact-presence differences. Across four
step-size/rate-hook variants, wall times were 10.63–24.40 s for 100 ms simulated,
versus a fresh default 8.128 s. These are single runs, not repeated speed claims.
No variant earns promotion. Default capture remains exactly equal to the earlier
full baseline JSON, including its attempt audit. The generic helper remains;
the experimental derivative policy stays off.

Evidence: `runs/full-robot/solver-performance/constraint-state-*`, including
the profile and snapshot summaries and default trajectory/profile; these ignored
artifacts are diagnostics, not the versioned robot baseline.

User clarified the immediate application: PLANC-style footstep guidance for
initial neural-controller training, followed by environmental perturbations.
This changes the fidelity priorities: accurate task-relevant kinematics,
actuator response/limits/latency, balance and contact outcomes; measure uncertainty
on hardware, then evaluate robustness across it. Numerical solver correctness
and model-to-hardware accuracy are separate gates. A reduced planner/training
model can be useful without resolving every internal transient, but reductions
must remain explicit, CAD-derived, and validated against relevant outcomes.
PLANC uses a reduced-order planner to guide teacher RL, student distillation,
and curriculum fine-tuning; its bipedal model needs adaptation for a quadruped.
Primary source: https://arxiv.org/html/2601.06286v1 .

### Shared constraint-only evaluation: exact rows at lower probe cost

Extracted loop/transmission residuals and reactions into one shared helper.
`Articulated::evaluate_constraint_rows` runs the existing forward kinematics and
same closure arithmetic without contact queries, cables or the backward force
pass. Full evaluation uses the helper with reaction accumulation; equation order,
CFM, stabilization, modal kinematics and topology-identity behavior are retained.
No derivative or simulation options have changed.

The 100 ms baseline replay is exactly equal to `commit-audit-trajectory.json`,
including its complete attempt audit. A new test checks bitwise row equality at
64 moving configurations spanning flex and identity options, with contact on and
off. Native mechanism/Jacobian/transmission suites: 21 passed, one pre-existing
unpromoted derivative gate ignored. The new test also passes without default
features, and the WASM robot library compiles.

New `compare_constraint_rows` diagnostic checks every saved stage plus four
joint-position probes per coordinate and measures alternating full/constraint-only
evaluations. At 291 baseline stages, 32,883 probes match bitwise; at 93 failed-path
stages, 10,509 probes match. All 28 loop/transmission rows match. With 100 timing
pairs per stage, full evaluation averages 75.39/76.21 us versus 5.254/5.321 us for
constraint-only evaluation, respectively: about 14.3x cheaper for this kernel.
These are saved stage states, not the terminal last-linearization point, and no
new derivative integration or trajectory speedup has been demonstrated.

Evidence and reproduction commands: `constraint-evaluator-manifest.json` and
`constraint-evaluator-*` in ignored `runs/full-robot/solver-performance/` (diagnostic
artifacts, not a versioned baseline). Next: use these exact cheap rows for guarded
state-derivative probes, verify against full-residual differences, and assess
convergence and trajectory accuracy before enabling any new derivative policy.

### Terminal failure isolated: angular state derivatives spoil the correction

Added `audit_last_action_only` to `capture_trajectory`, retaining a bounded audit
of the final attempted action without warmup consuming its capacity. Default
capture configuration/output remains unchanged when the option is false. Tests
verify identical reporting snapshots/events and exact retained-window attempts.
The rate-partial replay reproduces every prior snapshot and its failure, while
retaining all 93 attempts from the 34–36 ms action without hitting the 512 cap.

At the terminal fresh matrix (35.574707 ms, step 0.488281 µs), recorded residuals
reproduce bit-for-bit. The rate-partial and ordinary numerical configurations
have identical state matrices; 649 rate entries differ by at most 4.14466e-6.
Under common diagnostic equilibration their condition estimates are 6.40e6 and
5.32e13. These are scaling-dependent dense diagnostic estimates, not production
factorization measurements. Every tested correction fraction from 1 to 1/4096
worsens the residual for both matrices; knee angular rows dominate.

Independent actual-increment checks at radii 1e-6 and 1e-7 fail with respectively
277/220 resolved mismatches and 155/1334 inconclusive comparisons. Smaller probes
increase roundoff limitations; contact/actuator branch crossings also appear.
This does not make every discrepancy a smooth derivative error.

Extended `compare_matrices` with optional selected state-row probes, using
central h/h2 stencils and Richardson extrapolation while retaining the original
residual and rate matrix. A cubic reference test covers extrapolation, unchanged
other rows, fixed rates, and invalid row specifications. Replacing just eight
knee angular state rows at relative radius 1e-4 changes the full correction's
scaled residual from 1.85552e-4 to 6.49063e-8, versus initial 5.48456e-7: an 8.45×
reduction from the starting point, with condition estimate essentially unchanged.
Replacing all twenty knee rows provides no further improvement. Radius 1e-5
also improves the initial residual, but less strongly (3.67417e-7).

This isolates bad angular state derivatives as a useful target at this captured
failure; it is not a converged step, full trajectory, or timing promotion.
All simulation defaults and physical equations remain unchanged. Evidence:
`rate-failure-*` and `closure-row-*` in the ignored solver-performance directory.
Next: supply reliable constraint-state derivative blocks without repeated full
contact/force evaluations, then run the full accuracy and timing gates.

### Direct inertia derivative integration rejected on full-run convergence

Integrated direct rigid inertia and link acceleration maps into the existing
experimental rate/hybrid derivative hook, sharing the original loop derivative
equations and retaining the original modal-flexibility fallback. The candidate
passes the moving rigid mechanism checks and a new independent loaded-contact,
cable and loop-reaction rate-column check. Local correctness did not establish
full-run reliability.

With `articulated_rate_partials=true` on the unchanged 0.5 ms hold scene, both
old and candidate paths fail the requested 100 ms replay. Captures reproduce the
profile failures: old fails at Newton stage 35.5742 ms (last committed 35 ms,
356 subdivisions); candidate at 34.0107 ms (last committed 34 ms, 335
subdivisions). Both captures contain 19 reporting snapshots, with attempt audit
disabled. At the common 34 ms report, one contact is present only in the old
path. Among common measurements through 34 ms, maximum position difference is
24.39 µm, current difference 0.09177 A, and contact-force component difference
1.40017 N. These are differences between failed paths, not accuracy estimates.

The two prototype profile runs briefly overlapped and both failed, so their
wall times are not used. No completed trajectory speedup is claimed. Candidate
derivative/inertia source changes were removed byte-for-byte; the separately
validated `rigid_mass_matrix` building block remains. A loaded rigid-rate
regression is retained and passes with the restored path in native and
no-default-feature tests. Native release tools were rebuilt. All analytic
defaults remain off. Artifacts and comparison: `direct-rates-*` under
`runs/full-robot/solver-performance`.

Next priority is convergence/conditioning at the contact transition on identical
solver states, rather than promoting faster mass construction into a failing
trajectory path.

### Direct rigid inertia: validated building block, timestep unchanged

Added `Articulated::rigid_mass_matrix`, assembling link motion maps and their
mass/inertia contributions with one kinematics pass and no contact queries or
acceleration probes. It retains off-diagonal coupling, floating-base world
coordinates, branches, ball/prismatic joints and six-coordinate compliant fixed
joints. Modal flexibility is explicitly rejected; no physical state is silently
removed. This is direct inertia assembly, not the paper's recursive ABA.

Three tests cover analytic pendulum inertia, independent inverse-dynamics
columns and kinetic energy on a branched multibase mechanism, and invalid/flex
inputs. Native and no-default-feature runs pass; the library compiles for WASM.
CI includes both test configurations. No default Jacobian or timestep changed.

The runtime block comparison now uses direct inertia for the projected solve
and loaded acceleration probes for its independent full-system reference. All
240 committed hold states pass the matrix comparison; maximum entry difference
is 1.38293e-13. Maximum acceleration disagreement between those two ideal
mechanical solves is 2.28065e-8 in mixed acceleration units. Remaining-constraint
reaction disagreement is at most 4.91584e-9. These are fixed-state comparisons,
not a new trajectory or a contact-accuracy promotion.

Ten alternating mass-construction pairs per state average 492.45→23.43 µs
(21.0×). The probe timing excludes its already-prepared contact evaluator and
bias; the direct timing includes kinematics. Total diagnostic assembly includes
additional loads and constraint auditing and must not be assigned this speedup.
Artifacts: `runs/full-robot/solver-performance/rigid-inertia-*`. Next work is
integration into the experimental derivative/mechanical path and full-trajectory
accuracy/timing gates; all analytic defaults remain off.

### Output-signal probe reuse rejected on total-runtime evidence

Tried bypassing component evaluation when perturbing a produced signal unknown
that is absent from every component input, preserving the original balance
subtraction and finite-difference rounding. Feedback aliases remained on the
ordinary path. The prototype removed 235,898 of 3,302,572 FD residual calls
(7.14%) per 100 ms hold run. Its complete 51-frame/291-attempt capture is exactly
equal to the reference, and all deterministic profile fields and other work
counts match across six before/after pairs.

Initial median timings were 8.86291→8.85084 s (0.14%, mixed paired results).
A second variant removed full scratch allocation from output-only batches;
median timing worsened 8.68034→8.92111 s (2.77%), with all three pairs slower.
Neither demonstrates a full-runtime benefit, so both production variants were
removed. No trajectory or browser claim is made for the second variant; its
native full-run profiles and worker-count regression passed.

Retained an independent full-residual derivative regression for large outputs,
rounding, self-feedback and 1/2/4/8/16 workers. Production compiler code is
byte-identical to the pre-experiment source. Reproduction, source patches,
timings and initial capture are archived as `output-reuse-*` under the ignored
solver-performance directory. This supports prioritizing expensive dynamics
construction over already-cheap prepared output probes.

### Transmission coordinate experiment: mechanical block validated, not promoted

Added a reusable `TransmissionCoordinateMap` in `sim-domain-robot` and the
`compare_transmission_block` runtime example. Signed constant-ratio forests
support affine closure offsets, projected inertia/loads, remaining constraints,
and reconstruction of transmission reactions. Invalid cycles and singular
systems fail explicitly. Four analytic/reference tests pass with and without
native parallel features; the library also compiles for WASM.

Across all 240 committed states of the 100 ms hold capture, the projected ideal
transmission block reduces 62 unknowns to 46. Maximum acceleration disagreement
with the independent full ideal block is 3.10e-9 in mixed coordinate units;
maximum remaining-constraint reaction disagreement is 4.28e-9. The first
alternating kernel benchmark averages 33.55→27.33 microseconds (about 19% faster).
This is not Chignoli's recursive algorithm or a coupled-timestep speedup.
Both compared blocks explicitly set transmission regularization to zero;
production regularization and all solver defaults remain unchanged.

Exposed the existing dependency-checked prepared evaluator for repeated loaded
mechanical evaluations. Using it for unit-acceleration mass assembly preserves
every non-timing result across the 240 states. Separate preliminary runs measure
mean assembly 3.606→1.243 ms; this is not a paired full-runtime benchmark. Five
prepared-evaluation regressions pass. After the shared constructor refactor,
the entire default 51-frame/291-attempt capture is identical to the reference.
Evidence is under `runs/full-robot/solver-performance/transmission-block-*` and
`transmission-prepared-*`; ignored measurements are not a versioned baseline.
Next work is direct/recursive block construction and explicit integration/model
validation before any trajectory or realtime claim.

### Transmission elimination prerequisite: regularization is not a hard identity

Audited all 240 committed stages of the existing 100 ms hold capture. The eight
transmission rows use phi_ddot + 200 phi_dot + 10000 phi + 1e-6 lambda = 0.
Maximum angle mismatch is 4.70064e-11 rad, but the reaction-dependent term reaches
1.54014e-6 rad/s². Thus the geometric ratio is extremely close on this recording,
yet deleting its regularization term changes the discrete equations. An ideal
coordinate map must be compared explicitly rather than labeled an exact
reformulation of the current model. No new physics, solver option, or runtime
optimization was introduced in this audit. Reproduction and measured rows are
in `runs/full-robot/solver-performance/transmission-regularization-audit.py/.json`.
This hold-only evidence does not establish behavior under commanded motion.

### Latest experiment: separate pose reuse does not improve total runtime

A scoped pose cache reused the exact original forward-pass offsets, axes,
rotations and positions when base/joint/modal pose inputs were bit-identical.
Velocity/acceleration terms and forces still used current inputs. The initial
prototype passed 26 selected robot tests (one existing analytic gate remains
ignored) and reproduced the complete 51-frame/291-attempt 100 ms capture exactly.
Three native 16-worker pairs nevertheless regressed median runtime
8.01977→8.11194 s (1.15%). A compile-time-specialized variant avoiding inner-loop
cache branches also failed to establish a benefit: median 9.79508→9.88374 s
(0.91% slower), with mixed individual wins. Timings varied across the sessions;
compare paired variants, not absolute values across sessions. All deterministic
profile outputs/work counters agree for both variants. No full capture or browser
pass is claimed for the specialized variant.

Both production source files were restored byte-for-byte and the release tools
rebuilt after invalidating source timestamps. The multi-axis prepared-residual
regression test is retained and passes on restored code. `pose-reuse-*` archives
patches, profiles, capture, comparisons and logs. No speedup is promoted.
The user requested research into comparable real-time formulations; investigation
now prioritizes published local-loop embedding, loop-aware constrained dynamics,
and robust contact formulations before another small geometry-cache experiment. See [the research review](examples/full-robot/realtime-research.md)
for sources, limitations, and the proposed shared-library comparison.

### Latest decision: capped search does not earn promotion on impulse evidence

Replayed the search-merit cap prototype with committed contact-impulse audits at
0.5, 0.125 and 0.0625 ms nominal steps. Its coarse reporting frames reproduce the
earlier candidate exactly. Relative to the finest unchanged comparison run,
coarse pose RMS improves 0.494→0.458 mm, but net sideways floor-impulse error
grows 0.01359→0.03096 N·s. Final +X/-X foot impulse-vector errors grow
0.00615→0.01929 and 0.02096→0.03762 N·s; maximum accumulated +X error also grows.
The finest run remains a comparison level, not established ground truth.

A solver regression caught the prototype mislabeling its alternative merit as
the original Jacobian-scaled norm. Separating those scores restores all nine
convergence tests without changing search decisions. That diagnostic fix was
tested in the prototype; neither the cap nor its extra field remains in the
restored production solver. No new timing or broad accuracy claim is made.
Evidence: `merit-impulses-*`. The prior 21% fewer Jacobian rebuilds are insufficient
to justify mixed trajectory/impulse results.

Next reuse opportunity confirmed in source: changing velocity or acceleration
invalidates the complete kinematics cache even when pose inputs are unchanged.
The resulting forward pass recomputes joint rotations and pose transforms.
Investigate a separate pose cache with conservative bitwise dependency checks,
keeping all velocity/acceleration forces current and requiring exact capture
equivalence before accepting a speedup.

### Latest retained: committed-step audits and contact impulse comparison

Shared implicit attempts now record `committed: true/false`; legacy captures
retain unknown status. Event-search probes and discarded outer candidates stay
false, successful subdivision leaves become true only after their enclosing
candidate commits, and snapshot restoration revokes superseded commits. The
complete 100 ms robot capture is unchanged except for this metadata and its
explanatory notes: 240 committed solves, six successful discarded solves and
45 failed solves. Merely filtering successful solves would overcount work.

The shared Rust runtime now integrates linear contact impulses using committed
solver stages. It rejects missing/unknown/capped coverage and gaps or overlaps.
The recording CLI clears bounded audit storage between reporting windows; WASM
and the worker expose the same diagnostic. Analytic quadrature, discarded
candidate/event-search, partial subdivision failure, snapshot rollback, legacy
serialization and contact-window additivity tests pass. Native/browser first-
window impulse parity is exercised in the pendulum CI recipe; the existing
4 ms full-robot sparse gate also passes. Remote CI has not run.

All 100 ms reporting frames remain exact at each tested timestep. Integrated
floor vertical impulse is 3.89980, 3.89559 and 3.91474 N·s at nominal 0.5, 0.125
and 0.0625 ms respectively. The two finer levels differ by 0.49% in total vertical
impulse; individual vertical foot impulses differ by about 1.7% and 0.8%.
Sideways impulses and pose trajectories remain less stable. This qualifies the
large sampled force-peak differences but does not establish overall convergence.
No solver policy, tolerances, contact law or analytic-promotion defaults change.
Evidence: `commit-audit-*`, `commit-impulses-*` and the latest runtime audit.

### Latest accuracy investigation: small-angle rotation and an unconverged reference

The shared Rodrigues helper loses representable cross terms through cancellation
in `1-cos(angle)`: at 1e-9 rad about (0.6,0.8,0), R_xy becomes zero instead of
2.4e-19. An algebraically equivalent evaluation restores that term and its
numerical derivative; three focused rotation tests and 25 existing robot tests
pass. The full robot nevertheless takes 8.13 s in the initial 100 ms profile,
with 2,755 fresh Jacobians versus 2,743 in the reference. No speedup is established.

Recorded 100 ms comparisons at nominal 0.5, 0.125 and 0.0625 ms steps show that
neither evaluator has a converged trajectory reference at those resolutions.
For the unchanged evaluator, halving 0.125→0.0625 ms changes sampled contact forces
by up to 149.81 N, link positions by 2.76 mm, and a servo angle by 0.03162 rad.
Pointwise forces near impacts are not impulse comparisons, and finer nominal
steps still incur convergence subdivisions. Do not promote a changed evaluator
merely because its isolated rotation derivatives are more accurate.

The rotation candidate and tests are archived; production math is restored
byte-for-byte. `stable-rodrigues-*` contains source/test patches, full reporting
captures, comparisons and logs. Next validation work needs contact impulses on
actually committed substeps: a successful nonlinear trial can still be discarded
by an outer event search. Current attempt records explicitly warn of this but do
not carry commit status. This distinction must be resolved before integrating
forces from arbitrary attempt logs.

### Latest experiment: guarded backtracking; adaptive native/browser divergence isolated

The shared solver/runtime now exposes `guarded_backtracking`, off by default and
recorded in scenes/replays. It stops an exhaustive halving search after three
successively worse probes only when a decreasing trial exists and the old scaled
norm is above 100 times the absolute tolerance. Raw acceptance/correction checks
are unchanged. This is a heuristic, not a proof that later probes cannot improve.

Three sequential native pairs improve the 100 ms median 7.84782→7.42347 s
(5.4%); ordinary residual calls fall 25,292→20,645 (18.4%). All other work counters
match. The full 51-frame/291-attempt comparison differs only in policy/audit
metadata and exact omitted trial suffixes. The preliminary equivalent algorithm
also preserves accepted states and reported trajectories on a 200 ms hold and a
100 ms worm-command step. Each held-out case changes one rejected attempt; the
200 ms case proves that the guard can miss a later, better search point.

Nine convergence, 17 session and 12+8 dynamics/audit tests pass. The native/WASM
contact pendulum passes with two observed guarded stops; CI now requires that
branch to be exercised. The 12 ms full robot fails the existing native/WASM gate
for BOTH exhaustive and guarded policies, whose frames are identical within each
platform. No full-robot cross-platform promotion is justified.

A new captured-step diagnosis isolates adaptive scheduling: the browser rejects
the 4→4.5 ms attempt while native accepts it. Forcing native onto the browser's
26 accepted intervals makes every reported frame pass the unchanged 1e-7 gate
(maximum difference 2.224e-11). Initial tiny differences and search decisions
already diverge earlier; the low-level source is not yet identified. Matching an
imposed grid is not a timestep-accuracy proof or a fix to adaptive robustness.
Evidence: `bracket-option-*`, `bracket-search-*`, `platform-grid-*` and the runtime
audit. Guarded backtracking remains experimental/default-off.

A subsequent merit-weight cap experiment reduces fresh Jacobians 2,743→2,168
and takes 6.33 s in an initial 100 ms profile, but changes contact-force signals
by up to 24.17 N. It is archived and removed; the pre-experiment solver is
restored byte-for-byte. Raw acceptance stayed unchanged, so passing nonlinear
solves still does not establish equivalent trajectories. Further work needs
matched-grid/timestep accuracy evidence, not promotion from that timing alone.

### Latest retained: transfer fresh joint axes instead of cloning them

Fresh dynamics evaluations now move their owned joint-axis vectors into the
returned `Evaluation`. Previously they copied each joint's vector and then
dropped the originals. Prepared/borrowed axes still receive the required owned
copies. Public evaluation contents and arithmetic remain unchanged.

Three sequential native pairs improve median 100 ms runtime
7.91471→7.77826 s (1.72% lower). All work counters and final-frame/event/stats
checks match. The complete 51-frame/291-attempt capture is byte-identical to the
previous retained implementation. Eight articulated tests, 10/9 native/serial
Jacobian tests and 16 session tests pass; the prior ignored hybrid gate remains.
WASM build and both contact-pendulum/short sparse-robot browser gates pass with
exact replay and unchanged native/WASM differences.

A separately measured force-buffer reuse experiment regressed in all three
pairs (median 7.69831→7.76523 s relative to axis-only), so it is archived and
removed. Only the axis transfer is retained. No solver decisions, scheduling,
equations or analytic promotion policy change. Evidence: `axis-move-*`,
`force-move-*` and the latest runtime audit. Convergence/accuracy work stays open.

### Latest retained: prepared exclusion lookup saves another 2.8% of native runtime

Contact geometry now builds sparse neighboring-link exclusion metadata once and
shares it immutably across the prepared linearization's geometry queries. The
original tree/loop lookup still chooses each pair's origin/radius; fresh
evaluations rebuild metadata after model edits. Per-sample checks use a small
sorted adjacency row instead of repeatedly scanning every joint and loop.

Three sequential 16-worker pairs improve median 100 ms runtime
8.17592→7.94995 s (2.76% lower), with Jacobian assembly
3.52742→3.32501 s (5.74% lower). All work counters match. The complete
51-frame/291-attempt capture is byte-identical, including physical diagnostics
and Newton decisions. The attempt cap is not reached.

Native/serial Jacobian suites pass 10/9 tests, with the same pre-existing ignored
hybrid gate; a new exclusion-priority/model-edit unit test passes and is added to
CI. Sixteen session tests, WASM build, the contact pendulum browser gate and the
short full-robot sparse browser gate pass. Native/WASM differences are unchanged:
2.706e-11 for the pendulum and 2.349e-13 for the 4 ms robot recording, both with
exact replay. Browser checks establish portability, not a measured browser
speedup or a long full-robot trajectory guarantee.

Retain the scoped lookup cache. No equations, tolerances, analytic policy or
worker scheduling change. Convergence sensitivity and derivative promotion
remain open. Evidence is archived under `neighbour-cache-*` and documented in
the latest runtime audit.

### Latest audit: scratch reuse did not improve runtime; contact topology lookup is next

Tested private derivative scratch reuse first within fine Rayon column batches,
then across small components in each outer job. Three sequential pairs per
variant show no median benefit: 8.21467→8.23769 s for inner reuse and
8.18013→8.18793 s for outer reuse. Both pass compiler/Jacobian tests and preserve
profile final frames, events, solver stats and all work counters. Neither earns
promotion; both patches are archived and compiler source is restored exactly.

A six-second native stack sample of the retained run identifies repeated
`neighbour_band` searches inside contact geometry, alongside SDF evaluation and
allocation. Source inspection confirms each lookup scans joints and loops for
fixed export-pose exclusion metadata. Next investigate preparing that metadata
once per immutable evaluation/linearization scope, preserving tree/loop priority
and exact exclusion behavior. Sampling includes startup and idle workers, so its
counts are not wall-time percentages. No new speedup or analytic promotion is
claimed. See `fd-scratch-*`, `retained-cpu-sample*` and the latest runtime audit.

### Latest audit: larger symbolic caches offer little; accumulated-pattern experiment rejected

An exact-pattern trace of the retained 100 ms run found 2,709 distinct patterns
among 2,743 rebuilds. The current cache hits 12 times; even unlimited storage
would hit only 34 times. Cache capacity is not the main reason for misses.
Across the trace, 2,861 positions appear and 1,632 are always present; an average
matrix has 2,638 entries. This observed union is not a structural guarantee.

A temporary experiment retained previously seen positions as explicit zeros,
adding every newly encountered position before factorization. Symbolic builds
fell from 2,731 to 69, but assembly overhead and convergence work increased:
6,002→6,179 Newton iterations, 25,292→28,314 residual calls. The one diagnostic
run took 9.69 s versus the retained paired-run median of 8.21 s; no speedup is
claimed. The full 100 ms capture fails equivalence: 25,282/58,862 common
measurements exceed the existing diagnostic tolerance, per-link summed contact
force differs by up to 23.90 N, and contact presence differs at two report times.

The experiment is archived and removed. Production solver source is restored
byte-for-byte; a fresh retained replay matches final frame, event trace, solver
stats and all profiling counters. No analytic derivative or altered Newton
policy is promoted. Next prioritize residual/derivative work and the unresolved
convergence/accuracy sensitivity, rather than increasing the symbolic cache.
See the latest runtime audit and `symbolic-*` evidence. The previous direct-CSC
patch, logs and artifact manifest are also now archived.

### Latest: retained direct CSC construction; another 4.8% lower runtime

Factorization profiling found 0.287 s sorting/summing entries, 0.332 s rebuilding
the sparse matrix, 0.778 s symbolic lookup/build and 0.700 s numeric factorization
in a 100 ms run. The existing symbolic cache misses on 2,731/2,743 rebuilds;
cache capacity versus pattern variation remains to be investigated.

The solver now constructs CSC directly from its already summed, row-sorted
entries, preserving the old matrix structure and value bits. Three isolated
16-worker pairs give median 8.62348→8.21080 s (4.79% lower wall time); matrix
construction falls from 330 ms to 34 ms. All work counters match, and the full
51-frame/291-attempt capture remains byte-for-byte identical to the earlier
retained implementation. Newton decisions and physics are unchanged.

Both solver unit tests and eight convergence tests pass; the direct matrix test
is added to CI. Sixteen session tests and the WASM release build pass. The usual
21-frame browser contact gate passes. A new recorded-input browser mode also
tests the actual 600-unknown robot for 4 ms: three reporting frames, three
contacts, exact replay and native/WASM maximum difference 2.35e-13. This is sparse
path coverage, not a full-robot realtime or long-trajectory browser claim.

New profiling breakdowns and recorded-input/sparse browser checks are retained.
Next: distinguish symbolic-cache eviction from actual pattern variation before
changing cache policy, alongside the open convergence and accuracy work.
See `factor-audit-*`, `direct-csc-*` evidence and the latest runtime audit.

### Earlier: retained complete-evaluation reuse; 4.5% lower runtime with exact captures

Prepared articulated residuals now borrow the complete force/kinematics evaluation
when all physical inputs match bit for bit and only rates outside that calculation
change. `write_residual` still consumes the current position, bristle, modal and
sensor rates. State, acceleration or constitutive-input changes invalidate this
reuse; existing partial geometry/contact reuse remains available.

Three isolated 16-worker timing pairs for the same 100 ms workload give median
8.99526→8.58930 s (4.51% reduction). Jacobian assembly falls 10.35%; every solver
work counter is unchanged. Complete 51-frame/291-attempt captures are byte-for-byte
identical, including physical stage diagnostics, event traces, Newton decisions
and all measurements. The attempt cap is not reached.

Native/serial Jacobian suites pass 10/9 tests, including combined rate-only probes;
16 session tests pass. The WASM build and 21-frame real Chromium contact/replay
gate pass, with maximum native/WASM difference 2.70646e-11 and exact replay.
Existing CI commands cover the strengthened tests. The earlier failing hybrid
SDF derivative gate remains explicitly ignored and unpromoted.

Updated median profile: assembly 41.8%, ordinary residuals 30.3%, factorization
25.7% of wall time. These are the current cost proportions, not the original
93%/2.6% profile. This optimization preserves the existing trajectory; it does
not establish its physical accuracy or resolve the coarse-step ambiguity.
Convergence/retry and analytic-promotion work remain open. Evidence:
`evaluation-reuse-*` in the solver-performance directory and interactive browser
artifacts; details in the latest runtime audit.

### Earlier: step-doubling audit detects under-resolved steps, with measured overhead

Added `check_implicit_step_doubling` to the shared dynamics diagnostics. It compares
one step with two half steps from the same initial state and held context, with
order-correct differential endpoint estimates and separate algebraic differences.
It reuses a valid captured coarse result and preserves failed attempts. The
capture CLI accepts `doubling` after the Newton config and reports solver work.
This does not change the live timestep or solver policy.

The original 0.5 ms contact step is flagged on both roots: maximum angular-speed
differences are 0.2067 and 0.03452 rad/s. Three shorter intervals were additionally
reintegrated with 32 and 64 substeps from their own exact initial states. Their
largest angular-speed estimates range from about 26% below to 18% above the
observed fine-reference discrepancy. Reference refinement still changes those
errors, so these are useful diagnostics, not guaranteed bounds.

Checking a captured coarse result adds two implicit solves: 16–35 Newton
iterations, 5–12 fresh Jacobians and 19–73 ordinary residual calls in these
samples (excluding any necessary coarse re-solve for a changed initial state).
Do not turn this on everywhere as a claimed speedup. The next optimization needs
to reduce Jacobian work in error-controlled/retry steps and then demonstrate
matched-accuracy total runtime. Existing geometry/compiler optimizations remain;
analytic and faster-search experiments are still unpromoted. Dynamics tests,
the capture example, both example builds, and the dynamics WASM check pass.
See `step-doubling-*` evidence and the latest runtime-audit section.

### Earlier: identical timesteps still admit different contact solutions

The common-grid investigation rules out subdivision as the sole explanation for
the faster backtracking experiment's disagreement. After nine refinements, both
100 ms captures use exactly the same 405 successful trial steps and identical
event traces, yet the physical comparison still fails. The first divergent
stage begins at 40.5 ms: backward Euler, h=0.5 ms, ending at 41 ms.

Re-solving both captured terminal guesses with the established production solver
and **exactly the same initial state** preserves two different solutions. With
raw absolute tolerance tightened from 1e-10 to 1e-12, maximum residuals are
2.40e-13 and 3.28e-13, while the +X foot horizontal forces remain approximately
+4.66 N and -2.03 N. Terminal residuals reproduce bit for bit in the supplied
model context. This is evidence of distinct discrete solutions within tolerance;
it does not tell us which follows the continuous dynamics accurately. Keep the
faster search archived. Holding context fixed and refining this interval resolves
the observed starting-guess ambiguity at four or more substeps. Endpoint forces
and force-integral estimates continue changing with refinement through 256
substeps: root agreement alone is still not a timestep-accuracy pass. Next: use
this fixture to evaluate local error control and accuracy versus total cost.

Retained reusable diagnostics: `capture_trajectory` exports declared-unit frame
measurements, actual attempts, and algebraic/differential coordinate metadata.
`sim_dynamics::attempt_check::resolve_implicit_attempt` invokes the production
implicit step with a common initial state and fresh matrix, with no events or
subdivision. `resolve_captured_step` exposes that check for archived scenes.
Its optional substep count uses `refine_implicit_attempt` to repeat production
steps over the interval with held context and report every attempted solve.
Tests cover mixed midpoint DAE weights, changed-context rejection, failed solves,
and a scalar problem with two analytically known discrete roots.

The 12 ms fine-grid experiment does agree once actual subdivisions match (394
successful steps). Its only remaining raw-rate differences are unused algebraic
multiplier helper rates; multiplier values, physical reactions, contact forces,
closure and differential rates pass separately. That short result cannot stand
in for the failing 100 ms test. Full details and reproduction commands are in
the latest runtime audit; evidence uses `backtrack-grid-*` and
`backtrack-common-root-*`. No analytic derivative or faster search was promoted.

### Earlier: faster backtracking stays unpromoted; retained observational search audit

Captured 45 failed solves account for 1,837/6,002 Newton iteration entries
(30.6%) in the default 100 ms workload. Tested first sufficient decrease for
fresh-matrix backtracking, retaining the final correction/raw-residual gates.
One diagnostic timing pair improved 8.97635→6.56853 s, with ordinary residual
calls 25,292→12,091, fresh matrices 2,743→2,272 and subdivisions 44→34.

That performance result does not establish accuracy. Exported trajectories first
differ noticeably at 12 ms. Across 12 ms refinement runs at 0.5, 0.25, 0.125,
0.0625 and 0.03125 ms, agreement is nonmonotonic: the solvers agree closely at
0.125 ms, then disagree again at finer steps. At 0.03125 ms, the maximum matched
per-link floor-force component difference is 0.48846 N. Neither finest run is
an established physical reference, so no accuracy or default-policy promotion
is justified. Archived the prototype and restored the prior search decisions.

Retained optional `NewtonIteration.line_search` diagnostics: finite trial step
fractions/norms and the selected fraction. Old captures remain readable. A new
regression proves identical probes/work with audit on/off. All 291 full-robot
attempts match previous fields exactly after removing only the new observations.
The capture shows 24,462 line-search residual evaluations and 1,586 fresh
backtracked searches; the runtime scans 13 fractions through 1/4096. Of 1,336
searches with a sufficiently decreasing partial trial, 383 select a different
fraction when completing the old scan. The change was not merely skipping
redundant evaluations.

Eight convergence tests, 16 session tests, WASM build and the 21-frame real
browser contact/replay/audit suite pass. The previous geometry/compiler
optimizations remain retained. Next work needs a trustworthy common-time-grid
accuracy comparison for retry-policy changes, with the new trial-level audit
to locate divergent choices; no analytic derivative path was promoted.
See the latest runtime audit and `early-backtrack-*` / `backtrack-audit-*` evidence.

### Earlier: contact geometry reuse survives partial pose changes

Extended the immutable, per-linearization geometry cache to retain world sample
positions, floor/terrain depths and bounding boxes. When a probe changes some
link poses, reuse SDF hits and misses only for pairs where both poses remain
bit-identical; refresh every pair involving a moved body. Merge fresh/reused
hits in the original sample/pair order and still evaluate velocity-dependent
normal damping and friction from current inputs. Workers own their refreshed
results; the prepared cache remains immutable.

The initial floor-only cache had inconsistent timing and was not promoted on
its own. The combined partial-pair reuse gives three faster native paired runs:
median 9.50601→8.99857 seconds for the default robot's first 100 ms (5.34% less).
Jacobian assembly falls 4.36638→3.93966 seconds (9.77% less); all work counters
are unchanged. All 51 exported frames and all 291 captured solver attempts match
the saved, hash-verified pre-change baseline exactly, without audit truncation.

Native/serial derivative suites, four library tests, 16 session tests, 2,113
independent small-contact checks, and the real-browser 21-frame parity/replay
suite pass. Added a heightfield cache test and expanded the inter-body fixture
to three bodies so reused and fresh positive hits coexist; both are covered by
existing native/serial CI commands. The known stiff-contact derivative gate
remains ignored/unsatisfied. No analytic path was promoted.

The short native workload is still about 90× slower than realtime, with the
same 44 subdivisions and 2,743 Jacobian builds. This improves geometry work
inside residuals, not convergence. Coupled contact retry divergence, longer-run
accuracy and timestep refinement remain open. See the latest runtime audit and
`partial-geometry-*` evidence.

### Earlier: compiler layout reuse reduces full-robot runtime without changing solver work

Retained immutable port-index and local finite-difference row/column caches in
`sim-compile`. Port gathering no longer allocates an index vector per port on
every residual/probe. Local probes clear and inspect only their conservative
write rows, preserving the former global triplet traversal order. Geometry,
contact forces, perturbation sizes and solver tolerances are unchanged.

Three isolated alternating 16-worker pairs over the default full robot's first
100 ms give median runtime 10.35023→9.46437 seconds (8.56% lower). All profile
work counts agree. A separate full comparison matches all 51 exported frames
and all 291 captured solver attempts exactly, including stage states/rates,
residuals, contacts and original constraint diagnostics. The audit did not
truncate. This is preservation of the existing trajectory, not proof that its
physics or timestep accuracy is sufficient.

Native/serial compiler suites, 16 session tests, the 2,113-check independent
contact comparison, WASM build, and 21-frame real-browser parity/replay pass.
Added the local derivative regression to native and serial CI. The browser
fixture's maximum native difference is 2.71e-11 and its replay is exact; no
full-robot browser performance claim follows from that smaller fixture.

The robot still needs about 95 wall seconds per simulated second on this short
native workload. All 44 subdivisions and 2,743 Jacobian builds remain. Next work
must reduce expensive evaluations/rebuilds and diagnose the coupled contact
retry divergence; analytic derivatives remain experimental pending their full
accuracy and total-cost gates. See `runtime-audit.md` and `local-fd-gather-*`
evidence for commands, timings and artifact hashes.

### Earlier: exact bristle-column experiment rejected on total cost

Exact derivatives replace 87 numerical state probes and pass independent
loaded/shallow/separated contact checks, including flexible force propagation.
A larger fixture verifies deterministic column assembly at workers 1/2/4/8/16.
The physical force law is unchanged, and 26 native before/after exported frames
agree within the existing comparison tolerances (maximum difference 1.82e-9).

Nevertheless, the final paired benchmark is 1.31% slower, with 11 more Jacobian
builds and 168 more ordinary residual calls. Total iterations and subdivisions
are unchanged. Both full 50 ms independent comparisons still fail from 20 ms,
with maximum force error 22.066 N. A lower worst iteration count did not mean
less total work. Preserved the experiment and restored all four source/test
files; no new derivative path was promoted. Next work should target the coupled
contact pose/velocity derivatives and retry behavior rather than assuming more
exact columns automatically improve simulation. See the latest runtime-audit
section and `bristle-partials-*` evidence.

### Earlier: isolated 50 ms retry-grid divergence; target touchdown friction derivatives

Added bounded paired attempt capture after shared warmup and a reusable archived
stage checker. At identical 48 ms state/history, the candidate rejects the
49–49.5 ms trial that the reference accepts. Sharing 49.25/49.75 ms boundaries
makes all 2,312 suffix comparisons pass (maximum force difference 9.60e-9 N).
These boundaries are diagnostic, not production step control. The rejected
matrix still fails independent checks at three perturbation sizes.

A base-relative loop-position prototype fixes a demonstrated translation
cancellation and passes the short default gate, but does not eliminate the
later retry divergence. Preserved its patch/tests and restored all original
source files. The captured +X foot touchdown shows a stiff transient in the
existing bristle friction law, including large tangential forces at low normal
load. Next: exact derivatives of its affine bristle-state dependence, sharing
the existing force calculation and keeping the law unchanged, followed by full
accuracy/timing gates. Analytic promotion and timestep accuracy remain open.
See the latest runtime audit for complete evidence and limitations.

### Earlier: rate hook validated; structural combination fixes early retries but fails longer gate

Implemented the reusable complete-rate-derivative compiler hook while retaining
numerical state derivatives. The 64-state test halves perturbed rate work
(129→65 total residual calls), preserves state matrices exactly and agrees at
workers 1/2/4/8/16 and in serial builds. Fifteen runtime tests and the small
native/browser contact case pass (2,113 independent checks, 21 browser frames,
exact replay). Default full-robot outputs/events/work remain unchanged. The
articulated option stays false by default.

Rate-only full-robot derivatives regress to 289 subdivisions between 2 and 4 ms
and eventually fail at 35.574 ms. A new identical-point stage-correction audit
isolates the first failure at the 2.5–3 ms trial: rate-only and full hybrid
corrections both worsen the residual in the knees' angular closure equations.
Combining rates with existing topology-proved structural identities removes
that early burst and passes all 8,046 checks through 12 ms with zero subdivisions.
The original identity equations stay near roundoff in a separate stage audit.

The combined 100 ms test still fails 13,145/58,870 comparisons, first at 50 ms,
with maximum force difference 1.594 N and 30/27 candidate/reference subdivisions.
The numerical reference uses the same structural formulation; original-model
reaction agreement and timestep accuracy remain separate unpassed gates. The short 0.25 ms reference check also fails (maximum force difference 3.909 N),
so timestep consistency remains unresolved. There is no promoted analytic or
realtime result. Next: isolate the trial before the
50 ms divergence with identical prefix states and diagnose retry-grid versus
derivative error. See the latest runtime-audit sections and local
`rate-partials-*` evidence. The stage-correction example has a tested affine
reference and is included in CI; remote CI has not run.

### Earlier: relative-motion prototype rejected; isolate exact rate derivatives next

An equivalent relative-angular-motion formula passes three independent/general
motion tests and fixes an isolated common-rotation cancellation case. The full
robot nevertheless regresses: eight subdivisions through 12 ms, and the formerly
passing shared-grid comparison now fails 1,716/8,046 checks. Preserved the patch
and tests, restored all three original source files byte for byte, and rebuilt
the native runtime. The restored 8,046-check report matches the prior passing
report apart from timing. No part of the prototype remains enabled.

The existing opt-in hybrid Jacobian, using the original equations, accepts the
10.5 ms trial without retry, but its stage derivative check still has 161 resolved
mismatches and 119 inconclusive comparisons. This is a different warmup path and
does not isolate which hybrid block helps. Next experiment: a reusable compiler
hook for complete rate partials while retaining numerical state derivatives,
backed by the existing shared articulated rate-derivative implementation. That
must pass its own matrix, trajectory, native/WASM and total-cost gates; the full
hybrid path stays off. Details and `relative-axis-*`/`stage-hybrid-original*`
evidence are in the latest runtime-audit section.

### Earlier: actual-stage derivative audit exposes noisy knee rows

Added opt-in capture of the last fresh Jacobian point per attempted step and
the shared `check_implicit_jacobian` diagnostic. The new `sim-validate
stage-jacobian` command checks the actual Newton increment residual, including
theta/algebraic weights and 1/h rate factors. It requires the live base residual
to reproduce its captured value bit for bit and rejects changed contexts.

At the failed 10.5 ms solve, one compiled knee angular-row derivative is
0.001899289 while independent estimates at three stencil sizes remain near zero
(-1.48e-9 to 8.29e-8). The failed matrix and successful recovery matrices all
have resolved mismatches. Smaller stencils also increase inconclusive results;
contact-sensitive cases remain separate. This points to cancellation in
numerical constraint derivatives as a conditioning target, not a reason to
loosen tolerances or accept a converged solve as derivative validation.

All 23 selected audit/checker/runtime tests pass; native validation builds and
WASM checks. The default robot's pre-existing attempt fields through 12 ms are
unchanged. Next: test stable relative-motion closure expressions and derivatives
against these captured points, then whole trajectories and runtime. Full accuracy
and analytic promotion remain open. See the latest runtime-audit section and
local `stage-jacobian-*` evidence.

### Earlier: isolated the first retry-grid divergence

At 10–10.5 ms the compiled path subdivides while the independent whole-FD path
accepts a full step. Giving both the diagnostic 10.25 ms boundary makes all
8,046 checks through 12 ms pass (maximum force difference 2.244e-8 N).

Added opt-in `CompareConfig.shared_prefix_frames`, default zero. It replays and
verifies identical states/controller history before switching the reference's
derivatives and timestep. Both matrix caches are cleared; warmup is reported
separately and excluded from accuracy counts. The normal comparison is unchanged.

From identical 10 ms states, the two-millisecond suffix still fails when retry
grids differ (0.157809 N maximum force difference). With the shared boundary all
2,300 checks pass (3.767e-9 N). Halving only the reference timestep instead gives
0.334169 N difference, exposing timestep sensitivity. This does not yet validate
the 100 ms trajectory or explain derivative conditioning at the failed iterate.

All 14 runtime session tests pass, including shared-prefix controller continuity,
unmasked suffix timestep failures and zero-prefix compatibility. Native release
validation and WASM checks pass. No production physics/solver policy changed.
Next: inspect the actual failed stage Jacobian, especially knee reaction
directions, and continue timestep/error validation. Evidence and commands are in
the latest runtime-audit section and `coarse-first-divergence-*` local artifacts.

### Earlier: fresh-Jacobian budget experiment remains unpromoted

Tested a fresh matrix check for a negligible stale correction just before the
iteration budget expires. The prototype recovers the targeted 71.25 ms trial;
the 100 ms robot run drops from 44 to 38 subdivisions, 2,743 to 2,667 Jacobian
builds, and 25,292 to 24,580 ordinary residual evaluations. Later contact-force
components change by up to 0.04598 N, with matching contact identities.

Both before/after independent full-FD comparisons still fail 24,857/58,870
checks, beginning at 12 ms, before this prototype intervenes. Maximum force
disagreement remains 29.3485 N; the small later RMS improvement is insufficient
promotion evidence. No robust wall-time speedup is claimed from the single
experimental profile. All nine experimental convergence tests pass, including
exact-root recovery and rejection of a false small correction.

Restored the existing solver and rebuilt the runtime binaries. Its 20 ms control
matches the retained final frame, events, statistics and all work counts; all
seven retained convergence tests pass. Saved the prototype and its two tests as
`runs/full-robot/solver-performance/budget-refresh-experiment.patch`, along with
the captures and summary. Next priority is the earlier derivative/integration
divergence and comparison at identical saved states. The full accuracy and
analytic-promotion gates remain open; see the latest runtime-audit section.

### Earlier: longer solver validation and correction diagnostics

The default robot now has a 100 ms before/after stale-probe audit: all 291
attempted stages match exactly, including contact loads, original closure rows,
states and rates. Three alternating profile pairs retain a 6.8% runtime
reduction (11.054 → 10.301 s), with 6,216 fewer ordinary residual evaluations.
This is still about 103× slower than realtime for this interval.

The longer run reveals 45 failed nonlinear attempts and 44 subdivisions.
Failed attempts account for 1,837/6,002 iteration entries. Added optional,
bounded correction diagnostics to distinguish small raw residuals from small
unknown updates. A new analytic regression and both implicit-stage audit tests
pass; WASM checks. Re-running the entire capture reproduces every existing
field exactly. No convergence tolerance or refresh policy changed.

At 71.25 ms, late corrections satisfy ordinary bounds but miss the stricter
reused-Jacobian check; knee reaction multipliers dominate. A guarded fresh check
before exhausting the iteration budget is now an evidence-based next experiment,
not a promoted change. Full trajectory/timestep accuracy and stiff-contact
analytic promotion remain unresolved. See the latest section of
`examples/full-robot/runtime-audit.md` and local `stale-probes-long-*` evidence.

### Earlier: skip discarded stale-Jacobian probes

- A failed full trial with a reused matrix now refreshes immediately, eliminating
  backtracked candidates that the old solver always discarded. Fresh-matrix
  backtracking and final raw-residual verification remain intact. Regressions
  cover exact recovery, an unnecessary domain failure, and fresh backtracking.
- Default robot calls fall from 4,285 to 3,541: 744 discarded evaluations removed.
  Three paired runs improve mean time from 1.514 to 1.415 s (6.6% less) for the
  same 20 ms simulation. Finer numerical/hybrid runs improve 1.8%/2.0%.
- All final states, events, Jacobian builds, Newton iterations and subdivisions
  are unchanged. Both independent comparisons pass 12,766 checks with identical
  per-frame errors. The profiler records refresh counts and actual Newton settings.
- The selected solver/dynamics/robot/runtime suites pass 92 tests. WASM and both
  browser contact/replay gates pass. The known stiff-contact derivative gate,
  timestep accuracy and analytic promotion remain unresolved.

### Earlier: finer derivative batches reduce native runtime

- A worker sweep preserves exact results from one through sixteen workers.
  Shared scheduling now uses two-column derivative batches above two workers,
  retaining eight-column batches for small pools. Serial/WASM arithmetic is
  unchanged; the profiler records actual pool capacity and batch policy.
- Three default-scene pairs improve mean 20 ms simulation time from 1.722 to
  1.504 s (12.6% less). The finer shared-grid numerical case improves 9.2%, and
  hybrid improves 8.1% at sixteen workers. All final states, event traces and
  solver work counts remain identical.
- Both independent trajectory comparisons pass 12,766 checks with unchanged
  per-frame errors. Compiler and hybrid matrix tests now include sixteen workers;
  native/runtime tests, WASM build and both browser contact/replay gates pass.
- Ordinary residual evaluations now take ~0.50 s of the default run and ~0.95 s
  of the finer run. Reducing repeated residual work and rebuilds remains useful.
  The stiff-contact derivative gate, timestep accuracy and analytic promotion
  remain open; this scheduling change does not alter those acceptance criteria.

### Earlier: kinematic reuse helps single-worker execution

- Prepared numerical and hybrid evaluations now reuse complete kinematics when
  all motion and acceleration dependencies match bitwise. Bristle, reaction and
  temperature changes still compute their relevant loads/residuals. Cached
  joint axes are borrowed; the cache remains local to one Jacobian assembly.
- Two native single-worker pairs improve mean full-robot time from 6.242 to
  5.829 s for 20 ms simulated (6.6% less). With 16 workers, numerical averages
  ~1% less time and hybrid is unchanged; no robust parallel speedup is claimed.
- All paired final states, event traces and work counts match exactly. Both
  independent full-robot comparisons pass 12,766 checks with unchanged per-frame
  error summaries; the default scene also preserves its state/work counts.
- Native/serial/runtime tests and WASM build pass. Numerical and hybrid browser
  contact fixtures pass all 21 frames and exact replay. The stiff-contact
  derivative gate, timestep accuracy and analytic promotion remain open.

### Earlier: separate contact-stencil and roundoff failures

- The stiff SDF audit now records geometry, active sample identities and six
  reference stencil sizes for loaded, sliding and separated states. A -5 µm
  perturbation swaps active samples while the total contact count stays four.
- Derivative reports now expose coarse/fine slopes, stencil radius, truncation
  and roundoff estimates, plus counts by unresolved reason. At the loaded point,
  830 of 900 unresolved checks are roundoff-limited; 28 resolved mismatches remain.
  The diagnostics preserve every sweep classification and all tolerances.
- A centered-remainder prototype reduced local resolved mismatches but was 11%
  slower, added a subdivision and failed 375 full-robot trajectory comparisons.
  It was reverted. The previous contact-cache gain remains; hybrid stays opt-in.
- Independent checker, robot Jacobian and runtime session tests pass. The known
  ignored promotion gate still fails explicitly. Next: establish resolved,
  mode-preserving reference probes before accepting any derivative change.

### Earlier: hybrid contact reuse saves 13.7%, remains experimental

- Hybrid derivative workers now share the numerical path's dependency-checked
  contact/geometry preparation. Three paired full-robot runs fall from mean
  3.262 s to 2.816 s for 20 ms simulated time; assembly falls 28.5%.
  Final states, event traces and work counts match exactly.
- The independent comparison passes 12,766 checks on the shared grid, with
  per-frame error summaries identical to before reuse. A fresh comparison
  against numerical component derivatives still puts hybrid 2.1% slower.
- A new stiff SDF-pair derivative audit fails identically with reuse disabled.
  Its fixture is retained as an explicitly ignored promotion gate; neither
  tolerances nor physical properties were changed to obtain a pass. Diagnose
  stencil/cancellation and branch/geometry sensitivity before hybrid promotion.
- Native/serial regressions and WASM build pass. Real Chrome passes all 21
  contact-enabled hybrid frames and exact replay; independent fixture comparison
  passes 2,134 checks. Browser CI now includes this combination (not run remotely).
  Full-robot timestep accuracy, broader contact trajectories and analytic
  promotion remain open. See the latest runtime audit for commands and evidence.

### Earlier: shared-grid comparison isolates liftoff retries

- Added reusable, optional absolute step boundaries in `Simulation`, with no
  synthetic events or controller resets. The immutable schedule survives state
  restoration. Comparator configuration records shared boundaries for both
  runs; the profiler accepts and records the same list.
- At 31.25 µs, requesting 8.359375, 8.3671875 and 8.37109375 ms in both paths
  eliminates retries and passes all 12,766 full-robot comparisons. Maximum force
  difference is 3.23e-8 N, versus 0.507 N with different subdivision grids.
  These boundaries were selected from this recording; this is a diagnostic,
  not a general adaptive-step optimization or a timestep-accuracy promotion.
- Articulated hybrid derivatives also pass on that grid, but three paired runs
  take ~3.28 s versus ~2.80 s for numerical component derivatives (about 17%
  slower). Assembly costs more despite slightly fewer rebuilds. Exact motor
  derivatives add two retries and fail 1,011 checks (maximum force difference
  0.0247 N). Both analytic options remain off by default.
- Boundary, clock, event-order, snapshot and runtime tests pass; the new boundary
  cases are wired into CI and the WASM build passes. Next concrete optimization:
  the hybrid numerical remainder still calls uncached `sample_jacobian` for each
  perturbation, unlike the compiler path's prepared geometry/contact reuse.

### Earlier: reject false convergence and stop unproductive refreshes

- Newton acceptance now verifies each final residual against a fixed bound
  `absolute_tolerance + relative_tolerance * abs(initial_row_residual)`.
  Jacobian row scaling still conditions the solve but cannot loosen this check.
  Failed verification refreshes the Jacobian; repeated noncontracting failures
  return to timestep/branch recovery. Audits retain the actual residual bounds.
- The previously ignored no-root regression now passes normally. New tests
  verify stiff coupled equations with disparate row scales and successful fresh
  recovery after an inflated derivative. Relevant solver, integrator, robot,
  electrical, thermal and runtime tests pass; CI includes the regression.
- The default full-robot control keeps its final frame and solver counters
  exactly. In the 31.25 µs liftoff case the guard adds three necessary
  subdivisions. All 327 accepted trials in the 10 ms capture meet their bounds;
  maximum base force-balance error is 2.13e-9 N, versus the prior false 4.69 N
  acceptance. The bound is a residual-progress safeguard, not a calibrated
  per-equation physical accuracy budget.
- Stopping repeated unproductive refreshes removes 13 builds (552→539) and
  saves about 1.7% in two alternating pairs. Every accepted captured state/rate
  and the full-run final frame remain exact relative to the guarded version
  without this stopping rule. The fine run still costs about 2.95 s per 20 ms.
- Full-trajectory accuracy is still unresolved: the fine same-h comparison
  fails 1,408 checks with up to 0.507 N force difference and different subdivision
  histories. Both 21-frame contact-enabled browser cases pass native/WASM parity
  and exact replay; 86 selected native tests pass. Next: timestep/event handling at
  liftoff and a trustworthy refined reference before performance promotion.

### Earlier: false convergence at floor liftoff

- Same-h 31.25 µs captures first separate at 8.375 ms, when the last floor
  contact disappears. Both solves report `noise_floor_accept`, but their final
  base force-balance residuals are 4.694 N (compiled derivatives) and 1.354 N
  (whole numerical derivatives). Row scales of 1.69e-11 / 5.18e-14 hide these
  residuals in the scaled norm. No event-matrix reuse is enabled in either run.
- Initially added a deliberately ignored known-failure regression in
  `crates/sim-solve/tests/convergence.rs`: a jump residual with |F(x)|≥1 and
  provably no root is accepted after one FD Newton correction. Explicitly
  running it reproduced the failure before the guard above. It is now active
  and passing. Physical residual tolerances remain distinct from linear-system
  conditioning.

### Latest: timestep refinement exposes a sampled-encoder branch change

- Added per-unit maximum/RMS errors, per-frame summaries and the first failing
  reporting frame to the shared trajectory comparator. These include passing
  samples; the previous bounded worst-error list could hide divergence onset.
  Unit and runtime tests pass; the library test is included in CI.
- Full-robot 20 ms comparisons now cover 0.5→0.25→0.125→0.0625→0.03125→0.015625 ms.
  Errors are not monotonic: the 0.125→0.0625 ms pair differs by up to 0.165 mm
  in position and 34.8 N in force. Same-timestep independent numerical comparisons
  pass at 0.25, 0.125 and 0.0625 ms, in addition to the earlier 0.5 ms gate.
- Solve-point capture identifies the first differing encoder count at 5 ms:
  the +X worm angle is 0.000774610 versus 0.000716746 rad, straddling the
  0.000766990 rad half-count boundary. A different command enters the firmware
  queue and is applied at 6 ms. Encoder quantization, gains and CAD physics remain
  unchanged. This is an observed controller branch difference, not permission
  to dismiss subsequent physical-force errors.
- The finest pair reduces maximum position/force differences to 7.34 µm / 2.99 N,
  but still fails the unchanged comparison gate. At 31.25 µs, the same-h
  derivative comparison also fails (0.585 N maximum force difference), beginning
  at the 10 ms reporting frame and later developing a reference-only retry.
  Turning off event matrix reuse reproduces the same failures and force error.
  The finest runs therefore are not yet a trustworthy numerical reference.
  No timestep or experimental
  derivative promotion. Next: event-aware load/impulse and controller tick
  comparisons, with tighter nonlinear references before accepting accuracy gains.

### Earlier: guarded event-search Jacobian reuse (experimental)

- Optional `event_jacobian_reuse` permits an existing factorization during event
  location only when its actual build timestep is within 10% of the trial step.
  Current residuals and stale-matrix convergence/refresh safeguards still apply;
  mode changes invalidate the cache. Default remains off.
- Three alternating full-robot pairs improve 1.512/1.507/1.506 seconds to
  1.266/1.273/1.257 seconds per 20 ms: about 16% less time, with rebuilds
  333→270 and zero subdivisions. This is still about 63× slower than real time.
- All 12,774 numerical-reference trajectory checks pass with reuse explicitly
  disabled in the reference. Both complete option sets are saved in comparisons.
  The default full-robot frame and solver counters remain exactly unchanged.
- Event regression checks the actual matrix-age bound, independent analytic
  event/final state and fresh rebuild after a mode change. Runtime tests and all
  21 contact-enabled native/WASM frames pass, with exact browser replay.
  Added the experimental browser/reference case to CI; remote CI not run.
- Not promoted: finer-timestep accuracy remains unresolved, and this path is
  slower than unsplit backlash. Real-time acceptance must measure full-robot
  throughput and input latency at validated accuracy, separately from rendering.

### Earlier: experimental backlash events and physical-event ordering

- Fixed a reproduced generic event-order bug: simultaneous trial crossings were
  handled in guard declaration order. The integrator now locates them and applies
  the earliest physical event, then rechecks which later events remain valid.
- Added opt-in `backlash_events` / registry `backlash.events`. Motor engagement
  and release become explicit events with the original branch spring/damping
  laws. A held mode supports smooth trial continuation during root location;
  no velocity impulse or force smoothing. Defaults remain unchanged.
- The isolated missing-root fixture now crosses without retries and converges
  at first order toward an independent analytic solution. Positive/negative
  engagement, release, actual-shaft initialization and unforced mechanical-energy
  dissipation pass with numerical and exact motor derivatives.
- With certified knee identities, full-robot 0.5 ms retries fall 2→0 and the
  independent numerical-derivative comparison passes all 12,774 values. However,
  runtime rises ~0.78→1.46 s per 20 ms: 333 rebuilds versus 176, with about half
  the total time spent locating events. The original constraint formulation
  still has five retries. Four-times-finer timestep comparison still fails.
- Native/serial tests and the 21-frame contact-enabled browser gate pass; CI
  includes the event-order regression, motor events and browser experiment.
  No experimental promotion. Next: reduce Jacobian rebuilds during event location
  while retaining fresh-residual convergence checks and the accuracy gates.

### Earlier: stable joint-band contact classification and browser parity

- Native/WASM solve-point capture identified the first divergence at ~35.6 ms:
  two samples lie exactly on a 10 mm joint-exclusion radius. Roundoff toggled
  their contact membership, producing false force jumps and ~6e9 N/rad FD slopes.
- Added a shared strict-interior predicate with a coordinate-scaled roundoff
  guard. Boundary samples remain eligible for collision; genuinely interior
  samples remain excluded. No fixed clearance or solver-tolerance relaxation.
- The existing failing contact fixture now passes all 21 native/browser frames
  at unchanged 1e-7 tolerance (maximum difference 2.71e-11), with exact replay.
  Native retries fall 12→0, browser retries 25→0, and native rebuilds 3042→117.
  Three paired native runs improve ~1.03→0.19 s for 0.4 simulated seconds (~5.4×).
- Independent whole-residual numerical comparison passes all 2113 checks.
  Added rotating-boundary regressions, runtime trajectory/no-retry regression,
  and a required contact-enabled browser CI gate. Local gates pass; remote CI
  has not been run. All experimental derivative flags remain disabled here.
- Browser worker now exposes bounded implicit-attempt diagnostics through the
  shared Rust session API. Native CLI optionally exports all reporting frames;
  browser validation compares the complete supplied trace and retains failure
  evidence. Default diagnostic capture remains off.
- Full robot's default 20 ms frame/counters remain unchanged by the band fix.
  Its backlash retries, full-range constraint checks and timestep accuracy still
  require work; this fixture speedup is not a full-robot speedup claim.

### Earlier: fixed-pose contact geometry reuse

- Split inter-body SDF geometry from velocity-dependent contact loads. Prepared
  derivative queries reuse distances/normals only when every link position and
  rotation matches bit-for-bit; damping, friction and bristle rates remain fresh.
  Pose changes query geometry again. Cache lifetime is one linearization, with
  immutable data shared by native derivative workers and the same serial/WASM path.
- Three alternating paired default-robot runs take 2.134/2.139/2.148 s before and
  1.768/1.747/1.759 s after for 20 ms simulated: about 1.22×, or 18% less wall time.
  Jacobian assembly falls about 30%. Every work count remains identical.
- All ten 2–20 ms reporting frames, solver counters and event traces match the
  previous native executable exactly. The existing fine-step independent
  numerical comparison still passes all 12,638 checks with the same error ratio.
- Loaded SDF-pair, friction/damping, appearance/disappearance and all local-input
  cache checks pass bit-for-bit; native and serial-feature suites pass. Existing
  no-contact native/WASM parity and replay pass.
- A new contact-enabled browser diagnostic fails strict native/WASM parity.
  Reconstructing the previous contact path in an isolated WASM build gives the
  exact same browser initial/final/replay results; previous/new native frames also
  match. This is not caused by the new cache, but remains an open acceptance issue.
  Evidence is retained on failure; no tolerance relaxation or passing contact
  browser CI claim. See the runtime audit for reproduction.

### Earlier: isolated backlash failure and experimental motor derivatives

- Reproduced the false ~1e6 N·m/rad derivative with the actual motor component.
  A second isolated positive-inertia test proves that its discontinuous damping
  law can leave a backward-Euler step without any admissible root. This establishes
  a mechanism, not proof that the complete robot's failed step has no root.
- Added opt-in `analytic_motor_jacobian` / registry `jacobian.analytic`, preserving
  the physical residual. All local partials pass independent perturbation sweeps
  in smooth modes; switching surfaces explicitly have no claimed classical derivative.
- At 0.125 ms, the full robot passes all 12,638 comparisons against independent
  whole-residual numerical derivatives. At 0.5 ms it fails 1,074 comparisons and
  the identity path still retries twice. All experimental flags remain off.
- Paired 0.125 ms runs reduce component FD calls 229,964 → 187,720, but runtime
  remains ~1.17 s per 20 ms simulated. Cheap motor probes are not the main cost;
  no wall-time speedup or timestep-accuracy promotion is claimed.
- Five motor regressions, eight runtime tests, release WASM build and real browser
  pendulum replay/parity pass. CI now includes the motor tests and browser mode.
- Next: investigate mode-aware backlash stepping without silently changing its
  constitutive law, and target verified sparsity/reuse in the expensive articulated
  residual. Full-range constraint and timestep-refinement gates remain open.

### Earlier: exact implicit stages and identified retry causes

- Added bounded opt-in solve-point and Newton refresh/failure diagnostics, plus
  `sim-validate steps scene.json [frames] [attempt_limit]`. Physical reconstruction
  uses snapshot states AND rates; no reporting-time acceleration approximation.
- Corrected the earlier midpoint lead: this robot uses backward Euler throughout.
  Stage times are endpoints, so midpoint offsets do not explain its disagreement.
- At 0.125 ms over the first 2 ms, the original whole-residual numerical reference
  takes 195 Jacobian builds and five failed attempts; certified identities reduce
  that to 14 builds and no failures. Angular CFM rows dominate the false coupling.
- The identity path's remaining coarse-step retries at 14.5/14.75 ms lie 7–10 nrad
  inside the −X foot servo backlash edge. A ~10 nrad FD probe crosses the damping
  jump, producing a spurious ~1e6 N·m/rad derivative. Next: isolated reproducer and
  mode-local/exact derivative comparison, preserving the declared physical law.
- Release solver/dynamics/runtime tests and WASM build pass. Full-robot disabled
  capture matches previous final states and all work counters exactly, with no
  material overhead in paired runs. No experimental promotion yet.

### Earlier: original closure, rank and physical reaction audit

- Added a reusable, diagnostic-only original closure and scaled QR/SVD audit.
  Includes all bilateral rows, units, thresholds, selected directions and Gᵀλ;
  it never removes equations or changes runtime behavior.
- Robot starts with 28 rows / 34 velocities / rank 16. Weak extra singular values
  appear off the closure manifold; strict-threshold QR/SVD ranks sometimes differ.
  Preserve that uncertainty rather than using sampled rank to delete rows.
- At 0.125 ms over 20 ms, both baseline and identity paths retain original loop
  position closure within 2.93 nm and velocity closure within 3.72e-7 m/s.
  Original angular identity rows remain at numerical roundoff. Long-run/full-range
  and timestep-refinement promotion gates remain open.
- Four-bar motion/toggle, independent configuration derivatives, modal/floating
  velocity support, original stabilization terms and physical generalized-reaction
  checks pass. CI covers the new diagnostics; release WASM compilation passes.
- CLI: `sim-validate constraints scene.json [frames] [rank_config.json]`.
  Reports explicitly distinguish endpoint position/velocity from currently
  approximate diagnostic accelerations. At the first 2 ms divergence, combined
  +X foot reaction differs by 0.02647 N and thigh torque by 0.002178 N·m; the
  difference survives multiplier combination. Capturing exact solve points and
  retry causes is next; time placement remains a caveat.

### Earlier: sampling-clock correctness and reprofile

- Reproduced missed exact-endpoint ticks. Added reusable scheduled deadlines in
  the behavior/compiler/integrator APIs; migrated motor, IMU, controller and
  sensor sampling clocks. Preserved ordinary root events and fault-guard semantics.
- Boundary tests pass across four physics steps, with off-grid/simultaneous clocks,
  root ordering, invalid clocks and protection against roundoff-sized implicit
  steps. Controller and sampled-sensor endpoint regressions pass; added CI coverage.
- Current full robot: 242 events at every tested timestep. Default 20 ms runs take
  3.211 / 3.295 s, 380 Jacobians, six subdivisions. Opt-in knee identities take
  1.451 / 1.507 s, 176 Jacobians, two subdivisions. Roughly 55% remains Jacobian
  assembly and 16% factorization; no solver/engine replacement is justified.
- At 0.125 ms the identity path passes all 12638 independent numerical-derivative
  comparisons. Refinement to 0.0625 ms still fails (4457 mismatches), now with
  matching event counts. No promotion. Midpoint versus endpoint timing of
  algebraic sensor readings/reactions needs explicit comparison treatment.
- The default formulation also fails equal-step 0.125 ms numerical-reference
  comparison (1844 mismatches). The reference subdivides five times before 2 ms;
  candidate does not. First-frame +X foot world-X contact force differs by 0.02670 N.
  This is a physical-load disagreement requiring diagnosis, not multiplier gauge.
- Corrected profiler terminology: session actions/reporting every 2 ms, Rhai
  controller every 20 ms, firmware every 1 ms. Added event timelines to profiles.
- Native/WASM builds and real Chromium pendulum replay pass for default and
  hybrid paths (max native/WASM difference below 3.7e-16). These are portability
  checks, not full-robot acceptance. Live CAD and robot physical inputs untouched.
- Next: diagnose the first physical force divergence with correct time/frame
  conventions, report every original closure equation and rank histories, then
  test verified local coloring and guarded refresh policies. Both experimental
  options remain off. Full trajectory/accuracy promotion gates remain open.


User objective: reuse geometry/contact calculations, reduce convergence retries,
and distribute the large derivative workload across workers. Analytic derivatives
must earn promotion through full-trajectory accuracy and timing.

- [x] Native FD columns split across workers, ordered assembly, serial/WASM fallback.
- [x] Shared prepared-residual hook and dependency-checked articulated contact reuse.
- [x] Focused exactness tests, release suites, serial compiler, WASM/browser check.
- [x] Full robot 20 ms benchmark: 20.406 s one-worker uncached control → 3.902 s
      with reuse and 16 workers; final frame exactly equals prior reference.
- [ ] Reduce convergence work further: after explicit clock scheduling, the
      default 20 ms run needs 380 Jacobians, 938 Newton iterations and six
      subdivisions. Structural knee handling remains experimental.
- [ ] Full-trajectory accuracy, timestep refinement and timing promotion gates.
      Hybrid remains opt-in; no full-robot hybrid promotion is justified.

Earlier continuation evidence (superseded by the clock results below):

- Added opt-in `loop.structural_identities` / recorded runtime
  `structural_loop_identities` (both default false). Conservative exact tree-axis
  certification rejects nonparallel hinges, independent bases and flex boundaries.
  Certified angular rows retain CFM and state lanes but avoid cancellation.
- Prototype 20 ms run: 1.624 s, 250 Jacobians, 1139 Newton iterations, three
  subdivisions. This is experimental: changes to subdivision alter the trajectory.
- Independent derivative comparisons at 0.5/0.25/0.125 ms find respectively
  1052/1/0 mismatches. At 0.125 ms all 12638 values pass, with zero subdivisions.
  Hybrid at 0.125 ms also passes all 12638 values.
- **Refinement remains a failure:** 0.125 versus 0.0625 ms produces 4434
  mismatches and different event counts (230 versus 242). The extra block of
  twelve events appears by 12 ms. Inspect firmware clock/guard endpoint semantics
  in `sim-dynamics::step_inner` / `locate_event_inner` and motor firmware guards.
  Do not equate equal-step derivative agreement with time-converged physics.
- Comparison JSON now exposes per-frame accuracy and cumulative solver work.
- Hybrid remainder now has ordered native parallel columns too. Full-robot
  hybrid 20 ms: 11.04 s before versus 5.80 s afterward at 16 workers; one worker
  afterward takes 26.98 s. Exact same final frames and work counts (400 J, six
  subdivisions). Still slower than the numerical identity prototype; no promotion.
- Five-second four-bar closure/trajectory test passes in baseline, identity and
  hybrid modes; loaded hybrid triplets match exactly across 1/2/4/8 workers.
  Domain/runtime release suites pass. CI includes the new focused cases.
- Research handoff prompt: `examples/full-robot/optimization-research-brief.md`.

The current default remains parallel FD plus contact reuse; both experimental
flags stay off. See the latest clock and validation results below.

Latest user research steering: prioritize fewer residual evaluations, verified
constraint independence and fewer Jacobian rebuilds. Reprofile before considering
any new engine/linear solver. Primary PETSc coloring and IDA method documentation
checked; links and applicable caveats are in the audit.

- Added batched component-FD residual counters and reduced structural-pattern
  diagnostics to `sim-profile`. Current pattern: n=600, 50476 entries, 312 colors.
  Greedy coloring already exists in `sim-dynamics/src/jacobian.rs`; it is not the
  active per-behavior assembly path. Measure/refine local structural dependencies
  before replacing cheap local probes with colored whole-system evaluations.
- Paired reprofiles: accepted path 2.644/2.683 s, identity experiment 1.393/1.388 s;
  both match their prior final frames exactly, same work counts. Updated shares:
  Jacobian assembly ~58–59%, other residuals ~23–24%, factorization ~16%.
  Baseline/identity component FD residual calls: 565880/301000; other full residual
  calls: 5336/2629. Timing changes from earlier observations reflect run conditions,
  not another credited optimization. Evidence `reprofile-{baseline,identities}-{1,2}.json`.
- Modified Newton, cross-step cache checks, row equilibration and backtracking
  already exist. Instrument contraction/refresh reasons rather than duplicating
  them. Clock endpoint consistency is the immediate reference-integrity issue.
- Serial domain Jacobian tests and runtime tests pass; release WASM build passes.
  Chrome hybrid pendulum check passes (native/WASM difference 4.55e-18, exact
  replay, 0.4 simulated seconds in 154 ms). This is not a full-robot browser gate.

See `examples/full-robot/runtime-audit.md` for implementation, timings and limits.
This performance goal remains incomplete. The retained changes preserve the
baseline numerical equations/perturbations; no tolerances or physics were relaxed.

## Broader playground backlog

- [ ] Versioned CAD baseline, physical export, provenance and explicit units.
- [ ] Shared native/WASM runtime and observation/action/environment contracts.
- [ ] Twelve actuators, all transmissions, closed linkages and contact validated.
- [ ] Floating robot stands, walks, turns and stops under a controller.
- [ ] Rust/Rhai controller execution; Python remains on the CAD side.
- [ ] Native and browser viewers: responsive rendering, follow/orbit cameras,
      pause/reset, diagnostics and recording/replay.
- [ ] CAD/UI and automation export a complete static-hostable web bundle.
- [ ] CI: reproducibility, accuracy tolerances, native/WASM agreement, real browser
      interaction and explicit performance budgets.
- [ ] Delivered native playground, browser bundle, documentation and calibration
      limitations, verified against the full objective.

## Latest user steering: audit actual runtime costs before more Jacobian work

Completed an audit without changing simulation physics or enabling the hybrid.
See `examples/full-robot/runtime-audit.md` and the new native `sim-profile` CLI.
Two isolated full-robot 20 ms runs took 20.67 / 19.09 s, with identical final
frames matching the previous numerical baseline. Jacobian assembly is 92.9%
(17.73 s in the second run), other residuals 4.15%, factorization 2.6%; all
controller/firmware/event callbacks together take ~1.1 ms. Fixed-state contact
makes whole articulated evaluation 6.6–7.5× more expensive. Frame extraction is
~0.13 ms and JSON ~0.02 ms, not a present bottleneck. Session build is ~1.1 s.

Both runs: 40 nominal physics steps, 100 Newton calls, 1326 iterations, 470 fresh
Jacobians, 9 subdivisions, 230 events, no outer slice retries. First controller
frame has 13 Jacobian builds, final frame 117; per-build cost ~37–40 ms stays
similar. A separate five-second native stack sample finds one worker doing the
articulated work and the other 15 workers ~99% waiting. Existing parallelism is
across behaviors; the robot's large per-input differentiation loop is serial.
Hardware confirmed Intel i9-9980HK, 8 cores/16 threads (no Rosetta assumption).

Audit evidence: `runs/full-robot/runtime-audit/{profile.json,profile-repeat.json,
stacks.txt,stack-summary.json}`. CLI built and ran, zero frame count rejected,
all final frames match the prior numerical trajectory exactly. No existing
physics implementation was edited in this audit. Sampling/benchmark processes
are terminal. CAD UI/rendering was not measured. No new whole-robot accuracy or
real-time claim is warranted. The user asked to step back from Jacobian changes;
use this priority order for subsequent work: eliminate repeated geometry/contact
work, diagnose convergence/work amplification, balance the heavy work across
cores, then clean up reporting. Keep accuracy and end-to-end timing as gates.

## Latest steering: analytic Jacobians with independent validation

The user wants analytic derivatives and a quick mechanism to compare them with
numerical derivatives/solutions. An experimental articulated hybrid is now
implemented, but **must remain opt-in**: its longer robot run regressed badly.
No end-to-end analytic speedup has been established.

Implemented this continuation:

- `Behavior::jacobian_at(view, state_rates, out)` defaults to the old hook;
  the compiler supplies the actual residual rates. Existing components continue
  to work unchanged.
- `articulated/jacobian.rs`: exact inertial rate columns via the shared
  Newton–Euler operator with velocity/reaction/load biases removed; exact loop
  reaction force propagation (including modal boundary loading), ideal gear
  equations, kinematic identities and held IMU/rate identities. Configuration,
  geometric loop closure and contact state derivatives remain local FD. This is
  explicitly hybrid, and profiler labels now say “slots supplied”.
- Shared component `jacobian.hybrid` and recorded runtime scene option
  `hybrid_articulated_jacobian` are **false by default**. Setting true exercises
  the new path; global `numerical_jacobian:true` still bypasses it for the
  independent reference. Both choices are retained in recordings.
- Three new domain tests independently audit local residual columns/directions,
  reactions as large as 1e8, and exact rate/reaction columns with flex boundaries,
  revolute/prismatic joints, cable loads, held IMU signals and confirmed active
  contact at nonzero velocities/accelerations. All pass. The full local hybrid
  audit uses loop alpha=1 to resolve the numerical remainder at default checker
  tolerances; exact-column tests use the default production alpha=100. Do not
  claim all geometric derivatives pass at production alpha.
- Four runtime session tests include an explicit hybrid-vs-numerical pendulum
  replay, all passing. The release robot/compiler/dynamics/runtime suites pass.
  Workspace check and release WASM build pass. CI now includes the new domain
  tests and a hybrid native/WASM replay gate (not yet run remotely).
- Hybrid full robot audit at 2 ms reports 165 mismatches / 173 inconclusive
  out of 722400 comparisons, down from the prior 177 / 173. Equal-step robot
  replay still passes all 2296 comparisons over this tiny interval. Moving
  pendulum audit and 2037-value trajectory comparison also pass.
- **Performance promotion rejected:** the hybrid 20 ms run completed its first
  2 ms frame in 0.764 s, then failed to finish the next frame after approximately
  two minutes. Interrupted it; evidence in `runs/full-robot/hybrid-20ms.log` and
  `hybrid-20ms-status.json`. This is a convergence regression, not an accepted
  optimization. Its precise cause is not localized; numerical geometric loop
  errors and motor/contact branch switching remain candidates. Do not report
  this as a successful 20 ms trajectory or a measured speedup.
- Explicit experimental scenes/recordings are saved as `runs/*/hybrid.scene.json`
  and `hybrid.recording.json` so subsequent comparisons record the opt-in flag.

Final verification this continuation:

- The default numerical 20 ms run completed in 19.153 s wall (17.545 s physics,
  16.346 s Jacobian assembly, 470 fresh Jacobians / 1326 Newton iterations).
  Its full final frame is **exactly equal** to the previously saved
  `floating-classified-20ms-frame.json`. The earlier classified timing included
  concurrent builds; this new timing is not evidence of an analytic speedup.
  Evidence: `runs/full-robot/numerical-default-20ms*`.
- The explicitly enabled hybrid pendulum passed real Chromium native/WASM
  parity (maximum difference 4.554e-18), exact recording replay and worker
  responsiveness: 0.4 simulated seconds in 158.4 ms, 15 main-thread heartbeats.
  Evidence: `runs/interactive/hybrid-browser-report.json`.
- Reran CLI checks with scenes/recordings that explicitly store the experimental
  flag. Pendulum checks pass; full robot tiny trajectory comparison passes;
  full robot derivative audit still fails 165 mismatch / 173 inconclusive.
- Latest logs: `/tmp/hybrid-optin-tests.log` (all relevant release suites),
  `/tmp/hybrid-final-test.log` (expanded prismatic/flex/contact test),
  `/tmp/hybrid-workspace-check.log` (whole workspace),
  `/tmp/hybrid-wasm-build.log` (release WASM). `git diff --check` passes.
  All tests/builds/benchmarks are terminal; no ongoing solver job was left.

Next: localize the second-frame convergence failure using the saved full robot
and independent checker, then derive geometric loop Jacobians. Resolve timestep
sensitivity too. Preserve all native/browser/controller/export scope below.

Completed validation infrastructure:

- `sim-dynamics::jacobian_check`: central residual stencils at h and h/2, full
  state/rate columns and deterministic mixed directions. It detects unresolved
  references, one-sided branch boundaries and nonlinear directional superposition
  at intersecting branches. Inconclusive checks cannot pass. Five focused tests
  include an injected derivative sign bug and intentionally wrong sparsity.
- `Simulation::set_numerical_jacobian(true)` bypasses provided derivatives AND
  sparsity, clearing cached factorizations. `BuildOptions.numerical_jacobian`
  records this reference mode for native/WASM sessions.
- `sim-validate jacobian SCENE AFTER_FRAMES [CONFIG]` emits named, unit-labelled
  coordinates and separate retained mismatch/inconclusive details.
- `sim-validate compare RECORDING [CONFIG]` replays the same scene/seed/actions
  through provided and numerical derivative paths, comparing every stored state,
  poses, energy and aggregate contact force/depth every frame. Repeated node names
  remain distinct and include connected component names. Optional
  `reference_substeps:2` checks timestep sensitivity without changing the policy.
- Moving pendulum point: 5,616 derivative comparisons pass. Startup is correctly
  inconclusive at the bridge's intersecting clamp branches. Recorded pendulum
  trajectory: 2,037 comparisons pass at equal timesteps. Commands, semantics and
  tolerances are in `examples/interactive/README.md`; CLI gates are wired into
  `.github/workflows/browser.yml` but have not run on GitHub yet.
- Full robot at 2 ms: 722,400 derivative comparisons report 177 mismatches and
  173 inconclusive entries (`runs/full-robot/jacobian-check.json`). These are
  disagreements with the chosen numerical stencil, not proof every flagged
  formula is wrong. Loop rows show cancellation-sensitive numerical derivatives;
  contact and bridge switching need branch-aware operating points/scales.
- Full robot first 2 ms trajectory comparison passes 2,296 values at default
  tolerances (`runs/full-robot/numerical-trajectory-comparison.json`). This tiny
  interval does not establish standing, long-run accuracy or real-time speed.
- Pendulum h versus h/2 comparison FAILS with 437 out-of-tolerance values,
  including rotor-speed differences (`runs/interactive/half-step-comparison.json`).
  Do not claim timestep convergence from native/WASM agreement or equal-step
  derivative agreement. This is an additional required fidelity investigation.

Also corrected articulated rate reads: copying all state rates falsely marked
constraint multipliers and fixed bases as differential. They now remain
algebraic; closure and transmission classification regressions pass. The release
robot/dynamics/compiler/runtime suites passed. Browser replay/parity still passes
on the portability benchmark (maximum difference below 8e-18, 178 ms for 0.4 s).

A concrete bridge discrepancy is now explained from the saved audit point:
`+Y` foot-slide bridge has command 0, current -2.980220533e-8 A, supply 11.1 V,
and on-resistance 0.05 ohm. Its positive-command voltage clamp changes branch
around command 1.34e-10. The component FD perturbation (1e-8) and central
reference stencil (1e-5) cross that nearby switch differently. Their measured
slopes (-10.95099 vs -11.09983) are secants across branches, not a disagreement
between two analytic formulas. The true operating point is nonsmooth. Audit
reports now save states/rates as well as named coordinates. Generic numerical
branch detection is heuristic; some of the 177 mismatches can require this
triage and smaller/scaled stencils or explicit active-branch metadata.

Next: derive exact/hybrid articulated Jacobians, starting with the constraint and
inertial structure. Existing `Behavior::jacobian(View)` does not expose state
rates, so nonlinear inertial derivatives may need a backwards-compatible richer
linearization context. Compare each contribution against the numerical checker
at interior operating points and replay trajectories; investigate timestep error.
Keep all original playground/controller/export requirements below active.

## Verified progress

- CAD revision 1357 is preserved in `examples/full-robot/baseline/`, committed
  locally as `5c4c1a7`, with SHA-256 provenance. The live CAD remains untouched.
- Full floating physical export: `runs/full-robot/floating.simrobot.json` contains
  29 merged links, 105 joint records, twelve actuators and eight transmissions.
  Export uses cached derivations and optional pinned Embree acceleration. The
  formal baseline export completed in 139 seconds and records full derivation identity.
- `sim-runtime` provides shared physical composition and Rust/Rhai sessions;
  `sim-web` runs those sessions in a browser worker. Sessions support bounded
  inputs, reset and seeded action recording/replay. Integration with the existing
  environment trait and complete state lifetime management remain open.
- Real Chromium test of the motorized pendulum passed: native/WASM numeric
  difference below 2e-18; 0.4 simulated seconds took 266 ms in the browser; replay exact.
  Evidence: `runs/interactive/browser-runtime-report.json`. This is a portability
  gate, not acceptance of full robot dynamics or performance.
- Added translational encoder/velocity components with declared units and Rhai
  discovery. Robot and sensing domain suites passed.
- Contact sampling now preserves geometric extrema, including the thin foot tip.
  A full floating robot run detects contact, but does not yet establish standing.

## Current bottleneck and remaining work

The original release run took 195.7 wall seconds for 0.020 simulated seconds
(600 unknowns). Smaller numerical derivative perturbations and conservative
contact pair filtering reduce that to 39.6 seconds on the same scene. Fresh
Jacobians fell from 2,236 to 808, but assembly still consumes 93% of wall time.
Evidence: `runs/full-robot/floating-session.log` and
`runs/full-robot/floating-optimized-20ms.log`. Both runs finish with no solver
error; their final contact counts differ, so longer-run accuracy and timestep
sensitivity remain unproven. This is still ~2,000 times slower than real time.

The new shallow-contact derivative regression verifies normal stiffness at
0.1 micrometre penetration. A moving-bounds regression verifies conservative
pair rejection and stable ordering. With derivative settings held fixed, the
full robot frame after 4 ms matches exhaustive pair search exactly. Updated
robot domain tests (21), compiler/sensing/session tests and real browser replay
pass. Native/WASM agreement is checked on the motorized pendulum, not yet on the
complete robot.

`examples/full-robot/export.py --scene PATH` now packages the exported CAD model
and existing Rhai hold policy into the shared runtime scene format. The formal
baseline reproduction completed separately from the live CAD in 139 seconds; its
log is `runs/full-robot/reproduced-export.log`, its model is
`runs/full-robot/reproduced.simrobot.json`, and its scene is
`runs/full-robot/reproduced.scene.json`. The physical data matches the earlier
export exactly (excluding provenance); CAD SHA-256 verifies against the committed
baseline. Its 4 ms Rust/Rhai frame also matches the earlier commissioning frame
exactly, with three contacts and no solver error.

Next solver work should address the articulated element's expensive derivative
assembly and repeated contact convergence retries. Do not disable contact or
substitute kinematic animation to satisfy performance requirements.

There are no completed native/browser rendered playgrounds, full bundle export,
or standing/walking/turning controller yet. CAD sensor ownership, material
calibration, all mechanism validation, complete environment contracts and full
robot CI gates remain required. Existing automatic ideal joint measurements must
not be presented as declared hardware sensors. The existing CAD motion/video
changes predate this goal and must be preserved.

Additional audit items: outer experiment cache dependency identity does not yet
include optional Embree installation/version (inner collision keys do); runtime
model registration and dynamic string lifetimes need cleanup for repeated resets;
actuator electrical constants/gains remain estimated and need stall/no-load and
energy accounting tests before full actuator acceptance.
