# Native offline electrical and power authoring — T52

This bounded batch extends the accepted retained Study path, not the historical
whole-mission inventory. The field-by-field reference map is
[native-power-legacy-inventory.md](native-power-legacy-inventory.md). Source reading
is the verification contract: implementation and written fixtures are reviewed,
but compilation, execution, GUI parity, numerical qualification and platform
publication behavior are unverified. No legacy implementation is retired.

Launch when execution is authorized using the existing shell:
`cargo run -p sim-spatial -- --system examples/systems-builder/motor-driver-board/board.system.json`.
Navigate **Build → Actuators → Offline measured-PWM study**. Open an identification
archive or saved Study, then use **Electrical source and controller feedback**.
Add a regulated source or illustrative battery; edit registry parameter rows with
units, auxiliary current, provenance and the six optional source limits. Enable
hypothetical electrical feedback and declare voltage/current resolutions, sensing
evidence, compensation and sampled controller limits. Blank optional values mean
absent, never a passing threshold. Existing controller timing rows capture period,
observation delay, command delay and sample-age assumptions.

Simulate through the shared runtime. Select retained controller runs, recordings
and electrical predictions. Both recorded-command replay and own-feedback closed
loop prediction retain their existing meanings. Select a prediction and compare
its captured servo voltage, or enter a calibrated sidecar JSON path. File reads,
validation and execution happen in adopted Study jobs. Import is comparison against
the selected immutable prediction, not acquisition or a new calibration owner.
Review captured electrical samples, sampled peaks, energy, calibrated channel
residuals, circuit locations, signed gain/offset, uncertainty and observation windows
separately from motion tracking. Full traces stay in retained evidence; the native
panel shows bounded exact numeric previews. Review decisions do not promote values.
Use existing **Save new review** or export controls with a fresh destination; reopen
through **Saved study JSON** with its sibling `.study-inputs` companions intact.

Actual buttons/text entry and `system_ui` resolve the same rendered `Hit` values.
They produce stamped `StudyAction::RefineApply`, `RefineRun` or `ImportElectrical`.
REST `system_measured_study` uses those same actions; `op: status` supplies current
study identity/revision. `refine_apply` accepts externally tagged shared
`{"Electrical":{"SetSource":...}}` / `SetController` / selection commands.
`refine_run` accepts `{"Electrical":{"CompareServoVoltage":{"recording_hash":"…","prediction":0}}}`.
`import_electrical` takes `stamp`, `path` and immutable prediction index. Legacy
`configure`, `simulate`, `predict` and `compare_electrical` payloads stay compatible
and delegate reusable electrical behavior to the shared contract.

## Ownership and preservation

Study owns durable source/controller settings, electrical selections, comparisons,
review decisions, execution receipts and immutable input companions. StudyOwner's
existing action writer is authoritative; StudyUi owns only raw text and presentation
state. Existing Input → Actions → JobResults → SimSync → Present ordering is unchanged.
Buttons and Enter are occurrences; jobs and drafts are retained state. Existing panel
children own transient controls and teardown. No new simulation, persistence owner,
feature thread, text editor or held-motion behavior is introduced.

Preparation captures exact recording, prediction and calibrated measurement content
in the existing content-addressed Store. Receipts contain bounded references and
identities, not nested prior Studies or receipts. Original sidecar bytes, including
unknown/malformed input, are retained before parsing. Refused typed authoring keeps
its original state and command content. Current, stale, cancelled, failed and
switched/displaced completions remain on the original study or orphan receipt;
terminal work cannot replace newer drafts. Late cancellation retains the completed
execution in the receipt but refuses scored comparison attachment. Existing T51
publication snapshots retain raw unsubmitted form inputs; revision-scoped
acknowledgment never marks newer edits saved. Visible but durability-unconfirmed
publication remains failure and needs a fresh immutable destination.

Voltage-only servo evidence has no calibrated current or measured energy. Imposed
voltage cannot independently pass voltage validation. Supply and winding locations
remain distinct. Derived measured watts require exactly synchronized voltage/current
sample windows at the same explicitly named circuit; joules integrate that sampled
curve over its measurement interval. Sampled peaks are not switching peaks. Sampled
protection requests zero PWM after the declared delay; it is not battery disconnection.

Decisions: explicit source installation atomically clears the incompatible fixed
voltage override, matching legacy preset intent; removal does not invent a new voltage.
Malformed sidecars retain a rejected-input identity rather than executing a fallback
comparison. Successful comparison receipt operations use a bounded captured identity.
Raw unknown source/controller authoring is refused rather than silently normalized;
opaque existing Study fields and immutable original companions remain preserved.
These decisions are reversible; revisit if a reviewed richer shared source schema
supports the unknown fields or a different explicit voltage/source policy is needed.

## Bounded acceptance checklist

| Required ID | Source-only evidence |
|---|---|
| native-power-authoring:outcome-1 | Shared electrical commands; existing StudyAction apply owner; actual renderer collector and legacy delegates |
| native-power-authoring:outcome-2 | Study electrical Evidence, immutable comparison content companions, stamped jobs and existing save/reopen validation |
| native-power-authoring:outcome-3 | Existing control simulation, recording prediction and electrical_measurements evaluation; truthful calibrated/derived/hypothetical/unscored review |
| native-power-authoring:outcome-4 | Legacy power projection/configuration/compare dispatch delegate shared contracts; reference UI retained |
| native-power-authoring:task-T52.1 | Field inventory, transactional validators, bounded captured comparison inputs, additive compatibility and unexecuted electrical fixtures |
| native-power-authoring:task-T52.2 | Structured actual fields/buttons, stamped source/revision submissions and retained existing jobs |
| native-power-authoring:task-T52.3 | Captured electrical review/report, decisions, exact companions/reopen and written actual-control/lifecycle/publication fixtures |

Remaining external requirements: Python/RoboCAD/OCCT and CAD parity (§§8–9), FPGA
refinement and FPGA recording workflows, raw sweeps, hardware acquisition/calibration
and driving, serial writes/FPGA loading, accepted actuator registry promotion,
physical-source promotion and sim-to-real qualification. Browser/reference paths
stay available. Offline electrical authoring/comparison described here no longer
requires an external UI; no broader hardware or CAD parity is claimed.

## Final source traces

Paths below are relative to the repository. These are reading traces, not execution receipts.

| Workflow | Exact source path:line |
|---|---|
| Open archive / saved study → existing Io load and hydrate/validate | `crates/sim-spatial/src/builder/calibration/study/actions.rs:127`; `jobs.rs:85`; `crates/sim-runtime/src/experiment_study/publication.rs:24` |
| Actual source/feedback controls → raw stamped form → shared validated command | `study/electrical_ui.rs:20` and `:43`; `study/electrical_forms.rs:35`; `study/forms.rs:145`; `study/actions.rs:151`; `study/recording_jobs.rs:62`; `crates/sim-runtime/src/experiment_study/electrical.rs:17` and `:122` |
| system_ui / REST → same authoritative apply owner | `crates/sim-spatial/src/builder/calibration/study/actions.rs:30`, `:46`, `:83`; actual entity collector `study/ui.rs:43` |
| Simulate / both prediction purposes → captured existing execution | `study/electrical_ui.rs:52`, `:58`; `study/jobs.rs:140`; `study/recording_jobs.rs:180`; `crates/sim-runtime/src/experiment_study/refinement.rs:190` |
| Captured servo voltage / calibrated sidecar → exact companions and job validation | `study/electrical_ui.rs:64`; `study/actions.rs:164`; `study/recording_jobs.rs:168`; `crates/sim-runtime/src/experiment_study/electrical.rs:170`, `:176`, `:208` |
| Original-study attachment; cancellation cannot yield passing vectors | `study/jobs.rs:314`, `:321`, `:328`; `crates/sim-runtime/src/experiment_study/refinement.rs:203`, `:231` |
| Separate captured electrical trace/calibration/protection review | `study/electrical_ui.rs:71`, `:80`, `:89`; `crates/sim-runtime/src/experiment_study/report.rs:101` |
| Save-new/reopen, exact companions, review and raw drafts | `study/forms.rs:60`; `study/jobs.rs:166`, `:176`, `:340`; `crates/sim-runtime/src/experiment_study/publication.rs:12`, `:24`; `experiment_study/electrical.rs:233` |
| Legacy source/config/compare consumers delegate shared contracts | `crates/sim-viewer/src/experiments_ui/power_ui.rs:440`; `experiments_ui/rest.rs:148`; `experiments_ui/refinement.rs:178`; `experiments_ui.rs:240` |
| Written compatibility / actual-control / modal / lifecycle / publication fixtures | `crates/sim-runtime/src/experiment_study/electrical_fixtures.rs:1`; `study/electrical_ui_tests.rs:1`; `study/ui_tests.rs:421`; `study/electrical_lifecycle.rs:1`; `study/publication_lifecycle.rs:41`, `:79` |

`study/` abbreviates `crates/sim-spatial/src/builder/calibration/study/` only in this
table. Existing publication fixtures and source gates apply to captured electrical
Studies without a separate writer. Deserialized comparison results are review-only;
only a private nonserialized capability from shared job validation permits terminal
attachment. Reopen recomputes and crosschecks exact companions instead. Legacy draft
keys include append-only retained study identity; refused selections restore their
submitted values. These review-driven decisions prevent result fabrication and
cross-study raw-draft leakage without moving expensive validation onto frame systems.

Reopen first hydrates companions through Store metadata/path validation, then validates
the Study against their exact content. Reading found that validation before hydration
could never reopen new calibrated captures; publication policy and acknowledgment
owners are unchanged. Missing/corrupt companions still refuse reopening.
