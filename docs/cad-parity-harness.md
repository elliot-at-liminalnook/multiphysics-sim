# Shared CAD parity harness — T44

T44.1–T44.4 implement §9 phase 2 tooling by source review. Contracts, adapters,
corpus scenarios and fixtures are **written, uncompiled and unexecuted**. No
scenario, build, test, export, viewer, screenshot or hardware operation occurred.
This document is not an executed parity receipt or a signed user checklist.
T43 was accepted by source review at `4d2725f2`/`e6f6ed10` (call-0342), without
compilation or execution. RoboCAD and browser compatibility remain available.

## Shared module map and contract

`sim_runtime::cad_parity` is graphics-independent. It adds no viewer or UI logic.

| Owner / path | Responsibility |
|---|---|
| `crates/sim-runtime/src/cad_parity/contract.rs` | Version 1 manifest/scenario/operation inputs, expected outcomes, observation numerical owner, source and adapter identity, units/frame/provenance/uncertainty, policy/tolerance, receipts, reports and adapter trait |
| `compare.rs`, `gates.rs` | Reusable path-specific comparisons and fail-closed service/independent derivation/independent kernel gates |
| `native.rs`, `native_observations.rs` | Independent typed Rust CadClient dispatch, bounded component polling, uncertain mutation handling and actual-result expansion |
| `runner.rs`, `isolation.rs`, `process.rs` | Paired owned copies, hashes/closure preflight, existing CAD startup polling and headless child ownership/cancellation/reaping |
| `publication.rs`, `owned_path.rs`, `src/bin/cad_parity.rs` | Default not-run planning, future opt-in execution, atomic non-overwriting report publication |
| `crates/sim-runtime/src/cad_client/parity.rs` | Typed access to two narrowly read-only observation routes |
| `cad/robocad/parity_operations.py` | Direct authoritative Ops/document/component/motion dispatch; no native REST dispatch oracle |
| `parity_observations.py`, `parity_reference.py`, `parity_paths.py` | Shared canonical observation serialization, isolated model validation and direct reference receipts |
| `examples/cad-parity/` | Four existing real models identified by complete source/dependency SHA-256, units/frame/schema and explicit field policies |

Unknown contract fields and unsupported schema versions are refused. Operations
contain inputs, not expected-response JSON. A `Passed` receipt means an executed
operation matched its declared outcome; a rejected command must have an observed
`rejected` outcome. Preparation failure, process crash or missing response is not
an authoritative rejection. Component commit refusals require the authoritative
revision-guard diagnostic plus captured/live identity mismatch. Every receipt
retains process document ID/revision separately from durable archive SHA-256;
a mismatch against the manifest is never normalized away.

Observations distinguish Present, Missing, Unsupported and Invalid/non-finite.
Metadata comparisons retain explicit units, frames, numerical owners, provenance
and uncertainty. A future independent adapter may declare an explicit reference/native owner pair in policy; both original owner names remain in receipts. The policy does not fill missing CAD provenance. Numeric
acceptance is inclusive `|native-reference| <= absolute + relative*|reference|`.
At a zero reference only absolute tolerance applies; relative error is absent
unless both values are equal. Arithmetic overflow never becomes zero error or
JSON infinity. Numeric JSON integers outside the lossless binary64 range are
refused; exact comparison keeps integer identity. Each quantity policy contains
its tolerance justification. Current same-authority policies require zero error;
that is a stringent deterministic service expectation, not measured accuracy.

Point sets use bounded symmetric sampled-vertex Hausdorff distance in the declared
CAD-world frame, with an explicit 4096-point ceiling and separate exact topology
policy. This is not a continuous-surface Hausdorff measure or proof of manifoldness.
Topology diagnostics and tessellation face/triangle attribution are retained.
Surface inertia has mm⁴ units; volume inertia mm⁵; unavailable physical mass for
non-volume geometry is unsupported, never inferred from surface integrals.

## Isolation, cancellation and reports

Source/dependency hashes are checked before separate private temporary directories
are created for reference/native copies. Only declared files are copied. Relative
paths, regular files, symlink ancestors, ZIP entry bounds, embedded identity/schema
and operational dependency closure are validated. Embedded components remain the
CAD source; historical backup paths are provenance metadata and are never followed.
Models and caller-owned outputs are never overwritten. Rust source/copy/log/report
I/O and Python observation/report I/O retain directory descriptors and use
relative no-follow operations, so pathname replacement cannot redirect these
accesses. Python/OCCT subprocess loading still uses validated paths inside private
owned directories; this local tool assumes no adversarial same-UID interference
with an active subprocess workspace and is not an adversarial CAD sandbox.

The adapter trait is synchronous and takes a durable cancellation callback.
Long work uses reusable headless process ownership and the existing CAD service
startup polling, not feature threads. Unix children start in an owned process
group. Exit observation uses `waitid(WNOWAIT)` so the leader remains unreaped
through bounded TERM grace and the final group KILL. Only then may reaping release
its identity; repeated stop/drop cannot signal the released numeric group.
This requires exclusive child-wait ownership and normal SIGCHLD disposition;
unexpected ownership loss refuses further signalling. Descendants that escape
the owned process group are outside this mechanism's containment guarantee.
Component cancellation is requested once over the network and polled to acknowledgment.
Unknown mutating network outcomes stop dispatch and are never retried. Malformed
observations preserve the uncertainty of an earlier mutation. Non-Unix execution/publication is refused because the owned-directory capability is unavailable. Temporary workspaces/logs remain inspectable after a
future run; this batch created no execution workspaces or receipts.

Reports retain schema/code/model identity, adapter implementation/kernel/derivation
identity, coverage, per-operation observations and statuses. Compiled Rust source
identity and the current reference source fingerprint are distinct from a caller
label. Executed observations also record actual Python, OCP, RoboCAD, NumPy, SciPy and kernel-class identity; unavailable dependency versions are unsupported evidence. Planning receipts are NotRun and contain no execution timestamps. Startup
failure also has no fictional per-operation execution timestamp. Executed pass,
failed comparison, unsupported, deliberate difference, incomplete, uncertain and
cancelled statuses remain distinct.

After bounded reference shutdown, the runner always inspects the owned reference
report, including cancellation, timeout, nonzero exit and cleanup failure.
Deserializable receipts retain their original identities, statuses, timestamps,
revisions, observations and uncertain outcomes. Validation and shutdown issues
are recorded separately in `AdapterRun.execution_issues`, an optional additive
version-1 field (omission means no reported issue), and block every migration
gate. A partial receipt set remains partial; only absent or malformed output
requires synthetic timestamp-free unavailable-evidence receipts. Cooperative
SIGTERM publication is attempted, not guaranteed: forced termination or failed
shutdown may leave no usable report. These recovery and process-ordering fixtures
are written but unexecuted. A bounded reap failure retains the waitable identity
without further signals; eventual reaping is not guaranteed after ownership is
lost or the final owner is dropped.

A newly owned output directory is required. JSON and readable text are written and
fsynced before hard-link publication; existing names are never replaced. The
`SCENARIO.complete` marker is the atomic completion boundary for the whole bundle.
Readers must ignore bundles without it; a partial failed publication remains
incomplete evidence. Directory sync follows publication. Raw source paths/export
timestamps remain recorded as deliberate differences rather than being scrubbed.

Gate aggregation recomputes comparisons and requires nonempty operations,
policies and required coverage, ordered complete paired receipts, actual outcomes,
executed timestamps, identities and passing observations. Unsupported, missing,
non-finite, incomplete, cancelled, uncertain and unresolved deliberate differences
block gates. Per-scenario gates qualify only declared coverage, never the whole
ledger; whole-corpus migration and the user's unsigned checklist remain separate.
Both current adapters use Python/OCCT and RoboCAD physical derivations. Their
agreement cannot satisfy an independent numerical derivation/kernel gate, including
future Rust OCCT bindings that still share the reference numerical kernel.

## Future launch (not executed in T44)

After a later batch authorizes builds/execution, use the existing RoboCAD venv
and a built `cad_parity` binary (`sim-runtime` bin target; normally `target/debug/cad_parity` or `target/release/cad_parity`). Python/OCCT remain required; these paths require
no Qt window, screenshots or acceptance.py main. Some future declared workflow
families also need existing registry/experiment executables; nothing here ports
them. The current corpus does not invoke rendering, ffmpeg or hardware.

```sh
# Default planning writes honest NotRun reports; no adapter process is started.
cad_parity /abs/repo /abs/repo/examples/cad-parity/wheeled.json wheeled-contracts /abs/new-report-dir reviewed-code-label
# Explicit later execution, isolated real-model copies only:
cad_parity /abs/repo /abs/repo/examples/cad-parity/wheeled.json wheeled-contracts /abs/different-new-report-dir reviewed-code-label --execute --cancel-file /abs/owned-parent/cancel
```

The output directory must not exist and its parent must already exist. Creating
the cancellation token requests cancellation; the harness only observes it.
No command above was run. The reference CLI is an internal owned-workspace
adapter, not an alternate viewer or an entry point for editing original models.

## Corpus coverage and source trace

The [corpus inventory](../examples/cad-parity/README.md) records archive inspection,
selection rationale, declared tolerances, units/frames and coverage gaps. The four
models span embedded parameters, joints/motors, printable solids and closed linkage.
An ephemeral immutable CAD snapshot is reviewed after a live edit; positive
recorded-result replay remains absent. Sketch/direct-tool UI, print algorithms,
strength, belts, calibration and independent replacements remain unrepresented.
No whole-ledger parity claim follows from this bounded corpus.

The complete wheeled scenario trace is recorded below with path:line references
for the final source. It reaches real isolated operations and actual observations,
not fixture playback, then refuses gates because execution is absent in planning
and deliberate export metadata differences remain unresolved even after execution.

1. `examples/cad-parity/wheeled.json:5` names the original source, SHA-256,
   durable document ID, schema, mm and CAD-world; `:42` starts Observe → Rename
   → Undo → Redo → stale ConfigureRobot → in-memory Physical operations. Policies
   start at `:50`; raw export source metadata has a declared difference.
2. `crates/sim-runtime/src/bin/cad_parity.rs:49` calls shared `runner::paired`
   (`crates/sim-runtime/src/cad_parity/runner.rs:272`). The default path constructs
   timestamp-free NotRun receipts (`runner.rs:20`). Opt-in execution creates two
   separate owned copies (`isolation.rs:48`), whose bytes are verified through
   retained no-follow directory handles (`owned_path.rs:105`). No original save
   operation is offered by the harness.
3. Before loading, `runner.rs:145` invokes the bounded reference validator;
   `cad/robocad/parity_reference.py:29` checks ownership, hashes, ZIP/schema,
   units, frames and dependency closure. The direct reference then loads verified
   bytes (`parity_reference.py:117`) and calls `Dispatcher.execute`
   (`parity_operations.py:37`) → `Ops.rename` (`commands.py:333`), the same
   authoritative undo stack (`commands.py:305`) and guarded assembly metadata
   (`commands.py:257`). Native independently dispatches typed `CadClient`
   calls (`native.rs:165`, `:199`, `:215`) against its own service/model.
4. Real document/kernel observations come from `parity_observations.py:61`.
   Native reads the canonical serialization through the read-only route
   `cad/robocad/api.py:1571` and typed `cad_client/parity.rs:6`; mutation dispatch
   remains independent. The receipt retains source SHA, process document ID,
   revision, actual outcome, numerical owner, raw dependency versions, units,
   frame, provenance, uncertainty and missing/unsupported/invalid states.
5. Reference completion (`runner.rs:349`) is followed unconditionally by the
   owned report read (`runner.rs:352`). `runner.rs:109` preserves published
   receipts even after SIGTERM cooperative publication
   (`cad/robocad/parity_reference.py:251`). Validation refusal annotates the run
   (`runner.rs:102`), preserving raw identity and operation outcomes.
   The process owner observes without reaping (`process.rs:147`); its cleanup
   (`process.rs:95`) performs TERM/grace/KILL before bounded reaping.
   `process.rs:248` returns interruption and cleanup evidence together.
   `gates.rs:79` refuses every run-level interruption/cleanup issue; readable
   publication retains these issues (`publication.rs:58`).
6. `compare.rs:319` compares policies with field-specific errors and metadata;
   `gates.rs:44` recomputes diagnostics and refuses missing execution/coverage.
   Missing CAD provenance is not filled; declared export metadata differences
   block migration (`gates.rs:187`). Shared Python/OCCT refuses independent
   numerical derivation/kernel gates (`gates.rs:264`, `:273`). These line refs
   are under `crates/sim-runtime/src/cad_parity/`.
7. `publication.rs:14` creates a fresh owned output root; `publication.rs:37`
   prewrites JSON/text and atomically hard-links them, then the completion marker,
   using retained descriptors (`owned_path.rs:127`). Existing records are refused.
   Default planning is therefore a complete published **NotRun** report with
   false gates, not executed migration evidence. No such report was produced
   in this batch.

Paired lifecycle branches are also source-traced: `assembly.json:92` supplies
component parameters/start refusal/undo/redo/commit refusal/cancel inputs;
`native.rs:260` and `parity_operations.py:78` use the shared component owner.
Native requests cancel once and drains bounded status (`native.rs:300`); terminal
worker failure cannot masquerade as a validated revision refusal. Parameters are
observed through actual component definitions/occurrences and the edited Link's
volume. `linkage.json:96` supplies closure continuation; `native.rs:218` and
`motion_service.py:47` retain validated prior positions, not a reset branch.
`linkage.json:112` captures an ephemeral immutable source, edits live CAD, then
reviews the pinned source via `parity_operations.py:127`, `snapshots.py:33`,
`captured_review.py:18` and the independent native read-only route at `api.py:1554`.
Captured archive/physical hashes remain distinct from live kinematic identity.

Three independent reviewers read mathematics/schema/gates, adapter authority and
lifecycle, and corpus/isolation/publication/docs. Findings repaired include lossy
large-integer scalar/point conversion, fabricated startup timestamps, callback
signature mismatch, asynchronous cancel acknowledgment, worker crashes mistaken
for rejections, missing/null reply conflation, surface inertia units, unnecessary
pose solving for identity, late receipt loss, verified-byte reload and pathname
publication races. Unexecuted regression fixtures record these cases; source
review establishes implementation reasoning, not compilation or behavior tests.

## Batch checklist retained

| ID | Source-review evidence / limit |
|---|---|
| cad-parity-harness:outcome-1 | Versioned shared contract, comparison diagnostics and three distinct gates; fixtures unexecuted |
| cad-parity-harness:outcome-2 | Independent direct Ops vs typed Rust dispatch on separately owned models; no operation executed |
| cad-parity-harness:outcome-3 | Four inspected real archives/dependency hashes, explicit units/frame/provenance and field tolerances; originals preserved |
| cad-parity-harness:outcome-4 | Distinct execution/report states, non-overwriting publication and fail-closed gates; no executed pass |
| cad-parity-harness:task-T44.1 | contract/compare/gates and identity/provenance/units/non-finite/boundary fixtures |
| cad-parity-harness:task-T44.2 | direct/reference + native adapter, revision/component/captured/continuation and cancellation source paths |
| cad-parity-harness:task-T44.3 | corpus manifests, planning/execution report construction and atomic completion publication |
| cad-parity-harness:task-T44.4 | three independent source-review areas, resolved findings, architecture/ledger/inventory updates; compilation/runtime parity unverified |

## Behavior-changing decisions

Use `sim-runtime` modules over a new crate: existing typed CAD client and headless
shared services already live there, and the contract/comparator remain usable
without graphics. Use a synchronous headless process owner alongside the viewer's
asynchronous job reaper: CLI work has no UI thread and needs bounded child-group
cleanup, while physics stays in existing shared runtimes. Use an ephemeral
read-only captured observation cache, bounded to four labels, to compare pinned
captured identity after live edits without creating experiments or exports.
Captured source labels cannot be overwritten. Use an atomic completion marker
for the JSON/text bundle rather than treating a lone JSON file as publication.
Use descriptor-relative no-follow I/O to prevent symlink/path replacement from redirecting source reads or report writes. Require zero discrepancy for current same-authority service comparisons rather
than inventing numerical replacement tolerances. Revisit these choices only with
executed evidence, independent implementations or a changed workflow contract.

The lifecycle repair retains the leader identity until the last group signal
instead of reaping during polling. It separates shutdown issues from operation
receipts instead of relabelling recorded mutations. Revisit these choices if an
OS-specific identity capability or executed lifecycle evidence provides a stronger
containment guarantee. No phase-2 implementation is qualified by this source review.

The 2026-10-02 verification pass found that Darwin reports `EPERM` when a
retained process group contains only zombies. Cleanup accepts that result only
when bounded libproc enumeration verifies the exact group, its unreaped leader,
and exclusively zombie members in two stable observations. A live, inaccessible,
changed or oversized group still fails cleanup. Any wait-ownership observation
error permanently disables further PID/group operations. This keeps the existing
containment owner and avoids reaping before the last signal or ignoring `EPERM`
for live processes. Actual-child fixtures cover natural exit, a live descendant,
and external reaping; their execution results belong to the verification report.
