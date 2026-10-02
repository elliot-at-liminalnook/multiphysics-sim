# Bounded CAD parity corpus (T44)

These manifests and scenarios are written and source-reviewed, **unexecuted**.
They are input contracts, not result receipts or signed migration evidence.
Original archives, historical backups and measured values are never edited.

| Manifest | Real source inspected | Coverage |
|---|---|---|
| assembly.json | parametric-quadruped/assembly.rcad: 24 nodes, 4 hinges, 3 embedded definitions | Rename/undo/redo; component parameter rebuild, stale start, stale commit and cancellation |
| wheeled.json | wheeled-robot/baseline/robot.rcad: 12 nodes, 3 continuous joints | Mass/inertia, motor/joint physical export semantics and guarded revision rejection |
| printable.json | hx30hm/fixture-draft/print-kit.rcad: 4 bodies | Printable solid geometry/topology/tessellation and physical metadata; generator digest recorded without execution |
| linkage.json | quadruped-parametric/model/robot.rcad: 262 nodes, 105 joints, 23 embedded definitions | Closed-linkage reference pose, validated prior continuation, stale revision, ephemeral pinned CAD capture and missing captured-run rejection |

SHA-256 digests identify complete existing archive bytes, including embedded
B-rep and component definitions. `Document.load` restores embedded occurrences;
none of these sources has an operational external node.source/image reference.
Component definition provenance paths identify historical backups; they are
retained as provenance, never followed, rewritten or treated as current imports.
The printable generator is a declared hashed dependency for reproducibility,
not an operation. Future external operational dependencies must be declared and
hashed before an adapter may consume them; unsafe paths/symlinks are refused.

The camera turntable (37 nodes/13 joints), worm set (127 nodes/no joints), and
full-robot baseline (261 nodes/105 joints) were inspected but omitted to keep
this first corpus bounded: the selected sources cover their assembly/solid/joint
families while adding embedded parameter definitions and linkage continuation.
This is representative coverage, not whole-ledger coverage or whole-corpus parity.

Comparison policy is explicit per field. Current same-authority comparisons
require zero numeric discrepancy: identical reference routines on identical
archive bytes justify a deterministic service-contract expectation, not a claim
of numerical accuracy. Any widening needs executed discrepancy evidence and a
reviewed justification. The reference pose solver independently refuses closure
residual above 0.02 mm (`cad/robocad/pose.py`); our comparison tolerance does not
replace that guard. Tessellation compares a bounded symmetric sampled-vertex
Hausdorff metric in CAD world mm, with at most 4096 vertices per side and separate
exact topology diagnostics. It proves neither surface Hausdorff distance nor
orientation, manifoldness or independent kernel accuracy.

Raw export source paths/timestamps are retained and deliberately differ between
isolated adapters. That declared difference blocks migration gates. Missing
provenance/uncertainty stays missing; no measured value is supplied by the harness.
Captured/live identity includes an ephemeral pinned CAD snapshot reviewed after a live edit and a paired missing-capture refusal, but this corpus
contains no recorded experiment bundle: positive captured replay remains a gap.
Sketch/direct tools, printing algorithms/strength/splits, belts, calibration,
measured uncertainty, rendering and independent Rust derivations/kernel execution
are unrepresented. Explicit unsupported observations cannot pass a gate.

See [the harness guide](../../docs/cad-parity-harness.md) for module ownership,
future launch instructions, report semantics and the source trace. No build,
fixture, scenario, export, viewer or screenshot was executed in this batch.
