# Horizontal swing and asymmetric stance development

The 3.75 mm/s command now completes 13 qualified swings in the 24-second
forward/stop case at both 20 ms and 5 ms physics steps. No controller in these
30 trials passes the complete acceptance gate. This is a short-run geometry
milestone, not a validated sustained-speed or browser operating envelope.

The shared planner optionally distributes horizontal foot travel across raise
and lower while retaining the original vertical clearance, landing checks and
support gates. The original trajectory remains the default. The focused Rust
test checks clearance, support feet, phase progression, endpoint identity and
horizontal continuity through the apex.

`plan.json` / `status.json` preserve 18 initial trials. Extending the horizontal
trajectory alone does not solve the forward support or mechanism workspace
limits. `cycle-plan.json` / `cycle-status.json` preserve four rear-support
changes: 3.75 mm/s reaches a later rear gear/pulley reference collision, while
5 mm/s produces observed rear overlap. `asymmetric-plan.json` /
`asymmetric-status.json` preserve eight tests of opposite rear stance shifts
and a rear-first order. Moving the rear stance forward, with the original
order, clears the 3.75 mm/s short case. Rear-first ordering introduces a
pulley/chassis reference collision; 5 mm/s still produces rear overlap.

| 3.75 mm/s original order | 20 ms | 5 ms |
|---|---:|---:|
| Qualified swings | 13/13 | 13/13 |
| Final position error | 3.639 mm | 2.529 mm |
| Final yaw error | 0.001061 rad | 0.000544 rad |
| Position gate | Fail (>1 mm) | Fail (>1 mm) |

The 20 ms run travels 63.70 mm net over the episode. Regression of actual body
motion over the 4.02–16.8 s commanded window gives 3.264 mm/s. The short window
and large cyclic weight shifts make this insufficient to claim sustained
speed. Sampled positive shaft work is 4.009 J; this excludes electrical losses
and is not calibrated hardware energy. Native throughput was 2.65× with
24.29 ms transition p95; native timing is not browser acceptance.

The paired trajectory comparison fails the predeclared numerical screens:
maximum foot difference is 2.202 mm against 1 mm, and maximum body difference
is 2.109 mm against 0.5 mm. Preserving all swings under refinement is useful
but does not establish trajectory accuracy. All original motor bounds,
contact checks, support thresholds and acceptance limits remain unchanged.

The three integrity reports bind all trial recipes, actions, seed 0, captures
and acceptance results. Recipes are reconstructed by the corresponding
`prepare*.mjs`; run each output directory with
`node examples/full-robot/hybrid-speed/run.mjs OUTPUT STATUS_PATH`.
The planner source hash in each plan identifies the required revision.

## Earlier horizontal completion

`finish-plan.json` predeclares four follow-up cases. Finishing horizontal
travel at 85% of the combined swing leaves 114 ms of lowering with fixed XY.
It reduces the 20 ms stopping error to 0.854 mm and passes all 13 swings and
the short task. Actual travel regression is 3.489 mm/s over the 12.78-second
window, and net episode displacement is 67.46 mm. It remains a short case.
The corresponding 5 ms stopping error is 1.388 mm and fails. Finishing at 75%
also fails: 1.002 mm at 20 ms and 1.390 mm at 5 ms. No budget was rounded or
relaxed to accept the near miss.

The 85% pair's maximum foot/body differences fall to 1.340/1.480 mm, still
above the 1/0.5 mm screens. This supports investigating touchdown timing but
does not isolate contact drift as the sole cause. `finish-integrity.json`
checks all four recipes. The browser recipe preserves the 20 ms experimental
candidate verbatim; `browser-parity.json` checks 1,200 native/WASM transitions
at the existing mixed numeric portability tolerance, with exact replay/reset.
Maximum numeric difference is 1.102e-7, distinct from physical accuracy.

## Privileged feedback diagnosis

The existing teacher, with its learned output set to zero and its required
force/body/point observations enabled, passes both short task checks:
13/13 swings and stopping errors of 0.684 mm (20 ms) and 0.812 mm (5 ms).
Heading errors are 0.000348 and 0.000344 rad. This suggests that better feedback
can recover the endpoint margin. The paired foot/body differences remain
1.220/1.249 mm, so restoring the teacher does not solve timestep sensitivity.

`teacher-initialization-failure.json` retains the rejected missing-network
task binding. `teacher-status.json` retains both first-call failures with
floor-force observations disabled; `feedback-teacher-status.json` and its
integrity/refinement reports contain the correctly bound teacher results.
These are controller setup failures, not attempted physical trajectories.

## Finer physics and sustained travel

The 10/5 ms student comparison has 0.650/0.670 mm maximum foot/body differences;
its stopping check also fails. The 10/5 ms teacher comparison gives
0.553/0.569 mm, still above the body screen. The 5/2.5 ms teacher short pair
finally passes both task and numerical screens at 0.481/0.443 mm. This justified
the unchanged paired minute tests in SUSTAINED-PLAN.md.

Both minute runs complete all 41 qualified swings. The 5 ms teacher measures
**3.751 mm/s actual sustained travel** over the 4.02–56 s regression window,
with 212.86 mm net episode displacement and 7.420 J sampled positive shaft
work. Its final position/yaw errors are 1.092 mm / 0.002263 rad; the 2.5 ms
case gives 1.080 mm / 0.002967 rad. Both fail the 1 mm stopping gate. Paired
maximum foot/body differences grow to 0.799/0.837 mm over the minute, failing
the body screen. A short numerical pass did not extend to sustained accuracy.

One targeted standing-gain pair retains every moving-controller parameter and
physical bound. Increasing fully supported standing body gain from 0.75 to
1.5 fails during the final transfer: the 5 ms run detects a thigh gear/pulley
overlap at 56.32 s; the 2.5 ms run rejects a foot command of -0.01333 rad outside
the [-0.6,-0.02] rad bounds at 56.4 s. No limit was relaxed. This motivates
gating stronger standing feedback by completed transfer state, rather than
zero requested velocity alone. That follow-up has not yet been tested.

## Live browser and leaderboard

The 85% student profile's 24 s live forward/stop run passes all 15 swings with
0.933 mm final position and 0.000651 rad yaw error. Active throughput is
0.996× and p95 processing is 37.25 ms: both browser targets fail. Rendering
scheduling p95 is 16.67 ms. The first drawn stopped reference arrives 1.033 s
after key release; this includes transfer completion and is not physical
stopping latency or monitor presentation latency.

Live turning fails at sample 714 (14.28 s), before the scheduled reverse:
step 10 Raise predicts only 0.0374 N on a required support, against the 0.5 N
planning threshold. Native re-execution reproduces the failure and recorded
input events. `browser-status.json` retains both browser outcomes, actual host
details, native audit and command recordings. The timing probe now preserves
physics failures instead of waiting for successful completion. Reference host:
Intel i9-9980HK, macOS x64, Chrome 152.0.7977.76, headless WebGL enabled.

The leaderboard includes the exact experimental student and sustained teacher
recipes alongside prior milestones. It displays measured physical speed,
failed/missing gates and browser cost separately; no recipe is ranked. The
teacher's 5 ms live execution has not been benchmarked for realtime. All
profiles retain uncalibrated physical properties, ideal observations and
privileged planning. Faster reliable stopping, complete steering, sustained
numerical accuracy, browser timing, robustness and hardware transfer remain
unfinished.

## Separate turning posture

The original posture table ended at zero speed, so its fast-forward support
shifts also applied to pure turning. An explicit +3.75 mm/s knot preserves the
forward posture while restoring the original zero-speed posture and declaring
a slower -1.25 mm/s reverse bound. This improves the planned weakest support
from 0.0374 to 0.3978 N, still below 0.5 N. The student at 20 ms and teachers at
5/2.5 ms fail at the identical front-foot lift reference. The forward-only
preservation case still passes with exactly the same final position error.
`steering-status.json` and `steering-integrity.json` retain all four cases.

The remaining turning limit is now isolated to the support posture, before
reverse is reached. A rearward body shift during the front-foot lift is a
specific next hypothesis: using roughly 39 N total weight and a 0.32 m rear
support arm, 1 mm of rearward shift redistributes about 0.12 N to that support.
This estimate must be checked against CAD reach, all other support forces and
actual tracking; no additional shift has yet been accepted.

## Controlled steering and settled stopping

The subsequent -24 mm neutral front-lift support shift passes the student
20 ms and teacher 5 ms mixed steering tasks with 15 qualified swings.
-27 mm collides with the front gear/pulley at 9.4 s; that failure is retained.
Refining the successful profiles initially exposes 1.44/1.11 mm stopping
errors. Increasing standing gain immediately also fails: the last transfer
is still moving when the motion command becomes zero.

`settled-teacher.rhai` adds standing feedback only after reference joint
angles stay stable for 0.4 seconds with zero motion command. It uses existing
Rust corrections and retains all target bounds. Combining this with the
turning posture passes all four `combined-status.json` cases:

| Case | Step | Qualified swings | Final position error |
| --- | --- | --- | --- |
| Forward/stop, 60 s | 2.5 ms | 41 | 0.803 mm |
| Forward/stop, 60 s | 1.25 ms | 41 | 0.659 mm |
| Forward/turn/reverse/stop, 24 s | 2.5 ms | 15 | 0.818 mm |
| Forward/turn/reverse/stop, 24 s | 1.25 ms | 15 | 0.847 mm |

The 2.5 ms minute measures **3.755 mm/s** sustained travel. All added-gain
frames are idle; combined minute physics exactly preserves the earlier
settled teacher at 2.5 ms. The mixed trajectory difference passes the declared
1 mm foot / 0.5 mm body screen (0.305 / 0.323 mm). The minute still fails
the body screen (0.700 / 0.685 mm). These are development cases with the
existing 0.5 N push, not held-out robustness or calibrated hardware results.

## Compiler and derivative cost

The isolated scalar and SIMD/LTO builds preserve full 24-second native/WASM
parity and exact replay/reset. Rendered forward and mixed steering p95 remains
32.9–37.4 ms: a 5–7% compiler benefit, still above 20 ms. Build and episode
hashes are in `web/leaderboard/wasm-profile-status.json`.

Native profiling attributes 6.211 of 10.064 seconds to 2,908 Jacobian
assemblies, versus 0.631 seconds to online planning. Nested closure/dynamics
timers are not additive. The opt-in bounded shared Broyden solver first
reduces native elapsed time from 10.16 to 8.36 seconds on identical 20 ms
steering inputs. Both cases pass 15 swings, with maximum body difference
2.08 nanometres and no contact/phase identity differences. This comparison
does not establish timestep accuracy. `BROYDEN-PLAN.md`, the paired status,
integrity and trajectory reports preserve the exact experiment.
The isolated profiled rerun reduces fresh Jacobians from 2,908 to 1,996,
assembly time from 6.211 to 4.285 seconds and total wall time to 7.984 seconds.
The 6,213 bounded update attempts cost 0.075 seconds; 345 request a fresh
matrix. Profiling preserves every sampled physical frame and input exactly.

The matching SIMD/LTO browser build passes all 1,200 native/WASM transitions,
exact replay/reset and the eight-entry leaderboard UI checks. On/off testing
in the same WASM binary reduces rendered active steering p95 from 33.72 to
26.10 ms; the secant case reaches 1.0001 simulated seconds per wall second.
Forward/stop p95 is 27.39 ms with rate 0.9995. Both remain timing failures;
the independently re-executed forward task passes. Shift and return phases
remain the slowest. `broyden-browser-status.json` retains full compiler,
hardware, browser, recording, UI and acceptance evidence. The delivered
experimental bundle uses the source hashes captured at commit `4cbb3b6`;
the subsequent duplicate-triplet summation fix has a focused solver test.

## Fine minute reference and small secant limit

The repeated 1.25 ms teacher reproduces all 3,001 physical/task frames exactly.
Its new 0.625 ms comparison passes the predeclared minute accuracy screen:
maximum foot difference **0.509 mm**, body difference **0.470 mm**. Both
profiles pass 41 qualified swings; final position errors are 0.659 and
0.556 mm. This establishes the 1.25 ms combined teacher as a numerically
checked native reference for the tested minute and mixed steering cases.
It does not qualify the coarser 2.5 ms or 20 ms profiles. It also does not
establish calibrated physics, held-out robustness or realtime browser speed.

Extending secants to small, still-insufficient cached corrections preserves
the 20 ms task, with maximum body difference 0.834 nanometres. However,
native time is 8.073 versus 7.980 seconds, and profiling finds 2,018 fresh
Jacobians versus 1,996. Despite fewer Newton iterations, more capped/rejected
updates require rebuilding. This option stays off and is not promoted to
the browser. `fine-reference-status.json` and the two study plans retain
the counts, repeated-state checks and acceptance/trajectory evidence.
`TANGENT-PROBE-PLAN.md` predeclares the next experiment at the demonstrated
closure-mapping cost, keeping accepted physics and its gates unchanged.

The fine teacher also passes a full 3,000-transition native/WASM comparison
and exact replay/reset (maximum numeric difference 4.81e-10). The nine-entry
leaderboard passes exact recipe loading, replay, video and tamper checks.
`reference-browser-status.json` records those results and the compiled source
hashes. UI validation/compilation overlapped portions of parity, so its worker
timings are observational and are not rendered performance acceptance.
The separate `walking-reference.yml` CI job reconstructs the exact 3,000
actions and finer configuration, then enforces both minute task audits and
the same trajectory budgets. Its inputs match the locally evaluated files;
the remote workflow itself has not been run in this session.

## Tangent derivative probes

The optional shared implicit solver builds finite-difference derivatives from
a local closure tangent, while retaining full closure for residuals, accepted
endpoints and exact-derivative fallback. The initial 24-second native steering
case passes all 15 swings and finishes at 0.958 mm position error. It takes
6.077 seconds versus 8.261 seconds for the original derivative construction;
maximum foot/body differences are 1.79/1.11 nanometres. A separate profile
reduces closure mapping from 3.40 to 1.43 seconds, with six exact-derivative
fallbacks. This changes solver work, not the retained physical model.

The initial SIMD/WASM build fails the fixed host-portability screen at a
force sample: maximum difference 1.45e-7 N. Same-host replay/reset and the
ten-entry UI loading, replay, video and tamper checks pass. The failed parity
is retained in `tangent-probes-initial-browser.json`; rendered timing is
withheld for that candidate. Its coarse 20 ms timestep still misses the
separate 1 mm foot / 0.5 mm body refinement screen (1.599 / 1.481 mm).

`TANGENT-RADIUS-PLAN.md` declares three derivative radii without changing any
acceptance threshold. All three pass 33 analytic/contact/rollback tests and
the 15-swing native steering case. Body differences from the original solver
remain below 1.7 nanometres. The default radius exactly preserves the prior
1,201 physical/task frames and recording. Native runs overlap compilation;
their times are observational. Separate sequential browser checks determine
portability before any rendered performance evaluation.

Both larger radii pass fixed host portability and exact replay/reset. The
selected 1e-5 radius has maximum native/WASM difference 8.28e-9 and worker-only
p95 15.26 ms. With actual WebGL rendering, the same-binary steering comparison
reduces active p95 from **26.26 to 20.84 ms** (21%); forward/stop reaches
**22.24 ms**. Both still miss 20 ms. Active rates are 1.000052 and 1.000346;
steering overall rate is 0.998298, also a failure under the unchanged rule.
Transport/dispatch p95 is about 7–8 ms across active phases, so browser drawing
and delivery are a material remaining cost alongside the return-phase solver.

The recorded forward walk independently passes 15 swings and ends at 0.933 mm
position error. All 11 leaderboard entries pass exact loading, full selected
replay, WebM export, narrow-screen and tamper tests. The selected entry remains
unranked because timing, coarse timestep accuracy, sustained walking and
held-out robustness/terrain are incomplete or failed. `tangent-browser-status.json`
preserves the same-binary pair, all timings, inputs, hardware, parity and UI
evidence. An initial attempt to measure the new config under the old pinned
preset was rejected before physics; it is recorded as a packaging failure,
not a walking failure. The new entry packages the exact selected recipe.

## Display cadence does not close the latency gap

The explicit 30 fps display option leaves 50 Hz physics/control and pacing
unchanged. It reduces steering drawings from 43.48 to 29.63 per wall second,
but active p95 is essentially unchanged: automatic **20.710 ms**, capped
**20.715 ms**. Capped forward/stop is **22.240 ms**. Capped steering active
pace is 0.999937 and forward pace 1.000641; the original gates still fail.
Transport/dispatch p95 is 7.36 versus 7.30 ms. This experiment does not support
drawing frequency as the main cause of that delivery delay. Automatic display
stays the default; the optional cap is not a realtime qualification.

All three recordings exactly match the previously audited physics recipes,
seeds and inputs, and use the identical WASM module. The 11-entry UI suite
passes at the capped display setting, including full replay and real WebM
output. The control fits the narrow viewport, which was also visually
inspected. `display-cadence-status.json` retains the sequential measurements;
`display-cadence-integrity.json` binds them to the original captures and UI
checks. The next isolated transport experiment should distinguish outgoing
object cloning and message delivery from rendering and Rust stepping.

## JSON frame transport is a small cost

Sending the exact Rust JSON step frame to the receiver passes all 1,200
native/WASM transitions at the unchanged tolerance (maximum 8.28e-9), exact
replay/reset, and invalid-encoding state-preservation checks. The original
object reply remains available for compatibility. The viewer requests JSON
step replies and parses each frame once; timings include that receiving parse.

On the same bundle with automatic display, object steering active p95 is
**20.29 ms**, JSON steering **20.19 ms**, and JSON forward/stop **22.07 ms**.
All three meet active and overall pace, but all still miss the 20 ms gate.
Object/JSON transport plus dispatch p95 is 7.19/7.40 ms; JSON receiving parse
p95 is 0.185 ms. One sample per case does not establish a significant speed
advantage. Together with the display-cadence experiment, this rules out these
small presentation/encoding changes as a sufficient latency fix on this host.

All recordings exactly match the previous physically audited inputs and
recipes. Eleven-entry UI load/replay/video/mobile checks and the six-preset
fixture suite pass, including cancellation, reset recovery, condensed
mechanics and replayed physical errors. `frame-transport-status.json` and
`frame-transport-integrity.json` retain measurements and source bindings.
CI exercises JSON host parity and decoding while existing object clients
continue to cover the compatible reply path. Remote CI has not been run here.

## Temporal velocity prediction does not save total work

The shared default-off velocity predictor requires a 10% improvement in the
exact initial residual. Original per-row residual bounds are retained or
tightened, and a failed predicted solve restarts at the original velocities
with exact derivatives. Tests cover analytic force/viscous cases, rotating
closed linkages, invalidation, contact release, rollback and rejected Newton
trials. All 37 robot integration checks and 27 solver checks pass.

Both 24-second native steering cases pass 15 supported swings and finish at
0.958 mm position error. Maximum off/on foot and body differences are
0.697 and 2.535 nanometres. Default-off exactly preserves the previous 1,201
physical/task frames and recording. Profiling also preserves every frame.

Prediction is used in 742 of 1,200 accepted steps. Newton iterations fall
10,425→10,125 and fresh Jacobians 1,975→1,884, but closure mappings rise
23,117→23,736. Unprofiled runtime is **6.043→6.097 seconds**, and native active
stepping p95 **13.147→13.353 ms**; return-phase p95 also worsens. Profiled
time rises 6.223→6.644 seconds. The proposal screen costs more than it saves
on this robot. The option remains off and is not promoted to a WASM timing
experiment. `velocity-seed-summary.json` links all outcomes, exact preservation,
profiles and unchanged physical/solver-difference gates.

## Exact Jacobian-base reuse saves little browser time

The optional shared cache reuses an already exact closure mapping only when
every mechanical unknown matches bitwise. Derivative-probe context remains
separate, and the ordinary exact residual, endpoint acceptance and fallback
remain unchanged. All 38 focused linkage/contact/cache tests pass. The two
24-second native captures have strictly identical physical states, task
transitions and recordings apart from the declared option. The strict check
distinguishes signed zero and also verifies profiled runs and the previous
default-off reference.

Reuse removes 806 accepted Jacobian-base preparations. Native unprofiled time
falls **6.051→5.983 seconds** and profiled active transition p95
**13.757→13.517 ms**. Full 1,200-step JSON native/WASM parity passes with maximum
difference **8.282e-9**, exact replay/reset and invalid-input preservation.

On the same SIMD/LTO WASM with automatic display and JSON transport, rendered
steering active p95 is **20.55 ms off / 20.48 ms on**. Enabled forward/stop is
**22.67 ms**. All active and overall simulation rates pass; both enabled
latency cases still fail the original 20 ms requirement. One run per case
does not establish a significant browser speed advantage. Native replay of
the exact browser forward inputs passes all 15 swings, with 0.933 mm final
body error. The 20 ms versus 5 ms trajectory screen remains a failure; no
tolerance or physical parameter was relaxed.

`exact-probe-base-browser-status.json` retains the rendered outcomes and
hardware details. `exact-probe-base-identity.json`, `exact-probe-base-profile.json`
and `exact-probe-base-refinement.json` preserve the native comparisons. The
new leaderboard recipe explicitly enables reuse; the library default remains
off. This small exact-work optimization does not resolve realtime latency,
coarse-step accuracy or the outstanding learning and held-out robustness work.

The 12-entry browser suite passes every exact Load and run, selected full
replay, video export, narrow layout and tampered-recipe rejection. Its first
attempt timed out on a short video download. Focused 0.3/2-second clips both
produce valid WebM bytes, and a full retry with recorder-event diagnostics
passes without lengthening the clip or timeout. The cause was not reproduced;
the failed log and successful evidence are retained in
`exact-probe-base-browser-integrity.json`. CI now checks strict off/on native
steering identity and enabled host parity. Remote CI remains unrun.

## Faster teacher-to-student learning improves fine steering and stopping

The new reusable dataset preparation consumes pinned recipes, typed channels
and complete accepted training captures. Labels subtract the recorded local
tracking gain and are checked against applied servo targets; the runtime
rejects out-of-bounds commands rather than clipping them. Dataset tests cover
gain handling, mismatched labels, duplicate samples and reused validation
inputs. The four shared Rust neural tests also pass.

The fixed 64-unit network is trained on 4,200 samples from the 1.25 ms teacher
minute and mixed-steering cases. Epoch 949 is selected by training loss only.
Training loss falls 0.004911→0.00008280. The predeclared mirrored-turn episode
fails planned static support at 14.84 s; its 742 accepted samples remain
validation-only. Their loss falls 0.002734→0.0004837, with worst-joint RMS
0.001866 rad and maximum 0.01063 rad. No post-failure labels or replacement
passing validation episode are manufactured.

At 1.25 ms, the initial student finishes steering with 1.370 mm body error,
failing the 1 mm limit. The fitted student passes all 15 swings and finishes
at **0.787 mm**. Its full minute passes 41 swings, final body error **0.670 mm**
and yaw error **0.00334 rad**. Measured sustained travel is **3.75144 mm/s**,
with **6.6795 J** sampled positive shaft work over the episode. The still-ideal
encoder/IMU motor inputs exclude teacher body/foot/contact corrections; the
upstream planner remains privileged and hardware deployment is unverified.

The student fails the same reserved turn-to-forward transition as the teacher,
at the same time and identical planned forces (weak support 0.49146 N below
0.5 N). Inspection shows the new command is already latched when that shift
starts. Planned support decreases during body advance in swing; this is a
reference geometry limit, independent of learned motor feedback. The original
held-out failure is retained. Any future tuning on it makes it development
evidence and requires a fresh untouched evaluation episode.

The 0.625 ms minute also passes 41 swings and ends at 0.705 mm body error, but
1.25/0.625 ms body trajectory difference is **0.706 mm**, failing the 0.5 mm
screen (foot difference 0.740 mm passes). At 20 ms, short steering passes and
omitting unused feedback calculations preserves every physical frame and task
transition exactly. Minute stopping fails at both 20 and 5 ms: 1.572 and
2.195 mm final error. Their trajectory differences are **2.607 mm foot /
2.639 mm body**, both failures. Thus neither fine nor coarse student physics
has passed its full declared accuracy screen.

`fast-distillation-summary.json` and `fast-student-fidelity-summary.json`
retain complete/prefix metrics, labels, selected weights, gates and source
identity. Full 1,200-transition JSON browser parity for the 20 ms student
passes at the unchanged tolerance (maximum 2.143e-9), with exact replay/reset.

In the same WASM rendered comparison, previous/student steering active p95
is **19.97 / 19.52 ms**; new forward/stop is **20.69 ms**. All overall rates
exceed one, but both new active rates are about **0.9995×**, narrowly failing
the unchanged active pace gate. Forward also fails the latency gate. These
single runs do not establish a significant speed advantage or realtime
qualification. Native replay of the exact browser forward inputs passes all
15 swings and stops with 0.909 mm body error.

The 14-entry UI suite passes every exact Load and run, selected full replay,
WebM video export, narrow layout and tampered-recipe rejection. The fine and
browser learned recipes remain experimental and unranked; fine held-out
control and both fidelity accuracy failures are visible. Browser results and
recipe/native identity are retained in `fast-student-browser-status.json`
and `fast-student-browser-integrity.json`. CI now includes the typed dataset
guards and exact learned browser recipe's native acceptance/parity; remote
CI has not been run here.

## Transition support improvement exposes a stopping limit

The two predeclared forward front-foot support offsets both complete the
formerly aborted 32-second teacher sequence at 1.25 ms. At -22.5 mm, the
affected swing qualifies for only 0.14 s against the 0.20 s requirement, and
final body error is 1.610 mm. At -24 mm, **all 19 swings qualify** with no
sampled internal collisions, but final body error is **2.052 mm**. Heading
and tilt pass both cases. Neither passes the full unchanged task, so neither
is promoted and no minute/steering regression claim is made.

`transition-support-summary.json` and its integrity audit preserve both
outcomes. The revealed sequence is now development evidence; future held-out
evaluation must use new inputs. Additional support shift fixes the static
reference abort and, at -24 mm, the dynamic swing, but does not resolve the
teacher's final tracking error. This separates the next controller problem
from the independent coarse-integration accuracy and browser timing limits.

## Two-stage integration does not qualify a coarser browser profile

The default-off SDIRK2 experiment passes 45 focused Rust vector, mechanical,
contact and closed-linkage tests. The held-command scalar screen demonstrates
second-order improvement for a resolved mode, but also under-resolved stiff
overshoot. It rejects the proposed BDF2 shortcut across 50 Hz command changes;
none of these analytic results constitutes robot accuracy evidence.

All seven predeclared 24-second native steering outcomes are retained in
`sdirk-steering-summary.json` and verified by `sdirk-steering-integrity.json`.
Rebuilt default-off backward Euler exactly preserves all 1,201 physical frames
and task transitions from the previous student capture. Both 20 ms SDIRK2
cases fail at 1.32 s: teacher command bounds and student inter-link overlap.
The 10 ms teacher terminates at the same time on its upright task guard, with
large velocity growth. These are immediate failure reasons; the underlying
numerical cause has not yet been isolated.

The 5 ms teacher and 10/5 ms students each pass all 15 swings and stopping
gates. However, teacher SDIRK2 at 5 ms versus its 1.25 ms backward-Euler
reference differs by **0.762 mm foot / 0.564 mm body**: body fails the unchanged
0.5 mm budget. Student 10/5 ms differences are **1.016 mm foot / 1.045 mm body**,
failing both trajectory budgets. Passing endpoint task checks is insufficient.

Student stepping throughput is **1.231× at 10 ms** and **0.871× at 5 ms**;
teacher 5 ms is **0.453×**. These are native compute rates, excluding capture
serialization. Teacher/student solver settings differ and are explicitly
retained, so their timings do not isolate the effect of the integrator.
No SDIRK2 browser build or promotion follows from this failed screen. The
existing 14-entry browser milestone remains available and experimental.

The shared capture audit now distinguishes a verified task termination from
a runtime error. Prefix measurements require explicit opt-in and preserve the
termination reason, final frame time and physics-step count. Three focused
Node tests pass, including rejection of unexplained, truncated, mismatched
or continued partial captures; CI includes these checks. Remote CI was not run.

The follow-up tolerance diagnosis also fails at 1.32 s with both Newton
tolerances tightened from 1e-5 to 1e-8. Maximum accepted-prefix differences
are only **4.083e-9 rad joint position**, **7.229e-7 rad/s joint velocity** and
**3.192e-10 m body position**. Both reach about **247.622 rad/s** sampled gear
speed, with zero external force during the preceding return-phase samples.
The instrumented authored run preserves every recorded physical frame and
task transition exactly. Smaller residuals do not remove the early jump;
this simple solver-tolerance change is not a sufficient remedy.

`sdirk-tolerance-summary.json` preserves the two profiles and this comparison.
Profile stage/interval records may include work before a failed environment
transition; they are not counts of published control frames. The failed first
tool invocation (which incorrectly rejected the runner's documented exit 1
for a retained runtime error) is preserved under `runner-guard-rejection`
filenames. The reusable diagnostic runner now checks exit status against the
parsed outcome. No new physics or controller qualification is claimed.

The opt-in stage audit preserves the previous 20/5 ms teacher captures exactly.
Its 19 mechanical tests include analytic stage values, bitwise preservation,
and audit-window exclusion. The coarse jump appears in the second stage of
the 1.30–1.32 s macrostep: its seed's largest reduced speed is 5.372 rad/s
at the foot servo, and the endpoint reaches 247.622 rad/s. A proposed Newton
correction of 12,667.23 rad/s is accepted at fraction 1/64 before convergence
to the distant state. The bounded 5 ms audit reaches only 3.402 rad/s in this
window. The active regularized-Coulomb profile has zero recorded contact
memory and zero audited bristle rates, excluding memory overshoot here.

`sdirk-stage-summary.json` retains the evidence. The vector SDIRK primitive
already uses the first physical stage as the next Newton guess; the mechanical
adapter instead uses the affine seed. Testing that difference is the next
bounded experiment. The current evidence locates the jump but does not yet
prove another guess finds a bounded solution or qualifies coarse integration.

Starting the second mechanical solve from the first physical stage removes
both coarse early failures. Both 20 ms controllers now finish 24 seconds with
zero sampled internal collisions and sampled maximum gear speeds below
1.8 rad/s. The 41 mechanical/linkage tests pass, including invalid-guess
rejection and exact analytic audit values. The rebuilt default-off backward
Euler capture preserves all 1,201 physical frames and task transitions exactly.

This is a stability improvement, not qualification: teacher fails one of
15 swings and ends at **1.418 mm** body error; student passes all 15 swings
but ends at **2.917 mm**. Teacher versus the 1.25 ms reference differs by
**1.766 mm foot / 1.669 mm body**, failing both unchanged trajectory budgets.
Native compute is **1.029× teacher / 1.946× student**; these are not browser
rates. `sdirk-physical-guess-summary.json` retains every result. Finer teacher
steps are the next accuracy/cost screen; no coarse recipe is promoted.

The completed refinement screen retains that conclusion. At 10 ms, teacher
stopping passes at **0.603 mm**, but one swing fails. At 5 ms all 15 swings
and stopping pass (**0.677 mm**). The new 0.625 ms backward-Euler steering
reference also passes the task; 1.25/0.625 ms differences are **0.220 mm foot /
0.156 mm body**, passing the unchanged refinement budgets on these commands.

SDIRK2 at 10/5 ms differs by **0.909 mm foot / 0.883 mm body**. Against the
new 0.625 ms reference, the 20/10/5 ms maximum body differences are
**1.713 / 1.359 / 0.547 mm**: all fail the 0.5 mm screen. The 5 ms foot
difference is **0.788 mm**, which passes. Its body comparison against the
retained 1.25 ms reference also fails at 0.567 mm. Thus refining the reference
does not remove this accuracy failure. Changing the 5 ms stage guess alters
body trajectory by only 0.0116 mm, unlike the coarse root jump.

Native teacher rates at SDIRK2 10/5 ms and backward Euler 0.625 ms are
**0.635× / 0.461× / 0.382×**. `sdirk-physical-refinement-summary.json` retains
all eight comparisons and the task, timing and source records. The corrected
SDIRK method is useful shared experimental infrastructure, but this study
does not establish an accurate realtime browser profile. Keep the detailed
reference and existing browser recipes; the independent body-tracking,
held-out support, robustness and coarse-model performance work remains.

## Settled body and foot corrections oppose each other

At the revealed -24 mm support stop, all feet carry about 9.7–9.9 N and the
inferred standing body gain is exactly 1.5. Its body and world-foot angular
corrections have cosine **-0.985**. The 2x2 settled-feedback ablation preserves
the original full reference exactly and all 1,301 frames through 26 s before
the stop in every case. All 19 swings remain passing.

Releasing point correction after settling reduces body error from
**2.052 to 1.776 mm**, still failing 1 mm. Raising settled body gain to 4.5
is unstable: point-on/off cases drift **193.990 / 126.527 mm**. They remain
upright with no sampled internal collisions, but clearly fail stopping;
the combined case also fails heading. Thus upright motion alone would give
a misleading success signal.

The high-gain runs have **2,947 / 2,183** consecutive target-delta reversals
above 1e-5 rad after stopping, versus **141 / 152** in reference/point-release.
Maximum per-sample command changes rise from **0.028 / 0.036 rad** to
**0.232 / 0.291 rad**. Sampled positive shaft work increases from about
**3.14 J** to **10.42 / 9.38 J**. Maximum loaded-marker path rises from
about **0.091 m** to **1.405 / 0.603 m**; this is a geometric path proxy,
not resolved contact-patch slip.

`settled-stance-summary.json` retains all four failed full-task outcomes and
the source-bound diagnosis. Both invalid new point scales (-0.1 and 1.1)
are rejected with zero recorded physics steps. No case is promoted. The next
bounded control experiment is a slow, rate-limited integral bias using the
existing body suggestion, preserving the stable proportional gain.

## Bounded integral feedback passes the revealed stopping case

The shared Rust `control.angle_integral` component adds a bounded, rate-limited
angular bias. Registry ports and parameters, Rhai's native binding and the
direct Rust API share the update and validation. Seven focused tests cover
bounds, reversal without windup, leakage/release, a synthetic disturbed plant,
registry units/events, replay, failure rollback and integer-valued JSON inputs.
The initial binding rejected valid integer fields before physics; all three
zero-step failures are retained separately and the numeric boundary is fixed.

The repaired gain-zero controller preserves the full point-release reference
exactly, and all three gains preserve all 1,301 frames through 26 s. At gains
**0.5 / 1.0 per second**, the unchanged 32-second task passes all **19 swings**,
stopping at **0.758 / 0.425 mm** body error. Heading, tilt and sampled collision
checks pass. The stable proportional gain remains 1.5; integral bias is limited
to 0.04 rad and changes at no more than 0.01 rad/s.

Sampled positive shaft work is **3.156 / 3.158 J**, versus gain-zero's 3.146 J.
After-stop target-delta reversals are **148 / 150**, versus 152 in the reference;
the maximum command jump is 0.03652 rad, close to the reference's 0.03632 rad.
These observations avoid the high-gain ablation's large alternating commands
and drift. `settled-integral-summary.json` preserves every outcome and source.
This is revealed development success, not fresh held-out, timestep, terrain,
browser or hardware qualification. Gain 1.0 supplies the larger stopping margin
for the next frozen-candidate regression screen.

## Frozen integral teacher passes sustained and fresh command checks

All five predeclared regression cases pass. The 60-second run completes **41
qualified swings**, measures **3.778 mm/s** sustained forward travel, and stops
within **0.529 mm**. The original mixed steering run completes **15 swings**
and stops within **0.338 mm**. At 0.625 ms, the minute still passes all 41 swings,
measures 3.774 mm/s and stops within 0.487 mm. The full-trajectory comparison
is **0.495 mm foot / 0.327 mm body**, passing the unchanged 1 / 0.5 mm budgets.
This compares two finite timesteps; it does not establish hardware accuracy.

Both fresh 48-second episodes complete **23 qualified swings**. The unforced
case's three predeclared stop errors are **0.171 / 0.184 / 0.268 mm**; the
case with bounded lateral/forward pulses gives **0.171 / 0.182 / 0.267 mm**.
Every checkpoint is idle and meets the heading limit. The controller resumes
motion after releasing its bounded bias without failing these task gates.
These two cases were held out from this candidate's selection and are now
regression cases for future changes. Their short straight command windows
include transfer transients and are not substitutes for the sustained-minute
speed estimate.

Minute sampled positive shaft work is **6.868 J**, versus **6.734 J** at the
finer timestep. Native compute runs at **0.533x / 0.363x** for the two minute
timesteps; neither is realtime. Native throughput is separate from measured
walking speed and browser performance. Terrain, broader disturbance/seed
coverage, student observability and calibrated hardware remain unresolved.

`settled-integral-regression-summary.json` retains all task, stop and numerical
outcomes. `settled-integral-regression-recipes.json` contains every authored
configuration and sparse command sequence, with the versioned CAD-derived
scene and task hashes. `reproduce_settled_integral.mjs NEW_OUTPUT` reconstructs
all five cases without earlier ignored run inputs; its reconstructed actions
are checked against their original hashes. Raw captures remain local and are
identified separately by source hashes. The browser milestone uses a new
isolated WASM build with the shared integral binding.

## Browser milestone and contact-motion limit

The new isolated WASM passes all **1,200** steering transitions against native
at a maximum numeric difference of **2.20e-9**, with exact replay and reset.
Both rendered recordings exactly match the accepted native recipe, seed and
inputs. Steering/minute active browser pace is **0.405x / 0.421x**, with
**92.72 / 93.48 ms p95**: both fail the unchanged realtime gates. The render
interval p95 remains about 16.67 ms. This is a slow fine-model milestone, not
the required final realtime experience. Reversing also waits for a committed
transfer: the tested reverse reference first appears 1.36 simulated seconds
after the command, about 3.70 wall seconds in this slow run. Reference response
and drawing are distinct from causal physical response or monitor latency.

Every one of **16** leaderboard entries loads its pinned recipe and initial
action in WASM. The new steering entry replays its full tested sequence;
WebM export, narrow/desktop layouts and mutated-recipe rejection pass. The
results remain unranked. `settled-integral-browser-integrity.json` binds the
recordings, task outcomes and UI checks; minute host parity is not inferred
from the shorter steering comparison.

A new read-only contact diagnostic computes material velocity at each recorded
floor contact from COM velocity and angular velocity. Per-foot accumulated
load-weighted tangential motion is **198 / 167 / 198 / 152 mm** over the minute,
versus **213 mm** body advance. Pointwise motion including rotation is similar,
so the large marker paths cannot be explained solely by marker rotation.
These are integrated sampled speeds with changing contact weights, not paths
of one persistent material point. Nevertheless they expose substantial loaded
sliding in this provisional regularized-Coulomb floor model (1 mm/s smoothing
speed). Passing swing, stopping and timestep tests is insufficient to claim
slip-free walking or a credible hardware speed envelope.

`settled-integral-minute-contact-motion.json` records the per-foot motion and
sampled translational shear work. It does not set a retrospectively selected
acceptance threshold or resolve contact birth/death, between-sample peaks or
independent torsional dissipation. Contact-model sensitivity and anti-sliding
qualification must be addressed alongside realtime computation, before this
candidate can become a validated walking choice. The CI regression is added;
its remote execution has not been observed in this session.

## Smaller contact smoothing does not cure loaded sliding

With the teacher, CAD, friction coefficients, commands and solver tolerances
frozen, smoothing speeds **1 / 0.1 / 0.03 mm/s** all pass the minute's original
41-swing task. Sustained travel is **3.778 / 3.774 / 3.773 mm/s** and final stop
error is **0.529 / 0.461 / 0.447 mm**. These are distinct constitutive fidelity
profiles, not timestep refinements. The standard suite audit correctly rejects
different physics options; per-profile audits retain that rule, and a separate
cross-profile check proves that only the declared smoothing speed changes.

The largest per-foot accumulated load-weighted contact motion divided by net
body advance is **93.1% / 87.5% / 87.5%**. Every profile fails the prospective
**5%** anti-sliding screen declared before evaluating the alternatives. The
tenfold smoothing reduction gives only a small change, and a further reduction
has effectively plateaued. Native rates fall to **0.392x / 0.206x** from the
reference's 0.533x; p95 rises to **126 / 309 ms** from 80 ms. None is promoted.
`contact-smoothing-summary.json` retains every task and contact-screen outcome.

A focused analytic diagnostic test distinguishes pure rolling from sliding:
COM motion with cancelling angular velocity produces zero material-contact
motion; added 2 mm/s sliding produces the expected displacement and shear
work. Internal contacts are excluded. This test passes and is included in CI.
The large measured contact motion is therefore not simply counting a moving
COM or a rotating marker as sliding.

The reference body's horizontal path is **1.742 m** for **0.213 m** net advance.
Shift and return phases account for **0.715 / 0.755 m** of that path. Lower
smoothing is neither an adequate sliding fix nor a computational improvement.
The next diagnosis must attribute contact motion and traction demand to phase
and support role, then test body-shift/return motion or traction-aware control.
Do not compensate by inflating friction or actuator authority without an
explicit physical hypothesis and provenance. Numerical, browser, terrain and
held-out qualification of any resulting controller remain separate.

## Phase attribution and slower shifts isolate traction demand

The shared read-only contact kernel preserves every prior per-foot integral
exactly. Phase attribution uses the controller sample held over each physical
interval, with a focused test guarding against a one-sample offset. **77.7%**
of total sampled contact motion occurs in shift/return: **226 / 330 mm** summed
across the four feet. Both the selected transfer foot and other loaded feet
contribute. Raising/lowering account for **85 / 37 mm**. Many shift/return
samples reach a shear/normal ratio of 0.25, the observed traction limit.

Doubling shift/return duration while retaining each 5.175 mm planned step gives
**2.741 mm/s** measured travel, **30 passing swings** and **0.283 mm** final stop
error. The worst-foot contact-motion/body-advance ratio falls from **93.1% to
29.2%**, a substantial improvement but still above the fixed 5% screen.
Quadrupling gives **1.726 mm/s**, **19 passing swings**, **0.131 mm** stop error
and **25.0%** contact motion. The measured speed/reliability tradeoff therefore
does not justify simply slowing this gait further as the final experience.

All completed-transfer planned body/foot endpoints agree with the original
spatial sequence within **7e-18 m**; controller gains, CAD, force laws and motor
authority are unchanged. Sampled shaft work falls from 6.868 J to **5.294 /
4.192 J**, and native rates rise to **0.610x / 0.821x**. These are native timing
measurements, not browser acceptance. Both slower cases remain unqualified by
the prospective contact screen. All outcomes and phase breakdowns are retained
in `shift-duration-summary.json`.

The doubled case still accumulates 33/54 mm of shift/return contact motion and
47/25 mm during raise/lower. A more useful next gait can combine the body return
and next support shift into one smooth transfer, avoiding the stop at the
center while preserving landing/readiness guards and an explicit recenter on
stop. That must first be implemented and tested in the shared Rust sequence;
its contact quality, geometry and measured speed cannot be inferred here.

## Direct support transfer retains speed but remains unqualified

The shared Rust sequence now offers an explicit `direct_support_transfer`
option, default off. It holds the last swing support reference after landing,
then shifts directly to the next support posture. Stop handling recenters
explicitly, with readiness guards and transactional failure. Twenty sequence
tests cover geometry, phase continuity, turning/reversal, cancel/stop/resume,
guard failure and default serialization; the two existing runtime integration
checks also pass. The rebuilt default steering capture matches every physical
frame, task transition, recording and contract exactly (excluding only frame
wall-clock time).

With phase times `[0.58, 0.38, 0.38, 0.02, 0.02]`, the minute retains the
1.38-second nominal transfer, 3.75 mm/s command and original planned foot
landings: **41** landing sets differ by at most **4.44e-16 m**. All 41 swings
qualify and measured travel is **3.696 mm/s**. Final position error is **0.753
mm**, but heading error **0.01361 rad** fails the 0.005 rad gate. The contact
motion ratio improves to **45.2%**, still far above 5%. Native throughput is
0.507x and sampled shaft work is 6.244 J. This is a retained failed minute,
not a validated faster walking controller.

The direct steering case passes all **15** swings and stops within **0.349 mm**,
with heading **0.00453 rad**, close to its limit. Sampled reference acceleration
is at most **0.684 m/s²**, versus **2.565 m/s²** for the unchanged steering
reference. This is a reference finite-difference diagnostic, not actual COM
acceleration. Direct-minute summed contact motion is **168 mm in shift**,
**64/35 mm during raise/lower**, and only **2.3 mm in return**. The remaining
shift and loaded-foot tracking limit persists; merging phases alone does not
solve it. `direct-support-summary.json` retains the original task failures,
contact screen, identity checks, work and phase metrics.

The leaderboard now requires a separate loaded-contact-motion gate. Missing
evidence cannot satisfy it; the integral and direct-transfer candidates expose
their measured failures. This prevents future speed ranking from relying only
on supported swings, endpoint stopping and numerical agreement.

Direct steering also completes all 1,200 native/WASM transitions: maximum
numeric difference is **3.56e-10**, with exact same-host replay and reset.
Rendered keyboard control completes the accepted 24-second command sequence,
and its recording exactly matches the native recipe, seed and inputs. Active
walking runs at **0.415x**, with **89.515 ms p95** transition processing; both
realtime gates fail. Render interval p95 is **16.67 ms**, while mean WASM-call
time is **43.58 ms** versus **0.47 ms** transport/dispatch. Physics and controller
work dominate this measurement. The reverse command waits **1.36 simulated
seconds** to appear in the walking reference and **3.13 wall seconds** to be
drawn. These are reference-response measurements, not causal physical-response
latency. Full-minute browser timing, direct-gait timestep refinement, terrain
and new held-out qualification remain unmeasured.

The isolated 18-entry browser catalog passes every exact Load and run check,
full direct-steering input replay, real WebM export, desktop/mobile layout,
filter/comparison checks and rejection of modified recipe bytes. Static
eligibility and packaging checks pass, including an explicit failed-task case
and missing/failed contact evidence. All entries remain unranked. Browser
reports are retained in `direct-support-browser-{plan,status,parity,integrity}.json`;
the reusable sequential `measure_browser_suite.mjs` pins its plan, input files,
bundle manifest and measurement harness and retains failed timing outcomes.
`direct-support-browser-delivery.json` verifies that the delivered catalog uses
the same tested UI/worker/WASM and all 39 preset recipe bytes, with updated
evidence metadata for its 18 leaderboard entries.

## Loaded-foot damping does not resolve stance sliding

The shared `control.load_damping` component adds a signed displacement objective
opposing measured velocity, weighted by floor load. Registry equations, Rust
and Rhai use the same units and validation. The optional point-feedback adapter
uses contact-point velocity including rotation, excludes internal contacts,
adds only horizontal correction and keeps the existing angular cap. It remains
privileged flat-floor teacher feedback; no robot property or actuator authority
changed. Thirteen focused kernel, contact, Rhai and runtime tests pass, and the
browser CI workflow includes the new tests.

Four frozen 60-second development runs retain the same robot, direct-transfer
gait, inputs and original gates. The rebuilt position-only capture matches all
physical frames, transitions, recording and contract exactly, excluding only
frame wall time. In each enabled case, all **12,000** online marker observations
match the preceding committed frame's independently calculated contact load
and tangential velocity **exactly**.

| Damping seconds | Travel mm/s | Qualified swings | Stop mm | Heading rad | Worst-foot motion / body advance | Shaft work J | Native sim/wall |
|---|---:|---:|---:|---:|---:|---:|---:|
| Absent | 3.696 | 41/41 | 0.753 | 0.01361 | 45.2% | 6.244 | 0.523 |
| 0.05 | 3.695 | 41/41 | 0.765 | 0.01344 | 43.7% | 6.233 | 0.517 |
| 0.2 | 3.703 | 41/41 | 0.664 | 0.01347 | 42.0% | 6.339 | 0.506 |
| 0.5 | 3.732 | 37/41 | 0.430 | 0.01189 | 80.8% | 6.788 | 0.490 |

Every case fails the unchanged 0.005 rad heading limit and 5% contact-motion
screen. At 0.5 seconds, four transfers have only 0.14–0.18 seconds of simultaneous
clearance, unloading and support, below 0.2 seconds. All runs have zero sampled
internal contacts and tilt below 0.00432 rad.

The small improvement in the worst-foot ratio at 0.2 seconds does not represent
an overall reduction in contact motion: total integrated foot motion rises from
**273 to 284 mm**, including summed shift motion rising from **168 to 187 mm**.
At 0.5 seconds, motion of the other supporting feet during lower
rises from **25 to 219 mm**. Stronger velocity correction therefore disrupts
the coupled stance behavior despite using the correct observed signal. This
rules out simple gain escalation as the next useful direction. None is promoted
to the browser or ranked. `loaded-foot-damping-{plan,status,integrity,summary}.json`
and per-case metrics retain the failures and source identities.

The browser performance harness now also retains per-frame receipt and draw
submission timestamps on the same page clock as command dispatch. A fresh
24-second direct-steering run passes exact recording association and timeline
checks for **1,200** received frames, **1,194** distinct drawn frames and four
commands. Active throughput is 0.418x with 90.175 ms p95. This is instrumentation
for the outstanding paired physical-response experiment, not a claim that
reference response proves physical causality. The proof and measurement are in
`causal-response-browser-{status,timeline-integrity}.json`; the live viewer's
physics and controller recipe are unchanged.

## Paired command response separates body motion from reference changes

Five matched 24-second native runs repeat the accepted direct-steering case
and omit one forward, turn, reverse or stop command interval at a time. The
commanded run matches the earlier accepted capture exactly; every paired
physical prefix through its branch time is identical. The audit verifies that
only the selected interval's motion-command channels change, retaining feedback
gains, residual inputs, seed, configuration, robot and physics. All five runs
complete. The commanded, no-forward and no-turn runs pass the original task;
no-reverse and no-stop endpoint failures are retained.

The prospectively declared directed-difference thresholds are **0.1 mm** of
body translation or **0.0005 rad** of yaw, each sustained across **0.1 s** of
50 Hz samples. These are detection sensitivities, not new task gates. Position
is projected on the actual body-forward axis at command time. For stop, positive
displacement relative to continued reverse detects the first command effect;
it does not prove that motion has ceased.

| Command | First directed response, simulation s | Frame received, wall s | Frame drawn, wall s |
|---|---:|---:|---:|
| Forward | 0.96 | 2.204 | 2.225 |
| Turn | 1.22 | 3.031 | 3.038 |
| Reverse | 1.72 | 3.504 | 3.517 |
| Stop request | 0.02 | 0.096 | 0.116 |

Forward has a detectable horizontal difference in any direction at **0.28 s**,
well before the directed response at 0.96 s. Reverse similarly has an
any-direction response at **1.40 s** versus 1.72 s directed. A reference change,
or the first motion of any kind, therefore understates the delay to the declared
directed body response. Conversely, the stop request affects the physical
trajectory before the gait finishes landing and recenters; the 20 ms detection
must not be reported as completed stopping.

The browser recording has the same commanded recipe, seed and inputs; the
single-page timeline maps the detected native physical samples to their actual
receipt and WebGL submission times. This is paired native-model evidence
associated with browser frames. Counterfactual WASM portability, monitor
presentation, direct-gait timestep accuracy and hardware latency remain
unmeasured. `causal-response-{plan,status,integrity,summary}.json` and four compact
signal traces retain the inputs, exact-prefix proofs, sensitivities, failed
counterfactual endpoints and measured crossings. The reusable analyzer tests
reject transient and wrong-way crossings and verify yaw wrapping and skipped
drawn frames; those checks are included in browser CI.

The steering entry's evidence/comparison panel now shows all four directed
response times and their detection/stop limitations. Three catalog tests pass,
including rejection of invalid response metadata. The isolated 18-entry viewer
passes every Load and run, full steering replay, real WebM export, desktop and
phone response-panel checks, filters/comparison, and modified-recipe rejection.
`causal-response-browser-ui.json` binds these checks to the exact served bundle;
no new controller or physical qualification is implied by the UI change.

## Removing opposing feedback does not cure the loaded-foot motion

The moving direct-transfer target decomposition reconstructs the actual servo
targets to below 1e-10 rad. Body and foot correction vectors oppose one another
in every moving shift, raise, lower, return and settle sample. Shift's aggregate
cosine is **-0.759**, with **64.6%** norm-weighted cancellation. These are angular
target suggestions, not opposing physical-force measurements.

The first three zero-gain attempts were rejected before physics: the controller
schema fixed both gain inputs at 0.25. Their rejected captures and audit remain
versioned. A new explicit experimental input profile lowers only those two
input minima to zero, preserving maxima/initial values, physical actuator
limits and task gates. Its bounds-only baseline reproduces every prior physical
frame and transition exactly. All gain changes below apply only during nonzero
motion requests; original stopping inputs and the settled integral are restored.

| Moving feedback | Travel mm/s | Qualified swings | Stop mm | Heading rad | Worst-foot motion / body advance | Total foot motion mm | Shaft work J |
|---|---:|---:|---:|---:|---:|---:|---:|
| Original body + foot | 3.696 | 41/41 | 0.753 | 0.01361 | 45.2% | 273.2 | 6.244 |
| No body correction | 3.643 | 41/41 | 1.465 | 0.01095 | 45.4% | 273.5 | 6.310 |
| No foot correction | 3.700 | 41/41 | 0.782 | 0.01297 | 45.4% | 286.5 | 6.153 |
| Joint tracking only | 3.650 | 41/41 | 1.491 | 0.01020 | 44.5% | 284.0 | 6.236 |

Every case fails heading and the unchanged 5% contact-motion screen. Removing
body feedback also fails the 1 mm stop-position limit. Removing foot feedback
increases total contact motion. Thus the observed target cancellation does not
justify simply dropping a correction or escalating its gain. The remaining
stance motion persists under the feedforward joint trajectory and needs a
closer examination of contact-force distribution and contact-model behavior.
No ablation is promoted to the browser or ranked.

`feedback-ablation-{open-plan,open-status,open-integrity,summary}.json` and
per-case contact, phase, target-contribution and motion reports retain all four
physical outcomes. The audit now accepts a correctly omitted empty input-event
history after input rejection; a focused test also verifies that a fabricated
event is rejected. The correction-vector analytic test and audit regression
test pass and are included in browser CI.

`feedback-ablation-recipes.json` durably references the versioned scene,
configuration and task and stores all sparse inputs and the explicit input-bound
changes. `node examples/full-robot/whole-swing/reproduce_feedback_ablation.mjs
NEW_DIRECTORY` reconstructs all five recipes, including the original-bound
baseline, without a prior `runs/` tree. All reconstructed scenes, configurations,
tasks, seeds and dense action arrays match the measured recipes exactly;
`feedback-ablation-reproduction-check.json` records that check.

## Gentler motion separates some contact creep from remaining sliding

The next frozen study repeats the fixed-stride 2× Shift/Return gait with
regularized-Coulomb speed scales of 1, 0.1 and 0.03 mm/s. Its 5.175 mm planned
stride, 1.90 s transfer period, controller, CAD material coefficients, servo
limits, development input minute and push, and 1.25 ms backward Euler settings
are identical across profiles. Only the declared smoothing option differs.
The repeated 1 mm/s baseline exactly reproduces all 3,001 physical frames and
transitions from the previous 2× gait, excluding wall times.

| Smoothing mm/s | Travel mm/s | Qualified swings | Stop mm | Heading rad | Worst-foot motion / body advance | Total foot motion mm | Shaft work J |
|---|---:|---:|---:|---:|---:|---:|---:|
| 1 | 2.741 | 30/30 | 0.283 | 0.000393 | 29.2% | 162.8 | 5.294 |
| 0.1 | 2.738 | 30/30 | 0.335 | 0.000367 | 19.1% | 93.9 | 5.019 |
| 0.03 | 2.729 | 30/30 | 0.442 | 0.000557 | 17.8% | 89.7 | 4.950 |

All three complete the minute and pass the original task checks, with zero
sampled internal contacts and maximum body tilt below 0.00418 rad. All fail
the unchanged **5%** loaded-contact-motion screen. Smaller smoothing changes
the constitutive approximation; it is not a controller improvement or a newly
measured material property. Its benefit is larger here than with the original
fast shifts, but the remaining motion is substantial.

At 0.03 mm/s, Return contributes **42.697 mm** of across-foot contact motion,
Raise **26.056 mm**, Shift **13.207 mm**, and Lower **7.398 mm**. Return plus
Raise account for **76.6%** of the 89.750 mm total. Reducing smoothing from 0.1
to 0.03 mm/s barely changes either Return or Raise motion. This supports
testing a gentler direct support transfer that removes the return-to-center
excursion; phase association alone does not prove its causal effect or predict
that it will pass. The earlier faster direct-transfer gait remains a separate
heading/sliding failure.

Native measured throughput is 0.599×, 0.421× and 0.224×, with p95 transitions
66.9, 113.5 and 233.6 ms. The **0.1 mm/s timing is not an isolated comparison**:
a memory-heavy diagnostic briefly overlapped it. That diagnostic mistakenly
compared frame wall times, was terminated, and was replaced after simulation
with the explicit per-frame physical parity check. The 0.03 mm/s result still
demonstrates a large cost against the repeated baseline. These are native
shared-host observations, not browser realtime tests.

`GENTLER-CONTACT-PLAN.md` and `gentler-contact-{plan,status,integrity,summary}.json`
retain the prospective definitions, outcomes and independent per-profile
audits. The collector asserts identical parsed robot, controller, full config
and completed input histories, with only the declared physics option allowed
to vary. Per-case contact, phase and motion reports retain the measurements.
`gentler-contact-baseline-parity.json` records exact physical repeatability.

`node examples/full-robot/whole-swing/prepare_gentler_contact.mjs NEW_DIRECTORY`
reconstructs all three recipes from versioned inputs without prior captures;
the fresh reconstruction and match to the previous 2× gait's authored recipe
are recorded in `gentler-contact-reproduction-check.json`. The shared contact
kinematics/phase test and rejected-input audit test both pass. No Rust runtime
or live browser bundle changes, candidate promotion, timestep convergence,
fresh held-out, terrain, or hardware qualification are claimed by this study.
