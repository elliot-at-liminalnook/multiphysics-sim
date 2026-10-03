# Shared evidence publication — T51.1–T51.3

This bounded batch shares filesystem mechanics beneath existing owners. It adds no
document service, persistence owner, feature thread, simulation path or UI-thread
I/O. CAD remains physical authority. Data, schemas and legacy inline evidence are
preserved. Source and written fixtures are inspected only: compilation, fixtures,
exports, durability behavior, platform behavior and GUI parity remain unverified.
T50 was accepted in call-0386 across `5dd26014`, `11b9ce82` and `3a7d321a`; T51
does not redispatch it or establish whole-roadmap completion.

## Contract and decisions

`sim-runtime::publication` is shared because Study already resides in that crate
and preferences already depend on it. The primitive owns only filesystem stages:
parent preparation, uniquely owned same-directory temporary `create_new`, byte
write, file sync, immutable hard-link or replacement rename, owned temporary cleanup,
and parent-directory synchronization. `Policy::ImmutableNew` refuses an existing
destination; `Policy::Replace` permits replacement under consumer authorization.
Serialization, validation, expected snapshots, ordering gates, cancellation authority
and acknowledgments belong to consumers. Neither policy supplies a multi-file
transaction, cross-process compare-and-swap or a document owner.

Typed outcomes separate `Unpublished(Failure)`, `VisibleUnconfirmed(Failure)` and
`Confirmed { cleanup_error }`. Failure names the stage, I/O kind and diagnostic;
cleanup errors are retained rather than masking the original error. Cleanup touches
only a temporary file successfully created by that invocation. A collision cannot
transfer ownership or authorize deletion of somebody else's file. Destination files
and valid orphan companions are never cleanup targets. Consumer adapters surface
cleanup errors and fail acknowledgment even if durability was otherwise confirmed.

Required synchronization includes the destination file and every parent through
the filesystem root, covering newly created directory ancestry. Unsupported
directory synchronization and any ancestry-sync failure are unconfirmed outcomes,
never invented durability success. File sync and directory sync rely on the host
filesystem/OS guarantees; canonical ancestor resolution does not protect against
concurrent path or symlink changes. A visible outcome describes the publication or
consumer observation boundary, not a promise that another process cannot remove or
change the destination afterward. This is not evidence of power-loss testing, remote-storage
persistence or uniform platform support. Hard-link and replacement rename behavior
also depend on the host filesystem. The owned temporary resides beside its destination.

`confirm_existing` is recovery machinery, not identity validation: a consumer first
verifies content or its expected snapshot, then requires the same file/ancestry
synchronization. Readable matching bytes alone cannot erase unconfirmed durability.
`Hooks::before(stage, destination, working_path)` injects deterministic failures in
the production writer; fixtures do not introduce a second publication implementation.

The shared writer prepares missing parents for both policies; this includes new
Study destination directories. This is a reversible directory-creation behavior,
not permission to replace any artifact. Previously Study required the manifest parent
to exist while companions/preferences prepared directories locally. Revisit parent
preparation if an owner later requires an explicit no-directory-creation policy.

## Writers, acknowledgment owners and recovery

| Writer / entry | Publication boundary and acknowledgment | Retained recovery |
|---|---|---|
| Study save / report | `experiment_study/publication.rs::Study::{save_new_with,export_html_new_with}` validates/serializes and publishes companions first, immutable manifest/report last. Native Study jobs capture revision and authorize cancellation before publication; the terminal publisher alone marks that captured revision saved. Export never marks the draft saved. | Pending and live Study snapshots retain exact content bytes; failed terminal receipts retain captured evidence. A visible unconfirmed manifest/report remains untouched; inspect/reopen it and use a new filename for another immutable publication. |
| Content companions | `experiment_study/input_content.rs::Store::publish_with` verifies hash/length/content. Already present valid content is synchronized before reuse. During a new publication, only an existing-destination conflict permits verified concurrent reuse followed by `confirm_existing`; other failures remain failures. | Existing valid/corrupt files are never overwritten. Failed manifest publication may leave valid orphan companions. Preserve them. Reopen hydrates sibling `.study-inputs/<hash>.bin`; missing/corrupt paths are named errors. |
| Native Study action / result | `builder/calibration/study/actions.rs` routes typed save/export intent; `study/jobs.rs` captures, starts an adopted Io job and applies terminal results. The publication gate preserves existing cancellation permission; after publication begins it cannot promise rollback. | Cancellation/displacement retains exact source bytes and terminal receipts. Newer edits remain dirty when a captured revision completes. Authored-work blockers remain owned by StudyOwner/StudyUi. |
| Legacy Study callers | `sim-viewer/src/experiments_ui.rs` and `experiments_ui/rest.rs` call the same shared Study save/export APIs and their existing Saved result path. | String diagnostics carry unconfirmed visibility to existing status/result owners; no alternate filesystem writer remains in these callers. |
| Former immutable helper callers | `crates/sim-runtime/src/controller_refinement/fpga_design.rs:101` proposal export and `controller_refinement/cad.rs:271` accepted proposal save call shared ImmutableNew directly after their existing validation. | Existing Result callers receive visibility/durability diagnostics. No FPGA operation or CAD physical-authority change is introduced. |
| Preferences | `app/settings/publication.rs::publish_ordered_with` holds SettingsOwner's serialized gate, validates current external snapshot/schema, then uses shared Replace or confirm-existing. `settings/plugin.rs::land_save` acknowledges only its captured revision. | Visible revision floor and confirmed revision remain separate. Unconfirmed known bytes become the expected retry base while dirty state/diagnostic persists. Retry checks current file before syncing/replacing. Observed external edits survive refusal. |
| Ordinary close / Drop | `app/close.rs::authorize` rereads current drain stamp, readiness and authored-work blockers at Last on both authorization frames. SettingsOwner Drop submits its current snapshot through the same gate using existing complete-on-drop jobs. | New edits revoke armed close and loss acknowledgments. Late saved receipts cannot authorize newer work. Drop is best effort and not an ordinary close acknowledgment; abrupt exit can lose unpublished memory. |

Preference comparison is a prepublication check under the in-process gate. Another
process can write after comparison and before rename or confirmation; no OS lock or
cross-process CAS is claimed. Run one preference owner per destination. Refused
observed external edits require reviewing the preserved input and restarting/reloading
through existing ownership, rather than silently adopting it as an authorized base.

## Specialized writers retained

System undo journals retain command-history/undo transaction semantics; annotations
retain their existing locked read-check-apply-write service; recording writers retain
stream/pair lifetime and sidecar rules; CAD parity publication retains isolated owned
output paths and multi-record completion markers. Migrating these would require
separate transaction, streaming or isolation review. They remain outside T51 and do
not become clients of a global persistence service. Source geometry, legacy evidence,
recording artifacts and companions are neither rewritten nor deleted by this batch.

## Required acceptance IDs and source evidence

The seven IDs below refer to source reading and unexecuted fixtures, not executed
receipts. The following repository-relative anchors identify the implementation and actual consumers.

| ID | Evidence path |
|---|---|
| shared-evidence-publication:outcome-1 | Shared visibility/durability outcomes: `crates/sim-runtime/src/publication.rs:36` Outcome, `:90` publication, `:130` confirmation; failures reach native `crates/sim-spatial/src/builder/calibration/study/jobs.rs:210` and settings `crates/sim-spatial/src/app/settings/publication.rs:45`. |
| shared-evidence-publication:outcome-2 | Immutable Study preservation: `crates/sim-runtime/src/experiment_study/publication.rs:12`/`:35` companion-first save/report; `input_content.rs:86` verified synchronized reuse and `:71` hydrate; native `crates/sim-spatial/src/builder/calibration/study/jobs.rs:237` terminal receipts and `publication_lifecycle.rs:41` retained-byte acknowledgment fixtures. |
| shared-evidence-publication:outcome-3 | Ordered preferences and fail-closed close: `crates/sim-spatial/src/app/settings/publication.rs:24` visible floor/expected snapshot; `settings/plugin.rs:221` captured acknowledgment; `settings/mod.rs:264` drain; `app/close.rs:132` current blockers/stamp authorization; `settings/publication_fixtures.rs:123` drain revision changes. |
| shared-evidence-publication:outcome-4 | Removed duplicate mechanics and source-only documentation: former Study `write_new` is replaced by `crates/sim-runtime/src/experiment_study/publication.rs:12`; settings writer is replaced by `crates/sim-spatial/src/app/settings/publication.rs:39`; `settings/jobs.rs:414` reexports consumer ordering. This document and the T51 sections of architecture/preferences/close/recording documents describe execution limits. |
| shared-evidence-publication:task-T51.1 | Contract/source map: `crates/sim-runtime/src/publication.rs:15` stage seam, `:90` writer, `:130` confirmation; the writer/acknowledgment/recovery table above keeps policy with consumers. |
| shared-evidence-publication:task-T51.2 | Consumer migrations: `crates/sim-runtime/src/experiment_study/publication.rs:8`/`:32`, `input_content.rs:86`; `crates/sim-spatial/src/app/settings/publication.rs:21`, `settings/mod.rs:305` complete-on-drop submission; native `builder/calibration/study/jobs.rs:176` job and `:237` terminal owner; legacy `crates/sim-viewer/src/experiments_ui.rs:341` and `experiments_ui/rest.rs:207`. |
| shared-evidence-publication:task-T51.3 | Unexecuted fixtures/review docs: `crates/sim-runtime/src/publication/fixtures.rs:19` stage failures, `:39` collision, `:53` cleanup, `:68` ancestry; `experiment_study/publication_fixtures.rs:16` manifest failures, `:36` retry, `:59` concurrent reuse, `:82` collision/report refusal; `crates/sim-spatial/src/builder/calibration/study/publication_lifecycle.rs:41` actual native ack, `:61` cancellation/displacement, `:79` visible displaced evidence; `app/settings/publication_fixtures.rs:33` failures, `:46` retry, `:67` external edits, `:83` old/new revision, `:100` collision, `:123` drain; `app/close/tests.rs:90` revision invalidation; architecture records call-0386 T50 acceptance. |

Fixtures cover temporary collision without foreign cleanup, existing destinations,
write/file-sync/publication failures, post-publication directory-sync failure,
verified concurrent companion reuse and corrupt refusal, retained bytes after
failure/cancellation/displacement, visible-unconfirmed retry, external edits on retry
and revision changes during close draining. All remain uncompiled and unexecuted.
No builds, tests, launches, screenshots, executed exports, parity qualification,
hardware actions, remote pushes, deployment or paid compute are part of this batch.


Runtime shorthand `experiment_study/` is under `crates/sim-runtime/src/`; settings
and close shorthand in the evidence table is under `crates/sim-spatial/src/app/`.
Line numbers are navigation anchors, not execution receipts. Native Study captured
failure/cancellation/displacement recovery is additionally traced by
`crates/sim-spatial/src/builder/calibration/study/publication_lifecycle.rs:41`.

## Split acquisition sources (leg-in-process provenance repair)

`sim-runtime::hardware::bench::provenance` publishes exact production source
companions and a versioned named composite manifest before acquisition effects.
It uses ImmutableNew for absent artifacts, verifies existing bytes and confirms
existing file/directory durability for retries. A visible but unconfirmed artifact
is preserved and reported as an error; acquisition does not proceed. This extends
source retention without rewriting historical recordings or changing their reader
schemas. The scheme and producer/consumer reading traces are in
[leg-in-process.md](leg-in-process.md#acquisition-source-identity-repair-lip1--lip3).
Evidence here is unexecuted source review, not a durability experiment.
