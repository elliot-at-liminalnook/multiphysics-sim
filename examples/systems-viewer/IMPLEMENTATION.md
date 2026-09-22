# Systems viewer implementation record

Objective: implement and validate **all** of `systems-viewer-plan.md`. This record
tracks progress; it does not replace or narrow the acceptance requirements.

**Resumed for incremental spatial GUI delivery, 2026-09-20.** The full goal is
incomplete. The current GUI checklist and retained runtime handoff are at the
top of `systems-viewer-plan.md`. Existing evidence below remains historical.

## Current requirement status

| Plan requirement | Current state | Evidence / remaining work |
| --- | --- | --- |
| 1. Numerical and serialization baselines | Captured | `evidence/pre-migration-baseline.json`, baseline generator in `sim-compile/examples/systems_baseline.rs`; five fixtures and native host/toolchain identity retained |
| 1. Versioned description, frame and layout contracts | Implemented with focused tests | `sim-inspect`, `tests/contracts.rs`; authoring adapters still need to supply their retained IDs |
| 1. Native host compatibility | Rendered and visually inspected | Pinned eframe 0.36.2/Glow probe, `evidence/native-host-probe.png`; this is a catalog probe, not the schematic |
| 2. Definition traits, registry and validation | Implemented additive foundation | `sim-core::definitions`, built-in migration adapter and registry tests |
| 2. Compiler/domain/CAD/Rhai/serialization migration | Partial | Compiler connections now carry frozen definition handles; lane, derivative, sparse contribution/ownership and quaternion assembly use descriptors. Quantity references now carry open namespaced IDs, schema versions and validated canonical-unit metadata; compiled states and signals can use registered external quantities. Connector authoring now uses open references, registered composite expansion and registry-derived initial parameters/catalogue lanes. Explicit wildcard signal migration and final performance acceptance remain |
| 2. Native/WASM parity and <=5% performance regression gate | Pending | Foundation builds/checks and numerical fixture comparison are partial evidence; full migration gates remain |
| 3. External diffusion crate and analytic/conservation/refinement checks | Implemented with focused acceptance | `sim-example-diffusion` registers external quantity/connector traits, integrates ordinary models, exports registry metadata and renders in the generic viewer; retained native screenshot and analytic/conservation/refinement tests |
| 4. Generic extraction and committed state/flow subscriptions | Partial | `sim-inspect::model::describe` extracts actual topology, parameter units, state/port metadata and supplied authoring IDs; numeric per-port flow bindings and accepted implicit-stage capture now have deterministic/stochastic non-interference tests. Runtime subscriptions now bind states, acausal lanes and signals once and emit validated version-2 endpoint/stage frames. Optional diagnostics, live transport and the capture overhead gate remain |
| 5. Static schematic, layout, routing, grouping and persistence | Interactive preview, incomplete | Topology-aware placement, orthogonal obstacle routing, typed side ports, domain emphasis, exact terminal tracing, cancellable layout jobs, pins and navigation restoration are implemented. Annotations, display overrides, disjoint analysis groups, multi-selection, durable sidecars and undo/redo now have focused workflow tests. Full keyboard navigation and stress gates remain |
| 6. General session, worker commands, recording, replay and plots | Partial | General runtime session and serializable commands, fixed intervals, reset by fresh factory, independent bounded observation recording and exact-frame playback are implemented. Native process transport, recorded controller ingress, CAD adapters, plots and shared UI cursor remain |
| 7. CAD/CLI integration, stress budgets, CI and delivery documentation | Pending | Final acceptance report must cover every required gate |
| Spatial assembly and animation | Native checkpoints 1–3b delivered | Linked selection, shared worker controls/graphs, sampled shaft motion, uniform temperature colors and timed readouts; `evidence/live-checkpoint-3b/README.md`. CAD meshes, fields and linked replay remain |

## Commands and evidence

Use `/Users/elliot/.cargo/bin/cargo` on this host; the default shell PATH does not
include Cargo. Examples below use `cargo` for portability.

```sh
cargo test --release -p sim-core -p sim-compile
cargo run --release -p sim-compile --example systems_baseline -- \
  --check examples/systems-viewer/evidence/pre-migration-baseline.json
cargo test -p sim-core -p sim-inspect
cargo check -p sim-core -p sim-inspect --target wasm32-unknown-unknown
cargo check --workspace --all-targets --locked
cargo build -p sim-viewer --example host_probe --features native-probe
SIM_VIEWER_PROBE_SCREENSHOT=/absolute/path/native-host-probe.png \
  target/debug/examples/host_probe
cargo run -p sim-viewer
cargo run -p sim-viewer -- --model /path/to/model-world.json
cargo test -p sim-diagram
cargo run -p sim-runtime --bin sim-inspect-scene -- \
  examples/full-robot/whole-swing/browser.scene.json \
  examples/systems-viewer/full-robot.description.json
cargo run -p sim-viewer -- --description examples/systems-viewer/full-robot.description.json
cargo run -p sim-viewer -- --description examples/systems-viewer/full-robot.description.json \
  --focus-group cad/motor/0702845b41d3
```

The pre-migration fixture retains input `ModelWorld` serializations and sampled
outputs at 0.1 s intervals through 1 s, using 1 ms implicit midpoint steps.
Cases: RC charging, thermal cooling, coupled electrical/rotational/thermal motor
with composite plug, free planar frame and free spatial frame. The checker also
decodes legacy models, checks channel/sample counts and compares values with
`abs(error) <= 1e-10 * (1 + abs(reference))`. This is a migration comparison, not
an independent proof of every fixture's physics. Existing analytic tests remain
required; additional diffusion and flow checks have not been performed.

Timing values in the baseline are single captures on the retained Intel Mac
host, not a passing performance distribution. Performance acceptance needs
controlled repeated measurements after migration. The plan's proposed budgets
have not been relaxed or claimed met.

The first schematic preview uses the captured baseline models through the actual
shared model inspection API. Its default choices are RC, thermal, motor/heat and
free-body examples. It is a structure viewer: simulation transport, live values
and plots are not yet connected. Early routing places components in a deterministic
grid with column exits and one horizontal bus per displayed net/bundle. It is suitable for reviewing the interface
on small models, not evidence for the planned large-system layout budgets.

The native schematic was built, captured and visually inspected on this host
(`evidence/native-schematic-preview.png`), then launched as an interactive window.
Three diagram tests pass: deterministic branched routing, actual pointer
selection, and dragging with rerouting while preserving the model. The retained
logs are `evidence/native-viewer-build.log` and `evidence/diagram-tests.log`.
Visual inspection caught an averaged junction marker that could coincide with
an unrelated wire crossing; routing now anchors at a real terminal and marks
actual branch joins. The rejected screenshot is retained. An initial headless
test harness failed because it discarded egui texture updates without clearing
them; the harness now explicitly clears those updates because it has no GPU.
These checks do not establish full layout, accessibility or performance acceptance.

## Full CAD system preview

The user-selected current quadruped assembly is the primary full CAD acceptance
case. Its retained capture exports through the shared runtime into
`full-robot.description.json`: **195 compiled components, 597 ports, 242 nets,
56 declared groups**. The original CAD artifact's SHA-256 was checked against
the captured `source.cad_sha256` and matches. Export builds the existing system
without advancing physics or changing CAD, options or controller policy.

Composition records source-owned motor/joint/link IDs when creating components.
The viewer consumes these generic groups; it contains no quadruped or motor
topology rules. Overview bundles retain every original net and terminal in
source maps. Motor focus expands its internal components and retains external
connections as context. Headless projection tests check deterministic output,
net accounting, full boundary-terminal retention and component identities.

Screenshots are retained as `evidence/full-robot-overview.png` and
`evidence/full-robot-motor-focus.png`. An earlier render with wires through cards
is retained as `evidence/rejected-full-robot-overlap.png`. The revised routes
exit via column gaps. **Dense routing still needs further work**: coincident
corridors, domain visibility, grouped port presentation and automatic placement
have not passed the final readability/performance gates. Collapsing or focusing
currently starts a new presentation layout; persistent layout/undo is pending.

The articulated mechanical behavior remains a single compiled component. The
29 exported links and 105 CAD joint records are not individually expanded into
mechanical internals yet. The UI exposes the original importer warnings and
provisional fidelity notes. Live transport and values remain pending. This is
evidence of full capture inspection, not completion of the full CAD reasoning case.
This particular capture has no battery: each motor has an ideal supply and the
motors share a ground reference. The collapsed power group must not be read as
evidence of a shared battery model.

Native factory references now have an explicit parameter metadata flag and are
excluded from physical inspection. Component-local state declaration identities
are retained during compilation, so renaming display labels does not change
state observable IDs. Tests cover in-place rename and independent rebuild.

The final combined test build initially exhausted local disk space while
building a new dependency-feature combination. Its rejected build log is
retained; disposable `target/debug/incremental` caches were cleared before
retrying. Source and CAD captures were preserved. Automated screenshot mode now
exits with an error if saving fails, preventing a stale PNG from being mistaken
for a fresh capture after an out-of-space error.

Validation for this increment: **89 tests passed** across core, compilation,
inspection, diagram and Rhai. The workspace/all-targets check and the
core/compiler/runtime/inspection WASM check passed. All five retained baseline
trajectories still match at the original `1e-10` absolute/relative threshold.
Logs are retained in `evidence/compiler-inspection-diagram-rhai-tests.log`,
`compiler-mapping-workspace-check.log`, `compiler-mapping-wasm-check.log`, and
`compiler-mapping-baseline-check.log`. These do not establish the remaining
trait migration, custom-domain, observation, worker, playback or performance gates.

## Findings to carry into migration

- Legacy node state labels repeat (for example `node.voltage`); observations
  need explicit port/component bindings rather than lookup by label alone.
- Legacy entropy-production channels currently use `QuantityKind::Entropy`
  (`J/K`) even though the runtime comment identifies the value as `W/K`. Add a
  semantically correct entropy-rate definition during migration and document
  the schema correction separately from numerical regression.
- Current algebraic values and rates must be audited at accepted-state sampling;
  do not promise exact instantaneous flow based merely on existing state-store
  access. The planned observation non-interference gate remains essential.
- The eframe `__screenshot` helper captured a black buffer with Glow on this
  Mac. Source inspection showed capture after swapping buffers. The rejected
  image is retained as `evidence/rejected-glow-after-swap.png`. The probe now
  requests `ViewportCommand::Screenshot`, receives the render-time image and
  saves it before closing. The resulting image was opened and inspected.
- The initial full-workspace check exposed pre-existing missing Cargo feature
  gates on `prepare_contact_order_neighbors` and `optimize_joint_ipopt_steps`.
  Their source imports feature-gated conic/IPOPT APIs. Their manifest entries now
  declare the matching required features; failure logs are retained. Check the
  examples with `--features conic,native-ipopt` as well as the default workspace
  build so the correction does not simply hide a broken target.

## Completion policy

No stage is complete based on a scaffold, passing unrelated tests or a screenshot
alone. The final audit must inspect full compiler migration, external-domain
simulation, real schematic interactions, worker lifecycle, synchronized replay,
CAD capture and all named performance/accuracy gates. The active goal remains
in progress until those requirements have direct evidence.

## Open quantity references

`QuantityKind` is now an open reference with a namespaced definition ID, schema
version and canonical-unit cache. Built-in names remain constants; the private
legacy enum only translates old serialized spellings and builds built-in
metadata. New quantities register through `QuantityDefinition` or an owned
`QuantityDescriptor`. Compilation and standalone inspection reject missing
registrations and inconsistent cached units. Domain/controller changes clone
or borrow this metadata; the residual equations are unchanged.

The custom concentration tests cover reference round trips, missing/versioned
registrations, unit mismatch rejection, numerical integration, signal metadata
and generic inspection serialization. This proves the quantity path, not the
external acausal domain gate: connector authoring is still a closed enum.

A new test initially assumed midpoint-integrator signal unknowns were endpoint
samples. That assertion failed: the existing integrator evaluates algebraic
signals against the differential midpoint state. The test now checks the
endpoint state's analytic decay and the explicitly derived midpoint signal
separately at the same tolerance. No solver behavior or tolerance was changed.
Accepted-state signal/flow capture remains required before live observation
frames can label those values with an endpoint timestamp.

Source migration notes: `QuantityKind::Voltage` and other built-in constants
remain available. Variant glob imports become `sim_core::quantities::*`.
Quantity references and schemas containing them implement `Clone`, not `Copy`;
`unit()` borrows the reference, so retained units must be copied into owned
strings when a caller needs to mutate the originating state store. Custom
references must register their descriptor before compilation or inspection.

Validation so far for open quantities: **94 focused tests passed**, native
workspace/all-targets check passed, core/compiler/runtime/inspection WASM check
passed, and all five numerical baselines matched the unchanged `1e-10`
threshold. Logs are `open-quantity-tests.log`, `open-quantity-workspace-check.log`,
`open-quantity-wasm-check.log` and `open-quantity-baseline-check.log` under
`evidence/`. Rejected test assumptions are retained separately. Full stepping
performance distributions and accepted-state observation semantics remain
unvalidated; these results do not close those gates.

The four targeted `sim-runtime` motion-forecast tests also pass after migrating
quantity comparisons and controller metadata (`open-quantity-forecast-tests.log`).

The updated runtime re-exported the full quadruped without stepping. Its
serialized description is byte-for-byte identical to the retained capture:
195 components, 597 ports, 242 connections and 56 groups. Comparison hashes are
in `evidence/open-quantity-full-capture-comparison.json`; the build/export logs
are `open-quantity-viewer-build.log` and `open-quantity-full-capture.log`.
The rebuilt native viewer rendered the motor-focus view and its new screenshot
was visually inspected (`open-quantity-full-robot-focus.png`). Dense routing,
small port labels and incomplete mechanical expansion remain visible limitations.


## Ergonomics iteration: topology, routing and tracing

The current quadruped now uses deterministic connected-region placement,
compact grid exchanges that reduce connection length and prefer signal direction,
and orthogonal grid routing around component cards. Placement adjacency is only
an optimization aid; every rendered physical net still contains its original
terminals. Same-net branches share routes; distinct domains have stable colors,
signals use square terminals and directional arrows, and presentation bundles
remain dashed with their original-net mapping. Ports occupy labeled rows on the
sides of cards. The old whole-diagram bottom bus is gone.

Selecting a terminal highlights exactly its incident nets; selecting a component
highlights its connections and fades unrelated paths. Domain checkboxes dim
paths while retaining context. Background layout jobs are cancellable and revision
checked; they share immutable descriptions rather than cloning them during each
drag. Dragged nodes are pinned, Arrange preserves pins and clears other cards
around them, and Unpin is available. Back restores the previous camera, positions
and selection. View state is retained in memory only; this does not fulfill
sidecar persistence or undo acceptance.

Eleven focused tests pass, including actual pointer selection/dragging, precise
terminal tracing, full motor terminal retention, complete segment/card collision
checks, pin retention, cancellation and navigation restoration. The retained test
run measured the 16-node/15-net motor layout at about 30 ms and cancellation of a
running full-assembly layout at about 1.3 ms. These are individual observations,
not the proposed 200/2,000-node or UI p95 performance gates. The native viewer
build passes, and both `ergonomics-motor-focus.png` and
`ergonomics-quadruped-overview.png` were rendered and visually inspected.

The first new-router attempt (`layout-local-routing-motor.png`) was rejected as
too tall, with insufficient signal/thermal color distinction and label overlap.
Further iterations compacted placement, strengthened color contrast, separated
signal styling, shortened labels to fit their actual screen space and removed
unselected bundle-count clutter. `layout-compact-motor.png` retains an intermediate
placement for comparison. Current limitations include duplicate abbreviated
boundary labels, incomplete keyboard/accessibility support, annotations/user
groups and persistence, and unvalidated dense-layout performance. Open connector
authoring, the external diffusion domain, committed observations and the general
simulation worker/recording/replay path also remain required.


## Engineer-owned analysis workspace

The current quadruped now supports display-label overrides and notes on source
components, groups and individual connections. Engineers can Shift-click cards,
move/pin the selection together and create named, colored analysis groups. Focus
shows the selected group with its external context; group stripes are distinct
from connection-domain colors. Group membership is validated as disjoint. These
are presentation changes only: the original captured components, parameters,
ports, nets and CAD artifact are preserved. The generic `sim-diagram::analysis`
module owns this metadata, without robot-specific logic.

A versioned JSON workspace retains annotations, groups, camera, positions, pins,
focus, collapsed groups and domain emphasis. Undo/redo includes annotations,
groups and completed placement edits. Navigation/camera changes are saved but do
not each add an undo step. Saves use a same-directory temporary file, file sync,
atomic rename and optimistic file-content checks. A cooperating viewer lock
prevents simultaneous saves; a changed file is rejected rather than overwritten.
An interrupted process can leave a lock file; automatic stale-lock recovery and
power-loss durability of the parent directory are not established.

Each opened model retains its own workspace and filename. Save-and-close applies
open editors and saves all opened models, stopping on validation or file conflicts.
An unapplied editor prevents model switching. Invalid drafts remain available for
correction. Explicit discard-and-close remains available.

The retained illustrative workspace groups one hip motor's winding, case and
three thermal conductances for a cooling review:

```sh
cargo run -p sim-viewer -- \
  --description examples/systems-viewer/full-robot.description.json \
  --workspace examples/systems-viewer/quadruped-thermal-review.workspace.json
```

The notes pose engineering questions; they are not measured faults or calibrated
recommendations. The preview retains the other actuators, mount thermal paths and
environment as connected context. Full labels and notes remain inspectable when
card text is shortened. Use the inspector to edit notes/groups; use File… to choose
a workspace filename and Save workspace (or Command-S) to write it. Reopen using
`--workspace`. No simulation is running in this structure preview.

Validation: 19 focused tests across `sim-diagram` and `sim-viewer`, including actual
pointer selection and group movement, thermal-review creation, undo/redo, save and
reopen, source-capture byte preservation, invalid-group draft retention, ID collision
avoidance, multiple-model saves and changed-file rejection. See
`evidence/analysis-workspace-tests.log`. Native build and visual evidence are
`evidence/analysis-workspace-native-build.log` and
`evidence/analysis-thermal-review.png`. The initial unsupported arrow-glyph render
is retained as `evidence/rejected-analysis-label-glyphs.png`.

This increment does not complete static-viewer acceptance: large-system timing,
full keyboard/accessibility navigation, richer boundary labels and external-domain
rendering remain. Live values, committed flow capture, session/replay transport,
ConnectorKind authoring migration and full CAD launch integration also remain.


## Open connectors and independent diffusion extension

`ConnectorKind` now holds open namespaced, versioned identities. The private
legacy enum only defines built-in compatibility metadata and old JSON spellings.
Anonymous composite JSON owns its members; deserialization no longer leaks a
static allocation. Named composites use registered member names and semantic
identities. Their expansion, leaf connection, nested pin binding and inspection
parent traversal are generic. Malformed serialized member maps are rejected.

Registered component descriptors now derive initial-parameter names and units
from the frozen physical catalogue. Rhai/CAD catalogue export and experiment
channel inspection use those registered lanes. Failed component registration
stages its changes and leaves the previous registry intact. The runtime retains
its frozen metadata for inspection without residual-time registry lookups.
See `CONNECTOR-MIGRATION.md` for authoring compatibility details and limitations.

The separate `sim-example-diffusion` crate registers a previously unknown species
concentration and connector through the public traits. It implements a well-mixed
storage volume, linear diffusion conductance and a fixed-concentration boundary.
No core domain list, compiler case or viewer case is added for diffusion. The
exporter supplies explicit host registration; the viewer only reads the generic
description. The crate README records equations, sign convention, SI units,
illustrative parameters and predeclared numerical limits.

Acceptance checks cover analytic relaxation, amount conservation through a closed
pair's trajectory, timestep refinement, model serialization, semantic mismatch
rejection, registry-generated initial parameters/catalogue lanes, generic routing,
and a nested two-channel connector with pin binding. The h=0.2 s and h=0.1 s
relaxation errors were `1.5332463e-4` and `3.8323370e-5` mol/m³, respectively.
Energy diagnostics remain explicitly unavailable for this concentration model.
The initial nested test exposed a single-level restriction in inspection;
`evidence/rejected-diffusion-nested-inspection.log` retains that failure. Parent
validation now supports nested acyclic trees while rejecting invalid owners.

The native diffusion diagram is retained in `evidence/diffusion-native-viewer.png`
and its source in `diffusion.description.json`. It was visually inspected, with
readable components, parameters, typed terminals and the registered domain label.
The shared viewer's generic input label was corrected from "Captured CAD system"
to "Captured system", since imported descriptions need not originate in CAD.

The quadruped re-export remained byte-identical, SHA-256
`51890374ec98279ef6e0152099e6585471914f4886c8df3e615b2cd26ff27483`;
`evidence/open-connector-full-capture-comparison.json` records this comparison.
The five retained numerical fixtures also preserve their complete model/channel
serialization and sampled values at the original `1e-10` threshold. Single-run
stepping timings did not increase in that capture, but this is **not** evidence
for the required controlled <=5% regression gate; that gate remains open.

Final validation: **114 tests passed** across core, compiler, inspection, Rhai,
diffusion, diagram and viewer. The native workspace/all-targets and selected
WASM checks passed. Logs: `evidence/open-connector-final-tests.log`,
`open-connector-final-workspace-check.log`, `open-connector-final-wasm-check.log`,
`open-connector-final-baseline-check.log`, and `open-connector-final-viewer-build.log`.
The workspace CI test includes the extension crate; the baseline CI step now also
exports the diffusion description. No claim of a completed remote CI run is made.

Remaining full-plan work includes explicit signal wildcard semantics, supported
committed state/flow sampling, live sessions and process transport, plots,
recording/replay, CAD launch safety, keyboard/accessibility work, diagram stress
budgets and the controlled migration performance gate. The full goal is active.


## Accepted flow capture without observational reevaluation

`sim-compile::observation` now exposes numeric flow bindings tied to one runtime
instance. Captures reuse existing solver residual evaluations, match the solved
stage exactly, and publish only after a matching endpoint is committed. The
binding retains registered quantity identity and canonical units. Sampling does
not call equations, advance RNGs, run controllers/events or modify solver caches.
Backward Euler and implicit midpoint are supported; midpoint values retain their
actual stage time rather than claiming endpoint precision.

Owned frame observations include the owner's registered constitutive balance
rows as well as explicit through writes. This is essential for showing a body's
reaction/inertial contribution rather than an incorrect zero. Event jumps,
seeding, restore and consistency operations invalidate captures. Missing or
unsupported captures and invalid bindings return explicit errors.

See `OBSERVATIONS.md` for the API, timing semantics and limitations. Tests include
deterministic and seeded stochastic capture-on/off trajectory equality, unchanged
residual call counts, repeated-read non-interference, per-node balance, a known
force accelerating an owned body, foreign-runtime binding rejection, event and
restore invalidation, external diffusion units and nested composite flow mapping.
Validation: **77 tests passed**, including dynamics, compilation, external-domain
acceptance and a dynamics doctest. Native workspace/all-targets and selected WASM
checks passed, and the five retained numerical baselines still match. Logs are
retained as `evidence/accepted-flow-final-tests.log`,
`accepted-flow-final-workspace-check.log`, `accepted-flow-final-wasm-check.log` and
`accepted-flow-baseline-check.log`. This does not yet connect live values to the
viewer or establish the observation-overhead gate. Inspection frames must first
carry the accepted stage's time semantics without implying instantaneous endpoint
values. The overall implementation goal remains active.


## Runtime subscriptions and quadruped binding audit

`sim-inspect` now has an optional `runtime` feature with `RuntimeInspection`,
compiled subscriptions and typed version-2 sample frames. Differential endpoints
and accepted implicit-stage algebraic values/flows carry distinct timing. Invalid
stage intervals and foreign runtime handles are rejected. A failed runtime commit
keeps old endpoint values and withholds newer algebraic values and port flows.
Repeated reads execute no equations. Contract-only consumers retain the lighter
dependency path. See [OBSERVATIONS.md](OBSERVATIONS.md) for API and format details.

The full quadruped audit found 12 firmware-target quantity mismatches caused by
order-dependent legacy wildcard signal resolution. The compiler now selects the
concrete connected quantity and rejects conflicting typed consumers regardless of
terminal order. The original failing audit is retained; this does not complete
the separate explicit `Any` authoring migration.

Validation for this increment:

- **49 tests pass** across compiler, inspection and independent diffusion tests,
  including signal-order/type rejection, stage/endpoint timing, failed-commit
  withholding, conservation, custom units, serialization and identity checks.
- **38 Rhai integration tests pass** after the signal resolution fix; see
  `evidence/subscription-script-tests.log`.
- Native workspace/all-targets and WASM checks pass. Build checks alone do not
  establish browser numerical or performance parity.
- All five retained numerical baselines still match at the unchanged 1e-10
  absolute-plus-relative tolerance.
- Current quadruped: **1,408 observable bindings, zero binding failures** across
  195 components, 597 ports and 242 nets. Before stepping, 577 endpoint values
  are available; 831 algebraic/flow values are explicitly unavailable until an
  accepted evaluation exists. Sampling leaves its runtime snapshot unchanged.
  This audit does not claim a stepped quadruped live run.

Evidence: `subscription-validation.json`, `subscription-tests.log`,
`subscription-workspace-check.log`, `subscription-wasm-check.log`,
`subscription-baseline-check.log`, `quadruped-subscriptions.json` and
`rejected-quadruped-wildcard-bindings.json` under `evidence/`.

The CAD binding audit is available to headless callers and portable CI:

```sh
cargo run --locked -p sim-runtime --example inspect_scene_observations -- \
  examples/full-robot/whole-swing/browser.scene.json
```

The native application remains a structure/analysis viewer. Session commands,
worker transport, live values, plots, replay, full CAD launch integration and
performance/accessibility acceptance remain open requirements. Static-to-runtime
description identity reconciliation must also preserve the engineer's sidecar
work when capabilities change the description hash.


## General command/session and bounded observation recording

The new `sim-runtime::system_session` service wraps the shared compiler runtime.
It implements describe/subscribe/start/pause/step/reset/cancel plus recording
commands, using the same fixed-interval advance for scheduled ticks and headless
single-step calls. `ModelSource` supplies ordinary models and registry/authoring
identities; custom factories can retain existing composition/coupler setup.
Reset reconstructs the system, validates its identity before swapping, increments
the generation and returns any old recording. It does not reuse plant-only
snapshots as complete controller/random-state checkpoints.

Display and recording subscriptions are separate. Recording capacity includes the
initial point, and overflow stops execution before advancing another interval.
Failure preserves the last completed display/recording frame and an explicit
failed status. Exact-frame playback validates captured samples and never invokes
physics. Archives contain source-bound runtime identity, description, seed,
requested/default and actual per-island solver settings, selected observables,
completion reason and final status. Per-event logs are drained between intervals;
cumulative committed event counts are preserved.

See [SESSIONS.md](SESSIONS.md) for the API, format and retention policy. This is
in-memory observation capture. Native worker transport must still preserve
committed recording data outside a killable worker; controller input recording
and full re-execution metadata are not implemented. No process cancellation,
UI latency, plot/replay integration or performance gate is claimed by this step.


Session validation: **8 focused tests pass**, covering matching ordinary/runtime
interval results, pause/single-step boundaries, recording/display separation,
capacity protection, fresh reset generations, preservation after failed reset or
solve, failure cause retention through reset, hybrid-event state/count parity,
multirate metadata and archive validation. Native workspace/all-target checks
and the WASM build pass. Evidence is retained in `evidence/system-session-tests.log`,
`system-session-workspace-check.log`, `system-session-wasm-check.log` and
`system-session-validation.json`. These checks do not prove the remaining process,
UI, controller-input or performance requirements.


## User-requested stopping point: initial native worker

The user asked to stop and document this work before pivoting to a new feature.
The full viewer objective is **not complete**. No viewer/UI files were modified
in this final worker increment.

Added `sim-runtime::system_worker` (native only) and `sim-system-worker`:

- A reusable JSON-line process service delegates to `SystemSession` using a
  supplied registry and a captured `ModelWorld`. No CAD/scene controller adapter
  is present yet.
- Bounded request/reply channels, message-size checks, a single coalesced display
  frame, stale-generation rejection, and parent-owned process termination.
- The initial runner aims at 1x simulation time and emits display updates at
  approximately 30 Hz. Those are scheduling choices, not measured fidelity or
  responsiveness acceptance results.
- A conservative 16 MiB transport recording budget rejects oversized requests
  before recording starts. Durable parent-side recording is still missing.

**One native process smoke test passes**: launch/build the thermal model,
subscribe, single-step, validate its frame, reset, reject an old-generation
command and terminate/reap the child. Evidence: `evidence/system-worker-smoke.log`.
This proves basic process plumbing only. It does not prove UI responsiveness,
stalled-solver termination timing, display/reply ordering under load, long-run
queue budgets, recording recovery or full quadruped execution.

Resume with worker/client stress and error-path tests, then UI transport on a
small model. Ensure pause publishes the final completed frame, review reset/exit
ordering, reconcile static/runtime description IDs without losing workspaces,
and preserve committed recording outside a killable worker. Then integrate the
current quadruped through its existing controller path, retaining captured solver
and fidelity settings. Do not silently omit its scene controller program.

The full remaining checklist is at the top of `systems-viewer-plan.md`.

The shared runtime library still checks on WASM after this increment; the worker
binary has an explicit native-only execution boundary. Evidence:
`evidence/system-worker-wasm-check.log`. This is not browser-worker support.


## 2026-09-20 — spatial GUI checkpoint 1

The user resumed work with a requirement for incremental, reviewable GUI progress.
The plan now orders spatial inspection, linked views, runtime controls, replay,
CAD capture integration, fields, and full acceptance as separate checkpoints.
Only the first spatial inspection checkpoint is complete in this increment.

Implemented:

- Graphics-free `sim-inspect::spatial` contracts bind display primitives to a
  sealed `SystemDescription`; validate version, IDs, provenance, meter units,
  right-handed Y-up frame, dimensions, finite coordinates and unit quaternions.
- Shared presentation commands provide atomic selection validation, visibility,
  exploded offsets and connection-guide control without touching physical data.
- `sim-spatial` is a reusable Bevy library plus thin native CLI. Its dependencies
  include inspection and graphics, not runtime/solver code. Model-specific shape
  choices and placement live in the example sidecar, not renderer branches.
- A retained exporter derives the example description from the existing
  `motor_composite` baseline and shared registry: seven components, twelve ports,
  five nets, thirty-one declared observations, fourteen illustrative primitives.
  Source references hash the actual retained baseline file; the description's
  model source hash is the canonical serialized original `case.model` value.
- Native GUI supports 3D/list selection, registry-derived parameters/units, actual
  terminal membership, orbit/pan/zoom/fit, exploded view, hide/reveal and net hubs.
  Multi-terminal nets remain hubs with every terminal, not pairwise invented
  physical connections. Guides with hidden/unrepresented endpoints are omitted.
- Missing live data stays explicitly unavailable. Geometry and colors are marked
  illustrative. This fixture has no driver board or LED. It does not represent
  detailed CAD geometry, a resolved temperature field, or running physics.

Verification: **18 tests passed** (17 inspection/contract tests, including three
new spatial tests; one Bevy click/list/button-hold integration test). Shared
inspection compiles for WASM. CLI validates the retained artifact and rejects
unknown selections and incomplete input pairs before creating a window. Native
screenshots and selection logs are retained under `evidence/spatial-checkpoint-1`.
Rust formatting and changed tracked-file whitespace checks passed. Native host:
Intel macOS 26.6.2, AMD Radeon Pro 5300M, Metal, existing Bevy 0.16.1.

The first native review reported lag. The concurrent high-parallelism combined
package test build was stopped, not counted as passing evidence. Tests then ran
separately with two low-priority build jobs. The native launcher applies that
limit only to compilation; the GUI runs at normal priority. Inspection of Bevy's
installed winit source also exposed 5 s/60 s desktop idle waits that could leave
pipelined UI feedback stale. Focused reactive updates now wait at most 16.7 ms,
unfocused at most 250 ms. These are scheduling settings, **not measured latency
or FPS results**. The updated native window rendered with selection and overlay
changes visible. The original resource-heavy build and initial GUI logs are kept.

Still pending: synchronized cross-window selection or a single-window combined
host; live worker integration; controller ingress; dynamic poses, LED/thermal
observations, plots/replay; CAD meshes and bidirectional source selection; spatial
fields; browser graphics; persistent spatial camera/settings; full keyboard focus
and accessibility; exact interaction/large-scene performance distributions. Existing
schematic sidecars and open RoboCAD work were not modified by this preview.


## 2026-09-20 — linked assembly and schematic (GUI checkpoint 2)

Bidirectional source selection is implemented for components, ports and complete
nets. Two compact native windows share an ephemeral background selection session;
collapsed projection proxies never replace the original identities. Receiving
selection preserves schematic layout, focus/collapse, pins and unsaved analysis.
Exact-net inspection/annotation remains exact inside a projected bundle.

Launch: `./examples/systems-viewer/run-linked.sh`. The script builds both hosts
with two low-priority jobs, then opens them at normal priority. GUI clicks
verified both directions, port tracing, complete multi-terminal net selection
and reattachment. Final assembly text/connection-label polish is captured in
`evidence/spatial-checkpoint-2/reconnected-assembly.png`.

46 tests pass (33 inspection/diagram, 11 schematic, 2 Bevy); both native binaries
build and the graphics-free inspection crate checks on WASM. Physical fixture
hashes remained unchanged. Full results, individual test logs, screenshots and
limits: [checkpoint 2 record](evidence/spatial-checkpoint-2/README.md).

The user requested no further desktop interaction while watching shows; desktop
automation stopped and remaining records/checks were completed in the background.
GUI layout feedback is pending. Runtime-source fingerprinting currently includes
new UI crates and causes unnecessary runtime rebuilds after presentation edits;
that identity/build boundary needs a separate audit before the next iteration.

Live worker controls, measurements, animation, CAD meshes, fields, browser
rendering and final performance/accuracy gates remain pending. This completes
the linked-selection increment, not the entire systems viewer plan.


## 2026-09-20 — live controls and reusable connection graphs (checkpoint 3a)

The schematic now hosts the shared simulation worker with run/pause/step/reset/
cancel controls. Selecting a connection in either linked view offers its actual
terminal measurements, with units, availability and sign conventions. Up to
eight graphs remain pinned across selection changes. Their bounded display
history retains true sample times, marks accepted-stage versus endpoint values,
breaks at unavailable samples, and rejects stale frames after reset.

The graph is reusable: `sim-diagram::plot::TimeGraph` takes caller-owned samples,
units, time bounds, height, color and a shared cursor; it needs no worker/model.
`sim-inspect::plot` holds channel discovery and validated history. Independent
synthetic-data widget tests and a public usage doctest exercise reuse outside the
live panel. `sim-viewer::live_ui` provides only session/selection adaptation.

The new retained live capture binds the exact model and authoring identity map
by hash to the original sealed description. The three existing fixture files
are byte-for-byte unchanged. Worker compilation/stepping stays off the UI thread.

62 tests plus one doctest pass. All three native binaries build; inspection also
checks on WASM. Worker/headless samples match exactly over 20 fixed intervals,
and actual headless pointer events exercise the connection graph picker. The
final worker binary passes both process tests. Launchers and targeted formatting
checks pass. Full receipts: [checkpoint 3a verification](evidence/live-checkpoint-3a/README.md).

Launch: `./examples/systems-viewer/run-live.sh`. Native GUI review is pending:
the user's no-desktop-interaction request remains in force, so no windows were
opened or replaced. The assembly remains static; checkpoint 3b adds animation
from this same run. Curves are coalesced live observations, not lossless
recordings. Full recording/replay and final accuracy/performance gates remain
open, as does the costly presentation/runtime source-fingerprint boundary.


## 2026-09-20 — live physical assembly (checkpoint 3b)

The latest physical and schematic viewers were opened first, then the physical
view was connected to the schematic-owned worker. Explicit example bindings map
accepted angle samples to shaft rotation and temperature samples to uniform
component color. A white spoke exposes orientation on the symmetric wheel.
Timestamped angle, speed, current, temperature and signed heat-flow readouts use
the same observation stream as the graphs. No second solver or render-clock
integration was introduced. Source physical and geometry captures are unchanged.

Shared `sim-inspect::animation` and `sim-inspect::live` contracts validate the
bindings and runtime extension of authored observations. A bounded background
latest-snapshot transport rejects foreign, regressing and stale-reset data;
disconnected or stalled views hold their last samples with explicit status.
Animation subscriptions survive clearing graphs. Pause now returns the final
accepted sample, so both views stop at the exact completed interval.

Native review found and fixed the prior worker-description rejection of legitimate
compiled state observations. It also led to shorter unavailable-value text,
font-safe readouts, and a readable minimum graph-panel height. The launcher builds
the worker and schematic together to reuse their shared dependency feature set;
Bevy remains a separate build. The broader source-fingerprint audit stays open.

53 distinct focused tests passed (26 inspection, 4 spatial, 13 schematic, 8
session, 2 worker), including exact headless/worker frames and analytic angle/
temperature checks. Spatial and schematic tests passed again after UI polish;
both process tests also passed against the final worker binary. Native GUI
review verified changing motion/colors, linked selection, paused frame stability,
one 0.01 s step, reset, cancel/restart, graph-independent animation, and peer
closure. The final pair was reopened, reviewed and left running side by side.
The schematic's Save and close flow preserved its analysis sidecar when replacing
windows. The earlier no-desktop request was explicitly revoked by the user.

Launch: `./examples/systems-viewer/run-live.sh`. Full records, sample snapshots,
input hashes, test/build logs and screenshots are in
[evidence/live-checkpoint-3b/README.md](evidence/live-checkpoint-3b/README.md).
Recording/replay, CAD meshes, driver/LED extensions, spatial fields, browser
rendering, calibration and final performance/accuracy gates remain open.


### Shared annotation and REST increment — 2026-09-20

Both native viewers now expose the reusable `sim-api` loopback transport, bounded
ordered batches, discoverable command catalogs and job receipts. Commands execute
on the application owner through the existing selection, analysis and runtime
paths. `simctl` batches operations across both hosts in one invocation. Headless
hosts use those same adapters without opening windows.

The annotation-first increment adds source-bound, overlapping multi-component
discussions, typed component and saved-view links, hover emphasis and matching
group treatments. Shared `sim-inspect::annotations` commands validate and atomically
persist a common sidecar with revision checks and bounded undo/redo. Saved views
pair the physical camera/display profile with the schematic layout/focus. The
schematic editor protects drafts from conflicting changes and model switching
retains each model's annotation path. Physical cards expose shared links, selection,
group outlines and saved views; text editing is in the schematic or REST.

`sim-render` generates off-screen physical, section, schematic and graph PNGs on
background threads, retaining source identities, annotation links and observation
metadata. It renders display geometry and captured measurements, not another
physics implementation. Sections remain explicitly illustrative, not CAD solids.

35 focused tests and the actual two-host REST acceptance passed. Five final PNGs
were inspected off-screen. A reproduced large-response truncation on macOS was
fixed and covered by 4 MiB JSON / 2 MiB binary transport regression tests. The
source captures were unchanged. Native windows and audio were left untouched at
the user's request, so native visual review and broader tutorial UI control remain
next work. The older open windows need a future restart to load these additions.

Guide: [REST.md](REST.md). Receipts, images, hashes and logs:
[evidence/rest-api/README.md](evidence/rest-api/README.md).
