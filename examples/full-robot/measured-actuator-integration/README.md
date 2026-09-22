# Measured motors in the quadruped

September 15, 2026. Current priority: generate gaits using the provisional detailed
motor model. The historical 0.264° bench target is not a prerequisite for gait work.
**Status: implementation in progress.** Measured-voltage FPGA replay, fitting and
viewer actions are implemented; refits still fail validation. CAD profile authoring,
validation and a separate twelve-motor archive are implemented. Full quadruped export
and CAD-selected FPGA controller integration now work in the shared session and
controller environment. A 50 ms twelve-motor window and half-timestep comparison
pass. The shared library now has tested battery/branch coupling, SOC and terminal
energy integration. CAD power authoring and full-session selection now pass focused
Rust and CAD tests; the quadruped still uses explicit imposed supplies because its
battery/wiring parameters are unmeasured. Accepted calibration and full gait
qualification remain. See [gait generation](gait-generation/README.md),
[power coupling](power-coupling/README.md), [controller integration evidence](controller-integration/README.md)
and [implementation status](IMPLEMENTATION-STATUS.json).
The original audit below did not change CAD parameters or walking behavior.

Current gait result: at 100% controller authority the original gait completes ten
seconds without falling, moving 4.504 m (0.450 m/s), with passing replay and
half-timestep checks. The six-trial search's two-second winner slows to 0.161 m/s
over ten seconds; it is not promoted. The original gait also completes 30 seconds
without falling at 0.422 m/s net progress. Nonideal scenario comparisons are next.
The earlier 35%
pilot improves ten-second travel by 65%, with exact replay and timestep checks,
but one foot has very low sampled clearance. The full-authority original has
12-42 mm peak clearance on all four feet. These are provisional simulations,
not hardware or historical 300-second comparisons. See
[gait evidence and remaining checks](gait-generation/README.md).
See [what is actually modeled and measured accuracy](MOTOR-MODEL-STATUS.md).

## What the 0.57 m/s result means

The preserved result is 0.5735639103 m/s over 300 simulated seconds. Its twelve
actuators use an explicit effective-servo profile, with finite torque/speed limits
and mechanical load dynamics. It bypasses internal motor states, electrical
supply behavior, firmware timing, quantization, gearbox compliance/backlash and
heat. It is not a hardware-calibrated walking prediction.

The baseline has no battery and no authored sensors. Its controller observes
ideal simulator state. Changing CAD electrical constants alone cannot change
this effective-servo execution path. The independent detailed robot builder can
already assemble a shared battery circuit; the incremental environment/browser
motor path currently uses imposed supply/temperature boundaries. Both parts
must be considered when bringing measured behavior into the walking environment.

[Baseline audit and exact CAD/motor identities](baseline-audit.json) records the
preserved CAD hash and all twelve motor bindings. The CAD artifact's SHA-256 was
checked against the original walking input. The historical speed was located in
retained reports; no new walking benchmark was run during this audit.

## Intended experience

1. **Select the motor model in CAD.** Each actuator has a motor-family profile,
   with an optional physical-unit identity and measured deviations. CAD retains
   units, shaft coordinates, source captures, profile version and uncertainty.
   Physical bus IDs 10–12 are not assigned arbitrarily to quadruped joints.
   Unmeasured units can use an explicitly provisional family model.
2. **Describe the power wiring.** The robot owns its battery and power branches,
   including motors sharing a branch. Keep communication-bus grouping distinct
   from the power network. Moving several motors should change the electrical
   conditions seen by the others through the shared circuit.
3. **Run the same low-level controller.** The gait produces references; the shared
   fixed-point controller converts sampled feedback to held PWM at the declared
   cadence. Use the controller implementation compiled to the FPGA, not a new
   ideal position-servo substitute. Record gains, limits, timing and quantization.
4. **See motor and robot comparisons together.** The Rust viewer and headless
   experiments report requested versus achieved angles, speed, reversal response,
   voltage, saturation and tracking error. Simulated current and power are labeled
   as predictions until calibrated. Robot comparisons include net speed, distance,
   falls, slips and actuator feasibility using the existing task definitions.
5. **Keep fidelity visible.** Preserve the old simplified profile as a baseline.
   A measured-dynamics profile must actually execute its declared motor, control
   and power behavior. Unsupported combinations must fail visibly rather than
   silently falling back to the old effective servo. A future fast approximation
   needs an explicit error and performance comparison against the detailed model.

## First milestone: reproduce the bench

The initial plan put motor validation first. The user's current priority is gait
generation with explicitly provisional dynamics; calibration work continues
separately. The following requirements remain relevant to future calibration.

- Preserve the recorded training/validation/timing roles. The new stress captures
  were declared timing diagnostics; do not silently relabel them into a clean
  held-out set after inspecting their results. They can diagnose model structure.
- Replay recorded PWM and measured terminal-voltage histories through the shared
  Rust motor components. Then run the same controller on the model's own feedback.
  Compare both modes; agreement only under recorded PWM is insufficient.
- Fit only identifiable quantities. Keep inertia/friction/delay hypotheses distinct
  from observed angle/voltage, and retain parameter bounds and rejected candidates.
- Preserve the historical 3-count (0.264 degree) RMS comparison and failed fits.
  The user has retired it as a gate for gait experiments. Report actual RMS/peak
  error and reversal/acceleration behavior at the measurement resolution; a new
  calibration acceptance tolerance has not been selected.
- Validate individual-unit deviations and simultaneous operation separately. The
  present data supports a shared electrical effect, but cannot identify battery
  resistance or wiring losses from calibrated current measurements: none exist.
- Link accepted profiles to their exact captures and validation reports. Pending
  fits remain candidate/uncalibrated, never promoted as measured truth.

Existing September 14 fitted candidates failed held-out own-feedback accuracy;
none is accepted for automatic CAD promotion. The September 15 tests established
100 Hz operation and retained faster reversal data, not a newly accepted model.

## CAD and shared runtime implementation

1. Add a versioned measured-actuator profile reference and per-unit binding to CAD
   authoring/export, with explicit parameter provenance and source hashes. The
   current `motor_block` derives values from the catalog; the existing legacy
   identification block covers only a subset of the required properties.
2. Resolve the profile once in shared Rust, exposing parameters, units, typed
   ports and validation through the registry. Keep runtime physics out of CAD
   Python and viewer code. Preserve source input and resolved values for replay.
3. Reuse/extend the registered motor, driver, fixed-point controller and electrical
   components to execute acceleration, braking/reversal, friction and supply
   coupling. A measured trajectory must not become a time-based animation or a
   fixed sag percentage applied to arbitrary future motion.
4. Extend the shared incremental environment's execution path to honor these
   states and power connections. Interactive, headless and learning hosts must
   consume the same observation/action contract and simulated clock. Keep expensive
   work cancellable and off the viewer thread.
5. Export a separate CAD revision for the realistic candidate. Verify profiles
   survive CAD save/reload/export and that the runtime resolves their exact values.
   Preserve the historical CAD artifact and 0.5736 m/s experiment unchanged.

## Quadruped acceptance

- Compare the original gait first, with the same robot geometry, environment,
  seed, command schedule and scoring horizon. Change only the declared actuator,
  low-level-controller and power profile. Any observation-interface changes get
  their own comparison; do not confound motor dynamics with a sensor-policy rewrite.
- Confirm that changing supply voltage, simultaneous motor loading, controller
  cadence and a single unit's parameters affects the expected runtime behavior.
  Enforce physical joint/transmission coordinates; keep external worm/belt losses
  separate from motor-internal losses.
- Report complete-run net speed and failures. Do not count a partial prefix as a
  new sustained speed result. Check timestep sensitivity, replay and browser
  performance with the new states before using the profile for learning.
- Optimize the gait only after the unchanged-controller comparison is retained.
  Predictors trained on the old physics require new validation or retraining.

## Limits we must keep visible

The latest three-motor 100 Hz campaign reached sampled peaks near 123 degrees/s,
with 12.6% worst voltage sag at 55% PWM. Those short, small-excursion reversals do
not establish maximum no-load speed, loaded torque, or a universal reversal delay.
Do not replace the catalog speed rating with that observed peak. Acceleration and
reversal must emerge from a model consistent with the observed commands and load.

Unloaded motors cannot validate leg-load torque, external drivetrain compliance,
foot contact or a complete quadruped. Servo current registers remain uncalibrated;
there is no validated battery-current, watts or battery-discharge model from these
captures. These limitations do not prevent building a useful provisional simulation,
but they prevent calling the complete robot an accurate depiction of hardware yet.

## Evidence

- [100 Hz qualification](../../actuators/hx30hm/hardware/2026-09-15-fast-loop/README.md)
- [100 Hz high-speed stress](../../actuators/hx30hm/hardware/2026-09-15-high-speed-stress/README.md)
- [Per-motor fit failures and shared-load evidence](../../actuators/hx30hm/hardware/2026-09-14-three-motor-characterization/README.md)
- [Historical gait baseline](../../interactive/trusted-controller-plan.md)
- [CAD physical contract](../../../cad/PHYSICAL_MODEL.md)
