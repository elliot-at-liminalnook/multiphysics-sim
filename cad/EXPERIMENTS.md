# CAD + Rhai experiments

The **Experiments** dock connects the current CAD document to captured Rhai system
and controller sources and a separate Rust simulation process. Run history and
artifacts are stored under `runs/experiments/`. The [roadmap audit](../experimentation-audit.md)
records the delivered workflow and its accuracy, reproducibility and performance evidence.

## Build and try it

From the repository root, using the Python environment with `cad/requirements.txt`:

```sh
cargo build --release --locked --bin sim-experiment
python examples/experimentation/build_model.py
PYTHONPATH=cad python -m robocad.ui.app runs/experimentation-models/pendulum.rcad
```

Open **Experiments** in the toolbar. The default system imports `assembly`, the
captured live CAD assembly. The default controller sends a position step to each
actuated joint after 0.2 simulated seconds. Set parameters, then **Run experiment**.
Run creation captures unsaved committed edits without saving over the CAD file.
An unfinished modelling gesture is outside that capture.

The run list shows queued, building, running and terminal states. **Cancel run**
stops the worker process group. A controller or script error leaves completed runs
available. A completed simulation is distinct from its measured evaluation:
`passed`, `failed`, or `unchecked` when no expectations are declared.

Select a completed run and **Inspect / compare**. The review window owns its
captured CAD document, so live modelling can continue. Click or drag in the plot
to select the nearest recorded CAD sample; use Play for motion replay. Select a
part in the viewport or dropdown to filter its channels. Set a baseline in run
history before opening another run to overlay traces and compare settings.

**Pose** is geometric joint preview. **Simulated replay** displays measured Rust
simulation transforms, including fixed attachments. Rendered geometry uses mm;
physical inputs and channels use their declared SI units.

Use **Annotate sample** in review to discuss a run, signal and time. These comments
live in the CAD document, support the existing thread/comment APIs and undo, and
retain the historical run reference after subsequent design changes. Focus such
a comment to reopen its evidence. API callers can also attach a script location.

## Rhai authoring

Registered Rust components compose through typed ports:

```rhai
let disk = part("disk", "rotational.inertia", #{
    inertia: 2.0, damping: 0.5, "initial.speed": 3.0
});
connect([disk.port("shaft")]);
```

With a CAD assembly, `cad("assembly")` exposes native component ports using names
such as `assembly.port("component.port")`. System construction happens once;
equations and integration run in Rust. `parameters()` returns the captured
parameter map for that script.

Controllers implement this sampled interface:

```rhai
import "reference" as reference;
fn control(t, sensors, commands, state) {
    commands["hinge1.target"] = reference::step(t, parameters().target);
    #{commands: commands, state: state}
}
```

All actuator names must be returned, with finite numeric values. Select the
controller boundary explicitly: **position targets in radians** retain the Rust
servo firmware; **driver duty** bypasses that firmware and accepts commands in
`[-1, 1]`. Driver controllers close feedback using measured joint angle/speed and
motor current (`A`), torque (`N·m`) and speed (`rad/s`).
Each new run starts with a fresh state map and fresh plant state. Return updated
state explicitly. Imported functions and module constants remain available;
Rhai functions access top-level constants through `global::constant_name`.

**Link Rhai file**, inside each System or Controller tab, links that editor to an external
entry file. The editor becomes read-only, reflects external changes, and captures
all `.rhai` files below its directory at run creation. Imports resolve within that
captured bundle, relative to their importing module; parent traversal and absolute
imports are rejected. **Runs → Restore run inputs** restores the entry and imported
module contents for editing. Automatic reruns are enabled in **Parameters**, debounce for 750 ms,
and replace obsolete queued automatic runs.

The **Components** tab and `GET /experiments/catalogue` read the native Rust
registry's types, typed ports, units and parameter declarations. Rotational,
translational, electrical, thermal, hydraulic, chemical, radiative, granular,
magnetic, fluid, acoustic, line, domain bridges, control, sensing, actuators,
planar/contact/joint, multibody and robot components have complete declarations
(117 of 117 components) and reject unknown parameters in both Rhai and native
compilation. Native factories also validate relationships between parameters and
the imported model, including dynamic link/port membership.
Composite initial values use their expanded member names, such as
`initial.plug.thermal.temperature` in kelvin for `bridge.motor`. Radiation band
limits use micrometres, as declared by the native component, rather than metres.

The compass walker declares `time_scale` in seconds: physical time is its
normalized model time multiplied by `time_scale = sqrt(length/gravity)`.
The default of 1 s preserves the existing numerical example. Angles are radians,
rates are rad/s, and omitted initial rates scale inversely with `time_scale`.
The pitch–plunge model rejects a nonpositive mass matrix; when pitch is locked,
its holding moment is measured in N·m and contributes no kinetic energy.

Imported CAD assembly IMU channels use `imu.<sensor>.ax/ay/az` in m/s² and
`imu.<sensor>.gx/gy/gz` in rad/s, including their native held and bias states.
Registry port families can include a suffix, so the inspector and compiler use
the same channel declarations. Assembly `gravity` is a dimensionless multiplier
of the captured gravity vector. Loop regularization has separate
`loop.cfm.translation` (1/kg) and `loop.cfm.rotation` (1/(kg·m²)) values;
the old shared `loop.cfm` parameter is rejected because it mixed these dimensions.

Initial node values use `initial.<port>.<lane>`, or `initial.<lane>` when a
component has one physical port. They apply to the shared connected node,
including lanes stored inside another component. For example,
`initial.shaft.speed:4` starts an inertia at 4 rad/s, and an attached planar
sensor's `initial.frame.x:0.75` starts its connected body at 0.75 m. Repeated
assignments must agree, including short/qualified aliases and fixed constraints;
conflicts report both names and the unit. Omitted values retain native states or
connected constraints, otherwise zero. Algebraic initial values are starting
guesses that the solver makes consistent with the equations.

Every `Frame` or `PlanarFrame` connection requires exactly one declared frame
owner (a body, or a chain's tip). Connection order does not select ownership.
Missing or duplicate owners fail compilation with the connected port names.
Attaching a component to a CAD body for selection/result association does not
create a physical frame connection; that connection must still be authored.

The Galerkin duct and King's-law heat-release components use nondimensional
quantities and normalized simulation time. Their `NormalizedAcoustic` connector
cannot connect directly to a physical acoustic port; a physical duct derivation
needs explicit reference scales. These models do not infer those scales from CAD.
String taps use fractional positions in `[0, 1]`; initial values must refer to a
declared tap. Cell/mode counts and controller delays must be whole numbers (up to
4096); cell/mode counts must be positive. External-controller family parameter
values declare channel membership, while the connected signals determine units.

Sensors declare bandwidth in Hz, latency and sample period in seconds, and noise
as the standard deviation of each sampled reading in that channel's unit. Noise,
quantization, encoder counts and sampling faults require `period > 0`; a zero
period provides a continuous reading. Seeds are exact nonnegative integers up to
`2^53-1`. Latency uses an Erlang chain with 1–1024 stages. Fault modes are 0 (none),
1 (stuck), 2 (dropout) or 3 (skip `fault.samples` sampling periods).

The planar `sensor.imu` uses `noise.ax`, `noise.ay`, `quantum.ax`, and `quantum.ay`
in m/s², and `noise.gyro`/`quantum.gyro` in rad/s. Its corresponding bias parameters
use the same units. Acceleration ports and recorded states carry m/s²; its axes
use independent reproducible noise streams. Earlier ambiguous global IMU `noise`
and `quantum` parameters must be replaced by these channel parameters. This
planar component is separate from the CAD articulated model's built-in 3D IMUs.

For planar mechanics, a prismatic joint's `(ux, uy)` must be a unit direction.
`planar.bend.stiffness` is in joules for its `k × (1 − cos(angle))` energy law.
Quadratic drag accepts either a coefficient in kg/m or density, drag coefficient
and area; supplying both descriptions is an error. Terrain patches declare a
whole-number `patches` count and `patch0.x0`, `patch0.x1`, `patch0.y`, etc. in metres.
Each patch requires an end greater than its start and an index below the count;
the `edge` fade length is explicit. These horizontal patches are authored physics
geometry, not an automatic conversion of a CAD terrain surface.

## Document system graph

Open **Experiments → System graph** to edit components alongside the CAD viewport.
The native catalogue loads automatically. Choose a type and **New**, enter a
name and parameter values, then **Apply**. Values use the units shown in the
table; blank optional values retain native defaults. Hover a parameter for its
required/default value and bounds. **+ Parameter** adds dynamic family members.
Their units update as the name is entered and remain visible after saving;
unrecognized names show **Unknown** and cannot be applied to a fully declared type.
Select a CAD body and use **Attach to selected CAD body**, then Apply.

Click two port names or dots in the connection view to connect them. The cursor
changes while choosing the second port; Escape cancels. Clicking a port followed
by **Leave port open** declares a single-port physical connection. Click a wire
to highlight it and enable **Remove connection**. Drag the background to pan,
and use **Overview** to see the whole graph. **Focus selected** fits the selected
component's entire card; scroll or use +/− to zoom. Long names stay inside cards,
with full names and units available on hover. Clicking a component selects
its attached body in CAD. Edits are undoable; a form opened before another edit
cannot overwrite it—select the component again to reload the current revision.

The side panels share tabs by default, leaving space for CAD. Drag a dock title
to arrange panels separately. The inspector scrolls independently of the graph;
drag their divider to allocate more space to either. Run, check and cancel remain
below the active experiment tab. **Runs** contains result comparison, baseline,
restore and diagnostic controls.

The document also retains a system component graph. `GET /system` returns
`{revision, graph}`; `PUT /system` takes `{expected_revision, graph}` and publishes
one undoable edit. Python clients use `system_graph()` and
`set_system_graph(graph, expected_revision)`. Atomic batches/candidates support
`set_component_graph` through the same operation.

Granular edits use the same registry-derived checks as the inspector:

| Route | Operation |
| --- | --- |
| `GET /system/components[/id]` | Read components |
| `POST /system/components` | Create with `{expected_revision, component}` |
| `PATCH /system/components/id` | Update fields with `{expected_revision, component}` |
| `DELETE /system/components/id?expected_revision=N` | Remove a component, retaining shared nodes where possible |
| `GET /system/connections[/id]` | Read connection nodes |
| `POST /system/connections` | Connect with `{expected_revision, ports}`; joining an existing node retains its ID |
| `DELETE /system/connections/id?expected_revision=N` | Disconnect a node |

Python clients provide `add_system_component`, `update_system_component`,
`delete_system_component`, `connect_system_ports` and `delete_system_connection`.
Each requires an explicit expected revision. Component updates replace the
parameter map when supplied. Structural and registry errors return 422; revision
conflicts return 409. Final native compilation checks the complete assembled system.

Graph version 1 stores `components` and `connections` as objects keyed by stable
IDs. A component has `id`, `name`, native `type`, numeric `parameters` and optional
CAD `body_id`, `binding` and `derivation`. Connections contain `id` and `ports`, each identifying a
`component_id` and native `port` name. Include a one-port connection for an open
physical port. Native compilation validates parameters, port names and wiring.

Runs capture the graph, generate a registry-backed Rhai module and retain it in
`composition.json` and `specification.json`. `component_graph_mapping` links its
declarations to stable component/body IDs. Editable sources remain separate from
the generated module. **Restore run inputs** also restores the captured graph as
one undoable edit. Body attachment also associates component port traces with CAD
selection, including retained partial results. Graph-only edits reuse mechanical
CAD artifacts; geometry-dependent recipes have their own validated cache.

### Explicit geometry rules

In the inspector, attach a CAD body and select **Geometry rule**. Parameters owned
by the rule are read-only; supplying both derived and numeric values is an error.
Each run captures the authored rule, geometry hash, material inputs, formula,
resolved values and units in `component_derivations.json` and the result record.

- `{"kind":"body_thermal_capacity"}` on `thermal.capacitance` computes
  `heat_capacity = volume × density × specific_heat` in J/K. Volume crosses from
  mm³ to m³ and material density from g/cm³ to kg/m³. Optional `specific_heat`
  overrides the material value in J/(kg·K). This assumes the complete solid is
  one uniform-temperature body with constant material properties.
- `{"kind":"circular_fluid_volume","flow_direction":1}` on `fluid.pipe_ph`
  treats a closed circular cylinder as the fluid volume, deriving length, diameter
  and elevation rise in metres. `flow_direction:-1` reverses endpoints and rise.
  The native element uses its lumped water model; the recipe does not infer bends,
  wall thickness, roughness or local losses. Supply those supported physical
  parameters explicitly. A duct wall or arbitrary hollow solid is rejected.
  The fluid-volume proxy is excluded from mechanical solid mass; mechanical
  joints, mounts and sensors must reference the duct wall instead of that proxy.

### Existing CAD components and Rhai connections

Use **Check system** (or **System graph → Check**) to capture and compile the
current CAD, graph and scripts without advancing simulation time. It opens the
controller contract but does not call the sampled control law or evaluate measured
expectations. Checks have a `not_simulated` evaluation and cannot be selected as
simulation baselines or compared as trajectories. They use the same asynchronous
worker, immutable inputs, cancellation and CAD caches as simulation runs.

After a check, choose an imported component from the picker and click **Use
existing**. The inspector fills its type, body attachment and stable binding;
**Apply** saves the graph binding. Choosing a component already bound in the graph
selects it. Parameter tooltips show captured imported values; **Last check** shows
explicit parameter values from the compiled plant, including derived overrides.
Blank cells may use native defaults. Edits retain the previous values with a
stale-input notice until another check completes. Wiring diagnostics identify
the port names, units and captured source lines.

For APIs, send `POST /experiments` with `preflight:true` and the usual expected
revision, system/controller and settings. `GET /experiments/{id}/components`
returns the captured revision, state, stale flag, error, `imported` list and
`resolved` list. The imported list is retained even if a later binding or wiring
check fails; the resolved list becomes available after native plant compilation.
Each entry includes its native name, type, stable binding, body ID when known,
explicit parameter values and actual ports with registry-derived units. A
non-finite native default is represented as a string such as `"inf"`, never as an
editable numeric JSON parameter. Python provides `check_system(request)` and
`experiment_components(run_id)` for the same operations. Both lists are also
retained as `imported_components.json` and `resolved_components.json` in the run.

When reviewing a result, **Inspect live component** selects the plotted graph
component in the current document's inspector. It is disabled if that component
has been removed. **View captured source** opens a read-only copy of the system
declaration and selects its recorded line, including generated graph declarations.
Annotations made from these signals retain the source path, line and column along
with the run, signal, body and time evidence. Script component channels also carry
declaration locations; controller callback locations are not mapped automatically.

`GET /experiments/{id}/sources` (Python `experiment_sources(run_id)`) returns the
captured `system` source bundle and the `controller` bundle when present. The
system bundle includes the generated graph module after composition. These are
the run's retained inputs, so later editor changes do not alter the displayed
source. Before composition, the endpoint returns the original captured inputs.

**Bind existing** exposes an imported component and overrides only the supplied
parameters. Blank parameters retain the imported values. Its type must match;
missing targets, duplicate bindings and added dynamic ports are build errors.
This lets a derived housing replace the existing case's heat capacity without
adding another actuator or thermal storage element.

Prefer stable bindings such as `cad/<motor-body-id>/case`, `/winding`, `/unit`,
`/g_wc`, `/g_ca` and `/g_cm`; the mount uses `cad/<mount-link-id>/mount`.
These survive display-name edits. Native names such as `drive1.case` and `ambient`
are also accepted. The graph keeps its own stable ID and signal aliases such as
`graph/housing.node.temperature`, regardless of the imported name.

Rhai can use `component("graph/housing")` to reference a graph declaration,
including a forward declaration, and connect it to another part. Direct scripts
can use `bind_component("housing", "cad/<id>/case", "thermal.capacitance",
#{heat_capacity:20.0})`. Bindings refer to imported components, not declarations
created by the same script. `connect` extends or joins existing nets, expanding
composite plugs member by member. Repeated endpoints are source diagnostics;
native compilation still rejects incompatible units and multiple signal drivers.

The connection view supports wheel/button zoom, dragging to pan, **Overview**,
and **Focus selected** to inspect a component at readable scale.

## Profiles, seeds and physical overrides

Select **Quick check** (rigid, contact and CAD sensor noise disabled; 0.5 ms step,
10 ms recording) or **Validation** (contact, flexibility and CAD sensor noise
enabled; 0.25 ms step, 5 ms recording). Both default to 3.2 simulated seconds.
Parameter `settings` override profile defaults, and Rhai `configure.settings`
explicitly overrides those captured settings. The review header displays the
effective settings. Enabling a model does not establish its physical accuracy.

Flexible runs record attachment-frame positions and displacements in world metres
under `trace.flex`. The signal picker exposes each boundary's `dx`, `dy`, `dz`
and displacement magnitude, associated with the link's stable CAD IDs. These
signals support expectations, comparison, part filtering and evidence annotations.
Replay shows boundary arrows synchronized with the rigid-link poses. The arrow
scale selector magnifies only the display; plots retain physical metres.
**Frame replay** fits the current poses and arrows. The CAD mesh remains rigid:
these arrows do not reconstruct a continuous deformed surface or stress field.
Old captured runs without boundary samples retain rigid-pose replay.

CAD-generated modal coordinates explicitly use mass-normalized shapes:
amplitudes are `m·√kg` and rates `m·√kg/s`. Boundary displacement is the sum of
shape translations times amplitudes, rotated into the current world frame.
Custom modal blocks can declare `displacement` normalization (amplitudes in m);
legacy blocks with unspecified normalization report `modal`/`modal/s` and a
warning instead of claiming physical displacement units for their amplitudes.

For a selected joint, **Properties → Flex patch radius (mm)** declares the rigid
attachment patch used for flex reduction. Unit expressions such as `0.8 cm` are
accepted; clearing the field restores inference. Python/API operations use
`set_joint_physics(joint_id, flex_patch_radius=0.008)` in metres, or `None`/`null`
to restore inference. The default is hole radius + 2.4 mm wall, at least 4 mm.
This changes the attachment boundary condition, not just mesh quality. The
physical artifact captures the resolved radius, its source, selected-node count
and bounds, and whether a small patch needed nearest-available-node fallback.
Patch edits invalidate flex reduction while reusing unchanged CAD geometry.
If reduction fails, experiments with flex enabled fail with the affected link ID
and derivation diagnostic; the captured physical artifact remains available.
Correct the geometry/patches or explicitly disable flex to run the rigid model.
Post-derivation `configure.cad_overrides` cannot change patch settings: changing
only the radius metadata would leave the modal basis stale. Set the CAD joint
property before capturing the next run.
Attachment IDs preserve flex signal comparisons across renaming and reordering.

Set the integer `seed` in Parameters or an API request. `seed()` exposes it to
system scripts and Rhai controllers; process controllers receive `SIM_SEED`.
The supported range is `0` through `2^53-1`, exact in native numeric parameters.
The run seed also initializes native solver noise, including Langevin elements.
CAD sensor streams combine the run seed with the captured CAD seed. Turning noise
off disables stochastic noise and bias walk, retaining declared fixed biases and
quantization. Scripted stochastic components use their own declared parameters;
pass `seed()` to their native `seed` parameter. This is a single seeded run, not
a Monte Carlo sweep over captured uncertainty distributions.

`configure.cad_overrides` accepts numeric existing fields identified by section
(`links`, `joints`, `motors`), stable ID or name, and a JSON pointer, including
array indices. `configuration.json` records original values, replacements,
effective settings and the declaring script location. Repeated conflicting
overrides, unknown fields, invalid settings and non-finite values are errors.

## Captured process controllers

Select **Captured process bundle · JSON** to edit a bundle in the Controller tab,
or send `controller.language: "process"` through the API. For example:

```json
{
  "language": "process",
  "parameters": {"target1": 0.2},
  "process": {
    "runtime": "python",
    "entry": "controller.py",
    "files": {
      "controller.py": {"path": "/absolute/path/controller.py"},
      "helper.py": "def reference(t): return 0.2"
    },
    "arguments": []
  }
}
```

Files accept UTF-8 text, a local `path`, or `base64` contents. Capture stores bytes
and content hashes before acknowledgement; later edits cannot change queued runs.
Include imported project modules and data in this bundle. Python starts with
isolated, stdlib-only imports plus the captured directory; installed site-packages
and live `PYTHONPATH` are excluded. Native controllers use `runtime: "native"` and
a captured executable as their entry, with any needed local libraries included.
Native artifacts remain specific to their host platform and system libraries.
Parameters are available as JSON in `SIM_PARAMETERS`. Raw commands remain
available in `sim-cad`; experiment runs require captured bundles.

The Python reference in `examples/experimentation/controller.py` uses the same
`simloop` client and law as its Rhai counterpart. Acceptance compares every
controller frame, channel unit and plant sample exactly, and checks missing
dependencies and cancellation of a controller that ignores SIGTERM.

## Measured checks

Declare expectations in the system script:

```rhai
let assembly = cad("assembly");
configure(#{expectations: [
    #{name: "Settled tracking", signal: "joints/hinge1/angle", unit: "rad",
      reduction: "rmse", target: 0.2, start: 0.6, max: 0.05},
    #{name: "Peak current", signal: "motors/drive1/current", unit: "A",
      reduction: "max_abs", max: 2.5}
]});
```

Supported reductions are `min`, `max`, `max_abs`, `final`, `mean`, `rms`, and
`rmse` against a numeric `target`. Specify at least one bound (`min` or `max`),
an exact unit, and optionally a time window (`start`/`end`, seconds). Empty windows,
unknown channels, unknown units and non-finite values are errors. RMS and mean
are explicitly sample statistics, not continuous-time integrals.

Comparisons align shared signals on candidate sample times inside the shared
recorded interval. Plant traces interpolate linearly; sampled controller channels
hold their previous value. Unit mismatches are rejected for that channel.
Differences in system source/parameters, seed, simulation settings or controller
boundary are labelled. Review also compares total/moving mass and measured
objectives; objectives with different units or definitions are not subtracted.

## Shared API and CLI

The desktop REST service runs on its displayed localhost port. Existing
`python -m robocad.client` commands can call these routes:

| Request | Result |
| --- | --- |
| `GET /doc` | Document ID and current revision |
| `POST /experiments` | Capture inputs and return a queued run (202) |
| `GET /experiments` | Run history for this document |
| `GET /experiments/catalogue` | Native component and parameter discovery |
| `GET /experiments/{id}` | Status, progress, timing, evaluation |
| `POST /experiments/{id}/cancel` | Request process-tree cancellation |
| `GET /experiments/{id}/inputs` | Original captured specification |
| `GET /experiments/{id}/result` | Completed result and current CAD association |
| `GET /experiments/{id}/diagnostics` | Errors, stderr tail and partial-output flag |
| `GET /experiments/{id}/partial` | Explicitly partial output from failed/cancelled runs |
| `POST /experiments/{id}/compare` | Compare with body `{"baseline_id":"…"}` |
| `POST /doc/batch` | Atomic revision-checked edit batch |
| `GET/POST /candidates` | List/create isolated design candidates |
| `GET /candidates/{id}` | Reviewable change set and base revision |
| `POST /candidates/{id}/experiments` | Run the captured candidate |
| `POST /candidates/{id}/accept` | Publish after checking the base revision |
| `DELETE /candidates/{id}` | Mark a draft candidate discarded |

Run requests require `expected_revision` and accept `system`, `parameters`,
`controller`, `settings`, `profile`, `seed`, `label`, and `parent_run`. Source bundles use
`{"entry":"main.rhai","files":{"main.rhai":"…","helper.rhai":"…"}}`.
Settings expose `seconds`, `step`, `sample`, `contact`, `flex`, `planar`, `noise`.
`configure(#{settings: #{...}})` records explicit overrides in `resolved.json`.

Create a candidate with an atomic batch:

```json
{
  "expected_revision": 12,
  "label": "Lighter bracket",
  "operations": [
    {"op":"box", "args":[[0,0,0],[10,20,30]], "as":"bracket"},
    {"op":"set_material", "args":[[{"$ref":"bracket"}],"petg"]}
  ]
}
```

Operations use the same `Ops` methods and argument conversion as the regular CAD
API. Only document edits are allowed in a batch. A failed operation publishes
nothing. An accepted batch/candidate is one undo step. Intervening edits,
including comments, produce HTTP 409 with the current revision. Review candidate
geometry and its change set through **Runs → Review design candidates**; run it
using the current script/parameters, accept it, or discard it.

Headless examples use the same Python experiment manager and Rust runner:

```sh
python examples/experimentation/run.py
python examples/experimentation/run.py --two-joint
SIM_EXPERIMENT_REQUIRED=1 python -m pytest -q examples/experimentation/test_workspace.py
```

## Artifacts and current limits

Each run retains `input.json`, `model.rcad` when applicable, `system.json`, `configuration.json`,
`physical.json`, `specification.json`, `resolved.json`, event/stdout diagnostics,
`stderr.log`, and `result.json`. Runtime failures may retain `partial.json`;
partial output is never returned as a completed result. Historical result files
are immutable when the live document changes.

Runner executables and a deterministic ZIP of the Python derivation package are
captured under `artifacts/`. Workers check their contents and installed dependency
versions before execution. Historical runs therefore retain their own runner code.

The physical export cache checks input identity, derivation code and dependency
versions, fidelity settings, and cached content hashes. Intermediate caches cover
body mass properties and meshes, linked collision geometry, joints, fixed
attachments and flexible reduction. A geometry edit invalidates affected groups;
material edits reuse geometry where dependencies match. Cold and cached paths are
compared in acceptance tests, including deliberate corruption. Collision-grid
sampling is deterministic and independent of scenario randomness. Whole-model
cache hits avoid importing OCCT. Compiled plants start fresh on every run.

On POSIX hosts, run leases follow the worker through an editor crash; it can
finish and publish results for a new editor to inspect. Abandoned queued runs
become interrupted failures with their inputs retained. Other editor instances
refresh live records without claiming ownership; cancel a live run in its
originating editor. Cancellation from a different editor is currently rejected.

Timings separate CAD derivation and its cached stages, Rhai evaluation, native
compilation, stepping (including sampled controller calls), and trace/pose
recording. Viewport CPU rendering time is shown onscreen and saved as
`review-*.json` on closing a replay window; it excludes GPU/display latency.
On the development Intel i9-9980HK/macOS 26.6 host, capture measured about 58 ms
and the cached 3.2-s pendulum about 3 wall seconds. Dedicated CI gates require
100 ms capture/heartbeat, 250 ms cancellation acknowledgement, 2 s process-tree
termination, and cached budgets of 6 s for pendulum and 12 s for two joints.

The mechanism examples validate rigid, contact-disabled mechanisms with declared
bearings. A separate CAD falling-box acceptance covers a 50-mm PLA cube dropped
300 mm onto an explicit floor at z=0, with stiffness 200,000 N/m and damping
2,000 N·s/m, flexibility/noise disabled and a 0.5-ms step. It requires peak floor
penetration below 0.5 mm, a settled bottom within 5 micrometres of the floor after
1 s, and exact cached trace repeats. This is a numerical contract for that setup,
not a validation of general collision or material behavior.

The same test checks free fall against the CAD solver's backward-Euler result,
`dz = -g*t*(t+h)/2`, within 1e-8 m. Halving the timestep from 0.5 to 0.25 ms must
halve the error against analytical free fall; at 1 s those errors are about
2.45 and 1.23 mm.

A separate CAD flex worker test uses a 100×10×10-mm isotropic PLA beam, an
unloaded root roll joint, a tip IMU attachment, explicit 20°C ambient temperature,
0.1-ms integration steps and 1-ms recording. Its world-space boundary trace must
match reconstruction from the captured mass-normalized modes within 1e-10 m
absolute/1e-5 relative tolerance. At 0.2 s it must agree with the captured modal
static equilibrium, including thermal softening, within 1e-9 m absolute/0.1%
relative tolerance. Cached traces repeat exactly. This tests the reduced-model
integration and result/replay path; it does not establish continuum accuracy.
The same worker acceptance also runs with an explicitly declared 8-mm root patch
and requires derived frequency/static sag within 15% of Euler–Bernoulli beam
theory, using the captured effective modulus and exact CAD mass. On the measured
mesh these differences are approximately 1.8% and 3.0%. This bound is specific to
that geometry, material model and clamp. The default inferred 4.55-mm root patch
gives approximately 13% lower frequency and 31% greater sag than a fully clamped
analytical beam; those are different boundary conditions. Neither result is a
universal accuracy claim for thin walls, complex attachments or printed material.

The API-authored planar IMU uncertainty acceptance records 1,000 samples per axis
at 1 kHz with a declared 0.2 m/s² noise standard deviation. For captured seeds 17
and 18 it requires absolute noise mean below 0.03 m/s², sample standard deviation
between 0.18 and 0.22 m/s², absolute cross-axis correlation below 0.1, and exact
same-seed repeat traces. Acceleration noise must leave the noiseless gyro channel
unchanged. This validates the sampled-noise path, not distributions of unknown
material properties or hardware uncertainty. A run does not automatically sweep
the physical model's Monte Carlo uncertainty block.

General contact, flexible-material and hardware accuracy require calibration and
geometry-specific validation. The completed [roadmap audit](../experimentation-audit.md)
maps each workspace requirement to implementation and final validation evidence.
