# Live physical assembly — checkpoint 3b

2026-09-20. Implemented, tested, built and reviewed in the native GUI.

Launch: `./examples/systems-viewer/run-live.sh`.

## Implementation

The schematic remains the sole owner of the simulation worker. Its graph history
and the Bevy assembly consume the same timestamped observations. The assembly has
no runtime dependency and never advances physics. Rotation is a zero-order hold
of the actual angle sample; neither wall-clock integration nor extrapolation is
used. A white orientation spoke makes rotation visible on the symmetric wheel.

`sim-inspect::animation` owns source-bound rotation, temperature-color and scalar
readout bindings. The example JSON owns axes, pivots, displayed parts and color
ranges. It reuses the unchanged authored physical description and spatial
geometry. `sim-spatial` maps these contracts to Bevy transforms, materials and
readouts. Exploded offsets, selection, camera and visibility remain presentation
only. Temperature color is uniform on each bound part. The 293.15–297.15 K display
scale is clamped and illustrative, not a thermal safety limit or spatial field.
Motor current and heat-flow readouts retain their positive-into-component signs;
negative motor heat flow denotes heat leaving that component.

`sim-inspect::live` shares status and validates runtime descriptions, frames and
monotonic run/reset identity. Its native transport holds one latest snapshot in
each mailbox and atomically replaces a maximum 1 MiB file in the private ephemeral
selection-session directory. All transport I/O, serialization and receiving-side
validation run on background threads. Paused UI heartbeats continue; stale or
closed publishers are labeled while holding the last sample. Worker errors,
cancellation, restarting and unavailable quantities are visible. Restart/reset
clear the previous run's display values. The schematic subscribes to the union
of pinned graph channels and animation channels, so clearing graphs cannot stop
animation. This transport is a coalesced live preview, not a recording.

The first native review exposed a checkpoint-3a validation bug: the compiled
runtime legitimately adds state observations absent from the authored description
(e.g. rotor speed). Shared validation now permits those states and resolved
availability while preserving all authored physical definitions, quantities,
frames, signs and identities. The exact model capture binding remains enforced.
Pause also returns the final accepted frame so the assembly and graphs stop at
the worker's precise pause time rather than the last decimated display frame.

## Verification

53 distinct focused tests pass: 26 inspection, 4 spatial, 13 schematic, 8 shared
session and 2 process-worker tests. Worker frames match headless execution over
20 intervals. At 0.2 s, the animation's shaft angle and motor temperature agree
with their analytic solutions within 2e-6 rad and 2e-6 K, respectively. The worker
pause reply contains its exact final sample and subsequent status time is fixed.
No broader or calibrated sim-to-real accuracy claim is made.

All three native binaries build; the inspection crate checks on WASM and the
animation launch passes `--validate-only`. `verify.sh` retains the main commands;
inspection tests and subsequent UI polish checks have separate logs. The final
worker was checked again with the existing process-test harness. Both process
tests passed. The final polished pair was reopened and left running with a shaft
angle graph; `final-running.png` shows the delivered view.

The live launcher now builds the worker and schematic together, reusing the
schematic dependency feature set while retaining a separate Bevy build. A redundant
worker-only rebuild was stopped after consolidating that command; its partial
log is retained as `superseded-worker-build.log`. `final-gui.log` records the
successful final launch. The broader runtime-source identity boundary was not changed.

The actual native windows were opened before implementation and reviewed again
with live data. `gui-*.json` are atomic snapshots from that UI-owned run:
- Initial paused angle/speed zero and temperature 293.15 K; current/heat flow
  explicitly unavailable until the first accepted interval.
- Run changes angle, speed, current, signed heat flow and temperature; the wheel
  orientation and uniform component colors change visibly.
- Pause at 34.510 s holds the entire status and sample frame unchanged while the
  graph panel is resized. Both views show the same stopped time.
- One Step advances to 34.520 s, exactly one 0.01 s interval.
- Reset returns to time/angle zero and generation 1, with old graph history gone.
- Cancel reports Cancelled. Restart creates a different run ID, generation 0,
  paused at time zero.
- Clear graphs retains every animation subscription and advancing values.
- Closing the schematic releases the publisher; the assembly visibly reports
  disconnection and holds its last measurements (`disconnected.png`).
- Selecting the rotor in the schematic highlights its physical parts; selecting
  the supply mesh selects the schematic supply. The source selection receipt is
  retained in `gui-linked-selection.json`.

The previous schematic prompted to save on close; Save and close preserved its
analysis sidecar before replacement. CAD documents were not opened or changed.

The first inspection test run exposed floating-point rounding at a clamped color
endpoint. The mapping now returns its exact declared endpoint colors; the initial
failure is retained in `inspection-first-attempt.log`.

## Limits

Illustrative primitives remain display geometry, not CAD meshes or physical
collision geometry. No driver/LED component, fields, full recording/replay,
spatial history cursor, browser graphics or hardware calibration was added.
Frames can be dropped in the bounded preview and rapidly spinning shafts can
alias at display cadence. The source-fingerprint rebuild cost, full performance
distributions and broader numeric/browser acceptance gates remain open.

Final delivery hashes are in `binary-hashes.json`; `final-running.json` retains a
healthy running sample from the delivered processes. Existing warnings about
`ForceSlot::decision` and Bevy dependency `block` remain unchanged.
