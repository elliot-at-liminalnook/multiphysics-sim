# Local REST control for Rust systems viewers

Both native hosts use the `sim-api` transport. The physical assembly defaults to
`http://127.0.0.1:8421`; the schematic defaults to `http://127.0.0.1:8422`.
`--api-port 0` chooses a free port and prints its address. Bind failures are
reported, never silently redirected to another process. Existing running binaries
must be restarted to gain the new service; their windows need not be disturbed
while building or testing headlessly.

## Build and launch

```sh
# Build the worker and schematic with the same dependency features.
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_INCREMENTAL=false \
  ~/.cargo/bin/cargo build --locked -j 2 -p sim-runtime -p sim-viewer -p sim-api \
  --bin sim-system-worker --bin sim-viewer --bin simctl
# Keep Bevy's feature graph separate.
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_INCREMENTAL=false \
  ~/.cargo/bin/cargo build --locked -j 2 -p sim-spatial
```

Normal viewer launches expose REST automatically. To work entirely without windows:

```sh
target/debug/sim-spatial --headless --api-port 8421
target/debug/sim-viewer \
  --description examples/systems-viewer/spatial/motor-thermal.description.json \
  --live examples/systems-viewer/spatial/motor-thermal.live.json \
  --workspace /tmp/rest-review.workspace.json --headless --api-port 8422
```

The physical host subscribes to live measurements when joined to the same
`--selection-link` directory with its animation bindings. It never creates a
second simulation or derives physical parameters from display geometry. A
standalone physical service is a static assembly with measured channels unavailable.

## Protocol

| Endpoint | Meaning |
|---|---|
| `GET /` | Service identity, PID, protocol version and actual address |
| `GET /v1/capabilities` | Available commands, argument examples and limits |
| `GET /v1/state` | Latest application-owned state snapshot (up to 100 ms old) |
| `GET /v1/description` | Typed source components, ports, nets, observables, units and diagnostics |
| `GET /v1/measurements` | Latest actual measurements; no invented samples |
| `POST /v1/commands` | Submit one `{command, args}` operation |
| `POST /v1/batch` | Submit `{commands: [...], stop_on_error: true}` |
| `GET /v1/jobs/{id}` | Ordered per-command results and completion state |
| `GET /v1/jobs` | Retained jobs |
| `DELETE /v1/jobs/{id}` | Request cooperative cancellation; skip unstarted commands |
| `GET /v1/spatial` | Physical viewer's source-bound display geometry |
| `GET /v1/workspace` | Schematic's analysis document |
| `GET /v1/annotations` | Shared discussion document, revisions, view links and undo/redo history |
| `GET /v1/images` | Retained image artifacts |
| `GET /v1/images/{id}.png` | PNG bytes |
| `GET /v1/images/{id}` | Capture metadata, source/frame identities and render options |
| `GET /v1/experiment-capabilities` | Experiment operations and editable fields |

Mutations require `Content-Type: application/json`. HTTP 202 means **accepted**,
not applied. Poll the returned job URL until `succeeded`, `failed` or `cancelled`.
Each result contains the command index, name, `ok`, and either `value` or `error`.
Unknown commands, unknown fields and invalid source identities fail explicitly.
A batch is ordered, **not a transaction**: successful earlier edits are retained
if a later edit fails. By default remaining commands are skipped after an error;
`stop_on_error:false` collects independent failures and continues.

A pending simulation operation finishes only when its worker replies. Therefore
`pause → step → measurements` observes the completed step. Cancelling a batch
with an in-flight simulation request stops that worker before declaring the batch
cancelled. Use `simulation_restart` to construct it again from the capture.
Cancellation does not undo prior edits or finalized outputs.

Experiment operations use their existing cancellable background jobs. `open`,
`evaluate`, `refine` and `save` return **task acceptance** with `busy:true`; poll
`experiments / state` until `busy:false`, then inspect `error` and the retained
study. Use `experiments / cancel` to stop the experiment task. These jobs are
separate from the short REST batch which started them.

The queue holds 64 waiting batches, with at most 128 commands each, and retains
256 jobs in memory. Completed jobs are evicted oldest first. Copy required
receipts/evidence before eviction or application exit. Nothing automatically
retries a mutation. A client timeout reports the job URL and leaves its state
inspectable. Transport accepts up to 1 MiB request bodies; larger scientific
artifacts are opened by file path. The Rust client caps responses at 64 MiB.

## One tool call across both viewers

Write `actions.json`:

```json
{
  "batches": [
    {
      "url": "http://127.0.0.1:8421",
      "commands": [
        {"command": "display", "args": {"action": {"kind": "set_exploded", "enabled": true}}},
        {"command": "display", "args": {"action": {"kind": "set_connections", "enabled": true}}},
        {"command": "state"}
      ]
    },
    {
      "url": "http://127.0.0.1:8422",
      "commands": [
        {"command": "simulation", "args": {"action": {"command": "pause"}}},
        {"command": "simulation", "args": {"action": {"command": "step"}}},
        {"command": "measurements"}
      ]
    }
  ]
}
```

Then invoke once:

```sh
target/debug/simctl --plan actions.json
```

The client waits for each batch and prints JSON receipts. It exits nonzero on
failure or cancellation. Plans stop after a failed batch by default; the outer
`stop_on_error:false` continues to other services. There is no cross-process
transaction. Single-service usage: `simctl URL batch.json`. Inspection:
`simctl get http://127.0.0.1:8422 /v1/capabilities`.

## Capability coverage

Physical: exact component/port/net selection; hiding selected geometry and showing
all; exploded layout and connections; camera orbit, pan and zoom through absolute
camera state (meters/radians); fit; parts panel and compact layout; geometry,
provenance, animation bindings and last measured values. Read-only measurements
retain sample time and availability. No endpoint writes CAD or advances physics.

Schematic: source selection and model switching; load captured descriptions;
focus/back/overview and group expansion; full validated diagram positions, pins,
camera and zoom; asynchronous arrange/cancel; fit; domain emphasis and search;
analysis annotations, renaming, groups, undo/redo; full analysis replacement;
sidecar open/save-as with the existing conflict checks; simulation capture
load/unload/start/pause/step/reset/cancel/restart; graph subscriptions and cursor;
compiled runtime descriptions; bounded display history; full-rate begin/take recording through the Rust worker.
`begin_recording` is limited to 10,000 frames over REST. `take_recording` returns
the runtime's recording, including configuration, seeds and completion metadata.

Experiment review: opening archives, switching studies, filters and selection,
model and controller configuration, analysis notes, retained evaluation decisions,
plot cursor/zoom/series, selected/filtered trials, held-out evaluations by explicit
IDs, cancellation, immutable JSON/HTML output. `configure` changes only advertised
editable fields and validates the full study; original observations and retained
results cannot be replaced. Refinement actions use the existing Rust paths for
controller simulation, sensitivity, fits, robustness, imported recordings,
FPGA review/design/plan export, electrical comparison and explicit CAD proposals
or accepted **new** CAD artifacts. These endpoints do not drive hardware.

OS window management and scrolling are not domain commands. `fit` is a native
viewport request; headless callers should set explicit camera state. Open annotation/group drafts are exposed in `state`. Use `drafts` to edit or explicitly clear them, and `apply_drafts` to commit them. Other REST analysis edits refuse to overwrite an in-progress draft.

## Shared discussions and saved inspection views

Both hosts accept `--annotations PATH`; point them at the same sidecar. By
default the path is `DESCRIPTION_PATH.annotations.json`. Notes contain stable
source component/port/net identities, not screen coordinates. Multiple notes can
refer to overlapping component groups. The sidecar includes model identity,
revision, saved views and bounded shared undo/redo history; it is not a physics input.

Both hosts accept the same `annotations` command. Its `action.operation` is:

- `document`: read the current document.
- `edit`: apply `change`, optionally with `expected_revision` for optimistic concurrency.
- `select_note {id}`: select every referenced component.
- `follow_link {note,index}`: select the link target or restore its saved view.
- `emphasize {target}`: transient emphasis without changing selection or camera; clear with `{"kind":"none"}`.
- `save_view {id,label}`: capture the current host's camera/layout and selection.
- `restore_view {id}`: request both connected hosts to restore their saved profile.

`edit.change.operation` supports `put_note {note}`, `delete_note {id}`,
`put_view {view}`, `delete_view {id}`, `undo` and `redo`. Save a view under the same
ID from each host to associate a physical camera with a schematic layout.
Updating one host's profile preserves the other. A referenced view cannot be
deleted until its links are removed. Undo/redo is shared and persisted (last 32
edits); navigation and hover do not consume edit history.

Example batch for either host:

```json
{"commands":[
  {"command":"annotations","args":{"action":{"operation":"save_view","id":"drive","label":"Drive inspection"}}},
  {"command":"annotations","args":{"action":{"operation":"edit","change":{"operation":"put_note","note":{
    "id":"drive-discussion","label":"Motor and rotor","text":"Inspect the drive and its load together.",
    "targets":{"kind":"components","ids":["example/motor-thermal/motor","example/motor-thermal/rotor"]},
    "color":[26,135,145],
    "links":[
      {"label":"Motor","target":{"kind":"selection","target":{"kind":"components","ids":["example/motor-thermal/motor"]}}},
      {"label":"Inspection view","target":{"kind":"view","id":"drive"}}
    ]
  }}}}},
  {"command":"annotations","args":{"action":{"operation":"select_note","id":"drive-discussion"}}}
]}
```

Pass the revision returned by `document` as `expected_revision` when editing
existing text. Conflicts fail without writing. Independent edits without an
expected revision merge against the latest file under an advisory lock; writes
are atomic and run off the UI thread. Both hosts observe sidecar updates within
approximately 100 ms. A successful edit result is durable; a subsequent host's
published snapshot may still need one refresh interval.

The schematic inspector edits labels, text, component links and view links.
Both viewers display colored discussion cards with clickable/hoverable links,
matching group outlines, selection and shared undo/redo. Physical Shift-click
toggles component membership; its inspector can create a discussion from that
selection and save/restore inspection angles. Text editing is currently in the
schematic inspector or either REST service. View links restore display state;
they do not rewind simulation time. As with ordinary selection, referenced
components are revealed when a view link is followed. Schematic layouts are
validated against the active analysis workspace; deleted analysis groups cannot
be restored implicitly from a view link. Broader tutorial sequencing and OS-level
UI control are a separate increment.

## Off-screen images

`render` captures the host's current data and rasterizes it on a background CPU
thread. The completed result contains `url`, `metadata_url`, dimensions/options
and source identity. Download the PNG before the bounded artifact cache evicts
it (16 images / 64 MiB total, 32 MiB per image). Requests support 400–2400 pixels
wide, 300–2400 high, at most four million pixels. Rendering never opens a window,
changes the camera, or advances physics. Images are rendered inspection outputs,
not desktop screenshots; interactive links remain available in their metadata.

Physical command examples:

```json
{"command":"render","args":{"options":{"view":"isometric","size":{"width":1280,"height":900}}}}
{"command":"render","args":{"options":{"view":"front","parts":["example/motor-thermal/motor"],"section":{"axis":"x","offset":0.0,"keep_positive":false}}}}
```

Views are `current`, `isometric`, `front`, `top`, `right`. Orthographic framing
fits the requested parts; `current` uses the current orbit orientation. Optional
`include_hidden`, `exploded` and `connections` affect only this image. Sections
clip actual display primitives at an SI-meter plane and cap cut surfaces in
orange. These primitives are explicitly illustrative, **not full CAD solid
sections or collision geometry**. Physical images retain the measured frame's
run, generation, sequence, step and time when available, plus annotation links.

Schematic command examples:

```json
{"command":"render","args":{"request":{"kind":"schematic","options":{"fit":true}}}}
{"command":"render","args":{"request":{"kind":"graphs","options":{"time_range":[0.0,0.5]}}}}
{"command":"render","args":{"request":{"kind":"experiment"}}}
```

Schematic images use the current routed layout and discussion groups. Wait for
`layout_pending:false` before rendering. Graph images use actual observation
history (or the selected experiment trial), separate units and unavailable-value
gaps. The experiment legend’s Reference curve is the archive’s retained predicted
trace; baseline/candidate curves appear when that selected trial has an evaluation.
Choose up to eight observable IDs with `options.observables`; image height
must allow at least 110 pixels per graph plus a 100-pixel header. `cursor` and
`time_range` are display-only. The Rust client exposes `client::image` to retrieve
PNG bytes; `curl` can also download the returned URL.

## Extension and ownership

`sim-api` depends only on serde and the standard library. A bounded HTTP worker
pool parses requests and queues jobs. Only `Server::poll` on the application owner
thread calls domain adapters. No HTTP thread mutates a Bevy world, egui state or
physics state. Pending commands retain continuation data (the worker request ID)
instead of blocking rendering. Native and headless hosts call the same adapters. Published GET snapshots refresh at most ten times a second; static descriptions serialize only when source identity changes. Use a `state` command inside a batch for a read ordered immediately after earlier commands.

Add a typed command variant with `deny_unknown_fields`, advertise it in the
capability catalog, delegate to a shared validated operation, and test its
postconditions. Keep simulation in `SystemSession`/`sim-system-worker`; keep
analysis changes in `Journal`/`WorkspaceFile`; keep source selection in
`SelectionTarget`. Long operations should use existing workers and expose their
own progress/cancellation. Do not duplicate solvers in REST handlers.

The service trusts local processes and binds IPv4 loopback only. It rejects
nonlocal Host headers, Origin-bearing browser requests, transfer-encoding,
duplicate headers and oversized requests. It supplies no CORS permission and is
not a remotely authenticated deployment interface.

## Repeatable headless acceptance

After the build commands above:

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_INCREMENTAL=false \
  ~/.cargo/bin/cargo run --locked -p sim-api --example viewer_rest_smoke -- /tmp/viewer-rest-check
```

Use a new evidence directory for each run. The harness starts only its own
headless processes on ephemeral ports and stops them on exit. It checks commands
against real viewer state, runtime step/time/recording results, annotation undo,
layout edits, an actual archived motor-trial evaluation, immutable experiment
export, asynchronous errors, and optimistic workspace conflicts. It retains JSON
receipts, annotated physical/schematic PNGs, section and graph images, and process logs. It checks shared annotation edits, cross-host view links, undo/redo, stale-revision rejection and persistence, and checks that the captured physical description is
unchanged. It does not move, open or close native user windows.
