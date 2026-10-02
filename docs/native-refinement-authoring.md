# Native offline controller refinement — T48

Implemented in the existing Build → Actuators → Measured evidence retained-study
surface. Verification in this batch is reading only. All fixtures are written
and **unexecuted**; compilation, runtime behavior, interactive usability,
publication execution and parity remain unverified. No hardware operation,
launch, screenshot, test, build or export was performed.

[Field-by-field inventory](native-refinement-inventory.md) maps legacy controls,
REST operations, shared contracts, native controls and explicit exclusions.
Legacy compatibility remains available. The native launch/navigation path is
recorded there; no new viewer, mode, persistence owner or physics path is added.

## Ownership and design decisions

`StudyOwner` remains the global retained identity/revision, pending work and result
owner. Its existing action apply system writes drafts; global JobResults polling
publishes captured evidence. `StudyUi` writes retained stamped text and selection
intent, including closed-dock drafts. Saved `Study` is shared runtime data; accepted
CAD and actuator-registry physical definitions remain their existing owners.
Per-entity kit controls are transient dock children; linked despawn removes widgets
only. Existing global status controls remain available across modes/dock closure.

Input produces typed action occurrences. Pending work and cancellation requests
are durable state, never expiring messages. Public ViewerSet order remains Input
→ Actions → JobResults → SimSync → Present. Shared small validators run in Actions;
existing runtime simulation/analysis and raster/I/O work run through jobs. No
private cross-feature scheduling edge, feature thread or UI physics is introduced.
Pinned Bevy 0.19.1 source signatures are read before changes; existing kit text
entry and actual Button/Enabled/action bindings supply window and automation.

Decision: extend the T46/T47 owners rather than create a refinement workspace owner.
Why: identity, cancellation, close preservation and immutable publication must
remain one contract. Alternative: a second controller panel resource with its own
persistence. Revisit if a reviewed common evidence/document registry replaces
this retained owner. Decision: store bounded additive execution/publication capture
metadata in the shared saved Study envelope rather than only ephemeral native
receipts. Why: reopening must preserve reproducibility and rejected/failed work;
embedding a whole prior Study per saved receipt would recursively expand evidence.
Revisit if a versioned shared receipt schema replaces the additive compatible keys.

## Required checklist and reading evidence

| Checklist ID | Source trace and evidence | Scope |
|---|---|---|
| native-refinement-authoring:outcome-1 | `study/ui.rs` actual controls collection → `forms.rs` Hit/text → `actions.rs` StudyAction apply owner → shared `experiment_study/refinement.rs` commands; `app/route.rs`, `builder/ui_api.rs` consume the same actual rendered bindings | Source reviewed; not executed |
| native-refinement-authoring:outcome-2 | `study/jobs.rs::start_refinement` captures retained identity/revision/document and shared Capture before Job::spawn; shared `execute` calls control::simulate, calibration::sensitivity/attempt/robustness | Captured existing runtime; no physics duplication |
| native-refinement-authoring:outcome-3 | Global `jobs.rs::poll_owner/publish` attaches by original ID, retains stale/displaced/error/cancel facts; saved shared refinement_evidence and native receipts preserve capture; publication acknowledges captured revision only | Reopen/lifecycle fixtures unexecuted |
| native-refinement-authoring:outcome-4 | Legacy `experiments_ui/refinement.rs::Action::run`, `experiments_ui.rs::poll`, `experiments_ui/rest.rs` delegate assigned operations; inventory names deferred payloads and legacy UI | No retirement or parity execution |
| native-refinement-authoring:task-T48.1 | Shared authoring/refinement modules own transactional validation, frozen split selection, immutable preparation/dispatch/application, candidate provenance, monotonic exposure; compatibility fixtures preserve unknown data/refusals | Fixtures written and read only |
| native-refinement-authoring:task-T48.2 | Native action/jobs/state use existing retained owners; individual kit forms and trial selectors supply Experiment/Coordinate/Variant fields; close preservation reads existing pending/dirty/drafts facts | No independent lifecycle |
| native-refinement-authoring:task-T48.3 | Captured review/chart modules show actual sample times, metrics, limits, sensitivity, fit attempts/scenarios; explicit UseFit differs from review decisions; immutable save/new export/reopen keeps captured metadata | No physical acceptance claim |

All seven IDs remain durable for independent acceptance. Source review is the
verification method permitted here; their presence does not imply executed parity.
Exact path:line evidence is appended after integration review.

## End-to-end workflow

Open an archive or saved review through the dock path controls. Global load jobs
validate shared Study data and retain late/cancelled loads without stealing a
newer selection. Choose a retained study, edit individual controller/timing/knots,
conditions and task-limit fields through kit text. Stamped submissions fail closed
on late revision/identity and preserve their raw text. Shared validators clone
before commit; errors name input paths.

Simulate captures the complete experiment and model before launch. Progress and
cancel remain globally observable. Cancellation request and observed execution
cancellation are distinct; terminal results attach to the original study even if
another is selected or edited. Inspect captured settings/runtime and independent
sample-time target, feedback, physical angle and duty; failed/incomplete/cancelled
runs remain unscored. Sensitivity captures selected IDs and coordinates and shows
rank/correlations/warnings. Fit captures archive train/held-out IDs without changing
frozen roles; failures/partial attempts survive. Reviewing held-out evidence makes
exposure monotonic before later edits.

Fit completion does not adopt a candidate. Explicit use checks the captured fit,
device applicability and model validity, then records source fit hash and exposure
history on an exploratory draft. Review decisions/notes are separate durable
annotations. Robustness captures scenario model/timing/evidence and keeps failed or
incomplete scenarios inspectable, never a passing aggregate.

Save-new/export-new capture a revision and use the existing immutable jobs gate.
Existing destinations are preserved on failure. Save acknowledgment affects only
that revision; later edits remain dirty. Reopening retains additive execution and
publication captures, source linkage and opaque deferred payloads. A publication
capture describes the inputs/destination to the artifact; it is not a fabricated
post-write acknowledgment. Ordinary close and mode/document replacement read the
existing retained-study and text preservation facts; abrupt process exits still
cannot guarantee in-memory draft durability.

## Prior acceptance and limitations

T46 was accepted by source review in call-0360 across 1b0ca533, d238d6b6 and
c380210b. T47 was accepted in call-0364 at a0671a6b. Both remain uncompiled and
unexecuted in this source-only run. Architecture opening accounting is reconciled
with those acceptances instead of a stale ten-epic count. Native execution,
interactive layout, platform behavior and all new fixtures remain unverified.

External power/electrical, FPGA, recording/combined fitting, hardware acquisition,
raw sweeps, CAD proposals/acceptance and registry promotion remain outside T48.
Python/OCCT CAD remains the physical reference until separately demonstrated parity.
No accepted measurements, experiments, recordings or calibration data are removed.

## Final exact reading traces

| ID | Actual source trace (paths relative to repository) |
|---|---|
| native-refinement-authoring:outcome-1 | `crates/sim-spatial/src/builder/calibration/study/refinement_ui.rs:31` individual kit controls → `ui.rs:38` actual Button/Enabled/Hit collection → `crates/sim-spatial/src/app/actions.rs:503` system_ui discovery → `app/route.rs:51` activation → `study/actions.rs:43` sole owner → `:144` shared commands. `study/forms.rs:237` also sends selection edits through this owner. |
| native-refinement-authoring:outcome-2 | `crates/sim-spatial/src/builder/calibration/study/jobs.rs:139` immutable capture → `:159` dedicated jobs → `crates/sim-runtime/src/experiment_study/refinement.rs:147` existing runtime dispatch. `controller_refinement/authoring.rs:4` validates assumptions; `crates/sim-script/src/lib.rs:572` bounded static Rhai check never runs policy top-level code in the UI. |
| native-refinement-authoring:outcome-3 | `crates/sim-spatial/src/builder/calibration/study/jobs.rs:236` global polling → `:250` original identity publication → `:302` cancellation/stale/displacement evidence; `:194` immutable publication capture, `:332` revision-scoped acknowledgment; `study/actions.rs:85` retained rejection. `crates/sim-spatial/src/app/close.rs:80` reads existing preservation facts. |
| native-refinement-authoring:outcome-4 | `crates/sim-viewer/src/experiments_ui/refinement.rs:56` shared operation mapping, `:116` shared dispatch; `experiments_ui.rs:239` shared application; `experiments_ui/rest.rs:150` shared configure and `:214` shared prepare. Field inventory names all retained exclusions. |
| native-refinement-authoring:task-T48.1 | `crates/sim-runtime/src/experiment_study/refinement.rs:79` transactional commands, `:124` preparation, `:158` result application, `:192` compatible saved evidence checks; `controller_refinement/authoring.rs:47` coordinate checks, `:56` scenario checks. `experiment_study/compatibility.rs:98` starts unexecuted source fixtures. |
| native-refinement-authoring:task-T48.2 | `crates/sim-spatial/src/builder/calibration/study/forms.rs:85` stamped raw intent, `:93` actual acknowledgment, `refinement_forms.rs:7` scalar-to-command parsing, `refinement_ui.rs:31` structured fields/rows; `refinement_lifecycle.rs:1` explicitly unexecuted lifecycle fixtures. |
| native-refinement-authoring:task-T48.3 | `crates/sim-spatial/src/builder/calibration/study/refinement_ui.rs:108` captured metrics/settings, stale comparisons, explicit candidate-use/review; `refinement_chart.rs:10` archived run selection and `:17` actual sample-time rasters via jobs; `study/ui_tests.rs:225` unexecuted actual-control fixtures. `crates/sim-runtime/src/experiment_study.rs:608` escaped additive HTML evidence; existing save-new/ack preserves newer edits. |

## Reading findings and decisions

Three independent reviews covered shared/legacy validation and serialization,
jobs/publication/preservation, and actual controls/text/charts. Archive selections
initially lived only in UI state: durable shared commands now make accepted
selection edits dirty and preserve them on reopening. Precise Rhai schema filters
restore PID fields and arbitrary parameter names; structured parameter-value rows
support add/edit/remove including nested JSON values. Analysis review now labels
captured-versus-current inputs. Incomplete legacy fits remain unscored and cannot
supply candidates. Any retained controller run can be chosen for full trace charts.

Runtime reading found late requests mislabelled as executed cancellation and
final held-out prediction cancellation hidden inside a successful fit. Observed
cancellation now governs flags; additive FitAttempt.partial retains incomplete
scores/optimizer evidence separately from usable outcomes. Request flags remain
separate native receipt facts. Repair fixtures remain unexecuted. A concern about
omitted score adoption was withdrawn after confirming existing Fit.validate
already requires complete unique selected scores; the UI projection was hardened.

Decisions: allow empty coordinate/scenario drafts but validate execution and capture
the four legacy robustness defaults. Require device-specific fit coordinates to
have tuning-trial applicability. Require verified prediction traces for explicit
candidate use while keeping legacy summaries readable. Retain incomplete scoring
in an additive partial field rather than candidate outcomes. Alternatives were
rejecting unfinished drafts, hiding defaults, optimizing zero-influence devices,
adopting unverified summaries or dropping partial scores. Revisit with a separately
reviewed richer analysis/qualification contract; physical sources remain unchanged.

Final publication repair: malformed rejected/unsubmitted text is projected from the
existing StudyUi through the same native apply owner into the captured immutable
artifact (`study/actions.rs:74`, `study/jobs.rs:185`). It remains explicitly raw
unapplied intent, inspectable after reopening; it is not silently reparsed into
settings. Live drafts and their close blockers stay intact after publication.
Opaque older receipt fields are preserved. Decision: retain raw text as publication
evidence rather than refuse saving useful completed analyses or silently apply
invalid text. Revisit with a separately reviewed draft-recovery schema. Legacy
configure/widget rejection records likewise retain their rejected input projection.
All new projection/serialization fixtures are unexecuted.

Raw-input snapshots retain the current global form diagnostic explicitly, without falsely assigning it to every historical draft.
