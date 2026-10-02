# T52 bounded legacy electrical inventory

Source review only. No build, fixture execution, launch, export or numerical/GUI
parity is claimed. Legacy navigation remains `sim-viewer --experiments` → saved
Study → Controller refinement → Electrical & battery. Native navigation is Build
→ Actuators → retained Study → electrical authoring and captured review.

All legacy entries below are in `crates/sim-viewer/src/experiments_ui/power_ui.rs`.
Its widget projection commits through `experiment_study::refinement::apply`
(`Command::Electrical`) as one transaction. Invalid projection values remain
retained by source/archive identity; the JSON source editor retains rejected text.
The native consumer is `builder/calibration/study/electrical_ui.rs` and its
`electrical_forms.rs` parser, with the existing Study actions/jobs/apply owner.
The parent T52 source map gives final cross-module acceptance lines.

| Exact fields / operation | Legacy entry / REST compatibility | Shared owner / dependency / acceptance trace |
|---|---|---|
| `draft.power`; regulated/battery add presets, removal | Source buttons; configure `draft.power` | `electrical::Command::SetSource`; `power::Setup`; presets are explicit illustrative assumptions, not registry promotion |
| `source_component`, `source_parameters.*` | Component label and registry-unit parameter controls; full-source JSON; configure draft | Existing registry descriptor parameters/units and `Setup::validate`; native structured parameter rows; unknown JSON editor keys must not silently disappear |
| `conditions.voltage_v` | Adding an explicit source clears imposed voltage; configure draft conditions | `SetSource`, `SetVoltage`; source and imposed voltage remain distinct authoring conditions |
| `auxiliary_current_a`, `evidence` | Auxiliary load drag value, assumptions multiline; configure draft | `Setup`; finite/nonnegative load, explicit evidence; immutable run captures |
| source `limits.minimum_voltage_v` | Minimum bus voltage optional control; configure draft | `power::Limits`; electrical physical sampled acceptance |
| source `maximum_discharge_current_a`, `maximum_charge_current_a` | Discharge/charge optional controls; configure draft | Distinct sign conventions; missing thresholds remain unscored |
| source `maximum_winding_current_a` | Winding magnitude optional control; configure draft | Distinct winding circuit location; never substitute battery current |
| source `maximum_draw_power_w`, `maximum_return_power_w` | Draw/return optional controls; configure draft | Shared limits and energy trace; sampled peaks are not switching peaks |
| `experiment.electrical` add/remove | Observe supply voltage checkbox; configure experiment | `SetController(Option<Controller>)`; source/controller transaction refuses atomically |
| `sensing.voltage_quantum_v` | Voltage resolution control; configure experiment | `power::Sensing`; voltage observation has explicit resolution |
| `supply_current_quantum_a`, `winding_current_quantum_a` | Two optional resolution controls; configure experiment | Separate circuit channels, labelled simulated hypotheses until calibrated adapter evidence exists |
| `sensing.evidence` | Evidence multiline; configure experiment | Exact authored assumptions retained in captured experiment |
| `nominal_voltage_for_compensation_v` | Compensation optional voltage; configure experiment | Shared sampled controller/runtime; no independent viewer physics |
| controller six `limits.*` listed above | Sampled protection optional thresholds; configure experiment | Shared controller protection; observation/command timing from experiment; zero PWM is not battery disconnection |
| electrical simulation, run selection | Run electrical source; controller-run combo; REST refine `simulate` | Existing `Operation::Simulate`, `SelectControllerRun`, shared `control::simulate`; retained model/experiment and motion versus electrical scores |
| recording selection, both prediction purposes | Recording combo, Predict recorded PWM / Predict closed loop; REST refine `predict(index,purpose)` | `SelectRecording`, `PredictRecording`; existing recording runtime; purpose and recording fingerprint captured |
| electrical prediction selection | Prediction combo; old REST compare index | `Electrical::SelectPrediction`; shared evidence selection; checked short hash display |
| captured servo voltage | Compare captured servo voltage; REST `compare_electrical(index,null)` | `Electrical::CompareServoVoltage`; recording identity and immutable prediction, voltage-only scoring restrictions preserved |
| calibrated sidecar path / exact JSON | Path field/import-compare; REST `compare_electrical(index,path)` | Existing background Action::run reads bytes, Study input-content capture before parse, `CompareMeasurements`; same prepare/execute/apply owner; malformed input never executes |
| `Measurements` version, recording_hash, calibration/location/polarity/uncertainty/channel/sample/clock/source fields | Full sidecar input; configure does not invent calibration | Existing `electrical_measurements::Measurements::validate_recording/evaluate`; bounded exact companions, synchronized derived watts/joules only |
| channel RMS/peak error/pass, measured/predicted sample times, supply energy | Comparison result charts and energy labels | Existing `Evaluation`; `apply_outcome_with_inputs` retains additional-input identity and calibrated immutable evidence; missing channels/limits remain unscored |
| save-new/export-new/reopen, failed/cancelled work | Existing Study panel and REST save/open/cancel | Existing Study publication, retained job owner and T51 companions; no new persistence owner or external publication |

Legacy REST `configure` accepts draft, experiment, coordinates, scenarios and
existing other fields. Electrical source/voltage/controller commands execute in
its whole-configuration clone before replacing the original Study. The original
Configure envelope is retained in failure diagnostics. Existing externally tagged
snake_case `Action::CompareElectrical(usize, Option<String>)` remains compatible;
its execution delegates to shared electrical operations and its result delegates
to the authoritative shared application owner. Standalone electrical evaluation
and legacy vector-push application were removed.

The reference UI, FPGA plan/design/review, raw sweeps, hardware acquisition and
serial/FPGA operations remain available in their existing paths. None is newly
native or qualified by this inventory. Python/RoboCAD/OCCT parity, calibrated
hardware channels, physical-source acceptance, accepted registry promotion and
sim-to-real qualification remain external requirements. Review decisions do not
perform any of them.

Decision: malformed sidecars retain exact content companions and a rejected-input
operation, rather than running another comparison as a fallback. Decision:
legacy structured controls use an editable projection and shared transaction;
rejected source/controller values remain editable without partial Study mutation.
