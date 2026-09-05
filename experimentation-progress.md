# Experimentation workspace implementation evidence

Goal implementation and verification are complete. `experimentation-roadmap.md` is the completion contract.
The accepted extension in the goal attachment adds the component inspector,
connection view, explicit geometry derivations and coupled thermal example; its
requirements are now included at the start of the roadmap.
The final requirement-by-requirement evidence matrix is in
`experimentation-audit.md`; every scoped requirement and final gate has terminal evidence.

## Implemented and exercised

- Final closure: **117 native Rust tests**, **174 CAD/Python tests**, **28 actual
  workflow tests with all CI iteration budgets enabled**, **90 measured standalone
  pendulum checks**, and **16 pendulum checker/failure tests** passed. All-target
  viewer check and final release executable builds passed. Latest cached loops:
  pendulum 2.151 s, two joints 4.436 s, coupled thermal 1.568 s; acknowledgements
  38.1/45.9/51.0 ms. Final logs and requirement mappings are in the audit.
- Failed flex derivation now fails the experiment with link identity and retained
  physical diagnostics; explicit flex-off runs remain available. Two actual worker
  variants passed the failure/opt-out and normal accuracy/cache/replay checks in
  16.79 s. Native warnings and fidelity limits are visible in result review.
- Declared flex clamp settings now work through joint Properties (unit expressions,
  blank restores inference), Ops/API, snapshots, cache invalidation and native
  traces. Captured patch evidence includes radius/source, selected node count and
  bounds, fallback method, and stable attachment IDs. The 8-mm root clamp passes
  the actual CAD worker's 15% analytical frequency/sag contract. Two boundary
  variants and rejection of stale post-derivation radius metadata pass in 13.88 s
  (`runs/flex-clamp-final-worker.log`). Same-seed sensor noise now has a 1,000-sample
  per-axis statistical contract; its actual API/Rhai worker test passes.
- Final-stage broad regression: **172 CAD/Python tests passed in 36.58 s** and
  **28 actual workflow tests passed in 158.19 s** (`runs/flex-clamp-cad-all.log`,
  `runs/flex-clamp-workspace.log`). Later UI/config checks: **32 passed in 13.93 s**.
  Auto-rerun disable now cancels its pending debounce timer; desktop coverage proves
  coalescing, queued replacement and preserving already-running experiments.
- Current flex/frame gates: **27 actual experiment-worker tests passed in
  149.27 s** (`runs/flex-trace-workspace.log`); **30 CAD result/desktop/physical
  tests passed in 21.19 s** (`runs/flex-review-gate.log`), including a reviewed
  native Cocoa screenshot at `runs/flex-review-gate/test_flex_overlay_scrubs_scale0/flex-review.png`.
  Focused release native core/compile/robot/Rhai tests: **51 passed**, plus doc
  tests (`runs/flex-trace-native.log`). The full Rust workspace gate still needs
  repeating on the final implementation.
- Flex runs now capture physical world-space boundary displacement, with
  synchronized plots, stable CAD part filtering, comparison and scaled replay
  arrows. The UI labels the mesh as rigid and provides Frame replay; no full-field
  deformed-surface reconstruction is claimed. CAD modes explicitly declare mass
  normalization and correct amplitude/rate units; legacy unmarked modes warn.
- Frame quaternion validation runs after connected initial values resolve,
  covering split owner/attachment assignments in both connection orders. Valid
  split quaternions compile; invalid combinations retain both port names and
  captured source diagnostics. The actual worker regression passes.
- Runtime identity audit verifies captured simulator bytes, derivation-source
  hashes, installed dependency versions and interpreter version before execution.
  Four actual worker rejection tests cover each mismatch; controller artifact
  checks and observer ownership/recovery tests pass (13 tests, 0.80 s).
  Cross-editor cancellation remains an explicit originating-editor operation;
  observers refresh worker-owned history. This satisfies the scoped cancellation
  workflow without claiming remote ownership transfer or a hermetic OS image.
- An 18-component mixed-domain graph stays usable at 1280×800: side panels use
  scrollable tabs, the inspector and graph resize independently, and the viewport
  and run controls remain visible. Card labels elide with full-name tooltips;
  focus frames the entire selected card. Source linking, automatic reruns and
  result actions live in their corresponding source/parameters/history tabs.
- Explicit node initial values now reach owned frames and state-provided lanes.
  Qualified/short aliases and fixed constraints must agree; diagnostics identify
  both assignments and their unit. Missing values preserve native initial states.
  Generated initial-parameter hints describe this contextual default.
- Bodies and chain tips declare frame ownership through the native Behavior
  contract. Connection order cannot turn an attachment into an owner; missing or
  duplicate owners fail explicitly. Equation-level initialization/ownership
  diagnostics retain captured script locations, including imported CAD bindings.
- Shared parameter discovery covers 117/117 native component types. The last eight
  schemas include explicit walker time scaling, positive pitch–plunge kinetic
  energy, quaternion validation, dynamic chain/link membership and CAD assembly
  options/ports. Locked-pitch reaction torque no longer contributes kinetic energy.
  CAD IMU acceleration/gyro states and suffix-based port families carry physical
  units across native compilation, Rhai and the Python inspector. Loop force and
  torque regularization are separate dimensionally declared parameters.
- Nine sensor/
  actuator and sixteen planar/joint/contact types now declare defaults, bounds and
  equation-derived units. Planar IMU acceleration has a physical quantity type;
  its noise/quantization values are per channel with independent seeded streams.
  Sampling-dependent settings require a positive period. Terrain patch indices,
  extents, prismatic unit axes and conflicting drag descriptions fail explicitly.
- Dynamic parameter names in the inspector resolve their declared units while
  typing and after reload; unknown names are visible before Apply.
- Result signals carry captured graph/Rhai declaration locations. Review actions
  select the live graph component or open its read-only captured source; evidence
  annotations retain that location. UI/API/Python read the same retained system
  and controller bundles, including generated graph declarations.
- Build-only system checks capture/derive/compile the same inputs and open the
  controller contract without stepping or evaluating expectations. UI/API label
  them `not_simulated`; baseline/trajectory comparison rejects checks. Native
  manifests describe imported and compiled components, actual ports, units and
  explicit parameters. Imported values remain available after binding/wiring
  failure; diagnostics include port names, units and captured source locations.
- The graph inspector's imported-component picker fills a stable CAD binding and
  body/type without copying defaults into overrides. Already-bound components
  are selected instead of duplicated. `Last check` values remain visible during
  editing, with stale-source/document notices and unchanged draft inputs.
- Explicit, captured `body_thermal_capacity` and `circular_fluid_volume` recipes,
  with inspector controls, unit conversion, formula/input/output evidence and
  per-geometry caches. Fluid proxies are excluded from mechanical mass and reject
  mechanical attachments. Separate CAD and graph hashes preserve mechanical
  cache reuse for component-only edits.
- `component(name)` Rhai references and `bind_component` imports reuse native
  identity and validate merged parameters. Graph bindings can use stable
  `cad/<body-id>/<role>` identities; duplicate/type-mismatched bindings fail.
  Graph/Rhai connections extend native nets and expand composite members while
  retaining compiler unit and signal-driver checks.
- Coupled motor example in `build_model.build_thermal`: CAD-derived case storage,
  existing winding/cooling/mount paths, added temperature sensor, Rhai controller,
  CAD replay and graph signal aliases attached to bodies. The case binding
  replaces its original capacity; no second actuator/storage is created.
  Component port aliases are recorded in CAD-backed runs and retained partial
  output. Graph zoom and focus controls keep larger graphs inspectable.
- System graph inspector in the Experiments dock: native catalogue, numeric
  parameter table with declared units/defaults/bounds, CAD body attachment,
  component selection linked to CAD selection, clickable port connections,
  highlighted wire selection/removal, panning and fit controls. Connection mode
  changes the cursor; stale forms fail revision checks and retain edited values.
- Shared registry-derived graph CRUD for UI and REST/Python, including typed-port
  compatibility, integer/range checks, dynamic/composite port discovery, shared
  node extension retaining its ID, and component deletion retaining remaining
  shared physical nodes. Native compilation remains the final assembled-system
  validator. Restoring run inputs restores the captured graph as one undoable edit.

- Persisted document component graph, stable component/connection/body identities,
  atomic Ops/REST/Python replacement, revision conflicts, candidate diffs/acceptance,
  undo and snapshot/save-load retention. Captured graph components lower into a
  generated Rhai module and run through native registry validation and physics.
  Graph-only edits correctly mark historical graph-only results stale.

- Captured CAD documents, Rhai systems/imports and sampled controllers; asynchronous
  worker lifecycle, diagnostics, cancellation, partial results and stale detection.
- Captured runner executable and deterministic ZIP of Python derivation sources,
  checked before execution. Queued runs survive concurrent source changes.
- Native Qt editors, external Rhai linking, debounced reruns, history, synchronized
  plots/captured CAD replay, part filtering, baseline comparisons and objectives.
- Revision-checked atomic CAD batches and persisted candidates, reviewable diffs,
  isolated candidate simulation, acceptance/conflicts and one-step undo.
- Run/time/signal/source/part annotations through the document and REST APIs.
- Position-target and driver-duty boundaries; driver duty bypasses servo firmware
  and exposes measured motor current, torque, speed and joint channels.
- Captured Python/native process bundles. Python uses isolated stdlib imports plus
  bundled dependencies; restored UI inputs preserve the process backend.
- Deterministic collision-grid sampling; validated body properties, body meshes,
  linked collision, attachment, joint and flex caches. Material/geometry edits
  invalidate dependent artifacts. Whole-model hits avoid importing OCCT.
- Strict settings/requests, quick-check and validation profiles, seeded Rhai/CAD
  sensors, source-mapped physical overrides retaining defaults and effective values.
  Run seeds also feed native solver noise; its acceptance test passes exact seeded
  repeats and different-seed variation.
- POSIX worker leases persist across editor crashes; observers refresh live status,
  and abandoned queued runs become interrupted failures. Another live editor's run
  cannot be cancelled from an observing editor (explicit current limitation).
- Registry-derived UI/API discovery with native parameter validation now covers
  117/117 components, including hydraulic, chemical, radiative, granular, magnetic,
  fluid, acoustic, line, control and domain bridges. Physical mass-flow,
  chemical-potential, specific-enthalpy, radiosity and magnetic
  flux channels carry their actual units. Composite motor initial values use the
  compiler's expanded member names. Rhai errors now retain entry filenames.
  Normalized acoustics have a distinct connector; dynamic taps validate membership
  and positions, and discretization/controller delay parameters require integers.
  Worker-side configure errors retain the captured module's filename and line;
  terminal worker records retain exit codes.
- Separate derivation/script/compile/step/record timings; viewport CPU rendering
  measurements retained in review artifacts. Mass and expectation comparisons
  reject mismatched objective definitions or units.

## Verification evidence

- Final worker/UI/API gate after registry, shutdown and compact-panel changes:
  **26 passed in 137.01 s**, including contact refinement, controller cancellation,
  coupled thermal comparison/replay, reproducibility and performance budgets
  (`runs/registry-117-final-workspace.log` and `runs/registry-117-final-workspace`).
- Final 117-schema native workspace gate: **116 tests passed**, with the one
  host-sensitive gallery benchmark excluded as in CI. Phenomena acceptance took
  **515.75 s**, and doctests passed (`runs/registry-117-native-ci.log`).
  `cargo check --workspace --all-targets --locked` also passed, including the viewer.
- CAD/Python gate after compact panel changes: **164 passed in 31.92 s**
  (`runs/registry-117-compact-cad.log`). The 18-component native Cocoa interaction
  test passed in **3.43 s** and its screenshot was reviewed in
  `runs/large-graph-cocoa-gate`; it checks viewport/run visibility, card framing,
  label bounds and a real click on Apply after scrolling the inspector.
- The initial 117-schema worker gate passed 25 tests and exposed a cancellation
  test marker race (an empty file read before the controller finished writing).
  The marker now uses atomic rename and the fixture always closes its manager;
  the cancellation check passed in **7.14 s** (`runs/registry-117-cancellation`).
  The full repetition now passes all 26 tests, as recorded above.
- Released runner catalogue: **117/117 complete**, captured in
  `runs/registry-117.json`. Native compile/robot/Rhai gate: **48 tests passed** in
  `runs/articulated-schema-gate.log`, including two IMUs with distinct per-axis
  biases, channel/port unit agreement, invalid handles and exact CAD port checks.
  Standalone multibody/Rhai gates also cover walker time scaling, locked-pitch
  energy, physical parameter bounds and dynamic chain member validation.
- Controller shutdown: **9 coupler tests passed**, including unopened controllers
  receiving EOF, unresponsive controllers killed/reaped after a 250-ms grace
  period, and existing lockstep/shared-library paths. The old native acceptance
  run's hidden failure was an example supplying unused stiffness/damping to
  unilateral contact. Parameters now follow the selected contact model; the
  corrected full mechanism example passes (`runs/leg-schema-shutdown.json`).
- CAD free-fall/contact worker acceptance: **1 passed in 12.80 s**, four actual
  captured runs in `runs/contact-accuracy-gate`. Free fall matches backward Euler
  within 1e-8 m and halves analytical error when the step halves. The 50-mm PLA
  box settles 1.90 micrometres into the explicit floor, with 0.414-mm peak impact
  penetration; the cached contact trace repeats exactly. Limits are documented
  in `cad/EXPERIMENTS.md` and enforced in the existing CI workspace test module.
- The first broad native run failed after its controller shutdown deadlock was
  released; captured evidence is in `runs/native-acceptance-sample.txt` and
  `runs/initialization-native-ci.log`. A subsequent broad run is recorded in
  `runs/multibody-native-ci.log`; it predates the final chain/assembly changes.
- Initial-condition/frame-ownership worker gate: **25 passed in 126.13 s** in
  `runs/experimentation-initialization-gate`, including qualified initial speeds,
  sensor-first frame connections and source-located conflicts on CAD bindings.
  The corresponding native compile/robot/sensing/Rhai gate passed **56 tests**.
- A CAD free-body probe exposed an empty-Jacobian initialization panic for purely
  algebraic islands. Initialization now reports the residual of a stage with no
  unknowns and advances to solving the algebraic values. The dynamics gate passed
  **12 tests plus one doctest**, including constraint initialization and stepping.
  The full workspace run started before this solver fix; its result cannot verify
  that final change.
- `cargo check --workspace --all-targets --locked` passed before the latest initialization changes,
  including the viewer, in **1m 09s**. Full release workspace execution still
  requires repetition after the remaining registry and initialization work.
- Earlier experiment integration gate: **24 passed in 124.05 s** in
  `runs/experimentation-registry-gate`, on the 109-schema runner. It includes
  preflight, source navigation, native CAD/controller/thermal loops, graph sensor
  composition, cancellation/recovery, reproducibility and performance budgets.
  Coupled cached run: **48.64 ms acknowledgement**, **1.580 s total**, exact trace
  and controller repeats, **0.0371%** case heat-balance error.
- Latest CAD/Python regression: **163 passed in 27.44 s** after sensor and dynamic
  parameter inspector changes. Focused graph/inspector/real sensor API gate:
  **15 passed in 7.71 s** (`runs/registry-ui-api-gate`).
- Native Cocoa dynamic terrain inspector: **1 passed in 4.41 s**, with reviewed
  `runs/terrain-inspector-native-v2` screenshot showing editable patch names and
  their metre units in the same table as standard parameters.
- Combined current native sensing/Rhai gate: **35 passed** (13 sensing,
  13 authoring, 9 discovery), including the new planar/contact declarations.
- Native sensing gate: **13 tests passed**, including circuit/force readings,
  encoder sampling, faults, servo/driver limits and new independent IMU channels.
  Rhai authoring passed **21 tests** after sensing changes; the subsequent
  mechanics discovery gate passed **9 tests**, including source-located patch,
  axis, unit and alternative-parameter errors.
- Native wheel/chain/Jacobian gate: **12 tests passed** after planar/contact
  schemas, including analytical rolling acceleration and quadratic-drag decay.
- `runs/sensor-schema-api-gate`: actual graph CRUD → Rhai composition → worker
  simulation preserves acceleration/rate units, source mapping and exact seeded
  repeats, with different traces after a seed edit. **1 passed in 3.10 s**.
- Full CAD/Python regression after source navigation: **161 passed in 26.44 s**
  (`QT_QPA_PLATFORM=offscreen`, `cad/tests` and `clients/python`).
- Preflight integration gate: **23 passed in 121.25 s** in
  `runs/experimentation-preflight-gate`, covering compile-only checks, imported
  component discovery after failed bindings, native port/unit/source diagnostics
  and the existing complete experiment workflows. This predates source navigation.
- Native Cocoa imported-component picker check: **1 passed in 11.58 s**;
  `runs/preflight-native-ui` retains the reviewed inspector screenshot with
  imported values, compiled parameters, units and a stale-input notice.
- Source ownership, Qt and real coupled worker/API checks: **11 passed in
  25.22 s** in `runs/source-navigation-gate`. Captured controller/system sources
  survive later graph and geometry edits; declarations identify the correct
  component even when names have overlapping prefixes.
- Native Cocoa result navigation: **1 passed in 11.63 s** in
  `runs/source-navigation-native`. Reviewed the actual plot/replay/action layout;
  removing a live component disables inspector navigation while its captured
  declaration remains readable, and undo restores navigation.
- Complete experiment integration gate: **20 passed in 108.62 s** in
  `runs/experimentation-multiphysics-gate`, using the 84-schema release runner.
  This includes coupled inspector/run/baseline/replay/annotation controls, actual
  graph/geometry/API iteration, fluid proxies, pendulum/two-joint motion,
  Python/Rhai parity, cancellation, captured sources, cache and performance gates.
  This run measured **52.3 ms** coupled acknowledgement and **1.586 s** cached
  total, with the same **0.0371%** independent case heat-balance error.
- The coupled desktop workflow also passed on the native Cocoa platform in
  **11.40 s**, with a rendered replay/temperature/baseline screenshot and captured
  evidence annotation in `runs/coupled-thermal-native-review`. Visual inspection
  exposed overly broad Kelvin plot padding; plots now scale to the measured
  variation and show an explicit reference offset for small changes. The final
  result/Qt chart suite passed **15 tests in 4.05 s**, and the native UI loop
  passed again in **12.59 s** with the corrected chart and readable change labels
  (`runs/coupled-thermal-native-review-v2`).
- Full `sim-domain-robot` tests passed: one SDF unit test, six articulated-body
  tests (including contact, friction, flex and loop closure), five motor tests,
  and the CAD sensor-seed test. These exercise native behavior; they do not
  replace the remaining end-to-end CAD fidelity contracts.
- Full CAD/Python regression after recipes, bindings, zoom and partial evidence:
  **158 passed in 26.70 s**. The first attempt stalled in Qt's accessibility
  cache while legacy UI fixtures retained windows; its sampled native stack is
  `/tmp/physics-cad-pytest-sample.txt`. That confirmed-stalled test process was
  stopped. Fixtures now close and delete their windows and skip unused HTTP
  servers; the complete rerun passed. Product accessibility remains enabled.
- The native motor family now has five parameter schemas (motor, bridge, battery,
  firmware and thermal probe), including dimensional gains and state-of-charge
  bounds. The combined **five-test native motor gate passed**, including stall
  torque, unloaded speed, winding thermal time constant, backlash, powered
  firmware control and descriptor checks. The built release catalogue confirms
  **84/117** complete schemas.
- New native authoring/composition gate: **20 tests passed** (13 authoring,
  7 multiphysics), including imported-net extension, unchanged composite lanes,
  rejection of incompatible physical/signal nets and multiple signal outputs,
  merged parameter validation and duplicate-binding rejection.
- Graph/recipe/Qt inspector regression: **13 passed in 8.01 s**. This includes
  read-only derived parameters, recipe/binding undo/restore and zoom/focus. The
  subsequently added partial-evidence regression passed in the full 158-test run.
- `runs/coupled-thermal-api-gate`: coupled CAD/controller/thermal/API gate passed
  in **15.43 s**. Complete traces/controller samples repeat exactly; a graph edit
  changes cooling with CAD cache reuse; a scaled and renamed motor retains stable
  bindings and changes capacity by 1.728×. Independent case energy balance error
  **0.0371%** (2% limit), cached REST acknowledgement **58.3 ms** (100 ms limit),
  cached one-second run **1.649 s** (6 s limit). Metrics and four run IDs are in
  the test's `thermal-acceptance.json`. CI now includes this contract.
- `runs/fluid-cad-gate`: **2 passed in 8.66 s**, proving that a derived fluid
  cylinder coexists with the motorized CAD mechanism, is omitted from mechanical
  links, retains body-linked pressure signals, and rejects mechanical joints.
- Native thermal inspector reviewed in a separate transient desktop window;
  screenshots in `runs/coupled-thermal-ui`. Full-volume/lumped model limitations
  and reproduction steps are documented in `examples/experimentation/README.md`.
- Previous CAD-backed graph trace/cache gate: **3 passed in 10.00 s**. Captured
  manual and derived storage follow `dT/dt = Q/C` within 1e-8 K; graph edits reuse
  mechanical derivation and produce attached temperature channels with CAD replay.

- Earlier full CAD/Python suite: **153 passed in 52.66 s**, after inspector and
  granular API integration. Final connection-ID/deletion changes: **9 focused
  graph/API/UI tests passed in 5.89 s**. These include real Qt port clicks,
  stale-form rejection, CAD body attachment and native-catalogue REST operations.
- Native desktop inspector review performed in a separate transient window.
  Fixed narrow form fields and wires hidden behind cards; screenshots retained
  in `runs/system-graph-ui`. The user's existing CAD window was not modified.

- Earlier CAD and Python client suite: **148 passed in 45.44 s**,
  including the expanded cache suite and discovery/controller UI changes.
- Rhai authoring: **9 passed** including native trace parity, imports, diagnostics,
  fresh state, captured seeds, registry units and shared parameter validation.
- Full Rust workspace tests (excluding sim-app) passed, including the phenomena
  acceptance test (982.78 s) and exhibits. That suite predates the latest registry
  and unit additions. The new release CI command also passed on the 79-schema source,
  including phenomena acceptance in **526.86 s** and doc tests (exhibits skipped as
  in CI; they passed in the earlier full run).
- Seed/schema gate: **2 passed in 21.51 s**, proving native solver seeded repeats
  and independent CAD/system/controller changes with the 37-schema runner.
- Earlier focused Rust tests: 9 authoring tests, 6 multiphysics discovery/composition
  tests and 4 magnetic/radiative physics tests passed. That release runner's
  actual catalogue output confirmed 79/117 complete descriptions. Its full release
  CI command passed. Workspace/test compilation also passes.
- Component graph foundation: **9 focused document/API/candidate/capture tests
  passed in 2.69 s**. Actual captured thermal graph worker test passed in **2.03 s**:
  10 W / 20 J/K gives 0.5 K/s despite a live 100 W edit, with 1e-8 K tolerance.
  Artifacts: `runs/component-graph-foundation-v3`. This is a thermal storage proof,
  not the required full electromechanical–thermal example.
- Result/recovery/component-graph/UI regression suite: **16 passed in 3.90 s**
  after graph capture and graph-only stale detection changes. Full CAD/Python
  148-test evidence above predates this graph foundation.
- Configuration/source gate: **19 passed in 3.35 s**, including actual workers
  rejecting unknown settings, invalid boolean settings and overrides without CAD,
  all identifying the captured imported module at line 3. Artifacts are in
  `runs/experimentation-config-source-gate`.
- `runs/experimentation-integrated-gate`: **9 passed in 101.58 s**. Covers pendulum,
  two-joint replay/fixed attachments, cancellation/heartbeat, script-only systems,
  editor crash recovery, exact Python/Rhai parity, stubborn process cancellation,
  independent CAD/system/controller edits, and driver-duty/partial failure.
- Integration host: Intel i9-9980HK, macOS 26.6. Acknowledgements 61.8 ms / 57.3 ms;
  cached pendulum 3.293 s and two-joint 6.531 s. This run overlapped the CAD suite.
  Earlier isolated pendulum capture 57.8 ms and cached time 3.003 s. CI budgets
  are 100 ms acknowledgement/heartbeat, 250 ms cancellation acknowledgement,
  2 s process-tree termination, 6 s cached pendulum and 12 s cached two-joint.
- Per-part cache test passes cold/cached, geometry/material changes and corrupt
  intermediate recovery. Cached repeats are exact; independent OCCT re-queries
  compare numeric artifacts within 1e-12 absolute/relative tolerance.
- Expanded cache suite: **4 passed in 6.47 s**, adding changed joint pivots,
  physical overrides/undo, added fasteners, and sensor-driven flex boundary changes.
  Flex rebuilds compare modal frequencies and sag within 1e-8 relative tolerance;
  arbitrary eigenvector sign is not a physical difference.
- Native desktop visual review passed in a separate transient window. Screenshots
  in `runs/experiments-ui-v2`; two-joint replay scrubbed to 0.650 s. Six viewport
  frames averaged 4.13 ms CPU (4.18 ms max), excluding GPU/display latency.
- Usage/API/limits documented in `cad/EXPERIMENTS.md`, linked from README and the
  CAD guide. CI includes the real loop plus focused capture/cache/review tests.

## Documented fidelity scope

- The inferred 4.55-mm attachment and declared 8-mm
  clamp are different boundary conditions. The latter's measured 197.60 Hz and
  9.777 µm sag compare with analytical 201.13 Hz and 9.493 µm. The acceptance
  contracts cover documented setups, not universal material/contact accuracy.
