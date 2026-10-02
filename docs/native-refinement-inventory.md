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
