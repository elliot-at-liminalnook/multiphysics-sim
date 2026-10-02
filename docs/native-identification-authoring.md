# Native offline measured-PWM studies — T46

This batch implements T46.1–T46.3 in Build → Actuators → Measured evidence.
Verification is source reading only: fixtures are written but unexecuted; no
compilation, window launch, export execution, screenshot or parity run is evidence.
Legacy `sim-viewer --experiments` remains available. Hardware acquisition and
accepted registry/CAD promotion are outside this workflow.

## Ownership and scheduling contract (recorded before implementation)

Transient controls and chart-image entities belong to the existing builder dock
and its linked children; closing/rebuilding that dock despawns presentation only.
The global `StudyOwner` retains study identities, revisions, pending jobs and
receipts independently of Builder replacement. The global `StudyUi` retains stamped
text intent and unresolved submissions. The study action handler is the
one writer of drafts; its result handler is the one publisher of captured job
results. Durable `Study` files remain runtime-owned versioned data, and archived
observations and accepted registry values remain immutable physical evidence.

Input produces typed action occurrences; durable jobs and cancellation requests
remain resource state, not messages that can expire while a mode is inactive.
Public ordering is ViewerSet Input → Actions → JobResults → SimSync → Present.
The shared command owner validates transactional mutations in Actions; jobs run
archive/review I/O, existing runtime evaluation and immutable publication; frame
systems synchronize and present small projections. No viewer physics is added.

Study identities never reuse a number, even at the same revision. Captures name
that identity, source/model hashes, document identity and exact input revision.
Completion attaches to its retained owner, never whichever study happens to be
active. Publication acknowledgments name their captured revision. Spatial picks
continue through shared Selection; saved trial/evaluation view fields are study
review state, not a competing spatial selection model.

New Bevy APIs must be read from pinned 0.19.1 sources, following
`tools/claude-pair/prompts/bevy.md`; no obsolete APIs or feature-owned task pools.

Decision: retain studies globally rather than embed authored studies in Builder.
This preserves drafts and pending evidence across replacement and avoids silent
rebasing. Rejected alternative: replace the read-only archive slot with one mutable
study. Revisit if an authoritative multi-document evidence registry supersedes this
owner. Decision: exploratory candidates never publish measured physical truth;
refinement and promotion remain separate reviewed workflows.

## Batch checklist

- native-identification-authoring:outcome-1
- native-identification-authoring:outcome-2
- native-identification-authoring:outcome-3
- native-identification-authoring:outcome-4
- native-identification-authoring:task-T46.1
- native-identification-authoring:task-T46.2
- native-identification-authoring:task-T46.3

All IDs above are retained for independent source acceptance. Implementation and
review evidence will be recorded below; their presence alone is not acceptance.

## Source reading trace and launch path

Launch the existing viewer with a system, for example
`cargo run -p sim-spatial -- --system examples/systems-builder/motor-driver-board/board.system.json`,
then choose Actuators → Measured evidence. This launch command is documentation,
not a command executed in T46. The archive path starts with the retained HX archive;
Open archive calls `StudyAction::OpenArchive`. Open review calls `OpenReview` and
`Study::load`. Both go through `study/jobs.rs::start_load`, append an independent
retained identity and preserve existing studies. Choose selects an existing retained
study and records viewing exposure through shared commands.

`study/ui.rs::section` spawns kit path controls, candidate/conditions fields, trial
filters/rows and evaluation controls. `forms.rs::submission` parses field text into
the same stamped StudyAction carried by REST. Registry `commands::metadata` supplies
editable motor/bridge names, units and bounds. `commands::apply` transactionally
validates candidate, limits, view, notes and decisions; failed requests preserve
the previous validated Study. Held-out viewing/evaluation is monotonic exposure,
and subsequent candidate/limit edits mark validation influence.

Evaluate selected/filtered/held-out calls `jobs::start_evaluation`, which captures
Study settings, exact IDs, limits, source/document/native identity and exposure
before spawning the adopted Dedicated job. Only `experiment_study::evaluate` →
`simulate` → `actuator_bench` advances physics. Job progress and cancellation remain
in retained state. `jobs::poll_owner` publishes to the original retained ID, with
stale/displaced labels; it never publishes by active index or revision alone.
`ui.rs::status` provides global progress/cancellation outside the dock. Failure
receipts preserve captured inputs, and cancelled/incomplete trial pairs are unscored.

`builder/system_actions.rs` and `ui_api.rs::opens_system_ui` guard direct and
rendered-control document opening; `background.rs` rechecks before late load
publication and `open.rs::refuse_pending_open` preserves the prior document with
an observable refusal. `app/switch/prepare.rs` and `app/close.rs` also guard
unresolved text, dirty studies and pending evidence. Normal close is refused
until work is saved or explicitly resolved; abrupt process termination is not
durable recovery. Text acknowledgments clear only the accepted stamped submission,
never later typing. Optional component links use the existing document's shared
Selection. Evaluation capture includes runtime execution identity before spawning.

Select a captured evaluation and trial to inspect measured, archived prediction,
captured baseline and captured candidate series. `chart.rs` rasterizes through
shared chart behavior in Compute jobs, and guards publication by the captured chart
key. The panel renders runtime/model/source identity, assumptions, captured settings,
changes, per-trial errors, metrics and shared complete-pair summaries. Draft changes
mark captured evaluation settings stale instead of rewriting the traces.

Decision/notes controls use shared `SetDecision`/`SetNotes`. Save new and Export HTML
new call `jobs::start_publication` with an immutable Study snapshot; runtime
`save_new`/`export_html_new` validate and create a new destination with a hard link,
never overwrite an existing file. A cancellation/publication gate makes cancellation
before publication distinct from an already-started immutable write. Only a successful
matching Save acknowledgment marks that captured revision saved; HTML export and
failures leave dirty state intact. Reopen uses the same Study loader and preserves
captured evaluations, known deferred refinement fields and supported opaque sections.

`app/actions.rs` registers StudyAction; `app/route.rs` routes actual `study:*`
system_ui activations to that owner. The list is collected from spawned kit entities,
including enabled refusal states. `system_measured_study` accepts typed offline
operations and `status` reports retained studies/jobs/receipts. The older
`system_calibration_review` remains an archive-only compatibility API; the native
dock no longer starts an invisible duplicate archive load on first visit.

Normal close uses `app/close.rs::guarded_close`, preserving the pinned two-frame
ClosingWindow lifetime after a permitted request. Dirty studies, pending evidence
and unresolved form intent refuse silent closure. Mode switches and `system_open`
guard those same retained inputs. Abrupt termination, SIGKILL and process failure
cannot guarantee durability of unsaved in-memory work.

## Written evidence and limits

T46.1 source evidence: the field-by-field [inventory](native-identification-inventory.md),
runtime `experiment_study/commands.rs`, `compatibility.rs`, and legacy
`experiments_ui.rs` / `experiments_ui/rest.rs` adapters. T46.2/T46.3 source evidence:
native `study/actions.rs`, `state.rs`, `jobs.rs`, `forms.rs`, `ui.rs`, `chart.rs`,
`tests.rs`, `ui_tests.rs`, and normal-close fixtures in `app/close.rs`. The fixtures
inspect actual spawned controls/actions/enabled states, captured identities,
cancellation, displacement, nonfinite refusal and revision-scoped publication.
They are written and inspected only; no fixture was executed.

These traces cover all four outcome IDs and all three task IDs above for source
review. Independent acceptance remains the orchestrator's decision. They do not
establish compilation, interactive usability, numerical qualification, executed
round-trip parity or exact legacy feature parity. The Python/browser reference
paths stay. Controller/power/FPGA/refinement authoring and accepted actuator-registry
or CAD promotion still require their existing external tools and separately reviewed
evidence. This offline workflow needs no external UI to author measured-PWM studies.

T45 was accepted by source review at `6f3b701e`/`33d9da46`; its compilation and
execution remain unverified. T46 does not change that historical acceptance.

Review decisions: full REST study snapshots expose held-out traces, so both native
and legacy snapshot reads record validation exposure and dirty only newly exposed
studies. Rejected alternative: return full evidence while calling it unseen.
Revisit if a separate metadata-only snapshot contract avoids trace exposure.
Unresolved text is retained by its displayed study identity/revision until actual
action acceptance, or explicit discard. Parsing/enqueueing alone never acknowledges
a draft. Rejected configurations in the legacy staged editor remain saved rejection
evidence rather than disappearing after validated refusal.

Independent cross-readings covered shared/legacy compatibility, native lifecycle
and publication, and actual rendered controls/chart/routing. Review repairs included
system_ui's nested argument shape, pulse-only gates on saved archives, complete-pair
scores, uncollected path controls, unresolved text acknowledgment, duplicate chart
evaluation identities and full-snapshot split exposure. All review evidence is source
reading. The backend refused both attempts to allocate a fourth reviewer thread
(`agent thread limit reached`), so the three disjoint implementers independently
reviewed one another's areas in reviewer roles; nobody self-approved their own area.

## T46 review repairs

The repair retains all seven checklist IDs above and the full T46.1–T46.3 scope.
One shared complete-pair outcome requires both predictions and no baseline or
candidate errors. Summary, outcome filtering, native/legacy trial labels and HTML
consume it; candidate-only success remains UNSCORED while its traces and metrics
stay inspectable. Written fixtures exercise the actual HTML and legacy label helpers.

Evaluation capture stores `native_terminal` metadata tied to the original job,
study revision and document/source capture. `cancellation_requested` remains
distinct from `execution_cancelled`: cancellation after worker completion does not
change the runtime outcome. Serialization/reopening preserves the terminal record
and native inspection displays it. Save acknowledgments remain revision-scoped and
do not themselves make a successful saved draft dirty.

Decision: store terminal lifecycle metadata on the captured evaluation rather than
rewrite `Evaluation.cancelled` or add a dirty edit after every save acknowledgment.
The additive capture envelope preserves historical schemas and keeps request intent
distinct from runtime execution. Revisit if a versioned shared runtime lifecycle
record replaces this native capture extension. Decision: individual surviving
prediction metrics are evidence, but only a complete error-free pair has a scored
outcome; a candidate-only PASS would overstate validation.

Idle job/chart polling uses pinned change-detection facilities without declaring
semantic publication. Real terminal results, displacement, actions, text changes and
chart publication refresh presentation. Global progress/cancellation remains visible.
Written fixtures distinguish repeated idle frames from real publication and inspect
late-cancellation metadata after reopening. All fixtures remain unexecuted; these
source traces establish neither compilation nor executed UI/export parity.

Repair source owners: `experiment_study.rs::TrialResult::{pair,outcome}` and
`Study::render_html`, `experiment_study/commands.rs::filtered_ids`, legacy
`experiments_ui.rs::trial_outcome_label` and `experiments_ui/plots.rs`, native
`study/jobs.rs::{poll,publish}`, `study/actions.rs::apply`,
`study/ui.rs::{presentation_changed,comparison,collect}`, `study/chart.rs::receive`
and `study/forms.rs::input`. `builder/ui.rs::rebuild_panel` uses the same semantic
invalidation gate exercised by actual spawned-entity fixtures. Global status polls
progress independently; adapter cache updates do not invalidate their source panel.

Independent repair cross-readings cover scoring consumers, late-cancel/save races
and semantic publication. A dedicated reviewer allocation again failed with the
backend thread limit. Review repaired a fixture seam that initially inherited a
change tick from job injection: clearing trackers before terminal polling isolates
the real publication tick. Candidate-only fixtures explicitly retain a successful
prediction without errors, isolating the missing-baseline scoring refusal.
