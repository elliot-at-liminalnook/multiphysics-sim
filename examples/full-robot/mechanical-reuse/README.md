# Guarded mechanical derivative reuse

This performance experiment connects the existing modified-Newton workspace to
the effective-servo mechanical adapter. It keeps the paced student's robot,
network, planner, force laws, and solver tolerances. The prior checkpoint is
`35b7519`; `step-margin/` describes its behavior before this extension.

The selected candidate is now `restart-sustained.config.json`: a cached attempt
gets at most 24 Newton corrections. On failure it restarts from the original
state with fresh derivatives and the original 80-correction budget, before
considering subdivision. The original reuse-only trial remains in the recipes
and reports below. It reduced p95 from 28.5 to 22.6 ms but had a 0.485 s maximum
WASM call and active speed ratio 0.99744, failing both performance targets.

With fresh restart, the native minute needs 4,353 Jacobian builds rather than
7,232, uses 25 fresh restarts, and needs no subdivisions. Maximum sampled foot
difference from the paced baseline is below 5e-13 m; maximum pair-force
difference is below 1.6e-8 N. This addresses the trajectory changes seen in the
first trial, but not the coarse model's timestep sensitivity or hardware accuracy.

`RigidEmbedding::advance_implicit_mechanics_cached` carries an explicitly owned
workspace across successful intervals. Each trial clones the numerical state;
failed trials and failed intervals cannot commit it. The workspace contains
correction matrices, not cached physical results. Existing guards refresh after
poor convergence and invalidate reuse across incompatible timestep/layout/contact
changes. Physical force and contact evaluations remain current.

The runtime uses this API only in the opt-in `mechanical_subdivision` path.
Derivative reuse still requires `implicit.reuse_step_jacobian`. At controller
samples it clears the workspace unless
`implicit.reuse_controller_sample_jacobian` explicitly permits reuse across the
unchanged equation structure. Existing detailed motor/event adapters remain
separate. Configurations with both flags false retain the paced baseline.

## First trial evidence and limitations

The 60-second unforced run requires 4,573 Jacobian builds rather than 7,232
(37% fewer). Accepted endpoint evaluations fall from 182,297 to 131,402.
However, it now has four rejected trials requiring two 10 ms segments in place
of a 20 ms step, versus no subdivisions in the baseline. Counts of accepted
endpoints omit rejected solves; the profiling buckets include rejected work.
Concurrent profile wall times are not performance acceptance.

Short, reversal, 5 ms short refinement, and 20 ms unforced minute cases pass the
unchanged independent stepping/geometry/stopping checks. The unforced minute
completes 26 supported swings with 0.524 mm final body error. The three-push
5 ms minute **fails heading**, despite completing all 26 supported swings.

A full unforced 5 ms minute also **fails heading**: 0.005314 rad against a
0.005 rad gate. This reveals timestep sensitivity in the preceding paced model,
not just disturbance sensitivity. No gate was relaxed.

Whole 50 Hz sampled trajectory comparisons preserve frame and contact identities:

| Comparison | Maximum foot-marker difference | RMS foot-marker difference | Maximum sampled pair-force difference |
| --- | --- | --- | --- |
| 20 ms baseline vs reuse | 0.048 mm | 0.012 mm | 0.595 N |
| 20 ms baseline vs 5 ms reference | 0.673 mm | 0.247 mm | 6.138 N |
| 20 ms reuse vs 5 ms reference | 0.684 mm | 0.254 mm | 6.138 N |

These are differences, not an accuracy certificate. The reuse-versus-baseline
force maximum occurs at the first altered subdivision, at 12.52 s. Smaller
steps change the finite-step trajectory, so bitwise physical equivalence is not
claimed. Contact-pair presence differs at seven reporting samples between
coarse and refined runs; no gait-phase mismatch occurs. Between-sample impulses
and hardware forces remain unvalidated. Foot markers are checked against the
CAD source hash and transformed from the shared marker recipe.

`study-status.json` retains acceptance reports, work counts, rejection reasons,
source hashes and comparisons. All these cases are development data. Physical
parameters remain provisional, actor observations ideal, and upstream planning
privileged. Requested speed remains only 1.25 mm/s.

## Reproduce

From the repository root:

```sh
cargo test --locked -p sim-domain-robot --test embedded_step
cargo test --locked -p sim-runtime --test embedded_session --test environment --test step_reference
cargo build --locked --release -p sim-runtime --example run_environment
mkdir -p runs/full-robot/learning/mechanical-reuse
target/release/examples/run_environment examples/full-robot/student-distillation/scene.json examples/full-robot/mechanical-reuse/short.config.json examples/full-robot/walking-objective/task.json examples/full-robot/neural-teacher/train.actions.json --profile runs/full-robot/learning/mechanical-reuse/short.profile.json > runs/full-robot/learning/mechanical-reuse/short.native.json
node examples/full-robot/check_online_steps.mjs runs/full-robot/learning/mechanical-reuse/short.native.json runs/full-robot/learning/mechanical-reuse/short-acceptance
```

Use `neural-teacher/heldout.actions.json` for `reverse`, and
`browser-residual-policy/sustained.actions.json` for minute cases. Both action
paths are under `examples/full-robot/`. Keep failed reports. The standalone
`compare_mechanical_reuse.mjs` verifies configuration/scene/action compatibility
before comparing entire sampled trajectories. Pass `--timestep-reference` only
when allowing a different physics step with identical controller sampling.

The browser preset `robot-reused-student` (**Efficient student**) is a separate performance trial;
previous controller presets remain available. Browser timing and agreement
are recorded in `browser-status.json` when available. A damped linear mechanical
fixture independently verifies the backward-Euler solution, numerical reuse,
timestep invalidation, and rollback after failed force evaluations. Browser
tests verify actual Rust execution, replay/reset and interaction; they do not
establish sim-to-real accuracy.

## Revised browser delivery

The selected `restart-sustained.config.json` minute maintains 1.00060 simulated
seconds per wall second during active walking on the documented Intel
i9-9980HK Mac / Chrome 152 / UHD 630 host. Active transition p95 is **22.5 ms**,
improved from 28.5 ms but still above the **20 ms** target. The maximum measured
WASM call is 141 ms, down from 485 ms in the initial reuse-only trial. Rendering
schedule p95 is 16.67 ms. This is one rendered flat-floor episode, not broad
command/terrain acceptance or display-presentation/command-visible latency.

Native/WASM comparison of the complete 24-second short case has maximum
numerical difference 2.23e-10, with exact same-host replay/reset. All browser
controller presets pass their interaction checks. The implementation passes
13 domain tests and 20 runtime/environment/reference tests; workspace/all-target
compilation and the WASM release build pass. Whole remote CI status is not
claimed. `browser-status.json` binds results to the packaged configuration and
recorded keyboard inputs.

Reproduce the browser checks after building the WASM target:

```sh
node web/build-viewer.mjs runs/interactive/mechanical-restart/viewer --environment-only
node web/tests/environment.mjs runs/interactive/mechanical-restart/viewer robot-reused-student runs/full-robot/learning/mechanical-reuse/restart-short.native.json runs/interactive/mechanical-restart/parity.json examples/full-robot/mechanical-reuse/restart-short.config.json
node web/tests/viewer.mjs runs/interactive/mechanical-restart/viewer runs/interactive/mechanical-restart/viewer-report.json
node web/tests/live_performance.mjs runs/interactive/mechanical-restart/viewer robot-reused-student runs/interactive/mechanical-restart/live-performance.json sustained-forward
node examples/full-robot/summarize_reused_delivery.mjs
node web/serve-viewer.mjs runs/interactive/mechanical-restart/viewer 4188
```

Set `WASM_BINDGEN` and `CHROME_EXECUTABLE` if necessary. Previous presets remain
available, and the detailed CAD source is unchanged.

The shareable bundle is `~/robot-efficient-student-2026-09-07.zip`, with archive
and manifest hashes in `share-status.json`. Its `OPEN.txt` explains local
serving and static HTTPS hosting without installing CAD.
