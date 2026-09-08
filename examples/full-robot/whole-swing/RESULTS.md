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
