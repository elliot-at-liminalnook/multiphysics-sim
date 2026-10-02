# T46.1–T46.3 measured PWM compatibility inventory

This inventory is source evidence only. Fixtures are written, unexecuted. No exact
parity, execution, hardware acquisition, registry promotion or legacy retirement is
claimed. Build → Actuators → Measured evidence remains the native entry point;
`sim-viewer` Experiments → Measured PWM replay and `/experiments` remain compatibility
consumers. Source ownership: `sim-runtime/experiment_study` owns hypotheses,
validation, replay, immutable evidence and publication; native jobs own scheduling,
identity and revision acknowledgements; UI only emits typed authoring actions.

| Field/workflow | Existing owner/entry | Shared semantics and native acceptance dependency |
|---|---|---|
| Archive directory / saved-review filename | `experiments_ui::launch`, REST Open | `hx_archive::load`, `Study::new` / `Study::load`; native load jobs append retained studies, never replace existing evidence |
| Retained study choice | legacy `current` | Native retained identity + revision; shared Study remains unchanged saved schema version 1 |
| `archive.label`, interpretation, split policy, raw/observation/model hashes, integrity issues | `hx_archive::Archive` | Read-only source identity; jobs clone archive before start; `Evaluation.capture` preserves exact IDs/splits/hashes |
| Trial id, run, stage, device, kind, duty, duration | `hx_archive::Trial` | `commands::trial_ids`, `filtered_ids`, `validate_view`; unknown/duplicate IDs fail before replay |
| Trial measured, archived prediction, acceptance limits, comparison | comparison `Trace`, `compare` | Immutable observations/reference; native charts use captured traces and distinct series; shared summary requires complete error-free pairs |
| Host command windows, voltage/temperature ranges, release hypothesis | Trial, actuator bench | Replay validates finite bounded inputs; release remains explicit; no hardware or proprietary controller equivalence |
| Candidate `motor.*` | registry motor-unit descriptor | `commands::metadata` exposes registry units/defaults/bounds; `SetParameter` + descriptor validation, no copied accepted constants |
| Candidate `bridge.*` | registry H-bridge descriptor | Same registry metadata and `SetParameter`; no authoritative registry mutation |
| Supply override V, temperature °C | Conditions | `SetConditions`; absent means measured range midpoint, positive supply and above absolute zero |
| Load inertia kg·m², signed load torque N·m | Conditions | `SetConditions`; finite positive inertia, finite torque; labeled fixture hypotheses |
| Command delay s, integrator step s | Conditions / ModelSettings | `SetConditions`, `SetStep`; named paths, delay 0–0.5 s, step 10 µs–2 ms |
| Optional power setup | ModelSettings.power | Preserved, validated and captured; authoring deferred to existing external refinement/power UI |
| Reset candidate / use captured candidate | legacy candidate editor | `ResetCandidate`, `UseEvaluation`; baseline never changes; held-out influence monotonic |
| Separate RMSE/final error limits rad | Study.limits | `SetLimits`, `validate_limits`; finite nonnegative values, captured per evaluation; archived limits unchanged |
| Device, direction, absolute duty min/max, tuning/held-out role, outcome filters | ReviewView | `SetView`, `filtered_ids`; bounds/enum checks; incomplete pairs classify Unscored |
| Selected trial, selected evaluation, component reference | ReviewView | `SelectTrial`, `SelectEvaluation`, `SetView`; known identities, validation exposure when viewed |
| Run selected / filtered / held-out / explicit IDs | legacy `run`, REST Evaluate | `EvaluationSelection`, `trial_ids`, `Expose`; native clones full Study and identity before jobs call `evaluate` |
| Baseline/candidate predictions, runtime, integrator, seed, assumptions | Evaluation | Existing `evaluate` → `simulate` → `actuator_bench`; captured settings never edited by draft edits |
| Cancellation, per-trial failures, progress | evaluate cancel flag/progress callback | Cancelled/not reached IDs get unscored entries; no failed pair counted passing; native jobs retain displaced results |
| `validation_seen`, `validation_influenced` | Study | `Expose` and transactional apply; exposure monotonic, candidate/limits edits after exposure mark influence; refused commands cannot clear either |
| Decision + evaluation notes | Evaluation | `SetDecision`, shared DECISIONS; scope is tested conditions, never acceptance into CAD |
| Study notes | Study.notes | `SetNotes`; native late submissions require captured identity/revision |
| New saved review / standalone HTML | Study.save_new/export_html_new | Validation then create-new temp + hard-link destination; never overwrite; native jobs acknowledge captured revision only |
| Saved-review reopen | Study.load | Validates captured metrics and deferred payloads; top-level unknown fields and Workspace unknown sections survive serde flatten maps |

Legacy window inputs stage edits and invoke the same transactional commands as
REST and native controls. REST retains separate refinement keys as compatibility
surfaces; their existing validation/execution remains in controller_refinement.
No new controller/power/FPGA/refinement authoring or promotion is supplied here.
Deferred known payloads stay typed and validated; opaque unknown top-level review
and refinement sections are retained without interpretation. Unknown fields inside
other historical typed nested objects retain their preexisting serde behavior;
this batch does not claim a general lossless schema migration framework.

Concrete reading acceptance: `experiment_study/commands.rs` owns every offline
mutation and selection; `experiment_study.rs::evaluate` captures IDs/splits/hashes,
retains cancellation, and `Evaluation::summary` refuses incomplete pairs;
`experiments_ui.rs::run`/render staging and `experiments_ui/rest.rs` Configure/Review
reuse these commands. `experiment_study/compatibility.rs` writes round-trip opaque
payload, transactional nonfinite refusal, monotonic exposure, duplicate-ID and
cancelled captured-evidence fixtures. Native controls/lifecycle source traces are
recorded in the architecture and consolidation guide by the integration owner.

Decision: retain schema version 1 with additive defaulted capture/opaque maps rather
than force rewriting old reviews; revisit if a versioned migration is separately
reviewed. Decision: count only complete error-free pairs as pass/fail; candidate-only
success was misleading when baseline failed. Revisit if an explicit single-model
score contract is added, keeping pair-comparison results separate.

Legacy rejected rendered edits are retained as narrow, nonrecursive attempted-input
records under `Study.retained_fields.offline_rejected_drafts` (or a suffix when the
existing value is opaque). Identical consecutive attempts are deduplicated; an
unchanged idle frame does not create evidence. Validation still controls the active
model. Decision: preserve refused edits as evidence rather than silently discarding
them; an invalid model cannot run, and a separately reviewed editor-draft contract
could later expose retryable invalid forms directly. Revisit when such a shared
form-draft model exists. Trial row PASS/FAIL uses the same complete error-free pair
criterion as the aggregate summary.

Full legacy REST Study reads expose all archived traces through shared `expose`
before returning the snapshot and mark the first exposure dirty. Metadata-only
State reads remain metadata-only. The written legacy fixture reads the full review,
checks idempotent exposure revision handling, edits the candidate and round-trips
both leakage flags. Native launch snapshots use shared `execution_identity()` for
intended runtime/integrator/seed evidence even when cancelled before execution.

`TrialResult::outcome` is the sole measured-evaluation PASS/FAIL/UNSCORED contract:
baseline and candidate must both exist and the error list must be empty. Shared
summary, outcome filters, native/legacy rows and HTML report outcomes consume it;
individual prediction metrics and traces remain inspectable. The archived empirical
reference uses its separate frozen archival comparison. `Study::render_html` is the
actual report renderer used by `export_html_new`; written fixtures inspect its trial
row for candidate-only and error-bearing complete pairs, without publishing a file.
This centralization prevents presentation consumers from silently relaxing scoring.
