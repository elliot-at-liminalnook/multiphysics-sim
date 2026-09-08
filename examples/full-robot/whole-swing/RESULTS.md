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
