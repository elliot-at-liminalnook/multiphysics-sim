# Live controls and reusable graphs — checkpoint 3a

2026-09-20. Implemented and built; native GUI review deferred at the user's
request not to control the desktop while they watch shows. No GUI was opened,
focused, restarted or captured during this increment. Earlier open viewers were
left running with their existing state.

Launch the new version with `./examples/systems-viewer/run-live.sh`. It builds at
low priority with two jobs, opens the linked assembly and schematic, and starts
the process-backed simulation paused. The assembly remains static; its linked
selection drives the schematic's connection graph picker.

## Verification

62 distinct tests plus one usage doctest passed:
- 21 graphics-independent inspection tests, including terminal-based channel
  discovery, sample-time semantics, gaps, bounded history and stale frame gates.
- 16 diagram/widget tests, including two standalone graphs sharing a cursor
  without a worker/model and finite axes for empty/unavailable data; one public
  `TimeGraph` usage doctest also compiled and passed.
- 8 shared-session tests and 2 native worker tests. The authored motor test
  validates the exact capture/description identity, compares every frame from
  20 steps with the headless session, proves that measured values change, checks
  accepted-stage samples, then runs, pauses and resets the process worker.
  Both process tests were rerun against the final rebuilt worker binary.
- 13 schematic tests, including actual headless pointer events selecting a
  connection graph option, pinned selection across navigation, reset/stale-frame
  rejection, and preservation of existing analysis workflows.
- 2 spatial library selection tests.

All three native binaries build. The new live-launch CLI path passes
`--validate-only` without creating a window. The graphics-free inspection crate
checks for `wasm32-unknown-unknown`; this is not a browser GUI acceptance result.
Changed/new Rust implementation files pass targeted rustfmt checks, shell
launchers pass `bash -n`, and tracked changes pass `git diff --check`.

Logs are retained here. `inspection-and-diagram-tests.log` includes the earlier
14 diagram tests; `reusable-graph-tests.log` repeats those and adds the two new
widget tests and doctest. These repeats are not double-counted above. Existing
warnings about `ForceSlot::decision` and the Bevy dependency `block` are not new
warnings from this change.

## Contracts and limits

`sim-inspect::plot` owns reusable source-based channel discovery and validated
bounded history. `sim-diagram::plot::TimeGraph` accepts points/gaps, units,
optional time bounds, height, color and a caller-owned shared cursor. It exposes
chart geometry and the nearest actual sample. It has no worker, selection or
model dependency. The schematic only adapts its live session to these components.

The model, authored description and presentation JSON match the preceding
checkpoint's SHA-256 hashes exactly. `input-hashes.json` adds the new live capture,
which retains the exact ModelWorld, source-bound identity map, seed and solver
configuration. Source slots are checked against the captured model hash before
use, and the worker reconstructs and verifies the authored description.

Up to eight graphs retain 2,000 received display frames. Transport coalesces
updates; this is a decimated live preview, not a full recording or peak detector.
Every terminal retains its own quantity and sign convention. Gaps are not filled
with zeros; accepted-stage values use actual sample times. Reset clears history,
and cancel kills the worker while retaining already received graph data.

Native layout/interaction review, end-to-end GUI latency, 3D animation and
indicators, full recording/export/replay, a spatial playback cursor, CAD meshes,
resolved fields and final accuracy/performance gates remain pending.
