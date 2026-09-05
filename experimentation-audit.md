# Experimentation completion audit

Scope: `experimentation-roadmap.md` and its accepted multiphysics extension.
This audit preserves the full CAD/system/controller → run → review → revision
workflow. It does not treat a component demo or a green unit-test subset as the
completion criterion. All scoped requirements and final gates below are verified.

## Evidence sets

- **W**: `examples/experimentation/test_workspace.py`, actual captured CAD/Rhai/
  controller workers and Qt/API integration. Latest broad execution:
  `runs/final-workspace-acceptance.log`, 28 passed in 153.91 s with all CI
  performance environment thresholds enabled, using the rebuilt final runner.
  Subsequent flex-failure guard coverage: `runs/flex-failure-final.log`, two
  boundary variants passed in 16.79 s, including explicit failure and rigid opt-out.
- **C**: `cad/tests` and `clients/python/tests`. Latest broad execution:
  `runs/final-cad-python.log`, 174 passed in 40.16 s. Subsequent UI/config
  changes have additional gates recorded below.
- **R**: native release workspace CI command, including Rhai, compiler, domain,
  controller protocol and phenomena acceptance; current execution is
  `runs/flex-clamp-native-ci.log`: 117 passed, zero failed; one host-specific
  gallery benchmark excluded exactly as in CI. Phenomena acceptance: 530.39 s.
- **P**: `examples/motorized-pendulum/run.py`, declared analytical/numerical/
  reproducibility/performance limits in `expectations.json`; final execution
  `runs/final-motorized-pendulum/acceptance.json`: all 90 checks passed.
  Checker/failure regression: 16 passed (`runs/final-pendulum-regressions.log`).
- **UI**: native Cocoa reviews in `runs/flex-review-gate` and
  `runs/flex-clamp-cad`; mixed 18-component layout, inspector edits, plot/replay,
  flex scale and frame controls are exercised by `test_experiment_ui.py`.

## Accepted multiphysics extension

| Requirement | Implementation and proving checks |
| --- | --- |
| Inspector and connection view alongside CAD | `ui/system_graph.py`, docked in `ui/app.py`; C inspector edit/connect/stale-form tests and the native 18-component layout check. |
| Shared Rust descriptions across Rust/Rhai/Python/API/CAD | `sim-script::catalogue`, native registry descriptors, `component_graph.RegistryView`; C native catalogue CRUD checks all 117 descriptions complete; R authoring/discovery checks verify units and validation. |
| Attach physics to bodies; connect physical and control ports | Persisted `component_graph`, native `compose_sources`, `bind_component`, typed connection preview and compiler validation; W coupled motor/thermal and sensor graph API tests. |
| Thermal capacity, winding-to-housing connection, sensors/controllers | `build_thermal()` and W `coupled_motor_case_derivation_sensor_and_geometry_iteration`; one existing native motor is reused; case heat balance, sensor temperature and sampled control are measured. |
| Declared parameter units and actionable diagnostics | Registry-driven editors, source-aware preflight/runtime errors; C dynamic terrain and sensor-unit tests; W connection, initial-condition and failed-binding source tests. |
| Explicit configurable captured geometry derivation, including duct → fluid | `component_derivation.py`, thermal capacity and circular fluid-volume recipes; C formula/unit/orientation/ambiguity/cache checks; W fluid proxy exclusion and mechanical-conflict rejection. |
| Stable graph IDs, persistence, undo, candidates, captured runs | `component_graph.py`, document serialization and shared revision-checked operations; C graph snapshot/candidate/undo, API conflict and atomic validation tests. |
| Coupled accuracy/reproducibility/performance proof | W case heat balance within 2%, exact repeated traces/controllers, independent cooling/geometry changes; CI sets 6-s cached thermal and 100-ms acknowledgement budgets. Latest measured heat-balance relative error 0.000371, cached run 1.568 s, acknowledgement 51.0 ms. |

## Roadmap 1 — ownership and model contract

| Requirement | Implementation and proving checks |
| --- | --- |
| CAD owns geometry/materials/assembly/joints/attachments; capture unsaved committed state | `snapshots.capture` serializes the document without saving live state; C unsaved repeatability, excluded review/results state, candidate isolation. Transient tool gestures are outside document ownership. |
| Rhai owns composition/scenario/overrides/controller bindings/expectations | `sim-script`, `experiment.rs`, `experiment_config.py`; W independent CAD/system/controller edits and script-only workers. |
| Preserve CAD defaults, record override values and source; avoid duplicates/conflicts | Captured configuration evidence and existing-native bindings; C override ID/array/default evidence; R binding tests; W coupled test asserts exactly one native motor. Post-derivation flex patch changes are rejected because they require rebuilding the basis. |
| One ModelWorld/compiler/solver for native and Rhai paths | `sim-script::System::instantiate`, reusable CAD composition in `PhysicalRobot`; R `native_and_scripted_models_have_identical_traces` and native-net binding/composition tests. |
| Versioned immutable run inputs, imported modules, controller artifacts, seed/settings/units/runtime identity | `Experiments.create`, captured binary/source ZIP, `experiment_process.capture`, worker verification; C capture and four actual runtime-identity rejection cases; W captured imports/process dependency parity and fresh seeded runs. |
| Stable CAD IDs through merged links and results, source mapping, mm/SI boundary | `cad_mapping`, graph/script mappings and flex attachment IDs; C rename/replacement/reordered-attachment comparison tests; W independent homogeneous transform checks for both joints and fixed attachments. |

## Roadmap 2 — complete asynchronous workflow

| Requirement | Implementation and proving checks |
| --- | --- |
| Motorized pendulum imports CAD and declares scenario/controller/limits | `examples/experimentation/{system,controller}.rhai`, `build_model.py`, W measured pendulum, plus P independent pendulum acceptance. |
| Sampled Rhai controller uses Coupler and agrees with Python reference | `RhaiController`, captured process backend, named contract; R startup/state/channel/import tests; W exact Python/Rhai controller and plant trace parity. |
| Launch from editor/API without manual export | W actual Qt Run button and REST experiment creation; C transport status/revision tests. |
| Expensive derivation and simulation run outside live Qt/OCCT state | Captured-document Python worker process starts native runner; W heartbeat and editor-exit recovery tests. |
| Prompt run ID; queued/building/running/completed/failed/cancelled states, logs/stage timing | `Experiments`, atomic records, worker progress; W acknowledgement/heartbeat/failure/cancellation; stage timing retained with run artifacts. |
| Cancel worker and controller children, retain diagnostics/partial failure, never report failure as pass | Process-group termination and terminal record rules; W cancellation of SIGTERM-ignoring controller, failed controller/driver partial result, invalid script tests; C partial-result evidence test. |
| Keep last successful run through later build errors | Immutable history and result association; W background failure test asserts prior completed run survives; C result/live-edit review tests. |
| Independent CAD/system/controller edits produce new runs without Rust rebuild | W `independent_cad_system_and_controller_edits_change_captured_results` verifies behavior/property changes and one unchanged simulator hash. |

## Roadmap 3 — shared result review and revision

| Requirement | Implementation and proving checks |
| --- | --- |
| Baseline/candidate/status/assumptions/metrics and captured model review | `ExperimentsPanel`, `RunReview`, `CandidateReview`; W coupled UI baseline/change/review sequence; C history/candidate tests. Model limitations are displayed in review. |
| Synchronized plots/CAD replay; sample picking and part filtering; separate pose preview | `experiment_results`, separate captured Viewport; W two-joint matrix checks; C plot scrubbing/filter/capture separation; flex uses explicitly labelled boundary arrows with rigid meshes. |
| Compare units, sampling and scenario/fidelity/controller identity; mass/current/tracking objectives | `compare`, `value_at`, `evaluate_expectations`; C resampling/held commands/no extrapolation, matching definitions/units, renamed/replaced IDs; W coupled comparisons and driver channels. |
| Exact input identity and stale live association without changing history | Physical/graph hashes and captured sources; C snapshots and result rename/stale checks; W independent geometry and graph changes with immutable retained artifacts. |
| Annotations include run/signal/time/source/parts | Evidence schema and comment CRUD; C round-trip/undo validation; W coupled UI annotation retains captured result/source and selected part. |
| Atomic revision-checked batches and isolated candidate acceptance | `Candidates`, shared API/GUI operations; C one-undo batch, failed-batch rollback, stale revision, concurrent comment rejection and exact-base acceptance; W candidate runs retain identity. |

## Roadmap 4 — controller and script authoring

| Requirement | Implementation and proving checks |
| --- | --- |
| Registry component/port/unit/parameter discovery | All 117 descriptors are complete; C actual native catalogue API check; R domain/authoring/discovery tests; dynamic members validated by native registry and inspector. |
| Source diagnostics and captured reusable imported subsystems | R reusable-module/import cycle/binding tests; W imported configuration, connection, ownership/quaternion and runtime diagnostic checks. |
| Explicit control layer with supported driver commands and measured channels | `position_target` and `driver_duty` interfaces; W driver test confirms firmware bypass, current/torque/speed and partial failures; C restore preserves backend/interface. |
| Rhai editor, parameter edits, run/check actions and external-file changes | `ui/experiments.py`; C restore/import/file polling tests; W actual preflight picker and Qt run workflow. |
| Optional debounced reruns; supersede queued obsolete runs; fresh state | C `automatic_reruns_coalesce_replace_queued_runs_and_stop_when_disabled` proves coalescing, cancellation of queued predecessor, retention of running predecessor and stopping the pending timer. R/W controller and seeded repeat tests verify fresh runs; state migration across edits is explicitly outside this feature. |

## Roadmap 5 — iteration performance and fidelity

| Requirement | Implementation and proving checks |
| --- | --- |
| Reuse CAD artifacts for controller/scenario/graph edits | Content-keyed model and component recipe caches; W independent edits, coupled cooling changes and cached exact repeats. Compiled plant state is rebuilt fresh; unsafe runtime-state reuse is not claimed. |
| Recompute only dependencies for geometry/material/joint/attachment edits | `derivation_cache`, explicit stage keys; C mass/mesh/collision/joint/fastener/sensor/flex patch invalidation with unaffected-stage hits, undo and cold/cached invariant comparison. |
| Comments/camera/selection preserve physical identity | C snapshot review-edit test and evidence annotation tests. |
| Include tool/library/settings identity, verify content, recover corruption | Captured hashes/versions and cache content hashes; C captured artifact repair, runtime rejection and intermediate cache tests; W deliberate whole-model cache corruption regenerates identical traces. |
| Quick/validation profiles and visible effective settings/limitations | Shared `experiment_config` profiles and source overrides, result header/model limitations and native warnings; C profile/default/invalid-setting checks; W effective source configuration. Flex derivation failure cannot silently substitute a rigid experiment; the failure names the link and retains its physical artifact. Explicit flex-off preflight then succeeds. |
| Separate derivation/script/compile/step/record/render timings | Worker/native timing artifacts and viewport CPU timing on review close; W performance reports; native UI screenshots/review timing artifacts. GPU/display latency is explicitly excluded. |
| Enforce documented performance thresholds in CI | `.github/workflows/simulation.yml`; 100-ms run acknowledgement/heartbeat, 250-ms cancel acknowledgement, 2-s process-tree termination; 6/12/6-s cached pendulum/two-joint/thermal budgets. Latest cached observations: 2.151/4.436/1.568 s; acknowledgement 38.1/45.9/51.0 ms. The initial 3-s cached pendulum proposal is met by this observation; CI budgets remain the documented host-tolerant thresholds. |
| Contact, flex and uncertainty contracts without general accuracy claims | W falling-box/free-fall convergence, declared-clamp beam frequency/sag and modal equilibrium, 1,000-sample planar IMU statistics and exact seed repeats. `cad/EXPERIMENTS.md` gives setups/tolerances/limitations; no automatic material/geometry Monte Carlo sweep or hardware calibration is claimed. |

## Final gate evidence

| Gate | Terminal evidence |
| --- | --- |
| Release Rust workspace CI suite and doc tests | `runs/flex-clamp-native-ci.log`: 117 passed, zero failed, one gallery benchmark filtered as prescribed by CI; phenomena acceptance 530.39 s. |
| All targets, including viewer | `runs/final-all-targets.log`: `cargo check --workspace --all-targets --locked` passed in 8.97 s. |
| Final native executables | `runs/final-runner-build.log`: release `sim-cad` and `sim-experiment` build passed in 20.98 s. No source changes after this build affected Rust. |
| Standalone CAD → process controller → measured pendulum | `runs/final-motorized-pendulum/acceptance.json`: all 90 checks passed; 8.872 s total, including export. |
| Pendulum checker/controller failures | `runs/final-pendulum-regressions.log`: 16 passed in 2.48 s. |
| CAD/Python regression | `runs/final-cad-python.log`: 174 passed in 40.16 s. |
| Actual workflow with all CI iteration budgets enabled | `runs/final-workspace-acceptance.log`: 28 passed in 153.91 s on rebuilt runner; reports retained under the matching directory. |
| UI/config changes and source-aware patch rejection | `runs/final-config-ui.log`: 32 passed; `runs/flex-clamp-final-worker.log`: two actual worker variants passed, including rejected post-derivation patch overrides. |
| Native desktop final review | `runs/final-desktop-review.log`: four passed in 13.99 s; mixed-graph and flex screenshots inspected. Later warning display: `runs/warnings-final-desktop.log`, one passed in 3.04 s. |
| Flex derivation failure stays explicit | `runs/flex-failure-final.log`: two actual worker variants passed in 16.79 s; oversized overlapping root patch fails with link ID, retained physical diagnostic and successful explicit flex-off preflight. Successful clamp/replay/cache paths remain covered. |
| Python controller protocol command from CI | `runs/final-controller-protocol.log`: 11 unittest checks passed. |

The source-level requirement matrix, terminal logs, retained input/result artifacts
and native desktop screenshots prove the scoped delivery. Accuracy bounds remain
specific to documented examples. Cross-editor cancellation ownership, absence of
automatic material/geometry Monte Carlo sweeps, and rigid meshes with flex boundary
arrows are explicit behavior/limits, not unimplemented claims in this delivery.
