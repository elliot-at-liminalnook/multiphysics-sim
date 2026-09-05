# CAD + Rhai experimentation workspace

## Multiphysics extension (accepted goal)

The workspace also includes a registry-driven component inspector and connection
view alongside the CAD viewport. A persisted document graph owns stable component
and connection IDs, body attachments, parameter values and explicit derivation
recipes. UI, Python and REST edits use the same revision-checked document operations;
candidates, undo, saved documents and captured runs retain the graph.

Use the Rust registry for types, ports, parameter declarations, units and connection
validation. Compose graph components and Rhai systems into the same native world.
Expose body thermal capacities, motor winding/housing thermal connections, sensors
and controllers. Geometry derivation is explicit and captured, including the rule
and inputs used to turn a modeled duct into a fluid element. Complete a coupled
electromechanical–thermal example with measured limits and reproducibility/performance
CI gates, in addition to the pendulum and two-joint examples below.

Planned work. This replaces the earlier CAD-first experiment proposal with a
shared workflow for CAD, Rhai system definitions, sampled controllers, and
measured outcomes. The app's purpose is to let the user and coding agent iterate
together quickly, with reproducible inputs and reviewable evidence.

## The loop

Edit CAD / system script / controller → capture inputs → build the system from
Rust components → simulate → inspect synchronized traces and geometry → compare
with a baseline → retain or revise the candidate.

The GUI, CLI and agent API operate on the same experiment records and runner.
A script-only system uses the same workflow; CAD is an optional system input.

## 1. Define ownership and one model contract

- CAD owns geometry, material assignments, assembly identities, joints and
  attachments. Capture committed document state, including unsaved edits;
  an unfinished tool gesture is not part of a snapshot.
- Rhai defines system composition, scenario inputs, parameter overrides,
  controller bindings and experiment expectations. It can import a captured
  CAD assembly and compose additional registered Rust components around it.
- Preserve CAD defaults as explicit inputs. Record each override and its source
  in the resolved configuration; do not construct duplicate actuators or silently
  choose between conflicting CAD and script settings.
- Add a proposed `sim-script` adapter that builds the existing `ModelWorld`
  through the Rust component registry. Refactor the existing CAD builder into
  reusable composition functions where needed. Both authoring paths use the
  same compiler, physics components and solver.
- Define a versioned run specification containing CAD artifact hashes, entry
  script and imported-module contents, controller sources/artifacts, parameters,
  seed, solver/fidelity settings, units and library/binary identity. Capture
  imported files before starting work so a concurrent edit cannot alter a run.
- Carry stable CAD IDs through merged physical links and result records. Names
  remain display labels. Maintain mappings back to both CAD parts and script
  declarations, including units at the millimetre-to-SI boundary.

Acceptance: equivalent small Rust and Rhai definitions produce matching resolved
systems and traces within declared tolerances; a CAD change affects physical
properties without requiring the system script to be rewritten.

## 2. Build a complete, small CAD → Rhai → result workflow

Start with the existing motorized pendulum. A Rhai entry script imports its CAD
assembly, selects the scenario, binds a controller and declares measured limits.
Add a sampled Rhai controller adapter through the existing `Coupler` contract;
initially use the explicitly labelled position-target interface of the current
CAD motor adapter. Keep the existing Python controller as a parity reference.
System construction runs before simulation. Controller callbacks run on sampled
simulation time; physical equations and numerical integration remain in Rust.

- Launch from the editor or API without manually exporting intermediate files.
- Use immutable input snapshots and a separate worker process for expensive CAD
  derivation, compilation and simulation. Workers own their loaded CAD/kernel
  state; do not share live Qt/OCCT objects with them.
- Report queued, building, running, completed, failed and cancelled states, with
  logs and timings by stage. Run creation returns promptly with an ID.
- Cancellation stops the worker and any controller children. Preserve a failed
  run's diagnostics and label partial outputs; never present them as a pass.
- Preserve the last successful result while a new script has a build error.

Acceptance: change a CAD dimension, a Rhai system parameter, and a Rhai controller
parameter independently; each produces a new captured run with the expected
physical/behavioral change. No Rust rebuild is needed for these edits. Tests cover
controller failure, invalid scripts, cancellation and editor responsiveness.

## 3. Make results the shared review surface

- An Experiments panel shows the baseline, candidate, changed inputs, run status,
  assumptions and pass/fail metrics. Select a run to inspect its captured model.
- Retain time-series traces and add synchronized plots and CAD motion replay.
  Clicking a plot sample positions the model at that time; selecting a part
  filters relevant signals. Keep pose preview and simulated replay distinct.
- Compare runs with explicit units, sampling/time alignment and scenario identity.
  Label differences in fidelity or scenario rather than treating all comparisons
  as equivalent. Show changes in mass, tracking, current and other objectives.
- Bind results to exact input identities. Editing the current document marks its
  previous result association as outdated; it does not change historical runs.
- Extend annotations to include run ID, signal/time range and script location as
  well as part IDs. A discussion can capture both a design concern and its evidence.
- Enable atomic, revision-checked CAD edit batches and candidate documents. The
  user can keep editing while the agent tests a candidate. Accepting a candidate
  uses a reviewable change set and checks for intervening edits.

Acceptance: geometry edits cannot silently retain a current-results badge; rename
and fixed-link merging preserve mappings; plots, replay and annotations identify
one captured run. Concurrent edits fail with an actionable conflict.

## 4. Deepen controller and script authoring

- Provide registry-derived discovery for component types, ports, units and
  parameters. Fill gaps in parameter descriptors rather than maintaining a
  separate scripting component catalogue.
- Report errors at script source locations with the relevant component, port or
  CAD part. Include reusable subsystem functions and captured module imports.
- Make the controller boundary configurable and explicit: supervisory position
  targets first, then supported current/torque or driver-level commands. Expose
  the appropriate measured channels and preserve existing controller backends.
- Add a Rhai code view with diagnostics, parameter editing and run actions; allow
  external-editor changes to feed the same captured-source workflow.
- Optional automatic reruns debounce edits and supersede obsolete queued runs.
  A new version starts a new run from defined initial conditions. Continuing
  state across an edit is a separate feature requiring plant and controller
  state compatibility checks.

Acceptance: the same sampled control law and declared interface agree across
Rhai and the reference backend. Timing, units, startup state and failure behavior
are tested; controller updates cannot be mistaken for a different control layer.

## 5. Make repeated iterations fast

| Change | Reuse | Recompute |
| --- | --- | --- |
| Controller source or gains | CAD-derived artifacts; compatible plant build | Controller and fresh run state |
| Rhai scenario or system parameters | Unchanged CAD-derived artifacts | Affected system construction/compile stages and run state |
| One part's geometry | Artifacts outside its dependency set | Affected linked groups, collision data, inertia, attachments and relevant flex reduction |
| Material or physical joint settings | Geometry artifacts where valid | Dependent physical properties and compiled system |
| Comment, camera or selection | Physical model and existing runs | Review/display state only |

Use content-based cache keys including tool/library versions and derivation
settings. Cache reuse requires a proven dependency match; cold and cached paths
must produce equivalent outputs. Compiled runtime reuse is conditional on a
complete reset of all plant, controller and random state.

Provide explicit quick-check and validation profiles. The UI and run manifest
show enabled contact, flexibility, noise, solver settings and limitations.
Measure CAD derivation, script evaluation, compilation, stepping and rendering
separately before setting component-specific performance budgets.

Initial proposed targets for the small pendulum on a documented reference host:
run acknowledgement within 100 ms, UI heartbeat gaps below 100 ms during worker
execution, cancellation acknowledgement within 250 ms and worker-tree termination
within 2 s, and a cached controller-only 3.2-s experiment within 3 wall seconds.
These are development targets, not current guarantees. Establish repeatable host
measurements and enforce declared thresholds alongside correctness in CI.

## Delivery and proof

Ship section 1 and the complete small workflow in section 2 first, together with
minimal run history, exact-result identity, a basic trace plot and revision
checks from section 3. Then deepen replay/comparison, control interfaces and
incremental caching. Avoid building a broad scripting language surface before
one complete CAD/Rhai/control experiment is reviewable in the app.

Extend the existing acceptance suite with Rust/Rhai parity, captured-module
reproducibility, stale-result detection, unit/channel failures, atomic edit
conflicts, cancellation and cache invalidation. Next add a two-joint mechanism
with fixed attachments to prove mappings and motion beyond the pendulum. Add
contact, flexibility and uncertainty contracts incrementally; the current simple
acceptance example does not establish their accuracy.

The audit baseline passed 90 pendulum checks in 8.65 wall seconds on the local
machine: 3.84 s export and 0.95–1.42 s per process run. Build/install time was
excluded. That is evidence for the small example, not an assembly-size guarantee.
