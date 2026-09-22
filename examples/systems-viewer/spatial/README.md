# Spatial assembly and schematic — GUI checkpoints 1–2

Run from any directory:

```sh
/Users/elliot/physics-simulator/examples/systems-viewer/run-spatial.sh
```

Open both linked windows (builds at low priority, two jobs):

```sh
/Users/elliot/physics-simulator/examples/systems-viewer/run-linked.sh
```

Both windows read the same sealed `motor-thermal.description.json`. Select a
component in either view, a schematic port/wire, or an assembly connection hub
(or connection button). The other window highlights the corresponding source
identities. Gold connection guides show complete multi-terminal nets. Collapsed
schematic groups and bundles keep every constituent source ID.

The linked launch uses compact windows. Arrange them side by side; **Parts** in
the assembly and **Browser** in the schematic restore the component lists.
**Clear** / Escape in the assembly, or the schematic's **Clear shared selection**,
clears both. Selecting a hidden part from the schematic reveals it in the assembly.
Selection does not arrange the diagram, expand groups, change pins, or edit notes.
If a source is outside the current focused diagram, **Reveal in diagram** is an
explicit navigation action. Components without geometry remain inspectable.

Click a part or a component-list button to inspect its model parameters and
actual terminal connections. Right-drag orbits, shift-right-drag (or middle-drag) pans, the wheel zooms
in the assembly or scrolls the inspector. F fits, E toggles exploded presentation,
C toggles topology guides, and number keys select components in list order.
Selecting a hidden component reveals it. Exploding/hiding never changes the model.
Gold means selected; part colors do not represent temperature.

The model is the existing seven-component `motor_composite` numerical baseline:
motor, source, ground, rotor inertia, thermal capacitance, cooling conductance,
and ambient boundary. Shapes are **illustrative presentation primitives**, with
explicit meters and a right-handed Y-up display frame. They are not a CAD export,
measured dimensions, collision geometry or a source of inferred physics. A
cooling fin shape represents the lumped conductance, not a resolved heat field.
There is no driver board or LED in this fixture. The spatial view remains static;
use the live launcher below for process-backed simulation and graphs.

## Reproduction and identity

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_INCREMENTAL=false \
  cargo run --locked -p sim-runtime --example export_spatial_preview
cargo test --locked -p sim-inspect --test spatial
examples/systems-viewer/run-spatial.sh --validate-only
```

The exporter reuses `sim-inspect::model::describe` and the shared behavior
registry; it supplies explicit example-owned component identities and the hash
of the retained model serialization. It does not compile or advance the model.
It exports the description, source ModelWorld, a source-bound live worker capture,
and refreshes the existing
presentation's description binding. No geometry is derived from physical values.
The fixture is retained under `examples/`, not ignored experiment output.

`sim-inspect::spatial` validates source binding, geometry, IDs, units and frame
before Bevy opens a window. `SpatialViewState::apply` is the shared command path
for selection, visibility, exploded presentation and connection guides; the CLI
uses it too. No graphics dependencies enter the simulation/inspection contracts.
The `sim-spatial` library consumes any validated description and primitive sidecar;
there are no motor-specific branches in its renderer.

CLI options: `--description <file> --spatial <file>`, `--select <component-id>`,
`--exploded`, `--connections`, `--validate-only`, `--schematic`, `--compact`,
`--selection-link <session-directory>`. A custom input requires both
files. Scene loading currently happens before window creation; interactive large
CAD/mesh loading and cancellation are a later checkpoint.

## Local selection session

This checkpoint uses two native windows, with a Unix local session rather than
merging Bevy and egui renderer versions. `sim-inspect::selection` defines the
shared graphics-free target; its native adapter atomically replaces a bounded
record in a private temporary directory. Background workers handle file I/O,
write ordering and peer liveness; UI polling does not wait on disk. This is
transient selection state, separate from CAD files and analysis sidecars.

Closing either window leaves the other usable and changes its link status to
waiting. The session directory is printed at startup and remains available for
reattachment with `--selection-link`. One assembly and one schematic may join a
session. A different description, unknown ID, duplicate role, corrupt record or
sequence regression is rejected; the last valid UI remains available. There is
no network listener, daemon, model mutation, or physics stepping. Temporary
session directories contain only a few small files and are not replay records.

## Next GUI checkpoints

Connect worker controls and real measurements,
then add motion, LED state, thermal overlays and synchronized plots/replay. CAD
meshes and distributed field rendering follow. See `systems-viewer-plan.md` for
the full checklist and acceptance gates. This preview does not claim live
physics, browser support, CAD integration or final performance acceptance.


## Live controls and connection graphs — checkpoint 3a

```sh
./examples/systems-viewer/run-live.sh
```

This builds the shared process worker and both hosts with two low-priority jobs.
It opens the linked views and starts the simulation **paused**. Select a connection
in either window, then choose measurements under **Graph over time** in the
schematic inspector. **Run**, **Pause**, **Step**, **Reset** and **Cancel** control
the worker. Cancel kills the process independently of a blocked solver and keeps
already-received graphs. Restart worker starts a fresh run. Reset clears graph
history and rebuilds the captured model while preserving selected graph IDs.

Up to eight graphs remain pinned while selection changes. Each graph identifies
its component/terminal, quantity, canonical unit and declared sign convention.
There is no single fabricated current/heat-flow value for a multi-terminal net.
Unavailable channels show their reason. X is simulation time in seconds; solver
stage observations use their actual evaluation time and are labeled separately
from endpoint values. Hovering a graph moves a common time cursor; readouts use
the nearest actual sample, with its timestamp, rather than interpolation.

The host retains at most 2,000 received display frames, with discontinuities for
missing/unavailable data. Transport coalesces display updates, so these curves are
**decimated live previews**, not a lossless recording, peak detector or solver
accuracy assessment. Full recording/export/replay and a cursor shared with the
3D assembly are later work. Assembly geometry is still static; no temperature
field or motion is inferred from these plots.

The retained `motor-thermal.live.json` holds the exact ModelWorld, seed 71,
0.01 s output interval, requested implicit-midpoint integrator and an identity
map bound to the canonical model hash and sealed authored description. The
worker validates that binding before compilation. Runtime availability has its
own description identity; authoring/selection identity is preserved. Generic
captures can use the same contract without label-based GUI adapters.

Schematic only (after building):

```sh
target/debug/sim-viewer \
  --description examples/systems-viewer/spatial/motor-thermal.description.json \
  --live examples/systems-viewer/spatial/motor-thermal.live.json
```

The desktop was left untouched during implementation at the user's request.
Native visual review of this increment remains pending; headless UI interaction
and worker/headless numerical comparisons are recorded in the implementation log.

### Reusing the graph in another panel

`sim-inspect::plot` provides graphics-free channel discovery, labels/units and
bounded validated history. `sim-diagram::plot::TimeGraph` is a public egui widget
accepting a slice of timestamped points/gaps, units, optional time bounds, height
and color. It has no live-worker or selection dependency. Pass the same
caller-owned cursor to multiple instances to link their inspection time. Its
response exposes chart geometry and the nearest actual sample for other panels.
The widget has a standalone usage example and tests using synthetic data, without
constructing a simulation or viewer. `sim-viewer::live_ui` only adapts worker
samples and the current selection to these shared components.
