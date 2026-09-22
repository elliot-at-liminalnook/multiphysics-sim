# Continued realtime optimization checkpoint

Goal remains active. Realtime has NOT been achieved.

Latest selected BE6400 recipe: native 22.556381 s for 3 simulated seconds
(0.13300x), after verified restoration from the unsuccessful matrix-kernel trial.
The freshly replayed selected browser bundle takes 26.050920 s (0.11516x), with
208.835 ms p95 policy latency versus 20 ms. These single-run timings show normal
variation; they are not new optimization gains. Rendered command
routing passes, but realtime throughput and latency both fail. The largest
new qualified same-method optimization is guarded SDIRK matrix reuse
(1.593x coarse, 2.235x fine); it does not qualify larger SDIRK timesteps.
Physical calibration and controlled-robot timestep convergence remain unproven.

Detailed receipts and scope limits follow; earlier timings are historical pairs.

The selected physical recipe remains `warm-probes/config.json`. At the earlier
solver-sharing checkpoint, after sharing
one compiled nested solver body across callback types, the fresh native trial
runs 3 simulated seconds in 29.170505958 s (0.1028436x realtime), versus the paired
30.586520104 s baseline. Every physical frame field is exactly identical across
151 frames; see `warm-probes/shared-body.parity.json`. The 4.85% single-pair gain
is below the existing 20% optimization gate; do not advertise it as a new qualified
profile or a repeated-machine benchmark. Sharing the solver also avoids excessive
code duplication when adding multiple integration stages.

Implemented and tested: opt-in direct projected inertia, and opt-in coupled
SDIRK2 motor/mechanics stages using the registered component rate reads to
separate differential and algebraic states. No selected physical recipe changed.
63 focused tests pass; the robot crate compiles for wasm32 without default
features. See `coupled-sdirk2/final-tests.log` and `wasm-check.log`. One earlier
test checked the old error-message substring and was updated for the newly
supported staged motor adapter. The earlier stale compilation was intentionally
cancelled after source changes; it is not a numerical failure.

Screening evidence:
- `continued-optimization.json`: aggregate current results and test receipts.
- `optimization-screen.json`: initial four rejected candidates.
- `inertia-projection/`: 1.019x speedup, physical agreement, below 1.2x gate.
- `solver-secant/`, `solver-predict/`: physically close but slower.
- `step-3200hz/`: 1.54x faster, fails numerical/contact gates.
- `coupled-sdirk2/step-8/`: 17.983 s for 3 s, 1.70x faster, fails physical gates.
- `coupled-sdirk2/step-4/`: 28.952 s for 3 s, also fails physical gates.

The SDIRK2 primitives pass independent circuit/gearbox/load stage solutions,
algebraic-current checks, second-order convergence against an exact RL transient,
and exact FPGA sample/application deadlines and deterministic replay. This is
not whole-robot fidelity qualification. Continuous power/thermal state layouts
and SDIRK contact-impulse quadrature remain explicitly unsupported; requested
unsupported combinations fail rather than omit states or report BE impulses.

## Reference runs completed

Exec session 42980 completed normally; no benchmark from this checkpoint remains
running. Do not restart it. The full 6400 Hz SDIRK2 reference took 97.944 s; the
12800 Hz BE reference took 50.969 s, both for three simulated seconds.
`compare-refinement.mjs` reproduces `refinement-comparison.json`.

The existing 6400 Hz BE baseline itself fails the unchanged comparison gates
against 12800 Hz BE: peak motor angle 0.003297 rad (0.189 degrees), link origin
1.551 mm, current 0.123 A and contact force 27.181 N, with different contact
identities. Controller sample/application counts still match exactly. Neither
reference is established as the continuous-time solution. The fine SDIRK2 result
also differs from refined BE, and both coarser SDIRK candidates fail against the
fine SDIRK run. Thus none of the new larger-step profiles is qualified, and the
original baseline's timestep convergence is also unproven. Do not call these
pairwise differences measured hardware error or infer that either method is
necessarily closer to truth.

## Guarded SDIRK Jacobian reuse

`stage-proposal-reuse/` records a same-method solver optimization. Reuse is opt-in,
requires the existing step-reuse option, carries only a Newton correction matrix,
and retains exact residual/closure validation and cold retries. The affine second
stage discards velocity prediction history. Physical time discontinuities still
invalidate the first-stage proposal, and failed intervals leave committed
workspaces unchanged. The selected BE recipe remains unchanged.

- Coarse SDIRK: paired cold 17.907633 s, reused 11.244452 s, **1.593x** speedup,
  0.2668x realtime. Peak angle difference 3.93e-9 rad, link difference 1.60e-9 m,
  current difference 4.31e-8 A, contact force difference 2.38e-5 N.
- Fine SDIRK: previous cold 97.943629 s, reused 43.831631 s, **2.235x** speedup,
  0.06844x realtime. Peak angle difference 6.34e-9 rad, link difference 1.23e-9 m,
  current difference 3.45e-8 A, contact force difference 4.71e-5 N.
- Both match contact identities and sample/application counts and pass unchanged
  same-method numerical gates. Fine timing was not freshly interleaved with cold.
  Neither clears realtime, and this does not repair the integration-method and
  timestep fidelity failures described above.
- 61 focused tests pass (`stage-proposal-reuse/final-tests.log`), including linear
  independent stages, changed step sizes/commands, discontinuous clock, and failed
  interval rollback. wasm32 no-default-feature check passes. Profiled/unprofiled
  physical frames agree exactly (`profile-parity.json`).
- Profile: Jacobian assembly fell from 11.16 s to 3.45 s; dynamics preparation
  3.99 s and closure mapping 2.79 s now dominate much of the remaining work.
  Buckets nest. All three benchmark runs completed; session 83873 is finished.

`dense-refinement/` contains 161 snapshots per method over 0--0.2 s at
1.25 ms intervals. All 11 overlapping physical/policy frames match each original
capture exactly. For BE6400 versus BE12800, targets first differ in the 21.25 ms
snapshot, an encoder history value differs by one count at 25 ms (motor 11),
and held PWM differs by 0.016 at 26.25 ms. This directly observes an early
quantized controller branch difference; it does not prove which solution is
accurate or account for all later contact divergence. See `comparison.json` and
reproduce with `analyze.py`. No fidelity gate changed.

Next: investigate convergence with matched held-input windows and continue exact
optimizations of the selected BE equations. Guarded stage reuse is now implemented and measured as above; never reuse a
physical endpoint from an affine stage anchor. Multirate coupling remains
unimplemented. Also continue exact optimizations of the selected BE equations.

Do not mark the goal complete, relax failed gates, switch to an unrelated easier
robot, or treat native throughput/wasm compilation as rendered browser evidence.

## Held-boundary diagnostic

`held-boundary-refinement/` deliberately removes the policy and sampled
controller and applies zero winding volts at fixed 293.15 K to all twelve
original motors for 0.1 s. This is unpowered settling, not a replacement walking
profile. All seven windows complete and all sampled contact identities match.
BE6400/12800 angle differences are 1.059e-4 rad; BE12800/25600 differences
fall to 5.659e-5 rad. SDIRK800 and 1600 relative to SDIRK12800 give angle
differences of 1.041e-4 and 2.453e-5 rad respectively, consistent with the
expected refinement trend in this bounded case. Fine SDIRK6400/12800 differ
by 1.118e-6 rad and 0.083 micrometres in link position. Contact resultant
differences are recorded separately from the gait per-contact gate. These
results support further integration research but do not qualify the controlled
robot or establish an exact reference. No timings from this diagnostic count
as performance evidence because compilation overlapped execution.

## Exact kinematic sharing and current browser check

`shared-rigid-kinematics/` shares one immutable kinematics evaluation between
full inertia and forces. All 151 physical frames are exactly unchanged. Paired
native time is 28.238166 s before and 28.251849 s after: no speed gain, below
the 1.2x optimization gate. This structural refactor is not a newly qualified
physical recipe. All 64 focused release tests pass.

The same source builds for browser SIMD/LTO and runs three simulated seconds
in 28.606820 s (0.10487x realtime), p95 225.95 ms. Native/WASM physical fields
agree to numerical precision, including exact held commands/counters, and the
rendered W/A/S/D test passes. Screenshot inspected. No same-source scalar browser
benchmark exists for this compiler experiment, so do not attribute a gain to
compiler flags. The new isolated bundle is served on localhost:4190; earlier
viewer bundles are unchanged. Realtime acceptance still fails.

Next measured target: motor equation allocations. The selected BE profile makes
835596 component calls; `EmbeddedMotorBank::evaluate_with_rates` previously
allocated one residual Vec per motor and a telemetry Vec even when the caller
only used forces/residuals. The current `motor-evaluation-storage/` experiment
writes into the existing output slice and offers a force-only API while still
evaluating/validating every registered output. 38 relevant release tests pass. All 151 before/after physical frames match
exactly, as do profiled/unprofiled candidate frames. Paired time is 28.222062 s
before and 27.295874 s after (1.03393x), below the unchanged 1.2x gate. Native
sim/wall is 0.10991x, p95 218.09 ms. This is not a newly qualified profile.
All native benchmarking is complete; session 5986 finished normally. The
localhost:4190 browser bundle predates this final motor-storage change; do not
attribute its browser timing to this change.

Further investigation identified: the scene already selects
`analytic_motor_jacobian=true`, and `motor.rs::MotorUnit::jacobian` provides
registered state/rate/input derivatives. The condensed motor adapter currently
uses numeric inner derivatives. To use supplied derivatives correctly, retain
the chain rule through current-dependent driver voltage and any continuous
power/thermal boundaries; a fixed-boundary Jacobian alone is insufficient.
Keep numerical fallback for unsupported adapters and validate branch-local
derivatives and complete trajectories before accepting a new profile.

Final current check: wasm32 no-default-feature robot compilation passes after
the motor-storage change (`motor-evaluation-storage/wasm-check.log`). No running
benchmark requires recovery or restart. The isolated localhost:4190 static
viewer server remains available. No claim of mathematical or physical maximum
throughput is supported; substantial optimization work remains.

## Registered derivatives: measured and rejected

The nested solver can now use registered motor partials plus voltage/temperature
chain derivatives from independent boundary adapters. Unknown/shared-power
adapters remain numerical; invalid or failed supplied matrices retry cold with
the original numerical derivatives. The runtime now honors the existing scene
analytic-motor flag instead of hardcoding it off. The new solver option defaults
false. Seventy tests pass, including nonlinear derivative checks, both unknown
coordinate choices, BE/SDIRK, driver foldback and invalid/singular fallback.
The full WASM runtime compiles.

`registered-motor-derivatives/` is **not selected**: the supplied route takes
28.997610 s versus the same-binary numerical route's 27.240909 s (0.9394x).
It matches the trajectory to numerical precision; all contact identities and
controller counts match. The default path exactly matches the previous binary
in all 151 physical frames. The profile confirms 20390 supplied matrices and
7.4% fewer component calls, but derivative preparation costs 0.613 s and does
not produce a speed win. All benchmark processes have completed.

## Fine SDIRK reference also fails refinement

`sdirk-refinement-12800hz/` completes the full three-second 12800 Hz SDIRK run.
Against 6400 Hz, peak differences are 0.010859 rad (0.622 degrees), 4.977 mm,
0.15277 A and 8.5205 N, with different contact identities. Counters match.
No reference is yet demonstrated converged for this controlled trajectory.
The held-zero-voltage 0.1 s diagnostic does not resolve this full-run sensitivity.
`compare-refinement.mjs` now accepts explicit candidate, reference and output
prefixes so timestep reports do not overwrite solver-optimization receipts.

Next: prioritize the repeated mechanical closure/dynamics evaluations rather
than this slower derivative route. Also distinguish solver-tolerance sensitivity
from timestep/controller sensitivity before treating a finer trajectory as truth.
One possible numerical-only experiment is a cheap implicit velocity predictor
followed by the unchanged exact solve from the original state; any predictor
must remain private, use registered loads, and never commit approximate physics.
The predictor has now been implemented, tested and rejected for performance;
see the completed checkpoint below.

## Private mechanical predictor: measured and rejected

`mechanical-predictor/` preserves an opt-in frozen-mechanics predictor that
returns only an initial velocity guess. Exact correction still starts from the
original state and enforces the existing closure/contact/solver checks. Failed
prediction or correction falls back, and failed intervals preserve the caller's
workspace. The option defaults false and the selected recipe is unchanged.

The candidate takes 34.427664 s versus 27.968132 s for the same-binary baseline
(0.81237x), so it is not selected. Physical errors remain below all gates:
1.478e-8 rad, 4.973e-9 m, 1.150e-7 A and 5.208e-5 N; contacts and controller
counts agree. Its 7.117 s of predictor work saves too little exact solving.
There are 19020 used guesses and 180 fallbacks out of 19200 attempts.

All 94 focused tests pass, with additional fixed/rotating linkage coverage and
independent BE/SDIRK motor solutions. Full sim-web wasm32 compilation passes.
All 151 default-path frames and task transitions exactly match the preserved
binary, and profiling leaves both modes exactly unchanged. Benchmark session
89949 completed normally. The browser bundle is unchanged because this candidate
does not qualify for selection. Realtime remains unachieved.

## Tighter nonlinear solves do not resolve timestep sensitivity

`solver-tolerance-refinement/` completes four additional three-second runs with
the same preserved binary and prediction disabled. At fixed 6400 Hz, tightening
absolute/relative Newton tolerances from 1e-5 to 1e-7 changes angles by at most
6.938e-9 rad and contact forces by 3.653e-5 N. At fixed 12800 Hz, the changes
remain tiny (1.357e-8 rad, 1.919e-4 N). Saved FPGA states and commands match
exactly, as do contacts and sample/application counts. Tightening costs runtime.

With tight 1e-7 solves, halving the timestep still changes angles by 0.003297065
rad, link positions by 1.550790 mm, currents by 0.1228554 A and contact forces
by 27.18115 N, with different contact identities. This is essentially the
original discrepancy; insufficient nonlinear convergence does not explain it
at these tested tolerances. Neither integration trajectory is established as
truth. The fresh default-tolerance 12800 Hz run exactly reproduces the historical
reference's 151 frames and task transitions.

`analyze.mjs` verifies physical-input and binary equality and reproduces all
six comparisons. Sessions 84743 and 86454 completed. No simulation or build is
pending at this checkpoint. The optimization goal remains active; mathematical
maximum throughput and realtime have not been established. Next work should
target exact closure/dynamics costs and timestep/hybrid sensitivity, retaining
the current gates and rejected candidates.

## Analytic mechanism motion: incremental gain below promotion gate

`analytic-mechanism-motion/` extends the shared certified mechanism component
to supply tangent and curvature through its existing analytic derivatives.
Original numeric singular-value rank checks, tangent-direction checks and all
original position/velocity/acceleration closure equations remain enforced.
The option defaults false and requires complete analytic-position coverage.

Native analytic motion takes 25.371740 s versus 28.069789 s for the same-binary
numeric route (1.10634x; 9.61% less wall time). It matches the trajectory to
roundoff: maximum 2.442e-14 rad, 7.485e-15 m, 1.485e-13 A and 7.190e-11 N;
contact identities and controller counts match. Adding existing projected
inertia takes 26.119023 s, so the combination is slower than analytic motion
alone. Neither passes the unchanged 1.2x optimization gate or realtime gates;
neither is selected. The selected recipe remains `warm-probes/config.json`.

95 tests pass, including expanded branch/base/rank/factorization coverage and
signed transmissions with reordered independent coordinates. Full sim-web
wasm32 compilation passes. All 151 default-path frames and task transitions
exactly match the preserved binary; profiling leaves every measured route
exactly unchanged. Closure-mapping profile time falls from 8.774 to 6.728 s,
while dynamics preparation remains about 9.2 s. Those two areas remain major
costs. Source/input/binary identities and all failed-gate receipts are preserved.

The new and preceding predictor snapshot directories had a basename collision
between source and test `embedding.rs`. Both source copies were restored from
prior snapshots matching the originally recorded hashes; distinct test filenames
and explicit path maps now preserve both files. All affected hashes verify.

Sessions 2737 and 76956 completed; no benchmark or build is pending. The browser
bundle is unchanged. Realtime and maximum attainable throughput remain unproven,
and the goal remains active.

## Contiguous rigid motion columns and refreshed browser bundle

`native-cpu-sample/` records an OS CPU sample with substantial allocator activity.
It includes initialization and is not a steady-state allocation-time estimate;
sampled wall time is excluded from speed comparisons. The sampled trajectory
exactly matches all saved unsampled frames and task transitions.

`contiguous-motion-storage/` replaces per-link growing motion-column vectors
with one contiguous buffer and topology-derived ranges. Column order and every
arithmetic operation are preserved. The shared full inertia and original closure
Jacobian kernels both benefit; no physical equations or couplings are removed.
54 targeted tests pass, including mixed/branched joints, multiple free/grounded
bases, inverse dynamics, energy, closure and invalid inputs. Full sim-web wasm32
checks and an actual SIMD/LTO browser build pass.

Paired native: 27.234492 -> 24.949506 s (1.09158x), exactly identical in all 151
saved frames and task transitions. Combining prior analytic motion gives
23.533808 s (1.15725x), with only roundoff differences. Neither passes the 1.2x
gate, so the analytic option remains disabled. The selected physical recipe is
unchanged. Profiling leaves both trajectories exactly unchanged.

Fresh browser worker: preserved bundle 28.767835 s, current selected recipe
26.958385 s, current analytic experiment 24.971780 s. The old/current default
workers agree exactly in every saved frame and task transition; native/WASM
differences are roundoff-sized, with exactly equal FPGA states and commands.
The full comparison accounts for the native harness's separate transition array
and the worker's frame.learning, including their reset record. Both realtime
throughput and p95 latency fail (current selected p95 216.26 ms versus 20 ms).

The new local bundle at http://127.0.0.1:4191 passes rendered W/A/S/D/stop routing.
The status label now reports the measured 0.11x browser-physics result separately
from live rendering, while retaining provisional calibration and non-realtime
warnings. Complete binary/source/build/recipe receipts and screenshots are saved.
The preserved older bundle remains at port 4190. Native and worker benchmarks
completed; only static viewer servers may remain running. The goal stays active.

## Contiguous joint axes

`contiguous-joint-axes/` removes per-joint axis buffers from geometry-only
kinematics through one contiguous buffer and joint ranges. All arithmetic and
public diagnostic vectors are preserved. Fresh native pair: 25.018987 ->
23.895854 s, 1.04700x (4.49% less wall time), with all 151 physical frames and
task transitions exactly equal. Existing analytic motion plus this change takes
21.704751 s, 1.15270x, and agrees to roundoff. Neither clears the unchanged 1.2x
promotion or realtime gates; analytic motion remains unselected.

110 tests pass; one preexisting experimental hybrid SDF derivative promotion
test remains ignored because it fails its independent central-stencil audit.
The ignored test is not counted as passing. Full sim-web wasm32 compilation
passes. No new browser bundle was measured for this change; port 4191 still
serves the preceding motion-column build. Profiles and source/input/binary
identities are preserved, and default/profile trajectories are exactly equal.

## Hybrid motor workload visibility

`hybrid-solver-workload/` exposes the already-retained successful motor trial
statistics through shared read-only session/environment getters and standalone
profile output. Serialization happens after the timed run; stepping is unchanged.
Native and wasm32 checks pass, and all 151 physical frames and task transitions
exactly match the preceding executable.

The selected three-second replay has 19,200 successful trials, zero rejected
trials, 17,311 stages starting from a reused outer matrix (90.16%), and 91,123
outer Newton iterations. It performs 179,489 endpoint/auxiliary solves, 835,596
component evaluations, 414,653 auxiliary Newton iterations, and 160,289 dynamics
preparations. The existing accepted_implicit_steps array is empty for this
separate hybrid path; the new statistics cover every outer interval. See the
reproducible analysis receipt for scopes, maxima and scheduler reconciliation.

This is a diagnostic change, not a new physical or performance profile. Native
profiling completed in 24.485446 s; use the preceding unprofiled pair for speed
claims. No build or benchmark is pending at this checkpoint. Realtime and
maximum attainable throughput remain unproven; the goal remains active.

## Shared force outputs and current browser build

`force-output-storage/` separates private contiguous joint torque storage from
public diagnostic packaging in the shared articulated force kernel. Prepared
dynamics borrows its kinematics and consumes flat torques directly. All force,
constraint, contact/history and modal calculations remain shared and unchanged.

188 robot tests pass; one preexisting experimental hybrid SDF derivative audit
remains ignored and is not counted as passing. Native release and actual SIMD/LTO
WASM builds pass. Native paired wall time falls 23.862128 -> 22.967649 s (3.75%
less, 1.03895x), with exactly equal physical frames, task transitions, motor
solve statistics and interval diagnostics. Existing analytic motion yields
20.928730 s (1.14016x), with roundoff differences; it remains unselected. Both
fail the unchanged 1.2x promotion and realtime gates. Force-evaluation profile
time falls from 3.240 to 2.598 s; nested profile timers are not additive.

The fresh browser pair compares the preceding motion-column bundle with the new
joint-axis + force-output bundle: 27.025395 -> 26.637505 s (1.01456x). This small
single-pair change is not a robust browser gain estimate; p95 latency is slightly
higher (217.335 vs 217.105 ms). Analytic motion takes 23.343490 s (0.12852x).
Old/current browser physical frames and transitions agree exactly. Native/WASM
agreement covers all 151 frames and complete task transitions within 1e-7.

The new local bundle at http://127.0.0.1:4192 passes rendered W/A/S/D/stop routing
with 10 actions, 11 frames and no errors. The screenshot was freshly inspected;
it retains the accurate rounded 0.11x physics benchmark label and separately
reports 0.10x live rendering. Source/input/artifact/module hashes all verify.
All builds and benchmarks completed; only static viewer servers remain. Realtime,
controlled-robot timestep convergence and maximum attainable speed remain
unproven. The goal remains active.

## Matrix lifetime sweep: no qualified improvement

`jacobian-use-limit/` adds a validated optional limit on numerical matrix use,
retaining 64 by default. It is checked between stages/steps; original physical
acceptance, stale-matrix refresh, backtracking and transactional invalidation
remain unchanged. Effective-limit changes invalidate the numerical proposal.
56 focused tests pass, including independent analytic spring-mass solutions,
exact omitted/explicit-default equivalence, policy invalidation and rollback.
Native release and full sim-web wasm32 checks pass.

Same-build default wall time is 22.389637 s. Lifetimes 16/32/128/256/1024/4096
measure 26.801611/24.186997/22.122599/22.275917/23.983131/22.780078 s.
None clears the unchanged 1.2x promotion gate or realtime gates; default remains
selected. At 256 uses, endpoint evaluations drop 6.2% but Newton iterations rise
12.9% and full closure mappings rise 9.1%, eliminating the expected large gain.
At 16, fewer Newton iterations are outweighed by 36.1% more endpoint evaluations.

All alternatives preserve exact saved FPGA states/commands and contact identities,
with zero rejected trials. Maximum differences are 1.122e-8 rad, 4.036e-9 m,
1.174e-7 A and 4.340e-5 N; all same-timestep physical gates pass. Default output
and all retained diagnostics match the preceding binary exactly; profiling leaves
every measured trajectory and task transition unchanged.

The 1024/4096 runs have exactly equal physical output and solver workload yet
noticeably different timing, demonstrating variability in single-run measurements.
Do not call the 128-use 1.2% difference a robust improvement. All raw runs, source
snapshots, binary/input hashes and rejected qualification receipts are retained.
No browser rebuild was needed for unqualified policies; port 4192 is unchanged.
All builds and measurements are complete, and the goal stays active.

## Browser CPU profiling and rejected matrix kernels

`browser-cpu-profile/` adds a reusable actual-worker CDP CPU profiler at
web/tests/profile-worker-cpu.mjs. It starts after model construction and records
the unchanged three-second replay. 19,501 samples cover 28.102 weighted seconds,
including idle/protocol overhead. All 151 saved physical frames and full task
transitions exactly match the unprofiled reference. Generic masked GEMM has
2.627 s leaf weight, mainly under inertia projection and closure-direction
checks; kinematics and allocation are also prominent. Sample weights are
approximate, inclusive callers overlap, and profiled timing is not acceptance.

`direct-dense-products/` tested shared unpacked finite products that skip only
exact zero coefficients. 3 kernel tests, one documented example and 108 robot
tests pass, including independent constrained KKT, pendulum and contact balance.
Native default/direct/direct+analytic wall times are 22.810525/23.946855/21.232350 s.
Browser times are 25.714915/25.612125/23.796865 s. No candidate passes the unchanged
1.2x speed gate or realtime gates. Physical differences are roundoff-sized,
contact identities/cadence agree, and all native/worker fields and full task
transitions agree within 1e-7. Profiling preserves each trajectory exactly.

A second CPU sample shows 1.627 s in the new product and 1.469 s in its transpose
product, replacing the old generic kernel and supporting work without reducing
full runtime. The unsuccessful simulation source/test edits were therefore
restored exactly, and the new active kernel file was removed after hash checks.
All candidate sources, binaries, WASM, build manifests, CPU profiles and rejected
qualification receipts remain preserved. No unrelated edits were reverted.
The reusable CPU-profiler tool is retained; selected recipe is unchanged.

The restored source rebuilt and replayed successfully: all 151 physical frames
and task transitions exactly match the pre-experiment binary. Port 4192 remains
the selected preserved browser bundle; port 4193 serves only the archived
experimental binary with its default recipe. No build or measurement is pending.
Realtime, controlled-robot timestep convergence and maximum attainable speed
remain unproven. The goal remains active.

## Isolated outer secant updates

`outer-secant-only/` separates outer mechanical Newton secants from inner motor
secants using the existing auxiliary override. The previous combined-secant
experiment did not test this separation. All variants retain BE6400, exact
residual checks, numerical tolerances, FPGA deadlines and the original gates.

A fresh ordered native pair takes 22.763944 s for the default and 21.014733 s
with outer-only secants (1.0832x). Including negligible-step updates instead
slows the run to 24.147557 s. Combining outer-only secants with the existing
analytic mechanism motion option takes 19.126857 s (1.1902x), still below the
1.2x optimization gate. These are single-run screens, not repeated estimates.
All variants pass physical comparison gates, preserve exact saved FPGA states
and commands, and have exact profiled/unprofiled captures. The selected recipe
remains unchanged. Outer Newton iterations fall from 91,123 to 66,375, but
3,194 secant requests are rejected or capped. `outer-secant-lifetime/` is an
in-progress, source-pinned test separating cap hits and varying the update cap
without changing unsafe-update checks. Results must be recorded before drawing
any performance conclusion about that experiment.
