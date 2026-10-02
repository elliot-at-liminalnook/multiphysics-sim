# T43 native CAD experiments and motion: source-reading evidence

2026-10-01. Implementation and written regression fixtures; no compilation,
fixture execution, viewer launch, screenshot, capture, export, parity harness or
physical qualification. `done-by-reading` describes traced source paths only.
RoboCAD and the browser remain available. No experiments, rejected candidates,
recordings, measurements or user sources were removed.

## Batch checklist

| ID | Source-reading evidence |
|---|---|
| cad-experiments-motion:task-T43.1 | cad_client/{experiments,candidates,motion}.rs contracts → api.py routes → captured_review.py / motion_service.py; isolated captured_document/document; Qt consumers share samples |
| cad-experiments-motion:task-T43.2 | cad/experiments/ui.rs actual kit controls → CadButton → CadAction::CadExperiments → mod.rs:handle → form/source/lifecycle/reads → typed client; completed checks import through shared composition |
| cad-experiments-motion:task-T43.3 | experiment_review/ui.rs and motion/ui.rs actual controls → typed handlers → worker jobs; isolated scene, chart raster jobs, evidence threads, reference samples and bounded jobs/video.rs encoder |
| cad-experiments-motion:task-T43.4 | Actual Button/CadButton/Enabled fixtures, analytic/wire/lifecycle fixtures written; 63 individual ledger owners and counts reconciled; source review and execution distinguished |
| cad-experiments-motion:outcome-1 | panel.rs persistent Experiments section; ui_api.rs system_ui catalogues the same actions; actions/specs/rest_form share typed handlers; captured requests include identity/revision/draft stamps |
| cad-experiments-motion:outcome-2 | experiment_review/mod.rs requests isolated geometry plus stamped samples/sources; scene.rs displays captured entities separately; Live maps IDs explicitly; only guarded candidate acceptance promotes |
| cad-experiments-motion:outcome-3 | motion_service.py snapshots reference PoseModel definition on owner, solves on worker; Qt reuses sample_model; native display applies matrices without source edits; programs use authoritative guarded undo |
| cad-experiments-motion:outcome-4 | docs/cad-parity.md individually maps all 63 rows; 46 done-by-reading, 17 deliberate differences; zero executed parity claims or retirement |

## Connected path

Launch path remains `cargo run -p sim-spatial -- path/to/model.rcad` (not run
in this batch), or attach through the existing CAD service URL controls. Opening
`.rcad` starts the existing Python headless service; **Python/OCCT is required**.
Experiment checks/runs/catalogue need the existing shared Rust registry and
experiment executables configured through RoboCAD's experiment process service.
Video export needs local `ffmpeg`. There is no required Qt window for the
assigned authoring, captured/candidate review, reference pose/program and native
export implementation. Other legacy GUI-only workflows remain compatibility
surfaces; phase 2 Rust derivations and OCCT bindings are outside this batch.

The actual panel creates Experiments, Captured review and Motion sections.
Each feature draws its dock toggle and cancellation/return controls before
snapshot/dock guards. Kit text fields use TextDraft/InputFocus; no second text
entry reader exists. Buttons carry CadButton typed actions and Enabled readiness.
`system_ui` gathers those same controls and activation calls actions::handle;
REST specs decode the same CadAction variants. The existing action system is
the single writer/apply path in ViewerSet::Actions.

Experiment editors retain system/controller/parameter JSON, fidelity/profile,
interface and linked source bundles. Check captures inputs and exposes imported
component metadata to the existing composition handler. Run has one stamped
start; status, diagnostics, history and cancellation stay observable. A cancel
acknowledgment still reporting Running remains active. Unknown POST outcomes (including mutating 5xx and receipt-write failures after acceptance) are
not retried: nonpublishing GET discovery identifies remote records by local
unique labels where possible. Source-changing requests use the common guarded
edit path and authoritative undo. Unknown source edit outcomes block further
edits and replacement until Refresh/source inspection plus explicit revision
acknowledgment; the retained uncertainty receipt is never silently erased.

Linked reads, catalogue/status/source reads, debounce starts and mesh/chart
work run through jobs; identity includes generation/document/revision and draft
sequence. New/rebased drafts preserve earlier rejected requests. Focused text
cannot write into a switched draft. Dock closure requests durable cancellation;
mode/document replacement waits for remote work receipts. No start is blindly
replayed after a transport error. Captured annotation/live-navigation controls carry review sequences and refuse retained old captures after document replacement.

Review requests Experiments.captured_document or Candidates.document, never
live meshes as a substitute. Captured matrices/sample time, signals, sources,
metrics, baseline matching, filters and flex arrows carry captured identity.
Captured sources are read-only. Explicit Live controls map still-existing IDs
to the shared selection. Annotate uses the one annotations service with typed
run/time/source/hash evidence and authoritative undo. Show on model for evidence
opens its captured run/sample rather than inventing a live model pin.

Motion samples the reference solver with declared units, actual limits distinct
from display ranges, driver/transmission/loop closure assumptions and residuals.
Program editor/validation/sweep/persistence remain source owned; native guarded
routes coexist with the original legacy program API. Play/pause/seek is labelled
kinematic, never physics or teleoperation. One scene owner saves/restores live
visibility/transforms/camera; Return, Escape, failure and teardown invalidate
late work and restore the display. Geometry edits refuse during previews;
annotation and named-program metadata alone have an explicit auxiliary scope,
with all identity/revision/busy/unknown-outcome guards preserved.

Export uses the native CAD viewport, one pending screenshot and a bounded
channel; reference samples precede capture. Worker cropping/resizing/encoding
uses local process infrastructure. Resolution/fps/duration bounds, frame and
encoder timeouts, progress, durable cancellation and owned temporary files are
explicit. A publication gate serializes cancellation with the final rename.
Failure/cancellation before publication leaves the existing destination alone;
a job deletes only its own temporary directory. No export/capture was executed.

## Architecture and decisions

Durable CAD owns source/undo/programs; feature resources own drafts, remote
receipts, capture identities and cancellable jobs; transient entities own kit
widgets/captured rendering. Core plugins drain jobs even with docks closed;
window plugins alone register scene/assets/capture. Ordering uses public
ViewerSet/CadSet/camera sets. Occurrences use typed actions, durable pending work
uses resource state, and no feature starts threads or raw pool tasks. Installed
Bevy 0.19.1 signatures for App/add_systems, MessageReader/Writer, state teardown,
Gizmos, Screenshot/ScreenshotCaptured, Image and viewport/relationship handling
were read by implementers/reviewers. Captures use the engine's current observer
API; kit widgets remain the existing project wrappers.

Decisions: keep reference kinematics/geometry in headless Python until exact
parity permits Rust migration; expose strict native program routes alongside
legacy routes; use numeric fields/timestamp controls instead of Qt sliders;
allow only annotation/program auxiliary metadata in previews; block ambiguous
mutations pending explicit inspection acknowledgment; bound exports to 30s and
1800 frames, 64 MiB/raw frame and 2 GiB temporary PNGs, with 720p/1080p and 24/30/60fps. Revisit presentation/bounds when
executed usability/performance evidence exists, and authority only after the
parity harness. Four-thread concurrency forced captured-review/motion into one
implementer; completed implementers independently cross-reviewed other areas
instead of spawning additional pair-reviewer threads.

## Written fixtures and reading-found repairs

Unexecuted: cad_client/experiments_tests.rs wire guards; Python
robocad/test_motion_service_contract.py analytic units/stops/captured samples
and isolated authority; cad/tests/test_experiment_api.py identity refusal;
cad/experiments/tests.rs retained drafts, unknown outcomes and drawn controls;
cad/experiment_review/tests.rs and cad/motion/tests.rs drawn cancellation and
stale-document controls; cad/flow_tests.rs drawn unknown acknowledgment and
central preview guards; cad/threads/evidence_tests.rs strict evidence; jobs/video.rs
publication gate/cleanup fixtures. These are not receipts of executed behavior.

Reading found and repaired: missing rendered dock toggles; late focused writes
into another draft; linked bundle readiness; cancellation acknowledgment races;
candidate accept/discard publication race; preview source mutation gaps; unknown
mutation outcomes losing visibility; missing evidence navigation; export cancel
racing publication; transient capture cleanup, missing explicit frame/evidence navigation controls, initial-sample chart invalidation and stale display restoration and completed-capture document replacement after the read job was gone. Mutating server failures and candidate receipt failures after source publication now explicitly retain unknown outcomes.
Compilation, visual correctness, process execution, exact feature parity,
performance and physical qualification remain unverified.
