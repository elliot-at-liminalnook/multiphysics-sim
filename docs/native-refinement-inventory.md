# Offline controller refinement field and compatibility inventory — T48

Source-reviewed implementation, 2026-10-02; no compilation, fixture execution,
launch, publication execution or parity execution. Native navigation is
`sim-spatial --system examples/systems-builder/motor-driver-board/board.system.json`
→ Build → Actuators → Measured evidence → open archive or saved review → controller
refinement. The command is a future launch instruction, not an executed receipt.
Legacy `sim-viewer --experiments` remains a reference/compatibility UI.

## Owners and field mapping

Legacy entry points are `experiments_ui/refinement.rs::State::show` sections 0
(controller), 2 (sensitivity/archive fitting), 3 (robustness), and
`experiments_ui/rest.rs` configure/refine. Native fields are spawned by
`builder/calibration/study/refinement_ui.rs`, parsed by `refinement_forms.rs`,
then submitted through `StudyAction::RefineApply` or `RefineRun` in the existing
Actions owner. Shared contracts are `experiment_study::refinement` and
`controller_refinement::authoring`; shared runtime types below are authoritative.
Every field edit is stamped and retained through the existing kit text service.

| Workflow / exact fields | Legacy control or REST | Shared contract and source owner | Native controls / migration dependencies |
|---|---|---|---|
| Experiment version, name, device, component_id, fixture | Controller name, motor ID, fixture, CAD component; configure.experiment | control::Experiment; SetExperiment; component linkage is provenance, not CAD mutation | Individual experiment fields; version remains schema-owned; selected component linkage uses existing Selection |
| Rust PID kp, ki, kd, integral_limit, duty_limit | Five numeric controls; complete experiment JSON for policy kind | control::Policy::RustPid; sim-domain-control PWM PID validation | Policy buttons and individual gain/bound fields |
| Rhai source, parameters, duty_limit | Source editor; complete experiment JSON for parameters and limit | control::Policy::Rhai; shared compile/policy validation, existing Rhai runtime | Source, structured parameter-value add/edit/remove rows and duty limit fields; parameter object is supplementary structured data, not the whole form |
| timing.period_s, encoder_quantum_rad, observation_delay_ticks, command_delay_ticks, velocity_filter_s, maximum_observation_age_s, evidence | Feedback/timing controls; configure.experiment | control::Timing; SetExperiment; measured timing remains external evidence | Individual timing fields and evidence text; provisional assumptions remain displayed |
| trajectory[].time_s, position_rad; duration_s | Knot rows/add/remove, duration | control::Knot ordered finite knots; shared transactional validation | Individual row fields and add/remove controls; immutable run capture |
| voltage_v, temperature_c, initial_encoder_rad, seed | Supply, temperature, initial encoder; JSON seed | control::Experiment; existing physical runtime | Individual condition/initial-state/seed fields; no missing robot defaults introduced |
| limits.rms_rad, peak_rad, settled_rad, maximum_saturation_fraction; notes | Predeclared limits and notes | control::TrackingLimits; SetExperiment | Individual task limits; tail error is final 10% maximum, not measured settling time |
| Simulate, target/feedback/truth/duty traces, tracking score, failure/cancel | Run controller; refine:simulate | Operation::Simulate → control::simulate; Run retains experiment/model/runtime/sample times | Run control, captured plots and metrics; no separate viewer execution |
| coordinates[].path, device, lower, upper | Row text/numbers/device/remove, resistance starter; configure.coordinates | calibration::Coordinate; SetCoordinates; registry parameter names/bounds and device validation | Individual coordinate rows and add/remove; shared Family device deltas |
| Selected sensitivity trial IDs; norms, rank, singular values, pair correlations, warnings | Analyze training sensitivity; refine:sensitivity | Operation::Sensitivity → calibration::sensitivity | Durable shared trial selection and analyze controls; captured source model, IDs, rank and warnings |
| Fit training_ids, validation_ids; coordinates; 40 evaluation budget | Filtered frozen split selection; refine:fit | Operation::Fit → calibration::attempt; FitAttempt/Fit; original train/held splits validated | Archive trial selectors and fit control; no role reassignment; attempts, failed optimizer histories and scores retained |
| Explicit candidate use, device applicability, source fit | Use fitted model for selected device | Command::UseFit; candidate provenance/exposure; candidate.model(device) | Explicit exploratory-use control; fit completion does not adopt settings |
| scenarios[].label, model.motor/bridge/conditions/step_s, timing.*, evidence | Add current scenario/JSON scenario editor; configure.scenarios; refine:robustness | calibration::Variant; SetScenarios; Operation::Robustness → calibration::robustness | Structured scenario fields and model parameter rows; empty scenarios capture documented exploratory defaults |
| Result review kind/index/decision/notes | Result presentation; no separate physical acceptance implied | SetDecision; durable refinement_evidence.decisions | Review decision and notes controls; distinct from UseFit |
| Captured inputs, failure, cancellation request, execution cancellation, displacement | Existing legacy result vectors and task status | Capture/Outcome/application; additive shared refinement_evidence receipts | Global retained jobs/receipts; captured Study/document revision; no result relabeling |
| Save-new/export-new/open review | Legacy save operations, native publication controls | Study::save_new/export_html_new/load | Existing immutable publication jobs; only captured revision acknowledged; later edits stay dirty |

## Compatibility and exclusions

Legacy in-scope dispatch and result-vector application delegate to shared
preparation/execution/application. Legacy configure routes experiment,
coordinates and scenarios through the shared transactional commands. Its widget
projection commits these fields only after the same validators accept them.
Other sections and their schema payloads remain intact. Saved Study and Workspace
unknown fields retain their flattened opaque maps; new evidence fields default
when reopening older files. No old workspace is rewritten on opening.

External UI requirements remain explicit: electrical/power setup and comparison,
FPGA plan/design/review, hardware acquisition, raw sweep migration,
recording-based fitting and combined datasets, CAD proposals/acceptance and
accepted actuator registry promotion are deferred. Their legacy UI and Rust
runtime modules remain available; RoboCAD/Python and browser hardware/calibration
surfaces stay reference paths pending their separately proven phases. Offline
controller refinement does not establish hardware timing, model accuracy,
independent Rust CAD parity, physical-source acceptance or registry promotion.

Decision: keep the existing fixed archive optimizer budget (40) and frozen split
roles; introducing a new optimizer or relabeling held-out data would change the
experiment contract. Revisit with a separately reviewed budget/qualification API.
Decision: use fit candidates only as exploratory drafts, with immutable source
fit identity and monotonic exposure/influence. Revisit only with a separate
physical-source acceptance workflow; review decisions never perform adoption.

Saved archive selections and captured controller-run chart selection use additive shared evidence fields and typed commands, so save/reopen and dirty-study preservation cover them. Rhai parameter rows accept JSON values including nested objects and arrays per row; other experiment and scenario fields remain individual controls.

## T50 bounded controller-recording inventory

This supersedes only the T48 recording/combined exclusions above. The historical
whole-project inventory is not repeated. See [T50 evidence](native-recording-fit-authoring.md).

| Legacy field/control/route | Shared reusable owner and native consumer | Dependency and source acceptance trace |
|---|---|---|
| Section 4 recording_path, Import; REST Refine Import(path) | refinement::recordings::classify + Command::ImportRecording; native StudyAction::ImportRecording → recording_jobs | Recording validation reproduces captured controller calculations and PWM quantization; explicit controller experiment versus FPGA plan classification; malformed/deferred input retained |
| Recording.version, experiment, runtime, frames; frame control, command_request_s/receipt_s, drive_counts, voltage_v, temperature_c, current_raw_uncalibrated | Existing controller_refinement::Recording and validate; shared fingerprint; retained recording review | All source fields preserved. Current raw is uncalibrated, not converted to measured current. Host request/receipt windows and sample time remain captured assumptions |
| stop_request_s/receipt_s, completed, failure, stop_verified, initial_registers, transactions_origin_host_s, timing_evidence, source_hashes | Existing Recording type; shared import and bounded Capture inputs; native inspection | Incomplete controller captures retained without score. Fingerprint is content identity; code/cadence hashes alone are not acquisition identity |
| selected_recording combo; prediction purpose buttons; REST Predict(index,purpose) | Command::SelectRecording and Operation::PredictRecording{recording_hash,purpose}; native rendered selectors/typed actions | Hash identity replaces bare UI index as portable selection; recorded PWM replay versus own-feedback controller remain separate purpose labels and captures |
| Context Prepare setup snapshot / Capture setup revision; configure capture_contexts | CaptureContext::unknown/validate + Command::AppendContext; native structured property/artifact/binding fields | Append-only revision; recording_hash and captured fixture/CAD association validated; old declarations remain inspectable |
| Context fixture, attached_output_hardware, transmission, limitations | CaptureContext; native scalar controls and immutable append | Captured fixture facts cannot be rewritten; authored output/transmission descriptions remain evidence, not CAD edits |
| Property name/value/unit/coordinate_frame/origin/source/uncertainty_bounds | Existing Property and Origin validation; native row scalar fields | measured/derived/estimated/unknown labels, units, coordinate frames, sources and optional uncertainty bounds validated together |
| Artifact role/location/blake3; Binding hardware_id/cad_component_id/joint_id/source | Existing Artifact/Binding validators; native row controls | Durable artifact identity and stable CAD association; no artifact file read or CAD promotion in a frame |
| Section 2 assignment_rationale, role buttons, limits; configure recording_assignments | Command::AssignRecording + RecordingDataset::capture; native structured role/RMS/final-error/rationale drafts and explicit freeze | Entire run frozen by fingerprint; immutable role/limits/rationale, no held-out→tuning reassignment; incomplete captures not tuning data |
| FitRecordings / REST Refine FitRecordings | Operation::FitRecordings → existing CalibrationData/calibration::attempt → shared apply_outcome; native RefineRun | Existing coordinates, family model, 40-evaluation budget and frozen dataset; cancelled/failed/partial attempts remain evidence |
| FitCombined{selected,additional_study}; optional saved file control / REST route | Operation::FitCombined + shared dataset preparation; native StudyAction::FitCombined → adopted job load | Current archive stays unchanged; additional saved identity, original roles/limits, recording reservation/exposure captured. Ambiguous bare archive IDs rejected |
| Fit candidate button in recording/combined review | Command::UseRecordingFit; native explicit exploratory control | Completeness/cancellation/verified trace/device/source guards; source link retained. Decisions never registry/CAD acceptance |
| Captured recording/combined attempt case review | Command::SelectFitCase; native attempt/case controls and existing recording chart jobs | Durable content-stamped selection; measured observations from immutable dataset and baseline/candidate score predictions, including additional-source cases; missing comparisons labelled unscored |
| Rejected optional saved-study input | Existing combined job captures bytes before parsing; Study input-content store and shared/native bounded receipt references | Exact content stored once in immutable companions managed by existing publication; identity/length/version and available provenance survive malformed/invalid input, cancellation and publication; read failures retain precise diagnostics without invented input. Legacy inline raw remains unchanged |
| Additional historical Train plus later quarantine | Shared combined dataset/preparation | Refuses new tuning if either source quarantines the identity, even without host reservation merge; historical saved declarations stay readable |
| Prediction measured/predicted charts, assumptions, model_error, tracking; fit score/history/request/dataset panels | Existing Prediction, FitAttempt and dataset vectors; native captured-time review/chart jobs | Measured and predicted sample times preserved, model staleness and timing assumptions labelled; objective history/failures/partial results visible |
| Save / HTML / Open; rejected text and deferred fields | Existing Study save_new/export_html_new/load plus retained native publication jobs | Immutable destinations; captured revision acknowledged only; opaque Workspace/Study fields retained; older files keep their schema |

Legacy `experiments_ui/refinement.rs::Action::run` delegates recording prediction,
recording fit and combined fit to shared prepare/execute. Import classification and
result insertion use shared validators. Legacy widget/configure context/assignment
projections are append-only transactions; candidate adoption uses shared guards.
The retained legacy FPGA/electrical/power/CAD handlers remain compatibility paths.

Decision: prediction requires explicitly authored Study or frozen assignment limits,
instead of a hidden three/five encoder-quantum fallback. Why: scoring assumptions
must be inspectable and captured. Alternative: preserve silent legacy defaults.
Revisit if a shared labelled default-limit authoring command is introduced.


## T52 bounded electrical extension

Earlier external electrical/power gaps in this inventory describe the pre-T52
surface. [The T52 field inventory](native-power-legacy-inventory.md) and
[native navigation/source map](native-power-authoring.md) cover offline source and
feedback authoring, simulation, both recording predictions and calibrated comparison.
FPGA, raw sweep, acquisition/hardware and CAD requirements remain external. No
executed parity or reference retirement is claimed.
