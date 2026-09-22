# Rust multiphysics systems viewer implementation plan

Status: **checkpoint 3b implemented, tested and reviewed in the native GUI, 2026-09-20**.
The full goal remains incomplete. The GUI checkpoints below are the current
implementation order; the original numerical, identity, worker, and performance
requirements remain in force. Detailed evidence is in
[the implementation record](examples/systems-viewer/IMPLEMENTATION.md).


## Current direction: one system, three linked views

RoboCAD owns the physical definition and authoring. The shared Rust runtime owns
simulation, controller execution, observations, and experiment commands. A Rust
systems workspace presents **physical assembly, schematic, and plots** with the
same component identities, selection, run identity, and time cursor. Bevy owns
the spatial presentation; it is not a replacement solver or physical model.

Keep the existing egui schematic and analysis work functional. Start the Bevy
surface as a separate thin executable over graphics-independent `sim-inspect`
contracts, with a reusable `sim-spatial` presentation library. Checkpoint 2 links
selections through an ephemeral local session over the same sealed description.
Decide the final
single-window host after the first GUI feedback, without moving physics into UI.

### Succinct delivery checklist — visible feedback at every checkpoint

- [x] **0. Update this plan.** Retain the existing work and acceptance gates;
  make GUI feedback checkpoints explicit.
- [x] **1. Inspect a spatial assembly.** Open a motor/thermal example in Bevy;
  orbit/zoom/fit, select in 3D or the component list, inspect actual parameters,
  ports and connections, and explore exploded presentation. Label illustrative
  shapes and missing samples. Deliver a running GUI before adding simulation.
- [x] **2. Link the views.** Share selection between assembly and existing
  schematic; inspect the same component/net, preserve analysis sidecars, and
  maintain usable layout at different window sizes. Delivered for GUI feedback.
- [ ] **3. Run a coupled example.** Connect the existing worker to run/pause/
  step/reset/cancel. Use reusable library components for supply, driver, motor,
  thermal path and LED (add a validated component if absent). Show shaft motion,
  current, modeled losses, component temperature and LED state from accepted
  runtime observations. Start with the existing smaller motor fixture.
  - [x] **3a. Controls and connection graphs.** First GUI increment: run/pause/
    step/reset/cancel through the shared worker. Clicking a link offers each
    terminal's available quantities with units and sign conventions. Pin several
    time graphs while changing selection; retain bounded display history, use
    actual sample times, and clear history on reset. Assembly selections also
    drive the schematic's graph picker. Keep unavailable measurements explicit.
    Keep graph discovery/history in `sim-inspect` and the reusable `TimeGraph`
    widget in `sim-diagram`; other panels supply data and own the shared cursor.
  - [x] **3b. Animate accepted measurements.** Bind spatial motion and indicators
    to that same run, without introducing another simulation. Review before
    adding driver/LED components or field rendering. Delivered with sampled shaft
    rotation, temperature colors and timed readouts; see
    `examples/systems-viewer/evidence/live-checkpoint-3b/README.md`.
- [ ] **4. Add recording and linked replay.** Extend the live plots with one
  cursor for assembly, schematic and graphs, label accepted-stage versus endpoint
  times, preserve recording outside a killable worker, and verify headless/live
  agreement. Review this small complete workflow before scaling up.
- [ ] **5. Open CAD captures.** Export display geometry, physical bindings,
  source hashes, frames and units; open from RoboCAD without losing unsaved work.
  Carry selection/source IDs both ways and retain fidelity/override provenance.
  Review the full quadruped assembly and its actuator internals.
- [ ] **6. Add spatial fields.** Render temperature fields, heat flux, fluid
  network flows, then field slices/streamlines where the model supplies those
  data. Add one independently validated field example and review its controls.
- [ ] **7. Validate and deliver.** Enforce the original numeric/worker/UI gates,
  measure spatial rendering and browser fidelity/performance, finish automation,
  accessibility, capture metadata and durable examples. Report unpassed gates.

Each checkpoint ends with a runnable GUI, exact launch command, retained visual
and interaction evidence, known limitations, and a short feedback prompt. User
feedback can change presentation before the next checkpoint. Checked means its
own acceptance evidence exists; it does not mark the whole project complete.

### Spatial contracts and model honesty

- Add versioned presentation descriptions to `sim-inspect`: component/body IDs,
  shape/mesh references, explicit units and right-handed frames, local transforms,
  display provenance, and later observation bindings. Reject unknown identities,
  nonfinite/invalid geometry, foreign descriptions and unsupported versions.
- Keep model-specific topology and placement in example/CAD artifacts. Generic
  presentation primitives and selection belong in the shared library. Display
  shape dimensions, exploded offsets and colors never become mass, inertia,
  collision geometry, electrical parameters or thermal properties.
- A lumped component temperature produces a uniform component color. Surface
  gradients require spatial field samples and a declared mapping. A network flow
  indicator is distinct from a solved velocity field. Mesh tessellation must not
  imply physical resolution. Render unavailable/stale data explicitly.
- LED emissive appearance is a state visualization; photometric predictions
  require a declared optical model. Net lines show actual multi-terminal topology,
  with no inferred causal direction or invented pairwise physical connections.
- Physics uses simulation time and publishes timestamped immutable observations.
  Rendering may interpolate poses only with explicit timing semantics. Rendering
  cadence, camera movement and selection never step physics or mutate controllers.
- Keep geometry/export/solve work off the UI thread with progress/cancellation;
  bound display transport and reject old generations. All overrides go through
  shared validated commands and provide a promotion path back into CAD.

### Checkpoint 1 delivery — 2026-09-20

Implemented and opened on the Intel Mac. Launch:

```sh
./examples/systems-viewer/run-spatial.sh
```

Evidence: `examples/systems-viewer/evidence/spatial-checkpoint-1/` and
[usage/limits](examples/systems-viewer/spatial/README.md). The fixture binds 14
illustrative shapes to 7 actual components, 12 ports and 5 nets. Seventeen
inspection/contract tests and one Bevy interaction test pass; the graphics-free
contract checks on WASM. Native visuals were inspected. This does not validate
the Bevy browser renderer or any final numerical/performance gates.

The first interactive review reported lag. A high-parallelism test build was
stopped; the launcher now builds with two low-priority jobs and runs the GUI at
normal priority. Bevy's default desktop sleeps (5 s focused / 60 s unfocused)
were unsuitable for completing pipelined redraws: focused updates now wait at
most 1/60 s, unfocused at most 250 ms. The updated window and selections were
observed; no end-to-end input latency percentile has been measured or claimed.
Final large-scene/browser performance acceptance remains checkpoint 7.

Checkpoint 2 below now delivers linked selection. No runtime simulation was
added to either graphical host by the checkpoint 1 increment.

### Checkpoint 2 delivery — 2026-09-20

Implemented bidirectional component, port and net selection between native Bevy
assembly and egui schematic. Launch both with:

```sh
./examples/systems-viewer/run-linked.sh
```

The default is two compact windows. Selection uses the same sealed description
and source IDs; collapsed groups and bundles retain every member. Receiving a
selection preserves diagram positions, pins, camera, collapse/focus state and
analysis metadata. An exact physical net remains the inspection/annotation
scope even when it is drawn inside a larger bundle. Parts without geometry
remain inspectable; incomplete spatial connection guides are not fabricated.

Native sessions use bounded background mailboxes, atomic local records and peer
leases. The UI performs no transport file I/O. Closing either window leaves the
other usable; reconnecting restores the last selection. There is no daemon or
simulation stepping. The assembly's unfocused update interval is now 40 ms to
serve the link; that is an update setting, not a measured input-latency bound.

Acceptance: **46 tests passed** (33 inspection/diagram, 11 schematic, 2 Bevy),
including no echo, reconnect/duplicate-role/foreign-state rejection, full bundle
identity, exact-net inspection, and preservation of unsaved analysis and source
models. The graphics-free inspection contract also checks on WASM. Both native
binaries build. GUI clicks verified assembly → schematic and schematic →
assembly for components, ports and multi-terminal nets. Reattachment restored
the current selection. Physical fixture hashes remained unchanged.

Evidence and limitations:
[checkpoint 2 record](examples/systems-viewer/evidence/spatial-checkpoint-2/README.md).
The seven-component native fixture is the GUI acceptance surface; larger
collapsed bundles are covered by automated tests. The final assembly screenshot
shows the compact text/connection-label polish. Final desktop interaction stopped
at the user's request while they watched shows; background checks and records
were completed. Layout feedback remains open before choosing a single-window host.

Build follow-up: `sim-runtime/build.rs` currently fingerprints these new UI
crates, so presentation edits invalidate the runtime build. Audit the library
identity boundary and Cargo rerun rules before the next iteration; preserve
reproducible simulation-source identity while excluding presentation-only code.
Builds remain limited to two low-priority jobs.

Checkpoints 3a and 3b now provide accepted runtime measurements, graphs and
physical-view motion. Native review is retained in the checkpoint 3b record.
CAD mesh import and fields remain later.

### Checkpoint 3a — live controls and reusable connection graphs

Implemented and reviewed alongside checkpoint 3b: `./examples/systems-viewer/run-live.sh`.
The existing worker runs the retained motor/thermal model, initially paused.
Select a physical link in either view and choose its terminal measurements in the
schematic inspector. Graph choices remain pinned while navigating. Run, pause,
single-step, reset and cancellation use the shared runtime; no viewer integrates
physics. The authored description and exact model/identity capture are verified
before the worker compiles the model.

Reusable boundaries:
- `sim-inspect::plot`: source-based observable discovery and bounded frame history,
  preserving availability, units, sample times and run/reset identity.
- `sim-diagram::plot::TimeGraph`: public egui widget accepting points/gaps, units,
  time bounds, height, color and a caller-owned shared cursor. It can render saved
  or synthetic data without a simulation session. Its response exposes geometry
  and the nearest actual sample for caller-owned linked views.
- `sim-viewer::live_ui`: worker controls and an adapter from source selection and
  received frames to the reusable components.

This increment retains 2,000 coalesced display frames and up to eight graphs.
Curves are decimated live observations, not lossless recordings. Missing samples
break curves; reset clears history; accepted stages are plotted at their actual
sample times. Per-terminal flow conventions remain explicit.

The user requested background-only work while watching shows. Native visual
review is therefore pending, and the checklist remains open until that review.
Checkpoint 3b is now delivered: spatial motion and indicators use this same run.
Next after feedback: checkpoint 4, recording and linked replay.
Full lossless recording/replay, shared spatial playback cursor, CAD geometry,
resolved thermal/fluid fields and final accuracy/performance gates remain open.

### Checkpoint 1 scope and acceptance

Use the existing retained `motor_composite` baseline as the source for a sealed
shared description. Its seven components are motor, supply, electrical ground,
rotor inertia, thermal storage, cooling conductance and ambient boundary. It has
no driver board or LED yet; do not imply otherwise. Example geometry is an
illustrative assembly study, explicitly not a CAD export or calibrated design.

Acceptance: the native window renders on this Intel Mac; mesh and list picking
resolve to the same actual component IDs; the inspector shows registry-derived
parameters/units and real port/net membership; exploded/connection visibility
controls alter presentation only. Validate foreign/missing identities and invalid
geometry in the shared contract. Retain screenshot and interaction evidence.
Live values, motion, plots, CAD meshes, field rendering and cross-window linked
selection are explicitly later checkpoints.

## Historical handoff checkpoint — 2026-09-13

The current native UI is a usable **static systems/analysis viewer**. It does
not yet expose simulation controls, live values, plots or replay. Progress is
roughly halfway by scope, not a measured completion percentage.

Implemented and verified in preceding increments:

- Open quantity/connector definition traits and registry-based compilation;
  an independent diffusion crate exercises extension without core/viewer cases.
- Shared descriptions with source identities, typed ports, units and physical
  multi-terminal nets. The current quadruped capture has 195 components,
  597 ports, 242 nets and 56 declared groups; its 12 motors are represented.
- Diagram navigation, exact connection tracing, domain emphasis, orthogonal
  routing, cancellable layout, focus/back restoration and pinned positions.
- Engineer annotations, display names, disjoint custom groups, multi-selection,
  atomic sidecars and undo/redo. These are presentation changes, not CAD edits.
- Numeric runtime subscriptions, accepted-stage flow/algebraic observations and
  endpoint states with honest timing. All 1,408 quadruped observables bind;
  initial stage/flow values remain unavailable until an accepted solve exists.
- General session commands, fixed intervals, fresh-factory reset, bounded
  in-memory recording independent of display subscriptions, and validated
  exact-frame playback. Eight focused session tests and native/WASM build
  checks passed before the worker increment.

Newest work at the stopping point:

- `crates/sim-runtime/src/system_worker.rs` and the `sim-system-worker` binary
  add native JSON-line process transport, bounded request/reply channels,
  one coalesced display frame, generation checks and a parent-owned kill handle.
- This worker currently launches **ModelWorld captures only** with the supplied
  host registry. It is not connected to `sim-viewer`; the viewer files were not
  changed during this increment. **One native process smoke test passes** for
  build, subscription, step, reset, stale-command rejection and child termination.
  See `examples/systems-viewer/evidence/system-worker-smoke.log`; this is not
  completed worker acceptance. The shared library still checks on WASM; the
  process worker itself is native-only.

### What remains

1. Connect the worker/client to the viewer: run/pause/step/reset/cancel, build
   progress, step speed/sample age and stale/error states. Preserve all existing
   workspace edits, selection and positions when runtime capabilities change the
   description ID. Review final-frame ordering on pause/reset/exit.
2. Add live inspector/diagram values, bounded plots and recording/replay controls
   with one shared time cursor and explicit stage timing/missing samples.
3. Preserve committed recordings outside a killable worker, with bounded queues
   and explicit backpressure/failure. The current in-memory recording is lost if
   its worker is killed before retrieval. Implement recorded controller ingress
   and distinguish observation playback from full input re-execution.
4. Add CAD/Rhai session adapters and CAD launch integration. The quadruped scene
   contains a controller program: use its existing controller/runtime path rather
   than dropping the controller or inferring runnable physics from a description.
   Preserve unsaved CAD edits and captured options. Extend shared CLI/REST paths.
5. Complete explicit `SignalType::Any` authoring migration, remaining quantity
   semantics (including temperature differences and entropy-production rate
   units), and optional diagnostic-provider availability.
6. Finish keyboard/accessibility and label/composite details. Run the concrete
   full-quadruped reasoning tasks below; iterate on usability based on evidence.
7. Measure and enforce all original gates: <=5% migration stepping regression;
   200/400 and 2,000/4,000 layout/navigation budgets; stalled-worker UI feedback
   and cancellation; 64 observations at 30 Hz with <10% overhead; bounded
   history/queues; native/browser numerical parity and long-run behavior.
8. Complete the requirement-by-requirement acceptance audit, durable source/build/
   input/recording metadata, delivery commands and known-limit documentation.
   Spatial animation now follows the GUI checkpoints above; it still requires accepted runtime samples.

### Original runtime resume notes

First read `examples/systems-viewer/IMPLEMENTATION.md`, `OBSERVATIONS.md` and
`SESSIONS.md`; then inspect `system_worker.rs`, `system_session.rs` and
`crates/sim-viewer/src/main.rs`. Finish the process/client tests and wire a small
existing model into live UI controls before expanding to the controller-bearing
quadruped capture. Do not mark the goal complete on the strength of static UI,
build checks or narrow smoke tests.

The worktree is uncommitted. Preserve unrelated CAD fixture work in
`cad/scripts/hx30hm_fixture.py`, `cad/scripts/hx30hm_print_batch.py`,
`examples/actuators/hx30hm/fixture-draft/` and `result.json`. Do not terminate
existing viewer windows, which may hold unsaved analysis work.

```sh
/Users/elliot/.cargo/bin/cargo run --locked -p sim-viewer -- \
  --description examples/systems-viewer/full-robot.description.json
/Users/elliot/.cargo/bin/cargo test --locked -p sim-runtime --test system_session
/Users/elliot/.cargo/bin/cargo test --locked -p sim-runtime --test system_worker
```

---

Build an interactive schematic for arbitrary registered multiphysics systems.
New Rust domains must be able to register quantities, connectors, and components
and get a useful diagram without changes to core domain enums or viewer code.
Extend it with the spatial GUI checkpoints above, through the same model
identities and observations.

## Product and implementation decisions

- Start with a native Rust application: an egui diagram widget hosted by eframe.
  Keep graph extraction, layout, and runtime access independent of egui. Verify
  and pin compatible versions in the first implementation step. The official
  [egui project](https://github.com/emilk/egui) documents custom 2D painting and
  native/web operation; [eframe](https://docs.rs/eframe/latest/eframe/) provides
  the application host. The native schematic is implemented; see the evidence record.
- Make v1 an inspector: open a system, navigate its connections, inspect
  parameters and provenance, run/pause/step/reset, select quantities to plot,
  and scrub recorded samples. Node dragging changes diagram layout only.
- Use existing Rust/Rhai composition and captured CAD imports. Physical model
  edits continue through the existing authoring workflows. Future diagram
  editing must use shared validated commands and revision/undo semantics.
- Domain implementations are statically linked Rust crates. Registration can
  be explicit at application composition; dynamic shared-library loading is
  outside this delivery.
- Native delivery is first. Keep serializable descriptions/frames and a
  worker-compatible boundary for later browser hosting; do not claim a WASM
  target works until its build, transport, and numerical parity are tested.
- Physics always runs through the existing compiler/runtime. UI repainting,
  plotting, and flow indicators do not implement or advance physics.

## Original starting points and gaps (historical; see current handoff above)

- `sim-core::Behavior` already provides equations, state, events, and optional
  Jacobians. `BehaviorDescriptor` and `BehaviorRegistry` supply component
  factories, ports, and parameter metadata. Extend these rather than introducing
  a competing physics trait hierarchy.
- `QuantityKind` and `ConnectorKind` are closed enums. Connector lane layouts,
  derivative relations, and owned-frame behavior currently live in core;
  compiler paths also contain frame and thermal special cases.
- Signal validation currently treats `Dimensionless` as a wildcard. Preserve
  legacy behavior explicitly during migration, then represent wildcard typing
  separately from a physically dimensionless quantity.
- `sim-compile::Runtime` accepts a general `ModelWorld` and commits states to
  `StateStore`. Existing getters expose across quantities and signals; per-port
  through contributions need a supported observation path.
- `sim-runtime::session::Scene` requires a robot. General schematic sessions
  must be built around the existing general runtime rather than an invented
  empty robot or a second integrator.
- `sim-script::catalogue` exports registry descriptions. CAD's graph already
  owns persisted component/connection identities and maps to native instances.
  Preserve and extend that mapping; names and slotmap keys are not sufficient
  identities across independently rebuilt models.
- The Bevy phenomena viewer has reusable spatial shapes but per-exhibit
  presentation. Leave it functional while establishing the shared description
  and observation contract that a later spatial adapter can consume.

## Architecture and ownership

| Location | Responsibility |
| --- | --- |
| `sim-core` | Definition traits, versioned type identities, quantity/connector/component descriptors, registry validation |
| `sim-compile` | Resolve descriptors into numeric layouts, compile connection rules, expose committed observations |
| `sim-runtime` | General system session, capture/replay, shared commands and worker lifecycle |
| `sim-script` and CAD adapters | Preserve authoring IDs, source locations and provenance while composing systems |
| New `sim-inspect` | Serializable system description and observation catalog; no graphics dependency |
| New `sim-diagram` | Deterministic layout, routing, hit testing, presentation state and reusable egui widget |
| `sim-viewer` | Existing native schematic application, analysis, inspector/plots/transport and worker connection |
| `sim-spatial` | Reusable Bevy spatial presentation and thin native assembly explorer; consumes shared inspection contracts |

Dependency direction: domain crates depend on core; compilation and runtime use
registered definitions; inspection consumes those shared contracts; diagram and
application consume inspection. Physics crates never depend on graphics. Extract
additional crates only when a real dependency cycle or reuse need appears.

## 1. Establish baselines and lock the contracts

Capture representative numerical and serialization fixtures before migration:
electrical RC, thermal storage/conduction, coupled motor/heating, a composite
connector, and an owned mechanical frame. Reuse committed examples and existing
acceptance thresholds. Record runtime/compile cost on a named host.

Define versioned `SystemDescription`, `ObservableDescriptor`, `SampleFrame`, and
`DiagramState`. A description contains components, ports, multi-terminal nets,
explicit groups, source/CAD references, parameters, and available observations.
An observable has an identity, quantity reference, owner, sign/frame convention,
and availability. Frames carry run ID, model revision, step/time, sequence, values,
and quality/availability; layout state contains positions, collapsed groups,
camera and plot selections. Reject frames belonging to a different description.

Preserve source-owned IDs through compilation. For standalone Rhai, support
explicit IDs with deterministic source-based fallbacks scoped to a captured
model; communicate when edits change those fallback identities. Recompilation
invalidates resolved numeric handles, never silently reuses them.

Acceptance: fixtures are durably retained, contracts serialize deterministically,
and the native egui host builds and opens on the target Mac with pinned versions.

## 2. Introduce trait-defined quantities and connectors

Add object-safe `QuantityDefinition` and `ConnectorDefinition` traits that
produce owned serializable descriptors. Keep `Behavior` as the component physics
interface; offer a registration adapter for component definition/factory metadata.
Plain descriptors should also be registerable for simple declarations.

- Stable namespaced IDs and schema versions identify definitions. A frozen
  registry resolves them to compact handles before compiling a model.
- Quantity metadata includes semantic identity, dimensional exponents, canonical
  units and display conversions. Accommodate existing modal and square-root-mass
  quantities; do not assume all dimensions have integer powers. Distinguish
  absolute temperature from temperature differences and wildcard signal types.
- Connector descriptors declare lane identities, quantity references, derivative
  links, composite members, ownership requirements, and equation assembly rules.
  Compatibility requires declared semantic/schema compatibility, not matching
  display units or dimensions alone.
- Validate duplicates, conflicting versions, missing references, cyclic
  composites, invalid derivative dimensions, ownership mappings and units before
  compilation. Persist descriptor hashes and package/build identity with runs.
- Keep solver assembly rules finite initially: shared variables and balanced
  contributions, directed signals, derivative relations and owned frames. New
  domain names using these rules must not require compiler changes. A truly new
  coupling rule is a future compiler extension; do not promise arbitrary new
  mathematical formalisms through metadata alone.
- Represent supported power/entropy diagnostics explicitly. Do not infer power
  by multiplying arbitrary across/through lanes. Unsupported diagnostics are
  unavailable, not zero or passing.

Migration proceeds additively: register built-in definitions behind compatibility
adapters, migrate compiler lookups, then domain crates, Rhai/catalogue exports,
CAD consumers and recording serializers. Old formats decode through explicit
versioned translation. Resolve their definitions once, never inside residual
loops. Remove authoritative closed enums only after all supported consumers
use registered definitions; retain legacy deserializers as needed.

Acceptance: existing fixtures preserve topology, lane order, validation and
numerical results within existing limits. No solver tolerance is relaxed to
accept migration drift. Dependency changes build on native and existing browser
targets. Unexplained same-host stepping regression above 5% blocks migration.

## 3. Prove an external domain, before building the full UI

Add a small separate example crate implementing a diffusion network: a new
concentration quantity and connector, storage volumes, conductance elements and
a fixed-concentration boundary. It may reuse the registered molar-flow quantity.
Document equations, SI units, sign conventions and illustrative parameters.

The example registers into a caller-supplied registry and builds an ordinary
`ModelWorld`. Explicit dependency/registration wiring in the host is expected;
changes to core domain definitions, compiler cases or viewer cases are not.

Acceptance: analytic relaxation and closed-system amount conservation pass,
timestep refinement passes declared limits, and the registry/inspection export
fully describes the previously unknown types. This is the extension-system gate.

## 4. Build generic inspection and committed observations

Implement description extraction and observable binding in the shared inspection
API. Expose all declared states, acausal lanes and signals with useful labels;
support optional component readouts and safe generic symbol fallback. Render
invalid/unresolved systems structurally with diagnostics even when they cannot run.

Add per-port through-value observation in the compiler/runtime. Prefer capturing
contributions from a shared evaluation at the accepted state, with its matching
rates and stochastic inputs. Any evaluation performed for inspection must not
advance RNGs, trigger events, mutate controllers or alter solver caches. Account
explicitly for derivative-dependent quantities and algebraic values. When an
exact accepted-state observation is unavailable, report that fact.

Use an observation subscription compiled once to numeric bindings. Keep native
typed frames; encode only when crossing a process/file/web boundary. Never resend
static topology or parameter metadata on every sample. Selectable optional
diagnostics must distinguish absent implementations from valid zero values.

Acceptance: inspection on/off leaves a deterministic run unchanged; per-port
flows sum to the connection residual within declared solver tolerances; custom
domain values have correct units. Failed lookup returns diagnostics rather than
panicking. Rename/rebuild tests verify identity behavior.

## 5. Deliver the static interactive schematic

Build the graph as components plus connection junctions. A physical net joining
three terminals must remain one net; do not convert it into invented causal
arrows or arbitrary pairwise physics. Signal edges have explicit producer and
consumer direction. Support composite-port expansion and explicit open ports.

Provide pan/zoom/fit, selection, component/quantity search, incident-net highlight,
port tooltips, a registry-generated inspector and collapsible declared groups.
Use both text/shape and color for domain distinctions. Display long names without
overlap and retain inspectable full labels. Offer keyboard navigation and visible
focus. Save layout changes atomically in a separate versioned sidecar, with undo.

Add an analysis layer owned by the engineer: annotations, display-label overrides
and user-defined groups of components. Allow naming, annotating, moving and
collapsing these groups without changing CAD or physical topology. Save these
edits with the layout sidecar and include them in undo/redo. User group boundary
connections must remain traceable to every original terminal. Validate grouping
ambiguities explicitly rather than silently reassigning components. Review this
layer on the current quadruped while investigating a concrete modification.

Use deterministic initial placement, stable ID tie-breaking and orthogonal edge
routing. Layer directed signal regions; lay out physical subnetworks without
claiming causal direction. Respect pinned positions, disconnected islands,
cycles and collapsed groups. Compute expensive layout off the UI thread with
cancellation; apply results only to their originating model/layout revision.
Cache layout and text geometry; cull offscreen details and reduce labels at low
zoom. Do not relayout when values change.

Acceptance: RC, coupled motor/thermal and external diffusion systems all render
with the same widget and zero model-specific UI code. Layout is deterministic,
selection resolves to correct ports/components, and reopening preserves layout.
Retain screenshots for overlapping ports, branched nets, loops and dense graphs.

## 6. Connect live execution, plots and replay

Add a general system session in `sim-runtime` around `sim-compile::Runtime`.
Reuse/extract existing capture and control facilities without cloning stepping
logic. Accept captured Rust-built/Rhai/CAD-composed systems. Keep robot, world
and controller inputs separate; CAD properties remain owned by captured CAD.

Use a native worker process so a stuck solve can be terminated without freezing
the viewer. The same command service supports UI and headless/agent callers:
describe, subscribe, start, pause, step, reset, cancel and retrieve recording.
Progress includes build/run state and the last completed simulation time.
Explicit command inputs use the existing controller boundary and are recorded
at simulation steps; parameter inspection does not silently mutate a live plant.

Advance on fixed declared simulation intervals independent of repaint cadence.
Bound queues and coalesce display frames; preserve requested recording samples
separately, applying backpressure or explicit recording failure instead of silent
loss. Show simulation time, sample age, measured stepping speed and errors.
Tag restart generations so delayed frames from an old worker cannot overwrite
new state. Last committed samples remain visible after failure, marked stale.

Plots and diagram values share one time cursor. Live transport supports
run/pause/step/reset. Recorded playback/scrubbing reads captured samples without
rerunning physics or invoking controllers. Mark absent samples and interpolation
explicitly. Flow indicators use signed observed terminal contributions; motion
speed is a display scale, not a claim about material transit time.

Record captured inputs, source/definition hashes, seed, solver settings, inputs,
observables and completion/failure status. Distinguish observation playback from
input re-execution: the latter requires matching implementation identities and
reset/controller/random state. Do not inherit a claim of complete checkpoint
restore from the current plant-only snapshot facility.

Acceptance: matched headless/live runs agree at identical simulation times;
pause/single-step/reset obey their boundaries; slow and failed solves leave pan,
zoom and cancellation responsive; recorded diagram values equal plot samples.

## 7. Acceptance, integration and delivery

Initial performance budgets below are proposed acceptance targets, not measured
capabilities. Freeze fixture sizes, hardware, build mode and measurement method
before claiming them. Keep numerical and UI performance results separate.

| Case | Proposed gate |
| --- | --- |
| 200 components / 400 nets, cached layout | p95 UI update/paint CPU time below 16.7 ms on the named target Mac |
| Interaction while worker is intentionally stalled | selection/pan feedback below 100 ms; cancel stops worker within 1 s |
| Deterministic layout, 200 components | completes within 1 s on the named host |
| 2,000 components / 4,000 nets | responsive navigation with culling/grouping; cancellable layout within 5 s |
| 64 observed scalars at 30 Hz on representative coupled example | stepping plus capture overhead below 10% versus the same run without capture |
| Long run | bounded display queue and plot ring buffer; retained trace grows only according to explicit recording policy |

Add focused registry/compiler/inspection tests, external-crate contract tests,
headless GUI interaction tests, and rendered screenshot inspection. Include
native/WASM compilation and relevant existing numerical/browser parity gates
for foundational changes; run existing CAD schema/graph and Rhai tests. GPU and
timing checks run on a named host; portable CI enforces deterministic structural
and numerical acceptance, not arbitrary hosted-runner timing.

Deliver committed examples, format documentation and commands for native view,
headless capture and playback, plus measured acceptance results and known limits.
Expose the same system description through existing catalogue/inspection CLI or
REST surfaces. CAD can launch the viewer with a captured document without
reloading the editor or discarding unsaved edits. Do not rebuild Python's graph
editor in this phase.

## Original foundational implementation sequence (retained)

1. Baseline fixtures, description contract and native UI compatibility spike.
2. Registry traits and built-in compatibility bridge.
3. Compiler, domain and authoring/serialization migration, with numerical gates.
4. External diffusion-domain proof and generic inspection export.
5. Static schematic, layout persistence, inspector and interaction tests.
6. General session worker, committed flow observations, plots and recording.
7. CAD launch integration, stress cases, documentation and acceptance report.

The original first visible milestone was a static diagram with selectable typed ports for
both an existing model and the new external domain. V1 is complete after the
same models run, plot and replay through the shared runtime with the acceptance
results retained. No implementation dates are asserted before the compiler and
serialization migration has been scoped by the baseline step.

## Full CAD assembly reasoning case

The user-selected current quadruped assembly is the primary full CAD acceptance
case, alongside the small analytic examples. Start with the retained
`examples/full-robot/whole-swing/browser.scene.json` and its referenced CAD
artifact; preserve its fidelity settings and source identity. This capture has
12 motor records, 29 exported rigid links and 105 CAD joint records. Exported
CAD entities and compiled physics components are distinct counts.

The overview must expose declared subsystem groups and shared infrastructure.
Expand an actuator to inspect its drive, firmware, thermal path and measurement
connections; focus a selected component and its incident nets while keeping
boundary connections visible and traceable. Provide domain visibility controls
and source-CAD references. Hidden details must retain their actual identities
and must not imply disconnected or pairwise physical nets. Grouping metadata
belongs to the composition/inspection boundary, not robot-specific viewer code.
Mechanical internals packaged in one behavior must be identified as such; an
overview of that behavior is not evidence that every internal CAD joint is
individually rendered. Full assembly inspection must preserve the captured
model, parameter provenance and explicit approximation settings.

For this acceptance case, review the interface by completing concrete tasks:

- Locate a motor from its CAD identity and inspect the corresponding drive,
  firmware, mechanical output, thermal storage and measurement components.
- Follow its command, electrical supply, shaft and heat connections; identify
  every terminal on a selected physical net, including external boundaries.
- Return to the assembly overview without losing which motor was being
  investigated. Domain visibility must remain a presentation choice and must
  make hidden connections explicit.
- Distinguish source-owned CAD properties from experimental overrides and
  identify the capture's approximation settings and importer warnings.
- Identify ideal per-motor supplies and the shared ground in this capture
  accurately; do not imply that it includes a shared battery bus.

These tasks remain acceptance work until verified with the running viewer;
rendering the component count alone does not satisfy them.

## Later spatial animation

Add optional spatial presentation bindings keyed by the same component and
observable IDs: geometry references, frames/poses, scalar overlays and vector
anchors. CAD remains the source of physical geometry. A spatial renderer
consumes the same timestamped frames and selection/time-cursor state as the
schematic. No 3D field is mandatory in v1 and no second physics implementation
is introduced. Spatial interpolation and field visualizations require their
own fidelity validation when implemented.
