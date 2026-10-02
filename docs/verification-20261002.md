# Verification pass: 2026-10-02

This pass checks the accumulated range `aa34ef48433f..f541b05f7845` and its
verification repairs. It adds no features. Portable-study-artifacts remains
set aside and unaccepted; this pass does not accept T53 or replace the pending
LC1–LC3 calibration batch. CAD and measured physical definitions remain their
existing authorities. Concurrent architecture and coordinator changes are not
part of these commits.

Full command output is retained under `runs/verification-20261002-f541b05f/`;
window/REST receipts and screenshots are under
`.claude-pair/captures/verification-f541-*`. These ignored artifacts are
qualification evidence, not checked-in baselines.

## Repairs

- Compiler: repaired private fixture access, partial-move rejection evidence,
  fixture call signatures and expressions, and stale helpers/imports. CAD
  composition metadata required touching the unchanged sim-diagram root to
  invalidate stale local metadata; no sim-diagram source change was committed.
- Source review: cancelled or abandoned close requests cannot grant close intent
  or preference-loss consent. Independent STOP still precedes refusal. Higher
  modals preserve the underlying picker draft and own Escape/wheel input.
- Source review: adopting any retained evaluation exposes its trial evidence
  before applying the candidate, including an evaluation outside the current
  view. Held-out influence stays monotonic.
- Fresh window logs: remove redundant direct public-set membership when a leaf
  already belongs to that set transitively. Existing set nesting, chain order,
  run conditions and public ordering edges remain intact.
- Executed fixtures: construct recording fits with their actual imported source
  and held-out assignment; use delta bounds containing zero; attach legacy
  `SharedInputs` through the production adapter. Canonicalize only the fixture's
  trusted temporary parent before testing no-follow publication.
- Source review: cancelled component submissions preserve drafts; export
  cancellation retains its job until the actual terminal result, never promises
  to revoke a completed rename, and cannot overlap queued writers. Profiles-file
  reads retain decoded input and validate source/revision at completion and apply.
- Executed CAD harness: on Darwin, verify a bounded, stable group containing only
  zombies before accepting group-signaling `EPERM`. Keep the leader unreaped;
  ownership-observation failures permanently revoke future PID/group operations.
  See [the harness contract](cad-parity-harness.md).

- Native tests: a late cancellation request must not rewrite the runtime outcome
  in a retained receipt. Attachment remains cancelled/unscored while the actual
  worker outcome and separate request metadata remain truthful.
- Native tests: deferred activation tolerates a control disappearing before
  application (`try_insert`); unknown operation history uses its catalogue label.
  Fixtures now set authoritative document identity, select the visible top STOP,
  and follow actual input-row Tab navigation. Source guards exclude test-only
  keyboard injection but continue checking production text ownership and copy.

## Bevy ownership and responsibilities

No new entity, resource, document, persistence service, task or physics path is
introduced. Existing kit buttons and text fields remain per-entity interaction
state. CloseOwner and Picker remain global lifecycle/draft state under their
existing services; their pending state survives message expiry. Actions are
occurrences drained by the existing apply owner. Input and completion ordering
continues through public sets. Transient UI setup/teardown is unchanged. Frame
systems handle input/presentation; expensive retained-content and publication
work remains in existing jobs/runtime owners. The changed Bevy signatures were
read in pinned 0.19.1 source by the implementer and reviewers.

## Executed window scope

Build mode rendered its motor-driver board; delayed control discovery reached
readiness and the actual Studies and Actuators controls activated. Native CAD
attached to an isolated wheeled-model copy: model-tree selection, physical read,
annotation pin, reply, thread dock, save, a new server/viewer reopen, and resolve
were exercised through shared REST actions. The reopened thread retained its
identity and both messages. This is bounded evidence, not all Parts A–J parity.

The virtual bench alone supplied the copied calibration server's serial path
(`/dev/ttys007` during this pass). A fresh native Robot window connected with
fresh telemetry; rendered STOP activation succeeded. Remote motor selection
was refused by the existing gate. HW-01 connection and idle STOP are observed;
HW-02–HW-09 motion/interruption acceptance remains unverified and needs LC1–LC3.
No physical endpoint was opened. Server output and the CAD copy are isolated
under this pass's retained directory.

The initial CAD harness report was incomplete during preflight because of the
Darwin cleanup defect. Its original receipt remains retained. Same-authority
Python/OCCT comparisons cannot establish an independent Rust CAD replacement.

## Resource decision

When headroom reached 3.4 GiB, two unused October-1 compiler caches were removed:
`target/debug/incremental/sim_spatial-12b6gge42b4mm` and
`target/debug/incremental/sim_spatial-14i8tklorrpzk`. No active compiler used them.
Their nominal sizes totalled about 8 GiB; measured headroom rose to 8.8 GiB while
compilation continued. Binaries and retained data were preserved.

## Command receipts and limits

Earlier cargo elapsed times include lock/queue waits. Commands interrupted by
coordinator restarts have no completion claim. The coordinator subsequently
changed the dev profile to line tables and cleared its old cache; that protected
configuration edit is outside the worker commits.

| Command | Recorded result | Duration |
| --- | --- | --- |
| `cargo check --workspace --all-targets` | Initial compiler failures repaired; fourth attempt passed. Final post-repair attempts were interrupted. | Passed attempt 34m36s including waits |
| `cargo build -p sim-spatial -p sim-viewer` | Passed | 24m55s including waits |
| `cargo build -p sim-spatial` | Passed after schedule repairs; later queued attempt interrupted | 40m50s including waits |
| `cargo build -p sim-runtime --bin cad_parity --example hx_virtual_bench --example serve_actuator_calibration` | Passed | 31m49s including waits |
| `cargo test -p sim-runtime --lib experiment_study::` | 62 passed, two invalid fixtures failed and were repaired | Build 28m29s; tests 465.87s |
| `cargo test -p sim-runtime --lib experiment_study::refinement::recording_fixtures::` | All 12 passed, including the two repaired fixtures | Build 34m24s; tests 1.00s |
| `cargo test -p sim-runtime --lib cad_parity::` | 37 passed, one temporary-parent fixture failed and was repaired; all three actual-child Darwin fixtures passed | Build 31m51s; tests 2.03s |
| `cargo test -p sim-viewer --bin sim-viewer experiments_ui::` | 17 passed, one old-outcome fixture failed and was repaired | Build 30m18s; tests 1.57s |
| `cargo test -p sim-spatial --lib` | Initial orphan-doc parse error repaired; next command interrupted during compilation | No test completion |
| `cad/.venv/bin/python -m pytest -q tests/test_parity_reference.py tests/test_component_jobs.py tests/test_component_service.py tests/test_experiment_api.py robocad/test_motion_continuation_contract.py robocad/test_motion_service_contract.py` (from `cad`, executable `.venv/bin/python`) | 48 passed; one fixture bypassing Dispatcher construction failed and was repaired | 67.57s |
| `.venv/bin/python -m pytest -q tests/test_parity_reference.py::test_captured_source_missing_never_falls_back_to_live` (from `cad`) | Passed | 8.11s |

Each window probe has an exact launch argv, REST requests and results in its
retained receipt. Initial probes with an incorrect action shape, missing UI
revision, premature listener/control discovery or missing token page failed;
the succeeding retries are listed below rather than treating those failures as
acceptance. Original failed receipts are retained.

| Exercised step | Result and screenshot under `.claude-pair/captures/` |
| --- | --- |
| Build control discovery; Studies activation | Pass: `verification-f541-studies-activation/activated.png` |
| Actuators registry activation | Pass: `verification-f541-actuators/activated.png` |
| Build fresh-window schedule/error check | Pass: `verification-f541-final-build/build-final.png` |
| Phenomena fresh-window crash check | Pass: `verification-f541-phenomena/phenomena.png` |
| CAD source selection and physical read | Pass: `verification-f541-cad/cad-controls.png` and annotation receipt |
| CAD node annotation, reply, dock, save | Pass: `verification-f541-cad-annotations-retry/activated.png` |
| CAD new-service/new-window reopen and resolve | Pass: `verification-f541-cad-reopen-ready/reopened-resolved-thread.png` |
| HW-01 fresh virtual connection and idle HW-04 STOP | Partial acceptance only: `verification-f541-hw01-connected/activated.png`; motor selection correctly refused |

All seven `native-leg-calibration-completion` IDs (outcome-1 through outcome-4
and task-LC1 through task-LC3) remain pending. Full HW-01 reconnect/staleness and
HW-02–HW-09 motion/interruption checks were not established. No Parts A–J or
independent CAD migration qualification is claimed. Three pre-existing warnings
remain: sim-print `material`, runtime `ForceSlot::decision`, and the
`build_lesson_systems` example's `BRASS`. Their sources are unchanged in the
assigned range.

## Resumed profile verification

`cargo check --workspace --all-targets` passed in 10m34s after the coordinator's
profile/cache change. `cargo test -p sim-spatial --lib` compiled in 30m02s and
ran in 34.77s: 762 passed, 15 failed, one ignored. The failures revealed the
receipt/activation/history bugs and stale fixture/guard assumptions described
above; their targeted rerun is recorded below when complete.

`cargo test -p sim-viewer --bin sim-viewer experiments_ui::refinement::recording_tests::legacy_import_and_both_prediction_actions_delegate_shared_capture`
passed (build 8m23s; one test 0.44s), resolving the earlier legacy failure.

Profile-file completion deliberately compares the conservative local intent
revision as well as the authoritative source stamp. An unrelated UI touch may
therefore refuse an otherwise reusable decoded file; the exact path/content
remains retained for explicit retry. This prevents applying input over an edit
whose new server snapshot has not arrived.

Selected runtime command:

```sh
cargo test -p sim-runtime --lib -- cad_parity::publication::report_fixtures::planning_bundle_atomic_and_refuses_overwrite controller_refinement:: publication:: cad_client:: composition::
```

Build 5m27s; tests 1.63s: 132 passed and three old error-message expectations
failed. The corrected fixtures require exact raw errors/status/routes plus the
uncertain-mutation hint. The failed-only command below passed all three (build
30.27s, tests 0.02s):

```sh
cargo test -p sim-runtime --lib -- cad_client::physical_tests::results_files_are_named_by_path_and_errors_pass_through cad_client::print_tests::print_ops_send_the_python_signature cad_client::robot_tests::refusals_carry_robocads_text_and_status
```

Native failed-path rerun:

```sh
cargo test -p sim-spatial --lib -- refinement_lifecycle:: cad::composition:: cad::files::form::suspension_tests:: cad::ops::tests::every_entry_builds_calls_to_its_route cad::references::tests::controls_fit_the_pattern_and_round_trip_through_rest cad::surfaces::form::suspension_tests:: copy_guard_tests::window_text_does_not_send_people_to_rest robot::hardware::actions::input::activation_fixtures:: ui_kit::activation_tests::actual_hidden_disabled_and_despawned_controls_refuse_capture ui_kit::text::tests::keyboard_text_is_read_only_in_the_kit
```

Its first attempt caught a fixture import typo (`AccessibleLabel` is in
`bevy::ui::prelude`); corrected second attempt built in 3m08s and ran 0.26s:
28 passed, two stale source-scan/displayed-identity fixture expectations failed.
Final failed-only rerun, including both guards, passed all three (build 2m54s;
tests 0.19s):

```sh
cargo test -p sim-spatial --lib -- cad::composition::tests::drawn_pending_buttons_keep_shared_actions_without_a_usable_snapshot ui_kit::text::tests::keyboard_text_is_read_only_in_the_kit copy_guard_tests::window_text_does_not_send_people_to_rest
```

Thus every native and selected-runtime failure was individually repaired and
passed; unchanged passing tests were not repeated. One existing expensive
robot-preset test remains ignored. This is fixture verification, not all
workflow or numerical qualification.

## Final binaries and harness

```sh
cargo build -p sim-spatial -p sim-viewer -p sim-runtime --bin sim-spatial --bin sim-viewer --bin cad_parity --example hx_virtual_bench --example serve_actuator_calibration
```

Passed in 14m27s with no new warnings. The final fresh Build command below
succeeded, its screenshot was viewed, and its viewer log contained no warnings,
panics or deferred-command errors:

```sh
python3 tools/claude-pair/ui_capture.py --out .claude-pair/captures/verification-f541-profile-final --steps '[{"wait":8},{"command":"system_ui","args":{"action":{"operation":"controls"}}},{"command":"system_state"},{"screenshot":"profile-final"}]' -- --system examples/systems-builder/motor-driver-board/board.system.json
```

Screenshot: `.claude-pair/captures/verification-f541-profile-final/profile-final.png`.
Exact argv, start/finish times and successful REST steps are in `capture.json`.

The first repaired-process harness run completed in 50.966s (exit 0, reports
published) and exposed a five-second startup bound leaking into physical reads.
That harness-only repair was rebuilt with `cargo build -p sim-runtime --bin
cad_parity` (passed, 2m39s), then executed at a new destination:

```sh
target/debug/cad_parity /Users/elliot/physics-simulator /Users/elliot/physics-simulator/examples/cad-parity/wheeled.json wheeled-contracts /Users/elliot/physics-simulator/runs/verification-20261002-f541b05f/parity-wheeled-request-bounds verification-f541-request-bounds --execute --cancel-file /Users/elliot/physics-simulator/runs/verification-20261002-f541b05f/parity-cancel
```

Completed in 44.118s, exit 0. Observe, rename, undo, redo, stale-edit refusal
and physical export passed on both adapters; owned cleanup had no reported
error. All three qualification gates remain false because the original model
lacks explicit provenance/uncertainty, export timestamps/isolated paths differ
as declared, and the adapters share Python/OCCT authority. No tolerance was
widened and no missing physical evidence was invented. Original failed and
incomplete receipts remain preserved.

All worker-started windows, isolated CAD servers, calibration server and virtual
bench were stopped. No physical endpoint, FPGA operation, remote push, paid
compute or deployment occurred. The coordinator's profile/cache/coordinator
edits remain unstaged.

`cargo test -p sim-system --lib -- composition resolve::` passed all three
composition fixtures (build 1m32s; tests <0.01s). Final capture took 18.22s.
Routine `rg`, `sed`, `git diff/status/log`, process/listener checks and
`git diff --check` were read-only or repository inspection and completed in
roughly a second each; owned diff checks passed.

## Repair source anchors

- `crates/sim-spatial/src/app/close.rs:111`: abandoned close refusal.
- `crates/sim-spatial/src/app/picker/modal.rs:26`: suspended draft retention;
  `:86` gives higher modal input ownership.
- `crates/sim-runtime/src/experiment_study/commands.rs:100`: retained evaluation
  exposes evidence before candidate editing.
- `crates/sim-spatial/src/app/mod.rs:269`: public pipeline ordering; nested
  feature systems retain their existing leaf membership.
- `crates/sim-runtime/src/cad_parity/process.rs:154`: bounded zombie-only
  verification; `:220` permanently revokes lost wait ownership.
- `crates/sim-spatial/src/cad/components/mod.rs:150`: cancelled submission refusal.
- `crates/sim-spatial/src/cad/results/export.rs:193`: retained cancellation;
  `:237` consumes the actual terminal before starting queued work.
- `crates/sim-spatial/src/cad/results/link.rs:249` and `results/mod.rs:409`:
  captured profile source/revision checks and retained decoded input.
- `crates/sim-spatial/src/builder/calibration/study/jobs.rs:354`: exact worker
  cancellation observation retained separately from late requests.
- `crates/sim-spatial/src/ui_kit/activation.rs:124`: expired target tolerates
  deferred activation without an error.
- `crates/sim-spatial/src/cad/ops/args.rs:376`: operation label fallback.
- `crates/sim-runtime/src/cad_parity/native.rs:103`: shared request/edit bounds.

## Owned file manifest

The cumulative repair commits after `f541b05f` changed the following files.
Concurrent Cargo, architecture and coordinator files are excluded.

- `cad/tests/test_parity_reference.py`
- `crates/sim-runtime/src/cad_client/physical_tests.rs`
- `crates/sim-runtime/src/cad_client/print_tests.rs`
- `crates/sim-runtime/src/cad_client/robot_tests.rs`
- `crates/sim-runtime/src/cad_client/tests.rs`
- `crates/sim-runtime/src/cad_parity/native.rs`
- `crates/sim-runtime/src/cad_parity/process.rs`
- `crates/sim-runtime/src/cad_parity/process_fixtures.rs`
- `crates/sim-runtime/src/cad_parity/publication.rs`
- `crates/sim-runtime/src/experiment_study.rs`
- `crates/sim-runtime/src/experiment_study/commands.rs`
- `crates/sim-runtime/src/experiment_study/recording_fixtures.rs`
- `crates/sim-spatial/src/app/close.rs`
- `crates/sim-spatial/src/app/close/tests.rs`
- `crates/sim-spatial/src/app/close/ui.rs`
- `crates/sim-spatial/src/app/mod.rs`
- `crates/sim-spatial/src/app/picker/activation_tests.rs`
- `crates/sim-spatial/src/app/picker/modal.rs`
- `crates/sim-spatial/src/app/switch/mod.rs`
- `crates/sim-spatial/src/builder.rs`
- `crates/sim-spatial/src/builder/actions.rs`
- `crates/sim-spatial/src/builder/calibration.rs`
- `crates/sim-spatial/src/builder/calibration/study/actions.rs`
- `crates/sim-spatial/src/builder/calibration/study/jobs.rs`
- `crates/sim-spatial/src/builder/calibration/study/mod.rs`
- `crates/sim-spatial/src/builder/calibration/study/recording_ui.rs`
- `crates/sim-spatial/src/builder/calibration/study/ui.rs`
- `crates/sim-spatial/src/builder/calibration/study/ui_tests.rs`
- `crates/sim-spatial/src/builder/drafts.rs`
- `crates/sim-spatial/src/builder/placement/tests.rs`
- `crates/sim-spatial/src/cad/actions.rs`
- `crates/sim-spatial/src/cad/attach.rs`
- `crates/sim-spatial/src/cad/components/jobs.rs`
- `crates/sim-spatial/src/cad/components/mod.rs`
- `crates/sim-spatial/src/cad/components/tests.rs`
- `crates/sim-spatial/src/cad/components/ui.rs`
- `crates/sim-spatial/src/cad/composition/mod.rs`
- `crates/sim-spatial/src/cad/composition/tests.rs`
- `crates/sim-spatial/src/cad/composition/ui.rs`
- `crates/sim-spatial/src/cad/display/mod.rs`
- `crates/sim-spatial/src/cad/experiment_review/mod.rs`
- `crates/sim-spatial/src/cad/experiment_review/ui.rs`
- `crates/sim-spatial/src/cad/experiments/mod.rs`
- `crates/sim-spatial/src/cad/experiments/ui.rs`
- `crates/sim-spatial/src/cad/files/form.rs`
- `crates/sim-spatial/src/cad/files/mod.rs`
- `crates/sim-spatial/src/cad/flow_tests.rs`
- `crates/sim-spatial/src/cad/inspector/mod.rs`
- `crates/sim-spatial/src/cad/inspector/physical_edit.rs`
- `crates/sim-spatial/src/cad/materials/mod.rs`
- `crates/sim-spatial/src/cad/mod.rs`
- `crates/sim-spatial/src/cad/motion/mod.rs`
- `crates/sim-spatial/src/cad/motion/ui.rs`
- `crates/sim-spatial/src/cad/ops/args.rs`
- `crates/sim-spatial/src/cad/panel.rs`
- `crates/sim-spatial/src/cad/pick.rs`
- `crates/sim-spatial/src/cad/print/edits_tests.rs`
- `crates/sim-spatial/src/cad/print/fastener_tool.rs`
- `crates/sim-spatial/src/cad/print/jobs_tracker.rs`
- `crates/sim-spatial/src/cad/references/calibrate.rs`
- `crates/sim-spatial/src/cad/references/drop.rs`
- `crates/sim-spatial/src/cad/references/input.rs`
- `crates/sim-spatial/src/cad/references/tests.rs`
- `crates/sim-spatial/src/cad/results/export.rs`
- `crates/sim-spatial/src/cad/results/forms.rs`
- `crates/sim-spatial/src/cad/results/link.rs`
- `crates/sim-spatial/src/cad/results/mod.rs`
- `crates/sim-spatial/src/cad/results/overlay.rs`
- `crates/sim-spatial/src/cad/results/tests.rs`
- `crates/sim-spatial/src/cad/robot/mod.rs`
- `crates/sim-spatial/src/cad/robot/panel.rs`
- `crates/sim-spatial/src/cad/robot/tools.rs`
- `crates/sim-spatial/src/cad/selection/mod.rs`
- `crates/sim-spatial/src/cad/surfaces/form.rs`
- `crates/sim-spatial/src/cad/surfaces/mod.rs`
- `crates/sim-spatial/src/cad/surfaces/registry.rs`
- `crates/sim-spatial/src/cad/threads/annotate.rs`
- `crates/sim-spatial/src/cad/threads/input.rs`
- `crates/sim-spatial/src/cad/threads/pins.rs`
- `crates/sim-spatial/src/cad/transform/mod.rs`
- `crates/sim-spatial/src/cad/tree.rs`
- `crates/sim-spatial/src/cad/views/mod.rs`
- `crates/sim-spatial/src/camera/mod.rs`
- `crates/sim-spatial/src/copy_guard_tests.rs`
- `crates/sim-spatial/src/inspect_view/mod.rs`
- `crates/sim-spatial/src/lesson/mod.rs`
- `crates/sim-spatial/src/lib.rs`
- `crates/sim-spatial/src/notes.rs`
- `crates/sim-spatial/src/phenomena/mod.rs`
- `crates/sim-spatial/src/place_view.rs`
- `crates/sim-spatial/src/robot/hardware/actions/input.rs`
- `crates/sim-spatial/src/robot/mod.rs`
- `crates/sim-spatial/src/ui_kit/activation.rs`
- `crates/sim-spatial/src/ui_kit/text/tests.rs`
- `crates/sim-viewer/src/experiments_ui.rs`
- `crates/sim-viewer/src/experiments_ui/recording_tests.rs`
- `crates/sim-viewer/src/experiments_ui/refinement.rs`
- `docs/cad-checklist.md`
- `docs/cad-parity-harness.md`
- `docs/verification-20261002.md`
