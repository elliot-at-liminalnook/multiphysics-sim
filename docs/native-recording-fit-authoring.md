# Native controller recordings and fitting — T50

This bounded batch extends Build → Actuators → Measured evidence retained studies
in `sim-spatial`. Launch path remains `cargo run -p sim-spatial -- --system
examples/systems-builder/motor-driver-board/board.system.json`, then open the
Actuators dock and a measured archive or saved review. This command is documentation,
not an executed check. No new viewer, mode, optimizer, physics or persistence owner.
Implementation and fixtures are verified by reading only; compilation, fixture
execution, rendering, hit testing, publication and workflow parity remain unverified.

## Ownership before Bevy changes

Durable source is the shared saved Study and its recording/context/assignment/result
vectors. CAD and the accepted actuator registry continue to own physical definitions.
Global StudyOwner alone retains study identities, revisions, jobs and terminal
receipts; the existing action apply system and JobResults publisher are its writers.
Global StudyUi owns retained raw form intent, immutable submission stamps and text
mapping. Per-entity kit controls are transient dock children: existing renderers
spawn them and linked despawn removes presentation only. Dock, mode and document
changes do not remove retained work. No second focus or selection owner is added.

Input sends typed action occurrences; pending jobs/cancellation and unapplied text
are durable state. Public ViewerSet orders Input → Actions → JobResults → SimSync
→ Present. T49 public activation/focus containment applies to all added ordinary
controls; no private cross-feature ordering is introduced. Frame systems collect
intent and present evidence. Import reads/parsing/validation, additional-study
loading, prediction and fit execution belong to adopted jobs. Physics and the
40-evaluation optimizer remain existing runtime libraries. Pinned 0.19.1 signatures
are checked by reading; no execution receipt is asserted.

## Behavior decisions

Exact duplicate controller fingerprints are idempotent imports, preserving all
existing revisions, assignments and exposure. Ambiguous identities and conflicting
assignments are rejected; no case is silently retargeted. Alternatives were duplicate
rows or last-write-wins assignments. Revisit only with a versioned identity migration.

Whole-run role, limits and rationale are frozen together. Held-out reservation never
becomes tuning data. Exposure and influence are monotonic; imported/inspected evidence
is exploratory and needs fresh confirmation. Setup authoring appends immutable
revisions, with units, frames, origin and provenance, and cannot rewrite captured
facts. These choices preserve reproducibility rather than permit editable splits.
Revisit only with a separate reviewed dataset-version contract.

Optional additional saved studies are captured in job input before fitting; their
archive and recording identities/roles stay explicit. Bare archive trial collisions
are refused rather than silently routed to a different archive. Captured receipt
projections list bounded identities and setup/dataset inputs, without embedding
previous receipts recursively. Revisit if a shared namespaced dataset contract is
introduced. Deferred FPGA imports are validated/classified but never entered as
controller captures in native studies; the legacy reference remains available.

Explicit candidate use changes only the exploratory draft through shared guards;
review decisions are metadata. Failed, cancelled or incomplete fits cannot supply
a usable candidate or passing assessment. Immutable save-new/reopen uses the existing
Study publication owner; acknowledging a captured revision never acknowledges newer
edits. No CAD/registry promotion or hardware qualification is implied.

## Required checklist

| ID | Required source acceptance path |
|---|---|
| native-recording-fit-authoring:outcome-1 | Rendered recording controls → existing Hit/submission stamps → StudyAction apply; system_ui/REST consume the same bindings/owner → shared recording validators |
| native-recording-fit-authoring:outcome-2 | Shared recording identity/context append/frozen assignment → retained StudyOwner → immutable publication snapshot → Study::load validation |
| native-recording-fit-authoring:outcome-3 | Captured import/prediction/combined inputs → adopted jobs → existing runtime prediction/CalibrationData optimizer → original-study terminal receipts with cancellation/displacement |
| native-recording-fit-authoring:outcome-4 | Legacy Action::run/configure/widget projection/result consumer → shared preparation/execution/application; deferred compatibility surfaces remain |
| native-recording-fit-authoring:task-T50.1 | Shared transactional commands, classification, dataset identity/exposure/candidate guards and written compatibility fixtures |
| native-recording-fit-authoring:task-T50.2 | Existing retained native owners, stamped actual controls, jobs-owned reads and execution; incomplete captures unscored, FPGA deferred |
| native-recording-fit-authoring:task-T50.3 | Captured-time review, assumptions/staleness/objective history, explicit exploratory use, immutable publication/reopen and actual-consumer fixtures |

All seven IDs and T50.1–T50.3 remain required. Source traces and independent findings
are recorded during integration; this checklist alone is not acceptance evidence.

## Remaining external requirements

Python/OCCT RoboCAD remains the CAD reference until separately proven parity. Browser
hardware/calibration reference paths remain available. Hardware acquisition and
physical driving, FPGA recording authoring/review, electrical/power analysis, raw
sweep migration, CAD proposal acceptance, actuator-registry promotion and remaining
legacy experiment workflows are outside T50. No reference path is retired, no parity
execution occurred, and no hardware command was sent. Abrupt process exits still
cannot guarantee durability of unpublished in-memory drafts.

## Integration reading findings

Independent shared review found historical combined-fit reservations were omitted
from additional-dataset preparation. `recordings::frozen` now folds current maps
and every earlier recording/combined dataset before allowing a role. A refused
additional study with held-out evidence conflicting with local tuning retains an
identity-level held-out quarantine; it does not overwrite either frozen declaration.
Later tuning and exploratory use of a tuning candidate for that identity refuse.
This reversible inspection record is preferable to forgetting the rejected source
or changing an earlier assignment. Revisit only with separately versioned evidence.

Unknown legacy combined selections are validated before partitioning by role.
Coordinate authoring admits devices found in imported recordings; archive-only
sensitivity/fitting still checks their selected archive subset. Exposure revision
tracking compares compact metadata, so a map-only review change invalidates an
older publication acknowledgment. Recording identities are derived in jobs and
cached for presentation, avoiding frame serialization of captured samples; the
cache is not serialized as another source of truth. Grouped synchronous rejection
unlocks submission while retaining every raw draft; asynchronous acknowledgment
waits for terminal applied status and cannot erase newer text.

## Exact source traces and review scope

| ID | Concrete trace (repository-relative path:line) |
|---|---|
| native-recording-fit-authoring:outcome-1 | `crates/sim-spatial/src/builder/calibration/study/ui.rs:126` reaches `recording_ui.rs:12` structured controls; `forms.rs:144` recording scalar submissions and `forms.rs:232` grouped actions feed `actions.rs:148` existing apply owner. `actions.rs:32` REST parser/system_ui activation uses the same owner; `recording_jobs.rs:21`/`:62` call shared validation. |
| native-recording-fit-authoring:outcome-2 | `crates/sim-runtime/src/experiment_study/recordings.rs:35` historical frozen identities, `:49` additional quarantine, `:65` assignments, `crates/sim-runtime/src/controller_refinement/context.rs:53` truthful unknown setup; `crates/sim-runtime/src/experiment_study.rs:418` monotonic influence. Native `jobs.rs:167` immutable publication and `:320` revision acknowledgment; `:85` reopen through shared Study::load. |
| native-recording-fit-authoring:outcome-3 | `recording_jobs.rs:93` captures source/revision/document, `:117` loads optional saved evidence, `:128` reserves additional identities before preparation, `:142` executes shared runtime; `crates/sim-runtime/src/experiment_study/refinement.rs:179` calls existing prediction/CalibrationData optimizer; native `jobs.rs:284` applies job-cached inputs to original study and retains cancelled/failed/displaced evidence. |
| native-recording-fit-authoring:outcome-4 | `crates/sim-viewer/src/experiments_ui/refinement.rs:197` recording/combined shared preparation/dispatch, `:251` classified import, `:272` shared prediction, `experiments_ui.rs:239` shared result consumer, `experiments_ui/rest.rs:151` append-only configure. Legacy wire actions remain; new compatibility fixtures consume real Action/API paths. |
| native-recording-fit-authoring:task-T50.1 | `crates/sim-runtime/src/experiment_study/recordings.rs:8` classification, `:16` derived identity cache, `:76` dataset collisions, `:116` source-linked candidate guards, `:148` filtered request; `recording_fixtures.rs:1` old/opaque/duplicate/incomplete/purpose/reservation fixtures, all unexecuted. |
| native-recording-fit-authoring:task-T50.2 | `actions.rs:150` jobs-owned recording commands; `recording_jobs.rs:156` stamped prepared attachment; `forms.rs:105` deferred acknowledgment and `:294` terminal reconciliation; `recording_lifecycle.rs:26` production apply/poll lifetime fixtures and `:140` map-only save race. |
| native-recording-fit-authoring:task-T50.3 | `recording_ui.rs:27` all immutable setup revisions and `:76` assumptions/staleness/failed fit review; `recording_chart.rs:18` cache lookup and job conversion at `:21`; `ui_tests.rs:371` real purpose activation, `:421` scheduled modal restoration, `:442`/`:450` terminal acknowledgment, `:465` synchronous group rejection. Native publication remains the existing save-new/reopen path. |

Native paths abbreviated in this table are under
`crates/sim-spatial/src/builder/calibration/study/`; runtime and legacy shorthand
refer to the explicitly named crate/folder in the same row. Line numbers are navigation
anchors, not executed receipts. Independent reviewers covered shared identities,
exposure/applicability/compatibility, native jobs/publication, and actual controls,
focus/charts/legacy delegation; their concrete findings were repaired and reread.
The final fixture expectation was corrected to name the new held-out error path.
All fixtures, compilation and interactive behavior remain unexecuted.

Reading-only checks: scoped `git diff --check` reports no whitespace defects;
source searches confirm adopted jobs/public sets, bounded modules and unchanged
reference handlers. There were no builds, test runs, launches, screenshots, exports,
hardware operations, cache cleanup, data deletion or remote operations.
