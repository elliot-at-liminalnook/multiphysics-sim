# Native/browser runtime acceptance

This is a portability gate on the existing CAD motorized-pendulum benchmark.
It exercises the real articulated plant, electrical motor, firmware and Rhai
controller. It does not establish full-robot standing or walking acceptance.

`pendulum.scene.json` captures the model from
`examples/motorized-pendulum/build_model.py`, solver settings, a named command
input with radians and explicit limits, and the Rhai source. Python is only used
to author/export CAD; both execution targets use `sim-runtime::session::Session`.

```sh
cargo test --locked -p sim-runtime --test session
cargo build --locked --release -p sim-web --target wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127 --locked
mkdir -p runs/interactive/web
wasm-bindgen target/wasm32-unknown-unknown/release/sim_web.wasm --target web --out-dir runs/interactive/web
cp web/worker.js runs/interactive/web/
cargo run --locked --release -p sim-runtime --bin sim-session -- examples/interactive/pendulum.scene.json 20 > runs/interactive/native.frame.json
npm ci --prefix web
(cd web && npx playwright install chromium)
node web/tests/runtime.mjs runs/interactive/web examples/interactive/pendulum.scene.json runs/interactive/native.frame.json runs/interactive/browser-runtime-report.json
```

The browser test checks actual movement, native/WASM agreement within 1e-7
absolute across numeric frame fields, exact browser recording replay, reset,
invalid-command rejection, and main-thread responsiveness during worker stepping.
Budgets: load under 20 s and 0.4 simulated seconds under 5 wall seconds. Those are
initial portability gates, not the final full-robot real-time performance targets.
The current worker build has no renderer yet.

Recordings capture the scene, seed and every command. Replay reconstructs both
physics and controller state by executing that history; it is not a constant-time
state restore. Environment/UI/export integration and full-robot acceptance remain
tracked in `robot-playground-progress.md` at the repository root.

## Validate Jacobians and trajectories

Build the shared Rust validation command once:

```sh
cargo build --locked --release -p sim-runtime --bin sim-session --bin sim-validate
# Check a moving operating point, five controller frames into the episode:
./target/release/sim-validate jacobian examples/interactive/pendulum.scene.json 5 \
  > runs/interactive/jacobian-check.json
# Capture the exact scene, controller, seed and commands, then replay both paths:
./target/release/sim-session examples/interactive/pendulum.scene.json 20 \
  runs/interactive/check.recording.json > runs/interactive/check.frame.json
./target/release/sim-validate compare runs/interactive/check.recording.json \
  > runs/interactive/trajectory-check.json
```

Both commands return nonzero on failure; unresolved derivative checks also fail
acceptance. CI runs both and retains their JSON reports. The Jacobian report maps
row/column indices to component/state names and declared units. It checks every
state and rate column plus four deterministic mixed directions against central
residual differences at two stencil sizes. Neither the provided derivatives nor
their sparsity pattern supplies the numerical reference. A compiled Jacobian can
contain both analytic and finite-difference components; this check does not claim
that the whole robot has analytic derivatives yet.

The Newton solver also offers experimental `options.guarded_backtracking:true`
(Rust: `NewtonConfig::guarded_backtracking`). It stops a halving search after
three consecutively worse trials when it has found a decreasing step and the
initial scaled norm exceeds 100 times the absolute tolerance. Raw convergence
checks remain unchanged. It is off by default: a later trial can improve again,
so this is a heuristic rather than a guarantee of the exhaustive search's best
step. Record the option with experiments and compare accepted trajectories,
contact forces and total runtime against the default exhaustive reference.
Attempt audits mark actual early stops with `line_search.bracketed:true`.
The browser harness's `--require-guarded-search --audit` checks that a fixture
actually exercises this branch; its reported stop count covers captured attempts.

The articulated component has an **experimental, opt-in** hybrid Jacobian.
Set scene `options.hybrid_articulated_jacobian:true` (or the shared component's
`jacobian.hybrid:1`) to evaluate it. The default remains numerical: the full
robot's longer trajectory exposed a convergence regression, so the hybrid path
has not met its promotion gate. Inertial rate columns
use the shared Newton–Euler operator applied to unit accelerations with bias
loads removed; this is an exact linear response, not a finite difference.
Constraint-reaction columns, ideal transmission equations and kinematic
identities are exact as well. Geometry-dependent state derivatives (including
loop closure and contact) remain numerical. `Behavior::jacobian_at` supplies the
actual state rates to rate-dependent linearizations and preserves legacy
`jacobian` implementations by default. Profiling calls these **supplied** slots;
it does not count a hybrid component as wholly analytic.

`cargo test --locked -p sim-domain-robot --test jacobian` independently checks
moving grounded/floating mechanisms and exact columns with large reaction
forces, modal flexibility, nonzero velocity/acceleration, IMU storage, cable
loads and active contact. The all-column test uses a modest loop stabilization
gain to resolve the numerical remainder at default tolerances; the exact-column
tests retain the production gain. This does not certify the remaining numerical
loop derivatives at the full robot's operating points.


The default derivative tolerances are absolute `1e-7`, relative `1e-5`, stencil
radius `1e-5 * scale`. An optional fourth argument is a JSON `CheckConfig` with
explicit state/rate scales in the reported coordinate units. Set `columns:false`
for a faster directional check, retaining a positive `directions` count. Full
checks also test whether numerical directional derivatives agree with numerical
column superposition. This catches intersections of switching branches that can
look smooth along individual axes.

Branch boundaries and unresolved numerical cancellation are **inconclusive**,
not successful analytic checks. For example, this motor bridge at zero command
and zero current has intersecting clamp branches. Test interior points on each
side and keep separate transition/contact tests. Reducing the stencil or using
appropriate declared-unit scales can resolve a numerical uncertainty; increasing
tolerances solely to make a failing formula pass is not a fix.

The trajectory reference differences the complete implicit residual, bypasses
component Jacobians and their sparsity pattern, and clears cached factorizations.
It replays the same scene/seed/actions and compares all stored state channels,
link poses, energy, aggregate contact forces and penetration at every controller
frame, including the initial state. Reports include worst named discrepancies,
units, timing and applied tolerances. Defaults are absolute `1e-6` in each value's
unit plus relative `1e-5`; supply a third-argument JSON `CompareConfig` to declare
`absolute_by_unit` overrides. Set `reference_substeps:2` to compare against a
numerical-reference run at half the timestep as a separate convergence check.
That stricter comparison may expose integration error even when derivatives agree.

For a local investigation, set `shared_prefix_frames:5` to replay five actions
with identical candidate settings in both sessions before switching the reference
to independent derivatives. The validator checks matching states and controller
outputs at that boundary and clears both matrix caches. At least one action must
remain. Prefix work is reported separately; accuracy counts begin at the shared
boundary. Solver statistics remain cumulative, with the first suffix frame giving
their starting values. This tests the suffix, not the complete original trajectory.
Optional `shared_step_breakpoints_s` applies the same absolute boundaries to both
runs to diagnose differences caused by adaptive retry grids. Such selected
boundaries are diagnostics, not a production timestep policy.

To inspect the actual timestep Jacobian near a troublesome solve, use
`sim-validate stage-jacobian SCENE WARMUP_FRAMES DURATION_S [CHECK_CONFIG]`.
It holds the initial action, warms up for the requested report frames, then
captures up to 128 attempted solves over a duration no greater than one report
period. Each check reassembles provided derivatives at that trial's last fresh
linearization point. It includes the integrator's state/rate weights and refuses
a check if the live residual differs from its recorded base. External inputs
must remain unchanged. This checks reassembled derivatives, not a cached LU.
In these reports, `state[i]` means a Newton increment column; adapter `rate[i]`
columns are zero auxiliary derivatives. Supply the usual `CheckConfig` to sweep
stencil radii; inconclusive results still fail the gate. No production solver
policy changes when this diagnostic is used.

These tools validate derivative implementations and numerical consistency. They
cannot establish that shared physics equations, material properties or contact
models match reality. Analytic physical benchmarks, conservation/closure tests,
timestep convergence and eventually hardware measurements remain required.


Rate-only derivatives can be supplied by `Behavior::rate_jacobian_at`. The full
`jacobian_at` hook takes precedence. Returning true from the rate hook declares
complete rate derivatives for every residual, through contribution and signal;
omitted rate entries mean zero. State partials are ignored and remain numerical,
including state-dependent inertia/capacity evaluated at the actual rates.
Provider-backed `AcrossRate` inputs are state inputs; unprovided `AcrossRate`,
`AcrossDerivative` and `StateRate` inputs contribute to the rate matrix. This is
not an API for supplying an arbitrary subset of rate columns.

The profiler counts these slots under both numerical fallback and `slots with
supplied rate partials`. Compiler tests compare state matrices and probe counts
with workers 1/2/4/8/16 and in a build without parallel features. The articulated
experiment is `options.articulated_rate_partials` (registry `jacobian.rates`),
default false. Small native/browser fixtures pass, but the full robot regresses
severely and the option is **not promoted**; see the full-robot runtime audit.

`cargo run --release -p sim-runtime --example compare_matrices -- SCENE_A
SCENE_B POINT_JSON` compares supplied state/rate matrices at identical coordinates.
The point contains `island`, `time_s`, `states`, and `rates`. Both scenes must have
identical coordinate/algebraic contracts and bit-identical residuals; the example
uses initial external inputs, so callers must verify those match the capture.
An optional `stage` object supplies `step_s`, `theta` and `expected_residual`.
It compares dense corrections for `J_state * state_weight + J_rate / step_s`,
using algebraic weight 1 and differential weight theta. It reports singular
values, linear defects and fresh residuals at correction fractions 1 through
1/4096, with common numerical scales derived from scene A. These are diagnostic
dense solves, not replays of production sparse/cached Newton steps. Conditioning
is scale-dependent, and successful residual reduction is not a derivative or
trajectory acceptance gate. The example test checks mixed algebraic/differential
weights against an exact affine system and detects a deliberately wrong rate
block and changed captured residual.


For paired solve-point evidence, add `"attempt_audit_limit":128` to the
trajectory comparison configuration. The default is zero (disabled); the
maximum is 10,000 attempts per island per side. Capture begins after any shared
prefix and includes rejected trials, stage states/rates, last fresh
linearizations, original closure diagnostics and contact loads. The optional
`attempt_audit` report contains `candidate`, `reference` and `capacity_reached`;
a full buffer does not prove all attempts were captured. Capture leaves the
accuracy gates unchanged, so `passed` describes trajectory comparisons rather
than completeness of the optional attempt log. Capturing vectors adds overhead;
use an unaudited paired run for performance claims. Warmup and physical audit
postprocessing are excluded from suffix comparison timings.


To recheck an archived last fresh matrix without repeating warmup, run
`cargo run --release -p sim-runtime --example check_captured_stage -- SCENE
POINT_JSON [CHECK_CONFIG]`. The point contains `island`, `solve` (an
`ImplicitAttempt` from either side of the paired audit), and optional `seed`
(default 0). It uses the shared actual-increment checker and refuses a context
whose base residual differs bit for bit. Preserve coordinate ordering and
external inputs from the capture; this command does not reconstruct external
history. Its exit status is 0 for a passing derivative gate, 1 for mismatches or
inconclusive checks, and 2 for invalid/mismatched context.

Committed contact impulses can be audited from any physical recording:

```sh
cargo run --locked --release -p sim-runtime --example capture_contact_impulses -- recording.json > impulses.json
```

This uses `sim_runtime::contact_audit::committed_contact_impulses` and clears
bounded solve-audit storage between reporting windows. It requires complete
committed-stage coverage and rejects capped, unknown, overlapping or incomplete
records. Values are linear impulses in world axes (N·s), integrated using the
solver's stage rule; they exclude torsional moments and work/energy integrals.
Body-body contacts report force on the named link, without duplicating the
opposite reaction. Restrict to `other: null` when summing floor impulses.

In the browser worker, enable `set_attempt_audit_limit` before a short window,
then request `{type:'contact_impulse_report', start, end}` before the cap fills.
The WASM method is `contact_impulse_report(start, end)`. The browser harness's
`--impulses=REPORT` option compares the first window against the native CLI
report, alongside its existing trajectory/replay checks. JSON object key order
is ignored; field identity and numerical tolerances remain checked.
