# Realistic quadruped actuators — implementation plan

Goal: generate useful gaits for the CAD-defined quadruped with the current
provisional motor dynamics, while continuing model refinement separately. Preserve the historical
baseline; the new model remains provisional wherever measurements are missing.

Scope: unloaded motor behavior first, including differences between motors and
simultaneous operation. Loaded-leg and whole-robot accuracy require later physical
measurements; bench agreement alone cannot establish them.

1. **Freeze the evidence and comparison baseline.** Pin the CAD artifact, gait,
   controller configuration and bench captures. Preserve training/validation roles
   and rejected fits. Record the twelve CAD actuator identities; keep physical
   motor assignments explicit rather than guessing from bus IDs.

2. **Validate a reusable Rust motor model.** Reuse the shared motor/driver components
   to model inertia, friction, acceleration, braking and reversal. Compare recorded
   PWM replay using measured voltage histories, then run the FPGA's shared fixed-point
   controller on simulated feedback at the recorded cadence (including 100 Hz trials).
   Include feedback quantization, command latency and bus scheduling. Fit per-motor differences and validate
   on separate trials. Preserve the historical 0.264° RMS comparison results without
   requiring that target before gait search; report actual RMS and peak error,
   velocity, time to reach speed, stopping time and time to reach the opposite speed,
   with their measurement limits. Do not promote failed fits.

3. **Make CAD own the profiles.** Add versioned motor-family profiles, optional
   per-unit deviations, controller configuration and power-branch definitions.
   Store units, coordinate frames, provenance, uncertainty and capture references.
   Support save/reload/export through shared authoring and validation commands;
   expose the same definitions through the Rust registry for the future Rust CAD UI.

4. **Connect realistic dynamics to the shared runtime.** Resolve CAD profiles once.
   Integrate sampled feedback, held PWM, motor states and shared battery/wiring
   components into the environment used by the viewer, headless runs and learning.
   Model battery state, voltage sag and shared branch loading; report terminal
   voltage, amps, watts and energy. Separate signal-bus timing from power wiring.
   Preserve the simplified servo as an explicit comparison profile; reject settings
   that would silently bypass declared dynamics. Keep external transmission losses
   separate from motor-internal losses.

5. **Expose useful comparisons in the Rust viewer.** Show requested/measured/simulated
   motion, reversal response, voltage, current/power predictions, saturation and fit
   errors. Label measured, estimated and uncalibrated quantities. Run fitting and
   simulation off the UI thread with progress, cancellation and reproducible exports.
   Let users review candidate profiles and explicitly promote validated values to CAD.

6. **Reassess the unchanged quadruped gait.** Export a separate CAD revision and run
   matched experiments with the original controller, geometry, environment and seed.
   Compare complete-run speed, tracking, falls, slip, voltage and energy. Verify
   timestep sensitivity, replay and browser performance. Retain an unchanged-controller
   comparison alongside gait tuning. Short trials may screen candidates; qualify finalists
   over the full horizon before comparing sustained speed with the historical result.

## Acceptance and remaining measurement gaps

- CAD profiles survive round trips and resolve to the exact runtime parameters.
- Shared tests demonstrate finite reversal response, voltage-dependent performance,
  simultaneous-load coupling and isolated per-unit parameter changes.
- Bench validation covers both recorded-command replay and simulated closed-loop
  behavior; a fitted curve alone is insufficient.
- Loaded torque, leg/transmission behavior, calibrated current and battery parameters
  remain unmeasured. Keep those assumptions provisional; do not treat the observed
  123°/s peak or a measured sag percentage as universal motor constants.
- A fast approximation must demonstrate its error against the detailed model and
  meet measured responsiveness targets before becoming the default learning profile.

**Current priority: gait generation with provisional motor dynamics.** Finish
verification of the shared-runtime changes already underway, run the existing gait
under the current motor model, then generate and compare new gait candidates in
that same environment. The historical 0.264° calibration criterion is not a gate
for gait experiments. Retain measured prediction errors and failed fit records;
do not relabel the current model as calibrated. Revisit calibration acceptance
criteria separately using the accuracy the locomotion task actually needs.

**Next reviewable delivery:** a reproducible gait-search pilot using the current
twelve-motor CAD revision, historical command schedule and shared Rust environment.
Record baseline and candidate displacement, falls, numerical failures and simulation
cost. Keep the model provisional and retain bench-comparison errors alongside results.

Track progress separately in [implementation status](IMPLEMENTATION-STATUS.json) and
[validation evidence](voltage-conditioning/README.md). See [audit and detailed rationale](README.md).
