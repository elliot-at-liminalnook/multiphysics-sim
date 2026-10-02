# Portable retained-study artifacts — T53

SIMSTUDY v1 is one self-contained retained Study artifact. The shared Study owns
its manifest and every object registered by `input_contents.references`, once per
BLAKE3 identity. It preserves inline archive observations/evaluations, source hashes,
recordings, electrical results, raw rejected forms, unknown opaque fields and
unapplied terminal evidence. It does not package an arbitrary source tree or fetch
missing inputs. Source paths remain provenance, never extraction instructions.
No automatic migration, source rewriting, physical-source promotion or recursive
receipt wrapping occurs. Existing JSON, companions and HTML publication remain.

Navigate **Build → Actuators → Offline measured-PWM study** using the existing
native shell. **Open portable** reads an artifact; **Save portable** captures the
current revision, destination and raw forms into a new immutable artifact. Ordinary
saved-review open also recognizes portable content independent of extension.
Transfer only the portable file and reopen it at its new path. JSON saved reviews
still require every referenced `.study-inputs/<blake3>.bin` beside the manifest.
HTML is an inspection report, cannot reopen an editable Study, and its exact input
links still require companions. Reports never acknowledge editable-study saving.

Legacy experiment review adds **Save portable study** beside JSON and HTML;
REST `experiments` operation `save_portable` uses that same captured publication
handler. Existing `open` delegates shared detection. The batch example supports
`review_controller publish-portable INPUT_STUDY NEW_ARTIFACT`; all its existing
Study loads, including combined additional Studies, share detection. The separate
`gait_lab::Study` configuration is a different owner/format, outside this inventory.
No launch or command in this guide was executed in T53.

## Format and bounded failure contract

All integer fields are unsigned little-endian. The 24-byte header is exact ASCII
`SIMSTUDY` (8 bytes), version `u32` (=1), manifest length `u64`, object count `u32`.
The JSON Study manifest follows, then exactly that many objects: lowercase BLAKE3
ASCII hex (64 bytes), length `u64`, exact raw bytes. Writers sort by identity;
readers accept any object order. Detection uses magic bytes, never filename.
Unsupported versions fail closed. No artifact field controls a filesystem path.

Portable limits are 256 MiB for the complete container, 64 MiB for its JSON manifest,
4096 objects, 64 MiB per object and JSON nesting depth 64. The manifest allowance
retains room for existing inline evidence. Bounded source reads use a maximum plus
one sentinel byte, and lengths/counts/checked aggregate arithmetic precede object
allocation. The bounded JSON scan precedes deserialization; portable manifests
also reject duplicate JSON keys. These restrictions do not impose new byte,
object-count or depth-64 limits on historical JSON manifests or their companions.
Ordinary JSON retains its established serde recursion and compatibility behavior.
Portable recognized diagnostic/electrical JSON is checked before reconstructing
review caches or validating exact comparison inputs; opaque objects remain bytes.

Ordinary JSON publication checks the serialized manifest and recoverable exact
content through the shared legacy decoding/validation behavior before filesystem
publication. This refuses a newly authored representation that its loader cannot
reopen, preserving the input and destination. Historical JSON still requires its
companions, and loading it retains the historical memory-use characteristics;
the portable resource guarantees are not a claim about arbitrary legacy inputs.

Membership is exactly the Store reference set, including historical/rejected
objects. Extra objects and omitted objects fail; identical duplicate objects are
rejected rather than deduplicated on read. Conflicting declared lengths and wrong
hashes fail before attachment. Publication deduplicates valid Store references,
verifies all bytes and includes each object once. Decoding validates complete
framing, reference schema, count, length, membership, BLAKE3, truncation and trailing
bytes before recovering object bytes into the existing Store. Shared loading then
restores terminal caches and invokes existing Study validators. There is no sibling
companion read on the portable branch and no filesystem extraction.

Named failures use `study.portable.` with suffixes `size`, `json_size`, `json_depth`,
`manifest_size`, `object_count`, `object_size`, `aggregate_size`, `length`,
`truncated`, `magic`, `version`, `manifest`, `membership`, `identity`,
`duplicate_object`, `conflicting_object`, `hash_identity`, `missing_object`,
`trailing`. Existing Store/Study validation errors keep their established paths.
Some malformed omissions fail at framing/count validation before `missing_object`;
no error authorizes partial attachment. Corrupted magic falls into the legacy JSON
path and fails JSON parsing; it cannot silently load as a portable artifact.

Captured additional-study source bytes are one exact object. A portable source
larger than the 64-MiB object limit can be opened directly but cannot subsequently
be packaged as that exact captured additional-input object. Publication refuses
explicitly and retains the capture; no arbitrary chunking, eviction or rewrite is
performed. Ordinary JSON companions continue their existing immutable policy.

Portable serialization/publication runs in the existing jobs; shared
`publication::publish_with(ImmutableNew)` remains the only filesystem writer.
Existing destination conflicts refuse replacement. File/directory synchronization
failure after visibility remains visible-unconfirmed failure, never a saved receipt.
Recovery retains the captured Study and exact bytes; retry uses a fresh destination.
Native cancellation uses the current PublicationGate linearization. Legacy retains
its existing worker cancellation flag: cancellation before publication refuses the
write; requests racing a started write may leave a published destination, but never
acknowledge the review saved and surface that limitation. Pending jobs and
receipts remain durable owner state, and stale successful captures cannot mark a
newer revision saved. Reports never participate in editable revision acknowledgment.
Legacy pending/result publication ownership retains the exact captured Study,
destination, representation and revision even after later edits. Failed, cancelled
and stale completion recovery is visible in the existing experiment panel. Recover
as a separate retained review, then publish to a fresh destination; recovery never
replaces the newer current Study or overwrites a visible-unconfirmed destination.
Snapshots stay in memory under that owner and are not recursively embedded in
durable receipts. Abrupt process termination can still lose unpublished recovery.

## Focused source inventory

Paths `runtime/` below mean `crates/sim-runtime/src/`; `native/` means
`crates/sim-spatial/src/builder/calibration/study/`. Each row names actual ownership,
consumer dependency and portable acceptance rather than a whole-project inventory.

| Evidence family / entry point | Existing owner and dependency | Portable acceptance trace |
| --- | --- | --- |
| Inline archive, trial observations, evaluations, model/source identities | `runtime/experiment_study.rs:382` Study; `runtime/experiment_comparison.rs:157` Archive; shared runtime validators and registry remain authoritative | Complete manifest encode/decode; archive/source identities are retained, not recomputed from provenance paths |
| Controller recordings, predictions, contexts, assignments, fits; legacy FPGA/deferred payloads | `runtime/controller_refinement/workspace.rs:10`; `runtime/experiment_study/refinement.rs:11` commands; native `recording_jobs.rs:159`; legacy `experiments_ui/refinement.rs:219` | Inline workspace survives manifest; Store objects survive exact membership; shared load and validators retain existing incomplete/opaque semantics |
| Additional-study capture, including rejected/invalid/non-UTF8 input | Native `recording_jobs.rs:98` capture before parse, `:214` bounded reader; shared Store `input_content.rs:22`; receipt references and source provenance | `Study::manifest_value` diagnostic projection then `load_bytes`; merge additional Store identities; exact original input object survives portable publish/reopen without original path |
| Electrical settings, recorded servo voltage and calibrated sidecar comparisons | `runtime/experiment_study/electrical.rs:168` capture inputs; `:180` input references; `:254` exact comparison validation; native recording jobs; legacy shared command delegation | Manifest stores source/comparison identities; Store recovers exact recording/prediction/measurement bytes; shared validation follows cache restoration |
| Cancelled, failed, incomplete and unapplied terminal diagnostics | `runtime/experiment_study/terminal.rs:21` capture, `:33` retain, `:49` cache; `refinement.rs:223` application | Exact ResultData objects recover into Store/cache; cancellation/unapplied/UNSCORED flags remain; loading does not reinstall scored attachment capabilities |
| Raw rejected/unsubmitted forms and captured publication intent | Native `forms.rs` StudyUi; `jobs.rs:163` form capture and `:173` publication capture; legacy rejected-draft retained fields | Complete manifest retains original raw text, stamps, destination and bounded receipt metadata; no submit, clearing or recursive previous-Study embedding |
| Unknown opaque compatibility fields, old inline raw inputs and unrecognized envelopes | `runtime/experiment_study.rs:400` flattened retained fields; workspace `:12`, evidence `refinement.rs:71`; Store envelopes only validate recognized contracts | Preserve opaque JSON values and old inline payloads; no conversion, extraction or newly invented physical defaults |

Actual shared consumers found by source search: native `jobs.rs:100` open;
`recording_jobs.rs:136` captured additional load; legacy
`crates/sim-viewer/src/experiments_ui.rs:137` open and
`experiments_ui/refinement.rs:219` additional load; runtime
`examples/review_controller.rs:15` explicit portable publication plus all existing
review/fit/import/evaluate/export and additional Study loads;
`examples/review_experiments.rs:51` retains JSON/report reference publication.
Shared fixtures under `experiment_study/` and integration tests
`tests/{experiment_study,controller_refinement,fpga_controller}.rs` also consume
Study load/publication. They remain unexecuted; distinct gait-lab Study consumers
are not falsely migrated. No runtime dependency on the original archive directory
is introduced by portable load.

## Lifecycle and seven batch checklist IDs

Open an existing JSON review with its companions through shared Study load, or
open a portable artifact. StudyOwner retains its identity/revision and source
provenance. Edit through validated shared commands; rejected raw forms remain in
StudyUi. Save portable routes the rendered Hit (also system_ui activation) or REST
typed action to the same apply owner, captures StudyStamp/destination/raw inputs,
and submits an adopted I/O job. Its worker serializes/hashes and immutable-publishes.
JobResults retains cancellation/failure/displacement receipts and captured bytes;
only matching successful editable revisions acknowledge saving. Reopen the file at
an unrelated path through the same load job. Store hydration restores exact input
objects; terminal caches restore inspection without applying cancelled results.
Existing close blockers still read pending work, drafts and dirty Study revisions.

| Checklist ID | Source evidence |
| --- | --- |
| portable-study-artifacts:outcome-1 | This focused inventory; `crates/sim-runtime/src/experiment_study.rs:382`; `experiment_study/portable.rs:60` encode complete manifest/reference set; `:80` decode; `publication.rs:24` shared detection/hydration ordering |
| portable-study-artifacts:outcome-2 | Shared `experiment_study/portable.rs:80` bounded decoding and preattachment membership/hash checks; `:117` manifest preflight; `publication.rs:24` detection then terminal cache and Study validation; `terminal.rs:44` exact diagnostic cache recovery |
| portable-study-artifacts:outcome-3 | Native `study/ui.rs:88` / `:143` rendered open/save; `actions.rs:16` / `:27` typed actions and `:191` dispatch; `jobs.rs:173` capture, `:179` job submission, `:357` matching revision acknowledgment, `:47` cancellation gate; `state.rs:61` blockers; `publication_lifecycle.rs:103` actual-consumer fixtures; legacy common publish_captured and REST save_portable |
| portable-study-artifacts:outcome-4 | `experiment_study/publication.rs:12` existing JSON, `:24` portable detection, `:61` shared immutable portable publication, `:66` HTML/companion semantics; legacy `experiments_ui/refinement.rs:219` additional shared load; runtime `examples/review_controller.rs:15` portable CLI |
| portable-study-artifacts:task-T53.1 | `experiment_study/portable.rs:8` constants, `:18` bounded read, `:28` JSON bounds, `:49` membership size arithmetic, `:80` preattachment framing/hash validation, `:117` duplicate-key refusal; shared publication restores terminal cache/Study validation |
| portable-study-artifacts:task-T53.2 | Native `StudyPlugin` public Actions/JobResults registration; `forms.rs` raw fields/submission; `ui.rs` kit controls/discovery; `actions.rs` REST/system_ui handler; `jobs.rs` PublicationKind/gate/capture/ack; legacy common captured helper preserves JSON/report paths |
| portable-study-artifacts:task-T53.3 | Shared `experiment_study/portable_fixtures.rs`, native `study/publication_lifecycle.rs:103`, `study/ui_tests.rs:559`, `study/recording_lifecycle.rs:373`, `study/electrical_lifecycle.rs:82`, legacy `experiments_ui.rs` portable poll/relocation fixtures; architecture opening status and appended T52/T53 reconcile call-0396 acceptance across d9bc1455/929f6833; this guide states bounds and execution limitations |

Native state remains resource/durable data on StudyOwner and StudyUi with their
existing action/job writers. Kit controls and text focus are transient per-entity
dock children under existing teardown; action occurrences are expiring messages,
not pending-work storage. Public ViewerSet Input → Actions → JobResults → SimSync
→ Present ordering remains. Frames author/project small state; workers read,
decode, hash, serialize and publish; shared runtime validates and computes physics.
No new Bevy lifecycle, focus owner, feature thread or physics path is introduced.

The written fixtures cover relocated recovery, exact input/electrical/terminal
content, opaque compatibility, malformed/unsupported/truncated/oversized inputs,
duplicates/conflicts/missing membership, immutable conflict, publication-stage
failures, cancellation/displacement and stale/report acknowledgment. Fixtures are
explicitly **unexecuted**. Verification was reading only: no builds, tests, viewer
launches, screenshots, exports, benchmarks, parity or hardware operations.
Compilation, interactive behavior and platform durability remain unverified.
T52 is accepted by source review in call-0396 across `d9bc1455`/`929f6833`; T53
implementation does not imply a new acceptance receipt. Architecture §§8–9,
CAD/registry physical authority and independent hardware safety remain unchanged.
