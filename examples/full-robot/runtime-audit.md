# Robot runtime audit — 2026-09-05

The dominant cost is repeated whole-robot/contact evaluation during Jacobian
assembly, amplified by nonlinear convergence work. The current hold controller
and frame serialization consume a negligible fraction of the measured runtime.

This audits the existing full floating robot, not the pendulum portability demo:
29 merged links, 28 mechanical DOFs, 12 actuators, 195 compiled behaviors and
600 solver unknowns. The articulated component owns 156 states. Contact is on,
modal flexibility is off, the physics step is 0.5 ms, reporting interval 1 ms,
the session action/reporting period is 2 ms, and the Rhai hold-controller period
is 20 ms. Motor firmware samples every 1 ms. The experimental hybrid Jacobian is off.

Hardware: Intel Core i9-9980HK, 8 physical / 16 logical cores; macOS, Rust 1.97.1,
release build, working tree based on `5c4c1a7` with uncommitted runtime changes.
The CAD file and physics equations were not modified for this audit.

## Measured costs

Two unsampled runs each advanced 20 ms with identical final frames and identical
solver work counts. Stepping took 20.67 s and 19.09 s: approximately 1,000 times
slower than real time. The final frame exactly matches the prior numerical
baseline. The table uses the second run.

| Work | Time | Share of session stepping |
|---|---:|---:|
| Jacobian assembly | 17.732 s | 92.9% |
| Other residual evaluations / line searches | 0.792 s | 4.15% |
| Matrix factorization | 0.495 s | 2.60% |
| Back-substitution | 0.0194 s | 0.10% |
| All event callbacks, including controller/firmware | 0.00110 s | 0.0058% |

Timers are nested; do not add the parent implicit-solve/step timers to these.
Event location took 0.084 s, overlapping solver work. Callback time is an upper
bound on the runtime controller callback cost, not an isolated Rhai benchmark.
Controller compilation/initialization is included in startup instead.

Reading/parsing the exported scene took about 33 ms in the first run. Building
and initializing a session took 1.11–1.17 s. These are one-time costs outside the
stepping table; they exclude CAD geometry derivation/export and compilation of
the executable.

## Why it is expensive

1. **Each numerical derivative repeats the large articulated computation.**
   The compiler's fallback perturbs state/rate inputs and reevaluates the entire
   behavior. The robot is one large behavior, including collision detection,
   kinematics, inverse dynamics and loop constraints. Even inputs that do not
   change collision geometry take this path.
2. **Contact is expensive within that computation.** At the same final state,
   1,000-call measurements put a full evaluation at 90–140 microseconds and the
   same evaluation with contact omitted at 14–19 microseconds: 6.6–7.5 times
   cheaper. This diagnoses per-evaluation cost; it is not an alternate simulated
   trajectory or a proposal to remove contact. Sampling independently shows
   distance-field queries and articulated evaluation on the hot stack.
3. **The solver repeats the derivative work extensively.** Forty nominal physics
   steps generated 100 implicit Newton calls, 1,326 Newton iterations, 470 fresh
   Jacobians, 9 subdivision events and 230 discrete events. Event location and
   internal splitting contribute to these counts; 100 calls does not mean 100
   failed attempts. There were no outer slice retries and no successful branch
   restarts. The first 2 ms frame needed 13 Jacobians; the last needed 117.
   Per-build cost remained about 37–40 ms. The final three frames accounted for
   278 rebuilds (59% of the total). Their end-of-frame contact counts change from
   three to zero to one; the precise cause of difficult convergence is not yet
   localized.
4. **Parallelism is poorly balanced.** A separate five-second stack sample found
   one Rayon worker in articulated evaluation in 3,137 of 3,722 samples. The
   other 15 workers were waiting in roughly 99% of their samples, while the main
   thread waited in 93%. The code parallelizes across behaviors, but the large
   robot behavior's derivative-column loop is serial. Sampling percentages are
   per-thread observations, not percentages of total CPU work across threads.
5. **Reporting repeats some physics work, but its current cost is small.** A
   complete frame snapshot costs 0.125–0.135 ms; JSON serialization 0.018–0.020 ms.
   Pose extraction currently calls the full contact-free dynamics evaluator,
   and contact reporting calls the full evaluator again. This is avoidable, but
   is far below the seconds spent solving each frame. No renderer was running
   in these measurements: CAD GUI/rendering performance is a separate audit.

## Work priority

1. Separate reusable geometry/contact work from acceleration and reaction-force
   evaluation, and avoid unnecessary repeated calculations during differentiation.
   Cache validity must respect positions, velocities and contact branch changes.
2. Diagnose the difficult Newton steps using residuals, conditioning, active
   branches and work counts. Promote analytic/hybrid derivatives only when full
   trajectories pass accuracy checks **and** reduce total solver work/time.
3. Balance the heavy derivative work across workers, then measure overhead and
   deterministic results. Adding threads to the existing per-behavior split
   alone will not split the dominant task.
4. Deduplicate reporting/pose evaluation after the solver bottleneck is addressed.

The experimental hybrid remains opt-in: its earlier run stalled after 2 ms and
was interrupted after about two minutes. Its individual derivative checks do not
establish faster or robust full-robot integration. Controller rewriting, GPU
rendering changes and matrix-solver replacement are not supported as first
priorities by this profile. Future learned policies may have different costs;
this measurement covers the current simple hold policy.

## Reproduce

```sh
cargo build --locked --release -p sim-runtime --bin sim-profile
./target/release/sim-profile runs/full-robot/reproduced.scene.json 10 1000 > profile.json
```

The CLI records effective scene options, seed, startup time, per-frame solver
work, aggregate nested timers, fixed-state microbenchmarks and the final frame.
It does not change the scene or CAD file. Optional arguments are frame count and
microbenchmark repetitions. An external process timeout is appropriate when
profiling an experimental solver path that may struggle to converge.

Local evidence: `runs/full-robot/runtime-audit/profile.json`,
`profile-repeat.json`, `stacks.txt` and `stack-summary.json`.
Scene SHA-256:
`64fc325ae909195a50dbfe751af0e23daabed400266dd9a0c5f7f74930ccebf9`.
These are local audit results, not a completed CI performance gate or evidence
of standing/walking accuracy.

## Follow-up: parallel columns and scoped contact reuse

The numerical fallback now divides large behaviors' input columns into ordered
chunks of eight on native Rayon workers. Each chunk owns its scratch buffers;
the one-thread and WASM paths stay serial. Column order and perturbation arithmetic
are unchanged. This also retains parallel work across smaller behaviors.

`Behavior::prepare_linearization` optionally provides an immutable evaluator for
one Jacobian assembly. The articulated implementation reuses contact forces,
contact points and bristle rates when position, orientation, velocity, modal
deformation/speed and bristle states are unchanged. Acceleration and reaction
perturbations can reuse them. Changed dependencies recompute contact. Signed zero
is distinguished, and no cache survives into the next linearization or step.
Gravity accumulation order is preserved to avoid changing floating-point results.

Sequential runs on the same machine, same 20 ms scene, release build:

| Implementation | Workers | Session stepping | Jacobian assembly |
|---|---:|---:|---:|
| Parallel-column implementation, contact recomputed | 1 | 20.406 s | 19.013 s |
| Parallel-column implementation, contact recomputed | 16 | 4.314 s | 2.894 s |
| Parallel columns + scoped contact reuse | 1 | 15.186 s | 13.411 s |
| Parallel columns + scoped contact reuse | 16 | 3.902 s | 2.251 s |

The combined change is **5.2× faster** than its one-worker uncached control in
this short workload. Every final frame exactly equals the earlier numerical
reference. All four runs still use 100 Newton calls, 1,326 iterations, 470 fresh
Jacobians, nine subdivisions and 230 events. **Convergence work has not improved**;
the same work is cheaper. This is still about 195× slower than real time and is
not a long-trajectory performance result. Timings are individual observations,
not confidence intervals. Articulated hybrid derivatives remain disabled.

Evidence: `runs/full-robot/solver-performance/{parallel-1,parallel-16,reused-1,
reused-16}.json`. Set `RAYON_NUM_THREADS=1` or `16` before invoking `sim-profile`
to reproduce the worker comparison with the current implementation. Reproducing
the uncached measurements requires the implementation before scoped reuse.

Validation completed:

- Release compiler, robot-domain and runtime suites; serial compiler suite with
  `--no-default-features`.
- A nonlinear 64-state compiler fixture produces bit-identical derivative
  triplets with one, two, four and eight workers, and passes the independent
  residual-based derivative checker.
- Prepared versus uncached robot residuals agree bit-for-bit for every local
  state/port/signal/rate perturbation in both directions, including flex, loaded
  loops, cables, moving contact, and an airborne/reversed-velocity state.
- Release WASM build and real Chrome worker test: native/browser maximum
  difference `7.16e-18`, exact replay, 0.4 simulated seconds in 170 ms, with the
  main thread responsive. This browser case is the pendulum, not the full robot.
- CI now includes the parallel-column test; remote CI has not run.

### Convergence investigation still open

`SIM_NEWTON_TRACE=1 ./target/release/sim-session ...` localizes all nine failed
solves to rows including the **angular closure equations of the curved knee
links**. Seven failures exhaust line search and two reach the iteration limit.
Some raw angular residuals are only around `1e-12`–`1e-8`, while estimated
reaction corrections remain large. A small raw residual alone is not an
accuracy criterion for this ill-conditioned system.

The CAD joint paths between each loop's endpoints contain two revolute joints
parallel to the loop axis and a prismatic foot slide. Thus the rigid topology
appears to impose the angular alignment already; finite differences of nearly
identically zero closure expressions can amplify rounding noise relative to
the `1e-6` constraint-force-mixing term. This is a diagnosis to verify, not yet
an accepted residual/Jacobian change. Modal boundary rotations can invalidate
such redundancy, so any elimination must establish it from the shared mechanism
topology and active flexibility. No solver tolerances, constraint compliance or
model geometry were relaxed in this follow-up.

Next: verify redundant-row handling on rigid and flexible library fixtures,
then compare convergence counts, closure error, physical state and wall time
over complete recorded trajectories. Promotion of the experimental hybrid still
requires independent numerical comparisons, timestep refinement and end-to-end
timing improvement; the existing 2 ms robot comparison is insufficient.

## Follow-up: structural identities and trajectory comparisons

An experimental library implementation now certifies angular loop identities
from the compiled tree. It transports each loop axis to the common ancestor,
accepting only translations and rotations exactly parallel to that axis; modal
boundaries and even a `1e-9` axis tilt reject the certification. Shared ancestor
motion is allowed. Certified angular residuals evaluate their existing CFM term
directly, preserving multiplier lanes and state ordering. No compliance or solver
tolerance changes. The matching hybrid derivative rows use the same identity.

**This remains opt-in:** component parameter `loop.structural_identities=1`, or
recorded runtime option `structural_loop_identities:true`. Both default to false.
The proof covers a structural identity, but whole-trajectory integration still
needs validation before this becomes a default optimization.

At the original 0.5 ms timestep, the prototype advanced 20 ms in 1.624 s with
250 fresh Jacobians, 1,139 iterations and three subdivisions. Its final positions
differ from the prior baseline by up to about 10 micrometers; the different step
subdivisions make this an accuracy comparison, not a bit-identical speedup.
Independent whole-residual numerical derivatives disagree with the component
path on 1,052 of 12,642 values over the trajectory, including about 0.89 N of final
foot normal force. **That result fails the existing accuracy gate.**

Equal-step comparisons over the complete 20 ms recording, unchanged tolerances:

| Timestep | Mismatches | Compared values | Subdivisions: component / numerical |
|---|---:|---:|---:|
| 0.5 ms | 1,052 | 12,642 | prototype report predates per-frame work logging |
| 0.25 ms | 1 | 12,638 | 1 / 1 |
| 0.125 ms | 0 | 12,638 | 0 / 0 |
| 0.125 ms, hybrid derivative candidate | 0 | 12,638 | see recorded per-frame work |

The single 0.25 ms mismatch is a knee reaction force: 11.72 micronewtons error,
versus a 4.76 micronewton tolerance. At 0.125 ms the largest normalized error is
0.0253, comfortably below one. These are agreement checks between derivative
methods at the same timestep; they do not establish timestep convergence.

**Timestep refinement fails:** comparing component derivatives at 0.125 ms with
whole-residual numerical derivatives at 0.0625 ms gives 4,434 mismatches. The
smaller-step run also reports **242 events versus 230**, with the extra block of
12 appearing by 12 ms. Force differences include changes of sign. This warrants
investigation of clock/event boundary semantics before interpreting refinement
as a clean discretization-error measurement. The implementation detects strict
negative guard values, and firmware clocks use `next_tick - time`; an exact
reporting endpoint can therefore be treated differently from one reached after
floating-point rounding. This is a lead, not a verified fix or sole cause.

The comparison report now includes per-frame mismatch counts, maximum normalized
errors and each island's cumulative solver statistics. This makes divergence of
the integration/event histories visible instead of reporting only the worst
final differences.

The hybrid's numerical remainder also now uses ordered native parallel chunks,
with serial/no-default-feature/WASM fallback. It previously bypassed the compiler
fallback's column parallelism. The 0.5 ms hybrid case took 11.04 s before this
change and 5.80 s afterward with 16 workers; one worker afterward took 26.98 s.
All three final frames are exactly equal, with 400 Jacobian builds and six
subdivisions. These individual local timings are not a promotion result: the
hybrid remains slower than the non-hybrid structural-identity prototype.

Tests include a five-second moving four-bar trajectory with closure below one
micrometer and every sampled joint angle agreeing within one microradian across
baseline, structural-identity and hybrid modes. A loaded flexible/contact fixture
has bit-identical hybrid triplets with one, two, four and eight workers. The robot
domain and runtime release suites pass. These focused tests do not substitute
for the unresolved full-robot refinement check.

Local evidence under `runs/full-robot/solver-performance/`:
`loop-identities-16.json`, `loop-identities-compare.json`,
`loop-identities-h{2,4}-compare.json`, `identities-hybrid-h4-compare.json`,
`identities-h4-refinement.json`, and `parallel-hybrid-{1,16}.json`.
The first prototype profile predates the explicit flag; use
`loop-identities.scene.json` to select the same identity path in current code.
Both experimental options stay off by default. Next priorities are deterministic
clock/event boundaries and longer recorded trajectories, followed by promotion
only when accuracy and total runtime improve together.

## Research-guided reprofile and next experiments

The user's research review prioritizes fewer residual calls, verified constraint
independence and fewer rebuilds. The current library already implements greedy
column coloring (`sim-dynamics/src/jacobian.rs`), modified Newton reuse, row
equilibration and backtracking (`sim-solve/src/lib.rs`). The next work should
measure and improve their applicability, rather than duplicate them.

The [PETSc coloring documentation](https://petsc.org/main/manual/snes/) requires
same-color columns to have disjoint residual support. The
[IDA numerical-method documentation](https://sundials.readthedocs.io/en/latest/ida/Mathematics_link.html)
describes refresh after stale-Jacobian failure and separates nonlinear convergence
from time-integration error control. These support focused experiments on the
existing implementation, not a replacement solver decision.

Two sequential paired runs with the current binary, 16 workers and the same
20 ms workload, with no concurrent build/test process:

| Work | Accepted FD + reuse | Opt-in structural identities |
|---|---:|---:|
| Session stepping | 2.644 / 2.683 s | 1.393 / 1.388 s |
| Jacobian assembly | 1.535 / 1.561 s (~58%) | 0.817 / 0.812 s (~59%) |
| Other residuals | 0.641 / 0.642 s (~24%) | 0.315 / 0.315 s (~23%) |
| Factorization | 0.421 / 0.429 s (~16%) | 0.225 / 0.225 s (~16%) |
| Back-substitution | ~0.015 s | ~0.012 s |
| Component FD residual calls | 565,880 | 301,000 |
| Other full-residual calls | 5,336 | 2,629 |
| Fresh Jacobians | 470 | 250 |

Both pairs reproduce the respective previous final frames exactly. Shorter wall
times than earlier observations are not attributed to a new algorithmic gain;
solver work is unchanged and workstation conditions vary. The paired experimental
gain is about 1.9× here, versus 2.4× in the earlier observations. Desktop apps
remain running; these are not isolated-machine confidence intervals.

The new FD counter includes component fallback baselines and state/rate probes,
counted in batches to avoid an atomic operation per probe. It excludes probes
inside supplied hybrid derivative implementations. It must not be read as the
count of all behavior evaluations in the process.

The reduced implicit-system pattern has **600 unknowns, 50,476 structural entries
and 312 greedy colors** in both paths. It conservatively unions state and rate
dependencies. This gives a potential 600-to-312 probe reduction for a hypothetical
uncolored whole-system difference, not a predicted speedup over the current
component-local assembly. A whole-system probe evaluates all behaviors, whereas
the current compiler evaluates only the affected component. More precise verified
row dependencies within the large articulated component are the likely useful
coloring experiment. Contact-mode changes must not invalidate that pattern.

Evidence: `runs/full-robot/solver-performance/reprofile-{baseline,identities}-{1,2}.json`.
`sim-profile` now reports the structural pattern metrics and component FD counts.
Next: resolve clock/event boundary consistency, audit residual support on saved
states and contact transitions, then compare local coloring and guarded refresh
policies at matched trajectory error. Keep all original closure equations as
diagnostics; extend physical reaction/impulse and rank checks before promotion.


## Explicit clock scheduling and current profile

The missed-boundary hypothesis above is now reproduced and fixed for sampling
clocks. A 12-clock fixture at a 2 ms reporting endpoint previously ticked each
1 ms clock once instead of twice. `Behavior::scheduled_events` now declares
absolute deadlines; the compiler maps local guard indices, and the integrator
splits continuous advancement at known deadlines and processes endpoint ticks
before returning. Firmware, articulated IMUs, sampled controllers, the external
controller seam and sampled sensor chains use this hook. Ordinary root guards
and phase-dependent sensor fault guards retain their existing semantics.

Simultaneous clocks execute in deterministic deadline/guard order. Nonfinite or
nonadvancing schedules fail explicitly. A deadline within floating-point roundoff
of the requested endpoint does not create a tiny extra implicit step. A 150-step
motor-clock regression caught and now covers that singular-step edge case.

Controller/reporting terminology is corrected: this scene has a **2 ms session
input/report interval, 20 ms Rhai controller interval and 1 ms motor firmware
interval**. Earlier `sim-profile` artifacts mislabeled the session interval as
`controller_period_s`. The tool now uses the actual controller contract, reports
`action_period_s` separately and includes per-island event timelines. Clocks at
the end of a session advance observe inputs already supplied to that advance;
inputs supplied by the next call arrive after those endpoint ticks.

With all sampling clocks migrated, the robot reports **242 events** at 0.5,
0.125 and 0.0625 ms physics steps. The default and identity runs commit exactly
40 steps at 0.5 ms and spend zero time locating clock roots. This changes the
previously incorrect event timing, so an identical trajectory to the old clock
implementation is not an acceptance criterion.

Two sequential paired observations with 16 workers and 20 ms simulated time:

| Work | Default parallel FD + contact reuse | Opt-in structural identities |
|---|---:|---:|
| Session stepping | 3.211 / 3.295 s | 1.451 / 1.507 s |
| Jacobian assembly | 1.763 / 1.824 s (~55%) | 0.813 / 0.849 s (~56%) |
| Other residuals | 0.864 / 0.877 s (~27%) | 0.362 / 0.375 s (~25%) |
| Factorization | 0.532 / 0.539 s (~16%) | 0.239 / 0.245 s (~16%) |
| Component FD residual calls | 457,520 | 211,904 |
| Other full-residual calls | 4,285 | 1,809 |
| Fresh Jacobians | 380 | 176 |
| Newton iterations | 938 | 699 |
| Subdivisions | 6 | 2 |

These replace the earlier profile for current prioritization; historical timings
are retained above. The identity gain is about 2.2× in this pair, still experimental.
The structure remains 600 unknowns, 50,476 entries and 312 greedy colors.

At 0.125 ms, the identity path passes all **12,638** equal-timestep comparisons
against independent whole-residual numerical derivatives. Maximum normalized
error is 0.02062; both runs have zero subdivisions. This establishes derivative
agreement for this recording, not timestep-converged physics. The 0.125 versus
0.0625 ms numerical-reference refinement still fails with **4,457 mismatches**;
event counts now match at every reporting frame. The finer numerical reference
has six subdivisions versus zero for the candidate. Clock consistency therefore
removes a confounder without resolving the underlying timestep sensitivity.

The **default constraint formulation also fails** its own equal-timestep 0.125 ms
numerical-reference comparison: 1,844 mismatches. Its provided-derivative path
has no subdivisions, while the numerical reference subdivides five times in the
first 2 ms. At that first reporting frame the +X sliding foot crosshead's world-X
contact force differs by **0.02670 N** (-0.10870 versus -0.08200 N). This is an
actual contact-force difference, not a redundant-multiplier gauge. The evidence
supports investigating conditioning and differing accepted step histories; it
does not yet prove which equations or derivative errors cause the divergence.
The identity experiment's equal-step agreement is encouraging but does not
replace original-constraint closure, rank and timestep-convergence gates.

Tests pass for dynamics, compiler, robot, control, sensing, coupling and runtime,
including reporting boundaries across step sizes, off-grid/simultaneous clocks,
physical-root ordering, start-time ticks, invalid schedules, real sampled-sensor
event counts and controller endpoint timestamps. Native and release WASM builds
pass. Real Chromium pendulum runs pass in default and hybrid modes, replay exactly,
and differ from native by at most 3.7e-16; 0.4 simulated seconds takes about
177–181 ms. This is a portability fixture, not a full-robot browser performance gate.

A separate validation issue remains: the midpoint integrator stores some
algebraic sensor taps and reaction multipliers at midpoint while storing poses
at the endpoint. A damped tachometer fixture exposed that distinction. Force and
sensor comparisons using midpoint must account for the represented time. The
later solve-point trace below confirms that the full robot actually uses backward
Euler, so this midpoint-fixture observation does not explain its force disagreement.
Do not loosen tolerances or describe contact-force differences as multiplier ambiguity.

Evidence: `runs/full-robot/solver-performance/clock-final-*-profile.json`,
`clock-final-h4-compare.json`, `clock-final-h4-refinement.json`, and
`runs/interactive/clock{,-hybrid}-browser-report.json`. The profile artifacts in
this group precede the metadata-label correction described above; their 2 ms
`controller_period_s` field is the session interval. The physical run and counters
are unaffected by that reporting correction. `clock-event-trace-profile.json`
contains corrected metadata and exactly reproduces the default final frame and
solver counters: 20 ticks for each of 12 firmware guards, and two controller
callbacks at 0 and 20 ms. `clock-final-baseline-h4-compare.json` records the default
formulation's separate derivative-reference failure.


## Original-constraint and rank diagnostics

`Articulated::original_closure` independently recomputes every original loop and
transmission row, including angular identities skipped by the opt-in evaluator.
It exposes position, velocity, acceleration, stabilization/CFM and units.
`Articulated::audit_constraints` adds the bilateral velocity map G, physical
row/column scales, pivoted-QR row selection and singular values. It also reports
Gᵀλ in named generalized velocity coordinates, with conjugate force/torque units;
contact forces remain separate. None of these diagnostic selections changes the
simulation equations or enables either experimental option.

The map uses unit generalized-velocity basis evaluations of kinematics, which
are linear in velocities. It requires no contact queries, dynamics solves or
small finite-difference probes. This is the bilateral kinematic map, **not the
complete implicit Newton residual or a unilateral-contact rank test**.

Reproduce a history with:

```sh
RAYON_NUM_THREADS=16 ./target/release/sim-validate constraints   runs/full-robot/solver-performance/constraint-baseline.scene.json 10   runs/full-robot/solver-performance/constraint-rank.config.json
```

The local configuration uses a 0.1 m characteristic length, 1 rad characteristic
angle, 1e-10 relative and 1e-12 absolute rank thresholds. Both the configuration
and actual QR/SVD cutoffs are reported. This scaling is diagnostic and does not
change the model, solver tolerances or controller schedule.

The initial robot has **28 bilateral rows, 34 generalized velocity coordinates
and rank 16** by both methods. The resulting 18 instantaneous freedoms are
consistent with six floating-body motions plus twelve controlled motions; this
count alone does not establish controllability. During the 20 ms baseline and
identity trajectories at 0.125 ms:

| Original closure channel | Maximum absolute error | RMS over sampled rows/frames |
|---|---:|---:|
| Loop position | 2.923e-9 m | 5.785e-10 m |
| Loop velocity | 3.720e-7 m/s | 4.856e-8 m/s |
| Transmission position | 7.219e-12 rad | 1.373e-12 rad |
| Transmission velocity | 1.032e-9 rad/s | 2.082e-10 rad/s |

The identity path's original angular dot-product rows remain below 1.8e-18 in
position and 1.7e-16/s in velocity. Both trajectories have essentially the same
closure histories. These are short-run observations, not full-range or long-run
promotion gates.

At the strict diagnostic threshold, the estimated rank later rises to 17–18;
QR and SVD disagree at some frames near their respective cutoffs. The first 16
singular values stay at approximately one or above, while the largest additional
one reaches only 3.454e-8. These weak directions accompany nonzero closure at
points away from the exact constraint manifold. Do not infer new freedoms,
singularity, or safe fixed-row deletion from the integer rank alone. A threshold
sweep and configuration-region analysis remain necessary.

The CLI deliberately labels its acceleration/stabilized fields as **estimates**:
the runtime diagnostic view currently uses report-interval joint acceleration
estimates and does not reconstruct base/modal accelerations. Position/velocity
closure and the rank matrix do not depend on that approximation. Exact
acceleration-level acceptance needs the actual solver configuration/rate point;
the library API supports supplying it, but runtime capture is still open.

Validation includes moving four-bar poses, independent configuration differences
of closure, a known toggle where rank falls from two to one, empty/zero maps,
original stabilization/CFM equations, floating/modal velocity support, and
Gᵀλ checked against the change in independently evaluated inverse-dynamics
loads. A million-unit multiplier in a known null direction leaves generalized
reactions unchanged. This tests multiplier ambiguity without excusing contact
force disagreement. Focused release tests and release WASM compilation pass;
the constraint tests are included in CI (remote CI has not been run).

Evidence: `runs/full-robot/solver-performance/constraint-{baseline,identities}.json`
cover 20 ms, while `constraint-numerical.json` covers the first 2 ms around the
previously identified force divergence. The simulation path itself is unchanged
by this diagnostic addition. Next: capture the actual implicit solve point and
first retry reasons, then compare constraint reactions and contact loads at
consistent times before promoting a formulation or derivative change.

At the first 2 ms divergence, Gᵀλ differs by **0.02647 N** in the +X foot
translation coordinate and **0.002178 N·m** in thigh elevation. Thus the observed
difference survives combination into generalized physical loads; it is not
solely a nullspace redistribution of individual multipliers. The numerical
reference has five subdivisions before this frame versus zero in the candidate,
with equal event counts. Stored multipliers and endpoint geometry still have the
time-placement caveat above, so this is evidence for the next diagnosis rather
than an instantaneous force-accuracy verdict. Full data:
`runs/full-robot/solver-performance/constraint-first-reaction-differences.json`.


## Exact solve-point capture and remaining retry mechanisms

Added bounded, opt-in implicit-attempt capture in `sim-dynamics` and Newton
iteration diagnostics in `sim-solve`. Records contain stage time, method weight,
subdivision depth, branch status, stage state/rates, independently recomputed
terminal residuals, scaled worst rows, and Jacobian refresh decisions. Successful
nonlinear attempts are explicitly distinguished from finally committed steps:
records can include rejected outer steps or root-search trials. Capture is off
by default, and diagnostic residual re-evaluations are not ordinary solver-work
counters. The output reports when its configured capacity is reached.

`PhysicalRobot::generalized_at_solver_point` reconstructs physical states and
rates from that island's expanded snapshot, with no live-state or reporting-rate
fallback. `sim-validate steps scene.json [frames=1] [attempt_limit=512]` produces
named Newton rows, original closure/rank/reactions and contact loads at the same
stage point. Original stabilized rows match compiled stage residuals exactly for
the original formulation, and within 8.7e-16 for certified angular identities.

**Correction to the earlier time-placement lead:** the full robot runtime uses
backward Euler (`theta=1`) throughout these runs. It does not switch from midpoint
on retry, and its stage is the endpoint. The library supports that switch for
midpoint users, but it is not the explanation here. Earlier report-interval
acceleration estimates were insufficient for acceleration-level checks; the new
solve-point capture supplies actual rates and resolves that limitation. The
previous endpoint Gᵀλ/contact-force discrepancy is not a midpoint-offset artifact.

For the first 2 ms at nominal 0.125 ms steps:

| Derivatives / constraint evaluation | Attempted solves | Failed attempts | Jacobian builds |
|---|---:|---:|---:|
| Component derivatives, original rows | 16 | 0 | 17 |
| Component derivatives, certified identities | 16 | 0 | 15 |
| Independent whole-residual FD, original rows | 26 | 5 | 195 |
| Independent whole-residual FD, certified identities | 16 | 0 | 14 |

The original numerical reference first fails at 1.875 ms. Four failures end in
line-search exhaustion, one in the iteration limit. Worst scaled rows repeatedly
include knee angular alignment. At the saved first failure, geometric angular
closure/stabilization terms themselves are near floating-point roundoff; residuals
around 1e-11 are predominantly CFM times spurious angular multipliers. Their row
scales amplify them to roughly 1e-5. Exact identity evaluation removes those false
couplings and all five retries in this window. This supports the conditioning/
derivative-noise diagnosis; it is not yet a full-trajectory promotion result.

At the nominal 0.5 ms step over 20 ms, the two remaining identity-path failures
start at **14.5 and 14.75 ms**, on the −X foot servo's coupled torque/shaft/rotor
rows. They cycle through fresh-Jacobian partial steps and exhaust 40 iterations.
The second reaches the limit with a stale Jacobian; the first is already fresh,
so simply forcing refresh cannot explain away both failures.

Both saved states are just inside the motor coupling's backlash boundary:

| Attempt starts | Distance inside engagement boundary | Relative shaft speed | Damping torque jump |
|---|---:|---:|---:|
| 14.5 ms | 9.658e-9 rad | 0.103689 rad/s | 0.009850 N·m |
| 14.75 ms | 6.852e-9 rad | 0.085008 rad/s | 0.008076 N·m |

The CAD joint contributes 0.001330491831 rad total backlash. The motor coupling
uses 5% damping inside the gap and 100% outside it. A roughly 1e-8 rad forward
probe on gear angle crosses that jump and produces an artificial derivative near
1e6 N·m/rad; the local angular slope inside the gap is zero, and the engaged spring
stiffness is 50 N·m/rad. This is a concrete derivative failure mechanism. An
isolated reproducer must still establish whether exact/mode-local derivatives
resolve the solve, or whether the discontinuous implicit constitutive law lacks
a root for that step. Do not silently smooth the physical law or relax tolerances.

Validation: solver/dynamics/runtime release suites pass, including exact audited
versus unaudited trajectories, a mixed differential/algebraic stage with an
analytic answer, bounded failed-attempt capture, retry method/depth and physical
rate reconstruction. New tests are in CI. Release WASM builds pass. Paired full
robot runs before/after this instrumentation reproduce final frames, all solver
counters and residual-call counts exactly: 2.152/2.123 s and 2.131/2.149 s for
20 ms. No material disabled-capture overhead is measured; the lower absolute
wall times than previous observations are workstation variation, not a new gain.

Evidence: `runs/full-robot/solver-performance/implicit-{baseline,identities,numerical,numerical-identities}.json`,
`implicit-coarse-{baseline,identities}.json`, and `step-audit-{before,after}-{0,1}.json`.
Next: isolate the backlash derivative boundary, test a reusable component-level
solution without changing declared physics, then revisit full-trajectory error
and total-runtime gates. Both experimental options remain disabled by default.

## Backlash reproducer and experimental exact motor derivatives

The isolated `motor_jacobian` regression now reproduces the boundary-crossing
finite difference using `MotorUnit` itself. It also tests a single positive
inertia against the unchanged coupling law. For inertia 0.01 kg·m², step 0.5 ms,
half-gap 0.005 rad, speed 0.104 rad/s and a drive torque between the free-gap and
engaged damping loads, each affine backward-Euler branch has its root outside
that branch's valid gap interval. The residual jumps across zero at engagement.
Thus this constitutive law **can have no implicit-step root**, independent of
Jacobian quality. This does not prove nonexistence for the complete coupled robot.

Added shared, opt-in motor derivatives through runtime option
`analytic_motor_jacobian` and registry parameter `jacobian.analytic=1`. These cover
electrical, rotor, coupling, thermal and published-signal rows. The residual,
physical parameters and solver tolerances are unchanged. Derivatives use the
current branch; backlash engagement, the thermal clamp and absolute-value heat
loss at zero power do not have a claimed classical derivative at their kinks.
Tests check every local input/output partial over three perturbation sizes,
drive/back-drive and thermal modes, positive/negative/free backlash, zero backlash,
opt-in behavior, identical residuals and the discontinuous-step reproducer.

On the 20 ms robot run at 0.5 ms, exact motor derivatives with original constraints
use 335 Jacobian builds / four subdivisions versus the previous 380 / six.
With certified identities they use 180 builds / two subdivisions versus 176 / two.
The two failed attempts still start at 14.5 and 14.75 ms and hit the 40-iteration
limit. Correcting this component's derivatives does not cure the remaining retry
mechanism. `implicit-motor-analytic.json` records the failed solver stages.

Whole-residual independent numerical comparison, with certified identities in
both paths and identical schedules/tolerances:

| Nominal step | Compared values over 11 frames | Mismatches | Largest error / tolerance |
|---|---:|---:|---:|
| 0.5 ms | 12,642 | 1,074 | 26,140.65 |
| 0.125 ms | 12,638 | 0 | 0.02763 |

The finer-step pass is derivative-path agreement, **not timestep convergence**.
The preexisting refinement and longer-trajectory gates remain open. Neither
motor derivatives nor certified identities are promoted to defaults.

Two paired native runs at 0.125 ms, 16 Rayon workers, same scene/actions:

| Motor Jacobian | Wall seconds (pairs 1 / 2) | Rebuilds | Newton iterations | Component FD calls | Other residual calls |
|---|---:|---:|---:|---:|---:|
| Numerical | 1.174 / 1.165 | 191 | 2,076 | 229,964 | 2,896 |
| Exact, branch-local | 1.178 / 1.168 | 190 | 2,069 | 187,720 | 2,985 |

Both have 160 accepted steps, zero subdivisions and 242 events. The 18.4% drop in
component FD calls saves cheap motor evaluations, with no measured total speedup.
Jacobian assembly remains about 52–53% of runtime and factorization about 13–14%.
Focus further evaluation-count reduction on expensive articulated evaluations;
do not infer equal cost per residual call or compare wall times to older runs on
a differently loaded workstation. Build/startup time is reported separately.

Evidence is under `runs/full-robot/solver-performance/`:
`motor-analytic-{original,identities}-profile.json`,
`motor-analytic-{coarse,fine}-compare.json`, `implicit-motor-analytic.json`, and
`motor-fine-{control,exact}-{0,1}.json`. To reproduce the timing experiment, clone
the exported baseline scene, set `options.structural_loop_identities=true`,
`options.step=0.000125`, toggle `options.analytic_motor_jacobian`, and run
`RAYON_NUM_THREADS=16 ./target/release/sim-profile SCENE 10 1` in alternating pairs.
The final argument controls fixed-state microbenchmark repeats; these one-repeat
microbenchmarks are not used to draw kernel timing conclusions.

Validation: five motor regressions and eight runtime session tests pass. Release
WASM compiles; the real Chrome pendulum fixture with exact motor derivatives has
native/WASM maximum difference 2.39e-18, exact replay, and 0.4 simulated seconds
in 105.5 ms. This is a portability check, not a full-robot browser acceptance test.
Evidence: `runs/interactive/motor-analytic-browser-report.json`. CI includes the
local derivatives and browser mode; remote CI has not been run in this workspace.

Next: investigate mode-aware stepping at backlash engagement while preserving the
declared constitutive law. Treat smoothing or compliance changes as separate
physical-model experiments. In parallel with that research direction, the next
performance target is verified sparsity/reuse in the expensive articulated
residual, with existing whole-trajectory and timestep gates retained.

## Fixed-pose inter-body geometry reuse

Split SDF query geometry from contact force evaluation in the shared articulated
component. A prepared linearization now retains ordered intersecting sample/pair
identities, penetration depths and world normals. These results, including absent
contacts, are reused only when **every actual link position and rotation matches
bit-for-bit**. This check covers floating roots and flexible boundary kinematics,
rather than assuming a joint/state input cannot move geometry. The articulated
model, SDFs, samples and exclusions are immutable for the cache lifetime.

Velocity and bristle perturbations still recalculate contact damping, friction,
forces, moments and bristle rates. Pose changes perform fresh geometry queries.
Existing complete-force reuse remains limited to unchanged pose/velocity/bristle
dependencies. Floor contact is still evaluated fresh when full-force reuse is
invalid; this new cache targets inter-body geometry. Cache data is immutable,
scoped to one Jacobian assembly and safe to share across derivative workers;
there is no state carried across rejected steps or between worker perturbations.
Sample/pair accumulation order and physical equations are preserved.

Three alternating-order native pairs, 16 workers, default full robot, all
experimental derivative/constraint flags off, 20 ms simulated at nominal 0.5 ms:

| Pair | Previous wall s | Geometry-reuse wall s | Previous assembly s | Geometry-reuse assembly s |
|---|---:|---:|---:|---:|
| 1 | 2.134 | 1.768 | 1.251 | 0.880 |
| 2 | 2.139 | 1.747 | 1.254 | 0.877 |
| 3 | 2.148 | 1.759 | 1.258 | 0.882 |

This is approximately **1.22× total speed**, an 18% wall-time reduction and 30%
less Jacobian assembly time. Ordinary residual time stays ~0.51 s. Assembly is
now ~50% of wall time and factorization ~19%; the old 93%/2.6% profile still must
not be used. All counters stay identical: 380 rebuilds, 938 Newton iterations,
457,520 component FD residual calls, 4,285 other residual calls, six subdivisions
and 242 events. This optimization makes probes cheaper; it does not reduce their
number or resolve the remaining timestep/constraint difficulties.

Before/after prefix replays verify **all ten reporting frames at 2–20 ms exactly**,
including every pose, joint position, contact record and telemetry value, plus
solver counters, work counts and event traces. These reporting frames do not
expose every internal Newton state. Separately, the fine-step independent
whole-residual numerical comparison retains its 12,638/12,638 pass with unchanged
maximum error ratio 0.0276269477. Its experimental identity/motor options remain
unchanged and unpromoted; this does not resolve its timestep-refinement gate.

Validation: nine native Jacobian tests and eight serial-feature tests pass,
including a new pair of overlapping independent SDF bodies. Tests confirm forces
change at fixed geometry when velocity changes, and cached/uncached residuals
agree bit-for-bit for all local state/rate perturbations, bristle changes,
contact disappearance and contact appearance from an initially separated cache.
Eight articulated, three transmission and eight runtime tests also pass.
The Jacobian suite already runs in CI. Release WASM builds pass.

Evidence under `runs/full-robot/solver-performance/`:
`contact-geometry-final-{before,after}-{0,1,2}.json`,
`contact-geometry-prefix-{before,after}-{1..9}.json`,
`contact-geometry-prefix-comparison.json`, and `contact-geometry-fine-compare.json`.
The native previous binary was saved before editing as
`/tmp/sim-profile-before-contact-geometry`. Timing command:
`RAYON_NUM_THREADS=16 BINARY runs/full-robot/reproduced.scene.json 10 1`.
Builds and browser work were finished before the final three pairs.

### Contact-enabled browser diagnostic (resolved below)

The original no-contact pendulum browser gate still passes: native/WASM maximum
difference 2.78e-17, exact replay, 0.4 simulated seconds in 137.9 ms in the final
check. A contact-enabled fixture exposes a different issue. Clone
`examples/interactive/pendulum.scene.json`, set `options.contact=true`, raise
`robot.world.floor_z` by 1e-6 m and use `options.step=0.000125`. Two contact records
occur, and replay remains exact within the browser, but the final velocity channel
differs between native and WASM by 1.6047e-4, above the existing 1e-7 parity gate.
At 0.5 ms it also fails (4.66e-6 difference); smaller steps alone do not cure it.

The saved previous native executable and the new one produce identical frames
and solver statistics on this fixture. Reconstructing the pre-change contact
implementation in an isolated workspace and building it for WASM produces
**identical browser initial/final/replay/reset results** to the new geometry
implementation. Thus the observed cross-platform disagreement is not introduced
by this cache. It remains an unresolved accuracy/portability issue; it is not
treated as an acceptable tolerance adjustment or a passing contact-browser gate.

`web/tests/runtime.mjs` now accepts `--require-contacts` after the report path and
preserves native/browser frame evidence in `REPORT.frames.json` before asserting
parity. The evidence includes all browser reporting frames for first-divergence
diagnosis. The obstructed contact fixture requires at least 0.01 rad of motion;
the original free-motion fixture retains its 0.1 rad requirement. The parity
tolerance remains 1e-7 in both. The contact diagnostic is not yet added as a
required CI gate because it currently fails; existing gates are unchanged.

Evidence: `runs/interactive/contact-geometry{,-before}-browser-report.json.frames.json`,
`contact-geometry-{before,after}.json`, and
`contact-geometry-default-browser-report.json`. This issue, the backlash transition
failure and the full-robot timestep/constraint validation remain open next steps.

Prefix replay narrows the first failing contact-enabled reporting frame to
**40 ms**; the 20 ms frame passes the same comparison. At 40 ms, angular velocity
differs by 0.00616 rad/s and a contact-force Z component by 0.01946 N. Therefore
this is already a pose/load trajectory divergence, not just a final reporting
roundoff issue. All 20 native and browser reporting frames are retained in
`contact-geometry-native-frames.json`, the browser evidence file, and
`contact-geometry-cross-platform-comparison.json` under `runs/interactive/`.

## Joint-exclusion boundary flicker: cause, fix and validation

The new browser `set_attempt_audit_limit` / `implicit_attempt_report` worker
messages expose the same shared Rust solve-point diagnostics as native
`sim-validate steps`. The shared session validates a 0..10,000 per-island limit;
zero clears/disables capture, changing it clears old records without resetting
physics, and invalid limits leave records intact. Reports include per-island
limits, capacity flags and solver counters. Capture is off by default. The
browser's audited and unaudited final frames match exactly.

Comparing native and WASM traces narrowed the divergence further: at about
35.6 ms, WASM alternates between zero and two SDF contact records for nearly
identical poses. The two samples are at a radius of **exactly 10 mm** from the
joint origin, with local offsets ±6 mm and 8 mm. Their distance from that origin
is invariant under the revolute motion. The old test `distance < band_radius`
flipped with transform/subtraction rounding. A roughly 60 N contact-signal jump
over a ~1e-8 rad derivative probe created apparent slopes around 6e9 N/rad.
Newton's row scaling then hid substantial raw force residuals behind tiny
scaled values. The first different solve outcome starts at 36.25 ms: native
accepts the step, WASM rejects it and subdivides.

The correction is a shared `inside_exclusion_band` predicate. It excludes a
sample only when its distance is inside the radius by more than
`32 * f64::EPSILON * (max_abs(point) + max_abs(center) + radius)`.
This roundoff guard is about 3e-15 m for the fixture. Ambiguous boundary samples
remain eligible for a fresh SDF collision query, consistent with the strict
interior exclusion rule. This does not smooth contact forces, change declared
clearance or relax nonlinear/parity tolerances. It removes a numerical contact
classification artifact. The preexisting use of an export-pose neighbor band
remains an approximation; this change does not redesign that geometry policy.

The regression rotates these invariant-radius samples through 4,096 angles at
three spatial scales. Boundary samples remain eligible; points displaced inward
and outward by 1e-9 of the radius retain their respective classifications.

The **same failing** contact fixture, with all experimental derivative options
off and nominal step 0.125 ms, now has:

| Quantity over 0.4 simulated seconds | Previous | Corrected |
|---|---:|---:|
| Native subdivisions | 12 | 0 |
| WASM subdivisions | 25 | 0 |
| Native Jacobian rebuilds | 3,042 | 117 |
| Native Newton iterations | 14,559 | 11,617 |
| Native ordinary residual evaluations | 43,126 | 14,901 |

Three paired native wall times are 1.037→0.189 s, 1.015→0.193 s and 1.023→0.191 s:
approximately **5.4×** faster on this fixture. Do not apply that multiplier to
the full robot. Its default 20 ms final frame and all solver counters remain
identical to the geometry-reuse version; the separate backlash retries persist.

All **21 native/browser reporting frames** now agree at the unchanged 1e-7 gate,
with maximum numeric difference 2.71e-11 and exact browser replay. The audited
browser run takes 215.6 ms for 0.4 simulated seconds. Independent whole-residual
numerical derivatives pass **2,113/2,113** trajectory checks, maximum error ratio
1.074e-6. These checks cover the obstructed contact fixture and do not establish
full-robot timestep convergence or material calibration.

`sim-session scene.json [frames] [recording.json] [frame-trace.json]` now optionally
writes the initial and every reporting frame. The browser gate accepts either
the previous single final-frame JSON or this complete trace. With `--audit` it
also checks bounded solve-point capture; `--require-contacts` verifies the fixture
actually touched. Diagnostic evidence is saved before parity assertions. The
new contact-enabled CI gate checks the complete 21-frame trace, replay, response
and time budgets, with the original numeric tolerance. A Rust runtime regression
checks numerical-reference agreement and zero subdivisions. Existing mechanism,
Jacobian and session suites pass; remote CI has not been run in this workspace.

Evidence under `runs/interactive/`: `platform-native-attempts.json`,
`platform-browser-audit.json.frames.json`, `band-fixed-browser-report.json`,
`band-fixed-browser-report.json.frames.json`, `band-fixed-native-frames.json`,
`band-fixed-numerical-comparison.json`, and `band-{before,after}-{0,1,2}.json`.
The previous native executable was saved as `/tmp/sim-profile-before-band-predicate`.
The full-robot control is `runs/full-robot/solver-performance/band-fixed-original-profile.json`.
The existing full-robot fine-step numerical comparison also retains its unchanged
12,638/12,638 pass and 0.0276269477 maximum error ratio, recorded in
`runs/full-robot/solver-performance/band-fixed-fine-compare.json`. Serial-feature
library/Jacobian tests pass as well. Remaining work is the full robot's backlash
transition handling and its full-range constraint/timestep-accuracy gates.

## Experimental backlash events: reliable local transition, expensive location

Added `backlash_events` to runtime build options and `backlash.events` to the
motor registry, both **off by default**. A motor with nonzero backlash then has
one held, dimensionless mode state: free gap, positive engagement or negative
engagement. A one-time initialization event classifies the actual relative
gear/shaft angle. Guards locate engagement and release; jumps change only the
mode, not angle or velocity. The current branch is smoothly continued during
event-search trials, and advancement is split at the crossing. Each valid branch
retains the existing stiffness and 5%/100% damping law. Event samples use the
newly selected one-sided branch; this is an explicit event discretization,
not a claim of identical coarse-step trajectories to the unsplit formulation.

This work exposed and fixed a generic event-order defect. When several physical
guards crossed in one trial, the integrator previously chose the first declared
guard rather than the earliest crossing time. A test with an early event that
disables a later event reproduced the erroneous order `[later, early]` in the
old implementation. The integrator now locates the candidate crossings, selects
the earliest, applies its jump and reevaluates subsequent behavior. The regression
passes for RK4 and backward Euler, along with scheduled-clock and solve-audit tests.
Known clock deadlines still use the separate sorted scheduling path.

Independent component validation uses the actual `MotorUnit` against prescribed
electrical/shaft/thermal boundaries: inertia 0.01 kg·m², 0.00546 N·m drive,
0.104 rad/s initial speed and 0.01 rad total backlash. The isolated case previously
proved to have no valid unsplit backward-Euler root now crosses engagement without
subdivision. Seven timesteps from 0.5 ms through 7.8125 µs exhibit first-order
convergence to an independently derived exponential/free and damped-oscillator/
engaged solution. Both rotation signs and both numerical/provided derivatives
pass. A 0.5 s unforced run verifies repeated engagement/release, admissible modes,
nonincreasing mechanical energy and zero subdivisions. Initialization tests also
move the prescribed shaft, so the mode cannot simply be guessed from gear angle.

Full robot, 20 ms at nominal 0.5 ms, **certified knee identities enabled** and
experimental motor/articulated analytic derivatives disabled:

| Metric | Unsplit backlash | Explicit backlash events |
|---|---:|---:|
| Paired wall seconds | 0.787 / 0.782 | 1.464 / 1.460 |
| Subdivisions | 2 | 0 |
| Jacobian rebuilds | 176 | 333 |
| Newton iterations | 699 | 1,926 |
| Ordinary residual evaluations | 1,809 | 3,294 |
| Event-location calls | 0 | 9 |
| Committed substeps | 40 | 48 |
| Events | 242 | 262 |

The additional events are 12 mode initializations plus eight physical engagement
transitions; sampling-clock schedules remain unchanged. There are 612 Newton
unknowns instead of 600. Event-location work is about 0.81 s in the initial profile,
roughly half its 1.53 s total. Removing retries has **not** made this path faster:
it currently spends more on repeated trial solves and Jacobian rebuilds. These
measurements identify cross-trial linearization reuse as the next performance
experiment; physical residuals and convergence checks must stay current.

At the same 0.5 ms step, independent whole-residual numerical derivatives now
pass **12,774/12,774** trajectory comparisons (maximum error ratio 0.06583), where
the unsplit identity path previously failed. This is derivative-path agreement,
not a timestep-accuracy promotion. Comparing against four-times-finer numerical
stepping still fails **4,504** comparisons. With the original angular-constraint
formulation, the event path still has five subdivisions and 698 rebuilds; mode
events do not cure the independent constraint-conditioning problem.

The contact-enabled pendulum also passes: 2,134 independent numerical comparisons,
all 21 native/WASM reporting frames (maximum difference 2.61e-11), exact browser
replay, zero subdivisions and bounded diagnostic capture. The audited browser
run takes 223.6 ms for 0.4 simulated seconds. Tests run on native and serial builds;
the new cases are wired into CI, but remote CI has not been run here.
The default full-robot control retains its previous final frame and all solver
counters exactly. No CAD physical inputs or default experimental flags changed.

Evidence under `runs/full-robot/solver-performance/`:
`backlash-{control,events}-{0,1}.json`, `backlash-events-profile.json`,
`backlash-events-compare.json`, `backlash-events-refinement.json`,
`backlash-events-original-profile.json` and `backlash-default-control.json`.
The reference-refinement configuration sets `reference_substeps=4`; the event
recording retains the same scene, seed and controller actions. Browser/component
evidence is under `runs/interactive/backlash-events-*`. New tests are
`sim-dynamics/tests/root_order.rs` and `sim-domain-robot/tests/backlash_events.rs`.

### Guarded event-search matrix reuse (experimental, default off)

`BuildOptions.event_jacobian_reuse` allows reuse of an existing factorization
while locating a physical event, with the same integration rule and trial step
within 10% of the timestep at which that matrix was actually built. The stored
build timestep is not advanced by reuse, preventing accumulated drift. Newton
still treats the matrix as stale, evaluates current residuals, and retains its
contraction, line-search and refresh checks. Mode changes clear the cache.

Three alternating paired runs on the same current native binary, using the
20 ms full-robot event scene with certified knee identities:

| Metric | Event search without reuse | Guarded reuse |
|---|---:|---:|
| Wall seconds, three runs | 1.512 / 1.507 / 1.506 | 1.266 / 1.273 / 1.257 |
| Jacobian builds | 333 | 270 |
| Newton iterations | 1,926 | 1,717 |
| Ordinary residual evaluations | 3,294 | 2,907 |
| Subdivisions | 0 | 0 |
| Committed substeps / events | 48 / 262 | 48 / 262 |

This saves about 16% of wall time (1.19×), but remains roughly 63× slower than
real time for this short full-robot segment. It does not establish sustained
throughput or interactive input latency. The unsplit backlash experiment is
faster but has unresolved trajectory/accuracy gates of its own.

Independent numerical-reference comparison passes all 12,774 values, maximum
error ratio 0.03774. The reference explicitly disables event matrix reuse;
comparison output records both complete option sets. This is not a replacement
for the finer-timestep gate that failed on the preceding event experiment.
Default full-robot final frame and solver counters match the earlier default
control exactly.

The event regression checks an independent backward-Euler event/final-state
solution, fewer builds, actual matrix-build timestep bounds and fresh information
after a 100× change in decay rate at the event. Runtime tests pass. The loaded
contact browser case passes all 21 native/WASM frames (maximum difference
2.55e-11), exact replay and bounded audits; 0.4 simulated seconds takes 229.5 ms
in that much smaller fixture. Its independent numerical comparison also passes.
The experimental browser/reference gate is wired into CI, not yet run remotely.

Evidence: `solver-performance/{backlash-events,event-reuse}-paired-current-*.json`,
`event-reuse-compare.json`, `event-reuse-default-control.json` and
`runs/interactive/event-reuse-*`. Reuse remains opt-in; no CAD physics changed.

### Timestep refinement: sampled encoder decisions amplify small pose errors

The comparator now retains the earliest failing reporting-frame sample and
maximum/RMS errors grouped by declared unit, globally and per reporting frame.
These summaries include passing samples; RMS is over channel/frame samples,
not a continuous-time integral or a contact impulse. The bounded worst-error
list remains available. Tests cover a known RMS including passing samples,
large finite values and failure-onset/count consistency on a refined trajectory.
No solver equations, acceptance tolerances or CAD inputs changed in this audit.

Full-robot 20 ms, the same hold policy/seed/actions, 1 ms firmware and 20 ms Rhai
schedule, certified knee identities and backlash events enabled, event matrix
reuse enabled only in the candidate. Each reference uses independent whole-
residual numerical derivatives, disables event matrix reuse, and halves h:

| Candidate / reference h (µs) | Maximum position-channel error (µm) | Maximum force-channel error (N) | RMS force-channel error (N) |
|---|---:|---:|---:|
| 500 / 250 | 52.29 | 6.408 | 0.682 |
| 250 / 125 | 42.86 | 4.819 | 0.512 |
| 125 / 62.5 | 164.54 | 34.837 | 3.493 |
| 62.5 / 31.25 | 18.86 | 6.689 | 0.567 |
| 31.25 / 15.625 | 7.34 | 2.988 | 0.247 |

Every pair fails the unchanged tolerance gate. The listed maximum length errors
are link position components, not Euclidean pose errors. Force summaries include
contact loads, signals and constraint multipliers; the maximum in each pair is
an actual contact load or foot-load signal, not a redundant multiplier. At 14 ms
the 500/250 µs foot-load readings are 22.71 versus 29.12 N. These discrepancies
must not be dismissed because the model looks similar in the viewport.

Independent same-h comparisons pass all 12,774 values at 250, 125 and 62.5 µs
(maximum error ratios 0.0243, 0.0200 and 0.0183), in addition to the preceding
500 µs gate. Both 250 µs paths have one subdivision; both 125 and 62.5 µs paths
have none. Thus the large 125/62.5 µs difference is not explained by switching
derivative implementations or the event matrix reuse experiment.

The additional **31.25 µs same-h check fails**: 1,758 comparisons, maximum force
difference 0.5853 N and position difference 2.347 µm. The first failing reporting
frame is 10 ms, before the reference-only subdivision recorded by 12 ms. Thus
the finest candidate is not yet independently validated, and the finest h/2
comparison must not be promoted as a trustworthy accuracy reference. Repeating
the candidate with event matrix reuse disabled reproduces the same 1,758
failures and 0.5853 N force discrepancy; this failure persists independently
of that experiment (`event-reuse-refine-4-no-reuse-compare.json`).

Captured implicit-attempt initial states at firmware deadlines identify a
specific discrete decision. The first differing quantized previous-error state
across the twelve motors in these captures occurs at 5 ms, on the +X worm drive:

| At the 5 ms tick | h = 125 µs | h = 62.5 µs |
|---|---:|---:|
| Raw encoder angle (rad) | 0.000774609689 | 0.000716746021 |
| Rounded encoder count | 1 | 0 |
| Queued normalized command | −0.122089691 | −0.102102569 |

The declared encoder quantum is 0.001533980788 rad, putting its first rounding
boundary at 0.000766990394 rad. The coarse result lies just 7.62 µrad above that
boundary; the finer result lies below it. The commands, which also include a
measured-speed derivative term, enter the one-sample queue and are applied at
6 ms. This is a demonstrated controller branch difference preceding the large
force discrepancy, not a proof that it alone causes every later difference.
The quantizer is retained; removing it would change the physical/controller
model rather than validate an optimization.

At 2 ms, before this differing count, successive force discrepancies decrease
1.091→0.672→0.356→0.181→0.091 N. Later contact and sampled-controller transitions
prevent assigning that convergence rate to the entire trajectory. Further work
needs event-aware force/impulse comparisons and tighter nonlinear references;
neither final-state agreement nor a smooth animation establishes accuracy.

Evidence under `runs/full-robot/solver-performance/`:
`event-reuse-refine-{0..4}.recording.json` and matching comparison JSON,
`event-reuse-half-step.config.json`, `event-reuse-refine-{1,2,3,4}-same-h.json`,
`event-reuse-refine-{2,3}-steps.json` (uncapped, 90/164 attempts), and the compact
`event-reuse-quantizer-onset.json`. These remain short commissioning trajectories,
not stance/swing/impact/thermal or full operating-range acceptance.

### Reproduced accuracy defect: false convergence at floor liftoff (before guard)

Internal solve capture with event matrix reuse **off in both paths** localizes
the 31.25 µs same-h discrepancy to the step ending at 8.375 ms. Both trajectories
agree through 8.34375 ms; that step has one remaining floor contact. At 8.375 ms,
both recorded endpoints have zero contacts and both solvers report success via
`noise_floor_accept`. However, recomputing the actual terminal residual gives:

| Derivative path | Base x force-balance residual (N) | Corresponding Jacobian row scale |
|---|---:|---:|
| Compiled component derivatives | 4.6941254 | 1.6877e-11 |
| Independent whole-residual FD | 1.3539129 | 5.1785e-14 |

The base angular residuals are also material (−2.1003 and −0.6058 N·m about y).
These are equation residuals, not an ambiguous choice of redundant multipliers.
The base rows are assembled directly from `Evaluation.base_wrench`; their state
labels contain velocity names but their residual equations balance force/torque.
Very large derivative magnitudes can make those residuals appear tiny after
row scaling. Convergence acceptance currently allows a small correction or
scaled noise floor without a separate physical residual acceptance check.

The generic reproducer `crates/sim-solve/tests/convergence.rs` uses
`F(x)=x-1` for x<0 and `F(x)=x+1` otherwise. Neither branch has a valid root,
and |F(x)|≥1 everywhere. Starting at −1e-10 with a forward FD probe of about
1e-8, the solver accepts after one correction, returning residual 1.0000000049.
The initially ignored regression now runs normally with
`cargo test --locked --release -p sim-solve --test convergence` and must reject
this case after the guard below. The scalar case proves the acceptance weakness;
it does not establish that the complete robot's liftoff step lacks a root.

Evidence: `fine-divergence-{compiled,numerical}.scene.json`, matching
`*-steps.json` (324 attempts each, no capture truncation), and compact
`fine-divergence-liftoff-onset.json` under `runs/full-robot/solver-performance/`.
The guard below separates residual acceptance from Jacobian conditioning.
Its additional retries are reported explicitly; event handling and trajectory
accuracy remain open before more performance promotion.

### Residual acceptance guard and bounded refresh recovery

Newton now verifies the actual final residual against a fixed bound for every
row: `absolute_tolerance + relative_tolerance * abs(initial_row_residual)`.
The initial residual is captured once per nonlinear solve. Jacobian row scaling
still conditions the linear system and guides line search, but a later inflated
derivative cannot loosen these acceptance bounds. An exactly zero residual also
uses the raw values, avoiding a scaled underflow shortcut. This is a residual-
progress safeguard in the equations' supplied units, not a calibrated per-row
physical error budget or a timestep accuracy estimator.

A failed acceptance check refreshes the Jacobian before considering timestep
recovery. If three further checks fail to halve the error measured against the
fixed bounds, the solve returns `NotConverged` instead of spending the remaining
iterations at an apparent noise floor. Audits store `residual_limits` and the
`residual_acceptance_refresh` / `residual_acceptance_failure` decisions. Legacy
audit records deserialize with an empty limits vector, indicating no such check.

The active scalar regression rejects the proven no-root jump. Two additional
tests verify a valid coupled system across row scales 1e-9 through 1e12 and
successful fresh recovery after an inflated first Jacobian. All **86 tests**
across the selected solver, integrator, robot, electrical, thermal and runtime
suites pass. The regression is included in CI; remote CI has not been run.

The default full-robot control retains its final frame and solver counters
exactly. The 500 µs event/reuse experiment still completes with zero subdivisions;
its final contact force differs from the earlier result by at most 2.73e-8 N.
The 31.25 µs path now needs three subdivisions at liftoff. In the 10 ms captured
window, all 327 accepted trials satisfy their recorded bounds; the maximum base
force-balance residual is 2.121e-9 N and torque-balance residual is 9.459e-10 N·m.
The formerly accepted 4.69 N imbalance is rejected. Some accepted trials are
event-location probes rather than committed steps; the capture is not truncated.

Two alternating paired runs measure the bounded-refresh addition relative to
the new residual guard without that addition:

| Metric, 20 ms at h = 31.25 µs | Guard alone | Guard plus bounded refresh |
|---|---:|---:|
| Wall seconds, two runs | 3.011 / 2.993 | 2.959 / 2.942 |
| Jacobian builds | 552 | 539 |
| Newton iterations | 6,782 | 6,768 |
| Ordinary residual evaluations | 8,834 | 8,821 |
| Subdivisions | 3 | 3 |

The approximately 1.7% time saving is small but removes unproductive work.
Every accepted state and rate in the 10 ms capture is bit-for-bit identical,
and the full 20 ms final frame and solver statistics are identical. This does
not imply that the guarded path is faster than the old false-accepting solver.

The final independent same-h trajectory comparison **still fails**: 1,408 values
and a maximum force difference of 0.5066 N, with three candidate subdivisions
versus one reference subdivision. The next issue is event/timestep handling and
a trustworthy refined trajectory, not promotion of an analytic path. No physical
parameters or CAD inputs changed.

Both contact-enabled browser cases pass all 21 native/WASM frames with exact
replay: maximum differences 2.71e-11 (ordinary contact) and 2.55e-11 (experimental
backlash/event reuse). Their 0.4 simulated seconds take 215.6 and 222.8 ms in
Chrome 152. These are the small pendulum fixtures, not full-robot real-time tests.

Evidence: `residual-acceptance-{default,events,fine}-profile.json`,
`residual-{guard,guard-stall}-fine-paired-{0,1}.json`,
`residual-final-fine-{compare,steps}.json` and the earlier guarded capture
`residual-acceptance-fine-steps.json` under `runs/full-robot/solver-performance/`;
browser evidence is `runs/interactive/residual-final-{contact,event-reuse}.*`.

### Shared step boundaries isolate the derivative comparison

`Simulation::set_step_breakpoints(Vec<f64>)` requests additional absolute step
boundaries without adding events, changing physical inputs, or resetting the
controller. Times must be finite, nonnegative and strictly increasing; invalid
replacement is atomic. The full immutable list is retained, so restoring state
or rejecting an adaptive trial does not consume future boundaries. Boundaries
do not prevent additional convergence subdivisions. The default list is empty.

`CompareConfig.shared_step_breakpoints_s` applies and records the same list in
candidate and reference. `sim-profile` accepts an optional fourth argument: a
JSON array of these times, recorded in its output as `step_breakpoints_s`.
The unit tests compare against independently requested backward-Euler steps,
check clock event times/counts, invalid inputs and snapshot replay. A runtime
comparison also checks that boundaries preserve controller-event counts. The
tests pass, the new cases are wired into CI, and the WASM build passes.

For the 31.25 µs full-robot recording, the diagnostic schedule is:

```json
{"shared_step_breakpoints_s": [0.008359375, 0.0083671875, 0.00837109375]}
```

These are the smaller boundaries observed around the difficult liftoff step.
With them, **both derivative paths have zero subdivisions** and all **12,766**
comparisons pass. Maximum force difference is 3.221e-8 N, maximum position
component difference is 6.717e-15 m, and maximum tolerance ratio is 0.03220.
Both runs have 650 committed integration segments and 261 events. This isolates
the earlier 0.5066 N discrepancy to differing integration grids; it does not
prove that the chosen grid has adequate physical accuracy. A hand-selected grid
for one recording is not a production adaptive-step improvement.

Analytic options were reassessed separately on that grid. Exact motor derivatives
still fail 1,011 comparisons, with two candidate subdivisions and a maximum force
difference of 0.02467 N. Articulated hybrid derivatives pass all comparisons with
zero subdivisions and maximum tolerance ratio 0.02631. The latter's performance
was then measured in three alternating pairs on the same native binary and
16-worker configuration:

| Metric | Numerical component derivatives | Articulated hybrid derivatives |
|---|---:|---:|
| Wall seconds, three runs | 2.812 / 2.804 / 2.776 | 3.266 / 3.314 / 3.260 |
| Jacobian builds | 517 | 509 |
| Newton iterations | 6,649 | 6,643 |
| Ordinary residual calls | 8,110 | 8,524 |
| Assembly seconds, first pair | 1.241 | 1.676 |
| Factorization seconds, first pair | 0.420 | 0.408 |

Hybrid is about 17% slower despite slightly fewer builds. The profiler now warns
that its component FD counter excludes internal hybrid probes, so a lower count
must not be mistaken for a proportional reduction in all residual work. Both
analytic options remain experimental and off by default.

The next experiment identified at this point was: the hybrid remainder
uses `sample_jacobian`, which calls `evaluate` without the prepared contact and
geometry reuse available to the compiler's numerical path. Reusing the same
dependency-checked preparation there was measured in the following section.

Evidence under `runs/full-robot/solver-performance/`:
`shared-liftoff-grid.config.json`, `shared-liftoff-grid.points.json`,
`shared-liftoff-grid-compare.json`, `shared-grid-{motor,articulated}-compare.json`
and `shared-grid-{compiled,hybrid}-paired-{0,1,2}.json`.
For a profile, pass the scene and boundary list explicitly:

```sh
RAYON_NUM_THREADS=16 target/release/sim-profile \
  runs/full-robot/solver-performance/fine-divergence-compiled.scene.json 10 1 \
  runs/full-robot/solver-performance/shared-liftoff-grid.points.json
```

### Hybrid remainder reuses prepared contact and geometry

The hybrid Jacobian now shares one immutable `ContactLinearization` across its
base sample and numerical remainder workers. This uses the existing bitwise
checks: changed poses require fresh SDF geometry; changed velocities or bristle
states require fresh forces; acceleration and constraint-reaction perturbations
can reuse the complete contact result. Preparation is created only with contact
enabled. The contact-free inertial basis and physical equations are unchanged.

Three alternating before/after runs used the same 31.25 µs full-robot scene,
shared liftoff boundaries, 20 ms trajectory and 16 native workers:

| Metric | Hybrid before reuse | Hybrid after reuse |
|---|---:|---:|
| Wall seconds | 3.290 / 3.246 / 3.250 | 2.856 / 2.785 / 2.807 |
| Mean wall seconds | 3.262 | 2.816 |
| Mean Jacobian assembly seconds | 1.626 | 1.162 |
| Builds / Newton iterations / ordinary residual calls | 509 / 6,643 / 8,524 | 509 / 6,643 / 8,524 |
| Committed segments / events / subdivisions | 650 / 261 / 0 | 650 / 261 / 0 |

That is **13.7% less total time** (1.16× speedup) and 28.5% less assembly time.
Final frames, event traces and all profile work counts match exactly in every
pair. The independent whole-residual numerical comparison passes all **12,766**
checks at 11 report frames; maximum tolerance ratio is 0.02631, maximum error
among N-labelled channels is 2.633e-8 N. Every per-frame comparison summary and
unit-error summary matches the pre-reuse hybrid report exactly. This is still
a short shared-grid diagnostic, not a timestep-refined physical reference.

A second set of alternating runs on the current binary gives numerical component
derivatives 2.821 / 2.749 / 2.718 s (mean 2.763 s), versus hybrid 2.887 / 2.790 /
2.784 s (mean 2.820 s). Hybrid remains about **2.1% slower**. Its assembly time is
now similar, but it still performs 8,524 ordinary residual calls versus 8,110.
This optimization does not establish a reason to promote hybrid to default.

Validation also exposed an existing gap: the stiff, overlapping two-box SDF
fixture fails the full independent hybrid derivative audit (28 mismatches and
900 unresolved comparisons at the initial point). Disabling preparation gives
an identical report, including every reported derivative/error value. Relative
sliding also fails (54 mismatches, 782 unresolved comparisons). These observations
do not establish the individual causes; finite-difference cancellation, stencil
scale and contact branch/geometry changes need separate diagnosis. No tolerances
were loosened. `hybrid_stiff_sdf_pair_derivative_promotion_gate` retains the exact
fixture as an explicitly ignored, failing diagnostic while hybrid stays opt-in:

```sh
cargo test --locked --release -p sim-domain-robot --test jacobian \
  hybrid_stiff_sdf_pair_derivative_promotion_gate -- --ignored
```

The normal native articulated/Jacobian/transmission suite passes 20 tests;
serial Jacobian passes eight. Both report this one ignored promotion gate.
Existing prepared-residual tests verify bit-exact cached/uncached values across
every input/rate perturbation, pose changes, absent and loaded SDF contacts,
velocity-dependent forces, floor contact and flex. Worker-count checks pass.

WASM builds, and a real Chrome worker test with contact, backlash events, event
matrix reuse and hybrid enabled passes all 21 native/browser frames, two-contact
coverage and exact recording replay. Maximum native/WASM difference is 6.284e-14;
0.4 simulated seconds takes 223 ms in that small fixture. Its independent
trajectory comparison passes 2,134 checks. This is not full-robot browser timing.
The same contact-enabled hybrid gate is added to browser CI; remote CI was not run.

Evidence: `hybrid-reuse-{before,after}-paired-{0,1,2}.json`,
`hybrid-reuse-current-{numerical,hybrid}-{0,1,2}.json`, `hybrid-reuse-compare.json`,
`hybrid-{reuse,no-reuse}-pair-check.json`, `hybrid-reuse-moving-pair-check.json`
in `runs/full-robot/solver-performance/`; and `hybrid-reuse-contact.*` in
`runs/interactive/`. Robot CAD and all default analytic flags remain unchanged.

### Diagnose SDF stencil sensitivity; reject centered remainder experiment

The stiff two-box diagnostic now prints all three points (loaded, sliding and
apart) at six reference radii, 1e-3 through 1e-8, plus SDF sample geometry and
active contact sample identities under y perturbations. The underlying fixture,
physics and tolerance remain unchanged. It is still an explicitly ignored,
failing promotion gate; the command in the previous section now prints the full
sweep when `--nocapture` is appended.

At the loaded point, two active samples have penetration 7.4575e-7 m, while two
nearby inactive samples have signed distance +4.7266e-6 m. A -5 µm y perturbation
changes the active sample identities from `(0,2,1)` / `(1,5,0)` to `(0,7,1)` /
`(1,1,0)` (source link, sample, target link); -1 µm retains the original samples.
The contact count stays four throughout. The default 10 µm central reference
therefore crosses contact branches even though a contact-count check would miss
that change. This is observed query behavior, not an inferred multiplier issue.

`Difference` now reports the actual stencil parameter, coarse/fine numerical
slopes, the reference truncation estimate and the reference roundoff estimate.
`CheckReport.inconclusive_by_reason` counts every unresolved comparison, including
those omitted from the bounded differences list. Roundoff and stencil-resolution
failures now have separate reason strings. Classification and tolerance logic
are otherwise unchanged: all 18 point/stencil sweep pass flags, mismatch counts,
inconclusive counts and maximum error ratios match the pre-diagnostic reports.

At the default reference radius:

| Point | Resolved mismatches | Roundoff-limited | Stencil-limited | Branch warning | Direction/column disagreement |
|---|---:|---:|---:|---:|---:|
| Loaded | 28 | 830 | 42 | 2 | 26 |
| Sliding | 54 | 706 | 34 | 15 | 27 |
| Apart | 0 | 0 | 0 | 0 | 0 |

Counts are primary reasons; the detailed estimates can expose multiple issues
in a single comparison. Numerical cancellation dominates the unresolved count,
but does not explain away the resolved mismatches. A new independent affine
case, `1e12 + x`, verifies that a correct derivative with unresolvable numerical
probes is reported as roundoff-limited rather than silently accepted or blamed
on the supplied derivative.

A temporary centered remainder used `(R(x+h)-R(x-h))/(2h)`, with requested
`h=1e-6*(1+abs(x))` and the actual representable endpoint separation. It eliminated
the resolved mismatches at the default reference stencil in both contact points,
but left unresolved comparisons and introduced mismatches at some smaller radii.
This was **not** sufficient evidence of correct local contact derivatives.

Two alternating full-robot pairs on the same shared grid then measured:

| Metric | Retained forward remainder | Centered experiment |
|---|---:|---:|
| Wall seconds | 2.834 / 2.904 | 3.185 / 3.191 |
| Jacobian builds | 509 | 472 |
| Newton iterations | 6,643 | 6,734 |
| Ordinary residual calls | 8,524 | 8,485 |
| Subdivisions | 0 | 1 |

Despite fewer builds, centered was about 11% slower and failed 375 of 12,766
independent trajectory checks; maximum N-labelled error was 0.001778 N. The first
reported mismatch was the +X foot motor current at 18 ms. The centered prototype
was reverted; no new simulation option or physical approximation was retained.
The verified prepared-contact reuse remains in the forward hybrid path.

Tests: six independent derivative-check tests, nine normal robot Jacobian tests
(with the known promotion gate ignored), and thirteen runtime session tests pass.
The ignored diagnostic was run explicitly and still fails as documented. A final
full-robot control verifies the restored implementation against the retained
forward profile; the WASM build checks diagnostic serialization compatibility.

Evidence under `runs/full-robot/solver-performance/`:
`hybrid-sdf-{forward,central}-sweep.json`, `hybrid-sdf-final-diagnostics.json`,
`hybrid-stencil-{forward,central}-paired-{0,1}.json`, `hybrid-central-compare.json`,
`hybrid-centered-experiment.patch`, and `hybrid-stencil-final-control.json`.
The next derivative investigation needs mode-preserving probes with demonstrably
resolved reference slopes; merely switching the remainder stencil is rejected.

### Reuse complete kinematics when motion dependencies are unchanged

Prepared articulated evaluations now retain the link kinematics, joint points
and joint axes already computed while preparing contact geometry. They reuse
these only when q, qdot, qddot, every base pose/twist/acceleration, and modal
position/velocity/acceleration match bitwise. Reactions, bristle states and
thermal inputs do not change kinematics; their relevant force/residual work
still runs. Contact and kinematic validity are checked separately, so an
acceleration probe can reuse contact while requiring fresh kinematics, and a
bristle probe can reuse kinematics while requiring fresh friction forces.

Preparation receives the actual state rates. It copies precisely the rates used
by kinematics through `read_ctx`; unused grounded-base rates remain zero. A
changed motion dependency triggers the original complete forward pass. The
cache is immutable, scoped to one Jacobian assembly, and shared safely by
numerical/hybrid workers. No state is carried between steps or rejected trials.
Cached joint-axis arrays are borrowed during force evaluation rather than
allocating a clone for each joint. Only link kinematics and points returned in
`Evaluation` need owned copies. The uncached path moves its original vectors.

The first prototype cloned every cached vector and showed no useful 16-worker
gain (numerical mean 2.749 → 2.747 s; hybrid 2.763 → 2.790 s). The retained version
borrows joint axes. Alternating before/after pair order gives these results on
the same full-robot shared-grid 20 ms case:

| Path | Before wall seconds | Retained cache wall seconds | Mean change |
|---|---:|---:|---:|
| Numerical, 1 worker | 6.239 / 6.245 | 5.785 / 5.873 | 6.242 → 5.829 s; 6.6% less |
| Numerical, 16 workers | 2.769 / 2.737 / 2.711 | 2.778 / 2.712 / 2.647 | 2.739 → 2.712 s; ~1% less |
| Hybrid, 16 workers | 2.754 / 2.754 / 2.758 | 2.769 / 2.734 / 2.765 | 2.755 → 2.756 s; effectively unchanged |

The single-worker benefit supports retaining this exact reuse. The small,
variable parallel difference is not evidence of a robust parallel speedup, and
native serial timing is not a measured full-robot WASM speedup. The default
500 µs scene also keeps its result/work counts exactly (one control pair:
1.746 → 1.707 s, not a repeated performance claim).

Every before/after pair has identical final frame, event trace, solver statistics
and profile work counts. Both final numerical and hybrid independent trajectory
comparisons pass 12,766 checks, with every per-frame solver/error summary and
unit-error summary identical to the corresponding pre-cache comparison. This
continues to use the explicit shared boundaries; it does not promote the grid,
constraint reduction or hybrid derivatives as physically accurate defaults.

Validation: 20 native articulated/Jacobian/transmission tests, eight serial
Jacobian tests and thirteen runtime session tests pass. The previously documented
stiff-SDF derivative promotion gate remains explicitly ignored and unresolved.
Prepared-residual parity tests now also cover +0/-0 replacement in every state
and rate, large dependency changes, and return to the original prepared point.
These supplement all-input/rate perturbations, nonzero accelerations, flex,
loaded/absent contact and worker-count parity.

WASM builds and real Chrome passes both numerical-contact and hybrid-contact
fixtures: 21 native/browser frames each, two-contact coverage and exact replay.
Maximum native/WASM differences are 2.707e-11 and 6.284e-14 respectively. Their
0.4 s small-fixture runs take 216 and 214 ms; no before/after browser performance
claim is made. Existing browser CI exercises both paths; remote CI was not run.

Evidence in `runs/full-robot/solver-performance/`:
`kinematics-{numerical,hybrid}-{before,after}-{0,1,2}.json` (cloned prototype),
`kinematics-borrowed-{numerical,hybrid}-{before,borrowed}-{0,1,2}.json`,
`kinematics-serial-{before,borrowed}-{0,1}.json`,
`kinematics-final-{numerical,hybrid}-compare.json`,
`kinematics-default-{before,after}.json`, and
`kinematics-retained-implementation.patch`. Browser evidence is
`runs/interactive/kinematics-{numerical,hybrid}.*`.

### Balance derivative batches by worker-pool capacity

Reprofiled the retained kinematic/contact reuse on this eight-physical-core,
sixteen-logical-core host. Two opposite-order sweeps of the 31.25 µs shared-grid
full-robot case gave these means with the original eight-column batches:

| Workers | Total seconds | Jacobian assembly seconds |
|---|---:|---:|
| 1 | 5.820 | 4.304 |
| 2 | 4.151 | 2.622 |
| 4 | 3.072 | 1.539 |
| 8 | 2.815 | 1.217 |
| 16 | 2.739 | 1.144 |

All worker counts preserve the final frame, event trace, solver statistics and
profile work counts exactly. At sixteen workers, assembly was only about 42%
of total time; the old 93% profile is no longer representative.

Two-column batches distribute expensive geometry-changing probes more finely
among workers. Paired measurements, alternating before/after order, showed:

| Case | Eight-column mean | Two-column mean | Total time reduction |
|---|---:|---:|---:|
| Numerical, 2 workers (2 pairs) | 4.144 s | 4.186 s | -1.0% |
| Numerical, 4 workers (2 pairs) | 3.088 s | 3.011 s | 2.5% |
| Numerical, 8 workers (3 pairs) | 2.743 s | 2.563 s | 6.5% |
| Numerical, 16 workers (3 pairs) | 2.702 s | 2.452 s | 9.2% |
| Hybrid, 16 workers (3 pairs) | 2.762 s | 2.537 s | 8.1% |

The retained policy is shared in `sim_core::linearization_batch_columns`:
capacities of two or fewer retain eight-column batches; larger pools use two.
The compiler fallback and hybrid remainder both use it. Single-worker and WASM
paths still evaluate the input list serially, and small components still avoid
column splitting. Each batch has private scratch, and indexed collection retains
exact input/triplet order. Perturbation sizes and residual equations are unchanged.

The final shared policy was checked explicitly at two and sixteen workers.
`sim-profile` now records `compiled_derivative_worker_capacity`,
`compiled_derivative_batch_columns`, and `requested_rayon_threads`. Capacity comes
from the actual current pool, not just the environment request. These fields
report scheduling policy rather than measured simultaneous utilization; batch
size is unused on the serial path. The compiler's pool-capacity reporting is
also checked inside the existing installed-pool derivative tests.

The default 500 µs robot scene improved in three alternating pairs from
1.714 / 1.725 / 1.726 s to 1.500 / 1.507 / 1.506 s: mean **1.722 → 1.504 s**, or
**12.6% less wall time** for the same 20 ms simulation. Every paired final frame,
event trace, solver statistic and work count is identical, including the default
scene's six subdivisions. The finer shared-grid case retains zero subdivisions.
Both independent numerical/hybrid trajectory comparisons pass all 12,766 checks,
with every per-frame solver/error summary and unit-error summary identical to the
pre-batching result. The scheduling change does not promote the chosen timestep,
constraint identities, or experimental analytic derivatives.

The updated retained-path means are:

| Work | Default 500 µs scene | Shared-grid 31.25 µs scene |
|---|---:|---:|
| Total | 1.504 s | 2.452 s |
| Jacobian assembly | 0.658 s | 0.911 s |
| Ordinary residual evaluations | 0.499 s | 0.951 s |
| Factorization | 0.316 s | 0.422 s |

This makes repeated residual work and rebuild frequency substantial remaining
opportunities. Factorization is also a larger fraction now; these measurements
do not themselves justify replacing the solver. Timers are nested, so event
location and total Newton timers must not be added to the rows above.

Validation: compiler derivative triplet checks and hybrid loaded-contact matrix
checks now include sixteen workers as well as one, two, four and eight. The
compiler check, 20 normal robot tests and 13 runtime session tests pass. The known
stiff-contact derivative promotion gate remains ignored and unresolved. WASM
builds; real Chrome numerical and hybrid contact fixtures each pass all 21
native/browser frames, two-contact coverage and exact replay (maximum differences
2.707e-11 and 6.284e-14). Existing browser CI covers these paths; remote CI was not
run. There is no claimed WASM speedup from native batching.

Evidence in `runs/full-robot/solver-performance/`:
`worker-sweep-{1,2,4,8,16}-{0,1}.json`,
`batch-sweep-{numerical,hybrid}-w{8,16}-b{8,2}-{0,1,2}.json` (only tested combinations),
`batch-low-workers-w{2,4}-b{8,2}-{0,1}.json`,
`batch2-{numerical,hybrid}-compare.json`, `batch-policy-w{2,16}.json`, and
`batch-default-{before,after}-{0,1,2}.json`. Browser evidence is
`runs/interactive/worker-batch-{numerical,hybrid}.*`. To reproduce a worker count,
set `RAYON_NUM_THREADS` before running the documented profiler command; verify the
reported capacity and batch size in its output.

### Remove backtracking probes that a stale matrix cannot use

The reused-Jacobian path previously evaluated a full correction and, if it did
not decrease the scaled residual, searched every smaller step. It then discarded
all those candidates, restored the original point and residual, and rebuilt the
Jacobian regardless of the best partial result. It now performs that same refresh
immediately after the finite, nondecreasing full trial. Fresh matrices retain the
existing complete backtracking policy. The final correction still receives a
fresh residual evaluation and fixed raw-residual acceptance checks.

Two regressions failed before the change and pass afterward. Reusing a positive
factorization for `F(x)=1-x` at x=0 previously probed
`0,-1,-1/2,...,-1/256,1`; it now probes only `0,-1,1`, reaches the same exact root,
and rebuilds at the original `(x,F)=(0,1)`. A second version makes `(-0.75,0)` an
undefined residual domain: the old solver failed at a discarded -0.5 trial;
the new path reaches the valid root without probing that irrelevant interval.
A separate cubic-root regression confirms that a fresh Jacobian still backtracks
when its full correction is too large. The no-root/false-convergence tests remain
active and passing.

The library default has eight halvings. The robot runtime uses twelve
(`min_line_search=1/4096`), which explains the exact savings below. A new
`stale full-step refreshes` profile counter records each shortcut. `sim-profile`
also records every island's actual integrator and Newton settings, including
tolerances, iteration limit and minimum line-search factor.

Three alternating native pairs at sixteen workers:

| Case | Before mean | After mean | Ordinary residual calls | Stale full-step refreshes |
|---|---:|---:|---:|---:|
| Default 500 µs robot | 1.514 s | 1.415 s | 4,285 → 3,541 | 62 |
| Shared-grid numerical, 31.25 µs | 2.419 s | 2.376 s | 8,110 → 7,726 | 32 |
| Shared-grid hybrid, 31.25 µs | 2.530 s | 2.478 s | 8,524 → 8,008 | 43 |

The default case saves **744 ordinary residual evaluations (17.4%)** and **6.6%
total runtime**. The finer numerical/hybrid cases save 1.8%/2.0% total runtime.
Every residual-call reduction is exactly twelve times the new refresh counter.
Final frames, event traces, solver statistics and every other existing profile
work count are identical in every pair. Jacobian builds, Newton iterations and
subdivisions are unchanged; this eliminates discarded work rather than claiming
fewer accepted-state convergence iterations.

Both independent full-robot trajectory comparisons pass 12,766 checks, with
identical per-frame solver/error summaries and unit-error summaries to the
pre-change runs. These remain short shared-grid diagnostics; timestep accuracy,
stiff-contact derivative validation and analytic promotion are still open.

The selected suites pass 92 tests: seven solver tests, 28 dynamics tests,
44 robot tests and 13 runtime session tests. The documented stiff-SDF derivative
promotion gate remains explicitly ignored. The regression file is already in CI.
WASM builds; real Chrome numerical and hybrid contact fixtures each pass all 21
native/browser frames, contact coverage and exact replay. Maximum differences
remain 2.707e-11 and 6.284e-14. The small-fixture browser timings are observations,
not a before/after WASM performance claim; remote CI was not run.

Evidence in `runs/full-robot/solver-performance/`:
`stale-probes-{default,numerical,hybrid}-{before,after}-{0,1,2}.json`,
`stale-probes-{numerical,hybrid}-compare.json`,
`stale-probes-final-control.json` (including effective integrator settings),
`stale-probes-before-regressions.txt` and `stale-probes-solver-change.patch`.
Browser evidence is `runs/interactive/stale-probes-{numerical,hybrid}.*`.

### Longer stale-probe validation and correction diagnostics

Extended the unchanged default 500 µs scene from 20 ms to 100 ms (50 report
frames, sixteen derivative workers). Before/after `sim-validate steps` reports
are exactly equal, including all 291 attempted solve stages, their states,
rates, residuals, original constraint rows, generalized reactions and contact
forces. Neither capture reaches its 4,096-attempt capacity; both finish without
session error. There are 246 successful nonlinear trials and 45 failed trials.
A successful trial is not necessarily a committed segment: root searches and
outer retries can discard one. The solver reports 202 steps, 44 subdivisions
and 1,206 events. Contact counts across attempted stages range from zero to eight.

Three alternating profiler pairs give 11.054 s before and 10.301 s after, a
**6.8% runtime reduction**. Ordinary residual evaluations fall from 31,508 to
25,292: exactly twelve avoided probes for each of 518 stale full-step refreshes.
Every pair has identical final frames, complete event traces, solver statistics,
outer refinement counts and all other pre-existing work counters. Both paths
use 6,002 Newton iteration entries and 2,743 fresh Jacobians. The longer case
therefore confirms elimination of unused work; it does not demonstrate fewer
subdivisions or realtime performance (the retained run costs about 103 wall
seconds per simulated second over this particular interval).

The retained profile now spends 4.714 s assembling Jacobians, 3.027 s in ordinary
residual evaluations and 2.355 s in factorization. These are separate inner
timers; do not add their enclosing solve/step timers. The original 93% assembly
profile is no longer representative.

Every successful trial meets its fixed raw-residual bounds, with maximum
residual/bound 0.5471. Across those trial stages, original position-closure rows
have maximum/RMS 3.247e-7 / 4.012e-8 m; velocity closure has maximum/RMS
3.963e-5 / 2.345e-6 m/s. Transmission-angle closure peaks at 4.701e-11 rad.
The full summary separates dimensionless angular-alignment rows, metre rows,
and radian transmission rows and includes acceleration and stabilized residuals.
These are stage diagnostics using actual rates, not a timestep-refinement proof.

Failed trials consume 1,837 of the 6,002 audited iteration entries (30.6%; this
is an iteration fraction, not a measured wall-time fraction). Of 45 failures,
33 reach the iteration limit and twelve exhaust the fresh line search. Two
iteration-limit failures have terminal raw residuals already within their fixed
bounds. Small raw residuals alone do not establish a sufficiently small state
correction, so accepting them without further checks is not justified.

`NewtonIteration` now optionally records the eight largest corrections relative
to each unknown's actual negligible-correction bound, their signed values and
bounds, and the existing `negligible`, `tight`, and `at_floor` decisions. Legacy
records deserialize with no correction entry; iterations that exit before a
linear solve also have no correction entry. The diagnostic is opt-in through
the existing attempt audit and changes no solver acceptance or refresh rules.

Re-running all 291 attempts with these diagnostics reproduces every prior field
exactly. At the failed 49.5 ms stage, the last computed correction still exceeds
the ordinary bound: a knee reaction multiplier is 6.42 times its bound and the
+X worm rotor-speed correction is 1.08 times its bound. At the failed 71.25 ms
stage, the last two corrections meet the ordinary bound but fail the stricter
reused-Jacobian check. The final correction is dominated by knee reaction
multipliers (largest ordinary-bound ratio 0.220). The next iteration exits at
the budget before computing another correction. This identifies a concrete
experiment: reserve a guarded fresh-Jacobian convergence check before the
iteration budget expires. It has not yet been implemented or promoted, and
reaction-coordinate noise does not excuse differences in physical contact loads.

Validation for the added diagnostics: all seven solver convergence tests and
both implicit-stage audit tests pass. The new analytic linear-system regression
exposes an order-one correction hidden by a tiny equation residual and confirms
that enabling the audit leaves the solution and work diagnostics unchanged.
Both test files already run in browser CI. A native release validator rebuild
and a WASM `sim-web` check pass; no new browser execution or remote CI is claimed.

Reproduce the longer captures with each saved before/after release validator:

```sh
RAYON_NUM_THREADS=16 target/release/sim-validate steps runs/full-robot/reproduced.scene.json 50 4096
RAYON_NUM_THREADS=16 target/release/sim-profile runs/full-robot/reproduced.scene.json 50 10
```

Evidence in `runs/full-robot/solver-performance/`:
`stale-probes-long-{before,after}-steps.json`,
`stale-probes-long-{before,after}-{0,1,2}.json`,
`stale-probes-long-summary.json` (includes scene and baseline binary hashes),
`stale-probes-long-corrections-steps.json`, and
`stale-probes-long-correction-summary.json`. Capture process timings include
diagnostic evaluation/serialization; performance claims use the separate
profiler's session-step timer. The native/browser full-trajectory accuracy and
stiff-contact derivative promotion gates remain open.

### Experimental fresh check at the iteration budget: not promoted

Tested an immediate refresh when the last permitted correction meets ordinary
negligibility bounds but fails the tighter reused-Jacobian check. The prototype
keeps the original point and residual, clears the stale factorization, and
retries the same iteration with a fresh matrix. Existing correction and final
raw-residual gates remain intact; no tolerance, iteration limit, physical
parameter, or nominal timestep changes.

A focused scalar regression fails on the retained solver and passes on the
prototype: a factorization from `F=2x` is reused for `F=x-1e-9` with a one-iteration
budget. The retained solver takes a partial correction and exhausts the budget;
the prototype refreshes and reaches the exact root in one counted iteration.
A second regression supplies an inflated fresh derivative for a constant,
nonzero residual; the fresh check still rejects the nonexistent root. All nine
convergence tests pass with the prototype. Both experimental tests are preserved
in its patch, not installed as ignored tests in the retained suite.

The 100 ms default robot exercises the new branch once, at 71.25 ms. This trial
now succeeds. Across the complete interval:

| Work | Retained | Prototype |
|---|---:|---:|
| Attempted implicit solves | 291 | 279 |
| Failed attempts | 45 | 39 |
| Subdivisions | 44 | 38 |
| Newton iteration entries | 6,002 | 5,854 |
| Fresh Jacobians | 2,743 | 2,667 |
| Ordinary residual evaluations | 25,292 | 24,580 |

Every successful trial still meets its fixed residual bounds (worst ratio
0.5735), and maximum metre position-closure remains 3.247e-7. Reported frames
match exactly through 70 ms and first differ at 72 ms. Contact counts and
ordered `(link, other)` identities match in all 51 frames. The largest matched
contact-force component change is 0.04598 N in ground contact for link 11 at
84 ms. These are real trajectory changes following a changed integration grid;
they cannot be accepted solely because fewer retries occurred.

Independent full finite-difference comparisons were run before and after using
the same 50-action recording, nominal 500 µs timestep, default tolerances, and
no shared diagnostic breakpoints. Both fail **24,857 of 58,870 comparisons**.
Both first fail at 12 ms in +X foot-servo bridge current: difference 2.5003e-5 A
against tolerance 1.2250e-6 A. That predates the prototype's first intervention.
The complete comparison records through frame 35 (70 ms) are identical, as are
the reference solver statistics at every frame. The reference itself is not a
timestep-refined ground truth.

| Physical difference against independent FD | Retained | Prototype |
|---|---:|---:|
| Maximum force-channel difference | 29.3485 N | 29.3485 N |
| RMS force-channel difference | 2.30921 N | 2.30812 N |
| Maximum position-channel difference | 0.314372 mm | 0.314345 mm |
| RMS current-channel difference | 0.0370800 A | 0.0370571 A |

The worst force discrepancy occurs at 54 ms in the -X sliding-foot contact
signal, before the prototype changes anything. Small later RMS improvements do
not resolve the existing trajectory/reference failure or establish timestep
accuracy. The experiment therefore remains **unpromoted**. The solver source
and test suite have been restored exactly to the pre-experiment implementation;
release runtime binaries were rebuilt. A restored 20 ms profile reproduces the
retained final frame, full event trace, solver/refinement statistics and every
profile work counter exactly. All seven retained convergence tests pass.

The single experimental 100 ms profile took 10.217 s. The comparative validators
observed candidate times of 10.488/10.276 s, but these were not repeated isolated
performance pairs, and some compilation overlapped the long validation process.
No robust wall-time speedup is claimed. Each independent reference took about
166–168 s; these validation costs are not interactive simulator performance.

This result prioritizes resolving the earlier derivative/integration-grid
divergence and comparing identical saved states before relaxing any reuse
criterion. The targeted fresh check remains a useful candidate for that work,
with its original acceptance gates preserved.

Evidence under `runs/full-robot/solver-performance/`:
`budget-refresh-experiment.patch`, `budget-refresh-{before-test,after-tests}.txt`,
`budget-refresh-{before,after}-{frames,recording,compare}.json`,
`budget-refresh-after-{steps,profile}.json`, `budget-refresh-summary.json`,
`budget-refresh-binaries.json`, and `budget-refresh-restored-control.json`.
Preserved native prototype binaries are `/tmp/sim-{profile,validate,session}-budget-refresh-experiment`;
these are local investigation aids, not distributed artifacts or promoted defaults.

### First coarse-grid divergence: retries versus derivative error

Captured the independent whole-residual finite-difference path through 12 ms
with `sim-validate steps` (24 attempts, no truncation or session failure).
At 10–10.5 ms the compiled path fails a fresh line search and retries as two
250 µs steps; the independent path accepts one 500 µs step. Before that trial,
the largest state difference is only 2.108e-8 in a knee reaction multiplier.
Different integration grids precede the first failing report at 12 ms.

Adding the diagnostic boundary `0.01025` to both paths removes that first
disagreement: **8,046/8,046 checks pass** over 0–12 ms, with maximum force-channel
difference 2.244e-8 N and maximum error/tolerance ratio 0.02748. Both paths use
25 steps, 145 events and no subdivisions. This is a diagnostic boundary selected
from the failed trial, not a hard-coded runtime fix or timestep-accuracy proof.

Added `CompareConfig.shared_prefix_frames` (default zero) to distinguish
divergence inherited from earlier states from behavior at a matching starting
state. Both sessions replay that prefix with the candidate settings and the
same seed/actions, including controller history. The validator requires exact
runtime snapshot, stored-channel and episode-frame equality before proceeding.
It then clears both matrix caches, switches only the reference to independent
whole-residual derivatives and its requested timestep, and disables reference
event-matrix reuse. Prefix timings and the verified starting time are reported
separately in `shared_prefix`; warmup frames do not inflate independent accuracy
counts. The prefix must leave at least one action for independent comparison.
This mode diagnoses a suffix; it must not be presented as validation of the
full trajectory from the authored initial state.

Replaying five shared report frames establishes matching states at 10 ms. The
following two milliseconds give:

| Diagnostic suffix | Checks | Mismatches | Maximum force-channel difference |
|---|---:|---:|---:|
| Both nominal 500 µs, adaptive retries independent | 2,300 | 291 | 0.157809 N |
| Both nominal 500 µs, shared 10.25 ms boundary | 2,300 | 0 | 3.767e-9 N |
| Candidate 500 µs, reference 250 µs | 2,300 | 356 | 0.334169 N |

The first suffix again has one compiled-path subdivision and none in the
reference. With the shared boundary both have no subdivisions, identical work
statistics, and maximum error/tolerance ratio 0.004998. The timestep-refined
suffix retains the same initial state and controller history but fails the
strict derivative-level tolerances. It demonstrates local timestep sensitivity;
one halving is insufficient to establish a converged physical reference.

These results strongly implicate the retry grid in the first reported physical
disagreement. They do not prove the derivatives agree at every failed Newton
iterate or explain the different convergence behavior there. The next useful
check is the actual stage residual/Jacobian near the failed solve, particularly
the knee reaction directions, while continuing timestep/event accuracy work.
The 100 ms discrepancy and full-trajectory promotion gates remain unresolved.

All 14 runtime session tests pass. The new prefix regression uses changing
commands, checks controller-event continuity, verifies that warmup frames are
excluded, rejects an empty independent suffix, and ensures that timestep error
after the prefix still fails. Native release validation builds and WASM checks.
Re-running the zero-prefix shared-grid case reproduces all prior comparison
fields exactly after excluding timing and newly added metadata. No physics,
convergence policy, or production timestep changes were made.

Example configuration for the matching-prefix, shared-boundary diagnostic:

```json
{"shared_prefix_frames":5,"shared_step_breakpoints_s":[0.01025]}
```

Use `sim-validate compare RECORDING CONFIG` with six recorded report actions.
Evidence under `runs/full-robot/solver-performance/`:
`coarse-first-divergence-reference-steps.json`,
`coarse-first-divergence-reference.scene.json`,
`coarse-first-divergence.recording.json`,
`coarse-first-divergence-{grid,prefix,prefix-grid,prefix-refined,grid-control}-compare.json`,
their configuration files, and `coarse-first-divergence-summary.json`.

### Derivative noise at the failed Newton linearization

The attempt audit now captures the increment and base residual at each trial's
last fresh matrix build. This adds two vectors per audited trial; normal solves
do not copy them. Legacy records deserialize with no such point. A terminal
failed trial is not necessarily the point where its Jacobian was built, so the
new diagnostic deliberately uses the captured fresh-linearization point.

Added the shared `sim_dynamics::jacobian_check::check_implicit_jacobian` helper
and `sim-validate stage-jacobian SCENE WARMUP_FRAMES DURATION_S [CHECK_CONFIG]`.
The adapter reconstructs exactly the Newton residual's coordinates:
`x_stage = x_old + theta*u` for differential unknowns, `x_old + u` for algebraic
unknowns, and `rate = u/h` for both. It checks the assembled derivative
`J_x * diag(stage_weights) + J_rate/h` against independent residual differences.
Provided derivatives are reassembled, not extracted from cached factors. Before
probing, the live residual must reproduce the recorded base bit for bit; callers
must preserve the system and external inputs. A changed context is an error,
not an inconclusive or successful derivative check.

The CLI holds the initial action, replays the warmup, and captures at most 128
attempts over at most one report period. Trials that never build a fresh matrix
have no point to check. Captures include rejected solves and successful recovery
steps. State-column probes refer to Newton increments; rate-column probes check
the adapter's identically zero auxiliary derivatives. The existing stencil,
branch and roundoff checks and tolerances remain unchanged.

Warmed the default robot through 10 ms and examined the next 0.5 ms, including
the failed full step and both successful half-steps. All recorded base residuals
match the live system bit for bit. Sweeping the checker step multiplier gives
the following results at the failed trial's last fresh linearization:

| Stencil multiplier | Resolved mismatches | Inconclusive comparisons |
|---|---:|---:|
| 1e-5 | 380 | 86 |
| 1e-6 | 331 | 126 |
| 1e-7 | 342 | 1,244 |
| 1e-8 | 242 | 3,706 |

A repeatable example is the +Y knee angular-closure row (reduced row 54) versus
the +Y foot-servo speed increment (column 33). The compiled derivative is
**0.0018992892**. Independent estimates at the first three stencil sizes are
**-1.4803e-9, 3.4787e-9 and 8.2897e-8**, respectively. These remain resolved
mismatches at the unchanged tolerances. Other large discrepancies involve knee
closure rows and base velocity/angular-velocity increments. The successful
half-step matrices also contain resolved mismatches, so convergence alone is
not a derivative validation gate.

This implicates cancellation/noise in the compiled numerical constraint
derivatives as a concrete conditioning target. It is not a claim that every
reported discrepancy is roundoff: contact-sensitive probes remain separately
inconclusive, and smaller stencils create additional unresolved references.
The next experiment should evaluate numerically stable relative-motion forms
of the closure equations and their derivatives, retaining all original physical
constraints and validating full trajectories and total cost. No analytic path
has earned promotion from these checks.

Validation: three implicit-audit tests, six derivative-checker tests and all 14
runtime session tests pass. The new analytic mixed-DAE test verifies differential
and algebraic stage weights, detects an intentionally wrong derivative, rejects
a changed residual context and refuses missing legacy linearization points.
Native release validation builds and WASM checks. A new default robot capture
through 12 ms matches every existing attempted-solve field after removing only
the added metadata; the production equations and solver policy are unchanged.

Reproduce with a JSON check configuration such as `{"step":1e-6}`:

```sh
RAYON_NUM_THREADS=16 target/release/sim-validate stage-jacobian runs/full-robot/reproduced.scene.json 5 0.0005 CHECK_CONFIG
```

Evidence under `runs/full-robot/solver-performance/`:
`stage-jacobian-h1e-{5,6,7,8}.json`, corresponding `.config.json` files,
`stage-jacobian-summary.json`, and `stage-jacobian-capture-control.json`.

### Relative angular-motion rewrite: rejected by robot validation

Tested an algebraically equivalent angular-closure form using relative angular
motion. For world-space row axis `e`, mating axis `a`, and
`w_rel = w_b - w_a`, its derivatives are
`phi_dot = e dot (w_rel cross a)` and
`phi_ddot = e dot ((alpha_b - alpha_a - w_a cross w_rel) cross a
                   + w_rel cross (w_rel cross a))`.
The prototype shared this helper between production residuals and closure
diagnostics and used the equivalent relative acceleration expression in the
hybrid rate columns. Every original constraint, compliance term, reaction and
state remained present; no tolerance or physical parameter changed.

Three focused tests pass: agreement with the expanded equations at 128 general
motion samples, an analytic cosine-angle case with nonzero relative speed and
acceleration, and exact constant dot-product closure under common rotation up
to angular-speed scale 1e8. Replacing the helper with the original expanded form
makes the common-rotation regression fail, establishing a real cancellation
improvement in that isolated case.

The full robot nevertheless rejects this prototype. At checker multiplier 1e-6,
the failed trial's matrix has 290 resolved mismatches rather than 331, but its
warmup and linearization point have changed, so these counts are not a controlled
comparison at the same state. Translational closure and some angular rows still
have large discrepancies. The original problematic 10.5 ms full trial still
fails and needs both half-step recoveries.

More decisively, the 0–12 ms independent comparison now has 1,756 failures in
8,046 checks, eight compiled-path subdivisions and maximum force difference
0.601375 N. The shared 10.25 ms boundary, which passes with the retained code,
now has 1,716 failures and seven compiled-path subdivisions; the reference has
none. Better isolated invariance and fewer matrix-check mismatches do not earn
promotion when convergence and trajectories deteriorate.

The entire prototype and its three tests are preserved in
`relative-axis-experiment.patch`; the original three source files were restored
byte for byte and runtime release binaries rebuilt. The restored shared-grid
comparison passes all 8,046 checks and exactly reproduces every previous report
field except timing. No part of this rewrite remains enabled or installed as an
ignored regression. There is no claimed performance improvement.

A diagnostic using the existing opt-in hybrid Jacobian with the original
residual equations accepts the 10.5 ms step in one trial. Its stage matrix still
has 161 resolved mismatches and 119 inconclusive comparisons, so the complete
hybrid Jacobian remains unpromoted. This is not an identical-state comparison
against the compiled path: the hybrid also supplies derivatives during warmup.
It motivates a controlled experiment isolating exact rate derivatives from the
numerical state-derivative remainder. The compiler currently accepts only a
complete local Jacobian or full numerical fallback; a reusable rate-partial hook
would allow that experiment without duplicating the physics or adopting all
hybrid derivatives at once. Any such extension still needs matrix, trajectory,
native/WASM and total-runtime validation.

Evidence under `runs/full-robot/solver-performance/`:
`relative-axis-experiment.patch`,
`relative-axis-{unit-tests,expanded-regression}.txt`,
`relative-axis-{stage,short-compare,grid-compare}.json`,
`relative-axis-restored-grid-control.json`,
`stage-hybrid-original.scene.json`, and `stage-hybrid-original.json`.


### Complete rate-partial hook: small fixtures pass, full robot not promoted

Added the shared `Behavior::rate_jacobian_at` hook and compiler fallback that
keeps numerical state derivatives while accepting complete supplied rate
partials. Full Jacobians take precedence. The articulated implementation shares
its existing structured derivative blocks through `options.articulated_rate_partials`
(registry `jacobian.rates`), default false. No default physical equations or
solver acceptance settings change. The new profiler counter overlaps with FD
slots, because state derivatives are still numerical.

A 64-state ring with state-dependent rate coefficients reduces local residual
calls from 129 to 65 per matrix. Its numerical state triplets are exactly equal
to the full fallback; rate entries match the known formula. Counts and matrices
agree at workers 1/2/4/8/16 and in the serial feature build. Fifteen runtime
session tests pass, including a moving rate-only pendulum comparison. Robot
suites pass 8 articulated, 4 constraint-audit, 9 derivative and 3 transmission
tests; the existing stiff-contact derivative promotion test remains ignored
with its explicit failed-gate reason. The empty-constraint audit fixture was
repaired to rebuild its model after removing loops, rather than invalidating a
compiled state schema by clearing internal constraints in place.

The contact fixture passes 2,113 independent trajectory checks and 21
native/browser frames: maximum native/WASM difference 7.0954e-9, two contacts,
exact replay and 22 heartbeats. Browser time is 220.8 ms for 0.4 simulated seconds
on this small fixture, not a full-robot speed claim. The isolated bundle and
reports are in `runs/interactive/rate-partials-*`. Browser CI includes the
rate-only case; remote CI has not been run. The default 20 ms robot control
matches the earlier retained final frame, events, solver statistics, outer
refinements and existing work counters exactly; its new rate-partial counter
is zero.

The full robot rejects promotion. The rate-only 0–12 ms comparison fails
1,972/8,046 checks, with maximum force error 1.421327 N and 289 subdivisions
versus zero for the whole-residual numerical reference. The first failing
report is at 4 ms; all 289 subdivisions accumulate between 2 and 4 ms.
Adding the previously diagnostic shared 10.25 ms boundary does not help: it is
after the divergence. The intended 100 ms profile terminates at 35.57421875 ms
with Newton nonconvergence, so its empty profile JSON is not a timing result.
Short-run timings (roughly 59–68 seconds candidate versus 7 seconds reference)
are regression observations, not a paired benchmark or a speedup claim.

At the default path's captured terminal state in the failed 10.5 ms trial,
rate-only and full hybrid rate matrices are exactly equal, while rate-only
and default state matrices are exactly equal. Residuals match bit for bit in
both comparisons. These checks isolate the intended derivative replacement at
one point; they do not validate the derivative globally. The rate-only stage
check accepts its own 10.5 ms trial but still reports 177 resolved mismatches
and 123 inconclusive comparisons at radius multiplier 1e-6.

Local evidence under `runs/full-robot/solver-performance/`:
`rate-partials.scene.json`, `rate-partials-{stage,short-compare,grid-compare}.json`,
`rate-partials-profile.stderr`, `rate-partials-default-control.json`,
`rate-partials-matrix-point.json`, and
`rate-partials-vs-{hybrid,numerical}-matrices.json`.
The earlier matrix point is a terminal iterate, not the captured last fresh
linearization. The separate `rate-partials-stage-matrix-point.json` reconstructs
the last fresh linearization from its increment, initial state and timestep,
and requires its recorded residual to reproduce bit for bit.


#### Identical-point correction probes isolate the early retry burst

Extended the reusable `sim-runtime` example `compare_matrices` with optional
stage weights, a recorded-base check, common row/column equilibration, singular
values, dense linear-solve defects and fresh residual probes at correction
fractions 1 through 1/4096. An affine mixed algebraic/differential test verifies
the weights, detects a deliberately wrong rate block and rejects a changed
capture or overflowing combined matrix. This runs in browser CI as a native
test. No production solver path uses the diagnostic SVD or dense correction.

At the original default path's last fresh 10.5 ms linearization, the compiled
state matrices match exactly between default and rate-only options. With common
scales derived from the default matrix, initial residual norm is 1.179627e-4;
a full default correction increases it to 4.764346e-4, while the rate-only
correction reduces it to 4.659571e-6. Both scaled matrices have condition number
about 4.48e5. This is evidence of a better local direction, not proof of full
trajectory accuracy. The full-hybrid and rate-only corrections also nearly
agree at that point when compared under their own common scales; norms from
different choices of scaling must not be compared directly.

The rate-only 0–4 ms attempt capture has 596 attempts with no truncation. Its
first failure is the 2.5–3 ms trial. Reconstructing its last fresh linearization
again reproduces the base residual exactly. Rate-only and full hybrid have
identical rate matrices; their different state entries change the predicted
correction residual by only 4.68e-12 in the common scaled norm. Nevertheless,
both full corrections increase the actual norm from 1.171187e-4 to about
2.42077e-3. Even at fraction 1/4096 the norm is slightly larger than its initial
value. This refines the earlier state/rate-interaction hypothesis: adopting the
full hybrid state remainder does not repair this particular captured point.

The dominant full-correction row is the +Y knee's angular closure (`lambda3`):
raw residual grows from -1.53549e-10 to about -3.77602e-9, despite a near-zero
linear prediction. Its numerical row scale is 6.41e5. Other dominant rows are
the +/-Y knee angular closures. The default numerical rate matrix also gives
a poor correction at this rate-only captured state; its condition number under
the same common scales is about 1.31e9. This is not evidence that the exact
rate block alone caused a wrong physical force law, nor that dense factorization
should replace the production solver.

Combining supplied rates with the existing topology-proved structural loop
identities removes the early burst: through 12 ms, zero subdivisions, 24 steps,
145 events and all 8,046 independent numerical comparisons pass (maximum force
difference 2.18453e-8 N, maximum error/tolerance ratio 0.021746). Single-run
candidate/reference times were 0.201/3.449 seconds. The reference uses the same
structural-identity formulation; this is neither a paired production speedup
nor validation against the original unreduced equations. Both options remain
default false. Longer trajectories, every original closure equation, reaction
forces and timestep accuracy remain promotion requirements.

Additional local evidence: `rate-partials-stage-vs-{numerical,hybrid}.json`,
`rate-partials-first-steps.json`, `rate-partials-first-failure-point.json`,
`rate-partials-first-failure-vs-{numerical,hybrid}.json`, and
`rate-partials-identities-{short.recording,short-compare}.json` under the same
solver-performance directory. `compare_matrices` and its point schema are
documented in the interactive README.


The extended 100 ms comparison **fails** 13,145/58,870 checks. First disagreement
is the +X foot-slide bridge current at the 50 ms report; reports through 48 ms
pass. Maximum force difference is 1.593979 N. Candidate/reference both have 200
committed steps and 1,206 events, but 30/27 subdivisions and maximum Newton
iterations 38/37. Single-run times are 7.161/111.809 seconds, with some overlapping
diagnostic/test work; no paired performance claim is justified. This substantially
limits the short passing result and prevents promotion.

The separate 12 ms attempt audit has 24 successful attempts, no discarded
capacity, and recomputes all original closure equations at their actual stage
states/rates. Across the eight certified angular identities, maximum absolute
position/velocity/acceleration/stabilized residuals are respectively
1.30104e-18, 1.11022e-16, 2.84217e-14 and 2.65413e-14 (position is dimensionless
axis alignment; derivatives use seconds). Original translational closure stays
below 5.68844e-9 m and 1.22869e-6 m/s; its maximum stabilized residual is
1.66499e-12 m/s². Transmission closure stays below 1.52071e-12 rad and
5.82589e-10 rad/s. These support the recognized identities over this short
trajectory, not globally calibrated physics or original-formulation force
agreement. Scaled rank histories include (QR,SVD)=(16,16),(16,17),(17,17);
numerical rank at a single pose must not be used to delete further equations.

Extended evidence: `rate-partials-identities-long.recording.json`,
`rate-partials-identities-long-compare.json`, and
`rate-partials-identities-short-steps.json`. The next convergence investigation
should locate the first differing trial before the 50 ms report using identical
warmup states and separately test timestep refinement. Do not turn the combined
option on because its first 12 ms pass.


Halving only the independent reference timestep to 0.25 ms over the short
12 ms recording also fails: 2,467/8,046 comparisons, first at the 2 ms report,
maximum force difference 3.908659 N. The reference has 19 subdivisions and 67
committed steps, compared with candidate zero and 24; both have 145 events.
Controller sampling times remain fixed. This is a failed timestep-consistency
check, not evidence that the smaller-step trajectory is physically converged.
Evidence: `rate-partials-identities-refined.config.json`,
`rate-partials-identities-refined-compare.json`; the compact overall result is
`rate-partials-experiment-summary.json`. The short same-step passing result must
not be presented as adequate integration accuracy.


### Paired suffix audits isolate the 50 ms divergence; position prototype deferred

`CompareConfig.attempt_audit_limit` now optionally captures both comparison
suffixes (default zero, maximum 10,000 per island). Capture begins after shared
warmup and reports capacity exhaustion explicitly. The trajectory gate is
unchanged; a passing trajectory report does not certify a complete truncated
attempt log. Tests verify identical per-frame accuracy/work with capture on,
matching initial suffix states, exclusion of warmup, bounded capture and invalid
limits. The archived-stage example reuses `check_implicit_jacobian` and requires
a bit-identical captured base residual before independent derivative probes.
No production physics/solver option changes through either diagnostic.

With 24 shared frames, both sessions reproduce exactly the same state and
controller history at 48 ms. The 48–50 ms comparison fails 343/2,312 checks.
Candidate rejects both 49–49.5 and 49.5–50 ms full trials; reference accepts the
first and rejects the second. Both recover through half steps. Audit buffers
are complete (8 candidate and 6 reference attempts). The first differing trial
precedes the first differing report; this is not solely accumulated warmup error.

Giving both runs absolute boundaries 49.25 and 49.75 ms makes the suffix pass
all 2,312 checks, maximum force difference 9.59858e-9 N and maximum tolerance
ratio 0.0059152. Both have six successful suffix attempts and cumulative
statistics 102 committed steps, 18 subdivisions, 603 events. These selected
boundaries remain diagnostic and are not a production timestep policy or
independent evidence of time-discretization accuracy.

The rejected 49–49.5 ms trial's captured last fresh matrix remains unreliable:

| Checker radius multiplier | Resolved mismatches | Inconclusive comparisons |
| --- | ---: | ---: |
| 1e-5 | 193 | 3,657 |
| 1e-6 | 134 | 6,029 |
| 1e-7 | 132 | 10,730 |

These are identical-point checks with the original retained equations; all
three reproduce the captured base residual exactly. Resolved discrepancies
include translational knee closure derivatives with respect to common base
translation. Contact-sensitive columns also have many unresolved probes;
smaller stencils do not turn these into passing gates.

Tested a reusable base-relative anchor separation prototype. The forward
kinematics retained a per-link displacement relative to its connected-tree
base, used for same-tree loop separation; independent bases kept world-space
separation. An open, moving four-bar test establishes exact invariance under
common translations from 1e-8 to 1e9 m, and fails against the original code
already at 1e-8 m. All 25 selected mechanism tests pass with the prototype
(one pre-existing stiff-contact promotion gate remains ignored).

The prototype preserves the default 12 ms shared-grid gate (8,046 passing
checks, zero subdivisions, max force difference 2.11358e-8 N), but does not
repair the later failure: its 50 ms rate-plus-identities comparison still fails
352/29,920 checks, first at 50 ms, maximum force difference 0.700994 N and
20/18 candidate/reference subdivisions. Concurrent diagnostic timings are not
a paired speed benchmark. With no demonstrated later convergence benefit or
full promotion evidence, the prototype and regression are preserved as a patch;
all three edited source/test files were restored byte for byte. No additional
coordinate implementation or experimental switch is installed.

The original captured attempts provide a more targeted next hypothesis. The
failed 49.5 ms trial has newly active +X foot-ground contact with about 659.7 N
tangential force and 1.874 N normal force. That is a rejected Newton iterate,
not an accepted physical trajectory. The accepted 49.25–49.5 ms recovery still
has about 35.72 N tangential force and 0.8293 N normal force. The retained floor
law includes stored bristle deflection and its damping term, with a noncontact
relaxation rate of 200/s; it does not impose a hard instantaneous Coulomb cap.
Do not silently clamp or otherwise change that law to claim a speedup. The
next structured-derivative experiment should isolate its affine dependence on
bristle states at fixed geometry/velocity, using the shared force evaluator,
then check contact transitions, full trajectories and matched runtime.

Evidence under `runs/full-robot/solver-performance/`:
`rate-partials-identities-50ms.recording.json`,
`rate-partials-identities-prefix24{,-grid}.config.json`,
`rate-partials-identities-prefix24{,-grid}-compare.json`,
`rate-partials-identities-49ms-{attempt,point}.json`,
`rate-partials-identities-49ms-check-1e-{05,06,07}.json`,
`relative-position-experiment.patch`,
`relative-position-{tests,original-regression}.log`,
`relative-position-{default-grid,50ms}-compare.json`, and
`relative-position-restored-grid-control.json`.


Final verification after restoring the prototype: all 16 runtime session tests
pass, the native archived-stage example builds, and the WASM runtime checks
successfully (the existing unused motor-field warning remains). The restored
paired shared-grid control passes and matches every previous report field,
including attempted solves and physical diagnostics, exactly apart from timing.
Remote CI/browser execution was not rerun for these diagnostic-only additions.


### Exact bristle-state columns: independently correct, no total-cost promotion

Tested exact hybrid state columns for the floor law's affine dependence on its
stored bristle deflection at fixed geometry and velocity. Reused its existing
load regularization, slip decay, translational damping and twist coefficients;
propagated the resulting required forces/moments through the existing backward
force pass, including modal boundary loads. No force cap or other physical-law
change was introduced. All 87 bristle state probes of the floating 29-link model
were replaced in the experimental hybrid; its remaining pose/velocity state
partial derivatives stayed numerical. The default derivative path was unchanged.

A new independent affine-column test covers loaded, 0.1-micrometre shallow and
separated contact with nonzero stored deflection, moving joints/base and flex.
It checks every residual output using large symmetric bristle probes, which are
valid for these affine columns. The original fallback fails that tighter test:
one base-force derivative is 198633.09679353243 rather than
198633.096811273. The prototype passes. Collecting derivative coefficients leaves
physical residuals identical. Native worker-count checks (1/2/4/8/16), including
a larger fixture exceeding the parallel threshold, pass. The initial scalar
version also passes the serial-feature derivative suite. The existing stiff-SDF
promotion gate stays explicitly unpassed.

The first implementation collected coefficients during ordinary force calls and
propagated all new columns serially. Its paired 50 ms timings show no improvement
(medians 5.9713/5.9867 seconds). A revised version collects only while preparing a
Jacobian and distributes the independent columns in deterministic indexed chunks.
Its three isolated alternating native pairs at 16 workers are:

| Pair | Before (s) | Revised prototype (s) |
| --- | ---: | ---: |
| 1 | 5.965693 | 6.032718 |
| 2 | 5.948766 | 6.043586 |
| 3 | 5.992942 | 6.047062 |

Median total time is 1.31% worse. Median assembly time decreases slightly
(2.77960→2.73039 seconds), but residual time rises (1.77558→1.85470 seconds) and
factorization time rises (1.28562→1.33787 seconds). Total Newton iterations remain
3,622 and subdivisions remain 34. Rebuilds rise 1,494→1,505, ordinary residual
calls 14,845→15,013, and stale full-step refreshes 94→101. The worst per-step
iteration count falls 35→32; that isolated metric would misleadingly suggest
improved convergence. Component FD counters exclude the hybrid's internal state
remainder, so they are not a direct count of the 87 removed probes.

The before/after independent 0–50 ms comparisons both fail 7,219/29,920 checks,
first at 20 ms, with maximum force error 22.066294 N. Both references use the same
structural-identity formulation. Comparing native exported frames directly
(before versus revised prototype) gives 11,632 passing numeric comparisons over
26 frames, maximum absolute difference 1.81517e-9 and maximum tolerance ratio
0.00011303. This frame-only check does not include every internal stored state
and is not independent validation of the physical model. It demonstrates neither
improved physical accuracy nor a full trajectory promotion gate.

The prototype was therefore not promoted, even inside the existing experimental
hybrid implementation. Preserved its patch/tests/results and restored all four
edited source/test files byte for byte, then rebuilt the native runtime tools.
The default 20 ms control recorded during the experiment also matches all prior
physical outputs, events, solver work and existing profile counts exactly.
No browser speedup is claimed; no new WASM/browser behavior was installed.

This result shifts priority from isolated bristle-state columns to the remaining
pose/velocity contact derivatives and the convergence behavior of the coupled
stiff transient. Any state elimination or other formulation experiment must
preserve the existing discrete equations and compare all original states,
contact loads and closure equations; it is not permission to soften friction
or change acceptance tolerances.

Evidence under `runs/full-robot/solver-performance/`:
`bristle-partials-experiment.patch`, `bristle-partials-summary.json`,
`bristle-partials-{before,after}-50ms-compare.json`,
`bristle-partials-paired-timing.json`, `bristle-partials-final-paired-timing.json`,
`bristle-partials-final-profile-{before,parallel}-{1,2,3}.json`,
`bristle-partials-{before,after}.frames.json`,
`bristle-partials-native-frame-comparison.json`,
`bristle-partials-{final-tests,original-column-test,serial-tests}.log`,
`bristle-partials-default-control.json`, and `bristle-partials-restored-profile.json`.


Restoration verification passes: nine retained derivative tests (one pre-existing
ignored gate) and all 16 runtime session tests. The restored hybrid profile
matches the pre-experiment final frame, event trace, solver statistics, outer
refinements and every work counter exactly. All four source/test files match
their pre-experiment byte snapshots; `git diff --check` passes.


## Compiler layout reuse: exact trajectory preservation, 8.56% lower total cost

The component finite-difference fallback was repeatedly sorting input columns,
clearing full-island result buffers and traversing global sparsity rows that a
small component could never write. `Slot::gather` also allocated a fresh index
vector for every port on every residual/probe, despite immutable compiled
wiring. Cache flat port indices (including aliases in port order), conservative
input columns, written rows and the local subset of each global sparsity column
at compilation. Populate port indices before the initial guard evaluation.
Retain global sparsity and triplet order, perturbation sizes, rate-provider
mapping, prepared-contact lifetimes, private worker scratch and all physics.

Local row caching alone gave a noise-level 0.9% median gain in the earlier
`local-fd-profile-*` pairs. The retained combined change includes the port-index
cache. On the 29-link, 12-actuator, 600-unknown default scene, run:

```
RAYON_NUM_THREADS=16 <before-or-after-sim-profile> runs/full-robot/reproduced.scene.json 50 1
```

Three sequential alternating pairs, with no overlapping build/benchmark jobs:

| Pair/order | Before (s) | After (s) |
|---|---:|---:|
| 1, before→after | 10.374565 | 9.470816 |
| 2, after→before | 10.350231 | 9.462460 |
| 3, before→after | 10.067612 | 9.464368 |
| Median | 10.350231 | 9.464368 |

The median is 8.56% lower (1.094× throughput). Median Jacobian assembly decreases
4.67586→4.36981 s and ordinary residual evaluation 3.02599→2.60370 s.
Factorization is 2.34282→2.30569 s; this small difference is not an algorithmic
factorization improvement. Build medians are 0.89641→0.84532 s. Timers are nested.
All three paired final frames, event traces, solver statistics, outer retries
and individual profile work counters match exactly. Both versions perform
3,302,572 component FD residual calls, 25,292 ordinary residual evaluations,
2,743 Jacobian builds, 6,002 Newton iterations, 291 solves, 44 subdivisions and
1,206 events. This removes overhead, not nonlinear work.

A separate before/after `sim-session SCENE 50 RECORDING FRAMES` run compares all
51 physical reporting frames exactly. `sim-validate steps SCENE 50 512` compares
all fields of all 291 attempts exactly, including original and stage states,
rates, terminal residuals, Newton correction histories, last fresh matrix
points, original closure diagnostics and contact loads. Neither capture hits
its capacity. Trial states include rejected work and are not all accepted
trajectory states. Matching the baseline does not establish physical accuracy,
long-run behavior or timestep convergence.

Validation completed:

- Native compiler: 12 tests; serial compiler: 11 tests; session: 16 tests.
- Local derivative regression reproduces old global-buffer/global-row behavior
  on owned/provided lanes and requires exact matrix-triplet equality at worker
  counts 1/2/4/8/16. Existing large-component tests cover chunked derivatives.
- Added the local regression to native and serial CI (workflow edited; no remote
  CI execution is claimed).
- Independent small contact fixture: all 2,113 checks over 21 frames pass.
- Release WASM and isolated `local-fd-gather-web` bundle pass real-Chrome worker
  parity, 21 frames, two contacts, exact replay, reset, invalid-action rejection,
  bounded attempt capture and main-thread responsiveness. Maximum native/WASM
  difference 2.70646e-11; 0.4 simulated seconds take 201.6 ms with 20 heartbeats.
  This is a small-fixture acceptance result, not a full-robot browser speedup.

Evidence: `runs/full-robot/solver-performance/local-fd-gather-{paired-timing,
equivalence,manifest}.json`, `local-fd-gather-profile-{before,after}-{1,2,3}.json`,
`local-fd-gather-{before,after}.frames.json`, matching `*-attempts.json`,
`local-fd-gather-experiment.patch` and copied test/build logs. The manifest hashes
the source before/after, scene and native binaries. Browser and independent
reference evidence are in `runs/interactive/local-fd-gather-*`.

The retained change does not promote the experimental analytic paths or close
the timestep/force-disagreement gates. The measured workload remains roughly
95× slower than realtime. The next substantive target remains fewer costly
residual evaluations and rebuilds, with the coupled contact retry divergence
still requiring an accuracy-preserving solution.


## Geometry reuse across partially changed poses: 5.34% lower total cost

The prior prepared cache reused SDF geometry only when every link pose matched.
It also recomputed world sample positions and floor heights when velocities
changed at a fixed pose. The first prototype caches those samples/depths; its
three paired runs had two small regressions and one improvement, so the 1.49%
median reduction was not treated as a robust standalone win. Its patch and
`floor-geometry-profile-*` timings remain available.

The retained extension also handles a partially changed pose. Cache each link's
bounding box and samples. For SDF queries, reuse both positive hits and absence
of contact only when the source and target poses are bit-identical to the
prepared state. Refresh all pairs involving any moved body, including a fixed
sample against a moving target. Current broad-phase boxes include moved targets.
Sort merged positive hits by their unique (sample index, target index), restoring
the exact original force-accumulation order. Terrain and collision definitions
are immutable under the prepared borrow. Recompute damping/friction forces from
current velocities and bristle states; retain existing full-force dependency
checks. Cache lifetime stays within a single linearization, with immutable
sharing and independently owned refresh results in derivative workers.

Benchmark the same default 29-link, 600-unknown scene, with 16 workers, 50 report
frames (100 ms) and 100 fixed-state microbenchmark repeats. Run sequential
alternating pairs with no concurrent build/benchmark work:

```
RAYON_NUM_THREADS=16 <binary> runs/full-robot/reproduced.scene.json 50 100
```

| Pair/order | Before (s) | Partial reuse (s) |
|---|---:|---:|
| 1, before→after | 9.518370 | 8.916502 |
| 2, after→before | 9.502651 | 8.998567 |
| 3, before→after | 9.506006 | 9.111390 |
| Median | 9.506006 | 8.998567 |

Total median runtime decreases 5.34% (1.056× throughput). Jacobian assembly
decreases 4.36638→3.93966 s (9.77%); ordinary residual time is essentially
unchanged, 2.60353→2.59854 s. Factorization medians are 2.32801→2.28763 s; no
factorization algorithm changed. The uncached fixed-state contact evaluation
microbenchmark is slightly slower, 75.60→76.56 microseconds. This is a reuse
benefit during derivative evaluation, not a faster contact law in isolation.
Build medians are 0.85470→0.83929 s. All three pairs preserve final frames, event
traces, solver statistics, outer retries and every work counter exactly:
3,302,572 component FD calls, 25,292 ordinary residuals, 2,743 fresh matrices,
6,002 Newton iterations, 291 solves, 44 subdivisions and 1,206 events.

For a separate full-trajectory preservation check, run `sim-session SCENE 50
RECORDING FRAMES` and `sim-validate steps SCENE 50 512`. Compare all 51 exported
frames and all fields of all 291 captured attempts against the previous
`local-fd-gather-after` artifacts. SHA-256 verification confirms the saved
pre-change binaries are exactly those that generated that earlier evidence.
Every frame and attempt field matches exactly, including stage states/rates,
residuals, original closure diagnostics, contact loads and correction histories.
No attempt capture truncates. These checks preserve the existing trajectory;
they do not certify the physical model or timestep accuracy.

Validation passes: four library tests, ten native derivative tests, nine serial
derivative tests and all 16 runtime session tests. The pre-existing ignored
stiff-contact promotion gate remains unsatisfied. The new heightfield fixture
varies pose/velocity/rates through contact boundaries; the expanded three-body
fixture exercises merging reused and fresh positive SDF hits and invalidating
cached misses. Both compare prepared and freshly evaluated residual bits, and
are included in the existing native/serial CI test commands. CI itself has not
been run remotely in this work.

The release WASM build and isolated `partial-geometry-web` bundle pass the
real-Chrome contact acceptance test: 21 native/browser frames, maximum absolute
difference 2.70646e-11, exact recording replay, two contacts and 20 main-thread
heartbeats. The small fixture simulates 0.4 s in 207.4 ms; no full-robot browser
performance claim follows. Its independent numerical reference passes all
2,113 comparisons.

Evidence: `runs/full-robot/solver-performance/partial-geometry-{paired-timing,
equivalence,manifest}.json`, `partial-geometry-profile-{before,after}-{1,2,3}.json`,
`partial-geometry.frames.json`, `partial-geometry-attempts.json`, the combined
`partial-geometry-experiment.patch`, and copied build/test logs. Initial
floor-only results use `floor-geometry-*`. Browser and independent-reference
artifacts are under `runs/interactive/partial-geometry-*`.

The default workload remains about 90× slower than realtime. No derivative,
contact law, constraint equation, tolerance or solver retry policy changed.
The next goal work still needs to reduce nonlinear work and resolve the coupled
contact retry/accuracy divergence; these cache improvements do not close those
gates.


## First-decrease backtracking: promising cost, unproven trajectory accuracy

The retained 100 ms capture contains 291 nonlinear trials: 246 successful and
45 failed. Failed trials account for 1,837/6,002 audited Newton iteration entries
(30.6%). Thirty-three hit the iteration limit and twelve fail their line search.
A successful nonlinear trial may still belong to rejected outer work. Small raw
residuals do not by themselves prove convergence: some rejected trials still
require corrections exceeding their per-unknown bounds.

Tested a library-level prototype that stops fresh-matrix backtracking at the
first partial trial satisfying `new_norm <= (1 - 1e-4 * alpha) * old_norm`.
Full-step handling, stale-matrix refresh, final correction tests and fixed raw
residual acceptance bounds remain unchanged. In one isolated 16-worker pair
with 50 report frames and 100 microbenchmark repeats:

| Metric | Prior search | First sufficient decrease |
|---|---:|---:|
| 100 ms simulation wall time (s) | 8.976347 | 6.568530 |
| Ordinary residual evaluations | 25,292 | 12,091 |
| Fresh Jacobians | 2,743 | 2,272 |
| Newton iteration entries | 6,002 | 5,291 |
| Subdivisions | 44 | 34 |
| Maximum configured iteration count | 40 | 37 |

This is a single diagnostic timing pair (26.82% less wall time), not a promoted
or statistically established speedup. The whole exported trajectory changes:
first noticeable differences at 12 ms include 0.15781 N in aggregated floor load,
1.8623e-6 m in link position and 3.7170e-5 in native joint coordinates. At 100 ms,
maximum link position difference is 0.18463 mm and maximum joint-coordinate
difference is 0.00081377 (coordinates mix angular and linear units). These
comparisons establish disagreement with the prior path, not which path is more
accurate.

To investigate, run both solver binaries through 12 ms using the same controller
schedule and nominal steps 0.5, 0.25, 0.125, 0.0625 and 0.03125 ms. The algorithms
agree at 0.125 ms (all 3,086 recursive exported numeric checks pass; maximum
floor-load component difference 6.34e-11 N), but disagree again at smaller steps.
Aggregate floor samples by link to compare physical forces without assuming
point-index correspondence across contact modes. Moments use the shared world
origin. Maximum same-step floor-force component differences are:

| Step (ms) | Maximum difference (N) |
|---|---:|
| 0.5 | 0.157809 |
| 0.25 | 0.107177 |
| 0.125 | 6.34e-11 |
| 0.0625 | 0.0106181 |
| 0.03125 | 0.488461 |

Refinement also changes contact sets. Comparing 0.0625 ms against the old search
at 0.03125 ms still yields a maximum per-link floor-load component difference of
6.68917 N. The finest run is a diagnostic comparator, not a validated physical
reference. Neither algorithm establishes convergence to an accurate trajectory
in this study. These exported-frame comparisons omit some hidden states and do
not establish event/impulse or energy accuracy. The first-decrease prototype was
therefore archived and removed; unchanged small-fixture tests cannot authorize
its promotion. No browser build of that prototype was promoted or benchmarked.

### Retained backtracking diagnostics

Added optional, serde-backward-compatible `NewtonIteration.line_search` with
finite trial fractions and scaled residual norms, plus `selected_alpha` (`null`
when the search fails or a stale matrix is refreshed). Norms use the iteration's
fixed Jacobian row scale. These values are observations and do not affect any
solver decisions. The full robot's runtime Newton configuration has a minimum
fraction of 1/4096, so its unsuccessful fresh full trials scan 13 fractions;
`NewtonConfig::default()` itself uses 1/256.

The new full capture contains 5,430 searches and 24,462 trial residual calls.
Of 1,586 fresh backtracked searches, 1,336 have a sufficiently decreasing partial
trial. Completing the original scan selects the same fraction as the first
sufficient trial in 953 cases and a different fraction in 383. This retrospective
count explains why early stopping changes trajectories; it is not a prediction
of the changed policy's future iterations.

Validation passes: eight convergence tests, all 16 runtime session tests, release
WASM, and real-browser contact/replay/audit acceptance. The new cubic-root test
requires identical residual probes, final solution and solver work with audit
on/off, then checks recorded norms against those actual probes. Removing only
the added line-search observations from the new full-robot audit reproduces
all fields of all 291 earlier attempts exactly, including original states,
contacts, closure and correction histories; the capture does not truncate.
The browser checks 21 reporting frames, two contacts, exact replay and main-thread
responsiveness (19 heartbeats); maximum native/WASM difference is 2.70646e-11.
The existing convergence CI command includes the new regression. Remote CI was
not run as part of this experiment.

Evidence under `runs/full-robot/solver-performance/`: `early-backtrack-profile-
{before,after}.json`, `early-backtrack-experiment.patch`, `early-backtrack.frames.json`,
`early-backtrack-frame-differences.json`, `early-backtrack-refine-*` scenes and
traces, `early-backtrack-refinement-comparison.json`,
`early-backtrack-floor-wrench-comparison.json`, `early-backtrack-retry-audit.json`,
`early-backtrack-manifest.json`, `backtrack-audit-attempts.json`,
`backtrack-audit-equivalence.json`, retained patch and copied test/build logs.
Browser artifacts are under `runs/interactive/backtrack-audit-*`.

The prior validated geometry/compiler optimizations remain in place. A common
time grid and a converged trajectory reference are still needed to separate
solver-policy changes from adaptive-step/contact-event effects before promoting
this promising reduction in retries and residual calls.

## Common-grid and common-initial-state backtracking audit

Continued the archived first-sufficient-decrease experiment without retaining
its solver-policy change. `sim-solve/src/lib.rs` still matches the pre-experiment
`/tmp/before-grid-solve.rs` snapshot. The geometry/contact and compiler-layout
optimizations described above remain unchanged.

Added `capture_trajectory RECORDING [CONFIG]` to the runtime examples. Configuration
accepts absolute `step_breakpoints_s` and `attempt_audit_limit`. Reports include
declared-unit measurements at the initial state and every recorded action,
actual implicit attempts, event traces and each island's coordinates, units and
algebraic mask. Partial captures retain the error. Completion is not an accuracy
gate, and diagnostic capture wall time is not a performance benchmark. The
example regression compares capture against direct replay and is included in
the browser workflow's native checks.

Requested breakpoints alone did not guarantee identical actual grids: nonlinear
failures introduced new subdivisions at different locations. Iteratively unioned
the recorded trial endpoints (rounded to 15 decimal places), replayed both
policies, and compared actual `(start, step, success)` tuples.

For the 12 ms case with nominal h=0.03125 ms, the third common grid gives 394
identical successful trials, identical event traces, and agreement in measured
frames, stage states, contact forces, generalized reactions and all original
closure diagnostics. Differential-rate checks pass 122,140/122,140. The raw
rate report still has 1,804 mismatches among 114,260 algebraic helper-rate
comparisons, exclusively 13 knee-loop multiplier channels. Those rates are not
consumed by the articulated equations; their multiplier **values** and physical
reactions pass separately. Preserve the raw failures rather than silently
rewriting them. Maximum stage contact-force component difference is 1.74e-11 N.
This is agreement on discrete equations, not established timestep convergence.

The original 100 ms workload needed nine union refinements:

| Grid | Requested boundaries | Established trials / failures | Experimental trials / failures |
|---|---:|---:|---:|
| 1 | 261 | 317 / 28 | 307 / 23 |
| 2 | 301 | 349 / 24 | 327 / 13 |
| 3 | 333 | 371 / 19 | 367 / 17 |
| 4 | 365 | 399 / 17 | 375 / 5 |
| 5 | 384 | 388 / 2 | 392 / 4 |
| 6 | 390 | 400 / 5 | 392 / 1 |
| 7 | 396 | 398 / 1 | 408 / 6 |
| 8 | 403 | 403 / 0 | 407 / 2 |
| 9 | 405 | 405 / 0 | 405 / 0 |

All captures complete without hitting their 2,048-attempt cap. Grid 9 has exactly
matching actual steps and event traces, yet 202,977/732,204 physical comparisons
fail. It includes real state and contact-force differences, not merely unused
algebraic rates. Maximum stage contact-force component difference is 6.68532 N;
maximum reporting-frame pose-position difference is 9.80 micrometres. A small
visible pose difference can therefore conceal a material load disagreement.

### Isolating the first divergent discrete solve

Trial 136 begins at 40.5 ms, uses backward Euler (`theta=1`, h=0.5 ms), and
evaluates its stage at 41 ms. This is an endpoint, not a midpoint. Initial
differential states differ by at most 1.65e-11 in their respective units before
the two terminal solutions diverge. Both original solves pass their raw residual
gates (maxima 2.40e-13 and 1.89e-13). The +X foot's horizontal force changes
from +4.65953 N to -2.02579 N. Controller-state differences follow the step;
they do not explain away the first physical-stage disagreement.

Added `sim_dynamics::attempt_check::resolve_implicit_attempt` and the runtime
example `resolve_captured_step SCENE POINT COMMON_INITIAL_POINT [NEWTON_CONFIG]`.
The library helper first requires the captured terminal residual to reproduce
bit for bit. It maps the captured stage to an endpoint starting guess and invokes
the existing production `implicit_step`, using a fresh matrix and the supplied
common initial state, with no subdivisions, events or branch retries. Failures
remain inspectable attempts. Callers must preserve external context and coordinate
identity; matching one residual is not a complete hidden-state replay guarantee.

All four combinations of the two guesses and two initial states converge using
the established policy, retaining the two distinct contact-force solutions.
Then held the initial state exactly equal to the established capture and tightened
the absolute residual tolerance from 1e-10 to 1e-12:

| Terminal guess | Newton iterations | Maximum raw residual | +X foot horizontal force |
|---|---:|---:|---:|
| Established capture | 4 | 2.40e-13 | +4.65953 N |
| Experimental capture | 10 | 3.28e-13 | -2.02579 N |

Both satisfy their tightened per-row residual limits. Thus differing initial
roundoff and subdivision are insufficient explanations: the same discrete step
admits numerically distinct solutions within these tolerances. This is not proof
of exact mathematical multiplicity, nor does it identify the continuous-time
solution. It is a concrete reason to retain the current policy and establish a
smaller-step reference before deciding how to reduce retries.

The existing captured-linearization checker also reproduced its stored residual
bit for bit. A one-direction derivative check on that point reports 12 mismatches
and 13 inconclusive comparisons among 600; this derivative check **does not pass**
and is separate from context reproduction. No analytic path is promoted.

Validation: all `sim-dynamics` tests and its doctest pass, including three new
cases for common-initial mixed DAE stages, known distinct scalar discrete roots,
and preserved failure records. The capture example test passes. The release
re-solve example builds and runs the six full-robot local solves above. No new
browser or remote CI run is claimed for this diagnostic-only addition.

Evidence under `runs/full-robot/solver-performance/`: `backtrack-grid-{fine,full}-*`,
`backtrack-grid-40_5ms-*`, `backtrack-common-root-*`. For example:

```sh
cargo run --locked --release -p sim-runtime --example resolve_captured_step -- \
  runs/full-robot/reproduced.scene.json \
  runs/full-robot/solver-performance/backtrack-grid-40_5ms-after-point.json \
  runs/full-robot/solver-performance/backtrack-grid-40_5ms-before-point.json \
  runs/full-robot/solver-performance/backtrack-common-root-tight-config.json
```

### Fixed-context interval refinement

Extended the same diagnostic with `refine_implicit_attempt` and an optional final
`SUBSTEPS` argument after `NEWTON_CONFIG`. It repeats the production implicit
step over the original interval, with fresh matrices, held external context,
and no automatic subdivision or event jumps. The first refined predictor uses
the capture's rate; later predictors use the last solved rate. Algebraic guesses
use captured values, then the last solved values. Reports retain every attempted
step and stop at the first failure. One substep preserves the earlier re-solve
behavior exactly. Tests additionally check midpoint DAE refinement, interval
continuity, failure retention and convergence from both known scalar roots.

All local robot trials complete at 2, 4, 8, 16, 32, 64 and 256 subdivisions
for both starting guesses; 128 was also checked for the established guess.
All start from exactly the same established initial state with absolute raw
tolerance 1e-12. At two substeps, maximum stage contact-force disagreement is
26.37 N. At four and finer, it falls below 2.31e-11 N across all stages. This
resolves the observed starting-guess ambiguity locally, without proving that
every possible discrete root is unique.

The endpoint forces and integrated loads still change with timestep:

| Substeps in 0.5 ms | Final +X foot Fx (N) | Final Fz (N) | Sum of endpoint Fz × h (N·s) |
|---|---:|---:|---:|
| 4 | -1.828210 | 15.152016 | 0.011017230 |
| 16 | -1.882636 | 14.999736 | 0.012118830 |
| 64 | -1.901917 | 14.964103 | 0.012407191 |
| 128 | -1.905267 | 14.958275 | 0.012455437 |
| 256 | -1.906928 | 14.955370 | 0.012479645 |

The force integral uses backward-Euler endpoint quadrature, not an exact
continuous impulse. Differences between 64→128 and 128→256 approximately halve,
consistent with a first-order refinement trend in this interval. The 256-step
result is still not declared a converged physical reference. Agreement between
the two guesses at a fixed grid and accuracy as the grid is refined are separate
checks; the latter needs an explicit physical error budget.

The direction of the fine-grid endpoint force is closer to the archived faster
search's coarse result than to the established search's coarse result here.
That local observation does not promote the faster policy: neither one-step
result captures the refined force integral, and the full 100 ms gate still fails.

Additional evidence: `backtrack-common-refine-{N}-{before,after}.json` and
`backtrack-common-refinement-summary.json`. Append `64` to the command above to
reproduce a 64-substep diagnostic. Final validation passes 32 dynamics tests and
one doctest, including all six implicit-audit tests; the capture example test and
re-solve example build also pass. Both examples are included in the native CI
command. The next performance experiment should compare local error control at
matched physical accuracy, including its added residual/Jacobian cost. This turn
makes no new performance claim or solver-policy change.

## Step-doubling error audit and its cost

Added `check_implicit_step_doubling` in `sim-dynamics::attempt_check`. It retains
the coarse solve and both half-step attempts and computes their endpoint
difference. In a smooth asymptotic regime, the estimated differential-state
error of the **fine** endpoint is `abs(fine-coarse)/(2^p-1)`, with p=1 for
backward Euler and p=2 for midpoint. Midpoint differential endpoints are
reconstructed from the increment; algebraic coordinates remain separately
reported differences, without claiming the same local-error order. Unsupported
method weights and failed paths have no valid estimate. The routine does not
accept timesteps, process events, or change external context.

The helper reuses the captured coarse result when its initial state matches
bit for bit and a successful result meets the requested absolute residual
floor. Otherwise it re-solves the coarse step. A recorded failure remains
inspectable. Captures now additionally validate the state/rate stage mapping
within floating-point roundoff; matching a residual alone could otherwise admit
an inconsistent archived state/rate pair. Tests cover exact decay solutions for
both methods, differential/algebraic treatment, changed initial states,
inconsistent records and failed nonlinear solves.

The native CLI now accepts `doubling` in place of the substep count:

```sh
cargo run --locked --release -p sim-runtime --example resolve_captured_step -- \
  runs/full-robot/reproduced.scene.json \
  runs/full-robot/solver-performance/backtrack-grid-40_5ms-before-point.json \
  runs/full-robot/solver-performance/backtrack-grid-40_5ms-before-point.json \
  runs/full-robot/solver-performance/backtrack-common-root-tight-config.json \
  doubling
```

Reports contain both paths, differential error estimates, stage contact/closure
diagnostics, production solver counters and separately labelled diagnostic solve
wall time. Context/terminal audit residual evaluations are extra, uncounted work;
the timings are not an end-to-end benchmark. Root-policy source is unchanged.

### Detection and comparison with finer local references

Both original roots are flagged at h=0.5 ms. The established-root full/half-step
comparison changes a joint speed by up to 0.206715 rad/s and the +X foot's Fx by
-27.4331 N. Starting from the archived faster-search root, with the same exact
initial state, those differences are 0.0345161 rad/s and +0.267314 N. This is
evidence that the discrete step is under-resolved; neither coarse root is an
accuracy reference.

Also checked the first and last subintervals of the earlier 4-, 16- and 64-step
refinements, always using each subinterval's own exact initial state. The existing
trajectory-comparison scale (1e-6 absolute in each declared unit plus 1e-5
relative) flags 80–82 differential coordinates for the coarse roots and 3–62
for those shorter subintervals. This is a diagnostic comparison using existing
scales, not a newly chosen local error budget. Those tolerances must not silently
become an adaptive controller's acceptance criteria.

For three shorter intervals, recomputed 32- and 64-substep references from exactly
the same initial state, with held context and a 1e-12 raw Newton tolerance:

| Interval within 40.5–41 ms | h (µs) | Largest estimated angular-speed error (rad/s) | Largest two-half-step vs 64-step discrepancy (rad/s) | 32 vs 64 reference difference (rad/s) |
|---|---:|---:|---:|---:|
| First of four | 125 | 0.018539 | 0.025131 | 0.000969 |
| Last of four | 125 | 0.004534 | 0.003853 | 0.000152 |
| First of sixteen | 31.25 | 0.003278 | 0.003607 | 0.000124 |

These are per-unit maxima, not necessarily the same coordinate in every column.
The estimator is informative here, but is neither uniformly conservative nor a
proof of converged reference accuracy. Multiple roots and contact transitions
can invalidate the smooth-order interpretation; retain raw differences and
physical contact diagnostics alongside the estimate.

### Work added by the check

When the coarse result can be reused, the check adds exactly two implicit solves.
For the seven sampled cases with reusable coarse results, that costs 16–35 Newton
iterations, 5–12 fresh Jacobians and 19–73 ordinary residual calls. The other
coarse-root case requires a re-solve because its initial state differs bitwise:
its totals are three solves, 35 iterations, eight fresh Jacobians and 50 residual
calls. These numbers describe the diagnostic's fresh-matrix strategy, not a
prediction for a future integrator with shared caches.

Thus unconditional step doubling is not yet a speed optimization. It supplies
an accuracy signal that can reject a converged but unresolved coarse solve;
reducing retries at matched accuracy will also require reducing the added
Jacobian work or using a less expensive validated estimator. No full-trajectory
promotion or new runtime speedup is claimed from this local experiment.

Validation: 34 dynamics tests and one doctest pass (eight implicit-audit tests),
the capture example test passes, both runtime examples build, and `sim-dynamics`
checks for `wasm32-unknown-unknown`. No new browser execution or remote CI run.
Evidence: `step-doubling-*-point.json`, `step-doubling-*-final.json`,
`step-doubling-summary.json`, test/build logs and manifest under
`runs/full-robot/solver-performance/`. Earlier `*-report.json` captures precede
coarse-result reuse and are retained separately; use `*-final.json` for the work
counts above. Half-step predictors use the coarse result after any required
re-solve; the earlier original-seed diagnostic is archived separately. Existing geometry reuse and native derivative parallelism remain
retained; the analytic and first-decrease paths remain experimental.

## Complete articulated evaluation reuse (retained)

The prepared residual already reused geometry, contact forces and kinematics,
but still repeated cables, constraint loads, the force backward pass and joint
torque calculations when derivative probes changed only rates outside those
calculations. Examples include position rates and bristle-state rates: these
change the residual equations directly, while the articulated forces and
accelerations remain identical.

`ContactLinearization` now stores the complete base `Evaluation`. Its private
shared evaluator borrows that result only when kinematics dependencies (including
accelerations), every stored state and all temperature inputs match bit for bit.
The checks remain conservative: even an unused state or temperature change
invalidates full-result reuse. `write_residual` receives the current `Generalized`
and current context, so direct rate terms are always recomputed. The owned
evaluation API used by experimental hybrid sampling remains available. Mutable
scratch is still private to workers; no cache is shared across timesteps.

Construction performs the base force evaluation once, which replaces the
compiler's immediate unperturbed evaluation rather than adding another force
pass. Unmatched probes fall through to the existing partial contact/geometry
reuse path. No finite-difference step, derivative entry, constraint equation,
Newton decision, timestep, or summation order is intentionally changed.

### Timing and exact equivalence

Built separate unchanged and modified release executables before timing. Ran
three sequential before/after pairs with 16 native workers, the reproduced
29-link/12-motor scene, 50 actions (100 ms) and 100 microbenchmark repetitions.
No assistant builds or tests ran concurrently with those timing pairs.

| Pair | Before wall (s) | After wall (s) | Before Jacobian assembly (s) | After assembly (s) |
|---|---:|---:|---:|---:|
| 1 | 8.986695 | 8.589296 | 4.001619 | 3.604236 |
| 2 | 8.995255 | 8.619740 | 4.007993 | 3.589759 |
| 3 | 9.051063 | 8.485105 | 4.004375 | 3.502275 |

Median wall time decreases **4.51%**, 8.99526→8.58930 s; median assembly time
decreases **10.35%**, 4.00437→3.58976 s. All work counters match exactly:
2,743 fresh Jacobians, 6,002 Newton iterations, 25,292 ordinary residual calls,
3,302,572 component FD calls, 291 solves and 44 subdivisions. The optimization
reduces work inside each eligible probe, not the number of probes or retries.

Separate full captures with attempt limit 512 are **byte-for-byte identical**:
51 frames and 291 attempts, with no cap reached. This includes every stored
measurement, contact/closure diagnostic, captured linearization, Newton decision,
event trace and coordinate contract. Shared SHA-256:
`4b48e48c6b01b8b8cd269aa0145009280b6635ef454578c14acb1e8dfadda900`.
The capture adds diagnostics and was not used as the performance benchmark.

The updated median profile is 3.58976 s assembly (41.8% of wall), 2.60412 s
ordinary residuals (30.3%), and 2.20818 s factorization (25.7%). These current
proportions supersede the original 93% assembly/2.6% factorization figures when
prioritizing further work. They do not by themselves justify replacing the
linear solver; reusable structural/factorization work should be inspected first.

### Validation and scope

Strengthened the existing prepared-residual oracle with simultaneous changes to
position, modal-displacement and bristle rates. It asserts that residuals actually
change, while prepared and uncached values remain bit-identical, and returns to
the original point to check reuse after invalidation. Existing probes cover every
input and rate, signed zeros, contact transitions, heightfields, moving SDF pairs,
flexibility and nonzero motion.

Native Jacobian suite: 10 pass; serial/no-default-features: nine pass; 16 runtime
session tests pass. The pre-existing experimental hybrid SDF promotion test
remains ignored for its documented failure. WASM release build succeeds. The
real Chromium contact/replay/audit gate compares 21 frames, observes two contacts,
and passes with maximum native/WASM difference 2.70646e-11, exact recording replay,
and 20 main-thread heartbeats. That browser fixture is the contact pendulum, not
a full-robot realtime demonstration. Remote CI was not run; existing native and
serial CI commands include the strengthened oracle.

Retain this change. It accelerates the same finite-step robot trajectory and
preserves earlier geometry/compiler optimizations. It does not establish material
calibration, converged full-robot timestep accuracy, or an analytic derivative
promotion. The coarse-step multiple-solution and retry-policy investigations
remain open.

Evidence: `evaluation-reuse-{before,after}-{1,2,3}.json`, full trajectory captures,
`evaluation-reuse-comparison.json`, capture config, retained patch, copied test
logs and manifest under `runs/full-robot/solver-performance/`; browser bundle,
native trace and report under `runs/interactive/evaluation-reuse-*`.

## Factorization breakdown and direct CSC construction (retained)

Added optional profiling buckets inside the existing factorization timer. The
instrumented 100 ms run takes 8.57834 s, with final frame, event trace, solver
statistics and every pre-existing work counter matching the earlier retained
implementation. Its factorization breakdown is:

| Nested operation | Calls | Seconds |
|---|---:|---:|
| Sort and sum Jacobian entries | 2,743 | 0.286803 |
| Construct sparse matrix from triplets | 2,743 | 0.332310 |
| Symbolic cache lookup/build | 2,743 | 0.778289 |
| Symbolic cache misses (included above) | 2,731 | 0.776119 |
| Numeric sparse LU factorization | 2,743 | 0.699686 |

The complete factorization bucket is 2.21677 s; remaining time includes scaling
and key construction. Nested times must not be added to their parent twice.
Structural caching already exists, but barely hits in this workload. These
counts alone do not distinguish eviction from globally distinct patterns, so
they do not justify enlarging or replacing the cache yet.

The immediate exact optimization removes a redundant sort/construction pass.
`SparseJacobian::summed` retains its existing duplicate-addition order, producing
unique row-major entries. `scaled_column_matrix` counts entries per column,
prefix-sums the offsets and scatters those entries into CSC. Rows in each column
are consequently sorted already. It uses Faer's checked symbolic constructor
and the same scaling arithmetic. It neither drops explicit zeros nor pads the
pattern, changes ordering, chooses a new solver, or modifies Newton policy.

A new unit test compares the result directly against Faer's original triplet
constructor: column pointers, row indices and every value bit must match.
It includes duplicate cancellation, signed zeros, tiny values, empty columns,
mixed scales and dimensions 0, 1, 7 and 300. Both solver unit tests and all eight
convergence tests pass. The existing CI solver command now includes `--lib`,
so this structural/value equivalence test is exercised there too.

### Timing and native equivalence

Three isolated sequential pairs use the same 600-unknown scene, 16 native workers
and 100 ms simulated duration. Both executables include the new profiling buckets:

| Pair | Before wall (s) | After wall (s) | Before matrix setup (s) | After setup (s) |
|---|---:|---:|---:|---:|
| 1 | 8.653721 | 8.312803 | 0.331208 | 0.034263 |
| 2 | 8.623476 | 8.210800 | 0.328537 | 0.033697 |
| 3 | 8.537023 | 8.178681 | 0.330106 | 0.034050 |

Median wall time improves **4.79%**, 8.62348→8.21080 s; matrix setup falls about
89.7%, 0.330106→0.034050 s. Every old and new work counter matches across each
pair, including symbolic misses. Final frames, events and solver stats match.
The complete 100 ms capture is byte-identical to the earlier evaluation-reuse
capture: 51 frames, 291 attempts, SHA-256
`4b48e48c6b01b8b8cd269aa0145009280b6635ef454578c14acb1e8dfadda900`.
This checks physical stage diagnostics and all captured Newton decisions as well
as the displayed state. Sixteen runtime session tests also pass.

### Browser sparse-path coverage

The standard contact pendulum stays below the sparse-solver threshold, so its
existing browser check alone would not exercise this optimization. Extended
`web/tests/runtime.mjs` with optional `--recording=PATH` and `--require-sparse`.
Recorded mode uses the supplied seed/actions and requires the canonical scene
embedded in that Rust recording to match the supplied scene. The original
pendulum mode retains its 20 commands, motion assertion and five-second step
budget. Recorded diagnostics use a separate 30-second sanity budget and do not
claim realtime. Sparse coverage requires audit data for an exercised island with
at least 256 coordinates. Numerical comparison tolerance remains 1e-7; invalid
action rejection, replay, reset and main-thread responsiveness still apply.

Rust serialization fills default options and drops unused export metadata, so
the canonical recording scene is extracted into `direct-csc-robot.scene.json`;
the raw CAD-export JSON is not compared textually as if it were canonical.

WASM release build passes. The usual 21-frame contact pendulum gate passes with
maximum native/WASM difference 2.70646e-11 and exact replay. The actual full robot
is additionally exercised for two actions (4 ms), with the sparse gate enabled:
three frames, three contacts, maximum native/WASM difference 2.34923e-13, exact
replay and 32 main-thread heartbeats. It takes about 326 ms to advance that short
browser recording, excluding roughly 1.90 s load time. This is explicitly a short
portability test, not a long-trajectory or realtime acceptance result. Remote CI
was not run, and the full robot recording is a local diagnostic rather than a
new CI fixture.

Retain direct CSC construction, the observational timing buckets and the browser
recording mode. Preserve all earlier validated reuse changes; no analytic or
faster-search policy is promoted. Next investigate symbolic-cache locality and
pattern variation before changing its capacity or structure, and continue using
the earlier trajectory/error gates for convergence-related experiments.

Evidence under `runs/full-robot/solver-performance/`: `factor-audit-profile.json`,
`direct-csc-{before,after}-{1,2,3}.json`, `direct-csc-comparison.json`, complete
capture, retained patch, logs and manifest. Browser recordings, canonical scene,
native traces, bundle and reports are under `runs/interactive/direct-csc-*`.

## Symbolic-cache locality and accumulated-pattern experiment (rejected)

The next audit distinguishes capacity misses from actual pattern changes. A
temporary trace records every matrix pattern after the retained duplicate sum
and before cache lookup. It does not alter cache policy or matrix values. The
100 ms replay still matches the retained final frame, events, solver statistics
and every work counter. Trace I/O affects wall time, so its timing is not a
performance measurement.

The binary trace has 2,743 records and no scene-build factorizations. Each record
is four little-endian u64 values `(n, nnz, cache_hash, factor_index)`, followed by
`nnz` little-endian u32 `(row, column)` pairs. Compare full dimension/pattern
bytes, not hashes alone: there are **2,709 distinct patterns**, with no hash
collision in this capture. Of those, 2,678 occur once, 28 twice, and three three
times. Replaying the existing clear-on-miss policy reproduces all 12 cache hits
and 42 clears. LRU capacity 64 gives 19 hits; capacity 256 or unlimited storage
gives 34 hits. Increasing capacity could avoid at most 22 additional analyses
for this workload. The capacity/eviction hypothesis does not justify a change.

The patterns contain 1,773–2,747 entries, averaging 2,637.7. Their union has 2,861
positions, of which 1,632 occur in every matrix. Consecutive matrices differ by
a median 63 positions. The largest variable column sets belong to base pose and
velocity coordinates, followed by joint/slide coordinates. This identifies
where variation occurs; it does not establish whether a numerical zero is a
structural zero or prove sparsity in other contact modes. The compiler's
conservative structural pattern remains much larger: 50,476 positions and 312
greedy colors for the actual implicit residual.

### Temporary accumulated pattern

An isolated native experiment remembers all previously seen positions for a
matrix dimension. After summing entries in the original order, it preserves
their values and pads remembered-but-absent positions with zero. Every newly
encountered position is added before symbolic/numeric factorization; no observed
union is used to discard future dependencies. The experiment uses a BTreeMap
merge and process-global diagnostic storage, not a proposed production cache
lifetime or model-isolation design. `SIM_SYMBOLIC_UNION=1` enables it only in the
archived experimental executable/patch; it is removed from the worktree.

| Quantity | Retained | Experiment |
|---|---:|---:|
| Newton iterations | 6,002 | 6,179 |
| Jacobian builds | 2,743 | 2,937 |
| Ordinary residual calls | 25,292 | 28,314 |
| Symbolic analyses | 2,731 | 69 |
| Implicit attempts | 291 | 296 |
| Symbolic-analysis time (s) | about 0.77 | 0.019 |
| Entry summation/setup time (s) | about 0.28 | 1.378 |
| Full 100 ms runtime (s) | 8.211 paired-run median | 9.690 single diagnostic |

The BTreeMap implementation adds substantial setup overhead, and nonlinear work
also increases. These timings are not a new paired benchmark or a performance
bound on a better padding implementation. More decisively, numerical equivalence
fails despite retaining the equations: changing the sparse structure changes
factorization behavior and the subsequent nonlinear/adaptive path.

Both complete captures contain 51 reporting frames, at identical times. Neither
attempt cap is reached. Of 58,862 measurements present in both captures,
25,282 exceed `1e-6 + 1e-5 * max(abs(before), abs(after))` in their declared units;
45 reporting frames have a mismatch. The first recorded failure is at 12 ms,
with a 25.0 microamp difference in the +X foot-slide bridge current. Maximum
reported current difference later reaches 0.215 A, angular speed 1.092 rad/s,
and position 0.261 mm. These are diagnostic equivalence tolerances, not calibrated
physical accuracy requirements.

Contact presence differs at 34 and 50 ms. Missing contact measurements are
recorded explicitly rather than silently counted as matches. Summing contact
forces by link/other identity gives a maximum component difference of 23.902 N
at 62 ms, for link 25's floor-normal force (32.505 versus 8.603 N). This is a
physical contact-force difference, not an ambiguity in redundant loop
multipliers. Runtime event traces remain identical, which does not imply
identical contact modes in this continuous contact model. Different implicit
attempt grids are not compared as if their internal stage states were aligned.

### Decision and restoration

Reject the accumulated-pattern experiment. A cheaper symbolic phase does not
earn promotion when full-trajectory behavior differs and total work rises.
Production solver source is restored byte-for-byte to the retained direct-CSC
version, release profiler/capture binaries are rebuilt, and a fresh 100 ms
profiler replay matches the retained final frame, events, solver stats and all
work counters. The previous complete-capture and native/WASM evidence still
applies to that unchanged source; no new browser claim is made for this rejected
native experiment.

Archive under `runs/full-robot/solver-performance/`: `symbolic-locality.bin`,
instrumentation patch/profile/summary/records, `symbolic-pattern-variation.json`,
`symbolic-union-experiment.patch`, profile/capture/comparison script and report,
restoration checks, build logs and manifest. The temporary native executables
are also hashed in the manifest. These ignored run artifacts supplement this
versionable audit; they are not a replacement for a versioned regression fixture.

The retained profile now places roughly 43% in Jacobian assembly, 32% in ordinary
residuals and 23% in factorization, including about 9% in symbolic lookup/build.
Return priority to fewer residual evaluations and robust convergence/accuracy
validation. Increasing cache capacity is low value, and reordering sparse
factorization remains experimental until it passes the same trajectory gates.

## Derivative scratch reuse and retained CPU sample (experiments rejected)

Tested two ways to reduce compiler-side allocation while retaining the same
component probes, perturbation sizes and indexed triplet assembly order:

1. Use Rayon `map_init` for private state/rate/output/port/wrench scratch shared
   across the fine column batches belonging to one job.
2. Keep the established column-batch allocation and reuse private scratch across
   the smaller components handled by each outer job. Initialize lazily after
   analytic hooks, and clear only the current component's written rows as before.

Each variant passes the compiler library test plus `parallel_jacobian` and
`rate_jacobian` integration tests, including worker counts 1/2/4/8/16. This checks
the existing prepared evaluation, owned/provided lane and partial-rate contracts.
It is not a complete full-robot trajectory gate. Three sequential 16-worker
pairs per variant use the same 100 ms robot replay:

| Variant/pair | Before wall (s) | After wall (s) | Before Jacobian (s) | After Jacobian (s) |
|---|---:|---:|---:|---:|
| Inner 1 | 8.327393 | 8.368982 | 3.662074 | 3.610880 |
| Inner 2 | 8.214671 | 8.224151 | 3.539051 | 3.502205 |
| Inner 3 | 8.116617 | 8.237687 | 3.486376 | 3.502400 |
| Outer 1 | 8.226860 | 8.099066 | 3.558748 | 3.481471 |
| Outer 2 | 8.156144 | 8.221429 | 3.513007 | 3.549422 |
| Outer 3 | 8.180130 | 8.187925 | 3.520264 | 3.546418 |

Neither full-runtime median improves: inner 8.214671→8.237687 s, outer
8.180130→8.187925 s. Small changes are within a range where no reliable benefit
is established; do not claim a speedup from the best individual pair. All pairs
preserve final frames, events, solver statistics and every work counter. The
probes still total 3,302,572 component FD residual evaluations and 25,292 ordinary
residual evaluations. No assistant builds/tests overlap the timing pairs.

Reject both variants. Archive their patches and executables, restore compiler
source byte-for-byte, rebuild release profiler/session/capture binaries, and
verify a fresh retained profile matches the old final frame, events, solver stats
and all counters. Since timing did not qualify either variant, no additional
whole-trajectory capture or browser promotion was attempted. No production
scratch mechanism is retained.

### Sampling points back to repeated contact work

Run `/usr/bin/sample OWN_PROFILER_PID 6 1 -file retained-cpu-sample.txt` against
the restored profiler process during its 100 ms replay. The sample begins at
process launch and includes startup, active threads and waits. It is a diagnostic
sample, not an isolated benchmark, and stacked/parallel sample counts must not be
presented as wall-time percentages. The underlying restored replay still passes
the final-state/work-counter checks.

The collapsed leaf sample includes `evaluate_reusing_contacts` (731), contact
geometry hit traversal (628), SDF sampling (465), `kinematics` (300),
`neighbour_band` (273), `read_ctx` (269), `write_residual` (266), and
`contact_forces` (238), along with prominent allocation/free routines. These
counts are hints for source investigation, not a complete CPU attribution or a
forecast of speedup.

Source inspection finds that `ContactGeometry::new_reusing` calls
`Articulated::neighbour_band(i,j)` inside the sample/candidate-pair loop. Each
call linearly scans tree joints and then loop closures to derive an export-pose
exclusion origin/radius. The values are fixed while the articulated model is
immutably borrowed. A next experiment should reuse this metadata within that
scope, preserving first-tree-joint/first-loop selection and the existing
export-pose approximation exactly. Public model fields can be changed between
evaluations, so any longer-lived cache requires explicit invalidation; do not
silently freeze mutable topology or change the physical exclusion rule.

Evidence: `fd-scratch-{before,after}-{1,2,3}.json`,
`fd-scratch-outer-{before,after}-{1,2,3}.json`, both comparison reports and patches,
test/build logs, `fd-scratch-restoration-check.json`, `fd-scratch-manifest.json`,
and full/collapsed `retained-cpu-sample*` reports in the solver-performance run
directory. These remain local diagnostic artifacts alongside this versionable
audit. The active performance/accuracy goal is still incomplete.

## Prepared neighbor-exclusion lookup (retained)

The stack-sampling lead produced a useful contact optimization. Previously,
every sample/candidate pair surviving the bounds checks called `neighbour_band`,
scanning the entire tree-joint list followed by loop closures. That function
returns fixed export-pose metadata for the immutably borrowed model.

`ContactGeometry` now holds a sparse adjacency table in an immutable `Arc`.
When creating a fresh geometry cache, enumerate tree and loop neighbor pairs;
use the original `neighbour_band` function to select each distinct pair's origin
and radius, store both directions, and sort each row for lookup. This preserves
the first tree-joint/first-loop precedence, reversed/duplicate pairs, arithmetic
and signed zeros. Non-neighbor pairs remain absent. Storage is proportional to
links plus neighboring pairs, rather than a dense link-by-link matrix.

Geometry queries derived from that prepared point share the table while still
rechecking the existing pose/contact dependencies. Sample traversal, exclusion
tests, SDF queries and force-accumulation order are unchanged. A fresh evaluation
after public model edits rebuilds its table; no process-global topology cache is
introduced. The private reuse API documents that its base cache belongs to the
same immutably borrowed articulated model. The existing export-pose exclusion
approximation is preserved, not silently replaced with another collision rule.

### Performance and full-capture equivalence

Three isolated sequential pairs use the retained full robot, 16 workers, 50
actions/100 ms and 100 fixed-state microbenchmark repetitions. No assistant
build/test workload overlaps these timing pairs.

| Pair | Before wall (s) | After wall (s) | Before Jacobian (s) | After Jacobian (s) |
|---|---:|---:|---:|---:|
| 1 | 8.325329 | 7.961190 | 3.597316 | 3.325008 |
| 2 | 8.148596 | 7.910177 | 3.508819 | 3.311046 |
| 3 | 8.175917 | 7.949949 | 3.527421 | 3.344699 |

Full-runtime median falls **2.76%**, 8.175917→7.949949 s; assembly median falls
**5.74%**, 3.527421→3.325008 s. Every profiling work counter is unchanged, including
2,743 Jacobian builds, 6,002 Newton iterations, 25,292 ordinary residual calls and
3,302,572 component FD probes. Final frames, event traces and solver stats match
in each pair. These are native timings, not forecasts for another model/browser.

The complete 100 ms capture is byte-identical to the retained direct-CSC capture:
51 frames, 291 attempts, neither truncated nor capacity-limited. SHA-256 remains
`4b48e48c6b01b8b8cd269aa0145009280b6635ef454578c14acb1e8dfadda900`.
This includes per-stage physical diagnostics, contacts, measurements and Newton
decisions. It proves equivalence over that capture, not calibration to the real
robot or a new long-run error bound.

### Regression and portability checks

The native prepared/Jacobian suite passes 10 tests, serial configuration nine;
the existing experimental hybrid SDF derivative promotion gate remains ignored
and unpromoted. Those tests exercise moving source/target bodies, terrain,
contact transitions, changed rates and returning to the original prepared point.

A new unit test checks all pair lookups against the original scan, including
reversed duplicates, tree-over-loop priority, duplicate loops, non-neighbors,
signed-zero radii, immutable table sharing, and rebuilding after tree removal and
loop-origin edits. It is added to the browser CI workflow as
`cargo test --locked -p sim-domain-robot --lib exclusion_tests`. Sixteen runtime
session tests also pass. Remote CI itself was not run.

The WASM release build passes. An isolated `neighbour-cache-web` bundle passes
the 21-frame/0.4 s contact-pendulum browser gate: two contacts, maximum native/WASM
difference 2.706457e-11, exact replay and 20 main-thread heartbeats. The actual
600-unknown robot also passes the recorded-input sparse gate for 4 ms: three
frames, three contacts, maximum native/WASM difference 2.349232e-13, exact replay
and 33 heartbeats. The short robot browser test takes about 336 ms excluding
load; it is portability coverage, not realtime or a measured browser speedup.

Retain the scoped neighbor table and its test. No analytic derivative, Newton
policy, tolerance, worker scheduling or physical equation changes. The active
goal remains open for convergence sensitivity and derivative/trajectory accuracy.

Artifacts: `neighbour-cache-{before,after}-{1,2,3}.json`, comparison/capture,
retained patch, native/serial/unit/session/build/browser logs and manifest in
`runs/full-robot/solver-performance/`; isolated WASM bundle, native recordings,
canonical robot scene and browser reports in `runs/interactive/neighbour-cache-*`.
The manifest records source/binary/artifact hashes and notes that only test code
and a documentation comment were added after the native benchmark build.

## Fresh joint-axis ownership transfer (retained); force-buffer reuse rejected

Inspection of `evaluate_reusing_contacts` found an avoidable allocation in fresh
evaluations: kinematics owns one axis vector per joint, but torque evaluation
cloned each vector into the final `JointReaction` and then dropped the original
kinematics vectors. The final result needs ownership, so fresh vectors can be
moved instead. Torque calculations borrow the axes; after all force/modal work,
the owned-kinematics branch transfers its vectors to the result. The
borrowed-kinematics branch retains the original copies needed for owned output.

The same public `Evaluation` fields are returned. No diagnostic output is
omitted, no force calculation is skipped or reordered, and no mutable scratch
is shared between workers. This reduces work without changing numerical
derivatives, solver tolerances, worker scheduling or constitutive equations.

### Isolated timing pairs

Each row is a sequential native pair using 16 workers and the same 50-action,
100 ms robot workload. No assistant builds/tests overlap these timing pairs.

| Pair | Before wall (s) | Axis transfer wall (s) | Before Jacobian (s) | After Jacobian (s) |
|---|---:|---:|---:|---:|
| 1 | 7.969623 | 7.786869 | 3.396117 | 3.272855 |
| 2 | 7.914714 | 7.714344 | 3.310785 | 3.228755 |
| 3 | 7.880116 | 7.778257 | 3.288580 | 3.250134 |

Median full runtime improves **1.72%**, 7.914714→7.778257 s. Assembly median falls
about 1.83%, 3.310785→3.250134 s. All final frames, events, solver statistics and
work counters match, including 2,743 Jacobian builds, 6,002 Newton iterations,
25,292 ordinary residual calls and 3,302,572 component FD probes.

An additional experiment reused the external-force/moment vectors as the
backward-pass force/moment buffers once the original loads were no longer
needed. It preserved the arithmetic/results but failed the performance test:

| Pair | Axis-only wall (s) | Plus force-buffer reuse (s) |
|---|---:|---:|
| 1 | 7.698314 | 7.716635 |
| 2 | 7.663042 | 7.765231 |
| 3 | 7.744318 | 7.791006 |

All three pairs regress; median 7.698314→7.765231 s. Final frames, events/stats
and work counts still match. Archive and remove this second experiment, then
rebuild the retained axis-only implementation. Do not infer performance from
allocation count alone or multiply unrelated median ratios into a new benchmark.

### Full capture and portability

The retained axis-only complete 100 ms capture is byte-identical to the previous
neighbor-cache capture: 51 reporting frames and 291 implicit attempts, without
reaching the cap. SHA-256 remains
`4b48e48c6b01b8b8cd269aa0145009280b6635ef454578c14acb1e8dfadda900`.
This includes stage physics, measurements and solver decisions. Capture timing
is not used for performance; checks/build work may run alongside that diagnostic.

Eight articulated tests, ten native Jacobian tests, nine serial Jacobian tests
and sixteen runtime session tests pass. The existing hybrid SDF promotion test
remains ignored and unpromoted. Existing CI cases exercise the affected
evaluation/axis consumers; no implementation-mirroring allocation test is added.

WASM release build passes. An isolated `axis-move-web` bundle passes the normal
21-frame contact-pendulum browser test with exact replay, two contacts and maximum
native/WASM difference 2.706457e-11. The actual 600-unknown robot passes its short
recorded sparse gate: 4 ms, three frames/contacts, exact replay, maximum
native/WASM difference 2.349232e-13 and 30 main-thread heartbeats. Its roughly
305 ms step time is diagnostic only, not a browser speedup or realtime claim.
No long full-robot browser trajectory or remote CI result is asserted.

Retain axis ownership transfer only. The force-buffer experiment is absent from
the final source. Convergence sensitivity, retry reduction and analytic
derivative accuracy/promotion remain open under the active goal.

Evidence: `axis-move-{before,after}-{1,2,3}.json`, comparison/capture, retained
patch, native/serial/articulated/session/build/browser logs and manifest;
`force-move-*` includes its isolated patch, timings and rejection. Native
recordings, browser bundle and reports are in `runs/interactive/axis-move-*`.
Source/binary/artifact hashes are recorded in `axis-move-manifest.json`.

## Guarded backtracking option and native/WASM adaptive-grid diagnosis

The shared `NewtonConfig` and runtime `BuildOptions` expose
`guarded_backtracking`, false by default, serialized in scenes and recordings.
It stops a halving search after three successive increases only when an already
observed trial decreases the norm, the old norm exceeds 100 times the absolute
convergence tolerance, and untested halvings remain. The best observed trial is
used. Raw residual bounds, correction acceptance, Jacobian refresh and equations
are unchanged. `line_search.bracketed` identifies actual shortened searches.

This is explicitly a heuristic: a nonmonotone tail can improve again. The
exhaustive reference remains the default. An initial native environment-toggle
experiment was removed when exposing the same criterion through the shared API;
held-out results below belong to that initial equivalent implementation, not a
second execution of the finalized API.

### Native cost and accepted outcomes

Three sequential 16-worker pairs of the same finalized executable, identical
100 ms input/model and false/true scene flags:

| Pair | Exhaustive seconds | Guarded seconds |
|---|---:|---:|
| 1 | 7.898457 | 7.429668 |
| 2 | 7.847815 | 7.423469 |
| 3 | 7.762945 | 7.278631 |

Median wall time falls 5.407%, 7.847815→7.423469 s. Ordinary residual evaluations
fall 18.373%, 25,292→20,645. All other counters, final frames, events and solver
stats match. No assistant builds/tests overlap those timing pairs. This remains
roughly 74 wall seconds per simulated second, not realtime.

The full 51-frame/291-attempt native captures match after normalizing only the
false/true policy field in scene/audit options, the new shortened-search flag,
and trial arrays. Every retained trial array is an exact serialized prefix of
its reference, with identical selected alpha. There are 876 shortened searches
and 4,647 omitted probes. All other data, including signed zeros, intermediate
physics, accepted/rejected solver outcomes and measurements, remain compared.
The cap is not reached. The default-off capture separately matches the prior
axis-transfer capture after removing only the newly serialized false options.

Held-out 200 ms hold and 100 ms commanded-worm cases preserve every reported
frame, measurement, accepted terminal state/rate/residual, stage physical
result, event and attempt path. Each changes one rejected attempt (373 and 64,
respectively). The longer hold exposes a counterexample at 131 ms: the guard
chooses alpha 1/16, norm 1.825639e-7; an omitted 1/4096 probe would reach
1.810900e-7. Both paths eventually reject that attempt and their accepted results
match. Therefore the stronger all-attempt exact-prefix gate does NOT pass on
this held-out case, and accepted-outcome equivalence is reported separately.
This evidence supports an experimental option, not general equivalence.

Nine convergence tests pass, including a cubic root with fewer probes and a
scaled near-floor case that must retain exhaustive probes. Seventeen session
tests verify recorded policy and exact replay. Twelve dynamics unit tests and
eight implicit-audit integration tests pass. WASM release builds. CI adds a
native/WASM contact-pendulum case requiring an observed guarded stop, rather
than merely checking that the flag parses. The exact local CI command passes:
21 frames, 0.4 simulated seconds, two contacts, two guarded stops, exact replay,
maximum native/WASM difference 2.706457e-11. Remote CI has not been run.

### The 12 ms robot browser gate fails in both policies

The actual robot's seven-frame 12 ms browser comparison fails the unchanged
absolute 1e-7 gate first at 6 ms. Guarded and exhaustive native frame arrays are
identical, and their browser frame arrays are also identical. Both browser
replays are exact. Thus this test isolates the discrepancy from the guarded
search policy. It does not retroactively extend the previous 4 ms passing gate.

Maximum original differences include 9.537210e-7 m in link position,
2.028369e-5 rad in a servo-output angle, and 0.149681 N in contact force summed
by link/contact counterpart. The largest individual reported contact force
component difference is 0.050152 N. Contact aggregation and per-point values
must not be conflated.

Captured solves explain the first differing integration schedule. Search
choices already differ on the 1→1.5 ms attempt, while terminal state differences
remain about 1e-13. Dominant scaled rows include closure residuals around
1e-14; raw-small equations are heavily amplified by their Jacobian scales.
At 4→4.5 ms, native accepts and WASM fails a search after seven iterations,
with raw maximum residual 4.296965e-6. WASM then accepts two 0.25 ms steps.
Initial tiny platform differences predate that event; their low-level source is
not identified by this audit.

A native capture forced onto the saved WASM accepted-step boundaries completes
on exactly the same 26 intervals. Every reported field then passes the original
1e-7 comparison, with maximum difference 2.223599e-11 (a contact force component).
The ordinary native run had 25 accepted intervals. This is evidence that the
reported physical divergence can be explained by different accepted timesteps
in this case. It is not a fix to adaptive platform sensitivity, a claim that all
internal states match, or a timestep-refined physical-accuracy reference.

The replayable `platform-grid-diagnosis.py` checks frame provenance, audit caps,
coordinate identity, aligned accepted schedules and the unchanged comparison
threshold. It stops comparing attempts by index once their paths separate.
Evidence: `platform-grid-diagnosis.json`, the saved browser raw frames and
`runs/interactive/platform-grid-native.{config,capture}.json`. Keep the failed
normal browser gate visible; do not substitute the imposed-grid diagnostic for
that gate. Investigate how convergence merit scaling amplifies already tiny
closure rows while retaining raw acceptance and correction requirements.

### Merit-weight cap experiment: faster, not promoted

To test the above diagnosis, a temporary native-only toggle capped line-search
row weights at `absolute_tolerance / residual_limit[row]`. It did not change the
linear system, correction/stagnation acceptance tests or raw residual bounds.
The purpose was to prevent already raw-small equations from dominating search
choices merely because their Jacobian rows were tiny. This is a changed merit
function, not a change to physical equations or an analytic derivative.

One initial 16-worker 100 ms profile takes 6.327660 s, versus 7.898457 s for the
existing exhaustive comparison run. This is exploratory, not a paired median
speedup claim. Fresh Jacobians drop 2,743→2,168, component FD calls
3,302,572→2,610,272, stale full-step refreshes 518→168, ordinary residual calls
25,292→21,865 and Newton iterations 6,002→5,792. Attempts fall 291→286;
subdivision stats change 44→43. Both finish, but their trajectories differ.

The full capture with the original empty-breakpoint config completes 51 frames
without reaching the 512-attempt cap. Differences reach 24.165441 N in a foot
contact-force signal, 0.231321 A in supply current, 0.004314 rad in a servo angle,
1.065506 rad/s in servo speed, and 0.257245 mm in a link-position component.
These are not accepted fidelity changes. Positional attempt comparisons become
unaligned after the first differing path; the report explicitly lists that
limitation. The first capture accidentally used the earlier 12 ms alignment
breakpoints; it is retained under `tolerance-merit-extra-grid-trajectory.json`
and is NOT used for these numbers. The corrected capture uses
`tolerance-merit.config.json`, copied from the original reference capture.

Archive the patch and restore the solver byte-for-byte to the pre-experiment
version. No `SIM_TOLERANCE_MERIT` hook remains in production source. A common-grid
and timestep-refined reference would be required to distinguish different
finite-step solutions from convergence error; that comparison has not yet been
performed for this candidate. Do not describe the reference as continuous-time
ground truth or excuse force disagreement as multiplier nonuniqueness.

Evidence: `tolerance-merit-{profile,trajectory,outcomes}.json`, config, initial
extra-grid capture, build/restoration logs and isolated experiment patch. The
retained shared guarded-backtracking option and all earlier geometry/contact/
worker optimizations are unchanged. Full adaptive native/WASM robustness and
analytic derivative promotion remain open.

## Small-angle rotation accuracy and full-trajectory refinement

Investigated closure-evaluation precision after the native/WASM grid diagnosis.
The shared `math::rot_axis` computes Rodrigues' quadratic coefficient as
`1-cos(angle)`. At 1e-9 rad, the cosine rounds to one; for axis (0.6,0.8,0),
the representable R_xy term becomes zero instead of 2.4e-19. This can affect
finite differences of small rotation components even though the absolute matrix
error is tiny. It is a demonstrated local accuracy issue, not an established
sole cause of the full robot's retries.

The experiment uses `sin(angle)^2/(1+cos(angle))` when cosine is positive and the
original subtraction otherwise. It avoids cancellation near zero without an
extra trigonometric call or changing the mathematical rotation. It leaves
`rot_vec`'s existing very-small-angle approximation, constraints, solver,
geometry, materials and controller unchanged.

Three focused tests compare the small quadratic term against a Taylor reference,
its centered numerical derivative against the analytic value across perturbation
sizes, and arbitrary-axis rotations/inverses against quaternion evaluation.
All pass, along with eight articulated, four constraint-audit, ten Jacobian and
three transmission tests. The pre-existing ignored hybrid gate remains ignored.
Local rotation accuracy does not establish whole-residual derivative acceptance.

### Initial cost does not justify performance promotion

The initial native 16-worker 100 ms profile takes 8.132763 s, compared with
7.898457 s in the existing exhaustive reference profile. This is a screening
comparison, not a new paired timing study. Fresh Jacobians change 2,743→2,755,
ordinary residual calls 25,292→26,625, derivative probes 3,302,572→3,317,020 and
Newton iterations 6,002→5,762. Subdivision stats fall 44→43, but this does not
translate into less assembly work. No performance improvement is established.

### Neither path supplies a converged full-trajectory reference yet

Replayed identical CAD/model, controller and 50 recorded actions for 100 ms at
nominal steps 0.5, 0.125 and 0.0625 ms. Controller scheduling is unchanged.
The refined captures retain all 51 reporting snapshots and declared-unit
measurements, but omit per-attempt audit data to keep memory bounded. All runs
complete; reported solver counters still show subdivisions at the finer steps.

Maximum differences across reporting samples:

| Comparison | Contact/force signal (N) | Position (mm) | Servo angle (rad) |
|---|---:|---:|---:|
| Original vs stable rotation, 0.5 ms | 12.24048 | 0.16565 | 0.002861 |
| Original vs stable rotation, 0.125 ms | 5.93877 | 0.05134 | 0.000708 |
| Original vs stable rotation, 0.0625 ms | 25.74267 | 0.08154 | 0.002057 |
| Original 0.5 vs original 0.0625 ms | 116.99011 | 1.93498 | 0.025074 |
| Original 0.125 vs original 0.0625 ms | 149.80715 | 2.75995 | 0.031622 |
| Stable 0.5 vs stable 0.0625 ms | 110.49556 | 1.89906 | 0.025333 |
| Stable 0.125 vs stable 0.0625 ms | 171.06534 | 2.77371 | 0.030998 |

These are maxima by declared unit across measured signals, not one common
contact or joint for every comparison. The script records the exact channel,
time and values, plus largest per-signal sample RMS and contact-presence changes.
Missing aggregate contact forces contribute zero; missing penetration values
are NOT fabricated. The original/stable subdivision stats are 44/43 at 0.5 ms,
27/18 at 0.125 ms and 61/46 at 0.0625 ms. Equal nominal step sizes do not imply
matching accepted grids.

This does not establish convergence at the tested resolutions. Peak sampled
forces near impacts are not sufficient alone to judge accuracy; the reporting
samples can miss contact events and do not provide integrated impulses. Neither
finest run should be called continuous-time ground truth. Large changes in
positions and joint angles also require explanation before using these runs as
promotion references. Do not weaken the native/WASM gate or use the local
rotation tests to bypass full-trajectory validation.

The rotation experiment is archived and removed from production. The original
math source is restored byte-for-byte, and its release runtime binaries rebuilt.
The three experimental tests are saved with the candidate patch rather than
left as failing tests against the restored reference. Evidence lives in
`stable-rodrigues-{experiment.patch,rotations.rs,profile.json}`, build/test logs,
`stable-rodrigues-refine-{4,8}-{before,after}.json`, the coarse candidate capture,
and `stable-rodrigues-{compare.py,refinement-comparison.json}`.

### Next validation requirement: committed-step contact impulses

`implicit_attempt_report` already warns that a successful nonlinear solve may
belong to a discarded outer candidate or event-location probe. Therefore summing
force×step across all `solve_succeeded` records is not generally a valid impulse
integral. The specific prior 12 ms WASM/aligned-native diagnosis has now also
been checked to have successful intervals forming a contiguous partition of its
complete duration, but that observation must not become a general assumption.

Expose actual local simulation commit status before adding contact-impulse
comparisons, preserving unknown status for legacy captures. Include rejected
outer candidates, event-search probes and subdivided steps in the tests. Then
compare per-foot impulses, contact timing and pose/velocity convergence along
with pointwise forces. This is required evidence for promoting convergence
changes; the existing geometry/contact reuse and worker optimizations remain
unchanged and their prior exact-trajectory evidence remains applicable.

## Committed-step contact impulses: retained diagnostic infrastructure

`ImplicitAttempt.committed` now distinguishes local committed substeps from
successful-but-discarded nonlinear trials. New records carry true/false;
legacy records deserialize to unknown (`None`). Fresh diagnostic resolves remain
uncommitted. No numerical solver decision or equation changes.

The local simulation marks successful leaves only after the enclosing candidate
commits. Full candidates abandoned for an event, event-location probes, failed
solves and successful prefixes of a failed recursive advance remain false.
Restoring a snapshot revokes commits after the restored time while retaining the
attempt history. This also handles the physical runtime's snapshot-based slice
retries. The field describes local simulation commitment, not a guarantee about
arbitrary subsequent external transactions or physical correctness.

The full native 100 ms capture matches the previous capture exactly after
normalizing only the added status and updated explanatory notes. Frames,
measurements, solver choices, raw residuals, intermediate physics, counters and
signed zeros remain compared. Of 291 captured attempts, 240 are committed,
six succeed but are discarded, and 45 fail. The cap is not reached. Prior
successful-attempt comparisons remain conservative comparisons of extra work;
success alone must not be used as an accepted-impulse selector.

### Shared impulse diagnostics and coverage requirements

`sim_runtime::contact_audit::committed_contact_impulses` evaluates the unchanged
contact law at recorded implicit stage states and sums force times committed
substep duration, grouped by directed link/contact counterpart. Backward Euler
uses endpoint force and implicit midpoint its midpoint force. This is the
integration method's quadrature, not an exact continuous-time impulse.

The diagnostic rejects disabled/capped audits, unknown legacy status, failed
records marked committed, incomplete windows, overlaps and partial substeps.
The selected physical island's committed stages must partition the requested
window. The recording example `capture_contact_impulses` resets audit storage
only between completed reporting windows, bounding memory independently of the
recording length. It records per-window and total linear impulses, peak stage
force and sampled active-step duration. Continuous contact-event timing,
torsional impulses and work/energy integrals are not claimed. Body-body force
entries describe force on the named link without duplicating the equal/opposite
reaction; floor totals include only `other=null`.

The same function is exposed by WASM `contact_impulse_report(start,end)` and the
worker message of that name. The browser harness can compare its first window
against the native recording report with `--impulses=PATH`; this avoids claiming
complete coverage after the ordinary 512-attempt browser audit cap fills.
Comparisons now ignore JSON object-key order while preserving exact key sets
and the original numerical tolerance. A native JSON-value wrapper sorted keys
where the WASM struct serializer preserved declaration order; that formatting
mismatch was corrected without loosening physical comparisons.

### Full-robot timestep results using actual committed stages

Replayed the original evaluator at nominal 0.5, 0.125 and 0.0625 ms over the same
100 ms/50-action recording. Every reporting frame matches its pre-diagnostic
counterpart exactly. All 50 impulse windows have complete coverage; total
committed substeps are 240, 827 and 1,681 respectively. No reference video,
CAD property, contact geometry or controller setting changes.

| Nominal step | +X foot vertical impulse (N·s) | -X foot vertical impulse (N·s) | Total floor vertical impulse (N·s) |
|---|---:|---:|---:|
| 0.5 ms | 2.014004 | 1.885797 | 3.899801 |
| 0.125 ms | 1.984232 | 1.911361 | 3.895594 |
| 0.0625 ms | 2.018250 | 1.896495 | 3.914745 |

The two finer levels differ by 0.489% in total vertical floor impulse, about
1.685% on +X and 0.784% on -X relative to the finest comparison level. This is
more stable than the sampled instantaneous force peaks. It does not establish
convergence: total sideways floor impulse changes from +0.026020 to -0.017794
N·s in x, individual lateral impulses vary, and the previously observed position/
angle differences remain. The comparison script also records maximum cumulative
pair-impulse differences throughout the reporting windows, so agreement at the
final time cannot hide cancellation earlier in the run.

### Verification and remaining scope

Twelve implicit-audit tests cover normal commitment, event probes, bounded caps,
subdivision leaves, discarded successful prefixes, rollback, legacy metadata and
diagnostic resolves. Two event-order, four scheduled-event and two breakpoint
tests pass. Three runtime unit tests pass, including analytic midpoint/endpoint
force quadrature and invalid coverage. The 17 existing session tests and the new
contact additivity/unknown-status/cap test pass (the latter was rerun after
correcting the fixture assertion to allow signed body-body impulse). The capture
example regression passes. `serde_json` is added only as a dynamics test
dependency for legacy-format coverage.

WASM release build passes. The real-browser pendulum gate passes all 21 frames,
exact replay, the first-window native/WASM impulse comparison and two observed
guarded stops; maximum numerical difference remains 2.706457e-11. The existing
4 ms full-robot sparse gate also passes with exact replay and maximum difference
2.349232e-13. These are diagnostic portability results, not a full 100 ms browser
acceptance or a performance speedup. The earlier normal 12 ms adaptive browser
gate remains unresolved. CI now checks new commit metadata and includes the
native/browser impulse comparison; remote CI has not been run.

Evidence: `commit-audit-trajectory.json`, exact-capture comparison and source
patch, `commit-impulses-{coarse,refine-4,refine-8}.json`, their comparison script
and report, native/WASM/test logs and manifest. Native/browser bundle and reports
are isolated under `runs/interactive/commit-audit-*`. Existing performance changes
and experimental derivative flags are preserved. Further convergence/promotion
work should use these impulses alongside pose/velocity, closure, contact timing
and eventual work/energy checks rather than trusting peak-force agreement alone.

## Revisit capped search using committed impulse evidence: still unpromoted

Re-executed the native `SIM_TOLERANCE_MERIT=1` prototype with the new bounded
committed-impulse recorder at nominal 0.5, 0.125 and 0.0625 ms. CAD/model, contact,
controller and actions are unchanged. All 50 reporting windows complete at each
level, and the coarse candidate's reported frames reproduce its previous full
capture exactly. Committed-stage counts are 243, 818 and 1,673. These runs are
impulse diagnostics, not new timing benchmarks.

The cap changes only the line-search merit weights; the linear system, raw
residual acceptance and correction requirements remain unchanged. Against the
finest unchanged comparison level:

| Coarse method | Pose sample RMS (mm) | Maximum pose displacement difference (mm) | Final net floor-x impulse error (N·s) | Final floor-z impulse error (N·s) |
|---|---:|---:|---:|---:|
| Existing exhaustive search | 0.494270 | 1.935078 | 0.013594 | 0.014944 |
| Capped merit prototype | 0.458311 | 1.925567 | 0.030965 | 0.018496 |

Pose displacement is the Euclidean difference per link, and sample RMS includes
all links and reporting times. It is not a mass-weighted or continuous-time
error norm. Final +X foot impulse-vector error increases 0.006147→0.019294 N·s;
-X increases 0.020965→0.037623 N·s. Maximum cumulative +X error increases
0.153847→0.184220 N·s, while -X improves 0.261593→0.226837 N·s. Both final and
cumulative differences are retained so endpoint cancellation cannot hide an
earlier disagreement. The result is mixed, not a uniform fidelity improvement.

At equal nominal 0.125 ms, the candidate/reference maximum pose difference is
only 0.003165 mm and the floor-z impulse difference is 2.075144e-5 N·s. This small
case does not generalize: at 0.0625 ms the maximum pose difference is 0.108262 mm.
Candidate refinement from 0.125 to 0.0625 ms still changes maximum pose by
2.846785 mm and net floor-x impulse by 0.044218 N·s. The finest unchanged run
has not been established as continuous-time ground truth; these comparisons
also do not validate electrical/thermal channels, torsional contact moments,
work/energy balance or a trained controller.

### The audit regression caught a mislabeled prototype norm

Eight of nine convergence tests initially passed. The failing audit test checks
stored search norms against actual residual probes and the iteration's Jacobian
row weights. The prototype had reused `scaled_residual_norm` for its different
merit weighting, violating that documented meaning. It did not reveal a changed
raw acceptance condition, but it made those diagnostic norms misleading.

The prototype now records the original scaled norm and the optional experimental
merit separately. All nine convergence tests pass with the cap enabled after
that correction. The released diagnostic executable used for the impulse runs
predates this audit-only field fix; its force/pose outputs do not consume or
export those Newton score fields. Preserve this distinction in provenance. No
claim is made that the corrected prototype underwent a complete second timing
or browser study.

The cap and its extra audit field are archived and removed from production; the
retained solver is restored byte-for-byte and release tools rebuilt. Do not
promote the earlier 21% reduction in fresh Jacobians using pose RMS alone when
other physical comparisons worsen. The `merit-impulses-compare.py` script and
JSON report retain pair identity, final/cumulative impulses and pose/angle
metrics. The manifest records the environment-enabled experiment explicitly;
its scene's public options alone are not enough to reproduce the prototype.

### Next exact-reuse investigation

Source inspection confirms a remaining split worth measuring:
`ContactLinearization::evaluate_shared` reuses complete kinematics only when
position, velocity and acceleration inputs all match. A velocity-only or
acceleration-only derivative probe therefore runs the full forward pass,
including Rodrigues rotations and pose transforms that depend only on the
unchanged configuration. Contact geometry itself is already reused correctly.

Investigate separate prepared pose/joint-transform data while recomputing the
changed velocities and accelerations. Cache validity must compare base pose,
joint positions and modal configuration bit-for-bit, preserve signed zeros,
and remain scoped to the same immutable model. Saved offsets must come from
the original operations rather than subtraction of world points, which could
change rounding. Require prepared/uncached derivative probes and complete
trajectory/audit equality, then paired native timing and browser checks. This
is an identified next implementation opportunity, not a measured speedup yet.

## Separate pose cache: exact initial capture, no demonstrated speedup

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
and robust contact formulations before another small geometry-cache experiment.

## Output-only finite-difference probes: fewer calls, no runtime benefit

A compiler prototype reused a component's computed signals when perturbing an
output unknown that was absent from all its inputs. It retained the existing
floating-point balance subtraction, rather than substituting an exact identity
derivative. Self-feedback aliases were excluded. Across the 100 ms hold fixture,
FD calls fell 3,302,572→3,066,674; all other profile counts and deterministic
outputs were identical. The initial prototype's entire 51-frame/291-attempt
capture also exactly matched the reference.

Three initial pairs gave median 8.86291→8.85084 s, with mixed paired results.
Removing scratch allocation from output-only batches then gave 8.68034→8.92111 s,
with all three pairs slower. Neither variant was retained. The second variant
has native trajectory-summary/profile evidence, not a complete capture/browser
comparison. Production compiler code was restored, retaining only an independent
full-residual regression for rounding, feedback, and worker-count parity.
Artifacts: `runs/full-robot/solver-performance/output-reuse-*`.
