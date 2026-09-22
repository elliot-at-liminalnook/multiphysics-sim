# REST and shared annotation verification — 2026-09-20

The complete acceptance run is [run-20260920-complete](run-20260920-complete).
[API guide and examples](../../REST.md) describe launch, batching, images and annotations.

## Verified

35 focused tests passed: 6 transport/client, 4 inspection-library (including shared
annotation storage/history), 2 section renderer, 6 physical viewer and 17 schematic
viewer. Test and build logs are retained here. `verification.json` records input
and executable SHA-256 hashes and the toolchain. The shared worker and schematic
were built together; the physical host was built separately.

The actual two-process HTTP acceptance starts isolated headless viewers on free
ports, with a private selection session, annotation sidecar and analysis workspace.
It exercises shared multi-component notes, edits from both hosts, successful
deletion, cross-host undo/redo, stale-revision rejection, rejection of dangling
view links and foreign schematic layouts, component links, emphasis, paired saved
camera/layout profiles, physical view restoration and persistence. It also checks
ordered batches, native analysis undo, diagram edits, simulation reset/start/pause/
step, exact returned frame steps and run/generation identity, full-rate recording,
a real archived motor-trial evaluation, large study retrieval, immutable review
export, asynchronous failures and optimistic analysis-file conflicts.

All four source capture files are checked byte-for-byte unchanged. No CAD model
or hardware was edited or controlled. The desktop and audio remained untouched;
the original native viewers were left running. The tests' own processes were stopped.

The physical ECS test builds actual annotation cards and exercises link hover and
click using UI components without a window. Both executables compile their native
UI adapters. Native visual/desktop interaction was deliberately not performed.

## Images

All five PNGs were retrieved through HTTP, checked for their 1280×900 PNG dimensions,
and visually inspected off-screen. Paired JSON files preserve capture metadata.

- [Annotated physical assembly](run-20260920-complete/physical-annotated.png)
- [Clipped motor section](run-20260920-complete/physical-section.png)
- [Annotated schematic](run-20260920-complete/schematic-annotated.png)
- [Simulation measurements](run-20260920-complete/simulation-graphs.png)
- [Measured, archived prediction, baseline and candidate](run-20260920-complete/experiment-graphs.png)

The simulation graph uses 30 explicit steps after reset, rather than render-clock
integration. The experiment graph selects the evaluated trial; overlapping
baseline/candidate curves reflect the unchanged candidate configuration, not a
new calibration result. Original failed/no-motion observations are retained.
Physical geometry and its sections are illustrative display primitives, not CAD
solids. Measurement availability and exact frame identity are in each receipt.

## Regression and remaining boundaries

The initial large-response test reproduced truncated binary transfer on macOS.
Accepted sockets now explicitly use bounded blocking I/O, and the client checks
Content-Length. The 4 MiB JSON / 2 MiB binary regression passes; the compact
pre-fix failure and post-fix test log are retained.

The complete run's `review.workspace.json` intentionally contains external text:
it is the fixture proving that conflict detection preserves another writer's
work. Do not use it as a demo workspace. The discussion sidecar is valid JSON.
Earlier `run-20260920-a`, `-b`, and `-c` directories contain incomplete development
runs; `run-20260920-final` is an earlier passing run before choosing the evaluated
trial for the final graph. Use `run-20260920-complete` for acceptance receipts.

Text/link editing is available in the schematic inspector and both REST services.
Physical discussion cards support creation from selection, clickable links,
hover/group outlines, shared undo/redo and saved angles. Broader tutorial UI
sequencing, native visual review, CAD solid sections and large-model performance
benchmarking remain separate work. The CI acceptance step was added but remote CI
was not run in this session.
