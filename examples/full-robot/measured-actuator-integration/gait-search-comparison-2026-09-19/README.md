# Gait search comparison — in progress

Goal: search stride, cadence, foot clearance, stance duration, body height and
leg phases; screen candidates before dynamics; compare Bayesian optimization
and CMA-ES on matched budgets; validate finalists with detailed physics and
longer, varied-condition episodes. No hardware operation is part of this study.

## Frozen experiment choices

`protocol.json` declares two optimizer seeds, 33 attempts per algorithm/seed,
3.6-second candidate episodes and the same reserved 118.8 simulated seconds per
algorithm/seed. Rejected/failed attempts consume their slot. We retain actual
simulated time separately and include proposal, preparation, simulation and
capture-writing time in each algorithm's wall-time accounting. Trial receipt
and progress-file write overhead is outside that timer. The baseline counts as
attempt zero for each optimizer. Algorithms alternate on the same host.

The initial full-domain Latin-hypercube design is followed by Bayesian log-EI
once dimension+1 valid objective observations exist. If physics screening leaves
too few valid rows, an explicitly labeled 5%-range local bootstrap around the
best valid point collects them. These are not Bayesian acquisition steps and
must be counted separately in the results. Failed trials never masquerade as
measured objective values. CMA-ES uses the pinned Rust `cmaes` 0.2.2 backend,
normalized reflected bounds, failure ranking and deterministic restarts. Its
state is reconstructed from the complete observation prefix.

`contact-template.json` declares eight independent coordinates: stride,
cadence scale, clearance, stance fraction, body-height offset and three relative
foot phases. The first foot anchors the common phase. Actual cadence is
`cadence_scale / 0.9` Hz. Topology, CAD properties, controller code and physics
remain in the existing shared runtime. Candidate preparation recompiles the
contact motion through the shared CAD inverse-kinematics/reference compiler.

## Preparation evidence

- `baseline-preparation/`: retained rejection. The 1024-sample archived recipe
  exceeded its unchanged acceleration-interpolation tolerance.
- `baseline-preparation-fine/`: the existing 2048-sample fine recipe passed
  interpolation, but the original unfiltered reference requested 6.351 rad/s,
  above the declared provisional 5.512 rad/s reference-speed screen.
- `baseline-preparation-cadence08/`: passed preparation at 80% nominal cadence.
  Maximum requested reference speed is 5.081 rad/s. This is preparation evidence,
  not a dynamic trial. Its generated file predates the final initial-servo-target
  synchronization fix and must be regenerated before qualification/search.
- `baseline-preparation-final/`: passed on the final source, including explicit
  initial-servo-target synchronization. This is the qualification input.

All 24 relevant tests passed on the archived source; `regression-tests.log`
and `build.log` retain the results. `library-source.tar.gz` preserves the library
and host source. Matched qualification is now prepared under `qualification/`;
its capture/receipt files, once present, are the authoritative outcome.

The final baseline qualification **passed**, with **1.188x** stepping speedup
and no changed accuracy budgets. `qualification/qualification.json` contains
the full channel results. `comparison/` is now the active matched search, with
one durable directory per algorithm, seed and attempt. The final widened phase
domain also passed its materialization regression. No finalist has yet been
selected or validated.

The original baseline is kept in `rejected-original-baseline-values.json`.
The new starting values are in `baseline-values.json`. No dynamic objective was
used to select this starting point; no acceptance budget was relaxed.

Reference-speed caps are explicit provisional operational assumptions from the
archived actuator no-load-speed fields. They are not measured loaded limits.
The older approximate inverse-load audit still reports failures; its flags
remain visible. Passing the preparation screen certifies neither dynamic
stability nor continuous collision clearance nor real motor tracking.

## Shared code and execution

- `sim_runtime::contact_reference`: extracted reusable reference compiler;
  `compile_contact_reference` is now a thin file adapter.
- `sim_runtime::contact_exploration`: typed contact-template materialization,
  preparation screens, initial-state/servo-target binding and ordinary
  experiment specification. Registry operation: `experiment.prepare_contact_motion`.
- `sim_solve::evolution`: replayable CMA-ES proposal adapter.
- `sim_runtime::search_comparison`: optimizer adapters and matched timing/budget
  accounting, including failed attempts.
- `sim_runtime::motion_evaluation`: signed progress, all-motor tracking,
  completion, orientation and body-contact gates over runtime captures.
- `compare_gait_search`: durable host using those library APIs and the existing
  `CaptureSession`/`EmbeddedEnvironment`; cancellation is checked each task step.

Build all hosts with `--features evolution` so their runtime identities match.
Regenerate preparation, then use `reduced_exploration prepare/qualify` to save
current-source baseline fidelity evidence under `qualification/`. The comparison
host rechecks the saved captures and binds its baseline to that qualified spec.
`compare_gait_search comparison-config.json FRESH_DIRECTORY MAX_NEW_ATTEMPTS`
advances a bounded number of attempts. A saved attempt directory without a
receipt is retained for explicit recovery, never silently overwritten/restarted.

## Outstanding acceptance work

1. Finish final-source regression/build checks and regenerate baseline preparation.
2. Qualify the new starting baseline's reduced solver profile against detailed
   physics with unchanged mechanical/electrical/speed budgets.
3. Execute both seeds and algorithms through the full declared comparison budget;
   report proposal method counts, failures and improvement per wall hour.
4. Select one eligible finalist per algorithm/seed. Validate detailed 15-second
   episodes at nominal, lower/higher voltage, perturbed motor parameters, reduced
   friction and combined conditions, plus a nominal 30-second episode. Preserve
   every override receipt and rejected candidate. These are sensitivity scenarios,
   not measured uncertainty intervals or battery sag calibration.
5. Check detailed geometry/contact behavior, tracking and timestep sensitivity;
   publish results and limitations. Do not call this goal complete before the
   actual comparison and finalist validations are recorded.
